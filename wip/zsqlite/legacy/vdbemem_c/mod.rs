// Mesclado das partes traduzidas de vdbemem_c (as partes quebram funções grandes no meio).
#![allow(unused_imports, ambiguous_glob_reexports)]
use crate::prelude::*;

// ---- part_000.rs ----

/// Verdadeiro se X é uma potência de dois. O 0 é considerado potência de dois
/// aqui. Em outras palavras, retorna verdadeiro se X tem no máximo um bit ligado.
#[inline]
pub fn ispowerof2(x: u64) -> bool {
    (x & x.wrapping_sub(1)) == 0
}

// As rotinas `sqlite3VdbeCheckMemInvariants` e `sqlite3VdbeMemValidStrRep` só
// existem sob SQLITE_DEBUG (servem apenas a assert()), então somem neste porte.

/// Renderiza um objeto Mem que é um de MEM_INT, MEM_REAL ou MEM_INTREAL em um
/// buffer. O texto fica em `z_buf` terminado em zero e `p.n` recebe o tamanho.
fn vdbe_mem_render_num(sz: i32, z_buf: &mut [u8], p: &mut Mem) {
    if (p.flags & MEM_INT) != 0 {
        p.n = int64_to_text(p.u.i, z_buf);
    } else {
        let mut acc = StrAccum {
            db: None,
            z_text: Vec::new(),
            n_alloc: 0,
            mx_alloc: 0,
            n_char: 0,
            acc_error: 0,
            printf_flags: 0,
        };
        // O buffer inicial do C é z_buf com sz bytes; aqui só o tamanho vale.
        str_accum_init(&mut acc, None, sz, 0);
        let v = if (p.flags & MEM_INTREAL) != 0 {
            p.u.i as f64
        } else {
            p.u.r
        };
        let mut ap = VaList::new();
        ap.args.push_back(VaArg::Double(v));
        str_appendf(&mut acc, b"%!.15g", &mut ap);
        // Versão rápida de sqlite3StrAccumFinish(&acc).
        let n_char = acc.n_char as usize;
        z_buf[..n_char].copy_from_slice(&acc.z_text[..n_char]);
        z_buf[n_char] = 0;
        p.n = acc.n_char as i32;
    }
}

/// Se p_mem é um objeto com uma representação de string válida, garante que a
/// codificação interna dela seja `desired_enc`, um de SQLITE_UTF8,
/// SQLITE_UTF16LE ou SQLITE_UTF16BE.
///
/// Se p_mem não é uma string, ou a codificação já é a pedida, não faz nada.
/// Retorna SQLITE_OK se a conversão deu certo (ou não era necessária) e
/// SQLITE_NOMEM se um malloc() falhou durante a conversão.
pub fn vdbe_change_encoding(p_mem: &mut Mem, desired_enc: i32) -> i32 {
    if (p_mem.flags & MEM_STR) == 0 {
        p_mem.enc = desired_enc as u8;
        return SQLITE_OK;
    }
    if p_mem.enc == desired_enc as u8 {
        return SQLITE_OK;
    }
    // vdbe_mem_translate() retorna SQLITE_OK ou SQLITE_NOMEM. Se NOMEM, a
    // codificação do valor pode não ter mudado.
    vdbe_mem_translate(p_mem, desired_enc as u8)
}

/// Garante que `p_mem.z` aponte para uma alocação gravável de pelo menos n
/// bytes.
///
/// Se `b_preserve` é verdadeiro, copia o conteúdo de `p_mem.z` para a nova
/// alocação (p_mem precisa ser string ou blob). Se é falso, o conteúdo anterior
/// de `p_mem.z` é descartado.
///
/// No C, `z` e `z_malloc` são o mesmo ponteiro quando a memória é do próprio
/// Mem (sz_malloc>0 e nenhum de MEM_DYN, MEM_EPHEM, MEM_STATIC ligado). Aqui os
/// dois são `Vec<u8>` e o espelhamento é feito pelo lead no ponto de uso.
pub fn vdbe_mem_grow(p_mem: &mut Mem, n: i32, b_preserve: i32) -> i32 {
    let mut b_preserve = b_preserve;
    let z_is_malloc = p_mem.sz_malloc > 0
        && (p_mem.flags & (MEM_DYN | MEM_EPHEM | MEM_STATIC)) == 0;
    if p_mem.sz_malloc > 0 && b_preserve != 0 && z_is_malloc {
        // sqlite3DbReallocOrFree / sqlite3Realloc: preserva o prefixo.
        if n > 0 {
            p_mem.z_malloc.resize(n as usize, 0);
            p_mem.z = p_mem.z_malloc.clone();
        } else {
            // O realloc com tamanho 0 libera e devolve NULL.
            p_mem.z_malloc = Vec::new();
            p_mem.z = Vec::new();
        }
        b_preserve = 0;
    } else {
        if p_mem.sz_malloc > 0 {
            p_mem.z_malloc = Vec::new();
        }
        // sqlite3DbMallocRaw(db, n): sqlite3Malloc(n) devolve NULL para n<=0.
        p_mem.z_malloc = if n > 0 { vec![0u8; n as usize] } else { Vec::new() };
    }
    if p_mem.z_malloc.is_empty() {
        vdbe_mem_set_null(p_mem);
        p_mem.z = Vec::new();
        p_mem.sz_malloc = 0;
        return SQLITE_NOMEM_BKPT;
    } else {
        p_mem.sz_malloc = p_mem.z_malloc.len() as i32;
    }

    if b_preserve != 0 && !p_mem.z.is_empty() {
        let n_copy = (p_mem.n as usize).min(p_mem.z.len()).min(p_mem.z_malloc.len());
        let (z, z_malloc) = (&p_mem.z, &mut p_mem.z_malloc);
        z_malloc[..n_copy].copy_from_slice(&z[..n_copy]);
    }
    if (p_mem.flags & MEM_DYN) != 0 {
        if let Some(x_del) = p_mem.x_del {
            x_del(std::mem::take(&mut p_mem.z));
        }
    }

    p_mem.z = p_mem.z_malloc.clone();
    p_mem.flags &= !(MEM_DYN | MEM_EPHEM | MEM_STATIC);
    SQLITE_OK
}

/// Muda a alocação `p_mem.z_malloc` para ter pelo menos `sz_new` bytes. Se
/// `z_malloc` já atende ou excede o tamanho pedido, não faz nada além de
/// reapontar `z`.
///
/// Qualquer conteúdo de string ou blob pode ser descartado e o destrutor
/// `x_del` é chamado, se existir. MEM_INT, MEM_REAL, MEM_INTREAL e MEM_NULL
/// são preservados. Retorna SQLITE_OK ou um código de erro (provavelmente
/// SQLITE_NOMEM).
pub fn vdbe_mem_clear_and_resize(p_mem: &mut Mem, sz_new: i32) -> i32 {
    if p_mem.sz_malloc < sz_new {
        return vdbe_mem_grow(p_mem, sz_new, 0);
    }
    p_mem.z = p_mem.z_malloc.clone();
    p_mem.flags &= MEM_NULL | MEM_INT | MEM_REAL | MEM_INTREAL;
    SQLITE_OK
}

/// Se p_mem já é uma string, detecta se ela termina em zero, ou a torna uma
/// string terminada em zero se possível, e marca como tal.
///
/// É uma otimização: a operação correta continua mesmo que esta rotina não
/// faça nada.
pub fn vdbe_mem_zero_terminate_if_able(p_mem: &mut Mem) {
    if (p_mem.flags & (MEM_STR | MEM_TERM | MEM_EPHEM | MEM_STATIC)) != MEM_STR {
        // p_mem precisa ser string, e não pode ser efêmera nem estática.
        return;
    }
    if p_mem.enc != SQLITE_UTF8 as u8 {
        return;
    }
    if p_mem.z.is_empty() {
        return;
    }
    if (p_mem.flags & MEM_DYN) != 0 {
        // No C o ramo testa x_del == sqlite3_free (com msize) e x_del ==
        // sqlite3RCStrUnref. No modelo sem ponteiros as duas identidades não se
        // distinguem de um destrutor qualquer, e a rotina é só uma otimização,
        // então o ramo dinâmico não faz nada (a operação continua correta).
        return;
    } else if p_mem.sz_malloc >= p_mem.n + 1 && p_mem.z.len() > p_mem.n as usize {
        let n = p_mem.n as usize;
        p_mem.z[n] = 0;
        if p_mem.z_malloc.len() > n {
            p_mem.z_malloc[n] = 0;
        }
        p_mem.flags |= MEM_TERM;
        return;
    }
}

/// Já se sabe que p_mem contém uma string sem terminador. Acrescenta o
/// terminador zero.
///
/// São acrescentados três bytes zero, de modo que haja garantia de um par de
/// zeros em fronteira par para terminar uma string UTF-16, mesmo que o tamanho
/// inicial do buffer seja ímpar.
fn vdbe_mem_add_terminator(p_mem: &mut Mem) -> i32 {
    if vdbe_mem_grow(p_mem, p_mem.n + 3, 1) != 0 {
        return SQLITE_NOMEM_BKPT;
    }
    let n = p_mem.n as usize;
    p_mem.z[n] = 0;
    p_mem.z[n + 1] = 0;
    p_mem.z[n + 2] = 0;
    // z_malloc espelha z (no C são o mesmo ponteiro).
    p_mem.z_malloc[n] = 0;
    p_mem.z_malloc[n + 1] = 0;
    p_mem.z_malloc[n + 2] = 0;
    p_mem.flags |= MEM_TERM;
    SQLITE_OK
}


// ---- part_001.rs ----

/// Muda p_mem para que seu valor MEM_STR ou MEM_BLOB esteja armazenado em
/// `z_malloc`, onde pode ser escrito com segurança.
///
/// Retorna SQLITE_OK no sucesso ou SQLITE_NOMEM se o malloc falha.
///
/// No C, "z == z_malloc" significa que o buffer é do próprio Mem. Aqui isso é
/// `sz_malloc > 0` sem nenhum de MEM_DYN, MEM_EPHEM, MEM_STATIC ligado.
pub fn vdbe_mem_make_writeable(p_mem: &mut Mem) -> i32 {
    assert!(!vdbe_mem_is_row_set(p_mem));
    if (p_mem.flags & (MEM_STR | MEM_BLOB)) != 0 {
        if expand_blob(p_mem) != 0 {
            return SQLITE_NOMEM;
        }
        let z_is_malloc =
            p_mem.sz_malloc > 0 && (p_mem.flags & (MEM_DYN | MEM_EPHEM | MEM_STATIC)) == 0;
        if !z_is_malloc {
            let rc = vdbe_mem_add_terminator(p_mem);
            if rc != 0 {
                return rc;
            }
        }
    }
    p_mem.flags &= !MEM_EPHEM;
    // SQLITE_DEBUG: p_scopy_from = 0 some neste porte.

    SQLITE_OK
}

/// Se a Mem dada tem uma cauda preenchida com zeros, a transforma em um blob
/// ordinário armazenado em espaço alocado dinamicamente.
pub fn vdbe_mem_expand_blob(p_mem: &mut Mem) -> i32 {
    assert!((p_mem.flags & MEM_ZERO) != 0);
    assert!((p_mem.flags & MEM_BLOB) != 0 || mem_null_nochng(p_mem));
    assert!(!vdbe_mem_is_row_set(p_mem));

    // Número de bytes necessários para guardar o blob expandido.
    let mut n_byte: i32 = p_mem.n + p_mem.u.n_zero;
    if n_byte <= 0 {
        if (p_mem.flags & MEM_BLOB) == 0 {
            return SQLITE_OK;
        }
        n_byte = 1;
    }
    if vdbe_mem_grow(p_mem, n_byte, 1) != 0 {
        return SQLITE_NOMEM_BKPT;
    }
    assert!(!p_mem.z.is_empty());
    assert!(p_mem.z.len() as i32 >= n_byte);

    let start = p_mem.n as usize;
    let end = start + p_mem.u.n_zero as usize;
    p_mem.z[start..end].fill(0);
    // Mantém z_malloc espelhando z (no C são o mesmo ponteiro).
    p_mem.z_malloc = p_mem.z.clone();
    p_mem.n += p_mem.u.n_zero;
    p_mem.flags &= !(MEM_ZERO | MEM_TERM);
    SQLITE_OK
}

/// Garante que a Mem dada termine em zero.
pub fn vdbe_mem_nul_terminate(p_mem: &mut Mem) -> i32 {
    if (p_mem.flags & (MEM_TERM | MEM_STR)) != MEM_STR {
        SQLITE_OK // Nada a fazer.
    } else {
        vdbe_mem_add_terminator(p_mem)
    }
}

/// Acrescenta MEM_STR ao conjunto de representações da Mem dada. Só é chamada
/// se p_mem é um número de algum tipo, nunca NULL nem BLOB.
///
/// As representações MEM_INT, MEM_REAL ou MEM_INTREAL existentes são
/// invalidadas se `b_force` é verdadeiro e mantidas se é falso.
///
/// Um MEM_NULL nunca é passado a esta função: ela serve para converter valores
/// em texto para o usuário (sqlite3_value_text()) ou garantir que chaves de
/// b-tree sejam strings.
pub fn vdbe_mem_stringify(p_mem: &mut Mem, enc: u8, b_force: u8) -> i32 {
    const N_BYTE: i32 = 32;

    assert!((p_mem.flags & MEM_ZERO) == 0);
    assert!((p_mem.flags & (MEM_STR | MEM_BLOB)) == 0);
    assert!((p_mem.flags & (MEM_INT | MEM_REAL | MEM_INTREAL)) != 0);
    assert!(!vdbe_mem_is_row_set(p_mem));

    if vdbe_mem_clear_and_resize(p_mem, N_BYTE) != 0 {
        p_mem.enc = 0;
        return SQLITE_NOMEM_BKPT;
    }

    // O buffer sai de p_mem durante a renderização porque a rotina também
    // altera p_mem.n.
    let mut z_buf = std::mem::take(&mut p_mem.z);
    vdbe_mem_render_num(N_BYTE, &mut z_buf, p_mem);
    p_mem.z = z_buf;
    p_mem.z_malloc = p_mem.z.clone();
    assert!(!p_mem.z.is_empty());
    p_mem.enc = SQLITE_UTF8 as u8;
    p_mem.flags |= MEM_STR | MEM_TERM;
    if b_force != 0 {
        p_mem.flags &= !(MEM_INT | MEM_REAL | MEM_INTREAL);
    }
    vdbe_change_encoding(p_mem, enc as i32);
    SQLITE_OK
}

/// A célula p_mem contém o contexto de uma função de agregação. Chama o método
/// finalizador dessa função e guarda o resultado de volta em p_mem.
///
/// Retorna SQLITE_ERROR se o finalizador relata erro, SQLITE_OK caso contrário.
pub fn vdbe_mem_finalize(p_mem: &mut Mem, p_func: &FuncDef) -> i32 {
    assert!(p_mem.db.is_some());
    assert!(p_func.x_finalize.is_some());

    let mut ctx = Sqlite3Context::default();
    let mut t = Mem::default();
    t.flags = MEM_NULL;
    t.db = p_mem.db.clone();

    ctx.p_out = Rc::new(RefCell::new(t));
    ctx.p_mem = Some(Rc::new(RefCell::new(p_mem.clone())));
    ctx.p_func = Rc::new(RefCell::new(p_func.clone()));
    if let Some(db) = p_mem.db.as_ref().and_then(|w| w.upgrade()) {
        ctx.enc = enc(&db) as u8;
    }
    if let Some(x_finalize) = &p_func.x_finalize {
        x_finalize(&mut ctx); // IMP: R-24505-23230
    }
    // O finalizador mexe no acumulador in loco: devolve as mudanças a p_mem.
    if let Some(m) = &ctx.p_mem {
        *p_mem = m.borrow().clone();
    }
    assert!((p_mem.flags & MEM_DYN) == 0);
    if p_mem.sz_malloc > 0 {
        p_mem.z_malloc = Vec::new();
    }
    let t_val = ctx.p_out.borrow().clone();
    *p_mem = t_val;
    ctx.is_error
}

/// A célula p_accum contém o contexto de uma função de agregação. Chama o
/// método x_value dessa função e guarda o resultado na célula p_out.
///
/// Retorna SQLITE_ERROR se x_value() relata erro, SQLITE_OK caso contrário.
pub fn vdbe_mem_agg_value(p_accum: &mut Mem, p_out: &mut Mem, p_func: &FuncDef) -> i32 {
    assert!(p_func.x_value.is_some());
    assert!(p_accum.db.is_some());

    let mut ctx = Sqlite3Context::default();
    vdbe_mem_set_null(p_out);
    ctx.p_out = Rc::new(RefCell::new(p_out.clone()));
    ctx.p_mem = Some(Rc::new(RefCell::new(p_accum.clone())));
    ctx.p_func = Rc::new(RefCell::new(p_func.clone()));
    if let Some(db) = p_accum.db.as_ref().and_then(|w| w.upgrade()) {
        ctx.enc = enc(&db) as u8;
    }
    if let Some(x_value) = &p_func.x_value {
        x_value(&mut ctx);
    }
    // As cópias do contexto voltam para os Mem originais.
    *p_out = ctx.p_out.borrow().clone();
    if let Some(m) = &ctx.p_mem {
        *p_accum = m.borrow().clone();
    }
    ctx.is_error
}

/// Se a célula contém um valor que precisa ser liberado pelo callback externo
/// `x_del`, esta rotina o libera e põe `flags` em MEM_NULL.
///
/// Auxiliar de vdbe_mem_set_null() e vdbe_mem_release(); use essas como ponto
/// de entrada para liberar recursos de Mem.
fn vdbe_mem_clear_extern_and_set_null(p: &mut Mem) {
    assert!(vdbe_mem_dynamic(p));
    if (p.flags & MEM_AGG) != 0 {
        if let Some(p_def) = p.u.p_def.clone() {
            vdbe_mem_finalize(p, &p_def);
        }
        assert!((p.flags & MEM_AGG) == 0);
    }
    if (p.flags & MEM_DYN) != 0 {
        if let Some(x_del) = p.x_del {
            x_del(std::mem::take(&mut p.z));
        }
    }
    p.flags = MEM_NULL;
}

/// Libera a memória mantida por p, tanto a externa liberada por `x_del` quanto
/// a de `z_malloc`.
///
/// Auxiliar de vdbe_mem_release() no caso incomum em que há memória a liberar.
fn vdbe_mem_clear(p: &mut Mem) {
    if vdbe_mem_dynamic(p) {
        vdbe_mem_clear_extern_and_set_null(p);
    }
    if p.sz_malloc != 0 {
        p.z_malloc = Vec::new();
        p.sz_malloc = 0;
    }
    p.z = Vec::new();
}

/// Libera todos os recursos de memória mantidos pela Mem: a memória liberada
/// por `x_del` e a alocação `z_malloc`.
///
/// Use antes de abandonar uma Mem, ou para devolvê-la ao uso mínimo de memória.
/// Use vdbe_mem_set_null() para liberar só o espaço de `x_del` antes de
/// inserir conteúdo novo.
pub fn vdbe_mem_release(p: &mut Mem) {
    if vdbe_mem_dynamic(p) || p.sz_malloc != 0 {
        vdbe_mem_clear(p);
    }
}

/// Como vdbe_mem_release(), porém mais rápida quando se sabe de antemão que a
/// Mem não é MEM_DYN nem MEM_AGG.
pub fn vdbe_mem_release_malloc(p: &mut Mem) {
    assert!(!vdbe_mem_dynamic(p));
    if p.sz_malloc != 0 {
        vdbe_mem_clear(p);
    }
}

/// Valor inteiro que melhor representa o conteúdo de p_mem (string ou blob
/// passam por sqlite3Atoi64).
fn mem_int_value(p_mem: &Mem) -> i64 {
    let mut value: i64 = 0;
    atoi64(&p_mem.z, &mut value, p_mem.n, p_mem.enc);
    value
}

/// Devolve um inteiro que é o melhor que se consegue para representar o valor
/// de p_mem. Inteiro: exato. Ponto flutuante: a parte inteira. String ou blob:
/// tenta converter. NULL: 0.
pub fn vdbe_int_value(p_mem: &Mem) -> i64 {
    let flags = p_mem.flags;
    if (flags & (MEM_INT | MEM_INTREAL)) != 0 {
        p_mem.u.i
    } else if (flags & MEM_REAL) != 0 {
        real_to_i64(p_mem.u.r)
    } else if (flags & (MEM_STR | MEM_BLOB)) != 0 && !p_mem.z.is_empty() {
        mem_int_value(p_mem)
    } else {
        0
    }
}

/// Valor double que melhor representa o conteúdo de p_mem (via sqlite3AtoF).
fn mem_real_value(p_mem: &Mem) -> f64 {
    let mut val: f64 = 0.0;
    ato_f(&p_mem.z, &mut val, p_mem.n, p_mem.enc);
    val
}

/// Devolve a melhor representação de p_mem em double. Se já é double ou
/// inteiro, devolve o valor; string ou blob tentam converter; NULL dá 0.0.
pub fn vdbe_real_value(p_mem: &Mem) -> f64 {
    if (p_mem.flags & MEM_REAL) != 0 {
        p_mem.u.r
    } else if (p_mem.flags & (MEM_INT | MEM_INTREAL)) != 0 {
        p_mem.u.i as f64
    } else if (p_mem.flags & (MEM_STR | MEM_BLOB)) != 0 {
        mem_real_value(p_mem)
    } else {
        0.0
    }
}

/// Retorna 1 se p_mem representa verdadeiro, 0 se falso, e `if_null` se NULL.
pub fn vdbe_boolean_value(p_mem: &Mem, if_null: i32) -> i32 {
    if (p_mem.flags & (MEM_INT | MEM_INTREAL)) != 0 {
        return (p_mem.u.i != 0) as i32;
    }
    if (p_mem.flags & MEM_NULL) != 0 {
        return if_null;
    }
    (vdbe_real_value(p_mem) != 0.0) as i32
}

/// A Mem já é MEM_REAL ou MEM_INTREAL. Tenta torná-la MEM_INT se possível.
pub fn vdbe_integer_affinity(p_mem: &mut Mem) {
    assert!((p_mem.flags & (MEM_REAL | MEM_INTREAL)) != 0);
    assert!(!vdbe_mem_is_row_set(p_mem));

    if (p_mem.flags & MEM_INTREAL) != 0 {
        mem_set_type_flag(p_mem, MEM_INT);
    } else {
        let ix: i64 = real_to_i64(p_mem.u.r);

        // Só marca como inteiro se
        //
        //    (1) a conversão real->int->real é um no-op, e
        //    (2) o inteiro não é o maior nem o menor possível (ticket #3922).
        //
        // Os dois últimos termos impõem a segunda condição sob a suposição de
        // que o overflow da adição dá a volta.
        if p_mem.u.r == ix as f64 && ix > SMALLEST_INT64 && ix < LARGEST_INT64 {
            p_mem.u.i = ix;
            mem_set_type_flag(p_mem, MEM_INT);
        }
    }
}

/// Converte p_mem para inteiro, invalidando qualquer representação anterior.
pub fn vdbe_mem_integerify(p_mem: &mut Mem) -> i32 {
    assert!(!vdbe_mem_is_row_set(p_mem));

    let i = vdbe_int_value(p_mem);
    p_mem.u.i = i;
    mem_set_type_flag(p_mem, MEM_INT);
    SQLITE_OK
}


// ---- part_002.rs ----

// Notas de integração para o tech lead:
// - `Mem` (vdbeInt_h) precisa de um campo `p_row_set: Option<Box<RowSet>>` para
//   `vdbe_mem_set_row_set` e `vdbe_mem_is_row_set`: no C o RowSet vive em `Mem.z` e o
//   destrutor `sqlite3RowSetDelete` em `Mem.xDel`, o que o modelo `z: Vec<u8>` não comporta.
// - Sem SQLITE_DEBUG: `sqlite3VdbeMemAboutToChange` some (a macro `memAboutToChange` é vazia).
//   `vdbe_mem_is_row_set` fica porque a tradução de vdbe_mem_make_writeable a chama em assert.
// - Com SQLITE_OMIT_INCRBLOB indefinido (Debian) e SQLITE_OMIT_FLOATING_POINT indefinido,
//   só o primeiro ramo de `vdbe_mem_set_zero_blob` e a `vdbe_mem_set_double` existem.

/// Converte pMem para que seja do tipo MEM_REAL.
/// Invalida qualquer representação anterior.
pub fn vdbe_mem_realify(p_mem: &mut Mem) -> i32 {
    p_mem.u.r = vdbe_real_value(p_mem);
    mem_set_type_flag(p_mem, MEM_REAL);
    SQLITE_OK
}

/// Compara um valor de ponto flutuante com um inteiro. Retorna verdadeiro se os dois
/// valores são iguais dentro da precisão do valor de ponto flutuante.
///
/// Esta função assume que i foi obtido por atribuição a partir de r1.
///
/// Em algumas versões do GCC em máquinas de 32 bits, a comparação mais óbvia
/// "r1==(double)i" às vezes dá falso mesmo com r1 e (double)i idênticos bit a bit.
pub fn real_same_as_int(r1: f64, i: i64) -> bool {
    let r2 = i as f64;
    r1 == 0.0
        || (r1.to_ne_bytes() == r2.to_ne_bytes()
            && i >= -2251799813685248i64
            && i < 2251799813685248i64)
}

/// Converte um valor de ponto flutuante no inteiro mais próximo, de um jeito que
/// evita os avisos 'outside the range of representable values' do UBSAN.
pub fn real_to_i64(r: f64) -> i64 {
    if r < -9223372036854774784.0 {
        return SMALLEST_INT64;
    }
    if r > 9223372036854774784.0 {
        return LARGEST_INT64;
    }
    r as i64
}

/// Converte pMem para que tenha o tipo MEM_REAL ou MEM_INT.
/// Invalida qualquer representação anterior.
///
/// Faz-se todo esforço para forçar a conversão, mesmo que a entrada seja uma string
/// que não pareça completamente um número. Converte o quanto der da string e ignora
/// o resto.
pub fn vdbe_mem_numerify(p_mem: &mut Mem) -> i32 {
    if (p_mem.flags & (MEM_INT | MEM_REAL | MEM_INTREAL | MEM_NULL)) == 0 {
        let mut ix: i64 = 0;
        debug_assert!((p_mem.flags & (MEM_BLOB | MEM_STR)) != 0);
        let rc = ato_f(&p_mem.z, &mut p_mem.u.r, p_mem.n, p_mem.enc);
        let first = (rc == 0 || rc == 1)
            && atoi64(&p_mem.z, &mut ix, p_mem.n, p_mem.enc) <= 1;
        let use_int = first || {
            ix = real_to_i64(p_mem.u.r);
            real_same_as_int(p_mem.u.r, ix)
        };
        if use_int {
            p_mem.u.i = ix;
            mem_set_type_flag(p_mem, MEM_INT);
        } else {
            mem_set_type_flag(p_mem, MEM_REAL);
        }
    }
    debug_assert!((p_mem.flags & (MEM_INT | MEM_REAL | MEM_INTREAL | MEM_NULL)) != 0);
    p_mem.flags &= !(MEM_STR | MEM_BLOB | MEM_ZERO);
    SQLITE_OK
}

/// Converte o tipo de dado do valor em pMem segundo a afinidade "aff". Converter é
/// diferente de aplicar afinidade: a conversão é forçada, ou seja, o valor vira a
/// afinidade desejada mesmo que haja perda de dados. Usada (por exemplo) para
/// implementar o operador SQL "cast()".
pub fn vdbe_mem_cast(p_mem: &mut Mem, aff: u8, encoding: u8) -> i32 {
    if (p_mem.flags & MEM_NULL) != 0 {
        return SQLITE_OK;
    }
    match aff {
        SQLITE_AFF_BLOB => {
            // Na verdade uma conversão para BLOB
            if (p_mem.flags & MEM_BLOB) == 0 {
                value_apply_affinity(p_mem, SQLITE_AFF_TEXT, encoding);
                if (p_mem.flags & MEM_STR) != 0 {
                    mem_set_type_flag(p_mem, MEM_BLOB);
                }
            } else {
                p_mem.flags &= !(MEM_TYPEMASK & !MEM_BLOB);
            }
        }
        SQLITE_AFF_NUMERIC => {
            vdbe_mem_numerify(p_mem);
        }
        SQLITE_AFF_INTEGER => {
            vdbe_mem_integerify(p_mem);
        }
        SQLITE_AFF_REAL => {
            vdbe_mem_realify(p_mem);
        }
        _ => {
            debug_assert!(aff == SQLITE_AFF_TEXT);
            debug_assert!(MEM_STR == (MEM_BLOB >> 3));
            p_mem.flags |= (p_mem.flags & MEM_BLOB) >> 3;
            value_apply_affinity(p_mem, SQLITE_AFF_TEXT, encoding);
            p_mem.flags &= !(MEM_INT | MEM_REAL | MEM_INTREAL | MEM_BLOB | MEM_ZERO);
            if encoding != SQLITE_UTF8 as u8 {
                p_mem.n &= !1;
            }
            let rc = vdbe_change_encoding(p_mem, encoding as i32);
            if rc != 0 {
                return rc;
            }
            vdbe_mem_zero_terminate_if_able(p_mem);
        }
    }
    SQLITE_OK
}

/// Inicializa memória bruta como um objeto Mem consistente.
///
/// Faz-se o mínimo de inicialização possível.
pub fn vdbe_mem_init(p_mem: &mut Mem, db: Option<Weak<RefCell<Sqlite3>>>, flags: u16) {
    debug_assert!((flags & !MEM_TYPEMASK) == 0);
    p_mem.flags = flags;
    p_mem.db = db;
    p_mem.sz_malloc = 0;
}

/// Apaga qualquer valor anterior e põe o valor guardado em *pMem como NULL.
///
/// Esta rotina chama o destrutor Mem.x_del para descartar valores que precisam dele,
/// mas preserva a alocação Mem.z_malloc. Para liberar todos os recursos, use
/// vdbe_mem_release(), que invoca o destrutor e também desaloca Mem.z_malloc.
///
/// Use esta rotina para zerar o Mem antes de inserir um novo valor.
///
/// Use vdbe_mem_release() para apagar o Mem por completo antes de abandoná-lo.
pub fn vdbe_mem_set_null(p_mem: &mut Mem) {
    if vdbe_mem_dynamic(p_mem) {
        vdbe_mem_clear_extern_and_set_null(p_mem);
    } else {
        p_mem.flags = MEM_NULL;
    }
}

// `sqlite3ValueSetNull(sqlite3_value*)` é `vdbe_mem_set_null` sobre o mesmo objeto
// (`sqlite3_value` é o `Mem`): reexportação, sem função de repasse.
pub use self::vdbe_mem_set_null as value_set_null;

/// Apaga qualquer valor anterior e põe o valor como um BLOB de comprimento n
/// contendo só zeros.
pub fn vdbe_mem_set_zero_blob(p_mem: &mut Mem, n: i32) {
    vdbe_mem_release(p_mem);
    p_mem.flags = MEM_BLOB | MEM_ZERO;
    p_mem.n = 0;
    let n = if n < 0 { 0 } else { n };
    p_mem.u.n_zero = n;
    p_mem.enc = SQLITE_UTF8 as u8;
    p_mem.z = Vec::new();
}

/// O pMem sabidamente contém conteúdo que precisa ser destruído antes de uma troca
/// de valor. Invoca o destrutor e põe o valor como um inteiro de 64 bits.
fn vdbe_release_and_set_int64(p_mem: &mut Mem, val: i64) {
    vdbe_mem_set_null(p_mem);
    p_mem.u.i = val;
    p_mem.flags = MEM_INT;
}

/// Apaga qualquer valor anterior e põe o valor guardado em *pMem como val, de tipo
/// manifesto INTEGER.
pub fn vdbe_mem_set_int64(p_mem: &mut Mem, val: i64) {
    if vdbe_mem_dynamic(p_mem) {
        vdbe_release_and_set_int64(p_mem, val);
    } else {
        p_mem.u.i = val;
        p_mem.flags = MEM_INT;
    }
}

/// Põe a i_idx-ésima entrada do vetor a_mem[] com o valor inteiro val.
pub fn mem_set_array_int64(a_mem: &mut [Mem], i_idx: usize, val: i64) {
    vdbe_mem_set_int64(&mut a_mem[i_idx], val);
}

/// Um destrutor que não faz nada.
pub fn noop_destructor(_p: Vec<u8>) {}

/// O valor guardado em *pMem já deve ser NULL. Guarda também um ponteiro que o
/// acompanha.
pub fn vdbe_mem_set_pointer(
    p_mem: &mut Mem,
    p_ptr: Vec<u8>,
    z_p_type: Option<&'static [u8]>,
    x_destructor: Option<fn(Vec<u8>)>,
) {
    debug_assert!(p_mem.flags == MEM_NULL);
    vdbe_mem_clear(p_mem);
    p_mem.u.z_p_type = Some(z_p_type.unwrap_or(b""));
    p_mem.z = p_ptr;
    p_mem.flags = MEM_NULL | MEM_DYN | MEM_SUBTYPE | MEM_TERM;
    p_mem.e_subtype = b'p';
    p_mem.x_del = Some(x_destructor.unwrap_or(noop_destructor));
}

/// Apaga qualquer valor anterior e põe o valor guardado em *pMem como val, de tipo
/// manifesto REAL.
pub fn vdbe_mem_set_double(p_mem: &mut Mem, val: f64) {
    vdbe_mem_set_null(p_mem);
    if !is_nan(val) {
        p_mem.u.r = val;
        p_mem.flags = MEM_REAL;
    }
}

/// Retorna verdadeiro se o Mem contém um objeto RowSet. No C só existe com
/// SQLITE_DEBUG, para uso dentro de assert().
pub fn vdbe_mem_is_row_set(p_mem: &Mem) -> bool {
    (p_mem.flags & (MEM_BLOB | MEM_DYN)) == (MEM_BLOB | MEM_DYN) && p_mem.p_row_set.is_some()
}

/// Apaga qualquer valor anterior e põe o valor de pMem como um índice booleano vazio.
///
/// Retorna SQLITE_OK em caso de sucesso e SQLITE_NOMEM se ocorrer erro de alocação.
pub fn vdbe_mem_set_row_set(p_mem: &mut Mem) -> i32 {
    let db = match p_mem.db.as_ref().and_then(|w| w.upgrade()) {
        Some(db) => db,
        None => return SQLITE_NOMEM,
    };
    debug_assert!(!vdbe_mem_is_row_set(p_mem));
    vdbe_mem_release(p_mem);
    let p = match row_set_init(&db) {
        Some(p) => p,
        None => return SQLITE_NOMEM,
    };
    p_mem.p_row_set = Some(p);
    p_mem.flags = MEM_BLOB | MEM_DYN;
    // O destrutor sqlite3RowSetDelete é o Drop do Box<RowSet> em p_row_set.
    p_mem.x_del = Some(noop_destructor);
    SQLITE_OK
}

/// Retorna verdadeiro se o objeto Mem contém um TEXT ou BLOB grande demais, cujo
/// tamanho excede SQLITE_MAX_LENGTH.
pub fn vdbe_mem_too_big(p: &Mem) -> bool {
    if (p.flags & (MEM_STR | MEM_BLOB)) != 0 {
        let mut n = p.n;
        if (p.flags & MEM_ZERO) != 0 {
            n += p.u.n_zero;
        }
        let db = match p.db.as_ref().and_then(|w| w.upgrade()) {
            Some(db) => db,
            None => return false,
        };
        let limit = db.borrow().a_limit[SQLITE_LIMIT_LENGTH as usize];
        return n > limit;
    }
    false
}

/// Faz uma cópia rasa de pFrom em pTo. O conteúdo anterior de pTo é liberado. O
/// campo pFrom->z não é duplicado. Se pFrom->z for usado, pTo->z aponta para a
/// mesma coisa que pFrom->z e as flags recebem srcType (MEM_EPHEM ou MEM_STATIC).
fn vdbe_clr_copy(p_to: &mut Mem, p_from: &Mem, e_type: u16) {
    vdbe_mem_clear_extern_and_set_null(p_to);
    debug_assert!(!vdbe_mem_dynamic(p_to));
    vdbe_mem_shallow_copy(p_to, p_from, e_type);
}


// ---- part_003.rs ----

/// Faz uma cópia rasa de pFrom em pTo. O conteúdo anterior de pTo é liberado
/// quando ele é dinâmico. O buffer de pFrom não é duplicado: pTo passa a ver os
/// mesmos bytes, marcado como MEM_EPHEM ou MEM_STATIC conforme `src_type`.
pub fn vdbe_mem_shallow_copy(p_to: &mut Mem, p_from: &Mem, src_type: u16) {
    if vdbe_mem_dynamic(p_to) {
        vdbe_clr_copy(p_to, p_from, src_type);
        return;
    }
    // memcpy(pTo, pFrom, MEMCELLSIZE)
    mem_shallow_copy(p_to, p_from);
    if (p_from.flags & MEM_STATIC) == 0 {
        p_to.flags &= !(MEM_DYN | MEM_STATIC | MEM_EPHEM);
        debug_assert!(src_type == MEM_EPHEM || src_type == MEM_STATIC);
        p_to.flags |= src_type;
    }
}

/// Faz uma cópia completa de pFrom em pTo. O conteúdo anterior de pTo é
/// liberado antes da cópia.
pub fn vdbe_mem_copy(p_to: &mut Mem, p_from: &Mem) -> i32 {
    let mut rc = SQLITE_OK;

    if vdbe_mem_dynamic(p_to) {
        vdbe_mem_clear_extern_and_set_null(p_to);
    }
    // memcpy(pTo, pFrom, MEMCELLSIZE)
    mem_shallow_copy(p_to, p_from);
    p_to.flags &= !MEM_DYN;
    if (p_to.flags & (MEM_STR | MEM_BLOB)) != 0 {
        if 0 == (p_from.flags & MEM_STATIC) {
            p_to.flags |= MEM_EPHEM;
            rc = vdbe_mem_make_writeable(p_to);
        }
    }

    rc
}

/// Transfere o conteúdo de pFrom para pTo. Qualquer valor existente em pTo é
/// liberado. Se pFrom contém dados efêmeros, uma cópia é feita.
///
/// pFrom contém um SQL NULL quando esta rotina retorna.
pub fn vdbe_mem_move(p_to: &mut Mem, p_from: &mut Mem) {
    vdbe_mem_release(p_to);
    // memcpy(pTo, pFrom, sizeof(Mem)): os buffers mudam de dono em vez de serem
    // duplicados, já que pFrom vira NULL logo em seguida. O clone do resto dos
    // campos acompanha qualquer campo novo que o Mem ganhe.
    let z = std::mem::take(&mut p_from.z);
    let z_malloc = std::mem::take(&mut p_from.z_malloc);
    let mut moved = p_from.clone();
    moved.z = z;
    moved.z_malloc = z_malloc;
    *p_to = moved;
    p_from.flags = MEM_NULL;
    p_from.sz_malloc = 0;
}

/// Muda o valor de um Mem para ser uma string ou um BLOB.
///
/// A estratégia de gerência de memória depende do destrutor `x_del`. Se for
/// SQLITE_TRANSIENT, a string é copiada para um buffer (possivelmente já
/// existente) gerenciado pela estrutura Mem. Caso contrário, o buffer
/// existente é liberado e o conteúdo é adotado como está (MEM_STATIC).
/// No modelo sem ponteiros, o conteúdo de `z` é sempre de quem chama, então os
/// destrutores SQLITE_DYNAMIC e por função não têm o que liberar: o ramo do C
/// que os invocava (ou que adotava o buffer como zMalloc) não tem equivalente
/// aqui, e só Static e Transient existem em `Destructor`.
///
/// Se a string é grande demais (excede SQLITE_LIMIT_LENGTH) nenhuma alocação
/// ocorre e SQLITE_TOOBIG é retornado. Se a string pode ser guardada sem
/// alocar memória, ela é guardada. Se uma alocação é necessária, o valor de
/// pMem não muda em caso de falha.
///
/// O parâmetro `enc` é a codificação do texto, ou zero para guardar um blob.
///
/// Se `n` é negativo, a string vai até o primeiro caractere zero (excluído).
/// `n` precisa ser não negativo para blobs. `z` ausente (None) é o ponteiro
/// NULL do C e faz pMem virar SQL NULL.
pub fn vdbe_mem_set_str(
    p_mem: &mut Mem,
    z: Option<&[u8]>,
    n: i64,
    enc: u8,
    x_del: Destructor,
) -> i32 {
    let mut n_byte: i64 = n; // Novo valor para pMem->n
    let i_limit: i32; // Tamanho máximo de string ou blob
    let mut flags: u16; // Novo valor para pMem->flags
    let mut enc = enc;

    debug_assert!(enc != 0 || n >= 0);

    // Se z é um ponteiro NULL, pMem passa a conter um SQL NULL.
    let z = match z {
        Some(z) => z,
        None => {
            vdbe_mem_set_null(p_mem);
            return SQLITE_OK;
        }
    };

    i_limit = match p_mem.db.as_ref().and_then(|w| w.upgrade()) {
        Some(db) => db.borrow().a_limit[SQLITE_LIMIT_LENGTH as usize],
        None => SQLITE_MAX_LENGTH as i32,
    };
    // z[i] com o zero implícito depois do fim do slice (o C lê a memória do
    // terminador).
    let z_at = |i: i64| -> u8 { z.get(i as usize).copied().unwrap_or(0) };
    if n_byte < 0 {
        debug_assert!(enc != 0);
        if enc == SQLITE_UTF8 as u8 {
            n_byte = z.iter().position(|&b| b == 0).unwrap_or(z.len()) as i64;
        } else {
            n_byte = 0;
            while n_byte <= i_limit as i64 && (z_at(n_byte) | z_at(n_byte + 1)) != 0 {
                n_byte += 2;
            }
        }
        flags = MEM_STR | MEM_TERM;
    } else if enc == 0 {
        flags = MEM_BLOB;
        enc = SQLITE_UTF8 as u8;
    } else {
        flags = MEM_STR;
    }
    if n_byte > i_limit as i64 {
        // O C libera z aqui quando x_del é DYNAMIC ou função; o buffer é de
        // quem chama e some com o drop dele.
        vdbe_mem_set_null(p_mem);
        let db = p_mem.db.as_ref().and_then(|w| w.upgrade());
        let mut guard = db.as_ref().map(|d| d.borrow_mut());
        return error_to_parser(guard.as_deref_mut(), SQLITE_TOOBIG);
    }

    // O bloco a seguir define os novos valores de Mem.z e Mem.x_del. Também
    // liga em "flags" a marca da gerência de memória (MEM_DYN ou MEM_STATIC).
    if x_del == SQLITE_TRANSIENT {
        let mut n_alloc: i64 = n_byte;
        if (flags & MEM_TERM) != 0 {
            n_alloc += if enc == SQLITE_UTF8 as u8 { 1 } else { 2 };
        }
        if vdbe_mem_clear_and_resize(p_mem, n_alloc.max(32) as i32) != 0 {
            return SQLITE_NOMEM_BKPT;
        }
        // memcpy(pMem->z, z, nAlloc): o terminador vem de z (zeros).
        for i in 0..n_alloc as usize {
            p_mem.z[i] = z.get(i).copied().unwrap_or(0);
        }
        // z_malloc espelha z (no C são o mesmo ponteiro).
        p_mem.z_malloc = p_mem.z.clone();
    } else {
        vdbe_mem_release(p_mem);
        let mut buf = z.to_vec();
        if (flags & MEM_TERM) != 0 {
            // O slice pode terminar sem o zero que o C encontra na memória.
            let need = n_byte as usize + if enc == SQLITE_UTF8 as u8 { 1 } else { 2 };
            if buf.len() < need {
                buf.resize(need, 0);
            }
        }
        p_mem.z = buf;
        p_mem.x_del = None;
        flags |= MEM_STATIC;
    }

    p_mem.n = (n_byte & 0x7fffffff) as i32;
    p_mem.flags = flags;
    p_mem.enc = enc;

    if enc > SQLITE_UTF8 as u8 && vdbe_mem_handle_bom(p_mem) != 0 {
        return SQLITE_NOMEM_BKPT;
    }

    SQLITE_OK
}

/// Move dados para fora de uma chave ou dado de btree e para dentro de uma
/// estrutura Mem. Os dados são o payload da entrada para a qual pCur aponta
/// agora. `offset` e `amt` definem qual parte da chave ou do dado é lida. O
/// resultado é escrito em pMem.
///
/// O objeto pMem precisa ter sido inicializado. Esta rotina usa
/// pMem->z_malloc para guardar o conteúdo da btree, se possível. Espaço novo
/// de z_malloc é alocado se necessário. A rotina chamadora é responsável por
/// garantir que pMem seja destruído no fim.
///
/// Se a rotina falha por qualquer motivo (alocação ou leitura do disco), pMem
/// fica em estado inconsistente.
pub fn vdbe_mem_from_btree(p_cur: &mut BtCursor, offset: u32, amt: u32, p_mem: &mut Mem) -> i32 {
    let mut rc: i32;
    p_mem.flags = MEM_NULL;
    if btree_max_record_size(p_cur) < offset.wrapping_add(amt) as i64 {
        return sqlite_corrupt_bkpt(line!() as i32);
    }
    rc = vdbe_mem_clear_and_resize(p_mem, amt.wrapping_add(1) as i32);
    if SQLITE_OK == rc {
        rc = btree_payload(p_cur, offset, amt, &mut p_mem.z[..amt as usize]);
        if rc == SQLITE_OK {
            p_mem.z[amt as usize] = 0; // Área de excesso usada ao ler registros malformados
            p_mem.z_malloc = p_mem.z.clone(); // z_malloc espelha z
            p_mem.flags = MEM_BLOB;
            p_mem.n = amt as i32;
        } else {
            vdbe_mem_release(p_mem);
        }
    }
    rc
}

/// Variante de `vdbe_mem_from_btree` com deslocamento zero: se o payload cabe
/// inteiro na página local, o Mem recebe os bytes como efêmeros, sem copiar
/// para z_malloc.
pub fn vdbe_mem_from_btree_zero_offset(p_cur: &mut BtCursor, amt: u32, p_mem: &mut Mem) -> i32 {
    let mut available: u32 = 0; // Bytes disponíveis na página local da btree
    let mut rc = SQLITE_OK; // Código de retorno

    debug_assert!(!vdbe_mem_dynamic(p_mem));

    // btree_payload_fetch devolve o índice do payload local em a_data, com o
    // comprimento em `available`. Os bytes são copiados (o C só aponta para a
    // página), e o Mem os vê como efêmeros.
    let ix = btree_payload_fetch(p_cur, &mut available);
    {
        let page = p_cur.p_page.as_ref().unwrap().borrow();
        let end = (ix + available as usize).min(page.a_data.len());
        p_mem.z = page.a_data[ix.min(end)..end].to_vec();
    }

    if amt <= available {
        p_mem.flags = MEM_BLOB | MEM_EPHEM;
        p_mem.n = amt as i32;
    } else {
        rc = vdbe_mem_from_btree(p_cur, 0, amt, p_mem);
    }

    rc
}

/// O argumento pVal é sabidamente diferente de NULL. Converte-o em uma string
/// com a codificação `enc` e retorna o conteúdo terminado em zero.
fn value_to_text(p_val: &mut Mem, enc: u8) -> Option<&[u8]> {
    let utf16_aligned = SQLITE_UTF16_ALIGNED as u8;
    debug_assert!((enc & 3) == (enc & !utf16_aligned));
    debug_assert!((p_val.flags & MEM_NULL) == 0);
    if (p_val.flags & (MEM_BLOB | MEM_STR)) != 0 {
        if expand_blob(p_val) != 0 {
            return None;
        }
        p_val.flags |= MEM_STR;
        if p_val.enc != (enc & !utf16_aligned) {
            vdbe_change_encoding(p_val, enc & !utf16_aligned);
        }
        // SQLITE_PTR_TO_INT(pVal->z): o endereço do buffer é ímpar?
        if (enc & utf16_aligned) != 0
            && !p_val.z.is_empty()
            && 1 == ((p_val.z.as_ptr() as usize) & 1)
        {
            debug_assert!((p_val.flags & (MEM_EPHEM | MEM_STATIC)) != 0);
            if vdbe_mem_make_writeable(p_val) != SQLITE_OK {
                return None;
            }
        }
        vdbe_mem_nul_terminate(p_val); /* IMP: R-31275-44060 */
    } else {
        vdbe_mem_stringify(p_val, enc, 0);
    }
    if p_val.enc == (enc & !utf16_aligned) {
        Some(&p_val.z)
    } else {
        None
    }
}

/// Esta função só está disponível internamente, não faz parte da API externa.
/// Funciona de modo parecido com sqlite3_value_text(), exceto que os dados
/// voltam na codificação do segundo parâmetro, que precisa ser SQLITE_UTF16BE,
/// SQLITE_UTF16LE ou SQLITE_UTF8.
///
/// (2006-02-16:) O valor de `enc` pode ser combinado com SQLITE_UTF16_ALIGNED.
/// Nesse caso o resultado precisa estar alinhado em fronteira par de byte.
pub fn value_text(p_val: Option<&mut Mem>, enc: u8) -> Option<&[u8]> {
    let p_val = p_val?;
    debug_assert!((enc & 3) == (enc & !(SQLITE_UTF16_ALIGNED as u8)));
    if (p_val.flags & (MEM_STR | MEM_TERM)) == (MEM_STR | MEM_TERM) && p_val.enc == enc {
        return Some(&p_val.z);
    }
    if (p_val.flags & MEM_NULL) != 0 {
        return None;
    }
    value_to_text(p_val, enc)
}

/// Retorna verdadeiro se o objeto sqlite3_value pVal é um valor string ou blob
/// que usa o destrutor indicado no segundo argumento.
///
/// TODO do C: talvez um dia promover esta interface a API publicada para que
/// extensões de terceiros tenham acesso a ela.
pub fn value_is_of_class(p_val: &Mem, x_free: fn(Vec<u8>)) -> i32 {
    if (p_val.flags & (MEM_STR | MEM_BLOB)) != 0
        && (p_val.flags & MEM_DYN) != 0
        && p_val.x_del == Some(x_free)
    {
        1
    } else {
        0
    }
}

/// Cria um novo objeto sqlite3_value.
pub fn value_new(db: Option<Weak<RefCell<Sqlite3>>>) -> Option<Box<Mem>> {
    let p = Box::new(Mem {
        u: MemValue::default(),
        z: Vec::new(),
        n: 0,
        flags: MEM_NULL,
        enc: 0,
        e_subtype: 0,
        db,
        sz_malloc: 0,
        u_temp: 0,
        z_malloc: Vec::new(),
        x_del: None,
    });
    Some(p)
}

/// Objeto de contexto passado por sqlite3Stat4ProbeSetValue() até valueNew().
/// Veja os comentários de valueNew() para os detalhes.
pub struct ValueNewStat4Ctx<'a> {
    pub p_parse: &'a mut Parse,
    pub p_idx: IndexRef,
    pub pp_rec: &'a mut Option<Box<UnpackedRecord>>,
    pub i_val: i32,
}

/// Aloca e retorna um novo objeto sqlite3_value. Se o segundo argumento é
/// NULL, o objeto é alocado por sqlite3ValueNew().
///
/// Sem SQLITE_ENABLE_STAT4 (o caso do Debian 13) o contexto é ignorado e a
/// rotina é só `value_new`. O nome do C (valueNew) colide com o de
/// sqlite3ValueNew depois da conversão de nomes; aqui a versão estática ganha
/// o sufixo `_ctx`.
pub fn value_new_ctx(
    db: Option<Weak<RefCell<Sqlite3>>>,
    _p: Option<&mut ValueNewStat4Ctx>,
) -> Option<Box<Mem>> {
    value_new(db)
}


// ---- part_004.rs ----

// Nota de configuração: `valueFromFunction()` e `stat4ValueFromExpr()` só existem sob
// SQLITE_ENABLE_STAT4, que não está entre as opções de compilação do Debian 13 (ver
// CONVENTIONS.md). No C, sem STAT4, `valueFromFunction` é uma macro que vale SQLITE_OK e o ramo
// `TK_FUNCTION` de `valueFromExpr()` some junto; por isso as duas funções não são traduzidas.
// `ValueNewStat4Ctx` continua existindo (parâmetro de `value_new`), mas `p_ctx` é sempre `None`
// nos chamadores desta configuração.

/// Extrai um valor da expressão fornecida da maneira descrita acima de
/// `sqlite3ValueFromExpr()`. Aloca o objeto sqlite3_value com `value_new()`.
///
/// Se `p_ctx` é `None` e ocorre um erro depois que o objeto sqlite3_value foi alocado, ele é
/// liberado antes de retornar. Se `p_ctx` não é `None`, presume-se que o chamador libera qualquer
/// objeto alocado em todos os casos.
///
/// O `valueFromExpr` estático do C colide com o nome de `sqlite3ValueFromExpr` (que também vira
/// `value_from_expr`); a versão estática recebe o sufixo `_impl`.
pub(crate) fn value_from_expr_impl(
    db: &Rc<RefCell<sqlite3>>,
    p_expr: &Expr,
    enc: u8,
    affinity: u8,
    pp_val: &mut Option<Box<Mem>>,
    mut p_ctx: Option<&mut ValueNewStat4Ctx>,
) -> i32 {
    let mut op: i32;
    let mut p_val: Option<Box<Mem>> = None;
    let mut neg_int: i32 = 1;
    let mut z_neg: &[u8] = b"";
    let mut rc: i32 = SQLITE_OK;
    let mut p_expr: &Expr = p_expr;

    loop {
        op = p_expr.op as i32;
        if op == TK_UPLUS as i32 || op == TK_SPAN as i32 {
            p_expr = p_expr.p_left.as_deref().unwrap();
        } else {
            break;
        }
    }
    if op == TK_REGISTER as i32 {
        op = p_expr.op2 as i32;
    }

    // Expressões comprimidas só aparecem ao analisar a cláusula DEFAULT na definição de uma
    // coluna de tabela, e portanto só quando p_ctx é None.
    assert!((p_expr.flags & EP_TOKEN_ONLY) == 0 || p_ctx.is_none());

    if op == TK_CAST as i32 {
        assert!(!expr_has_property(p_expr, EP_INT_VALUE));
        let aff = affinity_type(p_expr.u.z_token.as_deref().unwrap_or(&[]), None);
        let p_left = p_expr.p_left.as_deref().unwrap();
        rc = value_from_expr_impl(db, p_left, enc, aff, pp_val, p_ctx.as_deref_mut());
        // testcase: rc != SQLITE_OK
        if let Some(v) = pp_val.as_deref_mut() {
            // Blobs de zeros só vêm de funções, não de valores literais, e funções só são
            // processadas sob STAT4.
            assert!((v.flags & MEM_ZERO) == 0);
            vdbe_mem_cast(v, aff, enc);
            value_apply_affinity(v, affinity, enc);
        }
        return rc;
    }

    // Trata inteiros negativos em um único passo. Isto é necessário no caso em que o valor é
    // -9223372036854775808. Exceto: não faz isto para literais hexadecimais.
    if op == TK_UMINUS as i32 {
        let p_left = p_expr.p_left.as_deref().unwrap();
        if p_left.op as i32 == TK_INTEGER as i32 || p_left.op as i32 == TK_FLOAT as i32 {
            let tok = p_left.u.z_token.as_deref().unwrap_or(&[]);
            if expr_has_property(p_left, EP_INT_VALUE)
                || tok.first().copied().unwrap_or(0) != b'0'
                || (tok.get(1).copied().unwrap_or(0) & !0x20u8) != b'X'
            {
                p_expr = p_left;
                op = p_expr.op as i32;
                neg_int = -1;
                z_neg = b"-";
            }
        }
    }

    'no_mem: {
        if op == TK_STRING as i32 || op == TK_FLOAT as i32 || op == TK_INTEGER as i32 {
            p_val = value_new(db, p_ctx.as_deref_mut());
            let v = match p_val.as_deref_mut() {
                Some(v) => v,
                None => break 'no_mem,
            };
            if expr_has_property(p_expr, EP_INT_VALUE) {
                vdbe_mem_set_int64(v, (p_expr.u.i_value as i64) * (neg_int as i64));
            } else {
                let tok = p_expr.u.z_token.as_deref().unwrap_or(&[]);
                let mut i_val: i64 = 0;
                if op == TK_INTEGER as i32 && 0 == dec_or_hex_to_i64(tok, &mut i_val) {
                    vdbe_mem_set_int64(v, i_val * neg_int as i64);
                } else {
                    // sqlite3MPrintf(db, "%s%s", zNeg, token): o Vec não falha por falta de
                    // memória, então o desvio para no_mem não ocorre aqui.
                    let mut z_val: Vec<u8> = Vec::with_capacity(z_neg.len() + tok.len() + 1);
                    z_val.extend_from_slice(z_neg);
                    z_val.extend_from_slice(tok);
                    value_set_str(v, -1, z_val, SQLITE_UTF8 as u8, SQLITE_DYNAMIC);
                }
            }
            if affinity == SQLITE_AFF_BLOB {
                if op == TK_FLOAT as i32 {
                    assert!(!v.z.is_empty() && v.flags == (MEM_STR | MEM_TERM));
                    let mut r: f64 = 0.0;
                    ato_f(&v.z[0..v.n as usize], &mut r, SQLITE_UTF8 as u8);
                    v.u.r = r;
                    v.flags = MEM_REAL;
                } else if op == TK_INTEGER as i32 {
                    // Este caso é exigido por -9223372036854775808 e outras strings que parecem
                    // inteiros mas não podem ser tratadas pela chamada a dec_or_hex_to_i64()
                    // acima.
                    value_apply_affinity(v, SQLITE_AFF_NUMERIC, SQLITE_UTF8 as u8);
                }
            } else {
                value_apply_affinity(v, affinity, SQLITE_UTF8 as u8);
            }
            assert!((v.flags & MEM_INTREAL) == 0);
            if (v.flags & (MEM_INT | MEM_INTREAL | MEM_REAL)) != 0 {
                // testcase: v.flags & MEM_INT
                // testcase: v.flags & MEM_REAL
                v.flags &= !MEM_STR;
            }
            if enc != SQLITE_UTF8 as u8 {
                rc = vdbe_change_encoding(v, enc);
            }
        } else if op == TK_UMINUS as i32 {
            // Este ramo acontece para vários sinais negativos. Ex: -(-5)
            let p_left = p_expr.p_left.as_deref().unwrap();
            if SQLITE_OK == value_from_expr_impl(db, p_left, enc, affinity, &mut p_val, p_ctx.as_deref_mut())
                && p_val.is_some()
            {
                let v = p_val.as_deref_mut().unwrap();
                vdbe_mem_numerify(v);
                if (v.flags & MEM_REAL) != 0 {
                    v.u.r = -v.u.r;
                } else if v.u.i == SMALLEST_INT64 {
                    // SQLITE_OMIT_FLOATING_POINT não está definido.
                    v.u.r = -(SMALLEST_INT64 as f64);
                    mem_set_type_flag(v, MEM_REAL);
                } else {
                    v.u.i = -v.u.i;
                }
                value_apply_affinity(v, affinity, enc);
            }
        } else if op == TK_NULL as i32 {
            p_val = value_new(db, p_ctx.as_deref_mut());
            let v = match p_val.as_deref_mut() {
                Some(v) => v,
                None => break 'no_mem,
            };
            vdbe_mem_set_null(v);
        } else if op == TK_BLOB as i32 {
            assert!(!expr_has_property(p_expr, EP_INT_VALUE));
            let tok = p_expr.u.z_token.as_deref().unwrap_or(&[]);
            assert!(tok.first().copied().unwrap_or(0) == b'x' || tok.first().copied().unwrap_or(0) == b'X');
            assert!(tok.get(1).copied().unwrap_or(0) == b'\'');
            p_val = value_new(db, p_ctx.as_deref_mut());
            let v = match p_val.as_deref_mut() {
                Some(v) => v,
                None => break 'no_mem,
            };
            let z_val: &[u8] = &tok[2..];
            let n_val: i32 = strlen30_nn(z_val) as i32 - 1;
            assert!(z_val[n_val as usize] == b'\'');
            vdbe_mem_set_str(v, hex_to_blob(db, z_val, n_val), (n_val / 2) as i64, 0, SQLITE_DYNAMIC);
        } else if op == TK_TRUEFALSE as i32 {
            assert!(!expr_has_property(p_expr, EP_INT_VALUE));
            p_val = value_new(db, p_ctx.as_deref_mut());
            if let Some(v) = p_val.as_deref_mut() {
                let tok = p_expr.u.z_token.as_deref().unwrap_or(&[]);
                v.flags = MEM_INT;
                v.u.i = if tok.get(4).copied().unwrap_or(0) == 0 { 1 } else { 0 };
                value_apply_affinity(v, affinity, enc);
            }
        }

        *pp_val = p_val;
        return rc;
    }

    // no_mem:
    oom_fault(db);
    // sqlite3DbFree(db, zVal): o Vec do texto já foi liberado ao sair do escopo.
    assert!(pp_val.is_none());
    assert!(p_ctx.is_none());
    value_free(p_val);
    SQLITE_NOMEM_BKPT
}

/// Cria um novo objeto sqlite3_value contendo o valor de `p_expr`.
///
/// Só funciona para expressões muito simples que consistem em um token constante (isto é, "5",
/// "5.1", "'uma string'"). Se a expressão pode ser convertida diretamente em um valor, o valor é
/// alocado e escrito em `pp_val`. O chamador é responsável por liberar o valor depois, passando-o
/// a `value_free()`. Se a expressão não pode ser convertida em valor, `pp_val` fica `None`.
pub fn value_from_expr(
    db: &Rc<RefCell<sqlite3>>,
    p_expr: Option<&Expr>,
    enc: u8,
    affinity: u8,
    pp_val: &mut Option<Box<Mem>>,
) -> i32 {
    match p_expr {
        Some(e) => value_from_expr_impl(db, e, enc, affinity, pp_val, None),
        None => 0,
    }
}


// ---- part_005.rs ----

// As funções sqlite3Stat4ProbeSetValue, sqlite3Stat4ValueFromExpr,
// sqlite3Stat4Column e sqlite3Stat4ProbeFree ficam sob
// `#ifdef SQLITE_ENABLE_STAT4`, opção que a build do Debian 13 não define
// (ver CONVENTIONS.md), então o ramo some, como manda a convenção.

/// Muda o valor string de um objeto sqlite3_value.
///
/// No C o ponteiro `v` pode ser nulo e a chamada vira um no-op; aqui o
/// chamador que tem `Option<&mut Mem>` só chama quando há valor.
///
/// A assinatura casa com o uso em `value_from_expr_impl`: o texto entra por valor (`Vec<u8>`), `n`
/// é `i64` como em `vdbe_mem_set_str`, e `x_del` é o destrutor (SQLITE_DYNAMIC, SQLITE_STATIC...)
/// no mesmo tipo que `vdbe_mem_set_str` espera.
pub fn value_set_str(v: &mut Mem, n: i64, z: Vec<u8>, enc: u8, x_del: Option<fn(Vec<u8>)>) {
    vdbe_mem_set_str(v, z, n, enc, x_del);
}

/// Libera um objeto sqlite3_value.
pub fn value_free(v: Option<Box<Mem>>) {
    let mut v = match v {
        Some(v) => v,
        None => return,
    };
    vdbe_mem_release(&mut v);
    // A liberação da memória do próprio objeto (sqlite3DbFreeNN) é o drop do Box.
    drop(v);
}

/// A rotina sqlite3ValueBytes() devolve o número de bytes do objeto
/// sqlite3_value supondo que ele use a codificação `enc`. A rotina
/// value_bytes() é uma função auxiliar.
/// Colide com o nome público, então a auxiliar estática leva o sufixo `_helper`.
fn value_bytes_helper(p_val: &mut Mem, enc: u8) -> i32 {
    if value_to_text(p_val, enc).is_some() {
        p_val.n
    } else {
        0
    }
}

pub fn value_bytes(p_val: &mut Mem, enc: u8) -> i32 {
    debug_assert!((p_val.flags & MEM_NULL) == 0 || (p_val.flags & (MEM_STR | MEM_BLOB)) == 0);
    if (p_val.flags & MEM_STR) != 0 && p_val.enc == enc {
        return p_val.n;
    }
    if (p_val.flags & MEM_STR) != 0 && enc != SQLITE_UTF8 as u8 && p_val.enc != SQLITE_UTF8 as u8 {
        return p_val.n;
    }
    if (p_val.flags & MEM_BLOB) != 0 {
        if (p_val.flags & MEM_ZERO) != 0 {
            return p_val.n + p_val.u.n_zero;
        } else {
            return p_val.n;
        }
    }
    if (p_val.flags & MEM_NULL) != 0 {
        return 0;
    }
    value_bytes_helper(p_val, enc)
}

