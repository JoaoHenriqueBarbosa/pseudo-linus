//! O estado global do zip (globals.c e as variáveis de `main` que sobrevivem à leitura dos argumentos):
//! opções, listas de entradas e de arquivos achados, e o arquivo de saída.

use jiff::tz::TimeZone;
use sysabi::Stat;

use super::consts::*;
use super::crypt::Keys;
use super::deflate::Deflate;
use super::out::{InFile, OutFile};

/// O desfecho de um erro fatal: o código de saída (o `ziperr` já imprimiu a mensagem e limpou).
#[derive(Debug, Clone, Copy)]
pub struct Exit(pub i32);

pub type R<T> = Result<T, Exit>;

/// `struct zlist`: uma entrada do arquivo zip (cabeçalho central e local).
#[derive(Clone, Default)]
pub struct Zlist {
    pub vem: u16,
    pub ver: u16,
    pub flg: u16,
    pub how: u16,
    pub tim: u64,
    pub crc: u64,
    pub siz: u64,
    pub len: u64,
    pub dsk: u64,
    pub att: u16,
    pub lflg: u16,
    pub off: u64,
    pub atx: u64,
    /// Nome externo bruto como foi dado, nome interno gravado, versão externa do interno e a
    /// versão de exibição.
    pub name: Vec<u8>,
    pub iname: Vec<u8>,
    pub zname: Vec<u8>,
    pub oname: Vec<u8>,
    pub extra: Vec<u8>,
    pub cextra: Vec<u8>,
    pub comment: Vec<u8>,
    pub uname: Option<Vec<u8>>,
    pub zuname: Option<Vec<u8>>,
    pub ouname: Option<Vec<u8>>,
    pub mark: i32,
    pub trash: bool,
    pub current: bool,
    pub dosflag: bool,
}

/// `struct flist`: um arquivo achado no disco que ainda não está no zip.
#[derive(Clone, Default)]
pub struct Flist {
    pub name: Vec<u8>,
    pub iname: Vec<u8>,
    pub zname: Vec<u8>,
    pub oname: Vec<u8>,
    pub uname: Option<Vec<u8>>,
    pub dosflag: bool,
    pub usize: u64,
}

/// `struct plist`: um padrão de `-i`, `-x` ou `-R`.
#[derive(Clone)]
pub struct Plist {
    pub zname: Vec<u8>,
    pub select: u8,
}

/// Tempos Unix de um arquivo (`iztimes`).
#[derive(Clone, Copy, Default)]
pub struct IzTimes {
    pub atime: i64,
    pub mtime: i64,
    pub ctime: i64,
}

/// O que `filetime` devolve de um arquivo.
#[derive(Clone, Copy, Default)]
pub struct FileInfo {
    pub tim: u64,
    pub attr: u64,
    /// Tamanho, ou -1 para o que não é arquivo comum nem link simbólico.
    pub size: i64,
    pub utim: IzTimes,
}

pub struct Zip {
    pub argv0: Vec<u8>,
    pub tz: TimeZone,

    // Opções de ação.
    pub action: i32,
    pub comadd: bool,
    pub zipedit: bool,
    pub latest: bool,
    pub test: bool,
    pub unzip_path: Option<Vec<u8>>,
    pub tempdir: bool,
    pub junk_sfx: bool,

    // Opções (globals.c).
    pub recurse: i32,
    pub dispose: bool,
    pub pathput: bool,
    pub method: i32,
    pub dosify: bool,
    pub verbose: i32,
    pub fix: i32,
    pub filesync: bool,
    pub adjust: bool,
    pub level: i32,
    pub translate_eol: i32,
    pub no_wild: bool,
    pub allow_regex: bool,
    pub wild_stop_at_dir: bool,
    pub dot_size: i64,
    pub dot_count: i64,
    pub display_counts: bool,
    pub display_bytes: bool,
    pub display_globaldots: bool,
    pub display_volume: bool,
    pub display_usize: bool,
    pub files_so_far: u64,
    pub bad_files_so_far: u64,
    pub files_total: u64,
    pub bytes_so_far: u64,
    pub good_bytes_so_far: u64,
    pub bad_bytes_so_far: u64,
    pub bytes_total: u64,
    pub logall: bool,
    pub logfile: Option<sysabi::Fd>,
    pub logfile_append: bool,
    pub logfile_path: Option<Vec<u8>>,
    pub dirnames: bool,
    pub filter_match_case: bool,
    pub diff_mode: bool,
    pub linkput: bool,
    pub noisy: bool,
    pub extra_fields: i32,
    pub use_descriptors: bool,
    pub zip_to_stdout: bool,
    pub allow_empty_archive: bool,
    pub copy_only: bool,
    pub allow_fifo: bool,
    pub show_files: i32,
    pub output_seekable: bool,
    pub force_zip64: i32,
    pub zip64_entry: bool,
    pub zip64_archive: bool,
    pub special: Option<Vec<u8>>,
    pub key: Option<Vec<u8>>,
    pub keys: Option<Keys>,
    pub tempath: Option<Vec<u8>>,
    pub utf8_force: bool,
    pub using_utf8: bool,
    pub unicode_escape_all: bool,
    pub unicode_mismatch: i32,
    pub scan_delay: i64,
    pub scan_dot_time: i64,
    pub scan_start: i64,
    pub scan_last: i64,
    pub scan_started: bool,
    pub scan_count: u64,
    pub before: u64,
    pub after: u64,
    pub have_out: bool,

    // O arquivo zip.
    pub zipfile: Vec<u8>,
    pub zipbeg: u64,
    pub cenbeg: u64,
    pub tempzn: u64,
    pub tempzip: Option<Vec<u8>>,
    pub y: Option<OutFile>,
    pub in_file: Option<InFile>,
    pub in_path: Vec<u8>,
    pub out_path: Vec<u8>,
    pub zip_attributes: u32,
    pub zipfile_exists: bool,
    pub zfiles: Vec<Zlist>,
    pub zsort: Vec<usize>,
    pub zusort: Vec<usize>,
    pub zcomment: Vec<u8>,
    pub zcount_at_read: usize,

    // Saída e posição (bfwrite).
    pub current_disk: u64,
    pub cd_start_disk: i64,
    pub cd_start_offset: u64,
    pub cd_entries_this_disk: u64,
    pub total_cd_entries: u64,
    pub bytes_this_split: u64,
    pub bytes_this_entry: u64,
    pub current_local_offset: u64,
    pub current_local_disk: u64,

    // Arquivos achados e padrões.
    pub found: Vec<Flist>,
    pub patterns: Vec<Plist>,
    pub icount: usize,
    pub rcount: usize,
    pub filterlist: Vec<Plist>,
    pub filelist: Vec<Vec<u8>>,

    // Mensagens.
    pub mesg_line_started: bool,
    pub logfile_line_started: bool,
    pub mesg_to_stderr: bool,
    pub error_level: i32,
    pub last_errno: Option<sysabi::Errno>,
    pub zipstate: i32,
    pub zipstatb: Option<Stat>,

    // O compressor persiste entre os arquivos (janela e tabelas).
    pub deflater: Option<Box<Deflate>>,
    pub bz_ibuf: Vec<u8>,

    // Entrada padrão em linhas (`-z`, `-c`, `-@`).
    pub stdin_buf: Vec<u8>,
    pub stdin_pos: usize,
    pub stdin_eof: bool,

    // Variáveis do `main` que atravessam as fases.
    pub grow: bool,
    pub kk: i32,
    pub first_listarg: i64,
    pub bad_open_is_error: bool,
    pub show_sd: bool,
    pub comment_stdin: bool,
    pub split_requested: bool,
    pub args_final: Vec<Vec<u8>>,
}

impl Zip {
    pub fn new(argv0: Vec<u8>) -> Zip {
        Zip {
            argv0,
            tz: crate::tz::local(),
            action: ADD,
            comadd: false,
            zipedit: false,
            latest: false,
            test: false,
            unzip_path: None,
            tempdir: false,
            junk_sfx: false,
            recurse: 0,
            dispose: false,
            pathput: true,
            method: BEST,
            dosify: false,
            verbose: 0,
            fix: 0,
            filesync: false,
            adjust: false,
            level: 6,
            translate_eol: 0,
            no_wild: false,
            allow_regex: true,
            wild_stop_at_dir: false,
            dot_size: 0,
            dot_count: 0,
            display_counts: false,
            display_bytes: false,
            display_globaldots: false,
            display_volume: false,
            display_usize: false,
            files_so_far: 0,
            bad_files_so_far: 0,
            files_total: 0,
            bytes_so_far: 0,
            good_bytes_so_far: 0,
            bad_bytes_so_far: 0,
            bytes_total: 0,
            logall: false,
            logfile: None,
            logfile_append: false,
            logfile_path: None,
            dirnames: true,
            filter_match_case: true,
            diff_mode: false,
            linkput: false,
            noisy: true,
            extra_fields: 1,
            use_descriptors: false,
            zip_to_stdout: false,
            allow_empty_archive: false,
            copy_only: false,
            allow_fifo: false,
            show_files: 0,
            output_seekable: true,
            force_zip64: -1,
            zip64_entry: false,
            zip64_archive: false,
            special: Some(b".Z:.zip:.zoo:.arc:.lzh:.arj".to_vec()),
            key: None,
            keys: None,
            tempath: None,
            utf8_force: false,
            // O zip do Debian faz `setlocale(LC_CTYPE, "en_US.UTF-8")`, que existe no sistema de
            // referência: os nomes não ASCII vão em UTF-8 com o bit 11.
            using_utf8: true,
            unicode_escape_all: false,
            unicode_mismatch: 1,
            scan_delay: 5,
            scan_dot_time: 2,
            scan_start: 0,
            scan_last: 0,
            scan_started: false,
            scan_count: 0,
            before: 0,
            after: 0,
            have_out: false,
            zipfile: Vec::new(),
            zipbeg: 0,
            cenbeg: 0,
            tempzn: 0,
            tempzip: None,
            y: None,
            in_file: None,
            in_path: Vec::new(),
            out_path: Vec::new(),
            zip_attributes: 0,
            zipfile_exists: false,
            zfiles: Vec::new(),
            zsort: Vec::new(),
            zusort: Vec::new(),
            zcomment: Vec::new(),
            zcount_at_read: 0,
            current_disk: 0,
            cd_start_disk: -1,
            cd_start_offset: 0,
            cd_entries_this_disk: 0,
            total_cd_entries: 0,
            bytes_this_split: 0,
            bytes_this_entry: 0,
            current_local_offset: 0,
            current_local_disk: 0,
            found: Vec::new(),
            patterns: Vec::new(),
            icount: 0,
            rcount: 0,
            filterlist: Vec::new(),
            filelist: Vec::new(),
            mesg_line_started: false,
            logfile_line_started: false,
            mesg_to_stderr: false,
            error_level: 0,
            last_errno: None,
            zipstate: -1,
            zipstatb: None,
            deflater: None,
            bz_ibuf: Vec::new(),
            stdin_buf: Vec::new(),
            stdin_pos: 0,
            stdin_eof: false,
            grow: false,
            kk: 0,
            first_listarg: 0,
            bad_open_is_error: false,
            show_sd: false,
            comment_stdin: true,
            split_requested: false,
            args_final: Vec::new(),
        }
    }

    pub fn is_stdout_zip(&self) -> bool {
        self.zipfile == b"-"
    }
}
