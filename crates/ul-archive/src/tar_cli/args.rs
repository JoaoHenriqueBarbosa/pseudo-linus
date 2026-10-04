//! Linha de comando do GNU tar 1.35: estilo antigo (`tar cvf x.tar`, o primeiro argumento sem `-` é um
//! grupo de letras cujos argumentos vêm dos argumentos seguintes, na ordem), opções curtas agrupadas,
//! longas com abreviação (o tar usa argp sobre o getopt_long: as mensagens do getopt saem com
//! `Try 'tar --help' or 'tar --usage' for more information.` e código 64), operandos na ordem em que
//! aparecem (o `-C` vale pros operandos seguintes).
//!
//! A ordem da tabela de opções longas é a do tar (observada nas mensagens de abreviação ambígua).

use crate::getopt::{Error as GetoptError, Getopt, HasArg, Item, LongOpt};

use super::header::Format;
use super::quote::{self, Quoting, Style};

/// Identificadores das opções longas sem letra (as com letra usam o próprio caractere).
pub mod id {
    pub const ATIME_PRESERVE: u32 = 0x100;
    pub const ACLS: u32 = 0x101;
    pub const ADD_FILE: u32 = 0x102;
    pub const ANCHORED: u32 = 0x103;
    pub const BACKUP: u32 = 0x104;
    pub const CHECK_DEVICE: u32 = 0x105;
    pub const CLAMP_MTIME: u32 = 0x106;
    pub const CHECKPOINT: u32 = 0x107;
    pub const CHECKPOINT_ACTION: u32 = 0x108;
    pub const DELETE: u32 = 0x109;
    pub const DELAY_DIR_RESTORE: u32 = 0x10a;
    pub const EXCLUDE: u32 = 0x10b;
    pub const EXCLUDE_CACHES: u32 = 0x10c;
    pub const EXCLUDE_CACHES_UNDER: u32 = 0x10d;
    pub const EXCLUDE_CACHES_ALL: u32 = 0x10e;
    pub const EXCLUDE_TAG: u32 = 0x10f;
    pub const EXCLUDE_IGNORE: u32 = 0x110;
    pub const EXCLUDE_IGNORE_RECURSIVE: u32 = 0x111;
    pub const EXCLUDE_TAG_UNDER: u32 = 0x112;
    pub const EXCLUDE_TAG_ALL: u32 = 0x113;
    pub const EXCLUDE_VCS: u32 = 0x114;
    pub const EXCLUDE_VCS_IGNORES: u32 = 0x115;
    pub const EXCLUDE_BACKUPS: u32 = 0x116;
    pub const FORCE_LOCAL: u32 = 0x117;
    pub const FULL_TIME: u32 = 0x118;
    pub const GROUP: u32 = 0x119;
    pub const GROUP_MAP: u32 = 0x11a;
    pub const HOLE_DETECTION: u32 = 0x11b;
    pub const HARD_DEREFERENCE: u32 = 0x11c;
    pub const IGNORE_FAILED_READ: u32 = 0x11d;
    pub const IGNORE_COMMAND_ERROR: u32 = 0x11e;
    pub const INDEX_FILE: u32 = 0x11f;
    pub const IGNORE_CASE: u32 = 0x120;
    pub const KEEP_NEWER_FILES: u32 = 0x121;
    pub const KEEP_DIRECTORY_SYMLINK: u32 = 0x122;
    pub const LEVEL: u32 = 0x123;
    pub const LZIP: u32 = 0x124;
    pub const LZMA: u32 = 0x125;
    pub const LZOP: u32 = 0x126;
    pub const MTIME: u32 = 0x127;
    pub const MODE: u32 = 0x128;
    pub const NO_SEEK: u32 = 0x129;
    pub const NO_CHECK_DEVICE: u32 = 0x12a;
    pub const NO_OVERWRITE_DIR: u32 = 0x12b;
    pub const NO_IGNORE_COMMAND_ERROR: u32 = 0x12c;
    pub const NO_SAME_OWNER: u32 = 0x12d;
    pub const NUMERIC_OWNER: u32 = 0x12e;
    pub const NO_SAME_PERMISSIONS: u32 = 0x12f;
    pub const NO_DELAY_DIR_RESTORE: u32 = 0x130;
    pub const NO_XATTRS: u32 = 0x131;
    pub const NO_SELINUX: u32 = 0x132;
    pub const NO_ACLS: u32 = 0x133;
    pub const NO_AUTO_COMPRESS: u32 = 0x134;
    pub const NEWER_MTIME: u32 = 0x135;
    pub const NO_QUOTE_CHARS: u32 = 0x136;
    pub const NULL: u32 = 0x137;
    pub const NO_NULL: u32 = 0x138;
    pub const NO_UNQUOTE: u32 = 0x139;
    pub const NO_VERBATIM_FILES_FROM: u32 = 0x13a;
    pub const NO_RECURSION: u32 = 0x13b;
    pub const NO_ANCHORED: u32 = 0x13c;
    pub const NO_IGNORE_CASE: u32 = 0x13d;
    pub const NO_WILDCARDS: u32 = 0x13e;
    pub const NO_WILDCARDS_MATCH_SLASH: u32 = 0x13f;
    pub const OCCURRENCE: u32 = 0x140;
    pub const OVERWRITE: u32 = 0x141;
    pub const OVERWRITE_DIR: u32 = 0x142;
    pub const ONE_TOP_LEVEL: u32 = 0x143;
    pub const OWNER: u32 = 0x144;
    pub const OWNER_MAP: u32 = 0x145;
    pub const OLD_ARCHIVE: u32 = 0x146;
    pub const ONE_FILE_SYSTEM: u32 = 0x147;
    pub const POSIX: u32 = 0x148;
    pub const PAX_OPTION: u32 = 0x149;
    pub const PROGRAM_NAME: u32 = 0x14a;
    pub const QUOTING_STYLE: u32 = 0x14b;
    pub const QUOTE_CHARS: u32 = 0x14c;
    pub const REMOVE_FILES: u32 = 0x14d;
    pub const RECURSIVE_UNLINK: u32 = 0x14e;
    pub const RMT_COMMAND: u32 = 0x14f;
    pub const RSH_COMMAND: u32 = 0x150;
    pub const RECORD_SIZE: u32 = 0x151;
    pub const RESTRICT: u32 = 0x152;
    pub const RECURSION: u32 = 0x153;
    pub const SPARSE_VERSION: u32 = 0x154;
    pub const SKIP_OLD_FILES: u32 = 0x155;
    pub const SAME_OWNER: u32 = 0x156;
    pub const SORT: u32 = 0x157;
    pub const SELINUX: u32 = 0x158;
    pub const SUFFIX: u32 = 0x159;
    pub const STRIP_COMPONENTS: u32 = 0x15a;
    pub const SHOW_DEFAULTS: u32 = 0x15b;
    pub const SHOW_SNAPSHOT_FIELD_RANGES: u32 = 0x15c;
    pub const SHOW_OMITTED_DIRS: u32 = 0x15d;
    pub const SHOW_TRANSFORMED_NAMES: u32 = 0x15e;
    pub const SHOW_STORED_NAMES: u32 = 0x15f;
    pub const TEST_LABEL: u32 = 0x160;
    pub const TO_COMMAND: u32 = 0x161;
    pub const TRANSFORM: u32 = 0x162;
    pub const TOTALS: u32 = 0x163;
    pub const UTC: u32 = 0x164;
    pub const UNQUOTE: u32 = 0x165;
    pub const USAGE: u32 = 0x166;
    pub const VOLNO_FILE: u32 = 0x167;
    pub const VERBATIM_FILES_FROM: u32 = 0x168;
    pub const VERSION: u32 = 0x169;
    pub const WARNING: u32 = 0x16a;
    pub const WILDCARDS: u32 = 0x16b;
    pub const WILDCARDS_MATCH_SLASH: u32 = 0x16c;
    pub const XATTRS: u32 = 0x16d;
    pub const XATTRS_INCLUDE: u32 = 0x16e;
    pub const XATTRS_EXCLUDE: u32 = 0x16f;
    pub const ZSTD: u32 = 0x170;
    pub const HELP: u32 = b'?' as u32;
}

const fn c(ch: u8) -> u32 {
    ch as u32
}

use HasArg::{No, Optional as Opt, Required as Req};

/// Tabela de opções longas, agrupada pela primeira letra na ordem do tar.
pub const LONG: &[LongOpt] = &[
    LongOpt::new("append", No, c(b'r')),
    LongOpt::new("atime-preserve", Opt, id::ATIME_PRESERVE),
    LongOpt::new("acls", No, id::ACLS),
    LongOpt::new("auto-compress", No, c(b'a')),
    LongOpt::new("absolute-names", No, c(b'P')),
    LongOpt::new("after-date", Req, c(b'N')),
    LongOpt::new("add-file", Req, id::ADD_FILE),
    LongOpt::new("anchored", No, id::ANCHORED),
    LongOpt::new("blocking-factor", Req, c(b'b')),
    LongOpt::new("bzip2", No, c(b'j')),
    LongOpt::new("backup", Opt, id::BACKUP),
    LongOpt::new("block-number", No, c(b'R')),
    LongOpt::new("create", No, c(b'c')),
    LongOpt::new("compare", No, c(b'd')),
    LongOpt::new("catenate", No, c(b'A')),
    LongOpt::new("concatenate", No, c(b'A')),
    LongOpt::new("check-device", No, id::CHECK_DEVICE),
    LongOpt::new("clamp-mtime", No, id::CLAMP_MTIME),
    LongOpt::new("compress", No, c(b'Z')),
    LongOpt::new("checkpoint", Opt, id::CHECKPOINT),
    LongOpt::new("checkpoint-action", Req, id::CHECKPOINT_ACTION),
    LongOpt::new("check-links", No, c(b'l')),
    LongOpt::new("confirmation", No, c(b'w')),
    LongOpt::new("diff", No, c(b'd')),
    LongOpt::new("delete", No, id::DELETE),
    LongOpt::new("delay-directory-restore", No, id::DELAY_DIR_RESTORE),
    LongOpt::new("dereference", No, c(b'h')),
    LongOpt::new("directory", Req, c(b'C')),
    LongOpt::new("extract", No, c(b'x')),
    LongOpt::new("exclude", Req, id::EXCLUDE),
    LongOpt::new("exclude-from", Req, c(b'X')),
    LongOpt::new("exclude-caches", No, id::EXCLUDE_CACHES),
    LongOpt::new("exclude-caches-under", No, id::EXCLUDE_CACHES_UNDER),
    LongOpt::new("exclude-caches-all", No, id::EXCLUDE_CACHES_ALL),
    LongOpt::new("exclude-tag", Req, id::EXCLUDE_TAG),
    LongOpt::new("exclude-ignore", Req, id::EXCLUDE_IGNORE),
    LongOpt::new("exclude-ignore-recursive", Req, id::EXCLUDE_IGNORE_RECURSIVE),
    LongOpt::new("exclude-tag-under", Req, id::EXCLUDE_TAG_UNDER),
    LongOpt::new("exclude-tag-all", Req, id::EXCLUDE_TAG_ALL),
    LongOpt::new("exclude-vcs", No, id::EXCLUDE_VCS),
    LongOpt::new("exclude-vcs-ignores", No, id::EXCLUDE_VCS_IGNORES),
    LongOpt::new("exclude-backups", No, id::EXCLUDE_BACKUPS),
    LongOpt::new("file", Req, c(b'f')),
    LongOpt::new("force-local", No, id::FORCE_LOCAL),
    LongOpt::new("format", Req, c(b'H')),
    LongOpt::new("full-time", No, id::FULL_TIME),
    LongOpt::new("files-from", Req, c(b'T')),
    LongOpt::new("get", No, c(b'x')),
    LongOpt::new("group", Req, id::GROUP),
    LongOpt::new("group-map", Req, id::GROUP_MAP),
    LongOpt::new("gzip", No, c(b'z')),
    LongOpt::new("gunzip", No, c(b'z')),
    LongOpt::new("hole-detection", Req, id::HOLE_DETECTION),
    LongOpt::new("hard-dereference", No, id::HARD_DEREFERENCE),
    LongOpt::new("help", No, id::HELP),
    LongOpt::new("incremental", No, c(b'G')),
    LongOpt::new("ignore-failed-read", No, id::IGNORE_FAILED_READ),
    LongOpt::new("ignore-command-error", No, id::IGNORE_COMMAND_ERROR),
    LongOpt::new("info-script", Req, c(b'F')),
    LongOpt::new("ignore-zeros", No, c(b'i')),
    LongOpt::new("index-file", Req, id::INDEX_FILE),
    LongOpt::new("interactive", No, c(b'w')),
    LongOpt::new("ignore-case", No, id::IGNORE_CASE),
    LongOpt::new("keep-old-files", No, c(b'k')),
    LongOpt::new("keep-newer-files", No, id::KEEP_NEWER_FILES),
    LongOpt::new("keep-directory-symlink", No, id::KEEP_DIRECTORY_SYMLINK),
    LongOpt::new("list", No, c(b't')),
    LongOpt::new("listed-incremental", Req, c(b'g')),
    LongOpt::new("level", Req, id::LEVEL),
    LongOpt::new("label", Req, c(b'V')),
    LongOpt::new("lzip", No, id::LZIP),
    LongOpt::new("lzma", No, id::LZMA),
    LongOpt::new("lzop", No, id::LZOP),
    LongOpt::new("mtime", Req, id::MTIME),
    LongOpt::new("mode", Req, id::MODE),
    LongOpt::new("multi-volume", No, c(b'M')),
    LongOpt::new("no-seek", No, id::NO_SEEK),
    LongOpt::new("no-check-device", No, id::NO_CHECK_DEVICE),
    LongOpt::new("no-overwrite-dir", No, id::NO_OVERWRITE_DIR),
    LongOpt::new("no-ignore-command-error", No, id::NO_IGNORE_COMMAND_ERROR),
    LongOpt::new("no-same-owner", No, id::NO_SAME_OWNER),
    LongOpt::new("numeric-owner", No, id::NUMERIC_OWNER),
    LongOpt::new("no-same-permissions", No, id::NO_SAME_PERMISSIONS),
    LongOpt::new("no-delay-directory-restore", No, id::NO_DELAY_DIR_RESTORE),
    LongOpt::new("no-xattrs", No, id::NO_XATTRS),
    LongOpt::new("no-selinux", No, id::NO_SELINUX),
    LongOpt::new("no-acls", No, id::NO_ACLS),
    LongOpt::new("new-volume-script", Req, c(b'F')),
    LongOpt::new("no-auto-compress", No, id::NO_AUTO_COMPRESS),
    LongOpt::new("newer", Req, c(b'N')),
    LongOpt::new("newer-mtime", Req, id::NEWER_MTIME),
    LongOpt::new("no-quote-chars", Req, id::NO_QUOTE_CHARS),
    LongOpt::new("null", No, id::NULL),
    LongOpt::new("no-null", No, id::NO_NULL),
    LongOpt::new("no-unquote", No, id::NO_UNQUOTE),
    LongOpt::new("no-verbatim-files-from", No, id::NO_VERBATIM_FILES_FROM),
    LongOpt::new("no-recursion", No, id::NO_RECURSION),
    LongOpt::new("no-anchored", No, id::NO_ANCHORED),
    LongOpt::new("no-ignore-case", No, id::NO_IGNORE_CASE),
    LongOpt::new("no-wildcards", No, id::NO_WILDCARDS),
    LongOpt::new("no-wildcards-match-slash", No, id::NO_WILDCARDS_MATCH_SLASH),
    LongOpt::new("occurrence", Opt, id::OCCURRENCE),
    LongOpt::new("overwrite", No, id::OVERWRITE),
    LongOpt::new("overwrite-dir", No, id::OVERWRITE_DIR),
    LongOpt::new("one-top-level", Opt, id::ONE_TOP_LEVEL),
    LongOpt::new("owner", Req, id::OWNER),
    LongOpt::new("owner-map", Req, id::OWNER_MAP),
    LongOpt::new("old-archive", No, id::OLD_ARCHIVE),
    LongOpt::new("one-file-system", No, id::ONE_FILE_SYSTEM),
    LongOpt::new("preserve-permissions", No, c(b'p')),
    LongOpt::new("preserve-order", No, c(b's')),
    LongOpt::new("portability", No, id::OLD_ARCHIVE),
    LongOpt::new("posix", No, id::POSIX),
    LongOpt::new("pax-option", Req, id::PAX_OPTION),
    LongOpt::new("program-name", Req, id::PROGRAM_NAME),
    LongOpt::new("quoting-style", Req, id::QUOTING_STYLE),
    LongOpt::new("quote-chars", Req, id::QUOTE_CHARS),
    LongOpt::new("remove-files", No, id::REMOVE_FILES),
    LongOpt::new("recursive-unlink", No, id::RECURSIVE_UNLINK),
    LongOpt::new("rmt-command", Req, id::RMT_COMMAND),
    LongOpt::new("rsh-command", Req, id::RSH_COMMAND),
    LongOpt::new("record-size", Req, id::RECORD_SIZE),
    LongOpt::new("read-full-records", No, c(b'B')),
    LongOpt::new("restrict", No, id::RESTRICT),
    LongOpt::new("recursion", No, id::RECURSION),
    LongOpt::new("sparse", No, c(b'S')),
    LongOpt::new("sparse-version", Req, id::SPARSE_VERSION),
    LongOpt::new("seek", No, c(b'n')),
    LongOpt::new("skip-old-files", No, id::SKIP_OLD_FILES),
    LongOpt::new("same-owner", No, id::SAME_OWNER),
    LongOpt::new("same-permissions", No, c(b'p')),
    LongOpt::new("same-order", No, c(b's')),
    LongOpt::new("sort", Req, id::SORT),
    LongOpt::new("selinux", No, id::SELINUX),
    LongOpt::new("starting-file", Req, c(b'K')),
    LongOpt::new("suffix", Req, id::SUFFIX),
    LongOpt::new("strip-components", Req, id::STRIP_COMPONENTS),
    LongOpt::new("show-defaults", No, id::SHOW_DEFAULTS),
    LongOpt::new("show-snapshot-field-ranges", No, id::SHOW_SNAPSHOT_FIELD_RANGES),
    LongOpt::new("show-omitted-dirs", No, id::SHOW_OMITTED_DIRS),
    LongOpt::new("show-transformed-names", No, id::SHOW_TRANSFORMED_NAMES),
    LongOpt::new("show-stored-names", No, id::SHOW_STORED_NAMES),
    LongOpt::new("test-label", No, id::TEST_LABEL),
    LongOpt::new("to-stdout", No, c(b'O')),
    LongOpt::new("to-command", Req, id::TO_COMMAND),
    LongOpt::new("touch", No, c(b'm')),
    LongOpt::new("tape-length", Req, c(b'L')),
    LongOpt::new("transform", Req, id::TRANSFORM),
    LongOpt::new("totals", Opt, id::TOTALS),
    LongOpt::new("update", No, c(b'u')),
    LongOpt::new("unlink-first", No, c(b'U')),
    LongOpt::new("use-compress-program", Req, c(b'I')),
    LongOpt::new("ungzip", No, c(b'z')),
    LongOpt::new("uncompress", No, c(b'Z')),
    LongOpt::new("utc", No, id::UTC),
    LongOpt::new("unquote", No, id::UNQUOTE),
    LongOpt::new("usage", No, id::USAGE),
    LongOpt::new("verify", No, c(b'W')),
    LongOpt::new("volno-file", Req, id::VOLNO_FILE),
    LongOpt::new("verbose", No, c(b'v')),
    LongOpt::new("verbatim-files-from", No, id::VERBATIM_FILES_FROM),
    LongOpt::new("version", No, id::VERSION),
    LongOpt::new("warning", Req, id::WARNING),
    LongOpt::new("wildcards", No, id::WILDCARDS),
    LongOpt::new("wildcards-match-slash", No, id::WILDCARDS_MATCH_SLASH),
    LongOpt::new("xattrs", No, id::XATTRS),
    LongOpt::new("xattrs-include", Req, id::XATTRS_INCLUDE),
    LongOpt::new("xattrs-exclude", Req, id::XATTRS_EXCLUDE),
    LongOpt::new("xz", No, c(b'J')),
    LongOpt::new("xform", Req, id::TRANSFORM),
    LongOpt::new("zstd", No, id::ZSTD),
];

/// Opções curtas (getopt): as que levam argumento têm `:`.
pub const SHORT: &str = "AcdrtuxGnSkUWOmpsMBiajJzZhPlRvwo?g:C:T:X:f:F:L:b:H:V:I:K:N:";

/// Letras que levam argumento no estilo antigo.
const OLD_WITH_ARG: &[u8] = b"gCTXfFLbHVIKN";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Create,
    Append,
    Update,
    Extract,
    List,
    Diff,
    Catenate,
    Delete,
    TestLabel,
}

impl Mode {
    fn from_id(i: u32) -> Option<Mode> {
        Some(match i {
            x if x == c(b'c') => Mode::Create,
            x if x == c(b'r') => Mode::Append,
            x if x == c(b'u') => Mode::Update,
            x if x == c(b'x') => Mode::Extract,
            x if x == c(b't') => Mode::List,
            x if x == c(b'd') => Mode::Diff,
            x if x == c(b'A') => Mode::Catenate,
            id::DELETE => Mode::Delete,
            id::TEST_LABEL => Mode::TestLabel,
            _ => return None,
        })
    }
}

/// Compressão escolhida.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Compression {
    Gzip,
    Bzip2,
    Xz,
    Lzma,
    Lzip,
    Zstd,
    Compress,
    Lzop,
    Program(Vec<u8>),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SortOrder {
    None,
    Name,
    Inode,
}

/// Opções de casamento de padrão em vigor (posicionais: valem pros padrões seguintes).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MatchFlags {
    pub anchored: Option<bool>,
    pub ignore_case: bool,
    pub wildcards: Option<bool>,
    pub match_slash: Option<bool>,
}

impl Default for MatchFlags {
    fn default() -> Self {
        MatchFlags { anchored: None, ignore_case: false, wildcards: None, match_slash: None }
    }
}

/// Um operando, com o diretório (`-C`) e as opções de casamento em vigor quando apareceu.
#[derive(Clone, Debug)]
pub struct NameArg {
    pub name: Vec<u8>,
    /// Sequência de `-C` em vigor (cumulativa, como chdir).
    pub chdir: Vec<Vec<u8>>,
    pub flags: MatchFlags,
    pub recursion: bool,
    /// Veio de um arquivo de `-T`.
    pub from_file: bool,
    /// É o próprio `-T ARQUIVO` (expandido na hora de processar, no lugar em que apareceu).
    pub list_file: bool,
}

/// Padrão de exclusão.
#[derive(Clone, Debug)]
pub struct Exclude {
    pub pattern: Vec<u8>,
    pub flags: MatchFlags,
}

/// Ação de `--checkpoint-action`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CheckpointAction {
    Bell,
    Dot,
    Echo(Option<Vec<u8>>),
    Ttyout(Vec<u8>),
    Wait(Vec<u8>),
    Sleep(u64),
    Exec(Vec<u8>),
    Totals,
}

/// Uma expressão de `--transform`.
#[derive(Clone, Debug)]
pub struct Transform {
    pub expr: Vec<u8>,
}

#[derive(Clone, Debug)]
pub struct Opts {
    pub mode: Option<Mode>,
    pub archives: Vec<Vec<u8>>,
    pub verbose: u32,
    pub block_number: bool,
    pub totals: bool,
    pub compression: Option<Compression>,
    pub auto_compress: bool,
    pub format: Option<Format>,
    pub blocking_factor: usize,
    pub record_size: Option<usize>,
    pub names: Vec<NameArg>,
    pub files_from: Vec<(Vec<u8>, Vec<Vec<u8>>)>,
    pub excludes: Vec<Exclude>,
    pub exclude_from: Vec<(Vec<u8>, MatchFlags)>,
    pub exclude_vcs: bool,
    pub exclude_backups: bool,
    pub exclude_caches: u8,
    pub exclude_tags: Vec<(Vec<u8>, u8)>,
    pub strip_components: usize,
    pub same_permissions: Option<bool>,
    pub same_owner: Option<bool>,
    pub numeric_owner: bool,
    pub owner: Option<Vec<u8>>,
    pub group: Option<Vec<u8>>,
    pub mode_changes: Option<Vec<u8>>,
    pub mtime: Option<Vec<u8>>,
    pub clamp_mtime: bool,
    pub sort: SortOrder,
    pub dereference: bool,
    pub hard_dereference: bool,
    pub keep_old_files: bool,
    pub skip_old_files: bool,
    pub overwrite: bool,
    pub overwrite_dir: Option<bool>,
    pub keep_newer_files: bool,
    pub unlink_first: bool,
    pub recursive_unlink: bool,
    pub keep_directory_symlink: bool,
    pub touch: bool,
    pub to_stdout: bool,
    pub to_command: Option<Vec<u8>>,
    pub remove_files: bool,
    pub check_links: bool,
    pub one_file_system: bool,
    pub ignore_zeros: bool,
    pub occurrence: Option<u64>,
    pub starting_file: Option<Vec<u8>>,
    pub newer: Option<Vec<u8>>,
    pub newer_mtime_only: bool,
    pub checkpoint: Option<u64>,
    pub checkpoint_actions: Vec<CheckpointAction>,
    pub warnings_off: Vec<Vec<u8>>,
    pub warnings_on: Vec<Vec<u8>>,
    pub show_transformed: bool,
    pub show_stored: bool,
    pub show_omitted_dirs: bool,
    pub transforms: Vec<Transform>,
    pub index_file: Option<Vec<u8>>,
    pub sparse: bool,
    pub sparse_version: Option<(u32, u32)>,
    pub absolute_names: bool,
    pub ignore_failed_read: bool,
    pub utc: bool,
    pub full_time: bool,
    pub quoting: Quoting,
    pub null: bool,
    pub verbatim_files_from: bool,
    pub unquote: bool,
    pub delay_dir_restore: Option<bool>,
    pub one_top_level: Option<Option<Vec<u8>>>,
    pub backup: Option<Option<Vec<u8>>>,
    pub suffix: Option<Vec<u8>>,
    pub label: Option<Vec<u8>>,
    pub interactive: bool,
    pub restrict: bool,
    pub verify: bool,
    pub pax_options: Vec<Vec<u8>>,
    pub atime_preserve: bool,
    pub preserve_order: bool,
    pub o_flag: bool,
    pub old_archive: bool,
    pub ignore_command_error: bool,
    pub program_name: Option<String>,
    pub read_full_records: bool,
    pub seek: Option<bool>,
    pub show_defaults: bool,
    pub multi_volume: bool,
    pub incremental: bool,
    pub listed_incremental: Option<Vec<u8>>,
    pub xattrs: bool,
    /// Todos os `-C` da linha, na ordem (a extração sem operandos usa o diretório resultante).
    pub final_chdir: Vec<Vec<u8>>,
}

impl Default for Opts {
    fn default() -> Self {
        Opts {
            mode: None,
            archives: Vec::new(),
            verbose: 0,
            block_number: false,
            totals: false,
            compression: None,
            auto_compress: false,
            format: None,
            blocking_factor: 20,
            record_size: None,
            names: Vec::new(),
            files_from: Vec::new(),
            excludes: Vec::new(),
            exclude_from: Vec::new(),
            exclude_vcs: false,
            exclude_backups: false,
            exclude_caches: 0,
            exclude_tags: Vec::new(),
            strip_components: 0,
            same_permissions: None,
            same_owner: None,
            numeric_owner: false,
            owner: None,
            group: None,
            mode_changes: None,
            mtime: None,
            clamp_mtime: false,
            sort: SortOrder::None,
            dereference: false,
            hard_dereference: false,
            keep_old_files: false,
            skip_old_files: false,
            overwrite: false,
            overwrite_dir: None,
            keep_newer_files: false,
            unlink_first: false,
            recursive_unlink: false,
            keep_directory_symlink: false,
            touch: false,
            to_stdout: false,
            to_command: None,
            remove_files: false,
            check_links: false,
            one_file_system: false,
            ignore_zeros: false,
            occurrence: None,
            starting_file: None,
            newer: None,
            newer_mtime_only: false,
            checkpoint: None,
            checkpoint_actions: Vec::new(),
            warnings_off: Vec::new(),
            warnings_on: Vec::new(),
            show_transformed: false,
            show_stored: false,
            show_omitted_dirs: false,
            transforms: Vec::new(),
            index_file: None,
            sparse: false,
            sparse_version: None,
            absolute_names: false,
            ignore_failed_read: false,
            utc: false,
            full_time: false,
            quoting: Quoting::default(),
            null: false,
            verbatim_files_from: false,
            unquote: true,
            delay_dir_restore: None,
            one_top_level: None,
            backup: None,
            suffix: None,
            label: None,
            interactive: false,
            restrict: false,
            verify: false,
            pax_options: Vec::new(),
            atime_preserve: false,
            preserve_order: false,
            o_flag: false,
            old_archive: false,
            ignore_command_error: false,
            program_name: None,
            read_full_records: false,
            seek: None,
            show_defaults: false,
            multi_volume: false,
            incremental: false,
            listed_incremental: None,
            xattrs: false,
            final_chdir: Vec::new(),
        }
    }
}

/// Como uma falha na linha de comando termina.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ArgError {
    /// Erro do getopt: mensagem pronta (com `tar: `), depois o "Try", código 64.
    Getopt(Vec<u8>),
    /// `USAGE_ERROR` do tar: `tar: msg`, "Try", código 2.
    Usage(Vec<u8>),
    /// `FATAL_ERROR`: `tar: msg` e "Error is not recoverable", código 2.
    Fatal(Vec<u8>),
    /// Texto pronto no stderr, código 2 (argmatch: "invalid argument ... Valid arguments are").
    Plain(Vec<u8>),
}

/// O que fazer depois da análise.
pub enum Parsed {
    Run(Box<Opts>),
    Help,
    Usage,
    Version,
    ShowDefaults,
}

/// Converte o estilo antigo (`tar cvf x.tar dir`) pro novo.
fn expand_old_style(args: &[Vec<u8>]) -> Result<Vec<Vec<u8>>, ArgError> {
    let Some(first) = args.get(1) else { return Ok(args.to_vec()) };
    if first.is_empty() || first[0] == b'-' {
        return Ok(args.to_vec());
    }
    let mut out = vec![args[0].clone()];
    let mut rest = args[2..].iter();
    for &ch in first {
        out.push(vec![b'-', ch]);
        if OLD_WITH_ARG.contains(&ch) {
            match rest.next() {
                Some(v) => out.push(v.clone()),
                None => {
                    return Err(ArgError::Usage(format!("Old option '{}' requires an argument.", ch as char).into_bytes()));
                }
            }
        }
    }
    out.extend(rest.cloned());
    Ok(out)
}

fn invalid_arg(value: &[u8], option: &str, valid: &[&[&str]]) -> ArgError {
    let mut s = Vec::new();
    s.extend_from_slice(b"tar: invalid argument ");
    s.extend_from_slice(&quote::locale(value));
    s.extend_from_slice(format!(" for \u{2018}--{option}\u{2019}\nValid arguments are:\n").as_bytes());
    for group in valid {
        s.extend_from_slice(b"  - ");
        let items: Vec<String> = group.iter().map(|v| format!("\u{2018}{v}\u{2019}")).collect();
        s.extend_from_slice(items.join(", ").as_bytes());
        s.push(b'\n');
    }
    ArgError::Plain(s)
}

fn parse_uint(v: &[u8]) -> Option<u64> {
    let s = std::str::from_utf8(v).ok()?;
    if s.is_empty() || !s.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    s.parse().ok()
}

/// Tamanho com sufixo (b, k, K, M, G...) como o `--record-size` e o `-L` aceitam.
fn parse_size_suffix(v: &[u8]) -> Option<u64> {
    let s = std::str::from_utf8(v).ok()?;
    let digits: String = s.chars().take_while(|c| c.is_ascii_digit()).collect();
    let n: u64 = digits.parse().ok()?;
    let suffix = &s[digits.len()..];
    let mult: u64 = match suffix {
        "" => 1,
        "b" => 512,
        "c" => 1,
        "k" | "K" => 1024,
        "M" => 1 << 20,
        "G" => 1 << 30,
        "T" => 1 << 40,
        "w" => 2,
        _ => return None,
    };
    n.checked_mul(mult)
}

const WARNINGS: &[&str] = &[
    "all", "alone-zero-block", "bad-dumpdir", "cachedir", "contiguous-cast", "file-changed", "file-ignored",
    "file-removed", "file-shrank", "file-unchanged", "filename-with-nuls", "ignore-archive", "ignore-newer",
    "new-directory", "rename-directory", "symlink-cast", "timestamp", "unknown-cast", "unknown-keyword", "xdev",
    "decompress-program", "existing-file", "xattr-write", "record-size", "failed-read", "missing-zero-blocks",
    "verbose",
];

const SIGNALS: &[&str] = &["SIGHUP", "SIGQUIT", "SIGINT", "SIGUSR1", "SIGUSR2", "HUP", "QUIT", "INT", "USR1", "USR2"];

struct State {
    opts: Opts,
    chdir: Vec<Vec<u8>>,
    flags: MatchFlags,
    recursion: bool,
    help: Option<u32>,
    exclude_flags_last: MatchFlags,
}

impl State {
    fn set_mode(&mut self, m: Mode) -> Result<(), ArgError> {
        if let Some(prev) = self.opts.mode
            && prev != m
        {
            return Err(ArgError::Usage(
                b"You may not specify more than one '-Acdtrux', '--delete' or  '--test-label' option".to_vec(),
            ));
        }
        self.opts.mode = Some(m);
        Ok(())
    }

    fn set_compression(&mut self, c: Compression) -> Result<(), ArgError> {
        if let Some(prev) = &self.opts.compression
            && *prev != c
        {
            return Err(ArgError::Usage(b"Conflicting compression options".to_vec()));
        }
        self.opts.compression = Some(c);
        Ok(())
    }

    fn operand(&mut self, name: Vec<u8>) {
        self.opts.names.push(NameArg {
            name,
            chdir: self.chdir.clone(),
            flags: self.flags,
            recursion: self.recursion,
            from_file: false,
            list_file: false,
        });
    }

    fn apply(&mut self, oid: u32, arg: Option<Vec<u8>>) -> Result<(), ArgError> {
        let a = || arg.clone().unwrap_or_default();
        if let Some(m) = Mode::from_id(oid) {
            return self.set_mode(m);
        }
        let o = &mut self.opts;
        match oid {
            x if x == c(b'G') => o.incremental = true,
            x if x == c(b'g') => o.listed_incremental = Some(a()),
            x if x == c(b'n') => o.seek = Some(true),
            id::NO_SEEK => o.seek = Some(false),
            x if x == c(b'S') => o.sparse = true,
            id::SPARSE_VERSION => {
                let v = a();
                let s = String::from_utf8_lossy(&v).into_owned();
                let (ma, mi) = s.split_once('.').unwrap_or((&s, "0"));
                match (ma.parse::<u32>(), mi.parse::<u32>()) {
                    (Ok(a), Ok(b)) => o.sparse_version = Some((a, b)),
                    _ => return Err(ArgError::Usage(b"Invalid sparse version value".to_vec())),
                }
                o.sparse = true;
            }
            x if x == c(b'C') => self.chdir.push(a()),
            x if x == c(b'T') => {
                o.files_from.push((a(), self.chdir.clone()));
                o.names.push(NameArg {
                    name: a(),
                    chdir: self.chdir.clone(),
                    flags: self.flags,
                    recursion: self.recursion,
                    from_file: false,
                    list_file: true,
                });
            }
            x if x == c(b'X') => o.exclude_from.push((a(), self.flags)),
            id::EXCLUDE => {
                o.excludes.push(Exclude { pattern: a(), flags: self.flags });
                self.exclude_flags_last = self.flags;
            }
            id::EXCLUDE_VCS => o.exclude_vcs = true,
            id::EXCLUDE_VCS_IGNORES => {}
            id::EXCLUDE_BACKUPS => o.exclude_backups = true,
            id::EXCLUDE_CACHES => o.exclude_caches = 1,
            id::EXCLUDE_CACHES_UNDER => o.exclude_caches = 2,
            id::EXCLUDE_CACHES_ALL => o.exclude_caches = 3,
            id::EXCLUDE_TAG => o.exclude_tags.push((a(), 1)),
            id::EXCLUDE_TAG_UNDER => o.exclude_tags.push((a(), 2)),
            id::EXCLUDE_TAG_ALL => o.exclude_tags.push((a(), 3)),
            id::EXCLUDE_IGNORE | id::EXCLUDE_IGNORE_RECURSIVE => {}
            id::ADD_FILE => self.operand(a()),
            id::ANCHORED => self.flags.anchored = Some(true),
            id::NO_ANCHORED => self.flags.anchored = Some(false),
            id::IGNORE_CASE => self.flags.ignore_case = true,
            id::NO_IGNORE_CASE => self.flags.ignore_case = false,
            id::WILDCARDS => self.flags.wildcards = Some(true),
            id::NO_WILDCARDS => self.flags.wildcards = Some(false),
            id::WILDCARDS_MATCH_SLASH => self.flags.match_slash = Some(true),
            id::NO_WILDCARDS_MATCH_SLASH => self.flags.match_slash = Some(false),
            id::RECURSION => self.recursion = true,
            id::NO_RECURSION => self.recursion = false,
            id::NULL => o.null = true,
            id::NO_NULL => o.null = false,
            id::UNQUOTE => o.unquote = true,
            id::NO_UNQUOTE => o.unquote = false,
            id::VERBATIM_FILES_FROM => o.verbatim_files_from = true,
            id::NO_VERBATIM_FILES_FROM => o.verbatim_files_from = false,
            x if x == c(b'k') => o.keep_old_files = true,
            id::KEEP_NEWER_FILES => o.keep_newer_files = true,
            id::KEEP_DIRECTORY_SYMLINK => o.keep_directory_symlink = true,
            id::SKIP_OLD_FILES => o.skip_old_files = true,
            id::OVERWRITE => o.overwrite = true,
            id::OVERWRITE_DIR => o.overwrite_dir = Some(true),
            id::NO_OVERWRITE_DIR => o.overwrite_dir = Some(false),
            id::ONE_TOP_LEVEL => o.one_top_level = Some(arg.clone()),
            id::RECURSIVE_UNLINK => o.recursive_unlink = true,
            id::REMOVE_FILES => o.remove_files = true,
            x if x == c(b'U') => o.unlink_first = true,
            x if x == c(b'W') => o.verify = true,
            id::IGNORE_COMMAND_ERROR => o.ignore_command_error = true,
            id::NO_IGNORE_COMMAND_ERROR => o.ignore_command_error = false,
            x if x == c(b'O') => o.to_stdout = true,
            id::TO_COMMAND => o.to_command = Some(a()),
            id::ATIME_PRESERVE => {
                if let Some(v) = &arg
                    && v != b"replace"
                    && v != b"system"
                {
                    return Err(invalid_arg(v, "atime-preserve", &[&["replace"], &["system"]]));
                }
                o.atime_preserve = true;
            }
            id::CLAMP_MTIME => o.clamp_mtime = true,
            id::DELAY_DIR_RESTORE => o.delay_dir_restore = Some(true),
            id::NO_DELAY_DIR_RESTORE => o.delay_dir_restore = Some(false),
            id::GROUP => o.group = Some(a()),
            id::OWNER => o.owner = Some(a()),
            id::GROUP_MAP | id::OWNER_MAP => {}
            id::MODE => o.mode_changes = Some(a()),
            id::MTIME => o.mtime = Some(a()),
            x if x == c(b'm') => o.touch = true,
            id::NO_SAME_OWNER => o.same_owner = Some(false),
            id::SAME_OWNER => o.same_owner = Some(true),
            id::NO_SAME_PERMISSIONS => o.same_permissions = Some(false),
            x if x == c(b'p') => o.same_permissions = Some(true),
            id::NUMERIC_OWNER => o.numeric_owner = true,
            id::SORT => {
                o.sort = match a().as_slice() {
                    b"none" => SortOrder::None,
                    b"name" => SortOrder::Name,
                    b"inode" => SortOrder::Inode,
                    v => return Err(invalid_arg(v, "sort", &[&["none"], &["name"], &["inode"]])),
                }
            }
            x if x == c(b's') => o.preserve_order = true,
            id::ACLS | id::NO_ACLS | id::SELINUX | id::NO_SELINUX | id::NO_XATTRS => {}
            id::XATTRS => o.xattrs = true,
            id::XATTRS_INCLUDE | id::XATTRS_EXCLUDE => {}
            id::FORCE_LOCAL => {}
            x if x == c(b'f') => o.archives.push(a()),
            x if x == c(b'F') => {}
            x if x == c(b'L') => o.multi_volume = true,
            x if x == c(b'M') => o.multi_volume = true,
            id::RMT_COMMAND | id::RSH_COMMAND | id::VOLNO_FILE => {}
            x if x == c(b'b') => {
                let v = a();
                match parse_uint(&v) {
                    Some(n) if n > 0 && n <= (i32::MAX as u64) / 512 => o.blocking_factor = n as usize,
                    _ => {
                        let mut m = v.clone();
                        m.extend_from_slice(b": Invalid blocking factor");
                        return Err(ArgError::Usage(m));
                    }
                }
            }
            x if x == c(b'B') => o.read_full_records = true,
            x if x == c(b'i') => o.ignore_zeros = true,
            id::RECORD_SIZE => {
                let v = a();
                match parse_size_suffix(&v) {
                    Some(n) if n % 512 == 0 && n > 0 => o.record_size = Some(n as usize),
                    Some(_) => return Err(ArgError::Usage(b"Record size must be a multiple of 512.".to_vec())),
                    None => {
                        let mut m = v.clone();
                        m.extend_from_slice(b": Invalid record size");
                        return Err(ArgError::Usage(m));
                    }
                }
            }
            x if x == c(b'H') => {
                let v = a();
                match Format::parse(&v) {
                    Some(f) => o.format = Some(f),
                    None => {
                        let mut m = v.clone();
                        m.extend_from_slice(b": Invalid archive format");
                        return Err(ArgError::Usage(m));
                    }
                }
            }
            id::OLD_ARCHIVE => o.old_archive = true,
            id::POSIX => o.format = Some(Format::Pax),
            id::PAX_OPTION => o.pax_options.push(a()),
            x if x == c(b'V') => o.label = Some(a()),
            x if x == c(b'a') => o.auto_compress = true,
            id::NO_AUTO_COMPRESS => o.auto_compress = false,
            x if x == c(b'I') => self.set_compression(Compression::Program(a()))?,
            x if x == c(b'j') => self.set_compression(Compression::Bzip2)?,
            x if x == c(b'J') => self.set_compression(Compression::Xz)?,
            x if x == c(b'z') => self.set_compression(Compression::Gzip)?,
            x if x == c(b'Z') => self.set_compression(Compression::Compress)?,
            id::LZIP => self.set_compression(Compression::Lzip)?,
            id::LZMA => self.set_compression(Compression::Lzma)?,
            id::LZOP => self.set_compression(Compression::Lzop)?,
            id::ZSTD => self.set_compression(Compression::Zstd)?,
            id::BACKUP => {
                if let Some(v) = &arg
                    && !matches!(v.as_slice(), b"none" | b"off" | b"simple" | b"never" | b"existing" | b"nil" | b"numbered" | b"t")
                {
                    return Err(invalid_arg(
                        v,
                        "backup",
                        &[&["none", "off"], &["simple", "never"], &["existing", "nil"], &["numbered", "t"]],
                    ));
                }
                o.backup = Some(arg.clone());
            }
            id::HARD_DEREFERENCE => o.hard_dereference = true,
            x if x == c(b'h') => o.dereference = true,
            x if x == c(b'K') => o.starting_file = Some(a()),
            id::NEWER_MTIME => {
                o.newer = Some(a());
                o.newer_mtime_only = true;
            }
            x if x == c(b'N') => o.newer = Some(a()),
            id::ONE_FILE_SYSTEM => o.one_file_system = true,
            x if x == c(b'P') => o.absolute_names = true,
            id::SUFFIX => o.suffix = Some(a()),
            id::STRIP_COMPONENTS => {
                let v = a();
                match parse_uint(&v) {
                    Some(n) => o.strip_components = n as usize,
                    None => {
                        let mut m = v.clone();
                        m.extend_from_slice(b": Invalid number of elements");
                        return Err(ArgError::Usage(m));
                    }
                }
            }
            id::TRANSFORM => o.transforms.push(Transform { expr: a() }),
            id::CHECKPOINT => {
                o.checkpoint = match &arg {
                    None => Some(10),
                    Some(v) => match parse_uint(v) {
                        Some(n) => Some(n),
                        None => return Err(ArgError::Fatal(b"--checkpoint value is not an integer".to_vec())),
                    },
                };
            }
            id::CHECKPOINT_ACTION => {
                let v = a();
                let act = if v == b"bell" {
                    CheckpointAction::Bell
                } else if v == b"dot" || v == b"." {
                    CheckpointAction::Dot
                } else if v == b"echo" {
                    CheckpointAction::Echo(None)
                } else if let Some(t) = v.strip_prefix(b"echo=") {
                    CheckpointAction::Echo(Some(t.to_vec()))
                } else if let Some(t) = v.strip_prefix(b"ttyout=") {
                    CheckpointAction::Ttyout(t.to_vec())
                } else if let Some(t) = v.strip_prefix(b"wait=") {
                    CheckpointAction::Wait(t.to_vec())
                } else if let Some(t) = v.strip_prefix(b"sleep=") {
                    CheckpointAction::Sleep(parse_uint(t).unwrap_or(0))
                } else if let Some(t) = v.strip_prefix(b"exec=") {
                    CheckpointAction::Exec(t.to_vec())
                } else if v == b"totals" {
                    CheckpointAction::Totals
                } else {
                    let mut m = b"".to_vec();
                    m.extend_from_slice(&quote::locale(&v));
                    m.extend_from_slice(b": unknown checkpoint action");
                    return Err(ArgError::Usage(m));
                };
                o.checkpoint_actions.push(act);
            }
            id::FULL_TIME => o.full_time = true,
            id::INDEX_FILE => o.index_file = Some(a()),
            x if x == c(b'l') => o.check_links = true,
            id::NO_QUOTE_CHARS => o.quoting.except.extend_from_slice(&a()),
            id::QUOTE_CHARS => o.quoting.extra.extend_from_slice(&a()),
            id::QUOTING_STYLE => {
                let v = a();
                match Style::parse(&v) {
                    Some(s) => o.quoting.style = s,
                    None => {
                        let mut m = b"Unknown quoting style '".to_vec();
                        m.extend_from_slice(&v);
                        m.extend_from_slice(b"'. Try 'tar --quoting-style=help' to get a list.");
                        return Err(ArgError::Fatal(m));
                    }
                }
            }
            x if x == c(b'R') => o.block_number = true,
            id::SHOW_DEFAULTS => o.show_defaults = true,
            id::SHOW_OMITTED_DIRS => o.show_omitted_dirs = true,
            id::SHOW_SNAPSHOT_FIELD_RANGES => {}
            id::SHOW_TRANSFORMED_NAMES => o.show_transformed = true,
            id::SHOW_STORED_NAMES => o.show_stored = true,
            id::TOTALS => {
                if let Some(v) = &arg
                    && !SIGNALS.contains(&String::from_utf8_lossy(v).as_ref())
                {
                    let mut m = b"Unknown signal name: ".to_vec();
                    m.extend_from_slice(v);
                    return Err(ArgError::Fatal(m));
                }
                if arg.is_none() {
                    o.totals = true;
                }
            }
            id::UTC => o.utc = true,
            x if x == c(b'v') => o.verbose += 1,
            id::WARNING => {
                let v = a();
                let (neg, name) = match v.strip_prefix(b"no-") {
                    Some(r) => (true, r.to_vec()),
                    None => (false, v.clone()),
                };
                if v != b"none" && !WARNINGS.contains(&String::from_utf8_lossy(&name).as_ref()) {
                    let groups: Vec<&[&str]> = WARNINGS.iter().map(std::slice::from_ref).collect();
                    return Err(invalid_arg(&v, "warning", &groups));
                }
                if v == b"none" {
                    o.warnings_off.push(b"all".to_vec());
                } else if neg {
                    o.warnings_off.push(name);
                } else {
                    o.warnings_on.push(name);
                }
            }
            x if x == c(b'w') => o.interactive = true,
            id::RESTRICT => o.restrict = true,
            x if x == c(b'o') => o.o_flag = true,
            id::OCCURRENCE => {
                o.occurrence = match &arg {
                    None => Some(1),
                    Some(v) => match parse_uint(v) {
                        Some(n) => Some(n),
                        None => {
                            let mut m = v.clone();
                            m.extend_from_slice(b": Invalid number");
                            return Err(ArgError::Fatal(m));
                        }
                    },
                };
            }
            id::LEVEL => {
                if parse_uint(&a()).is_none() {
                    return Err(ArgError::Usage(b"Invalid incremental level value".to_vec()));
                }
            }
            id::HOLE_DETECTION => {
                let v = a();
                if v != b"raw" && v != b"seek" {
                    return Err(invalid_arg(&v, "hole-detection", &[&["raw"], &["seek"]]));
                }
            }
            id::IGNORE_FAILED_READ => o.ignore_failed_read = true,
            id::CHECK_DEVICE | id::NO_CHECK_DEVICE => {}
            id::PROGRAM_NAME => o.program_name = Some(String::from_utf8_lossy(&a()).into_owned()),
            id::HELP | id::USAGE | id::VERSION => self.help = Some(oid),
            _ => {}
        }
        Ok(())
    }
}

/// Analisa a linha de comando inteira.
pub fn parse(args: &[Vec<u8>], argv0: &str) -> Result<Parsed, ArgError> {
    let args = expand_old_style(args)?;
    let mut st = State {
        opts: Opts::default(),
        chdir: Vec::new(),
        flags: MatchFlags::default(),
        recursion: true,
        help: None,
        exclude_flags_last: MatchFlags::default(),
    };
    for item in Getopt::from_env(&args, SHORT, LONG) {
        match item {
            Ok(Item::Opt(o)) => {
                st.apply(o.id, o.arg)?;
                // argp trata --help/--usage/--version na hora (o resto da linha não é olhado).
                match st.help {
                    Some(id::HELP) => return Ok(Parsed::Help),
                    Some(id::USAGE) => return Ok(Parsed::Usage),
                    Some(id::VERSION) => return Ok(Parsed::Version),
                    _ => {}
                }
            }
            Ok(Item::Operand(name)) => st.operand(name),
            Err(e) => return Err(ArgError::Getopt(getopt_message(&e, argv0))),
        }
    }
    if st.opts.show_defaults {
        return Ok(Parsed::ShowDefaults);
    }
    st.opts.final_chdir = st.chdir.clone();
    Ok(Parsed::Run(Box::new(st.opts)))
}

fn getopt_message(e: &GetoptError, argv0: &str) -> Vec<u8> {
    e.message_bytes(argv0)
}

/// Aplica a uma linha de `-T` que começa com `-` (opção, como o tar 1.35 faz sem
/// `--verbatim-files-from`): devolve o diretório do `-C`, se for isso.
pub fn files_from_option(line: &[u8]) -> Option<Vec<u8>> {
    if let Some(rest) = line.strip_prefix(b"-C") {
        let v = rest.strip_prefix(b" ").unwrap_or(rest);
        return Some(v.to_vec());
    }
    if let Some(rest) = line.strip_prefix(b"--directory=") {
        return Some(rest.to_vec());
    }
    if let Some(rest) = line.strip_prefix(b"--directory ") {
        return Some(rest.to_vec());
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn argv(v: &[&str]) -> Vec<Vec<u8>> {
        v.iter().map(|s| s.as_bytes().to_vec()).collect()
    }

    #[test]
    fn old_style_expands_in_order() {
        let a = expand_old_style(&argv(&["tar", "cvfb", "x.tar", "20", "dir"])).unwrap();
        assert_eq!(a, argv(&["tar", "-c", "-v", "-f", "x.tar", "-b", "20", "dir"]));
        assert_eq!(
            expand_old_style(&argv(&["tar", "cvbf", "20"])).unwrap_err(),
            ArgError::Usage(b"Old option 'f' requires an argument.".to_vec())
        );
    }

    #[test]
    fn ambiguity_lists_follow_tar_order() {
        let a = argv(&["tar", "--c"]);
        let e = parse(&a, "tar").err().unwrap();
        let ArgError::Getopt(m) = e else { panic!() };
        assert_eq!(
            String::from_utf8(m).unwrap(),
            "tar: option '--c' is ambiguous; possibilities: '--create' '--compare' '--catenate' '--concatenate' '--check-device' '--clamp-mtime' '--compress' '--checkpoint' '--checkpoint-action' '--check-links' '--confirmation'\n"
        );
    }

    #[test]
    fn mode_and_compression_conflicts() {
        assert!(matches!(parse(&argv(&["tar", "-ct"]), "tar"), Err(ArgError::Usage(_))));
        assert!(matches!(parse(&argv(&["tar", "-czjf", "x"]), "tar"), Err(ArgError::Usage(_))));
        let Ok(Parsed::Run(o)) = parse(&argv(&["tar", "-C", "a", "-cf", "x.tar", "b", "-C", "c", "d"]), "tar") else {
            panic!()
        };
        assert_eq!(o.names.len(), 2);
        assert_eq!(o.names[1].chdir, vec![b"a".to_vec(), b"c".to_vec()]);
    }
}
