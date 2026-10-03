//! Multicall dos utilitários originais, sem nenhuma alteração: despacha pelo nome do `argv[0]`
//! (`cat`, `sort`, `ls`, `head`, `wc`, `find`, `xargs`), repetindo o que o `main` de cada crate faz.
//!
//! Ele roda dentro do container do oráculo (mesma glibc do host), num diretório com links simbólicos
//! `cat -> uu-original` etc., e serve pra separar "fidelidade do uutils" de "custo do porte".

use std::ffi::OsString;
use std::io::Write;
use std::path::Path;

fn main() {
    let args: Vec<OsString> = std::env::args_os().collect();
    let name = args
        .first()
        .and_then(|a| Path::new(a).file_name())
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    if args.get(1).is_some_and(|a| a == "--demo-global-state") {
        demo_global_state();
        return;
    }
    let code = match name.as_str() {
        "find" => run_find(),
        "xargs" => run_xargs(),
        util => run_coreutil(util),
    };
    std::process::exit(code);
}

/// Dois utilitários originais chamados em sequência no mesmo processo (o que um pseudo-kernel
/// faria se só chamasse `uumain`): `wc` de um arquivo que não existe e depois `cat` de um que
/// existe. Mostra o estado global do uucore: o `EXIT_CODE` do `wc` vaza pro `cat`, e o nome nas
/// mensagens vem do argv do processo host, não do argv passado pro `uumain`. Espera `ok.txt` no
/// diretório corrente.
fn demo_global_state() {
    let _ = uucore::locale::setup_localization("wc");
    let wc = uu_wc::uumain(["wc", "nope.txt"].into_iter().map(OsString::from));
    let _ = uucore::locale::setup_localization("cat");
    let cat = uu_cat::uumain(["cat", "ok.txt"].into_iter().map(OsString::from));
    let _ = std::io::stdout().flush();
    println!("{{\"wc_missing_exit\": {wc}, \"cat_ok_exit\": {cat}}}");
}

/// O mesmo que a macro `uucore::bin!` expande: SIGPIPE, localização, `uumain`, flush.
fn run_coreutil(util: &str) -> i32 {
    uucore::panic::preserve_inherited_sigpipe();
    uucore::panic::mute_sigpipe_panic();
    if let Err(err) = uucore::locale::setup_localization(util) {
        eprintln!("Could not init the localization system: {err}");
        return 99;
    }
    let args = uucore::args_os();
    let code = match util {
        "cat" => uu_cat::uumain(args),
        "sort" => uu_sort::uumain(args),
        "ls" => uu_ls::uumain(args),
        "head" => uu_head::uumain(args),
        "wc" => uu_wc::uumain(args),
        other => {
            eprintln!("uu-original: utilitário desconhecido: {other}");
            return 127;
        }
    };
    if let Err(e) = std::io::stdout().flush() {
        eprintln!("Error flushing stdout: {e}");
    }
    code
}

/// O mesmo que `findutils/src/find/main.rs`.
fn run_find() -> i32 {
    uucore::panic::mute_sigpipe_panic();
    let args: Vec<String> = std::env::args().collect();
    let strs: Vec<&str> = args.iter().map(String::as_str).collect();
    let deps = findutils::find::StandardDependencies::new();
    findutils::find::find_main(&strs, &deps)
}

/// O mesmo que `findutils/src/xargs/main.rs`.
fn run_xargs() -> i32 {
    let args: Vec<String> = std::env::args().collect();
    let strs: Vec<&str> = args.iter().map(String::as_str).collect();
    findutils::xargs::xargs_main(&strs)
}
