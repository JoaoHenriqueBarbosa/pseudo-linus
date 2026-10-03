//! CLI `git` do protótipo: um módulo por subcomando, todos sobre [`Repo`].
//!
//! Cada arquivo aqui é contado no relatório como "linhas necessárias por comando".

pub mod add;
pub mod cat_file;
pub mod commit;
pub mod commit_tree;
pub mod diff;
pub mod hash_object;
pub mod init;
pub mod log;
pub mod ls_files;
pub mod ls_tree;
pub mod rev_parse;
pub mod status;
pub mod update_ref;
pub mod write_tree;

use std::collections::BTreeMap;

use gix_hash::ObjectId;

use super::store::Repo;
use crate::shell::Ctx;

/// Fontes dos comandos, pra contagem de linhas.
pub const SOURCES: &[(&str, &str)] = &[
    ("init", include_str!("init.rs")),
    ("hash-object", include_str!("hash_object.rs")),
    ("add", include_str!("add.rs")),
    ("write-tree", include_str!("write_tree.rs")),
    ("commit-tree", include_str!("commit_tree.rs")),
    ("update-ref", include_str!("update_ref.rs")),
    ("commit", include_str!("commit.rs")),
    ("log", include_str!("log.rs")),
    ("status", include_str!("status.rs")),
    ("diff", include_str!("diff.rs")),
    ("cat-file", include_str!("cat_file.rs")),
    ("ls-files", include_str!("ls_files.rs")),
    ("ls-tree", include_str!("ls_tree.rs")),
    ("rev-parse", include_str!("rev_parse.rs")),
    ("(compartilhado: despacho e utilitários)", include_str!("mod.rs")),
    ("(compartilhado: store sobre gix-*)", include_str!("../store.rs")),
    ("(compartilhado: diff de blobs)", include_str!("../textdiff.rs")),
];

/// Linhas de código (sem brancas, comentários e o bloco de testes).
pub fn loc(src: &str) -> usize {
    let body = src.split("#[cfg(test)]").next().unwrap_or(src);
    body.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with("//"))
        .count()
}

pub fn fatal(ctx: &mut Ctx<'_>, msg: &str) -> i32 {
    ctx.stderr.extend_from_slice(format!("fatal: {msg}\n").as_bytes());
    128
}

pub fn out(ctx: &mut Ctx<'_>, text: &str) {
    ctx.stdout.extend_from_slice(text.as_bytes());
}

/// Roda `f` com o repositório do caso, ou falha como o git fora de um repositório.
pub fn with_repo(ctx: &mut Ctx<'_>, f: impl FnOnce(&mut Repo<'_>, &BTreeMap<String, String>, &mut Vec<u8>, &mut Vec<u8>, &[u8]) -> anyhow::Result<i32>) -> i32 {
    let env = ctx.env.clone();
    let stdin = ctx.stdin.to_vec();
    let Some(mut repo) = Repo::open(ctx.fs) else {
        return fatal(ctx, "not a git repository (or any of the parent directories): .git");
    };
    let mut so = Vec::new();
    let mut se = Vec::new();
    let r = f(&mut repo, &env, &mut so, &mut se, &stdin);
    ctx.stdout.extend_from_slice(&so);
    ctx.stderr.extend_from_slice(&se);
    match r {
        Ok(code) => code,
        Err(e) => fatal(ctx, &e.to_string()),
    }
}

/// Assinatura de `GIT_<who>_NAME/EMAIL/DATE` (who = AUTHOR ou COMMITTER).
pub fn signature(env: &BTreeMap<String, String>, who: &str) -> gix_actor::Signature {
    let name = env.get(&format!("GIT_{who}_NAME")).cloned().unwrap_or_else(|| "root".into());
    let email = env.get(&format!("GIT_{who}_EMAIL")).cloned().unwrap_or_else(|| "root@localhost".into());
    let (seconds, offset) = env
        .get(&format!("GIT_{who}_DATE"))
        .and_then(|d| parse_raw_date(d))
        .unwrap_or((harness::FIXTURE_MTIME as i64, 0));
    gix_actor::Signature { name: name.into(), email: email.into(), time: gix_date::Time { seconds, offset } }
}

/// Data no formato bruto do git: `<segundos> <+hhmm>`.
pub fn parse_raw_date(s: &str) -> Option<(i64, i32)> {
    let (secs, tz) = s.trim().split_once(' ')?;
    let secs: i64 = secs.trim_start_matches('@').parse().ok()?;
    let sign = if tz.starts_with('-') { -1 } else { 1 };
    let digits = tz.trim_start_matches(['+', '-']);
    if digits.len() != 4 {
        return None;
    }
    let h: i32 = digits[..2].parse().ok()?;
    let m: i32 = digits[2..].parse().ok()?;
    Some((secs, sign * (h * 3600 + m * 60)))
}

/// Dias desde 1970-01-01 -> (ano, mês, dia), algoritmo de Howard Hinnant.
fn civil(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// Data no formato padrão do git: `Thu Jan 15 12:00:00 2026 +0000`.
pub fn format_date(t: &gix_date::Time) -> String {
    const DAYS: [&str; 7] = ["Thu", "Fri", "Sat", "Sun", "Mon", "Tue", "Wed"];
    const MONTHS: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
    let local = t.seconds + t.offset as i64;
    let days = local.div_euclid(86_400);
    let secs = local.rem_euclid(86_400);
    let (y, m, d) = civil(days);
    let sign = if t.offset < 0 { '-' } else { '+' };
    let off = t.offset.abs();
    format!(
        "{} {} {} {:02}:{:02}:{:02} {} {}{:02}{:02}",
        DAYS[days.rem_euclid(7) as usize],
        MONTHS[(m - 1) as usize],
        d,
        secs / 3600,
        secs % 3600 / 60,
        secs % 60,
        y,
        sign,
        off / 3600,
        off % 3600 / 60
    )
}

pub fn short_name(branch_ref: &str) -> &str {
    branch_ref.strip_prefix("refs/heads/").unwrap_or(branch_ref)
}

/// Linha de modo de 6 dígitos como no `ls-tree`/`cat-file -p`.
pub fn mode_str(mode: u32) -> String {
    format!("{mode:06o}")
}

pub fn kind_of_mode(mode: u32) -> &'static str {
    match mode {
        0o040000 => "tree",
        0o160000 => "commit",
        _ => "blob",
    }
}

pub fn hex(id: &ObjectId) -> String {
    id.to_hex().to_string()
}

/// Despacha `git <subcomando>`.
pub fn run_git(argv: &[String], ctx: &mut Ctx<'_>) -> i32 {
    let mut args: Vec<String> = argv[1..].to_vec();
    // Opções globais aceitas e ignoradas: -c chave=valor.
    while args.first().map(String::as_str) == Some("-c") && args.len() > 1 {
        args.drain(..2);
    }
    let Some(sub) = args.first().cloned() else {
        out(ctx, "usage: git [-v | --version] [-h | --help] <command> [<args>]\n");
        return 1;
    };
    let rest = &args[1..];
    match sub.as_str() {
        "init" => init::run(rest, ctx),
        "hash-object" => hash_object::run(rest, ctx),
        "add" => add::run(rest, ctx),
        "write-tree" => write_tree::run(rest, ctx),
        "commit-tree" => commit_tree::run(rest, ctx),
        "update-ref" => update_ref::run(rest, ctx),
        "commit" => commit::run(rest, ctx),
        "log" => log::run(rest, ctx),
        "status" => status::run(rest, ctx),
        "diff" => diff::run(rest, ctx),
        "cat-file" => cat_file::run(rest, ctx),
        "ls-files" => ls_files::run(rest, ctx),
        "ls-tree" => ls_tree::run(rest, ctx),
        "rev-parse" => rev_parse::run(rest, ctx),
        other => {
            ctx.stderr.extend_from_slice(format!("git: '{other}' is not a git command. See 'git --help'.\n").as_bytes());
            1
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_date_format() {
        let t = gix_date::Time { seconds: 1_768_478_400, offset: 0 };
        assert_eq!(format_date(&t), "Thu Jan 15 12:00:00 2026 +0000");
        let t = gix_date::Time { seconds: 0, offset: -3 * 3600 };
        assert_eq!(format_date(&t), "Wed Dec 31 21:00:00 1969 -0300");
    }
}
