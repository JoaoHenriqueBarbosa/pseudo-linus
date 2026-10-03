//! Um `std::fs::File` do host que chega por uma dependência e é lido aqui pelo trait `Read`, sem
//! escrever o nome do tipo nem chamar função de `std::fs`. O I/O acontece neste crate.

use std::io::Read;

pub fn read_len(path: &str) -> usize {
    let mut buf = Vec::new();
    match fs_reader::open_host_file(path) {
        Ok(mut handle) => handle.read_to_end(&mut buf).unwrap_or(0),
        Err(_) => 0,
    }
}
