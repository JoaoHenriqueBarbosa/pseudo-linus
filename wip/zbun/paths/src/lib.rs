//! `bun_paths`: o subconjunto de `src/paths/lib.rs` do bun 1.4.2 que o parser lê (`NODE_MODULES_NEEDLE` e o
//! módulo `fs` com `PathName` e o `Path` do resolvedor). Só POSIX: o separador é `/`.
//!
//! Fica fora o `PathName::dir_with_trailing_slash`, que estende a fatia além do fim com ponteiro cru e o parser
//! não usa.

/// `<SEP>node_modules<SEP>`: o trecho que diz se um caminho passa por um diretório `node_modules`.
pub const NODE_MODULES_NEEDLE: &[u8] = b"/node_modules/";

/// `resolve_path::last_index_of_sep` no POSIX.
fn last_index_of_sep(path: &[u8]) -> Option<usize> {
    path.iter().rposition(|&c| c == b'/')
}

/// `bun_core::is_sep_any`: aceita os dois separadores, como o upstream faz em qualquer sistema.
const fn is_sep_any(c: u8) -> bool {
    c == b'/' || c == b'\\'
}

pub mod fs {
    use super::{is_sep_any, last_index_of_sep};

    #[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
    pub struct PathName<'a> {
        pub base: &'a [u8],
        pub dir: &'a [u8],
        /// includes the leading `.`; extensionless files report `""`
        pub ext: &'a [u8],
        pub filename: &'a [u8],
    }

    impl<'a> PathName<'a> {
        /// Returns the extension of `path` including the leading `.`, or `""`
        /// for extensionless / dotfile basenames.
        pub fn find_extname(path: &[u8]) -> &[u8] {
            let start = last_index_of_sep(path).map(|i| i + 1).unwrap_or(0);
            let base = &path[start..];
            if let Some(dot) = base.iter().rposition(|&c| c == b'.') {
                if dot > 0 {
                    return &base[dot..];
                }
            }
            b""
        }

        #[inline]
        pub fn ext_without_leading_dot(&self) -> &'a [u8] {
            if !self.ext.is_empty() && self.ext[0] == b'.' { &self.ext[1..] } else { self.ext }
        }

        /// `/bar/foo/index.js` → `foo`; `/bar/foo.js` → `foo`.
        pub fn non_unique_name_string_base(&self) -> &'a [u8] {
            // /bar/foo/index.js -> foo
            if !self.dir.is_empty() && self.base == b"index" {
                // "/index" -> "index"
                return PathName::init(self.dir).base;
            }
            debug_assert!(!self.base.contains(&b'/'));
            // /bar/foo.js -> foo
            self.base
        }

        /// Formats [`Self::non_unique_name_string_base`] as a valid JS
        /// identifier.
        #[inline]
        pub fn fmt_identifier(&self) -> bun_core::fmt::FormatValidIdentifier<'a> {
            bun_core::fmt::fmt_identifier(self.non_unique_name_string_base())
        }

        /// Parses `path_` into its (dir, base, ext, filename) components; all
        /// returned slices borrow `path_`.
        pub fn init(path_: &'a [u8]) -> PathName<'a> {
            let mut path = path_;
            let mut base = path;
            let ext: &[u8];
            let mut dir = path;
            let mut is_absolute = true;
            let has_disk_designator =
                path.len() > 2 && path[1] == b':' && path[0].is_ascii_alphabetic() && is_sep_any(path[2]);
            if has_disk_designator {
                path = &path[2..];
            }

            while let Some(i) = last_index_of_sep(path) {
                // Stop if we found a non-trailing slash
                if i + 1 != path.len() && path.len() > i + 1 {
                    base = &path[i + 1..];
                    dir = &path[0..i];
                    is_absolute = false;
                    break;
                }

                // Ignore trailing slashes
                path = &path[0..i];
                base = path;
            }

            // Strip off the extension
            if let Some(dot) = base.iter().rposition(|&c| c == b'.') {
                ext = &base[dot..];
                base = &base[0..dot];
            } else {
                ext = b"";
            }

            if is_absolute {
                dir = b"";
            }

            if base.len() > 1 && is_sep_any(base[base.len() - 1]) {
                base = &base[0..base.len() - 1];
            }

            if !is_absolute && has_disk_designator {
                dir = &path_[0..dir.len() + 2];
            }

            let filename = if !dir.is_empty() { &path_[dir.len() + 1..] } else { path_ };

            PathName { dir, base, ext, filename }
        }
    }

    /// The bundler/resolver's logical path (display `pretty`, canonical `text`, `namespace`). Aqui com o
    /// tempo de vida do texto emprestado; o `Path` guardado no registro de import é o de `bun_ast::import_record`,
    /// com faixas do `Store`.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct Path<'a> {
        pub pretty: &'a [u8],
        pub text: &'a [u8],
        pub namespace: &'a [u8],
        pub is_disabled: bool,
        pub is_symlink: bool,
    }

    impl<'a> Path<'a> {
        /// Parsed (dir/base/ext/filename) view of `text`.
        #[inline]
        pub fn name(&self) -> PathName<'a> {
            PathName::init(self.text)
        }

        /// Sets `text`/`pretty` to the same slice, namespace defaults to `"file"`.
        #[inline]
        pub const fn init(text: &'a [u8]) -> Self {
            Self { pretty: text, text, namespace: b"file", is_disabled: false, is_symlink: false }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::fs::{Path, PathName};

    #[test]
    fn path_name_parts() {
        let n = PathName::init(b"/bar/foo/index.js");
        assert_eq!((n.dir, n.base, n.ext, n.filename), (&b"/bar/foo"[..], &b"index"[..], &b".js"[..], &b"index.js"[..]));
        assert_eq!(n.non_unique_name_string_base(), b"foo");
        assert_eq!(n.ext_without_leading_dot(), b"js");
        let rel = PathName::init(b"react-dom");
        assert_eq!((rel.dir, rel.base, rel.filename), (&b""[..], &b"react-dom"[..], &b"react-dom"[..]));
        assert_eq!(rel.fmt_identifier().to_string(), "react_dom");
        assert_eq!(PathName::init(b"./a/").base, b"a");
    }

    #[test]
    fn find_extname_and_path() {
        assert_eq!(PathName::find_extname(b"/x/y.tar.gz"), b".gz");
        assert_eq!(PathName::find_extname(b"/x/.bashrc"), b"");
        let p = Path::init(b"/src/app.tsx");
        assert_eq!((p.namespace, p.name().ext), (&b"file"[..], &b".tsx"[..]));
    }
}
