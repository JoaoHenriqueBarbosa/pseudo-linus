use zdeflate::{Deflate, Flush, Strategy, DEFAULT_COMPRESSION};

fn run(chunks: &[&[u8]]) -> String {
    let mut d = Deflate::new(DEFAULT_COMPRESSION, -15, 8, Strategy::Default).unwrap();
    let mut out = Vec::new();
    for chunk in chunks {
        let mut buf = vec![0u8; 16384];
        let p = d.deflate(chunk, &mut buf, Flush::None);
        out.extend_from_slice(&buf[..p.produced]);
    }
    let mut buf = vec![0u8; 16384];
    let p = d.deflate(&[], &mut buf, Flush::Finish);
    out.extend_from_slice(&buf[..p.produced]);
    out.iter().map(|b| format!("{b:02x}")).collect()
}

#[test]
fn split_finish_matches_zlib() {
    assert_eq!(run(&[b"abcabcabc"]), "4b4c4a4e042300");
    assert_eq!(run(&[b""]), "0300");
    assert_eq!(run(&[b"a"]), "4b0400");
    let long = "abc".repeat(40);
    assert_eq!(run(&[b"a", b"", long.as_bytes()]), "4b4c4c4a1e100400");
}
