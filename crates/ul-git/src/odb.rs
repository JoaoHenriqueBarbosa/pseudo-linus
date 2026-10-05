//! Banco de objetos: objetos soltos (leitura e escrita) e packfiles (leitura, com deltas), sobre o FS
//! do sandbox.
//!
//! O pack é lido sob demanda com `pread`: o `.idx` inteiro vai pra memória (é pequeno), e cada objeto
//! é descomprimido a partir do deslocamento dele, com um cache de bases de delta.

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, VecDeque};
use std::rc::Rc;

use miniz_oxide::inflate::stream::{InflateState, inflate};
use miniz_oxide::{DataFormat, MZFlush, MZStatus};
use sysabi::{Errno, Fd, OFlags};

use crate::hash::{self, Kind, Oid};
use crate::os;

/// Objeto lido: tipo e conteúdo.
pub type Object = (Kind, Rc<Vec<u8>>);

/// Busca de base de delta fora do pack (REF_DELTA cuja base mora em outro lugar).
type ExternalLookup = dyn Fn(&Oid) -> Option<(Kind, Vec<u8>)>;

pub struct Odb {
    pub objects_dir: Vec<u8>,
    packs: RefCell<Option<Rc<Vec<Pack>>>>,
    alternates: RefCell<Option<Rc<Vec<Odb>>>>,
    /// Cache de objetos já lidos (commits e trees, sobretudo), limitado em bytes.
    cache: RefCell<ObjCache>,
    depth: usize,
}

struct ObjCache {
    map: HashMap<Oid, Object>,
    order: VecDeque<Oid>,
    bytes: usize,
}

const OBJ_CACHE_BYTES: usize = 32 << 20;
const OBJ_CACHE_MAX_OBJECT: usize = 4 << 20;

impl ObjCache {
    fn new() -> ObjCache {
        ObjCache { map: HashMap::new(), order: VecDeque::new(), bytes: 0 }
    }

    fn get(&self, id: &Oid) -> Option<Object> {
        self.map.get(id).cloned()
    }

    fn put(&mut self, id: Oid, obj: Object) {
        let len = obj.1.len();
        if len > OBJ_CACHE_MAX_OBJECT || self.map.contains_key(&id) {
            return;
        }
        while self.bytes + len > OBJ_CACHE_BYTES {
            let Some(old) = self.order.pop_front() else { break };
            if let Some(o) = self.map.remove(&old) {
                self.bytes -= o.1.len();
            }
        }
        self.bytes += len;
        self.order.push_back(id);
        self.map.insert(id, obj);
    }
}

impl Odb {
    pub fn new(objects_dir: Vec<u8>) -> Odb {
        Odb::with_depth(objects_dir, 0)
    }

    fn with_depth(objects_dir: Vec<u8>, depth: usize) -> Odb {
        Odb {
            objects_dir,
            packs: RefCell::new(None),
            alternates: RefCell::new(None),
            cache: RefCell::new(ObjCache::new()),
            depth,
        }
    }

    fn loose_path(&self, id: &Oid) -> Vec<u8> {
        let hex = id.hex();
        let mut p = self.objects_dir.clone();
        p.push(b'/');
        p.extend_from_slice(&hex.as_bytes()[..2]);
        p.push(b'/');
        p.extend_from_slice(&hex.as_bytes()[2..]);
        p
    }

    /// Esquece os packs conhecidos (depois de criar um pack novo, por exemplo).
    pub fn reprepare(&self) {
        *self.packs.borrow_mut() = None;
    }

    fn packs(&self) -> Rc<Vec<Pack>> {
        if let Some(p) = self.packs.borrow().as_ref() {
            return p.clone();
        }
        let mut packs = Vec::new();
        let dir = os::join(&self.objects_dir, b"pack");
        if let Ok(entries) = os::read_dir(&dir) {
            let mut names: Vec<Vec<u8>> = entries.into_iter().map(|e| e.name).collect();
            names.sort();
            for n in names {
                if let Some(stem) = n.strip_suffix(b".idx") {
                    let idx_path = os::join(&dir, &n);
                    let mut pack_path = os::join(&dir, stem);
                    pack_path.extend_from_slice(b".pack");
                    if !os::exists(&pack_path) {
                        continue;
                    }
                    if let Ok(Some(p)) = Pack::open(&idx_path, pack_path) {
                        packs.push(p);
                    }
                }
            }
        }
        let rc = Rc::new(packs);
        *self.packs.borrow_mut() = Some(rc.clone());
        rc
    }

    fn alternates(&self) -> Rc<Vec<Odb>> {
        if let Some(a) = self.alternates.borrow().as_ref() {
            return a.clone();
        }
        let mut alts = Vec::new();
        if self.depth < 5 {
            let path = os::join(&self.objects_dir, b"info/alternates");
            if let Ok(Some(data)) = os::read_opt(&path) {
                for line in data.split(|b| *b == b'\n') {
                    let line = crate::object::trim_ascii(line);
                    if line.is_empty() || line.starts_with(b"#") {
                        continue;
                    }
                    let dir = if line.starts_with(b"/") { line.to_vec() } else { os::normalize_abs(&os::join(&self.objects_dir, line)) };
                    alts.push(Odb::with_depth(dir, self.depth + 1));
                }
            }
        }
        let rc = Rc::new(alts);
        *self.alternates.borrow_mut() = Some(rc.clone());
        rc
    }

    /// Lê um objeto; `Ok(None)` se não existe.
    pub fn read(&self, id: &Oid) -> Result<Option<Object>, String> {
        if let Some(o) = self.cache.borrow().get(id) {
            return Ok(Some(o));
        }
        let r = self.read_uncached(id)?;
        if let Some(o) = &r
            && o.0 != Kind::Blob
        {
            self.cache.borrow_mut().put(*id, o.clone());
        }
        Ok(r)
    }

    fn read_uncached(&self, id: &Oid) -> Result<Option<Object>, String> {
        if let Some(o) = self.read_loose(id)? {
            return Ok(Some(o));
        }
        for p in self.packs().iter() {
            if let Some(i) = p.idx.lookup(id) {
                let off = p.idx.offset(i);
                let (k, d) = p.read_at(off, &|base| self.read(base).ok().flatten().map(|(k, d)| (k, d.to_vec())))?;
                return Ok(Some((k, Rc::new(d))));
            }
        }
        for alt in self.alternates().iter() {
            if let Some(o) = alt.read(id)? {
                return Ok(Some(o));
            }
        }
        if *id == hash::EMPTY_TREE {
            return Ok(Some((Kind::Tree, Rc::new(Vec::new()))));
        }
        Ok(None)
    }

    fn read_loose(&self, id: &Oid) -> Result<Option<Object>, String> {
        let path = self.loose_path(id);
        let data = match os::read_opt(&path) {
            Ok(Some(d)) => d,
            Ok(None) => return Ok(None),
            Err(e) => return Err(format!("unable to read {}: {}", os::lossy(&path), e.message())),
        };
        let raw = miniz_oxide::inflate::decompress_to_vec_zlib(&data)
            .map_err(|_| format!("loose object {id} (stored in {}) is corrupt", os::lossy(&path)))?;
        let nul = raw.iter().position(|b| *b == 0).ok_or_else(|| format!("loose object {id} is corrupt"))?;
        let head = &raw[..nul];
        let sp = head.iter().position(|b| *b == b' ').ok_or_else(|| format!("loose object {id} is corrupt"))?;
        let kind = Kind::from_name(&head[..sp]).ok_or_else(|| format!("loose object {id} has unknown type"))?;
        let size: usize = std::str::from_utf8(&head[sp + 1..]).ok().and_then(|s| s.parse().ok()).ok_or_else(|| format!("loose object {id} is corrupt"))?;
        let body = raw[nul + 1..].to_vec();
        if body.len() != size {
            return Err(format!("loose object {id} (stored in {}) is corrupt", os::lossy(&path)));
        }
        Ok(Some((kind, Rc::new(body))))
    }

    pub fn exists(&self, id: &Oid) -> bool {
        if self.cache.borrow().map.contains_key(id) {
            return true;
        }
        if os::exists(&self.loose_path(id)) {
            return true;
        }
        if self.packs().iter().any(|p| p.idx.lookup(id).is_some()) {
            return true;
        }
        if self.alternates().iter().any(|a| a.exists(id)) {
            return true;
        }
        *id == hash::EMPTY_TREE
    }

    /// Grava um objeto solto (se ainda não existe) e devolve o id.
    pub fn write(&self, kind: Kind, data: &[u8]) -> Result<Oid, String> {
        let id = hash::hash_object(kind, data);
        if self.exists(&id) && id != hash::EMPTY_TREE {
            return Ok(id);
        }
        if id == hash::EMPTY_TREE && os::exists(&self.loose_path(&id)) {
            return Ok(id);
        }
        let mut raw = hash::header(kind, data.len());
        raw.extend_from_slice(data);
        let compressed = miniz_oxide::deflate::compress_to_vec_zlib(&raw, 1);
        let path = self.loose_path(&id);
        let dir = os::dirname(&path).to_vec();
        match os::mkdir(&dir, 0o777) {
            Ok(()) | Err(Errno::EEXIST) => {}
            Err(e) => return Err(format!("unable to create directory {}: {}", os::lossy(&dir), e.message())),
        }
        let tmp = os::join(&dir, format!("tmp_obj_{}_{}", os::getpid(), id.short(8)).as_bytes());
        os::write(&tmp, &compressed, 0o444).map_err(|e| format!("insufficient permission for adding an object to repository database {}: {}", os::lossy(&self.objects_dir), e.message()))?;
        if let Err(e) = os::rename(&tmp, &path) {
            let _ = os::unlink(&tmp);
            return Err(format!("unable to write file {}: {}", os::lossy(&path), e.message()));
        }
        Ok(id)
    }

    /// Todos os ids com esse prefixo hexadecimal (em minúsculas), soltos, em packs e alternates.
    pub fn find_prefix(&self, prefix: &[u8], out: &mut Vec<Oid>) {
        if prefix.len() < 2 {
            return;
        }
        let dir = os::join(&self.objects_dir, &prefix[..2]);
        if let Ok(entries) = os::read_dir(&dir) {
            for e in entries {
                if e.name.len() == 38 && e.name.starts_with(&prefix[2..]) {
                    let mut hex = prefix[..2].to_vec();
                    hex.extend_from_slice(&e.name);
                    if let Some(id) = Oid::from_hex(&hex) {
                        out.push(id);
                    }
                }
            }
        }
        for p in self.packs().iter() {
            p.idx.find_prefix(prefix, out);
        }
        for a in self.alternates().iter() {
            a.find_prefix(prefix, out);
        }
        out.sort();
        out.dedup();
    }

    /// Quantidade aproximada de objetos (só os empacotados, como o git).
    pub fn approximate_count(&self) -> usize {
        self.packs().iter().map(|p| p.idx.n).sum()
    }

    /// Ids de todos os objetos soltos.
    pub fn loose_ids(&self) -> Vec<Oid> {
        let mut out = Vec::new();
        for i in 0..256u32 {
            let hx = format!("{i:02x}");
            let dir = os::join(&self.objects_dir, hx.as_bytes());
            if let Ok(entries) = os::read_dir(&dir) {
                for e in entries {
                    if e.name.len() == 38 {
                        let mut h = hx.as_bytes().to_vec();
                        h.extend_from_slice(&e.name);
                        if let Some(id) = Oid::from_hex(&h) {
                            out.push(id);
                        }
                    }
                }
            }
        }
        out.sort();
        out
    }

    /// Ids de todos os objetos empacotados.
    pub fn packed_ids(&self) -> Vec<Oid> {
        let mut out = Vec::new();
        for p in self.packs().iter() {
            for i in 0..p.idx.n {
                out.push(p.idx.oid(i));
            }
        }
        out.sort();
        out.dedup();
        out
    }

    /// Caminhos dos packs (`.pack`) e quantos objetos cada um tem.
    pub fn pack_info(&self) -> Vec<(Vec<u8>, usize, u64)> {
        self.packs().iter().map(|p| (p.pack_path.clone(), p.idx.n, p.size)).collect()
    }
}

// ---- pack -------------------------------------------------------------------------------------

struct PackIdx {
    version: u32,
    n: usize,
    fanout: [u32; 256],
    data: Vec<u8>,
    names_off: usize,
    offsets_off: usize,
    large_off: usize,
}

impl PackIdx {
    fn parse(data: Vec<u8>) -> Option<PackIdx> {
        let be32 = |d: &[u8], at: usize| -> Option<u32> { Some(u32::from_be_bytes(d.get(at..at + 4)?.try_into().ok()?)) };
        let (version, fan_at) = if data.starts_with(b"\xfftOc") { (be32(&data, 4)?, 8) } else { (1, 0) };
        let mut fanout = [0u32; 256];
        for (i, f) in fanout.iter_mut().enumerate() {
            *f = be32(&data, fan_at + i * 4)?;
        }
        let n = fanout[255] as usize;
        let after_fan = fan_at + 1024;
        match version {
            1 => {
                if data.len() < after_fan + n * 24 {
                    return None;
                }
                Some(PackIdx { version, n, fanout, data, names_off: after_fan, offsets_off: after_fan, large_off: 0 })
            }
            2 => {
                let names_off = after_fan;
                let crc_off = names_off + n * 20;
                let offsets_off = crc_off + n * 4;
                let large_off = offsets_off + n * 4;
                if data.len() < large_off {
                    return None;
                }
                Some(PackIdx { version, n, fanout, data, names_off, offsets_off, large_off })
            }
            _ => None,
        }
    }

    fn name(&self, i: usize) -> &[u8] {
        if self.version == 1 {
            let at = self.names_off + i * 24 + 4;
            &self.data[at..at + 20]
        } else {
            let at = self.names_off + i * 20;
            &self.data[at..at + 20]
        }
    }

    fn oid(&self, i: usize) -> Oid {
        Oid::from_bytes(self.name(i)).expect("20 bytes")
    }

    fn offset(&self, i: usize) -> u64 {
        if self.version == 1 {
            let at = self.names_off + i * 24;
            return u32::from_be_bytes(self.data[at..at + 4].try_into().expect("4")) as u64;
        }
        let at = self.offsets_off + i * 4;
        let v = u32::from_be_bytes(self.data[at..at + 4].try_into().expect("4"));
        if v & 0x8000_0000 == 0 {
            return v as u64;
        }
        let k = (v & 0x7fff_ffff) as usize;
        let at = self.large_off + k * 8;
        self.data.get(at..at + 8).map(|b| u64::from_be_bytes(b.try_into().expect("8"))).unwrap_or(u64::MAX)
    }

    fn bucket(&self, first: u8) -> (usize, usize) {
        let lo = if first == 0 { 0 } else { self.fanout[first as usize - 1] as usize };
        let hi = self.fanout[first as usize] as usize;
        (lo, hi.min(self.n))
    }

    fn lookup(&self, id: &Oid) -> Option<usize> {
        let (mut lo, mut hi) = self.bucket(id.0[0]);
        while lo < hi {
            let mid = (lo + hi) / 2;
            match self.name(mid).cmp(&id.0[..]) {
                std::cmp::Ordering::Equal => return Some(mid),
                std::cmp::Ordering::Less => lo = mid + 1,
                std::cmp::Ordering::Greater => hi = mid,
            }
        }
        None
    }

    fn find_prefix(&self, prefix: &[u8], out: &mut Vec<Oid>) {
        let Some(first) = hash::hex_val(prefix[0]).zip(hash::hex_val(prefix[1])).map(|(a, b)| (a << 4) | b) else { return };
        let (lo, hi) = self.bucket(first);
        for i in lo..hi {
            let id = self.oid(i);
            if id.hex_starts_with(prefix) {
                out.push(id);
            }
        }
    }
}

struct Pack {
    pack_path: Vec<u8>,
    idx: PackIdx,
    fd: Cell<Option<Fd>>,
    size: u64,
    bases: RefCell<BaseCache>,
}

struct BaseCache {
    map: HashMap<u64, (Kind, Rc<Vec<u8>>)>,
    order: VecDeque<u64>,
    bytes: usize,
}

const BASE_CACHE_BYTES: usize = 16 << 20;

impl Drop for Pack {
    fn drop(&mut self) {
        if let Some(fd) = self.fd.take() {
            let _ = os::sysc().close(fd);
        }
    }
}

const OBJ_OFS_DELTA: u8 = 6;
const OBJ_REF_DELTA: u8 = 7;

enum Entry {
    Base(Kind, Vec<u8>),
    Ofs(u64, Vec<u8>),
    Ref(Oid, Vec<u8>),
}

impl Pack {
    fn open(idx_path: &[u8], pack_path: Vec<u8>) -> Result<Option<Pack>, Errno> {
        let data = os::read(idx_path)?;
        let Some(idx) = PackIdx::parse(data) else { return Ok(None) };
        let size = os::stat(&pack_path)?.size;
        Ok(Some(Pack { pack_path, idx, fd: Cell::new(None), size, bases: RefCell::new(BaseCache { map: HashMap::new(), order: VecDeque::new(), bytes: 0 }) }))
    }

    fn fd(&self) -> Result<Fd, String> {
        if let Some(fd) = self.fd.get() {
            return Ok(fd);
        }
        let fd = os::sysc()
            .openat(Fd::CWD, &self.pack_path, OFlags::RDONLY | OFlags::CLOEXEC, 0)
            .map_err(|e| format!("packfile {} cannot be accessed: {}", os::lossy(&self.pack_path), e.message()))?;
        self.fd.set(Some(fd));
        Ok(fd)
    }

    fn pread(&self, buf: &mut [u8], off: u64) -> Result<usize, String> {
        let fd = self.fd()?;
        let s = os::sysc();
        let mut got = 0;
        while got < buf.len() {
            match s.pread(fd, &mut buf[got..], off + got as u64) {
                Ok(0) => break,
                Ok(n) => got += n,
                Err(Errno::EINTR) => {}
                Err(e) => return Err(format!("error reading packfile {}: {}", os::lossy(&self.pack_path), e.message())),
            }
        }
        Ok(got)
    }

    fn corrupt(&self, off: u64) -> String {
        format!("packfile {} is corrupt at offset {off}", os::lossy(&self.pack_path))
    }

    /// Lê o cabeçalho e descomprime os dados da entrada em `off`.
    fn entry(&self, off: u64) -> Result<Entry, String> {
        let mut head = [0u8; 64];
        let n = self.pread(&mut head, off)?;
        let head = &head[..n];
        let mut i = 0;
        let mut c = *head.get(i).ok_or_else(|| self.corrupt(off))?;
        i += 1;
        let ty = (c >> 4) & 7;
        let mut size = (c & 15) as u64;
        let mut shift = 4;
        while c & 0x80 != 0 {
            c = *head.get(i).ok_or_else(|| self.corrupt(off))?;
            i += 1;
            size |= ((c & 0x7f) as u64) << shift;
            shift += 7;
            if shift > 63 {
                return Err(self.corrupt(off));
            }
        }
        match ty {
            OBJ_OFS_DELTA => {
                let mut c = *head.get(i).ok_or_else(|| self.corrupt(off))?;
                i += 1;
                let mut back = (c & 0x7f) as u64;
                while c & 0x80 != 0 {
                    c = *head.get(i).ok_or_else(|| self.corrupt(off))?;
                    i += 1;
                    back = ((back + 1) << 7) | (c & 0x7f) as u64;
                }
                if back > off {
                    return Err(self.corrupt(off));
                }
                let data = self.inflate_at(off + i as u64, size as usize)?;
                Ok(Entry::Ofs(off - back, data))
            }
            OBJ_REF_DELTA => {
                let base = Oid::from_bytes(head.get(i..i + 20).ok_or_else(|| self.corrupt(off))?).expect("20");
                let data = self.inflate_at(off + i as u64 + 20, size as usize)?;
                Ok(Entry::Ref(base, data))
            }
            t => {
                let kind = Kind::from_pack_type(t).ok_or_else(|| self.corrupt(off))?;
                let data = self.inflate_at(off + i as u64, size as usize)?;
                Ok(Entry::Base(kind, data))
            }
        }
    }

    fn inflate_at(&self, mut off: u64, size: usize) -> Result<Vec<u8>, String> {
        let mut out = Vec::new();
        out.try_reserve(size).map_err(|_| "out of memory".to_string())?;
        out.resize(size, 0);
        let mut state = InflateState::new_boxed(DataFormat::Zlib);
        let mut written = 0usize;
        let chunk = (size / 2 + 256).clamp(4096, 1 << 20);
        let mut buf = vec![0u8; chunk];
        let mut sink = [0u8; 1];
        loop {
            let n = self.pread(&mut buf, off)?;
            if n == 0 {
                return Err(self.corrupt(off));
            }
            let mut input = &buf[..n];
            while !input.is_empty() {
                let dst: &mut [u8] = if written < size { &mut out[written..] } else { &mut sink };
                let r = inflate(&mut state, input, dst, MZFlush::None);
                input = &input[r.bytes_consumed..];
                off += r.bytes_consumed as u64;
                if written < size {
                    written += r.bytes_written;
                } else if r.bytes_written > 0 {
                    return Err(self.corrupt(off));
                }
                match r.status {
                    Ok(MZStatus::StreamEnd) => {
                        if written != size {
                            return Err(self.corrupt(off));
                        }
                        return Ok(out);
                    }
                    Ok(_) => {
                        if r.bytes_consumed == 0 && r.bytes_written == 0 {
                            break;
                        }
                    }
                    Err(_) => return Err(self.corrupt(off)),
                }
            }
        }
    }

    fn cached_base(&self, off: u64) -> Option<(Kind, Rc<Vec<u8>>)> {
        self.bases.borrow().map.get(&off).cloned()
    }

    fn cache_base(&self, off: u64, kind: Kind, data: Rc<Vec<u8>>) {
        let mut c = self.bases.borrow_mut();
        let len = data.len();
        if len > BASE_CACHE_BYTES / 4 || c.map.contains_key(&off) {
            return;
        }
        while c.bytes + len > BASE_CACHE_BYTES {
            let Some(old) = c.order.pop_front() else { break };
            if let Some(o) = c.map.remove(&old) {
                c.bytes -= o.1.len();
            }
        }
        c.bytes += len;
        c.order.push_back(off);
        c.map.insert(off, (kind, data));
    }

    /// Objeto completo na posição `off`, resolvendo a cadeia de deltas.
    fn read_at(&self, off: u64, external: &ExternalLookup) -> Result<(Kind, Vec<u8>), String> {
        let mut deltas: Vec<(u64, Vec<u8>)> = Vec::new();
        let mut cur = off;
        let (kind, mut data): (Kind, Vec<u8>) = loop {
            if let Some((k, d)) = self.cached_base(cur) {
                break (k, d.to_vec());
            }
            match self.entry(cur)? {
                Entry::Base(k, d) => break (k, d),
                Entry::Ofs(base, d) => {
                    deltas.push((cur, d));
                    cur = base;
                }
                Entry::Ref(base, d) => {
                    deltas.push((cur, d));
                    if let Some(i) = self.idx.lookup(&base) {
                        cur = self.idx.offset(i);
                    } else {
                        let (k, b) = external(&base).ok_or_else(|| format!("failed to read delta base object {base}"))?;
                        break (k, b);
                    }
                }
            }
            if deltas.len() > 10_000 {
                return Err(self.corrupt(off));
            }
        };
        if !deltas.is_empty() {
            self.cache_base(cur, kind, Rc::new(data.clone()));
        }
        while let Some((at, delta)) = deltas.pop() {
            data = apply_delta(&data, &delta).ok_or_else(|| self.corrupt(at))?;
            if !deltas.is_empty() {
                self.cache_base(at, kind, Rc::new(data.clone()));
            }
        }
        Ok((kind, data))
    }
}

fn delta_varint(d: &[u8], i: &mut usize) -> Option<usize> {
    let mut v = 0usize;
    let mut shift = 0;
    loop {
        let c = *d.get(*i)?;
        *i += 1;
        v |= ((c & 0x7f) as usize) << shift;
        shift += 7;
        if c & 0x80 == 0 {
            return Some(v);
        }
        if shift > 63 {
            return None;
        }
    }
}

/// Aplica um delta do git (`copy`/`insert`) sobre a base.
pub fn apply_delta(base: &[u8], delta: &[u8]) -> Option<Vec<u8>> {
    let mut i = 0;
    let src_size = delta_varint(delta, &mut i)?;
    if src_size != base.len() {
        return None;
    }
    let dst_size = delta_varint(delta, &mut i)?;
    let mut out = Vec::new();
    out.try_reserve(dst_size).ok()?;
    while i < delta.len() {
        let cmd = delta[i];
        i += 1;
        if cmd & 0x80 != 0 {
            let mut off = 0usize;
            let mut len = 0usize;
            for k in 0..4 {
                if cmd & (1 << k) != 0 {
                    off |= (*delta.get(i)? as usize) << (8 * k);
                    i += 1;
                }
            }
            for k in 0..3 {
                if cmd & (0x10 << k) != 0 {
                    len |= (*delta.get(i)? as usize) << (8 * k);
                    i += 1;
                }
            }
            if len == 0 {
                len = 0x10000;
            }
            out.extend_from_slice(base.get(off..off.checked_add(len)?)?);
        } else if cmd != 0 {
            let n = cmd as usize;
            out.extend_from_slice(delta.get(i..i + n)?);
            i += n;
        } else {
            return None;
        }
    }
    (out.len() == dst_size).then_some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn delta_copy_and_insert() {
        let base = b"hello world";
        // base 11 bytes, resultado 12: copia 0..6, insere "there", copia o espaço (offset 5, 1 byte).
        let mut d = vec![11, 12];
        d.extend_from_slice(&[0x90, 6]);
        d.push(5);
        d.extend_from_slice(b"there");
        d.extend_from_slice(&[0x91, 5, 1]);
        let out = apply_delta(base, &d);
        assert_eq!(out.as_deref(), Some(&b"hello there "[..]));
    }
}
