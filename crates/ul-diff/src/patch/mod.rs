//! `patch` (GNU patch 2.8).
//!
//! Escrito a partir do manual, de especificação de comportamento e do que o oráculo (Debian 13)
//! mostra, nunca do código GPL. O localizador segue a especificação levantada no F08 (500/500 no
//! corpus aleatório); parser, mensagens, backups, rejeitos, git, `-D`, `-o`, `-r` e o resto são
//! nossos, conferidos caso a caso no oráculo.

pub mod apply;
pub mod backup;
pub mod hunk;
pub mod locate;
pub mod names;
pub mod opts;
pub mod parse;

use std::collections::BTreeSet;
use std::ffi::OsString;

use sysabi::sys;
use sysabi::{AtFlags, Ctx, Errno, Fd, FileType, OFlags, RenameFlags, SetTime, Stat, TimeSpec};

use apply::Builder;
use hunk::{Format, Hunk};
use locate::{Cursor, Matcher, locate_level};
use names::{Danger, HeaderName};
use opts::{MergeStyle, Opts, Parsed, Quoting, RejectFormat};
use parse::{Body, Chunk, Fatal, Scanner};

use crate::sysutil::{self, Output};

pub fn main(_ctx: &mut Ctx, args: &[OsString]) -> i32 {
    let argv = sysutil::args_bytes(args);
    let argv0 = sysutil::argv0(&argv);
    let parsed = opts::parse(&argv, &|name| sysutil::getenv(name));
    let o = match parsed {
        Parsed::Run(o) => *o,
        Parsed::Exit { code, stdout, stderr } => {
            let mut out = Output::stdout();
            out.write(&stdout);
            let _ = out.finish();
            sysutil::eprint(stderr);
            return code;
        }
    };
    let mut run = Run::new(o, argv0);
    let code = match run.main() {
        Ok(()) => run.status,
        Err(msg) => {
            run.out.flush();
            run.data.flush();
            let mut line = format!("{}: **** ", run.argv0).into_bytes();
            line.extend_from_slice(&msg);
            line.push(b'\n');
            sysutil::eprint(line);
            2
        }
    };
    run.out.flush();
    run.data.flush();
    code
}

/// Erro fatal: mensagem (sem o prefixo `patch: **** `).
type Fail = Vec<u8>;

struct Run {
    o: Opts,
    argv0: String,
    /// Mensagens (stdout, ou stderr quando `-o -` manda o conteúdo pro stdout).
    out: Output,
    /// Conteúdo do `-o -`.
    data: Output,
    status: i32,
    /// Arquivos de `-o` e `-r` já criados nesta execução (os seguintes acrescentam).
    outputs_started: BTreeSet<Vec<u8>>,
    rejects_started: BTreeSet<Vec<u8>>,
    /// Arquivos que já ganharam backup nesta execução (o backup guarda o conteúdo original).
    backed_up: BTreeSet<Vec<u8>>,
    umask: u32,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Ask {
    /// Resposta padrão (não há terminal pra perguntar).
    Default,
}

impl Run {
    fn new(o: Opts, argv0: String) -> Run {
        let msg_fd = if o.output.as_deref() == Some(b"-") { Fd::STDERR } else { Fd::STDOUT };
        let s = sys::current();
        let umask = s.umask(0o022);
        s.umask(umask);
        Run {
            o,
            argv0,
            out: Output::new(msg_fd),
            data: Output::stdout(),
            status: 0,
            outputs_started: BTreeSet::new(),
            rejects_started: BTreeSet::new(),
            backed_up: BTreeSet::new(),
            umask,
        }
    }

    fn q(&self, name: &[u8]) -> Vec<u8> {
        names::quote(name, self.o.quoting)
    }

    fn say(&mut self, msg: impl AsRef<[u8]>) {
        if !self.o.silent {
            self.out.write(msg.as_ref());
        }
    }

    fn say_always(&mut self, msg: impl AsRef<[u8]>) {
        self.out.write(msg.as_ref());
    }

    fn verbose(&mut self, msg: impl AsRef<[u8]>) {
        if self.o.verbose && !self.o.silent {
            self.out.write(msg.as_ref());
        }
    }

    /// Pergunta como o GNU faz sem terminal: escreve a pergunta, quebra a linha e fica com o padrão.
    fn ask(&mut self, prompt: &[u8]) -> Ask {
        self.out.write(prompt);
        self.out.write(b"\n");
        Ask::Default
    }

    fn read_patch(&mut self) -> Result<Vec<u8>, Fail> {
        let path = self.o.input.clone().or_else(|| self.o.positional.get(1).cloned());
        match path {
            Some(p) if p.as_slice() != b"-" => match sysutil::read_path(&p) {
                Ok(d) => Ok(d),
                Err(e) => {
                    let mut m = b"Can't open patch file ".to_vec();
                    m.extend_from_slice(&self.q(&p));
                    m.extend_from_slice(format!(" : {}", e.message()).as_bytes());
                    Err(m)
                }
            },
            _ => sysutil::read_fd(Fd::STDIN).map_err(|e| format!("read error : {}", e.message()).into_bytes()),
        }
    }

    fn main(&mut self) -> Result<(), Fail> {
        if let Some(dir) = self.o.directory.clone() {
            if let Err(e) = sys::current().chdir(&dir) {
                let mut m = b"Can't change to directory ".to_vec();
                m.extend_from_slice(&self.q(&dir));
                m.extend_from_slice(format!(" : {}", e.message()).as_bytes());
                return Err(m);
            }
        }
        let raw = self.read_patch()?;
        // O GNU abre a saída do -o logo no começo (fica vazia se nada for aplicado; um arquivo que já
        // existe é truncado e mantém o modo).
        if let Some(outp) = self.o.output.clone()
            && outp.as_slice() != b"-"
        {
            let fd = sys::open(&outp, OFlags::WRONLY | OFlags::CREAT | OFlags::TRUNC | OFlags::CLOEXEC, 0o600)
                .map_err(|e| self.write_error(&outp, e))?;
            let _ = sys::close(fd);
            self.outputs_started.insert(outp);
        }
        let first_line_crlf = raw.split_inclusive(|&c| c == b'\n').next().is_some_and(|l| l.ends_with(b"\r\n"));
        let text: Vec<u8> = if first_line_crlf && !self.o.binary {
            self.say("(Stripping trailing CRs from patch; use --binary to disable.)\n");
            let mut t = Vec::new();
            if t.try_reserve(raw.len()).is_err() {
                return Err(b"out of memory".to_vec());
            }
            for l in raw.split_inclusive(|&c| c == b'\n') {
                match l.strip_suffix(b"\r\n") {
                    Some(b) => {
                        t.extend_from_slice(b);
                        t.push(b'\n');
                    }
                    None => t.extend_from_slice(l),
                }
            }
            t
        } else {
            raw
        };
        let has_target = !self.o.positional.is_empty();
        let mut scanner = Scanner::new(&text, self.o.forced, has_target);
        let mut chunks = 0usize;
        while let Some(chunk) = scanner.next_chunk() {
            if self.o.verbose {
                let lead = if chunks == 0 { "Hmm...  Looks like" } else { "Hmm...  The next patch looks like" };
                self.verbose(format!("{lead} {} to me...\n", chunk.kind_text()));
            }
            chunks += 1;
            self.process(&chunk)?;
        }
        if chunks == 0 && !text.is_empty() {
            return Err(b"Only garbage was found in the patch input.".to_vec());
        }
        if chunks > 0 && scanner.has_trailing() && self.o.verbose {
            self.verbose("Hmm...  Ignoring the trailing garbage.\n");
        }
        if chunks > 0 {
            self.verbose("done\n");
        }
        Ok(())
    }

    /// Escolhe o arquivo alvo entre os nomes do cabeçalho (depois do `-p`), como o GNU fora do modo
    /// POSIX: entre os que existem, o de menos componentes, depois o de basename mais curto, depois o
    /// mais curto; no modo POSIX, o primeiro que existe.
    fn pick_existing(&mut self, cands: &[Vec<u8>]) -> Option<Vec<u8>> {
        let existing: Vec<&[u8]> = cands.iter().map(|c| c.as_slice()).filter(|c| sysutil::exists(c)).collect();
        if existing.is_empty() {
            return None;
        }
        if self.o.posix {
            return Some(existing[0].to_vec());
        }
        names::best_index(&existing).map(|i| existing[i].to_vec())
    }

    fn stripped(&mut self, h: &Option<HeaderName>, warned: &mut Vec<Vec<u8>>) -> Option<Vec<u8>> {
        let h = h.as_ref()?;
        if h.name == b"/dev/null" {
            return None;
        }
        let s = names::strip(&h.name, self.o.strip)?;
        match names::danger(&s) {
            Danger::Safe => Some(s),
            Danger::Absolute => {
                if !warned.contains(&s) {
                    let mut m = b"Ignoring potentially dangerous file name ".to_vec();
                    m.extend_from_slice(&self.q(&s));
                    m.push(b'\n');
                    self.say_always(m);
                    warned.push(s);
                }
                None
            }
            Danger::DotDot => None,
        }
    }

    fn process(&mut self, chunk: &Chunk) -> Result<(), Fail> {
        let git = chunk.git.clone();
        let mut old_h = chunk.old.clone();
        let mut new_h = chunk.new.clone();
        if let Some(g) = &git {
            if old_h.is_none() {
                old_h = if g.new_file_mode.is_some() {
                    Some(HeaderName { name: b"/dev/null".to_vec(), stamp: Vec::new() })
                } else {
                    g.a_name.clone().map(|name| HeaderName { name, stamp: Vec::new() })
                };
            }
            if new_h.is_none() {
                new_h = if g.deleted_file_mode.is_some() {
                    Some(HeaderName { name: b"/dev/null".to_vec(), stamp: Vec::new() })
                } else {
                    g.b_name.clone().map(|name| HeaderName { name, stamp: Vec::new() })
                };
            }
        }
        let old_none = old_h.as_ref().is_some_and(|h| h.says_nonexistent())
            || git.as_ref().is_some_and(|g| g.new_file_mode.is_some());
        let new_none = new_h.as_ref().is_some_and(|h| h.says_nonexistent())
            || git.as_ref().is_some_and(|g| g.deleted_file_mode.is_some());
        let mut warned = Vec::new();
        let old_s = self.stripped(&old_h, &mut warned);
        let new_s = self.stripped(&new_h, &mut warned);
        let index_s = chunk.index.as_ref().and_then(|i| names::strip(i, self.o.strip)).filter(|s| matches!(names::danger(s), Danger::Safe));
        let hunks: Vec<Hunk> = match &chunk.body {
            Body::Hunks { hunks, .. } => hunks.clone(),
            _ => Vec::new(),
        };
        let deferred_error = match &chunk.body {
            Body::Hunks { error, .. } => error.clone(),
            _ => None,
        };
        let mut reverse = self.o.reverse;
        let is_rename = git.as_ref().is_some_and(|g| g.rename_from.is_some());
        let is_copy = git.as_ref().is_some_and(|g| g.copy_from.is_some());

        // Alvo (arquivo lido) e destino (arquivo escrito).
        let explicit = self.o.positional.first().cloned();
        let creation_like = |rev: bool| -> bool {
            let side_none = if rev { new_none } else { old_none };
            side_none || hunks.first().is_some_and(|h| (if rev { &h.new } else { &h.old }).is_empty() && (if rev { h.new_first } else { h.old_first }) <= 1)
        };
        // Renomeação e cópia do git: com -R, o sentido se inverte.
        let (git_src, git_dst) = if reverse { (new_s.clone(), old_s.clone()) } else { (old_s.clone(), new_s.clone()) };
        if (is_rename || is_copy) && explicit.is_none() && !git_src.as_ref().is_some_and(|s| sysutil::exists(s)) {
            let what = if is_rename { "rename" } else { "copy" };
            self.say_always(format!("Cannot {what} file without two valid file names\n"));
            self.status = self.status.max(1);
            if let Some(e) = deferred_error {
                return Err(e.message());
            }
            return Ok(());
        }
        let target: Option<Vec<u8>> = if let Some(t) = &explicit {
            Some(t.clone())
        } else if is_rename || is_copy {
            git_src.clone()
        } else {
            let mut cands: Vec<Vec<u8>> = Vec::new();
            if self.o.posix {
                cands.extend(old_s.iter().cloned());
                cands.extend(new_s.iter().cloned());
                cands.extend(index_s.iter().cloned());
            } else {
                cands.extend(old_s.iter().cloned());
                cands.extend(new_s.iter().cloned());
                if cands.is_empty() {
                    cands.extend(index_s.iter().cloned());
                }
            }
            match self.pick_existing(&cands) {
                Some(t) => Some(t),
                None => {
                    let deletion = if reverse { old_none } else { new_none };
                    if creation_like(reverse) {
                        let pref = if reverse { old_s.clone().or(new_s.clone()) } else { new_s.clone().or(old_s.clone()) };
                        pref.or_else(|| index_s.clone())
                    } else if deletion {
                        if reverse { new_s.clone() } else { old_s.clone() }
                    } else {
                        None
                    }
                }
            }
        };
        let Some(target) = target else {
            return self.cant_find(chunk, &hunks, deferred_error);
        };
        let dest: Vec<u8> = if is_rename || is_copy { git_dst.clone().unwrap_or_else(|| target.clone()) } else { target.clone() };
        if !chunk.leading.is_empty() {
            self.verbose(leading_block(&chunk.leading));
        }

        let target_stat = sys::lstat(&target).ok();
        let exists = target_stat.is_some();
        let nonempty = target_stat.as_ref().is_some_and(|s| s.file_type() != FileType::Regular || s.size > 0);
        let mut apply_anyway = false;
        // Depois de decidir um conflito de criação/remoção, o GNU não tenta mais adivinhar inversão.
        let mut dwim = true;

        // Criação sobre arquivo que existe, remoção de arquivo que não existe.
        let creates = if reverse { new_none } else { old_none };
        let deletes = if reverse { old_none } else { new_none };
        let conflict = if creates && exists && nonempty {
            Some(("create the file", "which already exists!"))
        } else if deletes && !exists {
            Some(("delete the file", "which does not exist!"))
        } else {
            None
        };
        if let Some((what, why)) = conflict {
            dwim = false;
            let when = if reverse { ", when reversed," } else { "" };
            let mut m = format!("The next patch{when} would {what} ").into_bytes();
            m.extend_from_slice(&self.q(&target));
            m.extend_from_slice(format!(",\n{why}  ").as_bytes());
            if self.o.forward {
                m.extend_from_slice(b"Skipping patch.\n");
                self.say_always(m);
                return self.skip_hunks(&hunks, None, deferred_error, &target);
            } else if self.o.batch {
                m.extend_from_slice(if reverse { b"Ignoring -R.\n".as_slice() } else { b"Assuming -R.\n".as_slice() });
                self.say_always(m);
                reverse = !reverse;
            } else if self.o.force {
                m.extend_from_slice(b"Applying it anyway.\n");
                self.say_always(m);
                apply_anyway = true;
            } else {
                self.out.write(&m);
                let _ = self.ask(if reverse { b"Ignore -R? [n] ".as_slice() } else { b"Assume -R? [n] ".as_slice() });
                let _ = self.ask(b"Apply anyway? [n] ");
                self.say("Skipping patch.\n");
                return self.skip_hunks(&hunks, None, deferred_error, &target);
            }
        }

        // Arquivo que não é regular (diretório, ou symlink sem --follow-symlinks).
        if let Some(st) = &target_stat {
            let ft = st.file_type();
            let bad = ft == FileType::Directory || (ft == FileType::Symlink && !self.o.follow_symlinks) || !matches!(ft, FileType::Regular | FileType::Symlink);
            let symlink_patch = git.as_ref().is_some_and(|g| g.new_file_mode.or(g.new_mode).or(g.old_mode).is_some_and(|m| m & 0o170000 == 0o120000));
            if bad && !symlink_patch {
                let mut m = b"File ".to_vec();
                m.extend_from_slice(&self.q(&target));
                m.extend_from_slice(b" is not a regular file -- refusing to patch\n");
                self.say_always(m);
                let rej_hunks: Vec<Hunk> = hunks.iter().map(|h| if reverse { h.reversed() } else { h.clone() }).collect();
                return self.skip_hunks(&hunks, Some((&rej_hunks, &old_h, &new_h, reverse)), deferred_error, &target);
            }
        }

        match &chunk.body {
            Body::GitBinary => {
                let mut m = b"File ".to_vec();
                m.extend_from_slice(&self.q(&target));
                m.extend_from_slice(b": git binary diffs are not supported.\n");
                self.say_always(m);
                self.status = self.status.max(1);
                return Ok(());
            }
            Body::Ed(script) => return self.run_ed(&target, script),
            _ => {}
        }

        // Prereq: a palavra tem que aparecer no arquivo, separada por brancos.
        if let Some(word) = chunk.prereq() {
            let content = if exists { sysutil::read_path(&target).unwrap_or_default() } else { Vec::new() };
            if !has_word(&content, &word) {
                let w = String::from_utf8_lossy(&word).into_owned();
                if self.o.force {
                    self.say(format!("Warning: this file doesn't appear to be the {w} version -- patching anyway.\n"));
                } else if self.o.batch {
                    return Err(format!("This file doesn't appear to be the {w} version -- aborting.").into_bytes());
                } else {
                    let _ = self.ask(format!("This file doesn't appear to be the {w} version -- patch anyway? [n] ").as_bytes());
                    return Err(b"aborted".to_vec());
                }
            }
        }

        // Anúncio.
        let is_symlink_target = git.as_ref().is_some_and(|g| {
            g.new_file_mode.or(g.new_mode).is_some_and(|m| m & 0o170000 == 0o120000)
                || (g.deleted_file_mode.is_some_and(|m| m & 0o170000 == 0o120000))
        });
        {
            let verb = if self.o.dry_run { "checking" } else { "patching" };
            let kind = if is_symlink_target { "symbolic link" } else { "file" };
            let mut m = format!("{verb} {kind} ").into_bytes();
            match &self.o.output {
                Some(out) => {
                    m.extend_from_slice(&self.q(out));
                    m.extend_from_slice(b" (read from ");
                    m.extend_from_slice(&self.q(&target));
                    m.push(b')');
                }
                None => {
                    m.extend_from_slice(&self.q(&dest));
                    if is_rename {
                        m.extend_from_slice(b" (renamed from ");
                        m.extend_from_slice(&self.q(&target));
                        m.push(b')');
                    } else if is_copy {
                        m.extend_from_slice(b" (copied from ");
                        m.extend_from_slice(&self.q(&target));
                        m.push(b')');
                    }
                }
            }
            m.push(b'\n');
            self.say(m);
        }
        if matches!(chunk.body, Body::BinaryDiffer) {
            return Ok(());
        }

        // Conteúdo atual.
        let (input, input_stat): (Vec<u8>, Option<Stat>) = if exists {
            if target_stat.as_ref().is_some_and(|s| s.file_type() == FileType::Symlink) && is_symlink_target {
                let t = sys::current().readlinkat(Fd::CWD, &target).unwrap_or_default();
                (t, target_stat.clone())
            } else {
                match sysutil::read_path(&target) {
                    Ok(d) => (d, sys::stat(&target).ok()),
                    Err(e) => {
                        let mut m = b"Can't open file ".to_vec();
                        m.extend_from_slice(&self.q(&target));
                        m.extend_from_slice(format!(" : {}", e.message()).as_bytes());
                        return Err(m);
                    }
                }
            }
        } else {
            (Vec::new(), None)
        };

        let matcher = Matcher { ignore_whitespace: self.o.ignore_whitespace };
        let mut builder = Builder::new(&input, self.o.ifdef.clone());
        let mut cursor = Cursor::default();
        let mut net: isize = 0;
        let mut mismatch = false;
        let mut failed: Vec<Hunk> = Vec::new();
        let total = hunks.len();
        let file_crlf = builder.lines().first().is_some_and(|l| l.ends_with(b"\r\n"));
        let mut skipped_all = false;
        for (idx, raw_hunk) in hunks.iter().enumerate() {
            sys::checkpoint();
            let mut h = if reverse { raw_hunk.reversed() } else { raw_hunk.clone() };
            let n = idx + 1;
            cursor.frozen = builder.frozen();
            if idx == 0 && dwim && self.o.merge.is_none() && !self.o.force && !apply_anyway {
                let mut found = None;
                for f in 0..=self.o.fuzz {
                    if locate_level(builder.lines(), &h, &cursor, f, &matcher).is_some() {
                        found = Some(false);
                        break;
                    }
                    if locate_level(builder.lines(), &h.reversed(), &cursor, f, &matcher).is_some() {
                        found = Some(true);
                        break;
                    }
                }
                if found == Some(true) {
                    let what = if reverse { "Unreversed patch detected!  " } else { "Reversed (or previously applied) patch detected!  " };
                    if self.o.forward {
                        self.say_always(format!("{what}Skipping patch.\n"));
                        skipped_all = true;
                    } else if self.o.batch {
                        let assume = if reverse { "Ignoring -R." } else { "Assuming -R." };
                        self.say_always(format!("{what}{assume}\n"));
                        reverse = !reverse;
                        mismatch = true;
                        h = h.reversed();
                    } else {
                        self.out.write(what.as_bytes());
                        let q = if reverse { b"Ignore -R? [n] ".as_slice() } else { b"Assume -R? [n] ".as_slice() };
                        let _ = self.ask(q);
                        let _ = self.ask(b"Apply anyway? [n] ");
                        self.say("Skipping patch.\n");
                        skipped_all = true;
                    }
                    if skipped_all {
                        let rej: Vec<Hunk> = hunks.iter().map(|h| if reverse { h.reversed() } else { h.clone() }).collect();
                        return self.skip_hunks(&hunks, Some((&rej, &old_h, &new_h, reverse)), deferred_error, &target);
                    }
                }
            }
            if let Some(style) = self.o.merge {
                // --merge: sem fuzz, aplica direto; senão, fusão de três vias na melhor posição.
                if let Some(pos) = locate_level(builder.lines(), &h, &cursor, 0, &matcher) {
                    if builder.apply(&h, pos).is_ok() {
                        let offset = pos as isize - h.old_first as isize;
                        cursor.in_offset = offset;
                        if offset != 0 {
                            mismatch = true;
                        }
                        net += h.new.len() as isize - h.old.len() as isize;
                        continue;
                    }
                }
                let mut pos = None;
                for f in 1..=self.o.fuzz {
                    if let Some(p) = locate_level(builder.lines(), &h, &cursor, f, &matcher) {
                        pos = Some(p);
                        break;
                    }
                }
                let len = builder.lines().len() as isize;
                let pos = pos.unwrap_or_else(|| (h.old_first as isize + cursor.in_offset).clamp(1, len + 1) as usize);
                mismatch = true;
                match builder.merge(&h, pos, style == MergeStyle::Diff3) {
                    Ok(report) => {
                        cursor.in_offset = pos as isize - h.old_first as isize;
                        net += report.out_lines as isize - report.window as isize;
                        if report.conflict {
                            self.status = self.status.max(1);
                        }
                        let mut parts = Vec::new();
                        for (kind, ranges) in &report.parts {
                            if self.o.silent && *kind != apply::MergeKind::NotMerged {
                                continue;
                            }
                            let list: Vec<String> =
                                ranges.iter().map(|&(a, b)| if a == b { format!("{a}") } else { format!("{a}-{b}") }).collect();
                            parts.push(format!("{} at {}", kind.text(), list.join(",")));
                        }
                        if !parts.is_empty() {
                            self.say_always(format!("Hunk #{n} {}.\n", parts.join(", ")));
                        }
                    }
                    Err(apply::Misordered) => {
                        self.say_always("misordered hunks! output would be garbled\n");
                        let at = (h.old_first as isize + net).max(1);
                        self.say(format!("Hunk #{n} FAILED at {at}.\n"));
                        failed.push(h.shifted(net));
                    }
                }
                continue;
            }
            // Hunk de criação só casa em arquivo vazio.
            let creating_now = if reverse { new_none } else { old_none };
            let mut located = None;
            if !(creating_now && !input.is_empty() && h.old.is_empty()) {
                for f in 0..=self.o.fuzz {
                    if let Some(pos) = locate_level(builder.lines(), &h, &cursor, f, &matcher) {
                        located = Some((pos, f));
                        break;
                    }
                }
            }
            let mut ok = false;
            if let Some((pos, f)) = located {
                match builder.apply(&h, pos) {
                    Ok(()) => {
                        ok = true;
                        let offset = pos as isize - h.old_first as isize;
                        cursor.in_offset = offset;
                        let at = pos as isize + net;
                        if f > 0 || offset != 0 || self.o.verbose {
                            let mut m = format!("Hunk #{n} succeeded at {at}");
                            if f > 0 {
                                m.push_str(&format!(" with fuzz {f}"));
                            }
                            if offset != 0 {
                                m.push_str(&format!(" (offset {offset} line{})", if offset == 1 { "" } else { "s" }));
                            }
                            m.push_str(".\n");
                            self.say(m);
                        }
                        if f > 0 || offset != 0 {
                            mismatch = true;
                        }
                        net += h.new.len() as isize - h.old.len() as isize;
                    }
                    Err(apply::Misordered) => {
                        self.say_always("misordered hunks! output would be garbled\n");
                    }
                }
            }
            if !ok {
                mismatch = true;
                let at = (h.old_first as isize + net).max(1);
                let hunk_crlf = h.old.first().or(h.new.first()).is_some_and(|l| l.text.ends_with(b"\r\n"));
                let le = if file_crlf != hunk_crlf && !input.is_empty() { " (different line endings)" } else { "" };
                self.say(format!("Hunk #{n} FAILED at {at}{le}.\n"));
                failed.push(h.shifted(net));
            }
        }
        if let Some(e) = deferred_error {
            return Err(e.message());
        }
        let new_content = builder.finish();
        let failed_count = failed.len();
        let deleting = if reverse { old_none } else { new_none };

        if self.o.dry_run {
            if failed_count > 0 {
                self.status = self.status.max(1);
                self.say_always(format!("{failed_count} out of {total} hunk{} FAILED\n", plural(total)));
            }
            return Ok(());
        }

        // Rejeitos.
        let rej_path: Option<Vec<u8>> = if failed_count == 0 {
            None
        } else {
            match &self.o.reject_file {
                Some(r) if r.as_slice() == b"-" => None,
                Some(r) => Some(r.clone()),
                None => {
                    let base = self.o.output.clone().unwrap_or_else(|| dest.clone());
                    let mut p = base;
                    p.extend_from_slice(b".rej");
                    Some(p)
                }
            }
        };
        if let Some(rp) = &rej_path {
            let text = self.reject_text(&failed, &old_h, &new_h, reverse);
            self.write_reject(rp, &text)?;
        }

        // Backup e escrita.
        let backup_if_mismatch = self.o.backup_if_mismatch.unwrap_or(!self.o.posix);
        let want_backup = self.o.output.is_none() && (self.o.backup || (mismatch && backup_if_mismatch));
        if let Some(outp) = self.o.output.clone() {
            self.write_output(&outp, &new_content)?;
        } else {
            let file_mode = match (&input_stat, git.as_ref()) {
                (_, Some(g)) if g.new_mode.is_some() => g.new_mode.unwrap_or(0o644) & 0o7777,
                (Some(st), _) => st.perm(),
                (None, Some(g)) if g.new_file_mode.is_some() => g.new_file_mode.unwrap_or(0o644) & 0o7777,
                _ => 0o666 & !self.umask,
            };
            let moved = want_backup && self.make_backup(&target, exists, &input, input_stat.as_ref())?;
            let remove = (deleting && new_content.is_empty()) || (self.o.remove_empty && new_content.is_empty());
            if deleting && !new_content.is_empty() {
                if exists || is_rename {
                    self.write_file(&dest, &new_content, file_mode, exists && !moved)?;
                }
                let mut m = b"Not deleting file ".to_vec();
                m.extend_from_slice(&self.q(&dest));
                m.extend_from_slice(b" as content differs from patch\n");
                self.say_always(m);
                self.status = self.status.max(1);
            } else if remove {
                if exists && !moved {
                    let _ = sys::current().unlinkat(Fd::CWD, &dest, AtFlags::empty());
                }
                remove_empty_parents(&dest);
            } else if is_symlink_target && !deleting {
                let link_target = new_content.strip_suffix(b"\n").unwrap_or(&new_content).to_vec();
                if exists {
                    let _ = sys::current().unlinkat(Fd::CWD, &dest, AtFlags::empty());
                }
                backup::make_parents(&dest);
                if let Err(e) = sys::current().symlinkat(&link_target, Fd::CWD, &dest) {
                    return Err(self.write_error(&dest, e));
                }
            } else {
                self.write_file(&dest, &new_content, file_mode, exists && !moved && dest == target)?;
                if is_rename {
                    let _ = sys::current().unlinkat(Fd::CWD, &target, AtFlags::empty());
                    remove_empty_parents(&target);
                    self.verbose({
                        let mut m = b"Removing file ".to_vec();
                        m.extend_from_slice(&self.q(&target));
                        m.push(b'\n');
                        m
                    });
                }
            }
            if !remove && failed_count == 0 && (self.o.set_time || self.o.set_utc) {
                self.set_times(&dest, &old_h, &new_h, reverse, input_stat.as_ref());
            }
        }

        if failed_count > 0 {
            self.status = self.status.max(1);
            let mut m = format!("{failed_count} out of {total} hunk{} FAILED", plural(total)).into_bytes();
            if let Some(rp) = &rej_path {
                m.extend_from_slice(b" -- saving rejects to file ");
                m.extend_from_slice(&self.q(rp));
            }
            m.push(b'\n');
            self.say_always(m);
        }
        Ok(())
    }

    fn cant_find(&mut self, chunk: &Chunk, hunks: &[Hunk], error: Option<Fatal>) -> Result<(), Fail> {
        self.say(format!("can't find file to patch at input line {}\n", chunk.input_line));
        let has_names = chunk.old.is_some() || chunk.new.is_some() || chunk.index.is_some() || chunk.git.is_some();
        if has_names {
            if self.o.strip.is_some() {
                self.say("Perhaps you used the wrong -p or --strip option?\n");
            } else {
                self.say("Perhaps you should have used the -p or --strip option?\n");
            }
            let block = leading_block(&chunk.leading);
            self.say_always(block);
        }
        if self.o.batch || self.o.force {
            self.say("No file to patch.  Skipping patch.\n");
        } else {
            let _ = self.ask(b"File to patch: ");
            let _ = self.ask(b"Skip this patch? [y] ");
            self.say("Skipping patch.\n");
        }
        self.skip_hunks(hunks, None, error, b"")
    }

    /// Pula todos os hunks do arquivo; com `rej`, grava todos no rejeito.
    fn skip_hunks(
        &mut self,
        hunks: &[Hunk],
        rej: Option<(&[Hunk], &Option<HeaderName>, &Option<HeaderName>, bool)>,
        error: Option<Fatal>,
        target: &[u8],
    ) -> Result<(), Fail> {
        for (i, h) in hunks.iter().enumerate() {
            self.verbose(format!("Hunk #{} ignored at {}.\n", i + 1, h.old_first.max(1)));
        }
        if let Some(e) = error {
            return Err(e.message());
        }
        let total = hunks.len();
        self.status = self.status.max(1);
        if total == 0 {
            return Ok(());
        }
        let mut m = format!("{total} out of {total} hunk{} ignored", plural(total)).into_bytes();
        if let Some((rej_hunks, old_h, new_h, reverse)) = rej {
            let path: Option<Vec<u8>> = match &self.o.reject_file {
                Some(r) if r.as_slice() == b"-" => None,
                Some(r) => Some(r.clone()),
                None => {
                    let mut p = self.o.output.clone().unwrap_or_else(|| target.to_vec());
                    p.extend_from_slice(b".rej");
                    Some(p)
                }
            };
            if let Some(p) = path
                && !self.o.dry_run
            {
                m.extend_from_slice(b" -- saving rejects to file ");
                m.extend_from_slice(&self.q(&p));
                let text = self.reject_text(rej_hunks, old_h, new_h, reverse);
                self.write_reject(&p, &text)?;
            }
        }
        m.push(b'\n');
        self.say_always(m);
        Ok(())
    }

    /// Texto do rejeito: cabeçalho com os nomes (depois do `-p`) e datas, e os hunks.
    fn reject_text(&self, hunks: &[Hunk], old_h: &Option<HeaderName>, new_h: &Option<HeaderName>, reverse: bool) -> Vec<u8> {
        let label = |h: &Option<HeaderName>| -> Vec<u8> {
            match h {
                None => b"/dev/null".to_vec(),
                Some(h) => {
                    let mut v = if h.name == b"/dev/null" {
                        h.name.clone()
                    } else {
                        names::strip(&h.name, self.o.strip).unwrap_or_else(|| h.name.clone())
                    };
                    if !h.stamp.is_empty() {
                        v.push(b'\t');
                        v.extend_from_slice(&h.stamp);
                    }
                    v
                }
            }
        };
        let (a, b) = if reverse { (label(new_h), label(old_h)) } else { (label(old_h), label(new_h)) };
        let context = match self.o.reject_format {
            RejectFormat::Context => true,
            RejectFormat::Unified => false,
            RejectFormat::Auto => hunks.first().is_some_and(|h| h.format != Format::Unified),
        };
        let mut out = Vec::new();
        if context {
            out.extend_from_slice(b"*** ");
            out.extend_from_slice(&a);
            out.extend_from_slice(b"\n--- ");
            out.extend_from_slice(&b);
            out.push(b'\n');
            for h in hunks {
                if h.format == Format::Unified && self.o.reject_format == RejectFormat::Context {
                    let mut c = h.clone();
                    c.format = Format::Context;
                    c.write_context(&mut out);
                } else {
                    h.write_context(&mut out);
                }
            }
        } else {
            out.extend_from_slice(b"--- ");
            out.extend_from_slice(&a);
            out.extend_from_slice(b"\n+++ ");
            out.extend_from_slice(&b);
            out.push(b'\n');
            for h in hunks {
                h.write_unified(&mut out);
            }
        }
        out
    }

    fn write_reject(&mut self, path: &[u8], text: &[u8]) -> Result<(), Fail> {
        let append = self.o.reject_file.is_some() && self.rejects_started.contains(path);
        let flags = if append {
            OFlags::WRONLY | OFlags::APPEND | OFlags::CLOEXEC
        } else {
            OFlags::WRONLY | OFlags::CREAT | OFlags::TRUNC | OFlags::CLOEXEC
        };
        backup::make_parents(path);
        let fd = sys::open(path, flags, 0o666).map_err(|e| self.write_error(path, e))?;
        let r = sys::write_all(fd, text);
        let _ = sys::close(fd);
        r.map_err(|e| self.write_error(path, e))?;
        self.rejects_started.insert(path.to_vec());
        Ok(())
    }

    fn write_error(&self, path: &[u8], e: Errno) -> Vec<u8> {
        let mut m = b"Can't create file ".to_vec();
        m.extend_from_slice(&names::quote(path, self.o.quoting));
        m.extend_from_slice(format!(" : {}", e.message()).as_bytes());
        m
    }

    fn write_output(&mut self, path: &[u8], content: &[u8]) -> Result<(), Fail> {
        if path == b"-" {
            self.data.write(content);
            return Ok(());
        }
        let append = self.outputs_started.contains(path);
        if append {
            let fd = sys::open(path, OFlags::WRONLY | OFlags::APPEND | OFlags::CLOEXEC, 0).map_err(|e| self.write_error(path, e))?;
            let r = sys::write_all(fd, content);
            let _ = sys::close(fd);
            r.map_err(|e| self.write_error(path, e))?;
        } else {
            self.replace_file(path, content, 0o600)?;
            self.outputs_started.insert(path.to_vec());
        }
        Ok(())
    }

    /// Escreve num temporário ao lado e renomeia por cima (o arquivo vira um inode novo, como no GNU).
    fn replace_file(&mut self, path: &[u8], content: &[u8], mode: u32) -> Result<(), Fail> {
        backup::make_parents(path);
        let s = sys::current();
        let mut tmp = path.to_vec();
        let mut rnd = [0u8; 4];
        let _ = s.getrandom(&mut rnd);
        tmp.extend_from_slice(format!(".tmp{:08x}", u32::from_le_bytes(rnd)).as_bytes());
        let fd = s
            .openat(Fd::CWD, &tmp, OFlags::WRONLY | OFlags::CREAT | OFlags::EXCL | OFlags::CLOEXEC, 0o600)
            .map_err(|e| self.write_error(path, e))?;
        let w = sys::write_all(fd, content);
        let m = s.fchmod(fd, mode);
        let _ = s.close(fd);
        if let Err(e) = w.and(m) {
            let _ = s.unlinkat(Fd::CWD, &tmp, AtFlags::empty());
            return Err(self.write_error(path, e));
        }
        if let Err(e) = s.renameat2(Fd::CWD, &tmp, Fd::CWD, path, RenameFlags::empty()) {
            let _ = s.unlinkat(Fd::CWD, &tmp, AtFlags::empty());
            return Err(self.write_error(path, e));
        }
        Ok(())
    }

    fn write_file(&mut self, path: &[u8], content: &[u8], mode: u32, _in_place: bool) -> Result<(), Fail> {
        self.replace_file(path, content, mode)
    }

    /// Faz o backup do original (uma vez por arquivo por execução). Devolve se o original saiu do
    /// lugar (foi renomeado pro backup).
    fn make_backup(&mut self, target: &[u8], exists: bool, input: &[u8], st: Option<&Stat>) -> Result<bool, Fail> {
        if !self.backed_up.insert(target.to_vec()) {
            return Ok(false);
        }
        let name = backup::backup_name(&self.o, target);
        backup::make_parents(&name);
        let s = sys::current();
        if exists {
            if s.renameat2(Fd::CWD, target, Fd::CWD, &name, RenameFlags::empty()).is_ok() {
                return Ok(true);
            }
            let mode = st.map(|s| s.perm()).unwrap_or(0o644);
            self.replace_file(&name, input, mode)?;
            return Ok(false);
        }
        self.replace_file(&name, b"", 0o666 & !self.umask)?;
        Ok(false)
    }

    fn set_times(&mut self, dest: &[u8], old_h: &Option<HeaderName>, new_h: &Option<HeaderName>, reverse: bool, st: Option<&Stat>) {
        let (from, to) = if reverse { (new_h, old_h) } else { (old_h, new_h) };
        let default_off = if self.o.set_utc {
            Some(0)
        } else {
            let tz = crate::tz::local();
            let now = sys::current().clock_gettime(sysabi::Clock::Realtime).map(|t| t.sec).unwrap_or(0);
            Some(crate::tz::offset_seconds(now, &tz) as i64)
        };
        let Some(to_t) = to.as_ref().and_then(|h| names::parse_stamp(&h.stamp, default_off)) else { return };
        if let Some(st) = st
            && !self.o.force
        {
            let from_t = from.as_ref().and_then(|h| names::parse_stamp(&h.stamp, default_off));
            if from_t != Some((st.mtime.sec, st.mtime.nsec)) {
                let mut m = b"Not setting time of file ".to_vec();
                m.extend_from_slice(&self.q(dest));
                m.extend_from_slice(b" (time mismatch)\n");
                self.say_always(m);
                return;
            }
        }
        let t = TimeSpec { sec: to_t.0, nsec: to_t.1 };
        let _ = sys::current().utimensat(Fd::CWD, dest, SetTime::At(t), SetTime::At(t), AtFlags::empty());
    }

    /// Script do ed: o GNU entrega o script (mais `w` e `q`) pro `ed` rodando sobre uma cópia
    /// temporária do arquivo, via `sh -c`; sem `ed` no sistema, quem reclama é o shell e o patch sai
    /// com "ed FAILED".
    fn run_ed(&mut self, target: &[u8], script: &[u8]) -> Result<(), Fail> {
        let s = sys::current();
        let input = sysutil::read_path(target).unwrap_or_default();
        let mut rnd = [0u8; 4];
        let _ = s.getrandom(&mut rnd);
        let mut tmp = target.to_vec();
        tmp.extend_from_slice(format!(".o{:07x}", u32::from_le_bytes(rnd) & 0x0fff_ffff).as_bytes());
        let mut script_path = tmp.clone();
        script_path.extend_from_slice(b".ed");
        let mut full = script.to_vec();
        full.extend_from_slice(b"w\nq\n");
        let cleanup = |s: &std::sync::Arc<dyn sysabi::Syscalls>| {
            let _ = s.unlinkat(Fd::CWD, &tmp, AtFlags::empty());
            let _ = s.unlinkat(Fd::CWD, &script_path, AtFlags::empty());
        };
        if sysutil::write_file(&tmp, &input, 0o600).is_err() || sysutil::write_file(&script_path, &full, 0o600).is_err() {
            cleanup(&s);
            return Err(b"ed FAILED".to_vec());
        }
        let mut cmd = b"ed - ".to_vec();
        cmd.extend_from_slice(&names::quote(&tmp, Quoting::ShellAlways));
        self.out.flush();
        let spec = sysabi::SpawnSpec {
            path: b"/bin/sh".to_vec(),
            argv: vec![b"sh".to_vec(), b"-c".to_vec(), cmd],
            attrs: sysabi::ProcAttrs {
                fd_actions: vec![sysabi::FdAction::Open { fd: Fd::STDIN, path: script_path.clone(), flags: OFlags::RDONLY, mode: 0 }],
                ..sysabi::ProcAttrs::default()
            },
        };
        let ok = match s.spawn(spec) {
            Ok(pid) => matches!(s.wait4(sysabi::WaitTarget::Pid(pid), sysabi::WaitOptions::empty()), Ok(Some((_, sysabi::WaitStatus::Exited(0))))),
            Err(_) => false,
        };
        if !ok {
            cleanup(&s);
            return Err(b"ed FAILED".to_vec());
        }
        let result = sysutil::read_path(&tmp).unwrap_or_default();
        cleanup(&s);
        if !self.o.dry_run {
            let mode = sys::stat(target).map(|s| s.perm()).unwrap_or(0o666 & !self.umask);
            match self.o.output.clone() {
                Some(outp) => self.write_output(&outp, &result)?,
                None => self.replace_file(target, &result, mode)?,
            }
        }
        Ok(())
    }
}

/// `word` aparece em `text` com branco (ou começo/fim) dos dois lados.
fn has_word(text: &[u8], word: &[u8]) -> bool {
    if word.is_empty() || word.len() > text.len() {
        return false;
    }
    (0..=text.len() - word.len()).any(|i| {
        &text[i..i + word.len()] == word
            && (i == 0 || text[i - 1].is_ascii_whitespace())
            && text.get(i + word.len()).is_none_or(|c| c.is_ascii_whitespace())
    })
}

fn plural(n: usize) -> &'static str {
    if n == 1 { "" } else { "s" }
}

/// "The text leading up to this was:" com as linhas prefixadas por `|`.
fn leading_block(leading: &[Vec<u8>]) -> Vec<u8> {
    let mut m = b"The text leading up to this was:\n--------------------------\n".to_vec();
    for l in leading {
        m.push(b'|');
        m.extend_from_slice(l);
        if !l.ends_with(b"\n") {
            m.push(b'\n');
        }
    }
    m.extend_from_slice(b"--------------------------\n");
    m
}

/// Apaga os diretórios pais que ficaram vazios depois de remover um arquivo.
fn remove_empty_parents(path: &[u8]) {
    let s = sys::current();
    let mut p = path.to_vec();
    while let Some(i) = p.iter().rposition(|&c| c == b'/') {
        p.truncate(i);
        if p.is_empty() {
            break;
        }
        if s.unlinkat(Fd::CWD, &p, AtFlags::REMOVEDIR).is_err() {
            break;
        }
    }
}
