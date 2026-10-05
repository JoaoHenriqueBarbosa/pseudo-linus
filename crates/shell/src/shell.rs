//! Estado do interpretador e a semântica das variáveis (especiais dinâmicas, atributos, nameref).

use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::Arc;

use sysabi::{Clock, Errno, Fd, Pid, Syscalls};

use crate::arith::{ArithEnv, ArithError};
use crate::ast::{FunctionDef, Line};
use crate::options::Options;
use crate::vars::{Assoc, Attrs, ScopeKind, Value, Var, Vars};

/// Controle de fluxo que atravessa a execução.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Flow {
    /// `exit`, `set -e`, erro fatal: termina o shell (ou o subshell) com o status.
    Exit(i32),
    /// `return` de função ou de `source`.
    Return(i32),
    Break(u32),
    Continue(u32),
    /// Erro de expansão (aritmética, "bad substitution"): o bash descarta o comando de topo inteiro,
    /// `$?` vira 1 e a leitura continua no próximo comando.
    Discard,
}

pub type Exec = Result<i32, Flow>;

/// Moldura de chamada (pra `FUNCNAME`, `BASH_SOURCE`, `BASH_LINENO`, `caller`).
#[derive(Clone, Debug)]
pub struct Frame {
    pub name: String,
    pub source: Arc<str>,
    /// Linha, no chamador, de onde a função foi chamada.
    pub call_line: Line,
}

/// Job em segundo plano.
#[derive(Clone, Debug)]
pub struct Job {
    pub id: usize,
    pub pids: Vec<Pid>,
    pub text: String,
    /// Status de cada processo já colhido.
    pub status: Vec<Option<i32>>,
    pub reported: bool,
}

/// Ações de trap. Índice 0 é EXIT; 1..=64 são sinais.
#[derive(Clone, Debug, Default)]
pub struct Traps {
    pub signals: BTreeMap<i32, String>,
    pub err: Option<String>,
    pub debug: Option<String>,
    pub ret: Option<String>,
    /// Traps herdadas do pai num subshell: o `trap -p` ainda mostra até o subshell mudar alguma.
    pub inherited_display: Option<Box<Traps>>,
}

/// Sinal especial do `trap`.
pub const TRAP_EXIT: i32 = 0;

#[derive(Clone)]
pub struct Shell {
    pub vars: Vars,
    pub funcs: HashMap<String, Arc<FunctionDef>>,
    pub aliases: Arc<HashMap<String, String>>,
    pub opts: Options,
    /// `$1`, `$2`...
    pub params: Vec<Vec<u8>>,
    /// `$0`.
    pub arg0: Vec<u8>,
    /// `$?`.
    pub status: i32,
    pub pipestatus: Vec<i32>,
    /// `$!`.
    pub last_bg: Option<Pid>,
    pub traps: Traps,
    pub jobs: Vec<Job>,
    pub next_job: usize,
    /// Pilha do `pushd`/`popd` (sem o diretório corrente, que é o topo implícito).
    pub dirstack: Vec<Vec<u8>>,
    /// Tabela do `hash`: nome -> (caminho, acertos).
    pub hash: BTreeMap<String, (Vec<u8>, u32)>,
    pub frames: Vec<Frame>,
    /// Laços abertos (pro `break`/`continue`).
    pub loop_depth: u32,
    /// Contextos em que o `set -e` não vale (condição de if/while, lado esquerdo de && e ||, `!`).
    pub errexit_off: u32,
    /// `BASH_SUBSHELL`.
    pub subshell: u32,
    /// Nível de substituição de comando (repete o primeiro caractere do PS4 no xtrace).
    pub xtrace_level: u32,
    pub lineno: Line,
    /// `$$` (o pid do shell principal, igual em subshells).
    pub pid: Pid,
    /// Segundos (época) em que o shell começou, pro `SECONDS`.
    pub start_secs: i64,
    pub seconds_offset: i64,
    pub rand_seed: u32,
    pub last_random: u32,
    /// Fds que o próprio shell abriu pra guardar cópias durante redireções; um subshell fecha.
    pub saved_fds: Vec<Fd>,
    /// Modo `sh` (POSIX).
    pub posix: bool,
    /// Nome do texto em execução pras mensagens de erro de sintaxe (`-c`, `eval`, caminho).
    pub input_name: Arc<str>,
    /// Pilha de arquivos em `source` (o topo é o `BASH_SOURCE[0]` fora de função).
    pub source_stack: Vec<Arc<str>>,
    /// Profundidade de `source` (pro `return`).
    pub source_depth: u32,
    /// `BASH_COMMAND`.
    pub current_command: String,
    /// Variáveis especiais que o usuário desfez (perdem o comportamento dinâmico).
    pub specials_off: BTreeSet<String>,
    /// Incrementado quando muda algo que afeta o parse (aliases): o leitor reparseia.
    pub parse_generation: u64,
    /// Fds de substituição de processo abertos pelo comando corrente.
    pub procsub_fds: Vec<Fd>,
    /// Pids de substituições de processo pendentes.
    pub procsub_pids: Vec<Pid>,
    /// `-c` (pra `$-`).
    pub dash_c: bool,
    pub interactive: bool,
    /// Executando um trap (pra não reentrar).
    pub in_trap: u32,
    /// Shell rodando um script lido de arquivo (o `FUNCNAME` ganha "main").
    pub script_file: bool,
    /// Estamos num subshell (fork do shell principal).
    pub is_subshell: bool,
    /// Nível de funções aninhadas (pro FUNCNEST).
    pub func_depth: u32,
    /// O EXIT trap já rodou neste processo.
    pub exit_trap_done: bool,
    /// Status da última substituição de comando do comando simples corrente (vira o `$?` de um
    /// comando só de atribuições).
    pub last_cmdsub_status: Option<i32>,
    /// Builtins desligados com `enable -n`.
    pub disabled_builtins: BTreeSet<String>,
    /// Estado interno do `getopts`: (OPTIND em que a posição vale, posição dentro do argumento).
    pub getopts_state: (usize, usize),
}

pub fn sys() -> Arc<dyn Syscalls> {
    sysabi::sys::current()
}

/// Escreve tudo num fd, repetindo em escrita parcial e EINTR.
pub fn write_fd(fd: Fd, mut buf: &[u8]) -> Result<(), Errno> {
    let s = sys();
    while !buf.is_empty() {
        match s.write(fd, buf) {
            Ok(0) => return Err(Errno::EIO),
            Ok(n) => buf = &buf[n..],
            Err(Errno::EINTR) => {}
            Err(e) => return Err(e),
        }
    }
    Ok(())
}

/// Nomes de variáveis com comportamento dinâmico.
const DYNAMIC: &[&str] = &[
    "RANDOM", "SRANDOM", "SECONDS", "LINENO", "BASHPID", "EPOCHSECONDS", "EPOCHREALTIME", "BASH_SUBSHELL", "FUNCNAME",
    "BASH_SOURCE", "BASH_LINENO", "BASH_COMMAND", "GROUPS", "DIRSTACK", "SHELLOPTS", "BASHOPTS", "BASH_ARGV0",
];

impl Shell {
    pub fn new() -> Shell {
        Shell {
            vars: Vars::new(),
            funcs: HashMap::new(),
            aliases: Arc::new(HashMap::new()),
            opts: Options::default(),
            params: Vec::new(),
            arg0: b"bash".to_vec(),
            status: 0,
            pipestatus: vec![0],
            last_bg: None,
            traps: Traps::default(),
            jobs: Vec::new(),
            next_job: 1,
            dirstack: Vec::new(),
            hash: BTreeMap::new(),
            frames: Vec::new(),
            loop_depth: 0,
            errexit_off: 0,
            subshell: 0,
            xtrace_level: 0,
            lineno: 0,
            pid: 0,
            start_secs: 0,
            seconds_offset: 0,
            rand_seed: 0,
            last_random: 0,
            saved_fds: Vec::new(),
            posix: false,
            input_name: Arc::from("-c"),
            source_stack: Vec::new(),
            source_depth: 0,
            current_command: String::new(),
            specials_off: BTreeSet::new(),
            parse_generation: 0,
            procsub_fds: Vec::new(),
            procsub_pids: Vec::new(),
            dash_c: false,
            interactive: false,
            in_trap: 0,
            script_file: false,
            is_subshell: false,
            func_depth: 0,
            exit_trap_done: false,
            last_cmdsub_status: None,
            disabled_builtins: BTreeSet::new(),
            getopts_state: (1, 1),
        }
    }

    /// Inicializa a partir do processo corrente: ambiente, pid, relógio, variáveis padrão.
    pub fn init_from_process(&mut self) {
        let s = sys();
        self.pid = s.getpid();
        self.start_secs = s.clock_gettime(Clock::Realtime).map(|t| t.sec).unwrap_or(0);
        let mut seed = [0u8; 4];
        let _ = s.getrandom(&mut seed);
        self.rand_seed = u32::from_le_bytes(seed);
        for kv in s.environ() {
            let Some(eq) = kv.iter().position(|b| *b == b'=') else { continue };
            let name = &kv[..eq];
            if !crate::word::is_name(name) {
                continue;
            }
            let name = String::from_utf8_lossy(name).into_owned();
            let v = self.vars.global_entry(&name);
            v.value = Value::Scalar(kv[eq + 1..].to_vec());
            v.attrs.set(Attrs::EXPORT);
        }
        let shlvl = self
            .vars
            .get("SHLVL")
            .and_then(|v| v.scalar_value())
            .and_then(|v| std::str::from_utf8(v).ok())
            .and_then(|v| v.trim().parse::<i64>().ok())
            .unwrap_or(0);
        self.set_exported("SHLVL", (shlvl + 1).to_string().into_bytes());
        let cwd = s.getcwd().unwrap_or_else(|_| b"/".to_vec());
        // PWD herdado vale se apontar pro mesmo diretório (como o bash); senão o real.
        let pwd_ok = self
            .vars
            .get("PWD")
            .and_then(|v| v.scalar_value())
            .is_some_and(|p| p.starts_with(b"/") && same_dir(p, &cwd));
        if !pwd_ok {
            self.set_exported("PWD", cwd);
        }
        if self.vars.get("OLDPWD").is_none() {
            let v = self.vars.global_entry("OLDPWD");
            v.attrs.set(Attrs::EXPORT);
        }
        let ppid = s.getppid();
        let uid = s.getuid();
        let euid = s.geteuid();
        let uts = s.uname();
        let defaults: Vec<(&str, Vec<u8>, Attrs)> = vec![
            ("IFS", b" \t\n".to_vec(), Attrs::default()),
            ("PS4", b"+ ".to_vec(), Attrs::default()),
            ("OPTIND", b"1".to_vec(), Attrs::INTEGER),
            ("OPTERR", b"1".to_vec(), Attrs::default()),
            ("BASH", b"/usr/bin/bash".to_vec(), Attrs::default()),
            ("BASH_VERSION", b"5.2.37(1)-release".to_vec(), Attrs::default()),
            ("HOSTTYPE", b"x86_64".to_vec(), Attrs::default()),
            ("MACHTYPE", b"x86_64-pc-linux-gnu".to_vec(), Attrs::default()),
            ("OSTYPE", b"linux-gnu".to_vec(), Attrs::default()),
            ("HOSTNAME", uts.nodename.clone(), Attrs::default()),
            ("PPID", ppid.to_string().into_bytes(), Attrs(Attrs::INTEGER.0 | Attrs::READONLY.0)),
            ("UID", uid.to_string().into_bytes(), Attrs(Attrs::INTEGER.0 | Attrs::READONLY.0)),
            ("EUID", euid.to_string().into_bytes(), Attrs(Attrs::INTEGER.0 | Attrs::READONLY.0)),
        ];
        for (name, value, attrs) in defaults {
            if self.vars.get(name).is_some() && !matches!(name, "PPID" | "UID" | "EUID" | "BASH" | "BASH_VERSION") {
                continue;
            }
            let v = self.vars.global_entry(name);
            v.value = Value::Scalar(value);
            v.attrs.set(attrs);
        }
        if self.vars.get("PATH").is_none() {
            let v = self.vars.global_entry("PATH");
            v.value = Value::Scalar(b"/usr/local/bin:/usr/local/sbin:/usr/bin:/usr/sbin:/bin:/sbin:.".to_vec());
        }
        let mut versinfo = BTreeMap::new();
        for (i, p) in ["5", "2", "37", "1", "release", "x86_64-pc-linux-gnu"].iter().enumerate() {
            versinfo.insert(i as i64, p.as_bytes().to_vec());
        }
        let v = self.vars.global_entry("BASH_VERSINFO");
        v.value = Value::Indexed(versinfo);
        v.attrs = Attrs(Attrs::INDEXED.0 | Attrs::READONLY.0);
        let v = self.vars.global_entry("BASH_ARGC");
        v.value = Value::Indexed(BTreeMap::new());
        v.attrs = Attrs::INDEXED;
        let v = self.vars.global_entry("BASH_ARGV");
        v.value = Value::Indexed(BTreeMap::new());
        v.attrs = Attrs::INDEXED;
        let v = self.vars.global_entry("PIPESTATUS");
        v.value = Value::Indexed(BTreeMap::from([(0, b"0".to_vec())]));
        v.attrs = Attrs::INDEXED;
    }

    fn set_exported(&mut self, name: &str, value: Vec<u8>) {
        let v = self.vars.global_entry(name);
        v.value = Value::Scalar(value);
        v.attrs.set(Attrs::EXPORT);
    }

    // ---- mensagens ----

    /// Nome usado nas mensagens de erro: `BASH_SOURCE[0]` se houver, senão `$0`.
    pub fn error_name(&self) -> String {
        if let Some(src) = self.current_source() {
            if !src.is_empty() {
                return src.to_string();
            }
        }
        String::from_utf8_lossy(&self.arg0).into_owned()
    }

    fn current_source(&self) -> Option<Arc<str>> {
        if let Some(f) = self.frames.last() {
            return Some(f.source.clone());
        }
        self.source_stack.last().cloned()
    }

    /// Estamos no `sh` do Debian (dash): erros de execução com o formato `sh: N: ` e o "not found"
    /// seco para comando que não existe.
    pub fn dash_style(&self) -> bool {
        self.posix && self.arg0 == b"sh"
    }

    /// Prefixo `bash: line N: ` dos erros de execução.
    pub fn error_prefix(&self) -> String {
        if self.interactive {
            return format!("{}: ", self.error_name());
        }
        if self.dash_style() {
            return format!("{}: {}: ", self.error_name(), self.lineno);
        }
        format!("{}: line {}: ", self.error_name(), self.lineno)
    }

    /// Erro no stderr com o prefixo do bash.
    pub fn error(&self, msg: impl AsRef<str>) {
        let line = format!("{}{}\n", self.error_prefix(), msg.as_ref());
        let _ = write_fd(Fd::STDERR, line.as_bytes());
    }

    /// Erro em bytes (nomes de arquivo arbitrários).
    pub fn error_bytes(&self, msg: &[u8]) {
        let mut line = self.error_prefix().into_bytes();
        line.extend_from_slice(msg);
        line.push(b'\n');
        let _ = write_fd(Fd::STDERR, &line);
    }

    /// Erro de builtin: `bash: line N: nome: msg`.
    pub fn builtin_error(&self, builtin: &str, msg: impl AsRef<str>) {
        self.error(format!("{builtin}: {}", msg.as_ref()));
    }

    // ---- variáveis: leitura ----

    /// Resolve nameref: devolve o nome final (até 8 níveis, como o bash avisa depois disso).
    pub fn resolve_nameref(&self, name: &str) -> String {
        let mut cur = name.to_string();
        for _ in 0..8 {
            match self.vars.get(&cur) {
                Some(v) if v.attrs.has(Attrs::NAMEREF) => match v.scalar_value() {
                    Some(target) if !target.is_empty() => cur = String::from_utf8_lossy(target).into_owned(),
                    _ => return cur,
                },
                _ => return cur,
            }
        }
        cur
    }

    /// Variável visível, com as especiais dinâmicas calculadas na hora e nameref resolvido.
    pub fn lookup(&mut self, name: &str) -> Option<Cow<'_, Var>> {
        let name = self.resolve_nameref(name);
        if DYNAMIC.contains(&name.as_str()) && !self.specials_off.contains(&name) {
            if let Some(v) = self.dynamic(&name) {
                return Some(Cow::Owned(v));
            }
        }
        self.vars.get(&name).map(Cow::Borrowed)
    }

    /// Valor escalar (`$nome`), `None` se não definida.
    pub fn get_scalar(&mut self, name: &str) -> Option<Vec<u8>> {
        self.lookup(name).and_then(|v| v.scalar_value().map(|x| x.to_vec()))
    }

    /// Valor de uma variável comum (sem especiais dinâmicas): pra IFS, PS4, HOME...
    pub fn var_bytes(&self, name: &str) -> Option<&[u8]> {
        self.vars.get(name).and_then(|v| v.scalar_value())
    }

    fn dynamic(&mut self, name: &str) -> Option<Var> {
        let s = sys();
        let scalar = |v: Vec<u8>| Var::scalar(v);
        let indexed = |items: Vec<Vec<u8>>| Var {
            value: Value::Indexed(items.into_iter().enumerate().map(|(i, v)| (i as i64, v)).collect()),
            attrs: Attrs::INDEXED,
        };
        Some(match name {
            "RANDOM" => {
                let r = self.next_random();
                let mut v = scalar(r.to_string().into_bytes());
                v.attrs.set(Attrs::INTEGER);
                v
            }
            "SRANDOM" => {
                let mut b = [0u8; 4];
                let _ = s.getrandom(&mut b);
                scalar(u32::from_le_bytes(b).to_string().into_bytes())
            }
            "SECONDS" => {
                let now = s.clock_gettime(Clock::Realtime).map(|t| t.sec).unwrap_or(0);
                scalar((now - self.start_secs + self.seconds_offset).to_string().into_bytes())
            }
            "LINENO" => scalar(self.lineno.to_string().into_bytes()),
            "BASHPID" => scalar(s.getpid().to_string().into_bytes()),
            "EPOCHSECONDS" => scalar(s.clock_gettime(Clock::Realtime).map(|t| t.sec).unwrap_or(0).to_string().into_bytes()),
            "EPOCHREALTIME" => {
                let t = s.clock_gettime(Clock::Realtime).unwrap_or_default();
                scalar(format!("{}.{:06}", t.sec, t.nsec / 1000).into_bytes())
            }
            "BASH_SUBSHELL" => scalar(self.subshell.to_string().into_bytes()),
            "BASH_COMMAND" => scalar(self.current_command.clone().into_bytes()),
            "BASH_ARGV0" => scalar(self.arg0.clone()),
            "SHELLOPTS" => {
                let mut v = scalar(self.opts.shellopts());
                v.attrs.set(Attrs::READONLY);
                v
            }
            "BASHOPTS" => {
                let mut v = scalar(self.opts.bashopts());
                v.attrs.set(Attrs::READONLY);
                v
            }
            "FUNCNAME" => {
                if self.frames.is_empty() {
                    return None;
                }
                let mut items: Vec<Vec<u8>> = self.frames.iter().rev().map(|f| f.name.clone().into_bytes()).collect();
                if self.script_file {
                    items.push(b"main".to_vec());
                }
                indexed(items)
            }
            "BASH_SOURCE" => {
                let mut items: Vec<Vec<u8>> = self.frames.iter().rev().map(|f| f.source.as_bytes().to_vec()).collect();
                if self.script_file || !self.source_stack.is_empty() {
                    if let Some(s) = self.source_stack.last() {
                        items.push(s.as_bytes().to_vec());
                    }
                }
                if items.is_empty() {
                    return Some(indexed(Vec::new()));
                }
                indexed(items)
            }
            "BASH_LINENO" => {
                let mut items: Vec<Vec<u8>> = self.frames.iter().rev().map(|f| f.call_line.to_string().into_bytes()).collect();
                if self.script_file && !self.frames.is_empty() {
                    items.push(b"0".to_vec());
                }
                indexed(items)
            }
            "GROUPS" => indexed(s.getgroups().into_iter().map(|g| g.to_string().into_bytes()).collect()),
            "DIRSTACK" => {
                let mut items = vec![self.var_bytes("PWD").map(|p| p.to_vec()).unwrap_or_default()];
                items.extend(self.dirstack.iter().rev().cloned());
                indexed(items)
            }
            _ => return None,
        })
    }

    /// O `RANDOM` do bash 5.2 (Park-Miller com o filtro de compatibilidade > 50).
    pub fn next_random(&mut self) -> u32 {
        loop {
            let last = if self.rand_seed == 0 { 123_459_876 } else { self.rand_seed };
            let h = (last / 127_773) as i64;
            let l = last as i64 - 127_773 * h;
            let mut t = 16_807 * l - 2_836 * h;
            if t < 0 {
                t += 0x7fff_ffff;
            }
            self.rand_seed = t as u32;
            let r = ((self.rand_seed >> 16) ^ (self.rand_seed & 65_535)) & 32_767;
            if r != self.last_random {
                self.last_random = r;
                return r;
            }
        }
    }

    // ---- variáveis: escrita ----

    /// Valida que dá pra escrever na variável (readonly); imprime o erro do bash se não der.
    pub fn check_writable(&self, name: &str) -> bool {
        match self.vars.get(name) {
            Some(v) if v.attrs.has(Attrs::READONLY) => {
                self.error(format!("{name}: readonly variable"));
                false
            }
            _ => true,
        }
    }

    /// Converte um valor segundo os atributos (inteiro avalia, -l/-u/-c mudam a caixa).
    pub fn convert_value(&mut self, name: &str, attrs: Attrs, old: Option<&[u8]>, value: Vec<u8>, append: bool) -> Result<Vec<u8>, Flow> {
        if attrs.has(Attrs::INTEGER) {
            let mut n = self.arith_eval(&value)?;
            if append {
                let prev = match old {
                    Some(o) if !o.is_empty() => self.arith_eval(o)?,
                    _ => 0,
                };
                n = prev.wrapping_add(n);
            }
            let _ = name;
            return Ok(n.to_string().into_bytes());
        }
        let mut v = if append {
            let mut o = old.map(|o| o.to_vec()).unwrap_or_default();
            o.extend_from_slice(&value);
            o
        } else {
            value
        };
        if attrs.has(Attrs::LOWER) {
            v = self.map_case(&v, false);
        } else if attrs.has(Attrs::UPPER) {
            v = self.map_case(&v, true);
        } else if attrs.has(Attrs::CAPITALIZE) {
            let lower = self.map_case(&v, false);
            v = crate::expand::case_first(&lower, true, self.utf8());
        }
        Ok(v)
    }

    pub fn map_case(&self, v: &[u8], upper: bool) -> Vec<u8> {
        crate::expand::case_all(v, upper, self.utf8())
    }

    /// Atribuição escalar `nome=valor` (ou `nome+=valor`), com todas as regras do bash.
    pub fn assign_scalar(&mut self, name: &str, value: Vec<u8>, append: bool) -> Result<bool, Flow> {
        let name = self.resolve_nameref(name);
        if !self.check_writable(&name) {
            return Ok(false);
        }
        self.special_assign_hook(&name, &value);
        let (attrs, old, is_array) = match self.vars.get(&name) {
            Some(v) => (v.attrs, v.scalar_value().map(|x| x.to_vec()), matches!(v.value, Value::Indexed(_) | Value::Assoc(_))),
            None => (Attrs::default(), None, false),
        };
        let v = self.convert_value(&name, attrs, old.as_deref(), value, append)?;
        if is_array {
            let var = self.vars.entry(&name);
            match &mut var.value {
                Value::Indexed(m) => {
                    m.insert(0, v);
                }
                Value::Assoc(a) => a.insert(b"0".to_vec(), v),
                _ => {}
            }
        } else {
            let allexport = self.opts.get("allexport");
            let var = self.vars.entry(&name);
            var.value = Value::Scalar(v);
            if allexport {
                var.attrs.set(Attrs::EXPORT);
            }
        }
        Ok(true)
    }

    /// Efeitos colaterais de atribuir a algumas especiais.
    fn special_assign_hook(&mut self, name: &str, value: &[u8]) {
        match name {
            "RANDOM" if !self.specials_off.contains(name) => {
                let n = std::str::from_utf8(value).ok().and_then(|s| s.trim().parse::<i64>().ok()).unwrap_or(0);
                self.rand_seed = n as u32;
                self.last_random = 0;
            }
            "SECONDS" if !self.specials_off.contains(name) => {
                let n = std::str::from_utf8(value).ok().and_then(|s| s.trim().parse::<i64>().ok()).unwrap_or(0);
                let now = sys().clock_gettime(Clock::Realtime).map(|t| t.sec).unwrap_or(0);
                self.seconds_offset = n - (now - self.start_secs);
            }
            "LINENO" | "BASHPID" | "BASH_SUBSHELL" | "FUNCNAME" | "BASH_COMMAND" | "EPOCHSECONDS" | "EPOCHREALTIME" | "SRANDOM" => {
                // O bash aceita a atribuição mas o valor dinâmico continua valendo.
            }
            _ => {}
        }
    }

    /// Elemento de array: `nome[chave]=valor`.
    pub fn assign_element(&mut self, name: &str, key: &[u8], value: Vec<u8>, append: bool) -> Result<bool, Flow> {
        let name = self.resolve_nameref(name);
        if !self.check_writable(&name) {
            return Ok(false);
        }
        let attrs = self.vars.get(&name).map(|v| v.attrs).unwrap_or_default();
        let is_assoc = attrs.has(Attrs::ASSOC) || matches!(self.vars.get(&name).map(|v| &v.value), Some(Value::Assoc(_)));
        if is_assoc {
            let old = match self.vars.get(&name).map(|v| &v.value) {
                Some(Value::Assoc(a)) => a.get(key).cloned(),
                _ => None,
            };
            let v = self.convert_value(&name, attrs, old.as_deref(), value, append)?;
            let var = self.vars.entry(&name);
            if !matches!(var.value, Value::Assoc(_)) {
                var.value = Value::Assoc(Assoc::new());
            }
            if let Value::Assoc(a) = &mut var.value {
                a.insert(key.to_vec(), v);
            }
            var.attrs.set(Attrs::ASSOC);
            return Ok(true);
        }
        let idx = self.arith_eval(key)?;
        let idx = match self.resolve_index(&name, idx) {
            Some(i) => i,
            None => {
                self.error(format!("{name}[{}]: bad array subscript", String::from_utf8_lossy(key)));
                return Ok(false);
            }
        };
        let old = match self.vars.get(&name).map(|v| &v.value) {
            Some(Value::Indexed(m)) => m.get(&idx).cloned(),
            Some(Value::Scalar(s)) if idx == 0 => Some(s.clone()),
            _ => None,
        };
        let v = self.convert_value(&name, attrs, old.as_deref(), value, append)?;
        let var = self.vars.entry(&name);
        let map = match std::mem::replace(&mut var.value, Value::Unset) {
            Value::Indexed(m) => m,
            Value::Scalar(s) => BTreeMap::from([(0, s)]),
            _ => BTreeMap::new(),
        };
        let mut map = map;
        map.insert(idx, v);
        var.value = Value::Indexed(map);
        var.attrs.set(Attrs::INDEXED);
        Ok(true)
    }

    /// Índice negativo conta do fim (máximo + 1); `None` se ficar negativo.
    pub fn resolve_index(&self, name: &str, idx: i64) -> Option<i64> {
        if idx >= 0 {
            return Some(idx);
        }
        let max = match self.vars.get(name).map(|v| &v.value) {
            Some(Value::Indexed(m)) => m.keys().next_back().copied(),
            Some(Value::Scalar(_)) => Some(0),
            _ => None,
        };
        let max = max?;
        let r = max + 1 + idx;
        (r >= 0).then_some(r)
    }

    /// `unset nome`.
    pub fn unset_var(&mut self, name: &str) -> bool {
        let real = self.resolve_nameref(name);
        if let Some(v) = self.vars.get(&real) {
            if v.attrs.has(Attrs::READONLY) {
                self.error(format!("unset: {real}: cannot unset: readonly variable"));
                return false;
            }
        }
        if DYNAMIC.contains(&real.as_str()) {
            self.specials_off.insert(real.clone());
        }
        self.vars.unset(&real);
        true
    }

    pub fn utf8(&self) -> bool {
        let get = |n: &str| self.vars.get(n).and_then(|v| v.scalar_value()).filter(|v| !v.is_empty());
        let loc = get("LC_ALL").or_else(|| get("LC_CTYPE")).or_else(|| get("LANG"));
        match loc {
            Some(l) => {
                let l = String::from_utf8_lossy(l).to_ascii_lowercase();
                l.contains("utf-8") || l.contains("utf8")
            }
            None => false,
        }
    }

    pub fn ifs(&self) -> Vec<u8> {
        match self.vars.get("IFS") {
            Some(v) => v.scalar_value().map(|x| x.to_vec()).unwrap_or_else(|| b" \t\n".to_vec()),
            None => b" \t\n".to_vec(),
        }
    }

    /// Ambiente pros filhos: variáveis exportadas visíveis (a mais interna de cada nome).
    pub fn export_env(&self) -> Vec<Vec<u8>> {
        let mut seen: BTreeMap<String, Vec<u8>> = BTreeMap::new();
        let mut hidden: BTreeSet<String> = BTreeSet::new();
        for scope in self.vars.scopes().iter().rev() {
            for (k, v) in &scope.map {
                if seen.contains_key(k) || hidden.contains(k) {
                    continue;
                }
                if v.attrs.has(Attrs::EXPORT) {
                    if let Value::Scalar(s) = &v.value {
                        seen.insert(k.clone(), s.clone());
                        continue;
                    }
                }
                hidden.insert(k.clone());
            }
        }
        seen.into_iter()
            .map(|(k, v)| {
                let mut e = k.into_bytes();
                e.push(b'=');
                e.extend(v);
                e
            })
            .collect()
    }

    // ---- aritmética ----

    /// Avalia um texto aritmético já expandido. Erro: imprime a mensagem e devolve `Discard`
    /// (ou o fluxo fatal do `set -u`).
    pub fn arith_eval(&mut self, expr: &[u8]) -> Result<i64, Flow> {
        match arith_eval_raw(self, expr) {
            Ok(v) => Ok(v),
            Err((_, Some(f))) => Err(f),
            Err((e, None)) => {
                if !e.from_env {
                    self.error(&e.message);
                }
                Err(Flow::Discard)
            }
        }
    }

    pub fn function_scope_depth(&self) -> usize {
        self.vars.function_depth()
    }

    pub fn in_function(&self) -> bool {
        !self.frames.is_empty()
    }

    pub fn push_temp_scope(&mut self) {
        self.vars.push(ScopeKind::Temp);
    }
}

impl Default for Shell {
    fn default() -> Self {
        Shell::new()
    }
}

fn same_dir(a: &[u8], b: &[u8]) -> bool {
    let s = sys();
    match (s.fstatat(Fd::CWD, a, sysabi::AtFlags::empty()), s.fstatat(Fd::CWD, b, sysabi::AtFlags::empty())) {
        (Ok(x), Ok(y)) => x.dev == y.dev && x.ino == y.ino,
        _ => false,
    }
}

/// Avalia sem imprimir: o erro volta com o fluxo fatal (se o `set -u` disparou).
pub fn arith_eval_raw(sh: &mut Shell, expr: &[u8]) -> Result<i64, (ArithError, Option<Flow>)> {
    let mut env = ShellArith { sh, fatal: None };
    let r = crate::arith::eval(expr, &mut env);
    let fatal = env.fatal;
    r.map_err(|e| (e, fatal))
}

/// Ponte entre o avaliador aritmético e as variáveis do shell.
struct ShellArith<'a> {
    sh: &'a mut Shell,
    fatal: Option<Flow>,
}

impl ArithEnv for ShellArith<'_> {
    fn lookup(&mut self, name: &str, subscript: Option<&[u8]>) -> Result<Option<Vec<u8>>, ArithError> {
        let unbound = |sh: &Shell, shown: &str| {
            sh.error(format!("{shown}: unbound variable"));
        };
        match subscript {
            None => {
                let v = self.sh.get_scalar(name);
                if v.is_none() && self.sh.opts.get("nounset") {
                    unbound(self.sh, name);
                    self.fatal = Some(Flow::Exit(127));
                    return Err(ArithError { message: String::new(), from_env: true });
                }
                Ok(v)
            }
            Some(sub) => {
                let real = self.sh.resolve_nameref(name);
                let assoc = matches!(self.sh.vars.get(&real).map(|v| &v.value), Some(Value::Assoc(_)))
                    || self.sh.vars.get(&real).is_some_and(|v| v.attrs.has(Attrs::ASSOC));
                let r = if assoc {
                    match self.sh.vars.get(&real).map(|v| &v.value) {
                        Some(Value::Assoc(a)) => a.get(sub).cloned(),
                        _ => None,
                    }
                } else {
                    let idx = match crate::arith::eval(sub, self) {
                        Ok(i) => i,
                        Err(e) => return Err(e),
                    };
                    match self.sh.resolve_index(&real, idx) {
                        None => None,
                        Some(i) => match self.sh.lookup(&real).map(|v| v.into_owned()) {
                            Some(Var { value: Value::Indexed(m), .. }) => m.get(&i).cloned(),
                            Some(Var { value: Value::Scalar(s), .. }) if i == 0 => Some(s),
                            _ => None,
                        },
                    }
                };
                if r.is_none() && self.sh.opts.get("nounset") {
                    unbound(self.sh, &format!("{name}[{}]", String::from_utf8_lossy(sub)));
                    self.fatal = Some(Flow::Exit(127));
                    return Err(ArithError { message: String::new(), from_env: true });
                }
                Ok(r)
            }
        }
    }

    fn assign(&mut self, name: &str, subscript: Option<&[u8]>, value: i64) -> Result<(), ArithError> {
        let v = value.to_string().into_bytes();
        let r = match subscript {
            None => self.sh.assign_scalar(name, v, false),
            Some(sub) => {
                let real = self.sh.resolve_nameref(name);
                let assoc = matches!(self.sh.vars.get(&real).map(|v| &v.value), Some(Value::Assoc(_)));
                let key = if assoc {
                    sub.to_vec()
                } else {
                    match crate::arith::eval(sub, self) {
                        Ok(i) => i.to_string().into_bytes(),
                        Err(e) => return Err(e),
                    }
                };
                self.sh.assign_element(name, &key, v, false)
            }
        };
        match r {
            Ok(true) => Ok(()),
            Ok(false) => Err(ArithError { message: String::new(), from_env: true }),
            Err(f) => {
                self.fatal = Some(f);
                Err(ArithError { message: String::new(), from_env: true })
            }
        }
    }
}
