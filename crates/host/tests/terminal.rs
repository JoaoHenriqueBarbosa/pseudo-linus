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

struct Terminal {
    master: File,
    child: Child,
    /// O que chegou do mestre e ainda não foi consumido por um `expect`.
    pending: Vec<u8>,
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
        Terminal { master: File::from(master), child, pending: Vec::new() }
    }

    /// Troca o tamanho da janela. O `osh` aqui não é líder de sessão com este terminal de controle,
    /// então o SIGWINCH que o kernel mandaria ao grupo em primeiro plano vai à mão.
    fn resize(&mut self, rows: u16, cols: u16) {
        set_winsize(&self.master, rows, cols);
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
                Ok(n) if n > 0 => self.pending.extend_from_slice(&buf[..n]),
                end => panic!("{end:?} esperando {want:?}; na tela: {:?}", String::from_utf8_lossy(&self.pending)),
            }
        }
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
    assert!(t.expect("# ").ends_with(":~# "));
    assert!(!t.modes().intersects(COOKED), "{:?}", t.modes());
    // Eco do que foi digitado e a saída do comando, com o fim de linha do terminal do sandbox.
    t.send("echo dig$((40+2))\r");
    assert!(t.expect("# ").starts_with("echo dig$((40+2))\r\ndig42\r\n"));
    // O terminal é o do sandbox, com o TERM e o tamanho da janela do host.
    t.send("tty; echo $TERM\r");
    assert!(t.expect("# ").starts_with("tty; echo $TERM\r\n/dev/pts/0\r\nxterm-256color\r\n"));
    t.send("stty size\r");
    assert!(t.expect("# ").starts_with("stty size\r\n37 111\r\n"));
    t.resize(50, 132);
    t.send("stty size\r");
    assert!(t.expect("# ").starts_with("stty size\r\n50 132\r\n"));
    // Ctrl-C interrompe o comando em primeiro plano e o bash volta ao prompt. O `pronto` diz que o
    // processo já existe: o eco da linha digitada sai antes de o bash a ler.
    t.send("sh -c 'echo pronto; exec sleep 100'\r");
    t.expect("pronto\r\n");
    let started = Instant::now();
    t.send("\x03");
    assert_eq!(t.expect("# ").split_once("\r\n").map(|(echo, _)| echo), Some("^C"));
    assert!(started.elapsed() < Duration::from_secs(30));
    t.send("echo status=$?\r");
    assert!(t.expect("# ").starts_with("echo status=$?\r\nstatus=130\r\n"));
    // O osh sai com o status do bash e devolve o modo do terminal do host.
    t.send("exit 3\r");
    assert_eq!(t.wait(), 3);
    assert!(t.modes().contains(COOKED), "{:?}", t.modes());
}

#[test]
fn losing_the_host_terminal_ends_the_session() {
    let mut t = Terminal::start(24, 80);
    t.expect("# ");
    t.send("sleep 100\r");
    t.expect("sleep 100\r\n");
    // SIGHUP no osh (o emulador de terminal fechou): hangup no bash, e o osh sai com o status dele.
    kill_process(Pid::from_child(&t.child), Signal::HUP).unwrap();
    assert_eq!(t.wait(), 128 + Signal::HUP.as_raw());
    assert!(t.modes().contains(COOKED), "{:?}", t.modes());
}
