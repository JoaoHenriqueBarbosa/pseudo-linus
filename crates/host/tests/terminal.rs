//! O `osh` interativo local num terminal: um pty real do host com o `osh` no escravo, como um emulador
//! de terminal o roda, e o teste no mestre, no papel de quem digita e lê a tela.

use std::fs::File;
use std::io::{Read, Write};
use std::os::fd::AsFd;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use rustix::event::{PollFd, PollFlags, Timespec, poll};
use rustix::process::{Pid, Signal, kill_process};
use rustix::pty::{OpenptFlags, grantpt, openpt, ptsname, unlockpt};
use rustix::termios::{LocalModes, Winsize, tcgetattr, tcsetwinsize};

/// Prazo de cada espera, com margem, porque a primeira partida do sandbox monta a imagem inteira.
const PATIENCE: Duration = Duration::from_secs(120);
/// Os modos locais que o terminal do host perde enquanto o `osh` o usa em modo cru.
const COOKED: LocalModes = LocalModes::ICANON.union(LocalModes::ECHO).union(LocalModes::ISIG);

fn set_winsize(master: impl AsFd, rows: u16, cols: u16) {
    tcsetwinsize(master, Winsize { ws_row: rows, ws_col: cols, ws_xpixel: 0, ws_ypixel: 0 }).unwrap();
}

/// O que um emulador de terminal mostraria: o suficiente do VT100 para o que o bash e o editor de
/// linha escrevem (texto com quebra automática, `CR`, `LF`, `BS` e as sequências `CSI` de cursor e de
/// apagar). Cada caractere ocupa uma célula.
struct Screen {
    rows: usize,
    cols: usize,
    lines: Vec<Vec<char>>,
    row: usize,
    col: usize,
    /// O cursor passou da última coluna: o próximo caractere quebra a linha.
    wrap_pending: bool,
    /// Sequência de escape incompleta e bytes de UTF-8 incompletos do último pedaço.
    partial: Vec<u8>,
}

impl Screen {
    fn new(rows: u16, cols: u16) -> Screen {
        let (rows, cols) = (rows as usize, cols as usize);
        Screen { rows, cols, lines: vec![vec![' '; cols]; rows], row: 0, col: 0, wrap_pending: false, partial: Vec::new() }
    }

    fn resize(&mut self, rows: u16, cols: u16) {
        self.rows = rows as usize;
        self.cols = cols as usize;
        for l in &mut self.lines {
            l.resize(self.cols, ' ');
        }
        while self.lines.len() > self.rows {
            self.lines.remove(0);
            self.row = self.row.saturating_sub(1);
        }
        self.lines.resize(self.rows, vec![' '; self.cols]);
        self.col = self.col.min(self.cols - 1);
    }

    fn line_feed(&mut self) {
        if self.row + 1 == self.rows {
            self.lines.remove(0);
            self.lines.push(vec![' '; self.cols]);
        } else {
            self.row += 1;
        }
    }

    fn put(&mut self, c: char) {
        if self.wrap_pending {
            self.wrap_pending = false;
            self.col = 0;
            self.line_feed();
        }
        self.lines[self.row][self.col] = c;
        if self.col + 1 == self.cols {
            self.wrap_pending = true;
        } else {
            self.col += 1;
        }
    }

    fn csi(&mut self, params: &str, fin: char) {
        let nums: Vec<usize> = params.trim_start_matches('?').split(';').map(|n| n.parse().unwrap_or(0)).collect();
        let n = nums.first().copied().unwrap_or(0).max(1);
        if fin != 'm' && fin != 'h' && fin != 'l' {
            self.wrap_pending = false;
        }
        match fin {
            'A' => self.row = self.row.saturating_sub(n),
            'B' => self.row = (self.row + n).min(self.rows - 1),
            'C' => self.col = (self.col + n).min(self.cols - 1),
            'D' => self.col = self.col.saturating_sub(n),
            'H' => {
                self.row = nums.first().copied().unwrap_or(1).max(1) - 1;
                self.col = nums.get(1).copied().unwrap_or(1).max(1) - 1;
            }
            'J' => match nums.first().copied().unwrap_or(0) {
                2 => self.lines = vec![vec![' '; self.cols]; self.rows],
                _ => {
                    self.lines[self.row][self.col..].fill(' ');
                    for l in &mut self.lines[self.row + 1..] {
                        l.fill(' ');
                    }
                }
            },
            'K' => self.lines[self.row][self.col..].fill(' '),
            _ => {}
        }
    }

    fn feed(&mut self, data: &[u8]) {
        let mut bytes = std::mem::take(&mut self.partial);
        bytes.extend_from_slice(data);
        let mut i = 0;
        while i < bytes.len() {
            let b = bytes[i];
            match b {
                0x1b => {
                    let Some(&kind) = bytes.get(i + 1) else { break };
                    if kind != b'[' {
                        i += 2;
                        continue;
                    }
                    let Some(end) = bytes[i + 2..].iter().position(|c| (0x40..=0x7e).contains(c)) else { break };
                    let params = String::from_utf8_lossy(&bytes[i + 2..i + 2 + end]).into_owned();
                    self.csi(&params, bytes[i + 2 + end] as char);
                    i += 3 + end;
                    continue;
                }
                b'\r' => {
                    self.col = 0;
                    self.wrap_pending = false;
                }
                b'\n' => {
                    self.wrap_pending = false;
                    self.line_feed();
                }
                0x08 => {
                    self.wrap_pending = false;
                    self.col = self.col.saturating_sub(1);
                }
                0x00..=0x1f => {}
                _ => {
                    let len = match b {
                        0xC0..=0xDF => 2,
                        0xE0..=0xEF => 3,
                        0xF0..=0xF7 => 4,
                        _ => 1,
                    };
                    if i + len > bytes.len() {
                        break;
                    }
                    let c = std::str::from_utf8(&bytes[i..i + len]).ok().and_then(|s| s.chars().next()).unwrap_or('?');
                    self.put(c);
                    i += len;
                    continue;
                }
            }
            i += 1;
        }
        self.partial = bytes[i..].to_vec();
    }

    fn line(&self, row: usize) -> String {
        self.lines[row].iter().collect::<String>().trim_end().to_string()
    }

    /// A linha do cursor e as `n` acima dela, de cima para baixo.
    fn last_lines(&self, n: usize) -> Vec<String> {
        (self.row.saturating_sub(n)..=self.row).map(|r| self.line(r)).collect()
    }
}

struct Terminal {
    master: File,
    child: Child,
    /// O que chegou do mestre e ainda não foi consumido por um `expect`.
    pending: Vec<u8>,
    screen: Screen,
}

impl Terminal {
    /// Abre um pty do host de `rows` por `cols` e roda o `osh` com o escravo em stdin, stdout e stderr.
    fn start(rows: u16, cols: u16) -> Terminal {
        let master = openpt(OpenptFlags::RDWR | OpenptFlags::NOCTTY).unwrap();
        grantpt(&master).unwrap();
        unlockpt(&master).unwrap();
        let slave_path = ptsname(&master, Vec::new()).unwrap().into_string().unwrap();
        let slave = File::options().read(true).write(true).open(slave_path).unwrap();
        set_winsize(&master, rows, cols);
        let child = Command::new(env!("CARGO_BIN_EXE_osh"))
            .env("TERM", "xterm-256color")
            .stdin(Stdio::from(slave.try_clone().unwrap()))
            .stdout(Stdio::from(slave.try_clone().unwrap()))
            .stderr(Stdio::from(slave))
            .spawn()
            .unwrap();
        Terminal { master: File::from(master), child, pending: Vec::new(), screen: Screen::new(rows, cols) }
    }

    /// Troca o tamanho da janela. O `osh` aqui não é líder de sessão com este terminal de controle,
    /// então o SIGWINCH que o kernel mandaria ao grupo em primeiro plano vai à mão.
    fn resize(&mut self, rows: u16, cols: u16) {
        set_winsize(&self.master, rows, cols);
        self.screen.resize(rows, cols);
        kill_process(Pid::from_child(&self.child), Signal::WINCH).unwrap();
    }

    fn send(&mut self, keys: &str) {
        self.master.write_all(keys.as_bytes()).unwrap();
    }

    /// Lê a tela até `want` aparecer e devolve o que veio até ele, inclusive.
    fn expect(&mut self, want: &str) -> String {
        let deadline = Instant::now() + PATIENCE;
        loop {
            if let Some(at) = self.pending.windows(want.len()).position(|w| w == want.as_bytes()) {
                let seen: Vec<u8> = self.pending.drain(..at + want.len()).collect();
                return String::from_utf8_lossy(&seen).into_owned();
            }
            let left = deadline.saturating_duration_since(Instant::now());
            let timeout = Timespec { tv_sec: left.as_secs() as i64, tv_nsec: left.subsec_nanos() as _ };
            let ready = poll(&mut [PollFd::new(&self.master, PollFlags::IN)], Some(&timeout)).unwrap();
            let mut buf = [0u8; 4096];
            match if ready == 0 { Ok(0) } else { self.master.read(&mut buf) } {
                Ok(n) if n > 0 => {
                    self.pending.extend_from_slice(&buf[..n]);
                    self.screen.feed(&buf[..n]);
                }
                end => panic!("{end:?} esperando {want:?}; na tela: {:?}", String::from_utf8_lossy(&self.pending)),
            }
        }
    }

    /// Espera o editor de linha pedir a próxima linha (a colagem entre colchetes liga junto com o
    /// prompt) e mais um pouco de tela, e devolve a linha do cursor e as `n` de cima.
    fn prompt(&mut self, n: usize) -> Vec<String> {
        self.expect("\x1b[?2004h");
        self.expect("# ");
        self.settle();
        self.screen.last_lines(n)
    }

    /// Lê o que ainda chegar até a tela ficar parada por um instante.
    fn settle(&mut self) {
        loop {
            let timeout = Timespec { tv_sec: 0, tv_nsec: 300_000_000 };
            if poll(&mut [PollFd::new(&self.master, PollFlags::IN)], Some(&timeout)).unwrap() == 0 {
                return;
            }
            let mut buf = [0u8; 4096];
            match self.master.read(&mut buf) {
                Ok(n) if n > 0 => {
                    self.pending.extend_from_slice(&buf[..n]);
                    self.screen.feed(&buf[..n]);
                }
                _ => return,
            }
        }
    }

    /// Manda teclas e devolve a linha do cursor quando a tela para.
    fn keys(&mut self, keys: &str) -> String {
        self.send(keys);
        self.settle();
        self.pending.clear();
        self.screen.line(self.screen.row)
    }

    /// Os modos locais do terminal do host (o par mestre e escravo tem um termios só).
    fn modes(&self) -> LocalModes {
        tcgetattr(&self.master).unwrap().local_modes
    }

    /// Espera o `osh` terminar e devolve o código de saída dele.
    fn wait(&mut self) -> i32 {
        let deadline = Instant::now() + PATIENCE;
        loop {
            if let Some(status) = self.child.try_wait().unwrap() {
                return status.code().unwrap_or_else(|| panic!("o osh morreu: {status}"));
            }
            assert!(Instant::now() < deadline, "o osh não terminou");
            std::thread::sleep(Duration::from_millis(20));
        }
    }
}

impl Drop for Terminal {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[test]
fn interactive_bash_on_the_host_terminal() {
    let mut t = Terminal::start(37, 111);
    assert!(t.modes().contains(COOKED), "o pty do teste nasce em modo canônico");
    // Partida: o prompt do bash interativo, com o terminal do host já em modo cru.
    let start = t.prompt(0);
    assert!(start[0].ends_with(":~#"), "{start:?}");
    assert!(!t.modes().intersects(COOKED), "{:?}", t.modes());
    // Eco do que foi digitado e a saída do comando.
    t.send("echo dig$((40+2))\r");
    assert_eq!(t.prompt(2)[..2], ["root@".to_string() + &start[0][5..] + " echo dig$((40+2))", "dig42".into()]);
    // O terminal é o do sandbox, com o TERM e o tamanho da janela do host.
    t.send("tty; echo $TERM\r");
    assert_eq!(t.prompt(3)[1..], ["/dev/pts/0", "xterm-256color", &start[0]]);
    t.send("stty size; echo $LINES $COLUMNS\r");
    assert_eq!(t.prompt(3)[1..], ["37 111", "37 111", &start[0]]);
    t.resize(50, 132);
    t.send("stty size; echo $LINES $COLUMNS\r");
    assert_eq!(t.prompt(3)[1..], ["50 132", "50 132", &start[0]]);
    // Ctrl-C interrompe o comando em primeiro plano e o bash volta ao prompt.
    t.send("sh -c 'echo pronto; exec sleep 100'\r");
    t.expect("pronto\r\n");
    let started = Instant::now();
    t.send("\x03");
    assert_eq!(t.prompt(1), ["^C", &start[0]]);
    assert!(started.elapsed() < Duration::from_secs(30));
    t.send("echo status=$?\r");
    assert_eq!(t.prompt(1), ["status=130", &start[0]]);
    // O osh sai com o status do bash e devolve o modo do terminal do host.
    t.send("exit 3\r");
    assert_eq!(t.wait(), 3);
    assert!(t.modes().contains(COOKED), "{:?}", t.modes());
}

/// O editor de linha do bash interativo: as teclas de cursor e de edição mexem na linha em vez de
/// aparecer como `^[[A`, com histórico e completação, numa janela estreita em que a linha quebra.
#[test]
fn line_editing() {
    let mut t = Terminal::start(24, 60);
    let prompt = t.prompt(0)[0].clone();
    let p = |line: &str| format!("{prompt} {line}");
    // Cursor para a esquerda, HOME e END: o texto entra no meio da linha.
    assert_eq!(t.keys("cho abc"), p("cho abc"));
    assert_eq!(t.keys("\x1b[D\x1b[DX"), p("cho aXbc"));
    assert_eq!(t.keys("\x1b[He"), p("echo aXbc"));
    assert_eq!(t.keys("\x1b[F def"), p("echo aXbc def"));
    t.send("\r");
    assert_eq!(t.prompt(1), ["aXbc def", &prompt]);
    // Backspace apaga antes do cursor e Delete apaga sob ele.
    assert_eq!(t.keys("echo abcd\x1b[D\x1b[D\x7f\x1b[3~"), p("echo ad"));
    t.send("\r");
    assert_eq!(t.prompt(1), ["ad", &prompt]);
    // Seta para cima traz os comandos anteriores; para baixo volta à linha que estava sendo digitada.
    assert_eq!(t.keys("nova"), p("nova"));
    assert_eq!(t.keys("\x1b[A"), p("echo ad"));
    assert_eq!(t.keys("\x1b[A"), p("echo aXbc def"));
    assert_eq!(t.keys("\x1b[B\x1b[B"), p("nova"));
    assert_eq!(t.keys("\x15\x1b[A\x1b[A"), p("echo aXbc def"));
    t.send("\r");
    assert_eq!(t.prompt(1), ["aXbc def", &prompt]);
    // Teclas sem ação (PGUP, PGDN, F5) não deixam nada na linha.
    assert_eq!(t.keys("echo ok\x1b[5~\x1b[6~\x1b[15~"), p("echo ok"));
    t.send("\r");
    assert_eq!(t.prompt(1), ["ok", &prompt]);
    // Tab completa o caminho; com mais de um candidato, o segundo Tab lista.
    t.send("mkdir -p /tmp/dir_a /tmp/dir_b; touch '/tmp/um arquivo'\r");
    t.prompt(0);
    assert_eq!(t.keys("ls /tmp/um\t"), p("ls /tmp/um\\ arquivo"));
    t.send("\r");
    assert_eq!(t.prompt(1), ["'/tmp/um arquivo'", &prompt]);
    assert_eq!(t.keys("cd /tmp/di\t"), p("cd /tmp/dir_"));
    t.send("\t");
    t.settle();
    assert_eq!(t.screen.last_lines(1), ["dir_a/ dir_b/", &p("cd /tmp/dir_")]);
    // Ctrl-C numa linha pela metade descarta a linha.
    // O Ctrl-C vai depois de a linha aparecer: o tty descarta a entrada ainda não lida ao gerar o SIGINT.
    assert_eq!(t.keys("\x15echo metade"), p("echo metade"));
    t.send("\x03");
    assert_eq!(t.prompt(1), [p("echo metade^C"), prompt.clone()]);
    t.send("echo $?\r");
    assert_eq!(t.prompt(1), ["130", &prompt]);
    // Uma linha mais larga que a janela quebra e continua editável nas duas linhas da tela.
    let long = "x".repeat(70);
    t.send(&format!("echo {long}"));
    t.settle();
    let wrapped = t.screen.last_lines(1);
    assert_eq!(wrapped.concat(), p(&format!("echo {long}")));
    assert_eq!(t.keys("\x01\x1b[C\x1b[C\x1b[C\x1b[C\x1b[C-"), p("echo -").chars().chain(long.chars()).take(60).collect::<String>());
    t.send("\r");
    assert_eq!(t.prompt(0)[0], prompt);
    // O histórico do builtin `history` tem os comandos na ordem.
    t.send("history 2\r");
    let shown = t.prompt(3).concat();
    assert!(shown.contains(&format!("  echo -{long}")) && shown.contains("  history 2"), "{shown:?}");
    t.send("exit\r");
    assert_eq!(t.wait(), 0);
}

#[test]
fn losing_the_host_terminal_ends_the_session() {
    let mut t = Terminal::start(24, 80);
    t.prompt(0);
    t.send("sleep 100\r");
    t.expect("sleep 100\r\n");
    // SIGHUP no osh (o emulador de terminal fechou): hangup no bash, e o osh sai com o status dele.
    kill_process(Pid::from_child(&t.child), Signal::HUP).unwrap();
    assert_eq!(t.wait(), 128 + Signal::HUP.as_raw());
    assert!(t.modes().contains(COOKED), "{:?}", t.modes());
}
