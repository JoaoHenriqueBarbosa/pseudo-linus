//! Funções utilitárias puras do SQLite (`util.c`, mais o que `global.c` e `main.c` têm de puro).
//!
//! Convenções deste módulo:
//!
//! - Uma "string C" (terminada em NUL) vira `&[u8]`. Uma leitura além do fim da fatia devolve `0`,
//!   que é exatamente o NUL que o C encontraria; assim `z[i]` do C vira `at(z, i)` e nenhuma rotina
//!   entra em pânico por causa de um terminador ausente.
//! - Os campos de configuração que o C lê de `sqlite3Config` (`bUseLongDouble`) entram como
//!   parâmetro `use_long_double`: quem chama lê `crate::global`.
//! - O `long double` do C (80 bits em x86-64, o caso do Debian) é o tipo [`F80`] abaixo, com
//!   aritmética inteira exata e arredondamento para o par mais próximo, bit a bit igual ao x87.

use std::cmp::Ordering;
use std::sync::OnceLock;

use crate::consts::{
    LARGEST_INT64, LARGEST_UINT64, SMALLEST_INT64, SQLITE_ABORT_ROLLBACK, SQLITE_AFF_BLOB,
    SQLITE_AFF_INTEGER, SQLITE_AFF_NUMERIC, SQLITE_AFF_REAL, SQLITE_AFF_TEXT, SQLITE_DONE,
    SQLITE_MAX_U32, SQLITE_ROW, SQLITE_UTF8,
};
use crate::ctype::{is_digit, is_quote, is_space, is_xdigit, UPPER_TO_LOWER};

/// `LogEst` do sqliteInt.h: `typedef short LogEst`, aproximação de `10*log2(x)`.
pub type LogEst = i16;

/// Byte `i` da "string C" `z`; além do fim da fatia é o NUL terminador (`0`).
#[inline]
pub(crate) fn at(z: &[u8], i: usize) -> u8 {
    z.get(i).copied().unwrap_or(0)
}

// ---------------------------------------------------------------------------------------------
// Tabelas de global.c necessárias às rotinas deste módulo
// ---------------------------------------------------------------------------------------------

/// `sqlite3StdType[]`: nomes dos tipos de dados padrão (índice `eCType-1`).
pub const STD_TYPE: [&str; 6] = ["ANY", "BLOB", "INT", "INTEGER", "REAL", "TEXT"];

/// `sqlite3StdTypeLen[]`: comprimento de cada entrada de [`STD_TYPE`].
pub const STD_TYPE_LEN: [u8; 6] = [3, 4, 3, 7, 4, 4];

/// `sqlite3StdTypeAffinity[]`: afinidade associada a cada entrada de [`STD_TYPE`].
pub const STD_TYPE_AFFINITY: [u8; 6] = [
    SQLITE_AFF_NUMERIC,
    SQLITE_AFF_BLOB,
    SQLITE_AFF_INTEGER,
    SQLITE_AFF_INTEGER,
    SQLITE_AFF_REAL,
    SQLITE_AFF_TEXT,
];

/// `sqlite3StrBINARY`: nome da sequência de ordenação padrão.
pub const STR_BINARY: &str = "BINARY";

// ---------------------------------------------------------------------------------------------
// Ponto flutuante: NaN e estouro
// ---------------------------------------------------------------------------------------------

const EXP754: u64 = 0x7ff << 52;
const MAN754: u64 = (1u64 << 52) - 1;

/// `sqlite3IsNaN`: verdadeiro se `x` não é um número (NaN).
pub fn is_nan(x: f64) -> bool {
    let y = x.to_bits();
    (y & EXP754) == EXP754 && (y & MAN754) != 0
}

/// `sqlite3IsOverflow`: verdadeiro se `x` é NaN, `+Inf` ou `-Inf`.
pub fn is_overflow(x: f64) -> bool {
    let y = x.to_bits();
    (y & EXP754) == EXP754
}

// ---------------------------------------------------------------------------------------------
// Comprimento de string
// ---------------------------------------------------------------------------------------------

/// `sqlite3Strlen30`: comprimento da string C limitado aos 30 bits baixos de um inteiro com sinal.
pub fn strlen30(z: &[u8]) -> i32 {
    let n = z.iter().position(|&c| c == 0).unwrap_or(z.len());
    (0x3fffffff & n) as i32
}

// ---------------------------------------------------------------------------------------------
// Dequote
// ---------------------------------------------------------------------------------------------

/// `sqlite3Dequote`: converte, no próprio buffer, uma string SQL entre aspas em string normal,
/// removendo as aspas (e os colchetes do estilo MS-Access). O buffer guarda uma string terminada
/// em NUL; um NUL novo é gravado no fim da string sem aspas. Se a entrada não começa com um
/// caractere de aspas, nada acontece.
pub fn dequote(z: &mut [u8]) {
    if z.is_empty() {
        return;
    }
    let mut quote = z[0];
    if !is_quote(quote) {
        return;
    }
    if quote == b'[' {
        quote = b']';
    }
    let mut i = 1usize;
    let mut j = 0usize;
    loop {
        let c = at(z, i);
        // O C afirma `z[i] != 0`: o tokenizador sempre entrega a aspa de fechamento.
        debug_assert!(c != 0);
        if c == 0 {
            break;
        }
        if c == quote {
            if at(z, i + 1) == quote {
                z[j] = quote;
                j += 1;
                i += 1;
            } else {
                break;
            }
        } else {
            z[j] = c;
            j += 1;
        }
        i += 1;
    }
    z[j] = 0;
}

// ---------------------------------------------------------------------------------------------
// Comparação de strings sem diferenciar maiúsculas
// ---------------------------------------------------------------------------------------------

/// `sqlite3_stricmp`: como [`str_icmp`], mas aceita ponteiros nulos (`None`).
pub fn stricmp(z_left: Option<&[u8]>, z_right: Option<&[u8]>) -> i32 {
    match (z_left, z_right) {
        (None, r) => {
            if r.is_some() {
                -1
            } else {
                0
            }
        }
        (Some(_), None) => 1,
        (Some(l), Some(r)) => str_icmp(l, r),
    }
}

/// `sqlite3StrICmp`: compara duas strings C sem diferenciar maiúsculas (só ASCII).
pub fn str_icmp(z_left: &[u8], z_right: &[u8]) -> i32 {
    let mut i = 0usize;
    let mut c: i32;
    loop {
        c = at(z_left, i) as i32;
        let x = at(z_right, i) as i32;
        if c == x {
            if c == 0 {
                break;
            }
        } else {
            c = UPPER_TO_LOWER[c as usize] as i32 - UPPER_TO_LOWER[x as usize] as i32;
            if c != 0 {
                break;
            }
        }
        i += 1;
    }
    c
}

/// `sqlite3_strnicmp`: compara no máximo `n` bytes sem diferenciar maiúsculas; aceita `None`.
pub fn strnicmp(z_left: Option<&[u8]>, z_right: Option<&[u8]>, n: i32) -> i32 {
    let (a, b) = match (z_left, z_right) {
        (None, r) => return if r.is_some() { -1 } else { 0 },
        (Some(_), None) => return 1,
        (Some(l), Some(r)) => (l, r),
    };
    let mut n = n;
    let mut i = 0usize;
    // `while( N-- > 0 && *a!=0 && UpperToLower[*a]==UpperToLower[*b] )`
    loop {
        let cont = n > 0;
        n = n.wrapping_sub(1);
        if !(cont
            && at(a, i) != 0
            && UPPER_TO_LOWER[at(a, i) as usize] == UPPER_TO_LOWER[at(b, i) as usize])
        {
            break;
        }
        i += 1;
    }
    if n < 0 {
        0
    } else {
        UPPER_TO_LOWER[at(a, i) as usize] as i32 - UPPER_TO_LOWER[at(b, i) as usize] as i32
    }
}

/// `sqlite3StrIHash`: hash de 8 bits de uma string, insensível a maiúsculas.
pub fn str_i_hash(z: &[u8]) -> u8 {
    let mut h: u8 = 0;
    let mut i = 0usize;
    while at(z, i) != 0 {
        h = h.wrapping_add(UPPER_TO_LOWER[at(z, i) as usize]);
        i += 1;
    }
    h
}

// ---------------------------------------------------------------------------------------------
// Inteiro estendido de 80 bits (o `long double` do x87)
// ---------------------------------------------------------------------------------------------

/// Expoente máximo (não polarizado) de um `long double` normal.
const F80_EXP_MAX: i32 = 16383;
/// Expoente mínimo (não polarizado) de um `long double` normal.
const F80_EXP_MIN: i32 = -16382;
/// Expoente sentinela do infinito.
const F80_EXP_INF: i32 = i32::MAX;

/// Emulação exata do `long double` de 80 bits do x87, restrita ao que o SQLite usa em
/// `sqlite3AtoF` e `sqlite3FpDecode`: valores NÃO NEGATIVOS (o sinal é aplicado depois, sobre o
/// `double`), multiplicação, comparação, conversão de e para `u64` e `f64`.
///
/// O valor é `mant / 2^63 * 2^exp`, com `mant` normalizado (bit 63 ligado) e `exp` não polarizado
/// (`-16382..=16383`). Zero é `mant == 0`; o infinito tem `exp == i32::MAX`.
///
/// Toda operação arredonda para o par mais próximo com 64 bits de mantissa, como o x87 com a
/// palavra de controle padrão do Linux. Resultados abaixo de `2^-16382` (faixa denormal do x87)
/// são levados a zero: no SQLite eles só alimentam a conversão para `double`, que dá zero de
/// qualquer jeito, e o valor nunca volta a subir porque só se multiplica por fatores menores
/// que 1 nesse regime.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct F80 {
    mant: u64,
    exp: i32,
}

impl F80 {
    /// Zero.
    pub const ZERO: F80 = F80 { mant: 0, exp: 0 };
    /// Infinito positivo (o x87 estoura para ele).
    pub const INF: F80 = F80 {
        mant: 1u64 << 63,
        exp: F80_EXP_INF,
    };

    /// Verdadeiro para zero.
    pub fn is_zero(&self) -> bool {
        self.mant == 0
    }

    /// Verdadeiro para o infinito.
    pub fn is_inf(&self) -> bool {
        self.exp == F80_EXP_INF
    }

    /// Normaliza `(mant, exp)` já arredondado, aplicando estouro e subfluxo.
    fn finish(mant: u64, exp: i32) -> F80 {
        if exp > F80_EXP_MAX {
            F80::INF
        } else if exp < F80_EXP_MIN {
            F80::ZERO
        } else {
            F80 { mant, exp }
        }
    }

    /// Arredonda `(p + frac) * 2^scale2` para 64 bits de mantissa (par mais próximo), onde
    /// `frac` em `[0,1)` é diferente de zero se, e somente se, `sticky`.
    fn round_u128(p: u128, sticky: bool, scale2: i32) -> F80 {
        if p == 0 {
            return F80::ZERO;
        }
        let bl = 128 - p.leading_zeros() as i32;
        let mut exp = scale2 + bl - 1;
        let mant: u64;
        if bl <= 64 {
            debug_assert!(!sticky);
            mant = (p as u64) << (64 - bl);
        } else {
            let shift = (bl - 64) as u32;
            let q = (p >> shift) as u64;
            let rem = p & ((1u128 << shift) - 1);
            let half = 1u128 << (shift - 1);
            let up = rem > half || (rem == half && (sticky || q & 1 == 1));
            if up {
                let (m, overflow) = q.overflowing_add(1);
                if overflow {
                    mant = 1u64 << 63;
                    exp += 1;
                } else {
                    mant = m;
                }
            } else {
                mant = q;
            }
        }
        F80::finish(mant, exp)
    }

    /// `(long double)v` para um inteiro sem sinal de 64 bits (exato).
    pub fn from_u64(v: u64) -> F80 {
        if v == 0 {
            return F80::ZERO;
        }
        let lz = v.leading_zeros();
        F80 {
            mant: v << lz,
            exp: 63 - lz as i32,
        }
    }

    /// `(long double)x` para um `double` não negativo (exato, inclusive denormais do `double`).
    pub fn from_f64(x: f64) -> F80 {
        let bits = x.to_bits();
        let e = ((bits >> 52) & 0x7ff) as i32;
        let frac = bits & MAN754;
        if e == 0x7ff {
            return F80::INF;
        }
        if e == 0 {
            if frac == 0 {
                return F80::ZERO;
            }
            // Denormal do double: valor `frac * 2^-1074`.
            let t = F80::from_u64(frac);
            return F80 {
                mant: t.mant,
                exp: t.exp - 1074,
            };
        }
        F80 {
            mant: (frac | (1u64 << 52)) << 11,
            exp: e - 1023,
        }
    }

    /// `(u64)x`: truncamento para zero. O valor precisa ser menor que `2^64` (o SQLite garante).
    pub fn to_u64_trunc(&self) -> u64 {
        if self.mant == 0 || self.exp < 0 {
            return 0;
        }
        if self.exp >= 64 {
            return u64::MAX;
        }
        self.mant >> (63 - self.exp)
    }

    /// `(double)x`: arredonda para o par mais próximo, com estouro para `+Inf` e denormais.
    pub fn to_f64(&self) -> f64 {
        if self.mant == 0 {
            return 0.0;
        }
        if self.is_inf() {
            return f64::INFINITY;
        }
        let mut e = self.exp;
        if e > 1023 {
            return f64::INFINITY;
        }
        let shift: u32 = if e >= -1022 {
            11
        } else {
            (11 + (-1022 - e)) as u32
        };
        if shift > 64 {
            return 0.0;
        }
        let m = self.mant as u128;
        let mut q = m >> shift;
        let rem = m & ((1u128 << shift) - 1);
        let half = 1u128 << (shift - 1);
        if rem > half || (rem == half && q & 1 == 1) {
            q += 1;
        }
        if e >= -1022 {
            if q == (1u128 << 53) {
                q >>= 1;
                e += 1;
                if e > 1023 {
                    return f64::INFINITY;
                }
            }
            f64::from_bits((((e + 1023) as u64) << 52) | (q as u64 & MAN754))
        } else {
            // Denormal do double (um `q == 2^52` vira justamente o menor normal).
            f64::from_bits(q as u64)
        }
    }

    /// Multiplicação com arredondamento para o par mais próximo (`r *= x` do C).
    pub fn mul(&self, o: &F80) -> F80 {
        if self.is_zero() || o.is_zero() {
            return F80::ZERO;
        }
        if self.is_inf() || o.is_inf() {
            return F80::INF;
        }
        let p = (self.mant as u128) * (o.mant as u128);
        F80::round_u128(p, false, self.exp + o.exp - 126)
    }

    /// Comparação total entre valores não negativos.
    pub fn cmp(&self, o: &F80) -> Ordering {
        match (self.is_zero(), o.is_zero()) {
            (true, true) => Ordering::Equal,
            (true, false) => Ordering::Less,
            (false, true) => Ordering::Greater,
            (false, false) => (self.exp, self.mant).cmp(&(o.exp, o.mant)),
        }
    }

    /// `self < o`.
    pub fn lt(&self, o: &F80) -> bool {
        self.cmp(o) == Ordering::Less
    }

    /// `self >= o`.
    pub fn ge(&self, o: &F80) -> bool {
        self.cmp(o) != Ordering::Less
    }

    /// `self > o`.
    pub fn gt(&self, o: &F80) -> bool {
        self.cmp(o) == Ordering::Greater
    }

    /// Valor de uma constante decimal `m * 10^e10` arredondada para o par mais próximo, como o
    /// compilador faz com um literal `long double` (`1.0e+100L` etc.). Só cobre a faixa normal.
    pub fn from_dec(m: u128, e10: i32) -> F80 {
        if m == 0 {
            return F80::ZERO;
        }
        if e10 >= 0 {
            let mut n = Big::from_u128(m);
            n.mul_pow10(e10);
            let bl = n.bit_len();
            if bl <= 128 {
                return F80::round_u128(n.bits_at(0), false, 0);
            }
            let shift = bl - 128;
            F80::round_u128(n.bits_at(shift), n.low_bits_nonzero(shift), shift)
        } else {
            let mut d = Big::from_u128(1);
            d.mul_pow10(-e10);
            let bm = 128 - m.leading_zeros() as i32;
            let bd = d.bit_len();
            // k bits extras no numerador para o quociente ter ao menos 66 bits.
            let k = (bd + 66 - bm).max(0);
            let nbits = bm + k;
            let mut r = Big { l: Vec::new() };
            let mut q: u128 = 0;
            let mut i = nbits - 1;
            while i >= 0 {
                let bit = i >= k && (m >> (i - k)) & 1 == 1;
                r.shl1_or(bit);
                q <<= 1;
                if r.cmp(&d) != Ordering::Less {
                    r.sub_assign(&d);
                    q |= 1;
                }
                i -= 1;
            }
            F80::round_u128(q, !r.l.is_empty(), -k)
        }
    }
}

/// Inteiro sem sinal de precisão arbitrária, só o necessário para [`F80::from_dec`].
/// Limbs de 32 bits em ordem do menos significativo, sem limbs zero à esquerda.
struct Big {
    l: Vec<u32>,
}

impl Big {
    fn from_u128(v: u128) -> Big {
        let mut b = Big {
            l: vec![v as u32, (v >> 32) as u32, (v >> 64) as u32, (v >> 96) as u32],
        };
        b.trim();
        b
    }

    fn trim(&mut self) {
        while let Some(&0) = self.l.last() {
            self.l.pop();
        }
    }

    fn mul_small(&mut self, k: u32) {
        let mut carry = 0u64;
        for x in self.l.iter_mut() {
            let t = (*x as u64) * (k as u64) + carry;
            *x = t as u32;
            carry = t >> 32;
        }
        if carry != 0 {
            self.l.push(carry as u32);
        }
    }

    fn mul_pow10(&mut self, mut e: i32) {
        while e >= 9 {
            self.mul_small(1_000_000_000);
            e -= 9;
        }
        while e > 0 {
            self.mul_small(10);
            e -= 1;
        }
    }

    fn bit_len(&self) -> i32 {
        match self.l.last() {
            None => 0,
            Some(&t) => (self.l.len() as i32 - 1) * 32 + (32 - t.leading_zeros() as i32),
        }
    }

    fn bit(&self, i: i32) -> bool {
        if i < 0 {
            return false;
        }
        let w = (i / 32) as usize;
        w < self.l.len() && (self.l[w] >> (i % 32)) & 1 == 1
    }

    /// Os 128 bits a partir da posição `from` (menos significativo primeiro).
    fn bits_at(&self, from: i32) -> u128 {
        let mut q = 0u128;
        for k in 0..128 {
            if self.bit(from + k) {
                q |= 1u128 << k;
            }
        }
        q
    }

    /// Verdadeiro se algum dos bits `[0, n)` está ligado.
    fn low_bits_nonzero(&self, n: i32) -> bool {
        let full = (n / 32) as usize;
        for i in 0..full.min(self.l.len()) {
            if self.l[i] != 0 {
                return true;
            }
        }
        let rem = n % 32;
        rem != 0 && full < self.l.len() && self.l[full] & ((1u32 << rem) - 1) != 0
    }

    fn cmp(&self, o: &Big) -> Ordering {
        if self.l.len() != o.l.len() {
            return self.l.len().cmp(&o.l.len());
        }
        for i in (0..self.l.len()).rev() {
            if self.l[i] != o.l[i] {
                return self.l[i].cmp(&o.l[i]);
            }
        }
        Ordering::Equal
    }

    /// `self -= o`, com `self >= o`.
    fn sub_assign(&mut self, o: &Big) {
        let mut borrow = 0i64;
        for i in 0..self.l.len() {
            let sub = if i < o.l.len() { o.l[i] as i64 } else { 0 };
            let mut t = self.l[i] as i64 - sub - borrow;
            if t < 0 {
                t += 1i64 << 32;
                borrow = 1;
            } else {
                borrow = 0;
            }
            self.l[i] = t as u32;
        }
        self.trim();
    }

    /// `self = (self << 1) | bit`.
    fn shl1_or(&mut self, bit: bool) {
        let mut carry = bit as u32;
        for x in self.l.iter_mut() {
            let nc = *x >> 31;
            *x = (*x << 1) | carry;
            carry = nc;
        }
        if carry != 0 {
            self.l.push(carry);
        }
    }
}

/// Constantes `long double` que `sqlite3AtoF` e `sqlite3FpDecode` usam como literais `...L`.
struct LdConsts {
    p100: F80, /* 1.0e+100L */
    p10: F80,  /* 1.0e+10L */
    p1: F80,   /* 1.0e+01L */
    n100: F80, /* 1.0e-100L */
    n10: F80,  /* 1.0e-10L */
    n1: F80,   /* 1.0e-01L */
    p119: F80, /* 1.0e+119L */
    p29: F80,  /* 1.0e+29L */
    p19: F80,  /* 1.0e+19L (e o double 1.0e+19, que é exato) */
    n97: F80,  /* 1.0e-97L */
    p7: F80,   /* 1.0e+07L */
    p17: F80,  /* 1.0e+17L */
    /// `1.7976931348623157081452742373e+308L`, o limite de `double` do `sqlite3AtoF`.
    dbl_max: F80,
}

fn ld_consts() -> &'static LdConsts {
    static C: OnceLock<LdConsts> = OnceLock::new();
    C.get_or_init(|| LdConsts {
        p100: F80::from_dec(1, 100),
        p10: F80::from_dec(1, 10),
        p1: F80::from_dec(1, 1),
        n100: F80::from_dec(1, -100),
        n10: F80::from_dec(1, -10),
        n1: F80::from_dec(1, -1),
        p119: F80::from_dec(1, 119),
        p29: F80::from_dec(1, 29),
        p19: F80::from_dec(1, 19),
        n97: F80::from_dec(1, -97),
        p7: F80::from_dec(1, 7),
        p17: F80::from_dec(1, 17),
        dbl_max: F80::from_dec(17976931348623157081452742373u128, 280),
    })
}

// ---------------------------------------------------------------------------------------------
// Double-double (Dekker), usado quando `bUseLongDouble` está desligado
// ---------------------------------------------------------------------------------------------

/// `(u64)d` como o gcc gera em x86-64 (`cvttsd2si` com o truque do bit 63) para `d >= 0`:
/// acima de `2^64` o resultado é 0, como no hardware.
fn cvt_f64_to_u64(d: f64) -> u64 {
    const TWO63: f64 = 9223372036854775808.0;
    if d < TWO63 {
        d as i64 as u64
    } else {
        let t = d - TWO63;
        let i = if t < TWO63 { t as i64 as u64 } else { 0x8000000000000000 };
        i ^ 0x8000000000000000
    }
}

/// `dekkerMul2`: multiplicação double-double `(x[0],x[1]) *= (y,yy)`.
///
/// Referência: T. J. Dekker, "A Floating-Point Technique for Extending the Available
/// Precision", 1971-07-26. As operações de `f64` do Rust são IEEE estritas (sem registradores
/// estendidos e sem fusão de multiplicação e soma), o que o `volatile` do C força.
fn dekker_mul2(x: &mut [f64; 2], y: f64, yy: f64) {
    let m = x[0].to_bits() & 0xfffffffffc000000;
    let hx = f64::from_bits(m);
    let tx = x[0] - hx;
    let m = y.to_bits() & 0xfffffffffc000000;
    let hy = f64::from_bits(m);
    let ty = y - hy;
    let p = hx * hy;
    let q = hx * ty + tx * hy;
    let c = p + q;
    let mut cc = p - c + q + tx * ty;
    cc = x[0] * yy + x[1] * y + cc;
    x[0] = c + cc;
    x[1] = c - x[0];
    x[1] += cc;
}

// ---------------------------------------------------------------------------------------------
// sqlite3AtoF
// ---------------------------------------------------------------------------------------------

/// `sqlite3AtoF`: converte a representação textual de um número real para `double`.
///
/// `z` tem `length` bytes (bytes, não caracteres) na codificação `enc` e não é necessariamente
/// terminada em NUL. Devolve `(código, resultado)`; o resultado é gravado mesmo quando só um
/// prefixo da entrada é um número válido. O código é:
///
/// - `1`: a entrada é um inteiro puro
/// - `2` ou mais: tem ponto decimal ou cláusula `eNNN`
/// - `0` ou menos: não é um número válido
/// - `-1`: não é válido, mas tem um prefixo válido com ponto decimal e/ou `eNNN`
///
/// `use_long_double` é `sqlite3Config.bUseLongDouble` (verdadeiro em x86-64): escolhe entre a
/// aritmética de 80 bits ([`F80`]) e a double-double de Dekker.
pub fn atof(z: &[u8], length: i32, enc: u8, use_long_double: bool) -> (i32, f64) {
    let mut length = length;
    let incr: usize;
    let z_end: usize;
    let mut p: usize = 0;
    // sinal * significando * (10 ^ (esign * expoente))
    let mut sign: i32 = 1;
    let mut s: u64 = 0;
    let mut d: i32 = 0;
    let mut esign: i32 = 1;
    let mut e: i32 = 0;
    let mut e_valid = true;
    let mut n_digit: i32 = 0;
    let mut e_type: i32 = 1;
    let mut result: f64;

    if length == 0 {
        return (0, 0.0);
    }

    if enc as i32 == SQLITE_UTF8 {
        incr = 1;
        z_end = length.max(0) as usize;
    } else {
        incr = 2;
        length &= !1;
        let mut i = 3 - enc as i32;
        while i < length && at(z, i as usize) == 0 {
            i += 2;
        }
        if i < length {
            e_type = -100;
        }
        z_end = (i ^ 1) as usize;
        p = (enc & 1) as usize;
    }

    // pula os espaços iniciais
    while p < z_end && is_space(at(z, p)) {
        p += incr;
    }
    if p >= z_end {
        return (0, 0.0);
    }

    // sinal do significando
    if at(z, p) == b'-' {
        sign = -1;
        p += incr;
    } else if at(z, p) == b'+' {
        p += incr;
    }

    'parse: {
        // copia os dígitos significativos para o significando
        while p < z_end && is_digit(at(z, p)) {
            s = s.wrapping_mul(10).wrapping_add((at(z, p) - b'0') as u64);
            p += incr;
            n_digit += 1;
            if s >= (LARGEST_UINT64 - 9) / 10 {
                // pula os dígitos não significativos (aumenta o expoente em d)
                while p < z_end && is_digit(at(z, p)) {
                    p += incr;
                    d += 1;
                }
            }
        }
        if p >= z_end {
            break 'parse;
        }

        // ponto decimal
        if at(z, p) == b'.' {
            p += incr;
            e_type += 1;
            // copia os dígitos depois do ponto (diminui o expoente em d)
            while p < z_end && is_digit(at(z, p)) {
                if s < (LARGEST_UINT64 - 9) / 10 {
                    s = s.wrapping_mul(10).wrapping_add((at(z, p) - b'0') as u64);
                    d -= 1;
                    n_digit += 1;
                }
                p += incr;
            }
        }
        if p >= z_end {
            break 'parse;
        }

        // expoente
        if at(z, p) == b'e' || at(z, p) == b'E' {
            p += incr;
            e_valid = false;
            e_type += 1;

            // Evita uma leitura além do fim (inofensiva no C).
            if p >= z_end {
                break 'parse;
            }

            // sinal do expoente
            if at(z, p) == b'-' {
                esign = -1;
                p += incr;
            } else if at(z, p) == b'+' {
                p += incr;
            }
            // copia os dígitos para o expoente
            while p < z_end && is_digit(at(z, p)) {
                e = if e < 10000 {
                    e * 10 + (at(z, p) - b'0') as i32
                } else {
                    10000
                };
                p += incr;
                e_valid = true;
            }
        }

        // pula os espaços finais
        while p < z_end && is_space(at(z, p)) {
            p += incr;
        }
    }

    // do_atof_calc: zero é um caso especial
    if s == 0 {
        result = if sign < 0 { -0.0 } else { 0.0 };
    } else {
        // ajusta o expoente por d e atualiza o sinal
        e = (e * esign).wrapping_add(d);

        // tenta diminuir o expoente
        while e > 0 && s < (LARGEST_UINT64 / 10) {
            s *= 10;
            e -= 1;
        }
        while e < 0 && (s % 10) == 0 {
            s /= 10;
            e += 1;
        }

        if e == 0 {
            result = s as f64;
        } else if use_long_double {
            let c = ld_consts();
            let mut r = F80::from_u64(s);
            if e > 0 {
                while e >= 100 {
                    e -= 100;
                    r = r.mul(&c.p100);
                }
                while e >= 10 {
                    e -= 10;
                    r = r.mul(&c.p10);
                }
                while e >= 1 {
                    e -= 1;
                    r = r.mul(&c.p1);
                }
            } else {
                while e <= -100 {
                    e += 100;
                    r = r.mul(&c.n100);
                }
                while e <= -10 {
                    e += 10;
                    r = r.mul(&c.n10);
                }
                while e <= -1 {
                    e += 1;
                    r = r.mul(&c.n1);
                }
            }
            if r.gt(&c.dbl_max) {
                result = f64::INFINITY;
            } else {
                result = r.to_f64();
            }
        } else {
            let mut rr = [0.0f64; 2];
            rr[0] = s as f64;
            let s2 = cvt_f64_to_u64(rr[0]);
            rr[1] = if s >= s2 {
                s.wrapping_sub(s2) as f64
            } else {
                -(s2.wrapping_sub(s) as f64)
            };
            if e > 0 {
                while e >= 100 {
                    e -= 100;
                    dekker_mul2(&mut rr, 1.0e+100, -1.5902891109759918046e+83);
                }
                while e >= 10 {
                    e -= 10;
                    dekker_mul2(&mut rr, 1.0e+10, 0.0);
                }
                while e >= 1 {
                    e -= 1;
                    dekker_mul2(&mut rr, 1.0e+01, 0.0);
                }
            } else {
                while e <= -100 {
                    e += 100;
                    dekker_mul2(&mut rr, 1.0e-100, -1.99918998026028836196e-117);
                }
                while e <= -10 {
                    e += 10;
                    dekker_mul2(&mut rr, 1.0e-10, -3.6432197315497741579e-27);
                }
                while e <= -1 {
                    e += 1;
                    dekker_mul2(&mut rr, 1.0e-01, -5.5511151231257827021e-18);
                }
            }
            result = rr[0] + rr[1];
            if is_nan(result) {
                result = f64::INFINITY;
            }
        }
        if sign < 0 {
            result = -result;
        }
        debug_assert!(!is_nan(result));
    }

    // atof_return: verdadeiro se for número e não houver lixo depois dos espaços
    let rc = if p == z_end && n_digit > 0 && e_valid && e_type > 0 {
        e_type
    } else if e_type >= 2 && (e_type == 3 || e_valid) && n_digit > 0 {
        -1
    } else {
        0
    };
    (rc, result)
}

// ---------------------------------------------------------------------------------------------
// Inteiros: texto e conversões
// ---------------------------------------------------------------------------------------------

/// `sqlite3Int64ToText`: grava `v` em decimal em `z_out` (seguido de um NUL) e devolve o
/// comprimento sem o NUL. `z_out` precisa ter ao menos 21 bytes.
pub fn int64_to_text(v: i64, z_out: &mut [u8]) -> i32 {
    let mut z_temp = [0u8; 22];
    let mut x: u64 = if v < 0 {
        if v == SMALLEST_INT64 {
            1u64 << 63
        } else {
            (-v) as u64
        }
    } else {
        v as u64
    };
    let mut i = z_temp.len() - 2;
    z_temp[z_temp.len() - 1] = 0;
    loop {
        z_temp[i] = (x % 10) as u8 + b'0';
        x /= 10;
        if x == 0 {
            break;
        }
        i -= 1;
    }
    if v < 0 {
        i -= 1;
        z_temp[i] = b'-';
    }
    let n = z_temp.len() - i;
    z_out[..n].copy_from_slice(&z_temp[i..]);
    (z_temp.len() - 1 - i) as i32
}

/// `compare2pow63`: compara os 19 caracteres de `z` (a partir de `base`, de `incr` em `incr`)
/// com `9223372036854775808`. Devolve a diferença do último dígito se só ele diferir.
fn compare2pow63(z: &[u8], base: usize, incr: usize) -> i32 {
    let pow63 = b"922337203685477580";
    let mut c: i32 = 0;
    let mut i = 0usize;
    while c == 0 && i < 18 {
        c = (at(z, base + i * incr) as i32 - pow63[i] as i32) * 10;
        i += 1;
    }
    if c == 0 {
        c = at(z, base + 18 * incr) as i32 - b'8' as i32;
    }
    c
}

/// `sqlite3Atoi64`: converte um decimal (sem hexadecimal) em inteiro de 64 bits com sinal,
/// gravando em `num`. `length` é o número de bytes; a codificação é `enc`. Devolve:
///
/// - `-1`: nem um prefixo da entrada parece um inteiro
/// - `0`: sucesso, cabe em 64 bits com sinal
/// - `1`: sobra texto que não é espaço depois do inteiro
/// - `2`: grande demais para 64 bits com sinal ou malformado
/// - `3`: o caso especial `9223372036854775808`
pub fn atoi64(z: &[u8], num: &mut i64, length: i32, enc: u8) -> i32 {
    let mut length = length;
    let incr: usize;
    let mut u: u64 = 0;
    let mut neg = false;
    let mut non_num = false;
    let mut zn: usize = 0;
    let z_end: usize;
    if enc as i32 == SQLITE_UTF8 {
        incr = 1;
        z_end = length.max(0) as usize;
    } else {
        incr = 2;
        length &= !1;
        let mut i = 3 - enc as i32;
        while i < length && at(z, i as usize) == 0 {
            i += 2;
        }
        non_num = i < length;
        z_end = (i ^ 1) as usize;
        zn += (enc & 1) as usize;
    }
    while zn < z_end && is_space(at(z, zn)) {
        zn += incr;
    }
    if zn < z_end {
        if at(z, zn) == b'-' {
            neg = true;
            zn += incr;
        } else if at(z, zn) == b'+' {
            zn += incr;
        }
    }
    let z_start = zn;
    while zn < z_end && at(z, zn) == b'0' {
        zn += incr; /* pula os zeros à esquerda */
    }
    let mut i: usize = 0;
    while zn + i < z_end {
        let c = at(z, zn + i);
        if !(b'0'..=b'9').contains(&c) {
            break;
        }
        u = u.wrapping_mul(10).wrapping_add(c as u64).wrapping_sub(b'0' as u64);
        i += incr;
    }
    if u > LARGEST_INT64 as u64 {
        *num = if neg { SMALLEST_INT64 } else { LARGEST_INT64 };
    } else if neg {
        *num = -(u as i64);
    } else {
        *num = u as i64;
    }
    let mut rc = 0;
    if i == 0 && z_start == zn {
        /* nenhum dígito */
        rc = -1;
    } else if non_num {
        /* UTF16 com byte alto diferente de zero */
        rc = 1;
    } else if zn + i < z_end {
        /* bytes sobrando no fim */
        let mut jj = i;
        loop {
            if !is_space(at(z, zn + jj)) {
                rc = 1; /* texto que não é espaço depois do inteiro */
                break;
            }
            jj += incr;
            if zn + jj >= z_end {
                break;
            }
        }
    }
    if i < 19 * incr {
        /* menos de 19 dígitos: cabe em 64 bits */
        debug_assert!(u <= LARGEST_INT64 as u64);
        rc
    } else {
        /* número de 19 dígitos: compara com 9223372036854775808 */
        let c = if i > 19 * incr {
            1
        } else {
            compare2pow63(z, zn, incr)
        };
        if c < 0 {
            /* menor que 9223372036854775808: cabe */
            debug_assert!(u <= LARGEST_INT64 as u64);
            rc
        } else {
            *num = if neg { SMALLEST_INT64 } else { LARGEST_INT64 };
            if c > 0 {
                /* maior que 9223372036854775808: estoura */
                2
            } else {
                /* exatamente 9223372036854775808: cabe se negativo; positivo é o caso 3 */
                debug_assert!(u.wrapping_sub(1) == LARGEST_INT64 as u64);
                if neg {
                    rc
                } else {
                    3
                }
            }
        }
    }
}

/// `sqlite3HexToInt`: um byte hexadecimal vira o seu valor. Só vale se `h` é `0..9a..fA..F`.
pub fn hex_to_int(h: i32) -> u8 {
    let h = h + 9 * (1 & (h >> 6));
    (h & 0xf) as u8
}

/// `sqlite3DecOrHexToI64`: converte um literal inteiro UTF-8, decimal ou hexadecimal, em
/// inteiro de 64 bits. Devolve `0` (sucesso), `1` (texto sobrando), `2` (grande demais ou
/// malformado) ou `3` (o caso especial `9223372036854775808`).
pub fn dec_or_hex_to_i64(z: &[u8], out: &mut i64) -> i32 {
    if at(z, 0) == b'0' && (at(z, 1) == b'x' || at(z, 1) == b'X') {
        let mut u: u64 = 0;
        let mut i = 2usize;
        while at(z, i) == b'0' {
            i += 1;
        }
        let mut k = i;
        while is_xdigit(at(z, k)) {
            u = u.wrapping_mul(16).wrapping_add(hex_to_int(at(z, k) as i32) as u64);
            k += 1;
        }
        *out = u as i64;
        if k - i > 16 {
            return 2;
        }
        if at(z, k) != 0 {
            return 1;
        }
        0
    } else {
        // `strspn(z, "+- \n\t0123456789")`
        let mut n = 0usize;
        while matches!(at(z, n), b'+' | b'-' | b' ' | b'\n' | b'\t' | b'0'..=b'9') {
            n += 1;
        }
        let mut n = 0x3fffffff & n;
        if at(z, n) != 0 {
            n += 1;
        }
        atoi64(z, out, n as i32, SQLITE_UTF8 as u8)
    }
}

/// `sqlite3GetInt32`: se `z` representa um inteiro de 32 bits, devolve o valor. Aceita decimal
/// e hexadecimal (sem sinal); caracteres não numéricos depois do número são ignorados. Em caso
/// de falha o C não toca em `*pValue`, e aqui devolve `None`.
pub fn get_int32(z: &[u8]) -> Option<i32> {
    let mut v: i64 = 0;
    let mut zn = 0usize;
    let mut neg = false;
    if at(z, 0) == b'-' {
        neg = true;
        zn += 1;
    } else if at(z, 0) == b'+' {
        zn += 1;
    } else if at(z, 0) == b'0'
        && (at(z, 1) == b'x' || at(z, 1) == b'X')
        && is_xdigit(at(z, 2))
    {
        let mut u: u32 = 0;
        zn += 2;
        while at(z, zn) == b'0' {
            zn += 1;
        }
        let mut i = 0usize;
        while i < 8 && is_xdigit(at(z, zn + i)) {
            u = u.wrapping_mul(16).wrapping_add(hex_to_int(at(z, zn + i) as i32) as u32);
            i += 1;
        }
        if (u & 0x80000000) == 0 && !is_xdigit(at(z, zn + i)) {
            return Some(u as i32);
        } else {
            return None;
        }
    }
    if !is_digit(at(z, zn)) {
        return None;
    }
    while at(z, zn) == b'0' {
        zn += 1;
    }
    let mut i = 0usize;
    while i < 11 {
        let c = at(z, zn + i) as i32 - b'0' as i32;
        if !(0..=9).contains(&c) {
            break;
        }
        v = v * 10 + c as i64;
        i += 1;
    }
    // A representação decimal mais longa de um inteiro de 32 bits tem 10 dígitos.
    if i > 10 {
        return None;
    }
    if v - (neg as i64) > 2147483647 {
        return None;
    }
    if neg {
        v = -v;
    }
    Some(v as i32)
}

/// `sqlite3Atoi`: inteiro de 32 bits extraído de uma string; se não for inteiro, `0`.
pub fn atoi(z: &[u8]) -> i32 {
    get_int32(z).unwrap_or(0)
}

/// `sqlite3GetUInt32`: converte `z` (só decimal) em inteiro sem sinal de 32 bits. Em caso de
/// falha o C grava `0` em `*pI`; aqui devolve `None`.
pub fn get_u_int32(z: &[u8]) -> Option<u32> {
    let mut v: u64 = 0;
    let mut i = 0usize;
    while is_digit(at(z, i)) {
        v = v * 10 + at(z, i) as u64 - b'0' as u64;
        if v > 4294967296 {
            return None;
        }
        i += 1;
    }
    if i == 0 || at(z, i) != 0 {
        return None;
    }
    Some(v as u32)
}

// ---------------------------------------------------------------------------------------------
// sqlite3FpDecode
// ---------------------------------------------------------------------------------------------

/// `FpDecode` do sqliteInt.h: decodificação de um `double` em representação decimal aproximada.
///
/// No C `z` aponta para dentro de `zBuf` (ou para o literal `"0"`); aqui `z_off` é o deslocamento
/// do primeiro dígito em `z_buf` (use [`FpDecode::digits`]). O literal `"0"` do caso zero é
/// copiado para `z_buf[0]`, com `z_off == 0`.
#[derive(Clone, Debug, Default)]
pub struct FpDecode {
    /// `'+'` ou `'-'`.
    pub sign: u8,
    /// `1`: infinito; `2`: NaN; `0`: número comum.
    pub is_special: u8,
    /// Quantidade de dígitos significativos.
    pub n: i32,
    /// Posição do ponto decimal.
    pub i_dp: i32,
    /// Início dos dígitos em `z_buf`.
    pub z_off: usize,
    /// Armazenamento dos dígitos.
    pub z_buf: [u8; 24],
}

impl FpDecode {
    /// Os dígitos significativos a partir de `z` (`n` deles são válidos; não termina em NUL).
    pub fn digits(&self) -> &[u8] {
        &self.z_buf[self.z_off..]
    }
}

/// `sqlite3FpDecode`: decodifica `r` em dígitos decimais aproximados.
///
/// Arredonda para `i_round` dígitos significativos se positivo, ou para `-i_round` dígitos depois
/// do ponto decimal se negativo; sem arredondamento se zero. `mx_round` limita os dígitos.
/// `use_long_double` é `sqlite3Config.bUseLongDouble`.
pub fn fp_decode(r: f64, i_round: i32, mx_round: i32, use_long_double: bool) -> FpDecode {
    let mut p = FpDecode::default();
    let mut i_round = i_round;
    let mut r = r;
    let mut exp: i32 = 0;
    p.is_special = 0;
    p.z_off = 0;

    // Torna os negativos positivos e trata Infinito, 0.0 e NaN.
    if r < 0.0 {
        p.sign = b'-';
        r = -r;
    } else if r == 0.0 {
        p.sign = b'+';
        p.n = 1;
        p.i_dp = 1;
        p.z_buf[0] = b'0';
        p.z_off = 0;
        return p;
    } else {
        p.sign = b'+';
    }
    let mut v: u64 = r.to_bits();
    let e = (v >> 52) as i32;
    if (e & 0x7ff) == 0x7ff {
        p.is_special = 1 + (v != 0x7ff0000000000000) as u8;
        p.n = 0;
        p.i_dp = 0;
        return p;
    }

    // Multiplica r por potências de dez até cair entre 1.0e+19 e 1.0e+17.
    if use_long_double {
        let c = ld_consts();
        let mut rr = F80::from_f64(r);
        if rr.ge(&c.p19) {
            while rr.ge(&c.p119) {
                exp += 100;
                rr = rr.mul(&c.n100);
            }
            while rr.ge(&c.p29) {
                exp += 10;
                rr = rr.mul(&c.n10);
            }
            while rr.ge(&c.p19) {
                exp += 1;
                rr = rr.mul(&c.n1);
            }
        } else {
            while rr.lt(&c.n97) {
                exp -= 100;
                rr = rr.mul(&c.p100);
            }
            while rr.lt(&c.p7) {
                exp -= 10;
                rr = rr.mul(&c.p10);
            }
            while rr.lt(&c.p17) {
                exp -= 1;
                rr = rr.mul(&c.p1);
            }
        }
        v = rr.to_u64_trunc();
    } else {
        // Sem `long double` de verdade, usa a computação double-double de Dekker.
        let mut rr = [r, 0.0f64];
        if rr[0] > 9.223372036854774784e+18 {
            while rr[0] > 9.223372036854774784e+118 {
                exp += 100;
                dekker_mul2(&mut rr, 1.0e-100, -1.99918998026028836196e-117);
            }
            while rr[0] > 9.223372036854774784e+28 {
                exp += 10;
                dekker_mul2(&mut rr, 1.0e-10, -3.6432197315497741579e-27);
            }
            while rr[0] > 9.223372036854774784e+18 {
                exp += 1;
                dekker_mul2(&mut rr, 1.0e-01, -5.5511151231257827021e-18);
            }
        } else {
            while rr[0] < 9.223372036854774784e-83 {
                exp -= 100;
                dekker_mul2(&mut rr, 1.0e+100, -1.5902891109759918046e+83);
            }
            while rr[0] < 9.223372036854774784e+07 {
                exp -= 10;
                dekker_mul2(&mut rr, 1.0e+10, 0.0);
            }
            while rr[0] < 9.22337203685477478e+17 {
                exp -= 1;
                dekker_mul2(&mut rr, 1.0e+01, 0.0);
            }
        }
        v = if rr[1] < 0.0 {
            cvt_f64_to_u64(rr[0]).wrapping_sub(cvt_f64_to_u64(-rr[1]))
        } else {
            cvt_f64_to_u64(rr[0]).wrapping_add(cvt_f64_to_u64(rr[1]))
        };
    }

    // Extrai os dígitos significativos.
    let mut i: i32 = p.z_buf.len() as i32 - 1;
    debug_assert!(v > 0);
    while v != 0 {
        p.z_buf[i as usize] = (v % 10) as u8 + b'0';
        i -= 1;
        v /= 10;
    }
    debug_assert!(i >= 0 && (i as usize) < p.z_buf.len() - 1);
    p.n = p.z_buf.len() as i32 - 1 - i;
    debug_assert!(p.n > 0);
    p.i_dp = p.n + exp;
    if i_round <= 0 {
        i_round = p.i_dp - i_round;
        if i_round == 0 && p.z_buf[(i + 1) as usize] >= b'5' {
            i_round = 1;
            p.z_buf[i as usize] = b'0';
            i -= 1;
            p.n += 1;
            p.i_dp += 1;
        }
    }
    if i_round > 0 && (i_round < p.n || p.n > mx_round) {
        let zb = (i + 1) as usize;
        if i_round > mx_round {
            i_round = mx_round;
        }
        p.n = i_round;
        if p.z_buf[zb + i_round as usize] >= b'5' {
            let mut j = i_round - 1;
            loop {
                let k = zb + j as usize;
                p.z_buf[k] += 1;
                if p.z_buf[k] <= b'9' {
                    break;
                }
                p.z_buf[k] = b'0';
                if j == 0 {
                    p.z_buf[i as usize] = b'1';
                    i -= 1;
                    p.n += 1;
                    p.i_dp += 1;
                    break;
                } else {
                    j -= 1;
                }
            }
        }
    }
    p.z_off = (i + 1) as usize;
    debug_assert!(i + p.n < p.z_buf.len() as i32);
    while p.n > 0 && p.z_buf[p.z_off + (p.n - 1) as usize] == b'0' {
        p.n -= 1;
    }
    p
}

// ---------------------------------------------------------------------------------------------
// Varint
// ---------------------------------------------------------------------------------------------
//
// Codificação de inteiro de comprimento variável:
//
//   A = 0xxxxxxx    7 bits de dados e um bit de continuação
//   B = 1xxxxxxx    7 bits de dados e um bit de continuação
//   C = xxxxxxxx    8 bits de dados
//
//    7 bits - A
//   14 bits - BA
//   21 bits - BBA
//   28 bits - BBBA
//   35 bits - BBBBA
//   42 bits - BBBBBA
//   49 bits - BBBBBBA
//   56 bits - BBBBBBBA
//   64 bits - BBBBBBBBC

/// `putVarint64`: caminho geral de escrita (9 bytes se algum dos 8 bits altos estiver ligado).
fn put_varint64(p: &mut [u8], v: u64) -> i32 {
    let mut v = v;
    let mut buf = [0u8; 10];
    if v & (0xff000000u64 << 32) != 0 {
        p[8] = v as u8;
        v >>= 8;
        for i in (0..=7).rev() {
            p[i] = ((v & 0x7f) | 0x80) as u8;
            v >>= 7;
        }
        return 9;
    }
    let mut n = 0usize;
    loop {
        buf[n] = ((v & 0x7f) | 0x80) as u8;
        n += 1;
        v >>= 7;
        if v == 0 {
            break;
        }
    }
    buf[0] &= 0x7f;
    debug_assert!(n <= 9);
    let mut i = 0usize;
    for j in (0..n).rev() {
        p[i] = buf[j];
        i += 1;
    }
    n as i32
}

/// `sqlite3PutVarint`: grava `v` em `p` como varint (1 a 9 bytes) e devolve quantos bytes.
pub fn put_varint(p: &mut [u8], v: u64) -> i32 {
    if v <= 0x7f {
        p[0] = (v & 0x7f) as u8;
        return 1;
    }
    if v <= 0x3fff {
        p[0] = (((v >> 7) & 0x7f) | 0x80) as u8;
        p[1] = (v & 0x7f) as u8;
        return 2;
    }
    put_varint64(p, v)
}

/// Máscara `(0x7f<<14) | 0x7f` do `sqlite3GetVarint`.
const SLOT_2_0: u32 = 0x001fc07f;
/// Máscara `(0x7f<<28) | SLOT_2_0` do `sqlite3GetVarint`.
const SLOT_4_2_0: u32 = 0xf01fc07f;

/// `sqlite3GetVarint`: lê um varint de 64 bits de `p`. Devolve `(bytes lidos, valor)`.
/// Uma leitura além do fim da fatia enxerga zeros (o C leria a memória vizinha).
pub fn get_varint(p: &[u8]) -> (u8, u64) {
    let b_at = |i: usize| at(p, i) as u32;
    let mut a: u32;
    let mut b: u32;
    let mut s: u32;

    if b_at(0) < 0x80 {
        return (1, b_at(0) as u64);
    }
    if b_at(1) < 0x80 {
        return (2, (((b_at(0) & 0x7f) << 7) | b_at(1)) as u64);
    }

    a = b_at(0) << 14;
    b = b_at(1);
    a |= b_at(2);
    /* a: p0<<14 | p2 (sem máscara) */
    if a & 0x80 == 0 {
        a &= SLOT_2_0;
        b &= 0x7f;
        b <<= 7;
        a |= b;
        return (3, a as u64);
    }

    a &= SLOT_2_0;
    b <<= 14;
    b |= b_at(3);
    /* b: p1<<14 | p3 (sem máscara) */
    if b & 0x80 == 0 {
        b &= SLOT_2_0;
        a <<= 7;
        a |= b;
        return (4, a as u64);
    }

    b &= SLOT_2_0;
    s = a;
    /* s: p0<<14 | p2 (com máscara) */

    a <<= 14;
    a |= b_at(4);
    /* a: p0<<28 | p2<<14 | p4 (sem máscara) */
    if a & 0x80 == 0 {
        b <<= 7;
        a |= b;
        s >>= 18;
        return (5, ((s as u64) << 32) | a as u64);
    }

    s <<= 7;
    s |= b;
    /* s: p0<<21 | p1<<14 | p2<<7 | p3 (com máscara) */

    b <<= 14;
    b |= b_at(5);
    /* b: p1<<28 | p3<<14 | p5 (sem máscara) */
    if b & 0x80 == 0 {
        a &= SLOT_2_0;
        a <<= 7;
        a |= b;
        s >>= 18;
        return (6, ((s as u64) << 32) | a as u64);
    }

    a <<= 14;
    a |= b_at(6);
    /* a: p2<<28 | p4<<14 | p6 (sem máscara) */
    if a & 0x80 == 0 {
        a &= SLOT_4_2_0;
        b &= SLOT_2_0;
        b <<= 7;
        a |= b;
        s >>= 11;
        return (7, ((s as u64) << 32) | a as u64);
    }

    a &= SLOT_2_0;
    b <<= 14;
    b |= b_at(7);
    /* b: p3<<28 | p5<<14 | p7 (sem máscara) */
    if b & 0x80 == 0 {
        b &= SLOT_4_2_0;
        a <<= 7;
        a |= b;
        s >>= 4;
        return (8, ((s as u64) << 32) | a as u64);
    }

    a <<= 15;
    a |= b_at(8);
    /* a: p4<<29 | p6<<15 | p8 (sem máscara) */

    b &= SLOT_2_0;
    b <<= 8;
    a |= b;

    s <<= 4;
    b = b_at(4); /* p[-4] com p apontando para o byte 8 */
    b &= 0x7f;
    b >>= 3;
    s |= b;

    (9, ((s as u64) << 32) | a as u64)
}

/// `sqlite3GetVarint32` (a função do C, que supõe que o caso de um byte já foi tratado por
/// [`get_varint32`]): lê um varint de 32 bits. Se o varint não cabe em 32 bits sem sinal,
/// o valor é `0xffffffff`. Devolve `(bytes lidos, valor)`.
pub fn get_varint32_fn(p: &[u8]) -> (u8, u32) {
    debug_assert!(at(p, 0) & 0x80 != 0);

    if at(p, 1) & 0x80 == 0 {
        /* caso de dois bytes */
        return (2, (((at(p, 0) & 0x7f) as u32) << 7) | at(p, 1) as u32);
    }
    if at(p, 2) & 0x80 == 0 {
        /* caso de três bytes */
        return (
            3,
            (((at(p, 0) & 0x7f) as u32) << 14)
                | (((at(p, 1) & 0x7f) as u32) << 7)
                | at(p, 2) as u32,
        );
    }
    /* quatro ou mais bytes */
    let (n, v64) = get_varint(p);
    debug_assert!(n > 3 && n <= 9);
    if (v64 & SQLITE_MAX_U32) != v64 {
        (n, 0xffffffff)
    } else {
        (n, v64 as u32)
    }
}

/// Macro `getVarint32(A,B)` do sqliteInt.h: trata o caso de um byte sem chamar a função.
/// Devolve `(bytes lidos, valor)`.
#[inline]
pub fn get_varint32(p: &[u8]) -> (u8, u32) {
    let b0 = at(p, 0);
    if b0 < 0x80 {
        (1, b0 as u32)
    } else {
        get_varint32_fn(p)
    }
}

/// Macro `getVarint32NR(A,B)` do sqliteInt.h: só o valor, sem o tamanho.
#[inline]
pub fn get_varint32_nr(p: &[u8]) -> u32 {
    let b0 = at(p, 0) as u32;
    if b0 >= 0x80 {
        get_varint32_fn(p).1
    } else {
        b0
    }
}

/// Macro `putVarint32(A,B)` do sqliteInt.h: trata o caso de um byte sem chamar a função.
#[inline]
pub fn put_varint32(p: &mut [u8], v: u32) -> u8 {
    if v < 0x80 {
        p[0] = v as u8;
        1
    } else {
        put_varint(p, v as u64) as u8
    }
}

/// `sqlite3VarintLen`: quantos bytes `v` ocupa como varint.
pub fn varint_len(v: u64) -> i32 {
    let mut v = v;
    let mut i = 1;
    loop {
        v >>= 7;
        if v == 0 {
            break;
        }
        debug_assert!(i < 10);
        i += 1;
    }
    i
}

/// `sqlite3Get4byte`: inteiro de quatro bytes em ordem big-endian, lido de `p[0..4]`.
#[inline]
pub fn get4byte(p: &[u8]) -> u32 {
    u32::from_be_bytes([p[0], p[1], p[2], p[3]])
}

/// `sqlite3Put4byte`: grava `v` em quatro bytes big-endian em `p[0..4]`.
#[inline]
pub fn put4byte(p: &mut [u8], v: u32) {
    p[..4].copy_from_slice(&v.to_be_bytes());
}

/// Macro `get2byte(x)` do btreeInt.h: inteiro de dois bytes big-endian em `p[0..2]`.
#[inline]
pub fn get2byte(p: &[u8]) -> u32 {
    ((p[0] as u32) << 8) | p[1] as u32
}

/// Macro `put2byte(p,v)` do btreeInt.h: grava os 16 bits baixos de `v` em `p[0..2]`.
#[inline]
pub fn put2byte(p: &mut [u8], v: u32) {
    p[0] = (v >> 8) as u8;
    p[1] = v as u8;
}

// ---------------------------------------------------------------------------------------------
// BLOB literal
// ---------------------------------------------------------------------------------------------

/// `sqlite3HexToBlob`: converte os dígitos hexadecimais de um literal `x'hhhh'` no valor binário.
/// No C o espaço vem de `sqlite3DbMallocRawNN(db, n/2+1)`; aqui o `Vec` tem `n/2+1` bytes, os
/// bytes decodificados seguidos de um NUL.
pub fn hex_to_blob(z: &[u8], n: i32) -> Vec<u8> {
    let mut z_blob = vec![0u8; ((n / 2) + 1).max(1) as usize];
    let n = n - 1;
    let mut i: i32 = 0;
    while i < n {
        z_blob[(i / 2) as usize] = (hex_to_int(at(z, i as usize) as i32) << 4)
            | hex_to_int(at(z, (i + 1) as usize) as i32);
        i += 2;
    }
    z_blob[(i / 2) as usize] = 0;
    z_blob
}

// ---------------------------------------------------------------------------------------------
// Aritmética de 64 bits com detecção de estouro
// ---------------------------------------------------------------------------------------------
//
// O Debian compila com GCC, que usa `__builtin_*_overflow`: o resultado com volta (wrapping)
// é gravado em `*pA` mesmo quando há estouro, e o retorno é 1.

/// `sqlite3AddInt64`: `*a += b`. Devolve 0 em sucesso ou 1 se estourou.
pub fn add_int64(a: &mut i64, b: i64) -> i32 {
    let (r, o) = a.overflowing_add(b);
    *a = r;
    o as i32
}

/// `sqlite3SubInt64`: `*a -= b`. Devolve 0 em sucesso ou 1 se estourou.
pub fn sub_int64(a: &mut i64, b: i64) -> i32 {
    let (r, o) = a.overflowing_sub(b);
    *a = r;
    o as i32
}

/// `sqlite3MulInt64`: `*a *= b`. Devolve 0 em sucesso ou 1 se estourou.
pub fn mul_int64(a: &mut i64, b: i64) -> i32 {
    let (r, o) = a.overflowing_mul(b);
    *a = r;
    o as i32
}

/// `sqlite3AbsInt32`: valor absoluto, ou `2147483647` para `-2147483648`.
pub fn abs_int32(x: i32) -> i32 {
    if x >= 0 {
        return x;
    }
    if x == i32::MIN {
        return 0x7fffffff;
    }
    -x
}

// ---------------------------------------------------------------------------------------------
// LogEst
// ---------------------------------------------------------------------------------------------

/// `sqlite3LogEstAdd`: soma aproximada de dois valores LogEst (a escala é logarítmica).
pub fn log_est_add(a: LogEst, b: LogEst) -> LogEst {
    const X: [u8; 32] = [
        10, 10, /* 0,1 */
        9, 9, /* 2,3 */
        8, 8, /* 4,5 */
        7, 7, 7, /* 6,7,8 */
        6, 6, 6, /* 9,10,11 */
        5, 5, 5, /* 12-14 */
        4, 4, 4, 4, /* 15-18 */
        3, 3, 3, 3, 3, 3, /* 19-24 */
        2, 2, 2, 2, 2, 2, 2, /* 25-31 */
    ];
    let (ia, ib) = (a as i32, b as i32);
    if ia >= ib {
        if ia > ib + 49 {
            return a;
        }
        if ia > ib + 31 {
            return (ia + 1) as LogEst;
        }
        (ia + X[(ia - ib) as usize] as i32) as LogEst
    } else {
        if ib > ia + 49 {
            return b;
        }
        if ib > ia + 31 {
            return (ib + 1) as LogEst;
        }
        (ib + X[(ib - ia) as usize] as i32) as LogEst
    }
}

/// `sqlite3LogEst`: converte um inteiro em LogEst (aproximação de `10*log2(x)`).
pub fn log_est(x: u64) -> LogEst {
    const A: [i32; 8] = [0, 2, 3, 5, 6, 7, 8, 9];
    let mut x = x;
    let mut y: i32 = 40;
    if x < 8 {
        if x < 2 {
            return 0;
        }
        while x < 8 {
            y -= 10;
            x <<= 1;
        }
    } else {
        let i = 60 - x.leading_zeros() as i32;
        y += i * 10;
        x >>= i;
    }
    (A[(x & 7) as usize] + y - 10) as LogEst
}

/// `sqlite3LogEstFromDouble`: converte um `double` em LogEst.
pub fn log_est_from_double(x: f64) -> LogEst {
    if x <= 1.0 {
        return 0;
    }
    if x <= 2000000000.0 {
        return log_est(x as u64);
    }
    let a = x.to_bits();
    let e = ((a >> 52) as i64 - 1022) as LogEst;
    ((e as i32) * 10) as LogEst
}

/// `sqlite3LogEstToInt`: converte um LogEst em inteiro.
pub fn log_est_to_int(x: LogEst) -> u64 {
    let mut x = x as i32;
    let mut n: u64 = (x % 10) as i64 as u64;
    x /= 10;
    if n >= 5 {
        n = n.wrapping_sub(2);
    } else if n >= 1 {
        n = n.wrapping_sub(1);
    }
    if x > 60 {
        return LARGEST_INT64 as u64;
    }
    // Os deslocamentos mascaram a contagem a 6 bits, como o x86 (o C não define o excesso).
    if x >= 3 {
        n.wrapping_add(8).wrapping_shl((x - 3) as u32)
    } else {
        n.wrapping_add(8).wrapping_shr((3 - x) as u32)
    }
}

// ---------------------------------------------------------------------------------------------
// VList
// ---------------------------------------------------------------------------------------------
//
// Um VList é um vetor de inteiros: `[0]` é o número de inteiros alocados, `[1]` o número usado,
// e cada par nome/número ocupa 3 ou mais inteiros: o valor, o tamanho do par (em inteiros) e o
// nome (terminado em NUL) sobreposto aos inteiros seguintes. Aqui é um `Vec<i32>` cujo
// comprimento é `[0]`; os bytes do nome ficam em ordem little-endian dentro dos inteiros.

/// Grava o byte `b` na posição `k` da região que começa no inteiro `base`.
fn vlist_set_byte(v: &mut [i32], base: usize, k: usize, b: u8) {
    let slot = base + k / 4;
    let shift = (k % 4) * 8;
    let old = v[slot] as u32;
    v[slot] = ((old & !(0xffu32 << shift)) | ((b as u32) << shift)) as i32;
}

/// Lê o nome (sem o NUL) que começa no inteiro `base`.
fn vlist_read_name(v: &[i32], base: usize) -> Vec<u8> {
    let mut out = Vec::new();
    let mut k = 0usize;
    loop {
        let slot = base + k / 4;
        if slot >= v.len() {
            break;
        }
        let b = ((v[slot] as u32) >> ((k % 4) * 8)) as u8;
        if b == 0 {
            break;
        }
        out.push(b);
        k += 1;
    }
    out
}

/// `sqlite3VListAdd`: acrescenta o par nome/número a um VList (`None` é o VList nulo) e
/// devolve o VList resultante. Não há falha de alocação aqui (o C devolveria o VList original).
pub fn vlist_add(p_in: Option<Vec<i32>>, z_name: &[u8], n_name: i32, i_val: i32) -> Vec<i32> {
    let n_int = n_name / 4 + 3;
    let none = p_in.is_none();
    let mut v = p_in.unwrap_or_default();
    debug_assert!(none || v[0] >= 3); /* verifica que é possível acrescentar elementos */
    if none || v[1] + n_int > v[0] {
        /* aumenta a alocação */
        let n_alloc: i64 = (if none { 10 } else { 2 * (v[0] as i64) }) + n_int as i64;
        v.resize(n_alloc as usize, 0);
        if none {
            v[1] = 2;
        }
        v[0] = n_alloc as i32;
    }
    let i = v[1] as usize;
    v[i] = i_val;
    v[i + 1] = n_int;
    v[1] = i as i32 + n_int;
    debug_assert!(v[1] <= v[0]);
    for k in 0..n_name.max(0) as usize {
        vlist_set_byte(&mut v, i + 2, k, at(z_name, k));
    }
    vlist_set_byte(&mut v, i + 2, n_name.max(0) as usize, 0);
    v
}

/// `sqlite3VListNumToName`: o nome da variável do VList que tem o valor `i_val`, se houver.
pub fn vlist_num_to_name(p_in: &[i32], i_val: i32) -> Option<Vec<u8>> {
    if p_in.is_empty() {
        return None;
    }
    let mx = p_in[1];
    let mut i = 2i32;
    loop {
        if p_in[i as usize] == i_val {
            return Some(vlist_read_name(p_in, i as usize + 2));
        }
        i += p_in[i as usize + 1];
        if i >= mx {
            break;
        }
    }
    None
}

/// `sqlite3VListNameToNum`: o número da variável chamada `z_name` (`n_name` bytes), ou `0`.
pub fn vlist_name_to_num(p_in: &[i32], z_name: &[u8], n_name: i32) -> i32 {
    if p_in.is_empty() {
        return 0;
    }
    let mx = p_in[1];
    let mut i = 2i32;
    loop {
        let z = vlist_read_name(p_in, i as usize + 2);
        if z.len() == n_name.max(0) as usize && z[..] == z_name[..z.len().min(z_name.len())] {
            return p_in[i as usize];
        }
        i += p_in[i as usize + 1];
        if i >= mx {
            break;
        }
    }
    0
}

// ---------------------------------------------------------------------------------------------
// Mensagens de erro (main.c)
// ---------------------------------------------------------------------------------------------

/// `sqlite3ErrStr`: texto estático que descreve o tipo de erro `rc`.
pub fn err_str(rc: i32) -> &'static str {
    const A_MSG: [Option<&str>; 29] = [
        /* SQLITE_OK          */ Some("not an error"),
        /* SQLITE_ERROR       */ Some("SQL logic error"),
        /* SQLITE_INTERNAL    */ None,
        /* SQLITE_PERM        */ Some("access permission denied"),
        /* SQLITE_ABORT       */ Some("query aborted"),
        /* SQLITE_BUSY        */ Some("database is locked"),
        /* SQLITE_LOCKED      */ Some("database table is locked"),
        /* SQLITE_NOMEM       */ Some("out of memory"),
        /* SQLITE_READONLY    */ Some("attempt to write a readonly database"),
        /* SQLITE_INTERRUPT   */ Some("interrupted"),
        /* SQLITE_IOERR       */ Some("disk I/O error"),
        /* SQLITE_CORRUPT     */ Some("database disk image is malformed"),
        /* SQLITE_NOTFOUND    */ Some("unknown operation"),
        /* SQLITE_FULL        */ Some("database or disk is full"),
        /* SQLITE_CANTOPEN    */ Some("unable to open database file"),
        /* SQLITE_PROTOCOL    */ Some("locking protocol"),
        /* SQLITE_EMPTY       */ None,
        /* SQLITE_SCHEMA      */ Some("database schema has changed"),
        /* SQLITE_TOOBIG      */ Some("string or blob too big"),
        /* SQLITE_CONSTRAINT  */ Some("constraint failed"),
        /* SQLITE_MISMATCH    */ Some("datatype mismatch"),
        /* SQLITE_MISUSE      */ Some("bad parameter or other API misuse"),
        /* SQLITE_NOLFS       */ None,
        /* SQLITE_AUTH        */ Some("authorization denied"),
        /* SQLITE_FORMAT      */ None,
        /* SQLITE_RANGE       */ Some("column index out of range"),
        /* SQLITE_NOTADB      */ Some("file is not a database"),
        /* SQLITE_NOTICE      */ Some("notification message"),
        /* SQLITE_WARNING     */ Some("warning message"),
    ];
    let mut z_err = "unknown error";
    match rc {
        SQLITE_ABORT_ROLLBACK => {
            z_err = "abort due to ROLLBACK";
        }
        SQLITE_ROW => {
            z_err = "another row available";
        }
        SQLITE_DONE => {
            z_err = "no more rows available";
        }
        _ => {
            let rc = rc & 0xff;
            if rc >= 0 && (rc as usize) < A_MSG.len() {
                if let Some(m) = A_MSG[rc as usize] {
                    z_err = m;
                }
            }
        }
    }
    z_err
}

#[cfg(test)]
mod tests {
    use super::*;

    fn num(s: &str) -> (i32, f64) {
        atof(s.as_bytes(), s.len() as i32, SQLITE_UTF8 as u8, true)
    }

    #[test]
    fn f80_constants_are_exact_for_small_powers() {
        let c = ld_consts();
        assert_eq!(c.p7, F80::from_u64(10_000_000));
        assert_eq!(c.p17, F80::from_u64(100_000_000_000_000_000));
        assert_eq!(c.p19, F80::from_u64(10_000_000_000_000_000_000));
        assert_eq!(c.p1.mul(&c.p1), F80::from_u64(100));
    }

    #[test]
    fn atof_basic() {
        assert_eq!(num("1"), (1, 1.0));
        assert_eq!(num("1.5"), (2, 1.5));
        assert_eq!(num("0.1"), (2, 0.1));
        assert_eq!(num("1e23"), (2, 1e23));
        assert_eq!(num("  -2.5e2  "), (3, -250.0));
        assert_eq!(num("abc").0, 0);
        assert_eq!(num("1e999"), (2, f64::INFINITY));
    }

    #[test]
    fn atoi64_basic() {
        let mut v = 0i64;
        assert_eq!(atoi64(b"123", &mut v, 3, SQLITE_UTF8 as u8), 0);
        assert_eq!(v, 123);
        assert_eq!(atoi64(b"-9223372036854775808", &mut v, 20, SQLITE_UTF8 as u8), 0);
        assert_eq!(v, i64::MIN);
        assert_eq!(atoi64(b"9223372036854775808", &mut v, 19, SQLITE_UTF8 as u8), 3);
        assert_eq!(atoi64(b"9223372036854775809", &mut v, 19, SQLITE_UTF8 as u8), 2);
        assert_eq!(atoi64(b"12x", &mut v, 3, SQLITE_UTF8 as u8), 1);
        assert_eq!(atoi64(b"x", &mut v, 1, SQLITE_UTF8 as u8), -1);
    }

    #[test]
    fn int32_parsers() {
        assert_eq!(get_int32(b"2147483647"), Some(2147483647));
        assert_eq!(get_int32(b"-2147483648"), Some(i32::MIN));
        assert_eq!(get_int32(b"2147483648"), None);
        assert_eq!(get_int32(b"0x7fffffff"), Some(0x7fffffff));
        assert_eq!(get_int32(b"0x80000000"), None);
        assert_eq!(atoi(b"42abc"), 42);
        assert_eq!(get_u_int32(b"4294967295"), Some(u32::MAX));
        assert_eq!(get_u_int32(b"12a"), None);
    }

    #[test]
    fn varint_round_trip() {
        let mut buf = [0u8; 10];
        for &v in &[0u64, 1, 0x7f, 0x80, 0x3fff, 0x4000, 0x1fffff, 1 << 35, 1 << 56, u64::MAX, 1 << 63] {
            let n = put_varint(&mut buf, v);
            if v < (1u64 << 63) {
                // O C devolve 10 em varint_len para 64 bits cheios, embora a escrita use 9 bytes.
                assert_eq!(n, varint_len(v));
            }
            let (m, w) = get_varint(&buf);
            assert_eq!(m as i32, n);
            assert_eq!(w, v);
        }
    }

    #[test]
    fn fp_decode_digits() {
        let d = fp_decode(1.0, 17, 26, true);
        assert_eq!(d.sign, b'+');
        assert_eq!(&d.digits()[..d.n as usize], b"1");
        assert_eq!(d.i_dp, 1);
        let d = fp_decode(0.1, 15, 26, true);
        assert_eq!(&d.digits()[..d.n as usize], b"1");
        assert_eq!(d.i_dp, 0);
        let d = fp_decode(-123.456, 15, 26, true);
        assert_eq!(d.sign, b'-');
        assert_eq!(&d.digits()[..d.n as usize], b"123456");
        assert_eq!(d.i_dp, 3);
    }

    #[test]
    fn misc() {
        assert_eq!(log_est(10), 33);
        assert_eq!(log_est(1), 0);
        assert_eq!(str_icmp(b"AbC", b"aBc"), 0);
        assert!(str_icmp(b"abc", b"abd") < 0);
        assert_eq!(err_str(0), "not an error");
        assert_eq!(err_str(101), "no more rows available");
        assert_eq!(err_str(5 | (1 << 8)), "database is locked");
        let mut z = *b"\"a\"\"b\"\0";
        dequote(&mut z);
        assert_eq!(&z[..3], b"a\"b");
        assert_eq!(z[3], 0);
        let mut out = [0u8; 22];
        let n = int64_to_text(i64::MIN, &mut out);
        assert_eq!(&out[..n as usize], b"-9223372036854775808");
    }
}
