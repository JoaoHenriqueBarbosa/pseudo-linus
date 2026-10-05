//! O texto de um traceback do CPython 3.13, com os frames que o `python3 -c` mostra (a fonte da
//! linha e, nas expressões, a marca `~~~^^^` do 3.13).

/// `json/__init__.py` da biblioteca padrão do Debian 13.
pub const JSON_INIT: &str = "/usr/lib/python3.13/json/__init__.py";
/// `json/decoder.py` da biblioteca padrão do Debian 13.
pub const JSON_DECODER: &str = "/usr/lib/python3.13/json/decoder.py";

/// Um traceback em construção.
#[derive(Debug)]
pub struct Traceback {
    text: String,
}

impl Default for Traceback {
    fn default() -> Traceback {
        Traceback::new()
    }
}

impl Traceback {
    pub fn new() -> Traceback {
        Traceback { text: "Traceback (most recent call last):\n".to_string() }
    }

    fn header(&mut self, file: &str, line: u32, func: &str) {
        self.text.push_str(&format!("  File \"{file}\", line {line}, in {func}\n"));
    }

    /// Frame sem fonte (módulo congelado, como `<frozen codecs>`).
    pub fn bare(mut self, file: &str, line: u32, func: &str) -> Traceback {
        self.header(file, line, func);
        self
    }

    /// Frame com a linha de fonte sem marca (um `raise`). `source` já vem com a indentação e as
    /// quebras de linha que o Python mostra.
    pub fn plain(mut self, file: &str, line: u32, func: &str, source: &str) -> Traceback {
        self.header(file, line, func);
        self.text.push_str(source);
        self
    }

    /// Frame com a marca do 3.13 sob a expressão `expr` da linha `src`: `~` nos primeiros `split`
    /// caracteres e `^` no resto. A fonte é ASCII, então byte e coluna coincidem.
    pub fn marked(mut self, file: &str, line: u32, func: &str, src: &str, expr: &str, split: usize) -> Traceback {
        self.header(file, line, func);
        let col = src.find(expr).unwrap_or(0);
        self.text.push_str(&format!("    {src}\n"));
        self.text.push_str(&" ".repeat(4 + col));
        self.text.push_str(&"~".repeat(split));
        self.text.push_str(&"^".repeat(expr.len().saturating_sub(split)));
        self.text.push('\n');
        self
    }

    /// Fecha com a linha da exceção.
    pub fn finish(mut self, class: &str, message: &str) -> String {
        self.text.push_str(class);
        self.text.push_str(": ");
        self.text.push_str(message);
        self.text.push('\n');
        self.text
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn marks_the_expression() {
        let tb = Traceback::new().marked(
            "<string>",
            4,
            "<module>",
            "w.writerow(json.loads(line))",
            "json.loads(line)",
            10,
        );
        let text = tb.finish("ValueError", "x");
        let expected = "Traceback (most recent call last):\n  File \"<string>\", line 4, in <module>\n    w.writerow(json.loads(line))\n               ~~~~~~~~~~^^^^^^\nValueError: x\n";
        assert_eq!(text, expected);
    }
}
