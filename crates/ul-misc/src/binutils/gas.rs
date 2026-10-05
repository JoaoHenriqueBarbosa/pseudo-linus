//! `as` (GNU assembler) do binutils 2.44 (Debian 13), x86_64-linux-gnu.
//!
//! Escrito a partir do comportamento observado e da man page (o código do binutils é GPL e não
//! foi consultado). Cobre `--help`, `--version`, `-v`, validação de opções, a mensagem de arquivo
//! de entrada inexistente e os códigos de saída.
//!
//! Limitação documentada: não há montador. Para entrada legível o programa responde com um erro
//! fatal explícito e sai com 1, sem criar o objeto de saída. O texto de `--help` foi reproduzido de
//! memória e pode divergir em espaçamento do original.

use std::ffi::OsString;
use std::io::Write;

use crate::util::io::{self, File};

const VERSION_TEXT: &str = "GNU assembler (GNU Binutils for Debian) 2.44\n\
Copyright (C) 2025 Free Software Foundation, Inc.\n\
This program is free software; you may redistribute it under the terms of\n\
the GNU General Public License version 3 or later.\n\
This program has absolutely no warranty.\n\
This assembler was configured for a target of `x86_64-linux-gnu'.\n";

const VERBOSE_TEXT: &str =
    "GNU assembler version 2.44 (x86_64-linux-gnu) using BFD version (GNU Binutils for Debian) 2.44\n";

const HELP_BODY: &str = "Options:
  -a[sub-option...]	  turn on listings
		      Sub-options [default hls]:
		      c      omit false conditionals
		      d      omit debugging directives
		      g      include general info
		      h      include high-level source
		      l      include assembly
		      m      include macro expansions
		      n      omit forms processing
		      s      include symbols
		      =FILE  list to FILE (must be last sub-option)
  --alternate             initially turn on alternate macro syntax
  --compress-debug-sections[={none|zlib|zlib-gnu|zlib-gabi|zstd}]
		          compress DWARF debug sections
  --nocompress-debug-sections
		          don't compress DWARF debug sections
  -D                      produce assembler debugging messages
  --debug-prefix-map OLD=NEW
                          map OLD to NEW in debug information
  --defsym SYM=VAL        define symbol SYM to given value
  --elf-stt-common=[no|yes]
                          generate ELF common symbols with STT_COMMON type
  --sectname-subst        enable section name substitution sequences
  -f                      skip whitespace and comment preprocessing
  -g --gen-debug          generate debugging information
  --gstabs                generate STABS debugging information
  --gstabs+               generate STABS debug info with GNU extensions
  --gdwarf-<N>            generate DWARF<N> debugging information. 2 <= <N> <= 5
  --gdwarf-sections       generate per-function section names for DWARF line information
  --hash-size=<N>         ignored
  --help                  show this message and exit
  --target-help           show target specific options
  -I DIR                  add DIR to search list for .include directives
  -J                      don't warn about signed overflow
  -K                      warn when differences altered for long displacements
  -L,--keep-locals        keep local symbols (e.g. starting with `L')
  -M,--mri                assemble in MRI compatibility mode
  --MD FILE               write dependency information in FILE (default none)
  -nocpp                  ignored
  -no-pad-sections        do not pad the end of sections to alignment boundaries
  -o OBJFILE              name the object-file output OBJFILE (default a.out)
  -R                      fold data section into text section
  --reduce-memory-overheads
                          prefer smaller memory use at the cost of longer
                          assembly times
  --statistics            print various measured statistics from execution
  --strip-local-absolute  strip local absolute symbols
  --traditional-format    Use same format as native assembler when possible
  --version               print assembler version number and exit
  -W  --no-warn           suppress warnings
  --warn                  don't suppress warnings
  --fatal-warnings        treat warnings as errors
  -w                      ignored
  -X                      ignored
  -Z                      generate object file even after errors
  --listing-lhs-width     set the width in words of the output data column of
                          the listing
  --listing-lhs-width2    set the width in words of the continuation lines
                          of the output data column; ignored if smaller than
                          the width of the first line
  --listing-rhs-width     set the max width in characters of the lines from
                          the source file
  --listing-cont-lines    set the maximum number of continuation lines used
                          for the output data column of the listing
  @FILE                   read options from FILE
  -n                      Do not optimize code alignment
  -q                      quieten some warnings
  --32/--64/--x32         generate 32bit/64bit/x32 object
  -march=CPU[,+EXTENSION...]
                          generate code for CPU and EXTENSION, CPU is one of:
                           generic32, generic64, i386, i486, i586, i686,
                           pentium, pentiumpro, pentiumii, pentiumiii, pentium4,
                           prescott, nocona, core, core2, corei7, l1om, k1om,
                           iamcu, k6, k6_2, athlon, opteron, k8, amdfam10,
                           bdver1, bdver2, bdver3, bdver4, znver1, znver2,
                           znver3, znver4, znver5, btver1, btver2
  -mtune=CPU              optimize for CPU, CPU is one of the above
  -msse2avx               encode SSE instructions with VEX prefix
  -msse-check=[none|error|warning] (default: warning)
                          check SSE instructions
  -moperand-check=[none|error|warning] (default: warning)
                          check operand combinations for validity
  -mavxscalar=[128|256] (default: 128)
                          encode scalar AVX instructions with specific vector
                           length
  -mvexwig=[0|1] (default: 0)
                          encode VEX instructions with specific VEX.W value
                           for VEX.W bit ignored instructions
  -mevexlig=[128|256|512] (default: 128)
                          encode scalar EVEX instructions with specific vector
                           length
  -mevexwig=[0|1] (default: 0)
                          encode EVEX instructions with specific EVEX.W value
                           for EVEX.W bit ignored instructions
  -mevexrcig=[rne|rd|ru|rz] (default: rne)
                          encode EVEX instructions with specific EVEX.RC value
                           for SAE-only ignored instructions
  -mmnemonic=[att|intel] (default: att)
                          use AT&T/Intel mnemonic
  -msyntax=[att|intel] (default: att)
                          use AT&T/Intel syntax
  -mindex-reg             support pseudo index registers
  -mnaked-reg             don't require `%' prefix for registers
  -madd-bnd-prefix        add BND prefix for all valid branches
  -mshared                disable branch optimization for shared code
  -mx86-used-note=[no|yes] generate x86 used ISA and feature properties
  -mbig-obj               generate big object files
  -momit-lock-prefix=[no|yes] (default: no)
                          strip all lock prefixes
  -mfence-as-lock-add=[no|yes] (default: no)
                          encode lfence, mfence and sfence as
                           lock addl $0x0, (%{re}sp)
  -mrelax-relocations=[no|yes] (default: yes)
                          generate relax relocations
  -malign-branch-boundary=NUM (default: 0)
                          align branches within NUM byte boundary
  -malign-branch=TYPE[+TYPE...] (default: jcc+fused+nojcc)
                          TYPE is combination of jcc, fused, jmp, call, ret,
                           indirect
                          specify types of branches to align
  -malign-branch-prefix-size=NUM (default: 5)
                          align branches with NUM prefixes per instruction
  -mbranches-within-32B-boundaries
                          align branches within 32 byte boundary
  -mlfence-after-load=[no|yes] (default: no)
                          generate lfence after load
  -mlfence-before-indirect-branch=[none|all|register|memory] (default: none)
                          generate lfence before indirect near branch
  -mlfence-before-ret=[none|or|not|shl|yes] (default: none)
                          generate lfence before ret
  -mamd64                 accept only AMD64 ISA [default]
  -mintel64               accept only Intel64 ISA
Report bugs to <https://sourceware.org/bugzilla/>
";

/// Opções que consomem o argumento seguinte quando ele não vem após `=`.
const SEPARATE_ARG: &[&str] = &[
    "-o",
    "-I",
    "-MD",
    "--MD",
    "-defsym",
    "--defsym",
    "-debug-prefix-map",
    "--debug-prefix-map",
    "-listing-lhs-width",
    "--listing-lhs-width",
    "-listing-lhs-width2",
    "--listing-lhs-width2",
    "-listing-rhs-width",
    "--listing-rhs-width",
    "-listing-cont-lines",
    "--listing-cont-lines",
    "--hash-size",
    "-hash-size",
];

/// Opções que o montador reconhece (sem o argumento anexado).
const KNOWN_EXACT: &[&str] = &[
    "-alternate", "--alternate", "-sectname-subst", "--sectname-subst", "-D", "-f", "-J", "-K",
    "-L", "--keep-locals", "-keep-locals", "-M", "--mri", "-mri", "-nocpp", "--nocpp",
    "-no-pad-sections", "--no-pad-sections", "-R", "-reduce-memory-overheads",
    "--reduce-memory-overheads", "-statistics", "--statistics", "-strip-local-absolute",
    "--strip-local-absolute", "-traditional-format", "--traditional-format", "-W", "--no-warn",
    "-no-warn", "-warn", "--warn", "-fatal-warnings", "--fatal-warnings", "-w", "-X", "-Z", "-n",
    "-q", "-s", "-k", "--32", "--64", "--x32", "-32", "-64", "-x32", "-nocompress-debug-sections",
    "--nocompress-debug-sections", "-gdwarf-sections", "--gdwarf-sections", "-gen-debug",
    "--gen-debug", "-gstabs", "--gstabs", "-gstabs+", "--gstabs+", "-gdwarf", "-I", "-o", "-Qy",
    "-Qn",
];

/// Prefixos de opções reconhecidas com valor ou sufixo anexado.
const KNOWN_PREFIX: &[&str] = &[
    "-a", "-m", "-g", "-O", "-Q", "-I", "-o", "-MD", "--MD", "-defsym", "--defsym",
    "-compress-debug-sections", "--compress-debug-sections", "-elf-stt-common",
    "--elf-stt-common", "-debug-prefix-map", "--debug-prefix-map", "-hash-size", "--hash-size",
    "-listing-", "--listing-", "-gdwarf-", "--gdwarf-", "-march=", "-mtune=",
];

pub fn main(_ctx: &mut sysabi::Ctx, args: &[OsString]) -> i32 {
    io::run(|| run(args))
}

fn out(text: &str) {
    let mut o = io::stdout();
    let _ = o.write_all(text.as_bytes());
}

fn is_known(opt: &str) -> bool {
    KNOWN_EXACT.contains(&opt) || KNOWN_PREFIX.iter().any(|p| opt.starts_with(p))
}

fn run(args: &[OsString]) -> i32 {
    let prog = io::argv0(args);
    let argv = io::args_bytes(args);
    let mut files: Vec<Vec<u8>> = Vec::new();
    let mut i = 1;
    while i < argv.len() {
        let a = &argv[i];
        i += 1;
        if a == b"-" {
            files.push(a.clone());
            continue;
        }
        if a == b"--" {
            files.extend(argv[i..].iter().cloned());
            break;
        }
        if a.first() != Some(&b'-') {
            files.push(a.clone());
            continue;
        }
        let text = String::from_utf8_lossy(a).into_owned();
        match text.as_str() {
            "--help" | "-help" => {
                out(&format!("Usage: {prog} [option...] [asmfile...]\n{HELP_BODY}"));
                return 0;
            }
            "--target-help" | "-target-help" => {
                out("x86-64 options:\n  -32 | -64 | -x32        generate 32bit/64bit/x32 object\n");
                return 0;
            }
            "--version" | "-version" => {
                out(VERSION_TEXT);
                return 0;
            }
            "-v" | "--v" | "-V" => {
                io::eprint(VERBOSE_TEXT);
                continue;
            }
            _ => {}
        }
        if SEPARATE_ARG.contains(&text.as_str()) {
            if i >= argv.len() {
                io::eprint(format!("{prog}: option requires an argument -- '{}'\n", &text[1..]));
                return 1;
            }
            i += 1;
            continue;
        }
        if !is_known(&text) {
            io::eprint(format!("{prog}: unrecognized option '{text}'\n"));
            return 1;
        }
    }
    if files.is_empty() {
        files.push(b"-".to_vec());
    }
    let mut failed = false;
    for f in &files {
        if f == b"-" {
            continue;
        }
        if let Err(e) = File::open(f) {
            io::eprint(format!(
                "{prog}: can't open {} for reading: {}\n",
                io::lossy(f),
                e.message()
            ));
            failed = true;
        }
    }
    if failed {
        return 1;
    }
    // TODO: não há montador x86_64; sem objeto de saída.
    io::eprint(format!(
        "{prog}: Fatal error: assembling is not supported by this implementation\n"
    ));
    1
}
