//! Textos fixos do zip: ajuda, ajuda estendida, licença e a tela de versão, capturados byte a byte
//! do zip do Debian em `text/`.

pub const HELP: &[u8] = include_bytes!("text/help.txt");
pub const HELP2: &[u8] = include_bytes!("text/help2.txt");
pub const LICENSE: &[u8] = include_bytes!("text/license.txt");
/// `zip -v` até o título das variáveis de ambiente (os valores são do ambiente da vez).
pub const VERSION_HEAD: &[u8] = include_bytes!("text/version.txt");
