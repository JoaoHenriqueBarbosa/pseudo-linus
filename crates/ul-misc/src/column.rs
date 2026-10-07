//! `column` do util-linux 2.41.5 (pacote bsdextrautils do Debian 13).
//!
//! Três modos, como o original:
//!
//! - preenchimento por colunas (padrão) e por linhas (`-x`): as entradas são as linhas da entrada; a
//!   largura de cada coluna é a da maior entrada arredondada pro próximo múltiplo de 8 (tabulação) ou
//!   somada a `-S N` espaços;
//! - tabela (`-t`, ou `-J` pra JSON): cada linha vira uma linha da tabela, dividida em células por
//!   espaço em branco (sequências contam como um separador) ou pelos caracteres de `-s` (cada um é um
//!   separador, então há células vazias). A saída imita a libsmartcols: colunas alinhadas, separador
//!   `-o` (padrão dois espaços), última coluna sem preenchimento, cabeçalho com `-N`/`-C`, colunas
//!   escondidas, reordenadas, alinhadas à direita, árvore (`-r`/`-i`/`-p`) e JSON.
//!
//! O comportamento foi levantado em caixa preta contra o Debian 13 (o código da libsmartcols é
//! LGPL e não foi usado), inclusive a redução de largura com `-T`/`-W`/`-E` quando a tabela não cabe
//! em `-c` (ver `Table::to_text`).

use std::ffi::OsString;
use std::io::Write;

use sysabi::{Ctx, Errno};

use crate::util::io::{self, File};
use crate::util::{Getopt, GetoptError, HasArg, LongOpt, display_width};

const TABCHAR_CNT: usize = 8;
const OPT_HELP: i32 = 'h' as i32;

const LONGOPTS: &[LongOpt] = &[
    LongOpt::new("columns", HasArg::Required, 'c' as i32),
    LongOpt::new("fillrows", HasArg::No, 'x' as i32),
    LongOpt::new("help", HasArg::No, OPT_HELP),
    LongOpt::new("json", HasArg::No, 'J' as i32),
    LongOpt::new("keep-empty-lines", HasArg::No, 'L' as i32),
    LongOpt::new("output-separator", HasArg::Required, 'o' as i32),
    LongOpt::new("output-width", HasArg::Required, 'c' as i32),
    LongOpt::new("separator", HasArg::Required, 's' as i32),
    LongOpt::new("table", HasArg::No, 't' as i32),
    LongOpt::new("table-columns", HasArg::Required, 'N' as i32),
    LongOpt::new("table-column", HasArg::Required, 'C' as i32),
    LongOpt::new("table-columns-limit", HasArg::Required, 'l' as i32),
    LongOpt::new("table-hide", HasArg::Required, 'H' as i32),
    LongOpt::new("table-name", HasArg::Required, 'n' as i32),
    LongOpt::new("table-maxout", HasArg::No, 'm' as i32),
    LongOpt::new("table-noextreme", HasArg::Required, 'E' as i32),
    LongOpt::new("table-noheadings", HasArg::No, 'd' as i32),
    LongOpt::new("table-order", HasArg::Required, 'O' as i32),
    LongOpt::new("table-right", HasArg::Required, 'R' as i32),
    LongOpt::new("table-truncate", HasArg::Required, 'T' as i32),
    LongOpt::new("table-wrap", HasArg::Required, 'W' as i32),
    LongOpt::new("table-empty-lines", HasArg::No, 'L' as i32),
    LongOpt::new("table-header-repeat", HasArg::No, 'e' as i32),
    LongOpt::new("tree", HasArg::Required, 'r' as i32),
    LongOpt::new("tree-id", HasArg::Required, 'i' as i32),
    LongOpt::new("tree-parent", HasArg::Required, 'p' as i32),
    LongOpt::new("use-spaces", HasArg::Required, 'S' as i32),
    LongOpt::new("version", HasArg::No, 'V' as i32),
];

const SHORTOPTS: &str = "C:c:dE:eH:hi:Jl:LN:n:mO:o:p:R:r:s:S:tT:VW:x";

const USAGE: &str = "
Usage:
 column [options] [<file>...]

Columnate lists.

Options:
 -t, --table                      create a table
 -n, --table-name <name>          table name for JSON output
 -O, --table-order <columns>      specify order of output columns
 -C, --table-column <properties>  define column
 -N, --table-columns <names>      comma separated columns names
 -l, --table-columns-limit <num>  maximal number of input columns
 -E, --table-noextreme <columns>  don't count long text from the columns to column width
 -d, --table-noheadings           don't print header
 -m, --table-maxout               fill all available space
 -e, --table-header-repeat        repeat header for each page
 -H, --table-hide <columns>       don't print the columns
 -R, --table-right <columns>      right align text in these columns
 -T, --table-truncate <columns>   truncate text in the columns when necessary
 -W, --table-wrap <columns>       wrap text in the columns when necessary
 -L, --keep-empty-lines           don't ignore empty lines
 -J, --json                       use JSON output format for table

 -r, --tree <column>              column to use tree-like output for the table
 -i, --tree-id <column>           line ID to specify child-parent relation
 -p, --tree-parent <column>       parent to specify child-parent relation

 -c, --output-width <width>       width of output in number of characters
 -o, --output-separator <string>  columns separator for table output (default is two spaces)
 -s, --separator <string>         possible table delimiters
 -x, --fillrows                   fill rows before columns
 -S, --use-spaces <number>        minimal whitespaces between columns (no tabs)

 -h, --help                       display this help
 -V, --version                    display version

For more details see column(1).
";

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum Mode {
    FillCols,
    FillRows,
    Table,
}

/// Erro fatal com a mensagem que o `errx` do util-linux daria (sem o prefixo do programa).
struct Fatal(String);

#[derive(Default)]
struct Options {
    mode: Option<Mode>,
    json: bool,
    table_name: Option<String>,
    order: Option<String>,
    column_specs: Vec<String>,
    names: Option<String>,
    limit: Option<usize>,
    noextreme: Option<String>,
    hide: Option<String>,
    right: Option<String>,
    trunc: Option<String>,
    wrap: Option<String>,
    noheadings: bool,
    maxout: bool,
    keep_empty: bool,
    tree: Option<String>,
    tree_id: Option<String>,
    tree_parent: Option<String>,
    termwidth: Option<usize>,
    out_sep: Option<String>,
    in_sep: Option<Vec<char>>,
    use_spaces: Option<usize>,
}

/// `strtou32_or_err` do util-linux.
fn parse_u32(arg: &str, what: &str) -> Result<usize, Fatal> {
    let digits = arg.trim_start_matches([' ', '\t', '\n', '\x0b', '\x0c', '\r']);
    let range = || Fatal(format!("{what}: '{arg}': {}", Errno::ERANGE.message()));
    let invalid = || Fatal(format!("{what}: '{arg}'"));
    let body = digits.strip_prefix('+').unwrap_or(digits);
    if let Some(rest) = body.strip_prefix('-') {
        if !rest.is_empty() && rest.bytes().all(|b| b.is_ascii_digit()) {
            return Err(range());
        }
        return Err(invalid());
    }
    if body.is_empty() || !body.bytes().all(|b| b.is_ascii_digit()) {
        return Err(invalid());
    }
    match body.parse::<u64>() {
        Ok(n) if n <= u64::from(u32::MAX) => Ok(n as usize),
        _ => Err(range()),
    }
}

pub fn main(_ctx: &mut Ctx, args: &[OsString]) -> i32 {
    io::run(|| run(args))
}

fn run(args: &[OsString]) -> i32 {
    let argv = io::args_bytes(args);
    let argv0 = io::argv0(args);
    let fail = |msg: String| -> i32 {
        io::eprint(format!("column: {msg}\n"));
        1
    };
    let mut o = Options::default();
    let mut getopt = Getopt::from_env(&argv[1..], SHORTOPTS, LONGOPTS);
    // Grupos mutuamente exclusivos do original, com o nome longo de cada opção.
    let excl: &[&[(char, &str)]] = &[
        &[('C', "--table-column"), ('N', "--table-columns")],
        &[('J', "--json"), ('x', "--fillrows")],
        &[('t', "--table"), ('x', "--fillrows")],
    ];
    let mut seen_excl: Vec<Option<char>> = vec![None; excl.len()];
    while let Some(r) = getopt.next_opt() {
        let opt = match r {
            Ok(opt) => opt,
            Err(e) => return getopt_failure(&e, &argv0),
        };
        let c = opt.short().unwrap_or('\0');
        for (gi, group) in excl.iter().enumerate() {
            if group.iter().any(|(g, _)| *g == c) {
                match seen_excl[gi] {
                    Some(prev) if prev != c => {
                        let names: Vec<&str> = group.iter().map(|(_, n)| *n).collect();
                        return fail(format!("mutually exclusive arguments: {}", names.join(" ")));
                    }
                    _ => seen_excl[gi] = Some(c),
                }
            }
        }
        let arg = opt.arg_str();
        match c {
            'C' => o.column_specs.push(arg),
            'c' => match parse_u32(&arg, "invalid columns argument") {
                Ok(n) => o.termwidth = Some(n),
                Err(Fatal(m)) => return fail(m),
            },
            'd' => o.noheadings = true,
            'E' => o.noextreme = Some(arg),
            'e' => {}
            'H' => o.hide = Some(arg),
            'i' => o.tree_id = Some(arg),
            'J' => {
                o.json = true;
                o.mode = Some(Mode::Table);
            }
            'l' => match parse_u32(&arg, "invalid columns limit argument") {
                Ok(0) => return fail("columns limit must be greater than zero".into()),
                Ok(n) => o.limit = Some(n),
                Err(Fatal(m)) => return fail(m),
            },
            'L' => o.keep_empty = true,
            'N' => o.names = Some(arg),
            'n' => o.table_name = Some(arg),
            'm' => o.maxout = true,
            'O' => o.order = Some(arg),
            'o' => o.out_sep = Some(arg),
            'p' => o.tree_parent = Some(arg),
            'R' => o.right = Some(arg),
            'r' => o.tree = Some(arg),
            's' => o.in_sep = Some(arg.chars().collect()),
            'S' => match parse_u32(&arg, "invalid spaces argument") {
                Ok(n) => o.use_spaces = Some(n),
                Err(Fatal(m)) => return fail(m),
            },
            't' => o.mode = Some(Mode::Table),
            'T' => o.trunc = Some(arg),
            'W' => o.wrap = Some(arg),
            'x' => o.mode = Some(Mode::FillRows),
            'V' => {
                let mut out = io::stdout();
                let _ = out.write_all(b"column from util-linux 2.41.5\n");
                return 0;
            }
            _ if opt.id == OPT_HELP => {
                let mut out = io::stdout();
                let _ = out.write_all(USAGE.as_bytes());
                return 0;
            }
            _ => return getopt_failure(&GetoptError::Invalid(c), &argv0),
        }
    }
    let files = getopt.operands();
    let mode = o.mode.unwrap_or(Mode::FillCols);

    if o.tree.is_some() && (o.tree_id.is_none() || o.tree_parent.is_none()) {
        return fail("options --tree-id and --tree-parent are required for tree formatting".into());
    }
    if mode != Mode::Table
        && (o.order.is_some()
            || o.table_name.is_some()
            || o.wrap.is_some()
            || o.hide.is_some()
            || o.trunc.is_some()
            || o.noextreme.is_some()
            || o.right.is_some()
            || o.names.is_some()
            || !o.column_specs.is_empty())
    {
        return fail("option --table required for all --table-*".into());
    }
    if o.json && o.names.is_none() && o.column_specs.is_empty() {
        return fail("option --table-columns or --table-column required for --json".into());
    }

    // Lê todas as entradas; arquivo que não abre é avisado e conta como falha, erro de leitura é fatal.
    let mut lines: Vec<String> = Vec::new();
    let mut eval = 0;
    if files.is_empty() {
        if let Err(e) = read_lines(&mut File::stdin(), &mut lines) {
            return fail(format!("read failed: {}", e.message()));
        }
    } else {
        for f in &files {
            match File::open(f) {
                Ok(mut file) => {
                    if let Err(e) = read_lines(&mut file, &mut lines) {
                        return fail(format!("read failed: {}", e.message()));
                    }
                }
                Err(e) => {
                    io::eprint(format!("column: {}: {}\n", io::lossy(f), e.message()));
                    eval += 1;
                }
            }
        }
    }
    let termwidth = o.termwidth.unwrap_or_else(terminal_width);
    let mut out = io::stdout();
    match mode {
        Mode::FillCols | Mode::FillRows => {
            let entries: Vec<&String> = lines
                .iter()
                .filter(|l| o.keep_empty || !is_blank(l))
                .collect();
            let text = fill(&entries, termwidth, o.use_spaces, mode == Mode::FillRows);
            let _ = out.write_all(text.as_bytes());
            i32::from(eval != 0)
        }
        Mode::Table => {
            let mut table = match build_table(&o, &lines) {
                Ok(t) => t,
                Err(Fatal(m)) => return fail(m),
            };
            if table.rows.is_empty() {
                return i32::from(eval != 0);
            }
            let text = if o.json {
                table.to_json()
            } else {
                table.render_text(termwidth)
            };
            let _ = out.write_all(text.as_bytes());
            0
        }
    }
}

fn getopt_failure(e: &GetoptError, argv0: &str) -> i32 {
    io::eprint(format!(
        "{}\nTry 'column --help' for more information.\n",
        e.message(argv0)
    ));
    1
}

/// Largura do terminal como o `get_terminal_width(80)` do util-linux: `COLUMNS` válido, senão o
/// tamanho do tty, senão 80.
fn terminal_width() -> usize {
    if let Some(v) = sysabi::sys::try_current().and_then(|s| s.getenv(b"COLUMNS"))
        && let Ok(n) = String::from_utf8_lossy(&v).trim().parse::<i64>()
        && n > 0
    {
        return n as usize;
    }
    if let Some(s) = sysabi::sys::try_current()
        && s.isatty(sysabi::Fd::STDOUT)
        && let Ok(ws) = s.tcgetwinsize(sysabi::Fd::STDOUT)
        && ws.cols > 0
    {
        return usize::from(ws.cols);
    }
    80
}

/// Lê linhas como o `getline` + conversão pra caractere largo do original: o `\n` sai, a linha
/// termina no primeiro NUL, e byte que não forma UTF-8 válido vira o texto `\xHH`.
fn read_lines(file: &mut File, lines: &mut Vec<String>) -> Result<(), Errno> {
    let data = file.read_to_end_sys()?;
    if data.is_empty() {
        return Ok(());
    }
    let body = data.strip_suffix(b"\n").unwrap_or(&data);
    for (i, raw) in body.split(|b| *b == b'\n').enumerate() {
        if i % 4096 == 0 {
            sysabi::sys::checkpoint();
        }
        let raw = match raw.iter().position(|b| *b == 0) {
            Some(p) => &raw[..p],
            None => raw,
        };
        lines.try_reserve(1).map_err(|_| Errno::ENOMEM)?;
        lines.push(safe_text(raw));
    }
    Ok(())
}

fn safe_text(raw: &[u8]) -> String {
    let mut s = String::new();
    for chunk in raw.utf8_chunks() {
        s.push_str(chunk.valid());
        for b in chunk.invalid() {
            s.push_str(&format!("\\x{b:02x}"));
        }
    }
    s
}

fn is_ws(c: char) -> bool {
    matches!(c, ' ' | '\t' | '\n' | '\x0b' | '\x0c' | '\r')
}

/// Linha sem nenhum caractere que não seja espaço em branco (o original ignora essas).
fn is_blank(line: &str) -> bool {
    line.chars().all(is_ws)
}

// ------------------------------------------------------------------------------------------------
// Preenchimento (modo padrão e -x)

fn fill(entries: &[&String], termwidth: usize, use_spaces: Option<usize>, by_rows: bool) -> String {
    let mut out = String::new();
    if entries.is_empty() {
        return out;
    }
    let widths: Vec<usize> = entries.iter().map(|e| display_width(e)).collect();
    let maxwidth = widths.iter().copied().max().unwrap_or(0);
    let maxlength = match use_spaces {
        Some(n) => maxwidth + n,
        None => (maxwidth + TABCHAR_CNT) & !(TABCHAR_CNT - 1),
    };
    let numcols = (termwidth / maxlength.max(1)).max(1);
    let pad = |out: &mut String, chcnt: &mut usize, endcol: usize| match use_spaces {
        Some(_) => {
            while *chcnt < endcol {
                out.push(' ');
                *chcnt += 1;
            }
        }
        None => loop {
            let next = (*chcnt + TABCHAR_CNT) & !(TABCHAR_CNT - 1);
            if next > endcol {
                break;
            }
            out.push('\t');
            *chcnt = next;
        },
    };
    let n = entries.len();
    if by_rows {
        let (mut chcnt, mut col, mut endcol) = (0usize, 0usize, maxlength);
        for (i, e) in entries.iter().enumerate() {
            out.push_str(e);
            chcnt += widths[i];
            col += 1;
            if i + 1 == n {
                break;
            }
            if col == numcols {
                chcnt = 0;
                col = 0;
                endcol = maxlength;
                out.push('\n');
            } else {
                pad(&mut out, &mut chcnt, endcol);
                endcol += maxlength;
            }
        }
        if chcnt != 0 {
            out.push('\n');
        }
    } else {
        let numrows = n.div_ceil(numcols);
        for row in 0..numrows {
            let mut endcol = maxlength;
            let mut chcnt = 0;
            let mut base = row;
            while base < n {
                out.push_str(entries[base]);
                chcnt += widths[base];
                if base + numrows >= n {
                    break;
                }
                pad(&mut out, &mut chcnt, endcol);
                endcol += maxlength;
                base += numrows;
            }
            out.push('\n');
        }
    }
    out
}

// ------------------------------------------------------------------------------------------------
// Tabela

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum JsonType {
    String,
    Number,
    Boolean,
    ArrayString,
    ArrayNumber,
}

#[derive(Clone, Debug)]
struct Column {
    name: String,
    hidden: bool,
    right: bool,
    trunc: bool,
    wrap: bool,
    noextreme: bool,
    json: JsonType,
    /// Largura calculada pra saída.
    width: usize,
}

impl Column {
    fn new(name: String) -> Column {
        Column {
            name,
            hidden: false,
            right: false,
            trunc: false,
            wrap: false,
            noextreme: false,
            json: JsonType::String,
            width: 0,
        }
    }
}

struct Table {
    name: String,
    columns: Vec<Column>,
    /// Células por linha, na ordem das colunas originais; `None` é célula inexistente ou vazia.
    rows: Vec<Vec<Option<String>>>,
    headings: bool,
    sep: String,
    /// Coluna da árvore e, por linha, os filhos (índices de linhas) e as raízes.
    tree: Option<Tree>,
    /// Ordem de saída das colunas (índices em `columns`).
    order: Vec<usize>,
    maxout: bool,
}

struct Tree {
    column: usize,
    roots: Vec<usize>,
    children: Vec<Vec<usize>>,
}

/// Divide uma linha em células: separadores do `-s` (cada ocorrência separa) ou espaço em branco
/// (sequências contam como uma). Com `limit`, a última célula leva o resto da linha.
fn split_cells(line: &str, seps: Option<&[char]>, limit: Option<usize>) -> Vec<String> {
    let mut cells = Vec::new();
    match seps {
        Some(seps) => {
            if seps.is_empty() {
                return vec![line.to_string()];
            }
            let mut rest = line;
            loop {
                if limit.is_some_and(|l| cells.len() + 1 >= l) {
                    cells.push(rest.to_string());
                    break;
                }
                match rest.char_indices().find(|(_, c)| seps.contains(c)) {
                    Some((i, c)) => {
                        cells.push(rest[..i].to_string());
                        rest = &rest[i + c.len_utf8()..];
                    }
                    None => {
                        cells.push(rest.to_string());
                        break;
                    }
                }
            }
        }
        None => {
            let mut rest = line.trim_start_matches(is_ws);
            while !rest.is_empty() {
                if limit.is_some_and(|l| cells.len() + 1 >= l) {
                    cells.push(rest.to_string());
                    break;
                }
                let end = rest.find(is_ws).unwrap_or(rest.len());
                cells.push(rest[..end].to_string());
                rest = rest[end..].trim_start_matches(is_ws);
            }
        }
    }
    cells
}

/// Resolve uma lista `a,b,3` de colunas (nome exato ou número a partir de 1) em índices.
fn resolve_list(list: &str, columns: &[Column]) -> Result<Vec<usize>, Fatal> {
    let mut out = Vec::new();
    for item in list.split(',') {
        if item.is_empty() {
            continue;
        }
        if let Some(i) = columns.iter().position(|c| c.name == item) {
            out.push(i);
            continue;
        }
        if item.bytes().all(|b| b.is_ascii_digit()) {
            let n: usize = item.parse().unwrap_or(usize::MAX);
            if n == 0 {
                continue;
            }
            if n <= columns.len() {
                out.push(n - 1);
                continue;
            }
        }
        return Err(Fatal(format!("undefined column name '{item}'")));
    }
    Ok(out)
}

fn apply_column_spec(col: &mut Column, spec: &str) {
    for prop in spec.split(',') {
        let (key, value) = match prop.split_once('=') {
            Some((k, v)) => (k, Some(v)),
            None => (prop, None),
        };
        match (key, value) {
            ("name", Some(v)) => col.name = v.to_string(),
            ("trunc", None) => col.trunc = true,
            ("right", None) => col.right = true,
            ("wrap", None) => col.wrap = true,
            ("hidden" | "hide", None) => col.hidden = true,
            ("noextreme", None) => col.noextreme = true,
            ("json", Some(v)) => {
                col.json = match v {
                    "number" => JsonType::Number,
                    "boolean" => JsonType::Boolean,
                    "array-string" => JsonType::ArrayString,
                    "array-number" => JsonType::ArrayNumber,
                    _ => JsonType::String,
                }
            }
            // `width`, `strictwidth`, `color`, `headercolor` e propriedades desconhecidas não mudam a
            // saída fora de terminal (observado no original).
            _ => {}
        }
    }
}

/// Lista de colunas de uma opção (`-R`, `-T`...) e o atributo que ela liga em cada coluna.
type FlagList<'a> = (&'a Option<String>, fn(&mut Column));

fn build_table(o: &Options, lines: &[String]) -> Result<Table, Fatal> {
    let mut columns: Vec<Column> = Vec::new();
    if let Some(names) = &o.names {
        columns.extend(names.split(',').map(|n| Column::new(n.to_string())));
    }
    for spec in &o.column_specs {
        let mut col = Column::new(String::new());
        apply_column_spec(&mut col, spec);
        columns.push(col);
    }
    let headings = !columns.is_empty() && !o.noheadings;
    let mut rows: Vec<Vec<Option<String>>> = Vec::new();
    for line in lines {
        let cells = if is_blank(line) {
            if !o.keep_empty {
                continue;
            }
            Vec::new()
        } else {
            split_cells(line, o.in_sep.as_deref(), o.limit)
        };
        while columns.len() < cells.len() {
            columns.push(Column::new(String::new()));
        }
        rows.try_reserve(1)
            .map_err(|_| Fatal(Errno::ENOMEM.message()))?;
        rows.push(
            cells
                .into_iter()
                .map(|c| (!c.is_empty()).then_some(c))
                .collect(),
        );
    }
    let ncols = columns.len();
    for r in &mut rows {
        r.resize(ncols, None);
    }

    if let Some(list) = &o.hide {
        for item in list.split(',') {
            if item == "-" {
                for c in columns.iter_mut().filter(|c| c.name.is_empty()) {
                    c.hidden = true;
                }
            } else {
                for i in resolve_list(item, &columns)? {
                    columns[i].hidden = true;
                }
            }
        }
    }
    let flag_lists: [FlagList; 4] = [
        (&o.right, |c| c.right = true),
        (&o.trunc, |c| c.trunc = true),
        (&o.wrap, |c| c.wrap = true),
        (&o.noextreme, |c| c.noextreme = true),
    ];
    for (list, set) in flag_lists {
        if let Some(list) = list {
            for i in resolve_list(list, &columns)? {
                set(&mut columns[i]);
            }
        }
    }
    let mut order: Vec<usize> = Vec::new();
    if let Some(list) = &o.order {
        for i in resolve_list(list, &columns)? {
            if !order.contains(&i) {
                order.push(i);
            }
        }
    }
    for i in 0..ncols {
        if !order.contains(&i) {
            order.push(i);
        }
    }

    let tree = match (&o.tree, &o.tree_id, &o.tree_parent) {
        (Some(t), Some(id), Some(parent)) => {
            let col = single_column(t, &columns)?;
            let idc = single_column(id, &columns)?;
            let pc = single_column(parent, &columns)?;
            Some(build_tree(&rows, col, idc, pc))
        }
        _ => None,
    };

    Ok(Table {
        name: o.table_name.clone().unwrap_or_else(|| "table".into()),
        columns,
        rows,
        headings,
        sep: o.out_sep.clone().unwrap_or_else(|| "  ".into()),
        tree,
        order,
        maxout: o.maxout,
    })
}

fn single_column(name: &str, columns: &[Column]) -> Result<usize, Fatal> {
    match columns.iter().position(|c| c.name == name) {
        Some(i) => Ok(i),
        None => Err(Fatal(format!("undefined tree column name '{name}'"))),
    }
}

fn build_tree(rows: &[Vec<Option<String>>], column: usize, id: usize, parent: usize) -> Tree {
    let mut children = vec![Vec::new(); rows.len()];
    let mut roots = Vec::new();
    for (i, row) in rows.iter().enumerate() {
        let p = row[parent].as_deref();
        let found = p
            .and_then(|p| rows.iter().position(|r| r[id].as_deref() == Some(p)))
            .filter(|&j| j != i);
        match found {
            Some(j) => children[j].push(i),
            None => roots.push(i),
        }
    }
    Tree {
        column,
        roots,
        children,
    }
}

impl Table {
    /// Linhas na ordem de saída, com o prefixo de árvore de cada uma.
    fn ordered_rows(&self) -> Vec<(usize, String)> {
        let Some(tree) = &self.tree else {
            return (0..self.rows.len()).map(|i| (i, String::new())).collect();
        };
        let mut out = Vec::new();
        fn walk(
            tree: &Tree,
            node: usize,
            branch: &str,
            last: Option<bool>,
            out: &mut Vec<(usize, String)>,
        ) {
            let prefix = match last {
                None => String::new(),
                Some(true) => format!("{branch}\u{2514}\u{2500}"),
                Some(false) => format!("{branch}\u{251c}\u{2500}"),
            };
            out.push((node, prefix));
            let kids = &tree.children[node];
            let next_branch = match last {
                None => String::new(),
                Some(true) => format!("{branch}  "),
                Some(false) => format!("{branch}\u{2502} "),
            };
            for (k, &child) in kids.iter().enumerate() {
                walk(tree, child, &next_branch, Some(k + 1 == kids.len()), out);
            }
        }
        for &r in &tree.roots {
            walk(tree, r, "", None, &mut out);
        }
        out
    }

    fn cell_text(&self, row: usize, col: usize, prefix: &str) -> String {
        let data = self.rows[row][col].as_deref().unwrap_or("");
        if self.tree.as_ref().is_some_and(|t| t.column == col) {
            format!("{prefix}{data}")
        } else {
            data.to_string()
        }
    }

    fn render_text(&mut self, termwidth: usize) -> String {
        let visible: Vec<usize> = self
            .order
            .iter()
            .copied()
            .filter(|&i| !self.columns[i].hidden)
            .collect();
        let rows = self.ordered_rows();
        let cells: Vec<Vec<String>> = rows
            .iter()
            .map(|(r, prefix)| {
                visible
                    .iter()
                    .map(|&c| self.cell_text(*r, c, prefix))
                    .collect()
            })
            .collect();
        let sepw = display_width(&self.sep);
        // Larguras naturais: maior célula, ou o cabeçalho.
        let mut widths: Vec<usize> = Vec::with_capacity(visible.len());
        let mut minw: Vec<usize> = Vec::with_capacity(visible.len());
        for (k, &c) in visible.iter().enumerate() {
            let head = if self.headings {
                display_width(&self.columns[c].name)
            } else {
                0
            };
            let data = cells
                .iter()
                .map(|r| display_width(&r[k]))
                .max()
                .unwrap_or(0);
            let min = if self.headings { head.max(1) } else { 1 };
            let natural = if self.columns[c].noextreme {
                noextreme_width(&cells, k)
            } else {
                data
            };
            widths.push(natural.max(head).max(min));
            minw.push(min);
        }
        let total =
            |w: &[usize]| -> usize { w.iter().sum::<usize>() + sepw * w.len().saturating_sub(1) };
        let mut width = total(&widths);
        // Colunas -E ganham de volta o espaço que sobrar, até a maior célula.
        if width < termwidth {
            for (k, &c) in visible.iter().enumerate() {
                if !self.columns[c].noextreme {
                    continue;
                }
                let max = cells
                    .iter()
                    .map(|r| display_width(&r[k]))
                    .max()
                    .unwrap_or(0);
                let add = (termwidth - width).min(max.saturating_sub(widths[k]));
                widths[k] += add;
                width += add;
            }
        }
        if self.maxout && width < termwidth && !widths.is_empty() {
            'grow: loop {
                for w in widths.iter_mut() {
                    *w += 1;
                    width += 1;
                    if width >= termwidth {
                        break 'grow;
                    }
                }
            }
        }
        // Redução quando não cabe, como a libsmartcols se comporta (medido em caixa preta): encolhem as
        // colunas com -T, -W ou -E e também a última (que nunca é cortada na impressão, então isso só
        // decide quando parar). A cada rodada, da mais larga pra mais estreita (largura natural), a
        // coluna mais larga da tabela perde 3 e as outras 1, conferindo a largura depois de cada uma;
        // nenhuma passa do mínimo (o cabeçalho, ou 1).
        if width > termwidth && !visible.is_empty() {
            let natural = widths.clone();
            let widest =
                (0..natural.len()).fold(
                    0,
                    |best, k| if natural[k] > natural[best] { k } else { best },
                );
            let last = visible.len() - 1;
            let mut reducible: Vec<usize> = (0..visible.len())
                .filter(|&k| {
                    let col = &self.columns[visible[k]];
                    col.trunc || col.wrap || col.noextreme || k == last
                })
                .collect();
            reducible.sort_by(|a, b| natural[*b].cmp(&natural[*a]));
            while width > termwidth {
                let mut changed = false;
                for &k in &reducible {
                    if width <= termwidth {
                        break;
                    }
                    if widths[k] <= minw[k] {
                        continue;
                    }
                    let step = if k == widest { 3 } else { 1 };
                    let r = step.min(widths[k] - minw[k]);
                    widths[k] -= r;
                    width -= r;
                    changed = true;
                }
                if !changed {
                    break;
                }
            }
        }
        for (k, &c) in visible.iter().enumerate() {
            self.columns[c].width = widths[k];
        }

        let mut out = String::new();
        let nvis = visible.len();
        if self.headings && nvis > 0 {
            let heads: Vec<String> = visible
                .iter()
                .map(|&c| self.columns[c].name.clone())
                .collect();
            self.emit_row(&mut out, &visible, &heads, &widths);
        }
        for (i, row) in cells.iter().enumerate() {
            if i % 1024 == 0 {
                sysabi::sys::checkpoint();
            }
            self.emit_row(&mut out, &visible, row, &widths);
        }
        out
    }

    /// Escreve uma linha (ou várias, quando há quebra ou estouro de célula).
    fn emit_row(&self, out: &mut String, visible: &[usize], cells: &[String], widths: &[usize]) {
        let n = visible.len();
        // Cada coluna pode virar várias linhas físicas (wrap); estouro de célula sem truncar empurra o
        // resto da linha pra baixo, alinhado na coluna seguinte.
        let mut parts: Vec<Vec<String>> = Vec::with_capacity(n);
        for (k, &c) in visible.iter().enumerate() {
            let col = &self.columns[c];
            let text = &cells[k];
            let w = widths[k];
            if col.wrap && display_width(text) > w {
                parts.push(wrap_chunks(text, w));
            } else if col.trunc && display_width(text) > w {
                parts.push(vec![truncate_to(text, w)]);
            } else {
                parts.push(vec![text.clone()]);
            }
        }
        let sepw = display_width(&self.sep);
        let height = parts.iter().map(Vec::len).max().unwrap_or(1);
        for line in 0..height {
            // `pos` é a coluna do cursor; `start` é onde a coluna corrente começa. Célula maior que a
            // largura (coluna -E reduzida) não leva separador, e a coluna seguinte começa numa linha
            // nova, alinhada no lugar dela.
            let mut pos = 0usize;
            let mut start = 0usize;
            let mut overflowed = false;
            for k in 0..n {
                let col = &self.columns[visible[k]];
                let last = k + 1 == n;
                let w = widths[k];
                if overflowed {
                    out.push('\n');
                    out.push_str(&" ".repeat(start));
                    pos = start;
                    overflowed = false;
                }
                let text = parts[k].get(line).map(String::as_str).unwrap_or("");
                let tw = display_width(text);
                if col.right && tw < w && !(last && text.is_empty()) {
                    out.push_str(&" ".repeat(w - tw));
                    pos += w - tw;
                }
                out.push_str(text);
                pos += tw;
                let end = start + w;
                if !last {
                    if pos <= end {
                        out.push_str(&" ".repeat(end - pos));
                        out.push_str(&self.sep);
                        pos = end + sepw;
                    } else {
                        overflowed = true;
                    }
                    start = end + sepw;
                } else if self.maxout && pos < end {
                    out.push_str(&" ".repeat(end - pos));
                }
            }
            out.push('\n');
        }
    }

    fn to_json(&self) -> String {
        let mut out = String::new();
        out.push_str("{\n");
        out.push_str(&format!("   {}: [\n", json_string(&self.name)));
        let visible: Vec<usize> = self
            .order
            .iter()
            .copied()
            .filter(|&i| !self.columns[i].hidden)
            .collect();
        let roots: Vec<usize> = match &self.tree {
            Some(t) => t.roots.clone(),
            None => (0..self.rows.len()).collect(),
        };
        self.json_rows(&mut out, &roots, &visible, 6);
        out.push_str("   ]\n}\n");
        out
    }

    fn json_rows(&self, out: &mut String, rows: &[usize], visible: &[usize], indent: usize) {
        let pad = " ".repeat(indent);
        let inner = " ".repeat(indent + 3);
        for (n, &r) in rows.iter().enumerate() {
            if n == 0 {
                out.push_str(&format!("{pad}{{\n"));
            }
            let kids: &[usize] = self
                .tree
                .as_ref()
                .map(|t| t.children[r].as_slice())
                .unwrap_or(&[]);
            for (k, &c) in visible.iter().enumerate() {
                let col = &self.columns[c];
                let key = json_string(&col.name.to_lowercase());
                let comma = if k + 1 < visible.len() || !kids.is_empty() {
                    ","
                } else {
                    ""
                };
                let value = match &self.rows[r][c] {
                    None => "null".to_string(),
                    Some(v) => match col.json {
                        JsonType::String => json_string(v),
                        JsonType::Number => v.clone(),
                        // Falso é o que começa com `0`, `N` ou `n` (até "false" sai true, como no original).
                        JsonType::Boolean => {
                            if v.starts_with(['0', 'N', 'n']) {
                                "false".into()
                            } else {
                                "true".into()
                            }
                        }
                        JsonType::ArrayString | JsonType::ArrayNumber => {
                            let items: Vec<String> = v
                                .split('\n')
                                .map(|i| {
                                    if col.json == JsonType::ArrayString {
                                        json_string(i)
                                    } else {
                                        i.to_string()
                                    }
                                })
                                .collect();
                            let ipad = " ".repeat(indent + 7);
                            let mut s = String::from("[\n");
                            for (j, it) in items.iter().enumerate() {
                                s.push_str(&ipad);
                                s.push_str(it);
                                if j + 1 < items.len() {
                                    s.push(',');
                                }
                                s.push('\n');
                            }
                            s.push_str(&inner);
                            s.push(']');
                            s
                        }
                    },
                };
                out.push_str(&format!("{inner}{key}: {value}{comma}\n"));
            }
            if !kids.is_empty() {
                out.push_str(&format!("{inner}\"children\": [\n"));
                self.json_rows(out, kids, visible, indent + 6);
                out.push_str(&format!("{inner}]\n"));
            }
            if n + 1 < rows.len() {
                out.push_str(&format!("{pad}}},{{\n"));
            } else {
                out.push_str(&format!("{pad}}}\n"));
            }
        }
    }
}

/// Largura de uma coluna `-E`: ignora células maiores que a média (as "extremas").
fn noextreme_width(cells: &[Vec<String>], k: usize) -> usize {
    let ws: Vec<usize> = cells.iter().map(|r| display_width(&r[k])).collect();
    if ws.is_empty() {
        return 0;
    }
    let avg = ws.iter().sum::<usize>() / ws.len();
    ws.iter()
        .copied()
        .filter(|&w| w <= avg)
        .max()
        .unwrap_or(avg)
}

fn truncate_to(text: &str, w: usize) -> String {
    let mut out = String::new();
    let mut used = 0;
    for ch in text.chars() {
        let cw = display_width(ch.encode_utf8(&mut [0; 4]));
        if used + cw > w {
            break;
        }
        out.push(ch);
        used += cw;
    }
    out
}

fn wrap_chunks(text: &str, w: usize) -> Vec<String> {
    let w = w.max(1);
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut used = 0;
    for ch in text.chars() {
        let cw = display_width(ch.encode_utf8(&mut [0; 4]));
        if used + cw > w && !cur.is_empty() {
            out.push(std::mem::take(&mut cur));
            used = 0;
        }
        cur.push(ch);
        used += cw;
    }
    out.push(cur);
    out
}

/// String JSON como a libsmartcols escreve: aspas e barra escapadas, `\b \f \n \r \t` com escape
/// curto, outros controles como `\u00XX`, o resto (inclusive UTF-8 e DEL) cru.
fn json_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use sysabi::Program;
    use sysabi::testkit::TestKit;

    fn kit() -> TestKit {
        TestKit::new().programs([Program::bin("column", main)])
    }

    fn run(args: &[&str], stdin: &str) -> (String, String, i32) {
        let mut argv = vec!["column"];
        argv.extend_from_slice(args);
        let r = kit().run(&argv, stdin.as_bytes());
        (r.stdout_str(), r.stderr_str(), r.status.shell_status())
    }

    #[test]
    fn fill_columns_like_bsd_column() {
        let input: String = (1..=30).map(|i| format!("{i}\n")).collect();
        let (out, _, code) = run(&[], &input);
        assert_eq!(code, 0);
        assert_eq!(
            out,
            "1\t4\t7\t10\t13\t16\t19\t22\t25\t28\n2\t5\t8\t11\t14\t17\t20\t23\t26\t29\n3\t6\t9\t12\t15\t18\t21\t24\t27\t30\n"
        );
        let (out, _, _) = run(&["-x"], &input);
        assert!(out.starts_with("1\t2\t3\t4\t5\t6\t7\t8\t9\t10\n11\t"));
        let (out, _, _) = run(&["-S", "1"], "aaaaaaaaa\nb\nc\nd\ne\nf\ng\nh\ni\nj\n");
        assert_eq!(
            out,
            "aaaaaaaaa c         e         g         i\nb         d         f         h         j\n"
        );
        let (out, _, _) = run(&[], "one two  three\nfour\n\nfive\n");
        assert_eq!(out, "one two  three\tfour\t\tfive\n");
    }

    #[test]
    fn table_basics_match_util_linux() {
        let (out, _, _) = run(&["-t"], "a   b c\n  dd\teee    f  \nx\n");
        assert_eq!(out, "a   b    c\ndd  eee  f\nx        \n");
        let (out, _, _) = run(
            &["-t", "-s,"],
            "name,age,city\nana,30,são paulo\nbob,,rio\ncarl,5\n",
        );
        assert_eq!(
            out,
            "name  age  city\nana   30   são paulo\nbob        rio\ncarl  5    \n"
        );
        let (out, _, _) = run(&["-t", "-s,", "-o", " | "], "a,b,c\n1,22,333\n");
        assert_eq!(out, "a | b  | c\n1 | 22 | 333\n");
        let (out, _, _) = run(
            &["-t", "-s,", "-N", "X,YY,Z", "-R", "YY,3"],
            "a,b,c\n1,22,333\n",
        );
        assert_eq!(out, "X  YY    Z\na   b    c\n1  22  333\n");
        let (out, _, _) = run(
            &["-t", "-s,", "-N", "X,YY,Z", "-O", "Z,X"],
            "a,b,c\n1,22,333\n",
        );
        assert_eq!(out, "Z    X  YY\nc    a  b\n333  1  22\n");
        let (out, _, _) = run(&["-t", "-l", "2"], "a   b  c   d   \n");
        assert_eq!(out, "a  b  c   d   \n");
        let (out, _, _) = run(&["-t", "-s,", "-L"], "a,b\n\n1,2\n");
        assert_eq!(out, "a  b\n   \n1  2\n");
        let (out, _, _) = run(&["-t"], "\u{65e5}\u{672c} c\nd e\n");
        assert_eq!(out, "\u{65e5}\u{672c}  c\nd     e\n");
    }

    #[test]
    fn json_and_tree() {
        let (out, _, _) = run(&["-J", "-s,", "-N", "A,B,C"], "a,,c\n1,2,\n");
        assert_eq!(
            out,
            "{\n   \"table\": [\n      {\n         \"a\": \"a\",\n         \"b\": null,\n         \"c\": \"c\"\n      },{\n         \"a\": \"1\",\n         \"b\": \"2\",\n         \"c\": null\n      }\n   ]\n}\n"
        );
        let (out, _, _) = run(
            &["-t", "-N", "ID,P,NAME", "-r", "NAME", "-i", "ID", "-p", "P"],
            "1 0 r\n2 1 c1\n3 2 c2\n4 0 r2\n",
        );
        assert_eq!(
            out,
            "ID  P  NAME\n1   0  r\n2   1  \u{2514}\u{2500}c1\n3   2    \u{2514}\u{2500}c2\n4   0  r2\n"
        );
    }

    #[test]
    fn errors_and_exit_codes() {
        let (_, err, code) = run(&["--bogus"], "");
        assert_eq!(
            err,
            "column: unrecognized option '--bogus'\nTry 'column --help' for more information.\n"
        );
        assert_eq!(code, 1);
        let (_, err, code) = run(&["-J"], "a b\n");
        assert_eq!(
            err,
            "column: option --table-columns or --table-column required for --json\n"
        );
        assert_eq!(code, 1);
        let (_, err, _) = run(&["-c", "-5"], "");
        assert_eq!(
            err,
            "column: invalid columns argument: '-5': Numerical result out of range\n"
        );
        let (_, err, _) = run(&["-t", "-x"], "");
        assert_eq!(
            err,
            "column: mutually exclusive arguments: --table --fillrows\n"
        );
        let (_, err, code) = run(&["-t", "-R", "zz"], "a b c\n");
        assert_eq!(
            (err.as_str(), code),
            ("column: undefined column name 'zz'\n", 1)
        );
        let (out, err, code) = run(&["-t", "nope"], "");
        assert_eq!(
            (out.as_str(), err.as_str(), code),
            ("", "column: nope: No such file or directory\n", 1)
        );
    }

    #[test]
    fn invalid_utf8_becomes_hex_escape() {
        assert_eq!(safe_text(b"\xff\xfeab"), "\\xff\\xfeab");
    }
}
