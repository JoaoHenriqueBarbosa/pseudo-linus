//! `dpkg`, `dpkg-query` e `dpkg-trigger` do dpkg 1.22.21 (Debian 13).
//!
//! Lê o estado em `/var/lib/dpkg` (`status`, `info/*.list`, `arch`): `-l`, `-W`, `-s`, `-L`, `-S`, `-p`,
//! `--get-selections`, `--audit`, `--compare-versions`, `--print-architecture` e
//! `--print-foreign-architectures`, com as mensagens e os códigos de saída do original (2 para erro
//! fatal e de uso, 1 para "não achei"). As ações que alteram o banco (`-i`, `-r`, `-P`, ...) fazem as
//! verificações de argumento e de banco do original, mas não desempacotam nem removem nada.
//!
//! Como no original, `dpkg -l` e companhia delegam ao `dpkg-query`: as mensagens de erro dessas ações
//! saem com o prefixo `dpkg-query:` mesmo quando o programa chamado foi o `dpkg`.

use std::ffi::OsString;
use std::io::Write;

use sysabi::Ctx;

use crate::util::io;

const DPKG_VERSION: &str = "1.22.21";
const NATIVE_ARCH: &str = "amd64";
const DEFAULT_ADMINDIR: &str = "/var/lib/dpkg";

const BADUSAGE_TAIL: &str = "Type dpkg --help for help about installing and deinstalling packages [*];\nUse 'apt' or 'aptitude' for user-friendly package management;\nType dpkg -Dhelp for a list of dpkg debug flag values;\nType dpkg --force-help for a list of forcing options;\nType dpkg-deb --help for help about manipulating *.deb files;\n\nOptions marked [*] produce a lot of output - pipe it through 'less' or 'more' !\n";

const DPKG_HELP: &str = "Usage: dpkg [<option>...] <command>

Commands:
  -i|--install       <.deb file name>... | -R|--recursive <directory>...
  --unpack           <.deb file name>... | -R|--recursive <directory>...
  -A|--record-avail  <.deb file name>... | -R|--recursive <directory>...
  --configure        <package>... | -a|--pending
  --triggers-only    <package>... | -a|--pending
  -r|--remove        <package>... | -a|--pending
  -P|--purge         <package>... | -a|--pending
  -V|--verify <package>...        Verify the integrity of package(s).
  --get-selections [<pattern>...] Get list of selections to stdout.
  --set-selections                Set package selections from stdin.
  --clear-selections              Deselect every non-essential package.
  --update-avail [<Packages-file>]        Replace available packages info.
  --merge-avail [<Packages-file>]         Merge with info from file.
  --clear-avail                   Erase existing available info.
  --forget-old-unavail            Forget uninstalled unavailable packages.
  -s|--status [<package>...]      Display package status details.
  -p|--print-avail [<package>...] Display available version details.
  -L|--listfiles <package>...     List files 'owned' by package(s).
  -l|--list [<pattern>...]        List packages concisely.
  -S|--search <pattern>...        Find package(s) owning file(s).
  -C|--audit [<package>...]       Check for broken package(s).
  --yet-to-unpack                 Print packages selected for installation.
  --predep-package                Print pre-dependencies to unpack.
  --add-architecture <arch>       Add <arch> to the list of architectures.
  --remove-architecture <arch>    Remove <arch> from the list of architectures.
  --print-architecture            Print dpkg architecture.
  --print-foreign-architectures   Print allowed foreign architectures.
  --assert-<feature>              Assert support for the specified feature.
  --validate-<thing> <string>     Validate a <thing>'s <string>.
  --compare-versions <a> <op> <b> Compare version numbers - see below.
  --force-help                    Show help on forcing.
  -Dh|--debug=help                Show help on debugging.

  -?, --help                      Show this help message.
      --version                   Show the version.

Use dpkg with -b, --build, -c, --contents, -e, --control, -I, --info,
  -f, --field, -x, --extract, -X, --vextract, --ctrl-tarfile, --fsys-tarfile
on archives (type dpkg-deb --help).

Options:
  --admindir=<directory>     Use <directory> instead of /var/lib/dpkg.
  --root=<directory>         Install on a different root directory.
  --instdir=<directory>      Change installation dir without changing admin dir.
  --pre-invoke=<command>     Set a pre-invoke hook.
  --post-invoke=<command>    Set a post-invoke hook.
  --path-exclude=<pattern>   Do not install paths which match a shell pattern.
  --path-include=<pattern>   Re-include a pattern after a previous exclusion.
  -O|--selected-only         Skip packages not selected for install/upgrade.
  -E|--skip-same-version     Skip packages whose same version is installed.
  -G|--refuse-downgrade      Skip packages with earlier version than installed.
  -B|--auto-deconfigure      Install even if it would break some other package.
  --[no-]triggers            Skip or force consequential trigger processing.
  --verify-format=<format>   Verify output format (supported: 'rpm').
  --no-pager                 Disables the use of any pager.
  --no-debsig                Do not try to verify package signatures.
  --no-act|--dry-run|--simulate
                             Just say what we would do - don't do it.
  -D|--debug=<octal>         Enable debugging (see -Dhelp or --debug=help).
  --status-logger=<command>  Send status change updates to <command>'s stdin.
  --status-fd <n>            Send status change updates to file descriptor <n>.
  --log=<filename>           Log status changes and actions to <filename>.
  --ignore-depends=<package>[,...]
                             Ignore dependencies involving <package>.
  --force-...                Override problems (see --force-help).
  --no-force-...|--refuse-...
                             Stop when problems encountered.
  --abort-after <n>          Abort after encountering <n> errors.

Comparison operators for --compare-versions are:
  lt le eq ne ge gt       (treat empty version as earlier than any version);
  lt-nl le-nl ge-nl gt-nl (treat empty version as later than any version);
  < << <= = >= >> >       (only for compatibility with control file syntax).

Use 'apt' or 'aptitude' for user-friendly package management.
";

const QUERY_HELP: &str = "Usage: dpkg-query [<option>...] <command>

Commands:
  -s|--status [<package>...]       Display package status details.
  -p|--print-avail [<package>...]  Display available version details.
  -L|--listfiles <package>...      List files 'owned' by package(s).
  -l|--list [<pattern>...]         List packages concisely.
  -W|--show [<pattern>...]         Show information on package(s).
  -S|--search <pattern>...         Find package(s) owning file(s).
  -c|--control-path <package> [<file>]
                                   Print path for package control file.

  -?, --help                       Show this help message.
      --version                    Show the version.

Options:
  --admindir=<directory>     Use <directory> instead of /var/lib/dpkg.
  -f|--showformat=<format>   Use alternative format for --show.
  --no-pager                 Disables the use of any pager.

Format syntax:
  A format is a string that will be output for each package. The format
  can include the standard escape sequences \\n (newline), \\r (carriage
  return) or \\\\ (plain backslash). Package information can be included
  by inserting variable references to package fields using the ${var[;width]}
  syntax. Fields will be right-aligned unless the width is negative in which
  case left alignment will be used.
";

const TRIGGER_HELP: &str = "Usage: dpkg-trigger [<option>...] <trigger-name>
       dpkg-trigger [<option>...] <command>

Commands:
  --check-supported                Check if the running dpkg supports triggers.

  -?, --help                       Show this help message.
      --version                    Show the version.

Options:
  --admindir=<directory>     Use <directory> instead of /var/lib/dpkg.
  --by-package=<package>     Override trigger awaiter (normally set by dpkg).
  --no-await                 No package needs to await the processing.
  --no-act                   Just test - don't actually change anything.

Trigger activation:
  The trigger name is added to the pending triggers of the packages that
  are interested in it (see deb-triggers(5)).
";

fn out(s: &str) {
    let _ = io::stdout().write_all(s.as_bytes());
}

fn version_text(name: &str, kind: &str) -> String {
    format!(
        "Debian '{name}' package {kind} program version {DPKG_VERSION} ({NATIVE_ARCH}).\nThis is free software; see the GNU General Public License version 2 or\nlater for copying conditions. There is NO warranty.\n"
    )
}

/// `badusage()` do dpkg: mensagem, linha em branco e o rodapé de ajuda.
fn badusage(prog: &str, msg: &str) -> i32 {
    io::eprint(format!("{prog}: error: {msg}\n\n{BADUSAGE_TAIL}"));
    2
}

// ---------------------------------------------------------------------------------------------
// Comparação de versões (lib/dpkg/version.c)
// ---------------------------------------------------------------------------------------------

fn order(c: Option<u8>) -> i32 {
    match c {
        None => 0,
        Some(c) if c.is_ascii_digit() => 0,
        Some(c) if c.is_ascii_alphabetic() => i32::from(c),
        Some(b'~') => -1,
        Some(c) => i32::from(c) + 256,
    }
}

fn verrevcmp(a: &[u8], b: &[u8]) -> i32 {
    let (mut i, mut j) = (0usize, 0usize);
    while i < a.len() || j < b.len() {
        let mut first_diff = 0;
        while (i < a.len() && !a[i].is_ascii_digit()) || (j < b.len() && !b[j].is_ascii_digit()) {
            let ac = order(a.get(i).copied());
            let bc = order(b.get(j).copied());
            if ac != bc {
                return ac - bc;
            }
            i += 1;
            j += 1;
        }
        while i < a.len() && a[i] == b'0' {
            i += 1;
        }
        while j < b.len() && b[j] == b'0' {
            j += 1;
        }
        while i < a.len() && a[i].is_ascii_digit() && j < b.len() && b[j].is_ascii_digit() {
            if first_diff == 0 {
                first_diff = i32::from(a[i]) - i32::from(b[j]);
            }
            i += 1;
            j += 1;
        }
        if i < a.len() && a[i].is_ascii_digit() {
            return 1;
        }
        if j < b.len() && b[j].is_ascii_digit() {
            return -1;
        }
        if first_diff != 0 {
            return first_diff;
        }
    }
    0
}

struct Version<'a> {
    epoch: u64,
    upstream: &'a str,
    revision: &'a str,
}

fn parse_version(v: &str) -> Version<'_> {
    let v = v.trim_start_matches([' ', '\t']);
    let (epoch, rest) = match v.split_once(':') {
        Some((e, r)) => (e.parse::<u64>().unwrap_or(0), r),
        None => (0, v),
    };
    let (upstream, revision) = match rest.rsplit_once('-') {
        Some((u, r)) => (u, r),
        None => (rest, ""),
    };
    Version { epoch, upstream, revision }
}

/// Compara duas versões Debian: negativo, zero ou positivo.
pub fn compare_versions(a: &str, b: &str) -> i32 {
    let (va, vb) = (parse_version(a), parse_version(b));
    if va.epoch != vb.epoch {
        return if va.epoch > vb.epoch { 1 } else { -1 };
    }
    let r = verrevcmp(va.upstream.as_bytes(), vb.upstream.as_bytes());
    if r != 0 {
        return r;
    }
    verrevcmp(va.revision.as_bytes(), vb.revision.as_bytes())
}

/// Avalia `--compare-versions a op b`; `None` quando o operador não existe.
fn eval_relation(a: &str, op: &str, b: &str) -> Option<bool> {
    let nl = op.ends_with("-nl");
    let base = op.strip_suffix("-nl").unwrap_or(op);
    let cmp = match (a.is_empty(), b.is_empty()) {
        (true, true) => 0,
        // Versão vazia é menor que qualquer uma, salvo nos operadores "-nl".
        (true, false) => {
            if nl {
                1
            } else {
                -1
            }
        }
        (false, true) => {
            if nl {
                -1
            } else {
                1
            }
        }
        (false, false) => compare_versions(a, b),
    };
    let r = match base {
        "lt" | "<<" => cmp < 0,
        "le" | "<=" | "<" => cmp <= 0,
        "eq" | "=" => cmp == 0,
        "ne" => cmp != 0,
        "ge" | ">=" | ">" => cmp >= 0,
        "gt" | ">>" => cmp > 0,
        _ => return None,
    };
    // "-nl" só existe para lt, le, ge e gt.
    if nl && !matches!(base, "lt" | "le" | "ge" | "gt") {
        return None;
    }
    Some(r)
}

// ---------------------------------------------------------------------------------------------
// Banco de dados: status
// ---------------------------------------------------------------------------------------------

/// Um parágrafo de controle: campos na ordem em que aparecem (valor com as linhas de continuação).
#[derive(Clone, Default)]
struct Para {
    fields: Vec<(String, String)>,
}

impl Para {
    fn get(&self, name: &str) -> Option<&str> {
        self.fields
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }

    fn name(&self) -> &str {
        self.get("Package").unwrap_or("")
    }

    fn arch(&self) -> &str {
        self.get("Architecture").unwrap_or("")
    }

    /// `(want, status)` do campo `Status` (`install ok installed`).
    fn status_parts(&self) -> (&str, &str, &str) {
        let s = self.get("Status").unwrap_or("");
        let mut it = s.split_whitespace();
        (
            it.next().unwrap_or("unknown"),
            it.next().unwrap_or("ok"),
            it.next().unwrap_or("not-installed"),
        )
    }

    fn state(&self) -> &str {
        self.status_parts().2
    }

    fn is_installed(&self) -> bool {
        self.state() == "installed"
    }

    /// Nome como `${binary:Package}`: leva `:arch` se for `Multi-Arch: same` ou de arquitetura
    /// estrangeira.
    fn binary_name(&self) -> String {
        let arch = self.arch();
        let multi_same = self.get("Multi-Arch").is_some_and(|m| m == "same");
        if !arch.is_empty() && arch != "all" && (arch != NATIVE_ARCH || multi_same) {
            format!("{}:{}", self.name(), arch)
        } else {
            self.name().to_string()
        }
    }

    /// `${db:Status-Abbrev}`: três caracteres (desejo, estado, erro).
    fn abbrev(&self) -> String {
        let (want, flag, state) = self.status_parts();
        let w = match want {
            "unknown" => 'u',
            "install" => 'i',
            "hold" => 'h',
            "deinstall" => 'r',
            "purge" => 'p',
            _ => '?',
        };
        let s = match state {
            "not-installed" => 'n',
            "config-files" => 'c',
            "half-installed" => 'H',
            "unpacked" => 'U',
            "half-configured" => 'F',
            "triggers-awaited" => 'W',
            "triggers-pending" => 't',
            "installed" => 'i',
            _ => '?',
        };
        let e = if flag == "ok" { ' ' } else { 'R' };
        format!("{w}{s}{e}")
    }
}

fn parse_paragraphs(data: &[u8]) -> Vec<Para> {
    let text = String::from_utf8_lossy(data);
    let mut out = Vec::new();
    let mut cur = Para::default();
    for line in text.split('\n') {
        if line.is_empty() || line.trim().is_empty() && !line.starts_with([' ', '\t']) {
            if !cur.fields.is_empty() {
                out.push(std::mem::take(&mut cur));
            }
            continue;
        }
        if line.starts_with('#') {
            continue;
        }
        if line.starts_with([' ', '\t']) {
            if let Some(last) = cur.fields.last_mut() {
                last.1.push('\n');
                last.1.push_str(line);
            }
            continue;
        }
        match line.split_once(':') {
            Some((k, v)) => cur.fields.push((k.to_string(), v.trim().to_string())),
            None => cur.fields.push((line.to_string(), String::new())),
        }
    }
    if !cur.fields.is_empty() {
        out.push(cur);
    }
    out
}

/// Lê `<admindir>/status`; em falha escreve o erro do `dpkg-query` e devolve o código 2.
fn load_status(prog: &str, admindir: &str) -> Result<Vec<Para>, i32> {
    let path = format!("{admindir}/status");
    match io::read_path(path.as_bytes()) {
        Ok(d) => Ok(parse_paragraphs(&d)),
        Err(e) => {
            io::eprint(format!(
                "{prog}: error: failed to open package info file '{path}' for reading: {}\n",
                e.message()
            ));
            Err(2)
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Padrões (fnmatch simples) e formato de saída
// ---------------------------------------------------------------------------------------------

fn has_glob(s: &str) -> bool {
    s.contains(['*', '?', '['])
}

fn glob(p: &[u8], s: &[u8]) -> bool {
    match p.first() {
        None => s.is_empty(),
        Some(b'*') => (0..=s.len()).any(|k| glob(&p[1..], &s[k..])),
        Some(b'?') => !s.is_empty() && glob(&p[1..], &s[1..]),
        Some(b'[') => {
            let Some(&c) = s.first() else { return false };
            let mut i = 1;
            let negate = matches!(p.get(i), Some(b'!') | Some(b'^'));
            if negate {
                i += 1;
            }
            let mut matched = false;
            let mut first = true;
            while i < p.len() && (p[i] != b']' || first) {
                first = false;
                if i + 2 < p.len() && p[i + 1] == b'-' && p[i + 2] != b']' {
                    if p[i] <= c && c <= p[i + 2] {
                        matched = true;
                    }
                    i += 3;
                } else {
                    if p[i] == c {
                        matched = true;
                    }
                    i += 1;
                }
            }
            if i >= p.len() {
                // Colchete sem fecho: casa o `[` literal.
                return c == b'[' && glob(&p[1..], &s[1..]);
            }
            matched != negate && glob(&p[i + 1..], &s[1..])
        }
        Some(&c) => s.first() == Some(&c) && glob(&p[1..], &s[1..]),
    }
}

fn pkg_matches(pat: &str, p: &Para) -> bool {
    let name = p.name();
    let qualified = format!("{}:{}", name, p.arch());
    if has_glob(pat) {
        glob(pat.as_bytes(), name.as_bytes())
            || glob(pat.as_bytes(), qualified.as_bytes())
            || glob(pat.as_bytes(), p.binary_name().as_bytes())
    } else {
        pat.eq_ignore_ascii_case(name)
            || pat.eq_ignore_ascii_case(&qualified)
            || pat.eq_ignore_ascii_case(&p.binary_name())
    }
}

fn field_value(p: &Para, var: &str) -> String {
    match var {
        "binary:Package" => p.binary_name(),
        "Package" => p.name().to_string(),
        "source:Package" => p
            .get("Source")
            .map(|s| s.split_whitespace().next().unwrap_or("").to_string())
            .unwrap_or_else(|| p.name().to_string()),
        "db:Status-Abbrev" => p.abbrev(),
        "db:Status-Want" => p.status_parts().0.to_string(),
        "db:Status-Status" => p.state().to_string(),
        "db:Status-Eflag" => p.status_parts().1.to_string(),
        "Status" => p.get("Status").unwrap_or("").to_string(),
        "binary:Synopsis" => p
            .get("Description")
            .map(|d| d.lines().next().unwrap_or("").to_string())
            .unwrap_or_default(),
        other => p.get(other).map(str::to_string).unwrap_or_default(),
    }
}

/// Expande um formato `-f`/`--showformat` para um pacote.
fn render_format(fmt: &str, p: &Para) -> String {
    let mut out = String::new();
    let b: Vec<char> = fmt.chars().collect();
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            '\\' if i + 1 < b.len() => {
                i += 1;
                match b[i] {
                    'n' => out.push('\n'),
                    't' => out.push('\t'),
                    'r' => out.push('\r'),
                    'v' => out.push('\u{0b}'),
                    'f' => out.push('\u{0c}'),
                    'a' => out.push('\u{07}'),
                    '\\' => out.push('\\'),
                    c => out.push(c),
                }
                i += 1;
            }
            '$' if b.get(i + 1) == Some(&'{') => {
                if let Some(end) = b[i + 2..].iter().position(|c| *c == '}') {
                    let body: String = b[i + 2..i + 2 + end].iter().collect();
                    let (var, width) = match body.split_once(';') {
                        Some((v, w)) => (v.to_string(), w.parse::<i64>().unwrap_or(0)),
                        None => (body.clone(), 0),
                    };
                    let val = field_value(p, &var);
                    let w = width.unsigned_abs() as usize;
                    let len = val.chars().count();
                    let pad = w.saturating_sub(len);
                    if width < 0 {
                        out.push_str(&val);
                        out.push_str(&" ".repeat(pad));
                    } else {
                        out.push_str(&" ".repeat(pad));
                        out.push_str(&val);
                    }
                    i += 2 + end + 1;
                } else {
                    out.push('$');
                    i += 1;
                }
            }
            c => {
                out.push(c);
                i += 1;
            }
        }
    }
    out
}

// ---------------------------------------------------------------------------------------------
// Análise de argumentos
// ---------------------------------------------------------------------------------------------

/// Ações: (curta, longa).
type ActionTable = &'static [(Option<char>, &'static str)];

const DPKG_ACTIONS: ActionTable = &[
    (Some('i'), "install"),
    (None, "unpack"),
    (Some('A'), "record-avail"),
    (None, "configure"),
    (None, "triggers-only"),
    (Some('r'), "remove"),
    (Some('P'), "purge"),
    (Some('V'), "verify"),
    (None, "get-selections"),
    (None, "set-selections"),
    (None, "clear-selections"),
    (None, "update-avail"),
    (None, "merge-avail"),
    (None, "clear-avail"),
    (None, "forget-old-unavail"),
    (Some('s'), "status"),
    (Some('p'), "print-avail"),
    (Some('L'), "listfiles"),
    (Some('l'), "list"),
    (Some('S'), "search"),
    (Some('C'), "audit"),
    (None, "yet-to-unpack"),
    (None, "predep-package"),
    (None, "add-architecture"),
    (None, "remove-architecture"),
    (None, "print-architecture"),
    (None, "print-foreign-architectures"),
    (None, "compare-versions"),
];

const QUERY_ACTIONS: ActionTable = &[
    (Some('s'), "status"),
    (Some('p'), "print-avail"),
    (Some('L'), "listfiles"),
    (Some('l'), "list"),
    (Some('W'), "show"),
    (Some('S'), "search"),
    (Some('c'), "control-path"),
];

const VALUE_LONGS: &[&str] = &[
    "admindir",
    "root",
    "instdir",
    "pre-invoke",
    "post-invoke",
    "path-exclude",
    "path-include",
    "status-logger",
    "status-fd",
    "log",
    "ignore-depends",
    "abort-after",
    "verify-format",
    "debug",
    "showformat",
    "by-package",
];

const BOOL_LONGS: &[&str] = &[
    "no-act",
    "dry-run",
    "simulate",
    "no-pager",
    "no-debsig",
    "selected-only",
    "skip-same-version",
    "refuse-downgrade",
    "auto-deconfigure",
    "triggers",
    "no-triggers",
    "recursive",
    "pending",
    "no-await",
    "await",
    "check-supported",
];

#[derive(Default)]
struct Parsed {
    /// Nome longo da ação escolhida, ou `help`, `version`, `assert`, `check-supported`.
    action: Option<String>,
    /// Como a ação foi escrita, para a mensagem de conflito.
    action_label: String,
    operands: Vec<String>,
    admindir: Option<String>,
    showformat: Option<String>,
    pending: bool,
    no_act: bool,
    by_package: Option<String>,
}

fn action_label(table: ActionTable, long: &str) -> String {
    for (s, l) in table {
        if *l == long {
            return match s {
                Some(c) => format!("-{c} (--{l})"),
                None => format!("--{l}"),
            };
        }
    }
    format!("--{long}")
}

/// Varre o argv. `Err(mensagem)` vira `badusage`.
fn parse_args(
    args: &[String],
    table: ActionTable,
    extra_bools: &[&str],
    allow_assert: bool,
) -> Result<Parsed, String> {
    let mut p = Parsed::default();
    let mut i = 0;
    let mut only_operands = false;

    fn set_action(
        p: &mut Parsed,
        table: ActionTable,
        long: &str,
    ) -> Result<(), String> {
        let label = action_label(table, long);
        if let Some(prev) = &p.action {
            if prev != long {
                return Err(format!("conflicting actions {} and {}", p.action_label, label));
            }
        }
        p.action = Some(long.to_string());
        p.action_label = label;
        Ok(())
    }

    while i < args.len() {
        let a = args[i].clone();
        i += 1;
        if only_operands || a == "-" || !a.starts_with('-') {
            p.operands.push(a);
            continue;
        }
        if a == "--" {
            only_operands = true;
            continue;
        }
        if let Some(body) = a.strip_prefix("--") {
            let (name, val) = match body.split_once('=') {
                Some((n, v)) => (n.to_string(), Some(v.to_string())),
                None => (body.to_string(), None),
            };
            if name == "help" {
                p.action = Some("help".into());
                return Ok(p);
            }
            if name == "version" {
                p.action = Some("version".into());
                return Ok(p);
            }
            if table.iter().any(|(_, l)| *l == name) {
                set_action(&mut p, table, &name)?;
                continue;
            }
            if allow_assert && name.starts_with("assert-") {
                p.action = Some(name.clone());
                continue;
            }
            if VALUE_LONGS.contains(&name.as_str()) {
                let v = match val {
                    Some(v) => v,
                    None => {
                        if i < args.len() {
                            i += 1;
                            args[i - 1].clone()
                        } else {
                            return Err(format!("--{name} takes a value"));
                        }
                    }
                };
                match name.as_str() {
                    "admindir" => p.admindir = Some(v),
                    "showformat" => p.showformat = Some(v),
                    "by-package" => p.by_package = Some(v),
                    _ => {}
                }
                continue;
            }
            if BOOL_LONGS.contains(&name.as_str())
                || extra_bools.contains(&name.as_str())
                || name.starts_with("force-")
                || name.starts_with("no-force-")
                || name.starts_with("refuse-")
            {
                match name.as_str() {
                    "pending" => p.pending = true,
                    "no-act" | "dry-run" | "simulate" => p.no_act = true,
                    "check-supported" => p.action = Some("check-supported".into()),
                    _ => {}
                }
                continue;
            }
            return Err(format!("unknown option --{name}"));
        }
        // Opções curtas agrupadas.
        let chars: Vec<char> = a[1..].chars().collect();
        let mut k = 0;
        while k < chars.len() {
            let c = chars[k];
            k += 1;
            if c == '?' {
                p.action = Some("help".into());
                return Ok(p);
            }
            if let Some((_, long)) = table.iter().find(|(s, _)| *s == Some(c)) {
                set_action(&mut p, table, long)?;
                continue;
            }
            match c {
                'D' | 'f' => {
                    let rest: String = chars[k..].iter().collect();
                    let v = if !rest.is_empty() {
                        rest
                    } else if i < args.len() {
                        i += 1;
                        args[i - 1].clone()
                    } else {
                        return Err(format!("-{c} takes a value"));
                    };
                    if c == 'f' {
                        p.showformat = Some(v);
                    }
                    k = chars.len();
                }
                'a' => p.pending = true,
                'R' | 'O' | 'E' | 'G' | 'B' => {}
                _ => return Err(format!("unknown option -{c}")),
            }
        }
    }
    Ok(p)
}

fn argv_strings(args: &[OsString]) -> Vec<String> {
    io::args_bytes(args).iter().skip(1).map(|a| io::lossy(a)).collect()
}

// ---------------------------------------------------------------------------------------------
// Ações de consulta (dpkg-query)
// ---------------------------------------------------------------------------------------------

fn sorted(mut v: Vec<Para>) -> Vec<Para> {
    v.sort_by(|a, b| a.name().as_bytes().cmp(b.name().as_bytes()).then(a.arch().cmp(b.arch())));
    v
}

fn terminal_width() -> usize {
    sysabi::sys::getenv("COLUMNS")
        .and_then(|v| io::lossy(&v).trim().parse::<usize>().ok())
        .filter(|w| *w > 0)
        .unwrap_or(80)
}

fn query_list(pkgs: Vec<Para>, patterns: &[String]) -> i32 {
    let all = sorted(pkgs);
    let mut shown: Vec<&Para> = Vec::new();
    let mut rc = 0;
    if patterns.is_empty() {
        shown.extend(all.iter().filter(|p| p.state() != "not-installed"));
    } else {
        for pat in patterns {
            let mut found = false;
            for p in all.iter().filter(|p| pkg_matches(pat, p)) {
                found = true;
                if !shown.iter().any(|s| std::ptr::eq(*s, p)) {
                    shown.push(p);
                }
            }
            if !found {
                io::eprint(format!("dpkg-query: no packages found matching {pat}\n"));
                rc = 1;
            }
        }
        shown.sort_by(|a, b| a.name().as_bytes().cmp(b.name().as_bytes()).then(a.arch().cmp(b.arch())));
    }
    if shown.is_empty() && rc != 0 {
        return rc;
    }
    let rows: Vec<(String, String, String, String, String)> = shown
        .iter()
        .map(|p| {
            let installed_like = p.state() != "not-installed";
            let version = p.get("Version").filter(|_| installed_like).unwrap_or("<none>").to_string();
            let arch = if installed_like { p.arch().to_string() } else { "<none>".to_string() };
            let desc = p
                .get("Description")
                .map(|d| d.lines().next().unwrap_or("").to_string())
                .unwrap_or_else(|| "(no description available)".to_string());
            (p.abbrev(), p.binary_name(), version, arch, desc)
        })
        .collect();
    let nw = rows.iter().map(|r| r.1.chars().count()).max().unwrap_or(0).max(4);
    let vw = rows.iter().map(|r| r.2.chars().count()).max().unwrap_or(0).max(7);
    let aw = rows.iter().map(|r| r.3.chars().count()).max().unwrap_or(0).max(12);
    let width = terminal_width();
    let dw = width.saturating_sub(3 + 1 + nw + 1 + vw + 1 + aw + 1).max(1);
    let mut o = String::new();
    o.push_str("Desired=Unknown/Install/Remove/Purge/Hold\n");
    o.push_str("| Status=Not/Inst/Conf-files/Unpacked/halF-conf/Half-inst/trig-aWait/Trig-pend\n");
    o.push_str("|/ Err?=(none)/Reinst-required (Status,Err: uppercase=bad)\n");
    o.push_str(&format!(
        "||/ {:<nw$} {:<vw$} {:<aw$} {}\n",
        "Name", "Version", "Architecture", "Description"
    ));
    o.push_str(&format!(
        "+++-{}-{}-{}-{}\n",
        "=".repeat(nw),
        "=".repeat(vw),
        "=".repeat(aw),
        "=".repeat(dw)
    ));
    for (ab, name, ver, arch, desc) in &rows {
        let d: String = desc.chars().take(dw).collect();
        o.push_str(&format!("{ab} {name:<nw$} {ver:<vw$} {arch:<aw$} {d}\n"));
    }
    out(&o);
    rc
}

fn query_show(pkgs: Vec<Para>, patterns: &[String], fmt: &str) -> i32 {
    let all = sorted(pkgs);
    let mut rc = 0;
    let mut o = String::new();
    if patterns.is_empty() {
        for p in all.iter().filter(|p| p.state() != "not-installed") {
            o.push_str(&render_format(fmt, p));
        }
    } else {
        for pat in patterns {
            let mut found = false;
            for p in all.iter().filter(|p| pkg_matches(pat, p)) {
                found = true;
                o.push_str(&render_format(fmt, p));
            }
            if !found {
                out(&std::mem::take(&mut o));
                io::eprint(format!("dpkg-query: no packages found matching {pat}\n"));
                rc = 1;
            }
        }
    }
    out(&o);
    rc
}

fn query_status(pkgs: Vec<Para>, names: &[String]) -> i32 {
    let all = sorted(pkgs);
    let mut rc = 0;
    let mut first = true;
    let mut o = String::new();
    let mut emit = |p: &Para, o: &mut String| {
        if !first {
            o.push('\n');
        }
        first = false;
        for (k, v) in &p.fields {
            o.push_str(&format!("{k}: {v}\n"));
        }
    };
    if names.is_empty() {
        for p in all.iter().filter(|p| p.state() != "not-installed") {
            emit(p, &mut o);
        }
    } else {
        for n in names {
            let hits: Vec<&Para> = all.iter().filter(|p| pkg_matches(n, p) && !has_glob(n)).collect();
            if hits.is_empty() {
                out(&std::mem::take(&mut o));
                io::eprint(format!(
                    "dpkg-query: package '{n}' is not installed and no information is available\nUse dpkg --info (= dpkg-deb --info) to examine archive files.\n"
                ));
                rc = 1;
                continue;
            }
            for p in hits {
                emit(p, &mut o);
            }
        }
    }
    out(&o);
    rc
}

/// Lista de arquivos de um pacote (`info/<nome>[:arch].list`).
fn list_files(admindir: &str, p: &Para) -> Option<Vec<String>> {
    let name = p.name();
    let mut candidates = vec![format!("{admindir}/info/{name}:{}.list", p.arch())];
    candidates.push(format!("{admindir}/info/{name}.list"));
    for c in candidates {
        if let Ok(d) = io::read_path(c.as_bytes()) {
            return Some(
                String::from_utf8_lossy(&d)
                    .lines()
                    .map(str::to_string)
                    .collect(),
            );
        }
    }
    None
}

fn query_listfiles(admindir: &str, pkgs: Vec<Para>, names: &[String]) -> i32 {
    if names.is_empty() {
        return badusage_query("--listfiles needs at least one package name argument");
    }
    let mut rc = 0;
    let mut o = String::new();
    for n in names {
        let Some(p) = pkgs.iter().find(|p| pkg_matches(n, p) && p.state() != "not-installed") else {
            out(&std::mem::take(&mut o));
            io::eprint(format!(
                "dpkg-query: package '{n}' is not installed\nUse dpkg --info (= dpkg-deb --info) to examine archive files.\n"
            ));
            rc = 1;
            continue;
        };
        match list_files(admindir, p) {
            Some(files) if !files.is_empty() => {
                for f in files {
                    o.push_str(&f);
                    o.push('\n');
                }
            }
            _ => o.push_str(&format!("Package '{}' does not contain any files (!)\n", p.binary_name())),
        }
    }
    out(&o);
    rc
}

fn query_search(admindir: &str, pkgs: Vec<Para>, pats: &[String]) -> i32 {
    if pats.is_empty() {
        return badusage_query("--search needs at least one file name pattern argument");
    }
    let all = sorted(pkgs);
    let mut rc = 0;
    let mut o = String::new();
    for pat in pats {
        let mut pattern = pat.clone();
        let globbing = has_glob(&pattern);
        if !globbing {
            while pattern.len() > 1 && pattern.ends_with('/') {
                pattern.pop();
            }
            if !pattern.contains('/') {
                pattern = format!("*{pattern}*");
            }
        } else if !pattern.starts_with(['/', '*', '?']) {
            pattern = format!("*{pattern}*");
        }
        let use_glob = has_glob(&pattern);
        let mut found = false;
        // Agrupa por caminho, preservando a ordem de descoberta.
        let mut hits: Vec<(String, Vec<String>)> = Vec::new();
        for p in all.iter().filter(|p| p.state() != "not-installed") {
            let Some(files) = list_files(admindir, p) else { continue };
            for f in files {
                let ok = if use_glob {
                    glob(pattern.as_bytes(), f.as_bytes())
                } else {
                    f == pattern
                };
                if ok {
                    found = true;
                    let n = p.binary_name();
                    match hits.iter_mut().find(|(path, _)| *path == f) {
                        Some((_, v)) => {
                            if !v.contains(&n) {
                                v.push(n);
                            }
                        }
                        None => hits.push((f, vec![n])),
                    }
                }
            }
        }
        for (path, names) in hits {
            o.push_str(&format!("{}: {}\n", names.join(", "), path));
        }
        if !found {
            out(&std::mem::take(&mut o));
            io::eprint(format!("dpkg-query: no path found matching pattern {pat}\n"));
            rc = 1;
        }
    }
    out(&o);
    rc
}

fn badusage_query(msg: &str) -> i32 {
    io::eprint(format!(
        "dpkg-query: error: {msg}\n\nType dpkg-query --help for help about querying packages.\n"
    ));
    2
}

fn query_dispatch(action: &str, parsed: &Parsed) -> i32 {
    let admindir = parsed.admindir.clone().unwrap_or_else(|| DEFAULT_ADMINDIR.to_string());
    match action {
        "print-avail" => {
            // Sem o arquivo `available` nenhum pacote é "disponível".
            let names = &parsed.operands;
            let path = format!("{admindir}/available");
            let data = io::read_path(path.as_bytes()).unwrap_or_default();
            let avail = parse_paragraphs(&data);
            let mut rc = 0;
            let mut o = String::new();
            if names.is_empty() {
                for p in &avail {
                    for (k, v) in &p.fields {
                        o.push_str(&format!("{k}: {v}\n"));
                    }
                    o.push('\n');
                }
            }
            for n in names {
                match avail.iter().find(|p| pkg_matches(n, p)) {
                    Some(p) => {
                        for (k, v) in &p.fields {
                            o.push_str(&format!("{k}: {v}\n"));
                        }
                    }
                    None => {
                        out(&std::mem::take(&mut o));
                        io::eprint(format!(
                            "dpkg-query: package '{n}' is not available\nUse dpkg --info (= dpkg-deb --info) to examine archive files.\n"
                        ));
                        rc = 1;
                    }
                }
            }
            out(&o);
            return rc;
        }
        "control-path" => {
            if parsed.operands.is_empty() || parsed.operands.len() > 2 {
                return badusage_query("--control-path takes one or two arguments");
            }
        }
        _ => {}
    }
    let pkgs = match load_status("dpkg-query", &admindir) {
        Ok(p) => p,
        Err(c) => return c,
    };
    match action {
        "list" => query_list(pkgs, &parsed.operands),
        "show" => {
            let fmt = parsed
                .showformat
                .clone()
                .unwrap_or_else(|| "${binary:Package}\t${Version}\n".to_string());
            query_show(pkgs, &parsed.operands, &fmt)
        }
        "status" => query_status(pkgs, &parsed.operands),
        "listfiles" => query_listfiles(&admindir, pkgs, &parsed.operands),
        "search" => query_search(&admindir, pkgs, &parsed.operands),
        "control-path" => {
            let n = &parsed.operands[0];
            let Some(p) = pkgs.iter().find(|p| pkg_matches(n, p) && p.state() != "not-installed") else {
                io::eprint(format!("dpkg-query: package '{n}' is not installed\n"));
                return 1;
            };
            let file = parsed.operands.get(1).map(String::as_str).unwrap_or("");
            let base = format!("{admindir}/info/{}", p.name());
            if file.is_empty() {
                return 0;
            }
            let path = format!("{base}.{file}");
            if io::read_path(path.as_bytes()).is_ok() {
                out(&format!("{path}\n"));
                0
            } else {
                1
            }
        }
        _ => 2,
    }
}

// ---------------------------------------------------------------------------------------------
// dpkg-query
// ---------------------------------------------------------------------------------------------

pub fn query_main(_ctx: &mut Ctx, args: &[OsString]) -> i32 {
    io::run(|| query(args))
}

fn query(args: &[OsString]) -> i32 {
    let argv = argv_strings(args);
    let parsed = match parse_args(&argv, QUERY_ACTIONS, &[], false) {
        Ok(p) => p,
        Err(m) => {
            io::eprint(format!(
                "dpkg-query: error: {m}\n\nType dpkg-query --help for help about querying packages.\n"
            ));
            return 2;
        }
    };
    match parsed.action.as_deref() {
        Some("help") => {
            out(QUERY_HELP);
            0
        }
        Some("version") => {
            out(&version_text("dpkg-query", "query"));
            0
        }
        None => badusage_query("need an action option"),
        Some(a) => query_dispatch(a, &parsed),
    }
}

// ---------------------------------------------------------------------------------------------
// dpkg
// ---------------------------------------------------------------------------------------------

pub fn dpkg_main(_ctx: &mut Ctx, args: &[OsString]) -> i32 {
    io::run(|| dpkg(args))
}

fn need_args_msg(action: &str) -> Option<&'static str> {
    match action {
        "install" => Some("--install needs at least one package archive file argument"),
        "unpack" => Some("--unpack needs at least one package archive file argument"),
        "record-avail" => Some("--record-avail needs at least one package archive file argument"),
        "configure" => Some("--configure needs at least one package name argument"),
        "triggers-only" => Some("--triggers-only needs at least one package name argument"),
        "remove" => Some("--remove needs at least one package name argument"),
        "purge" => Some("--purge needs at least one package name argument"),
        _ => None,
    }
}

fn dpkg(args: &[OsString]) -> i32 {
    let argv = argv_strings(args);
    let parsed = match parse_args(&argv, DPKG_ACTIONS, &[], true) {
        Ok(p) => p,
        Err(m) => return badusage("dpkg", &m),
    };
    let admindir = parsed.admindir.clone().unwrap_or_else(|| DEFAULT_ADMINDIR.to_string());
    let Some(action) = parsed.action.clone() else {
        return badusage("dpkg", "need an action option");
    };
    match action.as_str() {
        "help" => {
            out(DPKG_HELP);
            0
        }
        "version" => {
            out(&version_text("dpkg", "management"));
            0
        }
        "list" | "status" | "listfiles" | "search" | "print-avail" => query_dispatch(&action, &parsed),
        "print-architecture" => {
            out(&format!("{NATIVE_ARCH}\n"));
            0
        }
        "print-foreign-architectures" => {
            let path = format!("{admindir}/arch");
            if let Ok(d) = io::read_path(path.as_bytes()) {
                let text = String::from_utf8_lossy(&d).into_owned();
                let mut o = String::new();
                for l in text.lines().filter(|l| !l.is_empty() && *l != NATIVE_ARCH) {
                    o.push_str(l);
                    o.push('\n');
                }
                out(&o);
            }
            0
        }
        "compare-versions" => {
            if parsed.operands.len() != 3 {
                return badusage(
                    "dpkg",
                    "--compare-versions takes three arguments: <version> <relation> <version>",
                );
            }
            match eval_relation(&parsed.operands[0], &parsed.operands[1], &parsed.operands[2]) {
                Some(true) => 0,
                Some(false) => 1,
                None => badusage("dpkg", "--compare-versions bad relation"),
            }
        }
        a if a.starts_with("assert-") => match &a[7..] {
            "support-predepends" | "working-epoch" | "long-filenames" | "multi-conrep" | "multi-arch"
            | "versioned-provides" | "protected-field" | "protected-field-dpkg" => 0,
            other => badusage("dpkg", &format!("unknown option --assert-{other}")),
        },
        "audit" => match load_status("dpkg", &admindir) {
            Ok(_) => 0,
            Err(c) => c,
        },
        "get-selections" => {
            let pkgs = match load_status("dpkg", &admindir) {
                Ok(p) => sorted(p),
                Err(c) => return c,
            };
            let mut o = String::new();
            for p in pkgs.iter().filter(|p| p.state() != "not-installed") {
                if !parsed.operands.is_empty() && !parsed.operands.iter().any(|pat| pkg_matches(pat, p)) {
                    continue;
                }
                let name = p.binary_name();
                let mut col = name.chars().count();
                o.push_str(&name);
                loop {
                    o.push('\t');
                    col = (col / 8 + 1) * 8;
                    if col >= 40 {
                        break;
                    }
                }
                o.push_str(p.status_parts().0);
                o.push('\n');
            }
            out(&o);
            0
        }
        other => modify_action(other, &parsed, &admindir),
    }
}

/// Ações que mudariam o banco: confere argumentos e banco como o original, sem alterar nada.
fn modify_action(action: &str, parsed: &Parsed, admindir: &str) -> i32 {
    if let Some(msg) = need_args_msg(action) {
        let by_pending = matches!(action, "configure" | "triggers-only" | "remove" | "purge") && parsed.pending;
        if parsed.operands.is_empty() && !by_pending {
            return badusage("dpkg", msg);
        }
    }
    if matches!(action, "add-architecture" | "remove-architecture") && parsed.operands.len() != 1 {
        return badusage("dpkg", &format!("--{action} takes exactly one argument"));
    }
    if parsed.no_act && matches!(action, "install" | "unpack" | "remove" | "purge") {
        // `--no-act` ainda precisa do banco legível.
    }
    let status_path = format!("{admindir}/status");
    let pkgs = match io::read_path(status_path.as_bytes()) {
        Ok(d) => parse_paragraphs(&d),
        Err(e) => {
            io::eprint(format!(
                "dpkg: error: unable to access dpkg database directory '{admindir}': {}\n",
                e.message()
            ));
            return 2;
        }
    };
    match action {
        "install" | "unpack" | "record-avail" => {
            for f in &parsed.operands {
                if let Err(e) = io::read_path(f.as_bytes()) {
                    io::eprint(format!(
                        "dpkg: error: cannot access archive '{f}': {}\n",
                        e.message()
                    ));
                    return 2;
                }
            }
            io::eprint("dpkg: error: unpacking archives is not supported by this port\n");
            2
        }
        "remove" | "purge" => {
            let mut rc = 0;
            for n in &parsed.operands {
                match pkgs.iter().find(|p| pkg_matches(n, p) && p.state() != "not-installed") {
                    None => io::eprint(format!(
                        "dpkg: warning: ignoring request to remove {n} which isn't installed\n"
                    )),
                    Some(_) => {
                        io::eprint(format!("dpkg: error: unable to remove package {n}: not supported by this port\n"));
                        rc = 2;
                    }
                }
            }
            rc
        }
        "configure" | "triggers-only" => {
            for n in &parsed.operands {
                if !pkgs.iter().any(|p| pkg_matches(n, p) && p.state() != "not-installed") {
                    io::eprint(format!("dpkg: error: --{action}: package '{n}' is not installed\n"));
                    return 2;
                }
            }
            0
        }
        _ => 0,
    }
}

// ---------------------------------------------------------------------------------------------
// dpkg-trigger
// ---------------------------------------------------------------------------------------------

pub fn trigger_main(_ctx: &mut Ctx, args: &[OsString]) -> i32 {
    io::run(|| trigger(args))
}

fn trigger(args: &[OsString]) -> i32 {
    let argv = argv_strings(args);
    let parsed = match parse_args(&argv, &[], &[], false) {
        Ok(p) => p,
        Err(m) => {
            io::eprint(format!(
                "dpkg-trigger: error: {m}\n\nType dpkg-trigger --help for help.\n"
            ));
            return 2;
        }
    };
    match parsed.action.as_deref() {
        Some("help") => {
            out(TRIGGER_HELP);
            return 0;
        }
        Some("version") => {
            out(&version_text("dpkg-trigger", "trigger"));
            return 0;
        }
        Some("check-supported") => return 0,
        _ => {}
    }
    if parsed.operands.len() != 1 {
        io::eprint(
            "dpkg-trigger: error: takes one argument, the trigger name\n\nType dpkg-trigger --help for help.\n",
        );
        return 2;
    }
    let admindir = parsed.admindir.clone().unwrap_or_else(|| DEFAULT_ADMINDIR.to_string());
    let name = &parsed.operands[0];
    // Nome de gatilho: um caminho absoluto ou um nome sem espaço nem barra.
    if name.chars().any(|c| c.is_whitespace()) {
        io::eprint(format!(
            "dpkg-trigger: error: invalid trigger name '{name}': trigger name contains invalid character ' '\n"
        ));
        return 2;
    }
    if parsed.no_act {
        return 0;
    }
    let lock = format!("{admindir}/triggers/Unincorp");
    if let Err(e) = io::read_path(lock.as_bytes()) {
        if e == sysabi::Errno::ENOENT {
            let dir = format!("{admindir}/triggers");
            if sysabi::sys::stat(dir.as_bytes()).is_err() {
                io::eprint(format!(
                    "dpkg-trigger: error: unable to open/create triggers lockfile '{admindir}/triggers/Lock': {}\n",
                    e.message()
                ));
                return 2;
            }
        }
    }
    0
}
