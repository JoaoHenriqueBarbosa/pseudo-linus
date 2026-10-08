//! `vdbetrace.c`: insere os valores dos parâmetros do comando (os "curingas") no texto SQL que
//! `sqlite3_trace()` e `sqlite3_expanded_sql()` entregam (SQLite 3.46.1, modelo v2).
//!
//! Desvios do C, todos decorrentes do modelo v2:
//!
//! * O `Vdbe` não guarda a conexão (`p->db`): quem chama passa `db` e o `Vdbe` separados. Como a
//!   função só lê os dois, ambos são referências compartilhadas; quem tem o `Vdbe` fora do slab
//!   (durante a execução) empresta `&*p`.
//! * O texto SQL e o resultado são `&[u8]` e `Vec<u8>` (sem o NUL final); o fim do texto é o fim
//!   da fatia ou o primeiro NUL. Como no C, o resultado é `None` se o texto resultante for vazio
//!   ou se houve erro de acumulação (`SQLITE_TOOBIG`).
//! * A conversão de um parâmetro de texto UTF-16 para UTF-8 usa `translate_bytes` direto, sem
//!   montar a `Mem` temporária (`utf8`) do C: o resultado é o mesmo.
//! * `SQLITE_TRACE_SIZE_LIMIT` não está definido na build do Debian: o texto e o blob saem
//!   inteiros. Os ramos `SQLITE_OMIT_TRACE` e `SQLITE_DEBUG` não existem.

use crate::connection::Connection;
use crate::consts::{
    MEM_BLOB, MEM_INT, MEM_INTREAL, MEM_NULL, MEM_REAL, MEM_STR, MEM_ZERO, SQLITE_LIMIT_LENGTH,
    SQLITE_UTF8, TK_VARIABLE,
};
use crate::printf::{PrintfArg, StrAccum};
use crate::tokenize::get_token;
use crate::util::{at, get_int32, strlen30};
use crate::utf::translate_bytes;
use crate::vdbe_types::Vdbe;
use crate::vdbeapi::vdbe_parameter_index;

/// `findNextHostParameter`: o número de bytes de `z_sql` (texto SQL UTF-8) até o primeiro
/// caractere de um parâmetro de host, e o tamanho do token do parâmetro. Se o texto não tem
/// parâmetros, devolve o tamanho total e zero.
fn find_next_host_parameter(z_sql: &[u8]) -> (usize, usize) {
    let mut n_total = 0usize;
    let mut pos = 0usize;
    while at(z_sql, pos) != 0 {
        let mut token_type = 0i32;
        let n = get_token(&z_sql[pos..], &mut token_type);
        debug_assert!(n > 0);
        if n <= 0 {
            break;
        }
        if token_type == TK_VARIABLE as i32 {
            return (n_total, n as usize);
        }
        n_total += n as usize;
        pos += n as usize;
    }
    (n_total, 0)
}

/// `sqlite3VdbeExpandSql`: o texto `z_raw_sql` com os parâmetros de host trocados pelos valores
/// ligados em `p`. Se `db.n_vdbe_exec` é maior que 1, o resultado é o texto com "-- " na frente
/// de cada linha (o comando é um gatilho ou subprograma). Procura os parâmetros nas formas `?`,
/// `?N`, `$A`, `@A`, `:A` e `#A`, sem olhar dentro de literais, nomes entre aspas e comentários;
/// o índice dos de forma textual vem do `OP_Variable` correspondente. O valor é escrito como
/// literal SQL.
pub fn expand_sql(db: &Connection, p: &Vdbe, z_raw_sql: &[u8]) -> Option<Vec<u8>> {
    let z = &z_raw_sql[..strlen30(z_raw_sql) as usize];
    let mut idx: i32 = 0; // Índice de um parâmetro de host.
    let mut next_index: i32 = 1; // Índice do próximo parâmetro `?`.
    let mut out = StrAccum::new(db.a_limit[SQLITE_LIMIT_LENGTH as usize] as u32);

    if db.n_vdbe_exec > 1 {
        let mut pos = 0usize;
        while at(z, pos) != 0 {
            let start = pos;
            loop {
                let c = at(z, pos);
                pos += 1;
                if c == b'\n' || at(z, pos) == 0 {
                    break;
                }
            }
            out.append(b"-- ");
            debug_assert!(pos > start);
            out.append(&z[start..pos]);
        }
    } else if p.n_var == 0 {
        out.append(z);
    } else {
        let mut pos = 0usize;
        while at(z, pos) != 0 {
            let (n, n_token) = find_next_host_parameter(&z[pos..]);
            debug_assert!(n > 0);
            out.append(&z[pos..pos + n]);
            pos += n;
            debug_assert!(at(z, pos) != 0 || n_token == 0);
            if n_token == 0 {
                break;
            }
            if z[pos] == b'?' {
                if n_token > 1 {
                    if let Some(v) = get_int32(&z[pos + 1..pos + n_token]) {
                        idx = v;
                    }
                } else {
                    idx = next_index;
                }
            } else {
                debug_assert!(matches!(z[pos], b':' | b'$' | b'@' | b'#'));
                idx = vdbe_parameter_index(p, &z[pos..pos + n_token]);
                debug_assert!(idx > 0);
            }
            pos += n_token;
            next_index = next_index.max(idx + 1);
            debug_assert!(idx > 0 && idx <= p.n_var);
            let Some(p_var) = p.a_var.get((idx - 1) as usize) else {
                continue;
            };
            if (p_var.flags & MEM_NULL) != 0 {
                out.append(b"NULL");
            } else if (p_var.flags & (MEM_INT | MEM_INTREAL)) != 0 {
                out.appendf(b"%lld", &[PrintfArg::Int(p_var.u_i)]);
            } else if (p_var.flags & MEM_REAL) != 0 {
                out.appendf(b"%!.15g", &[PrintfArg::Double(p_var.u_r)]);
            } else if (p_var.flags & MEM_STR) != 0 {
                let enc = db.enc;
                let text = &p_var.z[..(p_var.n.max(0) as usize).min(p_var.z.len())];
                // Mostra em UTF-8, convertendo a codificação do banco se preciso.
                let utf8;
                let text: &[u8] = if enc != SQLITE_UTF8 as u8 {
                    utf8 = translate_bytes(text, enc, SQLITE_UTF8 as u8);
                    &utf8
                } else {
                    text
                };
                out.appendf(
                    b"'%.*q'",
                    &[PrintfArg::Int(text.len() as i64), PrintfArg::Text(Some(text.to_vec()))],
                );
            } else if (p_var.flags & MEM_ZERO) != 0 {
                out.appendf(b"zeroblob(%d)", &[PrintfArg::Int(p_var.n_zero as i64)]);
            } else {
                debug_assert!((p_var.flags & MEM_BLOB) != 0);
                out.append(b"x'");
                let n_out = (p_var.n.max(0) as usize).min(p_var.z.len());
                const HEX: &[u8; 16] = b"0123456789abcdef";
                for &b in &p_var.z[..n_out] {
                    out.append(&[HEX[(b >> 4) as usize], HEX[(b & 0x0f) as usize]]);
                }
                out.append(b"'");
            }
        }
    }
    if out.acc_error != 0 {
        out.reset();
    }
    out.finish()
}
