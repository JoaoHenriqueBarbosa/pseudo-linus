//! H31: compressão e arquivos (gzip, bzip2, xz/lzma/lzip, zstd, tar, zip) contra as ferramentas GNU.
//!
//! Método:
//! 1. Corpus determinístico gerado aqui ([`corpus`]): vazio, 1 byte, log de 1 MB, registros binários de
//!    256 KiB, 64 KiB aleatórios.
//! 2. "GNU produz, candidato consome": o oráculo comprime o corpus em várias variantes por formato
//!    (níveis, checksums, multi-membro/multi-stream, multi-bloco, filtros) e monta tar (ustar, gnu,
//!    oldgnu, pax, v7) e zip (deflate, stored, bzip2) de uma árvore com modos, symlinks, hardlink, nomes
//!    longos e Unicode; cada candidato decodifica/lê e comparamos byte a byte com a entrada.
//! 3. "Candidato produz, GNU consome": cada codificador comprime o corpus em todos os níveis que oferece;
//!    a crate `tar` monta ustar, gnu e pax e a `zip` monta stored/deflate/bzip2/xz; o oráculo roda `-t`,
//!    descomprime e confere sha256, lista e extrai com `tar` e `unzip` e retrata modos, mtimes, links e
//!    inodes.
//! 4. Taxa e velocidade no nível padrão do GNU: o GNU medido dentro do container (laço com `date +%s%N`,
//!    descontado o custo de criar processo), o candidato medido no processo.
//! 5. depscan de cada crate (C na árvore reprova).
//! 6. Shim de CLI nosso sobre as crates ([`cli`]) pontuado contra o golden de `corpus/cases/archive`.

pub mod archives;
pub mod cli;
pub mod codecs;
pub mod corpus;

use std::collections::BTreeMap;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use harness::case::{FileSpec, FileTable};
use harness::{Case, CandidateResult, Conformance, Fit, HypothesisVerdict, Outcome, Verdict};
use serde::Serialize;
use serde_json::{Value, json};

use crate::common::{self, DepSummary, Part};
use archives::{TarMode, ZipMethod};
use codecs::{Codec, Format};
use corpus::Input;

/// Variante de compressão que o GNU produz.
struct Variant {
    format: Format,
    name: &'static str,
    /// Comando com `{in}`; a saída vai pra stdout. Variantes `multi` concatenam duas vezes.
    cmd: &'static str,
    /// Aplica a todas as entradas (true) ou só a text e binary.
    all_inputs: bool,
    /// Quantas vezes a entrada aparece descomprimida (2 nas concatenações).
    repeat: usize,
}

const fn v(format: Format, name: &'static str, cmd: &'static str, all_inputs: bool) -> Variant {
    Variant { format, name, cmd, all_inputs, repeat: 1 }
}

const fn multi(format: Format, cmd: &'static str) -> Variant {
    Variant { format, name: "multi", cmd, all_inputs: false, repeat: 2 }
}

fn variants() -> Vec<Variant> {
    use Format::*;
    vec![
        v(Gzip, "l1", "gzip -1 -n -c {in}", false),
        v(Gzip, "l6", "gzip -n -c {in}", true),
        v(Gzip, "l9", "gzip -9 -n -c {in}", false),
        v(Gzip, "named", "gzip -c {in}", false),
        v(Gzip, "rsyncable", "gzip --rsyncable -n -c {in}", false),
        multi(Gzip, "gzip -n -c {in}"),
        v(Bzip2, "l1", "bzip2 -1 -c {in}", false),
        v(Bzip2, "l9", "bzip2 -c {in}", true),
        multi(Bzip2, "bzip2 -c {in}"),
        v(Xz, "l0", "xz -0 -c {in}", false),
        v(Xz, "l6", "xz -c {in}", true),
        v(Xz, "l9e", "xz -9e -c {in}", false),
        v(Xz, "crc32", "xz --check=crc32 -c {in}", false),
        v(Xz, "sha256", "xz --check=sha256 -c {in}", false),
        v(Xz, "nocheck", "xz --check=none -c {in}", false),
        v(Xz, "blocks", "xz -T4 --block-size=65536 -c {in}", false),
        v(Xz, "x86", "xz --x86 --lzma2 -c {in}", false),
        v(Xz, "delta", "xz --delta=dist=4 --lzma2 -c {in}", false),
        multi(Xz, "xz -c {in}"),
        v(Lzma, "l0", "xz --format=lzma -0 -c {in}", false),
        v(Lzma, "l6", "xz --format=lzma -c {in}", true),
        v(Lzma, "l9", "xz --format=lzma -9 -c {in}", false),
        v(Lzip, "l0", "lzip -0 -c {in}", false),
        v(Lzip, "l6", "lzip -c {in}", true),
        v(Lzip, "l9", "lzip -9 -c {in}", false),
        v(Lzip, "members", "lzip -b 100KiB -c {in}", false),
        multi(Lzip, "lzip -c {in}"),
        v(Zstd, "l1", "zstd -q -1 -c {in}", false),
        v(Zstd, "l3", "zstd -q -c {in}", true),
        v(Zstd, "l19", "zstd -q -19 -c {in}", false),
        v(Zstd, "nocheck", "zstd -q --no-check -c {in}", false),
        v(Zstd, "stream", "cat {in} | zstd -q -c", false),
        multi(Zstd, "zstd -q -c {in}"),
    ]
}

/// Variante do GNU no nível padrão (base da medição de decodificação).
fn default_variant(f: Format) -> &'static str {
    match f {
        Format::Gzip => "l6",
        Format::Bzip2 => "l9",
        Format::Zstd => "l3",
        _ => "l6",
    }
}

fn spec_b64(data: &[u8]) -> FileSpec {
    FileSpec::Table(FileTable { content_b64: Some(STANDARD.encode(data)), ..FileTable::default() })
}

fn script_case(id: &str, script: String, files: BTreeMap<String, FileSpec>, timeout_ms: u64) -> Case {
    Case {
        id: id.to_string(),
        argv: Vec::new(),
        script: Some(script),
        stdin: None,
        stdin_b64: None,
        files,
        env: Default::default(),
        tags: Vec::new(),
        faketime: None,
        timeout_ms: Some(timeout_ms),
    }
}

// --- passo 2: GNU produz ---

fn produce_case(inputs: &[Input]) -> Case {
    let mut files = archives::tree_fixture("src");
    for i in inputs {
        files.insert(format!("in/{}", i.name), spec_b64(&i.data));
    }
    let mut s = String::from("set -u\n");
    s.push_str(&archives::tree_finish_script("src"));
    for f in Format::ALL {
        s.push_str(&format!("mkdir -p out/c/{}\n", f.label()));
    }
    for var in variants() {
        for i in inputs {
            if !var.all_inputs && !matches!(i.name, "text" | "binary") {
                continue;
            }
            let out = format!("out/c/{}/{}__{}.{}", var.format.label(), var.name, i.name, var.format.ext());
            let cmd = var.cmd.replace("{in}", &format!("in/{}", i.name));
            if var.repeat == 2 {
                s.push_str(&format!("{{ {cmd}; {cmd}; }} > {out}\n"));
            } else {
                s.push_str(&format!("{cmd} > {out}\n"));
            }
        }
    }
    s.push_str(&archives::gnu_archive_script());
    s.push_str("rm -rf in src\n");
    script_case("h31-produce", s, files, 300_000)
}

/// Medição do GNU no nível padrão: comprime e descomprime text e binary, desconta o custo de processo.
fn timing_case(inputs: &[Input]) -> Case {
    let mut files = BTreeMap::new();
    for i in inputs.iter().filter(|i| matches!(i.name, "text" | "binary")) {
        files.insert(format!("in/{}", i.name), spec_b64(&i.data));
    }
    let mut s = String::from(
        "t() { local label=$1; shift; local n=0 s e; s=$(date +%s%N); while :; do \"$@\" >/dev/null 2>&1; n=$((n+1)); \
         e=$(date +%s%N); if [ $n -ge 3 ] && [ $((e-s)) -ge 400000000 ]; then break; fi; done; echo \"TIME $label $n $((e-s))\"; }\n\
         t noop cat /dev/null\n",
    );
    for (name, enc, dec, ext) in gnu_tools() {
        for input in ["text", "binary"] {
            s.push_str(&format!("{enc} in/{input} > c.{ext}\n"));
            s.push_str(&format!("echo \"SIZE {name} {input} $(stat -c %s c.{ext})\"\n"));
            s.push_str(&format!("t enc-{name}-{input} {enc} in/{input}\n"));
            s.push_str(&format!("t dec-{name}-{input} {dec} c.{ext}\n"));
            s.push_str(&format!("rm -f c.{ext}\n"));
        }
    }
    s.push_str("rm -rf in\n");
    script_case("h31-timing", s, files, 300_000)
}

/// (nome, comprime, descomprime, extensão) no nível padrão de cada ferramenta GNU; zstd -1 entra pra
/// comparar com o ruzstd, que só tem o nível mais rápido.
fn gnu_tools() -> Vec<(&'static str, &'static str, &'static str, &'static str)> {
    vec![
        ("gzip", "gzip -n -c", "gzip -dc", "gz"),
        ("bzip2", "bzip2 -c", "bzip2 -dc", "bz2"),
        ("xz", "xz -c", "xz -dc", "xz"),
        ("lzma", "xz --format=lzma -c", "xz --format=lzma -dc", "lzma"),
        ("lzip", "lzip -c", "lzip -dc", "lz"),
        ("zstd", "zstd -q -c", "zstd -q -dc", "zst"),
        ("zstd1", "zstd -q -1 -c", "zstd -q -dc", "zst"),
    ]
}

#[derive(Clone, Debug, Default, Serialize)]
struct GnuTiming {
    size: BTreeMap<String, u64>,
    /// MB/s por rótulo (enc-gzip-text...), já sem o custo de criar processo.
    mbps: BTreeMap<String, f64>,
    spawn_ms: f64,
}

fn parse_timing(stdout: &str, inputs: &[Input]) -> GnuTiming {
    let len = |input: &str| inputs.iter().find(|i| i.name == input).map(|i| i.data.len()).unwrap_or(0);
    let mut t = GnuTiming::default();
    let mut raw: Vec<(String, u64, u64)> = Vec::new();
    for line in stdout.lines() {
        let f: Vec<&str> = line.split_whitespace().collect();
        match f.as_slice() {
            ["SIZE", name, input, size] => {
                t.size.insert(format!("{name}-{input}"), size.parse().unwrap_or(0));
            }
            ["TIME", label, n, ns] => raw.push((label.to_string(), n.parse().unwrap_or(1), ns.parse().unwrap_or(0))),
            _ => {}
        }
    }
    let spawn = raw.iter().find(|r| r.0 == "noop").map(|r| r.2 as f64 / r.1 as f64).unwrap_or(0.0);
    t.spawn_ms = spawn / 1e6;
    for (label, n, ns) in raw.iter().filter(|r| r.0 != "noop") {
        let per = (*ns as f64 / *n as f64 - spawn).max(1.0);
        let input = label.rsplit('-').next().unwrap_or("");
        t.mbps.insert(label.clone(), round2(len(input) as f64 / 1e6 / (per / 1e9)));
    }
    t
}

// --- passo 3: candidato produz ---

/// Arquivo nosso enviado ao oráculo: caminho dentro de `ours/`, formato e entrada esperada.
struct OursFile {
    codec: &'static str,
    path: String,
    expected_sha: String,
}

fn consume_case(ours: &[(String, Vec<u8>)]) -> Case {
    let mut files = BTreeMap::new();
    for (path, data) in ours {
        files.insert(path.clone(), spec_b64(data));
    }
    let mut s = String::from("set -u\n");
    s.push_str("find ours/c -type f | sort | while read -r f; do\n  case \"$f\" in\n");
    for f in Format::ALL {
        s.push_str(&format!("    *.{}) d=\"{}\"; t=\"{}\";;\n", f.ext(), f.gnu_decode(), f.gnu_test()));
    }
    s.push_str(
        "  esac\n  $t \"$f\" >/dev/null 2>err.txt; te=$?\n  $d \"$f\" >dec.bin 2>>err.txt; de=$?\n  \
         h=$(sha256sum <dec.bin | cut -c1-64)\n  echo \"ENC $f $te $de $h $(tr '\\n' ' ' <err.txt | head -c 200)\"\n\
         done\nrm -f dec.bin err.txt\n",
    );
    s.push_str(&archives::gnu_consume_archives_script());
    s.push_str("rm -rf ours\n");
    script_case("h31-consume", s, files, 300_000)
}

#[derive(Clone, Debug, Default, Serialize)]
struct Tally {
    total: usize,
    pass: usize,
    failures: Vec<String>,
}

impl Tally {
    fn add(&mut self, ok: bool, what: impl FnOnce() -> String) {
        self.total += 1;
        if ok {
            self.pass += 1;
        } else if self.failures.len() < 40 {
            self.failures.push(what());
        }
    }

    fn all_pass(&self) -> bool {
        self.total > 0 && self.pass == self.total
    }

    fn rate(&self) -> String {
        format!("{}/{}", self.pass, self.total)
    }
}

fn round2(x: f64) -> f64 {
    (x * 100.0).round() / 100.0
}

fn round4(x: f64) -> f64 {
    (x * 10_000.0).round() / 10_000.0
}

pub fn run() -> Result<Part> {
    let inputs = corpus::inputs();
    let codecs = codecs::all();
    let oracle = harness::Oracle::locate()?;
    let mut notes = Vec::new();

    // Passo 2 (e a medição do GNU) num container só.
    let outs = oracle.run(&[produce_case(&inputs), timing_case(&inputs)]).context("oráculo: produce/timing")?;
    let (produced, timing_out) = (&outs[0], &outs[1]);
    check_outcome("h31-produce", produced)?;
    check_outcome("h31-timing", timing_out)?;
    let gnu_timing = parse_timing(&String::from_utf8_lossy(timing_out.stdout.as_slice()), &inputs);

    // Decodificação dos arquivos GNU por cada codec.
    let mut decode: BTreeMap<&'static str, Tally> = BTreeMap::new();
    for var in variants() {
        for i in &inputs {
            if !var.all_inputs && !matches!(i.name, "text" | "binary") {
                continue;
            }
            let path = format!("out/c/{}/{}__{}.{}", var.format.label(), var.name, i.name, var.format.ext());
            let Some(data) = produced.files.read(&path) else {
                bail!("oráculo não devolveu {path}");
            };
            let want: Vec<u8> = i.data.repeat(var.repeat);
            for codec in codecs.iter().filter(|c| c.format() == var.format) {
                let got = codecs::decode_guarded(codec.as_ref(), data);
                let label = format!("{}/{}__{}", var.format.label(), var.name, i.name);
                decode.entry(codec.id()).or_default().add(matches!(&got, Ok(d) if *d == want), || match &got {
                    Ok(d) => format!("{label}: saída de {} bytes, esperado {}", d.len(), want.len()),
                    Err(e) => format!("{label}: {}: {}", e.kind(), e.message()),
                });
            }
        }
    }

    // Leitura dos tar e zip do GNU pelas crates.
    let mut gnu_tar_read: BTreeMap<String, archives::ArchiveCheck> = BTreeMap::new();
    for variant in ["ustar", "gnu", "oldgnu", "pax", "v7"] {
        let path = format!("out/tar/{variant}.tar");
        let data = produced.files.read(&path).with_context(|| format!("oráculo não devolveu {path}"))?;
        let check = match archives::read_tar(data) {
            Ok(seen) => archives::compare(&archives::subset(variant), &seen, true, true),
            Err(e) => archives::ArchiveCheck { problems: vec![format!("erro: {e:#}")], ..Default::default() },
        };
        gnu_tar_read.insert(variant.to_string(), check);
    }
    let mut gnu_zip_read: BTreeMap<String, archives::ArchiveCheck> = BTreeMap::new();
    for m in ["deflate", "stored", "bzip2"] {
        let path = format!("out/zip/{m}.zip");
        let data = produced.files.read(&path).with_context(|| format!("oráculo não devolveu {path}"))?;
        let check = match archives::read_zip(data) {
            Ok(seen) => archives::compare(&archives::subset("zip"), &seen, true, true),
            Err(e) => archives::ArchiveCheck { problems: vec![format!("erro: {e:#}")], ..Default::default() },
        };
        gnu_zip_read.insert(m.to_string(), check);
    }

    // Passo 3: o que os candidatos produzem.
    let mut ours: Vec<(String, Vec<u8>)> = Vec::new();
    let mut ours_index: Vec<OursFile> = Vec::new();
    let mut encode: BTreeMap<&'static str, Tally> = BTreeMap::new();
    for codec in &codecs {
        for &level in codec.levels() {
            for i in &inputs {
                let path = format!("ours/c/{}/l{level}__{}.{}", codec.id(), i.name, codec.format().ext());
                match codecs::encode_guarded(codec.as_ref(), &i.data, level) {
                    Ok(enc) if enc.len() as u64 <= harness::memtree::CAPTURE_LIMIT => {
                        ours_index.push(OursFile {
                            codec: codec.id(),
                            path: path.clone(),
                            expected_sha: harness::memtree::sha256_hex(&i.data),
                        });
                        ours.push((path, enc));
                    }
                    Ok(enc) => {
                        let tally = encode.entry(codec.id()).or_default();
                        tally.add(false, || format!("{path}: saída de {} bytes passa do limite de captura", enc.len()));
                    }
                    Err(e) => {
                        let tally = encode.entry(codec.id()).or_default();
                        tally.add(false, || format!("{path}: encode falhou: {e:#}"));
                    }
                }
            }
        }
    }
    let mut ours_tar: BTreeMap<TarMode, Vec<u8>> = BTreeMap::new();
    for mode in TarMode::ALL {
        let data = archives::build_tar(mode)?;
        ours.push((format!("ours/tar/{}.tar", mode.name()), data.clone()));
        ours_tar.insert(mode, data);
    }
    let mut zip_build_errors: BTreeMap<String, String> = BTreeMap::new();
    for m in ZipMethod::ALL {
        match archives::build_zip(m) {
            Ok(data) => ours.push((format!("ours/zip/{}.zip", m.name()), data)),
            Err(e) => {
                zip_build_errors.insert(m.name().to_string(), format!("{e:#}"));
            }
        }
    }
    let consumed = oracle.run(&[consume_case(&ours)]).context("oráculo: consume")?.remove(0);
    check_outcome("h31-consume", &consumed)?;
    let consume_out = String::from_utf8_lossy(consumed.stdout.as_slice()).into_owned();
    let mut enc_lines: BTreeMap<String, (i32, i32, String, String)> = BTreeMap::new();
    for line in consume_out.lines() {
        if let Some(rest) = line.strip_prefix("ENC ") {
            let f: Vec<&str> = rest.splitn(5, ' ').collect();
            if f.len() >= 4 {
                enc_lines.insert(
                    f[0].to_string(),
                    (f[1].parse().unwrap_or(-1), f[2].parse().unwrap_or(-1), f[3].to_string(), f.get(4).unwrap_or(&"").to_string()),
                );
            }
        }
    }
    for of in &ours_index {
        let r = enc_lines.get(&of.path);
        let ok = matches!(r, Some((0, 0, sha, _)) if *sha == of.expected_sha);
        encode.entry(of.codec).or_default().add(ok, || match r {
            Some((te, de, _, err)) => format!("{}: -t={te} -d={de} {}", of.path, err.trim()),
            None => format!("{}: sem resultado do oráculo", of.path),
        });
    }

    // GNU lendo os nossos tar e zip.
    let snaps = archives::parse_gnu_snapshot(&consume_out);
    let status = |prefix: &str, name: &str| -> Option<(i32, String)> {
        consume_out.lines().find_map(|l| {
            let rest = l.strip_prefix(&format!("{prefix} {name} "))?;
            let (code, msg) = rest.split_once(' ').unwrap_or((rest, ""));
            Some((code.parse().unwrap_or(-1), msg.trim().to_string()))
        })
    };
    let mut gnu_reads_ours_tar: BTreeMap<String, Value> = BTreeMap::new();
    let mut tar_write_ok = true;
    for mode in TarMode::ALL {
        let name = mode.name();
        let expected = mode.nodes();
        let snap = snaps.get(name).cloned().unwrap_or_default();
        let seen = archives::snapshot_to_seen(&expected, &snap);
        let check = archives::compare(&expected, &seen, true, true);
        let list = status("TARLIST", name);
        let extract = status("TARX", name);
        let clean = matches!(&list, Some((0, m)) if m.is_empty()) && matches!(&extract, Some((0, m)) if m.is_empty());
        let gnu_ext = archives::has_gnu_longlink(&ours_tar[&mode]);
        if mode != TarMode::UstarLong && !(clean && check.passed()) {
            tar_write_ok = false;
        }
        gnu_reads_ours_tar.insert(
            name.to_string(),
            json!({
                "tar_tvf": list, "tar_xf": extract, "entries_ok": check.ok, "entries": check.expected,
                "problems": check.problems, "uses_gnu_longlink": gnu_ext,
            }),
        );
    }
    let mut unzip_reads_ours: BTreeMap<String, Value> = BTreeMap::new();
    let mut zip_write_ok = true;
    for m in ZipMethod::ALL {
        let name = m.name();
        if let Some(err) = zip_build_errors.get(name) {
            unzip_reads_ours.insert(name.to_string(), json!({ "build_error": err }));
            zip_write_ok = false;
            continue;
        }
        let expected = archives::subset("zip");
        let snap = snaps.get(&format!("zip-{name}")).cloned().unwrap_or_default();
        let seen = archives::snapshot_to_seen(&expected, &snap);
        let check = archives::compare(&expected, &seen, false, true);
        let test = status("UNZIPT", name);
        let extract = status("UNZIPX", name);
        let ok = matches!(test, Some((0, _))) && matches!(extract, Some((0, _))) && check.passed();
        // unzip 6.0 não implementa o método 95 (xz): falha esperada do consumidor, não da crate.
        if m != ZipMethod::Xz && !ok {
            zip_write_ok = false;
        }
        unzip_reads_ours.insert(
            name.to_string(),
            json!({ "unzip_t": test, "unzip_x": extract, "entries_ok": check.ok, "entries": check.expected, "problems": check.problems }),
        );
    }

    // Passo 4: taxa e velocidade no processo, no nível padrão do GNU.
    let mut bench: BTreeMap<&'static str, Value> = BTreeMap::new();
    for codec in &codecs {
        let mut row = serde_json::Map::new();
        for input in ["text", "binary"] {
            let Some(i) = inputs.iter().find(|i| i.name == input) else { continue };
            if let Some(level) = codec.bench_level() {
                let enc = codecs::encode_guarded(codec.as_ref(), &i.data, level)?;
                let per = common::time_per_iter(Duration::from_millis(300), || {
                    let _ = codec.encode(&i.data, level);
                });
                row.insert(format!("ratio-{input}"), json!(round4(enc.len() as f64 / i.data.len() as f64)));
                row.insert(format!("enc-mbps-{input}"), json!(round2(common::mb_per_s(i.data.len(), per))));
            }
            let gnu_file = format!(
                "out/c/{}/{}__{input}.{}",
                codec.format().label(),
                default_variant(codec.format()),
                codec.format().ext()
            );
            if let Some(data) = produced.files.read(&gnu_file)
                && codecs::decode_guarded(codec.as_ref(), data).is_ok()
            {
                let per = common::time_per_iter(Duration::from_millis(300), || {
                    let _ = codec.decode(data);
                });
                row.insert(format!("dec-mbps-{input}"), json!(round2(common::mb_per_s(i.data.len(), per))));
            }
        }
        bench.insert(codec.id(), Value::Object(row));
    }
    // Saída byte a byte igual à do GNU no nível padrão (informativo: não é requisito de interoperabilidade).
    let mut identical: BTreeMap<&'static str, BTreeMap<&'static str, bool>> = BTreeMap::new();
    for codec in codecs.iter().filter(|c| c.bench_level() == Some(c.format().gnu_default_level())) {
        for input in ["text", "binary"] {
            let Some(i) = inputs.iter().find(|i| i.name == input) else { continue };
            let gnu_file = format!(
                "out/c/{}/{}__{input}.{}",
                codec.format().label(),
                default_variant(codec.format()),
                codec.format().ext()
            );
            let (Some(gnu), Ok(mut ours)) =
                (produced.files.read(&gnu_file), codecs::encode_guarded(codec.as_ref(), &i.data, codec.format().gnu_default_level()))
            else {
                continue;
            };
            if codec.format() == Format::Gzip && ours.len() > 9 {
                ours[9] = 3; // SO do cabeçalho é escolha do CLI
            }
            identical.entry(codec.id()).or_default().insert(input, ours == gnu);
        }
    }
    let gnu_ratio = |name: &str, input: &str| -> Option<f64> {
        let len = inputs.iter().find(|i| i.name == input)?.data.len() as f64;
        gnu_timing.size.get(&format!("{name}-{input}")).map(|s| round4(*s as f64 / len))
    };
    let mut gnu_bench = serde_json::Map::new();
    for (name, _, _, _) in gnu_tools() {
        let mut row = serde_json::Map::new();
        for input in ["text", "binary"] {
            row.insert(format!("ratio-{input}"), json!(gnu_ratio(name, input)));
            row.insert(format!("enc-mbps-{input}"), json!(gnu_timing.mbps.get(&format!("enc-{name}-{input}"))));
            row.insert(format!("dec-mbps-{input}"), json!(gnu_timing.mbps.get(&format!("dec-{name}-{input}"))));
        }
        gnu_bench.insert(name.to_string(), Value::Object(row));
    }

    // Tamanho da saída do candidato em relação à do GNU no nível padrão (texto de 1 MB).
    let gnu_name = |f: Format| match f {
        Format::Gzip => "gzip",
        Format::Bzip2 => "bzip2",
        Format::Xz => "xz",
        Format::Lzma => "lzma",
        Format::Lzip => "lzip",
        Format::Zstd => "zstd",
    };
    let mut size_vs_gnu: BTreeMap<&'static str, f64> = BTreeMap::new();
    for codec in &codecs {
        let ours = bench.get(codec.id()).and_then(|b| b.get("ratio-text")).and_then(Value::as_f64);
        if let (Some(ours), Some(gnu)) = (ours, gnu_ratio(gnu_name(codec.format()), "text")) {
            size_vs_gnu.insert(codec.id(), round2(ours / gnu));
        }
    }

    // Passo 5: depscan.
    let mut scans: BTreeMap<&str, DepSummary> = BTreeMap::new();
    for pkg in [
        "flate2", "miniz_oxide", "zlib-rs", "bzip2", "libbz2-rs-sys", "lzma-rust2", "lzma-rs", "xz4rust", "ruzstd",
        "structured-zstd", "tar", "zip",
    ] {
        scans.insert(pkg, common::dep_scan(pkg)?);
    }

    // Passo 6: shim de CLI contra o golden.
    let cases = common::load_cases("archive")?;
    let shim = ShimScores::run(&cases);

    // Montagem dos candidatos.
    let mut part = Part { key: "h31_archive".into(), ..Part::default() };
    let groups = candidate_groups();
    let mut role_best: BTreeMap<&str, Vec<(String, Fit)>> = BTreeMap::new();
    for g in &groups {
        let dec: Tally = merge(g.codecs.iter().filter_map(|c| decode.get(c)));
        let enc: Tally = merge(g.codecs.iter().filter_map(|c| encode.get(c)));
        let pure = g.packages.iter().all(|p| scans[p].pure_rust());
        let deps: Vec<&DepSummary> = g.packages.iter().map(|p| &scans[p]).collect();
        let category = deps.iter().map(|d| d.tree_category.clone()).max();
        let shim_conf = shim.for_tags(&cases, g.shim_tags, &g.shim_label);
        let worst_size = g.codecs.iter().filter_map(|c| size_vs_gnu.get(c).copied()).fold(None, |a: Option<f64>, x| {
            Some(a.map_or(x, |a| a.max(x)))
        });
        let ctx = FitContext {
            pure,
            tar_write_ok,
            zip_write_ok,
            gnu_tar_read: &gnu_tar_read,
            gnu_zip_read: &gnu_zip_read,
            worst_size_vs_gnu: worst_size,
        };
        let (fit, verdict_note) = decide_fit(g, &dec, &enc, &ctx);
        role_best.entry(g.role).or_default().push((g.name.to_string(), fit));
        let mut metrics = serde_json::Map::new();
        if g.role == "tar" {
            metrics.insert("gnu_archives_read".into(), json!(gnu_tar_read));
            metrics.insert("gnu_reads_ours".into(), json!(gnu_reads_ours_tar));
        } else if g.role == "zip" {
            metrics.insert("gnu_archives_read".into(), json!(gnu_zip_read));
            metrics.insert("unzip_reads_ours".into(), json!(unzip_reads_ours));
        } else {
            metrics.insert(
                "interop".into(),
                json!({
                    "gnu_to_candidate": { "pass": dec.pass, "total": dec.total, "failures": dec.failures },
                    "candidate_to_gnu": { "pass": enc.pass, "total": enc.total, "failures": enc.failures },
                }),
            );
            let b: BTreeMap<&str, &Value> = g.codecs.iter().filter_map(|c| bench.get(c).map(|v| (*c, v))).collect();
            metrics.insert("bench".into(), json!(b));
            let s: BTreeMap<&str, f64> = g.codecs.iter().filter_map(|c| size_vs_gnu.get(c).map(|v| (*c, *v))).collect();
            metrics.insert("output_size_vs_gnu_default_text".into(), json!(s));
        }
        metrics.insert("depscan".into(), json!(deps));
        if let Some(conf) = &shim_conf {
            metrics.insert("cli_shim".into(), json!({ "label": g.shim_label, "strict": conf.strict_pass, "lenient": conf.lenient_pass, "total": conf.total }));
        }
        part.candidates.push(CandidateResult {
            name: g.name.to_string(),
            version: g.version.to_string(),
            role: g.role.to_string(),
            category,
            conformance: shim_conf,
            fit,
            notes: format!("{} {verdict_note}", g.notes),
            metrics: Value::Object(metrics),
        });
    }

    // Veredito H31.
    let roles = ["gzip", "bzip2", "xz", "zstd", "tar", "zip"];
    let mut role_summary = serde_json::Map::new();
    let mut covered = 0;
    for role in roles {
        let list = role_best.get(role).cloned().unwrap_or_default();
        let best: Vec<String> =
            list.iter().filter(|(_, f)| matches!(f, Fit::Fits | Fit::FitsWithWork)).map(|(n, _)| n.clone()).collect();
        if !best.is_empty() {
            covered += 1;
        }
        role_summary.insert(role.to_string(), json!(best));
    }
    let verdict = if covered == roles.len() {
        Verdict::Confirmed
    } else if covered == 0 {
        Verdict::Refuted
    } else {
        Verdict::Partial
    };
    let pick = |id: &str| decode.get(id).map(Tally::rate).unwrap_or_default();
    let pick_enc = |id: &str| encode.get(id).map(Tally::rate).unwrap_or_default();
    let summary = format!(
        "{covered}/6 papéis com crate Rust pura que interopera nos dois sentidos (GNU->crate | crate->GNU): \
         flate2 {} | {}, zlib-rs {} | {}, bzip2 {} | {}, lzma-rust2 xz {} | {}, lzma {} | {}, lzip {} | {}, \
         structured-zstd {} | {}, ruzstd {} | {}; tar: GNU lê os nossos ustar/gnu/pax={tar_write_ok}, crate lê \
         ustar/gnu/oldgnu/pax/v7 do GNU={}; zip: unzip lê os nossos={zip_write_ok}, crate lê os do zip={}. \
         CLIs (gzip -l, tar tv, unzip -l, mensagens) são nossos.",
        pick("flate2-gzip"),
        pick_enc("flate2-gzip"),
        pick("zlib-rs-gzip"),
        pick_enc("zlib-rs-gzip"),
        pick("bzip2-rs"),
        pick_enc("bzip2-rs"),
        pick("lzma-rust2-xz"),
        pick_enc("lzma-rust2-xz"),
        pick("lzma-rust2-lzma"),
        pick_enc("lzma-rust2-lzma"),
        pick("lzma-rust2-lzip"),
        pick_enc("lzma-rust2-lzip"),
        pick("structured-zstd"),
        pick_enc("structured-zstd"),
        pick("ruzstd"),
        pick_enc("ruzstd"),
        gnu_tar_read.values().all(archives::ArchiveCheck::passed),
        gnu_zip_read.values().all(archives::ArchiveCheck::passed),
    );
    part.hypotheses.push(HypothesisVerdict {
        id: "H31".into(),
        verdict,
        summary,
        evidence: json!({
            "roles": role_summary,
            "decode_matrix": decode.iter().map(|(k, t)| (k.to_string(), t.rate())).collect::<BTreeMap<_, _>>(),
            "encode_matrix": encode.iter().map(|(k, t)| (k.to_string(), t.rate())).collect::<BTreeMap<_, _>>(),
            "c_deps": scans.iter().map(|(k, d)| (k.to_string(), d.c_deps.clone())).collect::<BTreeMap<_, _>>(),
        }),
    });

    notes.push(
        "H31: o flate2 não pode ter os backends miniz_oxide e zlib-rs no mesmo grafo (as features unificam); o \
         candidato zlib-rs usa a API segura zlib_rs::{Deflate, Inflate} em modo raw com o enquadramento gzip feito \
         aqui, que é exatamente o que o flate2 faz com a feature zlib-rs."
            .into(),
    );
    notes.push(format!(
        "H31: velocidades do GNU medidas no container em laço com date +%s%N, descontando {:.2} ms de criação de \
         processo por iteração; candidatos medidos no processo. Nível: o padrão do GNU (gzip 6, bzip2 9, xz/lzip 6, zstd 3).",
        gnu_timing.spawn_ms
    ));
    notes.push(
        "H31: depscan sem C em nenhuma das 12 crates (nenhum `links`, nenhuma build-dependency cc/cmake/bindgen). A \
         categoria (b) vem de código fora do caminho que usamos: tar (unpack e append_path em std::fs; a feature \
         xattr é ligada no grafo pela pure-magic, não por nós), zip (extract em std::fs), ruzstd e structured-zstd \
         (std::fs só em src/bin e testes), libbz2-rs-sys (ABI C exportada com tipos da libc) e zlib-rs (eprintln de \
         trace). O depscan também põe o zlib-rs sob o flate2 por causa da dependência fraca `zlib-rs?/std`, que não \
         está ativa (cargo tree -i zlib-rs mostra só o experimento)."
            .into(),
    );
    if archives::has_gnu_longlink(&ours_tar[&TarMode::UstarLong]) {
        notes.push(
            "H31: com Header::new_ustar e nome acima de 255 bytes (ou alvo de link acima de 100), o Builder da crate tar \
             grava ././@LongLink (extensão GNU) sem avisar: o GNU lê, mas o arquivo deixa de ser ustar POSIX. pax \
             exige montar os registros com append_pax_extensions (camada nossa, testada no modo pax)."
                .into(),
        );
    }
    if decode.get("xz4rust-xz").is_some_and(|t| t.failures.iter().any(|f| f.contains("multi") && f.contains("saída de"))) {
        notes.push(
            "H31: o XzReader do xz4rust devolve só o primeiro stream de um .xz concatenado (xz a b > c), sem erro: \
             truncamento silencioso."
                .into(),
        );
    }
    if let Some(v) = unzip_reads_ours.get("xz")
        && v.get("entries_ok") != v.get("entries")
    {
        notes.push(
            "H31: zip com método 95 (xz) escrito pela crate zip é recusado pelo unzip 6.0 do oráculo (\"unsupported \
             compression\", exit 81): limitação do consumidor; o Info-ZIP também não gera esse método."
                .into(),
        );
    }
    part.notes = notes;
    part.metrics = json!({
        "inputs": inputs.iter().map(|i| (i.name, i.data.len())).collect::<BTreeMap<_, _>>(),
        "codec_candidate": codecs.iter().map(|c| (c.id(), c.candidate())).collect::<BTreeMap<_, _>>(),
        "variants": variants().iter().map(|v| format!("{}/{}: {}", v.format.label(), v.name, v.cmd)).collect::<Vec<_>>(),
        "decode": decode,
        "encode": encode,
        "bench_candidates": bench,
        "bench_gnu": gnu_bench,
        "gnu_spawn_ms": round2(gnu_timing.spawn_ms),
        "tar": { "crate_reads_gnu": gnu_tar_read, "gnu_reads_ours": gnu_reads_ours_tar },
        "zip": { "crate_reads_gnu": gnu_zip_read, "unzip_reads_ours": unzip_reads_ours },
        "cli_shim": shim.summary(),
        "compress_bytes_identical_to_gnu": {
            "cli_cases_small_input": compress_bytes_identical(&cases),
            "default_level_corpus": identical,
        },
    });
    Ok(part)
}

fn check_outcome(id: &str, out: &Outcome) -> Result<()> {
    if let Some(why) = &out.unsupported {
        bail!("{id}: {why}");
    }
    if out.timed_out {
        bail!("{id}: timeout no oráculo");
    }
    if out.exit != Some(0) {
        bail!("{id}: exit {:?}: {}", out.exit, out.stderr.preview(400));
    }
    Ok(())
}

fn merge<'a>(it: impl Iterator<Item = &'a Tally>) -> Tally {
    let mut t = Tally::default();
    for x in it {
        t.total += x.total;
        t.pass += x.pass;
        t.failures.extend(x.failures.iter().cloned());
    }
    t
}

/// Um candidato do resultado: uma crate (ou crate + backend) e os codecs que a representam.
struct Group {
    name: &'static str,
    version: &'static str,
    role: &'static str,
    codecs: &'static [&'static str],
    packages: &'static [&'static str],
    shim_tags: &'static [&'static str],
    shim_label: String,
    notes: &'static str,
}

fn candidate_groups() -> Vec<Group> {
    vec![
        Group {
            name: "flate2 (miniz_oxide)",
            version: "flate2 1.1.10 + miniz_oxide 0.8.9",
            role: "gzip",
            codecs: &["flate2-gzip"],
            packages: &["flate2", "miniz_oxide"],
            shim_tags: &["gzip-l", "gzip-d", "gzip-t"],
            shim_label: "flate2".into(),
            notes: "GzEncoder/MultiGzDecoder sobre Read/Write; gzip -l, -t e as mensagens do gzip são nossas (a crate \
                    expõe só o erro de io, que classificamos por ErrorKind e texto).",
        },
        Group {
            name: "zlib-rs",
            version: "0.6.8",
            role: "gzip",
            codecs: &["zlib-rs-gzip"],
            packages: &["zlib-rs"],
            shim_tags: &["gzip-l", "gzip-d", "gzip-t"],
            shim_label: "zlib-rs".into(),
            notes: "API segura Deflate/Inflate (raw) com enquadramento gzip nosso, igual ao flate2 com a feature zlib-rs.",
        },
        Group {
            name: "bzip2 (libbz2-rs-sys)",
            version: "bzip2 0.6.1 + libbz2-rs-sys 0.2.5",
            role: "bzip2",
            codecs: &["bzip2-rs"],
            packages: &["bzip2", "libbz2-rs-sys"],
            shim_tags: &["bzip2-d"],
            shim_label: "bzip2".into(),
            notes: "Backend padrão da 0.6 é o porte Rust do libbzip2 (sem C); no nível 9 a saída é byte a byte a do \
                    bzip2 1.0.8. Compression::default() é 6, o bzip2 do GNU usa 9.",
        },
        Group {
            name: "lzma-rust2",
            version: "0.21.0",
            role: "xz",
            codecs: &["lzma-rust2-xz", "lzma-rust2-lzma", "lzma-rust2-lzip"],
            packages: &["lzma-rust2"],
            shim_tags: &["xz-d", "lzip-d", "lzma"],
            shim_label: "lzma-rust2".into(),
            notes: "Cobre .xz, .lzma e .lzip (leitor e escritor, filtros BCJ e delta). A feature padrão `optimization` usa \
                    unsafe localizado; sem ela a crate é forbid(unsafe_code).",
        },
        Group {
            name: "lzma-rs",
            version: "0.3.0",
            role: "xz",
            codecs: &["lzma-rs-xz", "lzma-rs-lzma"],
            packages: &["lzma-rs"],
            shim_tags: &["xz-d", "lzma"],
            shim_label: "lzma-rs".into(),
            notes: "forbid(unsafe_code); sem nível de compressão e sem lzip. O xz_compress grava LZMA2 em blocos não \
                    comprimidos e o lzma_compress só literais; o decodificador de xz não tem SHA-256 nem os filtros \
                    BCJ e delta.",
        },
        Group {
            name: "xz4rust",
            version: "0.2.3",
            role: "xz",
            codecs: &["xz4rust-xz"],
            packages: &["xz4rust"],
            shim_tags: &["xz-d"],
            shim_label: "xz4rust".into(),
            notes: "Só decodificador de .xz (sem .lzma, sem lzip, sem codificador).",
        },
        Group {
            name: "ruzstd",
            version: "0.9.0",
            role: "zstd",
            codecs: &["ruzstd"],
            packages: &["ruzstd"],
            shim_tags: &["zstd-d"],
            shim_label: "ruzstd".into(),
            notes: "Codificador só no nível Fastest (Default, Better e Best são unimplemented!()).",
        },
        Group {
            name: "structured-zstd",
            version: "0.0.58",
            role: "zstd",
            codecs: &["structured-zstd"],
            packages: &["structured-zstd"],
            shim_tags: &["zstd-d"],
            shim_label: "structured-zstd".into(),
            notes: "Fork do ruzstd com todos os níveis numéricos; versão 0.0.x (API instável).",
        },
        Group {
            name: "tar",
            version: "0.4.46",
            role: "tar",
            codecs: &[],
            packages: &["tar"],
            shim_tags: &["tar-tv", "tar-t", "tar-x"],
            shim_label: "tar".into(),
            notes: "Sem default features (sem xattr). Archive::entries resolve ././@LongLink e pax path/linkpath; \
                    Builder gera ././@LongLink sozinho mas pax só pelo primitivo append_pax_extensions (camada nossa); \
                    unpack usa std::fs, então a extração sobre o VFS é nossa.",
        },
        Group {
            name: "zip",
            version: "8.6.0",
            role: "zip",
            codecs: &[],
            packages: &["zip"],
            shim_tags: &["unzip-l", "unzip-x"],
            shim_label: "zip".into(),
            notes: "default-features = false com deflate-flate2 (miniz_oxide), bzip2 (Rust) e xz (lzma-rust2); a feature \
                    zstd puxaria o zstd-sys (C) e foi deixada de fora.",
        },
    ]
}

/// Insumos da decisão de encaixe além das matrizes do próprio candidato.
struct FitContext<'a> {
    pure: bool,
    tar_write_ok: bool,
    zip_write_ok: bool,
    gnu_tar_read: &'a BTreeMap<String, archives::ArchiveCheck>,
    gnu_zip_read: &'a BTreeMap<String, archives::ArchiveCheck>,
    /// Maior razão (saída do candidato / saída do GNU) no nível padrão, entre os codecs do candidato.
    worst_size_vs_gnu: Option<f64>,
}

/// Saída até 20% maior que a do GNU no nível padrão ainda conta como compressor utilizável.
const MAX_SIZE_VS_GNU: f64 = 1.2;

fn decide_fit(g: &Group, dec: &Tally, enc: &Tally, ctx: &FitContext) -> (Fit, String) {
    if !ctx.pure {
        return (Fit::DoesNotFit, "Reprovado: tem C na árvore.".into());
    }
    match g.role {
        "tar" => {
            let read_ok = ctx.gnu_tar_read.values().all(archives::ArchiveCheck::passed);
            if read_ok && ctx.tar_write_ok {
                (Fit::FitsWithWork, "Lê e escreve tudo que o GNU lê; pax e extração sobre o VFS ficam com a gente.".into())
            } else {
                (Fit::DoesNotFit, format!("Leitura ok={read_ok}, escrita ok={}.", ctx.tar_write_ok))
            }
        }
        "zip" => {
            let read_ok = ctx.gnu_zip_read.values().all(archives::ArchiveCheck::passed);
            if read_ok && ctx.zip_write_ok {
                (Fit::Fits, "Lê os zip do Info-ZIP e o unzip lê os nossos (stored, deflate, bzip2).".into())
            } else {
                (Fit::DoesNotFit, format!("Leitura ok={read_ok}, escrita ok={}.", ctx.zip_write_ok))
            }
        }
        _ => {
            let decode_ok = dec.all_pass();
            let encode_ok = enc.all_pass();
            if g.name == "xz4rust" {
                return if decode_ok {
                    (Fit::DoesNotFit, "Decodifica tudo do GNU, mas não codifica nem lê .lzma/.lz: não cobre o papel sozinho.".into())
                } else {
                    (Fit::DoesNotFit, format!("Decodificação {}.", dec.rate()))
                };
            }
            let rates = format!("GNU->crate {} e crate->GNU {}", dec.rate(), enc.rate());
            match (decode_ok, encode_ok, ctx.worst_size_vs_gnu) {
                (true, true, Some(s)) if s > MAX_SIZE_VS_GNU => (
                    Fit::DoesNotFit,
                    format!("{rates}, mas a saída fica {s:.2}x a do GNU no nível padrão: interopera, não serve de compressor."),
                ),
                (true, true, _) => (Fit::Fits, format!("{rates}.")),
                _ => (Fit::DoesNotFit, format!("{rates}.")),
            }
        }
    }
}

/// Placar do shim de CLI por conjunto de tags, por backend.
struct ShimScores {
    by_label: BTreeMap<String, cli::ArchiveCli>,
}

impl ShimScores {
    fn run(_cases: &[(Case, Outcome)]) -> ShimScores {
        let mut by_label = BTreeMap::new();
        let mk = |label: &str, o: Vec<Box<dyn Codec>>| (label.to_string(), cli::ArchiveCli::with(label, o));
        for (k, v) in [
            mk("flate2", vec![]),
            mk("zlib-rs", vec![Box::new(codecs::ZlibRsGzip)]),
            mk("bzip2", vec![]),
            mk("lzma-rust2", vec![]),
            mk("lzma-rs", vec![Box::new(codecs::LzmaRsXz), Box::new(codecs::LzmaRsLzma)]),
            mk("xz4rust", vec![Box::new(codecs::Xz4RustXz)]),
            mk("ruzstd", vec![Box::new(codecs::Ruzstd)]),
            mk("structured-zstd", vec![]),
            mk("tar", vec![]),
            mk("zip", vec![]),
        ] {
            by_label.insert(k, v);
        }
        ShimScores { by_label }
    }

    fn for_tags(&self, cases: &[(Case, Outcome)], tags: &[&str], label: &str) -> Option<Conformance> {
        let subset: Vec<(Case, Outcome)> =
            cases.iter().filter(|(c, _)| c.tags.iter().any(|t| tags.contains(&t.as_str()))).cloned().collect();
        if subset.is_empty() {
            return None;
        }
        let shim = self.by_label.get(label)?;
        Some(common::score_and_dump(shim, &subset, &format!("h31-cli-{label}")))
    }

    fn summary(&self) -> Value {
        json!({
            "backends": self.by_label.keys().collect::<Vec<_>>(),
            "note": "byte a byte contra corpus/cases/archive/listing.toml; cada candidato pontua só as tags do seu papel",
        })
    }
}

/// Bytes idênticos ao GNU na compressão (informativo: não é requisito de interoperabilidade).
pub fn compress_bytes_identical(cases: &[(Case, Outcome)]) -> BTreeMap<String, bool> {
    let shim = cli::ArchiveCli::with("padrão", vec![]);
    let mut out = BTreeMap::new();
    for (case, golden) in cases.iter().filter(|(c, _)| c.tags.iter().any(|t| t == "compress-bytes")) {
        let Ok(inv) = case.invocation() else { continue };
        let got = harness::Candidate::run(&shim, &inv);
        out.insert(case.id.clone(), got.stdout == golden.stdout && got.exit == golden.exit);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn variants_have_unique_names_per_format() {
        let mut seen = std::collections::BTreeSet::new();
        for v in variants() {
            assert!(seen.insert((v.format, v.name)), "{:?}/{}", v.format, v.name);
        }
    }

    #[test]
    fn timing_parser_subtracts_spawn() {
        let inputs = corpus::inputs();
        let out = "TIME noop 100 100000000\nSIZE gzip text 300000\nTIME enc-gzip-text 10 1010000000\n";
        let t = parse_timing(out, &inputs);
        assert_eq!(t.size["gzip-text"], 300_000);
        // 101 ms por iteração menos 1 ms de processo = 100 ms pra 1 MB = 10 MB/s.
        assert!((t.mbps["enc-gzip-text"] - 10.0).abs() < 0.01, "{:?}", t.mbps);
    }
}
