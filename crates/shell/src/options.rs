//! `set -o` e `shopt`: nomes, letras e valores iniciais do bash 5.2.37 não interativo.

/// Opções do `set -o`, na ordem alfabética em que o bash lista, com a letra (se houver).
pub const SET_OPTIONS: &[(&str, Option<u8>)] = &[
    ("allexport", Some(b'a')),
    ("braceexpand", Some(b'B')),
    ("emacs", None),
    ("errexit", Some(b'e')),
    ("errtrace", Some(b'E')),
    ("functrace", Some(b'T')),
    ("hashall", Some(b'h')),
    ("histexpand", Some(b'H')),
    ("history", None),
    ("ignoreeof", None),
    ("interactive-comments", None),
    ("keyword", Some(b'k')),
    ("monitor", Some(b'm')),
    ("noclobber", Some(b'C')),
    ("noexec", Some(b'n')),
    ("noglob", Some(b'f')),
    ("nolog", None),
    ("notify", Some(b'b')),
    ("nounset", Some(b'u')),
    ("onecmd", Some(b't')),
    ("physical", Some(b'P')),
    ("pipefail", None),
    ("posix", None),
    ("privileged", Some(b'p')),
    ("verbose", Some(b'v')),
    ("vi", None),
    ("xtrace", Some(b'x')),
];

/// Ordem das letras em `$-` (a tabela `shell_flags` do bash, flags.c).
pub const FLAG_ORDER: &[u8] = b"abefhikmnptuvxBCEHPT";

/// Opções do `shopt` na ordem do bash, com o valor inicial (não interativo).
pub const SHOPT_OPTIONS: &[(&str, bool)] = &[
    ("autocd", false),
    ("assoc_expand_once", false),
    ("cdable_vars", false),
    ("cdspell", false),
    ("checkhash", false),
    ("checkjobs", false),
    ("checkwinsize", true),
    ("cmdhist", true),
    ("compat31", false),
    ("compat32", false),
    ("compat40", false),
    ("compat41", false),
    ("compat42", false),
    ("compat43", false),
    ("compat44", false),
    ("complete_fullquote", true),
    ("direxpand", false),
    ("dirspell", false),
    ("dotglob", false),
    ("execfail", false),
    ("expand_aliases", false),
    ("extdebug", false),
    ("extglob", false),
    ("extquote", true),
    ("failglob", false),
    ("force_fignore", true),
    ("globasciiranges", true),
    ("globskipdots", true),
    ("globstar", false),
    ("gnu_errfmt", false),
    ("histappend", false),
    ("histreedit", false),
    ("histverify", false),
    ("hostcomplete", true),
    ("huponexit", false),
    ("inherit_errexit", false),
    ("interactive_comments", true),
    ("lastpipe", false),
    ("lithist", false),
    ("localvar_inherit", false),
    ("localvar_unset", false),
    ("login_shell", false),
    ("mailwarn", false),
    ("no_empty_cmd_completion", false),
    ("nocaseglob", false),
    ("nocasematch", false),
    ("noexpand_translation", false),
    ("nullglob", false),
    ("patsub_replacement", true),
    ("progcomp", true),
    ("progcomp_alias", false),
    ("promptvars", true),
    ("restricted_shell", false),
    ("shift_verbose", false),
    ("sourcepath", true),
    ("varredir_close", false),
    ("xpg_echo", false),
];

/// Estado de `set -o` e `shopt`, por índice nas tabelas acima.
#[derive(Clone, Debug)]
pub struct Options {
    set: Vec<bool>,
    shopt: Vec<bool>,
}

impl Default for Options {
    fn default() -> Self {
        let mut set = vec![false; SET_OPTIONS.len()];
        for (i, (name, _)) in SET_OPTIONS.iter().enumerate() {
            set[i] = matches!(*name, "braceexpand" | "hashall" | "interactive-comments");
        }
        Options { set, shopt: SHOPT_OPTIONS.iter().map(|(_, v)| *v).collect() }
    }
}

impl Options {
    pub fn set_index(name: &str) -> Option<usize> {
        SET_OPTIONS.iter().position(|(n, _)| *n == name)
    }

    pub fn shopt_index(name: &str) -> Option<usize> {
        SHOPT_OPTIONS.iter().position(|(n, _)| *n == name)
    }

    pub fn letter_index(c: u8) -> Option<usize> {
        SET_OPTIONS.iter().position(|(_, l)| *l == Some(c))
    }

    pub fn get(&self, name: &str) -> bool {
        Self::set_index(name).is_some_and(|i| self.set[i])
    }

    pub fn set(&mut self, name: &str, on: bool) -> bool {
        match Self::set_index(name) {
            Some(i) => {
                self.set[i] = on;
                // emacs e vi são mutuamente exclusivas.
                if on && (name == "emacs" || name == "vi") {
                    let other = if name == "emacs" { "vi" } else { "emacs" };
                    if let Some(j) = Self::set_index(other) {
                        self.set[j] = false;
                    }
                }
                true
            }
            None => false,
        }
    }

    pub fn get_index(&self, i: usize) -> bool {
        self.set[i]
    }

    pub fn shopt(&self, name: &str) -> bool {
        Self::shopt_index(name).is_some_and(|i| self.shopt[i])
    }

    pub fn set_shopt(&mut self, name: &str, on: bool) -> bool {
        match Self::shopt_index(name) {
            Some(i) => {
                self.shopt[i] = on;
                true
            }
            None => false,
        }
    }

    pub fn shopt_index_value(&self, i: usize) -> bool {
        self.shopt[i]
    }

    /// `$-` (sem o `c`/`s` finais, que dependem de como o shell foi chamado).
    pub fn flags(&self) -> Vec<u8> {
        let mut out = Vec::new();
        for c in FLAG_ORDER {
            if *c == b'i' {
                continue;
            }
            if let Some(i) = Self::letter_index(*c)
                && self.set[i] {
                    out.push(*c);
                }
        }
        out
    }

    /// `SHELLOPTS`: opções ligadas do `set -o`, separadas por `:`.
    pub fn shellopts(&self) -> Vec<u8> {
        let names: Vec<&str> = SET_OPTIONS.iter().enumerate().filter(|(i, _)| self.set[*i]).map(|(_, (n, _))| *n).collect();
        names.join(":").into_bytes()
    }

    /// `BASHOPTS`: opções ligadas do `shopt`.
    pub fn bashopts(&self) -> Vec<u8> {
        let names: Vec<&str> = SHOPT_OPTIONS.iter().enumerate().filter(|(i, _)| self.shopt[*i]).map(|(_, (n, _))| *n).collect();
        names.join(":").into_bytes()
    }
}
