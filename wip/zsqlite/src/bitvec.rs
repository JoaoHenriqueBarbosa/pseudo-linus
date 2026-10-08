//! Bitmap de tamanho fixo (bitvec.c do SQLite 3.46.1).
//!
//! Registra, por exemplo, quais páginas já foram gravadas no journal numa transação. Os bits
//! são numerados a partir de 1. Há três representações, como no C (a união `u` do struct):
//!
//! - `iSize <= BITVEC_NBIT`: um mapa de bits direto (`Bitmap`);
//! - `iSize > BITVEC_NBIT` e `iDivisor == 0`: uma tabela hash de até `BITVEC_MXHASH` valores
//!   distintos (`Hash`);
//! - caso contrário, `BITVEC_NPTR` sub-bitmaps, cada um cuidando de `iDivisor` valores
//!   (`Sub`).
//!
//! A posse é por `Box`/`Vec`: o que o C chama de `Bitvec*` é `Option<Box<Bitvec>>`.

use crate::consts::SQLITE_OK;

/// Tamanho da estrutura `Bitvec` no C, em bytes.
const BITVEC_SZ: usize = 512;
/// Tamanho da união, arredondado para baixo ao múltiplo do ponteiro (8 bytes):
/// `((512 - 3*sizeof(u32)) / sizeof(Bitvec*)) * sizeof(Bitvec*)`.
const BITVEC_USIZE: usize = ((BITVEC_SZ - 3 * 4) / 8) * 8;
/// Número de elementos (bytes) do mapa de bits.
const BITVEC_NELEM: usize = BITVEC_USIZE;
/// Bits por elemento do mapa.
const BITVEC_SZELEM: u32 = 8;
/// Número de bits do mapa.
const BITVEC_NBIT: u32 = (BITVEC_NELEM as u32) * BITVEC_SZELEM;
/// Número de valores `u32` da tabela hash.
const BITVEC_NINT: usize = BITVEC_USIZE / 4;
/// Máximo de entradas da tabela hash antes de subdividir e refazer o hash.
const BITVEC_MXHASH: u32 = (BITVEC_NINT / 2) as u32;
/// Número de sub-bitmaps.
const BITVEC_NPTR: usize = BITVEC_USIZE / 8;

/// `BITVEC_HASH(X)`: `(X*1) % BITVEC_NINT`.
#[inline]
fn bitvec_hash(x: u32) -> usize {
    x as usize % BITVEC_NINT
}

/// A união `u` do C.
enum Rep {
    /// `aBitmap[BITVEC_NELEM]`.
    Bitmap(Vec<u8>),
    /// `aHash[BITVEC_NINT]`.
    Hash(Vec<u32>),
    /// `apSub[BITVEC_NPTR]`.
    Sub(Vec<Option<Box<Bitvec>>>),
}

pub struct Bitvec {
    /// Índice máximo de bit.
    i_size: u32,
    /// Número de bits ligados; só vale para a representação hash.
    n_set: u32,
    /// Quantos bits cada sub-bitmap cuida.
    i_divisor: u32,
    u: Rep,
}

/// `sqlite3BitvecCreate`: cria um bitmap para bits entre 1 e `i_size`, todos limpos.
pub fn bitvec_create(i_size: u32) -> Box<Bitvec> {
    // O C zera a união inteira; a representação inicial depende só de iSize.
    let u = if i_size <= BITVEC_NBIT {
        Rep::Bitmap(vec![0; BITVEC_NELEM])
    } else {
        Rep::Hash(vec![0; BITVEC_NINT])
    };
    Box::new(Bitvec { i_size, n_set: 0, i_divisor: 0, u })
}

/// `sqlite3BitvecTestNotNull`: o bit `i` está ligado? Falso se `i` estiver fora da faixa.
pub fn bitvec_test_not_null(p: &Bitvec, i: u32) -> bool {
    let mut p = p;
    let mut i = i.wrapping_sub(1);
    if i >= p.i_size {
        return false;
    }
    while p.i_divisor != 0 {
        let bin = (i / p.i_divisor) as usize;
        i %= p.i_divisor;
        let Rep::Sub(subs) = &p.u else { return false };
        match subs[bin].as_deref() {
            Some(sub) => p = sub,
            None => return false,
        }
    }
    if p.i_size <= BITVEC_NBIT {
        let Rep::Bitmap(bits) = &p.u else { return false };
        (bits[(i / BITVEC_SZELEM) as usize] & (1 << (i & (BITVEC_SZELEM - 1)))) != 0
    } else {
        let Rep::Hash(hash) = &p.u else { return false };
        let mut h = bitvec_hash(i);
        i += 1;
        while hash[h] != 0 {
            if hash[h] == i {
                return true;
            }
            h = (h + 1) % BITVEC_NINT;
        }
        false
    }
}

/// `sqlite3BitvecTest`: como o anterior, mas aceita bitmap ainda não criado (falso).
pub fn bitvec_test(p: Option<&Bitvec>, i: u32) -> bool {
    p.is_some_and(|p| bitvec_test_not_null(p, i))
}

/// `sqlite3BitvecSet`: liga o bit `i`. Devolve 0 (`SQLITE_OK`) em sucesso. Quem chama garante
/// `0 < i <= iSize`. Pode alocar sub-bitmaps.
pub fn bitvec_set(p: Option<&mut Bitvec>, i: u32) -> i32 {
    let Some(mut p) = p else { return SQLITE_OK };
    debug_assert!(i > 0);
    debug_assert!(i <= p.i_size);
    let mut i = i - 1;
    while p.i_size > BITVEC_NBIT && p.i_divisor != 0 {
        let div = p.i_divisor;
        let bin = (i / div) as usize;
        i %= div;
        let Rep::Sub(subs) = &mut p.u else { return SQLITE_OK };
        if subs[bin].is_none() {
            subs[bin] = Some(bitvec_create(div));
        }
        match subs[bin].as_deref_mut() {
            Some(sub) => p = sub,
            None => return SQLITE_OK,
        }
    }
    if p.i_size <= BITVEC_NBIT {
        let Rep::Bitmap(bits) = &mut p.u else { return SQLITE_OK };
        bits[(i / BITVEC_SZELEM) as usize] |= 1 << (i & (BITVEC_SZELEM - 1));
        return SQLITE_OK;
    }
    let mut h = bitvec_hash(i);
    i += 1;
    let Rep::Hash(hash) = &mut p.u else { return SQLITE_OK };
    // Se não houve colisão e a tabela não fica totalmente cheia, só acrescenta, sem
    // subdividir nem refazer o hash (bitvec_set_end).
    let mut set_end = false;
    if hash[h] == 0 {
        if p.n_set < (BITVEC_NINT - 1) as u32 {
            set_end = true;
        }
    } else {
        // Houve colisão: confere se já está na tabela; se não, procura um lugar.
        loop {
            if hash[h] == i {
                return SQLITE_OK;
            }
            h += 1;
            if h >= BITVEC_NINT {
                h = 0;
            }
            if hash[h] == 0 {
                break;
            }
        }
    }
    // h aponta para o primeiro lugar livre. Confere se a tabela fica cheia demais.
    if !set_end && p.n_set >= BITVEC_MXHASH {
        let ai_values = hash.clone();
        p.u = Rep::Sub((0..BITVEC_NPTR).map(|_| None).collect());
        p.i_divisor = p.i_size.wrapping_add(BITVEC_NPTR as u32 - 1) / BITVEC_NPTR as u32;
        let mut rc = bitvec_set(Some(&mut *p), i);
        for &v in ai_values.iter() {
            if v != 0 {
                rc |= bitvec_set(Some(&mut *p), v);
            }
        }
        return rc;
    }
    p.n_set += 1;
    hash[h] = i;
    SQLITE_OK
}

/// `sqlite3BitvecClear`: desliga o bit `i`. O `pBuf` do C (espaço temporário para refazer a
/// tabela) vira uma cópia local.
pub fn bitvec_clear(p: Option<&mut Bitvec>, i: u32) {
    let Some(mut p) = p else { return };
    debug_assert!(i > 0);
    let mut i = i - 1;
    while p.i_divisor != 0 {
        let bin = (i / p.i_divisor) as usize;
        i %= p.i_divisor;
        let Rep::Sub(subs) = &mut p.u else { return };
        match subs[bin].as_deref_mut() {
            Some(sub) => p = sub,
            None => return,
        }
    }
    if p.i_size <= BITVEC_NBIT {
        let Rep::Bitmap(bits) = &mut p.u else { return };
        bits[(i / BITVEC_SZELEM) as usize] &= !(1 << (i & (BITVEC_SZELEM - 1)));
    } else {
        let Rep::Hash(hash) = &mut p.u else { return };
        let ai_values = hash.clone();
        hash.fill(0);
        p.n_set = 0;
        for &v in ai_values.iter() {
            if v != 0 && v != i + 1 {
                let mut h = bitvec_hash(v - 1);
                p.n_set += 1;
                while hash[h] != 0 {
                    h += 1;
                    if h >= BITVEC_NINT {
                        h = 0;
                    }
                }
                hash[h] = v;
            }
        }
    }
}

/// `sqlite3BitvecDestroy`: libera o bitmap e os sub-bitmaps (o `Drop` recursivo do `Box` faz
/// o trabalho que o C faz à mão).
pub fn bitvec_destroy(p: Option<Box<Bitvec>>) {
    drop(p);
}

/// `sqlite3BitvecSize`: o `iSize` com que o bitmap foi criado.
pub fn bitvec_size(p: &Bitvec) -> u32 {
    p.i_size
}

/// `sqlite3BitvecBuiltinTest`: teste extenso do Bitvec (usado por
/// `sqlite3_test_control(SQLITE_TESTCTRL_BITVEC_TEST)`). `a_op` é um programa de inteiros
/// (opcode seguido de 0, 1 ou 3 operandos; 0 encerra) e é alterado durante a execução, como
/// no C. `randomness` é o `sqlite3_randomness` (preenche o buffer dado com bytes aleatórios).
/// Devolve o número de erros, ou -1 se faltar memória.
pub fn bitvec_builtin_test(sz: i32, a_op: &mut [i32], randomness: &mut dyn FnMut(&mut [u8])) -> i32 {
    let mut p_bitvec = Some(bitvec_create(sz as u32));
    let mut p_v: Vec<u8> = vec![0; ((sz + 7) / 8 + 1) as usize];
    let mut rc: i32;
    let mut pc: usize = 0;
    let mut i: i32 = 0;

    // Testes com bitmap nulo.
    bitvec_set(None, 1);
    bitvec_clear(None, 1);

    // Roda o programa.
    loop {
        let op = a_op[pc];
        if op == 0 {
            break;
        }
        let mut nx: usize;
        match op {
            1 | 2 | 5 => {
                nx = 4;
                i = a_op[pc + 2] - 1;
                a_op[pc + 2] += a_op[pc + 3];
            }
            _ => {
                nx = 2;
                let mut buf = [0u8; 4];
                randomness(&mut buf);
                i = i32::from_ne_bytes(buf);
            }
        }
        a_op[pc + 1] -= 1;
        if a_op[pc + 1] > 0 {
            nx = 0;
        }
        pc += nx;
        i = (i & 0x7fffffff) % sz;
        let bit = (i + 1) as usize;
        if (op & 1) != 0 {
            p_v[bit >> 3] |= 1 << (bit & 7);
            if op != 5 && bitvec_set(p_bitvec.as_deref_mut(), (i + 1) as u32) != 0 {
                return -1;
            }
        } else {
            p_v[bit >> 3] &= !(1 << (bit & 7));
            bitvec_clear(p_bitvec.as_deref_mut(), (i + 1) as u32);
        }
    }

    // Confere se o vetor linear bate exatamente com o Bitvec. Supõe que sim (rc == 0).
    let bv = p_bitvec.as_deref();
    rc = bitvec_test(None, 0) as i32
        + bitvec_test(bv, (sz + 1) as u32) as i32
        + bitvec_test(bv, 0) as i32
        + (bv.map_or(0, bitvec_size) as i32 - sz);
    for i in 1..=sz {
        let iu = i as usize;
        if ((p_v[iu >> 3] & (1 << (iu & 7))) != 0) != bitvec_test(bv, i as u32) {
            rc = i;
            break;
        }
    }
    bitvec_destroy(p_bitvec);
    rc
}
