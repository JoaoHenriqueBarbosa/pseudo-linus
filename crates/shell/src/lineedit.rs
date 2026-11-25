//! O editor de linha do bash interativo: o papel do readline no modo emacs padrão do bash 5.2.
//!
//! Com o stdin num terminal, cada linha do prompt é lida com o tty sem `ICANON` e sem `ECHO`, como no
//! `rl_prep_terminal`: o editor ecoa, move o cursor, consulta o histórico e completa com Tab, e o
//! modo do tty guardado volta antes de o comando rodar. O `ISIG` fica ligado, então Ctrl-C ainda gera
//! SIGINT, que o shell trata (`Shell::sigint`). Como no bash com `enable-bracketed-paste` ligado, o
//! editor pede a colagem entre colchetes (`ESC[?2004h`) enquanto lê e a desliga ao terminar.
//!
//! O redesenho reescreve o prompt e a linha a partir do começo do prompt; o texto que aparece na tela
//! é o mesmo do readline, os bytes de controle que levam até ele não são.

use std::collections::BTreeSet;

use sysabi::termios::{ECHO, ECHONL, ICANON, ICRNL, IEXTEN, INLCR, VMIN, VTIME};
use sysabi::{Errno, Fd, FileType, SetAttrWhen};
use ul_common::width::wcwidth;

use crate::shell::{Shell, sys, write_fd};

/// O que a leitura de uma linha devolveu.
pub enum Input {
    /// A linha aceita, sem o fim de linha.
    Line(Vec<u8>),
    /// Ctrl-D numa linha vazia, ou o terminal fechou.
    Eof,
    /// Chegou SIGINT (Ctrl-C): a linha foi descartada e `sh.sigint` ficou ligado.
    Interrupted,
}

/// Marcas de texto invisível no prompt (`\[` e `\]` viram estas, como `RL_PROMPT_START_IGNORE` e
/// `RL_PROMPT_END_IGNORE`).
pub const PROMPT_START_IGNORE: u8 = 0x01;
pub const PROMPT_END_IGNORE: u8 = 0x02;

/// Os caracteres que separam palavras na completação (`rl_completer_word_break_characters` do bash,
/// sem `@`, `$` e `{`, que a completação trata no próprio editor).
const WORD_BREAK: &str = " \t\n\"'><=;|&(";
/// Os caracteres que a completação de nome de arquivo protege com `\` (`rl_filename_quote_characters`
/// do bash).
const FILENAME_QUOTE: &str = " \t\n\\\"'@<>=;|&()#$`?*[!:{";
/// Acima deste número de candidatos o readline pergunta antes de listar (`completion-query-items`).
const QUERY_ITEMS: usize = 100;
const BELL: &[u8] = b"\x07";

#[derive(Clone, Debug, PartialEq)]
enum Key {
    Char(char),
    /// Byte de controle: 0x00 a 0x1f e 0x7f.
    Ctrl(u8),
    /// `ESC` seguido de um caractere (Meta/Alt).
    Meta(char),
    /// `ESC [ parâmetros final`.
    Csi(Vec<u8>, u8),
    /// `ESC O final`.
    Ss3(u8),
    /// Texto colado entre `ESC[200~` e `ESC[201~`.
    Paste(Vec<u8>),
}

/// O que interrompe a leitura de teclas.
enum Stop {
    Eof,
    Interrupted,
}

struct Editor<'a> {
    sh: &'a mut Shell,
    prompt: Vec<u8>,
    buf: Vec<char>,
    pos: usize,
    /// Linha e coluna do cursor contadas a partir do começo do prompt, no último desenho.
    cursor_row: usize,
    cursor_col: usize,
    /// Já houve um desenho (o primeiro sai onde o cursor estiver, sem voltar ao começo da linha).
    drawn: bool,
    /// Posição no histórico; igual ao tamanho do histórico é a linha nova.
    hist_pos: usize,
    /// A linha nova guardada enquanto se navega pelo histórico.
    new_line: Vec<char>,
    kill: Vec<char>,
    /// A tecla anterior foi Tab (o segundo Tab seguido lista os candidatos).
    last_was_tab: bool,
    /// Bytes lidos do terminal e ainda não consumidos.
    input: Vec<u8>,
    input_pos: usize,
}

/// Lê uma linha com o editor. `None` quando o stdin não é um terminal (quem chama lê do jeito simples).
pub fn read_line(sh: &mut Shell, prompt: &[u8]) -> Option<Input> {
    let s = sys();
    let saved = s.tcgetattr(Fd::STDIN).ok()?;
    let mut raw = saved.clone();
    raw.c_lflag &= !(ICANON | ECHO | ECHONL | IEXTEN);
    raw.c_iflag &= !(ICRNL | INLCR);
    raw.c_cc[VMIN] = 1;
    raw.c_cc[VTIME] = 0;
    s.tcsetattr(Fd::STDIN, SetAttrWhen::Drain, &raw).ok()?;
    let bracketed = sh.var_bytes("TERM").is_none_or(|t| t != b"dumb");
    if bracketed {
        let _ = write_fd(Fd::STDERR, b"\x1b[?2004h");
    }
    let hist_pos = sh.history.entries.len();
    let mut ed = Editor {
        sh,
        prompt: prompt.to_vec(),
        buf: Vec::new(),
        pos: 0,
        cursor_row: 0,
        cursor_col: 0,
        drawn: false,
        hist_pos,
        new_line: Vec::new(),
        kill: Vec::new(),
        last_was_tab: false,
        input: Vec::new(),
        input_pos: 0,
    };
    ed.refresh();
    let result = ed.run();
    let _ = s.tcsetattr(Fd::STDIN, SetAttrWhen::Drain, &saved);
    if bracketed {
        let _ = write_fd(Fd::STDERR, b"\x1b[?2004l\r");
    }
    Some(result)
}

/// O prompt sem as marcas de texto invisível, para quem o escreve direto.
pub fn strip_prompt_markers(prompt: &[u8]) -> Vec<u8> {
    prompt.iter().copied().filter(|c| *c != PROMPT_START_IGNORE && *c != PROMPT_END_IGNORE).collect()
}

/// Largura de um caractere da linha na tela: os de controle aparecem como `^X`.
fn char_cells(c: char) -> usize {
    match c {
        '\x00'..='\x1f' | '\x7f' => 2,
        _ => wcwidth(c).max(0) as usize,
    }
}

/// `ESC [ n letra`, sem nada quando `n` é zero (o terminal leria zero como um).
fn cursor_move(out: &mut Vec<u8>, n: usize, letter: u8) {
    if n > 0 {
        out.extend(format!("\x1b[{n}").as_bytes());
        out.push(letter);
    }
}

fn is_word_char(c: char) -> bool {
    c.is_alphanumeric()
}

fn utf8_len(first: u8) -> usize {
    match first {
        0xC2..=0xDF => 2,
        0xE0..=0xEF => 3,
        0xF0..=0xF4 => 4,
        _ => 1,
    }
}

/// Escreve o texto e acompanha a posição do cursor na tela (linha, coluna), com a quebra automática
/// do terminal: um caractere que não cabe na linha vai para a seguinte.
struct Painter {
    out: Vec<u8>,
    cols: usize,
    row: usize,
    col: usize,
}

impl Painter {
    fn put(&mut self, c: char) {
        if c == '\n' {
            self.out.extend_from_slice(b"\r\n");
            self.row += 1;
            self.col = 0;
            return;
        }
        if c == '\t' {
            let n = 8 - self.col % 8;
            for _ in 0..n {
                self.put(' ');
            }
            return;
        }
        let w = char_cells(c);
        if self.col + w > self.cols {
            self.row += 1;
            self.col = 0;
        }
        match c {
            '\x00'..='\x1f' => self.out.extend_from_slice(&[b'^', c as u8 + 0x40]),
            '\x7f' => self.out.extend_from_slice(b"^?"),
            _ => {
                let mut tmp = [0u8; 4];
                self.out.extend_from_slice(c.encode_utf8(&mut tmp).as_bytes());
            }
        }
        self.col += w;
    }

    /// Onde o próximo caractere, de largura `w`, sai na tela.
    fn next_position(&self, w: usize) -> (usize, usize) {
        if self.col + w.max(1) > self.cols { (self.row + 1, 0) } else { (self.row, self.col) }
    }
}

impl Editor<'_> {
    fn columns(&self) -> usize {
        match sys().tcgetwinsize(Fd::STDIN) {
            Ok(ws) if ws.cols > 0 => ws.cols as usize,
            _ => 80,
        }
    }

    fn bell(&self) {
        let _ = write_fd(Fd::STDERR, BELL);
    }

    /// Redesenha o prompt e a linha e põe o cursor na posição de edição.
    fn refresh(&mut self) {
        let cols = self.columns();
        let mut p = Painter { out: Vec::new(), cols, row: 0, col: 0 };
        // Volta ao começo do prompt. Na primeira linha o movimento é relativo: o prompt pode ter saído
        // depois de uma saída sem fim de linha, e o que está antes dele fica na tela, como no readline.
        if self.drawn {
            if self.cursor_row > 0 {
                cursor_move(&mut p.out, self.cursor_row, b'A');
                p.out.push(b'\r');
            } else {
                cursor_move(&mut p.out, self.cursor_col, b'D');
            }
            p.out.extend_from_slice(b"\x1b[J");
        }
        self.drawn = true;
        let mut invisible = false;
        let prompt = std::mem::take(&mut self.prompt);
        let mut i = 0;
        while i < prompt.len() {
            let b = prompt[i];
            match b {
                PROMPT_START_IGNORE => invisible = true,
                PROMPT_END_IGNORE => invisible = false,
                _ if invisible => p.out.push(b),
                b'\r' => {
                    p.out.push(b);
                    p.col = 0;
                }
                0x1b | 0x07 => p.out.push(b),
                _ => {
                    let n = utf8_len(b).min(prompt.len() - i);
                    match std::str::from_utf8(&prompt[i..i + n]).ok().and_then(|s| s.chars().next()) {
                        Some(c) if !c.is_control() || c == '\n' || c == '\t' => p.put(c),
                        _ => p.out.extend_from_slice(&prompt[i..i + n]),
                    }
                    i += n;
                    continue;
                }
            }
            i += 1;
        }
        self.prompt = prompt;
        let mut cursor = None;
        for (k, c) in self.buf.iter().enumerate() {
            if k == self.pos {
                cursor = Some(if *c == '\n' { (p.row, p.col.min(cols)) } else { p.next_position(char_cells(*c)) });
            }
            p.put(*c);
        }
        // O cursor parado depois da última coluna: o terminal só quebra a linha no próximo caractere.
        if p.col >= cols {
            p.out.extend_from_slice(b" \r");
            p.row += 1;
            p.col = 0;
        }
        let (crow, ccol) = cursor.unwrap_or((p.row, p.col));
        if p.row == crow {
            if p.col > ccol {
                cursor_move(&mut p.out, p.col - ccol, b'D');
            } else {
                cursor_move(&mut p.out, ccol - p.col, b'C');
            }
        } else {
            cursor_move(&mut p.out, p.row - crow, b'A');
            p.out.push(b'\r');
            cursor_move(&mut p.out, ccol, b'C');
        }
        self.cursor_row = crow;
        self.cursor_col = ccol;
        let _ = write_fd(Fd::STDERR, &p.out);
    }

    /// Leva o cursor para depois do fim da linha (antes de escrever abaixo dela).
    fn move_to_end(&mut self) {
        let pos = self.pos;
        self.pos = self.buf.len();
        self.refresh();
        self.pos = pos;
    }

    // ---- entrada ----

    fn byte(&mut self) -> Result<u8, Stop> {
        loop {
            if self.input_pos < self.input.len() {
                let b = self.input[self.input_pos];
                self.input_pos += 1;
                return Ok(b);
            }
            let mut tmp = [0u8; 512];
            match sysabi::sys::read(Fd::STDIN, &mut tmp) {
                Ok(0) => return Err(Stop::Eof),
                Ok(n) => {
                    self.input.clear();
                    self.input.extend_from_slice(&tmp[..n]);
                    self.input_pos = 0;
                }
                Err(Errno::EINTR) => {
                    let _ = self.sh.run_pending_traps();
                    if self.sh.sigint {
                        return Err(Stop::Interrupted);
                    }
                }
                Err(Errno::EAGAIN) => {}
                Err(_) => return Err(Stop::Eof),
            }
        }
    }

    fn utf8_char(&mut self, first: u8) -> Result<char, Stop> {
        let n = utf8_len(first);
        let mut bytes = vec![first];
        for _ in 1..n {
            let b = self.byte()?;
            if b & 0xC0 != 0x80 {
                self.input_pos -= 1;
                break;
            }
            bytes.push(b);
        }
        Ok(std::str::from_utf8(&bytes).ok().and_then(|s| s.chars().next()).unwrap_or('\u{fffd}'))
    }

    fn key(&mut self) -> Result<Key, Stop> {
        let b = self.byte()?;
        if b == 0x1b {
            let b2 = self.byte()?;
            return match b2 {
                b'[' => {
                    let mut params = Vec::new();
                    loop {
                        let c = self.byte()?;
                        if (0x40..=0x7e).contains(&c) {
                            if c == b'~' && params == b"200" {
                                return self.paste();
                            }
                            return Ok(Key::Csi(params, c));
                        }
                        params.push(c);
                    }
                }
                b'O' => Ok(Key::Ss3(self.byte()?)),
                0x00..=0x7f => Ok(Key::Meta(b2 as char)),
                _ => Ok(Key::Meta(self.utf8_char(b2)?)),
            };
        }
        if b < 0x20 || b == 0x7f {
            return Ok(Key::Ctrl(b));
        }
        if b < 0x80 {
            return Ok(Key::Char(b as char));
        }
        Ok(Key::Char(self.utf8_char(b)?))
    }

    /// O texto colado até `ESC[201~`.
    fn paste(&mut self) -> Result<Key, Stop> {
        const END: &[u8] = b"\x1b[201~";
        let mut text = Vec::new();
        loop {
            text.push(self.byte()?);
            if text.ends_with(END) {
                text.truncate(text.len() - END.len());
                return Ok(Key::Paste(text));
            }
        }
    }

    // ---- edição ----

    fn insert(&mut self, text: &[char]) {
        for (k, c) in text.iter().enumerate() {
            self.buf.insert(self.pos + k, *c);
        }
        self.pos += text.len();
    }

    fn word_left(&self) -> usize {
        let mut p = self.pos;
        while p > 0 && !is_word_char(self.buf[p - 1]) {
            p -= 1;
        }
        while p > 0 && is_word_char(self.buf[p - 1]) {
            p -= 1;
        }
        p
    }

    fn word_right(&self) -> usize {
        let mut p = self.pos;
        while p < self.buf.len() && !is_word_char(self.buf[p]) {
            p += 1;
        }
        while p < self.buf.len() && is_word_char(self.buf[p]) {
            p += 1;
        }
        p
    }

    /// Recorta `from..to` para o kill ring.
    fn kill_range(&mut self, from: usize, to: usize) {
        if from < to {
            self.kill = self.buf.drain(from..to).collect();
            self.pos = from;
        }
    }

    fn set_line(&mut self, line: Vec<char>) {
        self.buf = line;
        self.pos = self.buf.len();
    }

    fn history_line(&self, idx: usize) -> Vec<char> {
        String::from_utf8_lossy(&self.sh.history.entries[idx]).chars().collect()
    }

    fn history_move(&mut self, to: usize) {
        let len = self.sh.history.entries.len();
        if to > len || to == self.hist_pos {
            self.bell();
            return;
        }
        if self.hist_pos == len {
            self.new_line = self.buf.clone();
        }
        self.hist_pos = to;
        let line = if to == len { self.new_line.clone() } else { self.history_line(to) };
        self.set_line(line);
    }

    /// Alt-. (`yank-last-arg`): a última palavra do comando anterior.
    fn yank_last_arg(&mut self) {
        let Some(last) = self.sh.history.entries.last() else {
            self.bell();
            return;
        };
        let text = String::from_utf8_lossy(last).into_owned();
        match text.split_whitespace().last() {
            Some(w) => {
                let w: Vec<char> = w.chars().collect();
                self.insert(&w);
            }
            None => self.bell(),
        }
    }

    fn run(&mut self) -> Input {
        loop {
            let key = match self.key() {
                Ok(k) => k,
                Err(stop) => return self.stop(stop),
            };
            match self.dispatch(key) {
                Ok(Some(input)) => return input,
                Ok(None) => {}
                Err(stop) => return self.stop(stop),
            }
        }
    }

    fn stop(&mut self, stop: Stop) -> Input {
        match stop {
            Stop::Eof => {
                self.move_to_end();
                Input::Eof
            }
            Stop::Interrupted => {
                self.move_to_end();
                let _ = write_fd(Fd::STDERR, b"^C");
                Input::Interrupted
            }
        }
    }

    fn accept(&mut self) -> Input {
        self.move_to_end();
        // O `\n` vira `\r\n` pelo `ONLCR` do tty, como no `rl_crlf`.
        let _ = write_fd(Fd::STDERR, b"\n");
        Input::Line(self.buf.iter().collect::<String>().into_bytes())
    }

    /// Executa uma tecla; `Some` termina a leitura.
    fn dispatch(&mut self, key: Key) -> Result<Option<Input>, Stop> {
        let was_tab = std::mem::replace(&mut self.last_was_tab, false);
        let len = self.buf.len();
        match key {
            Key::Ctrl(b'\r' | b'\n') => return Ok(Some(self.accept())),
            Key::Char(c) => self.insert(&[c]),
            Key::Paste(text) => {
                let text: Vec<char> = String::from_utf8_lossy(&text).replace("\r\n", "\n").replace('\r', "\n").chars().collect();
                self.insert(&text);
            }
            Key::Ctrl(0x01) => self.pos = 0,
            Key::Ctrl(0x05) => self.pos = len,
            Key::Ctrl(0x02) => {
                if self.pos > 0 {
                    self.pos -= 1
                } else {
                    self.bell()
                }
            }
            Key::Ctrl(0x06) => {
                if self.pos < len {
                    self.pos += 1
                } else {
                    self.bell()
                }
            }
            Key::Ctrl(0x10) => self.history_move(self.hist_pos.wrapping_sub(1)),
            Key::Ctrl(0x0e) => self.history_move(self.hist_pos + 1),
            Key::Ctrl(0x7f | 0x08) => {
                if self.pos > 0 {
                    self.pos -= 1;
                    self.buf.remove(self.pos);
                } else {
                    self.bell();
                }
            }
            Key::Ctrl(0x04) => {
                if len == 0 {
                    return Err(Stop::Eof);
                }
                if self.pos < len {
                    self.buf.remove(self.pos);
                } else {
                    self.bell();
                }
            }
            Key::Ctrl(0x0b) => self.kill_range(self.pos, len),
            Key::Ctrl(0x15) => self.kill_range(0, self.pos),
            Key::Ctrl(0x17) => {
                // unix-word-rubout: até o espaço anterior.
                let mut p = self.pos;
                while p > 0 && self.buf[p - 1].is_whitespace() {
                    p -= 1;
                }
                while p > 0 && !self.buf[p - 1].is_whitespace() {
                    p -= 1;
                }
                self.kill_range(p, self.pos);
            }
            Key::Ctrl(0x19) => {
                let k = self.kill.clone();
                self.insert(&k);
            }
            Key::Ctrl(0x14) => {
                // transpose-chars: no fim da linha troca os dois últimos.
                if len >= 2 && self.pos > 0 {
                    let p = if self.pos == len { self.pos - 1 } else { self.pos };
                    self.buf.swap(p - 1, p);
                    self.pos = p + 1;
                } else {
                    self.bell();
                }
            }
            Key::Ctrl(0x0c) => {
                let _ = write_fd(Fd::STDERR, b"\x1b[H\x1b[2J");
                self.drawn = false;
                self.cursor_row = 0;
                self.cursor_col = 0;
            }
            Key::Ctrl(0x09) => {
                self.complete(was_tab)?;
                self.last_was_tab = true;
            }
            Key::Ctrl(0x12) => return self.reverse_search(),
            Key::Ctrl(0x16) => {
                // quoted-insert: o próximo caractere entra como está.
                let b = self.byte()?;
                let c = if b < 0x80 { b as char } else { self.utf8_char(b)? };
                self.insert(&[c]);
            }
            Key::Meta('b' | 'B') => self.pos = self.word_left(),
            Key::Meta('f' | 'F') => self.pos = self.word_right(),
            Key::Meta('d' | 'D') => {
                let to = self.word_right();
                self.kill_range(self.pos, to);
            }
            Key::Meta('\x7f' | '\x08') => {
                let from = self.word_left();
                self.kill_range(from, self.pos);
            }
            Key::Meta('.' | '_') => self.yank_last_arg(),
            Key::Meta('<') => self.history_move(0),
            Key::Meta('>') => self.history_move(self.sh.history.entries.len()),
            Key::Csi(ref params, fin) => self.csi(params, fin),
            Key::Ss3(fin) => self.csi(b"", fin),
            _ => self.bell(),
        }
        self.refresh();
        Ok(None)
    }

    /// As teclas de cursor e de edição em `CSI` e `SS3`.
    fn csi(&mut self, params: &[u8], fin: u8) {
        let len = self.buf.len();
        // O modificador do xterm (`1;5` é Ctrl, `1;3` é Alt).
        let modified = params.split(|c| *c == b';').nth(1).is_some_and(|m| m == b"5" || m == b"3");
        match (params, fin) {
            (_, b'A') => self.history_move(self.hist_pos.wrapping_sub(1)),
            (_, b'B') => self.history_move(self.hist_pos + 1),
            (_, b'C') if modified => self.pos = self.word_right(),
            (_, b'D') if modified => self.pos = self.word_left(),
            (_, b'C') => {
                if self.pos < len {
                    self.pos += 1
                } else {
                    self.bell()
                }
            }
            (_, b'D') => {
                if self.pos > 0 {
                    self.pos -= 1
                } else {
                    self.bell()
                }
            }
            (_, b'H') | (b"1" | b"7", b'~') => self.pos = 0,
            (_, b'F') | (b"4" | b"8", b'~') => self.pos = len,
            (b"3", b'~') => {
                if self.pos < len {
                    self.buf.remove(self.pos);
                } else {
                    self.bell();
                }
            }
            (b"3;5", b'~') => {
                let to = self.word_right();
                self.kill_range(self.pos, to);
            }
            _ => self.bell(),
        }
    }

    // ---- busca no histórico ----

    /// Ctrl-R (`reverse-search-history`): o prompt vira `(reverse-i-search)`texto': ` e a linha mostra
    /// o comando mais recente que contém o texto.
    fn reverse_search(&mut self) -> Result<Option<Input>, Stop> {
        let saved_prompt = std::mem::take(&mut self.prompt);
        let saved_line = (self.buf.clone(), self.pos, self.hist_pos);
        let mut query: Vec<char> = Vec::new();
        let mut at = self.sh.history.entries.len();
        let mut failed = false;
        let result = loop {
            let q: String = query.iter().collect();
            self.prompt = format!("({}reverse-i-search)`{}': ", if failed { "failed " } else { "" }, q).into_bytes();
            self.refresh();
            let key = match self.key() {
                Ok(k) => k,
                Err(e) => break Err(e),
            };
            let search = |ed: &Self, from: usize, q: &str| -> Option<(usize, usize)> {
                (0..from).rev().find_map(|i| {
                    let line = String::from_utf8_lossy(&ed.sh.history.entries[i]).into_owned();
                    line.rfind(q).map(|byte| (i, line[..byte].chars().count()))
                })
            };
            let found = match key {
                Key::Char(c) => {
                    query.push(c);
                    search(self, (at + 1).min(self.sh.history.entries.len()), &query.iter().collect::<String>())
                }
                Key::Ctrl(0x12) => {
                    if query.is_empty() {
                        None
                    } else {
                        search(self, at, &q)
                    }
                }
                Key::Ctrl(0x7f | 0x08) => {
                    query.pop();
                    let q: String = query.iter().collect();
                    if q.is_empty() {
                        failed = false;
                        continue;
                    }
                    search(self, self.sh.history.entries.len(), &q)
                }
                Key::Ctrl(0x07) => {
                    (self.buf, self.pos, self.hist_pos) = saved_line.clone();
                    break Ok(None);
                }
                Key::Ctrl(b'\r' | b'\n') => {
                    self.prompt = saved_prompt.clone();
                    break Ok(Some(self.accept()));
                }
                other => {
                    // Outra tecla encerra a busca com a linha achada e é executada normalmente.
                    self.prompt = saved_prompt.clone();
                    break self.dispatch(other);
                }
            };
            match found {
                Some((i, col)) => {
                    failed = false;
                    at = i;
                    self.hist_pos = i;
                    self.buf = self.history_line(i);
                    self.pos = col;
                }
                None => {
                    failed = !query.is_empty();
                    self.bell();
                }
            }
        };
        self.prompt = saved_prompt;
        if matches!(result, Ok(None)) {
            self.refresh();
        }
        result
    }

    // ---- completação ----

    /// O começo da palavra que está sendo completada (um separador protegido por `\` não conta).
    fn word_start(&self) -> usize {
        let mut p = self.pos;
        while p > 0 {
            let c = self.buf[p - 1];
            if WORD_BREAK.contains(c) && !(p >= 2 && self.buf[p - 2] == '\\') {
                break;
            }
            p -= 1;
        }
        p
    }

    /// A palavra está na posição de comando: começo da linha ou depois de `;`, `|`, `&`, `(` ou de
    /// uma palavra reservada que abre comando.
    fn command_position(&self, start: usize) -> bool {
        let before: String = self.buf[..start].iter().collect();
        let before = before.trim_end();
        if before.is_empty() || before.ends_with([';', '|', '&', '(', '`', '{', '!']) || before.ends_with("$(") {
            return true;
        }
        let last = before.rsplit(|c: char| c.is_whitespace() || ";|&(".contains(c)).next().unwrap_or("");
        matches!(last, "then" | "else" | "elif" | "do" | "if" | "while" | "until" | "time" | "sudo" | "exec" | "command" | "nohup")
    }

    fn home(&self) -> Vec<u8> {
        self.sh.var_bytes("HOME").unwrap_or(b"/").to_vec()
    }

    fn is_dir(path: &[u8]) -> bool {
        sysabi::sys::stat(path).is_ok_and(|st| st.file_type() == FileType::Directory)
    }

    /// Candidatos de nome de arquivo para `word` (sem as barras de proteção): o diretório como foi
    /// digitado e os nomes, cada um com a marca de diretório.
    fn file_candidates(&self, word: &str, only_dirs: bool) -> (String, Vec<(String, bool)>) {
        let (dir, base) = match word.rfind('/') {
            Some(p) => (&word[..=p], &word[p + 1..]),
            None => ("", word),
        };
        let listed: Vec<u8> = if dir.is_empty() {
            b".".to_vec()
        } else if let Some(rest) = dir.strip_prefix("~/") {
            [self.home().as_slice(), b"/", rest.as_bytes()].concat()
        } else {
            dir.as_bytes().to_vec()
        };
        let mut out = Vec::new();
        if let Ok(entries) = sysabi::sys::read_dir(&listed) {
            for e in entries {
                let name = String::from_utf8_lossy(&e.name).into_owned();
                if name == "." || name == ".." || !name.starts_with(base) || (name.starts_with('.') && !base.starts_with('.')) {
                    continue;
                }
                let is_dir = match e.kind {
                    FileType::Directory => true,
                    FileType::Symlink => Self::is_dir(&[listed.as_slice(), b"/", &e.name].concat()),
                    _ => false,
                };
                if only_dirs && !is_dir {
                    continue;
                }
                out.push((name, is_dir));
            }
        }
        (dir.to_string(), out)
    }

    fn quote_filename(name: &str, at_word_start: bool) -> String {
        let mut q = String::new();
        for (k, c) in name.chars().enumerate() {
            if FILENAME_QUOTE.contains(c) || (c == '~' && k == 0 && at_word_start) {
                q.push('\\');
            }
            q.push(c);
        }
        q
    }

    fn complete(&mut self, list: bool) -> Result<(), Stop> {
        let start = self.word_start();
        let typed: String = self.buf[start..self.pos].iter().collect();
        let word = typed.replace('\\', "");
        // (texto já digitado que fica, nomes dos candidatos, marca de diretório, completação de arquivo)
        let (lead, mut names, filenames): (String, Vec<(String, bool)>, bool);
        if let Some(var) = word.strip_prefix('$').filter(|v| !v.contains('/')) {
            let set: BTreeSet<String> = self.sh.all_var_names().into_iter().filter(|n| n.starts_with(var)).collect();
            lead = "$".into();
            names = set.into_iter().map(|n| (n, false)).collect();
            filenames = false;
        } else if word == "~" {
            lead = String::new();
            names = vec![("~".into(), true)];
            filenames = true;
        } else if self.command_position(start) && !word.contains('/') {
            let mut set: BTreeSet<String> = crate::builtins::core::candidates(self.sh, &["command"], word.as_bytes())
                .into_iter()
                .map(|n| String::from_utf8_lossy(&n).into_owned())
                .filter(|n| n.starts_with(&word))
                .collect();
            let (_, dirs) = self.file_candidates(&word, true);
            let dirs: Vec<(String, bool)> = dirs.into_iter().filter(|(n, _)| !set.contains(n)).collect();
            lead = String::new();
            names = std::mem::take(&mut set).into_iter().map(|n| (n, false)).collect();
            names.extend(dirs);
            filenames = false;
        } else {
            let (dir, files) = self.file_candidates(&word, false);
            lead = dir;
            names = files;
            filenames = true;
        }
        names.sort();
        names.dedup();
        let base_len = word.chars().count() - lead.chars().count();
        if names.is_empty() {
            self.bell();
            return Ok(());
        }
        let quote = |s: &str, at_start: bool| if filenames || !lead.is_empty() || s.contains('/') { Self::quote_filename(s, at_start) } else { s.to_string() };
        let lead_q = if lead.starts_with("~/") { format!("~/{}", Self::quote_filename(&lead[2..], false)) } else { quote(&lead, true) };
        if names.len() == 1 {
            let (name, is_dir) = &names[0];
            let mut text = if name == "~" { "~".to_string() } else { format!("{lead_q}{}", quote(name, lead.is_empty())) };
            text.push(if *is_dir { '/' } else { ' ' });
            self.replace_word(start, &text);
            return Ok(());
        }
        let first = &names[0].0;
        let common = names.iter().fold(first.chars().count(), |n, (s, _)| first.chars().zip(s.chars()).take(n).take_while(|(a, b)| a == b).count());
        if common > base_len {
            let prefix: String = first.chars().take(common).collect();
            let text = format!("{lead_q}{}", quote(&prefix, lead.is_empty()));
            self.replace_word(start, &text);
            return Ok(());
        }
        if !list {
            self.bell();
            return Ok(());
        }
        self.list_candidates(&names)
    }

    fn replace_word(&mut self, start: usize, text: &str) {
        self.buf.drain(start..self.pos);
        self.pos = start;
        let t: Vec<char> = text.chars().collect();
        self.insert(&t);
    }

    /// Lista os candidatos em colunas, de cima para baixo, como o `rl_display_match_list`.
    fn list_candidates(&mut self, names: &[(String, bool)]) -> Result<(), Stop> {
        self.move_to_end();
        let mut out: Vec<u8> = b"\r\n".to_vec();
        if names.len() >= QUERY_ITEMS {
            let _ = write_fd(Fd::STDERR, format!("\r\nDisplay all {} possibilities? (y or n)", names.len()).as_bytes());
            loop {
                match self.key()? {
                    Key::Char('y' | 'Y' | ' ') => break,
                    Key::Char('n' | 'N') | Key::Ctrl(0x7f | 0x07) => {
                        let _ = write_fd(Fd::STDERR, b"\r\n");
                        self.drawn = false;
                        self.cursor_row = 0;
                        self.cursor_col = 0;
                        return Ok(());
                    }
                    _ => self.bell(),
                }
            }
        }
        let cols = self.columns();
        let width = names.iter().map(|(n, _)| ul_common::width::display_width(n)).max().unwrap_or(0) + 2;
        let mut limit = (cols / width).max(1);
        if limit != 1 && limit * width == cols {
            limit -= 1;
        }
        let rows = names.len().div_ceil(limit);
        for r in 0..rows {
            for c in 0..limit {
                let Some((name, is_dir)) = names.get(c * rows + r) else { break };
                let shown = if *is_dir { format!("{name}/") } else { name.clone() };
                out.extend_from_slice(shown.as_bytes());
                if c + 1 < limit {
                    let used = ul_common::width::display_width(&shown);
                    out.extend(std::iter::repeat_n(b' ', width.saturating_sub(used)));
                }
            }
            out.extend_from_slice(b"\r\n");
        }
        let _ = write_fd(Fd::STDERR, &out);
        self.drawn = false;
        self.cursor_row = 0;
        self.cursor_col = 0;
        Ok(())
    }
}
