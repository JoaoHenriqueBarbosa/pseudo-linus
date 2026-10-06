//! Execução: texto (comando completo por comando completo), listas, pipelines, comandos simples,
//! compostos, funções e subshells.

use std::sync::Arc;

use sysabi::{AtFlags, Errno, Fd, FdAction, FileType, OFlags, Pid, ProcAttrs, SigDisposition, Signal, SpawnSpec, WaitOptions, WaitStatus, WaitTarget};

use crate::ast::*;
use crate::builtins::{self, Arg, AssignArg, AssignedValue};
use crate::parse::{Chunk, ParseEnv, Reader, SyntaxError};
use crate::redir::Undo;
use crate::shell::{Exec, Flow, Frame, Job, Shell, sys, write_fd};
use crate::vars::{Attrs, ScopeKind, Value};

/// Que tipo de texto está sendo executado (muda o tratamento de erro de sintaxe e de `Discard`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextKind {
    /// `bash -c`, script, entrada padrão: erro de sintaxe encerra o shell.
    Main,
    /// `eval`: erro de sintaxe vira status 2.
    Eval,
    /// `source`/`.`: idem, e `return` sai do arquivo.
    Source,
    /// Corpo de trap.
    Trap,
}

/// Limite de profundidade de chamadas de função (proteção da pilha).
const MAX_FUNC_DEPTH: u32 = 4000;

/// Elemento expandido de `nome=(...)`: chave opcional, se é `+=`, valor.
pub type ArrayItem = (Option<Vec<u8>>, bool, Vec<u8>);

/// Builtins especiais do POSIX (atribuições na frente persistem em modo POSIX; erros fatais).
pub fn is_special_builtin(name: &[u8]) -> bool {
    matches!(
        name,
        b"break" | b":" | b"." | b"continue" | b"eval" | b"exec" | b"exit" | b"export" | b"readonly" | b"return" | b"set"
            | b"shift" | b"times" | b"trap" | b"unset" | b"source"
    )
}

fn is_decl_name(name: &str) -> bool {
    matches!(name, "declare" | "typeset" | "local" | "export" | "readonly")
}

impl Shell {
    pub fn parse_env(&self) -> ParseEnv {
        ParseEnv { aliases: if self.opts.shopt("expand_aliases") { Some(self.aliases.clone()) } else { None }, posix: self.posix }
    }

    /// Imprime um erro de sintaxe com o prefixo do bash (`bash: -c: line N: `).
    pub fn report_syntax_error(&self, e: &SyntaxError) {
        let ename = self.error_name();
        let iname = self.input_name.to_string();
        let prefix_at = |line: Line| {
            if self.interactive {
                format!("{ename}: ")
            } else if ename == iname {
                format!("{ename}: line {line}: ")
            } else {
                format!("{ename}: {iname}: line {line}: ")
            }
        };
        let prefix = prefix_at(e.line);
        let mut out = format!("{prefix}{}\n", e.message);
        if let Some(ctx) = &e.context {
            out.push_str(&format!("{prefix}`{ctx}'\n"));
        }
        if let Some((line, message)) = &e.follow {
            out.push_str(&format!("{}{message}\n", prefix_at(*line)));
        }
        let _ = write_fd(Fd::STDERR, out.as_bytes());
    }

    /// Lê e executa um texto, um comando completo por vez.
    pub fn run_text(&mut self, src: &str, kind: TextKind, input_name: Arc<str>, first_line: Line) -> Exec {
        let saved_input = std::mem::replace(&mut self.input_name, input_name.clone());
        let source: Arc<str> = if kind == TextKind::Source { input_name.clone() } else { Arc::from("") };
        let mut reader = Reader::new(src.to_string(), first_line, source);
        let r = self.run_reader(&mut reader, kind);
        self.input_name = saved_input;
        r
    }

    fn run_reader(&mut self, reader: &mut Reader, kind: TextKind) -> Exec {
        let mut status = 0;
        loop {
            let env = self.parse_env();
            let chunk = match reader.next_chunk(&env) {
                None => break,
                Some(c) => c,
            };
            let parsed = match chunk {
                Chunk::Error(e) => {
                    self.lineno = e.line;
                    self.report_syntax_error(&e);
                    self.status = 2;
                    return match kind {
                        TextKind::Main if !self.interactive => Err(Flow::Exit(2)),
                        _ => Ok(2),
                    };
                }
                Chunk::Commands(p) => p,
            };
            for (line, delim) in &parsed.heredoc_eof {
                let eof_line = self.lineno.max(*line);
                let saved = self.lineno;
                self.lineno = eof_line + 1;
                self.error(format!("warning: here-document at line {line} delimited by end-of-file (wanted `{delim}')"));
                self.lineno = saved;
            }
            let generation = self.parse_generation;
            let n = parsed.program.commands.len();
            for (i, list) in parsed.program.commands.iter().enumerate() {
                if self.opts.get("noexec") && !self.interactive {
                    continue;
                }
                // `bash -c`: o último comando do texto, se for simples e sozinho, sofre exec no
                // próprio processo do shell (o `CMD_NO_FORK` do `parse_and_execute`).
                if self.dash_c && kind == TextKind::Main && !self.is_subshell && i + 1 == n && reader.at_end() {
                    self.exec_last = single_simple(list);
                }
                let r = self.exec_list(list);
                self.exec_last = false;
                self.cleanup_procsubs();
                match r {
                    Ok(st) => {
                        status = st;
                        self.status = st;
                    }
                    Err(Flow::Discard) => {
                        self.status = 1;
                        status = 1;
                        if self.is_subshell {
                            return Err(Flow::Exit(1));
                        }
                    }
                    Err(Flow::Break(_)) | Err(Flow::Continue(_)) if self.loop_depth == 0 => {
                        status = self.status;
                    }
                    Err(f) => return Err(f),
                }
                self.run_pending_traps()?;
                if self.parse_generation != generation && i + 1 < n {
                    // Aliases mudaram: o resto precisa ser lido de novo.
                    reader.seek_line(parsed.starts[i + 1]);
                    break;
                }
            }
        }
        Ok(status)
    }

    // ---- listas ----

    pub fn exec_list(&mut self, list: &List) -> Exec {
        let mut status = self.status;
        for item in &list.items {
            if item.background {
                status = self.exec_async(&item.and_or)?;
            } else {
                status = self.exec_and_or(&item.and_or)?;
            }
            self.status = status;
        }
        Ok(status)
    }

    fn exec_and_or(&mut self, ao: &AndOr) -> Exec {
        let n = ao.rest.len();
        let mut status = self.exec_pipeline_ctx(&ao.first, n > 0)?;
        for (i, (conn, p)) in ao.rest.iter().enumerate() {
            let run = match conn {
                Connector::And => status == 0,
                Connector::Or => status != 0,
            };
            if run {
                self.status = status;
                status = self.exec_pipeline_ctx(p, i + 1 < n)?;
            }
        }
        Ok(status)
    }

    fn exec_pipeline_ctx(&mut self, p: &Pipeline, ignore_errexit: bool) -> Exec {
        if ignore_errexit {
            self.errexit_off += 1;
        }
        let r = self.exec_pipeline(p);
        if ignore_errexit {
            self.errexit_off -= 1;
        }
        r
    }

    /// Roda um pipeline e aplica `!`, PIPESTATUS, pipefail, trap ERR e `set -e`.
    pub fn exec_pipeline(&mut self, p: &Pipeline) -> Exec {
        stacker::maybe_grow(64 * 1024, 2 * 1024 * 1024, || self.exec_pipeline_inner(p))
    }

    fn exec_pipeline_inner(&mut self, p: &Pipeline) -> Exec {
        if p.negated {
            self.errexit_off += 1;
        }
        let started = p.time.map(|_| self.times_now());
        let r = if p.commands.len() == 1 {
            let r = self.exec_command(&p.commands[0]);
            if let Ok(st) = r {
                self.set_pipestatus(&[st]);
            }
            r
        } else {
            self.exec_multi(p)
        };
        if p.negated {
            self.errexit_off -= 1;
        }
        if let (Some(t), Some(start)) = (p.time, started) {
            self.report_time(t, start);
        }
        let mut status = r?;
        if p.negated {
            status = if status == 0 { 1 } else { 0 };
        }
        self.status = status;
        if status != 0 && !p.negated && self.pipeline_checkable(p) {
            self.err_trap_and_errexit(status)?;
        }
        Ok(status)
    }

    /// Comandos compostos (fora subshell, `[[` e `((`) não disparam o `-e` por conta própria: quem
    /// falhou dentro deles já disparou.
    fn pipeline_checkable(&self, p: &Pipeline) -> bool {
        if p.commands.len() > 1 {
            return true;
        }
        match &p.commands[0] {
            Command::Simple(_) => true,
            Command::FunctionDef(_) => false,
            Command::Compound(c, _) => matches!(c.kind, CompoundKind::Subshell(_) | CompoundKind::Cond(_) | CompoundKind::Arith(_)),
        }
    }

    /// Trap ERR e `set -e` depois de um comando que falhou fora de contexto ignorado.
    pub fn err_trap_and_errexit(&mut self, status: i32) -> Result<(), Flow> {
        if self.errexit_off > 0 {
            return Ok(());
        }
        let in_func = self.in_function();
        if let Some(cmd) = self.traps.err.clone()
            && (!in_func || self.opts.get("errtrace")) && self.in_trap == 0 {
                self.run_trap_command(&cmd, status)?;
                self.status = status;
            }
        if self.opts.get("errexit") {
            return Err(Flow::Exit(status));
        }
        Ok(())
    }

    fn set_pipestatus(&mut self, sts: &[i32]) {
        self.pipestatus = sts.to_vec();
        let v = self.vars.global_entry("PIPESTATUS");
        v.value = Value::Indexed(sts.iter().enumerate().map(|(i, s)| (i as i64, s.to_string().into_bytes())).collect());
        v.attrs.set(Attrs::INDEXED);
    }

    /// Pipeline de vários comandos: cada um num processo (o último no shell com `lastpipe`).
    fn exec_multi(&mut self, p: &Pipeline) -> Exec {
        let s = sys();
        let n = p.commands.len();
        let lastpipe = self.opts.shopt("lastpipe") && !self.opts.get("monitor");
        let mut pids: Vec<Option<Pid>> = Vec::with_capacity(n);
        let mut prev_read: Option<Fd> = None;
        let mut last_status: Option<i32> = None;
        for (i, cmd) in p.commands.iter().enumerate() {
            let is_last = i + 1 == n;
            let pipe = if is_last {
                None
            } else {
                match s.pipe2(OFlags::CLOEXEC) {
                    Ok(p) => Some(p),
                    Err(e) => {
                        self.error(format!("pipe error: {}", e.message()));
                        if let Some(r) = prev_read {
                            let _ = s.close(r);
                        }
                        return Ok(1);
                    }
                }
            };
            if is_last && lastpipe {
                // O último comando roda no próprio shell, lendo do pipe.
                let mut undo = Undo::default();
                if let Some(r) = prev_read.take() {
                    self.save_stdin_for_lastpipe(r, &mut undo);
                }
                let r = self.exec_command(cmd);
                self.undo_redirects(undo);
                match r {
                    Ok(st) => last_status = Some(st),
                    Err(f) => {
                        self.reap_pipeline(&pids);
                        return Err(f);
                    }
                }
                pids.push(None);
                break;
            }
            let mut actions = Vec::new();
            if let Some(r) = prev_read {
                actions.push(FdAction::Dup2 { from: r, to: Fd::STDIN });
                actions.push(FdAction::Close(r));
            }
            if let Some((r, w)) = pipe {
                actions.push(FdAction::Dup2 { from: w, to: Fd::STDOUT });
                actions.push(FdAction::Close(w));
                actions.push(FdAction::Close(r));
            }
            let child_cmd = cmd.clone();
            let mut child = self.subshell_clone();
            // Como no `&`: elemento que é comando simples sofre exec no próprio processo do pipeline.
            child.exec_last = matches!(cmd, Command::Simple(_));
            let attrs = ProcAttrs { fd_actions: actions, reset_signals: self.trapped_signals(), ..ProcAttrs::default() };
            let spawned = s.spawn_fn(attrs, b"bash".to_vec(), Box::new(move || child.run_subshell_command(&child_cmd)));
            if let Some(r) = prev_read.take() {
                let _ = s.close(r);
            }
            if let Some((r, w)) = pipe {
                let _ = s.close(w);
                prev_read = Some(r);
            }
            match spawned {
                Ok(pid) => pids.push(Some(pid)),
                Err(e) => {
                    self.error(format!("fork: {}", e.message()));
                    pids.push(None);
                }
            }
        }
        if let Some(r) = prev_read {
            let _ = s.close(r);
        }
        let mut statuses = Vec::with_capacity(n);
        for (i, pid) in pids.iter().enumerate() {
            match pid {
                Some(pid) => {
                    let st = self.wait_pid_cmd(*pid, &p.commands[i], i + 1 == n);
                    statuses.push(st);
                }
                None => statuses.push(last_status.unwrap_or(1)),
            }
        }
        self.set_pipestatus(&statuses);
        let status = if self.opts.get("pipefail") {
            statuses.iter().rev().find(|s| **s != 0).copied().unwrap_or(0)
        } else {
            *statuses.last().unwrap_or(&0)
        };
        Ok(status)
    }

    fn save_stdin_for_lastpipe(&mut self, r: Fd, undo: &mut Undo) {
        let redirect = Redirect {
            fd: RedirFd::Num(0),
            op: RedirOp::DupIn,
            target: RedirTarget::Word(crate::word::literal_word(&format!("{}-", r.0))),
        };
        let _ = self.apply_redirects(std::slice::from_ref(&redirect), Some(undo));
    }

    fn reap_pipeline(&mut self, pids: &[Option<Pid>]) {
        for pid in pids.iter().flatten() {
            let _ = self.wait_pid(*pid);
        }
    }

    /// Espera um filho e devolve o status do shell (128+sinal). Roda traps se o `wait4` for
    /// interrompido.
    pub fn wait_pid(&mut self, pid: Pid) -> i32 {
        match self.wait_raw(pid) {
            Some(st) => st.shell_status(),
            None => 127,
        }
    }

    fn wait_raw(&mut self, pid: Pid) -> Option<WaitStatus> {
        let s = sys();
        loop {
            match s.wait4(WaitTarget::Pid(pid), WaitOptions::empty()) {
                Ok(Some((_, st))) => return Some(st),
                Ok(None) => {
                    s.sched_yield();
                }
                Err(Errno::EINTR) => {
                    let _ = self.run_pending_traps();
                }
                Err(_) => return None,
            }
        }
    }

    /// Espera um comando de primeiro plano e imprime a morte por sinal como o bash.
    fn wait_pid_cmd(&mut self, pid: Pid, cmd: &Command, report: bool) -> i32 {
        let Some(st) = self.wait_raw(pid) else { return 127 };
        if report {
            self.report_signal_death(pid, st, &crate::print::command_text(cmd));
        }
        st.shell_status()
    }

    pub fn report_signal_death(&self, pid: Pid, st: WaitStatus, text: &str) {
        if let WaitStatus::Signaled { signal, core_dumped } = st {
            if signal == Signal::SIGINT || signal == Signal::SIGPIPE || self.xtrace_level > 0 {
                return;
            }
            if signal == Signal::SIGTERM {
                let _ = write_fd(Fd::STDERR, format!("{}\n", signal.description()).as_bytes());
                return;
            }
            let core = if core_dumped { "(core dumped) " } else { "" };
            let line = format!("{}{pid:5} {:<24}{core}{text}\n", self.error_prefix(), signal.description());
            let _ = write_fd(Fd::STDERR, line.as_bytes());
        }
    }

    // ---- subshells ----

    /// Cópia do estado pra rodar num processo filho (subshell, pipeline, `$(...)`).
    pub fn subshell_clone(&self) -> Shell {
        let mut c = self.clone();
        c.is_subshell = true;
        c.subshell += 1;
        c.jobs.clear();
        c.procsub_fds.clear();
        c.procsub_pids.clear();
        c.exit_trap_done = false;
        // O que o `trap -p` mostra: os traps do shell de origem, enquanto nenhum subshell no
        // caminho tiver mudado algum (subshells aninhados herdam a mesma visão).
        let mut display = match &self.traps.inherited_display {
            Some(original) => (**original).clone(),
            None => self.traps.clone(),
        };
        display.inherited_display = None;
        let keep_err = self.opts.get("errtrace");
        let keep_debug = self.opts.get("functrace");
        c.traps.signals.retain(|_, v| v.is_empty());
        c.traps.signals.remove(&crate::shell::TRAP_EXIT);
        if !keep_err {
            c.traps.err = None;
        }
        if !keep_debug {
            c.traps.debug = None;
            c.traps.ret = None;
        }
        c.traps.inherited_display = Some(Box::new(display));
        c
    }

    /// Sinais com trap (não ignorados): voltam ao padrão no filho.
    pub fn trapped_signals(&self) -> Vec<Signal> {
        self.traps.signals.iter().filter(|(n, v)| **n > 0 && !v.is_empty()).map(|(n, _)| Signal(*n)).collect()
    }

    fn enter_child(&mut self) {
        let s = sys();
        for fd in std::mem::take(&mut self.saved_fds) {
            let _ = s.close(fd);
        }
    }

    /// Corpo de um processo filho que roda um comando (estágio de pipeline).
    pub fn run_subshell_command(&mut self, cmd: &Command) -> i32 {
        self.enter_child();
        let r = self.exec_command(cmd);
        self.finish_subshell(r)
    }

    /// Corpo de um subshell que roda uma lista (`( ... )`, `<(...)`).
    pub fn run_subshell_list(&mut self, list: &List) -> i32 {
        self.enter_child();
        let r = self.exec_list(list);
        self.finish_subshell(r)
    }

    /// Corpo de `$(...)`.
    pub fn run_subshell_program(&mut self, program: &Program) -> i32 {
        self.enter_child();
        let mut status = 0;
        for list in &program.commands {
            match self.exec_list(list) {
                Ok(st) => {
                    status = st;
                    self.status = st;
                }
                Err(f) => return self.finish_subshell(Err(f)),
            }
        }
        self.finish_subshell(Ok(status))
    }

    fn finish_subshell(&mut self, r: Exec) -> i32 {
        let status = match r {
            Ok(st) => st,
            Err(Flow::Exit(n)) | Err(Flow::Return(n)) => n,
            Err(Flow::Discard) => 1,
            Err(Flow::Break(_)) | Err(Flow::Continue(_)) => self.status,
        };
        self.exit_shell(status)
    }

    /// Fim do shell (ou subshell): roda o trap EXIT e devolve o status final.
    pub fn exit_shell(&mut self, status: i32) -> i32 {
        self.status = status;
        if self.exit_trap_done {
            return status & 0xff;
        }
        self.exit_trap_done = true;
        if let Some(cmd) = self.traps.signals.get(&crate::shell::TRAP_EXIT).cloned() {
            self.traps.signals.remove(&crate::shell::TRAP_EXIT);
            if let Err(Flow::Exit(n)) = self.run_trap_command(&cmd, status) { return n & 0xff }
        }
        status & 0xff
    }

    // ---- comandos ----

    pub fn exec_command(&mut self, cmd: &Command) -> Exec {
        match cmd {
            Command::Simple(s) => self.exec_simple(s),
            Command::Compound(c, redirs) => {
                if redirs.is_empty() {
                    return self.exec_compound(c);
                }
                self.lineno = c.line;
                let mut undo = Undo::default();
                match self.apply_redirects(redirs, Some(&mut undo))? {
                    Ok(()) => {}
                    Err(_) => return Ok(1),
                }
                let r = self.exec_compound(c);
                self.undo_redirects(undo);
                r
            }
            Command::FunctionDef(f) => {
                self.funcs.insert(f.name.clone(), f.clone());
                Ok(0)
            }
        }
    }

    /// Expande o argumento de um builtin de declaração (`declare x=$v`, `local -a a=(...)`).
    fn expand_decl_arg(&mut self, a: &Assign) -> Result<AssignArg, Flow> {
        let index = match &a.index {
            Some(w) => Some(self.expand_word_string(w)?),
            None => None,
        };
        let value = match &a.value {
            AssignValue::Scalar(w) => AssignedValue::Scalar(self.expand_word_string(w)?),
            AssignValue::Array(elems) => AssignedValue::Array(self.expand_array_elems(elems)?),
        };
        Ok(AssignArg { name: a.name.clone(), index, append: a.append, value, raw: a.raw.to_string() })
    }

    pub fn expand_array_elems(&mut self, elems: &[ArrayElem]) -> Result<Vec<ArrayItem>, Flow> {
        let mut out = Vec::new();
        for e in elems {
            match &e.key {
                Some(k) => {
                    let key = self.expand_word_string(k)?;
                    let v = self.expand_word_string(&e.value)?;
                    out.push((Some(key), e.append, v));
                }
                None => {
                    for v in self.expand_word_fields(&e.value)? {
                        out.push((None, false, v));
                    }
                }
            }
        }
        Ok(out)
    }

    /// Faz uma atribuição de comando (prefixo ou comando só de atribuições).
    pub fn do_assign(&mut self, a: &Assign, temp: bool) -> Result<bool, Flow> {
        let arg = self.expand_decl_arg(a)?;
        if self.opts.get("xtrace") {
            self.xtrace_line(&[crate::print::assign_trace(&arg)]);
        }
        if temp {
            // Ambiente temporário de builtin ou função: vale só durante o comando, exportado.
            let name = arg.name.clone();
            if !self.check_writable(&name) {
                return Ok(false);
            }
            let v = match arg.value {
                AssignedValue::Scalar(v) => v,
                AssignedValue::Array(_) => Vec::new(),
            };
            let old = self.vars.get(&name).cloned();
            let attrs = old.as_ref().map(|o| o.attrs).unwrap_or_default();
            let value = self.convert_value(&name, attrs, old.as_ref().and_then(|o| o.scalar_value()), v, arg.append)?;
            let e = self.vars.top_entry(&name);
            e.value = Value::Scalar(value);
            e.attrs = attrs;
            e.attrs.set(Attrs::EXPORT);
            return Ok(true);
        }
        self.apply_assign_arg(&arg)
    }

    /// Aplica uma atribuição já expandida no escopo normal.
    pub fn apply_assign_arg(&mut self, arg: &AssignArg) -> Result<bool, Flow> {
        match (&arg.index, &arg.value) {
            (Some(idx), AssignedValue::Scalar(v)) => self.assign_element(&arg.name, idx, v.clone(), arg.append),
            (None, AssignedValue::Scalar(v)) => self.assign_scalar(&arg.name, v.clone(), arg.append),
            (_, AssignedValue::Array(items)) => self.assign_array(&arg.name, items, arg.append),
        }
    }

    /// `nome=(...)`.
    pub fn assign_array(&mut self, name: &str, items: &[ArrayItem], append: bool) -> Result<bool, Flow> {
        let name = self.resolve_nameref(name);
        if !self.check_writable(&name) {
            return Ok(false);
        }
        let is_assoc = self.vars.get(&name).is_some_and(|v| v.attrs.has(Attrs::ASSOC) || matches!(v.value, Value::Assoc(_)));
        if is_assoc {
            if !append {
                let v = self.vars.entry(&name);
                v.value = Value::Assoc(crate::vars::Assoc::new());
            }
            // Associativo: `[k]=v` (ou, no bash 5.1+, pares k v sem colchete).
            let mut pending_key: Option<Vec<u8>> = None;
            for (k, app, v) in items {
                match k {
                    Some(key) => {
                        self.assign_element(&name, key, v.clone(), *app)?;
                    }
                    None => match pending_key.take() {
                        Some(key) => {
                            self.assign_element(&name, &key, v.clone(), false)?;
                        }
                        None => pending_key = Some(v.clone()),
                    },
                }
            }
            if let Some(key) = pending_key {
                self.assign_element(&name, &key, Vec::new(), false)?;
            }
            return Ok(true);
        }
        let mut next: i64 = 0;
        if append {
            if let Some(v) = self.vars.get(&name) {
                next = match &v.value {
                    Value::Indexed(m) => m.keys().next_back().map_or(0, |k| k + 1),
                    Value::Scalar(_) => 1,
                    _ => 0,
                };
            }
        } else {
            let attrs = self.vars.get(&name).map(|v| v.attrs).unwrap_or_default();
            let v = self.vars.entry(&name);
            v.value = Value::Indexed(Default::default());
            v.attrs = attrs;
            v.attrs.set(Attrs::INDEXED);
        }
        // Garante que é array indexado mesmo vazio.
        {
            let v = self.vars.entry(&name);
            match &v.value {
                Value::Indexed(_) => {}
                Value::Scalar(s) => {
                    let s = s.clone();
                    v.value = Value::Indexed([(0, s)].into_iter().collect());
                }
                _ => v.value = Value::Indexed(Default::default()),
            }
            v.attrs.set(Attrs::INDEXED);
        }
        for (k, app, v) in items {
            let idx = match k {
                Some(key) => {
                    let i = self.arith_eval(key)?;
                    match self.resolve_index(&name, i) {
                        Some(i) => i,
                        None => {
                            self.error(format!("{name}[{}]: bad array subscript", String::from_utf8_lossy(key)));
                            continue;
                        }
                    }
                }
                None => next,
            };
            self.assign_element(&name, idx.to_string().as_bytes(), v.clone(), *app)?;
            next = idx + 1;
        }
        Ok(true)
    }

    pub fn exec_simple(&mut self, s: &Simple) -> Exec {
        // Só este comando: as substituições dentro dele e os seguintes não herdam.
        let exec_last = std::mem::take(&mut self.exec_last);
        self.lineno = s.line;
        self.last_cmdsub_status = None;
        // Durante um trap o `BASH_COMMAND` continua sendo o comando que disparou o trap.
        if self.in_trap == 0 {
            self.current_command = crate::print::simple_text(s);
        }
        if let Some(dbg) = self.traps.debug.clone()
            && self.in_trap == 0 && (!self.in_function() || self.opts.get("functrace")) {
                self.run_trap_command(&dbg, self.status)?;
            }
        // 1. Palavras.
        let decl = s.words.first().is_some_and(|w| is_decl_name(&w.raw));
        let mut args: Vec<Arg> = Vec::with_capacity(s.words.len());
        for (i, w) in s.words.iter().enumerate() {
            if decl && i > 0
                && let Some(a) = &w.assign {
                    args.push(Arg::Assign(self.expand_decl_arg(a)?));
                    continue;
                }
            for f in self.expand_word_fields(w)? {
                args.push(Arg::Word(f));
            }
        }
        // 2. Sem comando: atribuições no shell e redireções sem efeito duradouro.
        if args.is_empty() {
            let mut ok = true;
            for a in &s.assigns {
                if !self.do_assign(a, false)? {
                    ok = false;
                }
            }
            let mut undo = Undo::default();
            let redir_ok = self.apply_redirects(&s.redirects, Some(&mut undo))?.is_ok();
            self.undo_redirects(undo);
            let st = if !ok || !redir_ok { 1 } else { self.last_cmdsub_status.unwrap_or(0) };
            if !ok && !self.interactive && self.posix {
                return Err(Flow::Exit(1));
            }
            return Ok(st);
        }
        let name = args[0].as_bytes().to_vec();
        let name_str = String::from_utf8_lossy(&name).into_owned();

        // 3. Que comando é.
        let kind = self.resolve_command(&name);

        // 4. Atribuições: ambiente temporário (builtin, função) ou do filho (externo).
        let mut env_extra: Vec<Vec<u8>> = Vec::new();
        let mut pushed_temp = false;
        let persist = self.posix && matches!(kind, CmdKind::Builtin) && is_special_builtin(&name);
        if !s.assigns.is_empty() {
            match kind {
                CmdKind::External(_) | CmdKind::NotFound => {
                    // As atribuições valem também pras expansões seguintes (`a=1 b=$a cmd`).
                    self.push_temp_scope();
                    for a in &s.assigns {
                        if !self.do_assign(a, true)? {
                            self.vars.pop();
                            return Ok(1);
                        }
                    }
                    if let Some(scope) = self.vars.scopes().last() {
                        for (k, v) in &scope.map {
                            if let Value::Scalar(val) = &v.value {
                                let mut e = k.clone().into_bytes();
                                e.push(b'=');
                                e.extend_from_slice(val);
                                env_extra.push(e);
                            }
                        }
                    }
                    self.vars.pop();
                }
                _ => {
                    if persist {
                        for a in &s.assigns {
                            self.do_assign(a, false)?;
                        }
                    } else {
                        self.push_temp_scope();
                        pushed_temp = true;
                        for a in &s.assigns {
                            if !self.do_assign(a, true)? {
                                self.vars.pop();
                                return Ok(1);
                            }
                        }
                    }
                }
            }
        }

        // 5. xtrace.
        if self.opts.get("xtrace") {
            let words: Vec<Vec<u8>> = args.iter().map(|a| a.trace()).collect();
            self.xtrace_line(&words);
        }

        // 6. Redireções. `exec` sem comando as torna permanentes no shell.
        let mut undo = Undo::default();
        let permanent = matches!(kind, CmdKind::Builtin) && name_str == "exec" && args.len() == 1;
        let undo_ref = if permanent { None } else { Some(&mut undo) };
        let redir_ok = match self.apply_redirects(&s.redirects, undo_ref) {
            Ok(r) => r.is_ok(),
            Err(f) => {
                if pushed_temp {
                    self.vars.pop();
                }
                return Err(f);
            }
        };
        if !redir_ok {
            if pushed_temp {
                self.vars.pop();
            }
            return Ok(1);
        }

        // 7. Executa.
        let r = match kind {
            CmdKind::Function(f) => {
                let argv: Vec<Vec<u8>> = args.into_iter().map(Arg::into_bytes).collect();
                self.call_function(f, argv)
            }
            CmdKind::Builtin => builtins::run(self, &name_str, &args),
            CmdKind::External(path) => {
                let argv: Vec<Vec<u8>> = args.into_iter().map(Arg::into_bytes).collect();
                if exec_last && !self.traps.signals.contains_key(&crate::shell::TRAP_EXIT) {
                    return self.exec_external_in_place(&path, &argv, env_extra);
                }
                self.run_external(&path, &argv, env_extra, s)
            }
            CmdKind::NotFound => {
                if let Some(handler) = self.funcs.get("command_not_found_handle").cloned() {
                    let mut argv = vec![b"command_not_found_handle".to_vec()];
                    argv.extend(args.into_iter().map(Arg::into_bytes));
                    self.call_function(handler, argv)
                } else {
                    // O dash diz só "not found", sem o "command" do bash.
                    if self.dash_style() {
                        self.error(format!("{name_str}: not found"));
                    } else {
                        self.error(format!("{name_str}: command not found"));
                    }
                    Ok(127)
                }
            }
        };
        self.undo_redirects(undo);
        if pushed_temp {
            self.vars.pop();
        }
        r
    }

    /// Imprime uma linha de xtrace: PS4 expandido (o primeiro caractere repetido por nível) e as
    /// palavras já citadas.
    pub fn xtrace_line(&mut self, words: &[Vec<u8>]) {
        let ps4 = self.var_bytes("PS4").map(|v| v.to_vec()).unwrap_or_default();
        let saved = self.opts.get("xtrace");
        self.opts.set("xtrace", false);
        let expanded = match crate::word::parse_word(&String::from_utf8_lossy(&ps4), crate::word::WordOpts::mode(crate::word::Mode::HereDoc, self.lineno)) {
            Ok(parts) => self.expand_parts_string(&parts).unwrap_or(ps4.clone()),
            Err(_) => ps4.clone(),
        };
        self.opts.set("xtrace", saved);
        let mut line = Vec::new();
        if let Some(first) = expanded.first() {
            for _ in 0..self.xtrace_level {
                line.push(*first);
            }
        }
        line.extend_from_slice(&expanded);
        for (i, w) in words.iter().enumerate() {
            if i > 0 {
                line.push(b' ');
            }
            line.extend_from_slice(w);
        }
        line.push(b'\n');
        let _ = write_fd(Fd::STDERR, &line);
    }

    /// Classifica o nome do comando.
    fn resolve_command(&mut self, name: &[u8]) -> CmdKind {
        if name.contains(&b'/') {
            return CmdKind::External(name.to_vec());
        }
        let n = String::from_utf8_lossy(name);
        if self.posix && is_special_builtin(name) && builtins::is_builtin(self, &n) {
            return CmdKind::Builtin;
        }
        if let Some(f) = self.funcs.get(n.as_ref()) {
            return CmdKind::Function(f.clone());
        }
        if builtins::is_builtin(self, &n) {
            return CmdKind::Builtin;
        }
        match self.find_in_path(name, true) {
            Some(p) => CmdKind::External(p),
            None => CmdKind::NotFound,
        }
    }

    /// Procura no PATH (usando e preenchendo a tabela do `hash`).
    pub fn find_in_path(&mut self, name: &[u8], use_hash: bool) -> Option<Vec<u8>> {
        let key = String::from_utf8_lossy(name).into_owned();
        if use_hash
            && let Some((p, hits)) = self.hash.get_mut(&key) {
                *hits += 1;
                let p = p.clone();
                if is_executable_file(&p) {
                    return Some(p);
                }
                self.hash.remove(&key);
            }
        let found = self.search_path(name);
        if let Some(p) = &found
            && use_hash && self.opts.get("hashall") {
                self.hash.insert(key, (p.clone(), 1));
            }
        found
    }

    /// Busca pura no PATH: primeiro arquivo executável (não diretório); se nenhum, o primeiro
    /// arquivo que existe (pra dar "Permission denied").
    pub fn search_path(&self, name: &[u8]) -> Option<Vec<u8>> {
        let path = self.var_bytes("PATH").map(|p| p.to_vec()).unwrap_or_default();
        let mut first_existing = None;
        for dir in path.split(|c| *c == b':') {
            let mut cand = if dir.is_empty() { b".".to_vec() } else { dir.to_vec() };
            cand.push(b'/');
            cand.extend_from_slice(name);
            match sys().fstatat(Fd::CWD, &cand, AtFlags::empty()) {
                Ok(st) if st.file_type() == FileType::Directory => continue,
                Ok(st) => {
                    if st.mode & 0o111 != 0 {
                        return Some(cand);
                    }
                    if first_existing.is_none() {
                        first_existing = Some(cand);
                    }
                }
                Err(_) => {}
            }
        }
        first_existing
    }

    /// O ambiente exportado com as atribuições do próprio comando por cima.
    fn external_env(&self, env_extra: Vec<Vec<u8>>) -> Vec<Vec<u8>> {
        let mut env = self.export_env();
        for e in env_extra {
            let eq = e.iter().position(|c| *c == b'=').unwrap_or(e.len());
            let key = e[..=eq.min(e.len() - 1)].to_vec();
            env.retain(|x| !x.starts_with(&key));
            env.push(e);
        }
        env
    }

    /// Último comando do subshell de um `&`: o programa substitui o processo. Se o `execve` falhar,
    /// os erros são os de sempre e o subshell termina com o status deles.
    fn exec_external_in_place(&mut self, path: &[u8], argv: &[Vec<u8>], env_extra: Vec<Vec<u8>>) -> Exec {
        let env = self.external_env(env_extra);
        let e = sys().execve(path, argv, Some(&env));
        self.exec_failed(path, argv, &env, e)
    }

    /// Roda um programa externo e espera.
    fn run_external(&mut self, path: &[u8], argv: &[Vec<u8>], env_extra: Vec<Vec<u8>>, s: &Simple) -> Exec {
        let env = self.external_env(env_extra);
        let spec = SpawnSpec {
            path: path.to_vec(),
            argv: argv.to_vec(),
            attrs: ProcAttrs { env: Some(env.clone()), reset_signals: self.trapped_signals(), ..ProcAttrs::default() },
        };
        match sys().spawn(spec) {
            Ok(pid) => {
                let st = self.wait_raw(pid);
                let Some(st) = st else { return Ok(127) };
                self.report_signal_death(pid, st, &crate::print::simple_text(s));
                Ok(st.shell_status())
            }
            Err(e) => self.exec_failed(path, argv, &env, e),
        }
    }

    /// Erros do exec como o bash: 127 pra não achado, 126 pro resto; ENOEXEC vira script.
    fn exec_failed(&mut self, path: &[u8], argv: &[Vec<u8>], env: &[Vec<u8>], e: Errno) -> Exec {
        let shown = String::from_utf8_lossy(path).into_owned();
        if e == Errno::ENOEXEC {
            return self.run_as_script(path, argv, env);
        }
        if e == Errno::ENOENT {
            if self.dash_style() {
                // O dash não distingue: caminho com barra que não existe também é "not found".
                self.error(format!("{shown}: not found"));
            } else if path.contains(&b'/') && argv.first().is_some_and(|a| a.contains(&b'/')) {
                self.error(format!("{shown}: No such file or directory"));
            } else {
                // Achado no hash mas sumiu, ou caminho relativo: o bash diz "No such file".
                self.error(format!("{shown}: No such file or directory"));
            }
            return Ok(127);
        }
        if let Ok(st) = sys().fstatat(Fd::CWD, path, AtFlags::empty())
            && st.file_type() == FileType::Directory {
                self.error(format!("{shown}: Is a directory"));
                return Ok(126);
            }
        self.error(format!("{shown}: {}", e.message()));
        Ok(126)
    }

    /// Arquivo executável sem `#!` nem formato conhecido: o bash roda como script num subshell.
    fn run_as_script(&mut self, path: &[u8], argv: &[Vec<u8>], env: &[Vec<u8>]) -> Exec {
        let s = sys();
        let data = match sysabi::sys::read_file(path) {
            Ok(d) => d,
            Err(e) => {
                self.error(format!("{}: {}", String::from_utf8_lossy(path), e.message()));
                return Ok(126);
            }
        };
        if data.iter().take(80).any(|c| *c == 0) {
            self.error(format!("{}: cannot execute binary file", String::from_utf8_lossy(path)));
            return Ok(126);
        }
        let mut child = Shell::new();
        let script = String::from_utf8_lossy(&data).into_owned();
        let argv = argv.to_vec();
        let env = env.to_vec();
        let attrs = ProcAttrs { env: Some(env), ..ProcAttrs::default() };
        let spawned = s.spawn_fn(
            attrs,
            b"bash".to_vec(),
            Box::new(move || {
                child.init_from_process();
                child.arg0 = argv[0].clone();
                child.params = argv[1..].to_vec();
                let name: Arc<str> = Arc::from(String::from_utf8_lossy(&argv[0]).as_ref());
                child.source_stack.push(name.clone());
                child.script_file = true;
                let r = child.run_text(&script, TextKind::Main, name, 1);
                let st = match r {
                    Ok(st) => st,
                    Err(Flow::Exit(n)) => n,
                    Err(_) => child.status,
                };
                child.exit_shell(st)
            }),
        );
        match spawned {
            Ok(pid) => Ok(self.wait_pid(pid)),
            Err(e) => {
                self.error(format!("fork: {}", e.message()));
                Ok(126)
            }
        }
    }

    // ---- funções ----

    pub fn call_function(&mut self, f: Arc<FunctionDef>, argv: Vec<Vec<u8>>) -> Exec {
        let funcnest = self.var_bytes("FUNCNEST").and_then(|v| std::str::from_utf8(v).ok()).and_then(|v| v.parse::<u32>().ok()).unwrap_or(0);
        if (funcnest > 0 && self.func_depth >= funcnest) || self.func_depth >= MAX_FUNC_DEPTH {
            let limit = if funcnest > 0 { funcnest } else { MAX_FUNC_DEPTH };
            self.error(format!("{}: maximum function nesting level exceeded ({limit})", f.name));
            return Err(Flow::Discard);
        }
        self.frames.push(Frame { name: f.name.clone(), source: f.source.clone(), call_line: self.lineno });
        self.vars.push(ScopeKind::Function);
        let saved_params = std::mem::replace(&mut self.params, argv.into_iter().skip(1).collect());
        let saved_loop = std::mem::replace(&mut self.loop_depth, 0);
        self.func_depth += 1;
        let r = stacker::maybe_grow(64 * 1024, 2 * 1024 * 1024, || {
            let mut undo = Undo::default();
            if !f.redirects.is_empty() {
                match self.apply_redirects(&f.redirects, Some(&mut undo))? {
                    Ok(()) => {}
                    Err(_) => return Ok(1),
                }
            }
            let r = self.exec_compound(&f.body);
            self.undo_redirects(undo);
            r
        });
        let r = match r {
            Err(Flow::Return(n)) => Ok(n),
            other => other,
        };
        if let Some(ret) = self.traps.ret.clone()
            && self.in_trap == 0 && (self.opts.get("functrace") || true) {
                let st = *r.as_ref().unwrap_or(&self.status);
                let _ = self.run_trap_command(&ret, st);
            }
        self.func_depth -= 1;
        self.loop_depth = saved_loop;
        self.params = saved_params;
        self.vars.pop();
        // De volta ao chamador, o `LINENO` é o da linha da chamada (o trap ERR do comando que
        // chamou a função vê essa linha).
        if let Some(frame) = self.frames.pop() {
            self.lineno = frame.call_line;
        }
        r
    }

    // ---- compostos ----

    pub fn exec_compound(&mut self, c: &Compound) -> Exec {
        self.lineno = c.line;
        match &c.kind {
            CompoundKind::Brace(list) => self.exec_list(list),
            CompoundKind::Subshell(list) => {
                let s = sys();
                let mut child = self.subshell_clone();
                let list = list.clone();
                let attrs = ProcAttrs { reset_signals: self.trapped_signals(), ..ProcAttrs::default() };
                match s.spawn_fn(attrs, b"bash".to_vec(), Box::new(move || child.run_subshell_list(&list))) {
                    Ok(pid) => {
                        let st = self.wait_raw(pid);
                        let Some(st) = st else { return Ok(127) };
                        Ok(st.shell_status())
                    }
                    Err(e) => {
                        self.error(format!("fork: {}", e.message()));
                        Ok(1)
                    }
                }
            }
            CompoundKind::For { var, words, body } => {
                let items = match words {
                    Some(ws) => {
                        if self.opts.get("xtrace") {
                            let ex = self.expand_words(ws)?;
                            let mut t = vec![b"for".to_vec(), var.as_bytes().to_vec(), b"in".to_vec()];
                            t.extend(ex.iter().map(|w| crate::quote::xtrace_word(w)));
                            self.xtrace_line(&t);
                            ex
                        } else {
                            self.expand_words(ws)?
                        }
                    }
                    None => self.params.clone(),
                };
                self.run_for(var, items, body)
            }
            CompoundKind::Select { var, words, body } => {
                let items = match words {
                    Some(ws) => self.expand_words(ws)?,
                    None => self.params.clone(),
                };
                builtins::select_loop(self, var, items, body)
            }
            CompoundKind::ArithFor { init, cond, step, body } => {
                if let Some(i) = init
                    && let Err(f) = self.arith_command_eval(i, "((") {
                        return f;
                    }
                let mut status = 0;
                self.loop_depth += 1;
                let r = loop {
                    sys().checkpoint();
                    if let Some(c) = cond {
                        match self.arith_command_eval(c, "((") {
                            Ok(0) => break Ok(status),
                            Ok(_) => {}
                            Err(f) => break f,
                        }
                    }
                    match self.exec_list(body) {
                        Ok(st) => status = st,
                        Err(Flow::Break(n)) => {
                            if n > 1 {
                                break Err(Flow::Break(n - 1));
                            }
                            break Ok(0);
                        }
                        Err(Flow::Continue(n)) => {
                            if n > 1 {
                                break Err(Flow::Continue(n - 1));
                            }
                        }
                        Err(f) => break Err(f),
                    }
                    if let Some(s) = step
                        && let Err(f) = self.arith_command_eval(s, "((") {
                            break f;
                        }
                };
                self.loop_depth -= 1;
                r
            }
            CompoundKind::Case { word, items } => {
                let value = self.expand_word_string(word)?;
                if self.opts.get("xtrace") {
                    self.xtrace_line(&[b"case".to_vec(), crate::quote::xtrace_word(&value), b"in".to_vec()]);
                }
                let mut status = 0;
                let mut i = 0;
                let mut fall = false;
                while i < items.len() {
                    let item = &items[i];
                    let mut matched = fall;
                    if !matched {
                        for p in &item.patterns {
                            let pat = self.expand_word_pattern(p)?;
                            if crate::pattern::Pattern::new(&pat, self.match_opts(false)).matches(&value) {
                                matched = true;
                                break;
                            }
                        }
                    }
                    if matched {
                        status = self.exec_list(&item.body)?;
                        match item.term {
                            CaseTerm::Break => return Ok(status),
                            CaseTerm::FallThrough => {
                                fall = true;
                            }
                            CaseTerm::Continue => {
                                fall = false;
                            }
                        }
                    }
                    i += 1;
                }
                Ok(status)
            }
            CompoundKind::If { branches, else_body } => {
                for (cond, body) in branches {
                    self.errexit_off += 1;
                    let r = self.exec_list(cond);
                    self.errexit_off -= 1;
                    if r? == 0 {
                        return self.exec_list(body);
                    }
                }
                match else_body {
                    Some(b) => self.exec_list(b),
                    None => Ok(0),
                }
            }
            CompoundKind::While { cond, body } => self.run_while(cond, body, false),
            CompoundKind::Until { cond, body } => self.run_while(cond, body, true),
            CompoundKind::Arith(a) => {
                if self.opts.get("xtrace") {
                    let mut t = b"((".to_vec();
                    t.extend_from_slice(a.raw.as_bytes());
                    t.extend_from_slice(b"))");
                    self.xtrace_line(&[t]);
                }
                match self.arith_command_eval(a, "((") {
                    Ok(v) => Ok(if v != 0 { 0 } else { 1 }),
                    Err(f) => f,
                }
            }
            CompoundKind::Cond(e) => crate::cond::eval_cond_command(self, e),
            CompoundKind::Coproc { name, body } => self.exec_coproc(name, body),
        }
    }

    /// Avalia o texto aritmético de um comando (`((`, `let`, `for ((`): erro imprime com o prefixo
    /// do comando e dá status 1 (não descarta o comando de topo).
    pub fn arith_command_eval(&mut self, a: &ArithExp, prefix: &str) -> Result<i64, Exec> {
        let text = match self.expand_parts_string(&a.parts) {
            Ok(t) => t,
            Err(f) => return Err(Err(f)),
        };
        self.arith_eval_prefixed(&text, prefix)
    }

    /// Avalia com o prefixo de builtin nas mensagens (`((: `, `let: `).
    pub fn arith_eval_prefixed(&mut self, text: &[u8], prefix: &str) -> Result<i64, Exec> {
        match crate::shell::arith_eval_raw(self, text) {
            Ok(v) => Ok(v),
            Err((_, Some(f))) => Err(Err(f)),
            Err((e, None)) => {
                if !e.from_env {
                    self.error(format!("{prefix}: {}", e.message));
                }
                Err(Ok(1))
            }
        }
    }

    fn run_for(&mut self, var: &str, items: Vec<Vec<u8>>, body: &List) -> Exec {
        let mut status = 0;
        self.loop_depth += 1;
        let r = (|| {
            for it in items {
                sys().checkpoint();
                if !self.assign_scalar(var, it, false)? {
                    return Ok(1);
                }
                match self.exec_list(body) {
                    Ok(st) => status = st,
                    Err(Flow::Break(n)) => {
                        if n > 1 {
                            return Err(Flow::Break(n - 1));
                        }
                        return Ok(status);
                    }
                    Err(Flow::Continue(n)) => {
                        if n > 1 {
                            return Err(Flow::Continue(n - 1));
                        }
                    }
                    Err(f) => return Err(f),
                }
            }
            Ok(status)
        })();
        self.loop_depth -= 1;
        r
    }

    fn run_while(&mut self, cond: &List, body: &List, until: bool) -> Exec {
        let mut status = 0;
        self.loop_depth += 1;
        let r = loop {
            sys().checkpoint();
            self.errexit_off += 1;
            let c = self.exec_list(cond);
            self.errexit_off -= 1;
            let c = match c {
                Ok(c) => c,
                Err(Flow::Break(n)) => {
                    if n > 1 {
                        break Err(Flow::Break(n - 1));
                    }
                    break Ok(status);
                }
                Err(Flow::Continue(n)) => {
                    if n > 1 {
                        break Err(Flow::Continue(n - 1));
                    }
                    continue;
                }
                Err(f) => break Err(f),
            };
            if (c == 0) == until {
                break Ok(status);
            }
            match self.exec_list(body) {
                Ok(st) => status = st,
                Err(Flow::Break(n)) => {
                    if n > 1 {
                        break Err(Flow::Break(n - 1));
                    }
                    break Ok(status);
                }
                Err(Flow::Continue(n)) => {
                    if n > 1 {
                        break Err(Flow::Continue(n - 1));
                    }
                }
                Err(f) => break Err(f),
            }
        };
        self.loop_depth -= 1;
        r
    }

    fn exec_coproc(&mut self, name: &str, body: &Command) -> Exec {
        let s = sys();
        let (in_r, in_w) = match s.pipe2(OFlags::CLOEXEC) {
            Ok(p) => p,
            Err(e) => {
                self.error(format!("pipe error: {}", e.message()));
                return Ok(1);
            }
        };
        let (out_r, out_w) = match s.pipe2(OFlags::CLOEXEC) {
            Ok(p) => p,
            Err(e) => {
                let _ = s.close(in_r);
                let _ = s.close(in_w);
                self.error(format!("pipe error: {}", e.message()));
                return Ok(1);
            }
        };
        let mut child = self.subshell_clone();
        let cmd = body.clone();
        let attrs = ProcAttrs {
            fd_actions: vec![
                FdAction::Dup2 { from: in_r, to: Fd::STDIN },
                FdAction::Dup2 { from: out_w, to: Fd::STDOUT },
                FdAction::Close(in_r),
                FdAction::Close(in_w),
                FdAction::Close(out_r),
                FdAction::Close(out_w),
            ],
            ..ProcAttrs::default()
        };
        let spawned = s.spawn_fn(attrs, b"bash".to_vec(), Box::new(move || child.run_subshell_command(&cmd)));
        let _ = s.close(in_r);
        let _ = s.close(out_w);
        let pid = match spawned {
            Ok(p) => p,
            Err(e) => {
                let _ = s.close(in_w);
                let _ = s.close(out_r);
                self.error(format!("fork: {}", e.message()));
                return Ok(1);
            }
        };
        // Os fds do coproc ficam acima de 60, sem CLOEXEC pros comandos do shell usarem.
        let rfd = s.dup_min(out_r, Fd(60), false).unwrap_or(out_r);
        let wfd = s.dup_min(in_w, Fd(60), false).unwrap_or(in_w);
        if rfd != out_r {
            let _ = s.close(out_r);
        }
        if wfd != in_w {
            let _ = s.close(in_w);
        }
        let items = vec![(Some(b"0".to_vec()), false, rfd.0.to_string().into_bytes()), (Some(b"1".to_vec()), false, wfd.0.to_string().into_bytes())];
        self.assign_array(name, &items, false)?;
        self.assign_scalar(&format!("{name}_PID"), pid.to_string().into_bytes(), false)?;
        self.last_bg = Some(pid);
        let id = self.next_job;
        self.next_job += 1;
        self.jobs.push(Job { id, pids: vec![pid], text: format!("coproc {name} {}", crate::print::command_text(body)), status: vec![None], reported: false });
        self.arm_sigchld();
        Ok(0)
    }

    /// `cmd &`.
    fn exec_async(&mut self, ao: &AndOr) -> Exec {
        let s = sys();
        let mut child = self.subshell_clone();
        let ao2 = ao.clone();
        // Um comando simples sozinho: o bash faz exec dele no próprio subshell.
        child.exec_last = ao.rest.is_empty() && !ao.first.negated && ao.first.time.is_none() && matches!(ao.first.commands.as_slice(), [Command::Simple(_)]);
        let mut actions = Vec::new();
        if !self.interactive && !self.opts.get("monitor") {
            actions.push(FdAction::Open { fd: Fd::STDIN, path: b"/dev/null".to_vec(), flags: OFlags::RDONLY, mode: 0 });
        }
        let attrs = ProcAttrs {
            fd_actions: actions,
            reset_signals: self.trapped_signals(),
            ignore_signals: if self.opts.get("monitor") { Vec::new() } else { vec![Signal::SIGINT, Signal::SIGQUIT] },
            ..ProcAttrs::default()
        };
        let spawned = s.spawn_fn(
            attrs,
            b"bash".to_vec(),
            Box::new(move || {
                child.enter_child();
                let r = child.exec_and_or(&ao2);
                child.finish_subshell(r)
            }),
        );
        match spawned {
            Ok(pid) => {
                self.last_bg = Some(pid);
                let id = self.next_job;
                self.next_job += 1;
                self.jobs.push(Job { id, pids: vec![pid], text: crate::print::and_or_text(ao), status: vec![None], reported: false });
                self.arm_sigchld();
                Ok(0)
            }
            Err(e) => {
                self.error(format!("fork: {}", e.message()));
                Ok(1)
            }
        }
    }

    // ---- traps ----

    /// Roda os traps de sinais que chegaram.
    /// Passa a capturar SIGCHLD (uma vez), para que filhos em segundo plano sejam recolhidos assim
    /// que morrem e não fiquem zumbis visíveis no `ps` enquanto o shell espera noutra coisa.
    pub fn arm_sigchld(&mut self) {
        if !self.sigchld_armed {
            self.sigchld_armed = true;
            let _ = sys().sigaction(Signal::SIGCHLD, SigDisposition::Catch);
            // Um filho que morreu antes da captura não gerou sinal capturado: recolhe agora.
            self.reap_background();
        }
    }

    /// Recolhe sem bloquear os jobs que já terminaram, guardando o status para `wait` e `jobs`.
    pub fn reap_background(&mut self) {
        let s = sys();
        for j in &mut self.jobs {
            for (k, pid) in j.pids.iter().enumerate() {
                if j.status[k].is_none()
                    && let Ok(Some((_, st))) = s.wait4(WaitTarget::Pid(*pid), WaitOptions::NOHANG) {
                        j.status[k] = Some(st.shell_status());
                    }
            }
        }
    }

    pub fn run_pending_traps(&mut self) -> Result<(), Flow> {
        if self.in_trap > 0 {
            return Ok(());
        }
        let sigs = sys().take_caught_signals();
        if sigs.contains(&Signal::SIGCHLD) {
            self.reap_background();
        }
        for sig in sigs {
            if let Some(cmd) = self.traps.signals.get(&sig.0).cloned()
                && !cmd.is_empty() {
                    let st = self.status;
                    self.run_trap_command(&cmd, st)?;
                    self.status = st;
                }
        }
        Ok(())
    }

    /// Executa o texto de um trap preservando `$?` (o trap vê o status de quem disparou).
    pub fn run_trap_command(&mut self, cmd: &str, status: i32) -> Result<(), Flow> {
        self.in_trap += 1;
        let saved_status = self.status;
        let saved_line = self.lineno;
        let saved_cmd = self.current_command.clone();
        self.status = status;
        let r = self.run_text(cmd, TextKind::Trap, Arc::from("trap"), self.lineno);
        self.in_trap -= 1;
        self.lineno = saved_line;
        self.current_command = saved_cmd;
        match r {
            Ok(_) => {
                self.status = saved_status;
                Ok(())
            }
            Err(Flow::Exit(n)) => Err(Flow::Exit(n)),
            Err(Flow::Return(n)) => Err(Flow::Return(n)),
            Err(_) => {
                self.status = saved_status;
                Ok(())
            }
        }
    }

    // ---- time ----

    fn times_now(&self) -> (i64, u32, std::time::Duration, std::time::Duration) {
        let s = sys();
        let t = s.clock_gettime(sysabi::Clock::Monotonic).unwrap_or_default();
        let me = s.getrusage(sysabi::RusageWho::SelfProcess).unwrap_or_default();
        let ch = s.getrusage(sysabi::RusageWho::Children).unwrap_or_default();
        (t.sec, t.nsec, me.utime + ch.utime, me.stime + ch.stime)
    }

    fn report_time(&mut self, t: TimeSpec, start: (i64, u32, std::time::Duration, std::time::Duration)) {
        let end = self.times_now();
        let real_ns = (end.0 - start.0) as i128 * 1_000_000_000 + end.1 as i128 - start.1 as i128;
        let real = std::time::Duration::from_nanos(real_ns.max(0) as u64);
        let user = end.2.saturating_sub(start.2);
        let sysd = end.3.saturating_sub(start.3);
        let fmt: Vec<u8> = if t.posix {
            b"real %2R\nuser %2U\nsys %2S".to_vec()
        } else {
            match self.vars.get("TIMEFORMAT") {
                Some(v) => v.scalar_value().unwrap_or_default().to_vec(),
                None => b"\nreal\t%3lR\nuser\t%3lU\nsys\t%3lS".to_vec(),
            }
        };
        let mut out = crate::builtins::format_timeformat(&fmt, real, user, sysd);
        out.push(b'\n');
        let _ = write_fd(Fd::STDERR, &out);
    }
}

/// Classe do comando a executar.
enum CmdKind {
    Function(Arc<FunctionDef>),
    Builtin,
    External(Vec<u8>),
    NotFound,
}

fn is_executable_file(p: &[u8]) -> bool {
    match sys().fstatat(Fd::CWD, p, AtFlags::empty()) {
        Ok(st) => st.file_type() != FileType::Directory && st.mode & 0o111 != 0,
        Err(_) => false,
    }
}

/// Lista que é um único comando simples em primeiro plano, sem `!`, `time`, `&&` ou `||`.
fn single_simple(list: &List) -> bool {
    match list.items.as_slice() {
        [item] => {
            let ao = &item.and_or;
            !item.background
                && ao.rest.is_empty()
                && !ao.first.negated
                && ao.first.time.is_none()
                && matches!(ao.first.commands.as_slice(), [Command::Simple(_)])
        }
        _ => false,
    }
}
