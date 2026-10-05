// Sonda: o encoder do structured-zstd reproduz os bytes do zstd 1.5.7?
use std::io::Write;

use structured_zstd::encoding::{CompressionLevel, StreamingEncoder};

fn enc(data: &[u8], level: i32) -> Vec<u8> {
    let mut e = StreamingEncoder::new(Vec::new(), CompressionLevel::from_level(level));
    e.set_content_checksum(true).unwrap();
    e.set_pledged_content_size(data.len() as u64).unwrap();
    e.write_all(data).unwrap();
    e.finish().unwrap()
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let data = std::fs::read(&args[1]).unwrap();
    let level: i32 = args.get(2).map(|s| s.parse().unwrap()).unwrap_or(3);
    std::io::stdout().write_all(&enc(&data, level)).unwrap();
}
