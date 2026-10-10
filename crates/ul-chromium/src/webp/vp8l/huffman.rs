//! `utils/huffman_encode_utils.c` do libwebp 1.5.0: construção dos códigos de Huffman com limite de profundidade,
//! a suavização das contagens para o RLE e a codificação comprimida dos comprimentos.

const MAX_ALLOWED_CODE_LENGTH: usize = 15;

/// `HuffmanTreeCode`.
#[derive(Clone, Default)]
pub struct HuffmanCode {
    pub code_lengths: Vec<u8>,
    pub codes: Vec<u16>,
}

/// `HuffmanTreeToken`.
#[derive(Clone, Copy)]
pub struct Token {
    pub code: u8,
    pub extra_bits: u8,
}

fn collapse_to_stride_average(a: u32, b: u32) -> bool {
    (a as i32 - b as i32).abs() < 4
}

/// `OptimizeHuffmanForRle`: suaviza as contagens para que trechos parecidos virem repetições.
fn optimize_for_rle(counts: &mut [u32]) {
    let mut length = counts.len();
    loop {
        if length == 0 {
            return;
        }
        if counts[length - 1] != 0 {
            break;
        }
        length -= 1;
    }
    let mut good_for_rle = vec![false; counts.len()];
    let mut symbol = counts[0];
    let mut stride = 0usize;
    for i in 0..=length {
        if i == length || counts[i] != symbol {
            if (symbol == 0 && stride >= 5) || (symbol != 0 && stride >= 7) {
                for k in 0..stride {
                    good_for_rle[i - k - 1] = true;
                }
            }
            stride = 1;
            if i != length {
                symbol = counts[i];
            }
        } else {
            stride += 1;
        }
    }
    let (mut stride, mut limit, mut sum) = (0u32, counts[0], 0u32);
    for i in 0..=length {
        if i == length || good_for_rle[i] || (i != 0 && good_for_rle[i - 1]) || !collapse_to_stride_average(counts[i], limit) {
            if stride >= 4 || (stride >= 3 && sum == 0) {
                let count = if sum == 0 { 0 } else { ((sum + stride / 2) / stride).max(1) };
                for k in 0..stride as usize {
                    counts[i - k - 1] = count;
                }
            }
            stride = 0;
            sum = 0;
            limit = if i + 3 < length {
                (counts[i] + counts[i + 1] + counts[i + 2] + counts[i + 3] + 2) / 4
            } else if i < length {
                counts[i]
            } else {
                0
            };
        }
        stride += 1;
        if i != length {
            sum += counts[i];
            if stride >= 4 {
                limit = (sum + stride / 2) / stride;
            }
        }
    }
}

#[derive(Clone, Copy)]
struct Node {
    total_count: u32,
    value: i32,
    left: i32,
    right: i32,
}

fn set_bit_depths(node: &Node, pool: &[Node], depths: &mut [u8], level: u8) {
    if node.left >= 0 {
        set_bit_depths(&pool[node.left as usize], pool, depths, level + 1);
        set_bit_depths(&pool[node.right as usize], pool, depths, level + 1);
    } else {
        depths[node.value as usize] = level;
    }
}

/// `GenerateOptimalTree`: Huffman clássico, dobrando a contagem mínima até caber no limite de profundidade.
fn generate_optimal_tree(histogram: &[u32], depth_limit: u8, depths: &mut [u8]) {
    let size_orig = histogram.iter().filter(|&&c| c != 0).count();
    if size_orig == 0 {
        return;
    }
    let mut count_min = 1u32;
    loop {
        let mut tree: Vec<Node> = histogram
            .iter()
            .enumerate()
            .filter(|&(_, &c)| c != 0)
            .map(|(j, &c)| Node { total_count: c.max(count_min), value: j as i32, left: -1, right: -1 })
            .collect();
        // `CompareHuffmanTrees`: contagem decrescente, valor crescente no empate.
        tree.sort_by(|a, b| b.total_count.cmp(&a.total_count).then(a.value.cmp(&b.value)));
        if tree.len() > 1 {
            let mut pool: Vec<Node> = Vec::with_capacity(2 * tree.len());
            while tree.len() > 1 {
                let n = tree.len();
                let (last, prev) = (tree[n - 1], tree[n - 2]);
                tree.truncate(n - 2);
                pool.push(last);
                pool.push(prev);
                let count = last.total_count + prev.total_count;
                let k = tree.iter().position(|n| n.total_count <= count).unwrap_or(tree.len());
                let len = pool.len() as i32;
                tree.insert(k, Node { total_count: count, value: -1, left: len - 1, right: len - 2 });
            }
            set_bit_depths(&tree[0], &pool, depths, 0);
        } else {
            depths[tree[0].value as usize] = 1;
        }
        if depths.iter().copied().max().unwrap_or(0) <= depth_limit {
            break;
        }
        count_min *= 2;
    }
}

fn code_repeated_values(mut repetitions: usize, tokens: &mut Vec<Token>, value: u8, prev_value: u8) {
    if value != prev_value {
        tokens.push(Token { code: value, extra_bits: 0 });
        repetitions -= 1;
    }
    while repetitions >= 1 {
        if repetitions < 3 {
            for _ in 0..repetitions {
                tokens.push(Token { code: value, extra_bits: 0 });
            }
            break;
        } else if repetitions < 7 {
            tokens.push(Token { code: 16, extra_bits: (repetitions - 3) as u8 });
            break;
        } else {
            tokens.push(Token { code: 16, extra_bits: 3 });
            repetitions -= 6;
        }
    }
}

fn code_repeated_zeros(mut repetitions: usize, tokens: &mut Vec<Token>) {
    while repetitions >= 1 {
        if repetitions < 3 {
            for _ in 0..repetitions {
                tokens.push(Token { code: 0, extra_bits: 0 });
            }
            break;
        } else if repetitions < 11 {
            tokens.push(Token { code: 17, extra_bits: (repetitions - 3) as u8 });
            break;
        } else if repetitions < 139 {
            tokens.push(Token { code: 18, extra_bits: (repetitions - 11) as u8 });
            break;
        } else {
            tokens.push(Token { code: 18, extra_bits: 0x7f });
            repetitions -= 138;
        }
    }
}

/// `VP8LCreateCompressedHuffmanTree`.
pub fn compressed_tree(code: &HuffmanCode) -> Vec<Token> {
    let lengths = &code.code_lengths;
    let mut tokens = Vec::new();
    let mut prev_value = 8;
    let mut i = 0;
    while i < lengths.len() {
        let value = lengths[i];
        let mut k = i + 1;
        while k < lengths.len() && lengths[k] == value {
            k += 1;
        }
        let runs = k - i;
        if value == 0 {
            code_repeated_zeros(runs, &mut tokens);
        } else {
            code_repeated_values(runs, &mut tokens, value, prev_value);
            prev_value = value;
        }
        i += runs;
    }
    tokens
}

/// `ConvertBitDepthsToSymbols`: códigos canônicos com os bits invertidos (o fluxo do VP8L sai do bit baixo).
fn convert_depths_to_symbols(code: &mut HuffmanCode) {
    let mut depth_count = [0u32; MAX_ALLOWED_CODE_LENGTH + 1];
    for &l in &code.code_lengths {
        depth_count[l as usize] += 1;
    }
    depth_count[0] = 0;
    let mut next_code = [0u32; MAX_ALLOWED_CODE_LENGTH + 1];
    let mut c = 0u32;
    for i in 1..=MAX_ALLOWED_CODE_LENGTH {
        c = (c + depth_count[i - 1]) << 1;
        next_code[i] = c;
    }
    for (i, &l) in code.code_lengths.iter().enumerate() {
        let n = next_code[l as usize];
        next_code[l as usize] += 1;
        code.codes[i] = if l == 0 { 0 } else { (n.reverse_bits() >> (32 - u32::from(l))) as u16 };
    }
}

/// `VP8LCreateHuffmanTree`: a contagem é alterada no lugar pela suavização, como no C.
pub fn create_tree(histogram: &mut [u32], depth_limit: u8) -> HuffmanCode {
    let n = histogram.len();
    let mut code = HuffmanCode { code_lengths: vec![0; n], codes: vec![0; n] };
    optimize_for_rle(histogram);
    generate_optimal_tree(histogram, depth_limit, &mut code.code_lengths);
    convert_depths_to_symbols(&mut code);
    code
}
