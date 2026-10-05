//! `unzip` e `zipinfo` do Info-ZIP UnZip 6.0 (Debian 6.0-29+deb13u1), portados do C com as opções de
//! compilação do pacote (`unzip -v`): Unix, UTF-8, Zip64, bzip2, deflate64, unshrink, links
//! simbólicos, datas UT, UID/GID e decifração tradicional.
//!
//! Um módulo por arquivo do fonte: `opts` (uz_opts, zi_opts, envargs), `process` (process.c),
//! `list` (list.c), `zipinfo` (zipinfo.c), `extract` (extract.c), `fileio` (fileio.c e unix.c) e
//! `matching` (match.c).

mod crypt;
mod explode;
mod extract;
mod fileio;
mod inflate;
mod input;
mod list;
mod matching;
mod member;
mod opts;
mod output;
mod process;
mod testef;
mod text;
mod unix;
mod unshrink;
mod zipinfo;

use sysabi::Fd;

pub fn sys() -> std::sync::Arc<dyn sysabi::Syscalls> {
    sysabi::sys::current()
}

/// Códigos de saída (`PK_*` e `IZ_*` do unzip.h).
pub const PK_OK: i32 = 0;
pub const PK_WARN: i32 = 1;
pub const PK_ERR: i32 = 2;
pub const PK_BADERR: i32 = 3;
pub const PK_MEM: i32 = 4;
pub const PK_MEM3: i32 = 6;
pub const PK_NOZIP: i32 = 9;
pub const PK_PARAM: i32 = 10;
pub const PK_FIND: i32 = 11;
pub const PK_BOMB: i32 = 12;
pub const PK_DISK: i32 = 50;
pub const PK_EOF: i32 = 51;
pub const IZ_UNSUP: i32 = 81;
pub const IZ_BADPWD: i32 = 82;
/// O "arquivo zip" é um diretório (só interno, vira `PK_NOZIP`).
pub const IZ_DIR: i32 = 76;

/// Bits do `Info()`: saída de erro (que com `-t` vai pro stdout), quebra de linha antes se a linha
/// corrente não está no começo, e quebra garantida no fim.
pub const MSG_STDERR: u32 = 0x1;
pub const MSG_LNEWLN: u32 = 0x20;
pub const MSG_TNEWLN: u32 = 0x40;

/// As opções (`UzpOpts`).
#[derive(Default)]
pub struct Opts {
    pub exdir: Option<Vec<u8>>,
    pub pwdarg: Option<Vec<u8>>,
    pub zipinfo_mode: bool,
    pub aflag: i32,
    pub b_flag: bool,
    pub cflag: bool,
    pub c_flag: bool,
    pub d_flag: i32,
    pub fflag: bool,
    pub acorn_nfs_ext: bool,
    pub hflag: i32,
    pub jflag: bool,
    pub k_flag: bool,
    pub lflag: i32,
    pub l_flag: i32,
    pub overwrite_none: bool,
    pub overwrite_all: i32,
    pub qflag: i32,
    pub tflag: i32,
    pub t_flag: bool,
    pub uflag: bool,
    pub u_flag: i32,
    pub vflag: i32,
    pub v_flag: bool,
    pub w_flag: bool,
    pub x_flag: i32,
    pub zflag: i32,
    pub ddotflag: i32,
    pub cflxflag: i32,
}

/// O estado global (`Uz_Globs`): opções, operandos e o que o processamento de cada arquivo precisa.
pub struct Uz {
    pub o: Opts,
    /// `argv[0]` como o programa foi chamado (só pra decidir entre unzip e zipinfo).
    pub argv0: Vec<u8>,
    /// Nenhum argumento além do nome do programa.
    pub noargs: bool,
    /// Rodando como `unzipsfx`: o zip é o próprio executável e não há nova tentativa com `.zip`.
    pub sfx: bool,
    pub m_flag: bool,
    /// O arquivo zip pedido (pode ter curingas) e o da vez.
    pub wildzipfn: Vec<u8>,
    pub zipfn: Vec<u8>,
    /// Membros pedidos e excluídos (`-x`); vazio e `process_all_files` = todos.
    pub pfnames: Vec<Vec<u8>>,
    pub pxnames: Vec<Vec<u8>>,
    pub process_all_files: bool,
    pub extract_flag: bool,
    /// A saída está no começo de uma linha (o `G.sol`, comum ao stdout e ao stderr).
    pub sol: bool,
    /// O arquivo zip da vez e o registro de fim dele.
    pub zin: process::ZipIn,
    pub ecrec: process::Ecrec,
    pub real_ecrec_offset: i64,
    pub expect_ecrec_offset: i64,
    /// O último arquivo tentado não tinha registro de fim (muda a mensagem de "não achado").
    pub no_ecrec: bool,
    /// O membro da vez: cabeçalho central, informações, nome (já convertido pro sistema) e o
    /// nome cru inteiro, campo extra, e a última assinatura lida.
    pub crec: process::Crec,
    pub lrec: process::Lrec,
    /// Bytes comprimidos que faltam ler do membro (`G.csize`).
    pub csize: i64,
    /// O membro usa Zip64 (`G.zip64`).
    pub zip64: bool,
    pub pinfo: process::MinInfo,
    pub filename: Vec<u8>,
    pub filename_full: Vec<u8>,
    pub extra_field: Option<Vec<u8>>,
    pub sig: [u8; 4],
    /// O locale é UTF-8 (nomes Unicode saem crus em vez de convertidos) e `-U` (escapar tudo).
    pub native_is_utf8: bool,
    pub unicode_escape_all: bool,
    /// O fuso local (`tzset()`).
    pub tz: jiff::tz::TimeZone,
    /// `-T`: a data mais nova entre os membros e quantos contaram (`uxstamp`, `nmember`).
    pub time_stamp: (i64, u64),
    /// O estado da extração.
    pub x: extract::Ex,
}

impl Uz {
    fn new(argv0: Vec<u8>) -> Uz {
        Uz {
            o: Opts { lflag: -1, ..Opts::default() },
            argv0,
            noargs: false,
            sfx: false,
            m_flag: false,
            wildzipfn: Vec::new(),
            zipfn: Vec::new(),
            pfnames: Vec::new(),
            pxnames: Vec::new(),
            process_all_files: false,
            extract_flag: false,
            sol: true,
            zin: process::ZipIn::default(),
            ecrec: process::Ecrec::default(),
            real_ecrec_offset: 0,
            expect_ecrec_offset: 0,
            no_ecrec: false,
            crec: process::Crec::default(),
            lrec: process::Lrec::default(),
            csize: 0,
            zip64: false,
            pinfo: process::MinInfo::default(),
            filename: Vec::new(),
            filename_full: Vec::new(),
            extra_field: None,
            sig: [0; 4],
            native_is_utf8: locale_is_utf8(),
            unicode_escape_all: false,
            tz: crate::tz::local(),
            time_stamp: (0, 0),
            x: extract::Ex::default(),
        }
    }

    /// O `Info()`/`UzpMessagePrnt`: escreve já formatado, no stderr só se não estiver testando (`-t`
    /// manda tudo pro stdout), e mantém o estado de começo de linha.
    pub fn info(&mut self, flag: u32, msg: impl AsRef<[u8]>) {
        let msg = msg.as_ref();
        let fd = if flag & MSG_STDERR != 0 && self.o.tflag == 0 { Fd::STDERR } else { Fd::STDOUT };
        let mut buf = Vec::with_capacity(msg.len() + 2);
        if flag & MSG_LNEWLN != 0 && !self.sol {
            buf.push(b'\n');
        }
        buf.extend_from_slice(msg);
        if flag & MSG_TNEWLN != 0 && ((msg.is_empty() && !self.sol) || msg.last().is_some_and(|&c| c != b'\n')) {
            buf.push(b'\n');
        }
        if buf.is_empty() {
            return;
        }
        let _ = sysabi::sys::write_all(fd, &buf);
        self.sol = buf.last() == Some(&b'\n');
    }
}

/// O `nl_langinfo(CODESET)` depois do `setlocale(LC_CTYPE, "")` é UTF-8.
fn locale_is_utf8() -> bool {
    let get = |k: &str| crate::sysutil::getenv(k).filter(|v| !v.is_empty());
    let loc = get("LC_ALL").or_else(|| get("LC_CTYPE")).or_else(|| get("LANG")).unwrap_or_default();
    let l = String::from_utf8_lossy(&loc).to_ascii_lowercase();
    l.contains("utf-8") || l.contains("utf8")
}

/// Depois das opções: o arquivo zip, os membros pedidos, `-x` com os excluídos e `-d DIR` em qualquer
/// ponto (o laço do fim de `unzip()`). Um `-d` no meio da lista de membros encerra a lista.
fn split_operands(g: &mut Uz, rest: Vec<Vec<u8>>) -> Result<(), i32> {
    g.wildzipfn = rest[0].clone();
    let ops = &rest[1..];
    if ops.is_empty() {
        g.process_all_files = true;
        return Ok(());
    }
    // Faixas de `ops` dos membros pedidos e dos excluídos; `pf` vazio = os padrões (todos).
    let mut pf: Option<(usize, usize)> = Some((0, ops.len()));
    let mut px: Option<(usize, usize)> = None;
    let (mut in_files, mut in_xfiles) = (false, false);
    let mut k = 0;
    while k < ops.len() {
        let a = &ops[k];
        if g.o.exdir.is_none() && a.starts_with(b"-d") {
            let firstarg = k == 0;
            let mut dir = a[2..].to_vec();
            if in_files {
                pf = pf.map(|(s, e)| (s, e.min(k)));
                in_files = false;
            } else if in_xfiles {
                px = px.map(|(s, e)| (s, e.min(k)));
            }
            if dir.is_empty() {
                if k + 1 < ops.len() {
                    k += 1;
                    dir = ops[k].clone();
                } else {
                    g.info(MSG_STDERR, text::MUST_GIVE_EXDIR);
                    return Err(PK_PARAM);
                }
            }
            g.o.exdir = Some(dir);
            if firstarg {
                if k + 1 < ops.len() {
                    pf = Some((k + 1, ops.len()));
                } else {
                    g.process_all_files = true;
                    pf = None;
                    break;
                }
            }
        } else if !in_xfiles {
            if a == b"-x" {
                in_xfiles = true;
                if pf.is_some_and(|(s, _)| s == k) {
                    pf = None;
                } else if in_files {
                    pf = pf.map(|(s, e)| (s, e.min(k)));
                    in_files = false;
                }
                px = Some((k + 1, ops.len()));
            } else {
                in_files = true;
            }
        }
        k += 1;
    }
    if let Some((s, e)) = pf {
        g.pfnames = ops[s..e.max(s)].to_vec();
    }
    if let Some((s, e)) = px {
        g.pxnames = ops[s..e.max(s)].to_vec();
    }
    Ok(())
}

/// `unzip` (ou `zipinfo`, pelo nome do programa ou por `-Z` como primeira opção).
pub fn main(argv: &[Vec<u8>]) -> i32 {
    run(argv, false)
}

/// Acha o próprio executável como o `unzipsfx` faz: um `argv[0]` com barra é o caminho; sem barra,
/// a primeira entrada do `PATH` que tenha um arquivo regular com esse nome.
fn find_myself(argv0: &[u8]) -> Option<Vec<u8>> {
    if argv0.is_empty() {
        return None;
    }
    let is_file = |p: &[u8]| sysabi::sys::stat(p).is_ok_and(|st| st.file_type() != sysabi::FileType::Directory);
    if argv0.contains(&b'/') {
        return is_file(argv0).then(|| argv0.to_vec());
    }
    let path = crate::sysutil::getenv("PATH")?;
    for dir in path.split(|&c| c == b':') {
        let dir: &[u8] = if dir.is_empty() { b"." } else { dir };
        let cand = [dir, b"/", argv0].concat();
        if is_file(&cand) {
            return Some(cand);
        }
    }
    None
}

/// `unzipsfx`: o extrator autoextraível. O arquivo zip é o próprio executável (com o zip colado no
/// fim), então os operandos são só membros e padrões; as opções são as do `unzip`.
pub fn main_sfx(argv: &[Vec<u8>]) -> i32 {
    let argv0 = argv.first().cloned().unwrap_or_default();
    let Some(me) = find_myself(&argv0) else {
        let msg = [b"unzipsfx:  cannot find myself! [".as_slice(), &argv0, b"]\n"].concat();
        let _ = sysabi::sys::write_all(Fd::STDERR, &msg);
        return PK_PARAM;
    };
    // O arquivo zip entra depois das opções (e do argumento solto de `-d`), antes dos membros.
    let mut full = vec![argv0];
    let mut inserted = false;
    let mut k = 1;
    while k < argv.len() {
        let a = &argv[k];
        if !inserted && !a.starts_with(b"-") {
            full.push(me.clone());
            inserted = true;
        }
        full.push(a.clone());
        if !inserted && a == b"-d" && k + 1 < argv.len() {
            k += 1;
            full.push(argv[k].clone());
        }
        k += 1;
    }
    if !inserted {
        full.push(me);
    }
    run(&full, true)
}

fn run(argv: &[Vec<u8>], sfx: bool) -> i32 {
    let mut g = Uz::new(argv.first().cloned().unwrap_or_default());
    g.sfx = sfx;
    g.noargs = !sfx && argv.len() == 1;
    let base = crate::sysutil::basename(&g.argv0).to_vec();
    g.o.zipinfo_mode = !sfx
        && (base.len() >= 7 && base[..7].eq_ignore_ascii_case(b"zipinfo")
            || base.len() >= 2 && base[..2].eq_ignore_ascii_case(b"ii")
            || argv.get(1).is_some_and(|a| a.starts_with(b"-Z")));
    let mut args = opts::envargs(&g, argv);
    let rest = match if g.o.zipinfo_mode { opts::zi_opts(&mut g, &mut args) } else { opts::uz_opts(&mut g, &mut args) } {
        Ok(Some(rest)) => rest,
        Ok(None) => return PK_OK,
        Err(code) => return code,
    };
    if let Err(code) = split_operands(&mut g, rest) {
        return code;
    }
    if g.o.exdir.is_some() && !g.extract_flag {
        g.info(MSG_STDERR, text::NOT_EXTRACTING);
    }
    g.unicode_escape_all = g.o.u_flag == 1;
    g.process_zipfiles()
}
