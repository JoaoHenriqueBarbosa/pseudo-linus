//! Sonda do E06 (H19): uma "dependência de terceiros" qualquer que, por dentro, lê arquivo do host.
//! É o caso do jaq, do uutils ou do gix: o crate de userland só chama uma função pública, e quem toca
//! o `std::fs` é a dependência.

use std::io;
use std::path::Path;

/// Lê o arquivo inteiro do FS do host.
pub fn read_host_file(path: impl AsRef<Path>) -> io::Result<Vec<u8>> {
    std::fs::read(path)
}

/// Abre um arquivo do host e devolve o handle. Quem recebe não precisa escrever o nome do tipo.
pub fn open_host_file(path: impl AsRef<Path>) -> io::Result<std::fs::File> {
    std::fs::File::open(path)
}
