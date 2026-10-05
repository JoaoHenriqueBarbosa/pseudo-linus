//! `ld` e `ld.bfd` (GNU ld) do binutils 2.44 (Debian 13), x86_64-linux-gnu.
//!
//! Escrito a partir do comportamento observado e da man page (o código do binutils é GPL e não
//! foi consultado). Cobre `--help`, `--version`, `-v`, `-V`, validação de opções (as que
//! consomem argumento são conhecidas, o resto desconhecido dá o erro do original), `no input
//! files`, busca de `-l` nos diretórios de `-L` e nos padrões do x86_64, e `cannot find`.
//!
//! Limitação documentada: não há ligador. Com entradas válidas o programa falha com
//! `linking is not supported by this implementation` e sai com 1, sem criar a saída. O `--help`
//! traz só o começo e o final do texto original (as centenas de linhas do meio não foram
//! reproduzidas).

use std::ffi::OsString;
use std::io::Write;

use sysabi::sys;

use crate::util::io::{self, File};

const VERSION_LINE: &str = "GNU ld (GNU Binutils for Debian) 2.44\n";

const VERSION_REST: &str = "Copyright (C) 2025 Free Software Foundation, Inc.\n\
This program is free software; you may redistribute it under the terms of\n\
the GNU General Public License version 3 or (at your option) a later version.\n\
This program has absolutely no warranty.\n";

const EMULATIONS: &[&str] = &[
    "elf_x86_64",
    "elf32_x86_64",
    "elf_i386",
    "elf_iamcu",
    "elf_l1om",
    "elf_k1om",
    "i386pep",
    "i386pe",
];

const HELP_HEAD: &str = "  -a KEYWORD                  Shared library control for HP/UX compatibility
  -A ARCH, --architecture ARCH
                              Set architecture
  -b TARGET, --format TARGET  Specify target for following input files
  -c FILE, --mri-script FILE  Read MRI format linker script
  -d, -dc, -dp                Force common symbols to be defined
  --dependency-file FILE      Write dependency file
  --force-group-allocation    Force group members out of groups
  -e ADDRESS, --entry ADDRESS Set start address
  -E, --export-dynamic        Export all dynamic symbols
  --no-export-dynamic         Undo the effect of --export-dynamic
  -EB                         Link big-endian objects
  -EL                         Link little-endian objects
  -f SHLIB, --auxiliary SHLIB Auxiliary filter for shared object symbol table
  -F SHLIB, --filter SHLIB    Filter for shared object symbol table
  -g                          Ignored
  -G SIZE, --gpsize SIZE      Small data size (if no size, same as --shared)
  -h FILENAME, -soname FILENAME
                              Set internal name of shared library
  -I PROGRAM, --dynamic-linker PROGRAM
                              Set PROGRAM as the dynamic linker to use
  --no-dynamic-linker         Produce an executable with no program interpreter header
  --sort-common [={ascending|descending}]
                              Sort common symbols by alignment [in specified order]
  --sort-section name         Sort sections by name or maximum alignment
  --spare-dynamic-tags COUNT  How many tags to reserve in .dynamic section
  --split-by-file [=SIZE]     Split output sections every SIZE octets
  --split-by-reloc [=COUNT]   Split output sections every COUNT relocs
  --stats                     Print memory usage statistics
  --target-help               Display target specific options
  --task-link SYMBOL          Do task level linking
  --traditional-format        Use same format as native linker
  --section-start SECTION=ADDRESS
                              Set address of named section
  -Tbss ADDRESS               Set address of .bss section
  -Tdata ADDRESS              Set address of .data section
  -Ttext ADDRESS              Set address of .text section
  -Ttext-segment ADDRESS      Set address of text segment
  -Trodata-segment ADDRESS    Set address of rodata segment
  -Tldata-segment ADDRESS     Set address of ldata segment
  -u SYMBOL, --undefined SYMBOL
                              Start with undefined reference to SYMBOL
  --unique [=SECTION]         Don't merge input [SECTION | orphan] sections
  -Ur                         Build global constructor/destructor tables
  -v, --version               Print version information
  -V                          Print version and emulation information
  -x, --discard-all           Discard all local symbols
  -X, --discard-locals        Discard temporary local symbols (default)
  --warn-common               Warn about duplicate common symbols
  --warn-constructors         Warn if global constructors/destructors are seen
  --warn-multiple-gp          Warn if the multiple GP values are used
  --warn-once                 Warn only once per undefined symbol
  --warn-section-align        Warn if start of section changes due to alignment
  --warn-textrel              Warn if outputting a DT_TEXTREL
  --warn-alternate-em         Warn if an object has alternate ELF machine code
  --warning-unresolved-symbols
                              Report unresolved symbols as warnings
  --whole-archive             Include all objects from following archives
  --wrap SYMBOL               Use wrapper functions for SYMBOL
  @FILE                       Read options from FILE
";

/// Opções com argumento obrigatório, escritas sem os hífens (formas curta e longa).
const ARG_NAMES: &[&str] = &[
    "a", "A", "b", "c", "e", "f", "F", "G", "h", "I", "l", "L", "m", "o", "T", "u", "y", "Y", "z",
    "R", "P", "architecture", "format", "mri-script", "dependency-file", "entry", "auxiliary",
    "filter", "gpsize", "soname", "dynamic-linker", "sort-section", "spare-dynamic-tags",
    "task-link", "section-start", "Tbss", "Tdata", "Ttext", "Ttext-segment", "Trodata-segment",
    "Tldata-segment", "undefined", "wrap", "defsym", "rpath", "rpath-link", "Map", "version-script",
    "script", "library", "library-path", "output", "trace-symbol", "just-symbols", "hash-style",
    "dynamic-list", "export-dynamic-symbol", "export-dynamic-symbol-list", "assert", "init",
    "fini", "plugin", "plugin-opt", "audit", "depaudit", "emulation", "default-script",
    "image-base", "max-cache-size", "split-by-file-size", "demangle-style", "orphan-handling",
    "print-map-discarded", "retain-symbols-file", "symbol-ordering-file", "no-warn-mismatch-x",
    "sysroot-x", "section-ordering-file", "undefined-version-x",
];

/// Opções sem argumento obrigatório reconhecidas (nomes sem hífens).
const FLAG_NAMES: &[&str] = &[
    "d", "dc", "dp", "E", "EB", "EL", "g", "i", "M", "N", "n", "q", "r", "s", "S", "t", "x", "X",
    "Ur", "Z", "O", "Qy", "Qn", "export-dynamic", "no-export-dynamic", "shared", "static",
    "dynamic", "pie", "pic-executable", "no-pie", "relocatable", "strip-all", "strip-debug",
    "discard-all", "discard-locals", "trace", "nostdlib", "start-group", "end-group", "as-needed",
    "no-as-needed", "whole-archive", "no-whole-archive", "push-state", "pop-state", "gc-sections",
    "no-gc-sections", "print-gc-sections", "no-print-gc-sections", "eh-frame-hdr",
    "no-eh-frame-hdr", "warn-common", "warn-constructors", "warn-multiple-gp", "warn-once",
    "warn-section-align", "warn-textrel", "warn-alternate-em", "warn-unresolved-symbols",
    "warning-unresolved-symbols", "error-unresolved-symbols", "no-undefined", "allow-shlib-undefined",
    "no-allow-shlib-undefined", "allow-multiple-definition", "fatal-warnings", "no-fatal-warnings",
    "stats", "target-help", "traditional-format", "cref", "verbose", "print-map", "nmagic",
    "omagic", "no-omagic", "Bstatic", "Bdynamic", "Bsymbolic", "Bsymbolic-functions", "Bgroup",
    "Bshareable", "dn", "dy", "call_shared", "non_shared", "no-dynamic-linker", "no-keep-memory",
    "no-define-common", "force-group-allocation", "sort-common", "split-by-file",
    "split-by-reloc", "unique", "build-id", "compress-debug-sections", "no-demangle", "demangle",
    "enable-new-dtags", "disable-new-dtags", "fix-cortex-a53-835769", "relax", "no-relax",
    "check-sections", "no-check-sections", "accept-unknown-input-arch",
    "no-accept-unknown-input-arch", "no-warn-mismatch", "no-warn-rwx-segments",
    "warn-rwx-segments", "no-warn-execstack", "warn-execstack", "no-undefined-version",
    "undefined-version", "export-dynamic-symbol", "no-ctf-variables", "ctf-variables",
    "ctf-share-types", "emit-relocs", "embedded-relocs", "rosegment", "no-rosegment",
    "no-ld-generated-unwind-info", "ld-generated-unwind-info", "dependency-file-x", "Ur",
    "sysroot", "version", "help", "no-copy-dt-needed-entries", "copy-dt-needed-entries",
    "no-add-needed", "add-needed", "dll-verbose", "no-print-map-discarded",
    "discard-none", "gdb-index", "no-gdb-index", "z-x", "ignore-unresolved-symbol",
    "unresolved-symbols", "print-output-format", "print-sysroot", "no-fatal-warnings-x",
    "relax-x", "nostartfiles", "nodefaultlibs", "no-whole-archive-x", "start-lib", "end-lib",
    "default-symver", "default-imported-symver", "no-ctf-strings", "ctf-strings",
    "dynamic-linker-x", "exclude-libs", "no-undefined-x", "pic-veneer", "mmap-output",
    "no-mmap-output", "reduce-memory-overheads", "section-ordering", "ifunc-x", "sort-section-x",
    "Map-x", "no-keep-memory-x", "no-print-gc-sections-x", "oformat", "script-x", "auxiliary-x",
    "bank-window", "stub-group-size", "target1-rel", "target1-abs", "target2",
];

pub fn main(_ctx: &mut sysabi::Ctx, args: &[OsString]) -> i32 {
    io::run(|| run(args))
}

fn out(text: &str) {
    let mut o = io::stdout();
    let _ = o.write_all(text.as_bytes());
}

fn help(prog: &str) {
    let mut t = format!("Usage: {prog} [options] file...\nOptions:\n{HELP_HEAD}");
    t.push_str(&format!(
        "{prog}: supported targets: elf64-x86-64 elf32-x86-64 pei-x86-64 pe-bigobj-x86-64 pe-x86-64 elf64-l1om elf64-k1om elf64-little elf64-big elf32-i386 elf32-iamcu pe-i386 pei-i386 elf32-little elf32-big plugin srec symbolsrec verilog tekhex binary ihex\n"
    ));
    t.push_str(&format!(
        "{prog}: supported emulations: {}\n",
        EMULATIONS.join(" ")
    ));
    t.push_str("Report bugs to <https://sourceware.org/bugzilla/>\n");
    out(&t);
}

fn bad_usage(prog: &str) -> i32 {
    io::eprint(format!(
        "{prog}: use the --help option for usage information\n"
    ));
    1
}

/// Procura `-lNAME` ou `-l:FILE` nos diretórios `-L` e nos padrões do x86_64.
fn find_library(spec: &str, dirs: &[String], prefer_static: bool) -> bool {
    const DEFAULTS: &[&str] = &[
        "/usr/local/lib/x86_64-linux-gnu",
        "/lib/x86_64-linux-gnu",
        "/usr/lib/x86_64-linux-gnu",
        "/usr/lib/x86_64-linux-gnu64",
        "/usr/local/lib64",
        "/lib64",
        "/usr/lib64",
        "/usr/local/lib",
        "/lib",
        "/usr/lib",
        "/usr/x86_64-linux-gnu/lib64",
        "/usr/x86_64-linux-gnu/lib",
    ];
    let names: Vec<String> = if let Some(exact) = spec.strip_prefix(':') {
        vec![exact.to_string()]
    } else if prefer_static {
        vec![format!("lib{spec}.a")]
    } else {
        vec![format!("lib{spec}.so"), format!("lib{spec}.a")]
    };
    let all = dirs
        .iter()
        .map(String::as_str)
        .chain(DEFAULTS.iter().copied());
    for d in all {
        for n in &names {
            if sys::stat(format!("{d}/{n}").as_bytes()).is_ok() {
                return true;
            }
        }
    }
    false
}

fn run(args: &[OsString]) -> i32 {
    let prog = io::argv0(args);
    let argv = io::args_bytes(args);
    // Entradas na ordem da linha de comando: (é -l, texto).
    let mut inputs: Vec<(bool, String)> = Vec::new();
    let mut dirs: Vec<String> = Vec::new();
    let mut prefer_static = false;
    let mut printed_version = false;
    let mut i = 1;
    while i < argv.len() {
        let raw = String::from_utf8_lossy(&argv[i]).into_owned();
        i += 1;
        if raw == "-" || !raw.starts_with('-') {
            inputs.push((false, raw));
            continue;
        }
        let double = raw.starts_with("--");
        let body = raw.trim_start_matches('-');
        let (name, value) = match body.split_once('=') {
            Some((n, v)) => (n, Some(v.to_string())),
            None => (body, None),
        };
        match name {
            "help" if double => {
                help(&prog);
                return 0;
            }
            "version" if double => {
                out(VERSION_LINE);
                out(VERSION_REST);
                return 0;
            }
            "v" if !double => {
                out(VERSION_LINE);
                printed_version = true;
                continue;
            }
            "V" if !double => {
                out(VERSION_LINE);
                out("  Supported emulations:\n");
                for e in EMULATIONS {
                    out(&format!("   {e}\n"));
                }
                printed_version = true;
                continue;
            }
            "Bstatic" | "static" | "dn" | "non_shared" => prefer_static = true,
            "Bdynamic" | "dy" | "call_shared" | "dynamic" => prefer_static = false,
            _ => {}
        }
        // Forma curta com valor anexado (-lc, -L/dir, -ofoo, -zrelro).
        let short_attached = !double
            && body.len() > 1
            && body.is_char_boundary(1)
            && !ARG_NAMES.contains(&name)
            && !FLAG_NAMES.contains(&name)
            && ARG_NAMES.contains(&&body[..1]);
        if short_attached {
            let (k, v) = body.split_at(1);
            match k {
                "l" => inputs.push((true, v.to_string())),
                "L" => dirs.push(v.to_string()),
                _ => {}
            }
            continue;
        }
        if ARG_NAMES.contains(&name) {
            let val = match value {
                Some(v) => v,
                None => {
                    if i >= argv.len() {
                        if double || name.len() > 1 {
                            io::eprint(format!(
                                "{prog}: option '{}' requires an argument\n",
                                if double {
                                    format!("--{name}")
                                } else {
                                    format!("-{name}")
                                }
                            ));
                        } else {
                            io::eprint(format!(
                                "{prog}: option requires an argument -- '{name}'\n"
                            ));
                        }
                        return bad_usage(&prog);
                    }
                    let v = String::from_utf8_lossy(&argv[i]).into_owned();
                    i += 1;
                    v
                }
            };
            match name {
                "l" | "library" => inputs.push((true, val)),
                "L" | "library-path" => dirs.push(val),
                _ => {}
            }
            continue;
        }
        if FLAG_NAMES.contains(&name) {
            continue;
        }
        // `-Bsymbolic...`, `-O1`, `-plugin-opt=...`, `-z` agrupados e afins.
        if !double && (body.starts_with('B') || body.starts_with('O') || body.starts_with("plugin")) {
            continue;
        }
        io::eprint(format!("{prog}: unrecognized option '{raw}'\n"));
        return bad_usage(&prog);
    }
    if inputs.is_empty() {
        if printed_version {
            return 0;
        }
        io::eprint(format!("{prog}: no input files\n"));
        return 1;
    }
    let mut failed = false;
    for (is_lib, text) in &inputs {
        if *is_lib {
            if !find_library(text, &dirs, prefer_static) {
                io::eprint(format!(
                    "{prog}: cannot find -l{text}: No such file or directory\n"
                ));
                failed = true;
            }
            continue;
        }
        match File::open(text.as_bytes()) {
            Err(e) => {
                io::eprint(format!("{prog}: cannot find {text}: {}\n", e.message()));
                failed = true;
            }
            Ok(mut f) => {
                let mut head = [0u8; 8];
                let n = f.read_full(&mut head).unwrap_or(0);
                let known = n >= 4 && (&head[..4] == b"\x7fELF" || head[..n].starts_with(b"!<arch>"));
                if n > 0 && !known {
                    io::eprint(format!(
                        "{prog}: {text}: file format not recognized; treating as linker script\n{prog}: {text}:1: syntax error\n"
                    ));
                    failed = true;
                }
            }
        }
    }
    if failed {
        return 1;
    }
    // TODO: não há ligador; nenhuma saída é criada.
    io::eprint(format!(
        "{prog}: linking is not supported by this implementation\n"
    ));
    1
}
