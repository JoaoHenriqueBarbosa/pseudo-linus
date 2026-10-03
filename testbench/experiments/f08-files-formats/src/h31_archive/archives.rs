//! tar e zip: árvore de referência, escrita e leitura pelas crates e comparação com o que o GNU vê.

use std::collections::BTreeMap;
use std::io::{Cursor, Read, Write};

use anyhow::{Context, Result, bail};
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use harness::case::{FileSpec, FileTable};
use serde::Serialize;

/// 2026-01-15 12:00:00 UTC (mtime de toda fixture do harness).
pub const M: u64 = harness::FIXTURE_MTIME;
/// 2025-12-31 23:59:58 UTC (segundo par: cabe exato no horário DOS do zip).
pub const M_EXEC: u64 = 1_767_225_598;
/// 2001-02-03 04:05:06 UTC.
pub const M_OLD: u64 = 981_173_106;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Kind {
    File(Vec<u8>),
    Dir,
    Symlink(String),
    Hardlink(String),
}

#[derive(Clone, Debug)]
pub struct Node {
    pub path: String,
    pub kind: Kind,
    pub mode: u32,
    pub mtime: u64,
    /// Cabe em ustar (caminho até 255 com divisão prefixo/nome, alvo de link até 100).
    pub ustar: bool,
    /// Entra no zip (zip não tem hardlink; setuid e sticky o unzip não restaura sem -K).
    pub zip: bool,
}

fn node(path: &str, kind: Kind, mode: u32, mtime: u64) -> Node {
    Node { path: path.to_string(), kind, mode, mtime, ustar: true, zip: true }
}

pub fn long_dir() -> String {
    "d".repeat(60)
}

pub fn long_file() -> String {
    format!("{}.txt", "f".repeat(70))
}

/// Árvore de referência: `t/` é portável (ustar), `x/` só cabe em gnu e pax.
pub fn tree() -> Vec<Node> {
    let big = super::corpus::records(100 * 1024);
    let ld = long_dir();
    let mut v = vec![
        node("t", Kind::Dir, 0o755, M),
        node("t/a.txt", Kind::File(b"hello world\n".to_vec()), 0o644, M),
        node("t/abs", Kind::Symlink("/etc/passwd".into()), 0o777, M),
        node("t/big.bin", Kind::File(big), 0o644, M),
        node(&format!("t/{ld}"), Kind::Dir, 0o755, M),
        node(&format!("t/{ld}/{}", long_file()), Kind::File(b"long\n".to_vec()), 0o644, M),
        node("t/empty", Kind::Dir, 0o755, M),
        node("t/exec.sh", Kind::File(b"#!/bin/sh\necho hi\n".to_vec()), 0o755, M_EXEC),
        node("t/hard", Kind::Hardlink("t/a.txt".into()), 0o644, M),
        node("t/link", Kind::Symlink("a.txt".into()), 0o777, M),
        node("t/ro.txt", Kind::File(b"ro\n".to_vec()), 0o444, M),
        node("t/setuid", Kind::File(b"x".to_vec()), 0o4755, M),
        node("t/spaces and ünïcode.txt", Kind::File(b"u\n".to_vec()), 0o644, M),
        node("t/sticky", Kind::Dir, 0o1777, M),
        node("t/sub", Kind::Dir, 0o750, M),
        node("t/sub/deep.txt", Kind::File(b"deep\n".to_vec()), 0o600, M_OLD),
    ];
    for n in &mut v {
        if n.path == "t/hard" || n.path == "t/setuid" || n.path == "t/sticky" {
            n.zip = false;
        }
    }
    let x90 = |i: u32| format!("{}{i}", "x".repeat(90));
    let deep = format!("x/{}/{}/{}", x90(1), x90(2), x90(3));
    let extras = vec![
        node("x", Kind::Dir, 0o755, M),
        node("x/longlink", Kind::Symlink("target".repeat(20)), 0o777, M),
        node(&format!("x/{}", x90(1)), Kind::Dir, 0o755, M),
        node(&format!("x/{}/{}", x90(1), x90(2)), Kind::Dir, 0o755, M),
        node(&deep, Kind::Dir, 0o755, M),
        node(&format!("{deep}/file.txt"), Kind::File(b"very long\n".to_vec()), 0o644, M),
    ];
    for mut n in extras {
        n.ustar = false;
        n.zip = false;
        v.push(n);
    }
    v
}

/// Subconjunto de nós de uma variante de arquivo.
pub fn subset(variant: &str) -> Vec<Node> {
    let all = tree();
    match variant {
        "ustar" => all.into_iter().filter(|n| n.ustar).collect(),
        "v7" => all
            .into_iter()
            .filter(|n| ["t/a.txt", "t/exec.sh", "t/link", "t/hard", "t/sub", "t/sub/deep.txt"].contains(&n.path.as_str()))
            .collect(),
        "zip" => all.into_iter().filter(|n| n.zip).collect(),
        _ => all,
    }
}

fn shq(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

/// Fixture do caso do oráculo com a árvore (arquivos, diretórios e symlinks com modo; hardlink e
/// mtimes o script acerta depois).
pub fn tree_fixture(prefix: &str) -> BTreeMap<String, FileSpec> {
    let mut files = BTreeMap::new();
    for n in tree() {
        let spec = match &n.kind {
            Kind::File(d) => FileTable { content_b64: Some(STANDARD.encode(d)), mode: Some(n.mode), ..FileTable::default() },
            Kind::Dir => FileTable { dir: true, mode: Some(n.mode), ..FileTable::default() },
            Kind::Symlink(t) => FileTable { symlink: Some(t.clone()), ..FileTable::default() },
            Kind::Hardlink(_) => continue,
        };
        files.insert(format!("{prefix}/{}", n.path), FileSpec::Table(spec));
    }
    files
}

/// Comandos que completam a árvore materializada (hardlink e mtimes, dos mais fundos pros mais rasos).
pub fn tree_finish_script(prefix: &str) -> String {
    let mut s = format!("cd {prefix}\n");
    for n in tree() {
        if let Kind::Hardlink(target) = &n.kind {
            s.push_str(&format!("ln {} {}\n", shq(target), shq(&n.path)));
        }
    }
    for n in tree().iter().rev() {
        s.push_str(&format!("touch -h -d @{} {}\n", n.mtime, shq(&n.path)));
    }
    s.push_str("cd - >/dev/null\n");
    s
}

/// Comandos GNU que criam os arquivos de referência em `out/tar` e `out/zip` a partir de `src/`.
pub fn gnu_archive_script() -> String {
    let zip_list: Vec<String> = subset("zip").iter().map(|n| shq(&n.path)).collect();
    let zip_list = zip_list.join(" ");
    format!(
        "mkdir -p out/tar out/zip\n\
         cd src\n\
         tar --sort=name --format=ustar -cf ../out/tar/ustar.tar t\n\
         tar --sort=name --format=gnu -cf ../out/tar/gnu.tar t x\n\
         tar --sort=name --format=oldgnu -cf ../out/tar/oldgnu.tar t x\n\
         tar --sort=name --format=pax -cf ../out/tar/pax.tar t x\n\
         tar --format=v7 -cf ../out/tar/v7.tar t/a.txt t/exec.sh t/link t/hard t/sub\n\
         zip -q -y ../out/zip/deflate.zip {zip_list}\n\
         zip -q -y -0 ../out/zip/stored.zip {zip_list}\n\
         zip -q -y -Z bzip2 ../out/zip/bzip2.zip {zip_list}\n\
         cd ..\n"
    )
}

/// O que um leitor viu de uma entrada.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Seen {
    pub kind: String,
    pub mode: u32,
    pub mtime: u64,
    pub link: Option<String>,
    pub sha256: Option<String>,
}

/// Resultado da comparação de um arquivo inteiro contra a árvore esperada.
#[derive(Clone, Debug, Default, Serialize)]
pub struct ArchiveCheck {
    pub expected: usize,
    pub ok: usize,
    pub problems: Vec<String>,
}

impl ArchiveCheck {
    pub fn passed(&self) -> bool {
        self.problems.is_empty() && self.ok == self.expected
    }
}

fn expected_seen(n: &Node) -> Seen {
    let (kind, link, sha) = match &n.kind {
        Kind::File(d) => ("file", None, Some(harness::memtree::sha256_hex(d))),
        Kind::Dir => ("dir", None, None),
        Kind::Symlink(t) => ("symlink", Some(t.clone()), None),
        Kind::Hardlink(t) => ("hardlink", Some(t.clone()), None),
    };
    let mode = if matches!(n.kind, Kind::Symlink(_)) { 0o777 } else { n.mode };
    Seen { kind: kind.into(), mode, mtime: n.mtime, link, sha256: sha }
}

/// Compara o que foi visto com a árvore esperada. `check_symlink_mtime` e `check_dir_mtime` permitem
/// pular o mtime onde o consumidor não o restaura (o unzip não acerta o horário de symlink).
pub fn compare(
    expected: &[Node],
    seen: &BTreeMap<String, Seen>,
    check_symlink_mtime: bool,
    check_dir_mtime: bool,
) -> ArchiveCheck {
    let mut c = ArchiveCheck { expected: expected.len(), ..ArchiveCheck::default() };
    for n in expected {
        let want = expected_seen(n);
        let Some(got) = seen.get(&n.path) else {
            c.problems.push(format!("faltou {}", n.path));
            continue;
        };
        let mut diffs = Vec::new();
        if got.kind != want.kind {
            diffs.push(format!("tipo {} != {}", got.kind, want.kind));
        }
        if want.kind != "symlink" && got.mode != want.mode {
            diffs.push(format!("modo {:o} != {:o}", got.mode, want.mode));
        }
        let mtime_matters = match want.kind.as_str() {
            "symlink" => check_symlink_mtime,
            "dir" => check_dir_mtime,
            "hardlink" => false,
            _ => true,
        };
        if mtime_matters && got.mtime != want.mtime {
            diffs.push(format!("mtime {} != {}", got.mtime, want.mtime));
        }
        if got.link != want.link {
            diffs.push(format!("alvo {:?} != {:?}", got.link, want.link));
        }
        if want.sha256.is_some() && got.sha256 != want.sha256 {
            diffs.push("conteúdo diferente".into());
        }
        if diffs.is_empty() {
            c.ok += 1;
        } else {
            c.problems.push(format!("{}: {}", n.path, diffs.join(", ")));
        }
    }
    for path in seen.keys() {
        if !expected.iter().any(|n| &n.path == path) {
            c.problems.push(format!("sobrou {path}"));
        }
    }
    c
}

// --- tar pela crate `tar` ---

/// Como montar o arquivo com a crate.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum TarMode {
    /// Cabeçalho ustar, só a parte portável.
    Ustar,
    /// Cabeçalho ustar com os nomes longos: mostra o que a crate faz quando o nome não cabe.
    UstarLong,
    /// Cabeçalho gnu (nomes longos via ././@LongLink, que a crate gera sozinha).
    Gnu,
    /// Cabeçalho ustar + registros pax `path`/`linkpath` montados por nós com `append_pax_extensions`.
    Pax,
}

impl TarMode {
    pub const ALL: [TarMode; 4] = [TarMode::Ustar, TarMode::UstarLong, TarMode::Gnu, TarMode::Pax];

    pub fn name(self) -> &'static str {
        match self {
            TarMode::Ustar => "ustar",
            TarMode::UstarLong => "ustar-long",
            TarMode::Gnu => "gnu",
            TarMode::Pax => "pax",
        }
    }

    pub fn nodes(self) -> Vec<Node> {
        match self {
            TarMode::Ustar => subset("ustar"),
            _ => tree(),
        }
    }
}

fn base_header(mode: TarMode, n: &Node) -> Result<tar::Header> {
    let mut h = if mode == TarMode::Gnu { tar::Header::new_gnu() } else { tar::Header::new_ustar() };
    h.set_mode(n.mode);
    h.set_mtime(n.mtime);
    h.set_uid(0);
    h.set_gid(0);
    h.set_username("root")?;
    h.set_groupname("root")?;
    Ok(h)
}

/// Monta um tar com a crate `tar` a partir da árvore.
pub fn build_tar(mode: TarMode) -> Result<Vec<u8>> {
    let mut b = tar::Builder::new(Vec::new());
    for n in mode.nodes() {
        let mut h = base_header(mode, &n)?;
        let path = if n.kind == Kind::Dir { format!("{}/", n.path) } else { n.path.clone() };
        let (ty, data, link): (tar::EntryType, &[u8], Option<&str>) = match &n.kind {
            Kind::File(d) => (tar::EntryType::Regular, d, None),
            Kind::Dir => (tar::EntryType::Directory, b"", None),
            Kind::Symlink(t) => (tar::EntryType::Symlink, b"", Some(t)),
            Kind::Hardlink(t) => (tar::EntryType::Link, b"", Some(t)),
        };
        h.set_entry_type(ty);
        h.set_size(data.len() as u64);
        if mode == TarMode::Pax {
            append_pax_entry(&mut b, &mut h, &path, link, data)?;
            continue;
        }
        match link {
            Some(t) => b.append_link(&mut h, &path, t).with_context(|| format!("append_link {path}"))?,
            None => b.append_data(&mut h, &path, data).with_context(|| format!("append_data {path}"))?,
        }
    }
    Ok(b.into_inner()?)
}

/// pax "à mão" sobre a crate: o que não cabe no cabeçalho ustar vai num registro `x` antes da entrada.
fn append_pax_entry(
    b: &mut tar::Builder<Vec<u8>>,
    h: &mut tar::Header,
    path: &str,
    link: Option<&str>,
    data: &[u8],
) -> Result<()> {
    let mut records: Vec<(&str, &[u8])> = Vec::new();
    if h.set_path(path).is_err() {
        records.push(("path", path.as_bytes()));
        let last = path.trim_end_matches('/').rsplit('/').next().unwrap_or("x");
        let short: String = last.chars().take(90).collect();
        h.set_path(if path.ends_with('/') { format!("{short}/") } else { short })?;
    }
    if let Some(t) = link
        && h.set_link_name(t).is_err()
    {
        records.push(("linkpath", t.as_bytes()));
        h.set_link_name("pax-linkpath")?;
    }
    if !records.is_empty() {
        b.append_pax_extensions(records)?;
    }
    h.set_cksum();
    b.append(h, data)?;
    Ok(())
}

/// Lê um tar com a crate `tar` e devolve o que viu por caminho (sem barra final).
pub fn read_tar(data: &[u8]) -> Result<BTreeMap<String, Seen>> {
    let mut ar = tar::Archive::new(data);
    let mut seen = BTreeMap::new();
    for entry in ar.entries()? {
        let mut e = entry?;
        let raw_path = String::from_utf8_lossy(&e.path_bytes()).into_owned();
        let h = e.header().clone();
        let ty = h.entry_type();
        let link = e.link_name_bytes().map(|l| String::from_utf8_lossy(&l).into_owned());
        let mut content = Vec::new();
        e.read_to_end(&mut content)?;
        // v7 e tar antigos marcam diretório só pela barra final.
        let is_dir = ty.is_dir() || ((ty.is_file()) && raw_path.ends_with('/') && content.is_empty());
        let kind = if is_dir {
            "dir"
        } else if ty.is_symlink() {
            "symlink"
        } else if ty.is_hard_link() {
            "hardlink"
        } else if ty.is_file() {
            "file"
        } else {
            "other"
        };
        let path = raw_path.trim_end_matches('/').to_string();
        let mode = h.mode().unwrap_or(0) & 0o7777;
        let sha = (kind == "file").then(|| harness::memtree::sha256_hex(&content));
        let link = if kind == "symlink" || kind == "hardlink" { link } else { None };
        seen.insert(path, Seen { kind: kind.into(), mode, mtime: h.mtime().unwrap_or(0), link, sha256: sha });
    }
    Ok(seen)
}

/// A crate escreveu extensão GNU (././@LongLink) no arquivo?
pub fn has_gnu_longlink(data: &[u8]) -> bool {
    data.chunks(512).any(|block| block.starts_with(b"././@LongLink"))
}

// --- zip pela crate `zip` ---

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ZipMethod {
    Stored,
    Deflate,
    Bzip2,
    Xz,
}

impl ZipMethod {
    pub const ALL: [ZipMethod; 4] = [ZipMethod::Stored, ZipMethod::Deflate, ZipMethod::Bzip2, ZipMethod::Xz];

    pub fn name(self) -> &'static str {
        match self {
            ZipMethod::Stored => "stored",
            ZipMethod::Deflate => "deflate",
            ZipMethod::Bzip2 => "bzip2",
            ZipMethod::Xz => "xz",
        }
    }

    fn method(self) -> zip::CompressionMethod {
        match self {
            ZipMethod::Stored => zip::CompressionMethod::Stored,
            ZipMethod::Deflate => zip::CompressionMethod::Deflated,
            ZipMethod::Bzip2 => zip::CompressionMethod::Bzip2,
            ZipMethod::Xz => zip::CompressionMethod::Xz,
        }
    }
}

/// Converte segundos Unix (UTC) em data civil (algoritmo de Howard Hinnant).
pub fn civil(secs: u64) -> (i64, u32, u32, u32, u32, u32) {
    let days = (secs / 86_400) as i64;
    let rem = secs % 86_400;
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = yoe + era * 400 + i64::from(m <= 2);
    (y, m, d, (rem / 3600) as u32, (rem / 60 % 60) as u32, (rem % 60) as u32)
}

/// Inverso de [`civil`].
pub fn epoch(y: i64, m: u32, d: u32, hh: u32, mm: u32, ss: u32) -> u64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let m = m as i64;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d as i64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    (days * 86_400 + (hh * 3600 + mm * 60 + ss) as i64) as u64
}

fn dos_time(secs: u64) -> Result<zip::DateTime> {
    let (y, mo, d, h, mi, s) = civil(secs);
    zip::DateTime::from_date_and_time(y as u16, mo as u8, d as u8, h as u8, mi as u8, s as u8)
        .map_err(|e| anyhow::anyhow!("data fora do intervalo DOS: {e}"))
}

/// Monta um zip com a crate `zip` (subconjunto `zip` da árvore).
pub fn build_zip(method: ZipMethod) -> Result<Vec<u8>> {
    let mut w = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for n in subset("zip") {
        let opts = zip::write::SimpleFileOptions::default()
            .compression_method(method.method())
            .unix_permissions(n.mode)
            .last_modified_time(dos_time(n.mtime)?);
        match &n.kind {
            Kind::File(d) => {
                w.start_file(n.path.as_str(), opts)?;
                w.write_all(d)?;
            }
            Kind::Dir => w.add_directory(format!("{}/", n.path), opts)?,
            Kind::Symlink(t) => w.add_symlink(n.path.as_str(), t.as_str(), opts)?,
            Kind::Hardlink(_) => bail!("zip não tem hardlink"),
        }
    }
    Ok(w.finish()?.into_inner())
}

/// Lê um zip com a crate `zip`. mtime: o do campo estendido UT quando existe, senão o horário DOS (UTC).
pub fn read_zip(data: &[u8]) -> Result<BTreeMap<String, Seen>> {
    let mut ar = zip::ZipArchive::new(Cursor::new(data))?;
    let mut seen = BTreeMap::new();
    for i in 0..ar.len() {
        let mut f = ar.by_index(i)?;
        let name = f.name().trim_end_matches('/').to_string();
        let mode = f.unix_mode().unwrap_or(0) & 0o7777;
        let ut = f.extra_data_fields().find_map(|x| match x {
            zip::extra_fields::ExtraField::ExtendedTimestamp(t) => t.mod_time(),
            _ => None,
        });
        let mtime = match (ut, f.last_modified()) {
            (Some(t), _) => u64::from(t),
            (None, Some(dt)) => {
                epoch(dt.year() as i64, dt.month() as u32, dt.day() as u32, dt.hour() as u32, dt.minute() as u32, dt.second() as u32)
            }
            (None, None) => 0,
        };
        let is_dir = f.is_dir();
        let is_link = f.is_symlink();
        let mut content = Vec::new();
        f.read_to_end(&mut content)?;
        let (kind, link, sha) = if is_dir {
            ("dir", None, None)
        } else if is_link {
            ("symlink", Some(String::from_utf8_lossy(&content).into_owned()), None)
        } else {
            ("file", None, Some(harness::memtree::sha256_hex(&content)))
        };
        let mode = if kind == "symlink" { 0o777 } else { mode };
        seen.insert(name, Seen { kind: kind.into(), mode, mtime, link, sha256: sha });
    }
    Ok(seen)
}

// --- lado GNU: script que extrai e retrata, e o parser da saída ---

/// Script que, pra cada arquivo nosso em `ours/<dir>/`, lista, testa, extrai e imprime o retrato.
pub fn gnu_consume_archives_script() -> String {
    r#"
for a in ours/tar/*.tar; do
  n=$(basename "$a" .tar)
  tar tvf "$a" >/dev/null 2>err.txt; echo "TARLIST $n $? $(tr '\n' ' ' <err.txt | head -c 300)"
  mkdir -p "xt-$n"
  tar xf "$a" -C "xt-$n" 2>err.txt; echo "TARX $n $? $(tr '\n' ' ' <err.txt | head -c 300)"
  (cd "xt-$n" && find . -mindepth 1 -printf "ENT $n|%y|%m|%T@|%i|%n|%P|%l\n" && find . -type f -exec sha256sum {} + | sed "s|^\([0-9a-f]*\)  \./|SHA $n\|\1\||")
  rm -rf "xt-$n"
done
for z in ours/zip/*.zip; do
  n=$(basename "$z" .zip)
  unzip -t -q "$z" >err.txt 2>&1; echo "UNZIPT $n $? $(tr '\n' ' ' <err.txt | head -c 300)"
  mkdir -p "xz-$n"
  unzip -q "$z" -d "xz-$n" >err.txt 2>&1; echo "UNZIPX $n $? $(tr '\n' ' ' <err.txt | head -c 300)"
  (cd "xz-$n" && find . -mindepth 1 -printf "ENT zip-$n|%y|%m|%T@|%i|%n|%P|%l\n" && find . -type f -exec sha256sum {} + | sed "s|^\([0-9a-f]*\)  \./|SHA zip-$n\|\1\||")
  rm -rf "xz-$n"
done
rm -f err.txt
"#
    .to_string()
}

/// Retrato que o GNU extraiu, por arquivo: caminho -> (Seen, inode).
pub fn parse_gnu_snapshot(stdout: &str) -> BTreeMap<String, BTreeMap<String, (Seen, u64)>> {
    let mut out: BTreeMap<String, BTreeMap<String, (Seen, u64)>> = BTreeMap::new();
    let mut shas: BTreeMap<(String, String), String> = BTreeMap::new();
    for line in stdout.lines() {
        if let Some(rest) = line.strip_prefix("ENT ") {
            let f: Vec<&str> = rest.splitn(8, '|').collect();
            if f.len() < 8 {
                continue;
            }
            let kind = match f[1] {
                "f" => "file",
                "d" => "dir",
                "l" => "symlink",
                _ => "other",
            };
            let mode = u32::from_str_radix(f[2], 8).unwrap_or(0);
            let mtime = f[3].split('.').next().and_then(|s| s.parse().ok()).unwrap_or(0);
            let ino = f[4].parse().unwrap_or(0);
            let link = (kind == "symlink").then(|| f[7].to_string());
            let mode = if kind == "symlink" { 0o777 } else { mode };
            out.entry(f[0].to_string())
                .or_default()
                .insert(f[6].to_string(), (Seen { kind: kind.into(), mode, mtime, link, sha256: None }, ino));
        } else if let Some(rest) = line.strip_prefix("SHA ") {
            let f: Vec<&str> = rest.splitn(3, '|').collect();
            if f.len() == 3 {
                shas.insert((f[0].to_string(), f[2].to_string()), f[1].to_string());
            }
        }
    }
    for ((archive, path), sha) in shas {
        if let Some((seen, _)) = out.get_mut(&archive).and_then(|m| m.get_mut(&path)) {
            seen.sha256 = Some(sha);
        }
    }
    out
}

/// Converte o retrato do GNU pra comparação: hardlink vira "hardlink" quando o inode bate com o alvo.
pub fn snapshot_to_seen(expected: &[Node], snap: &BTreeMap<String, (Seen, u64)>) -> BTreeMap<String, Seen> {
    let mut seen: BTreeMap<String, Seen> = snap.iter().map(|(k, (s, _))| (k.clone(), s.clone())).collect();
    for n in expected {
        if let Kind::Hardlink(target) = &n.kind
            && let (Some((_, a)), Some((_, b))) = (snap.get(&n.path), snap.get(target))
            && a == b
            && let Some(s) = seen.get_mut(&n.path)
        {
            s.kind = "hardlink".into();
            s.link = Some(target.clone());
            s.sha256 = None;
        }
    }
    seen
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn civil_and_epoch_roundtrip() {
        assert_eq!(civil(M), (2026, 1, 15, 12, 0, 0));
        assert_eq!(civil(M_EXEC), (2025, 12, 31, 23, 59, 58));
        assert_eq!(civil(M_OLD), (2001, 2, 3, 4, 5, 6));
        for t in [0, M, M_EXEC, M_OLD, 4_102_444_800] {
            let (y, m, d, h, mi, s) = civil(t);
            assert_eq!(epoch(y, m, d, h, mi, s), t);
        }
    }

    #[test]
    fn tar_crate_roundtrip_matches_tree() {
        for mode in [TarMode::Ustar, TarMode::Gnu, TarMode::Pax] {
            let data = build_tar(mode).unwrap();
            let seen = read_tar(&data).unwrap();
            let check = compare(&mode.nodes(), &seen, true, true);
            assert!(check.passed(), "{}: {:?}", mode.name(), check.problems);
        }
    }

    #[test]
    fn pax_mode_avoids_gnu_extensions() {
        assert!(!has_gnu_longlink(&build_tar(TarMode::Pax).unwrap()));
        assert!(has_gnu_longlink(&build_tar(TarMode::Gnu).unwrap()));
    }

    #[test]
    fn zip_crate_roundtrip_matches_tree() {
        for m in [ZipMethod::Stored, ZipMethod::Deflate] {
            let data = build_zip(m).unwrap();
            let seen = read_zip(&data).unwrap();
            let check = compare(&subset("zip"), &seen, false, true);
            assert!(check.passed(), "{}: {:?}", m.name(), check.problems);
        }
    }

    #[test]
    fn snapshot_parser_reads_find_output() {
        let out = "ENT gnu|f|644|1768478400.0000000000|10|2|t/a.txt|\nENT gnu|f|644|1768478400.0|10|2|t/hard|\nSHA gnu|abc|t/a.txt\n";
        let snap = parse_gnu_snapshot(out);
        let gnu = &snap["gnu"];
        assert_eq!(gnu["t/a.txt"].0.sha256.as_deref(), Some("abc"));
        let nodes = vec![node("t/hard", Kind::Hardlink("t/a.txt".into()), 0o644, M)];
        let seen = snapshot_to_seen(&nodes, gnu);
        assert_eq!(seen["t/hard"].kind, "hardlink");
    }
}
