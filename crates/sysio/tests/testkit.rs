//! A fachada sobre o kernel de teste do sysabi: cada teste registra programas pequenos escritos
//! contra o `sysio` (como um porte escreveria) e confere stdout, stderr, status e arquivos.

use std::ffi::OsString;
use std::io::{BufRead, Read, Write};

use sysabi::testkit::TestKit;
use sysabi::{Ctx, Program};
use sysio::fs::{self, File, OpenOptions};
use sysio::os::unix::fs::PermissionsExt;
use sysio::process::{Command, Stdio};

fn args(argv: &[OsString]) -> Vec<String> {
    argv.iter().map(|a| a.to_string_lossy().into_owned()).collect()
}

/// `fsdemo`: cria, lê, lista, renomeia e canonicaliza.
fn fsdemo(_ctx: &mut Ctx, _argv: &[OsString]) -> i32 {
    sysio::run(|| {
        let mut out = sysio::io::stdout();
        fs::create_dir_all("d/e/f").unwrap();
        fs::write("d/a.txt", b"um\ndois\n").unwrap();
        let mut f = OpenOptions::new().append(true).open("d/a.txt").unwrap();
        f.write_all(b"tres\n").unwrap();
        drop(f);
        let text = fs::read_to_string("d/a.txt").unwrap();
        write!(out, "{text}").unwrap();
        let mut names: Vec<String> =
            fs::read_dir("d").unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
        names.sort();
        writeln!(out, "{}", names.join(",")).unwrap();
        fs::symlink("e/f", "d/link").unwrap();
        writeln!(out, "{}", fs::canonicalize("d/link/../../a.txt").unwrap().display()).unwrap();
        writeln!(out, "{}", fs::symlink_metadata("d/link").unwrap().file_type().is_symlink()).unwrap();
        fs::rename("d/a.txt", "d/b.txt").unwrap();
        writeln!(out, "{}", fs::exists("d/a.txt")).unwrap();
        let m = fs::metadata("d/b.txt").unwrap();
        writeln!(out, "{} {:o}", m.len(), m.permissions().mode() & 0o777).unwrap();
        let e = File::open("nope").unwrap_err();
        writeln!(out, "{}", sysio::errno::strerror(&e)).unwrap();
        let e = fs::canonicalize("d/b.txt/x").unwrap_err();
        writeln!(out, "{}", sysio::errno::strerror(&e)).unwrap();
        fs::remove_dir_all("d").unwrap();
        writeln!(out, "{}", fs::exists("d")).unwrap();
        0
    })
}

#[test]
fn fs_roundtrip() {
    let kit = TestKit::new().programs([Program::bin("fsdemo", fsdemo)]);
    let r = kit.run(&["fsdemo"], b"");
    assert_eq!(r.stderr_str(), "");
    assert_eq!(
        r.stdout_str(),
        "um\ndois\ntres\na.txt,e\n/work/d/a.txt\ntrue\nfalse\n13 644\nNo such file or directory\nNot a directory\nfalse\n"
    );
    assert_eq!(r.code(), 0);
}

/// `order`: stdout com buffer em bloco e stderr sem buffer, como o stdio da glibc num pipe.
fn order(_ctx: &mut Ctx, _argv: &[OsString]) -> i32 {
    sysio::run(|| {
        sysio::println!("primeiro no stdout");
        sysio::eprintln!("erro no meio");
        sysio::print!("fim sem quebra");
        3
    })
}

#[test]
fn stdout_is_block_buffered_and_flushed_at_end() {
    let kit = TestKit::new().programs([Program::bin("order", order)]);
    let r = kit.run(&["order"], b"");
    assert_eq!(r.stdout_str(), "primeiro no stdout\nfim sem quebra");
    assert_eq!(r.stderr_str(), "erro no meio\n");
    assert_eq!(r.code(), 3);
}

/// `exiter`: `process::exit` no meio descarrega o stdout.
fn exiter(_ctx: &mut Ctx, _argv: &[OsString]) -> i32 {
    sysio::run(|| {
        sysio::print!("antes do exit");
        sysio::process::exit(7);
    })
}

#[test]
fn exit_flushes_stdout() {
    let kit = TestKit::new().programs([Program::bin("exiter", exiter)]);
    let r = kit.run(&["exiter"], b"");
    assert_eq!(r.stdout_str(), "antes do exit");
    assert_eq!(r.code(), 7);
}

/// `echoargs`: imprime argv[1..] e o ambiente pedido.
fn echoargs(_ctx: &mut Ctx, argv: &[OsString]) -> i32 {
    sysio::run(|| {
        let a = args(argv);
        sysio::println!("{}", a[1..].join(" "));
        if let Ok(v) = sysio::env::var("FOO") {
            sysio::println!("FOO={v}");
        }
        let mut line = String::new();
        if sysio::io::stdin().lock().read_line(&mut line).unwrap() > 0 {
            sysio::print!("stdin: {line}");
        }
        a.len() as i32 - 1
    })
}

/// `parent`: imprime, cria filhos de vários jeitos e mostra o que eles devolveram.
fn parent(_ctx: &mut Ctx, _argv: &[OsString]) -> i32 {
    sysio::run(|| {
        sysio::println!("pai antes");
        let st = Command::new("echoargs").args(["herdado", "x"]).status().unwrap();
        sysio::println!("status {:?}", st.code());
        let out = Command::new("echoargs").arg("capturado").env("FOO", "bar").output().unwrap();
        sysio::println!("saida {:?} {:?}", String::from_utf8_lossy(&out.stdout), out.status.code());
        let mut child = Command::new("/usr/bin/echoargs").arg("pipe").stdout(Stdio::piped()).spawn().unwrap();
        let mut s = String::new();
        child.stdout.take().unwrap().read_to_string(&mut s).unwrap();
        sysio::println!("lido {s:?} {:?}", child.wait().unwrap().code());
        let e = Command::new("nao-existe").status().unwrap_err();
        sysio::println!("erro {}", sysio::errno::strerror(&e));
        let f = File::create("entrada").unwrap();
        drop(f);
        fs::write("entrada", b"linha do arquivo\n").unwrap();
        let st = Command::new("echoargs").stdin(File::open("entrada").unwrap()).status().unwrap();
        sysio::println!("com stdin {:?}", st.code());
        0
    })
}

#[test]
fn command_spawn_status_output_and_path_search() {
    let kit = TestKit::new().programs([Program::bin("echoargs", echoargs), Program::bin("parent", parent)]);
    let r = kit.run(&["parent"], b"");
    assert_eq!(r.stderr_str(), "");
    assert_eq!(
        r.stdout_str(),
        "pai antes\nherdado x\nstatus Some(2)\nsaida \"capturado\\nFOO=bar\\n\" Some(1)\nlido \"pipe\\n\" Some(1)\n\
         erro No such file or directory\n\nstdin: linha do arquivo\ncom stdin Some(0)\n"
    );
}

/// `cwdenv`: diretório corrente, ambiente e identidade pelo kernel.
fn cwdenv(_ctx: &mut Ctx, _argv: &[OsString]) -> i32 {
    sysio::run(|| {
        sysio::println!("{}", sysio::env::current_dir().unwrap().display());
        sysio::env::set_current_dir("/tmp").unwrap();
        sysio::println!("{}", sysio::env::current_dir().unwrap().display());
        sysio::env::set_var("NOVA", "1");
        sysio::println!("{:?}", sysio::env::var("NOVA"));
        sysio::env::remove_var("NOVA");
        sysio::println!("{:?}", sysio::env::var("NOVA"));
        sysio::println!("{}", sysio::users::uid2usr(sysio::users::geteuid()).unwrap());
        sysio::println!("{:o}", sysio::process::get_umask());
        let n = sysio::env::vars_os().filter(|(k, _)| k == "HOME").count();
        sysio::println!("{n}");
        0
    })
}

#[test]
fn env_cwd_and_identity() {
    let kit = TestKit::new().programs([Program::bin("cwdenv", cwdenv)]);
    let r = kit.run(&["cwdenv"], b"");
    assert_eq!(r.stdout_str(), "/work\n/tmp\nOk(\"1\")\nErr(NotPresent)\nroot\n22\n1\n");
}

/// `catlike`: lê o stdin inteiro em pedaços e devolve.
fn catlike(_ctx: &mut Ctx, _argv: &[OsString]) -> i32 {
    sysio::run(|| {
        let mut data = Vec::new();
        sysio::io::stdin().read_to_end(&mut data).unwrap();
        sysio::io::stdout().write_all(&data).unwrap();
        0
    })
}

#[test]
fn stdin_and_large_stdout() {
    let kit = TestKit::new().programs([Program::bin("catlike", catlike)]);
    let big: Vec<u8> = (0..100_000u32).map(|i| (i % 251) as u8).collect();
    let r = kit.run(&["catlike"], &big);
    assert_eq!(r.stdout, big);
}

/// `fulldev`: escrever no /dev/full via stdout redirecionado dá "write error" no fim.
fn fullwriter(_ctx: &mut Ctx, _argv: &[OsString]) -> i32 {
    sysio::run(|| {
        sysio::println!("vai falhar");
        0
    })
}

fn redirect_to_full(_ctx: &mut Ctx, _argv: &[OsString]) -> i32 {
    sysio::run(|| {
        let full = OpenOptions::new().write(true).open("/dev/full").unwrap();
        let st = Command::new("fullwriter").stdout(full).status().unwrap();
        sysio::println!("{:?}", st.code());
        0
    })
}

#[test]
fn final_flush_error_is_reported_like_close_stdout() {
    let kit = TestKit::new().programs([Program::bin("fullwriter", fullwriter), Program::bin("redir", redirect_to_full)]);
    let r = kit.run(&["redir"], b"");
    assert_eq!(r.stdout_str(), "Some(1)\n");
    assert_eq!(r.stderr_str(), "fullwriter: write error: No space left on device\n");
}

/// `threads`: thread nova herda o processo e o stdout.
fn threads(_ctx: &mut Ctx, _argv: &[OsString]) -> i32 {
    sysio::run(|| {
        let h = sysio::thread::spawn(|| {
            sysio::println!("da thread {}", sysio::process::id() > 0);
            fs::exists("/etc/passwd")
        });
        let r = h.join().unwrap();
        sysio::println!("main {r}");
        0
    })
}

#[test]
fn threads_inherit_process() {
    let kit = TestKit::new().programs([Program::bin("threads", threads)]);
    let r = kit.run(&["threads"], b"");
    assert_eq!(r.stdout_str(), "da thread true\nmain true\n");
}
