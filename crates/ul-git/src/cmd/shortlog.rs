//! `git shortlog` (builtin/shortlog.c): os commits agrupados por autor ou committer, com a
//! contagem e os assuntos recuados, ou só a contagem (`-s`). Com revisões, caminha pelo histórico
//! como o `log`; sem revisão e com a entrada padrão fora de um terminal, lê a saída de um
//! `git log` da entrada padrão.

use std::collections::HashSet;

use super::Git;
use super::log::{Filters, Walker, all_specs, resolve_revs, unrecognized};
use crate::error::{Fail, R};
use crate::object::{self, Commit};
use crate::opts::{self, Parsed};
use crate::os;

/// Os grupos pedidos (`--group`, `-c`). Sem nenhum, o padrão é o autor.
#[derive(Default, Clone, Copy)]
struct Groups {
    author: bool,
    committer: bool,
}

/// Lê `--group=<tipo>`, `--no-group` e `-c` na ordem em que vieram, como o `parse_group_option`.
fn parse_groups(p: &Parsed) -> R<Groups> {
    let mut g = Groups::default();
    for h in &p.hits {
        match h.id {
            "by-committer" => g.committer = !h.negated,
            "group" => {
                if h.negated {
                    g = Groups::default();
                    continue;
                }
                match h.value.as_deref().unwrap_or(b"") {
                    b"author" => g.author = true,
                    b"committer" => g.committer = true,
                    other => {
                        crate::error::error(&format!("unknown group type: {}", os::lossy(other)));
                        return Err(Fail::Exit(129));
                    }
                }
            }
            _ => {}
        }
    }
    if !g.author && !g.committer {
        g.author = true;
    }
    Ok(g)
}

/// O assunto como o `insert_one_record` o guarda: sem espaço inicial (nem linhas em branco), sem
/// o prefixo `[PATCH...]`, e as linhas do primeiro parágrafo unidas por um espaço, cada uma sem o
/// espaço final (o `format_subject`).
fn clean_subject(text: &[u8]) -> Vec<u8> {
    let mut s = text;
    while let Some((c, rest)) = s.split_first() {
        if !c.is_ascii_whitespace() {
            break;
        }
        s = rest;
    }
    let eol = s.iter().position(|c| *c == b'\n').unwrap_or(s.len());
    if s.starts_with(b"[PATCH")
        && let Some(eob) = s.iter().position(|c| *c == b']')
        && eob < eol
    {
        s = &s[eob + 1..];
    }
    while let Some((c, rest)) = s.split_first() {
        if !c.is_ascii_whitespace() || *c == b'\n' {
            break;
        }
        s = rest;
    }
    let mut out: Vec<u8> = Vec::new();
    let mut first = true;
    for line in s.split(|c| *c == b'\n') {
        let trimmed = object::trim_ascii(line);
        if trimmed.is_empty() {
            break;
        }
        if !first {
            out.push(b' ');
        }
        // Só o fim da linha é aparado; o começo fica como está (exceto na primeira, já tratada).
        let end = line.iter().rposition(|c| !c.is_ascii_whitespace()).map(|i| i + 1).unwrap_or(0);
        out.extend_from_slice(&line[..end]);
        first = false;
    }
    out
}

/// A lista de grupos em ordem de chave (o `string_list` ordenado do git) e os assuntos de cada um,
/// na ordem de chegada.
#[derive(Default)]
struct Log {
    groups: Vec<(Vec<u8>, Vec<Vec<u8>>)>,
}

impl Log {
    fn insert(&mut self, ident: Vec<u8>, subject: Vec<u8>) {
        match self.groups.binary_search_by(|(k, _)| k.as_slice().cmp(&ident)) {
            Ok(i) => self.groups[i].1.push(subject),
            Err(i) => self.groups.insert(i, (ident, vec![subject])),
        }
    }
}

/// `Nome` ou `Nome <e-mail>` (com `-e`).
fn ident_key(name: &[u8], email: &[u8], with_email: bool) -> Vec<u8> {
    let mut k = name.to_vec();
    if with_email {
        k.extend_from_slice(b" <");
        k.extend_from_slice(email);
        k.push(b'>');
    }
    k
}

/// O `read_from_stdin`: para cada linha `Author: ` (ou `author `), pula o resto do cabeçalho e as
/// linhas em branco, e a primeira linha da mensagem vira o assunto.
fn read_from_stdin(log: &mut Log, groups: Groups, with_email: bool) -> R<()> {
    let (m0, m1): (&[u8], &[u8]) = match (groups.author, groups.committer) {
        (true, false) => (b"Author: ", b"author "),
        (false, true) => (b"Commit: ", b"committer "),
        _ => return Err(Fail::Fatal("using multiple --group options with stdin is not supported".into())),
    };
    let data = os::stdin_all();
    let mut all: Vec<&[u8]> = data.split(|c| *c == b'\n').collect();
    // O último pedaço depois do `\n` final não é uma linha.
    if all.last().is_some_and(|l| l.is_empty()) {
        all.pop();
    }
    let mut lines = all.into_iter();
    while let Some(line) = lines.next() {
        let Some(v) = line.strip_prefix(m0).or_else(|| line.strip_prefix(m1)) else { continue };
        // Descarta o resto do cabeçalho, depois as linhas em branco.
        let mut oneline: &[u8] = b"";
        for l in lines.by_ref() {
            if l.is_empty() {
                break;
            }
        }
        for l in lines.by_ref() {
            if !l.is_empty() {
                oneline = l;
                break;
            }
        }
        // O `split_ident_line`: precisa de `<` e `>`.
        let Some(lt) = v.iter().position(|c| *c == b'<') else { continue };
        let Some(gt) = v[lt..].iter().position(|c| *c == b'>').map(|g| g + lt) else { continue };
        let name = object::trim_ascii(&v[..lt]);
        let email = &v[lt + 1..gt];
        log.insert(ident_key(name, email, with_email), clean_subject(oneline));
    }
    Ok(())
}

/// O `shortlog_add_commit`: um registro por grupo, sem repetir a mesma identidade no commit.
fn add_commit(log: &mut Log, c: &Commit, groups: Groups, with_email: bool) {
    let subject = clean_subject(&c.subject());
    let mut seen: HashSet<Vec<u8>> = HashSet::new();
    let mut idents = Vec::new();
    if groups.author {
        let a = c.author_ident();
        idents.push(ident_key(&a.name, &a.email, with_email));
    }
    if groups.committer {
        let m = c.committer_ident();
        idents.push(ident_key(&m.name, &m.email, with_email));
    }
    for id in idents {
        if seen.insert(id.clone()) {
            log.insert(id, subject.clone());
        }
    }
}

/// `git shortlog [-s] [-n] [-e] [-c] [--group=<tipo>] [<revisões>] [[--] <caminhos>...]`.
pub fn run(git: &mut Git, args: &[Vec<u8>]) -> R<i32> {
    let usage = git.usage();
    let mut specs = all_specs();
    // `-s` (sem patch), `-n` (máximo de commits), `--summary` do diff e `--committer=<padrão>` do
    // log têm outro sentido aqui: as opções do shortlog vêm antes das de revisão.
    specs.retain(|s| {
        !(s.short == Some(b's') || s.short == Some(b'n') || s.short == Some(b'e') || s.short == Some(b'c'))
            && !matches!(s.long, Some("summary" | "numbered" | "email" | "committer" | "group"))
    });
    specs.push(opts::flag(Some(b's'), "summary", "summary"));
    specs.push(opts::flag(Some(b'n'), "numbered", "numbered"));
    specs.push(opts::flag(Some(b'e'), "email", "email"));
    specs.push(opts::flag(Some(b'c'), "committer", "by-committer"));
    specs.push(opts::value(None, "group", "group"));
    let p = opts::parse(&specs, args, opts::KEEP_UNKNOWN, usage)?;
    unrecognized(&p)?;
    let groups = parse_groups(&p)?;
    let with_email = p.has("email");
    let mut log = Log::default();
    let (before, _after) = p.split_dashdash();
    let refs_opt = p.has("all") || p.has("branches") || p.has("tags") || p.has("remotes");
    if before.iter().all(|a| !is_revision(git, a)) && !refs_opt {
        // Sem revisão e com a entrada padrão fora de um terminal, o git lê o log da entrada.
        read_from_stdin(&mut log, groups, with_email)?;
    } else {
        let repo = git.repo()?;
        let revs = resolve_revs(repo, &p)?;
        let ps = if revs.paths.is_empty() { None } else { Some(git.pathspec(&revs.paths)?) };
        let filters = Filters::from(&p)?;
        let mut walker = Walker::new(repo, &revs.include, &revs.exclude, p.has("first-parent"), ps.as_ref())?;
        while let Some((id, show)) = walker.next()? {
            if !show {
                continue;
            }
            let c = repo.read_commit(&id)?;
            if filters.accepts(&c) {
                add_commit(&mut log, &c, groups, with_email);
            }
        }
    }
    output(&log, p.has("summary"), p.has("numbered"));
    Ok(0)
}

/// Se o argumento (antes do `--`) é uma revisão; fora de um repositório nada é.
fn is_revision(git: &Git, arg: &[u8]) -> bool {
    let Some(repo) = git.repo.as_ref() else { return false };
    let spec: &[u8] = arg.strip_prefix(b"^").unwrap_or(arg);
    let find = |sep: &[u8]| spec.windows(sep.len()).position(|w| w == sep);
    let parts: Vec<&[u8]> = if let Some(i) = find(b"...") {
        vec![&spec[..i], &spec[i + 3..]]
    } else if let Some(i) = find(b"..") {
        vec![&spec[..i], &spec[i + 2..]]
    } else {
        vec![spec]
    };
    parts.iter().all(|s| {
        let s: &[u8] = if s.is_empty() { b"HEAD" } else { s };
        matches!(repo.rev_parse_commit(s), Ok(Some(_)))
    })
}

/// O `shortlog_output`: com `-n`, ordenação estável pela contagem (decrescente); os assuntos saem
/// do último registrado pro primeiro, ou seja, do commit mais velho pro mais novo.
fn output(log: &Log, summary: bool, numbered: bool) {
    let mut order: Vec<&(Vec<u8>, Vec<Vec<u8>>)> = log.groups.iter().collect();
    if numbered {
        order.sort_by_key(|g| std::cmp::Reverse(g.1.len()));
    }
    let mut out: Vec<u8> = Vec::new();
    for (name, subjects) in order {
        if summary {
            out.extend_from_slice(format!("{:>6}\t", subjects.len()).as_bytes());
            out.extend_from_slice(name);
            out.push(b'\n');
        } else {
            out.extend_from_slice(name);
            out.extend_from_slice(format!(" ({}):\n", subjects.len()).as_bytes());
            for sub in subjects.iter().rev() {
                out.extend_from_slice(b"      ");
                out.extend_from_slice(sub);
                out.push(b'\n');
            }
            out.push(b'\n');
        }
    }
    os::out(&out);
}
