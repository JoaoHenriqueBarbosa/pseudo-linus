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

// ---- jidctred.c: saída reduzida para as escalas 1/2, 1/4 e 1/8 ----

const FIX_0_211164243: i64 = 1730;
const FIX_0_509795579: i64 = 4176;
const FIX_0_601344887: i64 = 4926;
const FIX_0_720959822: i64 = 5906;
const FIX_0_850430095: i64 = 6967;
const FIX_1_061594337: i64 = 8697;
const FIX_1_272758580: i64 = 10426;
const FIX_1_451774981: i64 = 11893;
const FIX_2_172734803: i64 = 17799;
const FIX_3_624509785: i64 = 29692;

/// `jpeg_idct_4x4`.
pub fn idct_4x4(coef: &[i16; 64], quant: &[u16; 64], out: &mut [u8], off: usize, stride: usize) {
    let mut ws = [0i64; 32];
    for col in 0..8 {
        if col == 4 {
            continue;
        }
        let deq = |k: usize| i64::from(coef[k * 8 + col]) * i64::from(quant[k * 8 + col]);
        if [1, 2, 3, 5, 6, 7].iter().all(|&k| coef[k * 8 + col] == 0) {
            let dc = deq(0) << PASS1_BITS;
            for k in 0..4 {
                ws[k * 8 + col] = dc;
            }
            continue;
        }
        let tmp0 = deq(0) << (CONST_BITS + 1);
        let tmp2 = deq(2) * FIX_1_847759065 + deq(6) * -FIX_0_765366865;
        let tmp10 = tmp0 + tmp2;
        let tmp12 = tmp0 - tmp2;
        let (z1, z2, z3, z4) = (deq(7), deq(5), deq(3), deq(1));
        let tmp0 = z1 * -FIX_0_211164243 + z2 * FIX_1_451774981 + z3 * -FIX_2_172734803 + z4 * FIX_1_061594337;
        let tmp2 = z1 * -FIX_0_509795579 + z2 * -FIX_0_601344887 + z3 * FIX_0_899976223 + z4 * FIX_2_562915447;
        let n = CONST_BITS - PASS1_BITS + 1;
        ws[col] = i64::from(descale(tmp10 + tmp2, n) as i32);
        ws[24 + col] = i64::from(descale(tmp10 - tmp2, n) as i32);
        ws[8 + col] = i64::from(descale(tmp12 + tmp0, n) as i32);
        ws[16 + col] = i64::from(descale(tmp12 - tmp0, n) as i32);
    }
    for row in 0..4 {
        let w = &ws[row * 8..row * 8 + 8];
        let o = off + row * stride;
        if [1, 2, 3, 5, 6, 7].iter().all(|&k| w[k] == 0) {
            let dc = range_limit_idct(i64::from(descale(w[0], PASS1_BITS + 3) as i32));
            out[o..o + 4].fill(dc);
            continue;
        }
        let tmp0 = w[0] << (CONST_BITS + 1);
        let tmp2 = w[2] * FIX_1_847759065 + w[6] * -FIX_0_765366865;
        let tmp10 = tmp0 + tmp2;
        let tmp12 = tmp0 - tmp2;
        let (z1, z2, z3, z4) = (w[7], w[5], w[3], w[1]);
        let tmp0 = z1 * -FIX_0_211164243 + z2 * FIX_1_451774981 + z3 * -FIX_2_172734803 + z4 * FIX_1_061594337;
        let tmp2 = z1 * -FIX_0_509795579 + z2 * -FIX_0_601344887 + z3 * FIX_0_899976223 + z4 * FIX_2_562915447;
        let n = CONST_BITS + PASS1_BITS + 3 + 1;
        let put = |v: i64| range_limit_idct(i64::from(descale(v, n) as i32));
        out[o] = put(tmp10 + tmp2);
        out[o + 3] = put(tmp10 - tmp2);
        out[o + 1] = put(tmp12 + tmp0);
        out[o + 2] = put(tmp12 - tmp0);
    }
}

/// `jpeg_idct_2x2`.
pub fn idct_2x2(coef: &[i16; 64], quant: &[u16; 64], out: &mut [u8], off: usize, stride: usize) {
    let mut ws = [0i64; 16];
    for col in 0..8 {
        if matches!(col, 2 | 4 | 6) {
            continue;
        }
        let deq = |k: usize| i64::from(coef[k * 8 + col]) * i64::from(quant[k * 8 + col]);
        if [1, 3, 5, 7].iter().all(|&k| coef[k * 8 + col] == 0) {
            let dc = deq(0) << PASS1_BITS;
            ws[col] = dc;
            ws[8 + col] = dc;
            continue;
        }
        let tmp10 = deq(0) << (CONST_BITS + 2);
        let tmp0 = deq(7) * -FIX_0_720959822 + deq(5) * FIX_0_850430095 + deq(3) * -FIX_1_272758580
            + deq(1) * FIX_3_624509785;
        let n = CONST_BITS - PASS1_BITS + 2;
        ws[col] = i64::from(descale(tmp10 + tmp0, n) as i32);
        ws[8 + col] = i64::from(descale(tmp10 - tmp0, n) as i32);
    }
    for row in 0..2 {
        let w = &ws[row * 8..row * 8 + 8];
        let o = off + row * stride;
        if [1, 3, 5, 7].iter().all(|&k| w[k] == 0) {
            let dc = range_limit_idct(i64::from(descale(w[0], PASS1_BITS + 3) as i32));
            out[o..o + 2].fill(dc);
            continue;
        }
        let tmp10 = w[0] << (CONST_BITS + 2);
        let tmp0 = w[7] * -FIX_0_720959822 + w[5] * FIX_0_850430095 + w[3] * -FIX_1_272758580 + w[1] * FIX_3_624509785;
        let n = CONST_BITS + PASS1_BITS + 3 + 2;
        out[o] = range_limit_idct(i64::from(descale(tmp10 + tmp0, n) as i32));
        out[o + 1] = range_limit_idct(i64::from(descale(tmp10 - tmp0, n) as i32));
    }
}

/// `jpeg_idct_1x1`: a média do bloco.
pub fn idct_1x1(coef: &[i16; 64], quant: &[u16; 64], out: &mut [u8], off: usize, _stride: usize) {
    let dc = i32::from(coef[0]) * i32::from(quant[0]);
    out[off] = range_limit_idct(i64::from(descale(i64::from(dc), 3) as i32));
}

/// A IDCT do tamanho escalado da componente (`jddctmgr.c`).
pub fn idct_scaled(n: usize, coef: &[i16; 64], quant: &[u16; 64], out: &mut [u8], off: usize, stride: usize) {
    match n {
        1 => idct_1x1(coef, quant, out, off, stride),
        2 => idct_2x2(coef, quant, out, off, stride),
        4 => idct_4x4(coef, quant, out, off, stride),
        _ => idct_islow(coef, quant, out, off, stride),
    }
}
