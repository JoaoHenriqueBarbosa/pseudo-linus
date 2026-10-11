//! Registros do VDBE: tipos seriais, desserialização, comparação de registros e chaves de índice
//! (`vdbeaux.c`, a parte que só precisa de bytes).
//!
//! Convenções deste módulo:
//!
//! - A chave "serializada" do C (`int nKey1, const void *pKey1`) é um `&[u8]` com exatamente os
//!   `nKey1` bytes. As leituras do C que passam do fim do buffer (que o C permite por causa do
//!   preenchimento que o btree deixa depois do payload) leem zeros aqui ([`at`]): nunca há
//!   pânico por registro corrompido.
//! - `sqlite3VdbeSerialGet` copia o texto e o blob para dentro da célula (o C aponta para o
//!   registro, com `MEM_Ephem`); a flag `MEM_Ephem` continua sendo posta.
//! - O que o C marca com `CORRUPT_DB`/`assert` de depuração não existe.
//!
//! ADIADAS: `sqlite3VdbeSerialPut` e o `OP_MakeRecord` (no 3.46.1 a serialização está
//! incorporada ao opcode, não há função separada: fica para o `vdbe.c`), `sqlite3VdbeSerialType`
//! (`#if 0` no C), os invólucros de `sqlite3VdbeIdxRowid` e `sqlite3VdbeIdxKeyCompare` que leem
//! o payload de um cursor (aqui estão as partes puras: [`idx_rowid`], [`idx_key_check_cell_size`]
//! e [`idx_key_compare`], que recebem o payload já lido) e `sqlite3VdbeRecordCompareDebug`
//! (`SQLITE_DEBUG`).

use std::rc::Rc;

use crate::consts::{
    EXP754, KEYINFO_ORDER_BIGNULL, KEYINFO_ORDER_DESC, MAN754, MEM_BLOB, MEM_EPHEM, MEM_INT,
    MEM_INTREAL, MEM_NULL, MEM_REAL, MEM_STR, MEM_ZERO, SQLITE_CORRUPT, SQLITE_OK,
};
use crate::mem::{
    int_float_compare, is_all_zero, mem_set_null, memcmp, vdbe_compare_mem_string_parts, KeyInfo,
    Mem, UnpackedRecord,
};
use crate::util::{at, get_varint32, get_varint32_fn, get_varint32_nr, varint_len};

/// `RecordCompare` do vdbeInt.h: compara uma chave serializada com um registro desempacotado.
pub type RecordCompare = fn(&[u8], &mut UnpackedRecord) -> i32;

/// `sqlite3SmallTypeSizes`: o tamanho dos dados para os tipos seriais menores que 128.
pub const SMALL_TYPE_SIZES: [u8; 128] = [
    /*   0 */ 0, 1, 2, 3, 4, 6, 8, 8, 0, 0, /*  10 */ 0, 0, 0, 0, 1, 1, 2, 2, 3, 3,
    /*  20 */ 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, /*  30 */ 9, 9, 10, 10, 11, 11, 12, 12, 13, 13,
    /*  40 */ 14, 14, 15, 15, 16, 16, 17, 17, 18, 18, /*  50 */ 19, 19, 20, 20, 21, 21, 22, 22,
    23, 23, /*  60 */ 24, 24, 25, 25, 26, 26, 27, 27, 28, 28, /*  70 */ 29, 29, 30, 30, 31, 31,
    32, 32, 33, 33, /*  80 */ 34, 34, 35, 35, 36, 36, 37, 37, 38, 38, /*  90 */ 39, 39, 40, 40,
    41, 41, 42, 42, 43, 43, /* 100 */ 44, 44, 45, 45, 46, 46, 47, 47, 48, 48,
    /* 110 */ 49, 49, 50, 50, 51, 51, 52, 52, 53, 53, /* 120 */ 54, 54, 55, 55, 56, 56, 57, 57,
];

/// `sqlite3VdbeSerialTypeLen`: o tamanho dos dados do tipo serial dado.
pub fn serial_type_len(serial_type: u32) -> u32 {
    if serial_type >= 128 {
        (serial_type - 12) / 2
    } else {
        SMALL_TYPE_SIZES[serial_type as usize] as u32
    }
}

/// `sqlite3VdbeOneByteSerialTypeLen`.
pub fn one_byte_serial_type_len(serial_type: u8) -> u8 {
    debug_assert!(serial_type < 128);
    SMALL_TYPE_SIZES[serial_type as usize]
}

/// O sufixo de `buf` a partir de `k` (vazio se `k` passa do fim).
#[inline]
fn sub(buf: &[u8], k: usize) -> &[u8] {
    buf.get(k..).unwrap_or(&[])
}

/// `TWO_BYTE_INT(x)`.
#[inline]
fn two_byte_int(x: &[u8]) -> i32 {
    (256 * (at(x, 0) as i8 as i32)) | at(x, 1) as i32
}

/// `THREE_BYTE_INT(x)`.
#[inline]
fn three_byte_int(x: &[u8]) -> i32 {
    (65536 * (at(x, 0) as i8 as i32)) | ((at(x, 1) as i32) << 8) | at(x, 2) as i32
}

/// `FOUR_BYTE_UINT(x)`.
#[inline]
fn four_byte_uint(x: &[u8]) -> u32 {
    ((at(x, 0) as u32) << 24) | ((at(x, 1) as u32) << 16) | ((at(x, 2) as u32) << 8) | at(x, 3) as u32
}

/// `vdbeRecordDecodeInt`: o inteiro de um tipo serial 1 a 9 (menos o 7) a partir dos bytes
/// `a_key`.
pub fn record_decode_int(serial_type: u32, a_key: &[u8]) -> i64 {
    match serial_type {
        0 | 1 => at(a_key, 0) as i8 as i64,
        2 => two_byte_int(a_key) as i64,
        3 => three_byte_int(a_key) as i64,
        4 => four_byte_uint(a_key) as i32 as i64,
        5 => four_byte_uint(sub(a_key, 2)) as i64 + (1i64 << 32) * two_byte_int(a_key) as i64,
        6 => {
            let x = ((four_byte_uint(a_key) as u64) << 32) | four_byte_uint(sub(a_key, 4)) as u64;
            x as i64
        }
        _ => serial_type as i64 - 8,
    }
}

/// `IsNaN(X)` do sqliteInt.h sobre o padrão de bits de um `double`.
#[inline]
fn is_nan_bits(x: u64) -> bool {
    (x & EXP754) == EXP754 && (x & MAN754) != 0
}

/// `serialGet7`: lê um real IEEE de 8 bytes em `u_r`. Devolve 1 (e deixa a célula `NULL`) se
/// for NaN, 0 senão.
fn serial_get7(buf: &[u8], mem: &mut Mem) -> i32 {
    let x = ((four_byte_uint(buf) as u64) << 32) + four_byte_uint(sub(buf, 4)) as u64;
    mem.u_r = f64::from_bits(x);
    if is_nan_bits(x) {
        mem.flags = MEM_NULL;
        return 1;
    }
    mem.flags = MEM_REAL;
    0
}

/// `sqlite3VdbeSerialGet`: desserializa `buf` como o tipo serial `serial_type` e grava o
/// resultado em `mem`. `buf` começa no primeiro byte do campo e vai até o fim do registro.
/// Texto e blob são copiados para `mem.z` (com `MEM_Ephem`, como no C); se `buf` for mais curto
/// que o campo (registro corrompido, onde o C lê fora do buffer) só os bytes existentes são
/// levados e `n` é o que foi levado.
pub fn serial_get(buf: &[u8], serial_type: u32, mem: &mut Mem) {
    match serial_type {
        10 => {
            // Uso interno: NULL com a flag "sem mudança" do UPDATE de tabela virtual.
            mem.flags = MEM_NULL | MEM_ZERO;
            mem.n = 0;
            mem.n_zero = 0;
        }
        // 11 é reservado para uso futuro.
        11 | 0 => {
            mem.flags = MEM_NULL;
        }
        // Inteiros de 8, 16, 24, 32, 48 e 64 bits em complemento de dois big-endian, e as
        // constantes 0 e 1.
        1..=6 | 8 | 9 => {
            mem.u_i = record_decode_int(serial_type, buf);
            mem.flags = MEM_INT;
        }
        7 => {
            // Real IEEE 754 de 64 bits big-endian.
            serial_get7(buf, mem);
        }
        _ => {
            // BLOB de (N-12)/2 bytes (N par) ou texto de (N-13)/2 bytes (N ímpar).
            let want = ((serial_type - 12) / 2) as usize;
            let avail = want.min(buf.len());
            mem.z = buf[..avail].to_vec();
            mem.sz_malloc = 0;
            mem.n = avail as i32;
            mem.flags = if serial_type & 1 != 0 { MEM_STR | MEM_EPHEM } else { MEM_BLOB | MEM_EPHEM };
        }
    }
}

/// `sqlite3VdbeAllocUnpackedRecord`: um `UnpackedRecord` com espaço para os `nKeyField + 1`
/// valores que `sqlite3VdbeRecordUnpack` pode gravar.
pub fn alloc_unpacked_record(p_key_info: Rc<KeyInfo>) -> UnpackedRecord {
    debug_assert!(p_key_info.a_sort_flags.len() >= p_key_info.n_key_field as usize);
    let n = p_key_info.n_key_field as usize + 1;
    UnpackedRecord {
        a_mem: (0..n).map(|_| Mem::default()).collect(),
        n_field: n as u16,
        p_key_info,
        u_i: 0,
        n: 0,
        default_rc: 0,
        err_code: 0,
        r1: 0,
        r2: 0,
        eq_seen: 0,
    }
}

/// `sqlite3VdbeRecordUnpack`: separa o registro `key` em campos e os grava em `p.a_mem`,
/// ajustando `p.n_field` para o número de campos lidos.
pub fn record_unpack(p_key_info: &KeyInfo, key: &[u8], p: &mut UnpackedRecord) {
    let n_key = key.len() as u32;
    p.default_rc = 0;
    let (n0, sz_hdr) = get_varint32(key);
    let mut idx: u32 = n0 as u32; /* Deslocamento em key[] do próximo item do cabeçalho */
    let mut d: u32 = sz_hdr;
    let mut u: usize = 0;
    while idx < sz_hdr && d <= n_key {
        let (n, serial_type) = get_varint32(sub(key, idx as usize));
        idx += n as u32;
        let m = &mut p.a_mem[u];
        m.enc = p_key_info.enc;
        // O serial_get grava as flags.
        m.sz_malloc = 0;
        m.z = Vec::new();
        serial_get(sub(key, d as usize), serial_type, m);
        d = d.wrapping_add(serial_type_len(serial_type));
        u += 1;
        if u >= p.n_field as usize {
            break;
        }
    }
    if d > n_key && u != 0 {
        // Num registro corrompido o último valor pode ter vindo de memória fora do registro:
        // vira NULL.
        mem_set_null(&mut p.a_mem[u - 1]);
    }
    debug_assert!(u <= p_key_info.n_key_field as usize + 1);
    p.n_field = u as u16;
}

/// `vdbeRecordCompareWithSkip` (`sqlite3VdbeRecordCompareWithSkip`): compara a chave
/// serializada `a_key1` com o registro `p`. Devolve negativo, zero ou positivo se `a_key1` for
/// menor, igual ou maior. Com `b_skip` o chamador já sabe que o primeiro campo é igual e a
/// comparação começa no segundo. As chaves não precisam ter o mesmo número de campos: se tudo
/// o que existe nas duas for igual, devolve `p.default_rc`.
///
/// Em corrupção grava `SQLITE_CORRUPT` em `p.err_code` e devolve 0; em falta de memória
/// `SQLITE_NOMEM`.
pub fn record_compare_with_skip(a_key1: &[u8], p: &mut UnpackedRecord, b_skip: bool) -> i32 {
    let n_key1 = a_key1.len() as u32;
    let mut rc: i32 = 0; /* Valor de retorno */
    let mut p_rhs: usize = 0; /* Próximo campo de p a comparar */
    let mut mem1 = Mem::default();
    let mut idx1: u32; /* Deslocamento do primeiro tipo no cabeçalho */
    let sz_hdr1: u32; /* Tamanho do cabeçalho do registro */
    let mut d1: u32; /* Deslocamento do próximo dado em a_key1 */
    let mut i: usize; /* Índice do próximo campo a comparar */

    // Com b_skip os dois primeiros elementos já são iguais: as variáveis começam no segundo.
    if b_skip {
        let mut s1 = at(a_key1, 1) as u32;
        if s1 < 0x80 {
            idx1 = 2;
        } else {
            let (n, v) = get_varint32_fn(sub(a_key1, 1));
            s1 = v;
            idx1 = 1 + n as u32;
        }
        sz_hdr1 = at(a_key1, 0) as u32;
        d1 = sz_hdr1.wrapping_add(serial_type_len(s1));
        i = 1;
        p_rhs += 1;
    } else {
        let h = at(a_key1, 0) as u32;
        if h < 0x80 {
            sz_hdr1 = h;
            idx1 = 1;
        } else {
            let (n, v) = get_varint32_fn(a_key1);
            sz_hdr1 = v;
            idx1 = n as u32;
        }
        d1 = sz_hdr1;
        i = 0;
    }
    if d1 > n_key1 {
        p.err_code = SQLITE_CORRUPT as u8;
        return 0; /* Corrupção */
    }

    let mut serial_type: u32;
    loop {
        let ki: &KeyInfo = &p.p_key_info;
        let rhs: &Mem = &p.a_mem[p_rhs];
        let rhs_flags = rhs.flags;

        if rhs_flags & (MEM_INT | MEM_INTREAL) != 0 {
            // O lado direito é inteiro.
            serial_type = at(a_key1, idx1 as usize) as u32;
            if serial_type >= 10 {
                rc = if serial_type == 10 { -1 } else { 1 };
            } else if serial_type == 0 {
                rc = -1;
            } else if serial_type == 7 {
                serial_get7(sub(a_key1, d1 as usize), &mut mem1);
                rc = -int_float_compare(rhs.u_i, mem1.u_r);
            } else {
                let lhs = record_decode_int(serial_type, sub(a_key1, d1 as usize));
                let rhs_i = rhs.u_i;
                if lhs < rhs_i {
                    rc = -1;
                } else if lhs > rhs_i {
                    rc = 1;
                }
            }
        } else if rhs_flags & MEM_REAL != 0 {
            // O lado direito é real.
            serial_type = at(a_key1, idx1 as usize) as u32;
            if serial_type >= 10 {
                // Tipos 12 ou mais são texto e blob (maiores que números); 10 e 11 são
                // reservados e tanto faz o resultado contra números.
                rc = if serial_type == 10 { -1 } else { 1 };
            } else if serial_type == 0 {
                rc = -1;
            } else if serial_type == 7 {
                if serial_get7(sub(a_key1, d1 as usize), &mut mem1) != 0 {
                    rc = -1; /* mem1 é NaN */
                } else if mem1.u_r < rhs.u_r {
                    rc = -1;
                } else if mem1.u_r > rhs.u_r {
                    rc = 1;
                }
            } else {
                serial_get(sub(a_key1, d1 as usize), serial_type, &mut mem1);
                rc = int_float_compare(mem1.u_i, rhs.u_r);
            }
        } else if rhs_flags & MEM_STR != 0 {
            // O lado direito é texto.
            serial_type = get_varint32_nr(sub(a_key1, idx1 as usize));
            if serial_type < 12 {
                rc = -1;
            } else if serial_type & 0x01 == 0 {
                rc = 1;
            } else {
                mem1.n = ((serial_type - 12) / 2) as i32;
                if d1.wrapping_add(mem1.n as u32) > n_key1 || (ki.n_all_field as usize) <= i {
                    p.err_code = SQLITE_CORRUPT as u8;
                    return 0; /* Corrupção */
                }
                let z1 = &a_key1[d1 as usize..(d1 + mem1.n as u32) as usize];
                if let Some(coll) = ki.a_coll.get(i).and_then(|c| c.as_ref()) {
                    rc = vdbe_compare_mem_string_parts(ki.enc, z1, rhs, coll, Some(&mut p.err_code));
                } else {
                    rc = memcmp(z1, rhs.bytes());
                    if rc == 0 {
                        rc = mem1.n - rhs.n;
                    }
                }
            }
        } else if rhs_flags & MEM_BLOB != 0 {
            // O lado direito é blob.
            debug_assert!(rhs_flags & MEM_ZERO == 0 || rhs.n == 0);
            serial_type = get_varint32_nr(sub(a_key1, idx1 as usize));
            if serial_type < 12 || (serial_type & 0x01) != 0 {
                rc = -1;
            } else {
                let n_str = ((serial_type - 12) / 2) as i32;
                if d1.wrapping_add(n_str as u32) > n_key1 {
                    p.err_code = SQLITE_CORRUPT as u8;
                    return 0; /* Corrupção */
                }
                let z1 = &a_key1[d1 as usize..(d1 + n_str as u32) as usize];
                if rhs_flags & MEM_ZERO != 0 {
                    if !is_all_zero(z1) {
                        rc = 1;
                    } else {
                        rc = n_str - rhs.n_zero;
                    }
                } else {
                    rc = memcmp(z1, rhs.bytes());
                    if rc == 0 {
                        rc = n_str - rhs.n;
                    }
                }
            }
        } else {
            // O lado direito é NULL.
            serial_type = at(a_key1, idx1 as usize) as u32;
            if serial_type == 0
                || serial_type == 10
                || (serial_type == 7 && serial_get7(sub(a_key1, d1 as usize), &mut mem1) != 0)
            {
                debug_assert!(rc == 0);
            } else {
                rc = 1;
            }
        }

        if rc != 0 {
            let sort_flags = ki.a_sort_flags.get(i).copied().unwrap_or(0);
            if sort_flags != 0
                && ((sort_flags & KEYINFO_ORDER_BIGNULL) == 0
                    || ((sort_flags & KEYINFO_ORDER_DESC) != 0)
                        != (serial_type == 0 || (rhs_flags & MEM_NULL) != 0))
            {
                rc = -rc;
            }
            return rc;
        }

        i += 1;
        if i == p.n_field as usize {
            break;
        }
        p_rhs += 1;
        d1 = d1.wrapping_add(serial_type_len(serial_type));
        if d1 > n_key1 {
            break;
        }
        idx1 += varint_len(serial_type as u64) as u32;
        if idx1 >= sz_hdr1 {
            p.err_code = SQLITE_CORRUPT as u8;
            return 0; /* Índice corrompido */
        }
    }

    // rc==0 aqui: uma das chaves ou as duas acabaram os campos e tudo o que havia era igual.
    p.eq_seen = 1;
    p.default_rc as i32
}

/// `sqlite3VdbeRecordCompare`: [`record_compare_with_skip`] sem pular o primeiro campo.
pub fn record_compare(a_key1: &[u8], p: &mut UnpackedRecord) -> i32 {
    record_compare_with_skip(a_key1, p, false)
}

/// `vdbeRecordCompareInt`: versão otimizada de [`record_compare`] para quando (a) o primeiro
/// campo de `p` é inteiro e (b) o tamanho do cabeçalho de `a_key1` cabe num byte. Só é usada
/// em esquemas em que o cabeçalho válido tem no máximo 63 bytes.
fn vdbe_record_compare_int(a_key1: &[u8], p: &mut UnpackedRecord) -> i32 {
    let a_key = sub(a_key1, (at(a_key1, 0) & 0x3F) as usize);
    let serial_type = at(a_key1, 1) as u32;
    let lhs: i64 = match serial_type {
        1..=6 | 8 | 9 => record_decode_int(serial_type, a_key),
        // Os casos 0 e 7 (e os demais) caem no comparador genérico.
        _ => return record_compare(a_key1, p),
    };

    debug_assert!(p.u_i == p.a_mem[0].u_i);
    let v = p.u_i;
    let res: i32;
    if v > lhs {
        res = p.r1 as i32;
    } else if v < lhs {
        res = p.r2 as i32;
    } else if p.n_field > 1 {
        // Os primeiros campos são iguais: compara os seguintes.
        res = record_compare_with_skip(a_key1, p, true);
    } else {
        // Primeiros campos iguais e sem seguintes: default_rc.
        res = p.default_rc as i32;
        p.eq_seen = 1;
    }
    res
}

/// `vdbeRecordCompareString`: versão otimizada de [`record_compare`] para quando (a) o primeiro
/// campo de `p` é texto, (b) usa a colação BINARY e (c) o tamanho do cabeçalho de `a_key1` cabe
/// num byte.
fn vdbe_record_compare_string(a_key1: &[u8], p: &mut UnpackedRecord) -> i32 {
    debug_assert!(p.a_mem[0].flags & MEM_STR != 0);
    debug_assert!(p.a_mem[0].n == p.n);
    let mut serial_type = at(a_key1, 1) as i8 as i32;
    // vrcs_restart: o tipo serial tem mais de um byte (o byte lido como `signed char` é
    // negativo); relê como varint. O `goto` do C só repete o teste com o valor novo.
    if serial_type < 0 {
        let (_, v) = get_varint32_fn(sub(a_key1, 1));
        serial_type = v as i32;
    }

    let res: i32;
    if serial_type < 12 {
        res = p.r1 as i32; /* a_key1 é um número ou NULL */
    } else if serial_type & 0x01 == 0 {
        res = p.r2 as i32; /* a_key1 é um blob */
    } else {
        let sz_hdr = at(a_key1, 0) as i32;
        let n_str = (serial_type - 12) / 2;
        if sz_hdr + n_str > a_key1.len() as i32 {
            p.err_code = SQLITE_CORRUPT as u8;
            return 0; /* Corrupção */
        }
        let z1 = &a_key1[sz_hdr as usize..(sz_hdr + n_str) as usize];
        let c = memcmp(z1, p.a_mem[0].bytes());
        if c > 0 {
            res = p.r2 as i32;
        } else if c < 0 {
            res = p.r1 as i32;
        } else {
            let d = n_str - p.n;
            if d == 0 {
                if p.n_field > 1 {
                    res = record_compare_with_skip(a_key1, p, true);
                } else {
                    res = p.default_rc as i32;
                    p.eq_seen = 1;
                }
            } else if d > 0 {
                res = p.r2 as i32;
            } else {
                res = p.r1 as i32;
            }
        }
    }
    res
}

/// `sqlite3VdbeFindCompare`: escolhe a função de comparação de chaves serializadas mais
/// adequada ao registro `p`, ajustando `r1`, `r2`, `u_i` e `n` conforme o caso.
pub fn find_compare(p: &mut UnpackedRecord) -> RecordCompare {
    // vdbeRecordCompareInt e vdbeRecordCompareString supõem que o tamanho do cabeçalho cabe
    // em um byte; a primeira também supõe que ler um pouco além do buffer é seguro, e por isso
    // só vale para registros de 13 campos ou menos (cabeçalho de no máximo 12*5+1+1 bytes).
    if p.p_key_info.n_all_field <= 13 {
        let flags = p.a_mem[0].flags;
        let sort0 = p.p_key_info.a_sort_flags.first().copied().unwrap_or(0);
        if sort0 != 0 {
            if sort0 & KEYINFO_ORDER_BIGNULL != 0 {
                return record_compare;
            }
            p.r1 = 1;
            p.r2 = -1;
        } else {
            p.r1 = -1;
            p.r2 = 1;
        }
        if flags & MEM_INT != 0 {
            p.u_i = p.a_mem[0].u_i;
            return vdbe_record_compare_int;
        }
        if (flags & (MEM_REAL | MEM_INTREAL | MEM_NULL | MEM_BLOB)) == 0
            && p.p_key_info.a_coll.first().map_or(true, |c| c.is_none())
        {
            debug_assert!(flags & MEM_STR != 0);
            p.n = p.a_mem[0].n;
            return vdbe_record_compare_string;
        }
    }
    record_compare
}

/// A parte pura de `sqlite3VdbeIdxRowid`: `m` é a entrada de índice inteira (o payload do
/// cursor, já lido) feita pelo `OP_MakeRecord`, que pode vir de um arquivo corrompido. O rowid
/// é o último campo. Grava em `rowid` e devolve `SQLITE_OK`, ou `SQLITE_CORRUPT`.
pub fn idx_rowid(m: &[u8], rowid: &mut i64) -> i32 {
    let n = m.len() as u32;

    // A entrada começa com o tamanho do cabeçalho.
    let sz_hdr = get_varint32_nr(m);
    if sz_hdr < 3 || sz_hdr > n {
        return SQLITE_CORRUPT;
    }

    // O último campo do índice deve ser inteiro: o ROWID.
    let type_rowid = get_varint32_nr(sub(m, sz_hdr as usize - 1));
    if type_rowid < 1 || type_rowid > 9 || type_rowid == 7 {
        return SQLITE_CORRUPT;
    }
    let len_rowid = SMALL_TYPE_SIZES[type_rowid as usize] as u32;
    if n < sz_hdr + len_rowid {
        return SQLITE_CORRUPT;
    }

    // Busca o inteiro no fim do registro.
    let mut v = Mem::default();
    serial_get(sub(m, (n - len_rowid) as usize), type_rowid, &mut v);
    *rowid = v.u_i;
    SQLITE_OK
}

/// A validação que `sqlite3VdbeIdxKeyCompare` faz antes de ler o payload: o tamanho da entrada
/// (`sqlite3BtreePayloadSize`) tem que estar entre 1 e `0x7fffffff`. Se não estiver, grava 0 em
/// `res` e devolve `SQLITE_CORRUPT`; senão `SQLITE_OK`.
pub fn idx_key_check_cell_size(n_cell_key: i64, res: &mut i32) -> i32 {
    if n_cell_key <= 0 || n_cell_key > 0x7fffffff {
        *res = 0;
        return SQLITE_CORRUPT;
    }
    SQLITE_OK
}

/// A parte pura de `sqlite3VdbeIdxKeyCompare`: compara a entrada de índice `key` (o payload
/// inteiro do cursor, já lido) com `p`, que não tem o rowid do fim (ou foi truncado antes
/// dele); o rowid da entrada é ignorado. Grava o resultado em `res`.
pub fn idx_key_compare(key: &[u8], p: &mut UnpackedRecord, res: &mut i32) -> i32 {
    *res = record_compare_with_skip(key, p, false);
    SQLITE_OK
}

// ---------------------------------------------------------------------------------------------
// Testes
// ---------------------------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::consts::MEM_TERM;
    use crate::mem::{mem_set_int64, mem_set_str, StrDtor, ENC_UTF8};

    fn key_info(n: u16) -> Rc<KeyInfo> {
        Rc::new(KeyInfo {
            enc: ENC_UTF8,
            n_key_field: n,
            n_all_field: n,
            a_sort_flags: vec![0; n as usize],
            a_coll: vec![None; n as usize],
        })
    }

    fn int(v: i64) -> Mem {
        let mut m = Mem::default();
        mem_set_int64(&mut m, v);
        m
    }

    fn text(s: &[u8]) -> Mem {
        let mut m = Mem::default();
        mem_set_str(&mut m, Some(s), s.len() as i64, ENC_UTF8, StrDtor::Transient, 1_000_000_000);
        m
    }

    /// O registro (5, "ab"): cabeçalho de 3 bytes, tipos 1 e 17.
    const REC: [u8; 6] = [3, 1, 17, 5, b'a', b'b'];

    fn rhs(vals: Vec<Mem>) -> UnpackedRecord {
        let mut p = alloc_unpacked_record(key_info(vals.len() as u16));
        p.n_field = vals.len() as u16;
        for (i, v) in vals.into_iter().enumerate() {
            p.a_mem[i] = v;
        }
        p
    }

    #[test]
    fn serial_types() {
        assert_eq!(serial_type_len(0), 0);
        assert_eq!(serial_type_len(6), 8);
        assert_eq!(serial_type_len(17), 2);
        assert_eq!(serial_type_len(127), 57);
        assert_eq!(serial_type_len(128), 58);
        for t in 12..128u32 {
            assert_eq!(SMALL_TYPE_SIZES[t as usize] as u32, (t - 12) / 2);
        }
    }

    #[test]
    fn serial_get_values() {
        let mut m = Mem::default();
        serial_get(&[0xff], 1, &mut m);
        assert_eq!((m.flags, m.u_i), (MEM_INT, -1));
        serial_get(&[0xff, 0xfe], 2, &mut m);
        assert_eq!(m.u_i, -2);
        serial_get(&[0x80, 0, 0], 3, &mut m);
        assert_eq!(m.u_i, -8388608);
        serial_get(&[0xff, 0xff, 0xff, 0xff, 0xff, 0xfe], 5, &mut m);
        assert_eq!(m.u_i, -2);
        serial_get(&[0, 0, 0, 0, 0, 0, 0, 7], 6, &mut m);
        assert_eq!(m.u_i, 7);
        serial_get(&[], 9, &mut m);
        assert_eq!(m.u_i, 1);
        serial_get(&1.5f64.to_bits().to_be_bytes(), 7, &mut m);
        assert_eq!((m.flags, m.u_r), (MEM_REAL, 1.5));
        serial_get(&f64::NAN.to_bits().to_be_bytes(), 7, &mut m);
        assert_eq!(m.flags, MEM_NULL);
        serial_get(b"abc", 19, &mut m);
        assert_eq!(m.flags, MEM_STR | MEM_EPHEM);
        assert_eq!(m.bytes(), b"abc");
        serial_get(b"abc", 18, &mut m);
        assert_eq!(m.flags, MEM_BLOB | MEM_EPHEM);
        assert_eq!(m.bytes(), b"abc");
        serial_get(&[], 10, &mut m);
        assert_eq!(m.flags, MEM_NULL | MEM_ZERO);
    }

    #[test]
    fn unpack_record() {
        let ki = key_info(2);
        let mut p = alloc_unpacked_record(ki.clone());
        assert_eq!(p.n_field, 3);
        record_unpack(&ki, &REC, &mut p);
        assert_eq!(p.n_field, 2);
        assert_eq!(p.a_mem[0].flags, MEM_INT);
        assert_eq!(p.a_mem[0].u_i, 5);
        assert_eq!(p.a_mem[1].bytes(), b"ab");
        assert_eq!(p.a_mem[1].flags & MEM_TERM, 0);
    }

    #[test]
    fn compare_generic_and_fast() {
        for (vals, want) in [
            (vec![int(6), text(b"ab")], -1),
            (vec![int(5), text(b"ab")], 0),
            (vec![int(5), text(b"ac")], -1),
            (vec![int(5), text(b"aa")], 1),
            (vec![int(4), text(b"ab")], 1),
        ] {
            let mut p = rhs(vals);
            assert_eq!(record_compare(&REC, &mut p).signum(), want);
            let f = find_compare(&mut p);
            assert_eq!(f(&REC, &mut p).signum(), want);
            assert_eq!(p.err_code, 0);
        }
    }

    #[test]
    fn compare_string_first() {
        let key = [2u8, 17, b'a', b'b'];
        for (s, want) in [(&b"ab"[..], 0), (b"ac", -1), (b"aa", 1), (b"a", 1), (b"abc", -1)] {
            let mut p = rhs(vec![text(s)]);
            assert_eq!(record_compare(&key, &mut p).signum(), want);
            let f = find_compare(&mut p);
            assert_eq!(f(&key, &mut p).signum(), want);
        }
    }

    #[test]
    fn compare_nulls_and_sort_order() {
        // (NULL) contra NULL: igual; contra 1: menor.
        let key = [2u8, 0];
        let mut p = rhs(vec![Mem::value_new()]);
        assert_eq!(record_compare(&key, &mut p), 0);
        let mut p = rhs(vec![int(1)]);
        assert_eq!(record_compare(&key, &mut p), -1);
        // DESC inverte.
        let ki = Rc::new(KeyInfo {
            enc: ENC_UTF8,
            n_key_field: 1,
            n_all_field: 1,
            a_sort_flags: vec![KEYINFO_ORDER_DESC],
            a_coll: vec![None],
        });
        let mut p = alloc_unpacked_record(ki);
        p.n_field = 1;
        p.a_mem[0] = int(1);
        assert_eq!(record_compare(&key, &mut p), 1);
    }

    #[test]
    fn corrupt_record_sets_err() {
        let mut p = rhs(vec![text(b"ab")]);
        // O texto diz ter 2 bytes mas o registro acaba antes.
        assert_eq!(record_compare(&[2, 17, b'a'], &mut p), 0);
        assert_eq!(p.err_code, SQLITE_CORRUPT as u8);
    }

    #[test]
    fn idx_rowid_ok_and_corrupt() {
        let mut rowid = 0;
        assert_eq!(idx_rowid(&[3, 17, 1, b'a', b'b', 7], &mut rowid), SQLITE_OK);
        assert_eq!(rowid, 7);
        assert_eq!(idx_rowid(&[3, 17, 7, 0, 0, 0, 0, 0, 0, 0, 0], &mut rowid), SQLITE_CORRUPT);
        assert_eq!(idx_rowid(&[2, 1], &mut rowid), SQLITE_CORRUPT);
        let mut res = 5;
        assert_eq!(idx_key_check_cell_size(0, &mut res), SQLITE_CORRUPT);
        assert_eq!(res, 0);
        assert_eq!(idx_key_check_cell_size(10, &mut res), SQLITE_OK);
    }
}
