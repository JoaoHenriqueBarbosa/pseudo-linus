//! Leitura de entradas igual ao `jq_util_input` do jq 1.7.1 (`src/util.c`): arquivos concatenados num
//! parser só, lidos linha a linha com `fgets` (buffer de 4096), contagem de linha por chunk que contém
//! `\n`, nome do arquivo corrente que vira inválido quando o arquivo acaba. É isso que define o
//! `(at <stdin>:N)` das mensagens de erro e o `input_line_number`.

use std::collections::VecDeque;

use jaq_json::{Rc, Val};

use super::json::{Next, Parser};

/// Leitor de arquivos injetado (MemTree no candidato em processo, `std::fs` no binário).
pub type ReadFile = Box<dyn Fn(&str) -> Result<Vec<u8>, String>>;

struct Source {
    data: Vec<u8>,
    pos: usize,
    eof: bool,
}

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
    /// Mensagens de erro de abertura de arquivo (vão pro stderr na ordem em que acontecem).
    pub errors: Vec<u8>,
    parser: Option<Parser>,
    slurped: Option<Slurped>,
    buf: Vec<u8>,
    stdin: Rc<Vec<u8>>,
    read_file: ReadFile,
    stream: bool,
    pending: VecDeque<Val>,
}

impl InputState {
    pub fn new(files: Vec<String>, stdin: Rc<Vec<u8>>, read_file: ReadFile, raw: bool, slurp: bool, seq: bool, stream: bool) -> Self {
        let files = if files.is_empty() { vec!["-".to_string()] } else { files };
        let slurped = slurp.then(|| if raw { Slurped::Str(Vec::new()) } else { Slurped::Arr(Vec::new()) });
        InputState {
            files,
            curr_file: 0,
            current: None,
            current_filename: None,
            current_line: 0,
            failures: 0,
            errors: Vec::new(),
            parser: (!raw).then(|| Parser::new(seq)),
            slurped,
            buf: Vec::new(),
            stdin,
            read_file,
            stream,
            pending: VecDeque::new(),
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

    fn read_more(&mut self) -> bool {
        let at_eof = self.current.as_ref().is_none_or(|s| s.eof);
        if at_eof {
            if self.current.is_some() {
                self.current = None;
                self.current_filename = None;
                self.current_line = 0;
            }
            if self.curr_file < self.files.len() {
                let f = self.files[self.curr_file].clone();
                self.curr_file += 1;
                if f == "-" {
                    self.current = Some(Source { data: self.stdin.as_ref().clone(), pos: 0, eof: false });
                    self.current_filename = Some("<stdin>".into());
                } else {
                    match (self.read_file)(&f) {
                        Ok(data) => self.current = Some(Source { data, pos: 0, eof: false }),
                        Err(e) => {
                            self.errors.extend_from_slice(format!("jq: error: Could not open file {f}: {e}\n").as_bytes());
                            self.failures += 1;
                            self.current = None;
                        }
                    }
                    self.current_filename = Some(f);
                }
                self.current_line = 0;
            }
        }
        self.buf.clear();
        if let Some(src) = &mut self.current {
            if src.pos >= src.data.len() {
                src.eof = true;
            } else {
                // fgets com buffer de 4096: até 4095 bytes ou até o '\n' inclusive.
                let limit = (src.pos + 4095).min(src.data.len());
                let end = match src.data[src.pos..limit].iter().position(|&b| b == b'\n') {
                    Some(i) => src.pos + i + 1,
                    None => limit,
                };
                self.buf.extend_from_slice(&src.data[src.pos..end]);
                src.pos = end;
                if self.buf.last() != Some(&b'\n') && src.pos >= src.data.len() {
                    src.eof = true;
                }
                if self.buf.contains(&b'\n') {
                    self.current_line += 1;
                }
            }
        }
        self.curr_file == self.files.len() && self.current.as_ref().is_none_or(|s| s.eof)
    }

    /// `jq_util_input_next_input`: `None` quando acabou.
    pub fn next_value(&mut self) -> Option<Result<Val, String>> {
        if let Some(v) = self.pending.pop_front() {
            return Some(Ok(v));
        }
        let r = self.next_raw();
        match r {
            Some(Ok(v)) if self.stream => {
                let mut events = Vec::new();
                to_stream(&v, &mut Vec::new(), true, &mut events);
                self.pending.extend(events);
                self.pending.pop_front().map(Ok)
            }
            other => other,
        }
    }

    fn next_raw(&mut self) -> Option<Result<Val, String>> {
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
                            return Some(Ok(Val::utf8_str(String::from_utf8_lossy(&value).into_owned())));
                        }
                        value.extend_from_slice(&self.buf);
                        self.buf.clear();
                        raw_value = Some(value);
                    }
                }
            } else {
                if self.parser.as_ref().expect("parser").remaining() == 0 {
                    is_last = self.read_more();
                    let buf = std::mem::take(&mut self.buf);
                    self.parser.as_mut().expect("parser").set_buf(&buf, !is_last);
                    self.buf = buf;
                }
                let parser = self.parser.as_mut().expect("parser");
                let next = parser.next();
                let remaining = parser.remaining();
                match (&mut self.slurped, next) {
                    (Some(Slurped::Arr(a)), Next::Value(v)) => {
                        a.push(v);
                        has_more = remaining > 0;
                    }
                    // Modo raw não tem parser; este braço não acontece.
                    (Some(Slurped::Str(_)), Next::Value(_)) => {}
                    (Some(_), Next::Error(e)) => return Some(Err(e)),
                    (Some(_), Next::None) => has_more = remaining > 0,
                    (None, Next::Value(v)) => return Some(Ok(v)),
                    (None, Next::Error(e)) => return Some(Err(e)),
                    (None, Next::None) => {}
                }
            }
            if !(!is_last || has_more) {
                break;
            }
        }
        match self.slurped.take() {
            Some(Slurped::Arr(a)) => Some(Ok(Val::Arr(Rc::new(a)))),
            Some(Slurped::Str(s)) => Some(Ok(Val::utf8_str(String::from_utf8_lossy(&s).into_owned()))),
            None => raw_value.map(|v| Ok(Val::utf8_str(String::from_utf8_lossy(&v).into_owned()))),
        }
    }
}

/// Eventos de `--stream` de um valor completo (iguais aos do `tostream`).
fn to_stream(v: &Val, path: &mut Vec<Val>, top: bool, out: &mut Vec<Val>) {
    let leaf = |path: &Vec<Val>, v: &Val| Val::Arr(Rc::new(vec![Val::Arr(Rc::new(path.clone())), v.clone()]));
    match v {
        Val::Arr(a) if !a.is_empty() => {
            for (i, x) in a.iter().enumerate() {
                path.push(Val::from(i));
                to_stream(x, path, false, out);
                path.pop();
            }
            let mut closing = path.clone();
            closing.push(Val::from(a.len() - 1));
            out.push(Val::Arr(Rc::new(vec![Val::Arr(Rc::new(closing))])));
        }
        Val::Obj(o) if !o.is_empty() => {
            let mut last = None;
            for (k, x) in o.iter() {
                path.push(k.clone());
                to_stream(x, path, false, out);
                path.pop();
                last = Some(k.clone());
            }
            let mut closing = path.clone();
            closing.push(last.expect("objeto não vazio"));
            out.push(Val::Arr(Rc::new(vec![Val::Arr(Rc::new(closing))])));
        }
        _ => {
            out.push(leaf(path, v));
            let _ = top;
        }
    }
}

/// Texto de `strerror` pros erros que importam.
pub fn strerror(e: &std::io::Error) -> String {
    match e.raw_os_error() {
        Some(2) => "No such file or directory".into(),
        Some(13) => "Permission denied".into(),
        Some(20) => "Not a directory".into(),
        Some(21) => "Is a directory".into(),
        _ => {
            let s = e.to_string();
            match s.find(" (os error") {
                Some(i) => s[..i].to_string(),
                None => s,
            }
        }
    }
}
