//! Família gprofng do GNU binutils 2.44 (Debian 13): o driver `gprofng` e as ferramentas
//! `gprofng-archive`, `gprofng-collect-app`, `gprofng-display-html`, `gprofng-display-src` e
//! `gprofng-display-text`, mais os nomes antigos `gp-*` (mesmos programas).
//!
//! Escrito a partir do comportamento observado e da man page (o código do binutils é GPL e não
//! foi consultado). Cobre `--version`, `--help`, validação do uso e os códigos de saída.
//!
//! Limitações documentadas (TODO): não há coleta de dados nem leitura de experimentos; com
//! entrada válida o programa falha com uma mensagem explícita e sai com 1. Os textos de ajuda das
//! ferramentas foram reproduzidos de memória e podem divergir do original; a mensagem de
//! experimento inexistente também é a melhor reconstrução disponível.

use std::ffi::OsString;
use std::io::Write;

use sysabi::sys;

use crate::util::io;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tool {
    Archive,
    CollectApp,
    DisplayHtml,
    DisplaySrc,
    DisplayText,
}

impl Tool {
    /// Nome canônico usado na linha de versão.
    fn canon(self) -> &'static str {
        match self {
            Tool::Archive => "gp-archive",
            Tool::CollectApp => "gp-collect-app",
            Tool::DisplayHtml => "gp-display-html",
            Tool::DisplaySrc => "gp-display-src",
            Tool::DisplayText => "gp-display-text",
        }
    }

    fn synopsis(self) -> &'static str {
        match self {
            Tool::Archive => "Usage: gprofng archive [OPTION(S)] EXPERIMENT(S)\n",
            Tool::CollectApp => "Usage: gprofng collect app [OPTION(S)] TARGET [TARGET_ARGUMENTS]\n",
            Tool::DisplayHtml => "Usage: gprofng display html [OPTION(S)] EXPERIMENT(S)\n",
            Tool::DisplaySrc => "Usage: gprofng display src [OPTION(S)] EXPERIMENT(S)\n",
            Tool::DisplayText => "Usage: gprofng display text [OPTION(S)] [COMMAND(S)] [-script <file>] EXPERIMENT(S)\n",
        }
    }

    fn blurb(self) -> &'static str {
        match self {
            Tool::Archive => "\nArchive the associated application binaries and sources in one or more\nexperiments, so that they can be moved to another machine.\n",
            Tool::CollectApp => "\nCollect performance data on the target program. In addition to Program\nCounter (PC) sampling, support for hardware event counters and clock\nprofiling is available.\n",
            Tool::DisplayHtml => "\nGenerate an HTML structure to view the performance data in a browser.\n",
            Tool::DisplaySrc => "\nDisplay the source code, interleaved with the instructions of the\nfunction(s) that match the specified name.\n",
            Tool::DisplayText => "\nDisplay the performance data in plain text format.\n",
        }
    }
}

const DRIVER_HELP: &str = "Usage: gprofng [OPTION(S)] COMMAND [KEYWORD] [ARGUMENTS]

This is the driver for the GNU gprofng tool suite to gather and analyze
performance data.

Options:

 --version           print the version number and exit.
 --help              print this help and exit.
 --check             check if the system is set up for profiling.
 --verbose           enable verbose mode to show diagnostic messages
                     about the processing of the application data.

Commands:

The following commands are supported:

 collect app         collect performance data on the target program.
 display text        display the performance data in ASCII table format.
 display html        generate an HTML file from one or more experiments.
 display src         display source code annotated with performance data.
 archive             include binaries and source code in an experiment
                     directory.

Use \"gprofng <command> --help\" to get more information on a command.

Report bugs to <https://sourceware.org/bugzilla/>
";

fn out(text: &str) {
    let mut o = io::stdout();
    let _ = o.write_all(text.as_bytes());
}

fn version(name: &str) {
    out(&format!("GNU {name} (GNU Binutils for Debian) 2.44\n"));
}

pub fn gprofng_main(_ctx: &mut sysabi::Ctx, args: &[OsString]) -> i32 {
    io::run(|| driver(args))
}

pub fn archive_main(_ctx: &mut sysabi::Ctx, args: &[OsString]) -> i32 {
    io::run(|| tool(Tool::Archive, &io::argv0(args), &io::args_bytes(args)[1..]))
}

pub fn collect_app_main(_ctx: &mut sysabi::Ctx, args: &[OsString]) -> i32 {
    io::run(|| tool(Tool::CollectApp, &io::argv0(args), &io::args_bytes(args)[1..]))
}

pub fn display_html_main(_ctx: &mut sysabi::Ctx, args: &[OsString]) -> i32 {
    io::run(|| tool(Tool::DisplayHtml, &io::argv0(args), &io::args_bytes(args)[1..]))
}

pub fn display_src_main(_ctx: &mut sysabi::Ctx, args: &[OsString]) -> i32 {
    io::run(|| tool(Tool::DisplaySrc, &io::argv0(args), &io::args_bytes(args)[1..]))
}

pub fn display_text_main(_ctx: &mut sysabi::Ctx, args: &[OsString]) -> i32 {
    io::run(|| tool(Tool::DisplayText, &io::argv0(args), &io::args_bytes(args)[1..]))
}

fn driver(args: &[OsString]) -> i32 {
    let prog = io::argv0(args);
    let argv = io::args_bytes(args);
    let mut i = 1;
    while i < argv.len() {
        let a = String::from_utf8_lossy(&argv[i]).into_owned();
        match a.as_str() {
            "--version" => {
                version("gprofng");
                return 0;
            }
            "--help" => {
                out(DRIVER_HELP);
                return 0;
            }
            "--check" | "--verbose" => i += 1,
            _ if a.starts_with('-') => {
                io::eprint(format!("{prog}: unrecognized option '{a}'\n"));
                io::eprint(DRIVER_HELP);
                return 1;
            }
            _ => break,
        }
    }
    let Some(cmd) = argv.get(i) else {
        io::eprint(DRIVER_HELP);
        return 1;
    };
    let rest = &argv[i + 1..];
    let cmd = String::from_utf8_lossy(cmd).into_owned();
    let sub = rest.first().map(|s| String::from_utf8_lossy(s).into_owned());
    match (cmd.as_str(), sub.as_deref()) {
        ("archive", _) => tool(Tool::Archive, &format!("{prog} archive"), rest),
        ("collect", Some("app")) => tool(Tool::CollectApp, &format!("{prog} collect app"), &rest[1..]),
        ("display", Some("text")) => tool(Tool::DisplayText, &format!("{prog} display text"), &rest[1..]),
        ("display", Some("html")) => tool(Tool::DisplayHtml, &format!("{prog} display html"), &rest[1..]),
        ("display", Some("src")) => tool(Tool::DisplaySrc, &format!("{prog} display src"), &rest[1..]),
        ("collect", _) | ("display", _) => {
            io::eprint(format!("{prog}: Error: unknown or missing keyword for command '{cmd}'\n"));
            io::eprint(DRIVER_HELP);
            1
        }
        _ => {
            io::eprint(format!("{prog}: Error: unknown command '{cmd}'\n"));
            io::eprint(DRIVER_HELP);
            1
        }
    }
}

/// Opções do `collect app` que consomem o argumento seguinte.
const COLLECT_ARG: &[&str] = &[
    "-o", "-O", "-p", "-h", "-j", "-J", "-d", "-F", "-A", "-C", "-t", "-y", "-P", "-l", "-M", "-m",
    "-S", "-s", "-c", "-N", "-n", "-x",
];

fn tool(kind: Tool, prog: &str, args: &[Vec<u8>]) -> i32 {
    let mut operands: Vec<String> = Vec::new();
    let mut i = 0;
    while i < args.len() {
        let a = String::from_utf8_lossy(&args[i]).into_owned();
        i += 1;
        if a == "--version" || a == "-V" {
            version(kind.canon());
            return 0;
        }
        if a == "--help" || a == "-h" && kind != Tool::CollectApp {
            out(&format!("{}{}\nReport bugs to <https://sourceware.org/bugzilla/>\n", kind.synopsis(), kind.blurb()));
            return 0;
        }
        if kind == Tool::CollectApp {
            if COLLECT_ARG.contains(&a.as_str()) {
                i += 1;
                continue;
            }
            if a.starts_with('-') && a.len() > 1 {
                continue;
            }
            // O primeiro operando é o alvo; o resto são argumentos dele.
            operands.push(a);
            break;
        }
        if a.starts_with('-') && a.len() > 1 {
            // Opções de exibição e de arquivamento não afetam a validação aqui.
            continue;
        }
        operands.push(a);
    }
    if operands.is_empty() {
        io::eprint(format!("{prog}: Error: no {} specified\n", if kind == Tool::CollectApp { "target program" } else { "experiment" }));
        io::eprint(kind.synopsis());
        return 1;
    }
    if kind != Tool::CollectApp {
        let mut missing = false;
        for e in &operands {
            if let Err(err) = sys::stat(e.as_bytes()) {
                io::eprint(format!(
                    "{prog}: Error: experiment `{e}' not found: {}\n",
                    err.message()
                ));
                missing = true;
            }
        }
        if missing {
            return 1;
        }
        io::eprint(format!(
            "{prog}: Error: reading experiments is not supported by this implementation\n"
        ));
        return 1;
    }
    io::eprint(format!(
        "{prog}: Error: data collection is not supported by this implementation\n"
    ));
    1
}
