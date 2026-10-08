//! Leitura incremental do stdin: o buffer de bytes cresce a cada `read` no descritor 0, de modo que
//! `for line in sys.stdin` acompanha um pipe vivo (`tail -f | python`), um terminal devolve linha a linha
//! e `sys.stdin.buffer` entrega os bytes exatos (sem passar por UTF-8). O texto decodifica por cima, com
//! newlines universais (`\r\n` e `\r` viram `\n`), como o `TextIOWrapper` do CPython.
//!
//! Cada operação é uma função pura sobre o buffer (`try_*`): quando ele não basta e o descritor ainda não
//! acabou, devolve `Stall`. Quem chama (`drive`) larga o empréstimo do arquivo, espera o descritor 0 cedendo
//! o turno às outras threads (`_net.wait_readable`) e só então lê do kernel, de modo que uma thread que
//! escreve no pipe do stdin do próprio processo roda enquanto a principal espera a linha.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use sysabi::{sys, Fd};

use crate::fork::{suspend, SuspendRequest};
use crate::modules::ModuleBuilder;
use crate::object::{Dict, Kw, Native, PyFile, Value};
use crate::vm::{current, internal, Callee, Entered, PyResult, Vm};

const CHUNK: usize = 64 * 1024;

thread_local! {
    /// A instrução em execução ainda não consumiu entrada e o laço sabe repeti-la depois de esperar o stdin (uma
    /// chamada de leitura ou o `for` sobre ele): a espera sai da nativa em vez de rodar aninhada.
    static REPLAYABLE: Cell<bool> = const { Cell::new(false) };
    /// A instrução repetida acabou de esperar o stdin: a espera não se repete, a leitura segue para o kernel.
    static RESUMED: Cell<bool> = const { Cell::new(false) };
}

/// Liga ou desliga a marca de instrução repetível (o laço a liga em volta de uma leitura do stdin e a desliga
/// depois, o que também encerra o `RESUMED` da repetição).
pub(crate) fn set_replayable(on: bool) {
    REPLAYABLE.with(|r| r.set(on));
    if !on {
        RESUMED.with(|r| r.set(false));
    }
}

/// A instrução é a repetição que vem de uma espera: o que ela fazia antes de ler (o prompt do `input`) já foi feito.
pub(crate) fn resumed() -> bool {
    RESUMED.with(Cell::get)
}

/// A chamada pode ser uma leitura do stdin que o laço sabe repetir: `input` ou o método de um arquivo ou de um
/// buffer padrão (um método de outro objeto nunca pede espera, então o engano só custa a cópia dos argumentos).
pub(crate) fn is_reader(func: &Value) -> bool {
    match func {
        Value::Builtin(name) => *name == "input",
        Value::NativeFn(f) => f.name == "input",
        Value::Bound(b) => matches!(b.recv, Value::Native(_) | Value::Ext(_)),
        _ => false,
    }
}

/// `_sys._stdin_resume()`: o auxiliar que esperou o stdin marca a repetição que vem a seguir.
fn resume(_vm: &mut Vm, _args: Vec<Value>, _kw: Kw) -> PyResult<Value> {
    RESUMED.with(|r| r.set(true));
    Ok(Value::None)
}

/// Registra a nativa no módulo `_sys`.
pub(crate) fn register(builder: ModuleBuilder) -> ModuleBuilder {
    builder.func("_stdin_resume", resume)
}

impl Vm {
    /// O quadro que atende um [`SuspendRequest::Wait`]: `_net.wait_then_call` espera o descritor e repete a
    /// chamada que parou (o valor dela é o resultado da instrução); sem chamada, `_net.wait_stdin` só espera e a
    /// instrução (o `for` sobre o stdin) é repetida pelo laço.
    pub(crate) fn stdin_wait_frame(&mut self, fd: i64, retry: Option<(Value, Vec<Value>, Kw)>) -> PyResult<Callee> {
        let net = crate::modules::import_value(self, "_net")?;
        let (name, args) = match retry {
            Some((func, args, kwargs)) => {
                let mut named = Dict::default();
                for (key, value) in kwargs {
                    named.set(Value::str(key), value)?;
                }
                ("wait_then_call", vec![Value::Int(fd), func, Value::list(args), Value::dict(named)])
            }
            None => ("wait_stdin", vec![Value::Int(fd)]),
        };
        let wait = self.load_attr(&net, name)?;
        match self.enter_callable(&wait, args, Vec::new())? {
            Entered::Frame(callee) => Ok(callee),
            Entered::Done(_) => Err(internal("stdin wait is not a Python function")),
        }
    }
}

/// O buffer não basta para a operação e o descritor 0 ainda pode dar mais bytes.
struct Stall;

/// Lê um bloco do descritor 0 para o fim do buffer. O fim do arquivo (ou um erro) marca `raw_eof`.
fn fill(f: &mut PyFile) {
    if f.raw_eof {
        return;
    }
    let mut chunk = vec![0u8; CHUNK];
    match sys::read(Fd::STDIN, &mut chunk) {
        Ok(0) | Err(_) => f.raw_eof = true,
        Ok(n) => f.raw.extend_from_slice(&chunk[..n]),
    }
}

/// Com `threading` em uso, espera o descritor 0 ter o que ler rodando as outras threads (sem outra coisa a
/// rodar, ou num descritor não bloqueante, volta de imediato e a leitura segue direto para o kernel). Numa
/// instrução repetível a espera sai da nativa: o pedido [`SuspendRequest::Wait`] faz o laço rodá-la em quadros
/// (onde a troca de thread vale) e repetir a instrução; a nativa aninhada só serve quando não há como repetir.
fn wait_readable() -> PyResult<()> {
    let Some(mut vm) = current() else { return Ok(()) };
    if !vm.modules.borrow().contains_key("threading") {
        return Ok(());
    }
    if RESUMED.with(|r| r.replace(false)) {
        return Ok(());
    }
    if REPLAYABLE.with(Cell::get) && vm.rust_nest.get() == 1 {
        return Err(suspend(SuspendRequest::Wait(i64::from(Fd::STDIN.0))));
    }
    let net = crate::modules::import_value(&mut vm, "_net")?;
    let wait = vm.load_attr(&net, "wait_readable")?;
    vm.call(&wait, vec![Value::Int(i64::from(Fd::STDIN.0))], Vec::new()).map(|_| ())
}

/// Roda `op` sobre o arquivo até ela ter o que precisa, buscando mais bytes do descritor 0 a cada `Stall`.
fn drive<T>(file: &Rc<RefCell<Native>>, op: impl Fn(&mut PyFile) -> Result<T, Stall>) -> PyResult<T> {
    loop {
        let step = match &mut *file.borrow_mut() {
            Native::File(f) => op(f),
            _ => return Err(internal("stdin is not a file")),
        };
        if let Ok(done) = step {
            // Consumiu entrada: a instrução não se repete mais (o que ela leu já saiu do buffer).
            REPLAYABLE.with(|r| r.set(false));
            // O fim do arquivo não é pegajoso: como o `BufferedReader` do CPython, a leitura seguinte volta ao
            // descritor (um `dup2` pode ter posto outro arquivo no 0, e o terminal segue depois do ^D).
            if let Native::File(f) = &mut *file.borrow_mut()
                && f.raw_eof
                && f.raw.is_empty()
            {
                f.raw_eof = false;
            }
            return Ok(done);
        }
        wait_readable()?;
        if let Native::File(f) = &mut *file.borrow_mut() {
            fill(f);
        }
    }
}

/// UTF-8 tolerante: bytes inválidos viram U+FFFD.
fn decode(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

/// Próxima linha de texto, com o `\n` do fim (se a entrada tinha terminador). `None` no fim do arquivo. O
/// `sys.stdin` do CPython no POSIX é aberto com `newline='\n'`: só o `\n` termina linha e o `\r` passa intacto.
pub fn text_line(file: &Rc<RefCell<Native>>) -> PyResult<Option<String>> {
    drive(file, try_text_line)
}

fn try_text_line(f: &mut PyFile) -> Result<Option<String>, Stall> {
    if let Some(i) = f.raw.iter().position(|&b| b == b'\n') {
        let line = decode(&f.raw[..=i]);
        f.raw.drain(..=i);
        return Ok(Some(line));
    }
    if !f.raw_eof {
        return Err(Stall);
    }
    if f.raw.is_empty() {
        return Ok(None);
    }
    let rest = decode(&f.raw);
    f.raw.clear();
    Ok(Some(rest))
}

/// Tudo o que falta até o fim do arquivo, como texto.
pub fn text_all(file: &Rc<RefCell<Native>>) -> PyResult<String> {
    drive(file, |f| {
        if !f.raw_eof {
            return Err(Stall);
        }
        let text = decode(&f.raw);
        f.raw.clear();
        Ok(text)
    })
}

/// Até `n` caracteres de texto (menos só no fim do arquivo).
pub fn text_chars(file: &Rc<RefCell<Native>>, n: usize) -> PyResult<String> {
    drive(file, |f| {
        let (mut text, used, count) = decode_prefix(&f.raw, n);
        if count < n && !f.raw_eof {
            return Err(Stall);
        }
        if count < n && used < f.raw.len() {
            // no fim do arquivo, uma sequência UTF-8 cortada vira U+FFFD
            text.push('\u{fffd}');
            f.raw.clear();
        } else {
            f.raw.drain(..used);
        }
        Ok(text)
    })
}

/// Decodifica no máximo `n` caracteres do início de `raw`: devolve o texto, os bytes que ele ocupou e
/// quantos caracteres saíram. Uma sequência UTF-8 cortada no fim do buffer não conta (espera mais bytes).
fn decode_prefix(raw: &[u8], n: usize) -> (String, usize, usize) {
    let mut out = String::new();
    let mut used = 0;
    let mut count = 0;
    while count < n && used < raw.len() {
        match std::str::from_utf8(&raw[used..]) {
            Ok(valid) => {
                for c in valid.chars().take(n - count) {
                    out.push(c);
                    used += c.len_utf8();
                    count += 1;
                }
                break;
            }
            Err(e) => {
                let good = e.valid_up_to();
                // até `good` o trecho é UTF-8 válido, por contrato de `valid_up_to`
                let valid = std::str::from_utf8(&raw[used..used + good]).unwrap_or("");
                for c in valid.chars() {
                    if count == n {
                        return (out, used, count);
                    }
                    out.push(c);
                    used += c.len_utf8();
                    count += 1;
                }
                match e.error_len() {
                    Some(bad) => {
                        if count == n {
                            break;
                        }
                        out.push('\u{fffd}');
                        used += bad;
                        count += 1;
                    }
                    // sequência incompleta no fim do buffer: ainda pode completar
                    None => break,
                }
            }
        }
    }
    (out, used, count)
}

/// Bytes exatos: `n` deles (bloqueia até juntar, ou até o fim do arquivo) ou tudo o que falta.
pub fn bytes_read(file: &Rc<RefCell<Native>>, take: Option<usize>) -> PyResult<Vec<u8>> {
    drive(file, |f| match take {
        Some(n) if f.raw.len() >= n || f.raw_eof => {
            let n = n.min(f.raw.len());
            Ok(f.raw.drain(..n).collect())
        }
        None if f.raw_eof => Ok(std::mem::take(&mut f.raw)),
        _ => Err(Stall),
    })
}

/// O que já está disponível, esperando só se o buffer estiver vazio (`read1`).
pub fn bytes_read1(file: &Rc<RefCell<Native>>, take: Option<usize>) -> PyResult<Vec<u8>> {
    drive(file, |f| {
        if f.raw.is_empty() && !f.raw_eof {
            return Err(Stall);
        }
        let n = take.map_or(f.raw.len(), |n| n.min(f.raw.len()));
        Ok(f.raw.drain(..n).collect())
    })
}

/// Próxima linha em bytes (até `\n` inclusive, sem tradução).
pub fn bytes_line(file: &Rc<RefCell<Native>>) -> PyResult<Vec<u8>> {
    drive(file, |f| {
        if let Some(i) = f.raw.iter().position(|&b| b == b'\n') {
            return Ok(f.raw.drain(..=i).collect());
        }
        if f.raw_eof {
            return Ok(std::mem::take(&mut f.raw));
        }
        Err(Stall)
    })
}
