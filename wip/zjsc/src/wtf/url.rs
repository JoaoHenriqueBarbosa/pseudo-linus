//! Porte parcial de `WTF/wtf/URL.{h,cpp}`, só o que o `SourceOrigin` e o `SourceProvider` usam:
//! `isNull`, `string`, `host`, `strippedForUseAsReport` e `fileSystemPath` para `file://`.
//!
//! Sem o `URLParser.cpp`: o C++ preenche `m_isValid`, `m_userStart`, `m_hostEnd` etc. ao analisar e
//! canonizar a entrada. Aqui a URL guarda a string como veio e os índices saem de uma varredura de
//! URL absoluta já canônica (`esquema:` seguido, ou não, de `//autoridade`). Entrada que não começa
//! com esquema fica inválida, como no C++ (`host` vazio, `strippedForUseAsReport` devolve a string).
//! Entrada não canônica (maiúsculas no esquema ou no host, `..` no caminho, escapes por fazer) NÃO é
//! canonizada: divergência conhecida até o `URLParser` ser portado.

use crate::wtf::text::wtf_string::String as WtfString;

/// `class URL`.
#[derive(Clone, Debug, Default)]
pub struct URL {
    string: WtfString,
    is_valid: bool,
    user_start: u32,
    credentials_end: u32,
    host_start: u32,
    host_end: u32,
    path_end: u32,
}

fn is_scheme_start(c: u16) -> bool {
    (c >= 'a' as u16 && c <= 'z' as u16) || (c >= 'A' as u16 && c <= 'Z' as u16)
}

fn is_scheme_continue(c: u16) -> bool {
    is_scheme_start(c) || (c >= '0' as u16 && c <= '9' as u16) || c == '+' as u16 || c == '-' as u16 || c == '.' as u16
}

impl URL {
    /// `URL()`: nula e inválida.
    pub fn new() -> URL {
        URL::default()
    }

    /// `URL(const String& absoluteURL)`.
    pub fn from_string(absolute_url: &WtfString) -> URL {
        let mut url = URL { string: absolute_url.clone(), ..URL::default() };
        url.scan();
        url
    }

    fn scan(&mut self) {
        let length = self.string.length();
        let at = |i: u32| self.string.code_unit_at(i);
        if length == 0 || !is_scheme_start(at(0)) {
            return;
        }
        let mut scheme_end = 1;
        while scheme_end < length && is_scheme_continue(at(scheme_end)) {
            scheme_end += 1;
        }
        if scheme_end >= length || at(scheme_end) != ':' as u16 {
            return;
        }
        let after_scheme = scheme_end + 1;

        // Termina o caminho no primeiro '?' ou '#'.
        let mut path_end = length;
        for i in after_scheme..length {
            let c = at(i);
            if c == '?' as u16 || c == '#' as u16 {
                path_end = i;
                break;
            }
        }

        let has_authority = after_scheme + 1 < length && at(after_scheme) == '/' as u16 && at(after_scheme + 1) == '/' as u16;
        let (user_start, credentials_end, host_start, host_end) = if has_authority {
            let authority_start = after_scheme + 2;
            let mut authority_end = path_end;
            for i in authority_start..path_end {
                if at(i) == '/' as u16 {
                    authority_end = i;
                    break;
                }
            }
            let mut credentials_end = authority_start;
            for i in authority_start..authority_end {
                if at(i) == '@' as u16 {
                    credentials_end = i + 1;
                }
            }
            // A porta começa no último ':' fora de colchetes (IPv6).
            let mut host_end = authority_end;
            let mut in_brackets = false;
            for i in credentials_end..authority_end {
                let c = at(i);
                if c == '[' as u16 {
                    in_brackets = true;
                } else if c == ']' as u16 {
                    in_brackets = false;
                } else if c == ':' as u16 && !in_brackets {
                    host_end = i;
                    break;
                }
            }
            (authority_start, credentials_end, credentials_end, host_end)
        } else {
            (after_scheme, after_scheme, after_scheme, after_scheme)
        };

        self.is_valid = true;
        self.user_start = user_start;
        self.credentials_end = credentials_end;
        self.host_start = host_start;
        self.host_end = host_end;
        self.path_end = path_end;
    }

    /// `isNull()`.
    pub fn is_null(&self) -> bool {
        self.string.is_null()
    }

    /// `isValid()`.
    pub fn is_valid(&self) -> bool {
        self.is_valid
    }

    /// `string()`.
    pub fn string(&self) -> &WtfString {
        &self.string
    }

    /// `host()`: o `StringView` vira `String`.
    pub fn host(&self) -> WtfString {
        if !self.is_valid {
            return WtfString::default();
        }
        self.string.substring(self.host_start, self.host_end - self.host_start)
    }

    /// `strippedForUseAsReport()`: tira as credenciais, a consulta e o fragmento.
    pub fn stripped_for_use_as_report(&self) -> WtfString {
        if !self.is_valid {
            return self.string.clone();
        }

        let end = self.credentials_end;
        if self.user_start == end && self.path_end == self.string.length() {
            return self.string.clone();
        }

        let mut result: Vec<u16> = Vec::new();
        for i in 0..self.user_start {
            result.push(self.string.code_unit_at(i));
        }
        for i in end..self.path_end {
            result.push(self.string.code_unit_at(i));
        }
        WtfString::from_utf16(&result)
    }

    /// `fileSystemPath()` para `file://`: o caminho com os escapes `%XX` desfeitos.
    pub fn file_system_path(&self) -> WtfString {
        if !self.is_valid {
            return WtfString::default();
        }
        let mut bytes: Vec<u8> = Vec::new();
        let mut i = self.host_end;
        while i < self.path_end {
            let c = self.string.code_unit_at(i);
            if c == '%' as u16 && i + 2 < self.path_end {
                let hi = (self.string.code_unit_at(i + 1) as u8 as char).to_digit(16);
                let lo = (self.string.code_unit_at(i + 2) as u8 as char).to_digit(16);
                if let (Some(hi), Some(lo)) = (hi, lo) {
                    bytes.push((hi * 16 + lo) as u8);
                    i += 3;
                    continue;
                }
            }
            let mut buffer = [0u8; 4];
            let ch = char::from_u32(c as u32).unwrap_or('\u{FFFD}');
            bytes.extend_from_slice(ch.encode_utf8(&mut buffer).as_bytes());
            i += 1;
        }
        WtfString::from_utf8(&bytes)
    }
}

impl PartialEq for URL {
    /// `operator==(const URL&, const URL&)`: igualdade das strings.
    fn eq(&self, other: &URL) -> bool {
        self.string == other.string
    }
}

impl Eq for URL {}

#[cfg(test)]
mod tests {
    use super::*;

    fn url(text: &str) -> URL {
        URL::from_string(&WtfString::from_latin1(text.as_bytes()))
    }

    #[test]
    fn host_and_report() {
        let u = url("https://user:pw@example.com:8080/a/b?q=1#f");
        assert_eq!(u.host(), WtfString::from_latin1(b"example.com"));
        assert_eq!(u.stripped_for_use_as_report(), WtfString::from_latin1(b"https://example.com:8080/a/b"));
    }

    #[test]
    fn relative_is_invalid() {
        let u = url("foo.js");
        assert!(!u.is_valid());
        assert!(u.host().is_null());
        assert_eq!(u.stripped_for_use_as_report(), WtfString::from_latin1(b"foo.js"));
    }

    #[test]
    fn file_path() {
        let u = url("file:///tmp/a%20b.js");
        assert_eq!(u.file_system_path(), WtfString::from_latin1(b"/tmp/a b.js"));
    }
}
