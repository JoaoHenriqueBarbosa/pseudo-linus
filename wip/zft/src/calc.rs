//! Aritmética de ponto fixo do `ftcalc.c` (build de 64 bits do Debian, com `FT_MulFix` em linha
//! para x86-64, que trunca os operandos para 32 bits).

pub type Pos = i64;
pub type Fixed = i64;

/// `FT_MulFix` via `FT_MulFix_x86_64`: arredonda para o mais próximo, com empate para longe de zero
/// nos positivos e para zero nos negativos.
pub fn mul_fix(a: i64, b: i64) -> i64 {
    let mut r = i64::from(a as i32) * i64::from(b as i32);
    r += 0x8000 + (r >> 63);
    i64::from((r >> 16) as i32)
}

fn split(x: i64, s: &mut i32) -> u64 {
    if x < 0 {
        *s = -*s;
        0u64.wrapping_sub(x as u64)
    } else {
        x as u64
    }
}

pub fn mul_div(a: i64, b: i64, c: i64) -> i64 {
    let mut s = 1;
    let (a, b, c) = (split(a, &mut s), split(b, &mut s), split(c, &mut s));
    let d = if c > 0 { (a.wrapping_mul(b).wrapping_add(c >> 1)) / c } else { 0x7FFF_FFFF } as i64;
    if s < 0 { d.wrapping_neg() } else { d }
}

pub fn mul_div_no_round(a: i64, b: i64, c: i64) -> i64 {
    let mut s = 1;
    let (a, b, c) = (split(a, &mut s), split(b, &mut s), split(c, &mut s));
    let d = if c > 0 { a.wrapping_mul(b) / c } else { 0x7FFF_FFFF } as i64;
    if s < 0 { d.wrapping_neg() } else { d }
}

pub fn div_fix(a: i64, b: i64) -> i64 {
    let mut s = 1;
    let (a, b) = (split(a, &mut s), split(b, &mut s));
    let q = if b > 0 { ((a << 16).wrapping_add(b >> 1)) / b } else { 0x7FFF_FFFF } as i64;
    if s < 0 { q.wrapping_neg() } else { q }
}

pub fn pix_floor(x: i64) -> i64 {
    x & !63
}

pub fn pix_round(x: i64) -> i64 {
    pix_floor(x.wrapping_add(32))
}

pub fn pix_ceil(x: i64) -> i64 {
    pix_floor(x.wrapping_add(63))
}

/// `FT_MSB`: índice do bit mais significativo.
pub fn msb(x: u32) -> i32 {
    31 - x.leading_zeros() as i32
}

/// `ft_trig_prenorm`, `ft_trig_pseudo_polarize` e `FT_Vector_Length` do `fttrigon.c` (CORDIC).
pub mod trig {
    const SAFE_MSB: i32 = 29;
    const ANGLE_PI2: i64 = 90 << 16;
    const SCALE: u64 = 0xDBD9_5B16;

    static ARCTAN: [i64; 22] = [
        1740967, 919879, 466945, 234379, 117304, 58666, 29335, 14668, 7334, 3667, 1833, 917, 458, 229,
        115, 57, 29, 14, 7, 4, 2, 1,
    ];

    fn downscale(val: i64) -> i64 {
        let s = val < 0;
        let v = if s { val.wrapping_neg() } else { val } as u64;
        let v = (v * SCALE + 0x4000_0000) >> 32;
        if s { -(v as i64) } else { v as i64 }
    }

    fn prenorm(x: &mut i64, y: &mut i64) -> i32 {
        let z = (x.unsigned_abs() | y.unsigned_abs()) as u32;
        let shift = super::msb(z);
        if shift <= SAFE_MSB {
            let s = SAFE_MSB - shift;
            *x = ((*x as u64) << s) as i64;
            *y = ((*y as u64) << s) as i64;
            s
        } else {
            let s = shift - SAFE_MSB;
            *x >>= s;
            *y >>= s;
            -s
        }
    }

    /// Devolve o ângulo; `x` fica com o comprimento ainda não escalado.
    fn pseudo_polarize(x: &mut i64, y: &mut i64) -> i64 {
        let mut theta: i64;
        if *y > *x {
            if *y > -*x {
                theta = ANGLE_PI2;
                let xt = *y;
                *y = -*x;
                *x = xt;
            } else {
                theta = if *y > 0 { ANGLE_PI2 * 2 } else { -ANGLE_PI2 * 2 };
                *x = -*x;
                *y = -*y;
            }
        } else if *y < -*x {
            theta = -ANGLE_PI2;
            let xt = -*y;
            *y = *x;
            *x = xt;
        } else {
            theta = 0;
        }
        let mut b = 1i64;
        for (k, &at) in ARCTAN.iter().enumerate() {
            let i = k + 1;
            let (xt, yt);
            if *y > 0 {
                xt = *x + ((*y + b) >> i);
                yt = *y - ((*x + b) >> i);
                theta += at;
            } else {
                xt = *x - ((*y + b) >> i);
                yt = *y + ((*x + b) >> i);
                theta -= at;
            }
            *x = xt;
            *y = yt;
            b <<= 1;
        }
        // Arredonda o ângulo para múltiplo de 16, como o original.
        if theta >= 0 { (theta + 8) & !15 } else { -((-theta + 8) & !15) }
    }

    pub fn length(mut x: i64, mut y: i64) -> i64 {
        if x == 0 {
            return y.abs();
        }
        if y == 0 {
            return x.abs();
        }
        let shift = prenorm(&mut x, &mut y);
        pseudo_polarize(&mut x, &mut y);
        let v = downscale(x);
        if shift > 0 { (v + (1 << (shift - 1))) >> shift } else { i64::from((v as u32) << -shift) }
    }

    /// `FT_Atan2`.
    pub fn atan2(mut dx: i64, mut dy: i64) -> i64 {
        if dx == 0 && dy == 0 {
            return 0;
        }
        prenorm(&mut dx, &mut dy);
        pseudo_polarize(&mut dx, &mut dy)
    }
}

/// `FT_Hypot`.
pub fn hypot(x: i64, y: i64) -> i64 {
    trig::length(x, y)
}
