//! Opções da linha de comando (zip.c e fileio.c): a tabela de opções, `get_option` com permutação
//! dos argumentos e valores de lista, e a leitura de `ZIPOPT` e `ZIP` do ambiente.

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Vt {
    NoValue,
    Required,
    ValueList,
}

pub struct Opt {
    pub short: &'static str,
    pub long: &'static str,
    pub vt: Vt,
    pub neg: bool,
    pub id: u32,
    pub name: &'static str,
}

pub const O_NON_OPTION_ARG: u32 = 0x1000;

pub const O_DB: u32 = 0x105;
pub const O_DC: u32 = 0x106;
pub const O_DD: u32 = 0x107;
pub const O_DES: u32 = 0x108;
pub const O_DF_: u32 = 0x110;
pub const O_DG: u32 = 0x111;
pub const O_DS: u32 = 0x112;
pub const O_DU: u32 = 0x113;
pub const O_DV: u32 = 0x114;
pub const O_FF: u32 = 0x115;
pub const O_FI: u32 = 0x116;
pub const O_FS: u32 = 0x117;
pub const O_H2: u32 = 0x118;
pub const O_LA: u32 = 0x121;
pub const O_LF: u32 = 0x122;
pub const O_LI: u32 = 0x123;
pub const O_LL: u32 = 0x124;
pub const O_MM_LOWER: u32 = 0x125;
pub const O_MM: u32 = 0x126;
pub const O_NW: u32 = 0x127;
pub const O_RE: u32 = 0x128;
pub const O_SB: u32 = 0x129;
pub const O_SC: u32 = 0x130;
pub const O_SD: u32 = 0x131;
pub const O_SF: u32 = 0x132;
pub const O_SO: u32 = 0x133;
pub const O_SP: u32 = 0x134;
pub const O_SU: u32 = 0x135;
pub const O_SU_UPPER: u32 = 0x136;
pub const O_SV: u32 = 0x137;
pub const O_TT: u32 = 0x138;
pub const O_TT_UPPER: u32 = 0x139;
pub const O_UN: u32 = 0x140;
pub const O_VE: u32 = 0x141;
pub const O_WS: u32 = 0x143;
pub const O_Z64: u32 = 0x145;

const fn o(short: &'static str, long: &'static str, vt: Vt, neg: bool, id: u32, name: &'static str) -> Opt {
    Opt { short, long, vt, neg, id, name }
}

const fn c(ch: char) -> u32 {
    ch as u32
}

/// A tabela `options[]` do zip.c para a compilação Unix do Debian (Unicode, Zip64 e links).
pub static OPTIONS: &[Opt] = &[
    o("0", "store", Vt::NoValue, false, c('0'), "store"),
    o("1", "compress-1", Vt::NoValue, false, c('1'), "compress 1"),
    o("2", "compress-2", Vt::NoValue, false, c('2'), "compress 2"),
    o("3", "compress-3", Vt::NoValue, false, c('3'), "compress 3"),
    o("4", "compress-4", Vt::NoValue, false, c('4'), "compress 4"),
    o("5", "compress-5", Vt::NoValue, false, c('5'), "compress 5"),
    o("6", "compress-6", Vt::NoValue, false, c('6'), "compress 6"),
    o("7", "compress-7", Vt::NoValue, false, c('7'), "compress 7"),
    o("8", "compress-8", Vt::NoValue, false, c('8'), "compress 8"),
    o("9", "compress-9", Vt::NoValue, false, c('9'), "compress 9"),
    o("A", "adjust-sfx", Vt::NoValue, false, c('A'), "adjust self extractor offsets"),
    o("b", "temp-path", Vt::Required, false, c('b'), "dir to use for temp archive"),
    o("c", "entry-comments", Vt::NoValue, false, c('c'), "add comments for each entry"),
    o("d", "delete", Vt::NoValue, false, c('d'), "delete entries from archive"),
    o("db", "display-bytes", Vt::NoValue, true, O_DB, "display running bytes"),
    o("dc", "display-counts", Vt::NoValue, true, O_DC, "display running file count"),
    o("dd", "display-dots", Vt::NoValue, true, O_DD, "display dots as process each file"),
    o("dg", "display-globaldots", Vt::NoValue, true, O_DG, "display dots for archive instead of files"),
    o("ds", "dot-size", Vt::Required, false, O_DS, "set progress dot size - default 10M bytes"),
    o("du", "display-usize", Vt::NoValue, true, O_DU, "display uncompressed size in bytes"),
    o("dv", "display-volume", Vt::NoValue, true, O_DV, "display volume (disk) number"),
    o("D", "no-dir-entries", Vt::NoValue, false, c('D'), "no entries for dirs themselves (-x */)"),
    o("DF", "difference-archive", Vt::NoValue, false, O_DF_, "create diff archive with changed/new files"),
    o("e", "encrypt", Vt::NoValue, false, c('e'), "encrypt entries, ask for password"),
    o("F", "fix", Vt::NoValue, false, c('F'), "fix mostly intact archive (try first)"),
    o("FF", "fixfix", Vt::NoValue, false, O_FF, "try harder to fix archive (not as reliable)"),
    o("FI", "fifo", Vt::NoValue, true, O_FI, "read Unix FIFO (zip will wait on open pipe)"),
    o("FS", "filesync", Vt::NoValue, false, O_FS, "add/delete entries to make archive match OS"),
    o("f", "freshen", Vt::NoValue, false, c('f'), "freshen existing archive entries"),
    o("fd", "force-descriptors", Vt::NoValue, false, O_DES, "force data descriptors as if streaming"),
    o("fz", "force-zip64", Vt::NoValue, true, O_Z64, "force use of Zip64 format, negate prevents"),
    o("g", "grow", Vt::NoValue, false, c('g'), "grow existing archive instead of replace"),
    o("h", "help", Vt::NoValue, false, c('h'), "help"),
    o("H", "", Vt::NoValue, false, c('h'), "help"),
    o("?", "", Vt::NoValue, false, c('h'), "help"),
    o("h2", "more-help", Vt::NoValue, false, O_H2, "extended help"),
    o("i", "include", Vt::ValueList, false, c('i'), "include only files matching patterns"),
    o("j", "junk-paths", Vt::NoValue, false, c('j'), "strip paths and just store file names"),
    o("J", "junk-sfx", Vt::NoValue, false, c('J'), "strip self extractor from archive"),
    o("k", "DOS-names", Vt::NoValue, false, c('k'), "force use of 8.3 DOS names"),
    o("l", "to-crlf", Vt::NoValue, false, c('l'), "convert text file line ends - LF->CRLF"),
    o("ll", "from-crlf", Vt::NoValue, false, O_LL, "convert text file line ends - CRLF->LF"),
    o("lf", "logfile-path", Vt::Required, false, O_LF, "log to log file at path (default overwrite)"),
    o("la", "log-append", Vt::NoValue, true, O_LA, "append to existing log file"),
    o("li", "log-info", Vt::NoValue, true, O_LI, "include informational messages in log"),
    o("L", "license", Vt::NoValue, false, c('L'), "display license"),
    o("m", "move", Vt::NoValue, false, c('m'), "add files to archive then delete files"),
    o("mm", "", Vt::NoValue, false, O_MM_LOWER, "not used"),
    o("MM", "must-match", Vt::NoValue, false, O_MM, "error if in file not matched/not readable"),
    o("n", "suffixes", Vt::Required, false, c('n'), "suffixes to not compress: .gz:.zip"),
    o("nw", "no-wild", Vt::NoValue, false, O_NW, "no wildcards during add or update"),
    o("o", "latest-time", Vt::NoValue, false, c('o'), "use latest entry time as archive time"),
    o("O", "output-file", Vt::Required, false, c('O'), "set out zipfile different than in zipfile"),
    o("p", "paths", Vt::NoValue, false, c('p'), "store paths"),
    o("P", "password", Vt::Required, false, c('P'), "encrypt entries, option value is password"),
    o("q", "quiet", Vt::NoValue, false, c('q'), "quiet"),
    o("r", "recurse-paths", Vt::NoValue, false, c('r'), "recurse down listed paths"),
    o("R", "recurse-patterns", Vt::NoValue, false, c('R'), "recurse current dir and match patterns"),
    o("RE", "regex", Vt::NoValue, false, O_RE, "allow [list] matching (regex)"),
    o("s", "split-size", Vt::Required, false, c('s'), "do splits, set split size (-s=0 no splits)"),
    o("sp", "split-pause", Vt::NoValue, false, O_SP, "pause while splitting to select destination"),
    o("sv", "split-verbose", Vt::NoValue, false, O_SV, "be verbose about creating splits"),
    o("sb", "split-bell", Vt::NoValue, false, O_SB, "when pause for next split ring bell"),
    o("sc", "show-command", Vt::NoValue, false, O_SC, "show command line"),
    o("sd", "show-debug", Vt::NoValue, false, O_SD, "show debug"),
    o("sf", "show-files", Vt::NoValue, true, O_SF, "show files to operate on and exit"),
    o("so", "show-options", Vt::NoValue, false, O_SO, "show options"),
    o("su", "show-unicode", Vt::NoValue, true, O_SU, "as -sf but also show escaped Unicode"),
    o("sU", "show-just-unicode", Vt::NoValue, true, O_SU_UPPER, "as -sf but only show escaped Unicode"),
    o("t", "from-date", Vt::Required, false, c('t'), "exclude before date"),
    o("tt", "before-date", Vt::Required, false, O_TT, "include before date"),
    o("T", "test", Vt::NoValue, false, c('T'), "test updates before replacing archive"),
    o("TT", "unzip-command", Vt::Required, false, O_TT_UPPER, "unzip command to use, name is added to end"),
    o("u", "update", Vt::NoValue, false, c('u'), "update existing entries and add new"),
    o("U", "copy-entries", Vt::NoValue, false, c('U'), "select from archive instead of file system"),
    o("UN", "unicode", Vt::Required, false, O_UN, "UN=quit, warn, ignore, no, escape"),
    o("v", "verbose", Vt::NoValue, false, c('v'), "display additional information"),
    o("", "version", Vt::NoValue, false, O_VE, "(if no other args) show version information"),
    o("ws", "wild-stop-dirs", Vt::NoValue, false, O_WS, "* stops at /, ** includes any /"),
    o("x", "exclude", Vt::ValueList, false, c('x'), "exclude files matching patterns"),
    o("X", "strip-extra", Vt::NoValue, true, c('X'), "-X- keep all ef, -X strip but critical ef"),
    o("y", "symlinks", Vt::NoValue, false, c('y'), "store symbolic links"),
    o("z", "archive-comment", Vt::NoValue, false, c('z'), "ask for archive comment"),
    o("Z", "compression-method", Vt::Required, false, c('Z'), "compression method"),
    o("@", "names-stdin", Vt::NoValue, false, c('@'), "get file names from stdin, one per line"),
];

const NO_MATCH: i64 = -1;
const SKIP_VALUE_ARG: i64 = -1;
const THIS_ARG_DONE: i64 = -2;
const START_VALUE_LIST: i64 = -3;
const IN_VALUE_LIST: i64 = -4;
const NON_OPTION_ARG: i64 = -5;
const STOP_VALUE_LIST: i64 = -6;
const READ_REST_ARGS_VERBATIM: i64 = -7;

/// O estado de `get_option` entre as chamadas.
pub struct OptState {
    pub args: Vec<Vec<u8>>,
    pub argnum: i64,
    pub optchar: i64,
    pub first_nonopt: i64,
    pub option_num: i64,
}

impl OptState {
    pub fn new(args: Vec<Vec<u8>>) -> OptState {
        OptState { args, argnum: 0, optchar: 0, first_nonopt: 0, option_num: NO_MATCH }
    }

    fn at(&self, i: i64) -> Option<&Vec<u8>> {
        if i < 0 { None } else { self.args.get(i as usize) }
    }
}

fn optionerr(err: &str, optind: usize, islong: bool) -> String {
    let o = &OPTIONS[optind];
    let nm = if islong { o.long } else { o.short };
    let optname = if !o.name.is_empty() { format!("'{}' ({})", nm, o.name) } else { format!("'{}'", nm) };
    err.replacen("%s", &optname, 1)
}

/// `get_shortopt`: devolve (id, valor, negado); `Err` é a mensagem do `ZIPERR(ZE_PARMS, ...)`.
fn get_shortopt(st: &mut OptState, argnum: i64, optchar: &mut i64) -> Result<(u32, Option<Vec<u8>>, bool), String> {
    let arg = st.args[argnum as usize].clone();
    *optchar += 1;
    let mut negated = false;
    let sopt = |i: i64| -> u8 { arg.get(i as usize).copied().unwrap_or(0) };
    if sopt(*optchar) == 0 {
        *optchar = 0;
        st.option_num = NO_MATCH;
        return Ok((0, None, false));
    }
    let s0 = sopt(*optchar);
    let s1 = sopt(*optchar + 1);
    let mut matched: i64 = -1;
    for (op, o) in OPTIONS.iter().enumerate() {
        let sb = o.short.as_bytes();
        if !sb.is_empty() && sb[0] == s0 {
            if sb.len() == 1 {
                matched = op as i64;
            } else if sb[1] == s1 {
                matched = op as i64;
                *optchar += 1;
                break;
            }
        }
    }
    if matched > -1 {
        let m = matched as usize;
        let o = &OPTIONS[m];
        // A) sinal de menos depois da opção nega.
        if sopt(*optchar + 1) == b'-' {
            if !o.neg {
                if o.vt == Vt::NoValue {
                    return Err(optionerr("option %s not negatable", m, false));
                }
            } else {
                negated = true;
                *optchar += 1;
            }
        }
        let mut value: Option<Vec<u8>> = None;
        if o.vt == Vt::Required || o.vt == Vt::ValueList {
            if sopt(*optchar + 1) != 0 {
                let mut clen = 1;
                if sopt(*optchar + clen) == b'=' {
                    clen += 1;
                }
                value = Some(arg[(*optchar + clen) as usize..].to_vec());
                *optchar = THIS_ARG_DONE;
            } else if let Some(next) = st.at(argnum + 1) {
                value = Some(next.clone());
                *optchar = if o.vt == Vt::ValueList { START_VALUE_LIST } else { SKIP_VALUE_ARG };
            } else {
                return Err(optionerr("option %s requires a value", m, false));
            }
        }
        st.option_num = matched;
        return Ok((o.id, value, negated));
    }
    Err(format!("short option '{}' not supported", s0 as char))
}

/// `get_longopt`.
fn get_longopt(st: &mut OptState, argnum: i64, optchar: &mut i64) -> Result<(u32, Option<Vec<u8>>, bool), String> {
    let full = st.args[argnum as usize].clone();
    let body = &full[2..];
    let (mut longopt, valuestart): (Vec<u8>, Option<Vec<u8>>) = match body.iter().position(|&b| b == b'=') {
        Some(i) => (body[..i].to_vec(), Some(body[i + 1..].to_vec())),
        None => (body.to_vec(), None),
    };
    // O último caractere antes do '=' (ou do fim), se for '-', nega.
    let mut negated = false;
    if longopt.last() == Some(&b'-') {
        negated = true;
        longopt.pop();
    }
    let lo = String::from_utf8_lossy(&longopt).into_owned();
    let mut matched: i64 = -1;
    for (op, o) in OPTIONS.iter().enumerate() {
        if o.long == lo {
            matched = op as i64;
            break;
        }
        if !o.long.is_empty() && o.long.as_bytes().starts_with(&longopt) || (o.long.is_empty() && longopt.is_empty()) {
            if matched > -1 {
                return Err(format!("long option '{}' ambiguous", lo));
            }
            matched = op as i64;
        }
    }
    if matched == -1 {
        return Err(format!("long option '{}' not supported", lo));
    }
    let m = matched as usize;
    let o = &OPTIONS[m];
    *optchar = THIS_ARG_DONE;
    if negated && !o.neg {
        return Err(optionerr("option %s not negatable", m, true));
    }
    let mut value: Option<Vec<u8>> = None;
    match o.vt {
        Vt::Required | Vt::ValueList => {
            if let Some(v) = valuestart {
                value = Some(v);
            } else if let Some(next) = st.at(argnum + 1) {
                value = Some(next.clone());
                *optchar = if o.vt == Vt::ValueList { START_VALUE_LIST } else { SKIP_VALUE_ARG };
            } else {
                return Err(optionerr("option %s requires a value", m, true));
            }
        }
        Vt::NoValue => {
            if valuestart.is_some() {
                return Err(optionerr("option %s does not allow a value", m, true));
            }
        }
    }
    st.option_num = matched;
    Ok((o.id, value, negated))
}

/// `get_option`: o próximo (id, valor, negado); id 0 quando acabaram os argumentos. As opções vêm
/// antes dos argumentos que não são opções (que são permutados para o fim).
pub fn get_option(st: &mut OptState) -> Result<(u32, Option<Vec<u8>>, bool), String> {
    let mut value: Option<Vec<u8>> = None;
    let mut negated = false;
    let argcnt = st.args.len() as i64;
    if argcnt < 2 {
        return Ok((0, None, false));
    }
    let mut first_nonoption_arg = st.first_nonopt;
    let mut argn = st.argnum;
    let mut optc = st.optchar;
    let mut read_rest = optc == READ_REST_ARGS_VERBATIM;
    if argn == -1 || argn == 0 {
        st.option_num = NO_MATCH;
        optc = THIS_ARG_DONE;
        first_nonoption_arg = -1;
    }
    let mut option_id: u32 = 0;
    if st.option_num != NO_MATCH {
        option_id = OPTIONS[st.option_num as usize].id;
    }
    let mut argcnt = argcnt;
    loop {
        if read_rest {
            argn += 1;
            if argn > argcnt || st.at(argn).is_none() {
                option_id = 0;
                break;
            }
            value = Some(st.args[argn as usize].clone());
            st.option_num = NO_MATCH;
            option_id = O_NON_OPTION_ARG;
            break;
        } else if matches!(optc, SKIP_VALUE_ARG | THIS_ARG_DONE | START_VALUE_LIST | IN_VALUE_LIST | STOP_VALUE_LIST) {
            // Permuta: move os argumentos que não são opção para depois desta opção.
            if first_nonoption_arg > -1 && st.at(first_nonoption_arg).is_some() {
                let v: i64 = if optc == SKIP_VALUE_ARG || optc == START_VALUE_LIST { 1 } else { 0 };
                let mut h = first_nonoption_arg;
                while h < argn {
                    let arg = st.args[first_nonoption_arg as usize].clone();
                    let mut j = first_nonoption_arg;
                    while j < argn + v {
                        st.args[j as usize] = st.args[(j + 1) as usize].clone();
                        j += 1;
                    }
                    st.args[j as usize] = arg;
                    h += 1;
                }
                first_nonoption_arg += 1 + v;
            }
        }

        if optc == STOP_VALUE_LIST {
            optc = THIS_ARG_DONE;
        }

        if optc == START_VALUE_LIST || optc == IN_VALUE_LIST {
            if optc == START_VALUE_LIST {
                argn += 1;
                optc = IN_VALUE_LIST;
            }
            argn += 1;
            if st.at(argn).is_none() && (optc == START_VALUE_LIST || optc == IN_VALUE_LIST) && first_nonoption_arg > -1 {
                // Termina a lista com "@".
                st.args.insert(first_nonoption_arg as usize, b"@".to_vec());
                argcnt = st.args.len() as i64;
                argn += 1;
                if first_nonoption_arg > -1 {
                    first_nonoption_arg += 1;
                }
            }
            match st.at(argn) {
                Some(a) if a.as_slice() == b"@" => {
                    optc = STOP_VALUE_LIST;
                    continue;
                }
                Some(a) if a.first() != Some(&b'-') => {
                    value = Some(a.clone());
                    break;
                }
                _ => {
                    argn -= 1;
                    optc = THIS_ARG_DONE;
                }
            }
        }

        if optc == SKIP_VALUE_ARG {
            argn += 2;
            optc = 0;
        } else if optc == THIS_ARG_DONE {
            argn += 1;
            optc = 0;
        }
        if argn > argcnt {
            break;
        }
        if st.at(argn).is_none() {
            if first_nonoption_arg > -1 && st.at(first_nonoption_arg).is_some() {
                if optc == NON_OPTION_ARG {
                    first_nonoption_arg += 1;
                }
                let j = argn;
                argn = first_nonoption_arg;
                first_nonoption_arg = j;
            }
            if argn > argcnt || st.at(argn).is_none() {
                option_id = 0;
                break;
            }
        }

        if first_nonoption_arg > -1 && st.at(first_nonoption_arg).is_none() {
            // Só sobraram argumentos que não são opções.
            if optc == NON_OPTION_ARG {
                argn += 1;
            }
            if argn > argcnt || st.at(argn).is_none() {
                option_id = 0;
                break;
            }
            value = Some(st.args[argn as usize].clone());
            optc = NON_OPTION_ARG;
            option_id = O_NON_OPTION_ARG;
            break;
        }

        let arg = st.args[argn as usize].clone();
        if arg.first() == Some(&b'-') {
            if arg.len() == 1 {
                // "-" sozinho: argumento que não é opção.
                st.option_num = NO_MATCH;
                if first_nonoption_arg < 0 {
                    first_nonoption_arg = argn;
                }
                argn += 1;
            } else if arg[1] == b'-' {
                if arg.len() == 2 {
                    // "--": o resto da linha é lido sem interpretação.
                    if first_nonoption_arg < 1 {
                        argn -= 1;
                    } else {
                        argn = first_nonoption_arg - 1;
                    }
                    read_rest = true;
                    optc = READ_REST_ARGS_VERBATIM;
                } else {
                    let r = get_longopt(st, argn, &mut optc)?;
                    option_id = r.0;
                    value = r.1;
                    negated = r.2;
                    break;
                }
            } else {
                let r = get_shortopt(st, argn, &mut optc)?;
                option_id = r.0;
                value = r.1;
                negated = r.2;
                if optc == 0 {
                    optc = THIS_ARG_DONE;
                } else {
                    break;
                }
            }
        } else {
            // Não é opção: permuta para o fim.
            if first_nonoption_arg < 0 {
                first_nonoption_arg = argn;
            }
            argn += 1;
        }
    }
    st.first_nonopt = first_nonoption_arg;
    st.argnum = argn;
    st.optchar = optc;
    Ok((option_id, value, negated))
}

/// `envargs`: acrescenta as opções de `ZIPOPT` (ou `ZIP`) do ambiente logo depois de `argv[0]`.
pub fn envargs(argv: Vec<Vec<u8>>, env: impl Fn(&str) -> Option<Vec<u8>>) -> Vec<Vec<u8>> {
    let mut envptr = env("ZIPOPT");
    let strip = |v: Vec<u8>| -> Vec<u8> {
        let n = v.iter().position(|b| !b.is_ascii_whitespace() && *b != 0x0b).unwrap_or(v.len());
        v[n..].to_vec()
    };
    envptr = envptr.map(strip);
    if envptr.as_ref().map_or(true, |v| v.is_empty()) {
        envptr = env("ZIP").map(strip);
    }
    let Some(e) = envptr.filter(|v| !v.is_empty()) else { return argv };
    let mut out = Vec::new();
    if let Some(first) = argv.first() {
        out.push(first.clone());
    }
    let mut p = 0usize;
    let isspace = |b: u8| b == b' ' || (9..=13).contains(&b);
    loop {
        let mut arg = Vec::new();
        if e.get(p) == Some(&b'"') {
            p += 1;
            while p < e.len() && e[p] != b'"' {
                if e[p] == b'\\' && p + 1 < e.len() {
                    p += 1;
                }
                arg.push(e[p]);
                p += 1;
            }
            if p < e.len() {
                p += 1;
            }
        } else {
            while p < e.len() && !isspace(e[p]) {
                arg.push(e[p]);
                p += 1;
            }
        }
        out.push(arg);
        while p < e.len() && isspace(e[p]) {
            p += 1;
        }
        if p >= e.len() {
            break;
        }
    }
    out.extend(argv.into_iter().skip(1));
    out
}
