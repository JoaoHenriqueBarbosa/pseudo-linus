//! Textos fixos do unzip e do zipinfo. Os longos (uso, ajuda estendida, versão) são a saída do
//! Debian capturada byte a byte, em `text/`.

pub const UNZIP_USAGE: &[u8] = include_bytes!("text/unzip_usage.txt");
pub const UNZIP_HELP: &[u8] = include_bytes!("text/unzip_help.txt");
pub const ZIPINFO_USAGE: &[u8] = include_bytes!("text/zipinfo_usage.txt");
/// `unzip -v` até o título das variáveis de ambiente (os valores são do ambiente da vez).
pub const VERSION_HEAD: &[u8] = include_bytes!("text/version.txt");

pub const NOT_EXTRACTING: &str = "caution:  not extracting; -d ignored\n";
/// O cabeçalho do `unzipsfx` e a linha de opções válidas do uso dele.
pub const SFX_BANNER: &str = "UnZipSFX 6.00 of 20 April 2009, by Info-ZIP (http://www.info-zip.org).\n";
pub const SFX_VALID_OPTIONS: &str = "Valid options are -tfupcz and -d <exdir>; modifiers are -abjnoqCLDMVX.\n";
pub const MUST_GIVE_EXDIR: &str = "error:  must specify directory to which to extract with -d option\n";
pub const ONLY_ONE_EXDIR: &str = "error:  -d option used more than once (only one exdir allowed)\n";
pub const MUST_GIVE_PASSWD: &str = "error:  must give decryption password with -P option\n";
pub const Z_FIRST: &str = "error:  -Z must be first option for ZipInfo mode (check UNZIP variable?)\n";
pub const INVALID_OPTIONS: &str = "error:  -fn or any combination of -c, -l, -p, -t, -u and -v options invalid\n";
pub const IGNORE_O_OPTION: &str = "caution:  both -n and -o specified; ignoring -o\n";

pub const READ_ERROR: &str = "error:  zipfile read error\n";
pub const REPORT_MSG: &str = "  (please check that you have transferred or created the zipfile in the\n  appropriate BINARY mode and that you have compiled UnZip properly)\n";
pub const FILENAME_NOT_MATCHED: &str = "caution: filename not matched:  ";
pub const EXCL_FILENAME_NOT_MATCHED: &str = "caution: excluded filename not matched:  ";
