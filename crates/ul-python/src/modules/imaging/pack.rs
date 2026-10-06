//! Empacotadores e desempacotadores de "rawmode" da libImaging (`Pack.c` e `Unpack.c` do Pillow
//! 11.1.0): convertem uma linha entre o formato de arquivo (bytes corridos, bits empacotados) e o
//! armazenamento interno (1 ou 4 bytes por pixel). Cobrem os rawmodes que o PNG, o BMP, o GIF, o
//! PPM, o `tobytes`/`frombytes` e o `getdata`/`putdata` usam.
//!
//! Portado da libImaging (MIT-CMU: Copyright © 1997-2006 Secret Labs AB, © 1996-1997 Fredrik Lundh).

/// `fn(saída, entrada, pixels)`.
pub type Shuffle = fn(&mut [u8], &[u8], usize);

/// Um conversor e quantos bits por pixel ele lê (unpack) ou escreve (pack).
#[derive(Clone, Copy)]
pub struct Codec {
    pub bits: u32,
    pub f: Shuffle,
}

/// Bytes de uma linha de `pixels` pixels a `bits` bits por pixel.
pub fn line_bytes(bits: u32, pixels: usize) -> usize {
    (pixels * bits as usize).div_ceil(8)
}

// ---- unpack ----

fn bits_msb(o: &mut [u8], i: &[u8], n: usize, width: u32, map: impl Fn(u8) -> u8) {
    let per = (8 / width) as usize;
    let mask = (1u8 << width) - 1;
    for x in 0..n {
        let byte = i[x / per];
        let shift = 8 - width * (x % per + 1) as u32;
        o[x] = map((byte >> shift) & mask);
    }
}

fn bits_lsb(o: &mut [u8], i: &[u8], n: usize, map: impl Fn(u8) -> u8) {
    for x in 0..n {
        o[x] = map((i[x / 8] >> (x % 8)) & 1);
    }
}

fn unpack1(o: &mut [u8], i: &[u8], n: usize) {
    bits_msb(o, i, n, 1, |v| if v != 0 { 255 } else { 0 });
}

fn unpack1i(o: &mut [u8], i: &[u8], n: usize) {
    bits_msb(o, i, n, 1, |v| if v != 0 { 0 } else { 255 });
}

fn unpack1r(o: &mut [u8], i: &[u8], n: usize) {
    bits_lsb(o, i, n, |v| if v != 0 { 255 } else { 0 });
}

fn unpack1ir(o: &mut [u8], i: &[u8], n: usize) {
    bits_lsb(o, i, n, |v| if v != 0 { 0 } else { 255 });
}

fn unpack18(o: &mut [u8], i: &[u8], n: usize) {
    for x in 0..n {
        o[x] = if i[x] != 0 { 255 } else { 0 };
    }
}

fn unpack_l2(o: &mut [u8], i: &[u8], n: usize) {
    bits_msb(o, i, n, 2, |v| v * 0x55);
}

fn unpack_l4(o: &mut [u8], i: &[u8], n: usize) {
    bits_msb(o, i, n, 4, |v| v * 0x11);
}

fn unpack_p1(o: &mut [u8], i: &[u8], n: usize) {
    bits_msb(o, i, n, 1, |v| v);
}

fn unpack_p2(o: &mut [u8], i: &[u8], n: usize) {
    bits_msb(o, i, n, 2, |v| v);
}

fn unpack_p4(o: &mut [u8], i: &[u8], n: usize) {
    bits_msb(o, i, n, 4, |v| v);
}

fn copy1(o: &mut [u8], i: &[u8], n: usize) {
    o[..n].copy_from_slice(&i[..n]);
}

fn copy2(o: &mut [u8], i: &[u8], n: usize) {
    o[..n * 2].copy_from_slice(&i[..n * 2]);
}

fn copy4(o: &mut [u8], i: &[u8], n: usize) {
    o[..n * 4].copy_from_slice(&i[..n * 4]);
}

/// `unpackRGBAI`: as quatro bandas invertidas (CMYK do Photoshop no JPEG Adobe).
fn copy4i(o: &mut [u8], i: &[u8], n: usize) {
    for (d, s) in o[..n * 4].iter_mut().zip(&i[..n * 4]) {
        *d = !s;
    }
}

fn unpack_li(o: &mut [u8], i: &[u8], n: usize) {
    for x in 0..n {
        o[x] = !i[x];
    }
}

fn unpack_l16b(o: &mut [u8], i: &[u8], n: usize) {
    for x in 0..n {
        o[x] = i[x * 2];
    }
}

fn unpack_l16(o: &mut [u8], i: &[u8], n: usize) {
    for x in 0..n {
        o[x] = i[x * 2 + 1];
    }
}

fn unpack_la(o: &mut [u8], i: &[u8], n: usize) {
    for x in 0..n {
        let (l, a) = (i[x * 2], i[x * 2 + 1]);
        o[x * 4..x * 4 + 4].copy_from_slice(&[l, l, l, a]);
    }
}

fn unpack_la16b(o: &mut [u8], i: &[u8], n: usize) {
    for x in 0..n {
        let (l, a) = (i[x * 4], i[x * 4 + 2]);
        o[x * 4..x * 4 + 4].copy_from_slice(&[l, l, l, a]);
    }
}

fn unpack_rgb(o: &mut [u8], i: &[u8], n: usize) {
    for x in 0..n {
        o[x * 4..x * 4 + 4].copy_from_slice(&[i[x * 3], i[x * 3 + 1], i[x * 3 + 2], 255]);
    }
}

fn unpack_bgr(o: &mut [u8], i: &[u8], n: usize) {
    for x in 0..n {
        o[x * 4..x * 4 + 4].copy_from_slice(&[i[x * 3 + 2], i[x * 3 + 1], i[x * 3], 255]);
    }
}

fn unpack_bgrx(o: &mut [u8], i: &[u8], n: usize) {
    for x in 0..n {
        o[x * 4..x * 4 + 4].copy_from_slice(&[i[x * 4 + 2], i[x * 4 + 1], i[x * 4], 255]);
    }
}

fn unpack_bgra(o: &mut [u8], i: &[u8], n: usize) {
    for x in 0..n {
        o[x * 4..x * 4 + 4].copy_from_slice(&[i[x * 4 + 2], i[x * 4 + 1], i[x * 4], i[x * 4 + 3]]);
    }
}

fn unpack_rgb16b(o: &mut [u8], i: &[u8], n: usize) {
    for x in 0..n {
        o[x * 4..x * 4 + 4].copy_from_slice(&[i[x * 6], i[x * 6 + 2], i[x * 6 + 4], 255]);
    }
}

fn unpack_rgba16b(o: &mut [u8], i: &[u8], n: usize) {
    for x in 0..n {
        o[x * 4..x * 4 + 4].copy_from_slice(&[i[x * 8], i[x * 8 + 2], i[x * 8 + 4], i[x * 8 + 6]]);
    }
}

fn unpack_i16b_i16(o: &mut [u8], i: &[u8], n: usize) {
    for x in 0..n {
        o[x * 2] = i[x * 2 + 1];
        o[x * 2 + 1] = i[x * 2];
    }
}

fn unpack_i16_to_i(o: &mut [u8], i: &[u8], n: usize) {
    for x in 0..n {
        let v = i32::from(u16::from_le_bytes([i[x * 2], i[x * 2 + 1]]));
        o[x * 4..x * 4 + 4].copy_from_slice(&v.to_le_bytes());
    }
}

fn unpack_i16b_to_i(o: &mut [u8], i: &[u8], n: usize) {
    for x in 0..n {
        let v = i32::from(u16::from_be_bytes([i[x * 2], i[x * 2 + 1]]));
        o[x * 4..x * 4 + 4].copy_from_slice(&v.to_le_bytes());
    }
}

fn unpack_i8_to_i(o: &mut [u8], i: &[u8], n: usize) {
    for x in 0..n {
        o[x * 4..x * 4 + 4].copy_from_slice(&i32::from(i[x]).to_le_bytes());
    }
}

fn unpack_i32b(o: &mut [u8], i: &[u8], n: usize) {
    for x in 0..n {
        let v = i32::from_be_bytes([i[x * 4], i[x * 4 + 1], i[x * 4 + 2], i[x * 4 + 3]]);
        o[x * 4..x * 4 + 4].copy_from_slice(&v.to_le_bytes());
    }
}

fn unpack_f32b(o: &mut [u8], i: &[u8], n: usize) {
    for x in 0..n {
        o[x * 4..x * 4 + 4].copy_from_slice(&[i[x * 4 + 3], i[x * 4 + 2], i[x * 4 + 1], i[x * 4]]);
    }
}

fn band(o: &mut [u8], i: &[u8], n: usize, k: usize) {
    for x in 0..n {
        o[x * 4 + k] = i[x];
    }
}

fn band0(o: &mut [u8], i: &[u8], n: usize) {
    band(o, i, n, 0);
}

fn band1(o: &mut [u8], i: &[u8], n: usize) {
    band(o, i, n, 1);
}

fn band2(o: &mut [u8], i: &[u8], n: usize) {
    band(o, i, n, 2);
}

fn band3(o: &mut [u8], i: &[u8], n: usize) {
    band(o, i, n, 3);
}

fn unpack_rgba_premul(o: &mut [u8], i: &[u8], n: usize) {
    for x in 0..n {
        let a = u32::from(i[x * 4 + 3]);
        let mut px = [0u8; 4];
        for k in 0..3 {
            let v = u32::from(i[x * 4 + k]);
            px[k] = if a == 0 { 0 } else if a == 255 { v as u8 } else { ((v * 255) / a).min(255) as u8 };
        }
        px[3] = a as u8;
        o[x * 4..x * 4 + 4].copy_from_slice(&px);
    }
}

/// `ImagingFindUnpacker`.
pub fn unpacker(mode: &str, rawmode: &str) -> Option<Codec> {
    let c = |bits: u32, f: Shuffle| Some(Codec { bits, f });
    match (mode, rawmode) {
        ("1", "1") => c(1, unpack1),
        ("1", "1;I") => c(1, unpack1i),
        ("1", "1;R") => c(1, unpack1r),
        ("1", "1;IR") => c(1, unpack1ir),
        ("1", "1;8") => c(8, unpack18),
        ("L", "L;2") => c(2, unpack_l2),
        ("L", "L;4") => c(4, unpack_l4),
        ("L" | "P", "L") | ("P", "P") => c(8, copy1),
        ("L", "L;I") => c(8, unpack_li),
        ("L", "L;16") => c(16, unpack_l16),
        ("L", "L;16B") | ("P", "PX") => c(16, unpack_l16b),
        ("LA" | "PA", "LA") | ("PA", "PA") => c(16, unpack_la),
        ("P", "P;1") => c(1, unpack_p1),
        ("P", "P;2") => c(2, unpack_p2),
        ("P", "P;4") => c(4, unpack_p4),
        ("RGB" | "RGBX", "RGB") | ("YCbCr", "YCbCr") => c(24, unpack_rgb),
        ("CMYK", "CMYK;I") => c(32, copy4i),
        ("RGB" | "RGBX", "BGR") => c(24, unpack_bgr),
        ("RGB" | "RGBX", "RGB;16B") => c(48, unpack_rgb16b),
        ("RGB" | "RGBX", "RGBX") | ("RGBA", "RGBA") | ("CMYK", "CMYK") | ("I" | "F", "I" | "F") => c(32, copy4),
        ("RGB" | "RGBX", "BGRX") => c(32, unpack_bgrx),
        ("RGB" | "RGBX", "RGBX;16B") | ("RGBA", "RGBA;16B") => c(64, unpack_rgba16b),
        ("RGBA", "LA") => c(16, unpack_la),
        ("RGBA", "LA;16B") => c(32, unpack_la16b),
        ("RGBA", "RGBa") => c(32, unpack_rgba_premul),
        ("RGBA", "BGRA") => c(32, unpack_bgra),
        ("RGB" | "RGBA" | "RGBX" | "CMYK", "R" | "C") => c(8, band0),
        ("RGB" | "RGBA" | "RGBX" | "CMYK", "G" | "M") => c(8, band1),
        ("RGB" | "RGBA" | "RGBX" | "CMYK", "B" | "Y") => c(8, band2),
        ("RGBA" | "RGBX" | "CMYK", "A" | "X" | "K") => c(8, band3),
        ("I;16" | "I;16B" | "I;16L" | "I;16N", r) if r == mode => c(16, copy2),
        ("I;16", "I;16B") => c(16, unpack_i16b_i16),
        ("I", "I;8") => c(8, unpack_i8_to_i),
        ("I", "I;16") => c(16, unpack_i16_to_i),
        ("I", "I;16B") => c(16, unpack_i16b_to_i),
        ("I", "I;32B") => c(32, unpack_i32b),
        ("F", "F;32BF") => c(32, unpack_f32b),
        _ => None,
    }
}

// ---- pack ----

fn pack_bits(o: &mut [u8], i: &[u8], n: usize, width: u32, map: impl Fn(u8) -> u8) {
    let per = (8 / width) as usize;
    let nbytes = n.div_ceil(per);
    o[..nbytes].fill(0);
    for x in 0..n {
        let shift = 8 - width * (x % per + 1) as u32;
        o[x / per] |= map(i[x]) << shift;
    }
}

fn pack1(o: &mut [u8], i: &[u8], n: usize) {
    pack_bits(o, i, n, 1, |v| u8::from(v != 0));
}

fn pack1i(o: &mut [u8], i: &[u8], n: usize) {
    pack_bits(o, i, n, 1, |v| u8::from(v == 0));
}

fn pack1l(o: &mut [u8], i: &[u8], n: usize) {
    for x in 0..n {
        o[x] = if i[x] != 0 { 255 } else { 0 };
    }
}

fn pack_p2(o: &mut [u8], i: &[u8], n: usize) {
    pack_bits(o, i, n, 2, |v| v & 3);
}

fn pack_p4(o: &mut [u8], i: &[u8], n: usize) {
    pack_bits(o, i, n, 4, |v| v & 15);
}

fn pack_la(o: &mut [u8], i: &[u8], n: usize) {
    for x in 0..n {
        o[x * 2] = i[x * 4];
        o[x * 2 + 1] = i[x * 4 + 3];
    }
}

fn pack_rgb(o: &mut [u8], i: &[u8], n: usize) {
    for x in 0..n {
        o[x * 3..x * 3 + 3].copy_from_slice(&i[x * 4..x * 4 + 3]);
    }
}

fn pack_bgr(o: &mut [u8], i: &[u8], n: usize) {
    for x in 0..n {
        o[x * 3..x * 3 + 3].copy_from_slice(&[i[x * 4 + 2], i[x * 4 + 1], i[x * 4]]);
    }
}

fn pack_bgrx(o: &mut [u8], i: &[u8], n: usize) {
    for x in 0..n {
        o[x * 4..x * 4 + 4].copy_from_slice(&[i[x * 4 + 2], i[x * 4 + 1], i[x * 4], 0]);
    }
}

fn pack_bgra(o: &mut [u8], i: &[u8], n: usize) {
    for x in 0..n {
        o[x * 4..x * 4 + 4].copy_from_slice(&[i[x * 4 + 2], i[x * 4 + 1], i[x * 4], i[x * 4 + 3]]);
    }
}

fn pack_band(o: &mut [u8], i: &[u8], n: usize, k: usize) {
    for x in 0..n {
        o[x] = i[x * 4 + k];
    }
}

fn pband0(o: &mut [u8], i: &[u8], n: usize) {
    pack_band(o, i, n, 0);
}

fn pband1(o: &mut [u8], i: &[u8], n: usize) {
    pack_band(o, i, n, 1);
}

fn pband2(o: &mut [u8], i: &[u8], n: usize) {
    pack_band(o, i, n, 2);
}

fn pband3(o: &mut [u8], i: &[u8], n: usize) {
    pack_band(o, i, n, 3);
}

fn pack_l16b(o: &mut [u8], i: &[u8], n: usize) {
    for x in 0..n {
        o[x * 2] = i[x];
        o[x * 2 + 1] = 0;
    }
}

fn pack_l16(o: &mut [u8], i: &[u8], n: usize) {
    for x in 0..n {
        o[x * 2] = 0;
        o[x * 2 + 1] = i[x];
    }
}

fn pack_i16_swap(o: &mut [u8], i: &[u8], n: usize) {
    for x in 0..n {
        o[x * 2] = i[x * 2 + 1];
        o[x * 2 + 1] = i[x * 2];
    }
}

fn pack_i_i16b(o: &mut [u8], i: &[u8], n: usize) {
    for x in 0..n {
        let v = i32::from_le_bytes([i[x * 4], i[x * 4 + 1], i[x * 4 + 2], i[x * 4 + 3]]).clamp(0, 65535) as u16;
        o[x * 2..x * 2 + 2].copy_from_slice(&v.to_be_bytes());
    }
}

/// `ImagingFindPacker`.
pub fn packer(mode: &str, rawmode: &str) -> Option<Codec> {
    let c = |bits: u32, f: Shuffle| Some(Codec { bits, f });
    match (mode, rawmode) {
        ("1" | "P", "1") | ("P", "P;1") => c(1, pack1),
        ("1", "1;I") => c(1, pack1i),
        ("1", "L") => c(8, pack1l),
        ("L", "L") | ("P", "P") => c(8, copy1),
        ("L", "L;16") => c(16, pack_l16),
        ("L", "L;16B") => c(16, pack_l16b),
        ("LA", "LA") | ("PA", "PA") => c(16, pack_la),
        ("P", "P;2") => c(2, pack_p2),
        ("P", "P;4") => c(4, pack_p4),
        ("RGB" | "RGBA" | "RGBX", "RGB") | ("YCbCr", "YCbCr") => c(24, pack_rgb),
        ("CMYK", "CMYK;I") => c(32, copy4i),
        ("RGB", "RGBX" | "RGBA") | ("RGBA", "RGBA") | ("RGBX", "RGBX") | ("CMYK", "CMYK") | ("I", "I") | ("F", "F") => {
            c(32, copy4)
        }
        ("RGB" | "RGBA" | "RGBX", "BGR") => c(24, pack_bgr),
        ("RGB" | "RGBX", "BGRX") => c(32, pack_bgrx),
        ("RGBA", "BGRA") => c(32, pack_bgra),
        ("RGB" | "RGBA" | "RGBX" | "CMYK", "R" | "C") => c(8, pband0),
        ("RGB" | "RGBA" | "RGBX" | "CMYK", "G" | "M") => c(8, pband1),
        ("RGB" | "RGBA" | "RGBX" | "CMYK", "B" | "Y") => c(8, pband2),
        ("RGBA" | "RGBX" | "CMYK", "A" | "X" | "K") => c(8, pband3),
        ("I;16" | "I;16B", r) if r == mode => c(16, copy2),
        ("I;16", "I;16B") | ("I;16B", "I;16") => c(16, pack_i16_swap),
        ("I", "I;16B") => c(16, pack_i_i16b),
        _ => None,
    }
}
