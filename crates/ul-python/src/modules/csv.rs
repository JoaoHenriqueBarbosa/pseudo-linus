//! Módulo `_csv` (porte de `Modules/_csv.c` do CPython 3.13): dialeto, leitor e escritor.
//!
//! O leitor é a máquina de estados de `parse_process_char`; o escritor segue `csv_writerow` e
//! `join_append_data`. `QUOTE_NONNUMERIC` no leitor não converte campos para `float` (devolve
//! strings), e `QUOTE_STRINGS`/`QUOTE_NOTNULL` não distinguem campo vazio de `None` na leitura.

use crate::object::{to_str, Value};

pub const QUOTE_MINIMAL: i32 = 0;
pub const QUOTE_ALL: i32 = 1;
pub const QUOTE_NONNUMERIC: i32 = 2;
pub const QUOTE_NONE: i32 = 3;
pub const QUOTE_STRINGS: i32 = 4;
pub const QUOTE_NOTNULL: i32 = 5;

/// Dialeto de CSV (padrão: `excel`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dialect {
    pub delimiter: char,
    pub quotechar: Option<char>,
    pub escapechar: Option<char>,
    pub doublequote: bool,
    pub skipinitialspace: bool,
    pub lineterminator: String,
    pub quoting: i32,
    pub strict: bool,
}

impl Default for Dialect {
    fn default() -> Self {
        Dialect {
            delimiter: ',',
            quotechar: Some('"'),
            escapechar: None,
            doublequote: true,
            skipinitialspace: false,
            lineterminator: "\r\n".to_string(),
            quoting: QUOTE_MINIMAL,
            strict: false,
        }
    }
}

/// `_csv.Error: <msg>`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CsvError {
    pub msg: String,
}

fn err<T>(msg: impl Into<String>) -> Result<T, CsvError> {
    Err(CsvError { msg: msg.into() })
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum State {
    StartRecord,
    StartField,
    EscapedChar,
    InField,
    InQuotedField,
    EscapeInQuotedField,
    QuoteInQuotedField,
    EatCrnl,
    AfterEscapedCrnl,
}

/// Fim de linha sintético (`EOL` do C, o `'\0'` que fecha cada linha).
const EOL: Option<char> = None;

/// Leitor (`_csv.reader`).
#[derive(Debug)]
pub struct Reader {
    dialect: Dialect,
    state: State,
    fields: Vec<String>,
    field: String,
    pub line_num: usize,
}

impl Reader {
    pub fn new(dialect: Dialect) -> Reader {
        Reader { dialect, state: State::StartRecord, fields: Vec::new(), field: String::new(), line_num: 0 }
    }

    fn save_field(&mut self) {
        let f = std::mem::take(&mut self.field);
        self.fields.push(f);
    }

    fn is_quote(&self, c: char) -> bool {
        self.dialect.quoting != QUOTE_NONE && self.dialect.quotechar == Some(c)
    }

    fn is_escape(&self, c: char) -> bool {
        self.dialect.escapechar == Some(c)
    }

    /// `parse_process_char`; `c == None` é o fim de linha.
    fn process_char(&mut self, c: Option<char>) -> Result<(), CsvError> {
        let is_nl = matches!(c, Some('\n') | Some('\r'));
        let mut state = self.state;
        // Os `fallthru` do C viram laços de reentrada no estado seguinte.
        loop {
            match state {
                State::StartRecord => {
                    if c == EOL {
                        // Linha vazia: registro vazio.
                        break;
                    } else if is_nl {
                        state = State::EatCrnl;
                        break;
                    }
                    state = State::StartField;
                    continue;
                }
                State::StartField => {
                    if is_nl || c == EOL {
                        self.save_field();
                        state = if c == EOL { State::StartRecord } else { State::EatCrnl };
                    } else {
                        let ch = c.unwrap();
                        if self.is_quote(ch) {
                            state = State::InQuotedField;
                        } else if self.is_escape(ch) {
                            state = State::EscapedChar;
                        } else if ch == ' ' && self.dialect.skipinitialspace {
                            // ignora o espaço inicial
                        } else if ch == self.dialect.delimiter {
                            self.save_field();
                        } else {
                            self.field.push(ch);
                            state = State::InField;
                        }
                    }
                    break;
                }
                State::EscapedChar => {
                    if is_nl {
                        self.field.push(c.unwrap());
                        state = State::AfterEscapedCrnl;
                        break;
                    }
                    self.field.push(c.unwrap_or('\n'));
                    state = State::InField;
                    break;
                }
                State::AfterEscapedCrnl => {
                    if c == EOL {
                        break;
                    }
                    state = State::InField;
                    continue;
                }
                State::InField => {
                    if is_nl || c == EOL {
                        self.save_field();
                        state = if c == EOL { State::StartRecord } else { State::EatCrnl };
                    } else {
                        let ch = c.unwrap();
                        if self.is_escape(ch) {
                            state = State::EscapedChar;
                        } else if ch == self.dialect.delimiter {
                            self.save_field();
                            state = State::StartField;
                        } else {
                            self.field.push(ch);
                        }
                    }
                    break;
                }
                State::InQuotedField => {
                    if let Some(ch) = c {
                        if self.is_escape(ch) {
                            state = State::EscapeInQuotedField;
                        } else if self.is_quote(ch) {
                            state = if self.dialect.doublequote { State::QuoteInQuotedField } else { State::InField };
                        } else {
                            self.field.push(ch);
                        }
                    }
                    break;
                }
                State::EscapeInQuotedField => {
                    self.field.push(c.unwrap_or('\n'));
                    state = State::InQuotedField;
                    break;
                }
                State::QuoteInQuotedField => {
                    if let Some(ch) = c {
                        if self.is_quote(ch) {
                            self.field.push(ch);
                            state = State::InQuotedField;
                            break;
                        }
                        if ch == self.dialect.delimiter {
                            self.save_field();
                            state = State::StartField;
                            break;
                        }
                    }
                    if is_nl || c == EOL {
                        self.save_field();
                        state = if c == EOL { State::StartRecord } else { State::EatCrnl };
                    } else if !self.dialect.strict {
                        self.field.push(c.unwrap());
                        state = State::InField;
                    } else {
                        let ch = c.unwrap();
                        self.state = state;
                        return err(format!(
                            "'{}' expected after '{}'",
                            self.dialect.delimiter,
                            self.dialect.quotechar.unwrap_or(ch)
                        ));
                    }
                    break;
                }
                State::EatCrnl => {
                    if is_nl {
                        // continua comendo
                    } else if c == EOL {
                        state = State::StartRecord;
                    } else {
                        self.state = state;
                        return err(
                            "new-line character seen in unquoted field - do you need to open the file with newline=''?",
                        );
                    }
                    break;
                }
            }
        }
        self.state = state;
        Ok(())
    }

    /// Próximo registro (`Reader_iternext`). `Ok(None)` é o fim (`StopIteration`).
    pub fn next_row(
        &mut self,
        next_line: &mut dyn FnMut() -> Option<String>,
    ) -> Result<Option<Vec<String>>, CsvError> {
        // parse_reset
        self.fields.clear();
        self.field.clear();
        self.state = State::StartRecord;

        loop {
            let line = match next_line() {
                Some(l) => l,
                None => {
                    if !self.field.is_empty() || self.state == State::InQuotedField {
                        if self.dialect.strict {
                            return err("unexpected end of data");
                        }
                        self.save_field();
                        break;
                    }
                    return Ok(None);
                }
            };
            self.line_num += 1;
            for ch in line.chars() {
                self.process_char(Some(ch))?;
            }
            self.process_char(EOL)?;
            if self.state == State::StartRecord {
                break;
            }
        }
        Ok(Some(std::mem::take(&mut self.fields)))
    }
}

/// Acrescenta um campo ao registro (`join_append_data`), aspeando se preciso.
fn join_append(
    d: &Dialect,
    out: &mut String,
    field: &str,
    mut quoted: bool,
) -> Result<(), CsvError> {
    let mut body = String::new();
    for c in field.chars() {
        let special = c == d.delimiter
            || Some(c) == d.escapechar
            || Some(c) == d.quotechar
            || d.lineterminator.contains(c)
            || c == '\n'
            || c == '\r';
        if special {
            let mut want_escape = false;
            if d.quoting == QUOTE_NONE {
                want_escape = true;
            } else {
                if Some(c) == d.quotechar {
                    if d.doublequote {
                        body.push(c);
                    } else {
                        want_escape = true;
                    }
                } else if Some(c) == d.escapechar {
                    want_escape = true;
                }
                if !want_escape {
                    quoted = true;
                }
            }
            if want_escape {
                match d.escapechar {
                    Some(e) => body.push(e),
                    None => return err("need to escape, but no escapechar set"),
                }
            }
        }
        body.push(c);
    }
    if quoted {
        let q = match d.quotechar {
            Some(q) => q,
            None => return err("quotechar must be set if quoting enabled"),
        };
        out.push(q);
        out.push_str(&body);
        out.push(q);
    } else {
        out.push_str(&body);
    }
    Ok(())
}

/// `writer.writerow(fields)`: devolve a linha com o terminador.
pub fn writerow(dialect: &Dialect, fields: &[Value]) -> Result<String, CsvError> {
    if dialect.quoting != QUOTE_NONE && dialect.quotechar.is_none() && dialect.quoting == QUOTE_ALL {
        return err("quotechar must be set if quoting enabled");
    }
    let mut out = String::new();
    for (i, f) in fields.iter().enumerate() {
        if i > 0 {
            out.push(dialect.delimiter);
        }
        let is_number = matches!(f, Value::Int(_) | Value::Float(_) | Value::Bool(_));
        let quoted = match dialect.quoting {
            QUOTE_NONNUMERIC => !is_number,
            QUOTE_ALL => true,
            QUOTE_STRINGS => matches!(f, Value::Str(_)),
            QUOTE_NOTNULL => !matches!(f, Value::None),
            _ => false,
        };
        let text = match f {
            Value::None => String::new(),
            other => to_str(other),
        };
        join_append(dialect, &mut out, &text, quoted)?;
    }
    if !fields.is_empty() && out.is_empty() {
        if dialect.quoting == QUOTE_NONE {
            return err("single empty field record must be quoted");
        }
        join_append(dialect, &mut out, "", true)?;
    }
    out.push_str(&dialect.lineterminator);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read_all(d: Dialect, lines: &[&str]) -> Result<Vec<Vec<String>>, CsvError> {
        let mut it = lines.iter().map(|s| s.to_string());
        let mut r = Reader::new(d);
        let mut rows = Vec::new();
        while let Some(row) = r.next_row(&mut || it.next())? {
            rows.push(row);
        }
        Ok(rows)
    }

    fn strs(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn reads_simple() {
        let rows = read_all(Dialect::default(), &["a,b,c\r\n"]).unwrap();
        assert_eq!(rows, vec![strs(&["a", "b", "c"])]);
    }

    #[test]
    fn reads_quoted_with_doubled_quotes() {
        let rows = read_all(Dialect::default(), &["\"a,\"\"b\"\"\",c\n"]).unwrap();
        assert_eq!(rows, vec![strs(&["a,\"b\"", "c"])]);
    }

    #[test]
    fn reads_multiline_quoted_field() {
        let rows = read_all(Dialect::default(), &["\"a\n", "b\",c\n"]).unwrap();
        assert_eq!(rows, vec![strs(&["a\nb", "c"])]);
    }

    #[test]
    fn reads_empty_line_as_empty_record() {
        let rows = read_all(Dialect::default(), &["\n", "x\n"]).unwrap();
        assert_eq!(rows, vec![Vec::<String>::new(), strs(&["x"])]);
    }

    #[test]
    fn rejects_bare_cr_in_unquoted_field() {
        let e = read_all(Dialect::default(), &["a\rb"]).unwrap_err();
        assert_eq!(
            e.msg,
            "new-line character seen in unquoted field - do you need to open the file with newline=''?"
        );
    }

    #[test]
    fn strict_eof_in_quoted_field() {
        let d = Dialect { strict: true, ..Dialect::default() };
        let e = read_all(d, &["\"abc"]).unwrap_err();
        assert_eq!(e.msg, "unexpected end of data");
        let rows = read_all(Dialect::default(), &["\"abc"]).unwrap();
        assert_eq!(rows, vec![strs(&["abc"])]);
    }

    #[test]
    fn writes_mixed_row() {
        let row = vec![
            Value::str("a"),
            Value::str("b,c"),
            Value::str("d\"e"),
            Value::str(""),
            Value::None,
            Value::Int(1),
            Value::Float(2.5),
        ];
        assert_eq!(writerow(&Dialect::default(), &row).unwrap(), "a,\"b,c\",\"d\"\"e\",,,1,2.5\r\n");
    }

    #[test]
    fn writes_single_empty_field_quoted() {
        assert_eq!(writerow(&Dialect::default(), &[Value::str("")]).unwrap(), "\"\"\r\n");
    }

    #[test]
    fn writes_empty_row() {
        assert_eq!(writerow(&Dialect::default(), &[]).unwrap(), "\r\n");
    }
}
