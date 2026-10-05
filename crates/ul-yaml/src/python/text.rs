//! O texto como o CPython 3.13 o vê nos arquivos e nos fluxos padrão.
//!
//! - `open(nome, newline='', encoding='utf-8')`: UTF-8 estrito, decodificado em blocos de 8192
//!   bytes pelo `TextIOWrapper`, linhas cortadas em `\n`, `\r` ou `\r\n` sem traduzir nada. Um erro
//!   de decodificação aparece quando a leitura da linha precisa do bloco que o contém, e a posição
//!   que a mensagem traz é relativa ao bloco (com a cauda incompleta do bloco anterior na frente).
//! - `sys.stdin` e `sys.stdout` em C.UTF-8: UTF-8 com `surrogateescape` e quebras universais na
//!   entrada.
//!
//! Um `str` do Python pode guardar um substituto isolado (U+D800..U+DFFF), coisa que um `char` do
//! Rust não guarda. Aqui um substituto vira o ponto correspondente nos últimos 2048 pontos do
//! plano 16 (U+10F800..U+10FFFF), uso privado que nenhum texto de verdade traz. Contagem de
//! caracteres, posições e mensagens de erro seguem iguais às do Python.

/// Tamanho do bloco de leitura do `TextIOWrapper` (`_CHUNK_SIZE`).
const CHUNK: usize = 8192;

const STAND_IN_BASE: u32 = 0x10F800;
const SURROGATE_BASE: u32 = 0xD800;

/// O `char` que representa o substituto isolado `surrogate` (U+D800..U+DFFF).
pub fn surrogate_to_char(surrogate: u32) -> char {
    char::from_u32(STAND_IN_BASE + (surrogate - SURROGATE_BASE)).unwrap_or('\u{fffd}')
}

/// O substituto isolado que `c` representa, se for um deles.
pub fn as_surrogate(c: char) -> Option<u32> {
    let v = c as u32;
    if v >= STAND_IN_BASE { Some(v - STAND_IN_BASE + SURROGATE_BASE) } else { None }
}

/// UTF-8 com `errors='surrogateescape'`: cada byte de uma sequência inválida vira o substituto
/// U+DC80..U+DCFF correspondente.
pub fn decode_surrogateescape(data: &[u8]) -> Vec<char> {
    let mut out: Vec<char> = Vec::with_capacity(data.len());
    let mut rest = data;
    loop {
        match std::str::from_utf8(rest) {
            Ok(s) => {
                out.extend(s.chars());
                return out;
            }
            Err(e) => {
                let valid = e.valid_up_to();
                out.extend(std::str::from_utf8(&rest[..valid]).unwrap_or_default().chars());
                let bad = e.error_len().unwrap_or(rest.len() - valid);
                for &b in &rest[valid..valid + bad] {
                    out.push(surrogate_to_char(0xDC00 + u32::from(b)));
                }
                rest = &rest[valid + bad..];
            }
        }
    }
}

/// Quebras universais do modo texto (`newline=None`): `\r\n` e `\r` viram `\n`.
pub fn translate_newlines(text: Vec<char>) -> Vec<char> {
    let mut out: Vec<char> = Vec::with_capacity(text.len());
    let mut i = 0;
    while i < text.len() {
        if text[i] == '\r' {
            out.push('\n');
            if text.get(i + 1) == Some(&'\n') {
                i += 1;
            }
        } else {
            out.push(text[i]);
        }
        i += 1;
    }
    out
}

/// Um trecho de substitutos que o UTF-8 não codifica: `[start, end)` em caracteres, e o primeiro
/// substituto (pra mensagem do erro de um caractere só).
#[derive(Debug)]
pub struct EncodeError {
    pub start: usize,
    pub end: usize,
    pub first: u32,
}

impl EncodeError {
    /// A mensagem do `UnicodeEncodeError`.
    pub fn message(&self) -> String {
        if self.end - self.start == 1 {
            format!(
                "'utf-8' codec can't encode character '\\u{:04x}' in position {}: surrogates not allowed",
                self.first, self.start
            )
        } else {
            format!(
                "'utf-8' codec can't encode characters in position {}-{}: surrogates not allowed",
                self.start,
                self.end - 1
            )
        }
    }
}

/// UTF-8 com `errors='surrogateescape'`: os substitutos U+DC80..U+DCFF voltam a ser o byte que
/// escaparam; qualquer outro substituto é erro, e o erro cobre a sequência inteira de substitutos
/// seguidos, como o codificador do CPython.
pub fn encode_surrogateescape(text: &str) -> Result<Vec<u8>, EncodeError> {
    let chars: Vec<char> = text.chars().collect();
    let mut out: Vec<u8> = Vec::with_capacity(text.len());
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if as_surrogate(c).is_none() {
            let mut buf = [0u8; 4];
            out.extend_from_slice(c.encode_utf8(&mut buf).as_bytes());
            i += 1;
            continue;
        }
        let start = i;
        while i < chars.len() && as_surrogate(chars[i]).is_some() {
            i += 1;
        }
        let run = &chars[start..i];
        let escapable = run.iter().all(|&r| matches!(as_surrogate(r), Some(0xDC80..=0xDCFF)));
        if !escapable {
            let first = as_surrogate(run[0]).unwrap_or(SURROGATE_BASE);
            return Err(EncodeError { start, end: i, first });
        }
        for &r in run {
            let s = as_surrogate(r).unwrap_or(0xDC80);
            out.push((s - 0xDC00) as u8);
        }
    }
    Ok(out)
}

/// Falha de decodificação UTF-8 estrita: o texto do `UnicodeDecodeError`.
#[derive(Debug)]
pub struct DecodeError {
    pub message: String,
}

fn decode_error(buf: &[u8], start: usize, len: usize, reason: &str) -> DecodeError {
    let message = if len == 1 {
        format!("'utf-8' codec can't decode byte 0x{:02x} in position {start}: {reason}", buf[start])
    } else {
        format!("'utf-8' codec can't decode bytes in position {start}-{}: {reason}", start + len - 1)
    };
    DecodeError { message }
}

/// Decodificador incremental estrito: devolve o texto e quantos bytes consumiu (a cauda incompleta
/// fica pro próximo bloco, a não ser que `last` diga que não há próximo).
fn decode_strict(buf: &[u8], last: bool) -> Result<(String, usize), DecodeError> {
    match std::str::from_utf8(buf) {
        Ok(s) => Ok((s.to_string(), buf.len())),
        Err(e) => {
            let valid = e.valid_up_to();
            match e.error_len() {
                None if !last => {
                    let text = std::str::from_utf8(&buf[..valid]).unwrap_or_default().to_string();
                    Ok((text, valid))
                }
                None => Err(decode_error(buf, valid, buf.len() - valid, "unexpected end of data")),
                Some(n) => {
                    let reason =
                        if (0xc2..=0xf4).contains(&buf[valid]) { "invalid continuation byte" } else { "invalid start byte" };
                    Err(decode_error(buf, valid, n, reason))
                }
            }
        }
    }
}

/// As linhas de um arquivo aberto com `newline=''` e `encoding='utf-8'`, entregues sob demanda.
#[derive(Debug)]
pub struct LineReader<'a> {
    data: &'a [u8],
    /// Próximo byte a entregar ao decodificador.
    pos: usize,
    /// Cauda incompleta (sequência UTF-8 cortada) do último bloco.
    pending: Vec<u8>,
    /// Texto decodificado; as linhas ainda não entregues começam em `start`.
    text: Vec<char>,
    start: usize,
    /// Até onde já se procurou fim de linha a partir de `start`.
    scan: usize,
    /// Um `\r` no fim do último bloco, retido até saber se o próximo caractere é `\n`.
    held_cr: bool,
    eof: bool,
}

impl<'a> LineReader<'a> {
    pub fn new(data: &'a [u8]) -> LineReader<'a> {
        LineReader { data, pos: 0, pending: Vec::new(), text: Vec::new(), start: 0, scan: 0, held_cr: false, eof: false }
    }

    /// A próxima linha, com o terminador (`\n`, `\r` ou `\r\n`) que ela tiver. `Ok(None)` no fim.
    pub fn next_line(&mut self) -> Result<Option<String>, DecodeError> {
        loop {
            if let Some(end) = self.line_end() {
                let line: String = self.text[self.start..end].iter().collect();
                self.start = end;
                self.scan = end;
                return Ok(Some(line));
            }
            if self.eof {
                if self.start >= self.text.len() {
                    return Ok(None);
                }
                let line: String = self.text[self.start..].iter().collect();
                self.start = self.text.len();
                self.scan = self.start;
                return Ok(Some(line));
            }
            self.read_chunk()?;
        }
    }

    /// Posição (exclusiva) do fim da primeira linha completa a partir de `start`.
    fn line_end(&mut self) -> Option<usize> {
        let mut i = self.scan;
        while i < self.text.len() {
            match self.text[i] {
                '\n' => return Some(i + 1),
                '\r' => {
                    if i + 1 < self.text.len() {
                        return Some(if self.text[i + 1] == '\n' { i + 2 } else { i + 1 });
                    }
                    // `\r` no fim do texto: nos blocos ele fica retido, então só chega aqui no fim
                    // da entrada.
                    if self.eof {
                        return Some(i + 1);
                    }
                    self.scan = i;
                    return None;
                }
                _ => i += 1,
            }
        }
        self.scan = i;
        None
    }

    /// Lê e decodifica o próximo bloco; depois do último, um bloco vazio que encerra a decodificação.
    fn read_chunk(&mut self) -> Result<(), DecodeError> {
        if self.start > 0 {
            self.text.drain(..self.start);
            self.scan -= self.start;
            self.start = 0;
        }
        let end = (self.pos + CHUNK).min(self.data.len());
        let last = self.pos >= self.data.len();
        let mut buf = std::mem::take(&mut self.pending);
        buf.extend_from_slice(&self.data[self.pos..end]);
        self.pos = end;
        let (decoded, consumed) = decode_strict(&buf, last)?;
        self.pending = buf[consumed..].to_vec();
        if self.held_cr {
            self.text.push('\r');
            self.held_cr = false;
        }
        let mut chars: Vec<char> = decoded.chars().collect();
        if !last && chars.last() == Some(&'\r') {
            chars.pop();
            self.held_cr = true;
        }
        self.text.extend(chars);
        if last {
            self.eof = true;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(data: &[u8]) -> Vec<String> {
        let mut reader = LineReader::new(data);
        let mut out = Vec::new();
        while let Some(line) = reader.next_line().unwrap() {
            out.push(line);
        }
        out
    }

    #[test]
    fn splits_on_universal_newlines_without_translating() {
        assert_eq!(lines(b"a\r\nb\rc\n\nd"), vec!["a\r\n", "b\r", "c\n", "\n", "d"]);
        assert_eq!(lines(b"a\r"), vec!["a\r"]);
    }

    #[test]
    fn decode_error_position_is_relative_to_the_chunk() {
        let mut data = vec![b'a'; 8192];
        data.push(0xff);
        let err = LineReader::new(&data).next_line().unwrap_err();
        assert_eq!(err.message, "'utf-8' codec can't decode byte 0xff in position 0: invalid start byte");
        let err = LineReader::new(b"\xe2\x82").next_line().unwrap_err();
        assert_eq!(err.message, "'utf-8' codec can't decode bytes in position 0-1: unexpected end of data");
    }

    #[test]
    fn surrogateescape_round_trip() {
        let chars = decode_surrogateescape(b"a\xffb");
        assert_eq!(chars.len(), 3);
        assert_eq!(as_surrogate(chars[1]), Some(0xDCFF));
        let text: String = chars.iter().collect();
        assert_eq!(encode_surrogateescape(&text).unwrap(), b"a\xffb");
        let lone = surrogate_to_char(0xD800).to_string();
        assert_eq!(encode_surrogateescape(&lone).unwrap_err().message(), "'utf-8' codec can't encode character '\\ud800' in position 0: surrogates not allowed");
    }
}
