//! `lesskey` do less 668 (pacote less do Debian 13): compila o arquivo de fonte (`~/.lesskey`, ou o
//! argumento) para o formato binário lido pelo `less` (`~/.less`, ou o `-o`).
//!
//! Formato de saída: cabeçalho `\0M+G`, depois as seções `c` (comandos), `e` (edição de linha) e
//! `v` (variáveis de ambiente), cada uma com o comprimento em dois bytes (menos significativo
//! primeiro), e o marcador final `End`.
//!
//! Fora do escopo desta versão: a tabela de nomes de ação (`ACTIONS`) está vazia, porque os
//! números vêm do `cmd.h` do less e não foram conferidos contra o oráculo; por isso as linhas
//! das seções `#command` e `#line-edit` que nomeiam uma ação dão erro. Também não há o `+=` do
//! `#env` nem a busca em `XDG_CONFIG_HOME`.

use std::ffi::OsString;
use std::io::{Read, Write};
use std::os::unix::ffi::{OsStrExt, OsStringExt};

use sysabi::Ctx;

use crate::util::io;

const USAGE: &str = "usage: lesskey [-o output] [input]\n";
const MAGIC: &[u8] = b"\0M+G";
const END: &[u8] = b"End";
const A_EXTRA: u8 = 0x80;
const EV_OK: u8 = 0x01;
/// Prefixo das teclas especiais (`\kX`), o `CONTROL('K')` do original.
const SK_SPECIAL_KEY: u8 = 0x0b;

/// Nome da ação e seu número em `cmd.h`. Vazia até a conferência com o oráculo.
const ACTIONS: &[(&str, u8)] = &[];

pub fn main(_ctx: &mut Ctx, args: &[OsString]) -> i32 {
    io::run(|| run(args))
}

#[derive(PartialEq, Clone, Copy)]
enum Section {
    Command,
    Edit,
    Env,
}

fn usage() -> i32 {
    io::eprint(USAGE.to_string());
    1
}

fn special_key(c: u8) -> Option<u8> {
    Some(match c {
        b'r' => 1,
        b'l' => 2,
        b'u' => 3,
        b'd' => 4,
        b'P' => 5,
        b'N' => 6,
        b'h' => 7,
        b'e' => 8,
        b'x' => 9,
        _ => return None,
    })
}

/// Lê uma sequência de teclas ou texto extra com escapes, parando no primeiro espaço quando
/// `stop_at_space`. Devolve os bytes e o índice onde parou.
fn parse_chars(s: &[u8], mut i: usize, stop_at_space: bool) -> Result<(Vec<u8>, usize), String> {
    let mut out = Vec::new();
    while i < s.len() {
        let c = s[i];
        if stop_at_space && (c == b' ' || c == b'\t') {
            break;
        }
        i += 1;
        match c {
            b'\\' => {
                let Some(&e) = s.get(i) else {
                    return Err("missing char after \\".to_string());
                };
                i += 1;
                match e {
                    b'b' => out.push(8),
                    b'e' => out.push(27),
                    b'n' => out.push(b'\n'),
                    b'r' => out.push(b'\r'),
                    b't' => out.push(b'\t'),
                    b'k' => {
                        let Some(&k) = s.get(i) else {
                            return Err("missing char after \\k".to_string());
                        };
                        i += 1;
                        match special_key(k) {
                            Some(code) => {
                                out.push(SK_SPECIAL_KEY);
                                out.push(code);
                            }
                            None => return Err("illegal char after \\k".to_string()),
                        }
                    }
                    b'0'..=b'7' => {
                        let mut v = (e - b'0') as u32;
                        let mut n = 1;
                        while n < 3 && matches!(s.get(i), Some(b'0'..=b'7')) {
                            v = v * 8 + (s[i] - b'0') as u32;
                            i += 1;
                            n += 1;
                        }
                        out.push(v as u8);
                    }
                    other => out.push(other),
                }
            }
            b'^' => {
                let Some(&x) = s.get(i) else {
                    return Err("missing char after ^".to_string());
                };
                i += 1;
                out.push(if x == b'?' { 0x7f } else { x & 0x1f });
            }
            _ => out.push(c),
        }
    }
    Ok((out, i))
}

fn skip_ws(s: &[u8], mut i: usize) -> usize {
    while matches!(s.get(i), Some(b' ' | b'\t')) {
        i += 1;
    }
    i
}

/// Uma linha de comando: `teclas ação [extra]`.
fn parse_command_line(line: &[u8], table: &mut Vec<u8>) -> Result<(), String> {
    let (keys, i) = parse_chars(line, 0, true)?;
    let j = skip_ws(line, i);
    if j == i {
        return Err("missing whitespace".to_string());
    }
    let mut k = j;
    while k < line.len() && !matches!(line[k], b' ' | b'\t') {
        k += 1;
    }
    let name = &line[j..k];
    let action = ACTIONS
        .iter()
        .find(|(n, _)| n.as_bytes() == name)
        .map(|&(_, v)| v)
        .ok_or_else(|| format!("unknown action: \"{}\"", String::from_utf8_lossy(name)))?;
    let m = skip_ws(line, k);
    let mut end = line.len();
    while end > m && matches!(line[end - 1], b' ' | b'\t') {
        end -= 1;
    }
    table.extend_from_slice(&keys);
    table.push(0);
    if m < end {
        let (extra, _) = parse_chars(&line[..end], m, false)?;
        table.push(action | A_EXTRA);
        table.extend_from_slice(&extra);
        table.push(0);
    } else {
        table.push(action);
    }
    Ok(())
}

/// Uma linha de `#env`: `NOME = valor`.
fn parse_env_line(line: &[u8], table: &mut Vec<u8>) -> Result<(), String> {
    let Some(eq) = line.iter().position(|&b| b == b'=') else {
        return Err("missing =".to_string());
    };
    let mut ne = eq;
    while ne > 0 && matches!(line[ne - 1], b' ' | b'\t') {
        ne -= 1;
    }
    let name = &line[..ne];
    if name.is_empty() {
        return Err("missing variable name".to_string());
    }
    let value = &line[skip_ws(line, eq + 1)..];
    table.push(EV_OK | A_EXTRA);
    table.extend_from_slice(name);
    table.push(0);
    table.extend_from_slice(value);
    table.push(0);
    Ok(())
}

fn push_section(out: &mut Vec<u8>, tag: u8, data: &[u8]) {
    out.push(tag);
    out.push((data.len() & 0xff) as u8);
    out.push(((data.len() >> 8) & 0xff) as u8);
    out.extend_from_slice(data);
}

/// Compila o texto-fonte; `Err` traz a mensagem já com o número da linha.
fn compile(src: &[u8]) -> Result<Vec<u8>, String> {
    let mut cmd = Vec::new();
    let mut edit = Vec::new();
    let mut env = Vec::new();
    let mut section = Section::Command;
    for (n, raw) in src.split(|&b| b == b'\n').enumerate() {
        let mut line = raw;
        while let [rest @ .., b'\r' | b' ' | b'\t'] = line {
            if line.last() == Some(&b'\r') || section != Section::Env {
                line = rest;
            } else {
                break;
            }
        }
        if line.is_empty() {
            continue;
        }
        if line[0] == b'#' {
            if line == b"#command" {
                section = Section::Command;
            } else if line == b"#line-edit" {
                section = Section::Edit;
            } else if line == b"#env" {
                section = Section::Env;
            }
            continue;
        }
        let res = match section {
            Section::Command => parse_command_line(line, &mut cmd),
            Section::Edit => parse_command_line(line, &mut edit),
            Section::Env => parse_env_line(line, &mut env),
        };
        if let Err(m) = res {
            return Err(format!("line {}: {}", n + 1, m));
        }
    }
    let mut out = MAGIC.to_vec();
    push_section(&mut out, b'c', &cmd);
    push_section(&mut out, b'e', &edit);
    push_section(&mut out, b'v', &env);
    out.extend_from_slice(END);
    Ok(out)
}

fn run(args: &[OsString]) -> i32 {
    let argv = io::args_bytes(args);
    let mut output: Option<Vec<u8>> = None;
    let mut idx = 1;
    while idx < argv.len() {
        let a = &argv[idx];
        if a.first() != Some(&b'-') || a.len() == 1 {
            break;
        }
        idx += 1;
        if a == b"--" {
            break;
        } else if a == b"-V" || a == b"--version" {
            let mut out = io::stdout();
            let _ = out.write_all(b"lesskey  version 668\n");
            return if out.flush().is_err() { 1 } else { 0 };
        } else if a == b"--help" {
            io::eprint(USAGE.to_string());
            return 0;
        } else if a == b"-o" || a == b"--output" {
            let Some(v) = argv.get(idx) else {
                return usage();
            };
            output = Some(v.clone());
            idx += 1;
        } else if let Some(v) = a.strip_prefix(b"--output=") {
            output = Some(v.to_vec());
        } else if let Some(v) = a.strip_prefix(b"-o") {
            output = Some(v.to_vec());
        } else {
            return usage();
        }
    }
    if argv.len() - idx > 1 {
        return usage();
    }
    let home = std::env::var_os("HOME").map(|h| h.into_vec()).unwrap_or_default();
    let input: Vec<u8> = match argv.get(idx) {
        Some(p) => p.clone(),
        None => [home.as_slice(), b"/.lesskey"].concat(),
    };
    let output: Vec<u8> = output.unwrap_or_else(|| [home.as_slice(), b"/.less"].concat());

    let mut src = Vec::new();
    if input == b"-" {
        let _ = std::io::stdin().read_to_end(&mut src);
    } else {
        let path = std::ffi::OsStr::from_bytes(&input);
        match std::fs::read(path) {
            Ok(d) => src = d,
            Err(_) => {
                io::eprint(format!("cannot open {}\n", String::from_utf8_lossy(&input)));
                return 1;
            }
        }
    }
    let bin = match compile(&src) {
        Ok(b) => b,
        Err(m) => {
            io::eprint(format!("{m}\n"));
            return 1;
        }
    };
    if output == b"-" {
        let mut out = io::stdout();
        let _ = out.write_all(&bin);
        return if out.flush().is_err() { 1 } else { 0 };
    }
    let path = std::ffi::OsStr::from_bytes(&output);
    if std::fs::write(path, &bin).is_err() {
        io::eprint(format!("cannot open {}\n", String::from_utf8_lossy(&output)));
        return 1;
    }
    0
}
