//! O arquivo de índice (`.git/index`): leitura das versões 2, 3 e 4 com as extensões opcionais
//! ignoradas, escrita na versão 2 (3 quando há flags estendidas), estágios de conflito e a
//! comparação de `stat` do git (com o caso "racy").

use sysabi::{Stat, TimeSpec};

use crate::error::{Fail, R};
use crate::hash::{self, Oid};
use crate::object::{self, MODE_EXEC, MODE_GITLINK, MODE_LINK};
use crate::os;

pub const FLAG_ASSUME_VALID: u16 = 0x8000;
pub const FLAG_EXTENDED: u16 = 0x4000;
pub const EXT_SKIP_WORKTREE: u16 = 0x4000;
pub const EXT_INTENT_TO_ADD: u16 = 0x2000;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IEntry {
    pub ctime: (u32, u32),
    pub mtime: (u32, u32),
    pub dev: u32,
    pub ino: u32,
    pub mode: u32,
    pub uid: u32,
    pub gid: u32,
    pub size: u32,
    pub oid: Oid,
    /// Bits altos das flags (assume-valid); o estágio fica em `stage`.
    pub flags: u16,
    pub stage: u8,
    /// Flags estendidas (skip-worktree, intent-to-add).
    pub ext: u16,
    pub path: Vec<u8>,
}

impl IEntry {
    /// Entrada nova a partir de um `stat` do arquivo.
    pub fn from_stat(path: Vec<u8>, oid: Oid, mode: u32, st: &Stat) -> IEntry {
        let mut e = IEntry::bare(path, oid, mode);
        e.set_stat(st);
        e
    }

    /// Entrada sem dados de `stat` (vinda de uma tree).
    pub fn bare(path: Vec<u8>, oid: Oid, mode: u32) -> IEntry {
        IEntry { ctime: (0, 0), mtime: (0, 0), dev: 0, ino: 0, mode, uid: 0, gid: 0, size: 0, oid, flags: 0, stage: 0, ext: 0, path }
    }

    pub fn set_stat(&mut self, st: &Stat) {
        self.ctime = (st.ctime.sec as u32, st.ctime.nsec);
        self.mtime = (st.mtime.sec as u32, st.mtime.nsec);
        self.dev = st.dev as u32;
        self.ino = st.ino as u32;
        self.uid = st.uid;
        self.gid = st.gid;
        self.size = st.size as u32;
    }

    pub fn intent_to_add(&self) -> bool {
        self.ext & EXT_INTENT_TO_ADD != 0
    }

    pub fn skip_worktree(&self) -> bool {
        self.ext & EXT_SKIP_WORKTREE != 0
    }

    pub fn assume_valid(&self) -> bool {
        self.flags & FLAG_ASSUME_VALID != 0
    }
}

#[derive(Clone, Debug, Default)]
pub struct Index {
    pub entries: Vec<IEntry>,
    /// `mtime` do arquivo do índice quando foi lido (pro teste de "racy").
    pub mtime: Option<TimeSpec>,
    pub version: u32,
    /// Leu-se de um arquivo existente?
    pub existed: bool,
}

fn be32(d: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_be_bytes(d.get(at..at + 4)?.try_into().ok()?))
}

fn be16(d: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_be_bytes(d.get(at..at + 2)?.try_into().ok()?))
}

/// Ordem do índice: caminho (bytes) e depois estágio.
pub fn cmp_path_stage(a: &[u8], sa: u8, b: &[u8], sb: u8) -> std::cmp::Ordering {
    a.cmp(b).then(sa.cmp(&sb))
}

impl Index {
    pub fn parse(data: &[u8]) -> Result<Index, String> {
        if data.len() < 12 + 20 || &data[..4] != b"DIRC" {
            return Err("bad signature 0x00000000".into());
        }
        let version = be32(data, 4).ok_or("bad index file")?;
        if !(2..=4).contains(&version) {
            return Err(format!("bad index version {version}"));
        }
        let body_end = data.len() - 20;
        if hash::sha1(&data[..body_end]) != data[body_end..] && data[body_end..] != [0u8; 20] {
            return Err("bad index file sha1 signature".into());
        }
        let count = be32(data, 8).ok_or("bad index file")? as usize;
        let mut entries = Vec::new();
        entries.try_reserve(count.min(1 << 24)).map_err(|_| "index too large".to_string())?;
        let mut at = 12;
        let mut prev: Vec<u8> = Vec::new();
        for _ in 0..count {
            let field = |k: usize| be32(data, at + k * 4).ok_or_else(|| "index file corrupt".to_string());
            let ctime = (field(0)?, field(1)?);
            let mtime = (field(2)?, field(3)?);
            let dev = field(4)?;
            let ino = field(5)?;
            let mode = field(6)?;
            let uid = field(7)?;
            let gid = field(8)?;
            let size = field(9)?;
            let oid = Oid::from_bytes(data.get(at + 40..at + 60).ok_or("index file corrupt")?).expect("20");
            let flags = be16(data, at + 60).ok_or("index file corrupt")?;
            let mut p = at + 62;
            let mut ext = 0;
            if flags & FLAG_EXTENDED != 0 {
                if version < 3 {
                    return Err("index file corrupt".into());
                }
                ext = be16(data, p).ok_or("index file corrupt")?;
                p += 2;
            }
            let stage = ((flags >> 12) & 3) as u8;
            let path: Vec<u8>;
            if version == 4 {
                // Prefixo comprimido: varint com quantos bytes tirar do nome anterior.
                let mut c = *data.get(p).ok_or("index file corrupt")?;
                p += 1;
                let mut strip = (c & 0x7f) as usize;
                while c & 0x80 != 0 {
                    c = *data.get(p).ok_or("index file corrupt")?;
                    p += 1;
                    strip = ((strip + 1) << 7) | (c & 0x7f) as usize;
                }
                let nul = data[p..body_end].iter().position(|b| *b == 0).ok_or("index file corrupt")? + p;
                if strip > prev.len() {
                    return Err("index file corrupt".into());
                }
                let mut name = prev[..prev.len() - strip].to_vec();
                name.extend_from_slice(&data[p..nul]);
                path = name;
                at = nul + 1;
            } else {
                let namelen = (flags & 0x0fff) as usize;
                let nul = if namelen < 0x0fff {
                    p + namelen
                } else {
                    data[p..body_end].iter().position(|b| *b == 0).ok_or("index file corrupt")? + p
                };
                path = data.get(p..nul).ok_or("index file corrupt")?.to_vec();
                let len = nul - at;
                at += (len + 8) & !7;
            }
            prev = path.clone();
            entries.push(IEntry { ctime, mtime, dev, ino, mode, uid, gid, size, oid, flags: flags & FLAG_ASSUME_VALID, stage, ext, path });
        }
        // Extensões: as opcionais (maiúscula) são ignoradas.
        while at + 8 <= body_end {
            let sig = &data[at..at + 4];
            let len = be32(data, at + 4).ok_or("index file corrupt")? as usize;
            if !sig[0].is_ascii_uppercase() {
                return Err(format!("index uses {} extension, which we do not understand", String::from_utf8_lossy(sig)));
            }
            at += 8 + len;
        }
        Ok(Index { entries, mtime: None, version, existed: true })
    }

    /// Lê o índice; ausente vira índice vazio.
    pub fn load(path: &[u8]) -> R<Index> {
        let data = match os::read_opt(path) {
            Ok(Some(d)) => d,
            Ok(None) => return Ok(Index { version: 2, ..Index::default() }),
            Err(e) => return Err(Fail::Fatal(format!("{}: index file open failed: {}", os::lossy(path), e.message()))),
        };
        let mut idx = Index::parse(&data).map_err(|e| Fail::Fatal(format!("{e}\nfatal: index file corrupt")))?;
        idx.mtime = os::stat(path).ok().map(|s| s.mtime);
        Ok(idx)
    }

    pub fn encode(&self) -> Vec<u8> {
        let extended = self.entries.iter().any(|e| e.ext != 0);
        let version: u32 = if extended { 3 } else { 2 };
        let mut out = Vec::new();
        out.extend_from_slice(b"DIRC");
        out.extend_from_slice(&version.to_be_bytes());
        out.extend_from_slice(&(self.entries.len() as u32).to_be_bytes());
        for e in &self.entries {
            let start = out.len();
            for v in [e.ctime.0, e.ctime.1, e.mtime.0, e.mtime.1, e.dev, e.ino, e.mode, e.uid, e.gid, e.size] {
                out.extend_from_slice(&v.to_be_bytes());
            }
            out.extend_from_slice(&e.oid.0);
            let namelen = e.path.len().min(0x0fff) as u16;
            let mut flags = e.flags | ((e.stage as u16 & 3) << 12) | namelen;
            if e.ext != 0 {
                flags |= FLAG_EXTENDED;
            }
            out.extend_from_slice(&flags.to_be_bytes());
            if e.ext != 0 {
                out.extend_from_slice(&e.ext.to_be_bytes());
            }
            out.extend_from_slice(&e.path);
            let len = out.len() - start;
            let padded = (len + 8) & !7;
            out.resize(start + padded, 0);
        }
        let digest = hash::sha1(&out);
        out.extend_from_slice(&digest);
        out
    }

    /// Grava com `index.lock` + rename.
    pub fn write(&self, path: &[u8]) -> R<()> {
        let data = self.encode();
        os::write_locked(path, &data).map_err(Fail::Fatal)
    }

    /// Posição de (caminho, estágio): `Ok` se existe, `Err` com o ponto de inserção.
    pub fn pos(&self, path: &[u8], stage: u8) -> Result<usize, usize> {
        self.entries.binary_search_by(|e| cmp_path_stage(&e.path, e.stage, path, stage))
    }

    /// Entrada de estágio 0.
    pub fn get(&self, path: &[u8]) -> Option<&IEntry> {
        self.pos(path, 0).ok().map(|i| &self.entries[i])
    }

    /// Alguma entrada (qualquer estágio) desse caminho?
    pub fn has_path(&self, path: &[u8]) -> bool {
        let i = self.pos(path, 0).unwrap_or_else(|i| i);
        self.entries.get(i).is_some_and(|e| e.path == path)
    }

    /// Entradas de todos os estágios de um caminho.
    pub fn stages(&self, path: &[u8]) -> Vec<&IEntry> {
        let i = self.pos(path, 0).unwrap_or_else(|i| i);
        self.entries[i..].iter().take_while(|e| e.path == path).collect()
    }

    /// Algum arquivo sob `dir/`?
    pub fn has_dir(&self, dir: &[u8]) -> bool {
        let mut p = dir.to_vec();
        p.push(b'/');
        let i = self.pos(&p, 0).unwrap_or_else(|i| i);
        self.entries.get(i).is_some_and(|e| e.path.starts_with(&p))
    }

    /// Faixa das entradas sob `dir/`.
    pub fn dir_range(&self, dir: &[u8]) -> std::ops::Range<usize> {
        if dir.is_empty() {
            return 0..self.entries.len();
        }
        let mut p = dir.to_vec();
        p.push(b'/');
        let start = self.pos(&p, 0).unwrap_or_else(|i| i);
        let mut end = start;
        while end < self.entries.len() && self.entries[end].path.starts_with(&p) {
            end += 1;
        }
        start..end
    }

    /// Adiciona (ou troca) uma entrada, tirando conflitos arquivo/diretório e os outros estágios do
    /// mesmo caminho, como o `add_index_entry` com `ADD_CACHE_OK_TO_REPLACE`.
    pub fn add(&mut self, e: IEntry) {
        // Remove os estágios de conflito do caminho, se a nova entrada é de estágio 0.
        if e.stage == 0 {
            let i = self.pos(&e.path, 0).unwrap_or_else(|i| i);
            let mut j = i;
            while j < self.entries.len() && self.entries[j].path == e.path {
                j += 1;
            }
            self.entries.drain(i..j);
        }
        // Um arquivo pai (a/b chega e "a" era arquivo).
        let mut k = 0;
        while let Some(slash) = e.path[k..].iter().position(|c| *c == b'/') {
            let parent = &e.path[..k + slash];
            while let Ok(i) = self.pos(parent, 0) {
                self.entries.remove(i);
            }
            for st in 1..=3 {
                if let Ok(i) = self.pos(parent, st) {
                    self.entries.remove(i);
                }
            }
            k += slash + 1;
        }
        // Diretório com o mesmo nome ("a" chega e havia a/...).
        let r = self.dir_range(&e.path);
        if !r.is_empty() {
            self.entries.drain(r);
        }
        match self.pos(&e.path, e.stage) {
            Ok(i) => self.entries[i] = e,
            Err(i) => self.entries.insert(i, e),
        }
    }

    /// Insere sem checar conflitos (montagem a partir de tree, estágios de merge).
    pub fn insert_raw(&mut self, e: IEntry) {
        match self.pos(&e.path, e.stage) {
            Ok(i) => self.entries[i] = e,
            Err(i) => self.entries.insert(i, e),
        }
    }

    /// Remove todos os estágios de um caminho; diz se havia algo.
    pub fn remove(&mut self, path: &[u8]) -> bool {
        let i = self.pos(path, 0).unwrap_or_else(|i| i);
        let mut j = i;
        while j < self.entries.len() && self.entries[j].path == path {
            j += 1;
        }
        let any = j > i;
        self.entries.drain(i..j);
        any
    }

    pub fn has_conflicts(&self) -> bool {
        self.entries.iter().any(|e| e.stage != 0)
    }

    /// Caminhos com conflito, em ordem.
    pub fn unmerged_paths(&self) -> Vec<Vec<u8>> {
        let mut out: Vec<Vec<u8>> = Vec::new();
        for e in &self.entries {
            if e.stage != 0 && out.last() != Some(&e.path) {
                out.push(e.path.clone());
            }
        }
        out
    }

    pub fn sort(&mut self) {
        self.entries.sort_by(|a, b| cmp_path_stage(&a.path, a.stage, &b.path, b.stage));
    }

    /// A entrada pode ter mudado mesmo com `stat` igual (escrita no mesmo segundo do índice).
    pub fn is_racy(&self, e: &IEntry) -> bool {
        match self.mtime {
            None => true,
            Some(m) => {
                let im = (m.sec as u32, m.nsec);
                im <= e.mtime
            }
        }
    }
}

/// `ie_match_stat`: o `stat` bate com a entrada? (Não olha conteúdo.)
pub fn stat_matches(e: &IEntry, st: &Stat, trust_exec: bool) -> bool {
    let mode = os::git_mode_of(st);
    if object::is_gitlink(e.mode) {
        return st.file_type() == sysabi::FileType::Directory;
    }
    let type_ok = match e.mode & 0o170000 {
        0o100000 => st.file_type() == sysabi::FileType::Regular && (!trust_exec || mode == object::canon_mode(e.mode)),
        0o120000 => st.file_type() == sysabi::FileType::Symlink,
        _ => false,
    };
    if !type_ok {
        return false;
    }
    e.mtime == (st.mtime.sec as u32, st.mtime.nsec)
        && e.ctime == (st.ctime.sec as u32, st.ctime.nsec)
        && e.ino == st.ino as u32
        && e.uid == st.uid
        && e.gid == st.gid
        && e.size == st.size as u32
}

/// Modo de índice pra um arquivo da worktree (respeitando `core.filemode=false`).
pub fn mode_for(st: &Stat, old: Option<u32>, trust_exec: bool) -> u32 {
    let m = os::git_mode_of(st);
    if m == object::MODE_TREE {
        return MODE_GITLINK;
    }
    if !trust_exec && m != MODE_LINK {
        if let Some(o) = old
            && object::is_reg(o)
        {
            return object::canon_mode(o);
        }
        return object::MODE_BLOB;
    }
    if m == MODE_EXEC || m == object::MODE_BLOB || m == MODE_LINK {
        return m;
    }
    m
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_and_order() {
        let mut idx = Index::default();
        let id = hash::hash_object(hash::Kind::Blob, b"x");
        idx.add(IEntry::bare(b"b".to_vec(), id, 0o100644));
        idx.add(IEntry::bare(b"a/c".to_vec(), id, 0o100644));
        idx.add(IEntry::bare(b"a".to_vec(), id, 0o100755));
        let paths: Vec<&[u8]> = idx.entries.iter().map(|e| e.path.as_slice()).collect();
        assert_eq!(paths, vec![b"a".as_slice(), b"b".as_slice()]);
        let data = idx.encode();
        let back = Index::parse(&data).ok().unwrap();
        assert_eq!(back.entries, idx.entries);
    }
}
