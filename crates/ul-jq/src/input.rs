//! Leitura das entradas igual ao `jq_util_input` do jq 1.7.1 (`src/util.c`): arquivos concatenados
//! num parser só, lidos com `fgets` de 4096 bytes, linha contada por pedaço que tem `\n`, nome do
//! arquivo corrente que some quando o arquivo acaba. É isso que define o `(at <stdin>:N)` das
//! mensagens de erro e o `input_line_number`.

use jaq_json::jqparse::{Next, Parser};
use jaq_json::{Rc, Val};

use crate::io::{self, Source};

/// Valor, mensagem de erro de parse, ou fim.
pub type Input = Option<Result<Val, String>>;

enum Slurped {
    Arr(Vec<Val>),
    Str(Vec<u8>),
}

pub struct InputState {
    files: Vec<String>,
    curr_file: usize,
    current: Option<Source>,
    current_filename: Option<String>,
    current_line: u64,
    pub failures: u32,
    parser: Option<Parser>,
    slurped: Option<Slurped>,
    buf: Vec<u8>,
    stream: Option<StreamState>,
}

impl InputState {
    pub fn new(files: Vec<String>, raw: bool, slurp: bool, seq: bool, stream: bool) -> Self {
        let files = if files.is_empty() { vec!["-".to_string()] } else { files };
        let slurped = slurp.then(|| if raw { Slurped::Str(Vec::new()) } else { Slurped::Arr(Vec::new()) });
        InputState {
            files,
            curr_file: 0,
            current: None,
            current_filename: None,
            current_line: 0,
            failures: 0,
            parser: (!raw).then(|| Parser::new(seq)),
            slurped,
            buf: Vec::new(),
            stream: stream.then(StreamState::default),
        }
    }

    pub fn current_filename(&self) -> Option<String> {
        self.current_filename.clone()
    }

    pub fn current_line(&self) -> u64 {
        self.current_line
    }

    /// `jq_util_input_get_position`.
    pub fn position(&self) -> String {
        match &self.current_filename {
            Some(f) => format!("{f}:{}", self.current_line),
            None => "<unknown>".to_string(),
        }
    }

    /// `jq_util_input_read_more`: devolve se esta foi a última leitura possível.
    fn read_more(&mut self) -> bool {
        let at_eof = self.current.as_ref().is_none_or(|s| s.at_eof() || s.error.is_some());
        if at_eof {
            if let Some(src) = &self.current
                && let Some(e) = src.error {
                    io::stderr(format!("jq: error: {}\n", io::strerror(e)).as_bytes());
                }
            if self.current.is_some() {
                self.current = None;
                self.current_filename = None;
                self.current_line = 0;
            }
            if self.curr_file < self.files.len() {
                let f = self.files[self.curr_file].clone();
                self.curr_file += 1;
                if f == "-" {
                    self.current = Some(Source::stdin());
                    self.current_filename = Some("<stdin>".into());
                } else {
                    match Source::open(&f) {
                        Ok(src) => self.current = Some(src),
                        Err(e) => {
                            io::stderr(format!("jq: error: Could not open file {f}: {}\n", io::strerror(e)).as_bytes());
                            self.failures += 1;
                        }
                    }
                    self.current_filename = Some(f);
                }
                self.current_line = 0;
            }
        }
        self.buf.clear();
        if let Some(src) = &mut self.current {
            let mut line = Vec::new();
            src.fgets(&mut line);
            if line.is_empty() {
                if src.error.is_some() {
                    self.failures += 1;
                }
            } else {
                if line.contains(&b'\n') {
                    self.current_line += 1;
                }
                // Com parser, o jq mede o pedaço com `strlen`: um NUL corta o resto.
                if self.parser.is_some() && !line.contains(&b'\n')
                    && let Some(nul) = line.iter().position(|b| *b == 0) {
                        line.truncate(nul);
                    }
                self.buf = line;
            }
        }
        self.curr_file == self.files.len() && self.current.as_ref().is_none_or(|s| s.at_eof() || s.error.is_some())
    }

    /// `jq_util_input_next_input`, com `--stream` aplicado por cima.
    pub fn next_value(&mut self) -> Input {
        if let Some(st) = &mut self.stream
            && let Some(v) = st.pending.pop_front() {
                return Some(Ok(v));
            }
        let r = self.next_raw();
        match (r, &mut self.stream) {
            (Some(Ok(v)), Some(st)) => {
                to_stream(&v, &mut Vec::new(), &mut st.pending);
                st.pending.pop_front().map(Ok)
            }
            (other, _) => other,
        }
    }

    fn next_raw(&mut self) -> Input {
        let mut raw_value: Option<Vec<u8>> = None;
        loop {
            let mut is_last = false;
            let mut has_more = false;
            if self.parser.is_none() {
                is_last = self.read_more();
                if !self.buf.is_empty() {
                    if let Some(Slurped::Str(s)) = &mut self.slurped {
                        s.extend_from_slice(&self.buf);
                    } else {
                        let mut value = raw_value.take().unwrap_or_default();
                        if self.buf.last() == Some(&b'\n') {
                            value.extend_from_slice(&self.buf[..self.buf.len() - 1]);
                            return Some(Ok(Val::from(jaq_json::jqparse::utf8_lossy(&value))));
                        }
                        value.extend_from_slice(&self.buf);
                        self.buf.clear();
                        raw_value = Some(value);
                    }
                }
            } else {
                if self.parser.as_ref().is_some_and(|p| p.remaining() == 0) {
                    is_last = self.read_more();
                    let buf = std::mem::take(&mut self.buf);
                    if let Some(p) = self.parser.as_mut() {
                        p.set_buf(&buf, !is_last);
                    }
                    self.buf = buf;
                }
                let parser = self.parser.as_mut()?;
                let next = parser.next();
                let remaining = parser.remaining();
                match (&mut self.slurped, next) {
                    (Some(Slurped::Arr(a)), Next::Value(v)) => {
                        a.push(v);
                        has_more = remaining > 0;
                    }
                    (Some(Slurped::Str(_)), Next::Value(_)) => {}
                    (Some(_), Next::Error(e)) => return Some(Err(e)),
                    (Some(_), Next::None) => has_more = remaining > 0,
                    (None, Next::Value(v)) => return Some(Ok(v)),
                    (None, Next::Error(e)) => return Some(Err(e)),
                    (None, Next::None) => {}
                }
            }
            if is_last && !has_more {
                break;
            }
        }
        match self.slurped.take() {
            Some(Slurped::Arr(a)) => Some(Ok(Val::Arr(Rc::new(a)))),
            Some(Slurped::Str(s)) => Some(Ok(Val::from(jaq_json::jqparse::utf8_lossy(&s)))),
            None => raw_value.map(|v| Ok(Val::from(jaq_json::jqparse::utf8_lossy(&v)))),
        }
    }
}

#[derive(Default)]
struct StreamState {
    pending: std::collections::VecDeque<Val>,
}

/// Eventos de `--stream` de um valor completo (os mesmos do `tostream`).
fn to_stream(v: &Val, path: &mut Vec<Val>, out: &mut std::collections::VecDeque<Val>) {
    let leaf = |path: &Vec<Val>, v: &Val| Val::Arr(Rc::new(vec![Val::Arr(Rc::new(path.clone())), v.clone()]));
    match v {
        Val::Arr(a) if !a.is_empty() => {
            for (i, x) in a.iter().enumerate() {
                path.push(Val::from(i));
                to_stream(x, path, out);
                path.pop();
            }
            let mut closing = path.clone();
            closing.push(Val::from(a.len() - 1));
            out.push_back(Val::Arr(Rc::new(vec![Val::Arr(Rc::new(closing))])));
        }
        Val::Obj(o) if !o.is_empty() => {
            let mut last = None;
            for (k, x) in o.iter() {
                path.push(k.clone());
                to_stream(x, path, out);
                path.pop();
                last = Some(k.clone());
            }
            let mut closing = path.clone();
            closing.push(last.unwrap_or(Val::Null));
            out.push_back(Val::Arr(Rc::new(vec![Val::Arr(Rc::new(closing))])));
        }
        _ => out.push_back(leaf(path, v)),
    }
}
