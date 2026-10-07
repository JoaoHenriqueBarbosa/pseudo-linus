//! Cenários de várias etapas do `tar` comparados com o GNU tar 1.35 do oráculo: listagem, extração,
//! acréscimo, atualização, concatenação, remoção, comparação, arquivos corrompidos e comprimidos.
//! Cada etapa compara stdout, stderr e código de saída; no fim, a árvore (conteúdo e modo).
//! Precisa do Docker: `cargo test -p ul-archive --test tar_ops -- --ignored --nocapture`.

use std::ffi::OsString;

use harness::{Entry, Invocation, MemTree, Oracle};
use pl_testing::{TestkitCandidate, tree_to_memtree};
use sysabi::{Ctx, Program};

fn ln_main(_ctx: &mut Ctx, args: &[OsString]) -> i32 {
    use std::os::unix::ffi::OsStrExt;
    let s = sysabi::sys::current();
    match s.linkat(sysabi::Fd::CWD, args[1].as_bytes(), sysabi::Fd::CWD, args[2].as_bytes(), sysabi::AtFlags::empty()) {
        Ok(()) => 0,
        Err(_) => 1,
    }
}

/// `mkdir DIR` mínimo pro testkit.
fn mkdir_main(_ctx: &mut Ctx, args: &[OsString]) -> i32 {
    use std::os::unix::ffi::OsStrExt;
    let s = sysabi::sys::current();
    for a in &args[1..] {
        if s.mkdirat(sysabi::Fd::CWD, a.as_bytes(), 0o777).is_err() {
            return 1;
        }
    }
    0
}

fn file(data: &[u8], mode: u32) -> Entry {
    Entry::file(data.to_vec(), mode)
}

fn tree() -> MemTree {
    let mut t = MemTree::new();
    t.insert("dir", Entry::dir(0o755));
    t.insert("dir/a.txt", file(b"hello world\n", 0o644));
    t.insert("dir/b.txt", file(b"bee\n", 0o640));
    t.insert("dir/run.sh", file(b"#!/bin/sh\n", 0o755));
    t.insert("dir/sub", Entry::dir(0o750));
    t.insert("dir/sub/c.dat", file(&[0u8, 1, 2, 3, 255], 0o600));
    t.insert("dir/link", Entry::symlink("a.txt"));
    t.insert("dir/sub/deep", Entry::dir(0o755));
    t.insert("dir/sub/deep/d.txt", file(b"deep\n", 0o644));
    t
}

/// Arquivos base gerados pelo GNU (tar e as compressões).
fn base_archives(oracle: &Oracle) -> MemTree {
    let script = "find . -type l -exec touch -h -d @1768478400 {} +; ln dir/a.txt dir/hard; touch -d @1768478400 dir; \
        tar --sort=name -cf a.tar dir; gzip -n -c a.tar > a.tgz; bzip2 -c a.tar > a.tbz; xz -c a.tar > a.txz; \
        zstd -q -c a.tar > a.tzst; lzip -c a.tar > a.tlz; tar --sort=name --format=pax -cf p.tar dir; \
        tar --sort=name -cf small.tar dir/a.txt dir/b.txt; rm -rf dir";
    let out = oracle.run_script("tar-ops-base", script, tree()).expect("oráculo");
    out.files
}

struct Step {
    argv: Vec<String>,
    stdin_file: Option<&'static str>,
}

fn step(v: &[&str]) -> Step {
    Step { argv: v.iter().map(|s| s.to_string()).collect(), stdin_file: None }
}

fn step_in(v: &[&str], stdin: &'static str) -> Step {
    Step { argv: v.iter().map(|s| s.to_string()).collect(), stdin_file: Some(stdin) }
}

struct Scenario {
    id: &'static str,
    files: MemTree,
    steps: Vec<Step>,
}

fn sh_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

fn with(base: &MemTree, keep: &[&str], extra: &[(&str, Entry)]) -> MemTree {
    let mut t = MemTree::new();
    for k in keep {
        if let Some(e) = base.get(k) {
            t.insert(k, e.clone());
        }
    }
    for (k, e) in extra {
        t.insert(k, e.clone());
    }
    t
}

fn scenarios(base: &MemTree) -> Vec<Scenario> {
    let a = base.read("a.tar").expect("a.tar").to_vec();
    let tgz = base.read("a.tgz").expect("a.tgz").to_vec();
    let small = base.read("small.tar").expect("small.tar").to_vec();
    let mut corrupt_first = a.clone();
    corrupt_first[0..4].copy_from_slice(b"ZZZZ");
    let mut corrupt_mid = a.clone();
    corrupt_mid[512..516].copy_from_slice(b"ZZZZ");
    let lone_zero: Vec<u8> = {
        let mut v = small[..1536].to_vec();
        v.extend_from_slice(&[0u8; 512]);
        v
    };
    let mut gz_garbage = tgz.clone();
    gz_garbage.extend_from_slice(b"garbage\n");
    let half = |name: &str| -> Vec<u8> {
        let d = base.read(name).expect("arquivo").to_vec();
        d[..d.len() / 2].to_vec()
    };
    let mut v = vec![
        Scenario { id: "x-basic", files: with(base, &["a.tar"], &[]), steps: vec![step(&["tar", "-xvf", "a.tar"])] },
        Scenario { id: "x-vv", files: with(base, &["a.tar"], &[]), steps: vec![step(&["tar", "-xvvf", "a.tar"])] },
        Scenario {
            id: "x-chdir-strip",
            files: with(base, &["a.tar"], &[("out", Entry::dir(0o755))]),
            steps: vec![step(&["tar", "-xvf", "a.tar", "-C", "out", "--strip-components=1"])],
        },
        Scenario {
            id: "x-chdir-missing",
            files: with(base, &["a.tar"], &[]),
            steps: vec![step(&["tar", "-xf", "a.tar", "-C", "nope"])],
        },
        Scenario {
            id: "x-members-wildcards",
            files: with(base, &["a.tar"], &[]),
            steps: vec![
                step(&["tar", "-xvf", "a.tar", "dir/sub", "dir/nope"]),
                step(&["tar", "-xvf", "a.tar", "--wildcards", "dir/*.txt"]),
                step(&["tar", "-tf", "a.tar", "--no-wildcards", "dir/*.txt"]),
            ],
        },
        Scenario {
            id: "x-exclude",
            files: with(base, &["a.tar"], &[]),
            steps: vec![step(&["tar", "-xvf", "a.tar", "--exclude=*.txt", "--exclude=deep"])],
        },
        Scenario {
            id: "x-existing",
            files: with(base, &["a.tar"], &[("dir/a.txt", file(b"local\n", 0o600)), ("dir/b.txt", file(b"lb\n", 0o644))]),
            steps: vec![
                step(&["tar", "-xkf", "a.tar"]),
                step(&["tar", "-xvf", "a.tar", "--skip-old-files", "dir/a.txt"]),
                step(&["tar", "-xf", "a.tar", "--overwrite", "dir/b.txt"]),
                step(&["tar", "-xf", "a.tar"]),
            ],
        },
        Scenario {
            id: "x-dir-file-conflicts",
            files: with(base, &["a.tar"], &[("dir/a.txt/inner", Entry::dir(0o755)), ("dir/sub", file(b"x", 0o644))]),
            steps: vec![step(&["tar", "-xf", "a.tar"])],
        },
        Scenario {
            id: "x-to-stdout",
            files: with(base, &["a.tar"], &[]),
            steps: vec![step(&["tar", "-xOvf", "a.tar", "dir/a.txt", "dir/sub"])],
        },
        Scenario {
            id: "x-one-top-level-transform",
            files: with(base, &["a.tar"], &[]),
            steps: vec![
                step(&["tar", "-xf", "a.tar", "--one-top-level=top"]),
                step(&["tar", "-xvf", "a.tar", "--transform=s,^dir,renamed,", "--show-transformed-names"]),
            ],
        },
        Scenario {
            id: "x-hardlink-missing",
            files: with(base, &["a.tar"], &[]),
            steps: vec![step(&["tar", "-xf", "a.tar", "dir/hard"])],
        },
        Scenario {
            id: "t-variants",
            files: with(base, &["a.tar", "p.tar"], &[]),
            steps: vec![
                step(&["tar", "-tvf", "a.tar"]),
                step(&["tar", "--full-time", "--utc", "-tvf", "p.tar"]),
                step(&["tar", "-tvRf", "a.tar"]),
                step(&["tar", "--numeric-owner", "-tvf", "a.tar", "dir/sub"]),
                step(&["tar", "--quoting-style=shell-always", "-tf", "a.tar", "dir/a.txt"]),
                step(&["tar", "--quoting-style=c", "-tf", "a.tar", "dir/link"]),
                step(&["tar", "--occurrence", "-tf", "a.tar", "dir/a.txt"]),
                step(&["tar", "-tf", "a.tar", "dir/nope", "dir/sub/"]),
            ],
        },
        Scenario {
            id: "t-compressed",
            files: with(base, &["a.tgz", "a.tbz", "a.txz", "a.tzst", "a.tlz"], &[]),
            steps: vec![
                step(&["tar", "-tf", "a.tgz"]),
                step(&["tar", "-tzf", "a.tgz", "dir/a.txt"]),
                step(&["tar", "-tf", "a.tbz", "dir/b.txt"]),
                step(&["tar", "-tJf", "a.txz", "dir/b.txt"]),
                step(&["tar", "--zstd", "-tf", "a.tzst", "dir/b.txt"]),
                step(&["tar", "--lzip", "-tf", "a.tlz", "dir/b.txt"]),
                step(&["tar", "-tjf", "a.tgz"]),
                step_in(&["tar", "-tf", "-"], "a.tgz"),
                step_in(&["tar", "-tzf", "-", "dir/run.sh"], "a.tgz"),
            ],
        },
        Scenario {
            id: "t-corrupt",
            files: with(
                base,
                &[],
                &[
                    ("first.tar", Entry::file(corrupt_first, 0o644)),
                    ("mid.tar", Entry::file(corrupt_mid, 0o644)),
                    ("lone.tar", Entry::file(lone_zero, 0o644)),
                    ("empty.tar", Entry::file(Vec::new(), 0o644)),
                    ("junk.tar", Entry::file(b"junk\n".to_vec(), 0o644)),
                    ("trunc.tar", Entry::file(a[..a.len().min(2600)].to_vec(), 0o644)),
                    ("gzhalf.tgz", Entry::file(half("a.tgz"), 0o644)),
                    ("gzjunk.tgz", Entry::file(gz_garbage, 0o644)),
                    ("xzhalf.txz", Entry::file(half("a.txz"), 0o644)),
                    ("zsthalf.tzst", Entry::file(half("a.tzst"), 0o644)),
                ],
            ),
            steps: vec![
                step(&["tar", "-tf", "first.tar"]),
                step(&["tar", "-tf", "mid.tar"]),
                step(&["tar", "-tf", "lone.tar"]),
                step(&["tar", "-tf", "empty.tar"]),
                step(&["tar", "-tf", "junk.tar"]),
                step(&["tar", "-tf", "trunc.tar"]),
                step(&["tar", "-xf", "trunc.tar"]),
                step(&["tar", "-tf", "gzhalf.tgz"]),
                step(&["tar", "-tf", "gzjunk.tgz"]),
                step(&["tar", "-tf", "xzhalf.txz"]),
                step(&["tar", "-tf", "zsthalf.tzst"]),
                step_in(&["tar", "-tzf", "-"], "junk.tar"),
            ],
        },
        Scenario {
            id: "r-u-A-delete",
            files: with(base, &["small.tar", "a.tar"], &[("dir/new.txt", file(b"new\n", 0o644))]),
            steps: vec![
                step(&["tar", "-rvf", "small.tar", "dir/new.txt"]),
                step(&["tar", "-tvf", "small.tar"]),
                step(&["tar", "-uvf", "small.tar", "dir"]),
                step(&["tar", "-tf", "small.tar"]),
                step(&["tar", "--delete", "-f", "small.tar", "dir/new.txt", "nope"]),
                step(&["tar", "-tf", "small.tar"]),
                step(&["tar", "-Af", "small.tar", "a.tar"]),
                step(&["tar", "-tf", "small.tar"]),
            ],
        },
        Scenario {
            id: "d-compare",
            files: with(
                base,
                &["a.tar"],
                &[
                    ("dir/a.txt", file(b"hello WORLD\n", 0o644)),
                    ("dir/b.txt", file(b"bee\n", 0o600)),
                    ("dir/run.sh", file(b"#!/bin/sh\nx\n", 0o755)),
                    ("dir/sub", Entry::dir(0o755)),
                    ("dir/link", Entry::symlink("b.txt")),
                ],
            ),
            steps: vec![step(&["tar", "-dvf", "a.tar"]), step(&["tar", "-df", "a.tar", "dir/sub"])],
        },
        Scenario {
            id: "arg-errors",
            files: MemTree::new(),
            steps: vec![
                step(&["tar"]),
                step(&["tar", "-cf", "x.tar"]),
                step(&["tar", "-ct"]),
                step(&["tar", "-czjf", "x", "a"]),
                step(&["tar", "--bogus"]),
                step(&["tar", "--s"]),
                step(&["tar", "cvbf", "20"]),
                step(&["tar", "-x", "--sort=foo"]),
                step(&["tar", "-x", "-b", "0"]),
                step(&["tar", "-x", "--format=foo"]),
                step(&["tar", "--quoting-style=foo", "-t"]),
                step(&["tar", "--version"]),
                step(&["tar", "--usage"]),
                step(&["tar", "--help"]),
                step(&["tar", "-tf", "nope.tar"]),
                step(&["tar", "-rzf", "x.tgz", "a"]),
            ],
        },
    ];
    v.retain(|s| !s.steps.is_empty());
    v
}

fn gnu_script(steps: &[Step]) -> String {
    let mut s = String::from("find . -type l -exec touch -h -d @1768478400 {} + 2>/dev/null; ");
    for (i, st) in steps.iter().enumerate() {
        let cmd: Vec<String> = st.argv.iter().map(|a| sh_quote(a)).collect();
        let input = match st.stdin_file {
            Some(f) => format!("< {f}"),
            None => "< /dev/null".to_string(),
        };
        s.push_str(&format!("{} {input} > .out{i} 2> .err{i}; echo -n $? > .rc{i}; ", cmd.join(" ")));
    }
    s
}

fn strip_meta(mut t: MemTree) -> MemTree {
    t.entries.retain(|k, _| !(k.starts_with(".out") || k.starts_with(".err") || k.starts_with(".rc")));
    t
}

#[test]
#[ignore]
fn ops_scenarios() {
    let oracle = Oracle::locate().expect("oráculo");
    let base = base_archives(&oracle);
    let mut programs = ul_archive::programs();
    programs.push(Program::bin("ln", ln_main));
    programs.push(Program::bin("mkdir", mkdir_main));
    let cand = TestkitCandidate::new("ul-archive tar", programs);
    let mut problems: Vec<String> = Vec::new();
    let mut steps_ok = 0;
    let mut steps_total = 0;
    for sc in scenarios(&base) {
        let gnu = oracle.run_script(sc.id, &gnu_script(&sc.steps), sc.files.clone()).expect("oráculo");
        let inv = Invocation {
            case_id: sc.id.into(),
            argv: vec!["tar".into()],
            script: None,
            stdin: Vec::new(),
            files: sc.files.clone(),
            env: Default::default(),
            faketime: None,
        };
        let kit = cand.kit_for(&inv);
        for (i, st) in sc.steps.iter().enumerate() {
            steps_total += 1;
            let stdin = st.stdin_file.and_then(|f| kit.read_file(&format!("/work/case/{f}"))).unwrap_or_default();
            let argv: Vec<&str> = st.argv.iter().map(String::as_str).collect();
            let r = kit.run(&argv, &stdin);
            let g_out = gnu.files.read(&format!(".out{i}")).unwrap_or_default();
            let g_err = gnu.files.read(&format!(".err{i}")).unwrap_or_default();
            let g_rc = String::from_utf8_lossy(gnu.files.read(&format!(".rc{i}")).unwrap_or_default()).to_string();
            let ok = g_out == r.stdout.as_slice() && g_err == r.stderr.as_slice() && g_rc == r.status.shell_status().to_string();
            if ok {
                steps_ok += 1;
            } else {
                problems.push(format!(
                    "{} passo {i} {:?}:\n  exit GNU {g_rc} nosso {}\n  GNU stdout {:?}\n  nosso      {:?}\n  GNU stderr {:?}\n  nosso      {:?}",
                    sc.id,
                    st.argv,
                    r.status.shell_status(),
                    String::from_utf8_lossy(g_out),
                    String::from_utf8_lossy(&r.stdout),
                    String::from_utf8_lossy(g_err),
                    String::from_utf8_lossy(&r.stderr)
                ));
            }
        }
        let ours_tree = tree_to_memtree(kit.tree("/work/case"));
        let gnu_tree = strip_meta(gnu.files.clone());
        let d = gnu_tree.diff(&ours_tree);
        if !d.is_empty() {
            problems.push(format!("{}: árvore final difere: {}", sc.id, d.join(" | ")));
        }
    }
    eprintln!("tar ops: {steps_ok}/{steps_total} passos iguais ao GNU, {} problemas", problems.len());
    let report = problems.iter().map(|p| format!("PROBLEMA {p}\n")).collect::<String>();
    if let Ok(path) = std::env::var("TAR_OPS_REPORT") {
        let _ = std::fs::write(path, &report);
    } else {
        eprint!("{report}");
    }
    assert!(problems.is_empty(), "{} problemas", problems.len());
}
