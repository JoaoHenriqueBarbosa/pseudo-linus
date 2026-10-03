//! Repositório git sobre `MemTree`, montado com os crates `gix-*` de baixo nível.
//!
//! O que é do gix: hash e codificação de objetos (`gix-hash`, `gix-object`), compressão (`gix-zlib`),
//! leitura de pack e índice de pack a partir de bytes (`gix-pack`, que aceita qualquer
//! `Deref<Target = [u8]>`), `packed-refs` a partir de bytes (`gix-ref`) e o arquivo de índice
//! (`gix-index`, `State::from_bytes` e `write_to`). O que é nosso: onde cada arquivo mora (o
//! `MemTree`), refs soltas, resolução de revisões e a política de escrita.

use std::collections::BTreeMap;
use std::io::Write;
use std::path::PathBuf;

use anyhow::{Context, Result, anyhow, bail};
use gix_hash::ObjectId;
use gix_object::Kind;
use harness::{Entry, MemTree};

pub const HASH: gix_hash::Kind = gix_hash::Kind::Sha1;

/// Converte os erros `Exn` do gix (que não são `std::error::Error`) em `anyhow`.
pub fn gx<T, E: std::fmt::Debug>(r: std::result::Result<T, E>) -> Result<T> {
    r.map_err(|e| anyhow!("{e:?}"))
}

pub fn hash_object(kind: Kind, data: &[u8]) -> Result<ObjectId> {
    gx(gix_object::compute_hash(HASH, kind, data))
}

/// Onde `HEAD` aponta.
#[derive(Clone, Debug, PartialEq)]
pub enum Head {
    /// `ref: refs/heads/x`; o `ObjectId` é `None` quando o ramo ainda não tem commits.
    Branch(String, Option<ObjectId>),
    Detached(ObjectId),
}

pub struct Repo<'a> {
    pub fs: &'a mut MemTree,
}

fn zlib_compress(data: &[u8]) -> Result<Vec<u8>> {
    let mut w = gix_zlib::stream::deflate::Write::new(Vec::new(), gix_zlib::Compression::BEST_SPEED);
    w.write_all(data)?;
    w.flush()?;
    Ok(w.into_inner())
}

fn zlib_inflate(input: &[u8], size_hint: usize) -> Result<Vec<u8>> {
    let mut state = gix_zlib::Decompress::new();
    let mut out = vec![0u8; size_hint];
    let mut rd = input;
    let n = gix_zlib::stream::inflate::read(&mut rd, &mut state, &mut out)?;
    out.truncate(n);
    Ok(out)
}

impl<'a> Repo<'a> {
    /// Abre o repositório do diretório do caso (`.git` na raiz).
    pub fn open(fs: &'a mut MemTree) -> Option<Repo<'a>> {
        matches!(fs.get(".git/HEAD"), Some(Entry::File { .. })).then_some(Repo { fs })
    }

    pub fn read(&self, rel: &str) -> Option<&[u8]> {
        self.fs.read(&format!(".git/{rel}"))
    }

    pub fn write(&mut self, rel: &str, data: impl Into<Vec<u8>>, mode: u32) {
        self.fs.insert(&format!(".git/{rel}"), Entry::file(data.into(), mode));
    }

    // -- objetos ---------------------------------------------------------------------------------

    pub fn write_object(&mut self, kind: Kind, data: &[u8]) -> Result<ObjectId> {
        let id = hash_object(kind, data)?;
        let hex = id.to_hex().to_string();
        let path = format!("objects/{}/{}", &hex[..2], &hex[2..]);
        if self.read(&path).is_none() {
            let mut raw = gix_object::encode::loose_header(kind, data.len() as u64).to_vec();
            raw.extend_from_slice(data);
            // Objetos soltos são somente leitura, como o git grava.
            self.write(&path, zlib_compress(&raw)?, 0o444);
        }
        Ok(id)
    }

    pub fn write_encoded(&mut self, obj: &dyn gix_object::WriteTo) -> Result<ObjectId> {
        let mut buf = Vec::new();
        obj.write_to(&mut buf)?;
        self.write_object(obj.kind(), &buf)
    }

    fn read_loose(&self, id: &ObjectId) -> Result<Option<(Kind, Vec<u8>)>> {
        let hex = id.to_hex().to_string();
        let Some(compressed) = self.read(&format!("objects/{}/{}", &hex[..2], &hex[2..])) else { return Ok(None) };
        let head = zlib_inflate(compressed, 64)?;
        let (kind, size, consumed) = gx(gix_object::decode::loose_header(&head))?;
        let full = zlib_inflate(compressed, consumed + size as usize)?;
        Ok(Some((kind, full[consumed..].to_vec())))
    }

    /// Pares (índice, pack) presentes em `objects/pack`.
    fn packs(&self) -> Vec<(String, String)> {
        let mut out = Vec::new();
        for k in self.fs.entries.keys() {
            if let Some(name) = k.strip_prefix(".git/objects/pack/")
                && let Some(stem) = name.strip_suffix(".idx")
            {
                out.push((format!("objects/pack/{stem}.idx"), format!("objects/pack/{stem}.pack")));
            }
        }
        out
    }

    fn read_packed(&self, id: &ObjectId) -> Result<Option<(Kind, Vec<u8>)>> {
        for (idx_path, pack_path) in self.packs() {
            let (Some(idx), Some(pack)) = (self.read(&idx_path), self.read(&pack_path)) else { continue };
            let index = gx(gix_pack::index::File::from_data(idx.to_vec(), PathBuf::from(&idx_path), HASH))?;
            let Some(entry_index) = index.lookup(id) else { continue };
            let offset = index.pack_offset_at_index(entry_index);
            let data = gx(gix_pack::data::File::from_data(pack.to_vec(), PathBuf::from(&pack_path), HASH))?;
            let entry = gx(data.entry(offset))?;
            let mut out = Vec::new();
            let mut inflate = gix_zlib::Inflate::default();
            // Bases por id (REF_DELTA) dentro do mesmo pack.
            let resolve = |base: &gix_hash::oid, _buf: &mut Vec<u8>| {
                index.lookup(base).and_then(|i| {
                    data.entry(index.pack_offset_at_index(i)).ok().map(gix_pack::data::decode::entry::ResolvedBase::InPack)
                })
            };
            let outcome = gx(data.decode_entry(entry, &mut out, &mut inflate, &resolve, &mut gix_pack::cache::Never))?;
            out.truncate(outcome.object_size as usize);
            return Ok(Some((outcome.kind, out)));
        }
        Ok(None)
    }

    pub fn read_object(&self, id: &ObjectId) -> Result<(Kind, Vec<u8>)> {
        if let Some(o) = self.read_loose(id)? {
            return Ok(o);
        }
        if let Some(o) = self.read_packed(id)? {
            return Ok(o);
        }
        bail!("objeto {id} não encontrado")
    }

    pub fn has_object(&self, id: &ObjectId) -> bool {
        self.read_object(id).is_ok()
    }

    /// Todos os ids conhecidos (soltos e em packs).
    pub fn all_ids(&self) -> Result<Vec<ObjectId>> {
        let mut ids = Vec::new();
        for k in self.fs.entries.keys() {
            if let Some(rest) = k.strip_prefix(".git/objects/")
                && rest.len() == 41
                && rest.as_bytes()[2] == b'/'
                && let Ok(id) = ObjectId::from_hex(format!("{}{}", &rest[..2], &rest[3..]).as_bytes())
            {
                ids.push(id);
            }
        }
        for (idx_path, _) in self.packs() {
            let Some(idx) = self.read(&idx_path) else { continue };
            let index = gx(gix_pack::index::File::from_data(idx.to_vec(), PathBuf::from(&idx_path), HASH))?;
            for i in 0..index.num_objects() {
                ids.push(index.oid_at_index(i).to_owned());
            }
        }
        ids.sort();
        ids.dedup();
        Ok(ids)
    }

    pub fn commit(&self, id: &ObjectId) -> Result<gix_object::Commit> {
        let (kind, data) = self.read_object(id)?;
        if kind != Kind::Commit {
            bail!("{id} não é commit");
        }
        let c = gx(gix_object::CommitRef::from_bytes(&data, HASH))?;
        gx(c.try_into())
    }

    pub fn tree(&self, id: &ObjectId) -> Result<gix_object::Tree> {
        let (kind, data) = self.read_object(id)?;
        if kind != Kind::Tree {
            bail!("{id} não é tree");
        }
        let t = gx(gix_object::TreeRef::from_bytes(&data, HASH))?;
        Ok(t.into())
    }

    /// Arquivos de uma tree, recursivamente: caminho -> (modo, id).
    pub fn flatten_tree(&self, id: &ObjectId, prefix: &str, out: &mut Vec<(String, u32, ObjectId)>) -> Result<()> {
        for e in self.tree(id)?.entries {
            let name = format!("{prefix}{}", e.filename);
            let mode = e.mode.value() as u32;
            if e.mode.is_tree() {
                self.flatten_tree(&e.oid, &format!("{name}/"), out)?;
            } else {
                out.push((name, mode, e.oid));
            }
        }
        Ok(())
    }

    // -- refs ------------------------------------------------------------------------------------

    fn packed_refs(&self) -> Result<Vec<(String, ObjectId)>> {
        let Some(bytes) = self.read("packed-refs") else { return Ok(Vec::new()) };
        let buf = gx(gix_ref::packed::Buffer::from_bytes(bytes, HASH))?;
        let mut out = Vec::new();
        for r in gx(buf.iter())? {
            let r = gx(r)?;
            out.push((r.name.as_bstr().to_string(), r.target()));
        }
        Ok(out)
    }

    /// Valor de uma ref (sem seguir simbólicas além de uma, que é o caso do HEAD).
    pub fn ref_value(&self, name: &str) -> Result<Option<ObjectId>> {
        if let Some(raw) = self.read(name) {
            let text = String::from_utf8_lossy(raw).trim().to_string();
            if let Some(target) = text.strip_prefix("ref: ") {
                return self.ref_value(target);
            }
            return Ok(Some(gx(ObjectId::from_hex(text.as_bytes()))?));
        }
        Ok(self.packed_refs()?.into_iter().find(|(n, _)| n == name).map(|(_, id)| id))
    }

    pub fn head(&self) -> Result<Head> {
        let raw = self.read("HEAD").context("sem HEAD")?;
        let text = String::from_utf8_lossy(raw).trim().to_string();
        match text.strip_prefix("ref: ") {
            Some(target) => Ok(Head::Branch(target.to_string(), self.ref_value(target)?)),
            None => Ok(Head::Detached(gx(ObjectId::from_hex(text.as_bytes()))?)),
        }
    }

    pub fn head_commit(&self) -> Result<Option<ObjectId>> {
        Ok(match self.head()? {
            Head::Branch(_, id) => id,
            Head::Detached(id) => Some(id),
        })
    }

    /// Grava uma ref; `HEAD` simbólico é seguido até o ramo.
    pub fn write_ref(&mut self, name: &str, id: &ObjectId) -> Result<()> {
        let target = if name == "HEAD" {
            match self.head()? {
                Head::Branch(b, _) => b,
                Head::Detached(_) => "HEAD".to_string(),
            }
        } else {
            name.to_string()
        };
        self.write(&target, format!("{}\n", id.to_hex()), 0o644);
        Ok(())
    }

    // -- revisões --------------------------------------------------------------------------------

    /// Resolve `HEAD`, nomes de ramo/tag, hex completo ou abreviado, `X~N`, `X^`, `X^{tree}`.
    pub fn rev_parse(&self, spec: &str) -> Result<ObjectId> {
        if let Some(base) = spec.strip_suffix("^{tree}") {
            let c = self.rev_parse(base)?;
            return Ok(self.commit(&c)?.tree);
        }
        if let Some((base, n)) = spec.rsplit_once('~') {
            let mut id = self.rev_parse(base)?;
            let n: usize = if n.is_empty() { 1 } else { n.parse()? };
            for _ in 0..n {
                id = *self.commit(&id)?.parents.first().context("sem pai")?;
            }
            return Ok(id);
        }
        if let Some(base) = spec.strip_suffix('^') {
            let id = self.rev_parse(base)?;
            return self.commit(&id)?.parents.first().copied().context("sem pai");
        }
        for candidate in [spec.to_string(), format!("refs/{spec}"), format!("refs/heads/{spec}"), format!("refs/tags/{spec}")] {
            if candidate == "HEAD" {
                if let Some(id) = self.head_commit()? {
                    return Ok(id);
                }
                bail!("HEAD sem commits");
            }
            if let Some(id) = self.ref_value(&candidate)? {
                return Ok(id);
            }
        }
        if spec.len() >= 4 && spec.len() <= 40 && spec.bytes().all(|b| b.is_ascii_hexdigit()) {
            let lower = spec.to_ascii_lowercase();
            let matches: Vec<ObjectId> =
                self.all_ids()?.into_iter().filter(|id| id.to_hex().to_string().starts_with(&lower)).collect();
            if matches.len() == 1 {
                return Ok(matches[0]);
            }
        }
        bail!("revisão desconhecida: {spec}")
    }

    /// Abreviação única de 7+ caracteres.
    pub fn abbrev(&self, id: &ObjectId) -> String {
        let hex = id.to_hex().to_string();
        let ids = self.all_ids().unwrap_or_default();
        for len in 7..=40 {
            let p = &hex[..len];
            if ids.iter().filter(|o| o.to_hex().to_string().starts_with(p)).count() <= 1 {
                return p.to_string();
            }
        }
        hex
    }

    // -- índice ----------------------------------------------------------------------------------

    pub fn index(&self) -> Result<gix_index::State> {
        match self.read("index") {
            Some(bytes) => {
                let (state, _) = gx(gix_index::State::from_bytes(
                    bytes,
                    filetime::FileTime::from_unix_time(harness::FIXTURE_MTIME as i64, 0),
                    HASH,
                    gix_index::decode::Options::default(),
                ))?;
                Ok(state)
            }
            None => Ok(gix_index::State::new(HASH)),
        }
    }

    pub fn write_index(&mut self, state: &gix_index::State) -> Result<()> {
        let mut buf = Vec::new();
        gx(state.write_to(&mut buf, gix_index::write::Options::default()))?;
        // `State::write_to` não grava o checksum final (quem grava é `gix_index::File`, que exige caminho).
        let mut hasher = gix_hash::hasher(HASH);
        hasher.update(&buf);
        let digest = gx(hasher.try_finalize())?;
        buf.extend_from_slice(digest.as_bytes());
        self.write("index", buf, 0o644);
        Ok(())
    }

    /// Índice como mapa caminho -> (modo, id), só estágio 0.
    pub fn index_map(&self) -> Result<IndexMap> {
        let state = self.index()?;
        let mut map = IndexMap::new();
        for e in state.entries() {
            map.insert(e.path(&state).to_string(), (e.mode.bits(), e.id));
        }
        Ok(map)
    }

    /// Regrava o índice a partir do mapa. O stat fica zerado (fora o tamanho): no sandbox não há
    /// inode nem ctime do host, e o git do oráculo, ao ver stat diferente, recalcula o hash.
    pub fn set_index_map(&mut self, map: &IndexMap, sizes: &BTreeMap<String, u32>) -> Result<()> {
        let mut state = gix_index::State::new(HASH);
        for (path, (mode, id)) in map {
            let stat = gix_index::entry::Stat { size: sizes.get(path).copied().unwrap_or(0), ..Default::default() };
            let mode = gix_index::entry::Mode::from_bits(*mode).context("modo de índice inválido")?;
            state.dangerously_push_entry(stat, *id, gix_index::entry::Flags::empty(), mode, path.as_str().into());
        }
        state.sort_entries();
        self.write_index(&state)
    }

    /// Arquivos da tree do HEAD (vazio se ainda não há commit).
    pub fn head_tree_map(&self) -> Result<IndexMap> {
        let mut map = IndexMap::new();
        if let Some(c) = self.head_commit()? {
            let mut files = Vec::new();
            self.flatten_tree(&self.commit(&c)?.tree, "", &mut files)?;
            for (p, m, id) in files {
                map.insert(p, (m, id));
            }
        }
        Ok(map)
    }
}

pub type IndexMap = BTreeMap<String, (u32, ObjectId)>;

/// Tamanhos dos arquivos do worktree, pro stat do índice.
pub fn worktree_sizes(fs: &MemTree) -> BTreeMap<String, u32> {
    worktree_files(fs).into_iter().map(|(p, _, d)| (p, d.len() as u32)).collect()
}

/// Arquivos do worktree (fora do `.git`): caminho -> (modo git, conteúdo do blob).
pub fn worktree_files(fs: &MemTree) -> Vec<(String, u32, Vec<u8>)> {
    let mut out = Vec::new();
    for (path, e) in &fs.entries {
        if path == ".git" || path.starts_with(".git/") {
            continue;
        }
        match e {
            Entry::File { mode, data: Some(d), .. } => {
                let m = if mode & 0o111 != 0 { 0o100755 } else { 0o100644 };
                out.push((path.clone(), m, d.as_slice().to_vec()));
            }
            Entry::Symlink { target } => out.push((path.clone(), 0o120000, target.as_bytes().to_vec())),
            _ => {}
        }
    }
    out
}
