//! IDCT inteira lenta e exata (`jidctint.c`, `jpeg_idct_islow`), o método padrão (`JDCT_ISLOW`) e o
//! único que o Pillow usa fora do modo rascunho. A saída passa pela tabela de saturação
//! `IDCT_range_limit` do `jdmaster.c`, cuja indexação mascarada com `RANGE_MASK` dá o mesmo efeito
//! de dobra que a tabela do C para valores muito fora da faixa.

const CONST_BITS: i32 = 13;
const PASS1_BITS: i32 = 2;

const FIX_0_298631336: i64 = 2446;
const FIX_0_390180644: i64 = 3196;
const FIX_0_541196100: i64 = 4433;
const FIX_0_765366865: i64 = 6270;
const FIX_0_899976223: i64 = 7373;
const FIX_1_175875602: i64 = 9633;
const FIX_1_501321110: i64 = 12299;
const FIX_1_847759065: i64 = 15137;
const FIX_1_961570560: i64 = 16069;
const FIX_2_053119869: i64 = 16819;
const FIX_2_562915447: i64 = 20995;
const FIX_3_072711026: i64 = 25172;

const RANGE_MASK: i64 = 255 * 4 + 3;

fn descale(x: i64, n: i32) -> i64 {
    (x + (1 << (n - 1))) >> n
}

/// `IDCT_range_limit(cinfo)[x & RANGE_MASK]`: a tabela começa em `CENTERJSAMPLE` dentro do
/// `sample_range_limit` montado pelo `prepare_range_limit_table`.
pub fn range_limit_idct(x: i64) -> u8 {
    let i = (x & RANGE_MASK) as usize;
    // Posições 0..128: x + 128; 128..512: 255; 512..896: zero; 896..1024: os valores 0..127.
    if i < 128 {
        (i + 128) as u8
    } else if i < 512 {
        255
    } else if i < 896 {
        0
    } else {
        (i - 896) as u8
    }
}

/// Uma passada de 8 pontos. `get(k)` lê o k-ésimo coeficiente da coluna ou da linha; devolve os
/// oito valores antes da descalagem final, na ordem 0..7.
fn butterfly(get: impl Fn(usize) -> i64) -> [i64; 8] {
    let z2 = get(2);
    let z3 = get(6);
    let z1 = (z2 + z3) * FIX_0_541196100;
    let tmp2 = z1 + z3 * -FIX_1_847759065;
    let tmp3 = z1 + z2 * FIX_0_765366865;

    let z2 = get(0);
    let z3 = get(4);
    let tmp0 = (z2 + z3) << CONST_BITS;
    let tmp1 = (z2 - z3) << CONST_BITS;

    let tmp10 = tmp0 + tmp3;
    let tmp13 = tmp0 - tmp3;
    let tmp11 = tmp1 + tmp2;
    let tmp12 = tmp1 - tmp2;

    let mut tmp0 = get(7);
    let mut tmp1 = get(5);
    let mut tmp2 = get(3);
    let mut tmp3 = get(1);

    let z1 = tmp0 + tmp3;
    let z2 = tmp1 + tmp2;
    let z3 = tmp0 + tmp2;
    let z4 = tmp1 + tmp3;
    let z5 = (z3 + z4) * FIX_1_175875602;

    tmp0 *= FIX_0_298631336;
    tmp1 *= FIX_2_053119869;
    tmp2 *= FIX_3_072711026;
    tmp3 *= FIX_1_501321110;
    let z1 = z1 * -FIX_0_899976223;
    let z2 = z2 * -FIX_2_562915447;
    let mut z3 = z3 * -FIX_1_961570560;
    let mut z4 = z4 * -FIX_0_390180644;

    z3 += z5;
    z4 += z5;

    tmp0 += z1 + z3;
    tmp1 += z2 + z4;
    tmp2 += z2 + z3;
    tmp3 += z1 + z4;

    [
        tmp10 + tmp3,
        tmp11 + tmp2,
        tmp12 + tmp1,
        tmp13 + tmp0,
        tmp13 - tmp0,
        tmp12 - tmp1,
        tmp11 - tmp2,
        tmp10 - tmp3,
    ]
}

/// `jpeg_idct_islow`: `coef` em ordem natural, `quant` a tabela de quantização em ordem natural.
/// Escreve 8x8 amostras em `out` a partir de `out[off]`, com passo `stride`.
pub fn idct_islow(coef: &[i16; 64], quant: &[u16; 64], out: &mut [u8], off: usize, stride: usize) {
    let mut ws = [0i64; 64];
    for col in 0..8 {
        let deq = |k: usize| i64::from(coef[k * 8 + col]) * i64::from(quant[k * 8 + col]);
        if (1..8).all(|k| coef[k * 8 + col] == 0) {
            let dc = deq(0) << PASS1_BITS;
            for k in 0..8 {
                ws[k * 8 + col] = dc;
            }
            continue;
        }
        let r = butterfly(deq);
        for k in 0..8 {
            // `(int)DESCALE(...)`: o espaço de trabalho do C é `int`.
            ws[k * 8 + col] = i64::from(descale(r[k], CONST_BITS - PASS1_BITS) as i32);
        }
    }
    for row in 0..8 {
        let w = &ws[row * 8..row * 8 + 8];
        let o = off + row * stride;
        if w[1..].iter().all(|&v| v == 0) {
            let dc = range_limit_idct(i64::from(descale(w[0], PASS1_BITS + 3) as i32));
            out[o..o + 8].fill(dc);
            continue;
        }
        let r = butterfly(|k| w[k]);
        for k in 0..8 {
            out[o + k] = range_limit_idct(i64::from(descale(r[k], CONST_BITS + PASS1_BITS + 3) as i32));
        }
    }
}
