//! `git log [--format=<f> | --pretty=<f> | --oneline] [-n N] [<rev>]`, caminhando os pais em ordem
//! de data do committer.

use std::collections::BTreeSet;

use gix_hash::ObjectId;

use super::{format_date, hex, short_name, with_repo};
use crate::git::store::{Head, Repo};
use crate::shell::Ctx;

enum Format {
    Medium,
    Oneline,
    Template(String),
}

fn expand(repo: &Repo<'_>, id: &ObjectId, c: &gix_object::Commit, tpl: &str) -> String {
    let msg = c.message.to_string();
    let subject = msg.lines().next().unwrap_or("").to_string();
    let body: String = msg.split_once("\n\n").map(|x| x.1.to_string()).unwrap_or_default();
    let mut s = String::new();
    let mut chars = tpl.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch != '%' {
            s.push(ch);
            continue;
        }
        let next: String = match chars.next() {
            Some('H') => hex(id),
            Some('h') => repo.abbrev(id),
            Some('T') => hex(&c.tree),
            Some('t') => repo.abbrev(&c.tree),
            Some('P') => c.parents.iter().map(hex).collect::<Vec<_>>().join(" "),
            Some('p') => c.parents.iter().map(|p| repo.abbrev(p)).collect::<Vec<_>>().join(" "),
            Some('s') => subject.clone(),
            Some('b') => body.clone(),
            Some('n') => "\n".into(),
            Some('%') => "%".into(),
            Some(w @ ('a' | 'c')) => {
                let sig = if w == 'a' { &c.author } else { &c.committer };
                match chars.next() {
                    Some('n') => sig.name.to_string(),
                    Some('e') => sig.email.to_string(),
                    Some('d') => format_date(&sig.time),
                    Some('t') => sig.time.seconds.to_string(),
                    Some(o) => format!("%{w}{o}"),
                    None => format!("%{w}"),
                }
            }
            Some(o) => format!("%{o}"),
            None => "%".into(),
        };
        s.push_str(&next);
    }
    s
}

pub fn run(args: &[String], ctx: &mut Ctx<'_>) -> i32 {
    with_repo(ctx, |repo, _env, out, err, _stdin| {
        let mut format = Format::Medium;
        let mut limit: Option<usize> = None;
        let mut rev: Option<String> = None;
        let mut i = 0;
        while i < args.len() {
            let a = args[i].as_str();
            if let Some(f) = a.strip_prefix("--format=").or_else(|| a.strip_prefix("--pretty=format:")).or_else(|| a.strip_prefix("--pretty=tformat:")) {
                format = Format::Template(f.to_string());
            } else if a == "--oneline" || a == "--pretty=oneline" {
                format = if a == "--oneline" { Format::Oneline } else { Format::Template("%H %s".into()) };
            } else if a == "-n" {
                i += 1;
                limit = args.get(i).and_then(|n| n.parse().ok());
            } else if let Some(n) = a.strip_prefix("--max-count=") {
                limit = n.parse().ok();
            } else if a.len() > 1 && a.starts_with('-') && a[1..].bytes().all(|b| b.is_ascii_digit()) {
                limit = a[1..].parse().ok();
            } else if !a.starts_with('-') {
                rev = Some(a.to_string());
            }
            i += 1;
        }
        let start = match &rev {
            Some(r) => repo.rev_parse(r)?,
            None => match repo.head()? {
                Head::Branch(b, None) => {
                    err.extend_from_slice(
                        format!("fatal: your current branch '{}' does not have any commits yet\n", short_name(&b)).as_bytes(),
                    );
                    return Ok(128);
                }
                Head::Branch(_, Some(id)) | Head::Detached(id) => id,
            },
        };
        // Fila por data do committer (mais novo primeiro), sem repetir.
        let mut seen = BTreeSet::new();
        let mut queue: Vec<(i64, ObjectId)> = vec![(repo.commit(&start)?.committer.time.seconds, start)];
        let mut shown = 0;
        let mut first = true;
        while let Some(pos) = (0..queue.len()).max_by_key(|&k| (queue[k].0, std::cmp::Reverse(k))) {
            let (_, id) = queue.remove(pos);
            if !seen.insert(id) {
                continue;
            }
            if limit.is_some_and(|l| shown >= l) {
                break;
            }
            let c = repo.commit(&id)?;
            let text = match &format {
                Format::Oneline => format!("{} {}\n", repo.abbrev(&id), c.message.to_string().lines().next().unwrap_or("")),
                Format::Template(t) => expand(repo, &id, &c, t) + "\n",
                Format::Medium => {
                    let mut s = String::new();
                    if !first {
                        s.push('\n');
                    }
                    s.push_str(&format!("commit {}\n", hex(&id)));
                    s.push_str(&format!("Author: {} <{}>\n", c.author.name, c.author.email));
                    s.push_str(&format!("Date:   {}\n\n", format_date(&c.author.time)));
                    for line in c.message.to_string().trim_end().lines() {
                        if line.is_empty() {
                            s.push('\n');
                        } else {
                            s.push_str(&format!("    {line}\n"));
                        }
                    }
                    s
                }
            };
            out.extend_from_slice(text.as_bytes());
            first = false;
            shown += 1;
            for p in &c.parents {
                queue.push((repo.commit(p)?.committer.time.seconds, *p));
            }
        }
        Ok(0)
    })
}
