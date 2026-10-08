//! `fts5_varint.c`: serialização de varints do FTS5.
//!
//! O formato de `sqlite3Fts5PutVarint`/`sqlite3Fts5GetVarint` é bit a bit o do
//! `sqlite3PutVarint`/`sqlite3GetVarint` do núcleo (a fonte diz que são cópias), então o
//! código não se repete: os nomes do FTS5 são reexportações de `crate::util`:
//!
//! * `sqlite3Fts5PutVarint(p, v)`  = [`fts5_put_varint`]  (`util::put_varint`, devolve os bytes)
//! * `sqlite3Fts5GetVarint(p, &v)` = [`fts5_get_varint`]  (`util::get_varint`, devolve `(bytes, valor)`)
//! * `sqlite3Fts5GetVarintLen(v)`  = [`fts5_get_varint_len`] (`util::varint_len`; igual para `v >= 128`,
//!   que é a única entrada que o C aceita)
//!
//! Só o que difere do núcleo é escrito aqui: [`fts5_get_varint32`] (o valor de 32 bits é truncado
//! em 31 bits, `& 0x7FFFFFFF`, e não saturado em `0xffffffff`), a macro `fts5FastGetVarint32`
//! e o auxiliar para acrescentar um varint a um `Vec<u8>`.
//!
//! Leitura além do fim da fatia enxerga zeros (o C leria a memória vizinha).

pub use crate::util::get_varint as fts5_get_varint;
pub use crate::util::put_varint as fts5_put_varint;
pub use crate::util::varint_len as fts5_get_varint_len;

/// `sqlite3Fts5GetVarint32`: lê um varint de `p` truncando o valor para 31 bits. Devolve
/// `(bytes lidos, valor)`. O caso de um byte também é tratado aqui (a versão do núcleo supõe
/// que o chamador já o tratou).
pub fn fts5_get_varint32(p: &[u8]) -> (i32, u32) {
    let at = |i: usize| p.get(i).copied().unwrap_or(0) as u32;
    let a = at(0);
    /* O caso de um byte, de longe o mais comum. */
    if a & 0x80 == 0 {
        return (1, a);
    }
    let b = at(1);
    /* O caso de dois bytes */
    if b & 0x80 == 0 {
        return (2, ((a & 0x7f) << 7) | b);
    }
    /* O caso de três bytes */
    let a3 = (a << 14) | at(2);
    if a3 & 0x80 == 0 {
        let a3 = a3 & ((0x7f << 14) | 0x7f);
        let b = (b & 0x7f) << 7;
        return (3, a3 | b);
    }
    /* Os casos raros de quatro bytes ou mais vão pela rotina de 64 bits. */
    let (n, v64) = fts5_get_varint(p);
    debug_assert!(n > 3 && n <= 9);
    (n as i32, (v64 as u32) & 0x7FFF_FFFF)
}

/// Macro `fts5FastGetVarint32(a, iOff, nVal)`: lê um varint de 32 bits em `a[*i_off..]`,
/// avança `*i_off` e devolve o valor. O caso de um byte não chama a função.
#[inline]
pub fn fts5_fast_get_varint32(a: &[u8], i_off: &mut usize) -> u32 {
    let rest = a.get(*i_off..).unwrap_or(&[]);
    let first = rest.first().copied().unwrap_or(0);
    if first & 0x80 != 0 {
        let (n, v) = fts5_get_varint32(rest);
        *i_off += n as usize;
        v
    } else {
        *i_off += 1;
        first as u32
    }
}

/// Acrescenta `v` como varint ao fim de `out` e devolve quantos bytes escreveu (o par do
/// `nData += sqlite3Fts5PutVarint(&p[nData], v)` do C, onde o buffer cresce por posse).
pub fn fts5_append_varint(out: &mut Vec<u8>, v: u64) -> i32 {
    let mut tmp = [0u8; 9];
    let n = fts5_put_varint(&mut tmp, v);
    out.extend_from_slice(&tmp[..n as usize]);
    n
}
