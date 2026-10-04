//! Interoperabilidade dos compressores com as ferramentas do oráculo (Debian 13), nos dois sentidos:
//! o que o GNU produz nós lemos, e o que nós produzimos o GNU lê. Também conta quantas saídas nossas
//! são byte a byte iguais às do GNU no mesmo nível.
//!
//! Precisa do docker e da imagem do oráculo: `cargo test -p ul-archive --test compress_interop --
//! --ignored --nocapture`.

use harness::{Entry, MemTree, Oracle};
use sysabi::testkit::TestKit;

/// Gerador determinístico (LCG), pra dados "aleatórios" reprodutíveis.
struct Lcg(u64);

impl Lcg {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
        self.0 >> 33
    }
}

/// Corpus: vazio, um byte, log de texto, registros binários e aleatório.
fn corpus() -> Vec<(&'static str, Vec<u8>)> {
    let mut log = Vec::new();
    let mut i = 0u64;
    while log.len() < 300_000 {
        log.extend_from_slice(
            format!("2026-01-15T12:{:02}:{:02}Z host app[{}]: request id={} status={} took={}ms\n", (i / 60) % 60, i % 60, 1000 + i % 7, i * 7919 % 100_003, [200, 200, 404, 500][(i % 4) as usize], i % 997).as_bytes(),
        );
        i += 1;
    }
    let mut rec = Vec::new();
    for k in 0..20_000u32 {
        rec.extend_from_slice(&k.to_le_bytes());
        rec.extend_from_slice(&(k.wrapping_mul(2_654_435_761)).to_be_bytes());
        rec.extend_from_slice(b"\0\0\x01\x02");
    }
    let mut rng = Lcg(31);
    let random: Vec<u8> = (0..100_000).map(|_| rng.next() as u8).collect();
    vec![("empty", Vec::new()), ("one", b"x".to_vec()), ("log", log), ("records", rec), ("random", random)]
}

fn kit_with(files: &[(String, Vec<u8>)]) -> TestKit {
    let k = TestKit::new().programs(ul_archive::programs());
    for (name, data) in files {
        k.put_file(format!("/work/{name}").as_bytes(), data, 0o644);
    }
    k
}

fn tree_of(files: &[(String, Vec<u8>)]) -> MemTree {
    let mut t = MemTree::new();
    for (n, d) in files {
        t.insert(n, Entry::file(d.clone(), 0o644));
    }
    t
}

/// Uma variante de compressão: nome, comando do GNU e o nosso (`{}` é o arquivo de entrada).
struct Variant {
    id: &'static str,
    ext: &'static str,
    gnu: &'static str,
    ours: &'static [&'static str],
    /// Comando do GNU que descomprime pra stdout.
    gnu_decode: &'static str,
    /// O nosso descompressor.
    ours_decode: &'static [&'static str],
}

fn variants() -> Vec<Variant> {
    vec![
        Variant { id: "gzip-1", ext: "gz", gnu: "gzip -1 -n -c", ours: &["gzip", "-1", "-n", "-c"], gnu_decode: "gzip -dc", ours_decode: &["gzip", "-dc"] },
        Variant { id: "gzip-6", ext: "gz", gnu: "gzip -n -c", ours: &["gzip", "-n", "-c"], gnu_decode: "gzip -dc", ours_decode: &["gzip", "-dc"] },
        Variant { id: "gzip-9", ext: "gz", gnu: "gzip -9 -n -c", ours: &["gzip", "-9", "-n", "-c"], gnu_decode: "gzip -dc", ours_decode: &["gzip", "-dc"] },
    ]
}

#[test]
#[ignore]
fn compress_matrix_against_oracle() {
    let oracle = Oracle::locate().expect("oráculo");
    let corpus = corpus();
    let files: Vec<(String, Vec<u8>)> = corpus.iter().map(|(n, d)| (n.to_string(), d.clone())).collect();
    // 1. O GNU comprime cada entrada em cada variante.
    let mut script = String::from("set -e; mkdir -p out; ");
    for v in variants() {
        for (n, _) in &corpus {
            script.push_str(&format!("{} {n} > out/{n}.{}.{}; ", v.gnu, v.id, v.ext));
        }
    }
    let gnu = oracle.run_script("compress-gnu", &script, tree_of(&files)).expect("oráculo comprimindo");
    assert_eq!(gnu.exit, Some(0), "{}", gnu.stderr.preview(500));
    // 2. Nós comprimimos as mesmas entradas e lemos as do GNU.
    let k = kit_with(&files);
    let mut ours_out: Vec<(String, Vec<u8>)> = Vec::new();
    let (mut equal, mut total, mut read_ok, mut read_total) = (0, 0, 0, 0);
    for v in variants() {
        for (n, data) in &corpus {
            let gname = format!("out/{n}.{}.{}", v.id, v.ext);
            let gdata = gnu.files.read(&gname).expect("saída do GNU").to_vec();
            let mut argv: Vec<&str> = v.ours.to_vec();
            argv.push(n);
            let r = k.run(&argv, b"");
            assert_eq!(r.code(), 0, "{} {n}: {}", v.id, r.stderr_str());
            total += 1;
            if r.stdout == gdata {
                equal += 1;
            } else {
                eprintln!("diferente do GNU: {} {n} ({} contra {} bytes)", v.id, r.stdout.len(), gdata.len());
            }
            ours_out.push((format!("{n}.{}.{}", v.id, v.ext), r.stdout.clone()));
            // Lemos a saída do GNU.
            k.put_file(format!("/work/g.{}", v.ext).as_bytes(), &gdata, 0o644);
            let mut dec: Vec<String> = v.ours_decode.iter().map(|s| s.to_string()).collect();
            dec.push(format!("g.{}", v.ext));
            let decv: Vec<&str> = dec.iter().map(String::as_str).collect();
            let r = k.run(&decv, b"");
            read_total += 1;
            if r.code() == 0 && &r.stdout == data {
                read_ok += 1;
            } else {
                eprintln!("não lemos o GNU: {} {n}: {}", v.id, r.stderr_str());
            }
        }
    }
    // 3. O GNU lê o que produzimos.
    let mut script = String::from("mkdir -p dec; ");
    for v in variants() {
        for (n, _) in &corpus {
            let f = format!("{n}.{}.{}", v.id, v.ext);
            script.push_str(&format!("{} {f} > dec/{f} 2>> errs || echo \"FALHA {f}\" >> errs; ", v.gnu_decode));
        }
    }
    let back = oracle.run_script("compress-back", &script, tree_of(&ours_out)).expect("oráculo lendo");
    let mut gnu_ok = 0;
    for v in variants() {
        for (n, data) in &corpus {
            let f = format!("dec/{n}.{}.{}", v.id, v.ext);
            if back.files.read(&f) == Some(data.as_slice()) {
                gnu_ok += 1;
            } else {
                eprintln!("o GNU não leu: {f}");
            }
        }
    }
    eprintln!(
        "compressores: GNU produz, nós lemos {read_ok}/{read_total}; nós produzimos, GNU lê {gnu_ok}/{total}; byte a byte iguais {equal}/{total}"
    );
    if let Some(errs) = back.files.read("errs") {
        eprintln!("{}", String::from_utf8_lossy(errs));
    }
    assert_eq!(read_ok, read_total);
    assert_eq!(gnu_ok, total);
}
