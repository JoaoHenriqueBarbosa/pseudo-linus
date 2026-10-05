//! Deflate da linhagem do Jean-loup Gailly, a do gzip 1.13 e a do zip 3.0. Os dois programas têm
//! a mesma codificação de blocos (`trees`), mas o casamento de cadeias difere em detalhes que mudam
//! a saída (o gzip insere toda posição na tabela de hash e zera os dois bytes depois do fim da
//! entrada; o zip não), então cada um tem o seu: o do zip em `zip_cli::zip::deflate`, puxando a
//! entrada, e o do gzip em [`gzip`], empurrado, que é como o codec recebe os dados.

pub mod gzip;
pub mod trees;

pub const MIN_MATCH: usize = 3;
pub const MAX_MATCH: usize = 258;
pub const WSIZE: usize = 0x8000;
pub const MIN_LOOKAHEAD: usize = MAX_MATCH + MIN_MATCH + 1;
pub const MAX_DIST: usize = WSIZE - MIN_LOOKAHEAD;

/// Tipo do arquivo, como o `set_file_type` detecta.
pub const UNKNOWN: u16 = 0xFFFF;
pub const BINARY: u16 = 0;
pub const ASCII: u16 = 1;

/// Método de compressão (só o zip usa `STORE` no arquivo inteiro).
pub const STORE: i32 = 0;

/// Para onde vão os bytes de cada bloco.
pub trait BlockSink {
    fn write(&mut self, data: &[u8]);
    /// `seekable()`: se o arquivo inteiro pode virar armazenado. O gzip o define como 0; no zip é o
    /// `fseekable(y)`, que despeja a saída.
    fn seekable(&mut self) -> bool;
    fn use_descriptors(&self) -> bool;
}
