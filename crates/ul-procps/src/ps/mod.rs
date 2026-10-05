//! `ps` do procps-ng 4.0.4 (Debian 13): opções SysV (Unix98), BSD e GNU longas, colunas `-o`, `-O`,
//! formatos predefinidos (`aux`, `-ef`, `-l`, `u`, `j`...), seleção por lista, ordenação, floresta
//! (`f`, `-H`), threads (`-L`, `-T`, `H`, `m`) e as mensagens de erro do original.
//!
//! O programa é um porte módulo a módulo do upstream: `parser` (parser.c), `sortformat`
//! (sortformat.c), `display` (display.c e select.c), `output` (output.c) e `help` (help.c). Os dados
//! vêm só do `/proc`, lido como a libproc2 faz (`proc`): a falta de um arquivo vira o mesmo valor
//! padrão do original. A personalidade (`PS_PERSONALITY`) está portada; o `PS_FORMAT` também.
//!
//! Diferenças conhecidas, todas em colunas que dependem de serviços que o sandbox não tem: as
//! colunas do systemd (`unit`, `slice`, `lsession`...) mostram `-` como o procps sem sessão do
//! systemd, e a coluna `numa` mostra `-1` (o procps só carrega o libnuma no `top`).

mod display;
mod help;
mod output;
mod parser;
mod proc;
mod sortformat;
mod table;
mod util;

use std::ffi::OsString;

use sysabi::{Ctx, sys};
use ul_misc::util::io;

use crate::common::Names;

pub use table::{FORMAT_ARRAY, MACRO_ARRAY};

/// Tamanho do buffer de saída do ps: limite de largura de qualquer coluna.
pub const OUTBUF_SIZE: i32 = 2 * 64 * 1024;

// Tipos de seleção.
pub const SEL_RUID: u8 = 1;
pub const SEL_EUID: u8 = 2;
pub const SEL_RGID: u8 = 5;
pub const SEL_EGID: u8 = 6;
pub const SEL_PGRP: u8 = 9;
pub const SEL_PID: u8 = 10;
pub const SEL_TTY: u8 = 11;
pub const SEL_SESS: u8 = 12;
pub const SEL_COMM: u8 = 13;
pub const SEL_PPID: u8 = 14;
pub const SEL_PID_QUICK: u8 = 15;

// Origem do formato (quem inventou a coluna).
pub const U98: i32 = 0;
pub const XXX: i32 = 1;
pub const DEC: i32 = 2;
pub const AIX: i32 = 3;
pub const SCO: i32 = 4;
pub const LNX: i32 = 5;
pub const BSD: i32 = 6;
pub const SUN: i32 = 7;
pub const HPU: i32 = 8;
pub const SGI: i32 = 9;
pub const SOE: i32 = 10;
pub const TST: i32 = 11;

// Flags de coluna: justificação (máscara 0x0f), depois os demais.
pub const CF_JUST_MASK: u32 = 0x0f;
pub const CF_USER: u32 = 1;
pub const CF_LEFT: u32 = 2;
pub const CF_RIGHT: u32 = 3;
pub const CF_UNLIMITED: u32 = 4;
pub const CF_WCHAN: u32 = 5;
pub const CF_SIGNAL: u32 = 6;
pub const CF_PIDMAX: u32 = 0x0000_0010;
pub const CF_PRINT_THREAD_ONLY: u32 = 0x1000_0000;
pub const CF_PRINT_PROCESS_ONLY: u32 = 0x2000_0000;
pub const CF_PRINT_EVERY_TIME: u32 = 0x4000_0000;
pub const CF_PRINT_AS_NEEDED: u32 = 0x8000_0000;
pub const CF_PRINT_MASK: u32 = 0xf000_0000;

// thread_flags.
pub const TF_B_H: u32 = 0x0001;
pub const TF_B_M: u32 = 0x0002;
pub const TF_U_M: u32 = 0x0004;
pub const TF_U_T: u32 = 0x0008;
pub const TF_U_L: u32 = 0x0010;
pub const TF_SHOW_PROC: u32 = 0x0100;
pub const TF_SHOW_TASK: u32 = 0x0200;
pub const TF_SHOW_BOTH: u32 = 0x0400;
pub const TF_LOOSE_TASKS: u32 = 0x0800;
pub const TF_NO_SORT: u32 = 0x1000;
pub const TF_MUST_USE: u32 = 0x4000;

// Personalidade.
pub const PER_BSD_H: u32 = 0x0002;
pub const PER_BSD_M: u32 = 0x0004;
pub const PER_IRIX_L: u32 = 0x0008;
pub const PER_FORCE_BSD: u32 = 0x0010;
pub const PER_GOOD_O: u32 = 0x0020;
pub const PER_OLD_M: u32 = 0x0040;
pub const PER_NO_DEFAULT_G: u32 = 0x0080;
pub const PER_ZAP_ADDR: u32 = 0x0100;
pub const PER_SANE_USER: u32 = 0x0200;
pub const PER_HPUX_X: u32 = 0x0400;
pub const PER_SVR4_X: u32 = 0x0800;

// Seleção simples por máscara.
pub const SS_B_X: u32 = 0x01;
pub const SS_B_G: u32 = 0x02;
pub const SS_U_D: u32 = 0x04;
pub const SS_U_A: u32 = 0x08;
pub const SS_B_A: u32 = 0x10;

// Formatos predefinidos.
pub const FF_UF: u32 = 0x0001;
pub const FF_UJ: u32 = 0x0002;
pub const FF_UL: u32 = 0x0004;
pub const FF_BJ: u32 = 0x0008;
pub const FF_BL: u32 = 0x0010;
pub const FF_BS: u32 = 0x0020;
pub const FF_BU: u32 = 0x0040;
pub const FF_BV: u32 = 0x0080;
pub const FF_LX: u32 = 0x0100;
pub const FF_LM: u32 = 0x0200;
pub const FF_FC: u32 = 0x0400;

// Modificadores de formato.
pub const FM_C: u32 = 0x0001;
pub const FM_J: u32 = 0x0002;
pub const FM_Y: u32 = 0x0004;
pub const FM_P: u32 = 0x0010;
pub const FM_M: u32 = 0x0020;
pub const FM_F: u32 = 0x0080;

// Origem das listas deferidas de ordenação e formato.
pub const SF_U_O_UP: i32 = 1;
pub const SF_U_O: i32 = 2;
pub const SF_B_O_UP: i32 = 3;
pub const SF_B_O: i32 = 4;
pub const SF_B_M: i32 = 5;
pub const SF_G_SORT: i32 = 6;
pub const SF_G_FORMAT: i32 = 7;

// Cabeçalhos.
pub const HEAD_SINGLE: i32 = 0;
pub const HEAD_NONE: i32 = 1;
pub const HEAD_MULTI: i32 = 2;

/// Resultado interno: `Err(código)` é o `exit(código)` do original.
pub type R<T> = Result<T, i32>;

/// Função que imprime uma coluna: escreve em `out` e devolve a quantidade de células de tela.
pub type PrFn = fn(&mut Ps, &proc::Pt, &mut Vec<u8>) -> usize;

/// Um item do `format_array`.
pub struct Fmt {
    pub spec: &'static str,
    pub head: &'static str,
    pub pr: PrFn,
    /// `pr` é o `pr_nop` (coluna que só imprime `-`).
    pub nop: bool,
    pub sr: Item,
    pub width: i32,
    pub vendor: i32,
    pub flags: u32,
}

/// Uma coluna da lista de formato final (`format_node`).
#[derive(Clone)]
pub struct FNode {
    pub name: Vec<u8>,
    /// `None` é o texto fixo do AIX (e do `:` do SGI).
    pub pr: Option<PrFn>,
    pub width: i32,
    pub vendor: i32,
    pub flags: u32,
}

/// Um item de ordenação.
#[derive(Clone)]
pub struct SortNode {
    pub sr: Item,
    /// 1 crescente, -1 decrescente; 0 (o `direction = 0` que o `short_sort_parse` deixa a partir do
    /// segundo código) faz a biblioteca recusar a ordenação, que então não acontece.
    pub order: i8,
}

/// Um nó de seleção (`selection_node`).
#[derive(Clone)]
pub struct SelNode {
    pub typecode: u8,
    /// Valores numéricos (pid, uid, gid, número do dispositivo) na ordem dada na linha de comando.
    pub nums: Vec<u64>,
    /// Nomes de comando do `-C` (até 63 bytes cada, sem terminador).
    pub cmds: Vec<Vec<u8>>,
}

/// Opção deferida de ordenação ou formato.
pub struct SfNode {
    pub sf: Vec<u8>,
    pub code: i32,
    pub s_cooked: Vec<SortNode>,
    pub f_cooked: Vec<FNode>,
}

/// Item de dado de processo que a coluna ou a ordenação consulta (`enum pids_item`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Item {
    AddrCodeEnd,
    AddrCodeStart,
    AddrCurrEip,
    AddrCurrEsp,
    AddrStackStart,
    AutogrpId,
    AutogrpNice,
    Cgname,
    Cgroup,
    Cmd,
    Cmdline,
    Exe,
    Flags,
    FltMaj,
    FltMin,
    IdEgid,
    IdEgroup,
    IdEuid,
    IdEuser,
    IdFgid,
    IdFgroup,
    IdFuid,
    IdFuser,
    IdLogin,
    IdPgrp,
    IdPid,
    IdPpid,
    IdRgid,
    IdRgroup,
    IdRuid,
    IdRuser,
    IdSession,
    IdSgid,
    IdSgroup,
    IdSuid,
    IdSuser,
    IdTgid,
    IdTpgid,
    IoReadBytes,
    IoReadChars,
    IoReadOps,
    IoWriteBytes,
    IoWriteCbytes,
    IoWriteChars,
    IoWriteOps,
    Lxcname,
    MemResPgs,
    MemShrPgs,
    Nice,
    Nlwp,
    Noop,
    NsCgroup,
    NsIpc,
    NsMnt,
    NsNet,
    NsPid,
    NsTime,
    NsUser,
    NsUts,
    OomAdj,
    OomScore,
    Priority,
    PriorityRt,
    Processor,
    ProcessorNode,
    RssRlim,
    SchedClass,
    SdMach,
    SdOuid,
    SdSeat,
    SdSess,
    SdSlice,
    SdUnit,
    SdUunit,
    Sigblocked,
    Sigcatch,
    Sigignore,
    Signals,
    Sigpending,
    SmapPrvTotal,
    SmapPss,
    State,
    Supgids,
    Supgroups,
    TicsAll,
    TicsBegan,
    TicsUser,
    TicsUserC,
    TimeAll,
    TimeElapsed,
    TtyName,
    Utilization,
    UtilizationC,
    VmData,
    VmExe,
    VmLib,
    VmRss,
    VmRssLocked,
    VmSize,
    VmStack,
    VsizeBytes,
    WchanName,
}

/// Estado global do programa (as variáveis globais do ps.c, parser.c, sortformat.c e output.c).
pub struct Ps {
    pub myname: String,
    // Linha de comando e estado do parser.
    pub argv: Vec<Vec<u8>>,
    pub thisarg: usize,
    pub force_bsd: bool,
    pub w_count: i32,
    // Globais.
    pub all_processes: bool,
    pub bsd_j_format: Option<&'static str>,
    pub bsd_l_format: Option<&'static str>,
    pub bsd_s_format: Option<&'static str>,
    pub bsd_u_format: Option<&'static str>,
    pub bsd_v_format: Option<&'static str>,
    pub sysv_f_format: Option<&'static str>,
    pub sysv_fl_format: Option<&'static str>,
    pub sysv_j_format: Option<&'static str>,
    pub sysv_l_format: Option<&'static str>,
    pub bsd_c_option: bool,
    pub bsd_e_option: bool,
    pub cached_euid: u32,
    pub cached_tty: i32,
    pub forest_prefix: Vec<u8>,
    pub forest_type: u8,
    pub format_flags: u32,
    pub format_list: Vec<FNode>,
    pub format_modifiers: u32,
    pub header_gap: i32,
    pub header_type: i32,
    pub include_dead_children: bool,
    pub lines_to_next_header: i32,
    pub lstart_format: Option<Vec<u8>>,
    pub negate_selection: bool,
    pub running_only: bool,
    pub page_size: i32,
    pub personality: u32,
    pub saved_personality_text: String,
    pub prefer_bsd_defaults: bool,
    pub screen_cols: i32,
    pub screen_rows: i32,
    /// A cabeça da lista do original é o índice 0 (o nó mais recente).
    pub selection_list: Vec<SelNode>,
    pub simple_select: u32,
    pub select_bits: u32,
    pub sort_list: Vec<SortNode>,
    pub thread_flags: u32,
    pub unix_f_option: bool,
    pub user_is_number: bool,
    pub wchan_is_number: bool,
    pub signal_names: bool,
    // sortformat.c
    pub sf_list: Vec<SfNode>,
    pub have_gnu_sort: bool,
    pub already_parsed_sort: bool,
    pub already_parsed_format: bool,
    pub errbuf: Option<String>,
    // output.c e display.c
    pub max_rightward: i32,
    pub wide_signals: bool,
    pub seconds_since_1970: i64,
    pub active_cols: i32,
    pub did_stuff: bool,
    pub proc_format_list: Vec<FNode>,
    pub task_format_list: Vec<FNode>,
    // libproc2.
    pub hertz: u64,
    pub boot_tics: u64,
    pub names: Names,
    pub pid_length_cache: Option<i32>,
    pub boot_time_cache: Option<i64>,
    pub mem_total_cache: Option<u64>,
    pub tz: Option<jiff::tz::TimeZone>,
    pub tty_map: Option<Vec<proc::TtyMapNode>>,
}

impl Ps {
    fn new(args: &[OsString]) -> Ps {
        let argv = io::args_bytes(args);
        let first = argv.first().cloned().unwrap_or_default();
        let name = match first.iter().rposition(|b| *b == b'/') {
            Some(i) => first[i + 1..].to_vec(),
            None => first,
        };
        Ps {
            myname: String::from_utf8_lossy(&name).into_owned(),
            argv,
            thisarg: 0,
            force_bsd: false,
            w_count: 0,
            all_processes: false,
            bsd_j_format: None,
            bsd_l_format: None,
            bsd_s_format: None,
            bsd_u_format: None,
            bsd_v_format: None,
            sysv_f_format: None,
            sysv_fl_format: None,
            sysv_j_format: None,
            sysv_l_format: None,
            bsd_c_option: false,
            bsd_e_option: false,
            cached_euid: 0,
            cached_tty: 0,
            forest_prefix: Vec::new(),
            forest_type: 0,
            format_flags: 0,
            format_list: Vec::new(),
            format_modifiers: 0,
            header_gap: -1,
            header_type: HEAD_SINGLE,
            include_dead_children: false,
            lines_to_next_header: 1,
            lstart_format: None,
            negate_selection: false,
            running_only: false,
            page_size: 4096,
            personality: 0,
            saved_personality_text: String::from("You found a bug!"),
            prefer_bsd_defaults: false,
            screen_cols: 80,
            screen_rows: 24,
            selection_list: Vec::new(),
            simple_select: 0,
            select_bits: 0,
            sort_list: Vec::new(),
            thread_flags: 0,
            unix_f_option: false,
            user_is_number: false,
            wchan_is_number: false,
            signal_names: false,
            sf_list: Vec::new(),
            have_gnu_sort: false,
            already_parsed_sort: false,
            already_parsed_format: false,
            errbuf: None,
            max_rightward: OUTBUF_SIZE - 1,
            wide_signals: false,
            seconds_since_1970: 0,
            active_cols: 0,
            did_stuff: false,
            proc_format_list: Vec::new(),
            task_format_list: Vec::new(),
            hertz: 100,
            boot_tics: 0,
            names: Names::new(),
            pid_length_cache: None,
            boot_time_cache: None,
            mem_total_cache: None,
            tz: None,
            tty_map: None,
        }
    }
}

pub fn main(_ctx: &mut Ctx, args: &[OsString]) -> i32 {
    io::run(|| {
        let mut ps = Ps::new(args);
        match display::run(&mut ps) {
            Ok(()) => 0,
            Err(code) => code,
        }
    })
}

/// Variável de ambiente como bytes.
pub(crate) fn getenv(name: &str) -> Option<Vec<u8>> {
    sys::getenv(name)
}
