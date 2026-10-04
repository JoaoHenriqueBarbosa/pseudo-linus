// Porte pseudo-linus: módulo novo (não existe no uutils).
//
//! Linha de comando com a semântica do `getopt_long` da glibc sobre a definição clap de um
//! utilitário.
//!
//! O clap aceita e recusa coisas diferentes do `getopt_long`, e diz isso com outras palavras:
//! `tail -n` sem valor, `cut -d -f1` (o getopt pega `-f1` como valor de `-d`), `--quiet=3`,
//! abreviação ambígua (`--s`), opção desconhecida. Aqui a linha é varrida como o getopt faz
//! (permutação, `--`, `-` sozinho, agrupamento de curtas, valor grudado ou no argumento seguinte,
//! abreviação de longa, `POSIXLY_CORRECT`), os erros saem com o texto exato da glibc seguidos de
//! `Try 'prog --help' for more information.`, e o clap recebe uma linha normalizada, sem
//! ambiguidade: cada opção na forma `--longa=valor` (ou curta com o valor grudado), na ordem
//! original, depois `--` e os operandos.
//!
//! As ajudas automáticas do clap (`-h`, `-V`) não existem no GNU: só `--help` e `--version`.

use std::ffi::{OsStr, OsString};
use std::io::Write as _;
#[cfg(unix)]
use std::os::unix::ffi::{OsStrExt, OsStringExt};

use clap::{Arg, ArgAction, ArgMatches, Command};

use crate::error::{UResult, USimpleError};

/// Como uma opção recebe valor (o `has_arg` do `struct option`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HasArg {
    /// `no_argument`
    No,
    /// `required_argument`
    Required,
    /// `optional_argument` (só grudado: `-xVAL`, `--opt=VAL`)
    Optional,
}

/// Uma opção da tabela, tirada de um `clap::Arg`.
#[derive(Clone, Debug)]
struct OptSpec {
    /// Id do argumento no clap (opções com o mesmo id são a mesma opção, como aliases).
    id: String,
    shorts: Vec<char>,
    longs: Vec<String>,
    has_arg: HasArg,
    /// Nome longo canônico usado na linha normalizada.
    long: Option<String>,
    short: Option<char>,
}

/// Configuração da varredura.
#[derive(Clone, Debug)]
pub struct Config {
    /// Código de saída de erro de uso (`usage (EXIT_FAILURE)` é 1 na maioria).
    pub exit_code: i32,
    /// Parar na primeira não-opção (optstring com `+`, ou `POSIXLY_CORRECT` no ambiente).
    pub stop_at_first_operand: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            exit_code: 1,
            stop_at_first_operand: false,
        }
    }
}

/// Uma opção reconhecida na linha.
#[derive(Clone, Debug)]
pub struct ParsedOpt {
    /// Id do argumento no clap.
    pub id: String,
    /// Valor, se houver.
    pub value: Option<OsString>,
}

/// Resultado da varredura: opções na ordem e operandos.
#[derive(Clone, Debug, Default)]
pub struct Scanned {
    /// Opções na ordem em que apareceram.
    pub options: Vec<ParsedOpt>,
    /// Operandos (não opções), na ordem.
    pub operands: Vec<OsString>,
}

/// Erro de getopt: a mensagem exata da glibc (sem o prefixo `prog: `).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GetoptError(pub Vec<u8>);

fn bytes(s: &OsStr) -> &[u8] {
    #[cfg(unix)]
    {
        s.as_bytes()
    }
    #[cfg(not(unix))]
    {
        s.as_encoded_bytes()
    }
}

fn os_from(b: Vec<u8>) -> OsString {
    #[cfg(unix)]
    {
        OsString::from_vec(b)
    }
    #[cfg(not(unix))]
    {
        OsString::from(String::from_utf8_lossy(&b).into_owned())
    }
}

fn has_arg_of(arg: &Arg) -> HasArg {
    let takes = match arg.get_action() {
        ArgAction::Set | ArgAction::Append => true,
        a => a.takes_values(),
    };
    if !takes {
        return HasArg::No;
    }
    match arg.get_num_args() {
        Some(r) if r.min_values() == 0 && r.max_values() == 0 => HasArg::No,
        Some(r) if r.min_values() == 0 => HasArg::Optional,
        _ => HasArg::Required,
    }
}

fn table(cmd: &Command) -> Vec<OptSpec> {
    let mut out = Vec::new();
    for arg in cmd.get_arguments() {
        if arg.is_positional() {
            continue;
        }
        let shorts = arg.get_all_short_aliases().unwrap_or_default();
        let shorts: Vec<char> = arg.get_short().into_iter().chain(shorts).collect();
        let longs = arg.get_all_aliases().unwrap_or_default();
        let longs: Vec<String> = arg
            .get_long()
            .into_iter()
            .chain(longs)
            .map(str::to_string)
            .collect();
        if shorts.is_empty() && longs.is_empty() {
            continue;
        }
        out.push(OptSpec {
            id: arg.get_id().as_str().to_string(),
            long: arg.get_long().map(str::to_string),
            short: arg.get_short(),
            shorts,
            longs,
            has_arg: has_arg_of(arg),
        });
    }
    let defined = |name: &str| out.iter().any(|o| o.longs.iter().any(|l| l == name));
    let mut extra = Vec::new();
    if !cmd.is_disable_help_flag_set() && !defined("help") {
        extra.push(OptSpec {
            id: "help".into(),
            shorts: Vec::new(),
            longs: vec!["help".into()],
            has_arg: HasArg::No,
            long: Some("help".into()),
            short: None,
        });
    }
    if !cmd.is_disable_version_flag_set() && cmd.get_version().is_some() && !defined("version") {
        extra.push(OptSpec {
            id: "version".into(),
            shorts: Vec::new(),
            longs: vec!["version".into()],
            has_arg: HasArg::No,
            long: Some("version".into()),
            short: None,
        });
    }
    out.extend(extra);
    out
}

/// Varre `args` (sem o `argv[0]`) como o `getopt_long` da glibc, com as opções de `cmd`.
pub fn scan(cmd: &Command, args: &[OsString], config: &Config) -> Result<Scanned, GetoptError> {
    let specs = table(cmd);
    scan_specs(&specs, args, config)
}

fn quote_err(parts: &[&[u8]]) -> GetoptError {
    GetoptError(parts.concat())
}

fn scan_specs(specs: &[OptSpec], args: &[OsString], config: &Config) -> Result<Scanned, GetoptError> {
    let posix = config.stop_at_first_operand || sysio::env::var_os("POSIXLY_CORRECT").is_some();
    let mut out = Scanned::default();
    let mut i = 0;
    while i < args.len() {
        let a = bytes(&args[i]);
        if a == b"--" {
            out.operands.extend(args[i + 1..].iter().cloned());
            break;
        }
        if a.len() < 2 || a[0] != b'-' {
            if posix {
                out.operands.extend(args[i..].iter().cloned());
                break;
            }
            out.operands.push(args[i].clone());
            i += 1;
            continue;
        }
        if a[1] == b'-' {
            // Opção longa.
            let body = &a[2..];
            let (name, value) = match body.iter().position(|&b| b == b'=') {
                Some(p) => (&body[..p], Some(&body[p + 1..])),
                None => (body, None),
            };
            let spec = find_long(specs, name, body)?;
            let full = spec
                .longs
                .iter()
                .find(|l| l.as_bytes() == name)
                .cloned()
                .unwrap_or_else(|| {
                    // Abreviação: o nome mostrado nas mensagens é o da entrada que casou primeiro.
                    spec.longs
                        .iter()
                        .find(|l| l.as_bytes().starts_with(name))
                        .cloned()
                        .unwrap_or_default()
                });
            let value = match (spec.has_arg, value) {
                (HasArg::No, Some(_)) => {
                    return Err(quote_err(&[
                        b"option '--",
                        full.as_bytes(),
                        b"' doesn't allow an argument",
                    ]));
                }
                (HasArg::No, None) => None,
                (_, Some(v)) => Some(os_from(v.to_vec())),
                (HasArg::Optional, None) => None,
                (HasArg::Required, None) => {
                    if i + 1 < args.len() {
                        i += 1;
                        Some(args[i].clone())
                    } else {
                        return Err(quote_err(&[
                            b"option '--",
                            full.as_bytes(),
                            b"' requires an argument",
                        ]));
                    }
                }
            };
            out.options.push(ParsedOpt {
                id: spec.id.clone(),
                value,
            });
            i += 1;
            continue;
        }
        // Grupo de opções curtas. O getopt anda byte a byte.
        let mut j = 1;
        while j < a.len() {
            let c = a[j];
            let spec = specs
                .iter()
                .find(|s| s.shorts.iter().any(|&sc| sc.is_ascii() && sc as u8 == c));
            let Some(spec) = spec else {
                return Err(quote_err(&[b"invalid option -- '", &[c], b"'"]));
            };
            match spec.has_arg {
                HasArg::No => {
                    out.options.push(ParsedOpt {
                        id: spec.id.clone(),
                        value: None,
                    });
                    j += 1;
                }
                HasArg::Optional => {
                    let rest = &a[j + 1..];
                    out.options.push(ParsedOpt {
                        id: spec.id.clone(),
                        value: (!rest.is_empty()).then(|| os_from(rest.to_vec())),
                    });
                    break;
                }
                HasArg::Required => {
                    let rest = &a[j + 1..];
                    let value = if !rest.is_empty() {
                        os_from(rest.to_vec())
                    } else if i + 1 < args.len() {
                        i += 1;
                        args[i].clone()
                    } else {
                        return Err(quote_err(&[b"option requires an argument -- '", &[c], b"'"]));
                    };
                    out.options.push(ParsedOpt {
                        id: spec.id.clone(),
                        value: Some(value),
                    });
                    break;
                }
            }
        }
        i += 1;
    }
    Ok(out)
}

/// Acha a opção longa `name` (exata, ou abreviação sem ambiguidade). `shown` é o texto depois do
/// `--` como o usuário escreveu (com `=valor`), que é o que a glibc mostra nos erros de opção
/// desconhecida e ambígua.
fn find_long<'a>(specs: &'a [OptSpec], name: &[u8], shown: &[u8]) -> Result<&'a OptSpec, GetoptError> {
    if let Some(s) = specs
        .iter()
        .find(|s| s.longs.iter().any(|l| l.as_bytes() == name))
    {
        return Ok(s);
    }
    let mut matches: Vec<(&str, &OptSpec)> = Vec::new();
    if !name.is_empty() {
        for s in specs {
            for l in &s.longs {
                if l.as_bytes().starts_with(name) {
                    matches.push((l.as_str(), s));
                }
            }
        }
    }
    match matches.first() {
        None => Err(quote_err(&[b"unrecognized option '--", shown, b"'"])),
        Some((_, first)) => {
            if matches.iter().all(|(_, s)| s.id == first.id) {
                Ok(first)
            } else {
                // A glibc lista as entradas na ordem da tabela do programa; as tabelas do
                // coreutils são, na prática, alfabéticas.
                let mut names: Vec<&str> = matches.iter().map(|(n, _)| *n).collect();
                names.sort_unstable();
                names.dedup();
                let mut msg = Vec::new();
                msg.extend_from_slice(b"option '--");
                msg.extend_from_slice(shown);
                msg.extend_from_slice(b"' is ambiguous; possibilities:");
                for n in names {
                    msg.extend_from_slice(b" '--");
                    msg.extend_from_slice(n.as_bytes());
                    msg.push(b'\'');
                }
                Err(GetoptError(msg))
            }
        }
    }
}

/// Linha normalizada pro clap: `argv0`, as opções na ordem (longa com `=`, ou curta com o valor
/// grudado), `--` e os operandos.
pub fn normalize(cmd: &Command, argv0: OsString, scanned: &Scanned) -> Vec<OsString> {
    let specs = table(cmd);
    normalize_specs(&specs, argv0, scanned)
}

fn normalize_specs(specs: &[OptSpec], argv0: OsString, scanned: &Scanned) -> Vec<OsString> {
    let mut out = vec![argv0];
    for o in &scanned.options {
        let Some(spec) = specs.iter().find(|s| s.id == o.id) else {
            continue;
        };
        match (&spec.long, &o.value) {
            (Some(l), Some(v)) => {
                let mut b = Vec::with_capacity(l.len() + 3 + v.len());
                b.extend_from_slice(b"--");
                b.extend_from_slice(l.as_bytes());
                b.push(b'=');
                b.extend_from_slice(bytes(v));
                out.push(os_from(b));
            }
            (Some(l), None) => out.push(OsString::from(format!("--{l}"))),
            (None, value) => {
                let c = spec.short.unwrap_or(' ');
                match value {
                    None => out.push(OsString::from(format!("-{c}"))),
                    Some(v) => {
                        let vb = bytes(v);
                        if vb.is_empty() || vb[0] == b'=' {
                            // O clap tira um `=` do começo de valor grudado em curta.
                            out.push(OsString::from(format!("-{c}")));
                            out.push(v.clone());
                        } else {
                            let mut b = format!("-{c}").into_bytes();
                            b.extend_from_slice(vb);
                            out.push(os_from(b));
                        }
                    }
                }
            }
        }
    }
    out.push(OsString::from("--"));
    out.extend(scanned.operands.iter().cloned());
    out
}

/// Escreve `prog: <erro do getopt>` e `Try 'prog --help' for more information.` no stderr.
pub fn report(err: &GetoptError) {
    let mut line = Vec::new();
    line.extend_from_slice(crate::execution_phrase().as_bytes());
    line.extend_from_slice(b": ");
    line.extend_from_slice(&err.0);
    line.push(b'\n');
    let _ = sysio::io::stderr().write_all(&line);
    let _ = writeln!(
        sysio::io::stderr(),
        "Try '{} --help' for more information.",
        crate::execution_phrase()
    );
}

/// Faz o parse de `args` (com o `argv[0]`) como o GNU: varre com a semântica do getopt, reporta os
/// erros dele como a glibc e entrega a linha normalizada ao clap. Os erros que sobrarem do clap
/// (valor inválido num `value_parser`, por exemplo) seguem pelo tratamento do uucore.
pub fn parse(cmd: Command, args: impl IntoIterator<Item = OsString>) -> UResult<ArgMatches> {
    parse_with(cmd, args, &Config::default())
}

/// [`parse`] com configuração.
pub fn parse_with(
    cmd: Command,
    args: impl IntoIterator<Item = OsString>,
    config: &Config,
) -> UResult<ArgMatches> {
    let args: Vec<OsString> = args.into_iter().collect();
    let (argv0, rest) = match args.split_first() {
        Some((a, r)) => (a.clone(), r),
        None => (OsString::new(), &[][..]),
    };
    let specs = table(&cmd);
    match scan_specs(&specs, rest, config) {
        Ok(scanned) => {
            let line = normalize_specs(&specs, argv0, &scanned);
            crate::clap_localization::handle_clap_result_with_exit_code(cmd, line, config.exit_code)
        }
        Err(e) => {
            report(&e);
            Err(USimpleError::new(config.exit_code, ""))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cmd() -> Command {
        Command::new("t")
            .version("1")
            .arg(Arg::new("lines").short('n').long("lines"))
            .arg(Arg::new("quiet").short('q').long("quiet").visible_alias("silent").action(ArgAction::SetTrue))
            .arg(Arg::new("sleep").short('s').long("sleep-interval"))
            .arg(
                Arg::new("follow")
                    .short('f')
                    .long("follow")
                    .num_args(0..=1)
                    .require_equals(true),
            )
            .arg(Arg::new("files").action(ArgAction::Append))
    }

    fn sc(args: &[&str]) -> Result<Scanned, GetoptError> {
        let a: Vec<OsString> = args.iter().map(OsString::from).collect();
        scan_specs(&table(&cmd()), &a, &Config::default())
    }

    fn err(args: &[&str]) -> String {
        String::from_utf8(sc(args).unwrap_err().0).unwrap()
    }

    #[test]
    fn messages() {
        assert_eq!(err(&["-k"]), "invalid option -- 'k'");
        assert_eq!(err(&["-n"]), "option requires an argument -- 'n'");
        assert_eq!(err(&["--lines"]), "option '--lines' requires an argument");
        assert_eq!(err(&["--quiet=3"]), "option '--quiet' doesn't allow an argument");
        assert_eq!(err(&["--foo=bar"]), "unrecognized option '--foo=bar'");
        assert_eq!(
            err(&["--s=1"]),
            "option '--s=1' is ambiguous; possibilities: '--silent' '--sleep-interval'"
        );
        assert_eq!(err(&["-h"]), "invalid option -- 'h'");
    }

    #[test]
    fn values_and_operands() {
        let s = sc(&["a", "-n", "-5", "-qf", "--lin=3", "--", "-x"]).unwrap();
        assert_eq!(s.operands, vec![OsString::from("a"), OsString::from("-x")]);
        let ids: Vec<&str> = s.options.iter().map(|o| o.id.as_str()).collect();
        assert_eq!(ids, vec!["lines", "quiet", "follow", "lines"]);
        assert_eq!(s.options[0].value.as_deref(), Some(OsStr::new("-5")));
        assert_eq!(s.options[2].value, None);
        assert_eq!(s.options[3].value.as_deref(), Some(OsStr::new("3")));
    }
}
