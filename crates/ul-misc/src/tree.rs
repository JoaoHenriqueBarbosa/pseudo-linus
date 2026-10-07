//! `tree` 2.2.1 (pacote tree 2.2.1-1 do Debian 13).
//!
//! Comportamento levantado em caixa preta contra o Debian 13 (o tree é GPL e o código dele não foi
//! usado): parser de opções próprio, listagem com as linhas `├── `, `└── ` e `│` seguido de dois
//! espaços sem quebra, metadados entre colchetes, ordenação, filtros (`-P`, `-I`, `--gitignore`,
//! `--prune`, `--matchdirs`), cores pelo `LS_COLORS`, saída JSON e XML (inclusive as esquisitices do
//! original em erro), `--du`, `--fromfile` e `-o`.
//!
//! Pendente (ver `STATUS.md`): saída HTML (`-H`, `-T`, `-R`, `--nolinks`, `--hintro`, `--houtro`),
//! `--info`/`--infofile`, `--hyperlink`, `--fromtabfile` e `--opt-toggle`.

use std::collections::BTreeSet;
use std::ffi::OsString;
use std::io::Write;

use sysabi::{AtFlags, Ctx, Errno, Fd, FileType, Mode, OFlags, Stat, mode, sys};
use ul_common::fnmatch::{Bytes, Flags, fnmatch};
use ul_common::fsutil::after_last_slash;

use crate::util::io::{self, File};
use crate::util::time;

const VERSION: &str = "tree v2.2.1 \u{a9} 1996 - 2024 by Steve Baker, Thomas Moore, Francesc Rocher, Florian Sesser, Kyosuke Tokoro\n";

const USAGE: &str = "usage: tree [-acdfghilnpqrstuvxACDFJQNSUX] [-L level [-R]] [-H [-]baseHREF]
\t[-T title] [-o filename] [-P pattern] [-I pattern] [--gitignore]
\t[--gitfile[=]file] [--matchdirs] [--metafirst] [--ignore-case]
\t[--nolinks] [--hintro[=]file] [--houtro[=]file] [--inodes] [--device]
\t[--sort[=]name] [--dirsfirst] [--filesfirst] [--filelimit[=]#] [--si]
\t[--du] [--prune] [--charset[=]X] [--timefmt[=]format] [--fromfile]
\t[--fromtabfile] [--fflinks] [--info] [--infofile[=]file] [--noreport]
\t[--hyperlink] [--scheme[=]schema] [--authority[=]host] [--opt-toggle]
\t[--version] [--help] [--] [directory ...]
";

const HELP: &str = "  ------- Listing options -------
  -a            All files are listed.
  -d            List directories only.
  -l            Follow symbolic links like directories.
  -f            Print the full path prefix for each file.
  -x            Stay on current filesystem only.
  -L level      Descend only level directories deep.
  -R            Rerun tree when max dir level reached.
  -P pattern    List only those files that match the pattern given.
  -I pattern    Do not list files that match the given pattern.
  --gitignore   Filter by using .gitignore files.
  --gitfile X   Explicitly read a gitignore file.
  --ignore-case Ignore case when pattern matching.
  --matchdirs   Include directory names in -P pattern matching.
  --metafirst   Print meta-data at the beginning of each line.
  --prune       Prune empty directories from the output.
  --info        Print information about files found in .info files.
  --infofile X  Explicitly read info file.
  --noreport    Turn off file/directory count at end of tree listing.
  --charset X   Use charset X for terminal/HTML and indentation line output.
  --filelimit # Do not descend dirs with more than # files in them.
  -o filename   Output to file instead of stdout.
  ------- File options -------
  -q            Print non-printable characters as '?'.
  -N            Print non-printable characters as is.
  -Q            Quote filenames with double quotes.
  -p            Print the protections for each file.
  -u            Displays file owner or UID number.
  -g            Displays file group owner or GID number.
  -s            Print the size in bytes of each file.
  -h            Print the size in a more human readable way.
  --si          Like -h, but use in SI units (powers of 1000).
  --du          Compute size of directories by their contents.
  -D            Print the date of last modification or (-c) status change.
  --timefmt fmt Print and format time according to the format fmt.
  -F            Appends '/', '=', '*', '@', '|' or '>' as per ls -F.
  --inodes      Print inode number of each file.
  --device      Print device ID number to which each file belongs.
  ------- Sorting options -------
  -v            Sort files alphanumerically by version.
  -t            Sort files by last modification time.
  -c            Sort files by last status change time.
  -U            Leave files unsorted.
  -r            Reverse the order of the sort.
  --dirsfirst   List directories before files (-U disables).
  --filesfirst  List files before directories (-U disables).
  --sort X      Select sort: name,version,size,mtime,ctime,none.
  ------- Graphics options -------
  -i            Don't print indentation lines.
  -A            Print ANSI lines graphic indentation lines.
  -S            Print with CP437 (console) graphics indentation lines.
  -n            Turn colorization off always (-C overrides).
  -C            Turn colorization on always.
  ------- XML/HTML/JSON/HYPERLINK options -------
  -X            Prints out an XML representation of the tree.
  -J            Prints out an JSON representation of the tree.
  -H baseHREF   Prints out HTML format with baseHREF as top directory.
  -T string     Replace the default HTML title and H1 header with string.
  --nolinks     Turn off hyperlinks in HTML output.
  --hintro X    Use file X as the HTML intro.
  --houtro X    Use file X as the HTML outro.
  --hyperlink   Turn on OSC 8 terminal hyperlinks.
  --scheme X    Set OSC 8 hyperlink scheme, default file://
  --authority X Set OSC 8 hyperlink authority/hostname.
  ------- Input options -------
  --fromfile    Reads paths from files (.=stdin)
  --fromtabfile Reads trees from tab indented files (.=stdin)
  --fflinks     Process link information when using --fromfile.
  ------- Miscellaneous options -------
  --opt-toggle  Enable option toggling.
  --version     Print version and exit.
  --help        Print usage and this help message and exit.
  --            Options processing terminator.
";

/// Charsets que o `--charset` sem argumento lista (a tabela embutida do tree).
const CHARSETS: &[&str] = &[
    "ISO-8859-1",
    "ISO-8859-1:1987",
    "ISO_8859-1",
    "latin1",
    "l1",
    "IBM819",
    "CP819",
    "csISOLatin1",
    "ISO-8859-3",
    "ISO_8859-3:1988",
    "ISO_8859-3",
    "latin3",
    "ls",
    "csISOLatin3",
    "ISO-8859-7",
    "ISO_8859-7:1987",
    "ISO_8859-7",
    "ELOT_928",
    "ECMA-118",
    "greek",
    "greek8",
    "csISOLatinGreek",
    "ISO-8859-8",
    "ISO_8859-8:1988",
    "iso-ir-138",
    "ISO_8859-8",
    "hebrew",
    "csISOLatinHebrew",
    "ISO-8859-9",
    "ISO_8859-9:1989",
    "iso-ir-148",
    "ISO_8859-9",
    "latin5",
    "l5",
    "csISOLatin5",
    "Shift_JIS",
    "MS_Kanji",
    "csShiftJIS",
    "EUC-JP",
    "Extended_UNIX_Code_Packed_Format_for_Japanese",
    "csEUCPkdFmtJapanese",
    "EUC-KR",
    "csEUCKR",
    "ISO-2022-JP",
    "csISO2022JP",
    "ISO-2022-JP-2",
    "csISO2022JP2",
    "IBM437",
    "cp437",
    "437",
    "csPC8CodePage437",
    "IBM852",
    "cp852",
    "852",
    "csPCp852",
    "IBM863",
    "cp863",
    "863",
    "csIBM863",
    "IBM855",
    "cp855",
    "855",
    "csIBM855",
    "IBM865",
    "cp865",
    "865",
    "csIBM865",
    "IBM866",
    "cp866",
    "866",
    "csIBM866",
    "IBM850",
    "cp850",
    "850",
    "csPC850Multilingual",
    "IBM00858",
    "CCSID00858",
    "CP00858",
    "PC-Multilingual-850+euro",
    "IBM869",
    "cp869",
    "869",
    "cp-gr",
    "csIBM869",
    "GB2312",
    "csGB2312",
    "UTF-8",
    "utf8",
    "Big5",
    "csBig5",
    "VISCII",
    "csVISCII",
    "KOI8-R",
    "csKOI8R",
    "KOI8-U",
    "ISO-8859-1-Windows-3.1-Latin-1",
    "csWindows31Latin1",
    "ISO-8859-2-Windows-Latin-2",
    "csWindows31Latin2",
    "windows-1250",
    "windows-1251",
    "windows-1253",
    "windows-1254",
    "windows-1255",
    "windows-1256",
    "windows-1256",
    "windows-1257",
];

/// Cores padrão quando não há `LS_COLORS` nem `TREE_COLORS`: o banco do `dircolors` do coreutils 9.7,
/// com arquivo comum em `00` (o tree pinta arquivo comum nesse caso, observado no Debian 13).
const DEFAULT_COLORS: &str = "no=00:fi=00:rs=0:di=01;34:ln=01;36:mh=00:pi=40;33:so=01;35:do=01;35:bd=40;33;01:cd=40;33;01:or=40;31;01:mi=00:su=37;41:sg=30;43:ca=00:tw=30;42:ow=34;42:st=37;44:ex=01;32:*.7z=01;31:*.ace=01;31:*.alz=01;31:*.apk=01;31:*.arc=01;31:*.arj=01;31:*.bz=01;31:*.bz2=01;31:*.cab=01;31:*.cpio=01;31:*.crate=01;31:*.deb=01;31:*.drpm=01;31:*.dwm=01;31:*.dz=01;31:*.ear=01;31:*.egg=01;31:*.esd=01;31:*.gz=01;31:*.jar=01;31:*.lha=01;31:*.lrz=01;31:*.lz=01;31:*.lz4=01;31:*.lzh=01;31:*.lzma=01;31:*.lzo=01;31:*.pyz=01;31:*.rar=01;31:*.rpm=01;31:*.rz=01;31:*.sar=01;31:*.swm=01;31:*.t7z=01;31:*.tar=01;31:*.taz=01;31:*.tbz=01;31:*.tbz2=01;31:*.tgz=01;31:*.tlz=01;31:*.txz=01;31:*.tz=01;31:*.tzo=01;31:*.tzst=01;31:*.udeb=01;31:*.war=01;31:*.whl=01;31:*.wim=01;31:*.xz=01;31:*.z=01;31:*.zip=01;31:*.zoo=01;31:*.zst=01;31:*.avif=01;35:*.jpg=01;35:*.jpeg=01;35:*.jxl=01;35:*.mjpg=01;35:*.mjpeg=01;35:*.gif=01;35:*.bmp=01;35:*.pbm=01;35:*.pgm=01;35:*.ppm=01;35:*.tga=01;35:*.xbm=01;35:*.xpm=01;35:*.tif=01;35:*.tiff=01;35:*.png=01;35:*.svg=01;35:*.svgz=01;35:*.mng=01;35:*.pcx=01;35:*.mov=01;35:*.mpg=01;35:*.mpeg=01;35:*.m2v=01;35:*.mkv=01;35:*.webm=01;35:*.webp=01;35:*.ogm=01;35:*.mp4=01;35:*.m4v=01;35:*.mp4v=01;35:*.vob=01;35:*.qt=01;35:*.nuv=01;35:*.wmv=01;35:*.asf=01;35:*.rm=01;35:*.rmvb=01;35:*.flc=01;35:*.avi=01;35:*.fli=01;35:*.flv=01;35:*.gl=01;35:*.dl=01;35:*.xcf=01;35:*.xwd=01;35:*.yuv=01;35:*.cgm=01;35:*.emf=01;35:*.ogv=01;35:*.ogx=01;35:*.aac=00;36:*.au=00;36:*.flac=00;36:*.m4a=00;36:*.mid=00;36:*.midi=00;36:*.mka=00;36:*.mp3=00;36:*.mpc=00;36:*.ogg=00;36:*.ra=00;36:*.wav=00;36:*.oga=00;36:*.opus=00;36:*.spx=00;36:*.xspf=00;36:*~=00;90:*#=00;90:*.bak=00;90:*.crdownload=00;90:*.dpkg-dist=00;90:*.dpkg-new=00;90:*.dpkg-old=00;90:*.dpkg-tmp=00;90:*.old=00;90:*.orig=00;90:*.part=00;90:*.rej=00;90:*.rpmnew=00;90:*.rpmorig=00;90:*.rpmsave=00;90:*.swp=00;90:*.tmp=00;90:*.ucf-dist=00;90:*.ucf-new=00;90:*.ucf-old=00;90:";

/// Seis meses do tree: data mais antiga que isso (ou no futuro) sai com o ano.
const SIX_MONTHS: i64 = 6 * 31 * 24 * 60 * 60;

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum Lines {
    Utf8,
    Ascii,
    Ansi,
    Cp437,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum Escape {
    Octal,
    Question,
    Raw,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum SortKey {
    Name,
    Version,
    Size,
    Mtime,
    Ctime,
    None,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum Format {
    Text,
    Json,
    Xml,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum ColorMode {
    Auto,
    Never,
    Always,
}

struct Opts {
    all: bool,
    dirs_only: bool,
    follow: bool,
    full_path: bool,
    xdev: bool,
    max_level: Option<usize>,
    patterns: Vec<Vec<u8>>,
    ignores: Vec<Vec<u8>>,
    gitignore: bool,
    gitfiles: Vec<Vec<u8>>,
    ignore_case: bool,
    matchdirs: bool,
    metafirst: bool,
    prune: bool,
    noreport: bool,
    lines: Option<Lines>,
    filelimit: usize,
    outfile: Option<Vec<u8>>,
    escape: Escape,
    quote: bool,
    perms: bool,
    user: bool,
    group: bool,
    size: bool,
    human: bool,
    si: bool,
    du: bool,
    date: bool,
    timefmt: Option<Vec<u8>>,
    ctime: bool,
    classify: bool,
    inodes: bool,
    device: bool,
    sort: SortKey,
    reverse: bool,
    dirsfirst: bool,
    filesfirst: bool,
    no_indent: bool,
    color: ColorMode,
    format: Format,
    fromfile: bool,
}

impl Default for Opts {
    fn default() -> Opts {
        Opts {
            all: false,
            dirs_only: false,
            follow: false,
            full_path: false,
            xdev: false,
            max_level: None,
            patterns: Vec::new(),
            ignores: Vec::new(),
            gitignore: false,
            gitfiles: Vec::new(),
            ignore_case: false,
            matchdirs: false,
            metafirst: false,
            prune: false,
            noreport: false,
            lines: None,
            filelimit: 0,
            outfile: None,
            escape: Escape::Octal,
            quote: false,
            perms: false,
            user: false,
            group: false,
            size: false,
            human: false,
            si: false,
            du: false,
            date: false,
            timefmt: None,
            ctime: false,
            classify: false,
            inodes: false,
            device: false,
            sort: SortKey::Name,
            reverse: false,
            dirsfirst: false,
            filesfirst: false,
            no_indent: false,
            color: ColorMode::Auto,
            format: Format::Text,
            fromfile: false,
        }
    }
}

impl Opts {
    fn any_meta(&self) -> bool {
        self.inodes
            || self.device
            || self.perms
            || self.user
            || self.group
            || self.size
            || self.date
    }
}

/// Resultado do parser: rodar, ou sair já com um código (help, versão, erro).
enum Parsed {
    Run(Box<Opts>, Vec<Vec<u8>>),
    Exit(i32),
}

fn fatal(msg: &str) -> Parsed {
    io::eprint(format!("tree: {msg}\n"));
    Parsed::Exit(1)
}

fn invalid(arg: &str) -> Parsed {
    io::eprint(format!("tree: Invalid argument {arg}.\n{USAGE}"));
    Parsed::Exit(1)
}

fn parse_level(s: &[u8]) -> Option<usize> {
    // atoi: dígitos do começo; zero, negativo ou lixo dão erro.
    let text = String::from_utf8_lossy(s);
    let t = text.trim_start();
    let digits: String = t.chars().take_while(char::is_ascii_digit).collect();
    match digits.parse::<usize>() {
        Ok(n) if n > 0 && !t.starts_with('-') => Some(n),
        _ => None,
    }
}

fn atoi(s: &[u8]) -> usize {
    let text = String::from_utf8_lossy(s);
    let digits: String = text
        .trim_start()
        .chars()
        .take_while(char::is_ascii_digit)
        .collect();
    digits.parse().unwrap_or(0)
}

fn parse_args(argv: &[Vec<u8>]) -> Parsed {
    let mut o = Opts::default();
    let mut dirs: Vec<Vec<u8>> = Vec::new();
    let mut i = 1;
    let mut only_dirs = false;
    let take_next = |i: &mut usize| -> Option<Vec<u8>> {
        let v = argv.get(*i).cloned();
        if v.is_some() {
            *i += 1;
        }
        v
    };
    while i < argv.len() {
        let arg = argv[i].clone();
        i += 1;
        if only_dirs || arg.len() < 2 || arg[0] != b'-' {
            dirs.push(arg);
            continue;
        }
        if arg == b"--" {
            only_dirs = true;
            continue;
        }
        if arg.starts_with(b"--") {
            let body = String::from_utf8_lossy(&arg[2..]).into_owned();
            let (name, inline) = match body.split_once('=') {
                Some((n, v)) => (n.to_string(), Some(v.as_bytes().to_vec())),
                None => (body.clone(), None),
            };
            // Opções com `[=]`: valor depois do `=` ou no próximo argumento.
            let valued = [
                "gitfile",
                "hintro",
                "houtro",
                "sort",
                "filelimit",
                "charset",
                "timefmt",
                "infofile",
                "scheme",
                "authority",
            ];
            if valued.contains(&name.as_str()) {
                let value = match inline {
                    Some(v) => Some(v),
                    None => take_next(&mut i),
                };
                let Some(value) = value else {
                    if name == "charset" {
                        let mut msg = String::from(
                            "tree: Missing argument to --charset\nValid charsets include:\n",
                        );
                        for c in CHARSETS {
                            msg.push_str(&format!("  {c}\n"));
                        }
                        io::eprint(msg);
                        return Parsed::Exit(1);
                    }
                    return fatal(&format!("Missing argument to --{name}"));
                };
                match name.as_str() {
                    "gitfile" => {
                        o.gitignore = true;
                        o.gitfiles.push(value);
                    }
                    "sort" => {
                        o.sort = match value.as_slice() {
                            b"name" => SortKey::Name,
                            b"version" => SortKey::Version,
                            b"size" => SortKey::Size,
                            b"mtime" => SortKey::Mtime,
                            b"ctime" => SortKey::Ctime,
                            b"none" => SortKey::None,
                            other => {
                                // O original escreve o começo no stderr e a lista no stdout.
                                io::eprint(format!(
                                    "tree: Sort type '{}' not valid, should be one of: ",
                                    io::lossy(other)
                                ));
                                let mut out = io::stdout();
                                let _ = out.write_all(b"name,version,size,mtime,ctime,none\n");
                                return Parsed::Exit(1);
                            }
                        }
                    }
                    "filelimit" => o.filelimit = atoi(&value),
                    "charset" => {
                        let v = String::from_utf8_lossy(&value).to_ascii_lowercase();
                        o.lines = Some(match v.as_str() {
                            "utf-8" | "utf8" => Lines::Utf8,
                            "ibm437" | "cp437" | "437" | "cspc8codepage437" => Lines::Cp437,
                            _ => Lines::Ascii,
                        });
                    }
                    "timefmt" => {
                        o.timefmt = Some(value);
                        o.date = true;
                    }
                    // HTML, --info e hyperlink: aceitos e sem efeito na saída de texto (pendentes).
                    _ => {}
                }
                continue;
            }
            if inline.is_some() {
                return invalid(&format!("`--{body}'"));
            }
            match name.as_str() {
                "help" => {
                    let mut out = io::stdout();
                    let _ = out.write_all(USAGE.as_bytes());
                    let _ = out.write_all(HELP.as_bytes());
                    return Parsed::Exit(0);
                }
                "version" => {
                    let mut out = io::stdout();
                    let _ = out.write_all(VERSION.as_bytes());
                    return Parsed::Exit(0);
                }
                "inodes" => o.inodes = true,
                "device" => o.device = true,
                "noreport" => o.noreport = true,
                "nolinks" | "hyperlink" | "info" | "fflinks" | "opt-toggle" | "fromtabfile" => {}
                "dirsfirst" => o.dirsfirst = true,
                "filesfirst" => o.filesfirst = true,
                "ignore-case" => o.ignore_case = true,
                "matchdirs" => o.matchdirs = true,
                "metafirst" => o.metafirst = true,
                "si" => {
                    o.si = true;
                    o.size = true;
                }
                "du" => {
                    o.du = true;
                    o.size = true;
                }
                "prune" => o.prune = true,
                "gitignore" => o.gitignore = true,
                "fromfile" => o.fromfile = true,
                _ => return invalid(&format!("`--{body}'")),
            }
            continue;
        }
        // Agrupamento de opções curtas.
        let mut j = 1;
        while j < arg.len() {
            let c = arg[j];
            j += 1;
            match c {
                b'a' => o.all = true,
                b'd' => o.dirs_only = true,
                b'l' => o.follow = true,
                b'f' => o.full_path = true,
                b'x' => o.xdev = true,
                b'R' => {}
                b'q' => o.escape = Escape::Question,
                b'N' => o.escape = Escape::Raw,
                b'Q' => o.quote = true,
                b'p' => o.perms = true,
                b'u' => o.user = true,
                b'g' => o.group = true,
                b's' => o.size = true,
                b'h' => {
                    o.human = true;
                    o.size = true;
                }
                b'D' => o.date = true,
                b'F' => o.classify = true,
                b'v' => o.sort = SortKey::Version,
                b't' => o.sort = SortKey::Mtime,
                b'c' => {
                    o.sort = SortKey::Ctime;
                    o.ctime = true;
                }
                b'U' => o.sort = SortKey::None,
                b'r' => o.reverse = true,
                b'i' => o.no_indent = true,
                b'A' => o.lines = Some(Lines::Ansi),
                b'S' => o.lines = Some(Lines::Cp437),
                b'n' => o.color = ColorMode::Never,
                b'C' => o.color = ColorMode::Always,
                b'J' => o.format = Format::Json,
                b'X' => o.format = Format::Xml,
                b'L' => {
                    let value = if arg.get(j).is_some_and(u8::is_ascii_digit) {
                        let v = arg[j..].to_vec();
                        j = arg.len();
                        Some(v)
                    } else {
                        take_next(&mut i)
                    };
                    let Some(value) = value else {
                        return fatal("Missing argument to -L option.");
                    };
                    match parse_level(&value) {
                        Some(n) => o.max_level = Some(n),
                        None => return fatal("Invalid level, must be greater than 0."),
                    }
                }
                b'P' | b'I' | b'o' | b'H' | b'T' => {
                    let Some(value) = take_next(&mut i) else {
                        return fatal(&format!("Missing argument to -{} option.", char::from(c)));
                    };
                    match c {
                        b'P' => o.patterns.push(value),
                        b'I' => o.ignores.push(value),
                        b'o' => o.outfile = Some(value),
                        // -H/-T (HTML): pendentes.
                        _ => {}
                    }
                }
                other => {
                    let shown = if other.is_ascii() {
                        char::from(other).to_string()
                    } else {
                        format!("\\x{other:02x}")
                    };
                    return invalid(&format!("-`{shown}'"));
                }
            }
        }
    }
    Parsed::Run(Box::new(o), dirs)
}

// ------------------------------------------------------------------------------------------------
// Padrões (-P, -I, gitignore)

/// Casamento de padrão do tree: `*`, `?`, `[...]` (com `^`/`!` e faixas), `\` escapa, e `|` separa
/// alternativas.
fn pattern_match(pattern: &[u8], text: &[u8], icase: bool) -> bool {
    split_alternatives(pattern)
        .iter()
        .any(|alt| fnmatch::<Bytes>(alt, text, tree_flags(icase)))
}

fn split_alternatives(p: &[u8]) -> Vec<Vec<u8>> {
    let mut out = vec![Vec::new()];
    let mut i = 0;
    while i < p.len() {
        match p[i] {
            b'\\' if i + 1 < p.len() => {
                let last = out.last_mut().expect("existe");
                last.push(p[i]);
                last.push(p[i + 1]);
                i += 2;
                continue;
            }
            b'|' => out.push(Vec::new()),
            c => out.last_mut().expect("existe").push(c),
        }
        i += 1;
    }
    out
}

/// Flags do matcher próprio do tree: lista `[...]` simples (sem classes nem `\` dentro) e `\` final
/// literal; `icase` liga a comparação sem distinguir maiúsculas.
const fn tree_flags(icase: bool) -> Flags {
    Flags::PLAIN_BRACKET.with(Flags::TRAILING_BACKSLASH_LITERAL, true).with(Flags::CASEFOLD, icase)
}

/// Uma regra de `.gitignore`.
#[derive(Clone, Debug)]
struct GitRule {
    /// Diretório (com `/` no fim) onde o arquivo de regras mora; o caminho testado é relativo a ele.
    base: Vec<u8>,
    pattern: Vec<u8>,
    negate: bool,
    dir_only: bool,
    /// Tem `/` no meio ou no começo: casa contra o caminho relativo, não só o nome.
    anchored: bool,
}

fn parse_gitignore(data: &[u8], base: &[u8]) -> Vec<GitRule> {
    let mut rules = Vec::new();
    for raw in data.split(|b| *b == b'\n') {
        let mut line = raw.to_vec();
        if line.last() == Some(&b'\r') {
            line.pop();
        }
        // Espaço no fim só conta escapado.
        while line.last() == Some(&b' ') && !(line.len() >= 2 && line[line.len() - 2] == b'\\') {
            line.pop();
        }
        if line.is_empty() || line[0] == b'#' {
            continue;
        }
        let mut negate = false;
        if line[0] == b'!' {
            negate = true;
            line.remove(0);
        } else if line.starts_with(b"\\!") || line.starts_with(b"\\#") {
            line.remove(0);
        }
        let mut dir_only = false;
        if line.last() == Some(&b'/') {
            dir_only = true;
            line.pop();
        }
        if line.is_empty() {
            continue;
        }
        let anchored = line.contains(&b'/');
        if line[0] == b'/' {
            line.remove(0);
        }
        rules.push(GitRule {
            base: base.to_vec(),
            pattern: line,
            negate,
            dir_only,
            anchored,
        });
    }
    rules
}

/// Casamento com `**` do gitignore sobre caminho relativo.
fn git_glob(p: &[u8], t: &[u8]) -> bool {
    if let Some(rest) = p.strip_prefix(b"**/") {
        if git_glob(rest, t) {
            return true;
        }
        return t
            .iter()
            .enumerate()
            .any(|(i, b)| *b == b'/' && git_glob(rest, &t[i + 1..]));
    }
    if p == b"**" {
        return true;
    }
    if let Some(pos) = find_sub(p, b"/**/") {
        let (head, tail) = (&p[..pos], &p[pos + 4..]);
        for (i, b) in t.iter().enumerate() {
            if *b == b'/' && glob_path(head, &t[..i]) {
                let rest = &t[i + 1..];
                if git_glob(tail, rest) {
                    return true;
                }
                if rest
                    .iter()
                    .enumerate()
                    .any(|(k, c)| *c == b'/' && git_glob(tail, &rest[k + 1..]))
                {
                    return true;
                }
            }
        }
        return false;
    }
    if let Some(head) = p.strip_suffix(b"/**") {
        return t
            .iter()
            .enumerate()
            .any(|(i, b)| *b == b'/' && glob_path(head, &t[..i]));
    }
    glob_path(p, t)
}

fn find_sub(h: &[u8], n: &[u8]) -> Option<usize> {
    h.windows(n.len()).position(|w| w == n)
}

/// Glob em que `*` e `?` não atravessam `/`.
fn glob_path(p: &[u8], t: &[u8]) -> bool {
    let ps: Vec<&[u8]> = p.split(|b| *b == b'/').collect();
    let ts: Vec<&[u8]> = t.split(|b| *b == b'/').collect();
    ps.len() == ts.len() && ps.iter().zip(&ts).all(|(a, b)| fnmatch::<Bytes>(a, b, tree_flags(false)))
}

fn git_ignored(rules: &[GitRule], path: &[u8], name: &[u8], is_dir: bool) -> bool {
    let mut ignored = false;
    for r in rules {
        if r.dir_only && !is_dir {
            continue;
        }
        let Some(rel) = path.strip_prefix(r.base.as_slice()) else {
            continue;
        };
        let hit = if r.anchored {
            git_glob(&r.pattern, rel)
        } else {
            fnmatch::<Bytes>(&r.pattern, name, tree_flags(false))
        };
        if hit {
            ignored = !r.negate;
        }
    }
    ignored
}

// ------------------------------------------------------------------------------------------------
// Árvore em memória

#[derive(Clone, Debug, PartialEq, Eq)]
enum Note {
    None,
    Recursive,
    FileLimit(usize),
    ErrorOpening,
}

#[derive(Clone, Debug)]
struct Node {
    /// Nome como aparece (base, ou o argumento da raiz, ou o caminho com `-f`).
    name: Vec<u8>,
    /// Caminho pras syscalls.
    path: Vec<u8>,
    lst: Option<Stat>,
    target: Option<Vec<u8>>,
    /// stat do alvo, pra links.
    tst: Option<Stat>,
    /// Diretório de verdade ou link que aponta pra diretório.
    is_dir: bool,
    children: Option<Vec<Node>>,
    note: Note,
    du: u64,
    /// Nó virtual do `--fromfile`: sem stat.
    virtual_node: bool,
    /// Diretório que casou com `-P` sob `--matchdirs`: tudo abaixo dele aparece.
    matched: bool,
}

impl Node {
    fn is_link(&self) -> bool {
        self.lst
            .as_ref()
            .is_some_and(|s| s.file_type() == FileType::Symlink)
    }

    fn is_real_dir(&self) -> bool {
        if self.virtual_node {
            return self.is_dir;
        }
        self.lst
            .as_ref()
            .is_some_and(|s| s.file_type() == FileType::Directory)
    }

}

struct Walker<'a> {
    o: &'a Opts,
    visited: BTreeSet<(u64, u64)>,
    root_dev: u64,
}

fn join(dir: &[u8], name: &[u8]) -> Vec<u8> {
    let mut p = dir.to_vec();
    if !p.ends_with(b"/") {
        p.push(b'/');
    }
    p.extend_from_slice(name);
    p
}

fn lstat(path: &[u8]) -> Option<Stat> {
    sys::try_current()?
        .fstatat(Fd::CWD, path, AtFlags::SYMLINK_NOFOLLOW)
        .ok()
}

fn stat(path: &[u8]) -> Option<Stat> {
    sys::try_current()?
        .fstatat(Fd::CWD, path, AtFlags::empty())
        .ok()
}

fn readlink(path: &[u8]) -> Option<Vec<u8>> {
    sys::try_current()?.readlinkat(Fd::CWD, path).ok()
}

impl Walker<'_> {
    fn pattern_hit(&self, list: &[Vec<u8>], name: &[u8], path: &[u8]) -> bool {
        list.iter().any(|p| {
            let subject = if p.contains(&b'/') { path } else { name };
            pattern_match(p, subject, self.o.ignore_case)
        })
    }

    /// Lê e filtra um diretório, e desce recursivamente.
    fn list(
        &mut self,
        dir: &Node,
        depth: usize,
        matched: bool,
        rules: &[GitRule],
    ) -> (Option<Vec<Node>>, Note) {
        let entries = match sys::read_dir(&dir.path) {
            Ok(e) => e,
            Err(_) => return (None, Note::ErrorOpening),
        };
        let mut rules: Vec<GitRule> = rules.to_vec();
        if self.o.gitignore {
            let gi = join(&dir.path, b".gitignore");
            if let Ok(data) = io::read_path(&gi) {
                let mut base = dir.path.clone();
                if !base.ends_with(b"/") {
                    base.push(b'/');
                }
                rules.extend(parse_gitignore(&data, &base));
            }
        }
        let mut kids: Vec<Node> = Vec::new();
        for (n, e) in entries.into_iter().enumerate() {
            if n % 256 == 0 {
                sys::checkpoint();
            }
            let name = e.name;
            if !self.o.all && name.first() == Some(&b'.') {
                continue;
            }
            let path = join(&dir.path, &name);
            let lst = lstat(&path);
            let is_link = lst
                .as_ref()
                .is_some_and(|s| s.file_type() == FileType::Symlink);
            let (target, tst) = if is_link {
                (readlink(&path), stat(&path))
            } else {
                (None, None)
            };
            let real_dir = lst
                .as_ref()
                .is_some_and(|s| s.file_type() == FileType::Directory);
            let is_dir = real_dir
                || (is_link
                    && tst
                        .as_ref()
                        .is_some_and(|s| s.file_type() == FileType::Directory));
            if self.pattern_hit(&self.o.ignores, &name, &path) {
                continue;
            }
            if !rules.is_empty() && git_ignored(&rules, &path, &name, is_dir) {
                continue;
            }
            if self.o.dirs_only && !is_dir {
                continue;
            }
            let treat_as_dir = real_dir || (is_link && is_dir && self.o.follow);
            // Com -P, diretório sempre aparece; com --matchdirs, o diretório que casa mostra todos os
            // arquivos dele (só os diretos: abaixo disso o filtro volta a valer).
            let mut child_matched = false;
            if !self.o.patterns.is_empty() {
                if treat_as_dir {
                    child_matched =
                        self.o.matchdirs && self.pattern_hit(&self.o.patterns, &name, &path);
                } else if !matched && !self.pattern_hit(&self.o.patterns, &name, &path) {
                    continue;
                }
            }
            let shown = if self.o.full_path {
                path.clone()
            } else {
                name.clone()
            };
            kids.push(Node {
                name: shown,
                path,
                lst,
                target,
                tst,
                is_dir,
                children: None,
                note: Note::None,
                du: 0,
                virtual_node: false,
                matched: child_matched,
            });
        }
        if self.o.filelimit > 0 && kids.len() > self.o.filelimit {
            return (None, Note::FileLimit(kids.len()));
        }
        sort_nodes(&mut kids, self.o);
        for kid in &mut kids {
            let inherited = kid.matched;
            let real_dir = kid.is_real_dir();
            let link_dir = kid.is_link() && kid.is_dir;
            if !(real_dir || (link_dir && self.o.follow)) {
                continue;
            }
            if self.o.max_level.is_some_and(|m| depth + 1 >= m) {
                continue;
            }
            let st = if link_dir {
                kid.tst.clone()
            } else {
                kid.lst.clone()
            };
            if let Some(st) = &st {
                if self.o.xdev && st.dev != self.root_dev {
                    continue;
                }
                if link_dir && self.visited.contains(&(st.dev, st.ino)) {
                    kid.note = Note::Recursive;
                    continue;
                }
                self.visited.insert((st.dev, st.ino));
            }
            let (children, note) = self.list(kid, depth + 1, inherited, &rules);
            kid.children = children;
            if note != Note::ErrorOpening {
                kid.note = note;
            }
        }
        (Some(kids), Note::None)
    }
}

fn version_cmp(a: &[u8], b: &[u8]) -> std::cmp::Ordering {
    use std::cmp::Ordering;
    // strverscmp: compara pedaços numéricos pelo valor (com a regra dos zeros à esquerda da glibc).
    let (mut i, mut j) = (0, 0);
    while i < a.len() && j < b.len() {
        if a[i].is_ascii_digit() && b[j].is_ascii_digit() {
            let si = i;
            while i < a.len() && a[i].is_ascii_digit() {
                i += 1;
            }
            let sj = j;
            while j < b.len() && b[j].is_ascii_digit() {
                j += 1;
            }
            let (na, nb) = (&a[si..i], &b[sj..j]);
            let fa = na.len() > 1 && na[0] == b'0';
            let fb = nb.len() > 1 && nb[0] == b'0';
            let ord = if fa || fb {
                // Parte fracionária: comparação lexicográfica, e a mais curta que é prefixo vem depois.
                match na.cmp(nb) {
                    Ordering::Equal => Ordering::Equal,
                    o if na.starts_with(nb) || nb.starts_with(na) => o.reverse(),
                    o => o,
                }
            } else {
                na.len().cmp(&nb.len()).then(na.cmp(nb))
            };
            if ord != Ordering::Equal {
                return ord;
            }
        } else {
            if a[i] != b[j] {
                return a[i].cmp(&b[j]);
            }
            i += 1;
            j += 1;
        }
    }
    (a.len() - i).cmp(&(b.len() - j))
}

fn base_name(n: &Node) -> &[u8] {
    after_last_slash(&n.path)
}

fn sort_nodes(nodes: &mut [Node], o: &Opts) {
    use std::cmp::Ordering;
    if o.sort == SortKey::None {
        return;
    }
    let cmp = |a: &Node, b: &Node| -> Ordering {
        if o.dirsfirst || o.filesfirst {
            let (da, db) = (a.is_dir, b.is_dir);
            if da != db {
                let dirs_first = if da {
                    Ordering::Less
                } else {
                    Ordering::Greater
                };
                return if o.dirsfirst {
                    dirs_first
                } else {
                    dirs_first.reverse()
                };
            }
        }
        let (na, nb) = (base_name(a), base_name(b));
        let by_name = na.cmp(nb);
        let ord = match o.sort {
            SortKey::Name | SortKey::None => by_name,
            SortKey::Version => version_cmp(na, nb),
            SortKey::Size => {
                let (sa, sb) = (
                    a.lst.as_ref().map_or(0, |s| s.size),
                    b.lst.as_ref().map_or(0, |s| s.size),
                );
                sb.cmp(&sa).then(by_name)
            }
            SortKey::Mtime => {
                let (ta, tb) = (
                    a.lst.as_ref().map(|s| s.mtime),
                    b.lst.as_ref().map(|s| s.mtime),
                );
                ta.cmp(&tb).then(by_name)
            }
            SortKey::Ctime => {
                let (ta, tb) = (
                    a.lst.as_ref().map(|s| s.ctime),
                    b.lst.as_ref().map(|s| s.ctime),
                );
                ta.cmp(&tb).then(by_name)
            }
        };
        if o.reverse { ord.reverse() } else { ord }
    };
    nodes.sort_by(cmp);
}

/// `--prune`: tira diretórios (e links pra diretório) sem nada dentro, de baixo pra cima.
fn prune(nodes: &mut Vec<Node>) {
    for n in nodes.iter_mut() {
        if let Some(kids) = &mut n.children {
            prune(kids);
        }
    }
    nodes.retain(|n| !n.is_dir || n.children.as_ref().is_some_and(|k| !k.is_empty()));
}

/// Soma do `--du`: tamanho próprio mais o dos descendentes listados.
fn compute_du(n: &mut Node) -> u64 {
    let own = n.lst.as_ref().map_or(0, |s| s.size);
    let kids: u64 = n
        .children
        .as_mut()
        .map_or(0, |k| k.iter_mut().map(compute_du).sum());
    n.du = own + kids;
    n.du
}

fn count(n: &Node, dirs: &mut u64, files: &mut u64) {
    if let Some(kids) = &n.children {
        for k in kids {
            if k.is_dir {
                *dirs += 1;
            } else {
                *files += 1;
            }
            count(k, dirs, files);
        }
    }
}

// ------------------------------------------------------------------------------------------------
// Formatação

fn locale_is_utf8() -> bool {
    let get = |k: &str| sys::getenv(k).filter(|v| !v.is_empty());
    let loc = get("LC_ALL")
        .or_else(|| get("LC_CTYPE"))
        .or_else(|| get("LANG"))
        .unwrap_or_default();
    let l = String::from_utf8_lossy(&loc).to_ascii_lowercase();
    l.contains("utf-8") || l.contains("utf8")
}

fn is_printable(c: char, utf8: bool) -> bool {
    if !utf8 {
        return (' '..='~').contains(&c);
    }
    !(c.is_control() || ('\u{80}'..='\u{9f}').contains(&c))
}

/// Nome com o escape do tree: octal pros não imprimíveis (padrão), `?` com `-q` (bytes inválidos
/// saem crus), nada com `-N`.
fn escape_name(name: &[u8], esc: Escape, utf8: bool) -> Vec<u8> {
    if esc == Escape::Raw {
        return name.to_vec();
    }
    let mut out = Vec::new();
    for chunk in name.utf8_chunks() {
        for c in chunk.valid().chars() {
            let mut buf = [0u8; 4];
            let bytes = c.encode_utf8(&mut buf).as_bytes();
            if is_printable(c, utf8) {
                out.extend_from_slice(bytes);
            } else if esc == Escape::Question {
                out.push(b'?');
            } else {
                for b in bytes {
                    out.extend_from_slice(format!("\\{b:03o}").as_bytes());
                }
            }
        }
        for &b in chunk.invalid() {
            if esc == Escape::Question {
                out.push(b);
            } else {
                out.extend_from_slice(format!("\\{b:03o}").as_bytes());
            }
        }
    }
    out
}

fn prot(m: Mode) -> String {
    let t = match m & mode::S_IFMT {
        mode::S_IFDIR => 'd',
        mode::S_IFLNK => 'l',
        mode::S_IFCHR => 'c',
        mode::S_IFBLK => 'b',
        mode::S_IFIFO => 'p',
        mode::S_IFSOCK => 's',
        _ => '-',
    };
    let bit = |mask: Mode, c: char| if m & mask != 0 { c } else { '-' };
    let special = |x: Mode, sp: Mode, set: char, unset: char| match (m & x != 0, m & sp != 0) {
        (true, true) => set,
        (false, true) => unset,
        (true, false) => 'x',
        (false, false) => '-',
    };
    let mut s = String::with_capacity(10);
    s.push(t);
    s.push(bit(0o400, 'r'));
    s.push(bit(0o200, 'w'));
    s.push(special(0o100, mode::S_ISUID, 's', 'S'));
    s.push(bit(0o040, 'r'));
    s.push(bit(0o020, 'w'));
    s.push(special(0o010, mode::S_ISGID, 's', 'S'));
    s.push(bit(0o004, 'r'));
    s.push(bit(0o002, 'w'));
    s.push(special(0o001, mode::S_ISVTX, 't', 'T'));
    s
}

fn uid_name(uid: u32) -> String {
    sysio::users::uid2usr(uid).unwrap_or_else(|_| uid.to_string())
}

fn gid_name(gid: u32) -> String {
    sysio::users::gid2grp(gid).unwrap_or_else(|_| gid.to_string())
}

/// Tamanho do `-h`/`--si`: ` %4d` sem unidade, senão uma casa decimal abaixo de 10 (em float, como o
/// original).
fn human(size: u64, si: bool) -> String {
    let base: u64 = if si { 1000 } else { 1024 };
    let units: &[u8] = if si { b"BkMGTPEZY" } else { b"BKMGTPEZY" };
    let mut idx = usize::from(size >= base);
    let mut s = size;
    while s >= base * base && idx + 1 < units.len() {
        s /= base;
        idx += 1;
    }
    if idx == 0 {
        return format!("{s:4}");
    }
    let v = (s as f32 / base as f32) as f64;
    if s / base >= 10 {
        format!("{v:3.0}{}", char::from(units[idx]))
    } else {
        format!("{v:3.1}{}", char::from(units[idx]))
    }
}

fn size_field(n: u64, o: &Opts) -> String {
    if o.human || o.si {
        human(n, o.si)
    } else {
        format!("{n:11}")
    }
}

fn date_field(st: &Stat, o: &Opts, tz: &jiff::tz::TimeZone, now: i64) -> String {
    let t = if o.ctime { st.ctime.sec } else { st.mtime.sec };
    if let Some(fmt) = &o.timefmt {
        return String::from_utf8_lossy(&ul_common::time::zone::strftime(fmt, t, tz)).into_owned();
    }
    let dt = time::civil(t, tz);
    let mon = time::MONTHS[dt.month() as usize - 1];
    if t > now || t < now - SIX_MONTHS {
        format!("{mon} {:2}  {}", dt.day(), dt.year())
    } else {
        format!("{mon} {:2} {:02}:{:02}", dt.day(), dt.hour(), dt.minute())
    }
}

/// Cores do `LS_COLORS`/`TREE_COLORS`.
struct Colors {
    codes: Vec<(String, String)>,
    exts: Vec<(Vec<u8>, String)>,
}

impl Colors {
    fn parse(spec: &str) -> Colors {
        let mut codes = Vec::new();
        let mut exts = Vec::new();
        for item in spec.split(':') {
            let Some((k, v)) = item.split_once('=') else {
                continue;
            };
            if let Some(ext) = k.strip_prefix('*') {
                exts.push((ext.as_bytes().to_vec(), v.to_string()));
            } else {
                codes.push((k.to_string(), v.to_string()));
            }
        }
        Colors { codes, exts }
    }

    fn code(&self, key: &str) -> Option<&str> {
        self.codes
            .iter()
            .rev()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
    }

    /// Código pra um arquivo pelo stat (o do link, ou do alvo).
    fn for_stat(&self, st: &Stat, name: &[u8]) -> Option<String> {
        let m = st.mode;
        let key = match st.file_type() {
            FileType::Directory => {
                if m & mode::S_ISVTX != 0 && m & 0o002 != 0 {
                    "tw"
                } else if m & 0o002 != 0 {
                    "ow"
                } else if m & mode::S_ISVTX != 0 {
                    "st"
                } else {
                    "di"
                }
            }
            FileType::Fifo => "pi",
            FileType::Socket => "so",
            FileType::BlockDevice => "bd",
            FileType::CharDevice => "cd",
            FileType::Symlink => "ln",
            FileType::Regular => {
                if m & mode::S_ISUID != 0 && self.code("su").is_some() {
                    "su"
                } else if m & mode::S_ISGID != 0 && self.code("sg").is_some() {
                    "sg"
                } else if m & 0o111 != 0 && self.code("ex").is_some() {
                    "ex"
                } else {
                    let ext = self.exts.iter().rev().find(|(e, _)| name.ends_with(e));
                    return ext
                        .map(|(_, c)| c.clone())
                        .or_else(|| self.code("fi").map(str::to_string));
                }
            }
        };
        self.code(key).map(str::to_string)
    }
}

fn colorize(text: &[u8], code: Option<String>) -> Vec<u8> {
    match code {
        Some(c) => {
            let mut v = format!("\x1b[{c}m").into_bytes();
            v.extend_from_slice(text);
            v.extend_from_slice(b"\x1b[0m");
            v
        }
        None => text.to_vec(),
    }
}

struct Render<'a> {
    o: &'a Opts,
    lines: Lines,
    utf8: bool,
    colors: Option<Colors>,
    tz: jiff::tz::TimeZone,
    now: i64,
}

impl Render<'_> {
    fn display_name(&self, raw: &[u8]) -> Vec<u8> {
        let mut v = escape_name(raw, self.o.escape, self.utf8);
        if self.o.quote {
            v.insert(0, b'"');
            v.push(b'"');
        }
        v
    }

    fn classify(&self, st: Option<&Stat>) -> &'static str {
        let Some(st) = st else { return "" };
        match st.file_type() {
            FileType::Directory => "/",
            FileType::Fifo => "|",
            FileType::Socket => "=",
            FileType::Regular if st.mode & 0o111 != 0 => "*",
            _ => "",
        }
    }

    fn meta(&self, n: &Node) -> Option<String> {
        if !self.o.any_meta() {
            return None;
        }
        let st = n.lst.as_ref()?;
        let mut s = String::new();
        if self.o.inodes {
            s.push_str(&format!(" {:7}", st.ino));
        }
        if self.o.device {
            s.push_str(&format!(" {:3}", st.dev as i32));
        }
        if self.o.perms {
            s.push(' ');
            s.push_str(&prot(st.mode));
        }
        if self.o.user {
            s.push_str(&format!(" {:<8.32}", uid_name(st.uid)));
        }
        if self.o.group {
            s.push_str(&format!(" {:<8.32}", gid_name(st.gid)));
        }
        if self.o.size {
            let v = if self.o.du { n.du } else { st.size };
            s.push(' ');
            s.push_str(&size_field(v, self.o));
        }
        if self.o.date {
            s.push(' ');
            s.push_str(&date_field(st, self.o, &self.tz, self.now));
        }
        s.replace_range(0..1, "[");
        s.push(']');
        Some(s)
    }

    fn name_part(&self, n: &Node, is_root: bool) -> Vec<u8> {
        let mut out = Vec::new();
        let shown = self.display_name(&n.name);
        let code = self.colors.as_ref().and_then(|c| {
            let st = n.lst.as_ref()?;
            if st.file_type() == FileType::Symlink {
                if n.tst.is_none() {
                    return c.code("or").or(c.code("ln")).map(str::to_string);
                }
                if c.code("ln") == Some("target") {
                    return c.for_stat(n.tst.as_ref()?, &n.name);
                }
            }
            c.for_stat(st, &n.name)
        });
        out.extend(colorize(&shown, code));
        let classify = self.o.classify && !self.o.dirs_only;
        if let Some(target) = &n.target {
            out.extend_from_slice(b" -> ");
            let t = self.display_name(target);
            let tcode = self.colors.as_ref().and_then(|c| match &n.tst {
                Some(ts) => c.for_stat(ts, target),
                None => c.code("mi").map(str::to_string),
            });
            out.extend(colorize(&t, tcode));
            if classify {
                out.extend_from_slice(self.classify(n.tst.as_ref()).as_bytes());
            }
        } else if classify && (!is_root || n.is_dir) {
            out.extend_from_slice(self.classify(n.lst.as_ref()).as_bytes());
        }
        match &n.note {
            Note::Recursive => out.extend_from_slice(b"  [recursive, not followed]"),
            Note::FileLimit(k) => out.extend_from_slice(
                format!("  [{k} entries exceeds filelimit, not opening dir]").as_bytes(),
            ),
            Note::ErrorOpening => out.extend_from_slice(b"  [error opening dir]"),
            Note::None => {}
        }
        out
    }

    fn indent(&self, ancestors_last: &[bool], last: bool) -> Vec<u8> {
        if self.o.no_indent {
            return Vec::new();
        }
        let (vert, branch, corner, blank): (&[u8], &[u8], &[u8], &[u8]) = match self.lines {
            Lines::Utf8 => (
                "\u{2502}\u{a0}\u{a0} ".as_bytes(),
                "\u{251c}\u{2500}\u{2500} ".as_bytes(),
                "\u{2514}\u{2500}\u{2500} ".as_bytes(),
                b"    ",
            ),
            Lines::Ascii => (b"|   ", b"|-- ", b"`-- ", b"    "),
            Lines::Cp437 => (b"\xb3   ", b"\xc3\xc4\xc4 ", b"\xc0\xc4\xc4 ", b"    "),
            Lines::Ansi => (b"x   ", b"tqq ", b"mqq ", b"    "),
        };
        let mut out = Vec::new();
        if self.lines == Lines::Ansi {
            out.extend_from_slice(b"\x1b(0");
        }
        for &l in ancestors_last {
            out.extend_from_slice(if l { blank } else { vert });
        }
        out.extend_from_slice(if last { corner } else { branch });
        if self.lines == Lines::Ansi {
            out.extend_from_slice(b"\x1b(B");
        }
        out
    }

    fn text(
        &self,
        out: &mut Vec<u8>,
        n: &Node,
        ancestors_last: &mut Vec<bool>,
        last: Option<bool>,
    ) {
        let meta = self.meta(n);
        let ind = match last {
            Some(l) => self.indent(ancestors_last, l),
            None => Vec::new(),
        };
        if self.o.metafirst {
            if let Some(m) = &meta {
                out.extend_from_slice(m.as_bytes());
                out.extend_from_slice(b"  ");
            }
            out.extend_from_slice(&ind);
        } else {
            out.extend_from_slice(&ind);
            if let Some(m) = &meta {
                out.extend_from_slice(m.as_bytes());
                out.extend_from_slice(b"  ");
            }
        }
        out.extend(self.name_part(n, last.is_none()));
        out.push(b'\n');
        if let Some(kids) = &n.children {
            if let Some(l) = last {
                ancestors_last.push(l);
            }
            for (i, k) in kids.iter().enumerate() {
                self.text(out, k, ancestors_last, Some(i + 1 == kids.len()));
            }
            if last.is_some() {
                ancestors_last.pop();
            }
        }
    }

    fn type_name(n: &Node) -> &'static str {
        if n.virtual_node {
            return if n.is_dir { "directory" } else { "file" };
        }
        match n.lst.as_ref().map(Stat::file_type) {
            Some(FileType::Directory) => "directory",
            Some(FileType::Symlink) => "link",
            Some(FileType::Fifo) => "fifo",
            Some(FileType::Socket) => "socket",
            Some(FileType::CharDevice) => "char",
            Some(FileType::BlockDevice) => "block",
            _ => "file",
        }
    }

    /// Atributos comuns de JSON e XML, na ordem do original.
    fn attrs(&self, n: &Node) -> Vec<(&'static str, AttrValue)> {
        let mut v = Vec::new();
        if let Some(t) = &n.target {
            v.push(("target", AttrValue::Str(t.clone())));
        }
        let Some(st) = &n.lst else { return v };
        if self.o.inodes {
            v.push(("inode", AttrValue::Num(st.ino)));
        }
        if self.o.device {
            v.push(("dev", AttrValue::Num(st.dev)));
        }
        if self.o.perms {
            v.push((
                "mode",
                AttrValue::Str(format!("{:04o}", st.mode & 0o7777).into_bytes()),
            ));
            v.push(("prot", AttrValue::Str(prot(st.mode).into_bytes())));
        }
        if self.o.user {
            v.push(("user", AttrValue::Str(uid_name(st.uid).into_bytes())));
        }
        if self.o.group {
            v.push(("group", AttrValue::Str(gid_name(st.gid).into_bytes())));
        }
        if self.o.size {
            let s = if self.o.du { n.du } else { st.size };
            if self.o.human || self.o.si {
                v.push((
                    "size",
                    AttrValue::Str(human(s, self.o.si).trim_start().as_bytes().to_vec()),
                ));
            } else {
                v.push(("size", AttrValue::Num(s)));
            }
        }
        if self.o.date {
            v.push((
                "time",
                AttrValue::Str(date_field(st, self.o, &self.tz, self.now).into_bytes()),
            ));
        }
        v
    }

    fn json(&self, out: &mut Vec<u8>, n: &Node, depth: usize, compact: bool) {
        let ind = if compact {
            Vec::new()
        } else {
            vec![b' '; 2 * (depth + 1)]
        };
        out.extend_from_slice(&ind);
        out.extend_from_slice(
            format!("{{\"type\":\"{}\",\"name\":", Self::type_name(n)).as_bytes(),
        );
        out.extend(json_str(&n.name));
        for (k, val) in self.attrs(n) {
            out.extend_from_slice(format!(",\"{k}\":").as_bytes());
            match val {
                AttrValue::Num(x) => out.extend_from_slice(x.to_string().as_bytes()),
                AttrValue::Str(s) => out.extend(json_str(&s)),
            }
        }
        let error = match &n.note {
            Note::Recursive => Some("recursive, not followed".to_string()),
            Note::FileLimit(k) => Some(format!("{k} entries exceeds filelimit, not opening dir")),
            _ => None,
        };
        if let Some(e) = error {
            out.extend_from_slice(format!(",\"contents\":[{{\"error\": \"{e}\"}}").as_bytes());
            if !compact {
                if depth == 0 {
                    out.push(b'\n');
                }
                out.extend_from_slice(&ind);
            }
            out.extend_from_slice(b"]}");
            return;
        }
        match &n.children {
            Some(kids) if !kids.is_empty() => {
                out.extend_from_slice(b",\"contents\":[");
                if !compact {
                    out.push(b'\n');
                }
                for (i, k) in kids.iter().enumerate() {
                    self.json(out, k, depth + 1, compact);
                    if i + 1 < kids.len() {
                        out.push(b',');
                    }
                    if !compact {
                        out.push(b'\n');
                    }
                }
                out.extend_from_slice(&ind);
                out.extend_from_slice(b"]}");
            }
            _ => out.push(b'}'),
        }
    }

    fn xml(&self, out: &mut Vec<u8>, n: &Node, depth: usize, compact: bool) {
        let ind = if compact {
            Vec::new()
        } else {
            vec![b' '; 2 * (depth + 1)]
        };
        let tag = Self::type_name(n);
        out.extend_from_slice(&ind);
        out.extend_from_slice(format!("<{tag} name=\"").as_bytes());
        out.extend(xml_str(&n.name));
        out.push(b'"');
        for (k, val) in self.attrs(n) {
            out.extend_from_slice(format!(" {k}=\"").as_bytes());
            match val {
                AttrValue::Num(x) => out.extend_from_slice(x.to_string().as_bytes()),
                AttrValue::Str(s) => out.extend(xml_str(&s)),
            }
            out.push(b'"');
        }
        out.push(b'>');
        let error = match &n.note {
            Note::Recursive => Some("recursive, not followed".to_string()),
            Note::FileLimit(k) => Some(format!("{k} entries exceeds filelimit, not opening dir")),
            _ => None,
        };
        if let Some(e) = error {
            out.extend_from_slice(format!("<error>{e}</error>").as_bytes());
            if !compact {
                if depth == 0 {
                    out.push(b'\n');
                }
                out.extend_from_slice(&ind);
            }
            out.extend_from_slice(format!("</{tag}>").as_bytes());
            return;
        }
        if let Some(kids) = &n.children
            && !kids.is_empty()
        {
            out.push(b'\n');
            for k in kids {
                self.xml(out, k, depth + 1, compact);
                if !compact {
                    out.push(b'\n');
                }
            }
            out.extend_from_slice(&ind);
        }
        out.extend_from_slice(format!("</{tag}>").as_bytes());
    }
}

enum AttrValue {
    Num(u64),
    Str(Vec<u8>),
}

fn json_str(s: &[u8]) -> Vec<u8> {
    let mut out = vec![b'"'];
    for &b in s {
        match b {
            b'"' => out.extend_from_slice(b"\\\""),
            b'\\' => out.extend_from_slice(b"\\\\"),
            b if b < 0x20 => out.extend_from_slice(format!("\\u{b:04x}").as_bytes()),
            b => out.push(b),
        }
    }
    out.push(b'"');
    out
}

fn xml_str(s: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    for &b in s {
        match b {
            b'&' => out.extend_from_slice(b"&amp;"),
            b'<' => out.extend_from_slice(b"&lt;"),
            b'>' => out.extend_from_slice(b"&gt;"),
            b'"' => out.extend_from_slice(b"&quot;"),
            b => out.push(b),
        }
    }
    out
}

// ------------------------------------------------------------------------------------------------
// --fromfile

fn fromfile_tree(name: &[u8], data: &[u8]) -> Node {
    let mut root = Node {
        name: name.to_vec(),
        path: name.to_vec(),
        lst: None,
        target: None,
        tst: None,
        is_dir: true,
        children: Some(Vec::new()),
        note: Note::None,
        du: 0,
        virtual_node: true,
        matched: false,
    };
    for line in data.split(|b| *b == b'\n') {
        let line = line.strip_suffix(b"\r").unwrap_or(line);
        if line.is_empty() {
            continue;
        }
        let trailing_dir = line.ends_with(b"/");
        let comps: Vec<&[u8]> = line
            .split(|b| *b == b'/')
            .filter(|c| !c.is_empty())
            .collect();
        let mut cur = &mut root;
        for (i, comp) in comps.iter().enumerate() {
            let last = i + 1 == comps.len();
            let kids = cur.children.get_or_insert_with(Vec::new);
            let pos = match kids.iter().position(|k| k.name == *comp) {
                Some(p) => p,
                None => {
                    let mut path = cur.path.clone();
                    path.push(b'/');
                    path.extend_from_slice(comp);
                    let node = Node {
                        name: comp.to_vec(),
                        path,
                        lst: None,
                        target: None,
                        tst: None,
                        is_dir: false,
                        children: None,
                        note: Note::None,
                        du: 0,
                        virtual_node: true,
                        matched: false,
                    };
                    let kids = cur.children.get_or_insert_with(Vec::new);
                    kids.push(node);
                    kids.len() - 1
                }
            };
            let kids = cur.children.as_mut().expect("tem filhos");
            cur = &mut kids[pos];
            if !last || trailing_dir {
                cur.is_dir = true;
                cur.children.get_or_insert_with(Vec::new);
            }
        }
    }
    root
}

fn sort_virtual(n: &mut Node, o: &Opts) {
    if let Some(kids) = &mut n.children {
        sort_nodes(kids, o);
        for k in kids.iter_mut() {
            sort_virtual(k, o);
        }
    }
}

// ------------------------------------------------------------------------------------------------

pub fn main(_ctx: &mut Ctx, args: &[OsString]) -> i32 {
    io::run(|| run(args))
}

fn run(args: &[OsString]) -> i32 {
    let argv = io::args_bytes(args);
    let (o, mut dirs) = match parse_args(&argv) {
        Parsed::Run(o, d) => (*o, d),
        Parsed::Exit(code) => return code,
    };
    if dirs.is_empty() {
        dirs.push(b".".to_vec());
    }
    let utf8 = locale_is_utf8();
    let lines = o
        .lines
        .unwrap_or(if utf8 { Lines::Utf8 } else { Lines::Ascii });
    let term = sys::getenv("TERM").filter(|t| !t.is_empty());
    let want_color = match o.color {
        ColorMode::Never => false,
        ColorMode::Always => true,
        ColorMode::Auto => o.outfile.is_none() && io::stdout_is_tty(),
    } && o.format == Format::Text
        && term.is_some();
    let colors = want_color.then(|| {
        let spec = sys::getenv("TREE_COLORS")
            .or_else(|| sys::getenv("LS_COLORS"))
            .map(|v| io::lossy(&v));
        Colors::parse(spec.as_deref().unwrap_or(DEFAULT_COLORS))
    });
    let render = Render {
        o: &o,
        lines,
        utf8,
        colors,
        tz: time::local_tz(),
        now: time::now().sec,
    };

    // Monta as árvores.
    let mut roots: Vec<Node> = Vec::new();
    let mut status = 0;
    let gitbase: Vec<GitRule> = o
        .gitfiles
        .iter()
        .filter_map(|f| io::read_path(f).ok().map(|d| parse_gitignore(&d, b"")))
        .flatten()
        .map(|mut r| {
            r.anchored = false;
            r
        })
        .collect();
    for d in &dirs {
        if o.fromfile {
            let data = if d == b"." {
                io::read_stdin()
            } else {
                io::read_path(d)
            };
            match data {
                Ok(data) => {
                    let mut n = fromfile_tree(d, &data);
                    sort_virtual(&mut n, &o);
                    roots.push(n);
                }
                Err(_) => {
                    status = 2;
                    roots.push(error_root(d));
                }
            }
            continue;
        }
        let lst = lstat(d);
        let st = stat(d);
        let Some(st_follow) = st.clone() else {
            status = 2;
            roots.push(error_root(d));
            continue;
        };
        if st_follow.file_type() != FileType::Directory {
            let mut n = error_root(d);
            n.lst = lst;
            n.is_dir = false;
            roots.push(n);
            continue;
        }
        let mut walker = Walker {
            o: &o,
            visited: BTreeSet::new(),
            root_dev: st_follow.dev,
        };
        walker.visited.insert((st_follow.dev, st_follow.ino));
        let mut root = Node {
            name: d.clone(),
            path: d.clone(),
            lst: Some(st_follow.clone()),
            target: None,
            tst: None,
            is_dir: true,
            children: None,
            note: Note::None,
            du: 0,
            virtual_node: false,
            matched: false,
        };
        let (children, note) = walker.list(&root, 0, false, &gitbase);
        root.children = children;
        root.note = note;
        if root.note == Note::ErrorOpening {
            status = 2;
        }
        roots.push(root);
    }
    if o.prune {
        for r in &mut roots {
            if let Some(k) = &mut r.children {
                prune(k);
            }
        }
    }
    let mut total_du = 0u64;
    if o.du {
        for r in &mut roots {
            total_du += compute_du(r);
        }
    }
    let (mut ndirs, mut nfiles) = (0u64, 0u64);
    for r in &roots {
        if r.is_dir && r.note != Note::ErrorOpening {
            ndirs += 1;
        } else if r.lst.is_some() {
            nfiles += 1;
        }
        count(r, &mut ndirs, &mut nfiles);
    }

    let mut out: Vec<u8> = Vec::new();
    match o.format {
        Format::Text => {
            for r in &roots {
                if r.note == Note::ErrorOpening {
                    let mut line = render.display_name(&r.name);
                    line.extend_from_slice(b"  [error opening dir]\n");
                    out.extend(line);
                    continue;
                }
                render.text(&mut out, r, &mut Vec::new(), None);
            }
            if !o.noreport {
                out.push(b'\n');
                if o.du {
                    let size = if o.human || o.si {
                        human(total_du, o.si)
                    } else {
                        format!("{total_du:11} bytes")
                    };
                    out.extend_from_slice(format!(" {size} used in ").as_bytes());
                }
                let d = if ndirs == 1 {
                    "directory"
                } else {
                    "directories"
                };
                if o.dirs_only {
                    out.extend_from_slice(format!("{ndirs} {d}\n").as_bytes());
                } else {
                    let f = if nfiles == 1 { "file" } else { "files" };
                    out.extend_from_slice(format!("{ndirs} {d}, {nfiles} {f}\n").as_bytes());
                }
            }
        }
        Format::Json => {
            let compact = o.no_indent;
            out.push(b'[');
            if !compact {
                out.push(b'\n');
            }
            for (i, r) in roots.iter().enumerate() {
                if r.note == Note::ErrorOpening && r.lst.is_none() {
                    // Raiz inexistente: o original perde o começo do objeto.
                    out.extend_from_slice(b",\"name\":");
                    out.extend(json_str(&r.name));
                    out.extend_from_slice(b",\"contents\":[{\"error\": \"error opening dir\"}");
                    if !compact {
                        out.extend_from_slice(b"\n  ");
                    }
                    out.extend_from_slice(b"]}");
                } else if !r.is_dir {
                    if !compact {
                        out.extend_from_slice(b"  ");
                    }
                    out.extend_from_slice(b"{\"type\":\"file\",\"name\":");
                    out.extend(json_str(&r.name));
                    out.extend_from_slice(b",\"contents\":[{\"error\": \"error opening dir\"}");
                    if !compact {
                        out.extend_from_slice(b"\n  ");
                    }
                    out.extend_from_slice(b"]}");
                } else {
                    render.json(&mut out, r, 0, compact);
                }
                if i + 1 < roots.len() {
                    out.push(b',');
                    if !compact {
                        out.push(b'\n');
                    }
                }
            }
            if !compact {
                out.push(b'\n');
            }
            if !o.noreport {
                out.push(b',');
                if !compact {
                    out.extend_from_slice(b"\n  ");
                }
                out.extend_from_slice(b"{\"type\":\"report\"");
                if o.du {
                    out.extend_from_slice(format!(",\"size\":{total_du}").as_bytes());
                }
                out.extend_from_slice(format!(",\"directories\":{ndirs}").as_bytes());
                if !o.dirs_only {
                    out.extend_from_slice(format!(",\"files\":{nfiles}").as_bytes());
                }
                out.push(b'}');
            }
            if !compact {
                out.push(b'\n');
            }
            out.extend_from_slice(b"]\n");
        }
        Format::Xml => {
            let compact = o.no_indent;
            out.extend_from_slice(b"<?xml version=\"1.0\" encoding=\"UTF-8\"?>");
            if !compact {
                out.push(b'\n');
            }
            out.extend_from_slice(b"<tree>");
            if !compact {
                out.push(b'\n');
            }
            for r in &roots {
                if r.note == Note::ErrorOpening || !r.is_dir {
                    out.extend_from_slice(b" name=\"");
                    out.extend(xml_str(&r.name));
                    out.extend_from_slice(b"\"><error>error opening dir</error>");
                    if !compact {
                        out.extend_from_slice(b"\n  ");
                    }
                    out.extend_from_slice(b"</unknown>");
                } else {
                    render.xml(&mut out, r, 0, compact);
                }
                if !compact {
                    out.push(b'\n');
                }
            }
            if !o.noreport {
                let nl = if compact { "" } else { "\n" };
                let i2 = if compact { "" } else { "  " };
                let i4 = if compact { "" } else { "    " };
                out.extend_from_slice(format!("{i2}<report>{nl}").as_bytes());
                if o.du {
                    out.extend_from_slice(format!("{i4}<size>{total_du}</size>{nl}").as_bytes());
                }
                out.extend_from_slice(
                    format!("{i4}<directories>{ndirs}</directories>{nl}").as_bytes(),
                );
                if !o.dirs_only {
                    out.extend_from_slice(format!("{i4}<files>{nfiles}</files>{nl}").as_bytes());
                }
                out.extend_from_slice(format!("{i2}</report>{nl}").as_bytes());
            }
            out.extend_from_slice(b"</tree>\n");
        }
    }

    match &o.outfile {
        Some(path) => {
            match File::open_with(path, OFlags::WRONLY | OFlags::CREAT | OFlags::TRUNC, 0o666) {
                Ok(mut f) => {
                    if let Err(e) = f.write_all(&out) {
                        io::eprint(format!(
                            "tree: {}: {}\n",
                            io::lossy(path),
                            Errno::from_io(&e).message()
                        ));
                        return 1;
                    }
                }
                Err(e) => {
                    io::eprint(format!(
                        "tree: invalid filename '{}': {}\n",
                        io::lossy(path),
                        e.message()
                    ));
                    return 1;
                }
            }
        }
        None => {
            let mut stdout = io::stdout();
            let _ = stdout.write_all(&out);
        }
    }
    status
}

fn error_root(name: &[u8]) -> Node {
    Node {
        name: name.to_vec(),
        path: name.to_vec(),
        lst: None,
        target: None,
        tst: None,
        is_dir: false,
        children: None,
        note: Note::ErrorOpening,
        du: 0,
        virtual_node: false,
        matched: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sysabi::Program;
    use sysabi::testkit::TestKit;

    fn kit() -> TestKit {
        TestKit::new()
            .programs([Program::bin("tree", main)])
            .file("/work/t/a.txt", "hi\n", 0o755)
            .file("/work/t/docs/readme.md", "", 0o644)
            .file("/work/t/src/sub/z.c", "", 0o644)
            .symlink("/work/t/link", "a.txt")
            .symlink("/work/t/broken", "nowhere")
            .symlink("/work/t/srcl", "src")
            .dir("/work/t/empty", 0o755)
            .cwd("/work/t")
    }

    const V: &str = "\u{2502}\u{a0}\u{a0} ";

    #[test]
    fn default_listing_and_report() {
        let r = kit().run(&["tree"], b"");
        let expected = format!(
            ".\n├── a.txt\n├── broken -> nowhere\n├── docs\n{V}└── readme.md\n├── empty\n├── link -> a.txt\n├── src\n{V}└── sub\n{V}    └── z.c\n└── srcl -> src\n\n6 directories, 5 files\n"
        );
        assert_eq!(r.stdout_str(), expected);
        assert_eq!(r.status.shell_status(), 0);
    }

    #[test]
    fn classify_ascii_and_dirs_only() {
        let r = kit().run(&["tree", "-F", "--charset", "ascii", "-L", "1"], b"");
        assert_eq!(
            r.stdout_str(),
            "./\n|-- a.txt*\n|-- broken -> nowhere\n|-- docs/\n|-- empty/\n|-- link -> a.txt*\n|-- src/\n`-- srcl -> src/\n\n5 directories, 3 files\n"
        );
        let r = kit().run(&["tree", "-d"], b"");
        assert_eq!(
            r.stdout_str(),
            format!(
                ".\n├── docs\n├── empty\n├── src\n{V}└── sub\n└── srcl -> src\n\n6 directories\n"
            )
        );
    }

    #[test]
    fn patterns_prune_and_errors() {
        let r = kit().run(&["tree", "-P", "*.c", "--prune"], b"");
        assert_eq!(
            r.stdout_str(),
            ".\n└── src\n    └── sub\n        └── z.c\n\n3 directories, 1 file\n"
        );
        let r = kit().run(&["tree", "-I", "src|docs|empty"], b"");
        assert_eq!(
            r.stdout_str(),
            ".\n├── a.txt\n├── broken -> nowhere\n├── link -> a.txt\n└── srcl -> src\n\n2 directories, 3 files\n"
        );
        let r = kit().run(&["tree", "nope"], b"");
        assert_eq!(
            (r.stdout_str().as_str(), r.status.shell_status()),
            ("nope  [error opening dir]\n\n0 directories, 0 files\n", 2)
        );
        let r = kit().run(&["tree", "-Z"], b"");
        assert!(
            r.stderr_str()
                .starts_with("tree: Invalid argument -`Z'.\nusage: tree ")
        );
        assert_eq!(r.status.shell_status(), 1);
        let r = kit().run(&["tree", "-L", "0"], b"");
        assert_eq!(
            r.stderr_str(),
            "tree: Invalid level, must be greater than 0.\n"
        );
    }

    #[test]
    fn json_and_xml() {
        let r = kit().run(&["tree", "-J", "docs"], b"");
        assert_eq!(
            r.stdout_str(),
            "[\n  {\"type\":\"directory\",\"name\":\"docs\",\"contents\":[\n    {\"type\":\"file\",\"name\":\"readme.md\"}\n  ]}\n,\n  {\"type\":\"report\",\"directories\":1,\"files\":1}\n]\n"
        );
        let r = kit().run(&["tree", "-X", "docs"], b"");
        assert_eq!(
            r.stdout_str(),
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<tree>\n  <directory name=\"docs\">\n    <file name=\"readme.md\"></file>\n  </directory>\n  <report>\n    <directories>1</directories>\n    <files>1</files>\n  </report>\n</tree>\n"
        );
    }

    #[test]
    fn human_sizes_like_tree() {
        assert_eq!(human(3, false), "   3");
        assert_eq!(human(4096, false), "4.0K");
        assert_eq!(human(1_500_000, false), "1.4M");
        assert_eq!(human(22 * 1024 + 100, false), " 22K");
        assert_eq!(human(4096, true), "4.1k");
        assert_eq!(human(1_500_000, true), "1.5M");
    }

    #[test]
    fn version_sort_and_escapes() {
        use std::cmp::Ordering;
        assert_eq!(version_cmp(b"file9", b"file10"), Ordering::Less);
        assert_eq!(version_cmp(b"a", b"b"), Ordering::Less);
        assert_eq!(
            escape_name(b"a\tb\xffc", Escape::Octal, true),
            b"a\\011b\\377c".to_vec()
        );
        assert_eq!(
            escape_name(b"a\tb\xff", Escape::Question, true),
            b"a?b\xff".to_vec()
        );
        assert_eq!(
            escape_name("é".as_bytes(), Escape::Octal, false),
            b"\\303\\251".to_vec()
        );
        assert!(pattern_match(b"*.md|*.c", b"z.c", false));
        assert!(pattern_match(b"[ab]*", b"big", false));
        assert!(!pattern_match(b"[!ab]*", b"big", false));
        assert!(pattern_match(b"A*", b"a.txt", true));
    }

    #[test]
    fn gitignore_rules() {
        let rules = parse_gitignore(b"*.log\nsrc/\n!keep.log\n/root.txt\ndocs/**/x\n", b"./");
        assert!(git_ignored(&rules, b"./build.log", b"build.log", false));
        assert!(!git_ignored(&rules, b"./keep.log", b"keep.log", false));
        assert!(git_ignored(&rules, b"./src", b"src", true));
        assert!(!git_ignored(&rules, b"./src", b"src", false));
        assert!(git_ignored(&rules, b"./root.txt", b"root.txt", false));
        assert!(!git_ignored(&rules, b"./a/root.txt", b"root.txt", false));
        assert!(git_ignored(&rules, b"./docs/a/b/x", b"x", false));
    }
}
