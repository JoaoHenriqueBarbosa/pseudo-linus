//! Nomes de revisão (o `get_oid_with_context` do git): refs com DWIM, hexadecimal completo e
//! abreviado, `^`, `~`, `^{tipo}`, `^{/texto}`, `@{N}`, `@{-N}`, `@{u}`, `<rev>:<caminho>`,
//! `:<caminho>`, `:N:<caminho>`, `:/texto` e a saída do `describe`.

use crate::error::{Fail, R, error, hint, warning};
use crate::hash::{self, Kind, Oid};
use crate::index::Index;
use crate::object;
use crate::os;
use crate::repo::Repo;

/// Resultado com o modo e o caminho quando veio de `<rev>:<caminho>`.
#[derive(Clone, Debug)]
pub struct Resolved {
    pub oid: Oid,
    pub mode: Option<u32>,
    pub path: Option<Vec<u8>>,
}

/// Contexto de desambiguação de hexadecimal abreviado.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Want {
    Any,
    Commitish,
    Treeish,
}

const DWIM_RULES: [&str; 6] = ["{}", "refs/{}", "refs/tags/{}", "refs/heads/{}", "refs/remotes/{}", "refs/remotes/{}/HEAD"];

/// Mensagem padrão de revisão desconhecida (`ambiguous argument`).
pub fn bad_revision(arg: &[u8]) -> Fail {
    Fail::Fatal(format!(
        "ambiguous argument '{}': unknown revision or path not in the working tree.\nUse '--' to separate paths from revisions, like this:\n'git <command> [<revision>...] -- [<file>...]'",
        os::lossy(arg)
    ))
}

impl Repo {
    /// DWIM de um nome de ref: `(nome completo, id)`, avisando ambiguidade como o git.
    pub fn dwim_ref(&self, name: &str) -> R<Option<(String, Oid)>> {
        if name.is_empty() {
            return Ok(None);
        }
        let mut found: Option<(String, Oid)> = None;
        let mut count = 0;
        for rule in DWIM_RULES {
            let full = rule.replace("{}", name);
            if !crate::refs::check_refname_format(&full, true, false) {
                continue;
            }
            // Só pseudo-refs (maiúsculas) e nomes com `refs/` valem como estão.
            if rule == "{}" && !full.starts_with("refs/") && !crate::refs::is_pseudoref_syntax(&full) {
                continue;
            }
            if let Some((_, Some(id))) = self.resolve_ref(&full)? {
                count += 1;
                if found.is_none() {
                    found = Some((full, id));
                }
                if !self.warn_ambiguous() {
                    break;
                }
            }
        }
        if count > 1 {
            warning(&format!("refname '{name}' is ambiguous."));
        }
        Ok(found)
    }

    /// Como o `dwim_ref`, mas aceita ref que existe e não resolve (ramo sem commits).
    pub fn dwim_ref_name(&self, name: &str) -> R<Option<String>> {
        for rule in DWIM_RULES {
            let full = rule.replace("{}", name);
            if rule == "{}" && !full.starts_with("refs/") && !crate::refs::is_pseudoref_syntax(&full) {
                continue;
            }
            if self.read_ref(&full)?.is_some() {
                return Ok(Some(full));
            }
        }
        Ok(None)
    }

    fn warn_ambiguous(&self) -> bool {
        self.config.get_bool("core.warnambiguousrefs").ok().flatten().unwrap_or(true)
    }

    /// Resolve uma revisão; `Ok(None)` quando não existe (o chamador escolhe a mensagem).
    pub fn rev_parse(&self, spec: &[u8]) -> R<Option<Oid>> {
        Ok(self.rev_parse_full(spec, Want::Any)?.map(|r| r.oid))
    }

    pub fn rev_parse_want(&self, spec: &[u8], want: Want) -> R<Option<Oid>> {
        Ok(self.rev_parse_full(spec, want)?.map(|r| r.oid))
    }

    /// Resolve e exige um commit.
    pub fn rev_parse_commit(&self, spec: &[u8]) -> R<Option<Oid>> {
        match self.rev_parse_want(spec, Want::Commitish)? {
            Some(id) => self.peel_to_commit(&id),
            None => Ok(None),
        }
    }

    pub fn rev_parse_full(&self, spec: &[u8], want: Want) -> R<Option<Resolved>> {
        if spec.is_empty() {
            return Ok(None);
        }
        if spec[0] == b':' {
            return self.resolve_colon(spec);
        }
        // `<rev>:<caminho>`: o primeiro `:` fora de `{...}`.
        let mut depth = 0;
        let mut colon = None;
        for (i, &c) in spec.iter().enumerate() {
            match c {
                b'{' => depth += 1,
                b'}' if depth > 0 => depth -= 1,
                b':' if depth == 0 => {
                    colon = Some(i);
                    break;
                }
                _ => {}
            }
        }
        if let Some(c) = colon {
            let tree_spec = &spec[..c];
            let path = &spec[c + 1..];
            let Some(base) = self.get_oid_1(tree_spec, Want::Treeish)? else {
                return Err(Fail::Fatal(format!("invalid object name '{}'.", os::lossy(tree_spec))));
            };
            let Some(tree) = self.peel_to_tree(&base)? else {
                return Err(Fail::Fatal(format!("invalid object name '{}'.", os::lossy(tree_spec))));
            };
            let rel = self.resolve_relative_path(path);
            return match self.tree_lookup(&tree, &rel)? {
                Some((mode, id)) => Ok(Some(Resolved { oid: id, mode: Some(mode), path: Some(rel) })),
                None => {
                    let rel_disp = os::lossy(&rel);
                    let on_disk = self.work_tree.is_some() && os::exists(&rel);
                    if on_disk {
                        Err(Fail::Fatal(format!("path '{rel_disp}' exists on disk, but not in '{}'", os::lossy(tree_spec))))
                    } else {
                        Err(Fail::Fatal(format!("path '{rel_disp}' does not exist in '{}'", os::lossy(tree_spec))))
                    }
                }
            };
        }
        Ok(self.get_oid_1(spec, want)?.map(|oid| Resolved { oid, mode: None, path: None }))
    }

    /// `./x` e `../x` são relativos ao cwd; o resto, ao topo.
    fn resolve_relative_path(&self, path: &[u8]) -> Vec<u8> {
        if path.starts_with(b"./") || path.starts_with(b"../") || path == b"." || path == b".." {
            let joined = [self.prefix.as_slice(), path].concat();
            return crate::pathspec::normalize_rel(&joined).unwrap_or_default();
        }
        path.to_vec()
    }

    fn resolve_colon(&self, spec: &[u8]) -> R<Option<Resolved>> {
        if let Some(rest) = spec.strip_prefix(b":/") {
            if rest.is_empty() {
                return Ok(None);
            }
            let mut starts = Vec::new();
            for (_, id) in self.list_refs("refs/")? {
                if let Some(c) = self.peel_to_commit(&id)? {
                    starts.push(c);
                }
            }
            if let Some(h) = self.head_oid()? {
                starts.push(h);
            }
            return Ok(self.search_message(&starts, rest)?.map(|oid| Resolved { oid, mode: None, path: None }));
        }
        let (stage, path) = if spec.len() >= 3 && (b'0'..=b'3').contains(&spec[1]) && spec[2] == b':' {
            (spec[1] - b'0', &spec[3..])
        } else {
            (0, &spec[1..])
        };
        let rel = self.resolve_relative_path(path);
        let idx = Index::load(&self.index_path())?;
        if let Ok(i) = idx.pos(&rel, stage) {
            let e = &idx.entries[i];
            return Ok(Some(Resolved { oid: e.oid, mode: Some(e.mode), path: Some(rel) }));
        }
        let disp = os::lossy(&rel);
        if stage == 0 && idx.stages(&rel).iter().any(|e| e.stage != 0) {
            return Err(Fail::Fatal(format!("path '{disp}' is in the index, but not at stage 0")));
        }
        if self.work_tree.is_some() && os::exists(&rel) {
            return Err(Fail::Fatal(format!("path '{disp}' exists on disk, but not in the index")));
        }
        Err(Fail::Fatal(format!("path '{disp}' does not exist (neither on disk nor in the index)")))
    }

    /// Commit mais novo (por data) alcançável de `starts` cuja mensagem casa com a regex.
    pub fn search_message(&self, starts: &[Oid], pattern: &[u8]) -> R<Option<Oid>> {
        let (negate, pat) = match pattern.strip_prefix(b"!") {
            Some(rest) if rest.starts_with(b"!") => (false, rest),
            Some(rest) => (true, rest),
            None => (false, pattern),
        };
        let re = regex_posix::Regex::new(pat, regex_posix::Syntax::POSIX_EXTENDED).ok();
        let mut seen = std::collections::HashSet::new();
        let mut queue: Vec<(i64, Oid)> = Vec::new();
        for s in starts {
            if seen.insert(*s) {
                queue.push((self.read_commit(s)?.commit_date(), *s));
            }
        }
        while !queue.is_empty() {
            queue.sort_by_key(|(d, _)| *d);
            let (_, id) = queue.pop().expect("não vazio");
            let c = self.read_commit(&id)?;
            let hit = match &re {
                Some(r) => r.is_match(&c.message),
                None => c.message.windows(pat.len()).any(|w| w == pat),
            };
            if hit != negate {
                return Ok(Some(id));
            }
            for p in c.parents {
                if seen.insert(p) {
                    queue.push((self.read_commit(&p)?.commit_date(), p));
                }
            }
        }
        Ok(None)
    }

    /// `get_oid_1`: sufixos `^{...}`, `^N`, `~N` e o nome básico.
    fn get_oid_1(&self, spec: &[u8], want: Want) -> R<Option<Oid>> {
        if spec.ends_with(b"}")
            && let Some(at) = find_last(spec, b"^{")
        {
            let base = &spec[..at];
            let inner = &spec[at + 2..spec.len() - 1];
            let Some(id) = self.get_oid_1(base, if inner == b"tree" { Want::Treeish } else { Want::Commitish })? else { return Ok(None) };
            return self.peel_onion(&id, inner);
        }
        // `~N` ou `^N` no fim.
        let mut k = spec.len();
        while k > 0 && spec[k - 1].is_ascii_digit() {
            k -= 1;
        }
        if k > 0 && (spec[k - 1] == b'~' || spec[k - 1] == b'^') {
            let op = spec[k - 1];
            let n: usize = if k == spec.len() { 1 } else { std::str::from_utf8(&spec[k..]).ok().and_then(|s| s.parse().ok()).unwrap_or(usize::MAX) };
            let base = &spec[..k - 1];
            if base.is_empty() {
                return Ok(None);
            }
            let Some(id) = self.get_oid_1(base, Want::Commitish)? else { return Ok(None) };
            let Some(mut c) = self.peel_to_commit(&id)? else { return Ok(None) };
            if op == b'^' {
                if n == 0 {
                    return Ok(Some(c));
                }
                let parents = self.parents(&c)?;
                return Ok(parents.get(n - 1).copied());
            }
            for _ in 0..n {
                match self.parents(&c)?.first() {
                    Some(p) => c = *p,
                    None => return Ok(None),
                }
            }
            return Ok(Some(c));
        }
        if let Some(id) = self.get_oid_basic(spec)? {
            return Ok(Some(id));
        }
        // Saída do describe: `...-g<hex>`.
        if let Some(g) = find_last(spec, b"-g") {
            let hex = &spec[g + 2..];
            if hex.len() >= 4 && hash::is_hex(hex) {
                return self.short_oid(hex, want, false);
            }
        }
        if spec.len() >= 4 && spec.len() < 40 && hash::is_hex(spec) {
            return self.short_oid(spec, want, true);
        }
        Ok(None)
    }

    fn peel_onion(&self, id: &Oid, inner: &[u8]) -> R<Option<Oid>> {
        match inner {
            b"" => self.peel(id, None).map(|o| o.map(|x| self.peel_tags(&x))),
            b"commit" => self.peel_to_commit(id),
            b"tree" => self.peel_to_tree(id),
            b"blob" => {
                let p = self.peel_tags(id);
                Ok((self.object_kind(&p)? == Some(Kind::Blob)).then_some(p))
            }
            b"tag" => Ok((self.object_kind(id)? == Some(Kind::Tag)).then_some(*id)),
            b"object" => Ok(Some(*id)),
            _ if inner.starts_with(b"/") => {
                let Some(c) = self.peel_to_commit(id)? else { return Ok(None) };
                self.search_message(&[c], &inner[1..])
            }
            _ => Ok(None),
        }
    }

    fn peel_tags(&self, id: &Oid) -> Oid {
        let mut cur = *id;
        for _ in 0..64 {
            match self.try_read(&cur) {
                Ok(Some((Kind::Tag, d))) => match object::parse_tag(&d) {
                    Ok(t) => cur = t.object,
                    Err(_) => break,
                },
                _ => break,
            }
        }
        cur
    }

    /// Hexadecimal abreviado; `report` imprime o erro de ambiguidade.
    fn short_oid(&self, hex: &[u8], want: Want, report: bool) -> R<Option<Oid>> {
        let lower = hex.to_ascii_lowercase();
        let mut cands = Vec::new();
        self.odb.find_prefix(&lower, &mut cands);
        if cands.len() > 1 && want != Want::Any {
            let filtered: Vec<Oid> = cands
                .iter()
                .copied()
                .filter(|c| match want {
                    Want::Commitish => self.peel_to_commit(c).ok().flatten().is_some(),
                    Want::Treeish => self.peel_to_tree(c).ok().flatten().is_some(),
                    Want::Any => true,
                })
                .collect();
            if filtered.len() == 1 {
                return Ok(Some(filtered[0]));
            }
        }
        match cands.len() {
            0 => Ok(None),
            1 => Ok(Some(cands[0])),
            _ => {
                if report {
                    error(&format!("short object ID {} is ambiguous", os::lossy(hex)));
                    let mut lines = String::from("The candidates are:");
                    for c in &cands {
                        lines.push('\n');
                        lines.push_str(&self.describe_candidate(c));
                    }
                    hint(&lines);
                }
                Ok(None)
            }
        }
    }

    fn describe_candidate(&self, id: &Oid) -> String {
        let ab = self.abbrev(id, self.abbrev_len());
        match self.try_read(id) {
            Ok(Some((Kind::Commit, d))) => {
                let c = object::parse_commit(&d).unwrap_or_default();
                let date = c.committer_ident();
                let ymd = crate::date::show_date(date.date.unwrap_or(0), date.tz, &crate::date::DateMode::Short);
                format!("  {ab} commit {ymd} - {}", os::lossy(&c.subject()))
            }
            Ok(Some((Kind::Tag, d))) => {
                let t = object::parse_tag(&d).ok();
                format!("  {ab} tag {}", t.map(|t| os::lossy(&t.name)).unwrap_or_default())
            }
            Ok(Some((k, _))) => format!("  {ab} {}", k.name()),
            _ => format!("  {ab} unknown"),
        }
    }

    /// Nome básico: hexadecimal completo, `@`, refs e reflog (`@{...}`).
    fn get_oid_basic(&self, spec: &[u8]) -> R<Option<Oid>> {
        if spec.len() == 40
            && let Some(id) = Oid::from_hex(spec)
        {
            return Ok(Some(id));
        }
        if spec == b"@" {
            return self.head_oid();
        }
        let s = String::from_utf8_lossy(spec).into_owned();
        // `<ref>@{...}` no fim.
        if s.ends_with('}')
            && let Some(at) = s.rfind("@{")
        {
            let base = &s[..at];
            let inner = &s[at + 2..s.len() - 1];
            return self.reflog_spec(base, inner);
        }
        Ok(self.dwim_ref(&s)?.map(|(_, id)| id))
    }

    fn reflog_spec(&self, base: &str, inner: &str) -> R<Option<Oid>> {
        if let Some(n) = inner.strip_prefix('-') {
            if !base.is_empty() {
                return Ok(None);
            }
            let Ok(n) = n.parse::<usize>() else { return Ok(None) };
            return match self.nth_prior_branch(n)? {
                Some(b) => Ok(self.dwim_ref(&b)?.map(|(_, id)| id).or(Oid::from_hex(b.as_bytes()))),
                None => Ok(None),
            };
        }
        let lower = inner.to_ascii_lowercase();
        if matches!(lower.as_str(), "u" | "upstream" | "push") {
            let branch = if base.is_empty() || base == "HEAD" {
                match self.current_branch()? {
                    Some(b) => b.trim_start_matches("refs/heads/").to_string(),
                    None => return Err(Fail::Fatal("HEAD does not point to a branch".into())),
                }
            } else {
                base.to_string()
            };
            let up = self.upstream_ref(&branch)?;
            return Ok(match up {
                Some(r) => self.ref_oid(&r)?,
                None => return Err(Fail::Fatal(format!("no upstream configured for branch '{branch}'"))),
            });
        }
        let refname = if base.is_empty() {
            match self.current_branch()? {
                Some(b) => b,
                None => "HEAD".to_string(),
            }
        } else if base == "HEAD" {
            "HEAD".to_string()
        } else {
            match self.dwim_ref_name(base)? {
                Some(r) => r,
                None => return Ok(None),
            }
        };
        let log = self.read_reflog(&refname);
        if let Ok(n) = inner.parse::<usize>() {
            if n == 0 {
                if let Some(last) = log.last() {
                    return Ok(Some(last.new));
                }
                return self.ref_oid(&refname);
            }
            if n >= log.len() {
                if n == log.len() && !log.is_empty() {
                    return Ok(Some(log[0].old).filter(|o| !o.is_zero()));
                }
                let short = refname.strip_prefix("refs/heads/").unwrap_or(&refname);
                return Err(Fail::Fatal(format!("log for '{short}' only has {} entries", log.len())));
            }
            return Ok(Some(log[log.len() - 1 - n].new));
        }
        // Data.
        let Some(t) = crate::date::approxidate(inner) else { return Ok(None) };
        for e in log.iter().rev() {
            if e.time <= t {
                return Ok(Some(e.new));
            }
        }
        Ok(log.first().map(|e| e.old).filter(|o| !o.is_zero()))
    }

    /// O N-ésimo ramo anterior pelo reflog do HEAD (`checkout: moving from A to B`).
    pub fn nth_prior_branch(&self, n: usize) -> R<Option<String>> {
        let log = self.read_reflog("HEAD");
        let mut count = 0;
        for e in log.iter().rev() {
            let msg = String::from_utf8_lossy(&e.message);
            if let Some(rest) = msg.strip_prefix("checkout: moving from ")
                && let Some(to) = rest.find(" to ")
            {
                count += 1;
                if count == n {
                    return Ok(Some(rest[..to].to_string()));
                }
            }
        }
        Ok(None)
    }

    /// Ref do upstream de um ramo (`refs/remotes/<remoto>/<ramo>` ou `refs/heads/<x>` se remote=.).
    pub fn upstream_ref(&self, branch: &str) -> R<Option<String>> {
        let remote = self.config.get(&format!("branch.{branch}.remote"));
        let merge = self.config.get(&format!("branch.{branch}.merge"));
        let (Some(remote), Some(merge)) = (remote, merge) else { return Ok(None) };
        if remote == "." {
            return Ok(Some(merge));
        }
        let short = merge.strip_prefix("refs/heads/").unwrap_or(&merge);
        Ok(Some(format!("refs/remotes/{remote}/{short}")))
    }
}

fn find_last(hay: &[u8], needle: &[u8]) -> Option<usize> {
    if hay.len() < needle.len() {
        return None;
    }
    (0..=hay.len() - needle.len()).rev().find(|&i| &hay[i..i + needle.len()] == needle)
}
