//! O codificador VP8L do libwebp 1.5.0 (`enc/vp8l_enc.c`) nos dois usos do Skia: a imagem sem perdas do
//! `toDataURL('image/webp', 1)` (método 0, qualidade 75, transparência trocada por zero) e o plano de alfa do chunk
//! `ALPH` (método 3, qualidade 24, pixels exatos).

mod backward_refs;
mod dsp;
mod histogram;
mod huffman;
mod predictor;
mod tables;

use super::bitwriter::Vp8lBitWriter;
use backward_refs::{get_backward_references, HashChain, PixOrCopy, LZ77_BOX, LZ77_RLE, LZ77_STANDARD, MAX_COLOR_CACHE_BITS};
use dsp::{bits_log2_floor, bundle_color_map, fast_log2, prefix_encode, sub_pixels, sub_sample_size, subtract_green, LOG_2_PRECISION_BITS};
use histogram::{bits_entropy, histo_image_symbols, Histogram, NUM_LENGTH_CODES, NUM_LITERAL_CODES};
use huffman::{compressed_tree, create_tree, HuffmanCode, Token};

const MAX_HUFF_IMAGE_SIZE: usize = 2600;
const MIN_HUFFMAN_BITS: u32 = 2;
const MAX_HUFFMAN_BITS: u32 = 9;
const MIN_TRANSFORM_BITS: u32 = 2;
const MAX_TRANSFORM_BITS: u32 = 9;
const MAX_PREDICTOR_IMAGE_SIZE: usize = 1 << 14;
const MAX_PALETTE_SIZE: usize = 256;
const CODE_LENGTH_CODES: usize = 19;
const PREDICTOR_TRANSFORM: u32 = 0;
const SUBTRACT_GREEN_TRANSFORM: u32 = 2;
const COLOR_INDEXING_TRANSFORM: u32 = 3;

/// `EntropyIx`.
#[derive(Clone, Copy, PartialEq, Eq)]
enum EntropyIx {
    Direct,
    Spatial,
    SubGreen,
    SpatialSubGreen,
    Palette,
}

/// `ClampBits`.
fn clamp_bits(width: usize, height: usize, bits: u32, min_bits: u32, max_bits: u32, image_size_max: usize) -> u32 {
    let mut bits = bits.clamp(min_bits, max_bits);
    let size = |b: u32| sub_sample_size(width, b) * sub_sample_size(height, b);
    let mut image_size = size(bits);
    while bits < max_bits && image_size > image_size_max {
        bits += 1;
        image_size = size(bits);
    }
    while bits > min_bits && image_size == 1 {
        image_size = size(bits - 1);
        if image_size != 1 {
            break;
        }
        bits -= 1;
    }
    bits
}

/// `GetColorPalette`: as cores distintas em ordem crescente, ou `None` acima de 256.
fn color_palette(argb: &[u32]) -> Option<Vec<u32>> {
    let mut colors: Vec<u32> = Vec::new();
    let mut last = !argb[0];
    for &p in argb {
        if p == last {
            continue;
        }
        last = p;
        if let Err(pos) = colors.binary_search(&p) {
            colors.insert(pos, p);
            if colors.len() > MAX_PALETTE_SIZE {
                return None;
            }
        }
    }
    Some(colors)
}

fn palette_component_distance(v: u32) -> u32 {
    if v <= 128 {
        v
    } else {
        256 - v
    }
}

fn palette_color_distance(a: u32, b: u32) -> u32 {
    let diff = sub_pixels(a, b);
    let rgb = palette_component_distance(diff & 0xff) + palette_component_distance((diff >> 8) & 0xff) + palette_component_distance((diff >> 16) & 0xff);
    rgb * 9 + palette_component_distance((diff >> 24) & 0xff)
}

/// `PaletteHasNonMonotonousDeltas`.
fn has_non_monotonous_deltas(palette: &[u32]) -> bool {
    let mut predict = 0u32;
    let mut sign_found = 0u8;
    for &c in palette {
        let diff = sub_pixels(c, predict);
        for (shift, low, high) in [(16, 1, 2), (8, 8, 16), (0, 64, 128)] {
            let d = (diff >> shift) & 0xff;
            if d != 0 {
                sign_found |= if d < 0x80 { low } else { high };
            }
        }
        predict = c;
    }
    sign_found & (sign_found << 1) != 0
}

/// `PaletteSort` com `kSortedDefault` (`minimize_delta` falso) ou `kMinimizeDelta`.
fn palette_sort(sorted: &[u32], minimize_delta: bool) -> Vec<u32> {
    let mut palette = sorted.to_vec();
    let mut n = palette.len();
    if !minimize_delta {
        if sorted[0] == 0 && n > 17 {
            palette.rotate_left(1);
        }
        return palette;
    }
    if !has_non_monotonous_deltas(sorted) {
        return palette;
    }
    if n > 17 && palette[0] == 0 {
        n -= 1;
        palette.swap(n, 0);
    }
    let mut predict = 0u32;
    for i in 0..n {
        let mut best = (i, u32::MAX);
        for k in i..n {
            let score = palette_color_distance(palette[k], predict);
            if best.1 > score {
                best = (k, score);
            }
        }
        palette.swap(best.0, i);
        predict = palette[i];
    }
    palette
}

/// `AnalyzeEntropy`: o modo de menor entropia e se o vermelho e o azul são sempre zero.
fn analyze_entropy(argb: &[u32], width: usize, height: usize, palette_size: Option<usize>, transform_bits: u32) -> (EntropyIx, bool) {
    if palette_size.is_some_and(|n| n <= 16) {
        return (EntropyIx::Palette, true);
    }
    const ALPHA: usize = 0;
    const ALPHA_PRED: usize = 1;
    const GREEN: usize = 2;
    const GREEN_PRED: usize = 3;
    const RED: usize = 4;
    const RED_PRED: usize = 5;
    const BLUE: usize = 6;
    const BLUE_PRED: usize = 7;
    const RED_SUB_GREEN: usize = 8;
    const RED_PRED_SUB_GREEN: usize = 9;
    const BLUE_SUB_GREEN: usize = 10;
    const BLUE_PRED_SUB_GREEN: usize = 11;
    const PALETTE: usize = 12;
    let mut histo = vec![[0u32; 256]; 13];
    let add = |h: &mut Vec<[u32; 256]>, p: u32, a: usize, r: usize, g: usize, b: usize| {
        h[a][(p >> 24) as usize] += 1;
        h[r][((p >> 16) & 0xff) as usize] += 1;
        h[g][((p >> 8) & 0xff) as usize] += 1;
        h[b][(p & 0xff) as usize] += 1;
    };
    let add_sub_green = |h: &mut Vec<[u32; 256]>, p: u32, r: usize, b: usize| {
        let green = (p as i32) >> 8;
        h[r][((((p as i32) >> 16) - green) & 0xff) as usize] += 1;
        h[b][((p as i32 - green) & 0xff) as usize] += 1;
    };
    let mut pix_prev = argb[0];
    for y in 0..height {
        for x in 0..width {
            let pix = argb[y * width + x];
            let pix_diff = sub_pixels(pix, pix_prev);
            pix_prev = pix;
            if pix_diff == 0 || (y > 0 && pix == argb[(y - 1) * width + x]) {
                continue;
            }
            add(&mut histo, pix, ALPHA, RED, GREEN, BLUE);
            add(&mut histo, pix_diff, ALPHA_PRED, RED_PRED, GREEN_PRED, BLUE_PRED);
            add_sub_green(&mut histo, pix, RED_SUB_GREEN, BLUE_SUB_GREEN);
            add_sub_green(&mut histo, pix_diff, RED_PRED_SUB_GREEN, BLUE_PRED_SUB_GREEN);
            let hash = ((u64::from(pix) + u64::from(pix >> 19)) * 0x39c5fba7 & 0xffff_ffff) >> 24;
            histo[PALETTE][hash as usize] += 1;
        }
    }
    for k in [RED_PRED_SUB_GREEN, BLUE_PRED_SUB_GREEN, RED_PRED, GREEN_PRED, BLUE_PRED, ALPHA_PRED] {
        histo[k][0] += 1;
    }
    let e: Vec<u64> = histo.iter().map(|h| bits_entropy(h)).collect();
    let tiles = (sub_sample_size(width, transform_bits) * sub_sample_size(height, transform_bits)) as u64;
    let entropy = [
        e[ALPHA] + e[RED] + e[GREEN] + e[BLUE],
        e[ALPHA_PRED] + e[RED_PRED] + e[GREEN_PRED] + e[BLUE_PRED] + tiles * u64::from(fast_log2(14)),
        e[ALPHA] + e[RED_SUB_GREEN] + e[GREEN] + e[BLUE_SUB_GREEN],
        e[ALPHA_PRED] + e[RED_PRED_SUB_GREEN] + e[GREEN_PRED] + e[BLUE_PRED_SUB_GREEN] + tiles * u64::from(fast_log2(24)),
        e[PALETTE] + ((palette_size.unwrap_or(0) as u64 * 8) << LOG_2_PRECISION_BITS),
    ];
    let modes = [EntropyIx::Direct, EntropyIx::Spatial, EntropyIx::SubGreen, EntropyIx::SpatialSubGreen, EntropyIx::Palette];
    let last = if palette_size.is_some() { 4 } else { 3 };
    let mut min = 0;
    for k in 1..=last {
        if entropy[min] > entropy[k] {
            min = k;
        }
    }
    let pairs = [(RED, BLUE), (RED_PRED, BLUE_PRED), (RED_SUB_GREEN, BLUE_SUB_GREEN), (RED_PRED_SUB_GREEN, BLUE_PRED_SUB_GREEN), (RED, BLUE)];
    let (r, b) = pairs[min];
    let zero = (1..256).all(|i| histo[r][i] | histo[b][i] == 0);
    (modes[min], zero)
}

/// `StoreHuffmanTreeOfHuffmanTreeToBitMask`.
fn store_code_length_code(bw: &mut Vp8lBitWriter, depths: &[u8]) {
    const ORDER: [usize; CODE_LENGTH_CODES] = [17, 18, 0, 1, 2, 3, 4, 5, 16, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15];
    let mut codes_to_store = CODE_LENGTH_CODES;
    while codes_to_store > 4 && depths[ORDER[codes_to_store - 1]] == 0 {
        codes_to_store -= 1;
    }
    bw.put_bits((codes_to_store - 4) as u32, 4);
    for &o in &ORDER[..codes_to_store] {
        bw.put_bits(u32::from(depths[o]), 3);
    }
}

/// `ClearHuffmanTreeIfOnlyOneSymbol`.
fn clear_if_only_one_symbol(code: &mut HuffmanCode) {
    if code.code_lengths.iter().filter(|&&l| l != 0).count() > 1 {
        return;
    }
    code.code_lengths.fill(0);
    code.codes.fill(0);
}

/// `StoreFullHuffmanCode`.
fn store_full_huffman_code(bw: &mut Vp8lBitWriter, tree: &HuffmanCode) {
    bw.put_bits(0, 1);
    let tokens: Vec<Token> = compressed_tree(tree);
    let mut histogram = [0u32; CODE_LENGTH_CODES];
    for t in &tokens {
        histogram[t.code as usize] += 1;
    }
    let mut code = create_tree(&mut histogram, 7);
    store_code_length_code(bw, &code.code_lengths);
    clear_if_only_one_symbol(&mut code);
    let (mut trailing_zero_bits, mut trimmed_length) = (0u32, tokens.len());
    for t in tokens.iter().rev() {
        let ix = t.code as usize;
        if ix == 0 || ix == 17 || ix == 18 {
            trimmed_length -= 1;
            trailing_zero_bits += u32::from(code.code_lengths[ix]);
            trailing_zero_bits += match ix {
                17 => 3,
                18 => 7,
                _ => 0,
            };
        } else {
            break;
        }
    }
    let write_trimmed = trimmed_length > 1 && trailing_zero_bits > 12;
    let length = if write_trimmed { trimmed_length } else { tokens.len() };
    bw.put_bits(u32::from(write_trimmed), 1);
    if write_trimmed {
        if trimmed_length == 2 {
            bw.put_bits(0, 3 + 2);
        } else {
            let nbits = bits_log2_floor((trimmed_length - 2) as u32);
            let nbitpairs = nbits / 2 + 1;
            bw.put_bits(nbitpairs - 1, 3);
            bw.put_bits((trimmed_length - 2) as u32, nbitpairs * 2);
        }
    }
    for t in &tokens[..length] {
        let ix = t.code as usize;
        bw.put_bits(u32::from(code.codes[ix]), u32::from(code.code_lengths[ix]));
        match ix {
            16 => bw.put_bits(u32::from(t.extra_bits), 2),
            17 => bw.put_bits(u32::from(t.extra_bits), 3),
            18 => bw.put_bits(u32::from(t.extra_bits), 7),
            _ => {}
        }
    }
}

/// `StoreHuffmanCode`.
fn store_huffman_code(bw: &mut Vp8lBitWriter, code: &HuffmanCode) {
    let used: Vec<usize> = code.code_lengths.iter().enumerate().filter(|&(_, &l)| l != 0).map(|(i, _)| i).take(3).collect();
    let count = used.len();
    let symbols = [used.first().copied().unwrap_or(0), used.get(1).copied().unwrap_or(0)];
    if count == 0 {
        bw.put_bits(0x01, 4);
    } else if count <= 2 && symbols[0] < 256 && symbols[1] < 256 {
        bw.put_bits(1, 1);
        bw.put_bits((count - 1) as u32, 1);
        if symbols[0] <= 1 {
            bw.put_bits(0, 1);
            bw.put_bits(symbols[0] as u32, 1);
        } else {
            bw.put_bits(1, 1);
            bw.put_bits(symbols[0] as u32, 8);
        }
        if count == 2 {
            bw.put_bits(symbols[1] as u32, 8);
        }
    } else {
        store_full_huffman_code(bw, code);
    }
}

/// `GetHuffBitLengthsAndCodes` de um histograma: literal, vermelho, azul, alfa e distância.
fn huffman_codes(h: &mut Histogram) -> [HuffmanCode; 5] {
    [create_tree(&mut h.literal, 15), create_tree(&mut h.red, 15), create_tree(&mut h.blue, 15), create_tree(&mut h.alpha, 15), create_tree(&mut h.distance, 15)]
}

fn write_code(bw: &mut Vp8lBitWriter, code: &HuffmanCode, index: usize) {
    bw.put_bits(u32::from(code.codes[index]), u32::from(code.code_lengths[index]));
}

/// `StoreImageToBitMask`.
fn store_image(bw: &mut Vp8lBitWriter, width: usize, histo_bits: u32, refs: &[PixOrCopy], symbols: &[u32], codes: &[[HuffmanCode; 5]]) {
    let histo_xsize = if histo_bits > 0 { sub_sample_size(width, histo_bits) } else { 1 };
    let tile_mask: usize = if histo_bits == 0 { 0 } else { !((1usize << histo_bits) - 1) };
    let (mut x, mut y) = (0usize, 0usize);
    let (mut tile_x, mut tile_y) = (0usize, 0usize);
    let mut set = &codes[((symbols[0] >> 8) & 0xffff) as usize];
    for v in refs {
        if tile_x != x & tile_mask || tile_y != y & tile_mask {
            tile_x = x & tile_mask;
            tile_y = y & tile_mask;
            set = &codes[((symbols[(y >> histo_bits) * histo_xsize + (x >> histo_bits)] >> 8) & 0xffff) as usize];
        }
        match *v {
            PixOrCopy::Literal(argb) => {
                for (k, component) in [1, 2, 0, 3].into_iter().enumerate() {
                    write_code(bw, &set[k], ((argb >> (component * 8)) & 0xff) as usize);
                }
            }
            PixOrCopy::CacheIdx(idx) => write_code(bw, &set[0], NUM_LITERAL_CODES + NUM_LENGTH_CODES + idx as usize),
            PixOrCopy::Copy { len, distance } => {
                let (code, n_bits, bits) = prefix_encode(len);
                let c = &set[0];
                let ix = NUM_LITERAL_CODES + code as usize;
                let depth = u32::from(c.code_lengths[ix]);
                bw.put_bits((bits << depth) | u32::from(c.codes[ix]), depth + n_bits);
                let (code, n_bits, bits) = prefix_encode(distance);
                write_code(bw, &set[4], code as usize);
                bw.put_bits(bits, n_bits);
            }
        }
        x += v.len();
        while x >= width {
            x -= width;
            y += 1;
        }
    }
}

/// `EncodeImageNoHuffman`: imagem auxiliar (modos, paleta, histogramas) com um único código e sem cache.
fn encode_image_no_huffman(bw: &mut Vp8lBitWriter, argb: &[u32], width: usize, height: usize, quality: i32, low_effort: bool) {
    let chain = HashChain::fill(quality, argb, width, height, low_effort);
    let (refs, _) = get_backward_references(width, height, argb, quality, false, LZ77_STANDARD | LZ77_RLE, 0, &chain);
    let mut histo = Histogram::new(0);
    histo.store_refs(&refs);
    let mut codes = huffman_codes(&mut histo);
    bw.put_bits(0, 1);
    for code in &mut codes {
        store_huffman_code(bw, code);
        clear_if_only_one_symbol(code);
    }
    store_image(bw, width, 0, &refs, &[0], std::slice::from_ref(&codes));
}

/// `EncodeImageInternal` sem a variante sem cache: o melhor dos tipos de LZ77 pedidos.
fn encode_image_internal(bw: &mut Vp8lBitWriter, argb: &[u32], width: usize, height: usize, quality: i32, low_effort: bool, lz77_configs: &[u32], cache_bits: u32, histogram_bits_in: u32) {
    let chain = HashChain::fill(quality, argb, width, height, low_effort);
    let cache_bits_init = if cache_bits == 0 { MAX_COLOR_CACHE_BITS } else { cache_bits };
    let bw_init = bw.clone();
    let mut best: Option<Vp8lBitWriter> = None;
    for &lz77 in lz77_configs {
        let (refs, cache_bits) = get_backward_references(width, height, argb, quality, low_effort, lz77, cache_bits_init, &chain);
        *bw = bw_init.clone();
        let (mut histos, symbols) = histo_image_symbols(width, height, &refs, quality, low_effort, histogram_bits_in, cache_bits);
        let mut codes: Vec<[HuffmanCode; 5]> = histos.iter_mut().map(huffman_codes).collect();
        if cache_bits > 0 {
            bw.put_bits(1, 1);
            bw.put_bits(cache_bits, 4);
        } else {
            bw.put_bits(0, 1);
        }
        let image_size = symbols.iter().map(|&s| s as usize + 1).max().unwrap_or(0);
        let mut histogram_argb: Vec<u32> = symbols.iter().map(|&s| s << 8).collect();
        let mut histogram_bits = histogram_bits_in;
        let write_image = image_size > 1;
        bw.put_bits(u32::from(write_image), 1);
        if write_image {
            histogram_bits = predictor::optimize_sampling(&mut histogram_argb, width, height, histogram_bits_in, MAX_HUFFMAN_BITS);
            bw.put_bits(histogram_bits - 2, 3);
            let (w, h) = (sub_sample_size(width, histogram_bits), sub_sample_size(height, histogram_bits));
            encode_image_no_huffman(bw, &histogram_argb[..w * h], w, h, quality, low_effort);
        }
        for set in codes.iter_mut().take(image_size) {
            for code in set.iter_mut() {
                store_huffman_code(bw, code);
                clear_if_only_one_symbol(code);
            }
        }
        store_image(bw, width, histogram_bits, &refs, &histogram_argb, &codes);
        if best.as_ref().is_none_or(|b| bw.num_bytes() < b.num_bytes()) {
            best = Some(bw.clone());
        }
    }
    if let Some(b) = best {
        *bw = b;
    }
}

/// `MapImageFromPalette`: os índices empacotados (`VP8LBundleColorMap`) e a largura resultante.
fn map_image_from_palette(argb: &[u32], width: usize, height: usize, palette: &[u32]) -> (Vec<u32>, usize) {
    let n = palette.len();
    let xbits = if n <= 4 {
        if n <= 2 {
            3
        } else {
            2
        }
    } else if n <= 16 {
        1
    } else {
        0
    };
    let packed_width = sub_sample_size(width, xbits);
    let mut out = vec![0u32; packed_width * height];
    let mut row = vec![0u8; width];
    let (mut prev_pix, mut prev_idx) = (palette[0], 0u8);
    for y in 0..height {
        for x in 0..width {
            let pix = argb[y * width + x];
            if pix != prev_pix {
                prev_idx = palette.iter().position(|&c| c == pix).unwrap_or(0) as u8;
                prev_pix = pix;
            }
            row[x] = prev_idx;
        }
        bundle_color_map(&row, xbits, &mut out[y * packed_width..(y + 1) * packed_width]);
    }
    (out, packed_width)
}

/// `EncodePalette`.
fn encode_palette(bw: &mut Vp8lBitWriter, palette: &[u32], low_effort: bool) {
    let n = palette.len();
    let encoded = if palette[n - 1] == 0 && n > 17 { n - 1 } else { n };
    bw.put_bits(1, 1);
    bw.put_bits(COLOR_INDEXING_TRANSFORM, 2);
    bw.put_bits((encoded - 1) as u32, 8);
    let mut tmp = vec![0u32; encoded];
    tmp[0] = palette[0];
    for i in 1..encoded {
        tmp[i] = sub_pixels(palette[i], palette[i - 1]);
    }
    encode_image_no_huffman(bw, &tmp, encoded, 1, 20, low_effort);
}

/// `VP8LEncodeStream` com um único `CrunchConfig` (o caso de todo método abaixo de 5).
pub fn encode_stream(argb: &[u32], width: usize, height: usize, quality: f32, method: i32, exact: bool) -> Vec<u8> {
    let mut bw = Vp8lBitWriter::new();
    write_stream(&mut bw, argb, width, height, quality, method, exact);
    bw.finish()
}

fn write_stream(bw: &mut Vp8lBitWriter, argb: &[u32], width: usize, height: usize, quality: f32, method: i32, exact: bool) {
    let quality = quality as i32;
    let low_effort = method == 0;
    let palette_sorted = color_palette(argb);
    let use_palette_possible = palette_sorted.is_some();
    let histo_bits = clamp_bits(width, height, (if use_palette_possible { 9 } else { 7 }) - method as u32, MIN_HUFFMAN_BITS, MAX_HUFFMAN_BITS, MAX_HUFF_IMAGE_SIZE);
    let max_transform_bits = if method < 4 {
        6
    } else if method > 4 {
        4
    } else {
        5
    };
    let transform_bits = histo_bits.min(max_transform_bits);
    // `EncoderAnalyze`.
    let palette_size = palette_sorted.as_ref().map(Vec::len);
    let (entropy_idx, n_lz77s) = if low_effort {
        (if use_palette_possible { EntropyIx::Palette } else { EntropyIx::SpatialSubGreen }, 1)
    } else {
        let n_lz77s = if palette_size.is_some_and(|n| n > 0 && n <= 16) { 2 } else { 1 };
        (analyze_entropy(argb, width, height, palette_size, transform_bits).0, n_lz77s)
    };
    let lz77_configs = [LZ77_STANDARD | LZ77_RLE, LZ77_BOX];
    let lz77_configs = &lz77_configs[..n_lz77s];

    // `EncodeStreamHook`.
    let use_palette = entropy_idx == EntropyIx::Palette;
    let use_subtract_green = matches!(entropy_idx, EntropyIx::SubGreen | EntropyIx::SpatialSubGreen);
    let use_predict = matches!(entropy_idx, EntropyIx::Spatial | EntropyIx::SpatialSubGreen);
    let mut cache_bits = 0;
    let (mut data, current_width) = match palette_sorted.as_deref().filter(|_| use_palette) {
        Some(sorted) => {
            let palette = palette_sort(sorted, !low_effort);
            encode_palette(bw, &palette, low_effort);
            if palette.len() < 1 << MAX_COLOR_CACHE_BITS {
                cache_bits = bits_log2_floor(palette.len() as u32) + 1;
            }
            map_image_from_palette(argb, width, height, &palette)
        }
        None => (argb.to_vec(), width),
    };
    if use_subtract_green {
        bw.put_bits(1, 1);
        bw.put_bits(SUBTRACT_GREEN_TRANSFORM, 2);
        subtract_green(&mut data);
    }
    if use_predict {
        // `ApplyPredictFilter`.
        let max_bits = clamp_bits(current_width, height, transform_bits, MIN_TRANSFORM_BITS, MAX_TRANSFORM_BITS, MAX_PREDICTOR_IMAGE_SIZE);
        let extra = if method > 4 { 2 * (method as u32 - 4) } else { 0 };
        let min_bits = clamp_bits(current_width, height, max_bits.saturating_sub(extra), MIN_TRANSFORM_BITS, MAX_TRANSFORM_BITS, MAX_PREDICTOR_IMAGE_SIZE);
        let (image, best_bits) = predictor::residual_image(current_width, height, min_bits, max_bits, low_effort, &mut data, exact);
        bw.put_bits(1, 1);
        bw.put_bits(PREDICTOR_TRANSFORM, 2);
        bw.put_bits(best_bits - MIN_TRANSFORM_BITS, 3);
        let (w, h) = (sub_sample_size(current_width, best_bits), sub_sample_size(height, best_bits));
        encode_image_no_huffman(bw, &image[..w * h], w, h, quality, low_effort);
    }
    bw.put_bits(0, 1);
    encode_image_internal(bw, &data, current_width, height, quality, low_effort, lz77_configs, cache_bits, histo_bits);
}

/// `VP8LEncodeImage` com a configuração do Skia (método 0, qualidade 75, sem `exact`): o conteúdo do chunk `VP8L` e
/// se a imagem tem transparência.
pub fn encode_image(rgba: &[u8], width: usize, height: usize) -> (Vec<u8>, bool) {
    // `WebPReplaceTransparentPixels(pic, 0)`.
    let argb: Vec<u32> = rgba
        .chunks_exact(4)
        .map(|p| {
            let v = u32::from(p[3]) << 24 | u32::from(p[0]) << 16 | u32::from(p[1]) << 8 | u32::from(p[2]);
            if p[3] == 0 {
                0
            } else {
                v
            }
        })
        .collect();
    let has_alpha = rgba.chunks_exact(4).any(|p| p[3] != 0xff);
    let mut bw = Vp8lBitWriter::new();
    bw.put_bits(0x2f, 8);
    bw.put_bits((width - 1) as u32, 14);
    bw.put_bits((height - 1) as u32, 14);
    bw.put_bits(u32::from(has_alpha), 1);
    bw.put_bits(0, 3);
    write_stream(&mut bw, &argb, width, height, 75.0, 0, false);
    (bw.finish(), has_alpha)
}
