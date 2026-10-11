//! O sabor zlib-ng 2.2.4 (`Deflate::new_zlib_ng`) contra o bun 1.4.2, que traz o zlib-ng no modo de
//! compatibilidade: `zlib.deflateRawSync(entrada, { level: 6 })` sobre 100000 `a` seguidos de 100000 `b` dá
//! 214 bytes (o zlib 1.3.1 dá 213), medido com `~/.bun/bin/bun`.

use zdeflate::{Deflate, Flush, Status, Strategy};

const BUN_RAW_LEVEL_6: &str = "edc1210100000002a0adfa7f84370c4002000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000bc2a000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000d05703";

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[test]
fn medium_level_6_matches_bun() {
    let mut input = vec![b'a'; 100_000];
    input.extend(std::iter::repeat_n(b'b', 100_000));
    let mut deflate = Deflate::new_zlib_ng(6, -15, 8, Strategy::Default).expect("parâmetros válidos");
    let mut out = vec![0u8; 4096];
    let progress = deflate.deflate(&input, &mut out, Flush::Finish);
    assert_eq!(progress.status, Ok(Status::StreamEnd));
    assert_eq!(hex(&out[..progress.produced]), BUN_RAW_LEVEL_6);
}
