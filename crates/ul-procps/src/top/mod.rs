//! `top` do procps-ng 4.0.4 (Debian 13) em modo batch (`-b`): o resumo (hora, carga, tarefas, CPUs,
//! memória e swap), o cabeçalho das colunas e as tarefas, com as opções `-b -c -d -E -e -H -i -n -O
//! -o -p -S -s -U -u -V -w -1 -h`.
//!
//! O programa é um porte de `top.c`: `args` (getopt e `parse_args`), `data` (stat, meminfo, pids e o
//! histórico dos ticks), `fields` (a tabela de campos e as chaves de ordenação), `frame` (o quadro) e
//! `text` (justificação e escalas). Os dados vêm só do `/proc`.
//!
//! Diferenças conhecidas: o modo interativo (sem `-b`) não existe no sandbox, os arquivos de
//! configuração (`~/.toprc`, `/etc/toprc`) não são lidos, e o tratamento de sinais do original
//! (SIGINT, SIGTERM) não é reproduzido.

mod args;
mod data;
mod fields;
mod frame;
mod text;

use std::collections::HashMap;
use std::ffi::OsString;
use std::io::Write;
use std::time::Duration;

use sysabi::{Ctx, Fd, sys};
use ul_misc::util::io;

use self::data::{Counts, Hist, MemVals, StatState};
use crate::ps::Ps;
use crate::ps::proc::Pt;

/// Resultado interno: `Err(código)` é o `exit(código)` do original.
pub(crate) type R<T> = Result<T, i32>;

/// Uma tarefa da última leitura, com os deltas do histórico.
pub struct Task {
    pub p: Pt,
    pub pcpu: u32,
    pub maj_delta: i32,
    pub min_delta: i32,
}

/// A janela única do top (`Winstk[0]`): o que os flags `DEF_WINFLGS` e as opções mudam.
struct Win {
    show_cmdline: bool,
    show_ctimes: bool,
    show_idleps: bool,
    /// `Qsrt_NORMAL`: ordem decrescente.
    qsrt_normal: bool,
    view_cpusum: bool,
    double_up: i32,
    sortindx: usize,
    usrseltyp: u8,
    usrselflg: bool,
    usrseluid: u32,
}

/// O estado global do programa.
pub struct Top {
    myname: String,
    batch: bool,
    thread_mode: bool,
    /// `Loops`: -1 não termina.
    loops: i32,
    delay: f32,
    secure: bool,
    width_mode: i32,
    w: Win,
    monpids: Vec<i32>,
    summ_mscale: usize,
    task_mscale: usize,
    // Geometria.
    screen_cols: i32,
    screen_rows: i64,
    w_set: bool,
    w_cols: i32,
    w_rows: i32,
    // Colunas.
    pid_width: i32,
    procflgs: Vec<usize>,
    varcolsz: i32,
    columnhdr: Vec<u8>,
    begtask: i64,
    begnext: i64,
    // Constantes e escalas do quadro.
    hertz: u64,
    cpu_cnt: i32,
    cpu_pmax: f32,
    frame_etscale: f32,
    uptime_sav: f64,
    boot_tics: u64,
    // Dados.
    stat: StatState,
    mem: MemVals,
    mem_secs: i64,
    hist: HashMap<i32, Hist>,
    tasks: Vec<Task>,
    counts: Counts,
    ps: Ps,
    // Saída.
    out: Vec<u8>,
    msg_row: i32,
    sum_row: Vec<u8>,
    sum_tog: i32,
}

pub fn main(_ctx: &mut Ctx, args: &[OsString]) -> i32 {
    io::run(|| {
        let mut top = Top::new(args);
        match top.run(io::args_bytes(args)) {
            Ok(()) => 0,
            Err(code) => code,
        }
    })
}

impl Top {
    fn new(args: &[OsString]) -> Top {
        let argv = io::args_bytes(args);
        let first = argv.first().cloned().unwrap_or_default();
        let name = match first.iter().rposition(|b| *b == b'/') {
            Some(i) => first[i + 1..].to_vec(),
            None => first,
        };
        Top {
            myname: String::from_utf8_lossy(&name).into_owned(),
            batch: false,
            thread_mode: false,
            loops: -1,
            delay: 3.0,
            secure: false,
            width_mode: 0,
            w: Win {
                show_cmdline: false,
                show_ctimes: false,
                show_idleps: true,
                qsrt_normal: true,
                view_cpusum: true,
                double_up: 1,
                sortindx: fields::EU_CPU,
                usrseltyp: 0,
                usrselflg: true,
                usrseluid: 0,
            },
            monpids: Vec::new(),
            summ_mscale: 1,
            task_mscale: 0,
            screen_cols: 80,
            screen_rows: i64::from(i32::MAX),
            w_set: false,
            w_cols: 0,
            w_rows: 0,
            pid_width: 5,
            procflgs: Vec::new(),
            varcolsz: 0,
            columnhdr: Vec::new(),
            begtask: -1,
            begnext: 1,
            hertz: 100,
            cpu_cnt: 0,
            cpu_pmax: 99.9,
            frame_etscale: 0.0,
            uptime_sav: 0.0,
            boot_tics: 0,
            stat: StatState::default(),
            mem: MemVals::default(),
            mem_secs: 0,
            hist: HashMap::new(),
            tasks: Vec::new(),
            counts: Counts::default(),
            ps: Ps::new(&[]),
            out: Vec::new(),
            msg_row: 0,
            sum_row: Vec::new(),
            sum_tog: 0,
        }
    }

    /// `puts`: o texto e uma quebra de linha.
    fn puts(&mut self, s: &str) {
        self.out.extend_from_slice(s.as_bytes());
        self.out.push(b'\n');
    }

    /// Escreve o que está pendente e descarrega o stdout (`fflush(stdout)`).
    fn flush(&mut self) {
        let mut so = io::stdout();
        let _ = so.write_all(&self.out);
        let _ = so.flush();
        self.out.clear();
    }

    /// `bye_bye(NULL)`: em batch termina o quadro com uma quebra de linha. Devolve o código 0.
    fn bye_ok(&mut self) -> i32 {
        if self.batch {
            self.out.push(b'\n');
        }
        self.flush();
        0
    }

    /// `whack_terminal`: em batch o terminal é o `dumb`; sem `-b` o top precisa de um terminal de
    /// verdade, que o sandbox não oferece.
    fn whack_terminal(&mut self) -> R<()> {
        if self.batch {
            return Ok(());
        }
        let term = sys::getenv("TERM").filter(|t| !t.is_empty());
        match term {
            None => {
                io::eprint("Error opening terminal: unknown.\n");
                return Err(1);
            }
            Some(t) if !crate::watch::terminfo_exists(&t) => {
                io::eprint(format!("Error opening terminal: {}.\n", String::from_utf8_lossy(&t)));
                return Err(1);
            }
            Some(_) => {}
        }
        let sysc = sys::current();
        if !sysc.isatty(Fd::STDIN) {
            return self.error_exit("failed tty get");
        }
        self.error_exit("interactive mode is not available, use -b")
    }

    /// `frame_make`: uma leitura e um quadro.
    fn frame_make(&mut self, first: bool) -> R<()> {
        self.tasks_refresh()?;
        self.cpus_refresh(2737)?;
        self.memory_refresh()?;
        if first {
            // A primeira vez prepara a bomba: uma pausa e outra leitura das tarefas, para que o
            // %CPU tenha um intervalo de onde sair.
            if let Some(sysc) = sys::try_current() {
                let _ = sysc.nanosleep(Duration::from_micros(100_000));
            }
            self.tasks_refresh()?;
        } else {
            self.out.extend_from_slice(b"\n\n");
        }
        self.msg_row = 0;
        self.summary_show();
        let max_lines = (self.screen_rows - i64::from(self.msg_row)) - 1;
        self.window_show(max_lines)?;
        self.flush();
        Ok(())
    }

    fn run(&mut self, argv: Vec<Vec<u8>>) -> R<()> {
        // `before`: o total de CPUs vem da primeira leitura do /proc/stat.
        if let Err(e) = self.stat.read() {
            return self.error_exit(&format!("library failed cpu statistics, at 3685: {e}"));
        }
        self.cpu_cnt = self.stat.cpus.len() as i32;
        self.parse_args(argv)?;
        self.whack_terminal()?;
        // `wins_stage_2`: com a soma de CPUs ou o nó NUMA à vista, nada de linhas lado a lado.
        if self.w.view_cpusum {
            self.w.double_up = 0;
        }
        self.zap_fieldstab()?;
        let mut first = true;
        loop {
            self.frame_make(first)?;
            first = false;
            if self.loops > 0 {
                self.loops -= 1;
            }
            if self.loops == 0 {
                break;
            }
            let secs = self.delay.max(0.0);
            if let Some(sysc) = sys::try_current() {
                let _ = sysc.nanosleep(Duration::from_secs_f64(f64::from(secs)));
            }
        }
        self.bye_ok();
        Ok(())
    }
}
