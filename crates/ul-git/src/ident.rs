//! Identidades de autor e committer, como o `ident.c` do git: ambiente (`GIT_AUTHOR_*`,
//! `GIT_COMMITTER_*`), configuração (`author.*`, `committer.*`, `user.*`), `EMAIL`, e por fim o
//! usuário do sistema (`/etc/passwd`) com `usuario@host`.

use crate::config::Config;
use crate::date;
use crate::error::{Fail, R};
use crate::object::Ident;
use crate::os;

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Who {
    Author,
    Committer,
}

const ENV_HINT: &str = "\n*** Please tell me who you are.\n\nRun\n\n  git config --global user.email \"you@example.com\"\n  git config --global user.name \"Your Name\"\n\nto set your account's default identity.\nOmit --global to set the identity only in this repository.\n\n";

fn env_hint(who: Who) {
    let head = match who {
        Who::Author => "Author identity unknown\n",
        Who::Committer => "Committer identity unknown\n",
    };
    os::errs(head);
    os::errs(ENV_HINT);
}

fn crud(c: u8) -> bool {
    c <= 32 || matches!(c, b',' | b':' | b';' | b'<' | b'>' | b'"' | b'\\' | b'\'')
}

/// `strbuf_addstr_without_crud`.
pub fn without_crud(s: &[u8]) -> Vec<u8> {
    let start = s.iter().position(|c| !crud(*c)).unwrap_or(s.len());
    let end = s.iter().rposition(|c| !crud(*c)).map(|e| e + 1).unwrap_or(start).max(start);
    s[start..end].iter().copied().filter(|c| !matches!(c, b'\n' | b'<' | b'>')).collect()
}

fn has_non_crud(s: &[u8]) -> bool {
    s.iter().any(|c| !crud(*c))
}

fn passwd_root() -> (String, String) {
    let uid = os::getuid();
    match sysio::users::passwd_by_uid(uid) {
        Some(p) => {
            let mut gecos = p.gecos.split(',').next().unwrap_or("").to_string();
            if gecos.contains('&') {
                let mut cap = p.name.clone();
                if let Some(f) = cap.get_mut(0..1) {
                    f.make_ascii_uppercase();
                }
                gecos = gecos.replace('&', &cap);
            }
            (p.name, gecos)
        }
        None => ("root".into(), String::new()),
    }
}

/// E-mail padrão: `EMAIL`, ou `usuario@host` (com `/etc/mailname` como o Debian). O bool diz se o
/// e-mail é "falso" (terminou em `.(none)`).
fn default_email() -> (Vec<u8>, bool) {
    if let Some(e) = os::getenv("EMAIL")
        && !e.is_empty()
    {
        return (e, false);
    }
    let (user, _) = passwd_root();
    let mut email = format!("{user}@").into_bytes();
    if let Ok(Some(m)) = os::read_opt(b"/etc/mailname") {
        let line = m.split(|c| *c == b'\n').next().unwrap_or(&[]).to_vec();
        if !line.is_empty() {
            email.extend_from_slice(&line);
            return (email, false);
        }
    }
    let host = os::hostname();
    email.extend_from_slice(&host);
    if !host.contains(&b'.') {
        email.extend_from_slice(b".(none)");
        return (email, true);
    }
    (email, false)
}

fn default_name() -> Vec<u8> {
    let (_, gecos) = passwd_root();
    gecos.trim().as_bytes().to_vec()
}

/// Nome e e-mail (sem data) de `who`, com as regras do `fmt_ident`. `strict` é o modo de commit.
pub fn name_email(cfg: &Config, who: Who, strict: bool) -> R<(Vec<u8>, Vec<u8>)> {
    let (pre, sect) = match who {
        Who::Author => ("GIT_AUTHOR", "author"),
        Who::Committer => ("GIT_COMMITTER", "committer"),
    };
    let use_config_only = cfg.get_bool("user.useconfigonly")?.unwrap_or(false);
    let cfg_email = cfg.get_bytes(&format!("{sect}.email")).or_else(|| cfg.get_bytes("user.email"));
    let cfg_name = cfg.get_bytes(&format!("{sect}.name")).or_else(|| cfg.get_bytes("user.name"));
    let email = match os::getenv(&format!("{pre}_EMAIL")).or(cfg_email) {
        Some(e) => e,
        None => {
            if strict && use_config_only {
                env_hint(who);
                return Err(Fail::Fatal("no email was given and auto-detection is disabled".into()));
            }
            let (e, bogus) = default_email();
            if strict && bogus {
                env_hint(who);
                return Err(Fail::Fatal(format!("unable to auto-detect email address (got '{}')", os::lossy(&e))));
            }
            e
        }
    };
    let (name, using_default) = match os::getenv(&format!("{pre}_NAME")).or(cfg_name) {
        Some(n) => (n, false),
        None => {
            if strict && use_config_only {
                env_hint(who);
                return Err(Fail::Fatal("no name was given and auto-detection is disabled".into()));
            }
            (default_name(), true)
        }
    };
    let mut name = name;
    if name.is_empty() {
        if strict {
            if using_default {
                env_hint(who);
            }
            return Err(Fail::Fatal(format!("empty ident name (for <{}>) not allowed", os::lossy(&email))));
        }
        name = passwd_root().0.into_bytes();
    }
    if strict && !has_non_crud(&name) {
        return Err(Fail::Fatal(format!("name consists only of disallowed characters: {}", os::lossy(&name))));
    }
    Ok((without_crud(&name), without_crud(&email)))
}

/// Identidade completa com data (`GIT_*_DATE` ou agora).
pub fn ident(cfg: &Config, who: Who, strict: bool) -> R<Ident> {
    let (name, email) = name_email(cfg, who, strict)?;
    let var = match who {
        Who::Author => "GIT_AUTHOR_DATE",
        Who::Committer => "GIT_COMMITTER_DATE",
    };
    let (t, tz) = match os::getenv(var) {
        Some(d) if !d.is_empty() => parse_ident_date(&d)?,
        _ => date::now_with_tz(),
    };
    Ok(Ident { name, email, date: Some(t), tz, tz_raw: Vec::new() })
}

/// Data no formato aceito pelo `parse_date`, com o erro do git.
pub fn parse_ident_date(d: &[u8]) -> R<(i64, i32)> {
    date::parse_date(d).ok_or_else(|| Fail::Fatal(format!("invalid date format: {}", os::lossy(d))))
}

/// Identidade com nome, e-mail e data explícitos (`--author`, `--date`).
pub fn with_date(name: Vec<u8>, email: Vec<u8>, when: (i64, i32)) -> Ident {
    Ident { name: without_crud(&name), email: without_crud(&email), date: Some(when.0), tz: when.1, tz_raw: Vec::new() }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crud_is_removed() {
        assert_eq!(without_crud(b"  <A B>, "), b"A B");
        assert_eq!(without_crud(b"a<b>c"), b"abc");
    }
}
