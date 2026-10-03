//! H38: edição de linha sobre o pty do sandbox, não sobre o tty do host.
//!
//! - **Negativos com evidência**: `reedline` e `rustyline` são provados presos ao tty do host por
//!   código (arquivo e linha, conferidos em tempo de execução) e por teste (a sonda `f15-tty-probe`
//!   roda sem terminal de controle, sob strace).
//! - **Candidatos**: o `LineEditor` do `termwiz` sobre uma implementação nossa da trait `Terminal`, e o
//!   `noline` sobre `embedded_io::{Read, Write}` nossos. Os dois ficam ligados a um pty em memória
//!   (dois buffers de bytes) com um mini VT do lado de "fora" que responde as consultas de posição de
//!   cursor (CPR), como o emulador de terminal do agente responderia. Um roteiro de teclas em bytes
//!   crus (texto, setas, Home/End, Ctrl-A/E/W/U/K, Backspace, Delete, histórico, Ctrl-C, Enter) é
//!   dirigido nos dois, e cada linha resultante é comparada com o que o readline do bash produziria.
//! - **Sem /dev/tty**: o roteiro roda num processo filho com `setsid` (sem terminal de controle) e
//!   sob strace; entre os marcadores não pode haver abertura de `/dev/tty`, `/dev/pts*`, `/dev/ptmx`
//!   nem ioctl de termios.

use std::collections::VecDeque;
use std::io::Write as _;
use std::path::Path;
use std::process::{Command, Stdio};

use anyhow::{Context, Result};
use harness::{CandidateResult, Fit, Verdict};
use serde::Serialize;
use serde_json::json;
use termwiz::caps::{Capabilities, ProbeHints};
use termwiz::input::{InputEvent, InputParser, KeyCode, KeyEvent, Modifiers};
use termwiz::lineedit::{Action, BasicHistory, History, LineEditor, LineEditorHost, Movement};
use termwiz::render::RenderTty;
use termwiz::render::terminfo::TerminfoRenderer;
use termwiz::surface::Change;
use termwiz::terminal::{ScreenSize, Terminal, TerminalWaker};

use crate::common::{self, Section};
use crate::evidence::{self, CodeEvidence};

pub const PROMPT: &str = "$ ";
const ROWS: u16 = 24;
const COLS: u16 = 80;
const MARK_BEGIN: &str = "/f15-strace-marker-begin";
const MARK_END: &str = "/f15-strace-marker-end";

/// terminfo compilado do xterm-256color (cópia do `data/` do termwiz 0.23.3), embutido no binário pra
/// que nenhum arquivo do host seja lido.
const XTERM_TERMINFO: &[u8] = include_bytes!("../data/xterm-256color");

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub enum Expect {
    Line(&'static str),
    Cancelled,
}

/// Um passo do roteiro: bytes crus digitados e o que o readline do bash 5.2 (modo emacs) devolveria.
pub struct Step {
    pub id: &'static str,
    pub keys: &'static [u8],
    pub expect: Expect,
}

/// O roteiro de teclas. Sequências de xterm: setas `ESC [ A..D`, Home `ESC [ H`, End `ESC [ F`,
/// Delete `ESC [ 3 ~`; Backspace é DEL (0x7f), Enter é CR.
pub const STEPS: &[Step] = &[
    Step { id: "text-enter", keys: b"echo hello\r", expect: Expect::Line("echo hello") },
    Step { id: "left-arrow-insert", keys: b"ac\x1b[Db\r", expect: Expect::Line("abc") },
    Step { id: "home-end", keys: b"bc\x1b[Ha\x1b[Fd\r", expect: Expect::Line("abcd") },
    // Home/End no formato vt220/linux console (`ESC [ 1 ~`, `ESC [ 4 ~`), que tmux e o console mandam.
    Step { id: "home-end-vt220", keys: b"bc\x1b[1~a\x1b[4~d\r", expect: Expect::Line("abcd") },
    Step { id: "ctrl-a-ctrl-e", keys: b"bc\x01a\x05d\r", expect: Expect::Line("abcd") },
    Step { id: "right-arrow", keys: b"ac\x01\x1b[Cb\r", expect: Expect::Line("abc") },
    Step { id: "ctrl-w-word", keys: b"echo foo bar\x17baz\r", expect: Expect::Line("echo foo baz") },
    // readline: unix-word-rubout apaga até o espaço anterior, não até a barra.
    Step { id: "ctrl-w-path", keys: b"ls /usr/lib\x17x\r", expect: Expect::Line("ls x") },
    // readline: unix-line-discard apaga do cursor até o começo da linha.
    Step { id: "ctrl-u-mid-line", keys: b"echo abc\x1b[D\x1b[D\x15\r", expect: Expect::Line("bc") },
    Step { id: "ctrl-k", keys: b"echo abc\x01\x1b[C\x1b[C\x1b[C\x1b[C\x1b[C\x0b\r", expect: Expect::Line("echo ") },
    Step { id: "backspace", keys: b"echoo\x7f x\r", expect: Expect::Line("echo x") },
    Step { id: "delete-key", keys: b"abc\x01\x1b[3~\r", expect: Expect::Line("bc") },
    Step { id: "utf8-backspace", keys: "ação\x1b[D\x7f\r".as_bytes(), expect: Expect::Line("aço") },
    Step { id: "hist-one", keys: b"one\r", expect: Expect::Line("one") },
    Step { id: "hist-two", keys: b"two\r", expect: Expect::Line("two") },
    Step { id: "history-up-twice", keys: b"\x1b[A\x1b[A\r", expect: Expect::Line("one") },
    Step { id: "history-up-edit", keys: b"\x1b[A!\r", expect: Expect::Line("one!") },
    Step { id: "history-up-down-restores", keys: b"draft\x1b[A\x1b[B\r", expect: Expect::Line("draft") },
    Step { id: "ctrl-c-cancels", keys: b"partial\x03", expect: Expect::Cancelled },
    Step { id: "after-ctrl-c", keys: b"after\r", expect: Expect::Line("after") },
];

// ---------------------------------------------------------------------------------------------
// Pty em memória e o mini VT do lado de fora
// ---------------------------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum VtState {
    Ground,
    Esc,
    Csi,
}

/// O suficiente de um VT100 pra acompanhar o cursor e responder `ESC [ 6 n` (DSR/CPR).
#[derive(Debug)]
pub struct Vt {
    rows: u16,
    cols: u16,
    row: u16,
    col: u16,
    saved: (u16, u16),
    state: VtState,
    params: Vec<u8>,
}

impl Vt {
    pub fn new(rows: u16, cols: u16) -> Vt {
        Vt { rows, cols, row: 1, col: 1, saved: (1, 1), state: VtState::Ground, params: Vec::new() }
    }

    fn csi(&mut self, final_byte: u8) -> Option<Vec<u8>> {
        let text = String::from_utf8_lossy(&self.params).into_owned();
        let nums: Vec<u16> = text.split(';').map(|p| p.trim_start_matches('?').parse().unwrap_or(0)).collect();
        let n = |i: usize| nums.get(i).copied().filter(|v| *v > 0).unwrap_or(1);
        match final_byte {
            b'H' | b'f' => {
                self.row = n(0).min(self.rows);
                self.col = n(1).min(self.cols);
            }
            b'A' => self.row = self.row.saturating_sub(n(0)).max(1),
            b'B' => self.row = (self.row + n(0)).min(self.rows),
            b'C' => self.col = (self.col + n(0)).min(self.cols),
            b'D' => self.col = self.col.saturating_sub(n(0)).max(1),
            b'G' => self.col = n(0).min(self.cols),
            b'n' if text == "6" => return Some(format!("\x1b[{};{}R", self.row, self.col).into_bytes()),
            _ => {}
        }
        None
    }

    /// Consome bytes de saída; devolve as respostas que o terminal mandaria de volta.
    pub fn feed(&mut self, bytes: &[u8]) -> Vec<Vec<u8>> {
        let mut answers = Vec::new();
        for &b in bytes {
            match self.state {
                VtState::Ground => match b {
                    0x1b => self.state = VtState::Esc,
                    b'\r' => self.col = 1,
                    b'\n' => self.row = (self.row + 1).min(self.rows),
                    0x08 => self.col = self.col.saturating_sub(1).max(1),
                    0x07 => {}
                    // Byte de continuação UTF-8 não anda o cursor.
                    b if (0x80..0xc0).contains(&b) => {}
                    b if b >= 0x20 => self.col = (self.col + 1).min(self.cols),
                    _ => {}
                },
                VtState::Esc => match b {
                    b'[' => {
                        self.params.clear();
                        self.state = VtState::Csi;
                    }
                    b'7' => {
                        self.saved = (self.row, self.col);
                        self.state = VtState::Ground;
                    }
                    b'8' => {
                        (self.row, self.col) = self.saved;
                        self.state = VtState::Ground;
                    }
                    _ => self.state = VtState::Ground,
                },
                VtState::Csi => {
                    if (0x40..=0x7e).contains(&b) {
                        if let Some(a) = self.csi(b) {
                            answers.push(a);
                        }
                        self.state = VtState::Ground;
                    } else {
                        self.params.push(b);
                    }
                }
            }
        }
        answers
    }
}

/// Os dois lados do pty: o que foi "digitado" (entrada do editor) e o que o editor escreveu.
#[derive(Debug)]
pub struct MemPty {
    pub input: VecDeque<u8>,
    pub output: Vec<u8>,
    pub vt: Vt,
    pub cpr_answers: usize,
    pub raw: bool,
    pub mode_switches: usize,
}

impl MemPty {
    pub fn new(keys: &[u8]) -> MemPty {
        MemPty {
            input: keys.iter().copied().collect(),
            output: Vec::new(),
            vt: Vt::new(ROWS, COLS),
            cpr_answers: 0,
            raw: false,
            mode_switches: 0,
        }
    }

    /// Escrita do editor: vai pro buffer de saída e passa pelo VT; respostas do VT entram na frente
    /// da fila de entrada (chegam antes do que o usuário ainda vai digitar).
    pub fn write_out(&mut self, bytes: &[u8]) {
        self.output.extend_from_slice(bytes);
        for answer in self.vt.feed(bytes) {
            self.cpr_answers += 1;
            for b in answer.into_iter().rev() {
                self.input.push_front(b);
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------
// termwiz: Terminal nosso
// ---------------------------------------------------------------------------------------------

struct Sink<'a>(&'a mut MemPty);

impl std::io::Write for Sink<'_> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.write_out(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl RenderTty for Sink<'_> {
    fn get_size_in_cells(&mut self) -> termwiz::Result<(usize, usize)> {
        Ok((COLS as usize, ROWS as usize))
    }
}

/// `Terminal` do termwiz sobre o pty em memória.
pub struct PtyTerminal {
    pub pty: MemPty,
    renderer: TerminfoRenderer,
    parser: InputParser,
    events: VecDeque<InputEvent>,
    size: ScreenSize,
}

pub fn capabilities() -> Result<Capabilities> {
    let db = terminfo::Database::from_buffer(XTERM_TERMINFO).map_err(|e| anyhow::anyhow!("terminfo: {e:?}"))?;
    // Com `terminfo_db` preenchido o termwiz não procura terminfo no FS do host.
    let hints = ProbeHints::default()
        .term(Some("xterm-256color".to_string()))
        .terminfo_db(Some(db))
        .mouse_reporting(Some(false));
    Capabilities::new_with_hints(hints).map_err(|e| anyhow::anyhow!("caps: {e}"))
}

impl PtyTerminal {
    pub fn new(keys: &[u8]) -> Result<PtyTerminal> {
        Ok(PtyTerminal {
            pty: MemPty::new(keys),
            renderer: TerminfoRenderer::new(capabilities()?),
            parser: InputParser::new(),
            events: VecDeque::new(),
            size: ScreenSize { rows: ROWS as usize, cols: COLS as usize, xpixel: 0, ypixel: 0 },
        })
    }
}

impl Terminal for PtyTerminal {
    fn set_raw_mode(&mut self) -> termwiz::Result<()> {
        self.pty.raw = true;
        self.pty.mode_switches += 1;
        Ok(())
    }

    fn set_cooked_mode(&mut self) -> termwiz::Result<()> {
        self.pty.raw = false;
        self.pty.mode_switches += 1;
        Ok(())
    }

    fn enter_alternate_screen(&mut self) -> termwiz::Result<()> {
        Ok(())
    }

    fn exit_alternate_screen(&mut self) -> termwiz::Result<()> {
        Ok(())
    }

    fn get_screen_size(&mut self) -> termwiz::Result<ScreenSize> {
        Ok(self.size)
    }

    fn set_screen_size(&mut self, size: ScreenSize) -> termwiz::Result<()> {
        self.size = size;
        Ok(())
    }

    fn render(&mut self, changes: &[Change]) -> termwiz::Result<()> {
        self.renderer.render_to(changes, &mut Sink(&mut self.pty))
    }

    fn flush(&mut self) -> termwiz::Result<()> {
        Ok(())
    }

    fn poll_input(&mut self, _wait: Option<std::time::Duration>) -> termwiz::Result<Option<InputEvent>> {
        loop {
            if let Some(e) = self.events.pop_front() {
                return Ok(Some(e));
            }
            if self.pty.input.is_empty() {
                // Num pty de verdade o leitor dormiria aqui; no teste, acabar a entrada é EOF.
                return Err(std::io::Error::new(std::io::ErrorKind::UnexpectedEof, "fim da entrada do pty").into());
            }
            let bytes: Vec<u8> = self.pty.input.drain(..).collect();
            let events = &mut self.events;
            self.parser.parse(&bytes, |e| events.push_back(e), false);
        }
    }

    fn waker(&self) -> TerminalWaker {
        // A trait exige devolver o `UnixTerminalWaker` concreto do termwiz (campo privado sobre um
        // UnixStream do host), que não dá pra construir de fora. O `LineEditor` nunca chama `waker()`
        // (conferido no código, ver evidência no JSON); o pty nosso acorda o leitor por condvar própria.
        panic!("PtyTerminal::waker: TerminalWaker não é construível fora do termwiz")
    }
}

/// Host do editor: histórico e, opcionalmente, as teclas do readline que o termwiz não liga por padrão.
struct ShellHost {
    history: BasicHistory,
    bash_bindings: bool,
}

fn is_ctrl(event: &InputEvent, c: char) -> bool {
    matches!(event, InputEvent::Key(KeyEvent { key: KeyCode::Char(k), modifiers })
        if modifiers.contains(Modifiers::CTRL) && k.eq_ignore_ascii_case(&c))
}

impl LineEditorHost for ShellHost {
    fn history(&mut self) -> &mut dyn History {
        &mut self.history
    }

    fn resolve_action(&mut self, event: &InputEvent, editor: &mut LineEditor) -> Option<Action> {
        if !self.bash_bindings {
            return None;
        }
        // O InputParser do termwiz emite Ctrl+letra em minúscula e o LineEditor só reconhece maiúscula
        // (evidência no JSON): sem esta tabela, Ctrl-A/E/K/C/B/F/P/N não fazem nada.
        let simple = [
            ('A', Action::Move(Movement::StartOfLine)),
            ('E', Action::Move(Movement::EndOfLine)),
            ('B', Action::Move(Movement::BackwardChar(1))),
            ('F', Action::Move(Movement::ForwardChar(1))),
            ('K', Action::Kill(Movement::EndOfLine)),
            ('H', Action::Kill(Movement::BackwardChar(1))),
            ('C', Action::Cancel),
            ('P', Action::HistoryPrevious),
            ('N', Action::HistoryNext),
            ('J', Action::AcceptLine),
            ('M', Action::AcceptLine),
            ('L', Action::Repaint),
            ('R', Action::HistoryIncSearchBackwards),
        ];
        for (c, action) in simple {
            if is_ctrl(event, c) {
                return Some(action);
            }
        }
        if is_ctrl(event, 'D') {
            // readline: Ctrl-D em linha vazia é EOF; com texto, apaga o caractere sob o cursor.
            let (line, _) = editor.get_line_and_cursor();
            return Some(if line.is_empty() {
                Action::EndOfFile
            } else {
                Action::KillAndMove(Movement::ForwardChar(1), Movement::None)
            });
        }
        if is_ctrl(event, 'U') {
            // unix-line-discard: do cursor até o começo.
            return Some(Action::Kill(Movement::StartOfLine));
        }
        if is_ctrl(event, 'W') {
            // unix-word-rubout: apaga pra trás até o espaço anterior (o termwiz usa fronteira de palavra).
            let (line, cursor) = editor.get_line_and_cursor();
            let before = &line[..cursor];
            let trimmed = before.trim_end_matches(' ');
            let start = trimmed.rfind(' ').map(|i| i + 1).unwrap_or(0);
            let new_line = format!("{}{}", &line[..start], &line[cursor..]);
            editor.set_line_and_cursor(&new_line, start);
            return Some(Action::Move(Movement::None));
        }
        None
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct StepResult {
    pub id: String,
    pub expected: String,
    pub got: String,
    pub ok: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct DriveReport {
    pub editor: String,
    pub steps: Vec<StepResult>,
    pub passed: usize,
    pub total: usize,
    pub output_bytes: usize,
    /// Quantas vezes o prompt apareceu no buffer de saída.
    pub prompts_rendered: usize,
    /// O render tem sequências CSI (o editor de fato desenhou no nosso buffer).
    pub has_escape_sequences: bool,
    /// As linhas aceitas aparecem no render.
    pub accepted_lines_in_output: usize,
    pub cpr_answers: usize,
    pub raw_mode_switches: usize,
    pub error: Option<String>,
}

fn expect_text(e: Expect) -> String {
    match e {
        Expect::Line(l) => l.to_string(),
        Expect::Cancelled => "<cancelado>".into(),
    }
}

fn finish(editor: &str, steps: Vec<StepResult>, pty: &MemPty, error: Option<String>) -> DriveReport {
    let out = String::from_utf8_lossy(&pty.output);
    let accepted_lines_in_output =
        steps.iter().filter(|s| s.ok && s.got != "<cancelado>" && !s.got.is_empty() && out.contains(s.got.as_str())).count();
    DriveReport {
        editor: editor.into(),
        passed: steps.iter().filter(|s| s.ok).count(),
        total: STEPS.len(),
        output_bytes: pty.output.len(),
        prompts_rendered: out.matches(PROMPT).count(),
        has_escape_sequences: out.contains("\x1b["),
        accepted_lines_in_output,
        cpr_answers: pty.cpr_answers,
        raw_mode_switches: pty.mode_switches,
        steps,
        error,
    }
}

fn all_keys() -> Vec<u8> {
    STEPS.iter().flat_map(|s| s.keys.iter().copied()).collect()
}

/// Dirige o `LineEditor` do termwiz pelo roteiro inteiro, num terminal só (histórico contínuo).
pub fn drive_termwiz(bash_bindings: bool) -> Result<DriveReport> {
    let mut term = PtyTerminal::new(&all_keys())?;
    let mut host = ShellHost { history: BasicHistory::default(), bash_bindings };
    let mut results = Vec::new();
    let mut error = None;
    {
        let mut editor = LineEditor::new(&mut term);
        editor.set_prompt(PROMPT);
        for step in STEPS {
            let got = match editor.read_line(&mut host) {
                Ok(Some(line)) => {
                    host.history.add(&line);
                    line
                }
                Ok(None) => "<cancelado>".to_string(),
                Err(e) => {
                    error = Some(format!("{}: {e}", step.id));
                    break;
                }
            };
            let expected = expect_text(step.expect);
            results.push(StepResult { id: step.id.into(), ok: got == expected, expected, got });
        }
    }
    let name = if bash_bindings { "termwiz 0.23.3 + teclas do readline no host" } else { "termwiz 0.23.3 (teclas padrão)" };
    Ok(finish(name, results, &term.pty, error))
}

// ---------------------------------------------------------------------------------------------
// noline: embedded_io nosso
// ---------------------------------------------------------------------------------------------

struct NolineIo {
    pty: MemPty,
}

impl embedded_io::ErrorType for NolineIo {
    type Error = std::convert::Infallible;
}

impl embedded_io::Read for NolineIo {
    fn read(&mut self, buf: &mut [u8]) -> std::result::Result<usize, Self::Error> {
        let mut n = 0;
        while n < buf.len() {
            match self.pty.input.pop_front() {
                Some(b) => {
                    buf[n] = b;
                    n += 1;
                }
                None => break,
            }
        }
        Ok(n)
    }
}

impl embedded_io::Write for NolineIo {
    fn write(&mut self, buf: &[u8]) -> std::result::Result<usize, Self::Error> {
        self.pty.write_out(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::result::Result<(), Self::Error> {
        Ok(())
    }
}

pub fn drive_noline() -> Result<DriveReport> {
    let mut io = NolineIo { pty: MemPty::new(&all_keys()) };
    let mut results = Vec::new();
    let mut error = None;
    match noline::builder::EditorBuilder::new_unbounded().with_unbounded_history().build_sync(&mut io) {
        Err(e) => error = Some(format!("build_sync: {e:?}")),
        Ok(mut editor) => {
            for step in STEPS {
                let before = io.pty.input.len();
                let got = match editor.readline(PROMPT, &mut io) {
                    Ok(line) => line.to_string(),
                    // Ctrl-C vira Aborted com entrada sobrando; Aborted sem entrada é EOF do teste.
                    Err(noline::error::NolineError::Aborted) if before > 0 && !io.pty.input.is_empty() => {
                        "<cancelado>".into()
                    }
                    Err(noline::error::NolineError::Aborted) if step.expect == Expect::Cancelled => "<cancelado>".into(),
                    Err(e) => {
                        error = Some(format!("{}: {e:?}", step.id));
                        break;
                    }
                };
                let expected = expect_text(step.expect);
                results.push(StepResult { id: step.id.into(), ok: got == expected, expected, got });
            }
        }
    }
    Ok(finish("noline 0.5.1", results, &io.pty, error))
}

// ---------------------------------------------------------------------------------------------
// Processo filho sob strace
// ---------------------------------------------------------------------------------------------

/// Modo filho: roda os três roteiros entre os marcadores e imprime o JSON no stdout.
pub fn child_main() -> Result<()> {
    let _ = std::fs::metadata(MARK_BEGIN);
    let reports = vec![drive_termwiz(false)?, drive_termwiz(true)?, drive_noline()?];
    let _ = std::fs::metadata(MARK_END);
    let mut out = std::io::stdout().lock();
    serde_json::to_writer(&mut out, &reports)?;
    out.write_all(b"\n")?;
    Ok(())
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct TraceSummary {
    pub syscalls_between_markers: usize,
    pub tty_opens: Vec<String>,
    pub termios_ioctls: Vec<String>,
    pub file_opens: Vec<String>,
    pub stdin_reads: usize,
    pub markers_found: bool,
}

/// Lê um trace do strace e resume o que aconteceu entre os marcadores.
pub fn summarize_trace(text: &str) -> TraceSummary {
    let mut s = TraceSummary::default();
    let mut inside = false;
    let mut seen_end = false;
    for line in text.lines() {
        if line.contains(MARK_BEGIN) {
            inside = true;
            continue;
        }
        if line.contains(MARK_END) {
            inside = false;
            seen_end = true;
            continue;
        }
        if !inside {
            continue;
        }
        // Sonda de disponibilidade do statx que o std faz no primeiro `metadata` (vem do marcador).
        if line.contains("statx(0, NULL") {
            continue;
        }
        s.syscalls_between_markers += 1;
        let short: String = line.chars().take(160).collect();
        if line.contains("/dev/tty") || line.contains("/dev/pts") || line.contains("/dev/ptmx") {
            s.tty_opens.push(short.clone());
        }
        if line.contains("ioctl(")
            && ["TCGETS", "TCSETS", "TIOCGWINSZ", "TIOCSWINSZ", "TCSETSW", "TCSETSF", "SNDCTL_TMR"].iter().any(|k| line.contains(k))
        {
            s.termios_ioctls.push(short.clone());
        }
        if line.contains("open(") || line.contains("openat(") {
            s.file_opens.push(short.clone());
        }
        if line.contains("read(0,") {
            s.stdin_reads += 1;
        }
    }
    s.markers_found = seen_end;
    s
}

/// Roda `cmd args` com `setsid` (sem terminal de controle), stdin dado, sob strace. Devolve stdout,
/// stderr e o resumo do trace.
fn traced(name: &str, program: &Path, args: &[&str], stdin: &[u8]) -> Result<(String, String, TraceSummary)> {
    let trace = common::cache_dir().join(format!("strace-{name}.txt"));
    let _ = std::fs::remove_file(&trace);
    let mut child = Command::new("setsid")
        .arg("-w")
        .arg("strace")
        .args(["-f", "-qq", "-e", "trace=%file,%desc", "-o"])
        .arg(&trace)
        .arg(program)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .with_context(|| format!("setsid strace {}", program.display()))?;
    {
        let mut i = child.stdin.take().expect("stdin");
        let _ = i.write_all(stdin);
    }
    let out = child.wait_with_output()?;
    let text = std::fs::read_to_string(&trace).unwrap_or_default();
    Ok((
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
        summarize_trace(&text),
    ))
}

// ---------------------------------------------------------------------------------------------
// Evidência de código dos negativos e do termwiz
// ---------------------------------------------------------------------------------------------

fn negative_evidence() -> Result<(Vec<CodeEvidence>, Vec<CodeEvidence>)> {
    let reedline = vec![
        evidence::find("reedline", "src/engine.rs", "terminal::enable_raw_mode()?;", "read_line liga o modo raw do terminal do processo (crossterm) antes de ler")?,
        evidence::find("reedline", "src/engine.rs", "crossterm::event::read()", "a entrada vem de crossterm::event::read(), que lê o tty do host; não há leitor injetável")?,
        evidence::find("reedline", "src/painting/painter.rs", "W::Terminal(std::io::BufWriter::new(std::io::stderr()))", "a saída do painter é o stderr do host")?,
        evidence::find("reedline", "src/painting/painter.rs", "Capture(Vec<u8>),", "o writer em memória existe, mas só com #[cfg(test)] e pub(crate)")?,
        evidence::find("reedline", "src/engine.rs", "let painter = Painter::new(W::terminal());", "Reedline::create() fixa o writer do terminal; nenhum construtor recebe I/O")?,
        evidence::find("crossterm", "src/terminal/sys/file_descriptor.rs", ".open(\"/dev/tty\")", "crossterm (usado pelo reedline) abre /dev/tty quando o stdin não é tty")?,
    ];
    let rustyline = vec![
        evidence::find("rustyline", "src/lib.rs", "mod tty;", "o módulo de terminal é privado: a trait Term não é exportada")?,
        evidence::find("rustyline", "src/lib.rs", "let term = Terminal::new(&config)?;", "Editor::with_history cria o terminal concreto da plataforma, sem parâmetro de I/O")?,
        evidence::find("rustyline", "src/tty/mod.rs", "pub use self::unix::*;", "fora de teste, o Terminal é o PosixTerminal (o dummy só existe com cfg(test) ou wasm)")?,
        evidence::find("rustyline", "src/tty/unix.rs", "OpenOptions::new().read(true).write(true).open(\"/dev/tty\")", "com Behavior::PreferTerm abre /dev/tty do host")?,
        evidence::find("rustyline", "src/tty/unix.rs", "AltFd(libc::STDIN_FILENO), AltFd(libc::STDOUT_FILENO)", "no modo padrão usa os fds 0 e 1 do processo host")?,
    ];
    Ok((reedline, rustyline))
}

fn termwiz_evidence() -> Result<Vec<CodeEvidence>> {
    Ok(vec![
        evidence::find("termwiz", "src/lineedit/mod.rs", "pub fn new(terminal: &'term mut dyn Terminal) -> Self", "LineEditor aceita qualquer implementação da trait Terminal")?,
        evidence::find("termwiz", "src/terminal/mod.rs", "fn waker(&self) -> TerminalWaker;", "a trait exige devolver um TerminalWaker concreto")?,
        evidence::find("termwiz", "src/terminal/unix.rs", "pipe: Arc<Mutex<UnixStream>>,", "o TerminalWaker é um UnixStream do host com campo privado: não dá pra construir fora do termwiz")?,
        evidence::find("termwiz", "src/render/terminfo.rs", "unimplemented!();", "o próprio teste do termwiz implementa waker() com unimplemented!()")?,
        evidence::find("termwiz", "src/caps/mod.rs", "None => terminfo::Database::from_env().ok(),", "sem terminfo_db nas dicas, Capabilities lê o terminfo do FS do host; com ele, não")?,
        evidence::find("termwiz", "src/input.rs", "key: KeyCode::Char((alpha as char).to_ascii_lowercase()),", "o InputParser emite Ctrl+letra (bytes 0x01 a 0x1a) como letra minúscula com CTRL")?,
        evidence::find("termwiz", "src/lineedit/mod.rs", "key: KeyCode::Char('C'),", "o LineEditor compara Ctrl+letra com maiúscula: Ctrl-A/E/K/C do parser do próprio termwiz nunca casam")?,
        evidence::find("termwiz", "src/lineedit/mod.rs", "key: KeyCode::Char('W'),", "Ctrl-W ligado a BackwardWord (fronteira de palavra, não espaço como o unix-word-rubout); Ctrl-U não tem ligação")?,
    ])
}

fn noline_evidence() -> Result<Vec<CodeEvidence>> {
    Ok(vec![
        evidence::find("noline", "src/lib.rs", "#![cfg_attr(not(test), no_std)]", "no_std: não tem como tocar o host")?,
        evidence::find("noline", "src/core.rs", "dbg!(x, y);", "os pontos de host que o depscan conta são dbg!/println! com #[cfg(test)] na instrução de cima (o depscan só descarta módulo e função de teste)")?,
        evidence::find("noline", "src/builder.rs", "pub fn build_sync<IO: embedded_io::Read + embedded_io::Write>(", "o editor recebe qualquer Read + Write")?,
        evidence::find("noline", "src/core.rs", "self.buffer.delete_after_char(0);", "Ctrl-U apaga a linha inteira (o readline apaga só do cursor pra trás)")?,
        evidence::find("noline", "src/core.rs", "ResetState::Done => panic!(\"Invalid state\"),", "uma resposta CPR fora de hora derruba o editor com panic")?,
        evidence::find("noline", "src/input.rs", "'H' => Self::CUP(", "ESC [ H (Home do xterm) vira CUP e toca o sino; só ESC [ 1 ~ é Home, e ESC [ F (End do xterm) é desconhecido")?,
        evidence::find("noline", "src/input.rs", "'R' => Self::CPR(arg1.unwrap(), arg2.unwrap()),", "ESC [ R sem argumentos na entrada (colado ou digitado) faz unwrap em None: panic")?,
    ])
}

fn scan_summary(name: &str) -> Result<(serde_json::Value, String)> {
    let s = depscan::scan(&common::manifest(), name)?;
    let v = json!({
        "own_category": s.root.category.letter(),
        "tree_category": s.tree_category.letter(),
        "own_host_points": s.root.counts.host_touch(),
        "tree_host_points": s.totals.host_touch(),
        "tree_deps": s.deps.len(),
        "tree_unsafe": s.totals.unsafe_total(),
        "c_deps": s.c_deps,
        "host_touching_deps": s.host_touching_deps,
    });
    Ok((v, s.root.version))
}

/// Pontos de host por arquivo do termwiz (pra mostrar que o caminho do LineEditor não passa por eles).
fn termwiz_host_files() -> Result<Vec<(String, usize)>> {
    let root = evidence::crate_root("termwiz")?;
    let mut out = Vec::new();
    let mut stack = vec![root.join("src")];
    while let Some(d) = stack.pop() {
        for item in std::fs::read_dir(&d)? {
            let p = item?.path();
            if p.is_dir() {
                stack.push(p);
            } else if p.extension().is_some_and(|x| x == "rs") {
                let mut c = depscan::Counts::default();
                depscan::scan_source(&std::fs::read_to_string(&p)?, &mut c);
                if c.host_touch() > 0 {
                    out.push((evidence::rel(&root, &p), c.host_touch()));
                }
            }
        }
    }
    out.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    Ok(out)
}

pub fn run() -> Result<Section> {
    let mut notes = Vec::new();
    // A sonda do reedline/rustyline é um binário separado.
    if let Err(e) = common::build_packages(&["f15-tty-probe"])? {
        anyhow::bail!("build da sonda de tty falhou:\n{e}");
    }
    let probe = common::bin_path("f15-tty-probe");
    let typed = b"echo hi\r\n";
    let mut probes = Vec::new();
    for mode in ["reedline", "rustyline", "rustyline-preferterm"] {
        let (_out, err, trace) = traced(mode, &probe, &[mode], typed)?;
        let report: serde_json::Value = err
            .lines()
            .rev()
            .find_map(|l| serde_json::from_str(l).ok())
            .unwrap_or_else(|| json!({ "outcome": "sem relatório", "stderr": err.chars().take(300).collect::<String>() }));
        probes.push(json!({ "mode": mode, "report": report, "trace": trace }));
    }

    // Os editores candidatos: no próprio processo (pra relatório) e num filho sob strace.
    let in_process = vec![drive_termwiz(false)?, drive_termwiz(true)?, drive_noline()?];
    // Se o binário foi recompilado durante a execução, o /proc/self/exe aponta pra "(deleted)": usa o
    // caminho do arquivo novo.
    let me = std::env::current_exe()?;
    let me = match me.to_string_lossy().strip_suffix(" (deleted)") {
        Some(p) => std::path::PathBuf::from(p),
        None => me,
    };
    let (child_out, child_err, child_trace) = traced("lineedit", &me, &["line-edit-child"], b"")?;
    let child_reports: Option<Vec<serde_json::Value>> = serde_json::from_str(child_out.trim()).ok();
    if child_reports.is_none() {
        notes.push(format!("filho do line-edit não devolveu JSON: {}", child_err.chars().take(300).collect::<String>()));
    }
    let child_same = child_reports
        .as_ref()
        .map(|r| r.iter().zip(&in_process).all(|(c, p)| c["passed"] == json!(p.passed)))
        .unwrap_or(false);
    let no_tty = child_trace.markers_found && child_trace.tty_opens.is_empty() && child_trace.termios_ioctls.is_empty();

    let (reedline_ev, rustyline_ev) = negative_evidence()?;
    let termwiz_ev = termwiz_evidence()?;
    let noline_ev = noline_evidence()?;
    let (reedline_scan, reedline_v) = scan_summary("reedline")?;
    let (rustyline_scan, rustyline_v) = scan_summary("rustyline")?;
    let (termwiz_scan, termwiz_v) = scan_summary("termwiz")?;
    let (noline_scan, noline_v) = scan_summary("noline")?;
    let termwiz_files = termwiz_host_files()?;

    let probe_touched_tty = |mode: &str| -> bool {
        probes.iter().any(|p| {
            p["mode"] == mode
                && (p["trace"]["tty_opens"].as_array().is_some_and(|a| !a.is_empty())
                    || p["trace"]["termios_ioctls"].as_array().is_some_and(|a| !a.is_empty())
                    || p["trace"]["stdin_reads"].as_u64().is_some_and(|n| n > 0))
        })
    };
    let reedline_host = probe_touched_tty("reedline");
    let rustyline_host = probe_touched_tty("rustyline") && probe_touched_tty("rustyline-preferterm");

    let tw_default = &in_process[0];
    let tw_bash = &in_process[1];
    let nl = &in_process[2];
    let failed = |r: &DriveReport| -> Vec<String> {
        r.steps.iter().filter(|s| !s.ok).map(|s| format!("{} (esperado {:?}, obtido {:?})", s.id, s.expected, s.got)).collect()
    };

    let mut candidates = vec![
        CandidateResult {
            name: "reedline".into(),
            version: reedline_v,
            role: "line-edit".into(),
            category: reedline_scan["own_category"].as_str().map(String::from),
            conformance: None,
            fit: Fit::DoesNotFit,
            notes: format!(
                "Preso ao tty do host: entrada por crossterm::event::read, modo raw no tty do processo, saída no \
                 stderr do host; o writer em memória só existe em cfg(test). Sonda sem terminal de controle: {}.",
                probes[0]["report"]["detail"].as_str().unwrap_or("?")
            ),
            metrics: json!({ "evidence": reedline_ev, "depscan": reedline_scan, "probe": probes[0] }),
        },
        CandidateResult {
            name: "rustyline".into(),
            version: rustyline_v,
            role: "line-edit".into(),
            category: rustyline_scan["own_category"].as_str().map(String::from),
            conformance: None,
            fit: Fit::DoesNotFit,
            notes: "Preso ao tty do host: módulo tty privado, Editor cria o PosixTerminal sobre os fds 0/1 do \
                    processo (ou /dev/tty com PreferTerm); sem trait pública pra I/O próprio."
                .into(),
            metrics: json!({ "evidence": rustyline_ev, "depscan": rustyline_scan, "probes": [probes[1].clone(), probes[2].clone()] }),
        },
        CandidateResult {
            name: "termwiz".into(),
            version: termwiz_v,
            role: "line-edit".into(),
            category: termwiz_scan["own_category"].as_str().map(String::from),
            conformance: None,
            fit: if tw_bash.passed == tw_bash.total { Fit::FitsWithWork } else { Fit::DoesNotFit },
            notes: format!(
                "LineEditor sobre Terminal nosso ligado ao pty em memória: {}/{} passos com as teclas padrão, \
                 {}/{} com a tabela de teclas do readline no LineEditorHost; render no buffer de saída \
                 ({} bytes, {} prompts). Com as teclas padrão, Ctrl-A/E/K/C não funcionam porque o InputParser \
                 do termwiz emite Ctrl+letra minúscula e o LineEditor compara com maiúscula (bug do próprio \
                 termwiz, vale também com o UnixTerminal dele); o host corrige sem fork, e também dá o Ctrl-U e o \
                 Ctrl-W do readline. Ressalvas: waker() da trait devolve um tipo concreto do host que não dá \
                 pra construir (o LineEditor não chama); Capabilities precisa de terminfo embutido senão lê o FS \
                 do host. A crate inteira toca o host em {} arquivos (UnixTerminal, caps), fora do caminho do \
                 LineEditor. Falhas com teclas padrão: {}.",
                tw_default.passed,
                tw_default.total,
                tw_bash.passed,
                tw_bash.total,
                tw_bash.output_bytes,
                tw_bash.prompts_rendered,
                termwiz_files.len(),
                failed(tw_default).join("; "),
            ),
            metrics: json!({
                "default_bindings": tw_default,
                "readline_bindings": tw_bash,
                "evidence": termwiz_ev,
                "depscan": termwiz_scan,
                "host_touching_files": termwiz_files,
            }),
        },
        CandidateResult {
            name: "noline".into(),
            version: noline_v,
            role: "line-edit".into(),
            category: noline_scan["own_category"].as_str().map(String::from),
            conformance: None,
            fit: if nl.passed == nl.total { Fit::Fits } else { Fit::FitsWithWork },
            notes: format!(
                "no_std sobre embedded_io::Read/Write nossos: {}/{} passos; precisa que o terminal responda CPR \
                 (respondido pelo VT do pty, {} respostas). Falhas: {}. Sem ganchos de tecla: corrigir exige fork. \
                 Os pontos de host que o depscan conta são dbg!/println! sob #[cfg(test)] (código de produção sem \
                 nenhum). Robustez: CPR fora de hora e ESC [ R sem argumento dão panic.",
                nl.passed,
                nl.total,
                nl.cpr_answers,
                failed(nl).join("; "),
            ),
            metrics: json!({ "drive": nl, "evidence": noline_ev, "depscan": noline_scan }),
        },
    ];
    candidates.iter_mut().for_each(|c| {
        if let Some(obj) = c.metrics.as_object_mut() {
            obj.insert("child_strace_no_tty".into(), json!(no_tty));
        }
    });

    let verdict = if !reedline_host && !rustyline_host { Verdict::Inconclusive } else { Verdict::Refuted };
    let summary = format!(
        "Refutada: reedline e rustyline falam direto com o tty do host (sem terminal de controle, a sonda do \
         reedline {}; a do rustyline {}), e nenhum aceita I/O próprio. O LineEditor do termwiz sobre um Terminal \
         nosso passa {}/{} passos do roteiro com teclas padrão e {}/{} com Ctrl-U/Ctrl-W do readline no host; o \
         noline passa {}/{}. No filho com setsid e strace ({} syscalls de arquivo e descritor entre os \
         marcadores{}): {} aberturas de tty e {} ioctls de termios. Recomendação: termwiz LineEditor com host \
         nosso.",
        if reedline_host { "tocou o tty do host" } else { "não tocou" },
        if rustyline_host { "leu fd 0 ou abriu /dev/tty" } else { "não tocou" },
        tw_default.passed,
        tw_default.total,
        tw_bash.passed,
        tw_bash.total,
        nl.passed,
        nl.total,
        child_trace.syscalls_between_markers,
        if child_trace.markers_found { "" } else { "; ATENÇÃO: marcadores não encontrados, trace inválido" },
        child_trace.tty_opens.len(),
        child_trace.termios_ioctls.len(),
    );
    let evidence = json!({
        "reedline_probe_touched_host_tty": reedline_host,
        "rustyline_probe_touched_host_tty": rustyline_host,
        "termwiz_default": format!("{}/{}", tw_default.passed, tw_default.total),
        "termwiz_readline_bindings": format!("{}/{}", tw_bash.passed, tw_bash.total),
        "noline": format!("{}/{}", nl.passed, nl.total),
        "child_trace": child_trace,
        "child_matches_in_process": child_same,
        "no_tty_in_child": no_tty,
        "steps": STEPS.iter().map(|s| json!({"id": s.id, "keys": String::from_utf8_lossy(s.keys), "expect": expect_text(s.expect)})).collect::<Vec<_>>(),
    });
    Ok(Section { hypothesis: "H38", verdict, summary, evidence, candidates, notes })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vt_answers_cpr_with_tracked_cursor() {
        let mut vt = Vt::new(24, 80);
        assert_eq!(vt.feed(b"\x1b7\x1b[999;999H\x1b[6n\x1b8"), vec![b"\x1b[24;80R".to_vec()]);
        assert_eq!(vt.feed(b"\r\x1b[J$ \x1b[6n"), vec![b"\x1b[1;3R".to_vec()]);
        assert_eq!(vt.feed("ação\x1b[6n".as_bytes()), vec![b"\x1b[1;7R".to_vec()]);
    }

    #[test]
    fn termwiz_runs_the_script_over_the_memory_pty() {
        let r = drive_termwiz(true).unwrap();
        assert!(r.error.is_none(), "{:?}", r.error);
        assert_eq!(r.steps.len(), STEPS.len());
        assert!(r.has_escape_sequences && r.prompts_rendered >= STEPS.len());
        assert!(r.steps.iter().find(|s| s.id == "text-enter").unwrap().ok);
        assert!(r.steps.iter().find(|s| s.id == "ctrl-c-cancels").unwrap().ok);
    }

    #[test]
    fn noline_runs_the_script_over_the_memory_pty() {
        let r = drive_noline().unwrap();
        assert!(r.error.is_none(), "{:?}", r.error);
        assert!(r.cpr_answers >= 2 * STEPS.len(), "noline consulta CPR a cada linha");
        assert!(r.steps.iter().find(|s| s.id == "text-enter").unwrap().ok);
    }

    #[test]
    fn trace_summary_only_counts_between_markers() {
        let t = "1 openat(AT_FDCWD, \"/dev/tty\", O_RDWR) = -1 ENXIO\n\
                 1 newfstatat(AT_FDCWD, \"/f15-strace-marker-begin\", 0x0) = -1 ENOENT\n\
                 1 ioctl(0, TCGETS, 0x7ff) = -1 ENOTTY\n\
                 1 read(0, \"x\", 1) = 1\n\
                 1 newfstatat(AT_FDCWD, \"/f15-strace-marker-end\", 0x0) = -1 ENOENT\n";
        let s = summarize_trace(t);
        assert!(s.markers_found);
        assert!(s.tty_opens.is_empty());
        assert_eq!(s.termios_ioctls.len(), 1);
        assert_eq!(s.stdin_reads, 1);
    }
}
