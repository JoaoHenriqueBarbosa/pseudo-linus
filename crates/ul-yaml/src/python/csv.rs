//! O módulo `csv` do CPython 3.13 (`Modules/_csv.c`) com o dialeto `excel`: vírgula, aspas duplas
//! dobradas, `QUOTE_MINIMAL`, sem `escapechar`, sem `skipinitialspace`, não estrito.
//!
//! A máquina de estados do leitor é a de `parse_process_char`. Com esse dialeto os estados que
//! dependem de `escapechar` (`ESCAPED_CHAR`, `ESCAPE_IN_QUOTED_FIELD`, `AFTER_ESCAPED_CRNL`) não
//! são alcançáveis, e o ramo estrito de `QUOTE_IN_QUOTED_FIELD` e o ignorar espaço inicial de
//! `START_FIELD` ficam desligados; por isso não aparecem aqui.

/// `csv.field_size_limit()` de fábrica.
pub const FIELD_LIMIT: usize = 131_072;

const DELIMITER: char = ',';
const QUOTE_CHAR: char = '"';

/// Um `_csv.Error`.
#[derive(Debug)]
pub struct CsvError(pub String);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum State {
    StartRecord,
    StartField,
    InField,
    InQuotedField,
    QuoteInQuotedField,
    EatCrnl,
}

/// O `csv.reader`: recebe as linhas como o iterador do arquivo as entrega (com o terminador) e
/// devolve um registro quando ele fecha.
#[derive(Debug)]
pub struct Reader {
    state: State,
    fields: Vec<String>,
    field: String,
    field_len: usize,
}

impl Default for Reader {
    fn default() -> Reader {
        Reader::new()
    }
}

impl Reader {
    pub fn new() -> Reader {
        Reader { state: State::StartRecord, fields: Vec::new(), field: String::new(), field_len: 0 }
    }

    /// `parse_save_field`.
    fn save_field(&mut self) {
        self.fields.push(std::mem::take(&mut self.field));
        self.field_len = 0;
    }

    /// `parse_add_char`.
    fn add_char(&mut self, c: char) -> Result<(), CsvError> {
        if self.field_len >= FIELD_LIMIT {
            return Err(CsvError(format!("field larger than field limit ({FIELD_LIMIT})")));
        }
        self.field.push(c);
        self.field_len += 1;
        Ok(())
    }

    /// O trabalho de `START_FIELD`.
    fn start_field(&mut self, c: Option<char>) -> Result<(), CsvError> {
        match c {
            None => {
                self.save_field();
                self.state = State::StartRecord;
            }
            Some('\n' | '\r') => {
                self.save_field();
                self.state = State::EatCrnl;
            }
            Some(QUOTE_CHAR) => self.state = State::InQuotedField,
            Some(DELIMITER) => self.save_field(),
            Some(ch) => {
                self.add_char(ch)?;
                self.state = State::InField;
            }
        }
        Ok(())
    }

    /// `parse_process_char`: `None` é o `EOL` que fecha cada linha.
    fn process(&mut self, c: Option<char>) -> Result<(), CsvError> {
        match self.state {
            State::StartRecord => match c {
                None => {}
                Some('\n' | '\r') => self.state = State::EatCrnl,
                Some(_) => {
                    self.state = State::StartField;
                    self.start_field(c)?;
                }
            },
            State::StartField => self.start_field(c)?,
            State::InField => match c {
                None => {
                    self.save_field();
                    self.state = State::StartRecord;
                }
                Some('\n' | '\r') => {
                    self.save_field();
                    self.state = State::EatCrnl;
                }
                Some(DELIMITER) => {
                    self.save_field();
                    self.state = State::StartField;
                }
                Some(ch) => self.add_char(ch)?,
            },
            State::InQuotedField => match c {
                None => {}
                Some(QUOTE_CHAR) => self.state = State::QuoteInQuotedField,
                Some(ch) => self.add_char(ch)?,
            },
            State::QuoteInQuotedField => match c {
                Some(QUOTE_CHAR) => {
                    self.add_char(QUOTE_CHAR)?;
                    self.state = State::InQuotedField;
                }
                Some(DELIMITER) => {
                    self.save_field();
                    self.state = State::StartField;
                }
                None => {
                    self.save_field();
                    self.state = State::StartRecord;
                }
                Some('\n' | '\r') => {
                    self.save_field();
                    self.state = State::EatCrnl;
                }
                Some(ch) => {
                    self.add_char(ch)?;
                    self.state = State::InField;
                }
            },
            State::EatCrnl => match c {
                Some('\n' | '\r') => {}
                None => self.state = State::StartRecord,
                Some(_) => {
                    return Err(CsvError(
                        "new-line character seen in unquoted field - do you need to open the file with newline=''?"
                            .to_string(),
                    ));
                }
            },
        }
        Ok(())
    }

    /// O registro pronto, se a máquina voltou a `START_RECORD`.
    fn take_record(&mut self) -> Option<Vec<String>> {
        if self.state == State::StartRecord { Some(std::mem::take(&mut self.fields)) } else { None }
    }

    /// Alimenta uma linha (com o terminador que ela tiver) e devolve o registro, se fechou. Uma
    /// linha dentro de um campo entre aspas deixa o registro aberto pra próxima.
    pub fn feed_line(&mut self, line: &str) -> Result<Option<Vec<String>>, CsvError> {
        for ch in line.chars() {
            self.process(Some(ch))?;
        }
        self.process(None)?;
        Ok(self.take_record())
    }

    /// Fim da entrada: um campo entre aspas que ficou aberto, ou com texto, ainda fecha o registro
    /// (o `Reader_iternext` não estrito salva o campo e entrega).
    pub fn finish(&mut self) -> Option<Vec<String>> {
        if self.field_len != 0 || self.state == State::InQuotedField {
            self.save_field();
            self.state = State::StartRecord;
            return Some(std::mem::take(&mut self.fields));
        }
        None
    }
}

/// `csv.writer(...).writerow(fields)` com `QUOTE_MINIMAL`: devolve o texto que o `writer` entrega
/// ao `write` do arquivo (registro mais o `lineterminator`).
pub fn format_row(fields: &[String], lineterminator: &str) -> String {
    let mut rec = String::new();
    for (i, field) in fields.iter().enumerate() {
        if i > 0 {
            rec.push(DELIMITER);
        }
        let mut quoted = false;
        let mut body = String::with_capacity(field.len());
        for c in field.chars() {
            if c == DELIMITER || c == QUOTE_CHAR || c == '\n' || c == '\r' || lineterminator.contains(c) {
                if c == QUOTE_CHAR {
                    // doublequote
                    body.push(QUOTE_CHAR);
                }
                quoted = true;
            }
            body.push(c);
        }
        if quoted {
            rec.push(QUOTE_CHAR);
            rec.push_str(&body);
            rec.push(QUOTE_CHAR);
        } else {
            rec.push_str(&body);
        }
    }
    // Um registro de um campo só, vazio, precisa de aspas pra não virar linha em branco.
    if !fields.is_empty() && rec.is_empty() {
        rec.push(QUOTE_CHAR);
        rec.push(QUOTE_CHAR);
    }
    rec.push_str(lineterminator);
    rec
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read_all(lines: &[&str]) -> Vec<Vec<String>> {
        let mut r = Reader::new();
        let mut rows = Vec::new();
        for l in lines {
            if let Some(row) = r.feed_line(l).unwrap() {
                rows.push(row);
            }
        }
        if let Some(row) = r.finish() {
            rows.push(row);
        }
        rows
    }

    #[test]
    fn reads_quotes_and_blank_lines() {
        let rows = read_all(&["a,\"say \"\"hi\"\"\",b\n", "\n", "\"x\"y,z\n"]);
        assert_eq!(rows[0], vec!["a", "say \"hi\"", "b"]);
        assert!(rows[1].is_empty());
        assert_eq!(rows[2], vec!["xy", "z"]);
    }

    #[test]
    fn multiline_and_unterminated() {
        let rows = read_all(&["1,\"crlf\r\n", "inside\"\n"]);
        assert_eq!(rows[0], vec!["1", "crlf\r\ninside"]);
        let rows = read_all(&["a,\"never closed\n"]);
        assert_eq!(rows[0], vec!["a", "never closed\n"]);
    }

    #[test]
    fn writes_minimal() {
        let f = |v: &[&str]| v.iter().map(|s| (*s).to_string()).collect::<Vec<_>>();
        assert_eq!(format_row(&f(&["x,y", "say \"hi\"", ""]), "\n"), "\"x,y\",\"say \"\"hi\"\"\",\n");
        assert_eq!(format_row(&f(&[""]), "\n"), "\"\"\n");
        assert_eq!(format_row(&f(&[]), "\r\n"), "\r\n");
        assert_eq!(format_row(&f(&["", ""]), "\n"), ",\n");
    }
}
