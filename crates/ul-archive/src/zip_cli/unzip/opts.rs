//! Opções: `envargs` (envargs.c), `uz_opts` (unzip.c) e `zi_opts` (zipinfo.c).

use super::{MSG_STDERR, PK_OK, PK_PARAM, Uz, text};

/// Resultado do parsing: `Ok(Some(resto))` com o arquivo zip e o que vem depois, `Ok(None)` quando a
/// ação já foi feita (ajuda, versão) e `Err(código)` pra sair.
pub type OptsResult = Result<Option<Vec<Vec<u8>>>, i32>;

fn is_space(c: u8) -> bool {
    matches!(c, b' ' | b'\t' | b'\n' | 0x0b | 0x0c | b'\r')
}

/// Põe as opções de `UNZIP` (ou `UNZIPOPT`; `ZIPINFO`/`ZIPINFOOPT` no zipinfo) logo depois do nome
/// do programa.
pub fn envargs(g: &Uz, argv: &[Vec<u8>]) -> Vec<Vec<u8>> {
    let (main, alt) = if g.o.zipinfo_mode { ("ZIPINFO", "ZIPINFOOPT") } else { ("UNZIP", "UNZIPOPT") };
    let pick = |name: &str| {
        crate::sysutil::getenv(name)
            .map(|v| v[v.iter().take_while(|&&c| is_space(c)).count()..].to_vec())
            .filter(|v| !v.is_empty())
    };
    let Some(env) = pick(main).or_else(|| pick(alt)) else { return argv.to_vec() };
    let mut out = vec![argv.first().cloned().unwrap_or_default()];
    out.extend(split_env(&env));
    out.extend(argv.iter().skip(1).cloned());
    out
}

/// `MAX(x - negative, 0)`, a forma de desligar com `-` as opções que acumulam.
fn decrease(x: &mut i32, negative: &mut i32) {
    *x = (*x - *negative).max(0);
    *negative = 0;
}

/// Opção que acumula: cada vez soma um, e negada (`--x`) desconta.
fn count(x: &mut i32, negative: &mut i32) {
    if *negative != 0 {
        decrease(x, negative);
    } else {
        *x += 1;
    }
}

/// Valor obrigatório de `-d`/`-P`: negada ou sem valor, a mensagem `missing` e `PK_PARAM`.
fn required_value(g: &mut Uz, neg: bool, args: &[Vec<u8>], i: &mut usize, arg: &[u8], pos: usize, missing: &str) -> Result<Vec<u8>, i32> {
    match option_value(args, i, arg, pos).filter(|_| !neg) {
        Some(v) => Ok(v),
        None => {
            g.info(MSG_STDERR, missing);
            Err(PK_PARAM)
        }
    }
}

/// `-M` só pagina quando o stdout é um terminal.
fn check_more(g: &mut Uz) {
    if g.m_flag && !super::sys().isatty(sysabi::Fd::STDOUT) {
        g.m_flag = false;
    }
}

/// Falta o arquivo zip ou houve erro: o texto de uso, e o código de saída se não for zero.
fn usage_result(g: &mut Uz, error: bool) -> OptsResult {
    match usage(g, error) {
        PK_OK => Ok(None),
        code => Err(code),
    }
}

/// Valor de `-d`/`-P`: o resto do argumento ou o próximo argumento, que não pode começar com `-`.
fn option_value(args: &[Vec<u8>], i: &mut usize, arg: &[u8], pos: usize) -> Option<Vec<u8>> {
    if pos < arg.len() {
        return Some(arg[pos..].to_vec());
    }
    if *i + 1 < args.len() {
        *i += 1;
        let v = args[*i].clone();
        if v.first() == Some(&b'-') { None } else { Some(v) }
    } else {
        None
    }
}

/// Opções do unzip (`uz_opts`). `args[0]` é o nome do programa.
pub fn uz_opts(g: &mut Uz, args: &mut [Vec<u8>]) -> OptsResult {
    let mut error = false;
    let mut negative = 0;
    let mut showhelp = 0;
    let mut i = 1;
    while i < args.len() && args[i].first() == Some(&b'-') {
        let arg = args[i].clone();
        let mut pos = 1;
        while pos < arg.len() {
            let c = arg[pos];
            pos += 1;
            let neg = negative != 0;
            // O `unzipsfx` só aceita as opções do texto de uso dele; as demais caem no erro.
            if g.sfx && !b"-abjnoqCLDMVXtfupczd".contains(&c) {
                error = true;
                continue;
            }
            match c {
                b'-' => negative += 1,
                b'a' => count(&mut g.o.aflag, &mut negative),
                b'b' => {
                    if neg {
                        negative = 0;
                    } else {
                        g.o.aflag = 0;
                    }
                }
                b'B' => (g.o.b_flag, negative) = (!neg, 0),
                b'c' => (g.o.cflag, negative) = (!neg, 0),
                b'C' => (g.o.c_flag, negative) = (!neg, 0),
                b'd' => {
                    if !neg && g.o.exdir.is_some() {
                        g.info(MSG_STDERR, text::ONLY_ONE_EXDIR);
                        return Err(PK_PARAM);
                    }
                    g.o.exdir = Some(required_value(g, neg, args, &mut i, &arg, pos, text::MUST_GIVE_EXDIR)?);
                    pos = arg.len();
                }
                b'D' => count(&mut g.o.d_flag, &mut negative),
                b'e' | b'x' => {}
                b'f' => {
                    (g.o.fflag, g.o.uflag, negative) = (!neg, !neg, 0);
                }
                b'F' => (g.o.acorn_nfs_ext, negative) = (!neg, 0),
                b'h' => {
                    if showhelp == 0 {
                        showhelp = if arg.get(pos) == Some(&b'h') { 2 } else { 1 };
                    }
                }
                b'j' => (g.o.jflag, negative) = (!neg, 0),
                b'K' => (g.o.k_flag, negative) = (!neg, 0),
                b'l' => count(&mut g.o.vflag, &mut negative),
                b'L' => count(&mut g.o.l_flag, &mut negative),
                b'M' => (g.m_flag, negative) = (!neg, 0),
                b'n' => (g.o.overwrite_none, negative) = (!neg, 0),
                b'o' => count(&mut g.o.overwrite_all, &mut negative),
                b'p' => {
                    if neg {
                        g.o.cflag = false;
                        g.o.qflag = (g.o.qflag - 999).max(0);
                        negative = 0;
                    } else {
                        g.o.cflag = true;
                        g.o.qflag += 999;
                    }
                }
                b'P' => {
                    if neg || g.o.pwdarg.is_none() {
                        g.o.pwdarg = Some(required_value(g, neg, args, &mut i, &arg, pos, text::MUST_GIVE_PASSWD)?);
                        pos = arg.len();
                    }
                }
                b'q' => count(&mut g.o.qflag, &mut negative),
                b't' => (g.o.tflag, negative) = (i32::from(!neg), 0),
                b'T' => (g.o.t_flag, negative) = (!neg, 0),
                b'u' => (g.o.uflag, negative) = (!neg, 0),
                b'U' => count(&mut g.o.u_flag, &mut negative),
                b'v' => {
                    if neg {
                        decrease(&mut g.o.vflag, &mut negative);
                    } else if g.o.vflag != 0 {
                        g.o.vflag += 1;
                    } else {
                        g.o.vflag = 2;
                    }
                }
                b'V' => (g.o.v_flag, negative) = (!neg, 0),
                b'W' => (g.o.w_flag, negative) = (!neg, 0),
                b'X' => count(&mut g.o.x_flag, &mut negative),
                b'z' => count(&mut g.o.zflag, &mut negative),
                b'Z' => {
                    g.info(MSG_STDERR, text::Z_FIRST);
                    error = true;
                }
                b':' => count(&mut g.o.ddotflag, &mut negative),
                b'^' => count(&mut g.o.cflxflag, &mut negative),
                _ => error = true,
            }
        }
        i += 1;
    }
    if showhelp > 0 {
        if showhelp == 2 {
            g.info(0, text::UNZIP_HELP);
        } else {
            usage(g, false);
        }
        return Ok(None);
    }
    if (g.o.cflag && (g.o.tflag != 0 || g.o.uflag)) || (g.o.tflag != 0 && g.o.uflag) || (g.o.fflag && g.o.overwrite_none) {
        g.info(MSG_STDERR, text::INVALID_OPTIONS);
        error = true;
    }
    g.o.aflag = g.o.aflag.min(2);
    if g.o.overwrite_all != 0 && g.o.overwrite_none {
        g.info(MSG_STDERR, text::IGNORE_O_OPTION);
        g.o.overwrite_all = 0;
    }
    check_more(g);
    if i >= args.len() || error {
        if g.o.vflag >= 2 && i >= args.len() {
            show_version_info(g);
            return Ok(None);
        }
        return usage_result(g, error || !g.noargs);
    }
    g.extract_flag = !(g.o.cflag || g.o.tflag != 0 || g.o.vflag != 0 || g.o.zflag != 0 || g.o.t_flag);
    Ok(Some(args[i..].to_vec()))
}

/// Formato de listagem do zipinfo; negado (`--s` etc.) vira -2, "nenhum pedido".
fn listing(lflag: &mut i32, level: i32, negative: &mut i32) {
    *lflag = if std::mem::take(negative) != 0 { -2 } else { level };
}

/// `-h`/`-t` do zipinfo: liga (ou, negado, desliga) o cabeçalho ou o rodapé em todos os formatos.
/// Ligado de propósito, vale mesmo com membros pedidos, e tira o formato do "nenhum pedido" (-1).
fn header_or_trailer(flags: [&mut bool; 2], explicit: &mut bool, lflag: &mut i32, negative: &mut i32) {
    let on = std::mem::take(negative) == 0;
    for f in flags {
        *f = on;
    }
    if on {
        *explicit = true;
        if *lflag == -1 {
            *lflag = 0;
        }
    }
}

/// Opções do zipinfo (`zi_opts`). `args[0]` é o nome do programa.
pub fn zi_opts(g: &mut Uz, args: &mut [Vec<u8>]) -> OptsResult {
    let mut error = false;
    let mut negative = 0;
    let (mut hflag_slmv, mut hflag_2, mut tflag_slm, mut tflag_2v) = (true, false, true, false);
    let (mut explicit_h, mut explicit_t) = (false, false);
    g.extract_flag = false;
    let mut i = 1;
    while i < args.len() && args[i].first() == Some(&b'-') {
        let arg = args[i].clone();
        for &c in &arg[1..] {
            let neg = negative != 0;
            match c {
                b'-' => negative += 1,
                b'1' => listing(&mut g.o.lflag, 1, &mut negative),
                b'2' => listing(&mut g.o.lflag, 2, &mut negative),
                b'l' => listing(&mut g.o.lflag, 5, &mut negative),
                b'm' => listing(&mut g.o.lflag, 4, &mut negative),
                b's' => listing(&mut g.o.lflag, 3, &mut negative),
                b'v' => listing(&mut g.o.lflag, 10, &mut negative),
                b'C' => (g.o.c_flag, negative) = (!neg, 0),
                b'h' => header_or_trailer([&mut hflag_2, &mut hflag_slmv], &mut explicit_h, &mut g.o.lflag, &mut negative),
                b'M' => (g.m_flag, negative) = (!neg, 0),
                b't' => header_or_trailer([&mut tflag_2v, &mut tflag_slm], &mut explicit_t, &mut g.o.lflag, &mut negative),
                b'T' => (g.o.t_flag, negative) = (!neg, 0),
                b'U' => count(&mut g.o.u_flag, &mut negative),
                b'W' => (g.o.w_flag, negative) = (!neg, 0),
                b'z' => {
                    if neg {
                        (g.o.zflag, negative) = (0, 0);
                    } else {
                        g.o.zflag = 1;
                    }
                }
                b'Z' => {}
                _ => error = true,
            }
        }
        i += 1;
    }
    if i >= args.len() || error {
        return usage_result(g, error);
    }
    check_more(g);
    // Os operandos depois do arquivo zip são membros pedidos.
    let members = args.len() - i - 1 > 0;
    if g.o.lflag < 0 || (members && g.o.lflag == 0) {
        g.o.lflag = 3;
    }
    match g.o.lflag {
        0 | 2 => (g.o.hflag, g.o.tflag) = (i32::from(hflag_2), i32::from(tflag_2v)),
        1 => (g.o.hflag, g.o.tflag, g.o.zflag) = (0, 0, 0),
        3..=5 => {
            g.o.hflag = i32::from(if members && !explicit_h { false } else { hflag_slmv });
            g.o.tflag = i32::from(if members && !explicit_t { false } else { tflag_slm });
        }
        10 => (g.o.hflag, g.o.tflag) = (i32::from(hflag_slmv), i32::from(tflag_2v)),
        _ => {}
    }
    Ok(Some(args[i..].to_vec()))
}

/// O texto de uso: no stdout quando pedido, no stderr (com `PK_PARAM`) quando é erro.
pub fn usage(g: &mut Uz, error: bool) -> i32 {
    if g.sfx {
        g.info(u32::from(error), [text::SFX_BANNER, text::SFX_VALID_OPTIONS].concat());
        return if error { PK_PARAM } else { PK_OK };
    }
    let text = if g.o.zipinfo_mode { text::ZIPINFO_USAGE } else { text::UNZIP_USAGE };
    g.info(u32::from(error), text);
    if error { PK_PARAM } else { PK_OK }
}

/// `unzip -v` sem arquivo: versão, opções de compilação e variáveis de ambiente.
fn show_version_info(g: &mut Uz) {
    if g.o.qflag > 3 {
        g.info(0, "600\n");
        return;
    }
    g.info(0, text::VERSION_HEAD);
    for name in ["UNZIP", "UNZIPOPT", "ZIPINFO", "ZIPINFOOPT"] {
        let value = crate::sysutil::getenv(name).filter(|v| !v.is_empty()).unwrap_or_else(|| b"[none]".to_vec());
        let mut line = format!("{name:>16}:  ").into_bytes();
        line.extend_from_slice(&value[..value.len().min(1024)]);
        line.push(b'\n');
        g.info(0, line);
    }
}

/// Separa o valor da variável em argumentos: brancos separam, e um argumento entre aspas pode ter
/// brancos e barras de escape (cada `\` some e protege o caractere seguinte).
fn split_env(s: &[u8]) -> Vec<Vec<u8>> {
    let mut args = Vec::new();
    let mut i = 0;
    loop {
        if s.get(i) == Some(&b'"') {
            i += 1;
            let start = i;
            while i < s.len() && s[i] != b'"' {
                if s[i] == b'\\' && i + 1 < s.len() {
                    i += 1;
                }
                i += 1;
            }
            let mut arg = Vec::new();
            let mut j = start;
            while j < i {
                if s[j] == b'\\' {
                    j += 1;
                    if j == i {
                        break;
                    }
                }
                arg.push(s[j]);
                j += 1;
            }
            args.push(arg);
            if i < s.len() {
                i += 1;
            }
        } else {
            let start = i;
            while i < s.len() && !is_space(s[i]) {
                i += 1;
            }
            args.push(s[start..i].to_vec());
        }
        while i < s.len() && is_space(s[i]) {
            i += 1;
        }
        if i >= s.len() {
            return args;
        }
    }
}
