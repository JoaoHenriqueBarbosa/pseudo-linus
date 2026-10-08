//! Os dados que o kernel entrega ao procfs: um processo ou thread ([`ProcData`]), a descrição de um fd
//! ([`FdLink`], [`FdInfo`]) e o estado global da máquina ([`SysData`]). O procfs só formata; quem sabe
//! os números é o kernel, através de [`ProcProvider`].

use crate::fs::Link;
use crate::mount::Loc;
use crate::types::*;

/// Destino de `/proc/<pid>/fd/N`.
#[derive(Clone, Debug)]
pub struct FdLink {
    /// Texto do `readlink` (caminho, `pipe:[N]`, `socket:[N]`...).
    pub text: Vec<u8>,
    /// Pra onde abrir leva.
    pub target: Link,
    /// Bits de permissão do link conforme o modo de abertura (`lr-x------`, `l-wx------`, `lrwx------`).
    pub perm: Mode,
}

/// O que `/proc/<pid>/fdinfo/N` mostra.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FdInfo {
    /// Deslocamento da descrição de arquivo aberto.
    pub pos: u64,
    /// `f_flags` mais `O_CLOEXEC` do fd (impresso em octal).
    pub flags: u32,
    /// Montagem do objeto (`mnt_id`).
    pub mnt_id: u32,
    /// Inode do objeto.
    pub ino: u64,
    /// Linhas que o `show_fdinfo` do objeto acrescenta (as `tfd:` do epoll), já com a quebra de linha.
    pub extra: String,
}

/// Máscaras de sinal, um bit por sinal (bit `n - 1` é o sinal `n`), como as linhas `Sig*` do `status`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SigMasks {
    /// Pendentes da thread (`SigPnd`).
    pub pending: u64,
    /// Pendentes do processo (`ShdPnd`).
    pub shared_pending: u64,
    pub blocked: u64,
    pub ignored: u64,
    pub caught: u64,
}

/// Memória de um processo, em kB, como as linhas `Vm*` e `Rss*` do `status`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MemData {
    pub vm_peak: u64,
    pub vm_size: u64,
    pub vm_lck: u64,
    pub vm_pin: u64,
    pub vm_hwm: u64,
    pub vm_rss: u64,
    pub rss_anon: u64,
    pub rss_file: u64,
    pub rss_shmem: u64,
    pub vm_data: u64,
    pub vm_stk: u64,
    pub vm_exe: u64,
    pub vm_lib: u64,
    pub vm_pte: u64,
    pub vm_swap: u64,
}

/// O que o procfs precisa saber de um processo (ou de uma de suas threads).
#[derive(Clone, Debug, Default)]
pub struct ProcData {
    /// Tgid.
    pub pid: Pid,
    /// Tid da thread descrita (igual a `pid` na principal e nos dados do processo).
    pub tid: Pid,
    pub ppid: Pid,
    pub pgid: Pid,
    pub sid: Pid,
    /// `R`, `S`, `D`, `T`, `Z`.
    pub state: char,
    pub comm: Vec<u8>,
    /// argv com um NUL depois de cada argumento.
    pub cmdline: Vec<u8>,
    /// Ambiente com um NUL depois de cada `NAME=valor`.
    pub environ: Vec<u8>,
    /// Uid e gid efetivos (os de dono de `/proc/<pid>`).
    pub uid: Uid,
    pub gid: Gid,
    /// Reais e salvos, pras linhas `Uid:`/`Gid:` do `status`.
    pub ruid: Uid,
    pub suid: Uid,
    pub rgid: Gid,
    pub sgid: Gid,
    /// Grupos suplementares.
    pub groups: Vec<Gid>,
    pub umask: Mode,
    pub cwd: Option<Loc>,
    pub root: Option<Loc>,
    pub exe: Option<Loc>,
    pub nice: i32,
    /// Threads vivas do processo.
    pub num_threads: u32,
    /// Tempo de CPU da thread (ou do processo todo, nos dados do processo), em ns.
    pub utime_ns: u64,
    pub stime_ns: u64,
    /// CPU dos filhos já colhidos, em ns.
    pub cutime_ns: u64,
    pub cstime_ns: u64,
    /// Criação, em ns desde o boot do sandbox.
    pub start_ns: u64,
    /// `None` quando o processo não tem mais espaço de endereçamento (zumbi).
    pub mem: Option<MemData>,
    /// `(cur, max)` de cada `RLIMIT_*`, na ordem dos números do Linux; `u64::MAX` é ilimitado.
    pub rlimits: [(u64, u64); 16],
    pub sig: SigMasks,
    /// Sinais enfileirados do usuário (`SigQ`, antes da barra).
    pub sigq: u64,
    /// Tamanho da tabela de fds (`FDSize`); 0 num zumbi.
    pub fdsize: u32,
    pub voluntary_ctxt: u64,
    pub nonvoluntary_ctxt: u64,
    /// Última CPU em que rodou.
    pub last_cpu: u32,
    /// CPUs do sandbox (`Cpus_allowed`).
    pub ncpus: u32,
    /// Política de escalonamento (`SCHED_*`, campo 41 do `stat`) e prioridade de tempo real (campo 40).
    pub policy: u32,
    pub rt_priority: u32,
    /// CPUs permitidas (`Cpus_allowed` e `Cpus_allowed_list`); `None` são as `ncpus` todas.
    pub cpus_allowed: Option<Vec<usize>>,
    /// Criado por `fork` e ainda sem `exec` (`PF_FORKNOEXEC`).
    pub fork_noexec: bool,
    /// Status de espera de um zumbi, no formato do `wait4` (`exit_code` do `stat`); 0 num vivo.
    pub exit_code: i32,
    /// Morreu por sinal (`PF_SIGNALED`).
    pub signaled: bool,
    /// Thread que não é a principal: o `exit_signal` do `stat` é -1.
    pub secondary: bool,
    /// Tempo total na CPU e trocas de contexto, pro `schedstat`.
    pub sched_runtime_ns: u64,
    pub sched_switches: u64,
}

/// Tempos de uma CPU virtual, em ns.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CpuTimes {
    pub user_ns: u64,
    pub nice_ns: u64,
    pub system_ns: u64,
}

/// Memória da máquina (o sandbox), em kB, nas grandezas que o kernel acompanha. O procfs deriva o resto
/// das linhas do `meminfo`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MemSystem {
    pub total: u64,
    /// Soma do anônimo residente dos processos.
    pub anon: u64,
    /// Páginas de arquivo mapeadas pelos processos.
    pub mapped: u64,
    /// Conteúdo dos tmpfs.
    pub shmem: u64,
    pub kernel_stack: u64,
    pub page_tables: u64,
    /// Soma do espaço de endereçamento dos processos.
    pub committed: u64,
}

/// Estado global da máquina: o que `stat`, `uptime` e `loadavg` mostram.
#[derive(Clone, Debug, Default)]
pub struct SysData {
    pub ncpus: u32,
    /// Tempo desde o boot do sandbox, em ns.
    pub uptime_ns: u64,
    /// Segundos Unix do boot.
    pub btime: i64,
    pub cpu: Vec<CpuTimes>,
    /// Trocas de contexto desde o boot.
    pub ctxt: u64,
    /// Processos e threads criados desde o boot.
    pub forks: u64,
    pub procs_running: u32,
    pub procs_blocked: u32,
    /// Processos e threads existentes (`nr_threads`).
    pub nr_threads: u32,
    /// Último pid alocado.
    pub last_pid: Pid,
    /// `avenrun` em ponto fixo (`FSHIFT` = 11).
    pub load: [u64; 3],
}

/// Fonte de dados do procfs: o kernel implementa.
pub trait ProcProvider: Send + Sync {
    /// pids visíveis no sandbox, em ordem crescente.
    fn pids(&self) -> Vec<Pid>;
    /// Dados do processo (a visão do tgid: tempos somados, threads contadas).
    fn process(&self, pid: Pid) -> Option<ProcData>;
    /// Dono e grupo do processo, sem montar o resto. `None` se não existe.
    fn owner(&self, pid: Pid) -> Option<(Uid, Gid)> {
        self.process(pid).map(|p| (p.uid, p.gid))
    }
    /// tids das threads vivas do processo, a principal primeiro e as outras em ordem crescente.
    fn tids(&self, pid: Pid) -> Option<Vec<Pid>> {
        self.process(pid).map(|p| vec![p.pid])
    }
    /// Dados de uma thread do processo. `None` se `tid` não é thread de `pid`.
    fn thread(&self, pid: Pid, tid: Pid) -> Option<ProcData> {
        if tid == pid { self.process(pid) } else { None }
    }
    /// Filhos da thread, em ordem crescente (`task/<tid>/children`).
    fn children(&self, pid: Pid, tid: Pid) -> Option<Vec<Pid>>;
    /// fds abertos, em ordem crescente.
    fn fds(&self, pid: Pid) -> Option<Vec<i32>>;
    fn fd(&self, cx: &Caller, pid: Pid, fd: i32) -> Option<FdLink>;
    /// Conteúdo de `fdinfo/N`.
    fn fdinfo(&self, cx: &Caller, pid: Pid, fd: i32) -> Option<FdInfo>;
    /// Estado global da máquina.
    fn system(&self) -> SysData;
    /// Memória da máquina (separada de `system` porque somar os processos custa mais).
    fn mem(&self) -> MemSystem;
    /// CPUs virtuais do sandbox.
    fn ncpus(&self) -> u32;
    /// `/proc/version`, com a quebra de linha.
    fn version(&self) -> Vec<u8>;
    /// `/proc/sys/kernel/pid_max`.
    fn pid_max(&self) -> u32 {
        4_194_304
    }
    /// `/proc/sys/kernel/osrelease` (o `uname -r`), sem a quebra de linha.
    fn os_release(&self) -> Vec<u8> {
        b"6.12.0".to_vec()
    }
    /// `/proc/sys/kernel/version` (o `uname -v`), sem a quebra de linha.
    fn kernel_version(&self) -> Vec<u8> {
        b"#1 SMP PREEMPT_DYNAMIC".to_vec()
    }
    /// `nodename` do UTS do sandbox (`/proc/sys/kernel/hostname`).
    fn hostname(&self) -> Vec<u8> {
        b"localhost".to_vec()
    }
    /// `domainname` do UTS do sandbox (`/proc/sys/kernel/domainname`).
    fn domainname(&self) -> Vec<u8> {
        b"(none)".to_vec()
    }
    /// Troca o `nodename` (escrita em `/proc/sys/kernel/hostname`, já cortada em 64 bytes).
    fn set_hostname(&self, _name: &[u8]) -> SysResult<()> {
        Err(Errno::EPERM)
    }
    /// Troca o `domainname`.
    fn set_domainname(&self, _name: &[u8]) -> SysResult<()> {
        Err(Errno::EPERM)
    }
    /// Os sockets TCP do namespace de rede, na ordem em que o `/proc/net/tcp` os lista.
    fn tcp_socks(&self) -> Vec<TcpSock> {
        Vec::new()
    }
    /// Os sockets do domínio Unix, na ordem em que o `/proc/net/unix` os lista.
    fn unix_socks(&self) -> Vec<UnixSockRow> {
        Vec::new()
    }
    /// Os sockets UDP com porta (os sem `bind` não entram na tabela), na ordem dos baldes do hash.
    fn udp_socks(&self) -> Vec<UdpSockRow> {
        Vec::new()
    }
    /// Quantos sockets existem no namespace (o `sockets: used` do `sockstat`): os completos de cada
    /// protocolo, sem os de time-wait, e os UDP também sem porta.
    fn socket_count(&self) -> usize {
        0
    }
}

/// Uma linha do `/proc/net/udp` ou `udp6` (`udp4_format_sock`/`udp6_sock_seq_show`).
#[derive(Clone, Debug)]
pub struct UdpSockRow {
    pub v6: bool,
    /// O balde do hash da porta (`udp_hashfn`).
    pub sl: u32,
    pub local_ip: [u8; 16],
    pub local_port: u16,
    pub remote_ip: [u8; 16],
    pub remote_port: u16,
    /// `TCP_ESTABLISHED` (1) conectado, `TCP_CLOSE` (7) sem par.
    pub state: u8,
    /// `sk_rmem_alloc`: a soma do `truesize` dos datagramas na fila.
    pub rx_queue: u32,
    pub uid: Uid,
    pub inode: u64,
    pub refcnt: u32,
    pub ptr: u32,
    pub drops: u32,
}

/// Uma linha do `/proc/net/unix` (`unix_seq_show`).
#[derive(Clone, Debug)]
pub struct UnixSockRow {
    /// O `%pK` do socket.
    pub ptr: u32,
    pub refcnt: u32,
    /// `__SO_ACCEPTCON` (0x10000) num socket em escuta.
    pub flags: u32,
    /// `SOCK_STREAM` (1), `SOCK_DGRAM` (2) ou `SOCK_SEQPACKET` (5).
    pub ty: u16,
    /// `SS_UNCONNECTED` (1), `SS_CONNECTING` (2, o par ainda não aceito) ou `SS_CONNECTED` (3).
    pub state: u8,
    /// Zero no par ainda não aceito, que não tem inode.
    pub inode: u64,
    /// O nome como o `bind` recebeu; no espaço abstrato começa com o byte nulo.
    pub path: Option<Vec<u8>>,
}

/// Estados do TCP no formato do `/proc/net/tcp` (`include/net/tcp_states.h`).
pub mod tcp_state {
    pub const ESTABLISHED: u8 = 0x01;
    pub const SYN_SENT: u8 = 0x02;
    pub const FIN_WAIT2: u8 = 0x05;
    pub const TIME_WAIT: u8 = 0x06;
    pub const CLOSE_WAIT: u8 = 0x08;
    pub const LISTEN: u8 = 0x0A;
}

/// Um socket TCP como o `tcp4_seq_show`/`tcp6_seq_show` o vê. Endereços IPv4 ocupam os 4 primeiros
/// bytes de `local_ip`/`remote_ip`, na ordem da rede.
#[derive(Clone, Debug)]
pub struct TcpSock {
    pub v6: bool,
    pub local_ip: [u8; 16],
    pub local_port: u16,
    pub remote_ip: [u8; 16],
    pub remote_port: u16,
    pub state: u8,
    pub tx_queue: u32,
    pub rx_queue: u32,
    /// Timer ativo (`tr`) e o tempo até ele disparar em ticks de 1/100 s (`tm->when`).
    pub timer: u8,
    pub when: u64,
    /// `icsk_retransmits` (`retrnsmt`): as retransmissões do SYN ou do segmento pendente.
    pub retrans: u32,
    pub uid: Uid,
    pub inode: u64,
    pub refcnt: u32,
    /// O `%pK` do socket: o ponteiro com hash, que no x86_64 sai com 32 bits significativos.
    pub ptr: u32,
    /// `rto ato qack snd_cwnd ssthresh` de um socket completo; `None` num socket de time-wait
    /// (inclusive o FIN_WAIT2 órfão), que não tem essas colunas.
    pub tail: Option<(u32, u32, u32, u32, i32)>,
}
