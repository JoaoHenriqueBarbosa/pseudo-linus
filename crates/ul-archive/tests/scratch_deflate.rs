//! Medição temporária: como os leitores das crates reagem às fixtures de erro.

use std::io::Read;

#[test]
#[ignore]
fn readers_on_fixtures() {
    let dir = std::env::var("FX").expect("FX");
    for name in ["a.txt.xz", "multi.xz", "crc32.xz", "nocheck.xz", "sha.xz", "trunc.xz", "trail.xz", "pad.xz", "bad.xz", "a.txt.lz", "multi.lz", "trunc.lz", "trail.lz", "crc.lz", "a.lzma", "a.txt.zst", "multi.zst", "nocheck.zst", "trunc.zst", "trail.zst"] {
        let data = std::fs::read(format!("{dir}/{name}")).unwrap();
        let mut out = Vec::new();
        let r = if name.ends_with(".xz") {
            lzma_rust2::XzReader::new(&data[..], true).read_to_end(&mut out)
        } else if name.ends_with(".lz") {
            lzma_rust2::LzipReader::new(&data[..]).read_to_end(&mut out)
        } else if name.ends_with(".lzma") {
            lzma_rust2::LzmaReader::new_mem_limit(&data[..], u32::MAX, None).unwrap().read_to_end(&mut out)
        } else {
            let mut rest = &data[..];
            let mut res = Ok(0);
            while !rest.is_empty() {
                match structured_zstd::decoding::StreamingDecoder::new(&mut rest) {
                    Ok(mut d) => {
                        if let Err(e) = d.read_to_end(&mut out) {
                            res = Err(e);
                            break;
                        }
                    }
                    Err(e) => {
                        res = Err(std::io::Error::other(format!("frame: {e} / {e:?}")));
                        break;
                    }
                }
            }
            res
        };
        match r {
            Ok(_) => eprintln!("{name}: ok {} bytes", out.len()),
            Err(e) => eprintln!("{name}: erro {:?} {e} ({} bytes antes)", e.kind(), out.len()),
        }
    }
}
