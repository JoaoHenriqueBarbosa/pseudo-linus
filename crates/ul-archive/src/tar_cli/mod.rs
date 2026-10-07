//! `tar` (GNU tar 1.35): criação, listagem, extração, acréscimo, atualização, comparação, concatenação
//! e remoção de membros, nos formatos gnu (padrão), oldgnu, ustar, pax e v7, com compressão em processo
//! (gzip, bzip2, xz, lzma, lzip, zstd) pelos codecs do crate.
//!
//! Escrito a partir do manual do GNU tar, do POSIX e do comportamento observado no oráculo; nada vem do
//! código do GNU tar. Todo I/O passa pelo `sysabi`.

pub mod args;
pub mod compare;
pub mod compress;
pub mod create;
pub mod date;
pub mod extract;
pub mod fnmatch;
pub mod header;
pub mod help;
pub mod list;
pub mod member;
pub mod modespec;
pub mod names;
pub mod owner;
pub use ul_common::quote;
pub mod reader;
pub mod transform;
pub mod writer;

use std::ffi::OsString;

use sysabi::{Ctx, Fd, OFlags};

use crate::sysutil::{self, Output};
use args::{ArgError, Mode, Opts, Parsed};
use member::Member;
use reader::{ReadError, Reader, Status};

/// Erro fatal: a mensagem já saiu, o programa termina com 2.
#[derive(Debug)]
pub struct Fatal;

pub type R<T> = Result<T, Fatal>;

/// Estado de uma execução do tar.
pub struct Tar {
    /// Nome nas mensagens (argv[0] como foi chamado).
    pub prog: String,
    pub o: Opts,
    pub out: Output,
    /// 0, 1 (diferenças no `-d`) ou 2 (erros).
    pub exit: i32,
    /// A listagem verbosa vai pro stderr (quando o arquivo sai no stdout).
    pub verbose_to_stderr: bool,
    pub owners: owner::Db,
    pub lister: list::Lister,
    /// Prefixos já avisados ("Removing leading ...").
    pub warned_prefixes: Vec<(Vec<u8>, bool)>,
    /// Transformações compiladas (`--transform`).
    pub transforms: Vec<transform::Expr>,
    /// O descompressor "filho" falhou (o fim prematuro dos dados não vira "does not look like").
    pub child_failed: bool,
    /// Exclusões (`--exclude`, `-X`, `--exclude-vcs`...).
    pub excluder: names::Excluder,
    /// Quantos checkpoints já dispararam.
    pub ckpt_count: u64,
    /// `--index-file`: a listagem verbosa vai pra esse fd.
    pub index_fd: Option<Fd>,
    /// Início da execução (relógio monotônico), pra taxa do `--totals`.
    pub start_time: sysabi::TimeSpec,
}

impl Tar {
    /// Dispara os checkpoints devidos depois de `records` registros lidos ou escritos.
    pub fn checkpoint(&mut self, records: u64, write: bool) {
        let Some(every) = self.o.checkpoint else { return };
        if every == 0 {
            return;
        }
        while (self.ckpt_count + 1).saturating_mul(every) <= records {
            self.ckpt_count += 1;
            let n = self.ckpt_count * every;
            let actions = if self.o.checkpoint_actions.is_empty() {
                vec![args::CheckpointAction::Echo(None)]
            } else {
                self.o.checkpoint_actions.clone()
            };
            for a in actions {
                self.checkpoint_action(&a, n, write);
            }
        }
    }

    fn checkpoint_text(&self, fmt: &[u8], n: u64, write: bool) -> Vec<u8> {
        let mut out = Vec::new();
        let mut i = 0;
        while i < fmt.len() {
            if fmt[i] == b'%' && i + 1 < fmt.len() {
                match fmt[i + 1] {
                    b'u' => out.extend_from_slice(n.to_string().as_bytes()),
                    b's' => out.extend_from_slice(if write { b"write" } else { b"read" }),
                    b'T' => {
                        let bytes = n * (self.o.record_size.unwrap_or(self.o.blocking_factor * 512) as u64);
                        let h = create::human_size(bytes);
                        let tag = if write { "W" } else { "R" };
                        out.extend_from_slice(format!("{tag}: {bytes} ({h}, {h}/s)").as_bytes());
                    }
                    b'%' => out.push(b'%'),
                    b'*' => {}
                    other => {
                        out.push(b'%');
                        out.push(other);
                    }
                }
                i += 2;
            } else {
                out.push(fmt[i]);
                i += 1;
            }
        }
        out
    }

    fn checkpoint_action(&mut self, a: &args::CheckpointAction, n: u64, write: bool) {
        use args::CheckpointAction as A;
        match a {
            A::Echo(None) => {
                let what = if write { "Write" } else { "Read" };
                self.msg(format!("{what} checkpoint {n}"));
            }
            A::Echo(Some(f)) => {
                let text = self.checkpoint_text(f, n, write);
                self.msg(text);
            }
            A::Dot => {
                if self.verbose_to_stderr {
                    self.out.flush();
                    sysutil::eprint(b".");
                } else {
                    self.out.write(b".");
                    self.out.flush();
                }
            }
            A::Bell | A::Ttyout(_) => {
                let text = match a {
                    A::Ttyout(f) => self.checkpoint_text(f, n, write),
                    _ => b"\x07".to_vec(),
                };
                if let Ok(fd) = open(b"/dev/tty", OFlags::WRONLY, 0) {
                    let _ = sysabi::sys::write_all(fd, &text);
                    let _ = sysabi::sys::close(fd);
                }
            }
            A::Sleep(s) => {
                let _ = sysabi::sys::current().nanosleep(std::time::Duration::from_secs(*s));
            }
            A::Totals => {
                let bytes = n * (self.o.record_size.unwrap_or(self.o.blocking_factor * 512) as u64);
                create::print_totals(self, if write { "written" } else { "read" }, bytes);
            }
            A::Exec(cmd) => {
                let _ = compress::run_shell_command(self, cmd);
            }
            A::Wait(_) => {}
        }
    }
}

impl Tar {
    /// Mensagem no stderr com o prefixo do programa; o stdout é descarregado antes, como o `error()` do
    /// gnulib faz.
    pub fn msg(&mut self, text: impl AsRef<[u8]>) {
        self.out.flush();
        let mut line = self.prog.clone().into_bytes();
        line.extend_from_slice(b": ");
        line.extend_from_slice(text.as_ref());
        line.push(b'\n');
        sysutil::eprint(line);
    }

    /// Erro não fatal: mensagem e status 2.
    pub fn error(&mut self, text: impl AsRef<[u8]>) {
        self.msg(text);
        self.exit = 2;
    }

    /// Erro fatal: mensagem e "Error is not recoverable".
    pub fn fatal(&mut self, text: impl AsRef<[u8]>) -> Fatal {
        self.msg(text);
        self.msg(b"Error is not recoverable: exiting now");
        self.exit = 2;
        Fatal
    }

    /// `nome: Cannot ...: strerror`, como o tar escreve erros de chamada de sistema.
    pub fn sys_error(&mut self, name: &[u8], what: &str, e: sysabi::Errno) {
        let mut m = quote::colon(name);
        m.extend_from_slice(format!(": {what}: {}", e.message()).as_bytes());
        self.error(m);
    }

    /// Linha de listagem verbosa (stdout ou stderr, conforme o arquivo; ou o `--index-file`).
    pub fn stdlis(&mut self, line: &[u8]) {
        if let Some(fd) = self.index_fd {
            let mut l = line.to_vec();
            l.push(b'\n');
            let _ = sysabi::sys::write_all(fd, &l);
            return;
        }
        if self.verbose_to_stderr {
            self.out.flush();
            let mut l = line.to_vec();
            l.push(b'\n');
            sysutil::eprint(l);
        } else {
            self.out.write(line);
            self.out.write(b"\n");
        }
    }

    /// Avisa uma vez por prefixo removido ("Removing leading `/' from member names").
    pub fn warn_prefix(&mut self, prefix: &[u8], link: bool) {
        if prefix.is_empty() || self.warned_prefixes.iter().any(|(p, l)| p == prefix && *l == link) {
            return;
        }
        self.warned_prefixes.push((prefix.to_vec(), link));
        let mut m = b"Removing leading `".to_vec();
        m.extend_from_slice(&quote::escape(prefix));
        m.extend_from_slice(if link { b"' from hard link targets" } else { b"' from member names" });
        self.msg(m);
    }

    /// Nome do arquivo (`-f`, `TAPE` ou `-`).
    pub fn archive_name(&self) -> Vec<u8> {
        if let Some(a) = self.o.archives.last() {
            return a.clone();
        }
        match sysutil::getenv("TAPE") {
            Some(t) if !t.is_empty() => t,
            _ => b"-".to_vec(),
        }
    }

    /// Prefixo "block N: " do `-R`.
    pub fn block_prefix(&self, block: u64) -> Vec<u8> {
        if self.o.block_number { format!("block {block}: ").into_bytes() } else { Vec::new() }
    }
}

/// Resultado de uma leitura de cabeçalhos em laço.
pub enum Flow {
    Continue,
    Stop,
}

impl Tar {
    /// Laço de leitura comum a `-t`, `-x`, `-d` e `--delete`: trata blocos de zeros, cabeçalhos
    /// inválidos ("This does not look like a tar archive", "Skipping to next header") e o fim.
    /// `action` recebe cada membro e tem que consumir os dados dele.
    pub fn read_and(
        &mut self,
        r: &mut Reader,
        action: &mut dyn FnMut(&mut Tar, &mut Reader, Member) -> R<Flow>,
    ) -> R<()> {
        let mut first = true;
        let mut in_failure = false;
        let record = self.o.record_size.unwrap_or(self.o.blocking_factor * 512).max(512) as u64;
        loop {
            sysabi::sys::checkpoint();
            self.checkpoint(r.offset.div_ceil(record), false);
            let status = r.read_header();
            for w in std::mem::take(&mut r.warnings) {
                self.error(w);
            }
            match status {
                Status::Member(m) => {
                    if in_failure {
                        self.error(b"Skipping to next header");
                        in_failure = false;
                    }
                    first = false;
                    match action(self, r, *m)? {
                        Flow::Continue => {}
                        Flow::Stop => return Ok(()),
                    }
                }
                Status::ZeroBlock => {
                    if in_failure {
                        self.error(b"Skipping to next header");
                        in_failure = false;
                    }
                    if self.o.block_number {
                        let line = format!("block {}: ** Block of NULs **", r.block - 1);
                        self.stdlis(line.as_bytes());
                    }
                    if self.o.ignore_zeros {
                        continue;
                    }
                    let at = r.block;
                    match r.read_header() {
                        Status::ZeroBlock => {}
                        _ => {
                            if self.warning_enabled(b"alone-zero-block") {
                                self.msg(format!("A lone zero block at {at}"));
                            }
                        }
                    }
                    return Ok(());
                }
                Status::EndOfFile => {
                    if self.o.block_number {
                        let line = format!("block {}: ** End of File **", r.block);
                        self.stdlis(line.as_bytes());
                    }
                    if first && !in_failure && !self.child_failed {
                        self.error(b"This does not look like a tar archive");
                    }
                    return Ok(());
                }
                Status::Failure => {
                    if first && !in_failure {
                        self.error(b"This does not look like a tar archive");
                    }
                    first = false;
                    in_failure = true;
                    self.exit = 2;
                }
                Status::Error(ReadError::UnexpectedEof) => {
                    return Err(self.fatal(b"Unexpected EOF in archive"));
                }
                Status::Error(ReadError::Io(e)) => {
                    let name = self.archive_name();
                    let mut m = quote::colon(&name);
                    m.extend_from_slice(format!(": Read error: {}", e.message()).as_bytes());
                    return Err(self.fatal(m));
                }
            }
        }
    }

    /// Um aviso da família `--warning` está ligado.
    pub fn warning_enabled(&self, name: &[u8]) -> bool {
        let mut on = true;
        for w in &self.o.warnings_off {
            if w == name || w == b"all" {
                on = false;
            }
        }
        for w in &self.o.warnings_on {
            if w == name || w == b"all" {
                on = true;
            }
        }
        on
    }

    /// Pula os dados de um membro, tratando fim inesperado.
    pub fn skip_member(&mut self, r: &mut Reader, m: &Member) -> R<()> {
        match r.skip_data(m.data_size()) {
            Ok(()) => Ok(()),
            Err(ReadError::UnexpectedEof) => Err(self.fatal(b"Unexpected EOF in archive")),
            Err(ReadError::Io(e)) => {
                let name = self.archive_name();
                let mut msg = quote::colon(&name);
                msg.extend_from_slice(format!(": Read error: {}", e.message()).as_bytes());
                Err(self.fatal(msg))
            }
        }
    }
}

/// `prog: msg` e o "Try ... --usage" (o `USAGE_ERROR` do tar); devolve 2.
pub fn usage_error(prog: &str, msg: &[u8]) -> i32 {
    let mut line = prog.as_bytes().to_vec();
    line.extend_from_slice(b": ");
    line.extend_from_slice(msg);
    line.push(b'\n');
    line.extend_from_slice(format!("Try '{prog} --help' or '{prog} --usage' for more information.\n").as_bytes());
    sysutil::eprint(line);
    2
}

pub fn main(_ctx: &mut Ctx, argv: &[OsString]) -> i32 {
    let args = sysutil::args_bytes(argv);
    let prog = sysutil::argv0(&args);
    let opts = match args::parse(&args, &prog) {
        Ok(Parsed::Run(o)) => *o,
        Ok(Parsed::Help) => return write_stdout(help::HELP),
        Ok(Parsed::Usage) => return write_stdout(help::USAGE),
        Ok(Parsed::Version) => return write_stdout(help::VERSION),
        Ok(Parsed::ShowDefaults) => return write_stdout(help::DEFAULTS),
        Err(ArgError::Getopt(m)) => {
            let mut line = m;
            line.extend_from_slice(format!("Try '{prog} --help' or '{prog} --usage' for more information.\n").as_bytes());
            sysutil::eprint(line);
            return 64;
        }
        Err(ArgError::Usage(m)) => return usage_error(&prog, &m),
        Err(ArgError::Fatal(m)) => {
            let mut line = format!("{prog}: ").into_bytes();
            line.extend_from_slice(&m);
            line.extend_from_slice(format!("\n{prog}: Error is not recoverable: exiting now\n").as_bytes());
            sysutil::eprint(line);
            return 2;
        }
        Err(ArgError::Plain(m)) => {
            let text = String::from_utf8_lossy(&m).replacen("tar: ", &format!("{prog}: "), 1);
            sysutil::eprint(text);
            return 2;
        }
    };
    let prog = opts.program_name.clone().unwrap_or(prog);
    let lister = list::Lister::new(opts.full_time, opts.utc, opts.numeric_owner);
    let mut t = Tar {
        prog,
        o: opts,
        out: Output::stdout(),
        exit: 0,
        verbose_to_stderr: false,
        owners: owner::Db::default(),
        lister,
        warned_prefixes: Vec::new(),
        transforms: Vec::new(),
        child_failed: false,
        excluder: names::Excluder::default(),
        ckpt_count: 0,
        index_fd: None,
        start_time: sysabi::sys::current().clock_gettime(sysabi::Clock::Monotonic).unwrap_or_default(),
    };
    let result = run(&mut t);
    t.out.flush();
    match result {
        Ok(()) => {
            if t.exit == 2 {
                t.msg(b"Exiting with failure status due to previous errors");
            }
            t.exit
        }
        Err(Fatal) => 2,
    }
}

fn write_stdout(text: &str) -> i32 {
    match sysabi::sys::write_all(Fd::STDOUT, text.as_bytes()) {
        Ok(()) => 0,
        Err(_) => 2,
    }
}

/// Validações que o tar faz depois de ler a linha de comando, e o despacho pro modo.
fn run(t: &mut Tar) -> R<()> {
    let Some(mode) = t.o.mode else {
        let p = t.prog.clone();
        usage_error(&p, b"You must specify one of the '-Acdtrux', '--delete' or '--test-label' options");
        t.exit = 2;
        return Err(Fatal);
    };
    if t.o.o_flag {
        if mode == Mode::Create {
            t.o.old_archive = true;
        } else {
            t.o.same_owner = Some(false);
        }
    }
    if t.o.old_archive && t.o.format.is_none() {
        t.o.format = Some(header::Format::V7);
    }
    for e in std::mem::take(&mut t.o.transforms) {
        match transform::Expr::parse(&e.expr) {
            Ok(x) => t.transforms.push(x),
            Err(msg) => {
                return Err(t.fatal(msg));
            }
        }
    }
    create::load_files_from(t)?;
    t.excluder = create::build_excluder(t)?;
    if let Some(idx) = t.o.index_file.clone() {
        match open(&idx, OFlags::WRONLY | OFlags::CREAT | OFlags::TRUNC, 0o666) {
            Ok(fd) => t.index_fd = Some(fd),
            Err(e) => {
                let mut m = quote::colon(&idx);
                m.extend_from_slice(format!(": Cannot open: {}", e.message()).as_bytes());
                return Err(t.fatal(m));
            }
        }
    }
    if mode == Mode::Create && t.o.names.is_empty() {
        let p = t.prog.clone();
        usage_error(&p, b"Cowardly refusing to create an empty archive");
        t.exit = 2;
        return Err(Fatal);
    }
    let writes_archive_to_stdout =
        matches!(mode, Mode::Create | Mode::Append | Mode::Update | Mode::Catenate) && t.archive_name() == b"-";
    if writes_archive_to_stdout || (mode == Mode::Extract && t.o.to_stdout) {
        t.verbose_to_stderr = true;
    }
    if mode == Mode::Delete && t.archive_name() == b"-" {
        t.verbose_to_stderr = true;
    }
    match mode {
        Mode::List => list_mode(t),
        Mode::Extract => extract::run(t),
        Mode::Create => create::run(t),
        Mode::Append | Mode::Update => create::append(t, mode == Mode::Update),
        Mode::Diff => compare::run(t),
        Mode::Catenate => create::catenate(t),
        Mode::Delete => compare::delete(t),
        Mode::TestLabel => compare::test_label(t),
    }
}

/// Abre o arquivo pra leitura, com detecção de compressão.
pub fn open_for_read(t: &mut Tar) -> R<(Reader, compress::Child)> {
    let name = t.archive_name();
    compress::open_read(t, &name)
}

/// Operandos de uma leitura. O `-K` entra na cabeça da lista como o `add_starting_file` do GNU, então,
/// depois de achado, também filtra os membros seguintes.
pub fn read_names(t: &Tar) -> Vec<args::NameArg> {
    let mut v = t.o.names.clone();
    if let Some(s) = &t.o.starting_file {
        let arg = args::NameArg {
            name: s.clone(),
            chdir: Vec::new(),
            flags: Default::default(),
            recursion: true,
            from_file: false,
            list_file: false,
        };
        v.insert(0, arg);
    }
    v
}

/// `-t`.
fn list_mode(t: &mut Tar) -> R<()> {
    let (mut r, child) = open_for_read(t)?;
    let mut names = names::NameList::new(&read_names(t));
    let starting = t.o.starting_file.clone();
    let mut started = starting.is_none();
    let res = t.read_and(&mut r, &mut |t, r, m| {
        if !started {
            if starting.as_deref().is_some_and(|s| names::name_matches(s, &m.name, Default::default(), true, true)) {
                started = true;
            } else {
                t.skip_member(r, &m)?;
                return Ok(Flow::Continue);
            }
        }
        if !names.items.is_empty() {
            match names.find(&m.name) {
                Some(i) => {
                    names.items[i].found += 1;
                    if let Some(occ) = t.o.occurrence
                        && names.items[i].found > occ
                    {
                        t.skip_member(r, &m)?;
                        return Ok(Flow::Continue);
                    }
                }
                None => {
                    t.skip_member(r, &m)?;
                    return Ok(Flow::Continue);
                }
            }
        }
        if t.excluder.excluded(&m.name) {
            t.skip_member(r, &m)?;
            return Ok(Flow::Continue);
        }
        let (prefix, _) = names::unsafe_prefix(&m.name);
        if !t.o.absolute_names && prefix.first() == Some(&b'/') {
            let slashes: Vec<u8> = prefix.iter().take_while(|&&c| c == b'/').copied().collect();
            t.warn_prefix(&slashes, false);
        }
        print_member(t, &m, m.main_block);
        t.skip_member(r, &m)?;
        if let Some(occ) = t.o.occurrence
            && !names.items.is_empty()
            && names.items.iter().all(|n| n.found >= occ)
        {
            return Ok(Flow::Stop);
        }
        Ok(Flow::Continue)
    });
    compress::finish_read(t, child, res)?;
    read_totals(t, &r);
    report_unmatched(t, &names);
    Ok(())
}

/// `--totals` da leitura: os bytes lidos, em registros inteiros.
pub fn read_totals(t: &mut Tar, r: &Reader) {
    if t.o.totals {
        let rec = t.o.record_size.unwrap_or(t.o.blocking_factor * 512).max(512) as u64;
        let bytes = r.offset.div_ceil(rec) * rec;
        create::print_totals(t, "read", bytes);
    }
}

/// Imprime o membro no estilo do `-t` (nome, ou linha longa com `-v`).
pub fn print_member(t: &mut Tar, m: &Member, block: u64) {
    let mut line = t.block_prefix(block);
    let shown = transform::display_name(t, &m.name);
    if t.o.verbose > 0 {
        let q = t.o.quoting.clone();
        line.extend_from_slice(&t.lister.line(m, &shown, &q));
    } else {
        line.extend_from_slice(&quote::quote_with(&shown, &t.o.quoting, false));
    }
    t.stdlis(&line);
}

/// "x: Not found in archive" pros operandos que não casaram.
pub fn report_unmatched(t: &mut Tar, names: &names::NameList) {
    let missing: Vec<Vec<u8>> = names.unmatched().map(|n| n.arg.name.clone()).collect();
    for name in missing {
        let mut m = quote::colon(&name);
        m.extend_from_slice(b": Not found in archive");
        t.error(m);
    }
    if let Some(occ) = t.o.occurrence {
        let short: Vec<Vec<u8>> =
            names.items.iter().filter(|n| n.found > 0 && n.found < occ).map(|n| n.arg.name.clone()).collect();
        for name in short {
            let mut m = quote::colon(&name);
            m.extend_from_slice(": Required occurrence not found in archive".to_string().as_bytes());
            t.error(m);
        }
    }
}

/// Abre um arquivo do sandbox, com as flags dadas.
pub fn open(path: &[u8], flags: OFlags, mode: u32) -> sysabi::sys::SysResult<Fd> {
    sysabi::sys::open(path, flags | OFlags::CLOEXEC, mode)
}
