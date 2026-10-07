//! Interoperabilidade do `tar` com o GNU tar 1.35 do oráculo (precisa do Docker e da imagem do
//! oráculo; rode com `cargo test -p ul-archive --test tar_interop -- --ignored --nocapture`).
//!
//! - Igualdade byte a byte: a mesma árvore e a mesma linha de comando no GNU e no nosso tar.
//! - GNU lê o nosso: `tar -tvf`, `tar -xf` e `tar -df` do GNU sobre o arquivo que o nosso gerou.
//! - Nosso lê o do GNU: listagem e extração do nosso sobre o arquivo que o GNU gerou.

use std::ffi::OsString;

use harness::{Entry, Invocation, MemTree, Oracle};
use pl_testing::TestkitCandidate;
use sysabi::{Ctx, Program};

const T0: i64 = 1_768_478_400;

/// `ln ALVO NOME` mínimo, só pra montar links físicos no testkit.
fn ln_main(_ctx: &mut Ctx, args: &[OsString]) -> i32 {
    use std::os::unix::ffi::OsStrExt;
    let s = sysabi::sys::current();
    match s.linkat(sysabi::Fd::CWD, args[1].as_bytes(), sysabi::Fd::CWD, args[2].as_bytes(), sysabi::AtFlags::empty()) {
        Ok(()) => 0,
        Err(_) => 1,
    }
}

fn candidate() -> TestkitCandidate {
    let mut p = ul_archive::programs();
    p.push(Program::bin("ln", ln_main));
    TestkitCandidate::new("ul-archive tar", p)
}

fn file(data: &[u8], mode: u32) -> Entry {
    Entry::file(data.to_vec(), mode)
}

/// Árvore com modos variados, link simbólico, Unicode, arquivo grande, vazio e diretórios especiais.
fn basic_tree() -> MemTree {
    let mut t = MemTree::new();
    t.insert("dir", Entry::dir(0o755));
    t.insert("dir/a.txt", file(b"hello world\n", 0o644));
    t.insert("dir/run.sh", file(b"#!/bin/sh\necho hi\n", 0o755));
    t.insert("dir/sub", Entry::dir(0o750));
    t.insert("dir/sub/b.txt", file(b"bb\n", 0o600));
    t.insert("dir/link", Entry::symlink("a.txt"));
    t.insert("dir/empty", Entry::dir(0o700));
    t.insert("dir/ro", file(b"ro\n", 0o444));
    t.insert("dir/setuid", file(b"s", 0o4755));
    t.insert("dir/sticky", Entry::dir(0o1777));
    t.insert("dir/spaces and ünïcode.txt", file("ç\n".as_bytes(), 0o644));
    t.insert("dir/zero", file(b"", 0o644));
    let big: Vec<u8> = (0..100_000u32).map(|i| (i.wrapping_mul(2_654_435_761) >> 13) as u8).collect();
    t.insert("dir/big.bin", file(&big, 0o644));
    t
}

/// Nomes e alvos longos (LongLink no gnu, prefixo no ustar, `path` no pax).
fn long_tree() -> MemTree {
    let mut t = MemTree::new();
    let d1 = "x".repeat(60);
    let d2 = "y".repeat(60);
    t.insert("long", Entry::dir(0o755));
    t.insert(&format!("long/{d1}"), Entry::dir(0o755));
    t.insert(&format!("long/{d1}/{d2}"), Entry::dir(0o755));
    t.insert(&format!("long/{d1}/{d2}/file.txt"), file(b"deep\n", 0o644));
    t.insert(&format!("long/{}", "f".repeat(99)), file(b"99\n", 0o644));
    t.insert(&format!("long/{}", "g".repeat(98)), file(b"98\n", 0o644));
    t.insert("long/longlink", Entry::symlink("t".repeat(130)));
    t
}

/// Nomes fora do ASCII e com caracteres especiais (o pax guarda `path` em UTF-8).
fn unicode_tree() -> MemTree {
    let mut t = MemTree::new();
    t.insert("dir", Entry::dir(0o755));
    t.insert("dir/ünï çödé.txt", file(b"u\n", 0o644));
    t.insert("dir/日本語", Entry::dir(0o755));
    t.insert("dir/日本語/ファイル", file(b"j\n", 0o644));
    t.insert("dir/tab\there", file(b"t\n", 0o644));
    t.insert("dir/back\\slash", file(b"b\n", 0o644));
    t
}

fn inv(files: MemTree) -> Invocation {
    Invocation {
        case_id: "tar-interop".into(),
        argv: vec!["tar".into()],
        script: None,
        stdin: Vec::new(),
        files,
        env: Default::default(),
        faketime: None,
    }
}

/// Roda o nosso tar no testkit (com o link físico dir/hard -> dir/a.txt quando `hardlink`) e devolve
/// o kit.
fn ours(files: &MemTree, steps: &[Vec<String>], hardlink: bool) -> (sysabi::testkit::TestKit, Vec<(Vec<u8>, Vec<u8>, i32)>) {
    let cand = candidate();
    let kit = cand.kit_for(&inv(files.clone()));
    if hardlink {
        kit.run(&["ln", "dir/a.txt", "dir/hard"], b"");
        kit.set_mtime(b"/work/case/dir", T0);
    }
    let mut outs = Vec::new();
    for s in steps {
        let argv: Vec<&str> = s.iter().map(String::as_str).collect();
        let r = kit.run(&argv, b"");
        let code = r.status.shell_status();
        outs.push((r.stdout, r.stderr, code));
    }
    (kit, outs)
}

fn sh_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

/// Script do oráculo: fixa a data dos links simbólicos, cria o link físico se pedido, roda os passos.
fn gnu_script(steps: &[Vec<String>], hardlink: bool) -> String {
    let mut s = String::from("find . -type l -exec touch -h -d @1768478400 {} + 2>/dev/null; ");
    if hardlink {
        s.push_str("ln dir/a.txt dir/hard; touch -d @1768478400 dir; ");
    }
    for (i, st) in steps.iter().enumerate() {
        let cmd: Vec<String> = st.iter().map(|a| sh_quote(a)).collect();
        s.push_str(&format!("{} > .out{i} 2> .err{i}; echo -n $? > .rc{i}; ", cmd.join(" ")));
    }
    s
}

fn args(v: &[&str]) -> Vec<String> {
    v.iter().map(|s| s.to_string()).collect()
}

struct Variant {
    id: &'static str,
    tree: fn() -> MemTree,
    hardlink: bool,
    create: Vec<String>,
    out: &'static str,
    /// Byte a byte esperado (formatos sem compressão ou com bzip2, que é determinístico).
    bytes: bool,
}

fn variants() -> Vec<Variant> {
    let mut v = Vec::new();
    for fmt in ["gnu", "oldgnu", "ustar", "v7", "pax"] {
        v.push(Variant {
            id: Box::leak(format!("basic-{fmt}").into_boxed_str()),
            tree: basic_tree,
            hardlink: true,
            create: args(&["tar", "--sort=name", &format!("--format={fmt}"), "-cf", "out.tar", "dir"]),
            out: "out.tar",
            bytes: fmt != "pax",
        });
    }
    for fmt in ["gnu", "oldgnu", "pax"] {
        v.push(Variant {
            id: Box::leak(format!("long-{fmt}").into_boxed_str()),
            tree: long_tree,
            hardlink: false,
            create: args(&["tar", "--sort=name", &format!("--format={fmt}"), "-cf", "out.tar", "long"]),
            out: "out.tar",
            bytes: fmt != "pax",
        });
    }
    for (flag, ext, bytes) in [("-z", "tgz", false), ("-j", "tbz", true), ("-J", "txz", false), ("--zstd", "tzst", false), ("--lzip", "tlz", false)] {
        v.push(Variant {
            id: Box::leak(format!("basic-gnu-{ext}").into_boxed_str()),
            tree: basic_tree,
            hardlink: true,
            create: args(&["tar", "--sort=name", flag, "-cf", Box::leak(format!("out.{ext}").into_boxed_str()), "dir"]),
            out: Box::leak(format!("out.{ext}").into_boxed_str()),
            bytes,
        });
    }
    v.push(Variant {
        id: "options-owner-mode-mtime",
        tree: basic_tree,
        hardlink: true,
        create: args(&[
            "tar", "--sort=name", "--owner=alice:1000", "--group=staff:50", "--mode=go-w", "--mtime=2020-02-03 04:05:06",
            "-cf", "out.tar", "dir",
        ]),
        out: "out.tar",
        bytes: true,
    });
    v.push(Variant {
        id: "numeric-blocking",
        tree: basic_tree,
        hardlink: true,
        create: args(&["tar", "--sort=name", "--numeric-owner", "-b", "1", "-cf", "out.tar", "dir/a.txt", "dir/sub"]),
        out: "out.tar",
        bytes: true,
    });
    for (id, tree) in [("basic-pax-notimes", basic_tree as fn() -> MemTree), ("long-pax-notimes", long_tree), ("unicode-pax", unicode_tree)] {
        let top = if id.starts_with("long") { "long" } else { "dir" };
        v.push(Variant {
            id,
            tree,
            hardlink: id.starts_with("basic"),
            create: args(&["tar", "--sort=name", "--format=pax", "--pax-option=delete=atime,delete=ctime", "-cf", "out.tar", top]),
            out: "out.tar",
            bytes: true,
        });
    }
    v.push(Variant {
        id: "verbose-cvv",
        tree: basic_tree,
        hardlink: true,
        create: args(&["tar", "--sort=name", "-cvvf", "out.tar", "dir", "./dir/a.txt"]),
        out: "out.tar",
        bytes: true,
    });
    v.push(Variant {
        id: "absolute-and-chdir",
        tree: basic_tree,
        hardlink: false,
        create: args(&["tar", "--sort=name", "-cvf", "out.tar", "/work/case/dir/a.txt", "-C", "dir", "sub", "../dir/ro", "nope"]),
        out: "out.tar",
        bytes: true,
    });
    v.push(Variant {
        id: "to-stdout",
        tree: basic_tree,
        hardlink: false,
        create: args(&["tar", "--sort=name", "-cvf", "-", "dir/sub"]),
        out: "out.tar",
        bytes: true,
    });
    v.push(Variant {
        id: "ustar-errors",
        tree: long_tree,
        hardlink: false,
        create: args(&["tar", "--sort=name", "--format=ustar", "-cf", "out.tar", "long"]),
        out: "out.tar",
        bytes: true,
    });
    v.push(Variant {
        id: "v7-errors",
        tree: long_tree,
        hardlink: false,
        create: args(&["tar", "--sort=name", "--format=v7", "-cf", "out.tar", "long"]),
        out: "out.tar",
        bytes: true,
    });
    v.push(Variant {
        id: "exclude-transform",
        tree: basic_tree,
        hardlink: false,
        create: args(&["tar", "--sort=name", "--exclude=*.txt", "--transform=s,^dir,top,", "-cf", "out.tar", "dir"]),
        out: "out.tar",
        bytes: true,
    });
    v
}

/// Primeira diferença entre dois arquivos tar, com o campo do cabeçalho.
fn first_diff(a: &[u8], b: &[u8]) -> String {
    let n = a.len().min(b.len());
    for i in 0..n {
        if a[i] != b[i] {
            let block = i / 512;
            let off = i % 512;
            let field = match off {
                0..=99 => "name",
                100..=107 => "mode",
                108..=115 => "uid",
                116..=123 => "gid",
                124..=135 => "size",
                136..=147 => "mtime",
                148..=155 => "chksum",
                156 => "typeflag",
                157..=256 => "linkname",
                257..=264 => "magic",
                265..=296 => "uname",
                297..=328 => "gname",
                329..=344 => "dev",
                _ => "prefix/extra",
            };
            let name = String::from_utf8_lossy(&a[block * 512..(block * 512 + 100).min(a.len())]).trim_end_matches('\0').to_string();
            return format!("byte {i} (bloco {block}, campo {field}, nome {name:?}): GNU {:02x} nosso {:02x}", a[i], b[i]);
        }
    }
    format!("tamanhos: GNU {} nosso {}", a.len(), b.len())
}

#[test]
#[ignore]
fn interop_matrix() {
    let oracle = Oracle::locate().expect("oráculo");
    let mut equal = 0;
    let mut bytes_total = 0;
    let mut problems: Vec<String> = Vec::new();
    for v in variants() {
        let tree = (v.tree)();
        // GNU cria; o arquivo dele volta como fixture pros testes cruzados.
        let gnu = oracle
            .run_script(v.id, &gnu_script(std::slice::from_ref(&v.create), v.hardlink), tree.clone())
            .expect("oráculo");
        let gnu_rc = String::from_utf8_lossy(gnu.files.read(".rc0").unwrap_or_default()).to_string();
        let to_stdout = v.create.iter().any(|a| a == "-");
        let gnu_bytes = if to_stdout {
            gnu.files.read(".out0").map(|d| d.to_vec()).unwrap_or_default()
        } else {
            gnu.files.read(v.out).map(|d| d.to_vec()).unwrap_or_default()
        };
        let (kit, outs) = ours(&tree, std::slice::from_ref(&v.create), v.hardlink);
        let our_bytes =
            if to_stdout { outs[0].0.clone() } else { kit.read_file(&format!("/work/case/{}", v.out)).unwrap_or_default() };
        if gnu_rc != outs[0].2.to_string() {
            problems.push(format!("{}: exit GNU {gnu_rc} nosso {} ({})", v.id, outs[0].2, String::from_utf8_lossy(&outs[0].1)));
        }
        let gnu_out = gnu.files.read(".out0").unwrap_or_default().to_vec();
        let gnu_err = gnu.files.read(".err0").unwrap_or_default().to_vec();
        if gnu_out != outs[0].0 || gnu_err != outs[0].1 {
            problems.push(format!(
                "{}: saída difere:\nGNU stdout {:?}\nnosso      {:?}\nGNU stderr {:?}\nnosso      {:?}",
                v.id,
                String::from_utf8_lossy(&gnu_out),
                String::from_utf8_lossy(&outs[0].0),
                String::from_utf8_lossy(&gnu_err),
                String::from_utf8_lossy(&outs[0].1)
            ));
        }
        if v.bytes {
            bytes_total += 1;
            if gnu_bytes == our_bytes {
                equal += 1;
            } else {
                problems.push(format!("{}: arquivo difere: {}", v.id, first_diff(&gnu_bytes, &our_bytes)));
            }
        }
        // GNU lê o nosso: listagem igual à do arquivo dele, extração igual, -d sem diferença.
        let mut cross = tree.clone();
        cross.insert("ours.bin", Entry::file(our_bytes.clone(), 0o644));
        cross.insert("gnu.bin", Entry::file(gnu_bytes.clone(), 0o644));
        let script = "find . -type l -exec touch -h -d @1768478400 {} + 2>/dev/null; \
            tar --full-time -tvf ours.bin > l.ours 2>&1; echo $? >> l.ours; \
            tar --full-time -tvf gnu.bin > l.gnu 2>&1; echo $? >> l.gnu; \
            mkdir xo xg; tar -xf ours.bin -C xo 2> e.ours; echo $? >> e.ours; tar -xf gnu.bin -C xg 2> e.gnu; echo $? >> e.gnu; \
            (cd xo && find . -printf '%p %M %u %g %s %T@ %l\\n' | sort) | awk '{ if ($6+0 > 1780000000) $6=\"NOW\"; print }' > f.ours; \
            (cd xg && find . -printf '%p %M %u %g %s %T@ %l\\n' | sort) | awk '{ if ($6+0 > 1780000000) $6=\"NOW\"; print }' > f.gnu; \
            diff -r --no-dereference xo xg > d.tree 2>&1; echo $? >> d.tree; rm -rf xo xg";
        let g = oracle.run_script(&format!("{}-cross", v.id), script, cross).expect("oráculo");
        let rd = |n: &str| String::from_utf8_lossy(g.files.read(n).unwrap_or_default()).to_string();
        if rd("l.ours") != rd("l.gnu") {
            problems.push(format!("{}: GNU lista o nosso diferente:\n{}\n---\n{}", v.id, rd("l.ours"), rd("l.gnu")));
        }
        if rd("e.ours") != rd("e.gnu") || rd("f.ours") != rd("f.gnu") || rd("d.tree") != "0\n" {
            problems.push(format!(
                "{}: GNU extrai o nosso diferente: {} / {}\n{}\n---\n{}\n{}",
                v.id,
                rd("e.ours"),
                rd("e.gnu"),
                rd("f.ours"),
                rd("f.gnu"),
                rd("d.tree")
            ));
        }
        // Nosso lê o do GNU: listagem igual à que o GNU faz do próprio arquivo.
        let mut mine = MemTree::new();
        mine.insert("gnu.bin", Entry::file(gnu_bytes.clone(), 0o644));
        let (kit2, outs2) = ours(&mine, &[args(&["tar", "--full-time", "-tvf", "gnu.bin"])], false);
        let ours_list = format!("{}{}\n", String::from_utf8_lossy(&outs2[0].0), outs2[0].2);
        if ours_list != rd("l.gnu") {
            problems.push(format!("{}: nosso lista o do GNU diferente:\n{}\n---\n{}", v.id, ours_list, rd("l.gnu")));
        }
        drop(kit2);
    }
    eprintln!("tar interop: {equal}/{bytes_total} arquivos byte a byte iguais ao GNU, {} problemas", problems.len());
    let report = problems.iter().map(|p| format!("PROBLEMA {p}\n")).collect::<String>();
    if let Ok(dir) = std::env::var("TAR_INTEROP_REPORT") {
        let _ = std::fs::write(dir, &report);
    } else {
        eprint!("{report}");
    }
    assert!(problems.is_empty(), "{} problemas", problems.len());
}
