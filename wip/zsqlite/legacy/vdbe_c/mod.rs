// Mesclado das partes traduzidas de vdbe_c (as partes quebram funções grandes no meio).
#![allow(unused_imports, ambiguous_glob_reexports)]
use crate::prelude::*;

// ---- part_000.rs ----

/// Aloca o VdbeCursor número i_cur. Retorna None se ficarmos sem memória.
///
/// A célula de memória que guarda o espaço do VdbeCursor é conveniente porque
/// números de cursor podem ser reusados para fins diferentes num mesmo programa
/// vdbe (cada uso pode pedir um tamanho) e porque as células oferecem alocação
/// expansível.
///
/// A célula de memória do cursor 0 é a_mem[0]. As demais são alocadas a partir
/// do topo do espaço de registros: o cursor 1 fica em a_mem[n_mem-1], o cursor 2
/// em a_mem[n_mem-2], e assim por diante.
pub fn allocate_cursor(
    p: &mut Vdbe,
    i_cur: i32,
    n_field: i32,
    e_cur_type: u8,
) -> Option<VdbeCursorRef> {
    let p_mem_index = if i_cur > 0 {
        (p.n_mem - i_cur) as usize
    } else {
        0
    };

    // Tamanho do bloco que o C reservaria. No modelo sem ponteiros o cursor é um
    // objeto próprio, mas o contador sz_malloc da célula segue a mesma conta.
    let n_byte: i32 = round8p(std::mem::size_of::<VdbeCursor>() as i32)
        + 2 * 4 * n_field
        + if e_cur_type == CURTYPE_BTREE {
            btree_cursor_size()
        } else {
            0
        };

    assert!(i_cur >= 0 && i_cur < p.n_cursor);
    if let Some(old_cursor) = p.ap_csr[i_cur as usize].take() {
        vdbe_free_cursor_nn(p, old_cursor);
    }

    // Antes havia uma chamada a sqlite3VdbeMemClearAndResize() para garantir
    // espaço em z_malloc. Para as células a_mem[] que guardam cursores é mais
    // rápido embutir a lógica.
    let p_mem = &mut p.a_mem[p_mem_index];
    assert_eq!(p_mem.flags, MEM_UNDEFINED);
    assert_eq!(p_mem.flags & MEM_DYN, 0);
    if p_mem.sz_malloc < n_byte {
        p_mem.z_malloc = vec![0u8; n_byte as usize];
        p_mem.z = Vec::new();
        p_mem.sz_malloc = n_byte;
    }

    let mut cursor = VdbeCursor::default();
    cursor.e_cur_type = e_cur_type;
    cursor.n_field = n_field as i16;
    cursor.a_type = vec![0u32; n_field as usize];
    cursor.a_offset = vec![0u32; n_field as usize];
    if e_cur_type == CURTYPE_BTREE {
        let mut bt_cursor = BtCursor::default();
        btree_cursor_zero(&mut bt_cursor);
        cursor.uc = VdbeCursorCursorUnion::PCursor(Box::new(bt_cursor));
    }

    let p_cx_ref = Rc::new(RefCell::new(cursor));
    p.ap_csr[i_cur as usize] = Some(p_cx_ref.clone());
    Some(p_cx_ref)
}

/// A string em p_rec é conhecida por parecer um inteiro e ter o valor de ponto
/// flutuante r_value. Retorna verdadeiro e grava o inteiro em *pi_value se a
/// string estiver no intervalo de um inteiro. Caso contrário, retorna falso.
fn also_an_int(p_rec: &Mem, r_value: f64, pi_value: &mut i64) -> bool {
    let i_value = real_to_i64(r_value);
    if real_same_as_int(r_value, i_value) {
        *pi_value = i_value;
        return true;
    }
    atoi64(&p_rec.z, pi_value, p_rec.n, p_rec.enc) == 0
}

/// Tenta converter um valor em representação numérica se for possível sem perda
/// de informação. Se a string parecer um número, converte-a; senão, deixa-a como
/// está.
///
/// Se b_try_for_int for verdadeiro, faz-se esforço extra para dar uma
/// representação inteira. Strings que parecem ponto flutuante mas sem parte
/// fracionária (exemplo: '48.00') ganham representação MEM_INT.
///
/// Se b_try_for_int for falso e a string tiver ponto decimal ou notação
/// exponencial, o resultado é só MEM_REAL, mesmo que haja representação inteira
/// exata.
pub fn apply_numeric_affinity(p_rec: &mut Mem, b_try_for_int: bool) {
    assert_eq!(p_rec.flags & (MEM_STR | MEM_INT | MEM_REAL | MEM_INTREAL), MEM_STR);
    let enc = p_rec.enc;
    let mut r_value: f64 = 0.0;
    let rc = ato_f(&p_rec.z, &mut r_value, p_rec.n, enc);
    if rc <= 0 {
        return;
    }
    let mut i_value: i64 = 0;
    if rc == 1 && also_an_int(p_rec, r_value, &mut i_value) {
        p_rec.u.i = i_value;
        p_rec.flags |= MEM_INT;
    } else {
        p_rec.u.r = r_value;
        p_rec.flags |= MEM_REAL;
        if b_try_for_int {
            vdbe_integer_affinity(p_rec);
        }
    }
    // TEXT->NUMERIC é muitos para um. Portanto é importante invalidar a
    // representação da string depois de calcular o equivalente numérico, porque
    // ela pode não ser a representação canônica do valor. Ticket
    // [343634942dd54ab57b7024] 2018-01-31.
    p_rec.flags &= !MEM_STR;
}


// ---- part_001.rs ----

// Notas de integração para o tech lead:
// - Tradução de vdbe.c (3.46.1), parte 1: de `applyAffinity` até `vdbeColumnFromOverflow`.
// - Sem SQLITE_DEBUG somem `sqlite3VdbeMemPrettyPrint`, `memTracePrint`, `registerTrace`,
//   `sqlite3PrintMem`, `sqlite3VdbeRegisterDump` e a macro REGISTER_TRACE (vazia). Também some
//   a chamada `memAboutToChange` de `out2Prerelease` (a macro é vazia sem SQLITE_DEBUG).
// - `checkSavepointCount` existe só para uso dentro de assert; fica como `check_savepoint_count`
//   para ser chamada de `debug_assert!`.
// - `sqlite3_value_numeric_type` está aqui por estar em vdbe.c, mas é API pública: o lead a
//   reexporta em `api` como `api::value_numeric_type`. O corpo chama `api::value_type`.
// - `sqlite3ValueApplyAffinity` só repassa para `applyAffinity` com os mesmos argumentos. Pela
//   regra de DRY do projeto vira reexportação do nome (`value_apply_affinity`), não função.
// - Modelo do RCStr (cadeia com contagem de referências): o cache de `VdbeTxtBlbCache` guarda um
//   `Vec<u8>` dono único (`p_c_value`). `sqlite3RCStrNew(n)` vira `vec![0u8; n]`, `RCStrUnref`
//   vira o drop do vetor e `RCStrRef` não existe. O Mem recebe uma cópia dos bytes do cache, então
//   a referência que o C pega e o destrutor `RCStrUnref` que o Mem carrega se cancelam. Como o
//   `Destructor` de `vdbe_mem_set_str` só tem `SQLITE_STATIC` e `SQLITE_TRANSIENT`, a chamada usa
//   `SQLITE_STATIC`. A falha de alocação (ramos `SQLITE_NOMEM`) não existe em Rust.
// - Registradores: `p.a_mem` é `Vec<Mem>` e os opcodes guardam índices `usize` (ver part_002).
//   Assinaturas assumidas: `ato_f(&[u8], &mut f64, n, enc) -> i32` e
//   `atoi64(&[u8], &mut i64, n, enc) -> i32`, como no C, com `n: i32` e `enc: u8`.
// - Nomes assumidos por convenção, ainda não traduzidos: `apply_numeric_affinity` (vdbe.c parte 0),
//   `vdbe_serial_type_len(u32) -> u32`, `vdbe_serial_get(&[u8], u32, &mut Mem) -> u32`,
//   `btree_offset(&mut BtCursor) -> i64`, `vdbe_int_value(&Mem) -> i64`, `api::value_type(&Mem) -> i32`.

/// Aplica a afinidade ao registro. O processamento depende do parâmetro `affinity`:
///
/// * SQLITE_AFF_INTEGER, SQLITE_AFF_REAL, SQLITE_AFF_NUMERIC: tenta converter para inteiro ou,
///   se não for possível, para ponto flutuante. A representação inteira é sempre preferida,
///   mesmo com afinidade REAL, porque ocupa menos espaço em disco.
/// * SQLITE_AFF_FLEXNUM: se o valor é texto, tenta convertê-lo num número de algum tipo, sem
///   fazer nenhuma outra mudança.
/// * SQLITE_AFF_TEXT: converte para a representação em texto.
/// * SQLITE_AFF_BLOB e SQLITE_AFF_NONE: nada a fazer, o registro não muda.
pub fn apply_affinity(p_rec: &mut Mem, affinity: u8, enc: u8) {
    if affinity >= SQLITE_AFF_NUMERIC {
        debug_assert!(
            affinity == SQLITE_AFF_INTEGER
                || affinity == SQLITE_AFF_REAL
                || affinity == SQLITE_AFF_NUMERIC
                || affinity == SQLITE_AFF_FLEXNUM
        );
        if (p_rec.flags & MEM_INT) == 0 {
            // OPTIMIZATION-IF-FALSE
            if (p_rec.flags & (MEM_REAL | MEM_INTREAL)) == 0 {
                if (p_rec.flags & MEM_STR) != 0 {
                    apply_numeric_affinity(p_rec, true);
                }
            } else if affinity <= SQLITE_AFF_REAL {
                vdbe_integer_affinity(p_rec);
            }
        }
    } else if affinity == SQLITE_AFF_TEXT {
        // Só tenta a conversão para TEXT se há uma representação inteira ou real mas nenhuma
        // representação em string (blob e NULL não são convertidos). Repetir a conversão quando
        // já existe string seria inofensivo, mas é desperdício de CPU.
        if 0 == (p_rec.flags & MEM_STR) {
            // OPTIMIZATION-IF-FALSE
            if (p_rec.flags & (MEM_REAL | MEM_INT | MEM_INTREAL)) != 0 {
                vdbe_mem_stringify(p_rec, enc, 1);
            }
        }
        p_rec.flags &= !(MEM_REAL | MEM_INT | MEM_INTREAL);
    }
}

/// Tenta converter o tipo de um argumento de função ou de uma coluna de resultado para uma
/// representação numérica, INTEGER ou REAL conforme o caso. Só converte se for possível sem
/// perda de informação, e devolve o tipo revisado do argumento.
pub fn value_numeric_type(p_val: &mut Mem) -> i32 {
    let mut e_type = api::value_type(p_val);
    if e_type == SQLITE_TEXT {
        apply_numeric_affinity(p_val, false);
        e_type = api::value_type(p_val);
    }
    e_type
}

/// Versão exportada de `apply_affinity()`: trabalha sobre `sqlite3_value`, não sobre o Mem
/// interno. Os dois são o mesmo tipo neste porte, então o nome é só uma reexportação.
pub use self::apply_affinity as value_apply_affinity;

/// O pMem guarda por ora apenas um tipo string (ou talvez um BLOB que possa ser interpretado como
/// string). Calcula o tipo numérico correspondente, se tiver um, e preenche os campos `u.r` e
/// `u.i` de acordo.
pub fn compute_numeric_type(p_mem: &mut Mem) -> u16 {
    let mut ix: i64 = 0;
    debug_assert!((p_mem.flags & (MEM_INT | MEM_REAL | MEM_INTREAL)) == 0);
    debug_assert!((p_mem.flags & (MEM_STR | MEM_BLOB)) != 0);
    if expand_blob(p_mem) != 0 {
        p_mem.u.i = 0;
        return MEM_INT;
    }
    let rc = ato_f(&p_mem.z, &mut p_mem.u.r, p_mem.n, p_mem.enc);
    if rc <= 0 {
        if rc == 0 && atoi64(&p_mem.z, &mut ix, p_mem.n, p_mem.enc) <= 1 {
            p_mem.u.i = ix;
            return MEM_INT;
        } else {
            return MEM_REAL;
        }
    } else if rc == 1 && atoi64(&p_mem.z, &mut ix, p_mem.n, p_mem.enc) == 0 {
        p_mem.u.i = ix;
        return MEM_INT;
    }
    MEM_REAL
}

/// Devolve o tipo numérico de pMem: MEM_INT, MEM_REAL, ambos ou nenhum.
///
/// Ao contrário de `apply_numeric_affinity()`, esta rotina não modifica `p_mem.flags`, mas
/// preenche `u.r` e `u.i` de forma apropriada.
pub fn numeric_type(p_mem: &mut Mem) -> u16 {
    debug_assert!(
        (p_mem.flags & MEM_NULL) == 0
            || p_mem.db.as_ref().and_then(|w| w.upgrade()).map_or(true, |d| {
                d.try_borrow().map_or(true, |b| b.malloc_failed != 0)
            })
    );
    if (p_mem.flags & (MEM_INT | MEM_REAL | MEM_INTREAL | MEM_NULL)) != 0 {
        return p_mem.flags & (MEM_INT | MEM_REAL | MEM_INTREAL | MEM_NULL);
    }
    debug_assert!((p_mem.flags & (MEM_STR | MEM_BLOB)) != 0);
    // O `return 0;` que o C deixa depois desta chamada é inalcançável.
    compute_numeric_type(p_mem)
}

/// Só é chamada de dentro de um assert: confere que `db.n_savepoint` está certo, isto é, que é
/// o número de savepoints que não são de transação na lista que começa em `db.p_savepoint`.
///
/// Uso: `debug_assert!(check_savepoint_count(db))`.
pub fn check_savepoint_count(db: &Sqlite3) -> bool {
    let mut n: i32 = 0;
    let mut p: Option<SavepointRef> = db.p_savepoint.clone();
    while let Some(sp) = p {
        n += 1;
        p = sp.borrow().p_next.clone();
    }
    debug_assert!(n == (db.n_savepoint + db.is_transaction_savepoint as i32));
    true
}

/// Parte fora do caminho comum de `out2_prerelease`: limpa o registro antes de ele receber um
/// valor inteiro.
pub fn out2_prerelease_with_clear(p_out: &mut Mem) {
    vdbe_mem_set_null(p_out);
    p_out.flags = MEM_INT;
}

/// Devolve o índice do registrador `p_op.p2` em `p.a_mem` depois de prepará-lo para ser
/// sobrescrito com um valor inteiro.
pub fn out2_prerelease(p: &mut Vdbe, p_op: &Op) -> usize {
    debug_assert!(p_op.p2 > 0);
    debug_assert!(p_op.p2 <= (p.n_mem + 1 - p.n_cursor));
    let idx = p_op.p2 as usize;
    let p_out = &mut p.a_mem[idx];
    if vdbe_mem_dynamic(p_out) {
        // OPTIMIZATION-IF-FALSE
        out2_prerelease_with_clear(p_out);
    } else {
        p_out.flags = MEM_INT;
    }
    idx
}

/// Calcula o hash do filtro de bloom usando os `p_op.p4.i` registros de `a_mem[]` a partir de
/// `p_op.p3`. Devolve o hash.
pub fn filter_hash(a_mem: &[Mem], p_op: &Op) -> u64 {
    let mut h: u64 = 0;

    debug_assert!(p_op.p4type == P4_INT32);
    let n_reg: i32 = match p_op.p4 {
        P4Value::Int32(i) => i,
        _ => 0,
    };
    let mx = p_op.p3 + n_reg;
    for i in p_op.p3..mx {
        let p = &a_mem[i as usize];
        if (p.flags & (MEM_INT | MEM_INTREAL)) != 0 {
            h = h.wrapping_add(p.u.i as u64);
        } else if (p.flags & MEM_REAL) != 0 {
            h = h.wrapping_add(vdbe_int_value(p) as u64);
        } else if (p.flags & (MEM_STR | MEM_BLOB)) != 0 {
            // Todas as strings têm o mesmo hash e todos os blobs têm o mesmo hash, mas pelo
            // menos esses hashes são diferentes entre si e de NULL.
            h = h.wrapping_add(4093u64 + (p.flags & (MEM_STR | MEM_BLOB)) as u64);
        }
    }
    h
}

/// Para OP_Column: separa o caso em que o conteúdo é carregado de páginas de overflow, para que
/// o código desse caso fique à parte do caso comum em que todo o conteúdo cabe na página.
/// Separar o código reduz a pressão de registradores e ajuda o caso comum a rodar mais rápido.
///
/// * `p_c`: o cursor de btree do qual se lê.
/// * `i_col`: a coluna a ler.
/// * `t`: o código do tipo serial do valor da coluna.
/// * `i_offset`: deslocamento até o início do valor.
/// * `cache_status`: valor atual de `Vdbe.cache_ctr`.
/// * `col_cache_ctr`: valor atual do contador do cache de coluna.
/// * `p_dest`: registrador onde o valor é guardado.
pub fn vdbe_column_from_overflow(
    p_c: &mut VdbeCursor,
    i_col: i32,
    t: i32,
    i_offset: i64,
    cache_status: u32,
    col_cache_ctr: u32,
    p_dest: &mut Mem,
) -> i32 {
    let mut rc: i32;
    let length_limit: i32 = match p_dest.db.as_ref().and_then(|w| w.upgrade()) {
        Some(db) => {
            let limit = db.borrow().a_limit[SQLITE_LIMIT_LENGTH as usize];
            limit
        }
        None => SQLITE_MAX_LENGTH as i32,
    };
    let encoding: i32 = p_dest.enc as i32;
    let len: i32 = vdbe_serial_type_len(t as u32) as i32;
    debug_assert!(p_c.e_cur_type == CURTYPE_BTREE);
    if len > length_limit {
        return SQLITE_TOOBIG;
    }
    let cursor: &mut BtCursor = match &mut p_c.uc {
        VdbeCursorCursorUnion::PCursor(c) => &mut **c,
        _ => unreachable!("cursor de btree esperado em vdbe_column_from_overflow"),
    };
    if len > 4000 && p_c.p_key_info.is_none() {
        // Guarda em cache os valores de coluna grandes que estão em páginas de overflow, usando
        // um RCStr (cadeia com contagem de referências), para que, se forem relidos, não seja
        // preciso copiá-los uma segunda vez. O custo de criar e gerenciar o cache só compensa
        // para valores TEXT e BLOB maiores.
        //
        // Só se faz isso em btrees de tabela, para que escritas em btrees de índice não precisem
        // limpar o cache. Isso ganha desempenho no caso comum em troca de generalidade.
        if p_c.col_cache == 0 {
            p_c.p_cache = Some(Box::new(VdbeTxtBlbCache {
                p_c_value: None,
                i_offset: 0,
                i_col: 0,
                cache_status: 0,
                col_cache_ctr: 0,
            }));
            p_c.col_cache = 1;
        }
        let p_cache: &mut VdbeTxtBlbCache = match p_c.p_cache.as_mut() {
            Some(c) => &mut **c,
            None => unreachable!("cache de coluna criado logo acima"),
        };
        if p_cache.p_c_value.is_none()
            || p_cache.i_col != i_col
            || p_cache.cache_status != cache_status
            || p_cache.col_cache_ctr != col_cache_ctr
            || p_cache.i_offset != btree_offset(cursor)
        {
            // Solta o valor antigo (RCStrUnref) e instala o novo buffer (RCStrNew), com três
            // bytes zero depois do conteúdo.
            p_cache.p_c_value = None;
            p_cache.p_c_value = Some(vec![0u8; len as usize + 3]);
            let p_buf: &mut Vec<u8> = match p_cache.p_c_value.as_mut() {
                Some(b) => b,
                None => unreachable!("buffer do cache instalado logo acima"),
            };
            rc = btree_payload(cursor, i_offset as u32, len as u32, &mut p_buf[..len as usize]);
            if rc != 0 {
                return rc;
            }
            p_buf[len as usize] = 0;
            p_buf[len as usize + 1] = 0;
            p_buf[len as usize + 2] = 0;
            p_cache.i_col = i_col;
            p_cache.cache_status = cache_status;
            p_cache.col_cache_ctr = col_cache_ctr;
            p_cache.i_offset = btree_offset(cursor);
        }
        let p_buf: &Vec<u8> = match p_cache.p_c_value.as_ref() {
            Some(b) => b,
            None => unreachable!("o cache tem buffer neste ponto"),
        };
        debug_assert!(t >= 12);
        // O Mem recebe uma cópia dos bytes do cache (ver as notas no alto do arquivo).
        if (t & 1) != 0 {
            rc = vdbe_mem_set_str(p_dest, Some(&p_buf[..]), len as i64, encoding as u8, SQLITE_STATIC);
            p_dest.flags |= MEM_TERM;
        } else {
            rc = vdbe_mem_set_str(p_dest, Some(&p_buf[..]), len as i64, 0, SQLITE_STATIC);
        }
    } else {
        rc = vdbe_mem_from_btree(cursor, i_offset as u32, len as u32, p_dest);
        if rc != 0 {
            return rc;
        }
        // `p_dest.z` é lido enquanto o próprio Mem é escrito, então a leitura vai sobre uma cópia.
        let raw: Vec<u8> = p_dest.z.clone();
        vdbe_serial_get(&raw, t as u32, p_dest);
        if (t & 1) != 0 && encoding == SQLITE_UTF8 as i32 {
            let end = len as usize;
            if p_dest.z.len() <= end {
                p_dest.z.resize(end + 1, 0);
            }
            p_dest.z[end] = 0;
            p_dest.flags |= MEM_TERM;
        }
    }
    p_dest.flags &= !MEM_EPHEM;
    rc
}


// ---- part_002.rs ----

// Notas de integração para o tech lead:
// - `sqlite3VdbeExec` é uma função única no C, mas o tradutor a cortou em vários trechos. Em Rust
//   ela vira: `ExecCtx` (os locais da função), `vdbe_exec_start` (o prólogo), `vdbe_exec_step_begin`
//   (o topo do laço) e uma função `op_xxx(&mut ExecCtx) -> Flow` por `case OP_Xxx`, nas partes
//   seguintes. O `vdbe_exec` final (laço, `match` sobre o opcode, rótulos de aborto) é montado pelo
//   lead no `mod.rs`.
// - `break` do switch vira `Flow::Next` (o laço faz `pc += 1`, como o `pOp++` do for). Cada `goto`
//   para rótulo fora do case vira variante de `Flow`: `NoMem`, `AbortDueToError`,
//   `AbortDueToInterrupt`. As partes seguintes acrescentam as variantes que faltarem.
// - Registradores (`pIn1`, `pOut`...) são índices `usize` em `p.a_mem: Vec<Mem>`, porque o C mantém
//   vários ponteiros vivos para o mesmo array. `p_op` é o índice `pc` em `p.a_op`.
// - Sob as opções do Debian não há SQLITE_DEBUG, SQLITE_TEST, VDBE_PROFILE, STMT_SCANSTATUS nem
//   VDBE_COVERAGE: somem o trace, a listagem, as checagens de IN1/IN2/IN3, `test_trace_breakpoint`,
//   `memAboutToChange`, `REGISTER_TRACE`, `VdbeBranchTaken` e o ramo `p5` do OP_Goto.
// - Callback de progresso: `x_progress: Option<Rc<dyn Fn() -> i32>>` (o argumento do C fica na
//   captura do closure). `sqlite3VdbeIOTraceSql` é macro vazia sem SQLITE_ENABLE_IOTRACE.
// - Nomes assumidos por convenção: `vdbe_enter(&VdbeRef)`, `atomic_load`, `vdbe_mem_dynamic(&Mem)`,
//   `api::value_type(&Mem)`, `enc(&Sqlite3) -> u8`, `db_mask_non_zero(DbMask)`.

/// Retorna o nome simbólico para o tipo de dado de um Mem.
pub fn vdbe_mem_type_name(p_mem: &Mem) -> &'static [u8] {
    const AZ_TYPES: [&[u8]; 5] = [
        b"INT",  // SQLITE_INTEGER
        b"REAL", // SQLITE_FLOAT
        b"TEXT", // SQLITE_TEXT
        b"BLOB", // SQLITE_BLOB
        b"NULL", // SQLITE_NULL
    ];
    AZ_TYPES[(api::value_type(p_mem) - 1) as usize]
}

/// Destino de controle de um `case OP_Xxx`. `Next` é o `break` do switch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Flow {
    Next,
    NoMem,
    AbortDueToError,
    AbortDueToInterrupt,
    /// `goto vdbe_return`: devolve `ctx.rc`.
    VdbeReturn,
    /// `goto too_big`.
    TooBig,
}

/// Cópia da instrução atual (`*pOp`). Os opcodes que mudam a instrução (OP_String8) gravam de
/// volta em `p.a_op[pc]`; os demais só leem.
pub fn cur_op(ctx: &ExecCtx) -> Op {
    ctx.p.borrow().a_op[ctx.pc].clone()
}

/// Os locais de `sqlite3VdbeExec`.
pub struct ExecCtx {
    pub p: VdbeRef,
    pub pc: usize,
    pub rc: i32,
    pub db: Sqlite3Ref,
    pub reset_schema_on_fault: u8,
    pub encoding: u8,
    pub i_compare: i32,
    pub n_vm_step: u64,
    pub n_progress_limit: u64,
    pub p_in1: usize,
    pub p_in2: usize,
    pub p_in3: usize,
    pub p_out: usize,
    pub col_cache_ctr: u32,
}

/// Prólogo de `sqlite3VdbeExec`. Devolve o contexto e, se o C faria `goto` antes de entrar no laço,
/// o destino do desvio.
pub fn vdbe_exec_start(p: &VdbeRef) -> (ExecCtx, Option<Flow>) {
    let (pc, db) = {
        let pb = p.borrow();
        (pb.pc as usize, pb.db.clone())
    };
    let encoding = enc(&db.borrow());
    let mut ctx = ExecCtx {
        p: p.clone(),
        pc,
        rc: SQLITE_OK,
        db: db.clone(),
        reset_schema_on_fault: 0,
        encoding,
        i_compare: 0,
        n_vm_step: 0,
        n_progress_limit: LARGEST_UINT64,
        p_in1: 0,
        p_in2: 0,
        p_in3: 0,
        p_out: 0,
        col_cache_ctr: 0,
    };

    {
        let pb = p.borrow();
        assert!(pb.e_vdbe_state == VDBE_RUN_STATE); // sqlite3_step() verifica isso
        if db_mask_non_zero(&pb.lock_mask) {
            drop(pb);
            vdbe_enter(p);
        }
    }
    {
        let dbb = db.borrow();
        if dbb.x_progress.is_some() {
            let i_prior = p.borrow().a_counter[SQLITE_STMTSTATUS_VM_STEP as usize] as u64;
            assert!(0 < dbb.n_progress_ops);
            let ops = dbb.n_progress_ops as u64;
            ctx.n_progress_limit = ops - (i_prior % ops);
        } else {
            ctx.n_progress_limit = LARGEST_UINT64;
        }
    }
    {
        let mut pb = p.borrow_mut();
        if pb.rc == SQLITE_NOMEM {
            // Acontece se um malloc() dentro de sqlite3_column_text() ou
            // sqlite3_column_text16() falhou.
            return (ctx, Some(Flow::NoMem));
        }
        assert!(pb.rc == SQLITE_OK || (pb.rc & 0xff) == SQLITE_BUSY);
        pb.rc = SQLITE_OK;
        assert!(pb.b_is_reader != 0 || pb.read_only != 0);
        pb.i_current_time = 0;
        assert!(pb.explain == 0);
    }
    db.borrow_mut().busy_handler.n_busy = 0;
    if atomic_load(&db.borrow().u1.is_interrupted) != 0 {
        return (ctx, Some(Flow::AbortDueToInterrupt));
    }
    (ctx, None)
}

/// Topo do laço `for(pOp=&aOp[p->pc]; 1; pOp++)`: contabiliza o passo.
pub fn vdbe_exec_step_begin(ctx: &mut ExecCtx) {
    // Erros são detectados por opcodes individuais, com salto imediato para abort_due_to_error.
    assert!(ctx.rc == SQLITE_OK);
    assert!(ctx.pc < ctx.p.borrow().a_op.len());
    ctx.n_vm_step += 1;
}

/// Rótulo `jump_to_p2`: a maioria dos saltos passa por aqui para atualizar o pOp.
pub fn jump_to_p2(ctx: &mut ExecCtx) -> Flow {
    let p2 = cur_op(ctx).p2;
    assert!(p2 > 0); // Nunca há saltos para a instrução 0
    assert!(p2 < ctx.p.borrow().n_op); // Saltos precisam estar no intervalo
    ctx.pc = (p2 - 1) as usize;
    Flow::Next
}

/// Rótulo `check_for_interrupt`. Os opcodes de fundo de laço (OP_Next, OP_Prev, OP_VNext,
/// OP_SorterNext) saltam para cá ao terminar: confere se sqlite3_interrupt() foi chamada ou se o
/// callback de progresso precisa ser invocado. Se o callback devolve não zero, a máquina sai com
/// SQLITE_ABORT/SQLITE_INTERRUPT como no C.
pub fn check_for_interrupt(ctx: &mut ExecCtx) -> Flow {
    if atomic_load(&ctx.db.borrow().u1.is_interrupted) != 0 {
        return Flow::AbortDueToInterrupt;
    }
    loop {
        let (x_progress, n_ops) = {
            let dbb = ctx.db.borrow();
            (dbb.x_progress.clone(), dbb.n_progress_ops as u64)
        };
        let cb = match x_progress {
            Some(cb) if ctx.n_vm_step >= ctx.n_progress_limit => cb,
            _ => break,
        };
        assert!(n_ops != 0);
        ctx.n_progress_limit += n_ops;
        if cb() != 0 {
            ctx.n_progress_limit = LARGEST_UINT64;
            ctx.rc = SQLITE_INTERRUPT;
            return Flow::AbortDueToError;
        }
    }
    Flow::Next
}

/// Rótulo `jump_to_p2_and_check_for_interrupt`.
pub fn jump_to_p2_and_check_for_interrupt(ctx: &mut ExecCtx) -> Flow {
    ctx.pc = (cur_op(ctx).p2 - 1) as usize;
    check_for_interrupt(ctx)
}

/// Opcode: Goto * P2 * * *
///
/// Salto incondicional para o endereço P2. A próxima instrução executada é a de índice P2 desde o
/// começo do programa. O parâmetro P1 não é usado, mas às vezes vale 1 como dica ao shell de que
/// este Goto é o fim de um laço.
pub fn op_goto(ctx: &mut ExecCtx) -> Flow {
    jump_to_p2_and_check_for_interrupt(ctx)
}

/// Opcode: Gosub P1 P2 * * *
///
/// Grava o endereço atual no registrador P1 e salta para o endereço P2.
pub fn op_gosub(ctx: &mut ExecCtx) -> Flow {
    let p1 = cur_op(ctx).p1;
    {
        let pb = ctx.p.borrow();
        assert!(p1 > 0 && p1 <= (pb.n_mem + 1 - pb.n_cursor));
    }
    ctx.p_in1 = p1 as usize;
    let pc = ctx.pc as i64;
    let mut pb = ctx.p.borrow_mut();
    let p_in1 = &mut pb.a_mem[ctx.p_in1];
    assert!(!vdbe_mem_dynamic(p_in1));
    p_in1.flags = MEM_INT;
    p_in1.u.i = pc;
    drop(pb);
    jump_to_p2_and_check_for_interrupt(ctx)
}

/// Opcode: Return P1 P2 P3 * *
///
/// Salta para o endereço guardado no registrador P1. Se P1 é um registrador de endereço de
/// retorno, isso é um retorno de sub-rotina. Se P3 é 1, o salto só ocorre se P1 guarda um inteiro;
/// senão a execução segue e o OP_Return vira no-op. Se P3 é 0, P1 precisa guardar um inteiro. O
/// valor em P1 não muda. P2 não é usado pelo motor, só serve de dica de indentação ao EXPLAIN.
pub fn op_return(ctx: &mut ExecCtx) -> Flow {
    let p1 = cur_op(ctx).p1;
    ctx.p_in1 = p1 as usize;
    let (flags, i) = {
        let pb = ctx.p.borrow();
        let m = &pb.a_mem[ctx.p_in1];
        (m.flags, m.u.i)
    };
    if (flags & MEM_INT) != 0 {
        ctx.pc = i as usize;
    }
    Flow::Next
}

/// Opcode: InitCoroutine P1 P2 P3 * *
///
/// Prepara o registrador P1 para fazer Yield à corrotina no endereço P3. Se P2 != 0 a
/// implementação da corrotina vem logo depois deste opcode, então salta por cima dela para P2.
pub fn op_init_coroutine(ctx: &mut ExecCtx) -> Flow {
    let (p1, p2, p3) = {
        let op = cur_op(ctx);
        (op.p1, op.p2, op.p3)
    };
    {
        let pb = ctx.p.borrow();
        assert!(p1 > 0 && p1 <= (pb.n_mem + 1 - pb.n_cursor));
        assert!(p2 >= 0 && p2 < pb.n_op);
        assert!(p3 >= 0 && p3 < pb.n_op);
    }
    ctx.p_out = p1 as usize;
    {
        let mut pb = ctx.p.borrow_mut();
        let p_out = &mut pb.a_mem[ctx.p_out];
        assert!(!vdbe_mem_dynamic(p_out));
        p_out.u.i = (p3 - 1) as i64;
        p_out.flags = MEM_INT;
    }
    if p2 == 0 {
        return Flow::Next;
    }
    jump_to_p2(ctx)
}


// ---- part_003.rs ----

// Continuação de `sqlite3VdbeExec`: os `case OP_Xxx` de EndCoroutine a Blob. Convenções do
// `ExecCtx` e do `Flow` em `part_002.rs`. O `goto vdbe_return` vira `Flow::VdbeReturn` (com o
// valor de retorno em `ctx.rc`), `goto too_big` vira `Flow::TooBig` e `goto no_mem` vira
// `Flow::NoMem`.
// - `OP_HaltIfNull` cai em `OP_Halt`: `op_halt_if_null` chama `op_halt` no ramo NULL.
// - `OP_String8` cai em `OP_String`: `op_string8` chama `op_string` no fim.
// - `OP_BeginSubrtn` e `OP_Null` compartilham `op_null`.
// - Nomes assumidos por convenção: `strlen30(&[u8]) -> i32`, `api::log(i32, &[u8])`,
//   `vdbe_error(&mut Vdbe, &[u8])` (mensagem já formatada), `vdbe_halt(&VdbeRef) -> i32`,
//   `vdbe_frame_restore(&VdbeFrameRef) -> i32`, `vdbe_set_changes(&Sqlite3Ref, i64)`,
//   `vdbe_change_encoding(&mut Mem, u8) -> i32`, `vdbe_mem_set_zero_blob(&mut Mem, i32)`,
//   `vdbe_mem_expand_blob(&mut Mem) -> i32`, `P4Value::{NotUsed, Static(Vec<u8>),
//   Dynamic(Vec<u8>), Int64(i64), Real(f64)}` (Static e Dynamic guardam o texto de `p4.z`).

/// Texto de `pOp->p4.z`, ou None quando o ponteiro é nulo.
fn p4_text(p_op: &Op) -> Option<&Vec<u8>> {
    match &p_op.p4 {
        P4Value::Static(z) | P4Value::Dynamic(z) => Some(z),
        _ => None,
    }
}

/// Opcode: EndCoroutine P1 * * * *
///
/// A instrução no endereço do registrador P1 é um Yield. Salta para o parâmetro P2 desse Yield.
/// Depois do salto, o registrador P1 fica com um valor tal que os OP_Yield seguintes voltam para
/// este mesmo OP_EndCoroutine.
///
/// Ver também: InitCoroutine
pub fn op_end_coroutine(ctx: &mut ExecCtx) -> Flow {
    let p1 = cur_op(ctx).p1;
    ctx.p_in1 = p1 as usize;
    let pc = ctx.pc as i64;
    let pb = &mut *ctx.p.borrow_mut();
    let p_in1 = &mut pb.a_mem[ctx.p_in1];
    assert!(p_in1.flags == MEM_INT);
    assert!(p_in1.u.i >= 0 && p_in1.u.i < pb.n_op as i64);
    let caller_p2 = {
        let p_caller = &pb.a_op[p_in1.u.i as usize];
        assert!(p_caller.opcode == OP_YIELD);
        assert!(p_caller.p2 >= 0 && p_caller.p2 < pb.n_op);
        p_caller.p2
    };
    p_in1.u.i = pc - 1;
    ctx.pc = (caller_p2 - 1) as usize;
    Flow::Next
}

/// Opcode: Yield P1 P2 * * *
///
/// Troca o contador de programa com o valor do registrador P1, o que cede o controle a uma
/// corrotina. Se a corrotina lançada por esta instrução termina com Yield ou Return, continua na
/// instrução seguinte. Se termina com EndCoroutine, salta para P2.
///
/// Ver também: InitCoroutine
pub fn op_yield(ctx: &mut ExecCtx) -> Flow {
    let p1 = cur_op(ctx).p1;
    ctx.p_in1 = p1 as usize;
    let pc = ctx.pc as i64;
    let mut pb = ctx.p.borrow_mut();
    let p_in1 = &mut pb.a_mem[ctx.p_in1];
    assert!(!vdbe_mem_dynamic(p_in1));
    p_in1.flags = MEM_INT;
    let pc_dest = p_in1.u.i as i32;
    p_in1.u.i = pc;
    ctx.pc = pc_dest as usize;
    Flow::Next
}

/// Opcode: HaltIfNull P1 P2 P3 P4 P5
///
/// Confere o valor do registrador P3. Se for NULL, faz Halt com os parâmetros P1, P2 e P4 como se
/// fosse uma instrução Halt. Se não for NULL, é no-op. O parâmetro P5 deve ser 1.
pub fn op_halt_if_null(ctx: &mut ExecCtx) -> Flow {
    let p3 = cur_op(ctx).p3;
    ctx.p_in3 = p3 as usize;
    let is_null = (ctx.p.borrow().a_mem[ctx.p_in3].flags & MEM_NULL) != 0;
    if !is_null {
        return Flow::Next;
    }
    // Cai em OP_Halt.
    op_halt(ctx)
}

/// Opcode: Halt P1 P2 * P4 P5
///
/// Sai imediatamente. Todos os cursores abertos etc. são fechados automaticamente.
///
/// P1 é o código de resultado devolvido por sqlite3_exec(), sqlite3_reset() ou
/// sqlite3_finalize(); numa parada normal deve ser SQLITE_OK (0). Se P1 != 0, P2 decide se a
/// transação atual sofre rollback: não faz se P2 == OE_Fail, faz se P2 == OE_Rollback; se
/// P2 == OE_Abort, desfaz as mudanças desta execução do VDBE mas não a transação.
///
/// Se P4 não é nulo, é uma mensagem de erro. P5 vai de 0 a 4 e modifica a string P4:
///
///    0: (sem mudança)
///    1: NOT NULL constraint failed: P4
///    2: UNIQUE constraint failed: P4
///    3: CHECK constraint failed: P4
///    4: FOREIGN KEY constraint failed: P4
///
/// Se P5 não é zero e P4 é NULL, tudo depois do ":" é omitido. Há um "Halt 0 0 0" implícito no
/// fim de todo programa, então pular além da última instrução equivale a executar Halt.
pub fn op_halt(ctx: &mut ExecCtx) -> Flow {
    let p_op = cur_op(ctx);

    // Um "OP_Halt SQLITE_INTERNAL * * * *" codificado de propósito indica defeito no gerador de
    // código. Levanta um assert para chamar a atenção de fuzzers e ferramentas de teste.
    assert!(p_op.p1 != SQLITE_INTERNAL);

    let p_frame = {
        let pb = ctx.p.borrow();
        if p_op.p1 == SQLITE_OK { pb.p_frame.clone() } else { None }
    };
    if let Some(p_frame) = p_frame {
        // Faz halt do subprograma. Devolve o controle ao quadro pai.
        {
            let mut pb = ctx.p.borrow_mut();
            pb.p_frame = p_frame.borrow().p_parent.clone();
            pb.n_frame -= 1;
        }
        let n_change = ctx.p.borrow().n_change;
        vdbe_set_changes(&ctx.db, n_change);
        let mut pcx = vdbe_frame_restore(&p_frame);
        if p_op.p2 == OE_IGNORE {
            // A instrução pcx é o OP_Program que invocou o subprograma que está parando. Se o p2
            // deste OP_Halt é OE_Ignore, o subprograma lança uma exceção IGNORE: salta para o
            // endereço dado pelo p2 do OP_Program chamador.
            pcx = ctx.p.borrow().a_op[pcx as usize].p2 - 1;
        }
        ctx.pc = pcx as usize;
        return Flow::Next;
    }
    {
        let pb = &mut *ctx.p.borrow_mut();
        pb.rc = p_op.p1;
        pb.error_action = p_op.p2 as u8;
        assert!(p_op.p5 <= 4);
        if pb.rc != 0 {
            if p_op.p5 != 0 {
                const AZ_TYPE: [&[u8]; 4] = [b"NOT NULL", b"UNIQUE", b"CHECK", b"FOREIGN KEY"];
                let mut msg: Vec<u8> = AZ_TYPE[(p_op.p5 - 1) as usize].to_vec();
                msg.extend_from_slice(b" constraint failed");
                vdbe_error(pb, &msg);
                if let Some(z) = p4_text(&p_op) {
                    let mut full: Vec<u8> = pb.z_err_msg.clone().unwrap_or_default();
                    full.extend_from_slice(b": ");
                    full.extend_from_slice(z);
                    pb.z_err_msg = Some(full);
                }
            } else {
                let z: Vec<u8> = p4_text(&p_op).cloned().unwrap_or_default();
                vdbe_error(pb, &z);
            }
            let pcx = ctx.pc;
            let mut log_msg: Vec<u8> = format!("abort at {} in [", pcx).into_bytes();
            log_msg.extend_from_slice(pb.z_sql.as_deref().unwrap_or(b""));
            log_msg.extend_from_slice(b"]: ");
            log_msg.extend_from_slice(pb.z_err_msg.as_deref().unwrap_or(b""));
            api::log(p_op.p1, &log_msg);
        }
    }
    let mut rc = vdbe_halt(&ctx.p);
    assert!(rc == SQLITE_BUSY || rc == SQLITE_OK || rc == SQLITE_ERROR);
    {
        let mut pb = ctx.p.borrow_mut();
        if rc == SQLITE_BUSY {
            pb.rc = SQLITE_BUSY;
        } else {
            assert!(rc == SQLITE_OK || (pb.rc & 0xff) == SQLITE_CONSTRAINT);
            {
                let dbb = ctx.db.borrow();
                assert!(rc == SQLITE_OK || dbb.n_deferred_cons > 0 || dbb.n_deferred_imm_cons > 0);
            }
            rc = if pb.rc != 0 { SQLITE_ERROR } else { SQLITE_DONE };
        }
    }
    ctx.rc = rc;
    Flow::VdbeReturn
}

/// Opcode: Integer P1 P2 * * *
///
/// O inteiro de 32 bits P1 é escrito no registrador P2.
pub fn op_integer(ctx: &mut ExecCtx) -> Flow {
    let p_op = cur_op(ctx);
    let pb = &mut *ctx.p.borrow_mut();
    ctx.p_out = out2_prerelease(pb, &p_op);
    pb.a_mem[ctx.p_out].u.i = p_op.p1 as i64;
    Flow::Next
}

/// Opcode: Int64 * P2 * P4 *
///
/// P4 é um inteiro de 64 bits. Escreve esse valor no registrador P2.
pub fn op_int64(ctx: &mut ExecCtx) -> Flow {
    let p_op = cur_op(ctx);
    let pb = &mut *ctx.p.borrow_mut();
    ctx.p_out = out2_prerelease(pb, &p_op);
    let i = match p_op.p4 {
        P4Value::Int64(i) => i,
        _ => panic!("OP_Int64 sem p4 inteiro"),
    };
    pb.a_mem[ctx.p_out].u.i = i;
    Flow::Next
}

/// Opcode: Real * P2 * P4 *
///
/// P4 é um ponto flutuante de 64 bits. Escreve esse valor no registrador P2.
pub fn op_real(ctx: &mut ExecCtx) -> Flow {
    let p_op = cur_op(ctx);
    let pb = &mut *ctx.p.borrow_mut();
    ctx.p_out = out2_prerelease(pb, &p_op);
    let r = match p_op.p4 {
        P4Value::Real(r) => r,
        _ => panic!("OP_Real sem p4 real"),
    };
    let p_out = &mut pb.a_mem[ctx.p_out];
    p_out.flags = MEM_REAL;
    assert!(!is_nan(r));
    p_out.u.r = r;
    Flow::Next
}

/// Opcode: String8 * P2 * P4 *
///
/// P4 é uma string UTF-8. Este opcode é transformado em String antes da primeira execução. Nessa
/// transformação o comprimento de P4 é calculado e guardado em P1.
pub fn op_string8(ctx: &mut ExecCtx) -> Flow {
    let p_op = cur_op(ctx);
    let z: Vec<u8> = p4_text(&p_op).expect("OP_String8 sem p4.z").clone();
    {
        let pb = &mut *ctx.p.borrow_mut();
        ctx.p_out = out2_prerelease(pb, &p_op);
        let mut p1 = strlen30(&z);

        if ctx.encoding != SQLITE_UTF8 {
            let p_out = &mut pb.a_mem[ctx.p_out];
            let rc = vdbe_mem_set_str(p_out, Some(&z[..]), -1, SQLITE_UTF8, SQLITE_STATIC);
            assert!(rc == SQLITE_OK || rc == SQLITE_TOOBIG);
            if rc != 0 {
                ctx.rc = rc;
                return Flow::TooBig;
            }
            if SQLITE_OK != vdbe_change_encoding(p_out, ctx.encoding) {
                return Flow::NoMem;
            }
            assert!(p_out.sz_malloc > 0);
            assert!(!vdbe_mem_dynamic(p_out));
            p_out.sz_malloc = 0;
            p_out.flags |= MEM_STATIC;
            // O texto antigo de P4 (P4_DYNAMIC) é solto ao ser substituído.
            let op = &mut pb.a_op[ctx.pc];
            op.p4type = P4_DYNAMIC;
            op.p4 = P4Value::Dynamic(p_out.z.clone());
            p1 = p_out.n;
        }
        let limit = ctx.db.borrow().a_limit[SQLITE_LIMIT_LENGTH as usize];
        pb.a_op[ctx.pc].p1 = p1;
        if p1 > limit {
            return Flow::TooBig;
        }
        pb.a_op[ctx.pc].opcode = OP_STRING;
    }
    assert!(ctx.rc == SQLITE_OK);
    // Cai em OP_String.
    op_string(ctx)
}

/// Opcode: String P1 P2 P3 P4 P5
///
/// A string P4 de comprimento P1 (bytes) é guardada no registrador P2.
///
/// Se P3 não é zero e o conteúdo do registrador P3 é igual a P5, o tipo do registrador P2 vira
/// BLOB: os mesmos bytes, interpretados como BLOB como se tivesse havido CAST.
pub fn op_string(ctx: &mut ExecCtx) -> Flow {
    let p_op = cur_op(ctx);
    let z: Vec<u8> = p4_text(&p_op).expect("OP_String sem p4.z").clone();
    let pb = &mut *ctx.p.borrow_mut();
    ctx.p_out = out2_prerelease(pb, &p_op);
    {
        let p_out = &mut pb.a_mem[ctx.p_out];
        p_out.flags = MEM_STR | MEM_STATIC | MEM_TERM;
        p_out.z = z;
        p_out.n = p_op.p1;
        p_out.enc = ctx.encoding;
    }
    if p_op.p3 > 0 {
        assert!(p_op.p3 <= (pb.n_mem + 1 - pb.n_cursor));
        ctx.p_in3 = p_op.p3 as usize;
        let p_in3 = &pb.a_mem[ctx.p_in3];
        assert!((p_in3.flags & MEM_INT) != 0);
        if p_in3.u.i == p_op.p5 as i64 {
            pb.a_mem[ctx.p_out].flags = MEM_BLOB | MEM_STATIC | MEM_TERM;
        }
    }
    Flow::Next
}

/// Opcode: BeginSubrtn * P2 * * *
///
/// Marca o início de uma sub-rotina que pode ser entrada em linha ou chamada por OP_Gosub. Ela
/// deve terminar num OP_Return cujo P1 seja igual ao P2 deste opcode e com P3 igual a 1. Entrando
/// em linha, o OP_Return só segue adiante; entrando por OP_Gosub, volta à primeira instrução
/// depois do OP_Gosub. Funciona carregando NULL no registrador P2, e é idêntico a OP_Null.
///
/// Opcode: Null P1 P2 P3 * *
///
/// Escreve NULL no registrador P2. Se P3 é maior que P2, escreve NULL também em P3 e em todos os
/// registradores entre P2 e P3. Se P3 é menor que P2 (tipicamente zero), só P2 vira NULL. Se P1
/// não é zero, ativa também MEM_Cleared, para que NULLs não comparem iguais mesmo com SQLITE_NULLEQ
/// em OP_Ne ou OP_Eq.
pub fn op_null(ctx: &mut ExecCtx) -> Flow {
    let p_op = cur_op(ctx);
    let pb = &mut *ctx.p.borrow_mut();
    ctx.p_out = out2_prerelease(pb, &p_op);
    let cnt = p_op.p3 - p_op.p2;
    assert!(p_op.p3 <= (pb.n_mem + 1 - pb.n_cursor));
    let null_flag: u16 = if p_op.p1 != 0 { MEM_NULL | MEM_CLEARED } else { MEM_NULL };
    {
        let p_out = &mut pb.a_mem[ctx.p_out];
        p_out.flags = null_flag;
        p_out.n = 0;
    }
    for k in 1..=cnt.max(0) as usize {
        let p_out = &mut pb.a_mem[ctx.p_out + k];
        vdbe_mem_set_null(p_out);
        p_out.flags = null_flag;
        p_out.n = 0;
    }
    Flow::Next
}

/// Opcode: SoftNull P1 * * * *
///
/// Dá ao registrador P1 o valor NULL como visto por OP_MakeRecord, mas sem liberar a memória de
/// string ou blob associada, para que cópias feitas antes por OP_SCopy continuem válidas.
pub fn op_soft_null(ctx: &mut ExecCtx) -> Flow {
    let p_op = cur_op(ctx);
    let mut pb = ctx.p.borrow_mut();
    assert!(p_op.p1 > 0 && p_op.p1 <= (pb.n_mem + 1 - pb.n_cursor));
    ctx.p_out = p_op.p1 as usize;
    let p_out = &mut pb.a_mem[ctx.p_out];
    p_out.flags = (p_out.flags & !(MEM_UNDEFINED | MEM_AFFMASK)) | MEM_NULL;
    Flow::Next
}

/// Opcode: Blob P1 P2 * P4 *
///
/// P4 aponta para um blob de P1 bytes. Guarda esse blob no registrador P2. Se P4 é nulo, constrói
/// em P2 um blob de P1 bytes preenchido com zeros.
pub fn op_blob(ctx: &mut ExecCtx) -> Flow {
    let p_op = cur_op(ctx);
    assert!(p_op.p1 <= SQLITE_MAX_LENGTH);
    let pb = &mut *ctx.p.borrow_mut();
    ctx.p_out = out2_prerelease(pb, &p_op);
    let p_out = &mut pb.a_mem[ctx.p_out];
    match p4_text(&p_op) {
        None => {
            vdbe_mem_set_zero_blob(p_out, p_op.p1);
            if vdbe_mem_expand_blob(p_out) != 0 {
                return Flow::NoMem;
            }
        }
        Some(z) => {
            vdbe_mem_set_str(p_out, Some(&z[..]), p_op.p1 as i64, 0, SQLITE_STATIC);
        }
    }
    p_out.enc = ctx.encoding;
    Flow::Next
}


// ---- part_004.rs ----

// Notas de integração para o tech lead:
// - Este trecho do vdbe.c é o miolo do `switch` de `sqlite3VdbeExec`: não há funções
//   no C, só blocos `case`. Cada `case` virou uma `fn op_xxx` com a mesma ordem do C.
//   O destino de cada `goto` do C é o valor devolvido, do tipo `OpFlow`, que o laço
//   de `vdbe_exec` precisa definir (`Next` é o `break` do case):
//     Next             -> break (segue para a próxima instrução)
//     TooBig           -> goto too_big
//     NoMem            -> goto no_mem
//     AbortDueToError  -> goto abort_due_to_error (o código vai em `*rc`)
//     VdbeReturn       -> goto vdbe_return (o código vai em `*rc`)
//     JumpToP2         -> goto jump_to_p2 (pOp = &aOp[pOp->p2 - 1])
//     JumpTo(addr)     -> pOp = &aOp[addr - 1] (OP_Jump e o rótulo op_column_corrupt)
// - `pOp - aOp` vira o parâmetro `i_op` (índice da instrução em `p.a_op`).
// - Registradores são `MemRef` (`Rc<RefCell<Mem>>`) em `p.a_mem` e `p.a_var`; o
//   aliasing do C (pIn1 == pOut etc.) é resolvido com `Rc::ptr_eq` e com empréstimos
//   curtos, nunca dois `borrow_mut` da mesma célula ao mesmo tempo.
// - Sem SQLITE_DEBUG/SQLITE_TEST: `memAboutToChange`, `REGISTER_TRACE`,
//   `UPDATE_MAX_BLOBSIZE` e os blocos `pScopyFrom` somem.
// - `Deephemeralize` é macro do vdbe.c com `goto no_mem` dentro; aqui é a função
//   `deephemeralize`, que devolve verdadeiro quando o C teria saltado para no_mem.
// - O tamanho do buffer de `Mem.z` no C vem de `sqlite3VdbeMemGrow`; aqui `z` é um
//   `Vec<u8>` e o OP_Concat o estende até `n_byte+2` antes de gravar, o que equivale
//   à garantia de capacidade do C.

/// Equivale ao macro `Deephemeralize(P)` do vdbe.c: se o valor é efêmero, torna-o
/// gravável. Devolve verdadeiro quando isso falhou (o C faz `goto no_mem`).
#[inline]
pub fn deephemeralize(p: &mut Mem) -> bool {
    (p.flags & MEM_EPHEM) != 0 && vdbe_mem_make_writeable(p) != 0
}

/// Equivale ao `memcpy(pTo, pFrom, MEMCELLSIZE)` do C: copia só a parte do `Mem` que
/// antecede o campo `db` (u, z, n, flags, enc, e_subtype), sem mexer em `db`,
/// `sz_malloc`, `z_malloc` nem `x_del`.
#[inline]
fn mem_copy_cell(to: &mut Mem, from: &Mem) {
    to.u = from.u.clone();
    to.z = from.z.clone();
    to.n = from.n;
    to.flags = from.flags;
    to.enc = from.enc;
    to.e_subtype = from.e_subtype;
}

/// Opcode: Variable P1 P2 * * *
/// Synopsis: r[P2]=parameter(P1)
///
/// Transfere o valor do parâmetro vinculado P1 para o registro P2.
pub fn op_variable(p: &mut Vdbe, p_op: &Op) -> OpFlow {
    debug_assert!(p_op.p1 > 0 && p_op.p1 <= p.n_var as i32);
    let p_var = p.a_var[(p_op.p1 - 1) as usize].clone();
    if vdbe_mem_too_big(&p_var.borrow()) != 0 {
        return OpFlow::TooBig;
    }
    let p_out = p.a_mem[p_op.p2 as usize].clone();
    let mut out = p_out.borrow_mut();
    if vdbe_mem_dynamic(&out) {
        vdbe_mem_set_null(&mut out);
    }
    // memcpy(pOut, pVar, MEMCELLSIZE): copia os campos até `db` (u, z, n, flags, enc, e_subtype)
    mem_copy_cell(&mut out, &p_var.borrow());
    out.flags &= !(MEM_DYN | MEM_EPHEM);
    out.flags |= MEM_STATIC | MEM_FROMBIND;
    OpFlow::Next
}

/// Opcode: Move P1 P2 P3 * *
/// Synopsis: r[P2@P3]=r[P1@P3]
///
/// Move os P3 valores dos registros P1..P1+P3-1 para os registros P2..P2+P3-1.
/// Os registros P1..P1+P3-1 ficam com NULL. É erro os intervalos se sobreporem
/// e é erro P3 ser menor que 1.
pub fn op_move(p: &mut Vdbe, p_op: &Op) -> OpFlow {
    let mut n = p_op.p3; // Número de registradores que faltam copiar
    let mut p1 = p_op.p1; // Registrador de origem
    let mut p2 = p_op.p2; // Registrador de destino
    debug_assert!(n > 0 && p1 > 0 && p2 > 0);
    debug_assert!(p1 + n <= p2 || p2 + n <= p1);

    loop {
        let p_in1 = p.a_mem[p1 as usize].clone();
        let p_out = p.a_mem[p2 as usize].clone();
        debug_assert!(!Rc::ptr_eq(&p_in1, &p_out));
        let mut out = p_out.borrow_mut();
        vdbe_mem_move(&mut out, &mut p_in1.borrow_mut());
        if deephemeralize(&mut out) {
            return OpFlow::NoMem;
        }
        p1 += 1;
        p2 += 1;
        n -= 1;
        if n == 0 {
            break;
        }
    }
    OpFlow::Next
}

/// Opcode: Copy P1 P2 P3 * P5
/// Synopsis: r[P2@P3+1]=r[P1@P3+1]
///
/// Faz uma cópia dos registros P1..P1+P3 para os registros P2..P2+P3.
///
/// Se o bit 0x0002 de P5 está ligado, limpa também MEM_SUBTYPE no destino. O bit
/// 0x0001 de P5 indica que este Copy não pode ser fundido e só serve ao
/// planejador. A cópia é profunda: strings e blobs constantes são duplicados.
pub fn op_copy(p: &mut Vdbe, p_op: &Op) -> OpFlow {
    let mut n = p_op.p3;
    let mut p1 = p_op.p1;
    let mut p2 = p_op.p2;
    debug_assert!(p2 != p1);

    loop {
        let p_in1 = p.a_mem[p1 as usize].clone();
        let p_out = p.a_mem[p2 as usize].clone();
        debug_assert!(!Rc::ptr_eq(&p_in1, &p_out));
        let mut out = p_out.borrow_mut();
        vdbe_mem_shallow_copy(&mut out, &p_in1.borrow(), MEM_EPHEM);
        if deephemeralize(&mut out) {
            return OpFlow::NoMem;
        }
        if (out.flags & MEM_SUBTYPE) != 0 && (p_op.p5 & 0x0002) != 0 {
            out.flags &= !MEM_SUBTYPE;
        }
        // if( (n--)==0 ) break;
        let last = n == 0;
        n -= 1;
        if last {
            break;
        }
        p1 += 1;
        p2 += 1;
    }
    OpFlow::Next
}

/// Opcode: SCopy P1 P2 * * *
/// Synopsis: r[P2]=r[P1]
///
/// Faz uma cópia rasa do registro P1 no registro P2. Se o valor é string ou
/// blob, a cópia só aponta para o original: se ele mudar, a cópia muda, e se for
/// liberado, a cópia fica inválida. Use OP_Copy para uma cópia completa.
pub fn op_scopy(p: &mut Vdbe, p_op: &Op) -> OpFlow {
    let p_in1 = p.a_mem[p_op.p1 as usize].clone();
    let p_out = p.a_mem[p_op.p2 as usize].clone();
    debug_assert!(!Rc::ptr_eq(&p_in1, &p_out));
    vdbe_mem_shallow_copy(&mut p_out.borrow_mut(), &p_in1.borrow(), MEM_EPHEM);
    OpFlow::Next
}

/// Opcode: IntCopy P1 P2 * * *
/// Synopsis: r[P2]=r[P1]
///
/// Transfere o inteiro do registro P1 para o registro P2. É uma versão
/// otimizada de SCopy que só serve para inteiros.
pub fn op_int_copy(p: &mut Vdbe, p_op: &Op) -> OpFlow {
    let p_in1 = p.a_mem[p_op.p1 as usize].clone();
    debug_assert!((p_in1.borrow().flags & MEM_INT) != 0);
    let p_out = p.a_mem[p_op.p2 as usize].clone();
    let i = p_in1.borrow().u.i;
    vdbe_mem_set_int64(&mut p_out.borrow_mut(), i);
    OpFlow::Next
}

/// Opcode: FkCheck * * * * *
///
/// Interrompe com SQLITE_CONSTRAINT se houver violações pendentes de chave
/// estrangeira; sem violações, não faz nada. Serve para levantar o erro antes de
/// devolver resultados como a contagem de linhas ou um RETURNING.
pub fn op_fk_check(p: &mut Vdbe, rc: &mut i32) -> OpFlow {
    *rc = vdbe_check_fk(p, 0);
    if *rc != SQLITE_OK {
        return OpFlow::AbortDueToError;
    }
    OpFlow::Next
}

/// Opcode: ResultRow P1 P2 * * *
/// Synopsis: output=r[P1@P2]
///
/// Os registros P1 até P1+P2-1 contêm uma linha de resultado. Faz o
/// sqlite3_step() terminar com SQLITE_ROW e prepara o sqlite3_stmt para dar
/// acesso a r(P1)..r(P1+P2-1) como a linha de resultado.
pub fn op_result_row(
    p: &mut Vdbe,
    db: &mut sqlite3,
    p_op: &Op,
    i_op: i32,
    rc: &mut i32,
) -> OpFlow {
    debug_assert!(p.n_res_column as i32 == p_op.p2);
    debug_assert!(p_op.p1 > 0);
    debug_assert!(p_op.p1 + p_op.p2 <= (p.n_mem + 1 - p.n_cursor) + 1);

    p.cache_ctr = p.cache_ctr.wrapping_add(2) | 1;
    p.p_result_row = Some(p.a_mem[p_op.p1 as usize].clone());
    if db.malloc_failed != 0 {
        return OpFlow::NoMem;
    }
    if (db.m_trace & SQLITE_TRACE_ROW) != 0 {
        if let Some(x_v2) = db.trace.x_v2.as_ref() {
            x_v2(SQLITE_TRACE_ROW, &db.p_trace_arg, p, None);
        }
    }
    p.pc = i_op + 1;
    *rc = SQLITE_ROW;
    OpFlow::VdbeReturn
}

/// Opcode: Concat P1 P2 P3 * *
/// Synopsis: r[P3]=r[P2]+r[P1]
///
/// Acrescenta o texto do registro P1 ao fim do texto do registro P2 e guarda o
/// resultado em P3 (P3 = P2 || P1). Se P1 ou P2 for NULL, P3 recebe NULL. É
/// ilegal P1 e P3 serem o mesmo registro; quando P3 é P2, às vezes dá para
/// evitar um memcpy().
pub fn op_concat(p: &mut Vdbe, db: &sqlite3, p_op: &Op, encoding: u8) -> OpFlow {
    let p_in1 = p.a_mem[p_op.p1 as usize].clone();
    let p_in2 = p.a_mem[p_op.p2 as usize].clone();
    let p_out = p.a_mem[p_op.p3 as usize].clone();
    debug_assert!(!Rc::ptr_eq(&p_in1, &p_out));
    let out_is_in2 = Rc::ptr_eq(&p_out, &p_in2);

    let mut flags1: u16 = p_in1.borrow().flags; // Flags iniciais de P1
    if ((flags1 | p_in2.borrow().flags) & MEM_NULL) != 0 {
        vdbe_mem_set_null(&mut p_out.borrow_mut());
        return OpFlow::Next;
    }
    if (flags1 & (MEM_STR | MEM_BLOB)) == 0 {
        if vdbe_mem_stringify(&mut p_in1.borrow_mut(), encoding, 0) != 0 {
            return OpFlow::NoMem;
        }
        flags1 = p_in1.borrow().flags & !MEM_STR;
    } else if (flags1 & MEM_ZERO) != 0 {
        if vdbe_mem_expand_blob(&mut p_in1.borrow_mut()) != 0 {
            return OpFlow::NoMem;
        }
        flags1 = p_in1.borrow().flags & !MEM_STR;
    }
    let mut flags2: u16 = p_in2.borrow().flags; // Flags iniciais de P2
    if (flags2 & (MEM_STR | MEM_BLOB)) == 0 {
        if vdbe_mem_stringify(&mut p_in2.borrow_mut(), encoding, 0) != 0 {
            return OpFlow::NoMem;
        }
        flags2 = p_in2.borrow().flags & !MEM_STR;
    } else if (flags2 & MEM_ZERO) != 0 {
        if vdbe_mem_expand_blob(&mut p_in2.borrow_mut()) != 0 {
            return OpFlow::NoMem;
        }
        flags2 = p_in2.borrow().flags & !MEM_STR;
    }
    let n1 = p_in1.borrow().n; // Comprimentos lidos antes de emprestar a saída
    let n2 = p_in2.borrow().n;
    let mut n_byte: i64 = n1 as i64 + n2 as i64; // Tamanho total da saída
    if n_byte > db.a_limit[SQLITE_LIMIT_LENGTH as usize] as i64 {
        return OpFlow::TooBig;
    }
    if vdbe_mem_grow(
        &mut p_out.borrow_mut(),
        (n_byte as i32).wrapping_add(2),
        if out_is_in2 { 1 } else { 0 },
    ) != 0
    {
        return OpFlow::NoMem;
    }
    let mut out = p_out.borrow_mut();
    mem_set_type_flag(&mut out, MEM_STR);
    // O Vec fica com pelo menos n_byte+2 bytes, como a capacidade garantida pelo C.
    let need = n_byte as usize + 2;
    if out.z.len() < need {
        out.z.resize(need, 0);
    }
    let (n1, n2) = (n1 as usize, n2 as usize);
    if !out_is_in2 {
        out.z[..n2].copy_from_slice(&p_in2.borrow().z[..n2]);
        debug_assert!((p_in2.borrow().flags & MEM_DYN) == (flags2 & MEM_DYN));
        p_in2.borrow_mut().flags = flags2;
    }
    out.z[n2..n2 + n1].copy_from_slice(&p_in1.borrow().z[..n1]);
    debug_assert!((p_in1.borrow().flags & MEM_DYN) == (flags1 & MEM_DYN));
    p_in1.borrow_mut().flags = flags1;
    if encoding > SQLITE_UTF8 as u8 {
        n_byte &= !1;
    }
    out.z[n_byte as usize] = 0;
    out.z[n_byte as usize + 1] = 0;
    out.flags |= MEM_TERM;
    out.n = n_byte as i32;
    out.enc = encoding;
    OpFlow::Next
}

/// Opcodes: Add, Subtract, Multiply, Divide, Remainder (P1 P2 P3 * *)
///
/// Add:       r[P3]=r[P1]+r[P2]
/// Multiply:  r[P3]=r[P1]*r[P2]
/// Subtract:  r[P3]=r[P2]-r[P1]
/// Divide:    r[P3]=r[P2]/r[P1]; se r[P1] é zero, o resultado é NULL
/// Remainder: r[P3]=r[P2]%r[P1]; se r[P1] é zero, o resultado é NULL
///
/// Se qualquer entrada for NULL, o resultado é NULL. As cinco instruções
/// compartilham este corpo, como no C; os rótulos `int_math`, `fp_math` e
/// `arithmetic_result_is_null` viram o fluxo abaixo.
pub fn op_arithmetic(p: &mut Vdbe, p_op: &Op) -> OpFlow {
    let p_in1 = p.a_mem[p_op.p1 as usize].clone();
    let p_in2 = p.a_mem[p_op.p2 as usize].clone();
    let p_out = p.a_mem[p_op.p3 as usize].clone();
    let mut type1: u16 = p_in1.borrow().flags; // Tipo numérico do operando esquerdo
    let mut type2: u16 = p_in2.borrow().flags; // Tipo numérico do operando direito

    // arithmetic_result_is_null:
    let result_is_null = || {
        vdbe_mem_set_null(&mut p_out.borrow_mut());
        OpFlow::Next
    };

    let mut int_math = false;
    if (type1 & type2 & MEM_INT) != 0 {
        int_math = true;
    } else if ((type1 | type2) & MEM_NULL) != 0 {
        return result_is_null();
    } else {
        type1 = numeric_type(&mut p_in1.borrow_mut());
        type2 = numeric_type(&mut p_in2.borrow_mut());
        if (type1 & type2 & MEM_INT) != 0 {
            int_math = true;
        }
    }

    if int_math {
        // int_math:
        let mut i_a: i64 = p_in1.borrow().u.i; // Valor inteiro do operando esquerdo
        let mut i_b: i64 = p_in2.borrow().u.i; // Valor inteiro do operando direito
        let mut goto_fp_math = false;
        match p_op.opcode {
            OP_ADD => {
                if add_int64(&mut i_b, i_a) != 0 {
                    goto_fp_math = true;
                }
            }
            OP_SUBTRACT => {
                if sub_int64(&mut i_b, i_a) != 0 {
                    goto_fp_math = true;
                }
            }
            OP_MULTIPLY => {
                if mul_int64(&mut i_b, i_a) != 0 {
                    goto_fp_math = true;
                }
            }
            OP_DIVIDE => {
                if i_a == 0 {
                    return result_is_null();
                }
                if i_a == -1 && i_b == SMALLEST_INT64 {
                    goto_fp_math = true;
                } else {
                    i_b /= i_a;
                }
            }
            _ => {
                if i_a == 0 {
                    return result_is_null();
                }
                if i_a == -1 {
                    i_a = 1;
                }
                i_b %= i_a;
            }
        }
        if !goto_fp_math {
            let mut out = p_out.borrow_mut();
            out.u.i = i_b;
            mem_set_type_flag(&mut out, MEM_INT);
            return OpFlow::Next;
        }
    }

    // fp_math:
    let r_a: f64 = vdbe_real_value(&mut p_in1.borrow_mut()); // Valor real do operando esquerdo
    let mut r_b: f64 = vdbe_real_value(&mut p_in2.borrow_mut()); // Valor real do operando direito
    match p_op.opcode {
        OP_ADD => r_b += r_a,
        OP_SUBTRACT => r_b -= r_a,
        OP_MULTIPLY => r_b *= r_a,
        OP_DIVIDE => {
            // (double)0 para o caso de SQLITE_OMIT_FLOATING_POINT
            if r_a == 0.0 {
                return result_is_null();
            }
            r_b /= r_a;
        }
        _ => {
            let mut i_a = vdbe_int_value(&mut p_in1.borrow_mut());
            let i_b = vdbe_int_value(&mut p_in2.borrow_mut());
            if i_a == 0 {
                return result_is_null();
            }
            if i_a == -1 {
                i_a = 1;
            }
            r_b = (i_b % i_a) as f64;
        }
    }
    // sqlite3IsNaN(rB) é rB!=rB
    if r_b.is_nan() {
        return result_is_null();
    }
    let mut out = p_out.borrow_mut();
    out.u.r = r_b;
    mem_set_type_flag(&mut out, MEM_REAL);
    OpFlow::Next
}


// ---- part_005.rs ----

// Notas de integração para o tech lead (mesma convenção de part_004.rs):
// - Cada `case` do `switch` de `sqlite3VdbeExec` é uma `fn op_xxx` que devolve `OpFlow`.
// - Além das variantes listadas em part_004.rs, este trecho usa duas variantes de salto:
//     JumpToP2     -> goto jump_to_p2 (pOp = &aOp[pOp->p2 - 1]; break)
//     JumpTo(addr) -> pOp = &aOp[addr - 1]; break (usado por OP_Jump em part_006.rs)
// - `iCompare` é variável local de `vdbe_exec`; aqui chega como `i_compare: &mut i32`.
// - Registradores são `MemRef` em `p.a_mem`; pIn1 == pIn3 é possível, então os empréstimos
//   são sempre curtos e nunca dois `borrow_mut` da mesma célula ao mesmo tempo.
// - As tabelas `sqlite3aLTb`, `sqlite3aEQb` e `sqlite3aGTb` do global.c são indexadas pelo
//   opcode e vivem aqui como `a_lt_b`, `a_eq_b` e `a_gt_b` (ordem NE, EQ, GT, LE, LT, GE).
//   Se o tech lead preferir mantê-las em global.c, basta apagar as três funções abaixo.
// - Assume-se `vdbe_int_value(&mut Mem) -> i64` e `apply_numeric_affinity(&mut Mem, i32)`,
//   `apply_affinity(&mut Mem, u8, u8)`, `mem_compare(&Mem, &Mem, Option<&CollSeqRef>) -> i32`.

/// Opcode: CollSeq P1 * * P4
///
/// P4 aponta para um objeto CollSeq. Se a próxima chamada de função de usuário ou de
/// agregação chamar `get_func_coll_seq()`, essa sequência de colação será devolvida. É
/// usado por min(), max() e nullif() embutidas.
///
/// Se P1 não é zero, é um registrador que um min() ou max() agregado subsequente põe em 1
/// quando a linha atual não é o mínimo ou o máximo. Esta instrução inicializa P1 com 0.
pub fn op_coll_seq(p: &mut Vdbe, p_op: &Op) -> OpFlow {
    debug_assert!(p_op.p4type == P4_COLLSEQ);
    if p_op.p1 != 0 {
        let p_mem = p.a_mem[p_op.p1 as usize].clone();
        vdbe_mem_set_int64(&mut p_mem.borrow_mut(), 0);
    }
    OpFlow::Next
}

/// Opcodes: BitAnd, BitOr, ShiftLeft, ShiftRight (P1 P2 P3 * *)
///
/// BitAnd:     r[P3]=r[P1]&r[P2]
/// BitOr:      r[P3]=r[P1]|r[P2]
/// ShiftLeft:  r[P3]=r[P2]<<r[P1]
/// ShiftRight: r[P3]=r[P2]>>r[P1]
///
/// Se qualquer entrada for NULL, o resultado é NULL.
pub fn op_bit_and_or_shift(p: &mut Vdbe, p_op: &Op) -> OpFlow {
    let p_in1 = p.a_mem[p_op.p1 as usize].clone();
    let p_in2 = p.a_mem[p_op.p2 as usize].clone();
    let p_out = p.a_mem[p_op.p3 as usize].clone();
    let flags_or = p_in1.borrow().flags | p_in2.borrow().flags;
    if (flags_or & MEM_NULL) != 0 {
        vdbe_mem_set_null(&mut p_out.borrow_mut());
        return OpFlow::Next;
    }
    let mut i_a: i64 = vdbe_int_value(&mut p_in2.borrow_mut());
    let mut i_b: i64 = vdbe_int_value(&mut p_in1.borrow_mut());
    let mut op: u8 = p_op.opcode;
    if op == OP_BITAND {
        i_a &= i_b;
    } else if op == OP_BITOR {
        i_a |= i_b;
    } else if i_b != 0 {
        debug_assert!(op == OP_SHIFTRIGHT || op == OP_SHIFTLEFT);

        // Se o deslocamento é negativo, desloca na outra direção
        if i_b < 0 {
            debug_assert!(OP_SHIFTRIGHT == OP_SHIFTLEFT + 1);
            op = (2 * OP_SHIFTLEFT as i32 + 1 - op as i32) as u8;
            i_b = if i_b > -64 { -i_b } else { 64 };
        }

        if i_b >= 64 {
            i_a = if i_a >= 0 || op == OP_SHIFTLEFT { 0 } else { -1 };
        } else {
            let mut u_a: u64 = i_a as u64;
            if op == OP_SHIFTLEFT {
                u_a <<= i_b as u32;
            } else {
                u_a >>= i_b as u32;
                // Extensão de sinal no deslocamento à direita de um número negativo
                if i_a < 0 {
                    u_a |= (((0xffffffffu64) << 32) | 0xffffffffu64) << (64 - i_b as u32);
                }
            }
            i_a = u_a as i64;
        }
    }
    let mut out = p_out.borrow_mut();
    out.u.i = i_a;
    mem_set_type_flag(&mut out, MEM_INT);
    OpFlow::Next
}

/// Opcode: AddImm P1 P2 * * *
/// Synopsis: r[P1]=r[P1]+P2
///
/// Soma a constante P2 ao valor do registrador P1. O resultado é sempre inteiro. Para
/// forçar qualquer registrador a ser inteiro, basta somar 0.
pub fn op_add_imm(p: &mut Vdbe, p_op: &Op) -> OpFlow {
    let p_in1 = p.a_mem[p_op.p1 as usize].clone();
    let mut in1 = p_in1.borrow_mut();
    vdbe_mem_integerify(&mut in1);
    in1.u.i = (in1.u.i as u64).wrapping_add(p_op.p2 as i64 as u64) as i64;
    OpFlow::Next
}

/// Opcode: MustBeInt P1 P2 * * *
///
/// Força o valor do registrador P1 a ser inteiro. Se o valor não é inteiro e não pode ser
/// convertido sem perda de dados, salta para P2 ou, se P2==0, levanta SQLITE_MISMATCH.
pub fn op_must_be_int(p: &mut Vdbe, p_op: &Op, encoding: u8, rc: &mut i32) -> OpFlow {
    let p_in1 = p.a_mem[p_op.p1 as usize].clone();
    let mut in1 = p_in1.borrow_mut();
    if (in1.flags & MEM_INT) == 0 {
        apply_affinity(&mut in1, SQLITE_AFF_NUMERIC as u8, encoding);
        if (in1.flags & MEM_INT) == 0 {
            if p_op.p2 == 0 {
                *rc = SQLITE_MISMATCH;
                return OpFlow::AbortDueToError;
            } else {
                return OpFlow::JumpToP2;
            }
        }
    }
    mem_set_type_flag(&mut in1, MEM_INT);
    OpFlow::Next
}

/// Opcode: RealAffinity P1 * * * *
///
/// Se o registrador P1 guarda um inteiro, converte-o em real. Usado ao extrair uma coluna
/// de afinidade REAL, que pode ter sido gravada como inteiro por economia de espaço.
pub fn op_real_affinity(p: &mut Vdbe, p_op: &Op) -> OpFlow {
    let p_in1 = p.a_mem[p_op.p1 as usize].clone();
    let mut in1 = p_in1.borrow_mut();
    if (in1.flags & (MEM_INT | MEM_INTREAL)) != 0 {
        vdbe_mem_realify(&mut in1);
    }
    OpFlow::Next
}

/// Opcode: Cast P1 P2 * * *
/// Synopsis: affinity(r[P1])
///
/// Força o valor do registrador P1 ao tipo definido por P2: 'A' BLOB, 'B' TEXT,
/// 'C' NUMERIC, 'D' INTEGER, 'E' REAL. Um NULL não muda.
pub fn op_cast(p: &mut Vdbe, p_op: &Op, encoding: u8, rc: &mut i32) -> OpFlow {
    debug_assert!(p_op.p2 >= SQLITE_AFF_BLOB as i32 && p_op.p2 <= SQLITE_AFF_REAL as i32);
    let p_in1 = p.a_mem[p_op.p1 as usize].clone();
    let mut in1 = p_in1.borrow_mut();
    // ExpandBlob(pIn1)
    *rc = if (in1.flags & MEM_ZERO) != 0 {
        vdbe_mem_expand_blob(&mut in1)
    } else {
        0
    };
    if *rc != 0 {
        return OpFlow::AbortDueToError;
    }
    *rc = vdbe_mem_cast(&mut in1, p_op.p2 as u8, encoding);
    if *rc != 0 {
        return OpFlow::AbortDueToError;
    }
    OpFlow::Next
}

/// Tabelas de comparação do global.c, indexadas pelo opcode (NE, EQ, GT, LE, LT, GE):
/// `sqlite3aLTb`, `sqlite3aEQb` e `sqlite3aGTb`.
const A_LT_B: [u8; 6] = [1, 0, 0, 1, 1, 0];
const A_EQ_B: [u8; 6] = [0, 1, 0, 1, 0, 1];
const A_GT_B: [u8; 6] = [1, 0, 1, 0, 0, 1];

#[inline]
fn a_lt_b(opcode: u8) -> i32 {
    A_LT_B[opcode as usize - OP_NE as usize] as i32
}

#[inline]
fn a_eq_b(opcode: u8) -> i32 {
    A_EQ_B[opcode as usize - OP_NE as usize] as i32
}

#[inline]
fn a_gt_b(opcode: u8) -> i32 {
    A_GT_B[opcode as usize - OP_NE as usize] as i32
}

/// Opcodes: Eq, Ne, Lt, Le, Gt, Ge (P1 P2 P3 P4 P5)
///
/// Compara os valores dos registradores P1 e P3 e salta para P2 se a relação vale
/// (Eq: r[P3]==r[P1], Ne: !=, Lt: <, Le: <=, Gt: >, Ge: >=). A parte SQLITE_AFF_MASK de
/// P5 é a afinidade usada para coagir as entradas antes da comparação; as conversões ficam
/// gravadas de volta em P1 e P3. Com SQLITE_NULLEQ em P5 o resultado nunca é NULL; com
/// SQLITE_JUMPIFNULL e algum operando NULL o salto é tomado. O resultado da comparação é
/// guardado em `i_compare` para o OP_Jump seguinte.
pub fn op_comparison(p: &mut Vdbe, p_op: &Op, encoding: u8, i_compare: &mut i32) -> OpFlow {
    let p_in1 = p.a_mem[p_op.p1 as usize].clone();
    let p_in3 = p.a_mem[p_op.p3 as usize].clone();
    let mut flags1: u16 = p_in1.borrow().flags; // Cópia do flags inicial de pIn1
    let mut flags3: u16 = p_in3.borrow().flags; // Cópia do flags inicial de pIn3
    let p5 = p_op.p5 as i32;
    let res: i32; // Resultado da comparação de pIn1 contra pIn3

    if (flags1 & flags3 & MEM_INT) != 0 {
        // Caso comum: comparação de dois inteiros
        let i1 = p_in1.borrow().u.i;
        let i3 = p_in3.borrow().u.i;
        if i3 > i1 {
            if a_gt_b(p_op.opcode) != 0 {
                return OpFlow::JumpToP2;
            }
            *i_compare = 1;
        } else if i3 < i1 {
            if a_lt_b(p_op.opcode) != 0 {
                return OpFlow::JumpToP2;
            }
            *i_compare = -1;
        } else {
            if a_eq_b(p_op.opcode) != 0 {
                return OpFlow::JumpToP2;
            }
            *i_compare = 0;
        }
        return OpFlow::Next;
    }
    if ((flags1 | flags3) & MEM_NULL) != 0 {
        // Um ou os dois operandos são NULL
        if (p5 & SQLITE_NULLEQ as i32) != 0 {
            // SQLITE_NULLEQ só aparece em OP_Eq ou OP_Ne: o salto depende de ambos
            // os operandos serem nulos.
            debug_assert!((flags1 & MEM_CLEARED) == 0);
            if (flags1 & flags3 & MEM_NULL) != 0 && (flags3 & MEM_CLEARED) == 0 {
                res = 0; // Operandos iguais
            } else {
                res = if (flags3 & MEM_NULL) != 0 { -1 } else { 1 }; // Operandos diferentes
            }
        } else {
            // Sem SQLITE_NULLEQ e com um operando NULL, o resultado é sempre NULL; o
            // salto é tomado se o bit SQLITE_JUMPIFNULL está ligado.
            if (p5 & SQLITE_JUMPIFNULL as i32) != 0 {
                return OpFlow::JumpToP2;
            }
            *i_compare = 1; // Operandos diferentes
            return OpFlow::Next;
        }
    } else {
        // Nenhum operando é NULL e o caso rápido de inteiros não se aplicou: comparação
        // geral.
        let affinity: u8 = (p5 & SQLITE_AFF_MASK as i32) as u8;
        let same = Rc::ptr_eq(&p_in1, &p_in3);
        if affinity >= SQLITE_AFF_NUMERIC as u8 {
            if ((flags1 | flags3) & MEM_STR) != 0 {
                if (flags1 & (MEM_INT | MEM_INTREAL | MEM_REAL | MEM_STR)) == MEM_STR {
                    apply_numeric_affinity(&mut p_in1.borrow_mut(), 0);
                    debug_assert!(flags3 == p_in3.borrow().flags);
                    flags3 = p_in3.borrow().flags;
                }
                if (flags3 & (MEM_INT | MEM_INTREAL | MEM_REAL | MEM_STR)) == MEM_STR {
                    apply_numeric_affinity(&mut p_in3.borrow_mut(), 0);
                }
            }
        } else if affinity == SQLITE_AFF_TEXT as u8 && ((flags1 | flags3) & MEM_STR) != 0 {
            if (flags1 & MEM_STR) != 0 {
                p_in1.borrow_mut().flags &= !(MEM_INT | MEM_REAL | MEM_INTREAL);
            } else if (flags1 & (MEM_INT | MEM_REAL | MEM_INTREAL)) != 0 {
                vdbe_mem_stringify(&mut p_in1.borrow_mut(), encoding, 1);
                let now = p_in1.borrow().flags;
                flags1 = (now & !MEM_TYPEMASK) | (flags1 & MEM_TYPEMASK);
                if same {
                    flags3 = flags1 | MEM_STR;
                }
            }
            if (flags3 & MEM_STR) != 0 {
                p_in3.borrow_mut().flags &= !(MEM_INT | MEM_REAL | MEM_INTREAL);
            } else if (flags3 & (MEM_INT | MEM_REAL | MEM_INTREAL)) != 0 {
                vdbe_mem_stringify(&mut p_in3.borrow_mut(), encoding, 1);
                let now = p_in3.borrow().flags;
                flags3 = (now & !MEM_TYPEMASK) | (flags3 & MEM_TYPEMASK);
            }
        }
        debug_assert!(p_op.p4type == P4_COLLSEQ || p_op.p4.p_coll.is_none());
        res = mem_compare(&p_in3.borrow(), &p_in1.borrow(), p_op.p4.p_coll.as_ref());
    }

    // Aqui res é negativo, zero ou positivo conforme reg[P1] seja menor que, igual a ou
    // maior que reg[P3]. Os 6 operadores são inteiros consecutivos na ordem
    // NE, EQ, GT, LE, LT, GE.
    debug_assert!(OP_EQ == OP_NE + 1);
    debug_assert!(OP_GT == OP_NE + 2);
    debug_assert!(OP_LE == OP_NE + 3);
    debug_assert!(OP_LT == OP_NE + 4);
    debug_assert!(OP_GE == OP_NE + 5);
    let res2: i32 = if res < 0 {
        a_lt_b(p_op.opcode)
    } else if res == 0 {
        a_eq_b(p_op.opcode)
    } else {
        a_gt_b(p_op.opcode)
    };
    *i_compare = res;

    // Desfaz as mudanças feitas por apply_affinity() nos registradores de entrada.
    p_in3.borrow_mut().flags = flags3;
    p_in1.borrow_mut().flags = flags1;

    if res2 != 0 {
        return OpFlow::JumpToP2;
    }
    OpFlow::Next
}


// ---- part_006.rs ----

// Notas de integração para o tech lead (mesma convenção de part_004.rs e part_005.rs):
// - Cada `case` é uma `fn op_xxx` que devolve `OpFlow` (Next, JumpToP2, JumpTo(addr), ...).
// - `iCompare` chega como `i_compare: i32` (leitura) ou `&mut i32` (escrita).
// - `p4` de `Op` é modelado como struct com um campo por membro da union do C, em snake_case:
//   `p4.i`, `p4.ai` (`Vec<u32>`), `p4.p_key_info` (`Option<Rc<KeyInfo>>`), `p4.p_coll`.
// - `KeyInfo.a_coll: Vec<Option<CollSeqRef>>`, `KeyInfo.a_sort_flags: Vec<u8>`.
// - `p.p_frame: Option<Rc<RefCell<VdbeFrame>>>` com `a_once: Vec<u8>`; `p.ap_csr` guarda
//   `Option<Rc<RefCell<VdbeCursor>>>` com `n_hdr_parsed` e `a_type: Vec<u32>`.
// - OP_Once altera `pOp->p1` (código que se auto-modifica), por isso recebe `i_op` e escreve em
//   `p.a_op[i_op]`; o chamador não deve manter empréstimo de `p.a_op` durante a chamada.
// - OP_Compare lê a permutação de `a_op[i_op - 1]`, por isso recebe `a_op` e `i_op`.

/// Opcode: ElseEq * P2 * * *
///
/// Deve seguir um OP_Lt ou OP_Gt (podem existir OP_ReleaseReg no meio). Se um OP_Eq sobre os
/// mesmos operandos teria sido verdadeiro, salta para P2; senão cai adiante.
pub fn op_else_eq(i_compare: i32) -> OpFlow {
    if i_compare == 0 {
        return OpFlow::JumpToP2;
    }
    OpFlow::Next
}

/// Opcode: Permutation * * * P4 *
///
/// Define a permutação usada pelo OP_Compare da instrução seguinte, guardada em P4. O
/// primeiro inteiro do vetor P4 é o comprimento e não faz parte da permutação.
pub fn op_permutation(a_op: &[Op], i_op: usize) -> OpFlow {
    debug_assert!(a_op[i_op].p4type == P4_INTARRAY);
    debug_assert!(!a_op[i_op].p4.ai.is_empty());
    debug_assert!(a_op[i_op + 1].opcode == OP_COMPARE);
    debug_assert!((a_op[i_op + 1].p5 as i32 & OPFLAG_PERMUTE as i32) != 0);
    OpFlow::Next
}

/// Opcode: Compare P1 P2 P3 P4 P5
/// Synopsis: r[P1@P3] <-> r[P2@P3]
///
/// Compara os vetores de registradores r[P1..P1+P3-1] ("A") e r[P2..P2+P3-1] ("B") e guarda
/// o resultado em `i_compare` para o OP_Jump seguinte. Com OPFLAG_PERMUTE em P5, a ordem
/// vem do OP_Permutation anterior. P4 é um KeyInfo com as colações e ordens. É uma
/// comparação de ordenação: NULLs são iguais, NULL < número < texto < blob.
pub fn op_compare_vectors(
    p: &mut Vdbe,
    a_op: &[Op],
    i_op: usize,
    i_compare: &mut i32,
) -> OpFlow {
    let p_op = &a_op[i_op];
    let a_permute: Option<&[u32]> = if (p_op.p5 as i32 & OPFLAG_PERMUTE as i32) == 0 {
        None
    } else {
        debug_assert!(i_op > 0);
        debug_assert!(a_op[i_op - 1].opcode == OP_PERMUTATION);
        debug_assert!(a_op[i_op - 1].p4type == P4_INTARRAY);
        Some(&a_op[i_op - 1].p4.ai[1..])
    };
    let n = p_op.p3;
    let p_key_info = p_op.p4.p_key_info.as_ref().unwrap();
    debug_assert!(n > 0);
    let p1 = p_op.p1 as usize;
    let p2 = p_op.p2 as usize;
    for i in 0..n as usize {
        let idx: usize = match a_permute {
            Some(perm) => perm[i] as usize,
            None => i,
        };
        debug_assert!(i < p_key_info.n_key_field as usize);
        let p_coll = p_key_info.a_coll[i].as_ref();
        let b_rev = (p_key_info.a_sort_flags[i] as i32 & KEYINFO_ORDER_DESC as i32) != 0;
        let m1 = p.a_mem[p1 + idx].clone();
        let m2 = p.a_mem[p2 + idx].clone();
        *i_compare = mem_compare(&m1.borrow(), &m2.borrow(), p_coll);
        if *i_compare != 0 {
            if (p_key_info.a_sort_flags[i] as i32 & KEYINFO_ORDER_BIGNULL as i32) != 0
                && (((m1.borrow().flags | m2.borrow().flags) & MEM_NULL) != 0)
            {
                *i_compare = -*i_compare;
            }
            if b_rev {
                *i_compare = -*i_compare;
            }
            break;
        }
    }
    debug_assert!(a_op[i_op + 1].opcode == OP_JUMP);
    OpFlow::Next
}

/// Opcode: Jump P1 P2 P3 * *
///
/// Salta para o endereço P1, P2 ou P3 conforme, no OP_Compare mais recente, o vetor P1 foi
/// menor que, igual a ou maior que o vetor P2. Deve vir logo após um OP_Compare.
pub fn op_jump(p_op: &Op, i_compare: i32) -> OpFlow {
    if i_compare < 0 {
        OpFlow::JumpTo(p_op.p1)
    } else if i_compare == 0 {
        OpFlow::JumpTo(p_op.p2)
    } else {
        OpFlow::JumpTo(p_op.p3)
    }
}

/// Opcodes: And, Or (P1 P2 P3 * *)
///
/// And: r[P3]=(r[P1] && r[P2]). Se P1 ou P2 é 0 (falso) o resultado é 0, mesmo com a outra
/// entrada NULL. Um NULL e verdadeiro, ou dois NULLs, dão NULL.
/// Or: r[P3]=(r[P1] || r[P2]). Se P1 ou P2 é diferente de zero o resultado é 1, mesmo com a
/// outra entrada NULL. Um NULL e falso, ou dois NULLs, dão NULL.
pub fn op_and_or(p: &mut Vdbe, p_op: &Op) -> OpFlow {
    const AND_LOGIC: [u8; 9] = [0, 0, 0, 0, 1, 2, 0, 2, 2];
    const OR_LOGIC: [u8; 9] = [0, 1, 2, 1, 1, 1, 2, 1, 2];
    // 0==FALSO, 1==VERDADEIRO, 2==DESCONHECIDO ou NULL
    let v1 = vdbe_boolean_value(&p.a_mem[p_op.p1 as usize].borrow(), 2);
    let v2 = vdbe_boolean_value(&p.a_mem[p_op.p2 as usize].borrow(), 2);
    let idx = (v1 * 3 + v2) as usize;
    let v: i32 = if p_op.opcode == OP_AND {
        AND_LOGIC[idx] as i32
    } else {
        OR_LOGIC[idx] as i32
    };
    let p_out = p.a_mem[p_op.p3 as usize].clone();
    let mut out = p_out.borrow_mut();
    if v == 2 {
        mem_set_type_flag(&mut out, MEM_NULL);
    } else {
        out.u.i = v as i64;
        mem_set_type_flag(&mut out, MEM_INT);
    }
    OpFlow::Next
}

/// Opcode: IsTrue P1 P2 P3 P4 *
/// Synopsis: r[P2] = coalesce(r[P1]==TRUE,P3) ^ P4
///
/// Implementa IS TRUE, IS FALSE, IS NOT TRUE e IS NOT FALSE. Interpreta r[P1] como booleano
/// e grava 0 ou 1 em r[P2]; se r[P1] é NULL grava P3. Inverte a resposta se P4 é 1.
pub fn op_is_true(p: &mut Vdbe, p_op: &Op) -> OpFlow {
    debug_assert!(p_op.p4type == P4_INT32);
    debug_assert!(p_op.p4.i == 0 || p_op.p4.i == 1);
    debug_assert!(p_op.p3 == 0 || p_op.p3 == 1);
    let b = vdbe_boolean_value(&p.a_mem[p_op.p1 as usize].borrow(), p_op.p3) ^ p_op.p4.i;
    let p_out = p.a_mem[p_op.p2 as usize].clone();
    vdbe_mem_set_int64(&mut p_out.borrow_mut(), b as i64);
    OpFlow::Next
}

/// Opcode: Not P1 P2 * * *
/// Synopsis: r[P2]= !r[P1]
///
/// Interpreta r[P1] como booleano e grava o complemento em r[P2]. Se r[P1] é NULL, grava
/// NULL em r[P2].
pub fn op_not(p: &mut Vdbe, p_op: &Op) -> OpFlow {
    let p_in1 = p.a_mem[p_op.p1 as usize].clone();
    let p_out = p.a_mem[p_op.p2 as usize].clone();
    if (p_in1.borrow().flags & MEM_NULL) == 0 {
        let b = vdbe_boolean_value(&p_in1.borrow(), 0);
        vdbe_mem_set_int64(&mut p_out.borrow_mut(), (b == 0) as i64);
    } else {
        vdbe_mem_set_null(&mut p_out.borrow_mut());
    }
    OpFlow::Next
}

/// Opcode: BitNot P1 P2 * * *
/// Synopsis: r[P2]= ~r[P1]
///
/// Interpreta r[P1] como inteiro e grava o complemento de uns em r[P2]. Se r[P1] é NULL,
/// grava NULL em r[P2].
pub fn op_bit_not(p: &mut Vdbe, p_op: &Op) -> OpFlow {
    let p_in1 = p.a_mem[p_op.p1 as usize].clone();
    let p_out = p.a_mem[p_op.p2 as usize].clone();
    vdbe_mem_set_null(&mut p_out.borrow_mut());
    if (p_in1.borrow().flags & MEM_NULL) == 0 {
        let v = !vdbe_int_value(&mut p_in1.borrow_mut());
        let mut out = p_out.borrow_mut();
        out.flags = MEM_INT;
        out.u.i = v;
    }
    OpFlow::Next
}

/// Opcode: Once P1 P2 * * *
///
/// Cai adiante na primeira vez que o opcode é encontrado em cada invocação do programa e
/// salta para P2 nas seguintes. Programas de nível superior comparam o P1 com o P1 do
/// OP_Init; subprogramas usam o bitmask `a_once` do VdbeFrame, porque o truque do código
/// que se auto-modifica não funciona com gatilhos recursivos.
pub fn op_once(p: &mut Vdbe, i_op: usize) -> OpFlow {
    debug_assert!(p.a_op[0].opcode == OP_INIT);
    if let Some(p_frame) = p.p_frame.clone() {
        let i_addr = i_op; // Endereço desta instrução
        let mut frame = p_frame.borrow_mut();
        if (frame.a_once[i_addr / 8] & (1u8 << (i_addr & 7))) != 0 {
            return OpFlow::JumpToP2;
        }
        frame.a_once[i_addr / 8] |= 1u8 << (i_addr & 7);
    } else if p.a_op[0].p1 == p.a_op[i_op].p1 {
        return OpFlow::JumpToP2;
    }
    p.a_op[i_op].p1 = p.a_op[0].p1;
    OpFlow::Next
}

/// Opcode: If P1 P2 P3 * *
///
/// Salta para P2 se r[P1] é verdadeiro (numérico e diferente de zero). Se r[P1] é NULL, só
/// salta se P3 é diferente de zero.
pub fn op_if(p: &mut Vdbe, p_op: &Op) -> OpFlow {
    let c = vdbe_boolean_value(&p.a_mem[p_op.p1 as usize].borrow(), p_op.p3);
    if c != 0 {
        return OpFlow::JumpToP2;
    }
    OpFlow::Next
}

/// Opcode: IfNot P1 P2 P3 * *
///
/// Salta para P2 se r[P1] é falso (valor numérico zero). Se r[P1] é NULL, só salta se P3 é
/// diferente de zero.
pub fn op_if_not(p: &mut Vdbe, p_op: &Op) -> OpFlow {
    let c = vdbe_boolean_value(
        &p.a_mem[p_op.p1 as usize].borrow(),
        (p_op.p3 == 0) as i32,
    ) == 0;
    if c {
        return OpFlow::JumpToP2;
    }
    OpFlow::Next
}

/// Opcode: IsNull P1 P2 * * *
/// Synopsis: if r[P1]==NULL goto P2
///
/// Salta para P2 se r[P1] é NULL.
pub fn op_is_null(p: &mut Vdbe, p_op: &Op) -> OpFlow {
    if (p.a_mem[p_op.p1 as usize].borrow().flags & MEM_NULL) != 0 {
        return OpFlow::JumpToP2;
    }
    OpFlow::Next
}

/// Opcode: IsType P1 P2 P3 P4 P5
/// Synopsis: if typeof(P1.P3) in P5 goto P2
///
/// Salta para P2 se o tipo de uma coluna de uma b-tree é um dos tipos do bitmask P5
/// (SQLITE_INTEGER 0x01, SQLITE_FLOAT 0x02, SQLITE_TEXT 0x04, SQLITE_BLOB 0x08,
/// SQLITE_NULL 0x10). P1 é um cursor cujo cache de decodificação da linha vale ao menos até
/// a coluna P3; se a linha tem menos de P3 colunas, usa P4 como tipo. Se P1 é -1, P3 é um
/// registrador e o tipo vem do valor dele. Com P1>=0 o opcode não distingue NULL de REAL
/// de forma confiável quando há NaN no banco.
pub fn op_is_type(p: &mut Vdbe, p_op: &Op) -> OpFlow {
    let type_mask: u16;

    debug_assert!(p_op.p1 >= -1 && p_op.p1 < p.n_cursor);
    if p_op.p1 >= 0 {
        let p_c = p.ap_csr[p_op.p1 as usize].clone().unwrap();
        let c = p_c.borrow();
        debug_assert!(p_op.p3 >= 0);
        if p_op.p3 < c.n_hdr_parsed as i32 {
            let serial_type: u32 = c.a_type[p_op.p3 as usize];
            if serial_type >= 12 {
                if (serial_type & 1) != 0 {
                    type_mask = 0x04; // SQLITE_TEXT
                } else {
                    type_mask = 0x08; // SQLITE_BLOB
                }
            } else {
                const A_MASK: [u8; 12] = [
                    0x10, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x2, 0x01, 0x01, 0x10, 0x10,
                ];
                type_mask = A_MASK[serial_type as usize] as u16;
            }
        } else {
            type_mask = 1u16 << (p_op.p4.i - 1);
        }
    } else {
        let t = api::value_type(&p.a_mem[p_op.p3 as usize].borrow());
        type_mask = 1u16 << (t - 1);
    }
    if (type_mask & p_op.p5 as u16) != 0 {
        return OpFlow::JumpToP2;
    }
    OpFlow::Next
}


// ---- part_007.rs ----

// Notas de integração para o tech lead (mesma convenção de part_004.rs a part_006.rs):
// - Cada `case` é uma `fn op_xxx` que devolve `OpFlow`; `JumpToP2` e `JumpTo(addr)` são as
//   variantes de salto definidas em part_005.rs.
// - Modelo assumido de `VdbeCursor` (campos em snake_case do C): `e_cur_type`, `null_row` (u8),
//   `deferred_moveto` (u8), `seek_result` (i32), `cache_status` (u32), `payload_size` (u32),
//   `sz_row` (u32), `n_hdr_parsed` (u16), `i_hdr_offset` (u32), `n_field` (i16),
//   `a_row: Option<Vec<u8>>` (cópia dos `sz_row` bytes locais do payload; pode virar
//   `Option<Rc<[u8]>>` sem mudar este código), `a_type: Vec<u32>` e `a_offset: Vec<u32>`
//   (no C `aOffset == aType + nField`; aqui são dois vetores, com `n_field + 1` entradas
//   no mínimo, e o código os estende se um cursor pseudo exigir mais), `uc.p_cursor:
//   Option<BtCursorRef>`, `ub.a_alt_map: Option<Vec<u32>>` e `p_alt_cursor:
//   Option<VdbeCursorRef>`.
// - `Mem.z` é `Vec<u8>` com `sz_malloc` coerente; na rota rápida de texto o `pDest->z =
//   pDest->zMalloc` do C não tem equivalente, porque `z` já é o buffer do próprio `Mem`.
// - A leitura do cabeçalho do registro opera sobre uma cópia acolchoada com 9 zeros, o que
//   reproduz a "sobreleitura inofensiva" que o C faz num registro com `a_offset[0]==0`.
// - `op_column` recebe `a_op0_p3` (o `aOp[0].p3` do C) porque o registro corrompido salta
//   para `aOp[aOp[0].p3-1]` quando esse valor é positivo, e `col_cache_ctr` (a variável
//   local `colCacheCtr` de `vdbe_exec`).
// - Assume-se: `vdbe_finish_moveto(&mut VdbeCursor) -> i32`, `vdbe_handle_moved_cursor(&mut
//   VdbeCursor) -> i32`, `btree_cursor_has_moved(&BtCursor) -> i32`, `btree_payload_size(&BtCursor)
//   -> u32`, `btree_payload_fetch(&BtCursor) -> (Vec<u8>, u32)`, `get_varint32(&[u8], &mut u32) -> u8`,
//   `vdbe_mem_from_btree_zero_offset(&BtCursor, u32, &mut Mem) -> i32`, `vdbe_serial_get(&[u8],
//   u32, &mut Mem) -> u32`, `vdbe_column_from_overflow(&mut VdbeCursor, i32, u32, i64, u32, u32,
//   &mut Mem) -> i32`, a tabela global `CTYPE_MAP: [u8; 256]` e `corrupt_error(i32) -> i32`
//   (o `SQLITE_CORRUPT_BKPT`).

/// Opcode: ZeroOrNull P1 P2 P3 * *
/// Synopsis: r[P2] = 0 OR NULL
///
/// Se os registradores P1 e P3 NÃO são NULL, grava zero no registrador P2. Se qualquer um
/// dos dois é NULL, grava NULL em P2.
pub fn op_zero_or_null(p: &mut Vdbe, p_op: &Op) -> OpFlow {
    let flags1 = p.a_mem[p_op.p1 as usize].borrow().flags;
    let flags3 = p.a_mem[p_op.p3 as usize].borrow().flags;
    let p_out = p.a_mem[p_op.p2 as usize].clone();
    if (flags1 & MEM_NULL) != 0 || (flags3 & MEM_NULL) != 0 {
        vdbe_mem_set_null(&mut p_out.borrow_mut());
    } else {
        vdbe_mem_set_int64(&mut p_out.borrow_mut(), 0);
    }
    OpFlow::Next
}

/// Opcode: NotNull P1 P2 * * *
/// Synopsis: if r[P1]!=NULL goto P2
///
/// Salta para P2 se o valor do registrador P1 não é NULL.
pub fn op_not_null(p: &mut Vdbe, p_op: &Op) -> OpFlow {
    if (p.a_mem[p_op.p1 as usize].borrow().flags & MEM_NULL) == 0 {
        return OpFlow::JumpToP2;
    }
    OpFlow::Next
}

/// Opcode: IfNullRow P1 P2 P3 * *
/// Synopsis: if P1.nullRow then r[P3]=NULL, goto P2
///
/// Verifica se o cursor P1 aponta para uma linha NULL. Se sim, grava NULL no registrador P3
/// e salta para P2. Se não, cai adiante sem mudar nada. Se P1 não é um cursor aberto, o
/// opcode não faz nada.
pub fn op_if_null_row(p: &mut Vdbe, p_op: &Op) -> OpFlow {
    debug_assert!(p_op.p1 >= 0 && p_op.p1 < p.n_cursor);
    let null_row = match p.ap_csr[p_op.p1 as usize].as_ref() {
        Some(p_c) => p_c.borrow().null_row != 0,
        None => false,
    };
    if null_row {
        let p_out = p.a_mem[p_op.p3 as usize].clone();
        vdbe_mem_set_null(&mut p_out.borrow_mut());
        return OpFlow::JumpToP2;
    }
    OpFlow::Next
}

/// Opcode: Offset P1 P2 P3 * *
/// Synopsis: r[P3] = sqlite_offset(P1)
///
/// Grava em r[P3] o deslocamento em bytes, no arquivo do banco, do início do payload do
/// registro para o qual o cursor P1 aponta. P2 é o número da coluna do argumento de
/// sqlite_offset(); o opcode não o usa, mas o gerador de código sim. Só existe com
/// -DSQLITE_ENABLE_OFFSET_SQL_FUNC, que não consta nas opções do Debian listadas em
/// CONVENTIONS.md; por isso fica atrás da feature `enable_offset_sql_func`.
#[cfg(feature = "enable_offset_sql_func")]
pub fn op_offset(p: &mut Vdbe, p_op: &Op, rc: &mut i32) -> OpFlow {
    debug_assert!(p_op.p1 >= 0 && p_op.p1 < p.n_cursor);
    let p_c = p.ap_csr[p_op.p1 as usize].clone();
    let p_out = p.a_mem[p_op.p3 as usize].clone();
    let p_c = match p_c {
        Some(c) if c.borrow().e_cur_type == CURTYPE_BTREE => c,
        _ => {
            vdbe_mem_set_null(&mut p_out.borrow_mut());
            return OpFlow::Next;
        }
    };
    let mut c = p_c.borrow_mut();
    if c.deferred_moveto != 0 {
        *rc = vdbe_finish_moveto(&mut c);
        if *rc != 0 {
            return OpFlow::AbortDueToError;
        }
    }
    let p_crsr = c.uc.p_cursor.clone().unwrap();
    if btree_eof(&p_crsr.borrow()) != 0 {
        vdbe_mem_set_null(&mut p_out.borrow_mut());
    } else {
        vdbe_mem_set_int64(&mut p_out.borrow_mut(), btree_offset(&p_crsr.borrow()));
    }
    OpFlow::Next
}

/// Opcode: Column P1 P2 P3 P4 P5
/// Synopsis: r[P3]=PX cursor P1 column P2
///
/// Interpreta os dados para os quais o cursor P1 aponta como uma estrutura montada por
/// MakeRecord e extrai a coluna P2, gravando-a no registrador P3. Se o registro tem menos
/// de (P2+1) valores, extrai NULL, ou o valor de P4 quando este é um P4_MEM.
///
/// Com o bit OPFLAG_LENGTHARG em P5, o resultado só será usado por length() ou equivalente,
/// então o conteúdo de blobs grandes não é carregado. Com OPFLAG_TYPEOFARG, o resultado só
/// será usado por typeof(), IS NULL, IS NOT NULL ou equivalentes, e todo carregamento de
/// conteúdo pode ser omitido.
pub fn op_column(
    p: &mut Vdbe,
    db: &sqlite3,
    p_op: &Op,
    a_op0_p3: i32,
    encoding: u8,
    col_cache_ctr: u32,
    rc: &mut i32,
) -> OpFlow {
    debug_assert!(p_op.p1 >= 0 && p_op.p1 < p.n_cursor);
    debug_assert!(p_op.p3 > 0 && p_op.p3 <= p.n_mem + 1 - p.n_cursor);
    let p_dest = p.a_mem[p_op.p3 as usize].clone();
    let mut p_c: VdbeCursorRef = p.ap_csr[p_op.p1 as usize].clone().unwrap();
    let mut p2: u32 = p_op.p2 as u32; // Número da coluna a recuperar

    // op_column_restart:
    'restart: loop {
        let p_c_cur = p_c.clone();
        let mut c = p_c_cur.borrow_mut();
        debug_assert!(
            p2 < c.n_field as u32 || (c.e_cur_type == CURTYPE_PSEUDO && c.seek_result == 0)
        );
        debug_assert!(c.e_cur_type != CURTYPE_VTAB);
        debug_assert!(c.e_cur_type != CURTYPE_PSEUDO || c.null_row != 0);
        debug_assert!(c.e_cur_type != CURTYPE_SORTER);
        // Garante o espaço de a_type e a_offset (o C tem nField+1 entradas garantidas).
        let need = p2 as usize + 2;
        if c.a_offset.len() < need {
            c.a_offset.resize(need, 0);
        }
        if c.a_type.len() < need {
            c.a_type.resize(need, 0);
        }

        // `goto op_column_read_header` do C: pula os testes de nHdrParsed e iHdrOffset.
        let mut read_header_direct = false;

        if c.cache_status != p.cache_ctr {
            if c.null_row != 0 {
                if c.e_cur_type == CURTYPE_PSEUDO && c.seek_result > 0 {
                    // Num cursor pseudo, seek_result identifica o registrador com o registro.
                    let p_reg = p.a_mem[c.seek_result as usize].clone();
                    let reg = p_reg.borrow();
                    debug_assert!((reg.flags & MEM_BLOB) != 0);
                    c.payload_size = reg.n as u32;
                    c.sz_row = reg.n as u32;
                    c.a_row = Some(reg.z[..reg.n as usize].to_vec());
                } else {
                    vdbe_mem_set_null(&mut p_dest.borrow_mut());
                    return OpFlow::Next; // goto op_column_out
                }
            } else {
                let p_crsr = c.uc.p_cursor.clone().unwrap();
                if c.deferred_moveto != 0 {
                    debug_assert!(c.is_ephemeral == 0);
                    let mut alt: Option<(VdbeCursorRef, u32)> = None;
                    if let Some(map) = c.ub.a_alt_map.as_ref() {
                        let i_map = map[1 + p2 as usize];
                        if i_map > 0 {
                            alt = Some((c.p_alt_cursor.clone().unwrap(), i_map - 1));
                        }
                    }
                    if let Some((p_alt, new_p2)) = alt {
                        drop(c);
                        p_c = p_alt;
                        p2 = new_p2;
                        continue 'restart;
                    }
                    *rc = vdbe_finish_moveto(&mut c);
                    if *rc != 0 {
                        return OpFlow::AbortDueToError;
                    }
                } else if btree_cursor_has_moved(&p_crsr.borrow()) != 0 {
                    *rc = vdbe_handle_moved_cursor(&mut c);
                    if *rc != 0 {
                        return OpFlow::AbortDueToError;
                    }
                    drop(c);
                    continue 'restart;
                }
                debug_assert!(c.e_cur_type == CURTYPE_BTREE);
                debug_assert!(btree_cursor_is_valid(&p_crsr.borrow()) != 0);
                c.payload_size = btree_payload_size(&p_crsr.borrow());
                let (a_row, sz_row) = btree_payload_fetch(&p_crsr.borrow());
                c.a_row = Some(a_row);
                c.sz_row = sz_row;
                debug_assert!(c.sz_row <= c.payload_size);
                debug_assert!(c.sz_row <= 65536); // O tamanho máximo de página é 64KiB
            }
            c.cache_status = p.cache_ctr;
            let first: u8 = c.a_row.as_ref().map(|r| r.first().copied().unwrap_or(0)).unwrap_or(0);
            if first < 0x80 {
                c.a_offset[0] = first as u32;
                c.i_hdr_offset = 1;
            } else {
                let mut v: u32 = 0;
                let n = get_varint32(c.a_row.as_ref().unwrap(), &mut v);
                c.a_offset[0] = v;
                c.i_hdr_offset = n as u32;
            }
            c.n_hdr_parsed = 0;

            if c.sz_row < c.a_offset[0] {
                // aRow não precisa conter a linha inteira, mas precisa cobrir o cabeçalho do
                // registro. Se não cobre, zera aRow para forçar a alocação do cabeçalho.
                c.a_row = None;
                c.sz_row = 0;

                // Garante que um banco corrompido não nos dê um cabeçalho gigante: tipos
                // têm de 1 a 5 bytes, mas os de 4 e 5 bytes ocupam tanto espaço de dados
                // que só cabem 4096 e 32 deles. O cabeçalho máximo é 32768*3 + 3 = 98307.
                if c.a_offset[0] > 98307 || c.a_offset[0] > c.payload_size {
                    return op_column_corrupt(a_op0_p3, rc);
                }
            } else {
                // Otimização: pula os primeiros testes (nHdrParsed<=p2). O ramo vale mesmo
                // com a_offset[0]==0, registro que o SQLite nunca gera mas aceita por
                // razões históricas.
                debug_assert!((c.n_hdr_parsed as u32) <= p2);
                read_header_direct = true;
            }
        } else if btree_cursor_has_moved(&c.uc.p_cursor.clone().unwrap().borrow()) != 0 {
            *rc = vdbe_handle_moved_cursor(&mut c);
            if *rc != 0 {
                return OpFlow::AbortDueToError;
            }
            drop(c);
            continue 'restart;
        }

        // Garante que ao menos as p2+1 primeiras entradas do cabeçalho foram analisadas e
        // que a_offset[] e a_type[] têm informação válida.
        let t: u32;
        if read_header_direct || (c.n_hdr_parsed as u32) <= p2 {
            // Se há mais cabeçalho para analisar, tenta extrair campos até o (p2+1)-ésimo.
            let mut proceed = read_header_direct;
            let mut s_mem = Mem::default();
            let mut z_data: Vec<u8> = Vec::new();
            if !read_header_direct && c.i_hdr_offset < c.a_offset[0] {
                // zData tem de apontar para bytes que cubram o cabeçalho.
                if c.a_row.is_none() {
                    let p_crsr = c.uc.p_cursor.clone().unwrap();
                    *rc = vdbe_mem_from_btree_zero_offset(
                        &p_crsr.borrow(),
                        c.a_offset[0],
                        &mut s_mem,
                    );
                    if *rc != SQLITE_OK {
                        return OpFlow::AbortDueToError;
                    }
                    z_data = s_mem.z.clone();
                } else {
                    z_data = c.a_row.as_ref().unwrap().to_vec();
                }
                proceed = true;
            } else if read_header_direct {
                z_data = c.a_row.as_ref().unwrap().to_vec();
            }

            if proceed {
                // Sobreleitura inofensiva do C: acolchoa com zeros.
                z_data.extend_from_slice(&[0u8; 9]);
                // op_column_read_header:
                // Preenche a_type[i] e a_offset[i] até o p2-ésimo campo.
                let mut i: usize = c.n_hdr_parsed as usize;
                let mut offset64: u64 = c.a_offset[i] as u64;
                let mut z_hdr: usize = c.i_hdr_offset as usize;
                let z_end_hdr: usize = c.a_offset[0] as usize;
                loop {
                    let mut tt: u32 = z_data[z_hdr] as u32;
                    c.a_type[i] = tt;
                    if tt < 0x80 {
                        z_hdr += 1;
                        offset64 += vdbe_one_byte_serial_type_len(tt as u8) as u64;
                    } else {
                        z_hdr += get_varint32(&z_data[z_hdr..], &mut tt) as usize;
                        c.a_type[i] = tt;
                        offset64 += vdbe_serial_type_len(tt) as u64;
                    }
                    i += 1;
                    if c.a_offset.len() <= i {
                        c.a_offset.resize(i + 1, 0);
                        c.a_type.resize(i + 1, 0);
                    }
                    c.a_offset[i] = (offset64 & 0xffffffff) as u32;
                    if !((i as u32) <= p2 && z_hdr < z_end_hdr) {
                        break;
                    }
                }

                // O registro está corrompido se: (1) os bytes do cabeçalho passam do tamanho
                // declarado; (2) o cabeçalho inteiro foi usado mas nem todos os dados; ou
                // (3) o fim dos dados passa do fim do registro.
                if (z_hdr >= z_end_hdr && (z_hdr > z_end_hdr || offset64 != c.payload_size as u64))
                    || offset64 > c.payload_size as u64
                {
                    if c.a_offset[0] == 0 {
                        i = 0;
                        z_hdr = z_end_hdr;
                    } else {
                        vdbe_mem_release(&mut s_mem);
                        return op_column_corrupt(a_op0_p3, rc);
                    }
                }

                c.n_hdr_parsed = i as u16;
                c.i_hdr_offset = z_hdr as u32;
                vdbe_mem_release(&mut s_mem);
            }

            // Se mesmo depois de extrair novas entradas nHdrParsed não chegou a p2, o
            // registro tem menos de p2 colunas: o resultado é o valor padrão ou NULL.
            if (c.n_hdr_parsed as u32) <= p2 {
                let mut dest = p_dest.borrow_mut();
                match p_op.p4.p_mem.as_ref() {
                    Some(p_mem) if p_op.p4type == P4_MEM => {
                        vdbe_mem_shallow_copy(&mut dest, &p_mem.borrow(), MEM_STATIC);
                    }
                    _ => vdbe_mem_set_null(&mut dest),
                }
                return OpFlow::Next; // goto op_column_out
            }
            t = c.a_type[p2 as usize];
        } else {
            t = c.a_type[p2 as usize];
        }

        // Extrai o conteúdo da (p2+1)-ésima coluna. Aqui a_offset[p2], a_offset[p2+1] e
        // a_type[p2] são todos válidos.
        debug_assert!(p2 < c.n_hdr_parsed as u32);
        debug_assert!(*rc == SQLITE_OK);
        let mut dest = p_dest.borrow_mut();
        if vdbe_mem_dynamic(&dest) {
            vdbe_mem_set_null(&mut dest);
        }
        debug_assert!(t == c.a_type[p2 as usize]);
        if c.sz_row >= c.a_offset[p2 as usize + 1] {
            // Caso comum: o conteúdo cabe na página original, fora de páginas de overflow.
            let off = c.a_offset[p2 as usize] as usize;
            let a_row: &[u8] = c.a_row.as_deref().unwrap_or(&[]);
            let z_data: &[u8] = if off <= a_row.len() { &a_row[off..] } else { &[] };
            if t < 12 {
                vdbe_serial_get(z_data, t, &mut dest);
            } else {
                // Se o valor é string, precisamos de um valor persistente, não MEM_Ephem.
                // É um atalho equivalente a vdbe_serial_get() seguido de
                // vdbe_deephemeralize().
                const A_FLAG: [u16; 2] = [MEM_BLOB, MEM_STR | MEM_TERM];
                let len: i32 = ((t - 12) / 2) as i32;
                dest.n = len;
                dest.enc = encoding;
                if dest.sz_malloc < len + 2 {
                    if len > db.a_limit[SQLITE_LIMIT_LENGTH as usize] {
                        return OpFlow::TooBig;
                    }
                    dest.flags = MEM_NULL;
                    if vdbe_mem_grow(&mut dest, len + 2, 0) != 0 {
                        return OpFlow::NoMem;
                    }
                }
                let need = len as usize + 2;
                if dest.z.len() < need {
                    dest.z.resize(need, 0);
                }
                dest.z[..len as usize].copy_from_slice(&z_data[..len as usize]);
                dest.z[len as usize] = 0;
                dest.z[len as usize + 1] = 0;
                dest.flags = A_FLAG[(t & 1) as usize];
            }
        } else {
            dest.enc = encoding;
            // Este ramo só ocorre quando o conteúdo está em páginas de overflow.
            let p5: u8 = (p_op.p5 as i32 & OPFLAG_BYTELENARG as i32) as u8;
            if (p5 != 0
                && (p5 == OPFLAG_TYPEOFARG as u8
                    || (t >= 12 && ((t & 1) == 0 || p5 == OPFLAG_BYTELENARG as u8))))
                || vdbe_serial_type_len(t) == 0
            {
                // O conteúdo é irrelevante para typeof(), para length(X) se X é blob e
                // quando o comprimento é zero: usa conteúdo falso em vez de ler do disco.
                // O vetor global CTYPE_MAP tem 256 bytes e começa com zeros.
                vdbe_serial_get(&CTYPE_MAP[..], t, &mut dest);
            } else {
                let off = c.a_offset[p2 as usize] as i64;
                *rc = vdbe_column_from_overflow(
                    &mut c,
                    p2 as i32,
                    t,
                    off,
                    p.cache_ctr,
                    col_cache_ctr,
                    &mut dest,
                );
                if *rc != 0 {
                    if *rc == SQLITE_NOMEM {
                        return OpFlow::NoMem;
                    }
                    if *rc == SQLITE_TOOBIG {
                        return OpFlow::TooBig;
                    }
                    return OpFlow::AbortDueToError;
                }
            }
        }
        // op_column_out:
        return OpFlow::Next;
    }
}

/// Rótulo `op_column_corrupt` de OP_Column: se `aOp[0].p3>0` salta para esse endereço
/// (`pOp = &aOp[aOp[0].p3-1]`), senão aborta com SQLITE_CORRUPT.
fn op_column_corrupt(a_op0_p3: i32, rc: &mut i32) -> OpFlow {
    if a_op0_p3 > 0 {
        OpFlow::JumpTo(a_op0_p3)
    } else {
        *rc = corrupt_error(line!() as i32);
        OpFlow::AbortDueToError
    }
}


// ---- part_008.rs ----

/// Código de `REAL` quando o inteiro cabe em 6 bytes: troca MEM_INT por MEM_INTREAL; senão
/// converte para MEM_REAL. `drop_str` reproduz o `~(MEM_Int|MEM_Str)` do OP_Affinity (o
/// OP_TypeCheck só limpa MEM_INT).
fn real_affinity_fixup(p_in1: &mut Mem, drop_str: bool) {
    // Ao aplicar afinidade REAL, se o resultado ainda for um MEM_INT que cabe em 6 bytes,
    // muda o tipo para MEM_INTREAL para manter o valor inteiro de alta resolução, mas saber
    // que o tipo na verdade quer ser REAL.
    if p_in1.u.i <= 140737488355327i64 && p_in1.u.i >= -140737488355328i64 {
        p_in1.flags |= MEM_INTREAL;
        p_in1.flags &= !MEM_INT;
    } else {
        p_in1.u.r = p_in1.u.i as f64;
        p_in1.flags |= MEM_REAL;
        p_in1.flags &= if drop_str { !(MEM_INT | MEM_STR) } else { !MEM_INT };
    }
}

/// Opcode: TypeCheck P1 P2 P3 P4 *
/// Synopsis: typecheck(r[P1@P2])
///
/// Aplica afinidades ao intervalo de P2 registros começando com P1. As afinidades vêm do
/// objeto Table em P4. Se algum valor não puder ser coagido para o tipo correto, lança um
/// erro.
///
/// Este opcode é parecido com OP_Affinity, mas força o tipo do registro para o tipo da coluna
/// da tabela. Serve para implementar a "strict affinity".
///
/// Colunas GENERATED ALWAYS AS ... STATIC só são verificadas se P3 for zero. Com P3 diferente
/// de zero, não há verificação de tipo para colunas geradas estáticas. Colunas virtuais são
/// calculadas na hora da consulta e por isso nunca são verificadas.
pub fn op_type_check(p_op: &VdbeOp, db: &sqlite3, p: &mut Vdbe, a_mem: &mut [Mem]) -> CursorOpFlow {
    debug_assert!(p_op.p4_type == P4_TABLE);
    let p_tab_ref = match &p_op.p4 {
        P4Value::Table(tab) => tab.clone(),
        _ => unreachable!("OP_TypeCheck sem P4_TABLE"),
    };
    let p_tab = p_tab_ref.borrow();
    debug_assert!((p_tab.tab_flags & TF_STRICT) != 0);
    debug_assert!(p_tab.n_nv_col as i32 == p_op.p2);
    let encoding = enc(db);

    let mut idx = p_op.p1 as usize; // posição de pIn1 em a_mem
    for i in 0..(p_tab.n_col as usize) {
        let a_col = &p_tab.a_col[i];
        if (a_col.col_flags & COLFLAG_GENERATED) != 0 {
            if (a_col.col_flags & COLFLAG_VIRTUAL) != 0 {
                continue;
            }
            if p_op.p3 != 0 {
                idx += 1;
                continue;
            }
        }
        debug_assert!(idx < (p_op.p1 + p_op.p2) as usize);
        let p_in1 = &mut a_mem[idx];
        apply_affinity(p_in1, a_col.affinity, encoding);
        let mut type_error = false;
        if (p_in1.flags & MEM_NULL) == 0 {
            match a_col.e_c_type {
                COLTYPE_BLOB => {
                    if (p_in1.flags & MEM_BLOB) == 0 {
                        type_error = true;
                    }
                }
                COLTYPE_INTEGER | COLTYPE_INT => {
                    if (p_in1.flags & MEM_INT) == 0 {
                        type_error = true;
                    }
                }
                COLTYPE_TEXT => {
                    if (p_in1.flags & MEM_STR) == 0 {
                        type_error = true;
                    }
                }
                COLTYPE_REAL => {
                    debug_assert!((p_in1.flags & MEM_INTREAL) == 0);
                    if (p_in1.flags & MEM_INT) != 0 {
                        real_affinity_fixup(p_in1, false);
                    } else if (p_in1.flags & (MEM_REAL | MEM_INTREAL)) == 0 {
                        type_error = true;
                    }
                }
                _ => {
                    // COLTYPE_ANY: aceita qualquer coisa.
                }
            }
        }
        if type_error {
            // vdbe_type_error
            let type_name = vdbe_mem_type_name(p_in1);
            vdbe_error(
                p,
                b"cannot store %s value in %s column %s.%s",
                &[
                    type_name,
                    std_type[(a_col.e_c_type - 1) as usize],
                    &p_tab.z_name[..],
                    &a_col.z_cn_name[..],
                ],
            );
            return CursorOpFlow::Abort(SQLITE_CONSTRAINT_DATATYPE);
        }
        idx += 1;
    }
    debug_assert!(idx == (p_op.p1 + p_op.p2) as usize);
    CursorOpFlow::Next
}

/// Opcode: Affinity P1 P2 * P4 *
/// Synopsis: affinity(r[P1@P2])
///
/// Aplica afinidades a um intervalo de P2 registros começando com P1.
///
/// P4 é uma string de P2 caracteres. O N-ésimo caractere indica a afinidade de coluna que
/// deve ser usada para a N-ésima célula de memória do intervalo.
pub fn op_affinity(p_op: &VdbeOp, db: &sqlite3, a_mem: &mut [Mem]) -> CursorOpFlow {
    let z_affinity: &[u8] = match &p_op.p4 {
        P4Value::String(s) => s.as_slice(),
        _ => unreachable!("OP_Affinity sem P4 string"),
    };
    debug_assert!(p_op.p2 > 0);
    debug_assert!(z_affinity.len() == p_op.p2 as usize);
    let encoding = enc(db);
    for (k, &aff) in z_affinity.iter().enumerate() {
        let p_in1 = &mut a_mem[p_op.p1 as usize + k];
        apply_affinity(p_in1, aff, encoding);
        if aff == SQLITE_AFF_REAL && (p_in1.flags & MEM_INT) != 0 {
            real_affinity_fixup(p_in1, true);
        }
    }
    CursorOpFlow::Next
}

/// Opcode: MakeRecord P1 P2 P3 P4 *
/// Synopsis: r[P3]=mkrec(r[P1@P2])
///
/// Converte P2 registros começando com P1 no formato de registro usado como linha de dados
/// numa tabela ou como chave de índice. O opcode OP_Column decodifica o registro depois.
///
/// P4 pode ser uma string de P2 caracteres. O N-ésimo caractere indica a afinidade que deve
/// ser usada para o N-ésimo campo da chave. Se P4 for NULL, todos os campos têm afinidade BLOB.
///
/// Sem SQLITE_ENABLE_NULL_TRIM (o caso do Debian), P5 só vale OPFLAG_NOCHNG_MAGIC dentro de
/// um assert e não afeta o resultado.
pub fn op_make_record(p_op: &VdbeOp, db: &sqlite3, p: &Vdbe, a_mem: &mut [Mem]) -> CursorOpFlow {
    let mut n_data: u64 = 0; // Número de bytes de dados
    let mut n_hdr: i32 = 0; // Número de bytes do cabeçalho
    let mut n_zero: i64 = 0; // Bytes zero no fim do registro
    let encoding = enc(db);

    let z_affinity: Option<&[u8]> = match &p_op.p4 {
        P4Value::String(s) => Some(s.as_slice()),
        _ => None,
    };
    debug_assert!(p_op.p1 > 0 && p_op.p2 > 0 && p_op.p2 + p_op.p1 <= (p.n_mem + 1 - p.n_cursor) + 1);
    let i_data0 = p_op.p1 as usize; // primeiro campo do registro
    let n_field = p_op.p2 as usize;
    let i_last = i_data0 + n_field - 1; // último campo do registro

    // O registrador de saída não pode ser um dos de entrada.
    debug_assert!(p_op.p3 < p_op.p1 || p_op.p3 >= p_op.p1 + p_op.p2);
    let i_out = p_op.p3 as usize;

    // Aplica a afinidade pedida a todas as entradas.
    if let Some(z_aff) = z_affinity {
        for (k, &aff) in z_aff.iter().enumerate() {
            let p_rec = &mut a_mem[i_data0 + k];
            apply_affinity(p_rec, aff, encoding);
            if aff == SQLITE_AFF_REAL && (p_rec.flags & MEM_INT) != 0 {
                p_rec.flags |= MEM_INTREAL;
                p_rec.flags &= !MEM_INT;
            }
            debug_assert!(k + 1 == z_aff.len() || i_data0 + k + 1 <= i_last);
        }
    }

    // Percorre os elementos para saber quanto espaço o registro pede. Ao fim, Mem.u_temp de
    // cada termo guarda o serial-type usado:
    //
    //   u_temp          tipo
    //   0               NULL
    //   1 a 4           inteiro com sinal de 1, 2, 3, 4 bytes
    //   5               inteiro com sinal de 6 bytes
    //   6               inteiro com sinal de 8 bytes
    //   7               float IEEE
    //   8, 9            constantes inteiras 0 e 1
    //   10, 11          reservados
    //   N>=12 e par     BLOB
    //   N>=13 e ímpar   texto
    let mut i_rec = i_last;
    loop {
        if (a_mem[i_rec].flags & MEM_NULL) != 0 {
            if (a_mem[i_rec].flags & MEM_ZERO) != 0 {
                // Valores com MEM_NULL e MEM_ZERO nascem em xColumn de tabelas virtuais que
                // nunca chamam sqlite3_result_xxxxx() ao calcular uma coluna que não muda num
                // UPDATE. Recebem o serial-type interno 10 para chegar ao xUpdate como um
                // sqlite3_value_nochange() de verdade.
                a_mem[i_rec].u_temp = 10;
            } else {
                a_mem[i_rec].u_temp = 0;
            }
            n_hdr += 1;
        } else if (a_mem[i_rec].flags & (MEM_INT | MEM_INTREAL)) != 0 {
            // Decide entre 1, 2, 4, 6 ou 8 bytes.
            let i: i64 = a_mem[i_rec].u.i;
            let uu: u64 = if i < 0 { !i as u64 } else { i as u64 };
            n_hdr += 1;
            if uu <= 127 {
                if (i & 1) == i && p.min_write_file_format >= 4 {
                    a_mem[i_rec].u_temp = 8 + (uu as u32);
                } else {
                    n_data += 1;
                    a_mem[i_rec].u_temp = 1;
                }
            } else if uu <= 32767 {
                n_data += 2;
                a_mem[i_rec].u_temp = 2;
            } else if uu <= 8388607 {
                n_data += 3;
                a_mem[i_rec].u_temp = 3;
            } else if uu <= 2147483647 {
                n_data += 4;
                a_mem[i_rec].u_temp = 4;
            } else if uu <= 140737488355327u64 {
                n_data += 6;
                a_mem[i_rec].u_temp = 5;
            } else {
                n_data += 8;
                if (a_mem[i_rec].flags & MEM_INTREAL) != 0 {
                    // Se o valor é IntReal e vai ocupar 8 bytes como inteiro, é melhor
                    // guardá-lo como float de 8 bytes.
                    a_mem[i_rec].u.r = a_mem[i_rec].u.i as f64;
                    a_mem[i_rec].flags &= !MEM_INTREAL;
                    a_mem[i_rec].flags |= MEM_REAL;
                    a_mem[i_rec].u_temp = 7;
                } else {
                    a_mem[i_rec].u_temp = 6;
                }
            }
        } else if (a_mem[i_rec].flags & MEM_REAL) != 0 {
            n_hdr += 1;
            n_data += 8;
            a_mem[i_rec].u_temp = 7;
        } else {
            debug_assert!((a_mem[i_rec].flags & (MEM_STR | MEM_BLOB)) != 0);
            debug_assert!(a_mem[i_rec].n >= 0);
            let mut len: u32 = a_mem[i_rec].n as u32;
            let mut serial_type: u32 =
                len.wrapping_mul(2).wrapping_add(12).wrapping_add(((a_mem[i_rec].flags & MEM_STR) != 0) as u32);
            if (a_mem[i_rec].flags & MEM_ZERO) != 0 {
                serial_type = serial_type.wrapping_add((a_mem[i_rec].u.n_zero as u32).wrapping_mul(2));
                if n_data != 0 {
                    if vdbe_mem_expand_blob(&mut a_mem[i_rec]) != SQLITE_OK {
                        return CursorOpFlow::NoMem;
                    }
                    len = len.wrapping_add(a_mem[i_rec].u.n_zero as u32);
                } else {
                    n_zero += a_mem[i_rec].u.n_zero as i64;
                }
            }
            n_data += len as u64;
            n_hdr += varint_len(serial_type as u64);
            a_mem[i_rec].u_temp = serial_type;
        }
        if i_rec == i_data0 {
            break;
        }
        i_rec -= 1;
    }

    // EVIDENCE-OF: R-22564-11647 O cabeçalho começa com um único varint que dá o total de
    // bytes do cabeçalho, incluindo o próprio varint de tamanho.
    if n_hdr <= 126 {
        // O caso comum.
        n_hdr += 1;
    } else {
        // Caso raro de cabeçalho muito grande.
        let n_varint = varint_len(n_hdr as u64);
        n_hdr += n_varint;
        if n_varint < varint_len(n_hdr as u64) {
            n_hdr += 1;
        }
    }
    let n_byte: i64 = n_hdr as i64 + n_data as i64;

    // Garante que o registrador de saída tem um buffer grande o bastante para o registro.
    if n_byte + n_zero <= a_mem[i_out].sz_malloc as i64 {
        // O registrador de saída já é grande o bastante: nenhuma checagem nem ampliação. No
        // modelo sem ponteiros, z passa a ser uma cópia do buffer de z_malloc (que fica
        // intacto, como no C onde z aponta para ele).
        let mut z = a_mem[i_out].z_malloc.clone();
        if z.len() < n_byte as usize {
            z.resize(n_byte as usize, 0);
        }
        a_mem[i_out].z = z;
    } else {
        // Garante que a saída não é grande demais e depois amplia o registrador.
        if n_byte + n_zero > db.a_limit[SQLITE_LIMIT_LENGTH as usize] as i64 {
            return CursorOpFlow::TooBig;
        }
        if vdbe_mem_clear_and_resize(&mut a_mem[i_out], n_byte as i32) != SQLITE_OK {
            return CursorOpFlow::NoMem;
        }
    }

    // Escreve o registro num buffer local e o copia para a saída no fim.
    let mut buf: Vec<u8> = vec![0u8; n_byte as usize];
    let mut i_hdr: usize = 0; // onde escrever o próximo byte do cabeçalho
    let mut i_payload: usize = n_hdr as usize; // onde escrever o próximo byte do payload
    if n_hdr < 0x80 {
        buf[i_hdr] = n_hdr as u8;
        i_hdr += 1;
    } else {
        i_hdr += put_varint(&mut buf[i_hdr..], n_hdr as u64);
    }
    let mut i_rec = i_data0;
    loop {
        let serial_type: u32 = a_mem[i_rec].u_temp;
        // EVIDENCE-OF: R-06529-47362 Depois do varint de tamanho vêm um ou mais varints
        // adicionais, um por coluna.
        // EVIDENCE-OF: R-64536-51728 Os valores de cada coluna vêm logo depois do cabeçalho.
        if serial_type <= 7 {
            buf[i_hdr] = serial_type as u8;
            i_hdr += 1;
            if serial_type != 0 {
                // Valor NULL não muda i_payload.
                let mut v: u64 = if serial_type == 7 {
                    a_mem[i_rec].u.r.to_bits()
                } else {
                    a_mem[i_rec].u.i as u64
                };
                let len = small_type_sizes[serial_type as usize] as usize;
                debug_assert!(len >= 1 && len <= 8 && len != 5 && len != 7);
                // Big-endian: o byte menos significativo vai para o fim do campo.
                for k in (0..len).rev() {
                    buf[i_payload + k] = (v & 0xff) as u8;
                    v >>= 8;
                }
                i_payload += len;
            }
        } else if serial_type < 0x80 {
            buf[i_hdr] = serial_type as u8;
            i_hdr += 1;
            if serial_type >= 14 && a_mem[i_rec].n > 0 {
                let n = a_mem[i_rec].n as usize;
                buf[i_payload..i_payload + n].copy_from_slice(&a_mem[i_rec].z[..n]);
                i_payload += n;
            }
        } else {
            i_hdr += put_varint(&mut buf[i_hdr..], serial_type as u64);
            if a_mem[i_rec].n != 0 {
                let n = a_mem[i_rec].n as usize;
                buf[i_payload..i_payload + n].copy_from_slice(&a_mem[i_rec].z[..n]);
                i_payload += n;
            }
        }
        if i_rec == i_last {
            break;
        }
        i_rec += 1;
    }
    debug_assert!(n_hdr as usize == i_hdr);
    debug_assert!(n_byte as usize == i_payload);

    let p_out = &mut a_mem[i_out];
    p_out.z[..n_byte as usize].copy_from_slice(&buf);
    p_out.n = n_byte as i32;
    p_out.flags = MEM_BLOB;
    if n_zero != 0 {
        p_out.u.n_zero = n_zero as i32;
        p_out.flags |= MEM_ZERO;
    }
    CursorOpFlow::Next
}


// ---- part_009.rs ----

/// Opcode: Count P1 P2 P3 * *
/// Synopsis: r[P2]=count()
///
/// Armazena no registrador P2 o número de entradas (um valor inteiro) da tabela ou do índice
/// aberto pelo cursor P1.
///
/// Se P3==0, obtém-se uma contagem exata, o que exige visitar toda página btree da tabela. Se
/// P3 for diferente de zero, devolve-se uma estimativa baseada na posição atual do cursor.
pub fn op_count(p_op: &VdbeOp, db: &sqlite3, p: &mut Vdbe, a_mem: &mut [Mem]) -> CursorOpFlow {
    let p_c_ref = p.ap_csr[p_op.p1 as usize].as_ref().expect("OP_Count sem cursor").clone();
    let n_entry: i64;
    {
        let mut p_c = p_c_ref.borrow_mut();
        debug_assert!(p_c.e_cur_type == CURTYPE_BTREE);
        let p_crsr = seek_bt_cursor(&mut p_c);
        if p_op.p3 != 0 {
            n_entry = btree_row_count_est(p_crsr);
        } else {
            let mut n: i64 = 0;
            let rc = btree_count(db, p_crsr, &mut n);
            if rc != SQLITE_OK {
                return CursorOpFlow::Abort(rc);
            }
            n_entry = n;
        }
    }
    let p_out = out2_prerelease(p, p_op, a_mem);
    p_out.u.i = n_entry;
    CursorOpFlow::CheckForInterrupt
}

/// Opcode: Savepoint P1 * * P4 *
///
/// Abre, libera ou reverte o savepoint nomeado pelo parâmetro P4, conforme o valor de P1.
/// Para abrir um savepoint novo, P1==0 (SAVEPOINT_BEGIN). Para liberar (confirmar) um
/// savepoint existente, P1==1 (SAVEPOINT_RELEASE). Para reverter um savepoint existente,
/// P1==2 (SAVEPOINT_ROLLBACK).
///
/// `pc` é o índice da instrução em aOp (o `pOp - aOp` do C).
pub fn op_savepoint(p_op: &VdbeOp, pc: i32, db: &mut sqlite3, p: &mut Vdbe) -> CursorOpFlow {
    let p1 = p_op.p1;
    let z_name: &[u8] = match &p_op.p4 {
        P4Value::String(s) => s.as_slice(),
        _ => unreachable!("OP_Savepoint sem nome"),
    };
    let mut rc: i32 = SQLITE_OK;

    debug_assert!(db.p_savepoint.is_none() || db.auto_commit == 0);
    debug_assert!(p1 == SAVEPOINT_BEGIN || p1 == SAVEPOINT_RELEASE || p1 == SAVEPOINT_ROLLBACK);
    debug_assert!(db.p_savepoint.is_some() || db.is_transaction_savepoint == 0);
    debug_assert!(p.b_is_reader != 0);

    if p1 == SAVEPOINT_BEGIN {
        if db.n_vdbe_write > 0 {
            // Um savepoint novo não pode ser criado se há instruções de escrita ativas
            // (isto é, handles de blob incrementais abertos para leitura e escrita).
            vdbe_error(p, b"cannot open savepoint - SQL statements in progress", &[]);
            rc = SQLITE_BUSY;
        } else {
            // Esta chamada é válida mesmo se este savepoint for na verdade um savepoint de
            // transação (e portanto não deve disparar callbacks xSavepoint()). Se um
            // savepoint de transação está sendo aberto, o vetor db.a_v_trans é vazio.
            debug_assert!(db.auto_commit == 0 || db.n_v_trans == 0);
            rc = vtab_savepoint(db, SAVEPOINT_BEGIN, db.n_statement + db.n_savepoint);
            if rc != SQLITE_OK {
                return CursorOpFlow::Abort(rc);
            }

            // Cria a estrutura do savepoint novo.
            let p_new = Savepoint {
                z_name: z_name.to_vec(),
                n_deferred_cons: db.n_deferred_cons,
                n_deferred_imm_cons: db.n_deferred_imm_cons,
                p_next: db.p_savepoint.clone(),
            };

            // Se não há transação aberta, marca este como "savepoint de transação" especial.
            if db.auto_commit != 0 {
                db.auto_commit = 0;
                db.is_transaction_savepoint = 1;
            } else {
                db.n_savepoint += 1;
            }

            // Liga o savepoint novo à lista do handle do banco de dados.
            db.p_savepoint = Some(Rc::new(RefCell::new(p_new)));
        }
    } else {
        debug_assert!(p1 == SAVEPOINT_RELEASE || p1 == SAVEPOINT_ROLLBACK);
        let mut i_savepoint: i32 = 0;

        // Acha o savepoint nomeado. Se não existe, devolve um erro ao usuário.
        let mut p_savepoint_opt: Option<SavepointRef> = db.p_savepoint.clone();
        while let Some(p_sp) = p_savepoint_opt.clone() {
            if str_i_cmp(&p_sp.borrow().z_name, z_name) == 0 {
                break;
            }
            p_savepoint_opt = p_sp.borrow().p_next.clone();
            i_savepoint += 1;
        }
        if let Some(p_savepoint) = p_savepoint_opt {
            if db.n_vdbe_write > 0 && p1 == SAVEPOINT_RELEASE {
                // Não se pode liberar (confirmar) um savepoint se há instruções de escrita
                // ativas.
                vdbe_error(p, b"cannot release savepoint - SQL statements in progress", &[]);
                rc = SQLITE_BUSY;
            } else {
                // Decide se este é um savepoint de transação. Se for, e o comando for RELEASE,
                // a transação corrente é confirmada.
                let is_transaction = p_savepoint.borrow().p_next.is_none() && db.is_transaction_savepoint != 0;
                if is_transaction && p1 == SAVEPOINT_RELEASE {
                    rc = vdbe_check_fk(p, 1);
                    if rc != SQLITE_OK {
                        return CursorOpFlow::VdbeReturn(rc);
                    }
                    db.auto_commit = 1;
                    if vdbe_halt(p) == SQLITE_BUSY {
                        p.pc = pc;
                        db.auto_commit = 0;
                        rc = SQLITE_BUSY;
                        p.rc = rc;
                        return CursorOpFlow::VdbeReturn(rc);
                    }
                    rc = p.rc;
                    if rc != SQLITE_OK {
                        db.auto_commit = 0;
                    } else {
                        db.is_transaction_savepoint = 0;
                    }
                } else {
                    let is_schema_change: bool;
                    i_savepoint = db.n_savepoint - i_savepoint - 1;
                    if p1 == SAVEPOINT_ROLLBACK {
                        is_schema_change = (db.m_db_flags & DBFLAG_SCHEMACHANGE) != 0;
                        for ii in 0..db.n_db as usize {
                            if let Some(p_bt) = db.a_db[ii].p_bt.clone() {
                                rc = btree_trip_all_cursors(&p_bt, SQLITE_ABORT_ROLLBACK, (!is_schema_change) as i32);
                                if rc != SQLITE_OK {
                                    return CursorOpFlow::Abort(rc);
                                }
                            }
                        }
                    } else {
                        debug_assert!(p1 == SAVEPOINT_RELEASE);
                        is_schema_change = false;
                    }
                    for ii in 0..db.n_db as usize {
                        if let Some(p_bt) = db.a_db[ii].p_bt.clone() {
                            rc = btree_savepoint(&p_bt, p1, i_savepoint);
                            if rc != SQLITE_OK {
                                return CursorOpFlow::Abort(rc);
                            }
                        }
                    }
                    if is_schema_change {
                        expire_prepared_statements(db, 0);
                        reset_all_schemas_of_connection(db);
                        db.m_db_flags |= DBFLAG_SCHEMACHANGE;
                    }
                }
                if rc != SQLITE_OK {
                    return CursorOpFlow::Abort(rc);
                }

                // Seja RELEASE ou ROLLBACK, destrói todos os savepoints aninhados dentro do
                // savepoint operado.
                loop {
                    let p_top = db.p_savepoint.clone().expect("lista de savepoints sem o alvo");
                    if Rc::ptr_eq(&p_top, &p_savepoint) {
                        break;
                    }
                    db.p_savepoint = p_top.borrow().p_next.clone();
                    db.n_savepoint -= 1;
                }

                // Se for RELEASE, destrói também o savepoint operado. Se for ROLLBACK TO,
                // restaura o número de violações de restrições adiadas para o valor guardado
                // quando o savepoint foi criado.
                if p1 == SAVEPOINT_RELEASE {
                    debug_assert!(Rc::ptr_eq(db.p_savepoint.as_ref().unwrap(), &p_savepoint));
                    db.p_savepoint = p_savepoint.borrow().p_next.clone();
                    if !is_transaction {
                        db.n_savepoint -= 1;
                    }
                } else {
                    debug_assert!(p1 == SAVEPOINT_ROLLBACK);
                    db.n_deferred_cons = p_savepoint.borrow().n_deferred_cons;
                    db.n_deferred_imm_cons = p_savepoint.borrow().n_deferred_imm_cons;
                }

                if !is_transaction || p1 == SAVEPOINT_ROLLBACK {
                    rc = vtab_savepoint(db, p1, i_savepoint);
                    if rc != SQLITE_OK {
                        return CursorOpFlow::Abort(rc);
                    }
                }
            }
        } else {
            vdbe_error(p, b"no such savepoint: %s", &[z_name]);
            rc = SQLITE_ERROR;
        }
    }
    if rc != SQLITE_OK {
        return CursorOpFlow::Abort(rc);
    }
    if p.e_vdbe_state == VDBE_HALT_STATE {
        return CursorOpFlow::VdbeReturn(SQLITE_DONE);
    }
    CursorOpFlow::Next
}

/// Opcode: AutoCommit P1 P2 * * *
///
/// Define o flag de auto-commit do banco de dados como P1 (1 ou 0). Se P2 for verdadeiro,
/// reverte as transações btree ativas. Se houver VMs ativas (além desta), um ROLLBACK falha.
/// Um COMMIT falha se houver VMs de escrita ativas ou VMs ativas que usam cache compartilhado.
///
/// Esta instrução faz a VM parar.
pub fn op_auto_commit(p_op: &VdbeOp, pc: i32, db: &mut sqlite3, p: &mut Vdbe) -> CursorOpFlow {
    let desired_auto_commit = p_op.p1;
    let i_rollback = p_op.p2;
    debug_assert!(desired_auto_commit == 1 || desired_auto_commit == 0);
    debug_assert!(desired_auto_commit == 1 || i_rollback == 0);
    debug_assert!(db.n_vdbe_active > 0); // Pelo menos esta VM está ativa
    debug_assert!(p.b_is_reader != 0);

    if desired_auto_commit != db.auto_commit as i32 {
        if i_rollback != 0 {
            debug_assert!(desired_auto_commit == 1);
            rollback_all(db, SQLITE_ABORT_ROLLBACK);
            db.auto_commit = 1;
        } else if desired_auto_commit != 0 && db.n_vdbe_write > 0 {
            // Se a instrução implementa um COMMIT e outras VMs estão escrevendo, devolve um
            // erro indicando que as outras VMs precisam terminar antes.
            vdbe_error(p, b"cannot commit transaction - SQL statements in progress", &[]);
            return CursorOpFlow::Abort(SQLITE_BUSY);
        } else {
            let rc = vdbe_check_fk(p, 1);
            if rc != SQLITE_OK {
                return CursorOpFlow::VdbeReturn(rc);
            }
            db.auto_commit = desired_auto_commit as u8;
        }
        if vdbe_halt(p) == SQLITE_BUSY {
            p.pc = pc;
            db.auto_commit = (1 - desired_auto_commit) as u8;
            p.rc = SQLITE_BUSY;
            return CursorOpFlow::VdbeReturn(SQLITE_BUSY);
        }
        close_savepoints(db);
        if p.rc == SQLITE_OK {
            CursorOpFlow::VdbeReturn(SQLITE_DONE)
        } else {
            CursorOpFlow::VdbeReturn(SQLITE_ERROR)
        }
    } else {
        let msg: &[u8] = if desired_auto_commit == 0 {
            b"cannot start a transaction within a transaction"
        } else if i_rollback != 0 {
            b"cannot rollback - no transaction is active"
        } else {
            b"cannot commit - no transaction is active"
        };
        vdbe_error(p, msg, &[]);
        CursorOpFlow::Abort(SQLITE_ERROR)
    }
}

/// Opcode: Transaction P1 P2 P3 P4 P5
///
/// Inicia uma transação no banco de dados P1 se não houver uma ativa. Se P2 for diferente de
/// zero, inicia uma transação de escrita ou, se já há uma de leitura ativa, promove-a para
/// escrita. Se P2 for zero, inicia uma transação de leitura. Se P2 for 2 ou mais, inicia uma
/// transação exclusiva.
///
/// P1 é o índice do arquivo de banco de dados em que a transação começa. O índice 0 é o
/// arquivo principal e o 1 é o arquivo das tabelas temporárias. Índices de 2 em diante são de
/// bancos anexados.
///
/// Se uma transação de escrita é iniciada e o flag Vdbe.uses_stmt_journal é verdadeiro, uma
/// transação de instrução também pode ser aberta: isso acontece se a conexão não está em modo
/// autocommit ou se há outras instruções ativas.
///
/// Se P5!=0, o opcode também confere o cookie do esquema contra P3 e o contador de geração do
/// esquema contra P4. Se diferirem, levanta SQLITE_SCHEMA e a execução termina.
pub fn op_transaction(p_op: &VdbeOp, pc: i32, db: &mut sqlite3, p: &mut Vdbe) -> CursorOpFlow {
    let mut rc: i32 = SQLITE_OK;
    let mut i_meta: i32 = 0;

    debug_assert!(p.b_is_reader != 0);
    debug_assert!(p.read_only == 0 || p_op.p2 == 0);
    debug_assert!(p_op.p2 >= 0 && p_op.p2 <= 2);
    debug_assert!(p_op.p1 >= 0 && p_op.p1 < db.n_db);
    if p_op.p2 != 0 && (db.flags & (SQLITE_QUERYONLY | SQLITE_CORRUPTRDONLY)) != 0 {
        if (db.flags & SQLITE_QUERYONLY) != 0 {
            // Escritas proibidas pelo "PRAGMA query_only=TRUE".
            rc = SQLITE_READONLY;
        } else {
            // Escritas proibidas por um SQLITE_CORRUPT anterior na transação corrente.
            rc = SQLITE_CORRUPT;
        }
        return CursorOpFlow::Abort(rc);
    }
    let i_db = p_op.p1 as usize;
    let p_bt_opt = db.a_db[i_db].p_bt.clone();

    if let Some(p_bt) = p_bt_opt {
        rc = btree_begin_trans(&p_bt, p_op.p2, Some(&mut i_meta));
        if rc != SQLITE_OK {
            if (rc & 0xff) == SQLITE_BUSY {
                p.pc = pc;
                p.rc = rc;
                return CursorOpFlow::VdbeReturn(rc);
            }
            return CursorOpFlow::Abort(rc);
        }

        if p.uses_stmt_journal != 0 && p_op.p2 != 0 && (db.auto_commit == 0 || db.n_vdbe_read > 1) {
            debug_assert!(btree_txn_state(&p_bt) == SQLITE_TXN_WRITE);
            if p.i_statement == 0 {
                debug_assert!(db.n_statement >= 0 && db.n_savepoint >= 0);
                db.n_statement += 1;
                p.i_statement = db.n_savepoint + db.n_statement;
            }

            rc = vtab_savepoint(db, SAVEPOINT_BEGIN, p.i_statement - 1);
            if rc == SQLITE_OK {
                rc = btree_begin_stmt(&p_bt, p.i_statement);
            }

            // Guarda o valor atual do contador de restrições adiadas do handle. Se a
            // transação de instrução for revertida, esse contador também precisa voltar.
            p.n_stmt_def_cons = db.n_deferred_cons;
            p.n_stmt_def_imm_cons = db.n_deferred_imm_cons;
        }
    }
    debug_assert!(p_op.p5 == 0 || p_op.p4_type == P4_INT32);
    if rc == SQLITE_OK
        && p_op.p5 != 0
        && (i_meta != p_op.p3 || db.a_db[i_db].p_schema.borrow().i_generation != p4_int32(p_op))
    {
        // IMPLEMENTATION-OF: R-03189-51135 A cada instrução SQL executada, a versão do
        // esquema é conferida para garantir que o esquema não mudou desde o preparo.
        p.z_err_msg = Some(b"database schema has changed".to_vec());
        // Se o cookie de esquema do arquivo casa com o da representação em memória, o
        // esquema não é recarregado do arquivo. Com tabelas virtuais isso não é só uma
        // otimização: elas costumam guardar dados em outras tabelas SQLite, consultadas de
        // dentro de xNext() e afins, e uma consulta desatualizada não deve descartar o esquema.
        if db.a_db[i_db].p_schema.borrow().schema_cookie != i_meta {
            reset_one_schema(db, p_op.p1);
        }
        p.expired = 1;
        rc = SQLITE_SCHEMA;

        // Zera change_cnt_on para que o valor de sqlite3_changes() não seja alterado em
        // vdbe_halt(). Se a instrução for repreparada, change_cnt_on volta a ser ligado.
        p.change_cnt_on = 0;
    }
    if rc != SQLITE_OK {
        return CursorOpFlow::Abort(rc);
    }
    CursorOpFlow::Next
}


// ---- part_010.rs ----

/// Opcode: ReadCookie P1 P2 P3 * *
///
/// Lê o cookie de número P3 do banco de dados P1 e o escreve no registrador P2. P3==1 é a
/// versão do esquema. P3==2 é o formato do banco de dados. P3==3 é o tamanho recomendado do
/// cache do pager, e assim por diante. P1==0 é o arquivo principal e P1==1 é o arquivo das
/// tabelas temporárias.
///
/// Antes de executar esta instrução precisa haver um read-lock no banco de dados (uma
/// transação iniciada ou um cursor aberto).
pub fn op_read_cookie(p_op: &VdbeOp, db: &sqlite3, p: &mut Vdbe, a_mem: &mut [Mem]) -> CursorOpFlow {
    debug_assert!(p.b_is_reader != 0);
    let i_db = p_op.p1;
    let i_cookie = p_op.p3;
    debug_assert!(i_cookie < SQLITE_N_BTREE_META as i32);
    debug_assert!(i_db >= 0 && i_db < db.n_db);
    let p_bt = db.a_db[i_db as usize].p_bt.as_ref().expect("OP_ReadCookie sem btree");
    let mut i_meta: u32 = 0;
    btree_get_meta(p_bt, i_cookie, &mut i_meta);
    let p_out = out2_prerelease(p, p_op, a_mem);
    p_out.u.i = (i_meta as i32) as i64; // o C lê o u32 num `int` e depois promove a i64
    CursorOpFlow::Next
}

/// Opcode: SetCookie P1 P2 P3 * P5
///
/// Escreve o valor inteiro P3 no cookie de número P2 do banco de dados P1. P2==1 é a versão do
/// esquema. P2==2 é o formato do banco de dados. P2==3 é o tamanho recomendado do cache do
/// pager, e assim por diante. P1==0 é o arquivo principal e P1==1 é o arquivo das tabelas
/// temporárias.
///
/// Uma transação precisa estar iniciada antes deste opcode.
///
/// Se P2 for o cookie SCHEMA_VERSION (número 1), a versão interna do esquema vira P3-P5. O
/// "PRAGMA schema_version=N" usa P5 igual a 1, para que a versão interna difira da versão do
/// esquema do banco e resulte num reset do esquema.
pub fn op_set_cookie(p_op: &VdbeOp, db: &mut sqlite3, p: &mut Vdbe) -> CursorOpFlow {
    vdbe_incr_write_counter(p, None);
    debug_assert!(p_op.p2 < SQLITE_N_BTREE_META as i32);
    debug_assert!(p_op.p1 >= 0 && p_op.p1 < db.n_db);
    debug_assert!(p.read_only == 0);
    let i_db = p_op.p1 as usize;
    let p_bt = db.a_db[i_db].p_bt.clone().expect("OP_SetCookie sem btree");
    // Ver a nota sobre o deslocamento de índices em OP_ReadCookie.
    let rc = btree_update_meta(&p_bt, p_op.p2, p_op.p3 as u32);
    if p_op.p2 == BTREE_SCHEMA_VERSION as i32 {
        // Quando o cookie do esquema muda, registra o cookie novo internamente.
        db.a_db[i_db].p_schema.borrow_mut().schema_cookie = (p_op.p3 as u32).wrapping_sub(p_op.p5 as u32) as i32;
        db.m_db_flags |= DBFLAG_SCHEMACHANGE;
        fk_clear_trigger_cache(db, p_op.p1);
    } else if p_op.p2 == BTREE_FILE_FORMAT as i32 {
        // Registra mudanças no formato do arquivo.
        db.a_db[i_db].p_schema.borrow_mut().file_format = p_op.p3 as u8;
    }
    if p_op.p1 == 1 {
        // Invalida todas as instruções preparadas sempre que o esquema do banco TEMP muda.
        // Ticket #1644.
        expire_prepared_statements(db, 0);
        p.expired = 0;
    }
    if rc != SQLITE_OK {
        return CursorOpFlow::Abort(rc);
    }
    CursorOpFlow::Next
}

/// Opcode: OpenRead P1 P2 P3 P4 P5
/// Synopsis: root=P2 iDb=P3
///
/// Abre um cursor somente leitura para a tabela cuja página raiz é P2 num arquivo de banco de
/// dados. O arquivo é determinado por P3: 0 é o banco principal, 1 é o banco das tabelas
/// temporárias e acima de 1 é o banco anexado correspondente. O cursor novo recebe o
/// identificador P1. É um erro P1 ser negativo.
///
/// Bits permitidos em P5: 0x02 OPFLAG_SEEKEQ: o cursor só será usado em buscas de igualdade
/// (implementadas como um par OP_SeekGE/OP_IdxGT ou OP_SeekLE/OP_IdxLT).
///
/// P4 pode ser um inteiro (P4_INT32) ou um KeyInfo (P4_KEYINFO). Com KeyInfo, a tabela aberta
/// é uma btree de índice cujo conteúdo e ordenação o KeyInfo define. Com inteiro, é uma btree
/// de tabela com número de colunas não menor que o valor de P4.
///
/// Opcode: ReopenIdx P1 P2 P3 P4 P5
///
/// Funciona como OP_OpenRead, mas antes confere se o cursor P1 já está aberto na mesma btree;
/// se estiver, o opcode vira um no-op. Só pode ser usado com P5==0 ou P5==OPFLAG_SEEKEQ e com
/// P4 sendo P4_KEYINFO. O valor de P3 deve ser o mesmo de todo outro ReopenIdx ou OpenRead do
/// mesmo cursor.
///
/// Opcode: OpenWrite P1 P2 P3 P4 P5
///
/// Abre um cursor de leitura e escrita P1 na tabela ou índice cuja página raiz é P2 (ou cuja
/// raiz está no registrador P2 se o bit OPFLAG_P2ISREG estiver em P5). Bits de P5: OPFLAG_SEEKEQ
/// (0x02), OPFLAG_FORDELETE (0x08, dica ao mecanismo de armazenamento) e OPFLAG_P2ISREG (0x10,
/// usa o conteúdo do registrador P2 como página raiz).
pub fn op_open_read_write(p_op: &VdbeOp, db: &mut sqlite3, p: &mut Vdbe, a_mem: &mut [Mem]) -> CursorOpFlow {
    let rc: i32;
    let hints = (p_op.p5 as u32) & ((OPFLAG_BULKCSR as u32) | (OPFLAG_SEEKEQ as u32));

    if p_op.opcode == OP_REOPENIDX {
        debug_assert!(p_op.p5 == 0 || p_op.p5 as u32 == OPFLAG_SEEKEQ as u32);
        debug_assert!(p_op.p4_type == P4_KEYINFO);
        if let Some(p_cur_ref) = p.ap_csr[p_op.p1 as usize].clone() {
            let mut p_cur = p_cur_ref.borrow_mut();
            if p_cur.pgno_root == p_op.p2 as u32 {
                debug_assert!(p_cur.i_db as i32 == p_op.p3); // Garantido pelo gerador de código
                debug_assert!(p_cur.e_cur_type == CURTYPE_BTREE);
                btree_clear_cursor(seek_bt_cursor(&mut p_cur));
                // open_cursor_set_hints
                btree_cursor_hint_flags(seek_bt_cursor(&mut p_cur), hints);
                return CursorOpFlow::Next;
            }
        }
        // Se o cursor não está aberto ou está aberto noutro índice, cai no OP_OpenRead para
        // forçar a reabertura.
    }

    debug_assert!(p_op.opcode == OP_OPENWRITE || p_op.p5 == 0 || p_op.p5 as u32 == OPFLAG_SEEKEQ as u32);
    debug_assert!(p.b_is_reader != 0);
    debug_assert!(p_op.opcode == OP_OPENREAD || p_op.opcode == OP_REOPENIDX || p.read_only == 0);

    if p.expired == 1 {
        return CursorOpFlow::Abort(SQLITE_ABORT_ROLLBACK);
    }

    let mut n_field: i32 = 0;
    let mut p_key_info: Option<KeyInfoRef> = None;
    let mut p2: u32 = p_op.p2 as u32;
    let i_db = p_op.p3;
    debug_assert!(i_db >= 0 && i_db < db.n_db);
    let p_x = db.a_db[i_db as usize].p_bt.clone().expect("OP_OpenRead sem btree");
    let wr_flag: u32;
    if p_op.opcode == OP_OPENWRITE {
        debug_assert!(OPFLAG_FORDELETE as u32 == BTREE_FORDELETE as u32);
        wr_flag = (BTREE_WRCSR as u32) | ((p_op.p5 as u32) & (OPFLAG_FORDELETE as u32));
        let file_format = db.a_db[i_db as usize].p_schema.borrow().file_format;
        if file_format < p.min_write_file_format {
            p.min_write_file_format = file_format;
        }
    } else {
        wr_flag = 0;
    }
    if ((p_op.p5 as u32) & (OPFLAG_P2ISREG as u32)) != 0 {
        debug_assert!(p2 > 0);
        debug_assert!(p2 <= (p.n_mem + 1 - p.n_cursor) as u32);
        debug_assert!(p_op.opcode == OP_OPENWRITE);
        let p_in2 = &mut a_mem[p2 as usize];
        debug_assert!((p_in2.flags & MEM_INT) != 0);
        vdbe_mem_integerify(p_in2);
        p2 = p_in2.u.i as i32 as u32;
        // O valor de p2 sempre vem de um OP_CreateBtree anterior, que o deixa em 2 ou mais
        // ou falha. Numa falha, a instrução preparada teria parado antes de chegar aqui.
        debug_assert!(p2 >= 2);
    }
    if p_op.p4_type == P4_KEYINFO {
        if let P4Value::KeyInfo(k) = &p_op.p4 {
            debug_assert!(k.borrow().enc == enc(db));
            n_field = k.borrow().n_all_field as i32;
            p_key_info = Some(k.clone());
        }
    } else if p_op.p4_type == P4_INT32 {
        n_field = p4_int32(p_op);
    }
    debug_assert!(p_op.p1 >= 0);
    debug_assert!(n_field >= 0);
    let p_cur_ref = match allocate_cursor(p, p_op.p1, n_field, CURTYPE_BTREE) {
        Some(c) => c,
        None => return CursorOpFlow::NoMem,
    };
    let mut p_cur = p_cur_ref.borrow_mut();
    p_cur.i_db = i_db as i8;
    p_cur.null_row = 1;
    p_cur.is_ordered = 1;
    p_cur.pgno_root = p2;
    rc = btree_cursor(&p_x, p2, wr_flag as i32, p_key_info.as_ref(), &mut p_cur.uc);
    p_cur.p_key_info = p_key_info;
    // Define VdbeCursor.is_table. Versões antigas conferiam aqui se os flags da página raiz
    // eram sãos e relatavam corrupção; essa checagem foi para a camada btree.
    p_cur.is_table = (p_op.p4_type != P4_KEYINFO) as u8;

    // open_cursor_set_hints
    debug_assert!(OPFLAG_BULKCSR as u32 == BTREE_BULKLOAD as u32);
    debug_assert!(OPFLAG_SEEKEQ as u32 == BTREE_SEEK_EQ as u32);
    btree_cursor_hint_flags(seek_bt_cursor(&mut p_cur), hints);
    if rc != SQLITE_OK {
        return CursorOpFlow::Abort(rc);
    }
    CursorOpFlow::Next
}

/// Opcode: OpenDup P1 P2 * * *
///
/// Abre um cursor novo P1 que aponta para a mesma tabela efêmera que o cursor P2. O cursor P2
/// precisa ter sido aberto por um OP_OpenEphemeral anterior. Só cursores efêmeros podem ser
/// duplicados.
///
/// Cursores efêmeros duplicados servem para auto-junções de visões materializadas.
pub fn op_open_dup(p_op: &VdbeOp, p: &mut Vdbe) -> CursorOpFlow {
    let p_orig_ref = p.ap_csr[p_op.p2 as usize].as_ref().expect("OP_OpenDup sem cursor").clone();
    debug_assert!(p_orig_ref.borrow().is_ephemeral != 0); // Só cursores efêmeros podem ser duplicados

    let n_field = p_orig_ref.borrow().n_field;
    let p_cx_ref = match allocate_cursor(p, p_op.p1, n_field, CURTYPE_BTREE) {
        Some(c) => c,
        None => return CursorOpFlow::NoMem,
    };
    let mut p_orig = p_orig_ref.borrow_mut();
    let mut p_cx = p_cx_ref.borrow_mut();
    p_cx.null_row = 1;
    p_cx.is_ephemeral = 1;
    p_cx.p_key_info = p_orig.p_key_info.clone();
    p_cx.is_table = p_orig.is_table;
    p_cx.pgno_root = p_orig.pgno_root;
    p_cx.is_ordered = p_orig.is_ordered;
    p_cx.ub = p_orig.ub.clone();
    p_cx.no_reuse = 1;
    p_orig.no_reuse = 1;
    let p_btx = match &p_cx.ub {
        VdbeCursorUnion::PBtx(b) => b.clone(),
        _ => unreachable!("OP_OpenDup de cursor sem btree efêmera"),
    };
    let pgno_root = p_cx.pgno_root;
    let p_key_info = p_cx.p_key_info.clone();
    let rc = btree_cursor(&p_btx, pgno_root, BTREE_WRCSR as i32, p_key_info.as_ref(), &mut p_cx.uc);
    // btree_cursor() só pode falhar para o primeiro cursor aberto num banco de dados. Como já
    // existe um cursor aberto quando este opcode roda, ele não pode falhar.
    debug_assert!(rc == SQLITE_OK);
    let _ = rc;
    CursorOpFlow::Next
}

/// Opcode: OpenEphemeral P1 P2 P3 P4 P5
/// Synopsis: nColumn=P2
///
/// Abre um cursor novo P1 para uma tabela transitória. O cursor é sempre aberto para leitura
/// e escrita, mesmo que o banco principal seja somente leitura. A tabela efêmera é apagada
/// automaticamente quando o cursor fecha.
///
/// Se o cursor P1 já está aberto numa tabela efêmera, a tabela é limpa (todo o conteúdo é
/// apagado).
///
/// P2 é o número de colunas da tabela efêmera. O cursor aponta para uma tabela BTree se P4==0
/// e para um índice BTree se P4 não for 0; nesse caso P4 é o KeyInfo que define o formato das
/// chaves do índice.
///
/// P5 pode ser uma máscara dos flags BTREE_* de btree.h. Os flags BTREE_OMIT_JOURNAL e
/// BTREE_SINGLE são adicionados automaticamente.
///
/// Se P3 for positivo, reg[P3] é modificado de leve para poder servir como dado de tamanho
/// zero em OP_Insert. É uma otimização que evita um OP_Blob extra.
///
/// Opcode: OpenAutoindex P1 P2 * P4 *
///
/// Funciona como OP_OpenEphemeral, com outro nome para distinguir o uso: as tabelas criadas
/// por ele servem a índices transitórios automáticos em junções.
pub fn op_open_ephemeral(p_op: &VdbeOp, db: &mut sqlite3, p: &mut Vdbe, a_mem: &mut [Mem]) -> CursorOpFlow {
    const VFS_FLAGS: i32 = SQLITE_OPEN_READWRITE
        | SQLITE_OPEN_CREATE
        | SQLITE_OPEN_EXCLUSIVE
        | SQLITE_OPEN_DELETEONCLOSE
        | SQLITE_OPEN_TRANSIENT_DB;
    debug_assert!(p_op.p1 >= 0);
    debug_assert!(p_op.p2 >= 0);
    if p_op.p3 > 0 {
        // Faz de reg[P3] um valor que sirva de dado para btree_insert() com tamanho zero.
        debug_assert!(p_op.p2 == 0); // Só usado quando o número de colunas é zero
        debug_assert!(p_op.opcode == OP_OPENEPHEMERAL);
        debug_assert!((a_mem[p_op.p3 as usize].flags & MEM_NULL) != 0);
        a_mem[p_op.p3 as usize].n = 0;
        a_mem[p_op.p3 as usize].z.clear();
    }
    let rc: i32;
    let p_cx_ref: VdbeCursorRef;
    let existing = p.ap_csr[p_op.p1 as usize].clone();
    let reuse = match &existing {
        Some(c) => {
            let c = c.borrow();
            c.no_reuse == 0 && p_op.p2 <= c.n_field
        }
        None => false,
    };
    if reuse {
        // Se a tabela efêmera já está aberta e não tem duplicatas de OP_OpenDup, apaga todo o
        // conteúdo para ficar vazia de novo, em vez de criar uma tabela nova.
        let c_ref = existing.unwrap();
        let (p_btx, pgno_root) = {
            let mut c = c_ref.borrow_mut();
            debug_assert!(c.is_ephemeral != 0);
            c.seq_count = 0;
            c.cache_status = CACHE_STALE;
            match &c.ub {
                VdbeCursorUnion::PBtx(b) => (b.clone(), c.pgno_root),
                _ => unreachable!("cursor efêmero sem btree"),
            }
        };
        rc = btree_clear_table(&p_btx, pgno_root, None);
        p_cx_ref = c_ref;
    } else {
        p_cx_ref = match allocate_cursor(p, p_op.p1, p_op.p2, CURTYPE_BTREE) {
            Some(c) => c,
            None => return CursorOpFlow::NoMem,
        };
        let mut r: i32;
        let mut p_cx = p_cx_ref.borrow_mut();
        p_cx.is_ephemeral = 1;
        let mut p_btx_opt: Option<BtreeRef> = None;
        r = btree_open(
            &db.p_vfs,
            None,
            db,
            &mut p_btx_opt,
            BTREE_OMIT_JOURNAL | BTREE_SINGLE | (p_op.p5 as i32),
            VFS_FLAGS,
        );
        if let Some(b) = &p_btx_opt {
            p_cx.ub = VdbeCursorUnion::PBtx(b.clone());
        }
        if r == SQLITE_OK {
            let p_btx = p_btx_opt.clone().expect("btree_open sem btree");
            r = btree_begin_trans(&p_btx, 1, None);
            if r == SQLITE_OK {
                // Se é preciso um índice transitório, cria-o chamando btree_create_table()
                // com o flag BTREE_BLOBKEY antes de abri-lo. Se é preciso uma tabela
                // transitória, usa a tabela criada automaticamente com página raiz 1 (uma
                // tabela BLOB_INTKEY).
                p_cx.p_key_info = match &p_op.p4 {
                    P4Value::KeyInfo(k) => Some(k.clone()),
                    _ => None,
                };
                if let Some(p_key_info) = p_cx.p_key_info.clone() {
                    debug_assert!(p_op.p4_type == P4_KEYINFO);
                    let mut root: u32 = 0;
                    r = btree_create_table(&p_btx, &mut root, BTREE_BLOBKEY | (p_op.p5 as i32));
                    p_cx.pgno_root = root;
                    if r == SQLITE_OK {
                        debug_assert!(p_cx.pgno_root == SCHEMA_ROOT + 1);
                        debug_assert!(p_key_info.borrow().enc == enc(db));
                        r = btree_cursor(&p_btx, p_cx.pgno_root, BTREE_WRCSR as i32, Some(&p_key_info), &mut p_cx.uc);
                    }
                    p_cx.is_table = 0;
                } else {
                    p_cx.pgno_root = SCHEMA_ROOT;
                    r = btree_cursor(&p_btx, SCHEMA_ROOT, BTREE_WRCSR as i32, None, &mut p_cx.uc);
                    p_cx.is_table = 1;
                }
            }
            p_cx.is_ordered = (p_op.p5 as i32 != BTREE_UNORDERED as i32) as u8;
            if r != SQLITE_OK {
                btree_close(&p_btx);
            }
        }
        rc = r;
    }
    if rc != SQLITE_OK {
        return CursorOpFlow::Abort(rc);
    }
    p_cx_ref.borrow_mut().null_row = 1;
    CursorOpFlow::Next
}


// ---- part_011.rs ----

/// Resultado da execução de um opcode deste trecho, no lugar dos `goto` e do `break` do
/// `switch` de `sqlite3VdbeExec()`. O laço principal traduz cada variante assim:
///
/// * `Next`: o `break` do `case` (segue para a instrução seguinte);
/// * `JumpToP2`: `goto jump_to_p2`;
/// * `SkipNext`: `pOp++; break;` (pula a instrução que vem logo depois);
/// * `NoMem`: `goto no_mem`;
/// * `Abort(rc)`: `rc = ...; goto abort_due_to_error`, com o código de erro em `rc`;
/// * `JumpToNextP2`: `pOp++; goto jump_to_p2` (salta para o P2 da instrução seguinte);
/// * `JumpToP2CheckInterrupt`: `goto jump_to_p2_and_check_for_interrupt`;
/// * `CheckForInterrupt`: `goto check_for_interrupt` (sem salto);
/// * `TooBig`: `goto too_big`;
/// * `VdbeReturn(rc)`: `rc = ...; goto vdbe_return` (sem passar por abort_due_to_error).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CursorOpFlow {
    VdbeReturn(i32),
    Next,
    JumpToP2,
    SkipNext,
    NoMem,
    Abort(i32),
    JumpToNextP2,
    JumpToP2CheckInterrupt,
    CheckForInterrupt,
    TooBig,
}

/// Valor inteiro do P4 de uma instrução (`pOp->p4.i`), que vale 0 se o P4 não for P4_INT32.
pub fn p4_int32(p_op: &VdbeOp) -> i32 {
    match &p_op.p4 {
        P4Value::Int32(n) => *n,
        _ => 0,
    }
}

/// Acesso ao cursor de árvore B de um `VdbeCursor` (`pC->uc.pCursor`). Vale para os cursores
/// CURTYPE_BTREE e CURTYPE_PSEUDO, que guardam um `BtCursor` em `uc`.
pub fn seek_bt_cursor(p_c: &mut VdbeCursor) -> &mut BtCursor {
    match &mut p_c.uc {
        VdbeCursorCursorUnion::PCursor(p_cursor) => &mut **p_cursor,
        _ => unreachable!("cursor sem BtCursor em uc"),
    }
}

/// Opcode: SorterOpen P1 P2 P3 P4 *
///
/// Este opcode funciona como OP_OpenEphemeral, exceto que abre um índice transiente
/// projetado especificamente para ordenar tabelas grandes com um algoritmo de merge-sort
/// externo.
///
/// Se o argumento P3 for diferente de zero, indica que o sorter pode assumir que uma
/// ordenação estável considerando os primeiros P3 campos de cada chave basta para produzir
/// os resultados exigidos.
pub fn op_sorter_open(p_op: &VdbeOp, p: &mut Vdbe, db: &sqlite3) -> CursorOpFlow {
    debug_assert!(p_op.p1 >= 0);
    debug_assert!(p_op.p2 >= 0);
    let p_cx_ref = match allocate_cursor(p, p_op.p1, p_op.p2, CURTYPE_SORTER) {
        Some(p_cx_ref) => p_cx_ref,
        None => return CursorOpFlow::NoMem,
    };
    let mut p_cx = p_cx_ref.borrow_mut();
    if let P4Value::KeyInfo(p_key_info) = &p_op.p4 {
        p_cx.p_key_info = Some(p_key_info.clone());
    }
    let rc = vdbe_sorter_init(db, p_op.p3, &mut p_cx);
    if rc != SQLITE_OK {
        return CursorOpFlow::Abort(rc);
    }
    CursorOpFlow::Next
}

/// Opcode: SequenceTest P1 P2 * * *
/// Synopsis: if( cursor[P1].ctr++ ) pc = P2
///
/// P1 é um cursor de sorter. Se o contador de sequência estiver zerado neste momento,
/// salta para P2. Salte ou não, incrementa o valor da sequência.
pub fn op_sequence_test(p_op: &VdbeOp, p: &mut Vdbe) -> CursorOpFlow {
    debug_assert!(p_op.p1 >= 0 && p_op.p1 < p.n_cursor);
    let p_c_ref = p.ap_csr[p_op.p1 as usize]
        .as_ref()
        .expect("OP_SequenceTest sem cursor")
        .clone();
    let mut p_c = p_c_ref.borrow_mut();
    let old_count = p_c.seq_count;
    p_c.seq_count = old_count.wrapping_add(1);
    if old_count == 0 {
        return CursorOpFlow::JumpToP2;
    }
    CursorOpFlow::Next
}

/// Opcode: OpenPseudo P1 P2 P3 * *
/// Synopsis: P3 columns in r[P2]
///
/// Abre um novo cursor que aponta para uma tabela falsa contendo uma única linha de dados.
/// O conteúdo dessa linha é o conteúdo do registro de memória P2. Em outras palavras, o
/// cursor P1 vira um apelido para o conteúdo MEM_BLOB do registro P2.
///
/// Uma pseudotabela criada por este opcode guarda uma linha de saída do sorter, para que a
/// linha possa ser decomposta em colunas individuais com o opcode OP_Column. OP_Column é o
/// único opcode de cursor que funciona com uma pseudotabela.
///
/// P3 é o número de campos dos registros que a pseudotabela vai guardar. Se P2 for 0 ou
/// negativo, o pseudocursor devolve NULL em toda coluna.
pub fn op_open_pseudo(p_op: &VdbeOp, p: &mut Vdbe) -> CursorOpFlow {
    debug_assert!(p_op.p1 >= 0);
    debug_assert!(p_op.p3 >= 0);
    let p_cx_ref = match allocate_cursor(p, p_op.p1, p_op.p3, CURTYPE_PSEUDO) {
        Some(p_cx_ref) => p_cx_ref,
        None => return CursorOpFlow::NoMem,
    };
    let mut p_cx = p_cx_ref.borrow_mut();
    p_cx.null_row = 1;
    p_cx.seek_result = p_op.p2;
    p_cx.is_table = 1;
    // Dá a este pseudocursor um ponteiro BtCursor falso para que p_cx possa ser passado com
    // segurança a vdbe_cursor_moveto(). Isso evita um teste de e_cur_type==CURTYPE_BTREE
    // dentro de vdbe_cursor_moveto(), que é uma otimização de desempenho.
    p_cx.uc = VdbeCursorCursorUnion::PCursor(btree_fake_valid_cursor());
    debug_assert!(p_op.p5 == 0);
    CursorOpFlow::Next
}

/// Opcode: Close P1 * * * *
///
/// Fecha um cursor aberto anteriormente como P1. Se P1 não estiver aberto neste momento,
/// a instrução não faz nada.
pub fn op_close(p_op: &VdbeOp, p: &mut Vdbe) -> CursorOpFlow {
    debug_assert!(p_op.p1 >= 0 && p_op.p1 < p.n_cursor);
    let p_old = p.ap_csr[p_op.p1 as usize].take();
    vdbe_free_cursor(p, p_old);
    CursorOpFlow::Next
}

// OP_ColumnsUsed só existe com SQLITE_ENABLE_COLUMN_USED_MASK, opção que o Debian 13 não
// liga; por isso o opcode não tem tradução aqui.

/// Opcode: SeekGE P1 P2 P3 P4 *
/// Synopsis: key=r[P3@P4]
///
/// Se o cursor P1 se refere a uma tabela SQL (B-Tree com chaves inteiras), usa o valor do
/// registro P3 como chave. Se o cursor P1 se refere a um índice SQL, P3 é o primeiro de um
/// vetor de P4 registros usados como chave de índice desempacotada.
///
/// Reposiciona o cursor P1 para que aponte para a menor entrada maior ou igual ao valor da
/// chave. Se não houver registros maiores ou iguais à chave e P2 não for zero, salta para P2.
///
/// Se o cursor P1 foi aberto com a flag OPFLAG_SEEKEQ, este opcode ou cai num registro que
/// casa exatamente com a chave, ou provoca um salto para P2. Quando o cursor é OPFLAG_SEEKEQ,
/// este opcode precisa ser seguido por um opcode IdxLE com os mesmos argumentos. O IdxGT é
/// pulado se este opcode tiver sucesso, mas será usado nas iterações seguintes do laço. A
/// flag OPFLAG_SEEKEQ é uma dica à camada btree de que a busca é de igualdade.
///
/// Este opcode deixa o cursor configurado para andar em ordem direta, do começo para o fim.
/// Em outras palavras, o cursor passa a usar Next, não Prev.
///
/// Veja também: Found, NotFound, SeekLt, SeekGt, SeekLe
///
/// Opcode: SeekGT P1 P2 P3 P4 *
/// Synopsis: key=r[P3@P4]
///
/// Igual a SeekGE, mas aponta para a menor entrada estritamente maior que a chave. Se não
/// houver registros maiores que a chave e P2 não for zero, salta para P2. O cursor fica
/// configurado para andar em ordem direta.
///
/// Opcode: SeekLT P1 P2 P3 P4 *
/// Synopsis: key=r[P3@P4]
///
/// Reposiciona o cursor P1 para que aponte para a maior entrada menor que a chave. Se não
/// houver registros menores que a chave e P2 não for zero, salta para P2. O cursor fica
/// configurado para andar em ordem reversa, do fim para o começo: usa Prev, não Next.
///
/// Opcode: SeekLE P1 P2 P3 P4 *
/// Synopsis: key=r[P3@P4]
///
/// Reposiciona o cursor P1 para que aponte para a maior entrada menor ou igual à chave.
/// Se não houver registros menores ou iguais à chave e P2 não for zero, salta para P2. O
/// cursor fica configurado para andar em ordem reversa. Com OPFLAG_SEEKEQ vale o mesmo
/// contrato descrito em SeekGE, mas o opcode seguinte é um IdxGE e o que é pulado é o IdxGE.
pub fn op_seek(p_op: &VdbeOp, p: &mut Vdbe, a_mem: &mut [Mem]) -> CursorOpFlow {
    debug_assert!(p_op.p1 >= 0 && p_op.p1 < p.n_cursor);
    debug_assert!(p_op.p2 != 0);
    let p_c_ref = p.ap_csr[p_op.p1 as usize]
        .as_ref()
        .expect("OP_Seek sem cursor")
        .clone();
    let mut p_c = p_c_ref.borrow_mut();
    debug_assert!(p_c.e_cur_type == CURTYPE_BTREE);
    debug_assert!(OP_SEEKLE == OP_SEEKLT + 1);
    debug_assert!(OP_SEEKGE == OP_SEEKLT + 2);
    debug_assert!(OP_SEEKGT == OP_SEEKLT + 3);
    debug_assert!(p_c.is_ordered != 0);

    let mut oc: u8 = p_op.opcode; // Opcode
    let mut eq_only = false; // Só interessam resultados de igualdade
    let mut res: i32 = 0; // Resultado da comparação
    let mut rc: i32;
    p_c.null_row = 0;

    p_c.deferred_moveto = 0;
    p_c.cache_status = CACHE_STALE;

    // O rótulo seek_not_found do C: os `break 'search` abaixo são os `goto seek_not_found`.
    'search: {
        if p_c.is_table != 0 {
            // A flag OPFLAG_SEEKEQ/BTREE_SEEK_EQ só é ligada em cursores de índice.

            // O valor de entrada em P3 pode ser de qualquer tipo: inteiro, real, string,
            // blob ou NULL. Mas precisa ser inteiro antes de a busca poder ser feita, então
            // converte.
            let p_in3 = &mut a_mem[p_op.p3 as usize];
            let flags3: u16 = p_in3.flags;
            if (flags3 & (MEM_INT | MEM_REAL | MEM_INTREAL | MEM_STR)) == MEM_STR {
                apply_numeric_affinity(p_in3, false);
            }
            let i_key: i64 = vdbe_int_value(p_in3); // Valor inteiro da chave
            let new_type: u16 = p_in3.flags; // Tipo depois de aplicar a afinidade numérica
            p_in3.flags = flags3; // Mas devolve o tipo ao original

            // Se o valor de P3 não pôde virar inteiro sem perda de informação, é preciso um
            // tratamento especial...
            if (new_type & (MEM_INT | MEM_INTREAL)) == 0 {
                if (new_type & MEM_REAL) == 0 {
                    if (new_type & MEM_NULL) != 0 || oc >= OP_SEEKGE {
                        return CursorOpFlow::JumpToP2;
                    } else {
                        rc = btree_last(seek_bt_cursor(&mut p_c), &mut res);
                        if rc != SQLITE_OK {
                            return CursorOpFlow::Abort(rc);
                        }
                        break 'search;
                    }
                }
                let c: i32 = int_float_compare(i_key, p_in3.u.r);

                // Se a aproximação i_key for maior que o termo real da busca, troca > por >=
                // e <= por <. Por exemplo, se o termo é 4.9 e a aproximação inteira é 5:
                //
                //        (x >  4.9)    ->     (x >= 5)
                //        (x <= 4.9)    ->     (x <  5)
                if c > 0 {
                    debug_assert!(OP_SEEKGE == OP_SEEKGT - 1);
                    debug_assert!(OP_SEEKLT == OP_SEEKLE - 1);
                    debug_assert!((OP_SEEKLE & 0x0001) == (OP_SEEKGT & 0x0001));
                    if (oc & 0x0001) == (OP_SEEKGT & 0x0001) {
                        oc -= 1;
                    }
                }
                // Se a aproximação i_key for menor que o termo real da busca, troca < por <=
                // e >= por >.
                else if c < 0 {
                    debug_assert!(OP_SEEKLE == OP_SEEKLT + 1);
                    debug_assert!(OP_SEEKGT == OP_SEEKGE + 1);
                    debug_assert!((OP_SEEKLT & 0x0001) == (OP_SEEKGE & 0x0001));
                    if (oc & 0x0001) == (OP_SEEKLT & 0x0001) {
                        oc += 1;
                    }
                }
            }
            rc = btree_table_moveto(seek_bt_cursor(&mut p_c), i_key, 0, &mut res);
            p_c.moveto_target = i_key; // Usado por OP_Delete
            if rc != SQLITE_OK {
                return CursorOpFlow::Abort(rc);
            }
        } else {
            // Para um cursor com a dica OPFLAG_SEEKEQ/BTREE_SEEK_EQ, só os opcodes
            // OP_SeekGE e OP_SeekLE são permitidos, e eles precisam ser seguidos
            // imediatamente por um OP_IdxGT ou OP_IdxLT, respectivamente, com a mesma chave.
            if btree_cursor_has_hint(seek_bt_cursor(&mut p_c), BTREE_SEEK_EQ) != 0 {
                eq_only = true;
                debug_assert!(p_op.opcode == OP_SEEKGE || p_op.opcode == OP_SEEKLE);
            }

            let n_field: i32 = match &p_op.p4 {
                P4Value::Int32(n) => *n, // Número de colunas ou campos da chave
                _ => 0,
            };
            debug_assert!(p_op.p4type == P4_INT32);
            debug_assert!(n_field > 0);

            // A conta abaixo equivale à seguinte, só que mais rápida:
            //   if( oc==OP_SeekGT || oc==OP_SeekLE ){
            //     r.default_rc = -1;
            //   }else{
            //     r.default_rc = +1;
            //   }
            let default_rc: i8 = if (1 & (oc - OP_SEEKLT)) != 0 { -1 } else { 1 };
            debug_assert!(oc != OP_SEEKGT || default_rc == -1);
            debug_assert!(oc != OP_SEEKLE || default_rc == -1);
            debug_assert!(oc != OP_SEEKGE || default_rc == 1);
            debug_assert!(oc != OP_SEEKLT || default_rc == 1);

            let i_first = p_op.p3 as usize;
            let mut r = UnpackedRecord {
                p_key_info: p_c.p_key_info.clone(),
                a_mem: a_mem[i_first..i_first + (n_field as usize)].to_vec(),
                u: UnpackedRecordU::I(0),
                n: 0,
                n_field: n_field as u16,
                default_rc,
                err_code: 0,
                r1: 0,
                r2: 0,
                eq_seen: 0,
            };
            rc = btree_index_moveto(seek_bt_cursor(&mut p_c), &mut r, &mut res);
            if rc != SQLITE_OK {
                return CursorOpFlow::Abort(rc);
            }
            if eq_only && r.eq_seen == 0 {
                debug_assert!(res != 0);
                break 'search;
            }
        }
        if oc >= OP_SEEKGE {
            debug_assert!(oc == OP_SEEKGE || oc == OP_SEEKGT);
            if res < 0 || (res == 0 && oc == OP_SEEKGT) {
                res = 0;
                rc = btree_next(seek_bt_cursor(&mut p_c), 0);
                if rc != SQLITE_OK {
                    if rc == SQLITE_DONE {
                        res = 1;
                    } else {
                        return CursorOpFlow::Abort(rc);
                    }
                }
            } else {
                res = 0;
            }
        } else {
            debug_assert!(oc == OP_SEEKLT || oc == OP_SEEKLE);
            if res > 0 || (res == 0 && oc == OP_SEEKLT) {
                res = 0;
                rc = btree_previous(seek_bt_cursor(&mut p_c), 0);
                if rc != SQLITE_OK {
                    if rc == SQLITE_DONE {
                        res = 1;
                    } else {
                        return CursorOpFlow::Abort(rc);
                    }
                }
            } else {
                // res pode ser negativo porque a tabela está vazia. Confere se é o caso.
                res = btree_eof(seek_bt_cursor(&mut p_c));
            }
        }
    }
    // seek_not_found:
    debug_assert!(p_op.p2 > 0);
    if res != 0 {
        return CursorOpFlow::JumpToP2;
    } else if eq_only {
        return CursorOpFlow::SkipNext; // Pula o OP_IdxLt ou OP_IdxGT que vem em seguida
    }
    CursorOpFlow::Next
}


// ---- part_012.rs ----

/// Opcode: SeekScan P1 P2 * * P5
/// Synopsis: Scan-ahead up to P1 rows
///
/// Este opcode é um prefixo do OP_SeekGE: precisa vir imediatamente antes dele (`p_next` é
/// essa instrução seguinte). Usa os operandos P1 a P4 do OP_SeekGE seguinte (chamados
/// SeekOP.P1 a SeekOP.P4); deste opcode só valem P1, P2 e P5 (This.P1, This.P2 e This.P5).
///
/// Ajuda a otimizar operadores IN em índices de várias colunas quando o IN está nos termos
/// finais do índice, trocando buscas na árvore B por passos até a linha seguinte. Uma resposta
/// correta sai também se este opcode for omitido ou for um no-op.
///
/// SeekGE.P3 e SeekGE.P4 identificam a chave desempacotada (o "alvo") em que o cursor
/// SeekGE.P1 deve parar. Se o cursor não aponta para uma linha válida, o opcode é um no-op e
/// o controle passa ao OP_SeekGE. Se aponta para uma linha válida antes do alvo, tenta
/// posicioná-lo no alvo ou depois dele chamando btree_next() entre 1 e This.P1 vezes.
///
/// This.P5 diz o que fazer se o cursor termina numa linha válida além do alvo: com P5 falso
/// salta para SeekGE.P2 (encerra o laço); com P5 verdadeiro salta para This.P2.
///
/// Resultados possíveis:
///
/// 1. cursor sem linha válida: cai no OP_SeekGE seguinte;
/// 2. cursor ainda antes do alvo depois de This.P1 passos: cai no OP_SeekGE;
/// 3. cursor no alvo: salta para This.P2;
/// 4. cursor saiu do fim do índice: salta para SeekGE.P2, encerrando o laço;
/// 5. cursor numa linha além do alvo: salta para SeekOP.P2 se This.P5==0, senão para This.P2.
pub fn op_seek_scan(
    p_op: &VdbeOp,
    p_next: &VdbeOp,
    p: &mut Vdbe,
    db: &Sqlite3Ref,
    a_mem: &[Mem],
) -> CursorOpFlow {
    debug_assert!(p_next.opcode == OP_SEEKGE);
    debug_assert!(p_op.p1 > 0);
    let p_c_ref = p.ap_csr[p_next.p1 as usize]
        .as_ref()
        .expect("OP_SeekScan sem cursor")
        .clone();
    let mut p_c = p_c_ref.borrow_mut();
    debug_assert!(p_c.e_cur_type == CURTYPE_BTREE);
    debug_assert!(p_c.is_table == 0);
    if btree_cursor_is_valid_nn(seek_bt_cursor(&mut p_c)) == 0 {
        return CursorOpFlow::Next;
    }
    let mut n_step: i32 = p_op.p1;
    debug_assert!(n_step >= 1);
    let n_field = p4_int32(p_next) as u16;
    let i_first = p_next.p3 as usize;
    let mut r = UnpackedRecord {
        p_key_info: p_c.p_key_info.clone(),
        a_mem: a_mem[i_first..i_first + (n_field as usize)].to_vec(),
        u: UnpackedRecordU::I(0),
        n: 0,
        n_field,
        default_rc: 0,
        err_code: 0,
        r1: 0,
        r2: 0,
        eq_seen: 0,
    };
    let mut res: i32 = 0;
    loop {
        let rc = vdbe_idx_key_compare(db, &mut p_c, &mut r, &mut res);
        if rc != SQLITE_OK {
            return CursorOpFlow::Abort(rc);
        }
        if res > 0 && p_op.p5 == 0 {
            // seekscan_search_fail: salta para SeekGE.P2, encerrando o laço
            return CursorOpFlow::JumpToNextP2;
        }
        if res >= 0 {
            // Salta para This.P2, ignorando o opcode OP_SeekGE
            return CursorOpFlow::JumpToP2;
        }
        if n_step <= 0 {
            return CursorOpFlow::Next;
        }
        n_step -= 1;
        p_c.cache_status = CACHE_STALE;
        let rc = btree_next(seek_bt_cursor(&mut p_c), 0);
        if rc != SQLITE_OK {
            if rc == SQLITE_DONE {
                // rc = SQLITE_OK; goto seekscan_search_fail
                return CursorOpFlow::JumpToNextP2;
            }
            return CursorOpFlow::Abort(rc);
        }
    }
}

/// Opcode: SeekHit P1 P2 P3 * *
/// Synopsis: set P2<=seekHit<=P3
///
/// Aumenta ou diminui o valor seekHit do cursor P1, se preciso, para que fique entre P2 e P3.
///
/// O seekHit é o máximo de termos de um índice para os quais se sabe que há ao menos uma
/// correspondência. Se for menor que o total de termos de igualdade da busca, o OP_IfNoHope
/// pode rodar para ver se o laço IN pode ser abandonado cedo. P1 deve ser um cursor de árvore B
/// válido.
pub fn op_seek_hit(p_op: &VdbeOp, p: &mut Vdbe) -> CursorOpFlow {
    debug_assert!(p_op.p1 >= 0 && p_op.p1 < p.n_cursor);
    let p_c_ref = p.ap_csr[p_op.p1 as usize]
        .as_ref()
        .expect("OP_SeekHit sem cursor")
        .clone();
    let mut p_c = p_c_ref.borrow_mut();
    debug_assert!(p_op.p3 >= p_op.p2);
    if (p_c.seek_hit as i32) < p_op.p2 {
        p_c.seek_hit = p_op.p2 as u16;
    } else if (p_c.seek_hit as i32) > p_op.p3 {
        p_c.seek_hit = p_op.p3 as u16;
    }
    CursorOpFlow::Next
}

/// Opcode: IfNotOpen P1 P2 * * *
/// Synopsis: if( !csr[P1] ) goto P2
///
/// Se o cursor P1 não está aberto ou foi posto numa linha NULL pelo OP_NullRow, salta para a
/// instrução P2. Senão, segue adiante.
pub fn op_if_not_open(p_op: &VdbeOp, p: &mut Vdbe) -> CursorOpFlow {
    debug_assert!(p_op.p1 >= 0 && p_op.p1 < p.n_cursor);
    let jump = match &p.ap_csr[p_op.p1 as usize] {
        None => true,
        Some(p_cur) => p_cur.borrow().null_row != 0,
    };
    if jump {
        return CursorOpFlow::JumpToP2CheckInterrupt;
    }
    CursorOpFlow::Next
}

/// Opcode: Found P1 P2 P3 P4 *
/// Synopsis: key=r[P3@P4]
///
/// Se P4==0, o registro P3 guarda um blob feito por MakeRecord. Se P4>0, P3 é o primeiro de P4
/// registros que formam um registro desempacotado. O cursor P1 está num índice. Se o registro
/// é prefixo de alguma entrada de P1, salta para P2 e deixa P1 na entrada correspondente.
/// O cursor fica em estado em que Next funciona, mas Prev não.
///
/// Opcode: NotFound P1 P2 P3 P4 *
///
/// Igual a Found, mas salta para P2 se o registro NÃO é prefixo de nenhuma entrada; se é,
/// segue adiante com P1 na entrada correspondente. Nem Next nem Prev funcionam depois.
///
/// Opcode: IfNoHope P1 P2 P3 P4 *
///
/// Mesmos operandos de NotFound e IdxGT. É só uma tentativa de otimização: se seekHit de P1 é
/// menor que P4, roda como OP_NotFound para ver se há esperança de correspondência; se não há,
/// salta para P2 e abandona o laço IN cedo. Se seekHit >= P4, segue adiante.
///
/// Opcode: NoConflict P1 P2 P3 P4 *
///
/// Como NotFound, mas salta sempre que algum campo da chave de busca é NULL (uma chave com NULL
/// não conflita). Se todos os campos são não NULL e não há entrada correspondente, salta para
/// P2; se há, segue adiante com P1 na linha correspondente.
pub fn op_found(p_op: &VdbeOp, p: &mut Vdbe, a_mem: &mut [Mem]) -> CursorOpFlow {
    debug_assert!(p_op.p1 >= 0 && p_op.p1 < p.n_cursor);
    debug_assert!(p_op.p4type == P4_INT32);
    let p_c_ref = p.ap_csr[p_op.p1 as usize]
        .as_ref()
        .expect("OP_Found sem cursor")
        .clone();
    let mut p_c = p_c_ref.borrow_mut();
    if p_op.opcode == OP_IFNOHOPE && (p_c.seek_hit as i32) >= p4_int32(p_op) {
        return CursorOpFlow::Next;
    }
    // Daqui em diante o OP_IfNoHope cai no OP_NotFound (deliberate_fall_through).
    debug_assert!(p_c.e_cur_type == CURTYPE_BTREE);
    debug_assert!(p_c.is_table == 0);
    let i_first = p_op.p3 as usize;
    let n_field = p4_int32(p_op) as u16;
    let mut seek_result: i32 = 0;
    let rc: i32;
    if n_field > 0 {
        // Valores da chave num vetor de registradores
        debug_assert!(a_mem[i_first..i_first + (n_field as usize)]
            .iter()
            .all(|m| (m.flags & MEM_ZERO) == 0 || m.n == 0));
        let mut r = UnpackedRecord {
            p_key_info: p_c.p_key_info.clone(),
            a_mem: a_mem[i_first..i_first + (n_field as usize)].to_vec(),
            u: UnpackedRecordU::I(0),
            n: 0,
            n_field,
            default_rc: 0,
            err_code: 0,
            r1: 0,
            r2: 0,
            eq_seen: 0,
        };
        rc = btree_index_moveto(seek_bt_cursor(&mut p_c), &mut r, &mut seek_result);
    } else {
        // Chave composta gerada pelo OP_MakeRecord
        debug_assert!((a_mem[i_first].flags & MEM_BLOB) != 0);
        debug_assert!(p_op.opcode != OP_NOCONFLICT);
        let rc_expand = expand_blob(&mut a_mem[i_first]);
        debug_assert!(rc_expand == SQLITE_OK || rc_expand == SQLITE_NOMEM);
        if rc_expand != SQLITE_OK {
            return CursorOpFlow::NoMem;
        }
        let mut p_idx_key = match vdbe_alloc_unpacked_record(p_c.p_key_info.as_ref()) {
            Some(p_idx_key) => p_idx_key,
            None => return CursorOpFlow::NoMem,
        };
        vdbe_record_unpack(
            p_c.p_key_info.as_ref(),
            a_mem[i_first].n,
            &a_mem[i_first].z,
            &mut p_idx_key,
        );
        p_idx_key.default_rc = 0;
        rc = btree_index_moveto(seek_bt_cursor(&mut p_c), &mut p_idx_key, &mut seek_result);
    }
    p_c.seek_result = seek_result;
    if rc != SQLITE_OK {
        return CursorOpFlow::Abort(rc);
    }
    let already_exists = p_c.seek_result == 0;
    p_c.null_row = if already_exists { 0 } else { 1 };
    p_c.deferred_moveto = 0;
    p_c.cache_status = CACHE_STALE;
    if p_op.opcode == OP_FOUND {
        if already_exists {
            return CursorOpFlow::JumpToP2;
        }
    } else {
        if !already_exists {
            return CursorOpFlow::JumpToP2;
        }
        if p_op.opcode == OP_NOCONFLICT {
            // Para o OP_NoConflict, salta se algum campo de entrada for NULL, pois uma chave
            // com NULL não conflita
            for ii in 0..(n_field as usize) {
                if (a_mem[i_first + ii].flags & MEM_NULL) != 0 {
                    return CursorOpFlow::JumpToP2;
                }
            }
        }
        if p_op.opcode == OP_IFNOHOPE {
            p_c.seek_hit = p4_int32(p_op) as u16;
        }
    }
    CursorOpFlow::Next
}


// ---- part_013.rs ----

/// Equivale ao macro `HAS_UPDATE_HOOK(DB)` do sqliteInt.h com SQLITE_ENABLE_PREUPDATE_HOOK:
/// há um hook de pré-atualização ou de atualização configurado.
#[inline]
pub fn has_update_hook(db: &sqlite3) -> bool {
    db.x_pre_update_callback.is_some() || db.x_update_callback.is_some()
}

/// Opcode: SeekRowid P1 P2 P3 * *
/// Synopsis: intkey=r[P3]
///
/// P1 é o índice de um cursor aberto numa árvore B de tabela SQL (com chaves inteiras). Se o
/// registro P3 não contém um inteiro ou se P1 não contém um registro com rowid P3, salta
/// imediatamente para P2. Ou, se P2 é 0, levanta um erro SQLITE_CORRUPT. Se P1 contém um
/// registro com rowid P3, deixa o cursor apontando para esse registro e segue adiante.
///
/// O OP_NotExists faz a mesma operação, mas com ele o registro P3 deve conter
/// obrigatoriamente um inteiro; com este opcode, P3 pode não conter. O OP_NotFound faz a mesma
/// operação em árvores B de índice (com chaves de vários valores).
///
/// Este opcode deixa o cursor em estado em que Next e Prev não funcionam.
///
/// Opcode: NotExists P1 P2 P3 * *
/// Synopsis: intkey=r[P3]
///
/// Como o SeekRowid, mas P3 é sempre um inteiro: se P1 não contém um registro com rowid P3,
/// salta para P2 (ou levanta SQLITE_CORRUPT se P2 é 0). Senão deixa o cursor no registro e
/// segue adiante. O mesmo estado de cursor do SeekRowid vale aqui.
pub fn op_seek_rowid(
    p_op: &VdbeOp,
    p: &mut Vdbe,
    a_mem: &[Mem],
    encoding: u8,
) -> CursorOpFlow {
    let p_in3 = &a_mem[p_op.p3 as usize];
    let i_key: i64 = if p_op.opcode == OP_SEEKROWID && (p_in3.flags & (MEM_INT | MEM_INTREAL)) == 0
    {
        // Se p_in3.u.i não contém um inteiro, calcula i_key como o valor inteiro de p_in3.
        // Salta para P2 se p_in3 não pode virar inteiro sem perda de informação. O tipo de
        // p_in3 não pode mudar, pois outras partes da declaração preparada o usam.
        let mut x = p_in3.clone();
        apply_affinity(&mut x, SQLITE_AFF_NUMERIC, encoding);
        if (x.flags & MEM_INT) == 0 {
            return CursorOpFlow::JumpToP2;
        }
        x.u.i
    } else {
        // OP_NotExists (ou OP_SeekRowid com inteiro): o registro P3 já é um inteiro
        debug_assert!((p_in3.flags & MEM_INT) != 0 || p_op.opcode == OP_SEEKROWID);
        p_in3.u.i
    };
    debug_assert!(p_op.p1 >= 0 && p_op.p1 < p.n_cursor);
    // notExistsWithKey:
    let p_c_ref = p.ap_csr[p_op.p1 as usize]
        .as_ref()
        .expect("OP_SeekRowid sem cursor")
        .clone();
    let mut p_c = p_c_ref.borrow_mut();
    debug_assert!(p_c.is_table != 0);
    debug_assert!(p_c.e_cur_type == CURTYPE_BTREE);
    let mut res: i32 = 0;
    let mut rc = btree_table_moveto(seek_bt_cursor(&mut p_c), i_key, 0, &mut res);
    debug_assert!(rc == SQLITE_OK || res == 0);
    p_c.moveto_target = i_key; // Usado pelo OP_Delete
    p_c.null_row = 0;
    p_c.cache_status = CACHE_STALE;
    p_c.deferred_moveto = 0;
    p_c.seek_result = res;
    if res != 0 {
        debug_assert!(rc == SQLITE_OK);
        if p_op.p2 == 0 {
            rc = SQLITE_CORRUPT_BKPT;
        } else {
            return CursorOpFlow::JumpToP2;
        }
    }
    if rc != SQLITE_OK {
        return CursorOpFlow::Abort(rc);
    }
    CursorOpFlow::Next
}

/// Opcode: Sequence P1 P2 * * *
/// Synopsis: r[P2]=cursor[P1].ctr++
///
/// Encontra o próximo número de sequência disponível para o cursor P1 e o escreve no
/// registro P2. O número de sequência do cursor é incrementado depois desta instrução.
pub fn op_sequence(p_op: &VdbeOp, p: &mut Vdbe) -> CursorOpFlow {
    debug_assert!(p_op.p1 >= 0 && p_op.p1 < p.n_cursor);
    let p_c_ref = p.ap_csr[p_op.p1 as usize]
        .as_ref()
        .expect("OP_Sequence sem cursor")
        .clone();
    let mut p_c = p_c_ref.borrow_mut();
    debug_assert!(p_c.e_cur_type != CURTYPE_VTAB);
    let p_out = out2_prerelease(p, p_op);
    p_out.borrow_mut().u.i = p_c.seq_count;
    p_c.seq_count = p_c.seq_count.wrapping_add(1);
    CursorOpFlow::Next
}

/// Parte do OP_NewRowid que trata o registro P3 do AUTOINCREMENT: `p_mem` guarda o maior rowid
/// gerado antes. Devolve o novo `v`, ou SQLITE_FULL se o valor chegou ao máximo.
fn new_rowid_autoincrement(
    p_mem: &mut Mem,
    mut v: i64,
    use_random_rowid: bool,
    max_rowid: i64,
) -> Result<i64, i32> {
    vdbe_mem_integerify(p_mem);
    debug_assert!((p_mem.flags & MEM_INT) != 0); // mem(P3) contém um inteiro
    if p_mem.u.i == max_rowid || use_random_rowid {
        return Err(SQLITE_FULL); // IMP: R-17817-00630
    }
    if v < p_mem.u.i + 1 {
        v = p_mem.u.i + 1;
    }
    p_mem.u.i = v;
    Ok(v)
}

/// Opcode: NewRowid P1 P2 P3 * *
/// Synopsis: r[P2]=rowid
///
/// Obtém um novo número de registro inteiro (o "rowid") usado como chave de uma tabela. O
/// número ainda não foi usado como chave na tabela do cursor P1 e é escrito no registro P2.
///
/// Se P3>0, P3 é um registro no quadro raiz deste VDBE que guarda o maior número de registro
/// gerado até então. Nenhum rowid novo pode ser menor que esse valor. Quando ele chega ao
/// máximo, gera-se o erro SQLITE_FULL. O registro P3 é atualizado com o número gerado. Esse
/// mecanismo de P3 implementa o AUTOINCREMENT.
pub fn op_new_rowid(p_op: &VdbeOp, p: &mut Vdbe, a_mem: &mut [Mem]) -> CursorOpFlow {
    // O maior rowid possível: (i64)((((u64)0x7fffffff)<<32) | (u64)0xffffffff)
    const MAX_ROWID: i64 = i64::MAX;
    let mut v: i64 = 0; // O novo rowid
    let mut res: i32 = 0; // Resultado de um btree_last()
    let p_out = out2_prerelease(p, p_op);
    debug_assert!(p_op.p1 >= 0 && p_op.p1 < p.n_cursor);
    let p_c_ref = p.ap_csr[p_op.p1 as usize]
        .as_ref()
        .expect("OP_NewRowid sem cursor")
        .clone();
    let mut p_c = p_c_ref.borrow_mut();
    debug_assert!(p_c.is_table != 0);
    debug_assert!(p_c.e_cur_type == CURTYPE_BTREE);

    // O próximo rowid é obtido em duas etapas. Primeiro tenta-se achar o maior rowid existente
    // e somar um. Se o maior já é o inteiro positivo máximo, recorre-se ao segundo algoritmo,
    // probabilístico: escolhe-se um rowid ao acaso e vê-se se já existe na tabela. Se não
    // existe, deu certo; se existe, escolhe-se outro, até 100 vezes.
    if p_c.use_random_rowid == 0 {
        let rc = btree_last(seek_bt_cursor(&mut p_c), &mut res);
        if rc != SQLITE_OK {
            return CursorOpFlow::Abort(rc);
        }
        if res != 0 {
            v = 1; // IMP: R-61914-48074
        } else {
            debug_assert!(btree_cursor_is_valid(Some(&*seek_bt_cursor(&mut p_c))) != 0);
            v = btree_integer_key(seek_bt_cursor(&mut p_c));
            if v >= MAX_ROWID {
                p_c.use_random_rowid = 1;
            } else {
                v += 1; // IMP: R-29538-34987
            }
        }
    }

    if p_op.p3 != 0 {
        // P3 deve ser uma célula de memória válida.
        debug_assert!(p_op.p3 > 0);
        let use_random = p_c.use_random_rowid != 0;
        let result = if let Some(p_frame_first) = p.p_frame.clone() {
            let mut p_frame = p_frame_first;
            loop {
                let p_parent = p_frame.borrow().p_parent.clone();
                match p_parent {
                    Some(p_parent) => p_frame = p_parent,
                    None => break,
                }
            }
            debug_assert!(p_op.p3 <= p_frame.borrow().n_mem);
            let p_mem_ref = p_frame.borrow().a_mem[p_op.p3 as usize].clone();
            let mut p_mem = p_mem_ref.borrow_mut();
            new_rowid_autoincrement(&mut p_mem, v, use_random, MAX_ROWID)
        } else {
            debug_assert!(p_op.p3 <= (p.n_mem + 1 - p.n_cursor));
            new_rowid_autoincrement(&mut a_mem[p_op.p3 as usize], v, use_random, MAX_ROWID)
        };
        match result {
            Ok(new_v) => v = new_v,
            Err(rc) => return CursorOpFlow::Abort(rc),
        }
    }
    if p_c.use_random_rowid != 0 {
        // IMPLEMENTATION-OF: R-07677-41881 Se o maior ROWID é igual ao maior inteiro possível
        // (9223372036854775807), o mecanismo passa a escolher ROWIDs candidatos positivos ao
        // acaso até achar um que não foi usado.
        debug_assert!(p_op.p3 == 0); // Não há modo aleatório em tabela AUTOINCREMENT.
        let mut cnt: i32 = 0;
        let mut rc;
        loop {
            let mut buf = [0u8; 8];
            randomness(8, &mut buf);
            v = i64::from_ne_bytes(buf);
            v &= MAX_ROWID >> 1;
            v += 1; // Garante que v é maior que zero
            rc = btree_table_moveto(seek_bt_cursor(&mut p_c), v, 0, &mut res);
            if rc != SQLITE_OK || res != 0 {
                break;
            }
            cnt += 1;
            if cnt >= 100 {
                break;
            }
        }
        if rc != SQLITE_OK {
            return CursorOpFlow::Abort(rc);
        }
        if res == 0 {
            return CursorOpFlow::Abort(SQLITE_FULL); // IMP: R-38219-53002
        }
        debug_assert!(v > 0); // EV: R-40812-03570
    }
    p_c.deferred_moveto = 0;
    p_c.cache_status = CACHE_STALE;
    p_out.borrow_mut().u.i = v;
    CursorOpFlow::Next
}

/// Opcode: Insert P1 P2 P3 P4 P5
/// Synopsis: intkey=r[P3] data=r[P2]
///
/// Escreve uma entrada na tabela do cursor P1. Cria uma entrada nova se não existe, ou
/// sobrescreve os dados de uma existente. Os dados são o MEM_Blob do registro P2 e a chave
/// está no registro P3, que deve ser um MEM_Int.
///
/// Se OPFLAG_NCHANGE de P5 está ligada, o contador de mudanças é incrementado. Se
/// OPFLAG_LASTROWID está ligada, o rowid fica guardado para sqlite3_last_insert_rowid().
///
/// Se OPFLAG_USESEEKRESULT está ligada, a implementação pode evitar uma busca desnecessária
/// no cursor P1; só deve ser ligada se não houve buscas antes no cursor ou se a mais recente
/// usou chave igual a P3. OPFLAG_ISUPDATE distingue UPDATE de INSERT, o que só importa para o
/// hook de atualização.
///
/// P4 pode apontar para uma Table ou ser NULL. Se não é NULL, o hook de atualização
/// (sqlite3.xUpdateCallback) é chamado após uma inserção bem-sucedida.
///
/// Só funciona em tabelas; a instrução equivalente para índices é OP_IdxInsert.
///
/// O hook de pré-atualização exige o `VdbeRef` do statement (e não `&mut Vdbe`), por isso este
/// opcode recebe `p_v` e só toma empréstimos curtos dele.
pub fn op_insert(
    p_op: &VdbeOp,
    p_v: &VdbeRef,
    db: &Sqlite3Ref,
    a_mem: &[Mem],
    col_cache_ctr: &mut u32,
) -> CursorOpFlow {
    let p5 = p_op.p5 as u8;
    let p_data = &a_mem[p_op.p2 as usize]; // Célula MEM com os dados do registro
    let p_key = &a_mem[p_op.p3 as usize]; // Célula MEM com a chave do registro
    debug_assert!(p_op.p1 >= 0 && p_op.p1 < p_v.borrow().n_cursor);
    let p_c_ref = p_v.borrow().ap_csr[p_op.p1 as usize]
        .clone()
        .expect("OP_Insert sem cursor");
    debug_assert!(p_c_ref.borrow().e_cur_type == CURTYPE_BTREE);
    debug_assert!(p_c_ref.borrow().deferred_moveto == 0);
    debug_assert!((p5 & OPFLAG_ISNOOP) != 0 || p_c_ref.borrow().is_table != 0);
    debug_assert!(p_op.p4type == P4_TABLE || p_op.p4type >= P4_STATIC);
    debug_assert!((p_key.flags & MEM_INT) != 0);
    let n_key: i64 = p_key.u.i;

    // Nome do banco (usado pelo hook de atualização) e a Table do P4
    let (z_db, mut p_tab): (Vec<u8>, Option<TableRef>) =
        if p_op.p4type == P4_TABLE && has_update_hook(&db.borrow()) {
            let i_db = p_c_ref.borrow().i_db;
            debug_assert!(i_db >= 0);
            let z_db = db.borrow().a_db[i_db as usize]
                .z_db_sname
                .clone()
                .unwrap_or_default();
            let p_tab = match &p_op.p4 {
                P4Value::Table(p_tab) => Some(p_tab.clone()),
                _ => None,
            };
            debug_assert!(
                (p5 & OPFLAG_ISNOOP) != 0
                    || p_tab.as_ref().map_or(false, |t| has_rowid(&t.borrow()))
            );
            (z_db, p_tab)
        } else {
            (Vec::new(), None)
        };

    // Invoca o hook de pré-atualização, se houver
    if let Some(p_tab_ref) = p_tab.clone() {
        let (has_pre_update, has_update) = {
            let db_b = db.borrow();
            (
                db_b.x_pre_update_callback.is_some(),
                db_b.x_update_callback.is_some(),
            )
        };
        if has_pre_update && (p5 & OPFLAG_ISUPDATE) == 0 {
            vdbe_pre_update_hook(
                p_v,
                &p_c_ref,
                SQLITE_INSERT,
                &z_db,
                &p_tab_ref,
                n_key,
                p_op.p2,
                -1,
            );
        }
        if !has_update || p_tab_ref.borrow().a_col.is_empty() {
            // Impede o hook pós-atualização de rodar onde não deve
            p_tab = None;
        }
    }
    if (p5 & OPFLAG_ISNOOP) != 0 {
        return CursorOpFlow::Next;
    }

    debug_assert!((p5 & OPFLAG_LASTROWID) == 0 || (p5 & OPFLAG_NCHANGE) != 0);
    if (p5 & OPFLAG_NCHANGE) != 0 {
        p_v.borrow_mut().n_change += 1;
        if (p5 & OPFLAG_LASTROWID) != 0 {
            db.borrow_mut().last_rowid = n_key;
        }
    }
    debug_assert!((p_data.flags & (MEM_BLOB | MEM_STR)) != 0 || p_data.n == 0);
    let x = BtreePayload {
        p_key: None,
        n_key,
        p_data: Some(p_data.z[..(p_data.n as usize)].to_vec()),
        a_mem: None,
        n_mem: 0,
        n_data: p_data.n,
        n_zero: if (p_data.flags & MEM_ZERO) != 0 {
            p_data.u.n_zero
        } else {
            0
        },
    };
    debug_assert!(BTREE_PREFORMAT == OPFLAG_PREFORMAT as i32);
    let rc;
    {
        let mut p_c = p_c_ref.borrow_mut();
        let seek_result = if (p5 & OPFLAG_USESEEKRESULT) != 0 {
            p_c.seek_result
        } else {
            0
        };
        rc = btree_insert(
            seek_bt_cursor(&mut p_c),
            &x,
            (p5 & (OPFLAG_APPEND | OPFLAG_SAVEPOSITION | OPFLAG_PREFORMAT)) as i32,
            seek_result,
        );
        p_c.deferred_moveto = 0;
        p_c.cache_status = CACHE_STALE;
    }
    *col_cache_ctr = col_cache_ctr.wrapping_add(1);

    // Invoca o hook de atualização, se preciso.
    if rc != SQLITE_OK {
        return CursorOpFlow::Abort(rc);
    }
    if let Some(p_tab_ref) = p_tab {
        let (x_update_callback, p_update_arg) = {
            let db_b = db.borrow();
            (db_b.x_update_callback.clone(), db_b.p_update_arg.clone())
        };
        let p_tab_b = p_tab_ref.borrow();
        debug_assert!(x_update_callback.is_some());
        debug_assert!(!p_tab_b.a_col.is_empty());
        if let Some(x_update_callback) = x_update_callback {
            x_update_callback(
                &p_update_arg,
                if (p5 & OPFLAG_ISUPDATE) != 0 {
                    SQLITE_UPDATE
                } else {
                    SQLITE_INSERT
                },
                &z_db,
                &p_tab_b.z_name,
                n_key,
            );
        }
    }
    CursorOpFlow::Next
}


// ---- part_014.rs ----

/// Opcode: RowCell P1 P2 P3 * *
///
/// P1 e P2 são cursores abertos, ambos sobre o mesmo tipo de tabela (intkey ou índice). Este
/// opcode faz parte da cópia da linha atual de P2 para P1. Se os cursores são de tabelas
/// intkey, o registro P3 contém o rowid a usar no registro novo de P1; se são de índice, P3
/// não é usado.
///
/// Deve ser seguido por um Insert ou IdxInsert com a flag OPFLAG_PREFORMAT, que completa a
/// inserção. `p_next` é essa instrução seguinte (só entra nas verificações).
pub fn op_row_cell(
    p_op: &VdbeOp,
    p_next: &VdbeOp,
    p: &mut Vdbe,
    a_mem: &[Mem],
) -> CursorOpFlow {
    debug_assert!(p_next.opcode == OP_INSERT || p_next.opcode == OP_IDXINSERT);
    debug_assert!(p_next.opcode == OP_INSERT || p_op.p3 == 0);
    debug_assert!(p_next.opcode == OP_IDXINSERT || p_op.p3 > 0);
    debug_assert!((p_next.p5 as u8 & OPFLAG_PREFORMAT) != 0);
    let p_dest_ref = p.ap_csr[p_op.p1 as usize]
        .as_ref()
        .expect("OP_RowCell sem cursor de destino")
        .clone();
    let p_src_ref = p.ap_csr[p_op.p2 as usize]
        .as_ref()
        .expect("OP_RowCell sem cursor de origem")
        .clone();
    let i_key: i64 = if p_op.p3 != 0 {
        a_mem[p_op.p3 as usize].u.i
    } else {
        0
    };
    let mut p_dest = p_dest_ref.borrow_mut();
    let mut p_src = p_src_ref.borrow_mut();
    let rc = btree_transfer_row(
        seek_bt_cursor(&mut p_dest),
        seek_bt_cursor(&mut p_src),
        i_key,
    );
    if rc != SQLITE_OK {
        return CursorOpFlow::Abort(rc);
    }
    CursorOpFlow::Next
}

/// Opcode: Delete P1 P2 P3 P4 P5
///
/// Apaga o registro para o qual o cursor P1 aponta no momento.
///
/// Se o bit OPFLAG_SAVEPOSITION de P5 está ligado, o cursor fica apontando para o registro
/// seguinte ou para o anterior da tabela. Se fica no seguinte, o próximo Next é um no-op, de
/// modo que se pode apagar um registro dentro de um laço Next. Com o bit desligado o cursor
/// fica em estado indefinido.
///
/// O bit OPFLAG_AUXDELETE de P5 indica que este delete é um de vários associados à remoção de
/// uma linha de tabela e de todas as suas entradas de índice. Exatamente um deles é o
/// "primário"; os outros estão em cursores OPFLAG_FORDELETE ou marcados com AUXDELETE.
///
/// Se OPFLAG_NCHANGE (0x01) de P2 (atenção: P2, não P5) está ligado, o contador de mudanças é
/// incrementado. Se OPFLAG_ISNOOP (0x40) de P2 está ligado, roda o hook de pré-atualização
/// de deletes, mas a árvore B não muda. Isso acontece quando o OP_Delete será seguido por um
/// OP_Insert com a mesma chave, que sobrescreve a entrada.
///
/// P1 não pode ser pseudotabela: precisa ser uma tabela real com várias linhas.
///
/// Se P4 não é NULL, aponta para uma Table. Nesse caso o hook de atualização, o de
/// pré-atualização ou ambos podem ser chamados, e o cursor P1 deve ter sido posicionado com
/// OP_NotFound antes. O hook de pré-atualização roda se P4 não é NULL; o de atualização roda
/// se P4 não é NULL e OPFLAG_NCHANGE de P2 está ligado.
///
/// Se OPFLAG_ISUPDATE está ligado em P2, P3 contém o endereço da célula de memória com o valor
/// que o rowid da linha terá depois do update.
///
/// Recebe o `VdbeRef` do statement porque o hook de pré-atualização o exige.
pub fn op_delete(
    p_op: &VdbeOp,
    p_v: &VdbeRef,
    db: &Sqlite3Ref,
    a_mem: &[Mem],
    col_cache_ctr: &mut u32,
) -> CursorOpFlow {
    let opflags: i32 = p_op.p2;
    let p5 = p_op.p5 as u8;
    debug_assert!(p_op.p1 >= 0 && p_op.p1 < p_v.borrow().n_cursor);
    let p_c_ref = p_v.borrow().ap_csr[p_op.p1 as usize]
        .clone()
        .expect("OP_Delete sem cursor");
    debug_assert!(p_c_ref.borrow().e_cur_type == CURTYPE_BTREE);
    debug_assert!(p_c_ref.borrow().deferred_moveto == 0);

    // Se o hook de atualização ou o de pré-atualização for chamado, z_db recebe o nome do
    // banco que vai para ele, e p_tab uma cópia de p4.pTab. Se p5 tem SAVEPOSITION, o cursor
    // foi movido pela última vez com OP_Next ou OP_Prev (e não por Seek ou NotFound), e então
    // moveto_target recebe o rowid atual.
    let (z_db, p_tab): (Vec<u8>, Option<TableRef>) =
        if p_op.p4type == P4_TABLE && has_update_hook(&db.borrow()) {
            let mut p_c = p_c_ref.borrow_mut();
            debug_assert!(p_c.i_db >= 0);
            let z_db = db.borrow().a_db[p_c.i_db as usize]
                .z_db_sname
                .clone()
                .unwrap_or_default();
            let p_tab = match &p_op.p4 {
                P4Value::Table(p_tab) => p_tab.clone(),
                _ => unreachable!("OP_Delete com P4_TABLE sem Table"),
            };
            if (p5 & OPFLAG_SAVEPOSITION) != 0 && p_c.is_table != 0 {
                p_c.moveto_target = btree_integer_key(seek_bt_cursor(&mut p_c));
            }
            (z_db, Some(p_tab))
        } else {
            (Vec::new(), None)
        };

    // Invoca o hook de pré-atualização, se preciso.
    if let Some(p_tab_ref) = &p_tab {
        if db.borrow().x_pre_update_callback.is_some() {
            debug_assert!(
                (opflags & OPFLAG_ISUPDATE as i32) == 0
                    || !has_rowid(&p_tab_ref.borrow())
                    || (a_mem[p_op.p3 as usize].flags & MEM_INT) != 0
            );
            let moveto_target = p_c_ref.borrow().moveto_target;
            vdbe_pre_update_hook(
                p_v,
                &p_c_ref,
                if (opflags & OPFLAG_ISUPDATE as i32) != 0 {
                    SQLITE_UPDATE
                } else {
                    SQLITE_DELETE
                },
                &z_db,
                p_tab_ref,
                moveto_target,
                p_op.p3,
                -1,
            );
        }
    }
    if (opflags & OPFLAG_ISNOOP as i32) != 0 {
        return CursorOpFlow::Next;
    }

    // As únicas flags possíveis são SAVEPOSITION e AUXDELETE
    debug_assert!((p5 & !(OPFLAG_SAVEPOSITION | OPFLAG_AUXDELETE)) == 0);
    debug_assert!(OPFLAG_SAVEPOSITION == BTREE_SAVEPOSITION);
    debug_assert!(OPFLAG_AUXDELETE == BTREE_AUXDELETE);

    let (rc, moveto_target) = {
        let mut p_c = p_c_ref.borrow_mut();
        let rc = btree_delete(seek_bt_cursor(&mut p_c), p5);
        p_c.cache_status = CACHE_STALE;
        p_c.seek_result = 0;
        (rc, p_c.moveto_target)
    };
    *col_cache_ctr = col_cache_ctr.wrapping_add(1);
    if rc != SQLITE_OK {
        return CursorOpFlow::Abort(rc);
    }

    // Invoca o hook de atualização, se preciso.
    if (opflags & OPFLAG_NCHANGE as i32) != 0 {
        p_v.borrow_mut().n_change += 1;
        let (x_update_callback, p_update_arg) = {
            let db_b = db.borrow();
            (db_b.x_update_callback.clone(), db_b.p_update_arg.clone())
        };
        if let Some(x_update_callback) = x_update_callback {
            if let Some(p_tab_ref) = &p_tab {
                let p_tab_b = p_tab_ref.borrow();
                if has_rowid(&p_tab_b) {
                    x_update_callback(
                        &p_update_arg,
                        SQLITE_DELETE,
                        &z_db,
                        &p_tab_b.z_name,
                        moveto_target,
                    );
                    debug_assert!(p_c_ref.borrow().i_db >= 0);
                }
            }
        }
    }
    CursorOpFlow::Next
}

/// Opcode: ResetCount * * * * *
///
/// O valor do contador de mudanças é copiado para o contador do handle do banco (devolvido
/// pelas chamadas seguintes a sqlite3_changes()). Depois o contador interno da VM volta a 0.
/// É usado por programas de trigger.
pub fn op_reset_count(p: &mut Vdbe, db: &Sqlite3Ref) -> CursorOpFlow {
    vdbe_set_changes(&mut db.borrow_mut(), p.n_change);
    p.n_change = 0;
    CursorOpFlow::Next
}

/// Opcode: SorterCompare P1 P2 P3 P4
/// Synopsis: if key(P1)!=trim(r[P3],P4) goto P2
///
/// P1 é um cursor de sorter. Compara um prefixo do blob do registro P3 com um prefixo da
/// entrada para a qual o cursor do sorter aponta. Só os primeiros P4 campos de r[P3] e do
/// registro do sorter são comparados.
///
/// Se P3 ou o sorter tem um NULL num dos campos significativos (sem contar os P4 campos finais,
/// que são ignorados), a comparação é tida como igual.
///
/// Segue adiante se os dois registros são iguais; salta para P2 se são diferentes.
pub fn op_sorter_compare(p_op: &VdbeOp, p: &mut Vdbe, a_mem: &[Mem]) -> CursorOpFlow {
    let p_c_ref = p.ap_csr[p_op.p1 as usize]
        .as_ref()
        .expect("OP_SorterCompare sem cursor")
        .clone();
    let mut p_c = p_c_ref.borrow_mut();
    debug_assert!(p_c.e_cur_type == CURTYPE_SORTER);
    debug_assert!(p_op.p4type == P4_INT32);
    let p_in3 = &a_mem[p_op.p3 as usize];
    let n_key_col = p4_int32(p_op);
    let mut res: i32 = 0;
    let rc = vdbe_sorter_compare(&mut p_c, p_in3, n_key_col, &mut res);
    if rc != SQLITE_OK {
        return CursorOpFlow::Abort(rc);
    }
    if res != 0 {
        return CursorOpFlow::JumpToP2;
    }
    CursorOpFlow::Next
}

/// Opcode: SorterData P1 P2 P3 * *
/// Synopsis: r[P2]=data
///
/// Escreve no registro P2 os dados atuais do sorter do cursor P1. Depois limpa o cache de
/// cabeçalhos de coluna do cursor P3.
///
/// Normalmente serve para tirar um registro do sorter e pô-lo num registro que é a origem de
/// um cursor de pseudotabela criado com OpenPseudo (o identificado por P3). Limpar o cache de
/// P3 aqui poupa uma instrução NullRow separada.
pub fn op_sorter_data(p_op: &VdbeOp, p: &mut Vdbe, a_mem: &mut [Mem]) -> CursorOpFlow {
    debug_assert!(p_op.p1 >= 0 && p_op.p1 < p.n_cursor);
    {
        let p_c_ref = p.ap_csr[p_op.p1 as usize]
            .as_ref()
            .expect("OP_SorterData sem cursor")
            .clone();
        let p_c = p_c_ref.borrow();
        debug_assert!(p_c.e_cur_type == CURTYPE_SORTER);
        let p_out = &mut a_mem[p_op.p2 as usize];
        let rc = vdbe_sorter_rowkey(&p_c, p_out);
        debug_assert!(rc != SQLITE_OK || (p_out.flags & MEM_BLOB) != 0);
        if rc != SQLITE_OK {
            return CursorOpFlow::Abort(rc);
        }
    }
    p.ap_csr[p_op.p3 as usize]
        .as_ref()
        .expect("OP_SorterData sem cursor de pseudotabela")
        .borrow_mut()
        .cache_status = CACHE_STALE;
    CursorOpFlow::Next
}

/// Opcode: RowData P1 P2 P3 * *
/// Synopsis: r[P2]=data
///
/// Escreve no registro P2 o conteúdo completo da linha para a qual o cursor P1 aponta, sem
/// interpretação nenhuma: é copiado para P2 exatamente como está no arquivo do banco. Se P1 é
/// um índice, o conteúdo é a chave da linha; se é uma tabela, são os dados.
///
/// O cursor P1 deve apontar para uma linha válida (não uma linha NULL) de uma tabela real, não
/// de uma pseudotabela.
///
/// Se P3!=0, este opcode pode criar um ponteiro efêmero para a página do banco. O conteúdo do
/// registro de saída é então invalidado assim que o cursor se mover, inclusive por movimentos
/// causados por outros cursores que "salvam" a posição do atual para escrever na mesma tabela.
/// Se P3==0, faz-se uma cópia dos dados na memória: P3!=0 é mais rápido, P3==0 é mais seguro.
///
/// Se P3!=0, o conteúdo de P2 não serve para o OP_Result, e qualquer OP_Result o invalida. O
/// conteúdo de P2 é invalidado por opcodes como OP_Function ou por qualquer uso de outro cursor
/// que aponte para a mesma tabela.
pub fn op_row_data(p_op: &VdbeOp, p: &mut Vdbe, db: &Sqlite3Ref) -> CursorOpFlow {
    let p_out_ref = out2_prerelease(p, p_op);
    debug_assert!(p_op.p1 >= 0 && p_op.p1 < p.n_cursor);
    let p_c_ref = p.ap_csr[p_op.p1 as usize]
        .as_ref()
        .expect("OP_RowData sem cursor")
        .clone();
    let mut p_c = p_c_ref.borrow_mut();
    debug_assert!(p_c.e_cur_type == CURTYPE_BTREE);
    debug_assert!(p_c.null_row == 0);

    // Os OP_RowData sempre seguem OP_NotExists, OP_SeekRowid ou OP_Rewind/OP_Next sem
    // instruções no meio que possam invalidar o cursor. Se isso mudar, o conserto é inserir
    // uma chamada a vdbe_cursor_moveto().
    debug_assert!(p_c.deferred_moveto == 0);
    let p_crsr = seek_bt_cursor(&mut p_c);
    debug_assert!(btree_cursor_is_valid(Some(&*p_crsr)) != 0);

    let n: u32 = btree_payload_size(p_crsr);
    if n > db.borrow().a_limit[SQLITE_LIMIT_LENGTH as usize] as u32 {
        return CursorOpFlow::TooBig;
    }
    let mut p_out = p_out_ref.borrow_mut();
    let rc = vdbe_mem_from_btree_zero_offset(p_crsr, n, &mut p_out);
    if rc != SQLITE_OK {
        return CursorOpFlow::Abort(rc);
    }
    if p_op.p3 == 0 && deephemeralize(&mut p_out) {
        return CursorOpFlow::NoMem;
    }
    CursorOpFlow::Next
}

/// Opcode: Rowid P1 P2 * * *
/// Synopsis: r[P2]=PX rowid of P1
///
/// Guarda no registro P2 um inteiro: a chave da entrada de tabela para a qual P1 aponta.
///
/// P1 pode ser uma tabela comum ou virtual. Havia um opcode OP_VRowid separado para tabelas
/// virtuais; este opcode agora serve aos dois tipos.
pub fn op_rowid(p_op: &VdbeOp, p: &mut Vdbe) -> CursorOpFlow {
    let p_out_ref = out2_prerelease(p, p_op);
    debug_assert!(p_op.p1 >= 0 && p_op.p1 < p.n_cursor);
    let p_c_ref = p.ap_csr[p_op.p1 as usize]
        .as_ref()
        .expect("OP_Rowid sem cursor")
        .clone();
    let mut p_c = p_c_ref.borrow_mut();
    debug_assert!(p_c.e_cur_type != CURTYPE_PSEUDO || p_c.null_row != 0);
    let v: i64;
    if p_c.null_row != 0 {
        p_out_ref.borrow_mut().flags = MEM_NULL;
        return CursorOpFlow::Next;
    } else if p_c.deferred_moveto != 0 {
        v = p_c.moveto_target;
    } else if p_c.e_cur_type == CURTYPE_VTAB {
        let mut v_rowid: i64 = 0;
        let rc = match &mut p_c.uc {
            VdbeCursorCursorUnion::PVCur(p_v_cur) => {
                let p_vtab_ref = p_v_cur.p_vtab.clone().expect("cursor virtual sem tabela");
                let x_rowid = p_vtab_ref
                    .borrow()
                    .p_module
                    .x_rowid
                    .expect("módulo virtual sem xRowid");
                let rc = x_rowid(&mut **p_v_cur, &mut v_rowid);
                vtab_import_errmsg(p, &mut p_vtab_ref.borrow_mut());
                rc
            }
            _ => unreachable!("cursor CURTYPE_VTAB sem PVCur"),
        };
        if rc != SQLITE_OK {
            return CursorOpFlow::Abort(rc);
        }
        v = v_rowid;
    } else {
        debug_assert!(p_c.e_cur_type == CURTYPE_BTREE);
        let rc = vdbe_cursor_restore(&mut p_c);
        if rc != SQLITE_OK {
            return CursorOpFlow::Abort(rc);
        }
        if p_c.null_row != 0 {
            p_out_ref.borrow_mut().flags = MEM_NULL;
            return CursorOpFlow::Next;
        }
        v = btree_integer_key(seek_bt_cursor(&mut p_c));
    }
    p_out_ref.borrow_mut().u.i = v;
    CursorOpFlow::Next
}


// ---- part_015.rs ----

/// Opcode: NullRow P1 * * * *
///
/// Move o cursor P1 para uma linha nula. Qualquer OP_Column executado enquanto o cursor está na
/// linha nula sempre escreve NULL.
///
/// Se o cursor P1 ainda não foi aberto, abre-o agora como um pseudocursor especial que devolve
/// NULL em toda coluna.
pub fn op_null_row(p_op: &VdbeOp, p: &mut Vdbe) -> CursorOpFlow {
    debug_assert!(p_op.p1 >= 0 && p_op.p1 < p.n_cursor);
    let p_c_ref = match p.ap_csr[p_op.p1 as usize].clone() {
        Some(p_c_ref) => p_c_ref,
        None => {
            // Se o cursor ainda não está aberto, cria um tipo especial de pseudocursor que
            // sempre dá linhas nulas.
            let p_c_ref = match allocate_cursor(p, p_op.p1, 1, CURTYPE_PSEUDO) {
                Some(p_c_ref) => p_c_ref,
                None => return CursorOpFlow::NoMem,
            };
            {
                let mut p_c = p_c_ref.borrow_mut();
                p_c.seek_result = 0;
                p_c.is_table = 1;
                p_c.no_reuse = 1;
                p_c.uc = VdbeCursorCursorUnion::PCursor(btree_fake_valid_cursor());
            }
            p_c_ref
        }
    };
    let mut p_c = p_c_ref.borrow_mut();
    p_c.null_row = 1;
    p_c.cache_status = CACHE_STALE;
    if p_c.e_cur_type == CURTYPE_BTREE {
        btree_clear_cursor(seek_bt_cursor(&mut p_c));
    }
    CursorOpFlow::Next
}

/// Opcode: SeekEnd P1 * * * *
///
/// Posiciona o cursor P1 no fim da árvore B, para anexar uma entrada nova. Presume-se que o
/// cursor só serve para anexar; por isso, se o cursor é válido, ele já aponta para o fim da
/// árvore e nada muda nele.
///
/// Opcode: Last P1 P2 * * *
///
/// O próximo uso de Rowid, Column ou Prev em P1 se refere à última entrada da tabela ou índice.
/// Se a tabela ou índice está vazio e P2>0, salta imediatamente para P2. Se P2 é 0 ou se não
/// está vazio, segue adiante.
///
/// Este opcode deixa o cursor configurado para andar em ordem reversa, do fim para o começo:
/// o cursor usa Prev, não Next.
pub fn op_last(p_op: &VdbeOp, p: &mut Vdbe) -> CursorOpFlow {
    debug_assert!(p_op.p1 >= 0 && p_op.p1 < p.n_cursor);
    let p_c_ref = p.ap_csr[p_op.p1 as usize]
        .as_ref()
        .expect("OP_Last sem cursor")
        .clone();
    let mut p_c = p_c_ref.borrow_mut();
    debug_assert!(p_c.e_cur_type == CURTYPE_BTREE);
    let mut res: i32 = 0;
    if p_op.opcode == OP_SEEKEND {
        debug_assert!(p_op.p2 == 0);
        p_c.seek_result = -1;
        if btree_cursor_is_valid_nn(seek_bt_cursor(&mut p_c)) != 0 {
            return CursorOpFlow::Next;
        }
    }
    let rc = btree_last(seek_bt_cursor(&mut p_c), &mut res);
    p_c.null_row = res as u8;
    p_c.deferred_moveto = 0;
    p_c.cache_status = CACHE_STALE;
    if rc != SQLITE_OK {
        return CursorOpFlow::Abort(rc);
    }
    if p_op.p2 > 0 && res != 0 {
        return CursorOpFlow::JumpToP2;
    }
    CursorOpFlow::Next
}

/// Opcode: IfSizeBetween P1 P2 P3 P4 *
///
/// Seja N o número aproximado de linhas da tabela ou índice do cursor P1 e X igual a
/// 10*log2(N) se N é positivo, ou -1 se N é zero. Salta para P2 se X está entre P3 e P4,
/// inclusive.
pub fn op_if_size_between(p_op: &VdbeOp, p: &mut Vdbe) -> CursorOpFlow {
    debug_assert!(p_op.p1 >= 0 && p_op.p1 < p.n_cursor);
    debug_assert!(p_op.p4type == P4_INT32);
    debug_assert!(p_op.p3 >= -1 && p_op.p3 <= 640 * 2);
    debug_assert!(p4_int32(p_op) >= -1 && p4_int32(p_op) <= 640 * 2);
    let p_c_ref = p.ap_csr[p_op.p1 as usize]
        .as_ref()
        .expect("OP_IfSizeBetween sem cursor")
        .clone();
    let mut p_c = p_c_ref.borrow_mut();
    let mut res: i32 = 0;
    let rc = btree_first(seek_bt_cursor(&mut p_c), &mut res);
    if rc != SQLITE_OK {
        return CursorOpFlow::Abort(rc);
    }
    let sz: i64 = if res != 0 {
        -1 // codificação de -Infinito
    } else {
        let sz = btree_row_count_est(seek_bt_cursor(&mut p_c));
        debug_assert!(sz > 0);
        log_est(sz as u64) as i64
    };
    if sz >= p_op.p3 as i64 && sz <= p4_int32(p_op) as i64 {
        return CursorOpFlow::JumpToP2;
    }
    CursorOpFlow::Next
}

/// Opcode: SorterSort P1 P2 * * *
///
/// Depois que todos os registros foram inseridos no sorter identificado por P1, este opcode
/// faz a ordenação de fato. Salta para P2 se não há registros a ordenar. É um apelido de
/// OP_Sort e OP_Rewind usado para objetos Sorter.
///
/// Opcode: Sort P1 P2 * * *
///
/// Faz exatamente o mesmo que OP_Rewind, mas incrementa uma variável global não documentada
/// usada em testes. A ordenação escreve registros num índice de ordenação, rebobina-o e o
/// reproduz do começo ao fim; usa-se OP_Sort no lugar de OP_Rewind para que os testes de
/// regressão vejam se o otimizador elimina corretamente as ordenações.
///
/// Opcode: Rewind P1 P2 * * *
///
/// O próximo uso de Rowid, Column ou Next em P1 se refere à primeira entrada da tabela ou
/// índice. Se está vazio, salta imediatamente para P2; se não está, segue adiante. Se P2 é
/// zero, afirma-se que a tabela P1 nunca está vazia e o salto nunca ocorre.
///
/// Este opcode deixa o cursor configurado para andar em ordem direta, do começo para o fim:
/// o cursor usa Next, não Prev.
pub fn op_rewind(p_op: &VdbeOp, p: &mut Vdbe) -> CursorOpFlow {
    if p_op.opcode == OP_SORTERSORT || p_op.opcode == OP_SORT {
        p.a_counter[SQLITE_STMTSTATUS_SORT as usize] += 1;
        // Cai no OP_Rewind (deliberate_fall_through)
    }
    debug_assert!(p_op.p1 >= 0 && p_op.p1 < p.n_cursor);
    debug_assert!(p_op.p5 == 0);
    debug_assert!(p_op.p2 >= 0 && p_op.p2 < p.n_op);
    let p_c_ref = p.ap_csr[p_op.p1 as usize]
        .as_ref()
        .expect("OP_Rewind sem cursor")
        .clone();
    let mut p_c = p_c_ref.borrow_mut();
    let is_sorter = p_c.e_cur_type == CURTYPE_SORTER;
    debug_assert!(is_sorter == (p_op.opcode == OP_SORTERSORT));
    let mut res: i32 = 1;
    let rc;
    if is_sorter {
        rc = vdbe_sorter_rewind(&mut p_c, &mut res);
    } else {
        debug_assert!(p_c.e_cur_type == CURTYPE_BTREE);
        rc = btree_first(seek_bt_cursor(&mut p_c), &mut res);
        p_c.deferred_moveto = 0;
        p_c.cache_status = CACHE_STALE;
    }
    if rc != SQLITE_OK {
        return CursorOpFlow::Abort(rc);
    }
    p_c.null_row = res as u8;
    if p_op.p2 > 0 && res != 0 {
        return CursorOpFlow::JumpToP2;
    }
    CursorOpFlow::Next
}

/// Opcode: Next P1 P2 P3 * P5
///
/// Avança o cursor P1 para o próximo par chave/dado da tabela ou índice. Se não há mais pares,
/// segue adiante. Se o avanço deu certo, salta imediatamente para P2.
///
/// O Next só é válido depois de SeekGT, SeekGE ou OP_Rewind usados para posicionar o cursor;
/// não pode seguir SeekLT, SeekLE nem OP_Last. O cursor P1 deve ser de uma tabela real, não de
/// pseudotabela, e deve ter sido aberto antes deste opcode, senão o programa falha.
///
/// P3 é uma dica para a camada btree: se P3==1, P1 é um índice SQL e a instrução poderia ter
/// sido omitida se o índice fosse único. P3 costuma ser 0 e é sempre 0 ou 1. Se P5 é positivo
/// e o salto ocorre, o contador de eventos número P5-1 do statement é incrementado.
///
/// Opcode: Prev P1 P2 P3 * P5
///
/// Recua o cursor P1 para o par chave/dado anterior. Se não há anterior, segue adiante; se o
/// recuo deu certo, salta imediatamente para P2. Só vale depois de SeekLT, SeekLE ou OP_Last,
/// nunca depois de SeekGT, SeekGE ou OP_Rewind. P3 e P5 valem como no Next.
///
/// Opcode: SorterNext P1 P2 * * P5
///
/// Funciona como o OP_Next, mas P1 deve ser um sorter em que o OP_SorterSort já foi chamado.
/// Avança o cursor para o próximo registro ordenado, ou segue adiante se não há mais.
pub fn op_next(p_op: &VdbeOp, p: &mut Vdbe, db: &Sqlite3Ref) -> CursorOpFlow {
    debug_assert!(p_op.p1 >= 0 && p_op.p1 < p.n_cursor);
    let p_c_ref = p.ap_csr[p_op.p1 as usize]
        .as_ref()
        .expect("OP_Next sem cursor")
        .clone();
    let mut p_c = p_c_ref.borrow_mut();
    let rc = if p_op.opcode == OP_SORTERNEXT {
        debug_assert!(p_c.e_cur_type == CURTYPE_SORTER);
        vdbe_sorter_next(db, &mut p_c)
    } else {
        debug_assert!(
            p_op.p5 == 0
                || p_op.p5 as i32 == SQLITE_STMTSTATUS_FULLSCAN_STEP
                || p_op.p5 as i32 == SQLITE_STMTSTATUS_AUTOINDEX
        );
        debug_assert!(p_c.deferred_moveto == 0);
        debug_assert!(p_c.e_cur_type == CURTYPE_BTREE);
        if p_op.opcode == OP_PREV {
            btree_previous(seek_bt_cursor(&mut p_c), p_op.p3)
        } else {
            btree_next(seek_bt_cursor(&mut p_c), p_op.p3)
        }
    };
    // next_tail:
    p_c.cache_status = CACHE_STALE;
    if rc == SQLITE_OK {
        p_c.null_row = 0;
        p.a_counter[p_op.p5 as usize] += 1;
        return CursorOpFlow::JumpToP2CheckInterrupt;
    }
    if rc != SQLITE_DONE {
        return CursorOpFlow::Abort(rc);
    }
    p_c.null_row = 1;
    CursorOpFlow::CheckForInterrupt
}

/// Opcode: IdxInsert P1 P2 P3 P4 P5
/// Synopsis: key=r[P2]
///
/// O registro P2 guarda uma chave de índice SQL feita pelas instruções MakeRecord. Este opcode
/// escreve a chave no índice P1. Os dados da entrada são nulos.
///
/// Se P4 não é zero, é o número de valores da chave desempacotada de reg(P2); nesse caso P3 é
/// o índice do primeiro registro da chave desempacotada, o que às vezes é uma otimização.
///
/// Se P5 tem o bit OPFLAG_APPEND, é uma dica à camada de árvore B de que a inserção
/// provavelmente é um append. Se tem o bit OPFLAG_NCHANGE, o contador de mudanças é
/// incrementado por esta instrução. Se tem OPFLAG_USESEEKRESULT, a implementação pode evitar
/// uma busca desnecessária no cursor P1; a flag só deve ser ligada se não houve buscas antes no
/// cursor ou se a mais recente usou chave equivalente a P2.
///
/// Só funciona em índices; a instrução equivalente para tabelas é OP_Insert.
pub fn op_idx_insert(p_op: &VdbeOp, p: &mut Vdbe, a_mem: &mut [Mem]) -> CursorOpFlow {
    let p5 = p_op.p5 as u8;
    debug_assert!(p_op.p1 >= 0 && p_op.p1 < p.n_cursor);
    let p_c_ref = p.ap_csr[p_op.p1 as usize]
        .as_ref()
        .expect("OP_IdxInsert sem cursor")
        .clone();
    let mut p_c = p_c_ref.borrow_mut();
    debug_assert!(p_c.e_cur_type != CURTYPE_SORTER);
    debug_assert!((a_mem[p_op.p2 as usize].flags & MEM_BLOB) != 0 || (p5 & OPFLAG_PREFORMAT) != 0);
    if (p5 & OPFLAG_NCHANGE) != 0 {
        p.n_change += 1;
    }
    debug_assert!(p_c.e_cur_type == CURTYPE_BTREE);
    debug_assert!(p_c.is_table == 0);
    let rc = expand_blob(&mut a_mem[p_op.p2 as usize]);
    if rc != SQLITE_OK {
        return CursorOpFlow::Abort(rc);
    }
    let p_in2 = &a_mem[p_op.p2 as usize];
    let n_mem = p4_int32(p_op) as u16;
    let i_first = p_op.p3 as usize;
    let x = BtreePayload {
        p_key: Some(p_in2.z[..(p_in2.n as usize)].to_vec()),
        n_key: p_in2.n as i64,
        p_data: None,
        a_mem: if n_mem > 0 {
            Some(a_mem[i_first..i_first + (n_mem as usize)].to_vec())
        } else {
            None
        },
        n_mem,
        n_data: 0,
        n_zero: 0,
    };
    let seek_result = if (p5 & OPFLAG_USESEEKRESULT) != 0 {
        p_c.seek_result
    } else {
        0
    };
    let rc = btree_insert(
        seek_bt_cursor(&mut p_c),
        &x,
        (p5 & (OPFLAG_APPEND | OPFLAG_SAVEPOSITION | OPFLAG_PREFORMAT)) as i32,
        seek_result,
    );
    debug_assert!(p_c.deferred_moveto == 0);
    p_c.cache_status = CACHE_STALE;
    if rc != SQLITE_OK {
        return CursorOpFlow::Abort(rc);
    }
    CursorOpFlow::Next
}


// ---- part_016.rs ----

// Notas de integração para o tech lead (mesma convenção do part_004):
// - Cada `case OP_Xxx` do `switch` de `sqlite3VdbeExec` é uma `fn op_xxx`; o destino do
//   `goto`/`break` do C é o `OpFlow` devolvido. Onde o C faz `rc = X; goto
//   abort_due_to_error`, a função grava `*rc = X` e devolve `OpFlow::AbortDueToError`.
//   `goto jump_to_p2` é `OpFlow::JumpToP2`.
// - Registradores são `MemRef` em `p.a_mem`; cursores são `Option<VdbeCursorRef>` em
//   `p.ap_csr`. `uc.pCursor` é `VdbeCursorCursorUnion::PCursor(Box<BtCursor>)`.
// - Nomes assumidos do restante do porte: `corrupt_error(line)` (sqlite3CorruptError, que
//   é o que o macro SQLITE_CORRUPT_BKPT expande), `report_error(code, line, msg)`
//   (sqlite3ReportError), `writable_schema(db)`, `vdbe_incr_write_counter(p, Option<&VdbeCursor>)`,
//   `vdbe_mem_from_btree_zero_offset`, `vdbe_record_compare_with_skip`,
//   `vdbe_idx_rowid(db, &mut BtCursor, &mut i64)`, `P4Value::IntArray(Vec<u32>)` e
//   `VdbeCursor.ub = CursorUb::AAltMap(Option<Vec<u32>>)` (o `aAltMap` do C).
// - Sem SQLITE_DEBUG: `memIsValid`, `REGISTER_TRACE` e `VdbeBranchTaken` somem.

/// Opcode: SorterInsert P1 P2 * * *
/// Synopsis: key=r[P2]
///
/// O registro P2 guarda uma chave de índice SQL feita pelas instruções MakeRecord.
/// Este opcode grava essa chave no ordenador P1. Os dados da entrada são nulos.
pub fn op_sorter_insert(p: &mut Vdbe, p_op: &Op, rc: &mut i32) -> OpFlow {
    debug_assert!(p_op.p1 >= 0 && p_op.p1 < p.n_cursor as i32);
    let p_c = p.ap_csr[p_op.p1 as usize]
        .clone()
        .expect("OP_SorterInsert: cursor ausente");
    vdbe_incr_write_counter(p, Some(&p_c.borrow()));
    debug_assert!(is_sorter(&p_c.borrow()));
    let p_in2 = p.a_mem[p_op.p2 as usize].clone();
    debug_assert!((p_in2.borrow().flags & MEM_BLOB) != 0);
    debug_assert!(p_c.borrow().is_table == 0);
    if (p_in2.borrow().flags & MEM_ZERO) != 0 {
        *rc = vdbe_mem_expand_blob(&mut p_in2.borrow_mut());
        if *rc != SQLITE_OK {
            return OpFlow::AbortDueToError;
        }
    }
    *rc = vdbe_sorter_write(&mut p_c.borrow_mut(), &p_in2.borrow());
    if *rc != SQLITE_OK {
        return OpFlow::AbortDueToError;
    }
    OpFlow::Next
}

/// Opcode: IdxDelete P1 P2 P3 * P5
/// Synopsis: key=r[P2@P3]
///
/// O conteúdo dos P3 registros a partir do registro P2 forma uma chave de índice
/// desempacotada. Este opcode remove essa entrada do índice aberto pelo cursor P1.
///
/// Se P5 não for zero, levanta SQLITE_CORRUPT_INDEX caso nenhuma entrada de índice
/// correspondente seja encontrada. Isso acontece ao executar um UPDATE ou DELETE
/// quando a entrada de índice a atualizar ou apagar não existe. Para alguns usos de
/// IdxDelete (exemplo: o operador EXCEPT) não importa que nenhuma entrada seja
/// encontrada; nesses casos P5 é zero. Também não levanta esse erro (autocorretivo e
/// não crítico) no modo writable_schema.
pub fn op_idx_delete(p: &mut Vdbe, db: &sqlite3, p_op: &Op, rc: &mut i32) -> OpFlow {
    debug_assert!(p_op.p3 > 0);
    debug_assert!(
        p_op.p2 > 0 && p_op.p2 + p_op.p3 <= (p.n_mem + 1 - p.n_cursor as i32) + 1
    );
    debug_assert!(p_op.p1 >= 0 && p_op.p1 < p.n_cursor as i32);
    let p_c = p.ap_csr[p_op.p1 as usize]
        .clone()
        .expect("OP_IdxDelete: cursor ausente");
    debug_assert!(p_c.borrow().e_cur_type == CURTYPE_BTREE);
    vdbe_incr_write_counter(p, Some(&p_c.borrow()));
    let first = p_op.p2 as usize;
    let r = UnpackedRecord {
        p_key_info: p_c.borrow().p_key_info.clone(),
        n_field: p_op.p3 as u16,
        default_rc: 0,
        a_mem: p.a_mem[first..first + p_op.p3 as usize].to_vec(),
        ..Default::default()
    };
    let mut res: i32 = 0;
    {
        let mut c = p_c.borrow_mut();
        let p_crsr = match &mut c.uc {
            VdbeCursorCursorUnion::PCursor(b) => b,
            _ => unreachable!("OP_IdxDelete: cursor sem BtCursor"),
        };
        *rc = btree_index_moveto(p_crsr, &r, &mut res);
        if *rc != SQLITE_OK {
            return OpFlow::AbortDueToError;
        }
        if res == 0 {
            *rc = btree_delete(p_crsr, BTREE_AUXDELETE);
            if *rc != SQLITE_OK {
                return OpFlow::AbortDueToError;
            }
        } else if p_op.p5 != 0 && !writable_schema(db) {
            *rc = report_error(SQLITE_CORRUPT_INDEX, line!() as i32, "index corruption");
            return OpFlow::AbortDueToError;
        }
    }
    let mut c = p_c.borrow_mut();
    debug_assert!(c.deferred_moveto == 0);
    c.cache_status = CACHE_STALE;
    c.seek_result = 0;
    OpFlow::Next
}

/// Opcode: DeferredSeek P1 * P3 P4 *
/// Synopsis: Move P3 to P1.rowid if needed
///
/// P1 é um cursor de índice aberto e P3 é um cursor na tabela correspondente. Este
/// opcode faz uma busca adiada do cursor de tabela P3 até a linha que corresponde à
/// linha atual de P1.
///
/// É uma busca adiada. Nada acontece de fato até o cursor ser usado para ler um
/// registro. Assim, se nenhuma leitura ocorrer, nenhuma E/S desnecessária é feita.
///
/// P4 pode ser um vetor de inteiros (tipo P4_INTARRAY) com uma entrada para cada
/// coluna da tabela P3. Se a entrada a(i) for diferente de zero, ler a coluna a(i)-1
/// do cursor P3 equivale a fazer a busca adiada e depois ler a coluna i de P1. Essa
/// informação é guardada em P3 e usada para redirecionar as leituras de P3 para P1,
/// podendo evitar a busca e a leitura do cursor P3.
///
/// Opcode: IdxRowid P1 P2 * * *
/// Synopsis: r[P2]=rowid
///
/// Escreve no registro P2 um inteiro que é a última entrada do registro no fim da
/// chave de índice apontada pelo cursor P1. Esse inteiro deve ser o rowid da entrada
/// da tabela para a qual esta entrada de índice aponta.
///
/// Ver também: Rowid, MakeRecord.
pub fn op_deferred_seek_or_idx_rowid(
    p: &mut Vdbe,
    db: &sqlite3,
    p_op: &Op,
    rc: &mut i32,
) -> OpFlow {
    debug_assert!(p_op.p1 >= 0 && p_op.p1 < p.n_cursor as i32);
    let p_c = p.ap_csr[p_op.p1 as usize]
        .clone()
        .expect("OP_DeferredSeek/IdxRowid: cursor ausente");
    debug_assert!(p_c.borrow().e_cur_type == CURTYPE_BTREE || is_null_cursor(&p_c.borrow()));
    debug_assert!(p_c.borrow().is_table == 0 || is_null_cursor(&p_c.borrow()));
    debug_assert!(p_c.borrow().deferred_moveto == 0);
    debug_assert!(p_c.borrow().null_row == 0 || p_op.opcode == OP_IDXROWID);

    // Os opcodes IdxRowid e Seek são combinados pela semelhança entre
    // sqlite3VdbeCursorRestore() e sqlite3VdbeIdxRowid().
    *rc = vdbe_cursor_restore(&mut p_c.borrow_mut());

    // sqlite3VdbeCursorRestore() pode falhar se o cursor foi perturbado desde a última
    // posição e ocorre erro (OOM ou E/S) ao reposicioná-lo.
    if *rc != SQLITE_OK {
        return OpFlow::AbortDueToError;
    }

    if p_c.borrow().null_row == 0 {
        let mut rowid: i64 = 0;
        {
            let mut c = p_c.borrow_mut();
            let p_cur = match &mut c.uc {
                VdbeCursorCursorUnion::PCursor(b) => b,
                _ => unreachable!("OP_DeferredSeek/IdxRowid: cursor sem BtCursor"),
            };
            *rc = vdbe_idx_rowid(db, p_cur, &mut rowid);
        }
        if *rc != SQLITE_OK {
            return OpFlow::AbortDueToError;
        }
        if p_op.opcode == OP_DEFERREDSEEK {
            debug_assert!(p_op.p3 >= 0 && p_op.p3 < p.n_cursor as i32);
            let p_tab_cur = p.ap_csr[p_op.p3 as usize]
                .clone()
                .expect("OP_DeferredSeek: cursor de tabela ausente");
            let mut t = p_tab_cur.borrow_mut();
            debug_assert!(t.e_cur_type == CURTYPE_BTREE);
            debug_assert!(t.is_table != 0);
            t.null_row = 0;
            t.moveto_target = rowid;
            t.deferred_moveto = 1;
            t.cache_status = CACHE_STALE;
            debug_assert!(
                p_op.p4type == P4_INTARRAY || matches!(p_op.p4, P4Value::NotUsed)
            );
            debug_assert!(t.is_ephemeral == 0);
            t.ub = CursorUb::AAltMap(match &p_op.p4 {
                P4Value::IntArray(ai) => Some(ai.clone()),
                _ => None,
            });
            debug_assert!(p_c.borrow().is_ephemeral == 0);
            t.p_alt_cursor = Some(p_c.clone());
        } else {
            let p_out = out2_prerelease(p, p_op);
            p_out.borrow_mut().u.i = rowid;
        }
    } else {
        debug_assert!(p_op.opcode == OP_IDXROWID);
        vdbe_mem_set_null(&mut p.a_mem[p_op.p2 as usize].borrow_mut());
    }
    OpFlow::Next
}

/// Opcode: FinishSeek P1 * * * *
///
/// Se o cursor P1 foi movido antes por OP_DeferredSeek, completa essa busca agora,
/// sem mais demora. Se a busca já ocorreu, esta instrução não faz nada.
pub fn op_finish_seek(p: &mut Vdbe, p_op: &Op, rc: &mut i32) -> OpFlow {
    debug_assert!(p_op.p1 >= 0 && p_op.p1 < p.n_cursor as i32);
    let p_c = p.ap_csr[p_op.p1 as usize]
        .clone()
        .expect("OP_FinishSeek: cursor ausente");
    if p_c.borrow().deferred_moveto != 0 {
        *rc = vdbe_finish_moveto(&mut p_c.borrow_mut());
        if *rc != SQLITE_OK {
            return OpFlow::AbortDueToError;
        }
    }
    OpFlow::Next
}

/// Opcode: IdxGE P1 P2 P3 P4 *
/// Synopsis: key=r[P3@P4]
///
/// Os P4 valores de registro a partir de P3 formam uma chave de índice desempacotada
/// que omite a PRIMARY KEY. Compara esse valor de chave com o índice para o qual P1
/// aponta, ignorando os campos PRIMARY KEY ou ROWID no fim.
///
/// Se a entrada de índice de P1 for maior ou igual ao valor da chave, salta para P2.
/// Caso contrário segue para a próxima instrução.
///
/// Opcode: IdxGT P1 P2 P3 P4 *
/// Synopsis: key=r[P3@P4]
///
/// Igual ao IdxGE, mas salta para P2 se a entrada de índice de P1 for estritamente
/// maior que o valor da chave.
///
/// Opcode: IdxLT P1 P2 P3 P4 *
/// Synopsis: key=r[P3@P4]
///
/// Os P4 valores de registro a partir de P3 formam uma chave de índice desempacotada
/// que omite a PRIMARY KEY ou o ROWID. Compara essa chave com o índice para o qual P1
/// aponta, ignorando a PRIMARY KEY ou o ROWID do índice P1.
///
/// Se a entrada de índice de P1 for menor que o valor da chave, salta para P2.
/// Caso contrário segue para a próxima instrução.
///
/// Opcode: IdxLE P1 P2 P3 P4 *
/// Synopsis: key=r[P3@P4]
///
/// Igual ao IdxLT, mas salta para P2 se a entrada de índice de P1 for menor ou igual
/// ao valor da chave.
pub fn op_idx_le_gt_lt_ge(p: &mut Vdbe, db: &sqlite3, p_op: &Op, rc: &mut i32) -> OpFlow {
    debug_assert!(p_op.p1 >= 0 && p_op.p1 < p.n_cursor as i32);
    let p_c = p.ap_csr[p_op.p1 as usize]
        .clone()
        .expect("OP_IdxXX: cursor ausente");
    debug_assert!(p_c.borrow().is_ordered != 0);
    debug_assert!(p_c.borrow().e_cur_type == CURTYPE_BTREE);
    debug_assert!(p_c.borrow().deferred_moveto == 0);
    debug_assert!(p_op.p4type == P4_INT32);
    let n_field = match p_op.p4 {
        P4Value::Int32(i) => i,
        _ => 0,
    };
    let default_rc: i8 = if p_op.opcode < OP_IDXLT {
        debug_assert!(p_op.opcode == OP_IDXLE || p_op.opcode == OP_IDXGT);
        -1
    } else {
        debug_assert!(p_op.opcode == OP_IDXGE || p_op.opcode == OP_IDXLT);
        0
    };
    let first = p_op.p3 as usize;
    let r = UnpackedRecord {
        p_key_info: p_c.borrow().p_key_info.clone(),
        n_field: n_field as u16,
        default_rc,
        a_mem: p.a_mem[first..first + n_field as usize].to_vec(),
        ..Default::default()
    };

    // Versão embutida de sqlite3VdbeIdxKeyCompare()
    let mut res: i32;
    {
        let mut c = p_c.borrow_mut();
        debug_assert!(c.e_cur_type == CURTYPE_BTREE);
        let p_cur = match &mut c.uc {
            VdbeCursorCursorUnion::PCursor(b) => b,
            _ => unreachable!("OP_IdxXX: cursor sem BtCursor"),
        };
        debug_assert!(btree_cursor_is_valid(p_cur) != 0);
        let n_cell_key: i64 = btree_payload_size(p_cur) as i64;
        // nCellKey fica sempre entre 0 e 0xffffffff pela forma como btreeParseCellPtr()
        // e sqlite3GetVarint32() são implementados.
        if n_cell_key <= 0 || n_cell_key > 0x7fffffff {
            *rc = corrupt_error(line!() as i32);
            return OpFlow::AbortDueToError;
        }
        let mut m = Mem::default();
        vdbe_mem_init(&mut m, db, 0);
        *rc = vdbe_mem_from_btree_zero_offset(p_cur, n_cell_key as u32, &mut m);
        if *rc != SQLITE_OK {
            return OpFlow::AbortDueToError;
        }
        res = vdbe_record_compare_with_skip(m.n, &m.z, &r, 0);
        vdbe_mem_release_malloc(&mut m);
    }
    // Fim da versão embutida de sqlite3VdbeIdxKeyCompare()

    debug_assert!((OP_IDXLE & 1) == (OP_IDXLT & 1) && (OP_IDXGE & 1) == (OP_IDXGT & 1));
    if (p_op.opcode & 1) == (OP_IDXLT & 1) {
        debug_assert!(p_op.opcode == OP_IDXLE || p_op.opcode == OP_IDXLT);
        res = -res;
    } else {
        debug_assert!(p_op.opcode == OP_IDXGE || p_op.opcode == OP_IDXGT);
        res += 1;
    }
    debug_assert!(*rc == SQLITE_OK);
    if res > 0 {
        return OpFlow::JumpToP2;
    }
    OpFlow::Next
}

/// Opcode: Destroy P1 P2 P3 * *
///
/// Apaga uma tabela ou índice inteiro de banco de dados, cuja página raiz no arquivo
/// é dada por P1.
///
/// A tabela destruída fica no arquivo principal se P3==0. Se P3==1, ela fica no
/// arquivo auxiliar usado para guardar as tabelas criadas com CREATE TEMPORARY TABLE.
///
/// Se AUTOVACUUM estiver habilitado, outra página raiz pode ser movida para a página
/// raiz recém-apagada para manter todas as raízes contíguas no começo do arquivo. O
/// valor anterior da raiz que foi movida (antes da mudança) é guardado no registro
/// P2. Se nenhuma página precisou se mover (porque a tabela apagada já era a última do
/// banco), grava-se zero no registro P2. Com AUTOVACUUM desabilitado, grava-se zero.
///
/// Este opcode levanta erro se houver algum VM leitor ativo quando for chamado. Isso
/// evita a dificuldade de atualizar cursores existentes quando uma raiz é movida num
/// banco AUTOVACUUM. O erro é levantado mesmo que o banco não seja AUTOVACUUM, para
/// não criar incompatibilidade entre os modos com e sem autovacuum.
///
/// Ver também: Clear
pub fn op_destroy(
    p: &mut Vdbe,
    db: &mut sqlite3,
    p_op: &Op,
    rc: &mut i32,
    reset_schema_on_fault: &mut i32,
) -> OpFlow {
    vdbe_incr_write_counter(p, None);
    debug_assert!(p.read_only == 0);
    debug_assert!(p_op.p1 > 1);
    let p_out = out2_prerelease(p, p_op);
    p_out.borrow_mut().flags = MEM_NULL;
    if db.n_vdbe_read > db.n_v_destroy + 1 {
        *rc = SQLITE_LOCKED;
        p.error_action = OE_ABORT;
        return OpFlow::AbortDueToError;
    }
    let i_db = p_op.p3;
    debug_assert!(db_mask_test(p.btree_mask, i_db));
    let mut i_moved: u32 = 0; // Só para calar aviso do compilador
    let p_bt = db.a_db[i_db as usize]
        .p_bt
        .clone()
        .expect("OP_Destroy: Btree ausente");
    *rc = btree_drop_table(&p_bt, p_op.p1 as u32, &mut i_moved);
    {
        let mut out = p_out.borrow_mut();
        out.flags = MEM_INT;
        out.u.i = i_moved as i64;
    }
    if *rc != SQLITE_OK {
        return OpFlow::AbortDueToError;
    }
    if i_moved != 0 {
        root_page_moved(db, i_db, i_moved, p_op.p1 as u32);
        // Todas as operações OP_Destroy ocorrem no mesmo btree
        debug_assert!(*reset_schema_on_fault == 0 || *reset_schema_on_fault == i_db + 1);
        *reset_schema_on_fault = i_db + 1;
    }
    OpFlow::Next
}


// ---- part_017.rs ----

// Notas de integração para o tech lead (mesma convenção do part_004 e do part_016):
// - `fn op_xxx(...) -> OpFlow`: `Next` é o `break`; `AbortDueToError` é `goto
//   abort_due_to_error` com o código em `*rc`; `NoMem` é `goto no_mem`;
//   `CheckForInterrupt` é `goto check_for_interrupt` (o laço confere a interrupção e
//   segue para a próxima instrução).
// - Nomes assumidos: `vdbe_error(p, msg: &[u8])` recebe a mensagem já formatada
//   (o `"%s"` do C sobre `NULL` vira vazio); `m_printf(Option<&sqlite3>, fmt, &[PrintfArg])`
//   (sqlite3MPrintf); `exec(db, sql, callback, &mut Option<Vec<u8>>)` (sqlite3_exec),
//   onde o callback é `Option<&mut dyn FnMut(&mut sqlite3, &[Option<Vec<u8>>], &[Vec<u8>]) -> i32>`
//   (recebe o `db` para evitar o aliasing de `initData.db` do C); `init_callback(db,
//   &mut InitData, az_obj, az_col)` (sqlite3InitCallback); `InitData` sem o campo `db`
//   e com `z_err_msg: Option<Vec<u8>>` no lugar de `pzErrMsg` (o conteúdo é movido para
//   `p.z_err_msg` ao final, só se este ainda estiver vazio, que é o que o C faz);
//   `mem_row_set_mut(&mut Mem) -> &mut RowSet` (o `(RowSet*)pIn1->z` do C);
//   `P4Value::IntArray(Vec<u32>)` com `a_root[0] == n_root`.
// - Sem SQLITE_DEBUG: `memIsValid`, `memAboutToChange` e `UPDATE_MAX_BLOBSIZE` somem
//   (este último só é definido com SQLITE_DEBUG ou SQLITE_ENABLE_...; aqui não há efeito).

/// Opcode: Clear P1 P2 P3
///
/// Apaga todo o conteúdo da tabela ou índice de banco de dados cuja página raiz no
/// arquivo é dada por P1. Mas, diferente de Destroy, não remove a tabela ou o índice
/// do arquivo.
///
/// A tabela limpa fica no arquivo principal se P2==0. Se P2==1, ela fica no arquivo
/// auxiliar usado para guardar as tabelas criadas com CREATE TEMPORARY TABLE.
///
/// Se P3 for diferente de zero, o contador de linhas alteradas é incrementado pelo
/// número de linhas da tabela limpa. Se P3 for maior que zero, o valor guardado no
/// registro P3 também é incrementado pelo número de linhas da tabela limpa.
///
/// Ver também: Destroy
pub fn op_clear(p: &mut Vdbe, db: &sqlite3, p_op: &Op, rc: &mut i32) -> OpFlow {
    vdbe_incr_write_counter(p, None);
    let mut n_change: i64 = 0;
    debug_assert!(p.read_only == 0);
    debug_assert!(db_mask_test(p.btree_mask, p_op.p2));
    let p_bt = db.a_db[p_op.p2 as usize]
        .p_bt
        .clone()
        .expect("OP_Clear: Btree ausente");
    *rc = btree_clear_table(&p_bt, p_op.p1 as u32, &mut n_change);
    if p_op.p3 != 0 {
        p.n_change += n_change;
        if p_op.p3 > 0 {
            p.a_mem[p_op.p3 as usize].borrow_mut().u.i += n_change;
        }
    }
    if *rc != SQLITE_OK {
        return OpFlow::AbortDueToError;
    }
    OpFlow::Next
}

/// Opcode: ResetSorter P1 * * * *
///
/// Apaga todo o conteúdo da tabela efêmera ou do ordenador aberto no cursor P1.
///
/// Este opcode só funciona para cursores usados em ordenação e abertos com
/// OP_OpenEphemeral ou OP_SorterOpen.
pub fn op_reset_sorter(p: &mut Vdbe, db: &mut sqlite3, p_op: &Op, rc: &mut i32) -> OpFlow {
    debug_assert!(p_op.p1 >= 0 && p_op.p1 < p.n_cursor as i32);
    let p_c = p.ap_csr[p_op.p1 as usize]
        .clone()
        .expect("OP_ResetSorter: cursor ausente");
    let mut c = p_c.borrow_mut();
    if is_sorter(&c) {
        match &mut c.uc {
            VdbeCursorCursorUnion::PSorter(p_sorter) => vdbe_sorter_reset(db, p_sorter),
            _ => unreachable!("OP_ResetSorter: cursor sem ordenador"),
        }
    } else {
        debug_assert!(c.e_cur_type == CURTYPE_BTREE);
        debug_assert!(c.is_ephemeral != 0);
        match &mut c.uc {
            VdbeCursorCursorUnion::PCursor(p_cursor) => {
                *rc = btree_clear_table_of_cursor(p_cursor);
            }
            _ => unreachable!("OP_ResetSorter: cursor sem BtCursor"),
        }
        if *rc != SQLITE_OK {
            return OpFlow::AbortDueToError;
        }
    }
    OpFlow::Next
}

/// Opcode: CreateBtree P1 P2 P3 * *
/// Synopsis: r[P2]=root iDb=P1 flags=P3
///
/// Aloca uma nova b-tree no arquivo principal do banco se P1==0, no arquivo TEMP se
/// P1==1, ou num banco anexado se P1>1. O argumento P3 deve ser 1 (BTREE_INTKEY) para
/// uma tabela com rowid e 2 (BTREE_BLOBKEY) para um índice ou tabela WITHOUT ROWID.
/// O número da página raiz da nova b-tree é guardado no registro P2.
pub fn op_create_btree(p: &mut Vdbe, db: &sqlite3, p_op: &Op, rc: &mut i32) -> OpFlow {
    vdbe_incr_write_counter(p, None);
    let p_out = out2_prerelease(p, p_op);
    let mut pgno: u32 = 0;
    debug_assert!(p_op.p3 == BTREE_INTKEY || p_op.p3 == BTREE_BLOBKEY);
    debug_assert!(p_op.p1 >= 0 && p_op.p1 < db.n_db);
    debug_assert!(db_mask_test(p.btree_mask, p_op.p1));
    debug_assert!(p.read_only == 0);
    let p_bt = db.a_db[p_op.p1 as usize]
        .p_bt
        .clone()
        .expect("OP_CreateBtree: Btree ausente");
    *rc = btree_create_table(&p_bt, &mut pgno, p_op.p3);
    if *rc != SQLITE_OK {
        return OpFlow::AbortDueToError;
    }
    p_out.borrow_mut().u.i = pgno as i64;
    OpFlow::Next
}

/// Opcode: SqlExec P1 P2 * P4 *
///
/// Executa a instrução ou instruções SQL dadas na string P4.
///
/// O parâmetro P1 é uma máscara de bits de opções:
///
///    0x0001     Desabilita os callbacks de Auth e Trace enquanto as instruções de
///               P4 executam.
///
///    0x0002     Define db->nAnalysisLimit como P2 enquanto as instruções de P4
///               executam.
pub fn op_sql_exec(p: &mut Vdbe, db: &mut sqlite3, p_op: &Op, rc: &mut i32) -> OpFlow {
    vdbe_incr_write_counter(p, None);
    db.n_sql_exec += 1;
    let mut z_err: Option<Vec<u8>> = None;
    let x_auth = db.x_auth.clone();
    let m_trace = db.m_trace;
    let saved_analysis_limit = db.n_analysis_limit;
    if (p_op.p1 & 0x0001) != 0 {
        db.x_auth = None;
        db.m_trace = 0;
    }
    if (p_op.p1 & 0x0002) != 0 {
        db.n_analysis_limit = p_op.p2;
    }
    let z_sql: Vec<u8> = match &p_op.p4 {
        P4Value::Static(z) | P4Value::Dynamic(z) => z.clone(),
        _ => Vec::new(),
    };
    *rc = exec(db, &z_sql, None, &mut z_err);
    db.n_sql_exec -= 1;
    db.x_auth = x_auth;
    db.m_trace = m_trace;
    db.n_analysis_limit = saved_analysis_limit;
    if z_err.is_some() || *rc != SQLITE_OK {
        vdbe_error(p, z_err.as_deref().unwrap_or(b""));
        if *rc == SQLITE_NOMEM {
            return OpFlow::NoMem;
        }
        return OpFlow::AbortDueToError;
    }
    OpFlow::Next
}

/// Opcode: ParseSchema P1 * * P4 *
///
/// Lê e analisa todas as entradas da tabela de esquema do banco P1 que casam com a
/// cláusula WHERE P4. Se P4 for um ponteiro nulo, o esquema inteiro de P1 é
/// reanalisado.
///
/// Este opcode chama o analisador sintático para criar uma nova máquina virtual e
/// depois executa essa nova máquina. É, portanto, um opcode reentrante.
pub fn op_parse_schema(p: &mut Vdbe, db: &mut sqlite3, p_op: &Op, rc: &mut i32) -> OpFlow {
    // Toda instrução preparada que invoca este opcode segura mutexes em todos os
    // btrees. Isso é pré-requisito para invocar sqlite3InitCallback().
    let i_db = p_op.p1;
    debug_assert!(i_db >= 0 && i_db < db.n_db);
    debug_assert!(
        db_has_property(db, i_db, DB_SCHEMALOADED)
            || db.malloc_failed != 0
            || (corrupt_db(db) && (db.flags & SQLITE_NOSCHEMAERROR) != 0)
    );

    let p4_z: Option<&Vec<u8>> = match &p_op.p4 {
        P4Value::Static(z) | P4Value::Dynamic(z) => Some(z),
        _ => None,
    };
    if p4_z.is_none() {
        schema_clear(db.a_db[i_db as usize].p_schema.as_ref().expect("OP_ParseSchema: esquema ausente"));
        db.m_db_flags &= !DBFLAG_SCHEMAKNOWNOK;
        *rc = init_one(db, i_db, &mut p.z_err_msg, p_op.p5 as u32);
        db.m_db_flags |= DBFLAG_SCHEMACHANGE;
        p.expired = 0;
    } else {
        let z_p4 = p4_z.unwrap();
        let z_schema: &[u8] = LEGACY_SCHEMA_TABLE;
        let mut init_data = InitData {
            i_db,
            m_init_flags: 0,
            mx_page: btree_last_page(
                db.a_db[i_db as usize]
                    .p_bt
                    .as_ref()
                    .expect("OP_ParseSchema: Btree ausente"),
            ),
            rc: SQLITE_OK,
            n_init_row: 0,
            z_err_msg: None,
            ..Default::default()
        };
        let z_sql = m_printf(
            Some(&*db),
            b"SELECT*FROM\"%w\".%s WHERE %s ORDER BY rowid",
            &[
                PrintfArg::Bytes(db.a_db[i_db as usize].z_db_s_name.clone()),
                PrintfArg::Bytes(z_schema.to_vec()),
                PrintfArg::Bytes(z_p4.clone()),
            ],
        );
        match z_sql {
            None => {
                *rc = SQLITE_NOMEM_BKPT;
            }
            Some(z_sql) => {
                debug_assert!(db.init.busy == 0);
                db.init.busy = 1;
                init_data.rc = SQLITE_OK;
                init_data.n_init_row = 0;
                debug_assert!(db.malloc_failed == 0);
                let mut cb = |d: &mut sqlite3, az_obj: &[Option<Vec<u8>>], az_col: &[Vec<u8>]| {
                    init_callback(d, &mut init_data, az_obj, az_col)
                };
                *rc = exec(db, &z_sql, Some(&mut cb), &mut None);
                if *rc == SQLITE_OK {
                    *rc = init_data.rc;
                }
                if *rc == SQLITE_OK && init_data.n_init_row == 0 {
                    // O opcode ParseSchema com argumento P4 não nulo deve analisar ao
                    // menos uma instrução SQL. Menos que isso indica que a tabela
                    // sqlite_schema está corrompida.
                    *rc = corrupt_error(line!() as i32);
                }
                db.init.busy = 0;
                if p.z_err_msg.is_none() {
                    p.z_err_msg = init_data.z_err_msg.take();
                }
            }
        }
    }
    if *rc != SQLITE_OK {
        reset_all_schemas_of_connection(db);
        if *rc == SQLITE_NOMEM {
            return OpFlow::NoMem;
        }
        return OpFlow::AbortDueToError;
    }
    OpFlow::Next
}

/// Opcode: LoadAnalysis P1 * * * *
///
/// Lê a tabela sqlite_stat1 do banco P1 e carrega o conteúdo dela na tabela hash
/// interna de índices. Isso faz a análise ser usada ao preparar todas as consultas
/// seguintes.
pub fn op_load_analysis(db: &mut sqlite3, p_op: &Op, rc: &mut i32) -> OpFlow {
    debug_assert!(p_op.p1 >= 0 && p_op.p1 < db.n_db);
    *rc = analysis_load(db, p_op.p1);
    if *rc != SQLITE_OK {
        return OpFlow::AbortDueToError;
    }
    OpFlow::Next
}

/// Opcode: DropTable P1 * * P4 *
///
/// Remove as estruturas de dados internas (em memória) que descrevem a tabela de nome
/// P4 no banco P1. É chamado depois que uma tabela é apagada do disco (com o opcode
/// Destroy), para manter a representação interna do esquema consistente com o disco.
pub fn op_drop_table(p: &mut Vdbe, db: &mut sqlite3, p_op: &Op) -> OpFlow {
    vdbe_incr_write_counter(p, None);
    if let P4Value::Static(z) | P4Value::Dynamic(z) = &p_op.p4 {
        unlink_and_delete_table(db, p_op.p1, z);
    }
    OpFlow::Next
}

/// Opcode: DropIndex P1 * * P4 *
///
/// Remove as estruturas de dados internas (em memória) que descrevem o índice de nome
/// P4 no banco P1. É chamado depois que um índice é apagado do disco (com o opcode
/// Destroy), para manter a representação interna do esquema consistente com o disco.
pub fn op_drop_index(p: &mut Vdbe, db: &mut sqlite3, p_op: &Op) -> OpFlow {
    vdbe_incr_write_counter(p, None);
    if let P4Value::Static(z) | P4Value::Dynamic(z) = &p_op.p4 {
        unlink_and_delete_index(db, p_op.p1, z);
    }
    OpFlow::Next
}

/// Opcode: DropTrigger P1 * * P4 *
///
/// Remove as estruturas de dados internas (em memória) que descrevem o gatilho de nome
/// P4 no banco P1. É chamado depois que um gatilho é apagado do disco (com o opcode
/// Destroy), para manter a representação interna do esquema consistente com o disco.
pub fn op_drop_trigger(p: &mut Vdbe, db: &mut sqlite3, p_op: &Op) -> OpFlow {
    vdbe_incr_write_counter(p, None);
    if let P4Value::Static(z) | P4Value::Dynamic(z) = &p_op.p4 {
        unlink_and_delete_trigger(db, p_op.p1, z);
    }
    OpFlow::Next
}

/// Opcode: IntegrityCk P1 P2 P3 P4 P5
///
/// Faz uma análise do banco de dados atualmente aberto. Guarda no registro (P1+1) o
/// texto de uma mensagem de erro que descreve os problemas. Se nenhum problema for
/// encontrado, guarda NULL em (P1+1).
///
/// O registro (P1) contém um a menos que o número máximo de erros permitido. No máximo
/// reg(P1) erros são relatados. Em outras palavras, a análise para assim que reg(P1)
/// erros forem vistos. Reg(P1) é atualizado com o número de erros restantes.
///
/// Os números de página raiz de todas as tabelas do banco são inteiros guardados no
/// argumento P4_INTARRAY.
///
/// Se P5 for diferente de zero, a verificação é feita no arquivo auxiliar, não no
/// arquivo principal.
///
/// Este opcode é usado para implementar o pragma integrity_check.
pub fn op_integrity_ck(
    p: &mut Vdbe,
    db: &mut sqlite3,
    p_op: &Op,
    rc: &mut i32,
    encoding: u8,
) -> OpFlow {
    debug_assert!(p.b_is_reader != 0);
    debug_assert!(p_op.p4type == P4_INTARRAY);
    let n_root = p_op.p2; // Número de tabelas a verificar (número de páginas raiz)
    let a_root: &[u32] = match &p_op.p4 {
        P4Value::IntArray(ai) => ai,
        _ => unreachable!("OP_IntegrityCk: P4 não é INTARRAY"),
    };
    debug_assert!(n_root > 0);
    debug_assert!(!a_root.is_empty());
    debug_assert!(a_root[0] == n_root as u32);
    debug_assert!(p_op.p1 > 0 && (p_op.p1 + 1) <= (p.n_mem + 1 - p.n_cursor as i32));
    let pn_err = p.a_mem[p_op.p1 as usize].clone(); // Registro com os erros restantes
    debug_assert!((pn_err.borrow().flags & MEM_INT) != 0);
    debug_assert!((pn_err.borrow().flags & (MEM_STR | MEM_BLOB)) == 0);
    let p_in1 = p.a_mem[(p_op.p1 + 1) as usize].clone();
    debug_assert!(p_op.p5 < db.n_db);
    debug_assert!(db_mask_test(p.btree_mask, p_op.p5));
    let mut n_err: i32 = 0; // Número de erros relatados
    let mut z: Option<Vec<u8>> = None; // Texto do relatório de erros
    let p_bt = db.a_db[p_op.p5 as usize]
        .p_bt
        .clone()
        .expect("OP_IntegrityCk: Btree ausente");
    let first = p_op.p3 as usize;
    let max_err = (pn_err.borrow().u.i + 1) as i32;
    *rc = btree_integrity_check(
        db,
        &p_bt,
        &a_root[1..],
        &p.a_mem[first..first + n_root as usize],
        n_root,
        max_err,
        &mut n_err,
        &mut z,
    );
    vdbe_mem_set_null(&mut p_in1.borrow_mut());
    if n_err == 0 {
        debug_assert!(z.is_none());
    } else if *rc != SQLITE_OK {
        return OpFlow::AbortDueToError;
    } else {
        pn_err.borrow_mut().u.i -= (n_err - 1) as i64;
        vdbe_mem_set_str(
            &mut p_in1.borrow_mut(),
            z.take().unwrap_or_default(),
            -1,
            SQLITE_UTF8,
        );
    }
    vdbe_change_encoding(&mut p_in1.borrow_mut(), encoding);
    OpFlow::CheckForInterrupt
}

/// Opcode: RowSetAdd P1 P2 * * *
/// Synopsis: rowset(P1)=r[P2]
///
/// Insere o valor inteiro guardado no registro P2 num objeto RowSet guardado no
/// registro P1.
///
/// Uma asserção falha se P2 não for um inteiro.
pub fn op_row_set_add(p: &mut Vdbe, p_op: &Op) -> OpFlow {
    let p_in1 = p.a_mem[p_op.p1 as usize].clone();
    let p_in2 = p.a_mem[p_op.p2 as usize].clone();
    debug_assert!((p_in2.borrow().flags & MEM_INT) != 0);
    if (p_in1.borrow().flags & MEM_BLOB) == 0 {
        if vdbe_mem_set_row_set(&mut p_in1.borrow_mut()) != SQLITE_OK {
            return OpFlow::NoMem;
        }
    }
    debug_assert!(vdbe_mem_is_row_set(&p_in1.borrow()));
    let v = p_in2.borrow().u.i;
    row_set_insert(mem_row_set_mut(&mut p_in1.borrow_mut()), v);
    OpFlow::Next
}


// ---- part_018.rs ----

// Notas de integração para o tech lead (mesma convenção do part_004 e do part_016):
// - `fn op_xxx(...) -> OpFlow`: `Next` é o `break`; `JumpToP2` é `goto jump_to_p2`;
//   `JumpToP2AndCheckForInterrupt` é `goto jump_to_p2_and_check_for_interrupt` e
//   `CheckForInterrupt` é `goto check_for_interrupt` (as duas variantes são novas no
//   enum `OpFlow`). `ProgramEntered` é o fim de OP_Program: `p.a_op`, `p.a_mem`,
//   `p.ap_csr` já são os do subprograma; o laço deve pôr o índice da instrução em 0
//   (o `pOp = &aOp[-1]` do C, seguido do `pOp++`), reler `a_op`/`a_mem` de `p` e então
//   conferir a interrupção.
// - Modelo do quadro (VdbeFrame): no C o quadro mora na memória do registro P3
//   (`pRt->z`). Aqui o registro guarda `Mem.p_frame: Option<VdbeFrameRef>`
//   (`Rc<RefCell<VdbeFrame>>`) e `Mem.x_del = Some(vdbe_frame_mem_del)`; `Mem.n` fica 0
//   (o tamanho em bytes do C não é observável). O quadro guarda o estado do PAI
//   (`a_mem`, `ap_csr`, `a_op`, `n_mem`, `n_cursor`, `n_op`, `pc`) movido com
//   `std::mem::take`, e a memória do filho em `a_child_mem: Vec<MemRef>`. O
//   `sqlite3VdbeFrameRestore` (vdbeaux) precisa devolver esses campos a `p` com
//   `std::mem::take` também. `Vdbe.p_self: Weak<RefCell<Vdbe>>` é o `pFrame->v = p`.
// - Mensagens de erro ficam em inglês e idênticas ao C: aparecem na saída do sqlite3.
// - Sem SQLITE_DEBUG: `memIsValid`, `pScopyFrom`, `MemSetTypeFlag(MEM_Undefined)` do
//   laço de verificação e `iFrameMagic` somem.

/// Opcode: RowSetRead P1 P2 P3 * *
/// Synopsis: r[P3]=rowset(P1)
///
/// Extrai o menor valor do objeto RowSet em P1 e põe esse valor no registro P3. Ou, se
/// o objeto RowSet P1 estiver inicialmente vazio, deixa P3 inalterado e salta para a
/// instrução P2.
pub fn op_row_set_read(p: &mut Vdbe, p_op: &Op) -> OpFlow {
    let p_in1 = p.a_mem[p_op.p1 as usize].clone();
    debug_assert!(
        (p_in1.borrow().flags & MEM_BLOB) == 0 || vdbe_mem_is_row_set(&p_in1.borrow())
    );
    let mut val: i64 = 0;
    let got = (p_in1.borrow().flags & MEM_BLOB) != 0
        && row_set_next(mem_row_set_mut(&mut p_in1.borrow_mut()), &mut val) != 0;
    if !got {
        // O índice booleano está vazio
        vdbe_mem_set_null(&mut p_in1.borrow_mut());
        OpFlow::JumpToP2AndCheckForInterrupt
    } else {
        // Um valor foi tirado do índice
        vdbe_mem_set_int64(&mut p.a_mem[p_op.p3 as usize].borrow_mut(), val);
        OpFlow::CheckForInterrupt
    }
}

/// Opcode: RowSetTest P1 P2 P3 P4
/// Synopsis: if r[P3] in rowset(P1) goto P2
///
/// Supõe-se que o registro P3 guarda um inteiro de 64 bits. Se o registro P1 contiver
/// um objeto RowSet e esse objeto contiver o valor guardado em P3, salta para P2. Caso
/// contrário, insere o inteiro de P3 no RowSet e segue para o próximo opcode.
///
/// O objeto RowSet é otimizado para o caso em que conjuntos de inteiros são inseridos
/// em fases distintas, e cada conjunto não tem duplicatas. Cada conjunto é identificado
/// por um valor P4 único. O primeiro conjunto deve ter P4==0, o último deve ter P4==-1
/// e todos os outros devem ter P4>0.
///
/// Isso permite otimizações: (a) quando P4==0 não há necessidade de procurar P3 no
/// RowSet, pois ele garantidamente não o contém, (b) quando P4==-1 não há necessidade
/// de inserir o valor, pois nunca será procurado, e (c) quando um valor que faz parte
/// do conjunto X é inserido, não há necessidade de procurar se o mesmo valor já foi
/// inserido como parte do conjunto X (só se foi inserido antes como parte de outro).
pub fn op_row_set_test(p: &mut Vdbe, p_op: &Op) -> OpFlow {
    let p_in1 = p.a_mem[p_op.p1 as usize].clone();
    let p_in3 = p.a_mem[p_op.p3 as usize].clone();
    let i_set: i32 = match p_op.p4 {
        P4Value::Int32(i) => i,
        _ => 0,
    };
    debug_assert!((p_in3.borrow().flags & MEM_INT) != 0);
    let v = p_in3.borrow().u.i;

    // Se houver qualquer coisa além de um objeto rowset na célula P1, apaga agora e
    // inicializa P1 com um rowset vazio.
    if (p_in1.borrow().flags & MEM_BLOB) == 0 {
        if vdbe_mem_set_row_set(&mut p_in1.borrow_mut()) != SQLITE_OK {
            return OpFlow::NoMem;
        }
    }
    debug_assert!(vdbe_mem_is_row_set(&p_in1.borrow()));
    debug_assert!(p_op.p4type == P4_INT32);
    debug_assert!(i_set == -1 || i_set >= 0);
    if i_set != 0 {
        let exists = row_set_test(mem_row_set_mut(&mut p_in1.borrow_mut()), i_set, v);
        if exists != 0 {
            return OpFlow::JumpToP2;
        }
    }
    if i_set >= 0 {
        row_set_insert(mem_row_set_mut(&mut p_in1.borrow_mut()), v);
    }
    OpFlow::Next
}

/// Opcode: Program P1 P2 P3 P4 P5
///
/// Executa o programa de gatilho passado em P4 (tipo P4_SUBPROGRAM).
///
/// P1 contém o endereço da célula de memória que guarda a primeira célula de um vetor
/// de valores usados como argumentos do subprograma. P2 contém o endereço para onde
/// saltar se o subprograma lançar uma exceção IGNORE com a função RAISE(). P2 pode ser
/// zero, se não houver possibilidade de uma exceção IGNORE. O registro P3 contém o
/// endereço de uma célula de memória nesta VM (a VM pai) que é usada para alocar a
/// memória exigida pelo sub-vdbe em tempo de execução.
///
/// P4 é um ponteiro para a VM que contém o programa de gatilho.
///
/// Se P5 for diferente de zero, a invocação recursiva de programas está habilitada.
pub fn op_program(
    p: &mut Vdbe,
    db: &mut sqlite3,
    p_op: &Op,
    i_op: i32,
    rc: &mut i32,
) -> OpFlow {
    let p_program: SubProgramRef = match &p_op.p4 {
        P4Value::SubProgram(sp) => sp.clone(),
        _ => unreachable!("OP_Program: P4 não é SUBPROGRAM"),
    };
    let p_rt = p.a_mem[p_op.p3 as usize].clone();
    let (prog_n_op, prog_n_mem, prog_n_csr, prog_token) = {
        let sp = p_program.borrow();
        (sp.n_op, sp.n_mem, sp.n_csr, sp.token)
    };
    debug_assert!(prog_n_op > 0);

    // Se a flag p5 estiver limpa, a invocação recursiva de gatilhos fica desabilitada
    // por compatibilidade retroativa (p5 é definida se este subprograma é de fato um
    // gatilho, e não uma ação de chave estrangeira, e a flag definida e limpa pelo
    // comando "PRAGMA recursive_triggers" está limpa).
    //
    // É a invocação recursiva de gatilhos, no nível SQL, que fica desabilitada. Em
    // alguns casos um único gatilho pode gerar mais de um SubProgram (se o gatilho
    // puder executar com mais de um algoritmo ON CONFLICT diferente). Os SubProgram de
    // um mesmo gatilho têm todos o mesmo valor em SubProgram.token.
    if p_op.p5 != 0 {
        let mut cur = p.p_frame.clone();
        while let Some(f) = cur {
            if f.borrow().token == prog_token {
                return OpFlow::Next;
            }
            cur = f.borrow().p_parent.clone();
        }
    }

    if p.n_frame >= db.a_limit[SQLITE_LIMIT_TRIGGER_DEPTH as usize] {
        *rc = SQLITE_ERROR;
        vdbe_error(p, b"too many levels of trigger recursion");
        return OpFlow::AbortDueToError;
    }

    // O registro pRt guarda a memória necessária para salvar o estado do programa
    // atual e a memória exigida em tempo de execução para executar o programa de
    // gatilho. Se este gatilho já disparou antes, pRt já está alocado. Senão, precisa
    // ser inicializado.
    let p_frame: VdbeFrameRef;
    if (p_rt.borrow().flags & MEM_BLOB) == 0 {
        // SubProgram.nMem é o número de células de memória usadas pelo programa em
        // SubProgram.aOp. Além delas, é preciso uma célula de memória para cada cursor
        // usado pelo programa. A variável local n_mem (e depois VdbeFrame.nChildMem)
        // recebe esse valor.
        let mut n_mem = prog_n_mem + prog_n_csr;
        debug_assert!(n_mem > 0);
        if prog_n_csr == 0 {
            n_mem += 1;
        }
        let mut child_mem: Vec<MemRef> = Vec::with_capacity(n_mem as usize);
        for _ in 0..n_mem {
            let mut m = Mem::default();
            vdbe_mem_init(&mut m, db, MEM_UNDEFINED);
            child_mem.push(Rc::new(RefCell::new(m)));
        }
        p_frame = Rc::new(RefCell::new(VdbeFrame {
            v: p.p_self.clone(),
            n_child_mem: n_mem,
            n_child_csr: prog_n_csr,
            token: prog_token,
            a_child_mem: child_mem,
            ..Default::default()
        }));
        let mut rt = p_rt.borrow_mut();
        vdbe_mem_release(&mut rt);
        rt.flags = MEM_BLOB | MEM_DYN;
        rt.p_frame = Some(p_frame.clone());
        rt.n = 0;
        rt.x_del = Some(vdbe_frame_mem_del);
    } else {
        p_frame = p_rt
            .borrow()
            .p_frame
            .clone()
            .expect("OP_Program: registro blob sem quadro");
        let f = p_frame.borrow();
        debug_assert!(p_rt.borrow().x_del == Some(vdbe_frame_mem_del));
        debug_assert!(
            prog_n_mem + prog_n_csr == f.n_child_mem
                || (prog_n_csr == 0 && prog_n_mem + 1 == f.n_child_mem)
        );
        debug_assert!(prog_n_csr == f.n_child_csr);
        debug_assert!(i_op == f.pc);
    }

    // Salva o estado do pai no quadro (os campos de pFrame->aMem a pFrame->nOp do C) e
    // põe o subprograma no lugar.
    p.n_frame += 1;
    let n_child_mem;
    let a_child_mem;
    {
        let mut f = p_frame.borrow_mut();
        f.p_parent = p.p_frame.clone();
        f.last_rowid = db.last_rowid;
        f.n_change = p.n_change;
        f.n_db_change = db.n_change;
        debug_assert!(f.p_aux_data.is_none());
        f.p_aux_data = p.p_aux_data.take();
        f.pc = i_op;
        f.a_mem = std::mem::take(&mut p.a_mem);
        f.n_mem = p.n_mem;
        f.ap_csr = std::mem::take(&mut p.ap_csr);
        f.n_cursor = p.n_cursor;
        f.a_op = std::mem::take(&mut p.a_op);
        f.n_op = p.n_op;
        f.a_once = vec![0u8; ((prog_n_op + 7) / 8) as usize];
        n_child_mem = f.n_child_mem;
        a_child_mem = f.a_child_mem.clone();
    }
    p.n_change = 0;
    p.p_frame = Some(p_frame);
    p.a_mem = a_child_mem;
    p.n_mem = n_child_mem;
    p.n_cursor = prog_n_csr as u16;
    p.ap_csr = vec![None; prog_n_csr as usize];
    p.a_op = p_program.borrow().a_op.clone();
    p.n_op = prog_n_op;
    OpFlow::ProgramEntered
}

/// Opcode: Param P1 P2 * * *
///
/// Este opcode só existe em subprogramas chamados por OP_Program. Copia um valor
/// guardado numa célula de memória do quadro chamador (pai) para a célula P2 do espaço
/// de endereçamento do quadro atual. É usado pelos programas de gatilho para acessar
/// os valores new.* e old.*.
///
/// O endereço da célula no quadro pai é obtido somando o valor do argumento P1 ao
/// valor do argumento P1 da instrução OP_Program chamadora.
pub fn op_param(p: &mut Vdbe, p_op: &Op) -> OpFlow {
    let p_out = out2_prerelease(p, p_op);
    let p_frame = p
        .p_frame
        .clone()
        .expect("OP_Param: fora de subprograma");
    let p_in = {
        let f = p_frame.borrow();
        let idx = p_op.p1 + f.a_op[f.pc as usize].p1;
        f.a_mem[idx as usize].clone()
    };
    vdbe_mem_shallow_copy(&mut p_out.borrow_mut(), &p_in.borrow(), MEM_EPHEM);
    OpFlow::Next
}

/// Opcode: FkCounter P1 P2 * * *
/// Synopsis: fkctr[P1]+=P2
///
/// Incrementa um "contador de restrição" em P2 (P2 pode ser negativo ou positivo). Se
/// P1 for diferente de zero, o contador de restrição do banco é incrementado
/// (restrições de chave estrangeira adiadas). Caso contrário, se P1 for zero, o
/// contador da instrução é incrementado (restrições de chave estrangeira imediatas).
pub fn op_fk_counter(p: &mut Vdbe, db: &mut sqlite3, p_op: &Op) -> OpFlow {
    if (db.flags & SQLITE_DEFERFKS) != 0 {
        db.n_deferred_imm_cons += p_op.p2 as i64;
    } else if p_op.p1 != 0 {
        db.n_deferred_cons += p_op.p2 as i64;
    } else {
        p.n_fk_constraint += p_op.p2 as i64;
    }
    OpFlow::Next
}

/// Opcode: FkIfZero P1 P2 * * *
/// Synopsis: if fkctr[P1]==0 goto P2
///
/// Este opcode testa se um contador de restrição de chave estrangeira é zero no
/// momento. Se for, salta para a instrução P2. Caso contrário segue para a próxima.
///
/// Se P1 for diferente de zero, o salto ocorre se o contador de restrição do banco
/// (o que conta violações de restrições adiadas) for zero. Se P1 for zero, o salto
/// ocorre se o contador de restrição da instrução (violações de chave estrangeira
/// imediatas) for zero.
pub fn op_fk_if_zero(p: &Vdbe, db: &sqlite3, p_op: &Op) -> OpFlow {
    if p_op.p1 != 0 {
        if db.n_deferred_cons == 0 && db.n_deferred_imm_cons == 0 {
            return OpFlow::JumpToP2;
        }
    } else if p.n_fk_constraint == 0 && db.n_deferred_imm_cons == 0 {
        return OpFlow::JumpToP2;
    }
    OpFlow::Next
}

/// Opcode: MemMax P1 P2 * * *
/// Synopsis: r[P1]=max(r[P1],r[P2])
///
/// P1 é um registro no quadro raiz desta VM (o quadro raiz difere do quadro atual se
/// esta instrução estiver executando dentro de um subprograma). Define o valor do
/// registro P1 como o máximo entre o valor atual dele e o valor do registro P2.
///
/// Esta instrução levanta erro se a célula de memória não for inicialmente um inteiro.
pub fn op_mem_max(p: &mut Vdbe, p_op: &Op) -> OpFlow {
    let p_in1: MemRef = if let Some(first) = &p.p_frame {
        let mut p_frame = first.clone();
        loop {
            let parent = p_frame.borrow().p_parent.clone();
            match parent {
                Some(pa) => p_frame = pa,
                None => break,
            }
        }
        let m = p_frame.borrow().a_mem[p_op.p1 as usize].clone();
        m
    } else {
        p.a_mem[p_op.p1 as usize].clone()
    };
    vdbe_mem_integerify(&mut p_in1.borrow_mut());
    let p_in2 = p.a_mem[p_op.p2 as usize].clone();
    vdbe_mem_integerify(&mut p_in2.borrow_mut());
    let v2 = p_in2.borrow().u.i;
    let mut in1 = p_in1.borrow_mut();
    if in1.u.i < v2 {
        in1.u.i = v2;
    }
    OpFlow::Next
}

/// Opcode: IfPos P1 P2 P3 * *
/// Synopsis: if r[P1]>0 then r[P1]-=P3, goto P2
///
/// O registro P1 deve conter um inteiro. Se o valor do registro P1 for 1 ou maior,
/// subtrai P3 do valor de P1 e salta para P2.
///
/// Se o valor inicial do registro P1 for menor que 1, o valor fica inalterado e o
/// controle passa para a próxima instrução.
pub fn op_if_pos(p: &mut Vdbe, p_op: &Op) -> OpFlow {
    let p_in1 = p.a_mem[p_op.p1 as usize].clone();
    let mut in1 = p_in1.borrow_mut();
    debug_assert!((in1.flags & MEM_INT) != 0);
    if in1.u.i > 0 {
        in1.u.i -= p_op.p3 as i64;
        return OpFlow::JumpToP2;
    }
    OpFlow::Next
}

/// Opcode: OffsetLimit P1 P2 P3 * *
/// Synopsis: if r[P1]>0 then r[P2]=r[P1]+max(0,r[P3]) else r[P2]=(-1)
///
/// Este opcode faz um cálculo comum associado ao processamento de LIMIT e OFFSET.
/// r[P1] guarda o contador de limite. r[P3] guarda o contador de deslocamento. O
/// opcode calcula o valor combinado de LIMIT e OFFSET e guarda esse valor em r[P2]. O
/// valor calculado de r[P2] é o número total de linhas que precisarão ser visitadas
/// para completar a consulta.
///
/// Se r[P3] for zero ou negativo, não há OFFSET e r[P2] recebe o valor do LIMIT, r[P1].
///
/// Se r[P1] for zero ou negativo, não há LIMIT e r[P2] recebe -1.
///
/// Caso contrário, r[P2] recebe a soma de r[P1] e r[P3].
pub fn op_offset_limit(p: &mut Vdbe, p_op: &Op) -> OpFlow {
    let p_in1 = p.a_mem[p_op.p1 as usize].clone();
    let p_in3 = p.a_mem[p_op.p3 as usize].clone();
    let p_out = out2_prerelease(p, p_op);
    debug_assert!((p_in1.borrow().flags & MEM_INT) != 0);
    debug_assert!((p_in3.borrow().flags & MEM_INT) != 0);
    let mut x: i64 = p_in1.borrow().u.i;
    let offset: i64 = {
        let i3 = p_in3.borrow().u.i;
        if i3 > 0 { i3 } else { 0 }
    };
    if x <= 0 || add_int64(&mut x, offset) != 0 {
        // Se o LIMIT for menor ou igual a zero, repete para sempre. Isso é
        // documentado. Mas também, se LIMIT+OFFSET passar de 2^63, repete para sempre.
        // Isso não é documentado. Na verdade, poderia se argumentar que o laço deveria
        // terminar. Mas supondo 1 bilhão de iterações por segundo (muito além da
        // capacidade de qualquer hardware atual), levaria quase 300 anos para chegar
        // ao limite. Então repetir para sempre é uma aproximação razoável.
        p_out.borrow_mut().u.i = -1;
    } else {
        p_out.borrow_mut().u.i = x;
    }
    OpFlow::Next
}


// ---- part_019.rs ----

// Notas de integração para o tech lead (mesma convenção do part_004 e do part_016):
// - `fn op_xxx(...) -> OpFlow`; erro vai em `*rc` + `OpFlow::AbortDueToError`; `*rc`
//   chega aqui como SQLITE_OK (o `rc` do C vale OK na entrada de cada opcode, e
//   `OP_JournalMode` lê esse valor).
// - OP_AggStep/OP_AggInverse e OP_AggStep1 formam um `case` com queda (fall through) no
//   C. `op_agg_step` faz a preparação do contexto (grava `P4_FUNCCTX` e troca o opcode
//   por OP_AGGSTEP1 em `p.a_op[i_op]`) e termina chamando `op_agg_step1`. Como as duas
//   mexem em `p.a_op`, o laço deve passar um `p_op` clonado (ou copiar o necessário).
// - `sqlite3_context` é `ContextRef = Rc<RefCell<sqlite3_context>>` com `p_mem:
//   Option<MemRef>`, `p_out: MemRef`, `p_func: FuncDefRef`, `argv: Vec<MemRef>`,
//   `argc`, `i_op`, `p_vdbe: Weak<..>`, `skip_flag`, `is_error`, `enc`. As funções do
//   usuário são `Rc<dyn Fn(&ContextRef, &[MemRef])>` em `FuncDef.x_s_func` e
//   `x_inverse`. `P4Value::FuncCtx(ContextRef)` e `P4Value::FuncDef(FuncDefRef)`.
// - `journal_modename(mode) -> Option<&'static [u8]>` é o `sqlite3JournalModename` do
//   pragma.c; não é redefinido aqui (DRY).
// - Sem SQLITE_DEBUG: `uTemp==0x1122e0e3`, `memIsValid` e `REGISTER_TRACE` somem.

/// Opcode: IfNotZero P1 P2 * * *
/// Synopsis: if r[P1]!=0 then r[P1]--, goto P2
///
/// O registro P1 deve conter um inteiro. Se o conteúdo do registro P1 for inicialmente
/// maior que zero, decrementa o valor de P1. Se for diferente de zero (negativo ou
/// positivo), salta também para P2. Se o registro P1 for inicialmente zero, deixa-o
/// inalterado e segue para a próxima instrução.
pub fn op_if_not_zero(p: &mut Vdbe, p_op: &Op) -> OpFlow {
    let p_in1 = p.a_mem[p_op.p1 as usize].clone();
    let mut in1 = p_in1.borrow_mut();
    debug_assert!((in1.flags & MEM_INT) != 0);
    if in1.u.i != 0 {
        if in1.u.i > 0 {
            in1.u.i -= 1;
        }
        return OpFlow::JumpToP2;
    }
    OpFlow::Next
}

/// Opcode: DecrJumpZero P1 P2 * * *
/// Synopsis: if (--r[P1])==0 goto P2
///
/// O registro P1 deve guardar um inteiro. Decrementa o valor de P1 e salta para P2 se
/// o novo valor for exatamente zero.
pub fn op_decr_jump_zero(p: &mut Vdbe, p_op: &Op) -> OpFlow {
    let p_in1 = p.a_mem[p_op.p1 as usize].clone();
    let mut in1 = p_in1.borrow_mut();
    debug_assert!((in1.flags & MEM_INT) != 0);
    if in1.u.i > SMALLEST_INT64 {
        in1.u.i -= 1;
    }
    if in1.u.i == 0 {
        return OpFlow::JumpToP2;
    }
    OpFlow::Next
}

/// Opcode: AggStep * P2 P3 P4 P5
/// Synopsis: accum=r[P3] step(r[P2@P5])
///
/// Executa a função xStep de um agregado. A função tem P5 argumentos. P4 é um ponteiro
/// para a estrutura FuncDef que especifica a função. O registro P3 é o acumulador.
///
/// Os P5 argumentos são tirados do registro P2 e dos seus sucessores.
///
/// Opcode: AggInverse * P2 P3 P4 P5
/// Synopsis: accum=r[P3] inverse(r[P2@P5])
///
/// Executa a função xInverse de um agregado. Mesmos argumentos do AggStep.
///
/// Opcode: AggStep1 P1 P2 P3 P4 P5
/// Synopsis: accum=r[P3] step(r[P2@P5])
///
/// Executa a função xStep (se P1==0) ou xInverse (se P1!=0) de um agregado. A função
/// tem P5 argumentos. P4 é um ponteiro para a estrutura FuncDef que especifica a
/// função. O registro P3 é o acumulador.
///
/// Este opcode é codificado inicialmente como OP_AggStep0. Na primeira avaliação, o
/// FuncDef guardado em P4 é convertido num sqlite3_context e o opcode é trocado. Assim,
/// a inicialização do sqlite3_context acontece uma só vez, em vez de a cada chamada da
/// função de passo.
pub fn op_agg_step(
    p: &mut Vdbe,
    db: &sqlite3,
    p_op: &Op,
    i_op: i32,
    rc: &mut i32,
    encoding: u8,
) -> OpFlow {
    debug_assert!(p_op.p4type == P4_FUNCDEF);
    let n = p_op.p5 as i32;
    debug_assert!(p_op.p3 > 0 && p_op.p3 <= (p.n_mem + 1 - p.n_cursor as i32));
    debug_assert!(n == 0 || (p_op.p2 > 0 && p_op.p2 + n <= (p.n_mem + 1 - p.n_cursor as i32) + 1));
    debug_assert!(p_op.p3 < p_op.p2 || p_op.p3 >= p_op.p2 + n);
    let p_func = match &p_op.p4 {
        P4Value::FuncDef(f) => f.clone(),
        _ => unreachable!("OP_AggStep: P4 não é FUNCDEF"),
    };
    let mut out = Mem::default();
    vdbe_mem_init(&mut out, db, MEM_NULL);
    let p_ctx: ContextRef = Rc::new(RefCell::new(sqlite3_context {
        p_mem: None,
        p_out: Rc::new(RefCell::new(out)),
        p_func,
        i_op,
        p_vdbe: p.p_self.clone(),
        skip_flag: 0,
        is_error: 0,
        enc: encoding,
        argc: n,
        argv: Vec::new(),
        ..Default::default()
    }));
    {
        let op = &mut p.a_op[i_op as usize];
        op.p4type = P4_FUNCCTX;
        op.p4 = P4Value::FuncCtx(p_ctx);

        // OP_AggInverse deve ter P1==1 e OP_AggStep deve ter P1==0
        debug_assert!(op.p1 == (if op.opcode == OP_AGGINVERSE { 1 } else { 0 }));

        op.opcode = OP_AGGSTEP1;
    }
    // Cai no OP_AggStep1
    let p_op1 = p.a_op[i_op as usize].clone();
    op_agg_step1(p, &p_op1, i_op, rc)
}

/// Segunda metade do case de AggStep (a do `case OP_AggStep1`); ver `op_agg_step`.
pub fn op_agg_step1(p: &mut Vdbe, p_op: &Op, i_op: i32, rc: &mut i32) -> OpFlow {
    debug_assert!(p_op.p4type == P4_FUNCCTX);
    let p_ctx: ContextRef = match &p_op.p4 {
        P4Value::FuncCtx(c) => c.clone(),
        _ => unreachable!("OP_AggStep1: P4 não é FUNCCTX"),
    };
    let p_mem = p.a_mem[p_op.p3 as usize].clone();

    // Se esta função estiver dentro de um gatilho, o vetor de registros aMem[] pode
    // mudar de uma avaliação para a seguinte. O bloco abaixo confere se o vetor mudou
    // e, se sim, reinicializa as partes relevantes do objeto sqlite3_context.
    {
        let mut c = p_ctx.borrow_mut();
        let same = matches!(&c.p_mem, Some(m) if Rc::ptr_eq(m, &p_mem));
        if !same {
            c.p_mem = Some(p_mem.clone());
            let argc = c.argc as usize;
            c.argv = (0..argc)
                .map(|i| p.a_mem[p_op.p2 as usize + i].clone())
                .collect();
        }
    }

    p_mem.borrow_mut().n += 1;
    debug_assert!(p_ctx.borrow().p_out.borrow().flags == MEM_NULL);
    debug_assert!(p_ctx.borrow().is_error == 0);
    debug_assert!(p_ctx.borrow().skip_flag == 0);
    let argv = p_ctx.borrow().argv.clone();
    if p_op.p1 != 0 {
        let f = p_ctx.borrow().p_func.borrow().x_inverse.clone();
        (f.expect("OP_AggStep1: xInverse ausente"))(&p_ctx, &argv);
    } else {
        let f = p_ctx.borrow().p_func.borrow().x_s_func.clone();
        (f.expect("OP_AggStep1: xSFunc ausente"))(&p_ctx, &argv); /* IMP: R-24505-23230 */
    }

    if p_ctx.borrow().is_error != 0 {
        if p_ctx.borrow().is_error > 0 {
            let msg = value_text(&p_ctx.borrow().p_out.borrow());
            vdbe_error(p, &msg);
            *rc = p_ctx.borrow().is_error;
        }
        if p_ctx.borrow().skip_flag != 0 {
            debug_assert!(p.a_op[i_op as usize - 1].opcode == OP_COLLSEQ);
            let i = p.a_op[i_op as usize - 1].p1;
            if i != 0 {
                vdbe_mem_set_int64(&mut p.a_mem[i as usize].borrow_mut(), 1);
            }
            p_ctx.borrow_mut().skip_flag = 0;
        }
        {
            let c = p_ctx.borrow();
            vdbe_mem_release(&mut c.p_out.borrow_mut());
            c.p_out.borrow_mut().flags = MEM_NULL;
        }
        p_ctx.borrow_mut().is_error = 0;
        if *rc != SQLITE_OK {
            return OpFlow::AbortDueToError;
        }
    }
    debug_assert!(p_ctx.borrow().p_out.borrow().flags == MEM_NULL);
    debug_assert!(p_ctx.borrow().skip_flag == 0);
    OpFlow::Next
}

/// Opcode: AggFinal P1 P2 * P4 *
/// Synopsis: accum=r[P1] N=P2
///
/// P1 é a posição de memória que é o acumulador de uma função de agregação ou de
/// janela. Executa a função finalizadora de um agregado e guarda o resultado em P1.
///
/// P2 é o número de argumentos que a função de passo recebe e P4 é um ponteiro para o
/// FuncDef desta função. O argumento P2 não é usado por este opcode. Ele existe só
/// para desambiguar funções que aceitam números variáveis de argumentos. O argumento
/// P4 só é necessário quando a função de passo não foi chamada antes.
///
/// Opcode: AggValue * P2 P3 P4 *
/// Synopsis: r[P3]=value N=P2
///
/// Chama a função xValue() e guarda o resultado no registro P3.
///
/// P2 é o número de argumentos que a função de passo recebe e P4 é um ponteiro para o
/// FuncDef desta função. O argumento P2 não é usado por este opcode. Ele existe só
/// para desambiguar funções que aceitam números variáveis de argumentos. O argumento
/// P4 só é necessário quando a função de passo não foi chamada antes.
pub fn op_agg_value_or_final(p: &mut Vdbe, p_op: &Op, rc: &mut i32, encoding: u8) -> OpFlow {
    debug_assert!(p_op.p1 > 0 && p_op.p1 <= (p.n_mem + 1 - p.n_cursor as i32));
    debug_assert!(p_op.p3 == 0 || p_op.opcode == OP_AGGVALUE);
    let p_func = match &p_op.p4 {
        P4Value::FuncDef(f) => f.clone(),
        _ => unreachable!("OP_AggFinal/AggValue: P4 não é FUNCDEF"),
    };
    let mut p_mem = p.a_mem[p_op.p1 as usize].clone();
    debug_assert!((p_mem.borrow().flags & !(MEM_NULL | MEM_AGG)) == 0);
    if p_op.p3 != 0 {
        let p_out = p.a_mem[p_op.p3 as usize].clone();
        *rc = vdbe_mem_agg_value(&mut p_mem.borrow_mut(), &mut p_out.borrow_mut(), &p_func);
        p_mem = p_out;
    } else {
        *rc = vdbe_mem_finalize(&mut p_mem.borrow_mut(), &p_func);
    }
    if *rc != SQLITE_OK {
        let msg = value_text(&p_mem.borrow());
        vdbe_error(p, &msg);
        return OpFlow::AbortDueToError;
    }
    vdbe_change_encoding(&mut p_mem.borrow_mut(), encoding);
    OpFlow::Next
}

/// Opcode: Checkpoint P1 P2 P3 * *
///
/// Faz checkpoint do banco P1. Não faz nada se P1 não estiver em modo WAL no momento.
/// O parâmetro P2 é um entre SQLITE_CHECKPOINT_PASSIVE, FULL, RESTART ou TRUNCATE.
/// Grava 1 ou 0 em mem[P3] conforme o checkpoint retorne SQLITE_BUSY ou não. Grava em
/// mem[P3+1] o número de páginas no WAL após o checkpoint e em mem[P3+2] o número de
/// páginas do WAL que passaram por checkpoint após a conclusão. Em caso de erro,
/// mem[P3+1] e mem[P3+2] são inicializados com -1.
pub fn op_checkpoint(p: &mut Vdbe, db: &mut sqlite3, p_op: &Op, rc: &mut i32) -> OpFlow {
    debug_assert!(p.read_only == 0);
    let mut a_res: [i32; 3] = [0, -1, -1]; // Resultados
    debug_assert!(
        p_op.p2 == SQLITE_CHECKPOINT_PASSIVE
            || p_op.p2 == SQLITE_CHECKPOINT_FULL
            || p_op.p2 == SQLITE_CHECKPOINT_RESTART
            || p_op.p2 == SQLITE_CHECKPOINT_TRUNCATE
    );
    let (mut n_log, mut n_ckpt) = (-1i32, -1i32);
    *rc = checkpoint(db, p_op.p1, p_op.p2, &mut n_log, &mut n_ckpt);
    a_res[1] = n_log;
    a_res[2] = n_ckpt;
    if *rc != SQLITE_OK {
        if *rc != SQLITE_BUSY {
            return OpFlow::AbortDueToError;
        }
        *rc = SQLITE_OK;
        a_res[0] = 1;
    }
    for i in 0..3usize {
        let p_mem = p.a_mem[p_op.p3 as usize + i].clone();
        vdbe_mem_set_int64(&mut p_mem.borrow_mut(), a_res[i] as i64);
    }
    OpFlow::Next
}

/// Opcode: JournalMode P1 P2 P3 * *
///
/// Muda o modo de journal do banco P1 para P3. P3 deve ser um dos valores
/// PAGER_JOURNALMODE_XXX. Se a mudança for entre os vários modos de rollback (delete,
/// truncate, persist, off e memory), é uma operação simples e não exige E/S.
///
/// Se a mudança entrar ou sair do modo WAL, o procedimento é mais complicado.
///
/// Grava no registro P2 uma string com o modo de journal final.
pub fn op_journal_mode(
    p: &mut Vdbe,
    db: &sqlite3,
    p_op: &Op,
    rc: &mut i32,
    encoding: u8,
) -> OpFlow {
    let p_out = out2_prerelease(p, p_op);
    let mut e_new = p_op.p3; // Novo modo de journal
    debug_assert!(
        e_new == PAGER_JOURNALMODE_DELETE
            || e_new == PAGER_JOURNALMODE_TRUNCATE
            || e_new == PAGER_JOURNALMODE_PERSIST
            || e_new == PAGER_JOURNALMODE_OFF
            || e_new == PAGER_JOURNALMODE_MEMORY
            || e_new == PAGER_JOURNALMODE_WAL
            || e_new == PAGER_JOURNALMODE_QUERY
    );
    debug_assert!(p_op.p1 >= 0 && p_op.p1 < db.n_db);
    debug_assert!(p.read_only == 0);

    let p_bt = db.a_db[p_op.p1 as usize]
        .p_bt
        .clone()
        .expect("OP_JournalMode: Btree ausente"); // Btree que muda de modo de journal
    let p_pager = btree_pager(&p_bt); // Pager associado a p_bt
    let e_old = pager_get_journal_mode(&p_pager); // Modo de journal antigo
    if e_new == PAGER_JOURNALMODE_QUERY {
        e_new = e_old;
    }
    debug_assert!(btree_holds_mutex(&p_bt));
    if !pager_ok_to_change_journal_mode(&p_pager) {
        e_new = e_old;
    }

    let z_filename = pager_filename(&p_pager, 1); // Nome do arquivo de banco do pager

    // Não permite a transição para journal_mode=WAL num banco em armazenamento
    // temporário ou se o VFS não suporta memória compartilhada.
    if e_new == PAGER_JOURNALMODE_WAL
        && (strlen30(&z_filename) == 0 /* Arquivo temporário */
            || !pager_wal_supported(&p_pager)) /* Sem suporte a memória compartilhada */
    {
        e_new = e_old;
    }

    if e_new != e_old && (e_old == PAGER_JOURNALMODE_WAL || e_new == PAGER_JOURNALMODE_WAL) {
        if db.auto_commit == 0 || db.n_vdbe_read > 1 {
            *rc = SQLITE_ERROR;
            let msg: &[u8] = if e_new == PAGER_JOURNALMODE_WAL {
                b"cannot change into wal mode from within a transaction"
            } else {
                b"cannot change out of wal mode from within a transaction"
            };
            vdbe_error(p, msg);
            return OpFlow::AbortDueToError;
        } else {
            if e_old == PAGER_JOURNALMODE_WAL {
                // Ao sair do modo WAL, fecha o arquivo de log. Se tiver sucesso, a
                // chamada a PagerCloseWal() faz checkpoint e apaga o arquivo de
                // write-ahead-log. Um lock EXCLUSIVE ainda pode ficar retido no
                // arquivo de banco após um retorno bem-sucedido.
                *rc = pager_close_wal(&p_pager, db);
                if *rc == SQLITE_OK {
                    pager_set_journal_mode(&p_pager, e_new);
                }
            } else if e_old == PAGER_JOURNALMODE_MEMORY {
                // Não dá para passar direto de MEMORY para WAL. Usa o modo OFF como
                // intermediário.
                pager_set_journal_mode(&p_pager, PAGER_JOURNALMODE_OFF);
            }

            // Abre uma transação no arquivo de banco. Qualquer que seja o modo de
            // journal, esta transação sempre usa um rollback journal.
            debug_assert!(btree_txn_state(&p_bt) != SQLITE_TXN_WRITE);
            if *rc == SQLITE_OK {
                *rc = btree_set_version(&p_bt, if e_new == PAGER_JOURNALMODE_WAL { 2 } else { 1 });
            }
        }
    }

    if *rc != SQLITE_OK {
        e_new = e_old;
    }
    e_new = pager_set_journal_mode(&p_pager, e_new);

    {
        let z = journal_modename(e_new).unwrap_or(b"");
        let mut out = p_out.borrow_mut();
        out.flags = MEM_STR | MEM_STATIC | MEM_TERM;
        out.z = z.to_vec();
        out.n = strlen30(z);
        out.enc = SQLITE_UTF8;
    }
    vdbe_change_encoding(&mut p_out.borrow_mut(), encoding);
    if *rc != SQLITE_OK {
        return OpFlow::AbortDueToError;
    }
    OpFlow::Next
}


// ---- part_020.rs ----

/// Desfecho de um opcode que não termina com um simples `break` do C.
/// `Next` é o `break`; os demais são os `goto` do laço de sqlite3VdbeExec.
pub enum OpFlow {
    Next,
    AbortDueToError,
    JumpToP2,
    NoMem,
}

/// Recupera o `sqlite3` dono do Vdbe (o campo `db` é um `Weak`).
pub fn vdbe_db(p: &Vdbe) -> Rc<RefCell<sqlite3>> {
    p.db.upgrade().expect("sqlite3 liberado antes do Vdbe")
}

#[cfg(not(feature = "sqlite_omit_vacuum"))]
#[cfg(not(feature = "sqlite_omit_attach"))]
/// Opcode: Vacuum P1 P2 * * *
///
/// Limpa a base de dados inteira P1. P1 é 0 para "main" e 2 ou mais para uma
/// base de dados anexada. A base de dados "temp" não pode ser limpada.
///
/// Se P2 não for zero, então ele é um registro contendo uma string que é
/// o arquivo em que o resultado do vacuum deve ser gravado. Quando P2 é
/// zero, o vacuum sobrescreve a base de dados original.
pub fn op_vacuum(p: &mut Vdbe, p_op: &VdbeOp, a_mem: &[Mem]) -> OpFlow {
    debug_assert!(p.read_only == 0);
    let p2_mem = if p_op.p2 != 0 {
        Some(&a_mem[p_op.p2 as usize])
    } else {
        None
    };
    let db = vdbe_db(p);
    let rc = run_vacuum(&mut p.z_err_msg, &db, p_op.p1, p2_mem);
    if rc != 0 {
        p.rc = rc;
        return OpFlow::AbortDueToError;
    }
    OpFlow::Next
}

#[cfg(not(feature = "sqlite_omit_autovacuum"))]
/// Opcode: IncrVacuum P1 P2 * * *
///
/// Executa um passo único do procedimento de vacuum incremental na base
/// de dados P1. Se o vacuum terminou, salta para a instrução P2. Senão,
/// passa para a próxima instrução.
pub fn op_incr_vacuum(p: &mut Vdbe, p_op: &VdbeOp) -> OpFlow {
    let db = vdbe_db(p);
    debug_assert!(p_op.p1 >= 0 && (p_op.p1 as usize) < db.borrow().a_db.len());
    debug_assert!(db_mask_test(p.btree_mask, p_op.p1));
    debug_assert!(p.read_only == 0);

    let p_bt = db.borrow().a_db[p_op.p1 as usize].p_bt.clone();

    let rc = btree_incr_vacuum(&p_bt);
    if rc != 0 {
        if rc != SQLITE_DONE {
            p.rc = rc;
            return OpFlow::AbortDueToError;
        }
        p.rc = SQLITE_OK;
        return OpFlow::JumpToP2;
    }
    OpFlow::Next
}

/// Opcode: Expire P1 P2 * * *
///
/// Faz com que as declarações pré-compiladas vençam. Quando uma declaração
/// vencida é executada usando sqlite3_step(), ela irá se reprepará automaticamente
/// (se foi originalmente criada usando sqlite3_prepare_v2()) ou falhará com
/// SQLITE_SCHEMA.
///
/// Se P1 é 0, então todas as declarações SQL vencem. Se P1 não é zero, então
/// apenas a declaração atualmente em execução expira.
///
/// Se P2 é 0, então as declarações SQL expiram imediatamente. Se P2 é 1, então
/// as declarações SQL em execução podem continuar executando até terminar.
/// O caso P2==1 ocorre quando um CREATE INDEX ou mudança de esquema similar
/// acontece que pode ajudar a declaração rodar mais rápido, mas não afeta a
/// correção da operação.
pub fn op_expire(p: &mut Vdbe, p_op: &VdbeOp) -> OpFlow {
    debug_assert!(p_op.p2 == 0 || p_op.p2 == 1);
    if p_op.p1 == 0 {
        let db = vdbe_db(p);
        expire_prepared_statements(&db, p_op.p2);
    } else {
        p.expired = p_op.p2 + 1;
    }
    OpFlow::Next
}

/// Opcode: CursorLock P1 * * * *
///
/// Bloqueia a árvore B para a qual o cursor P1 aponta de forma que a árvore B
/// não possa ser escrita por outro cursor.
pub fn op_cursor_lock(p: &mut Vdbe, p_op: &VdbeOp) -> OpFlow {
    debug_assert!(p_op.p1 >= 0 && (p_op.p1 as usize) < p.ap_csr.len());
    let p_c = p.ap_csr[p_op.p1 as usize].as_ref().unwrap();
    let mut p_c = p_c.borrow_mut();
    debug_assert!(p_c.e_cur_type == CURTYPE_BTREE);

    if let VdbeCursorCursorUnion::PCursor(cursor) = &mut p_c.uc {
        btree_cursor_pin(cursor);
    }
    OpFlow::Next
}

/// Opcode: CursorUnlock P1 * * * *
///
/// Desbloqueia a árvore B para a qual o cursor P1 aponta de forma que possa
/// ser escrita por outros cursores.
pub fn op_cursor_unlock(p: &mut Vdbe, p_op: &VdbeOp) -> OpFlow {
    debug_assert!(p_op.p1 >= 0 && (p_op.p1 as usize) < p.ap_csr.len());
    let p_c = p.ap_csr[p_op.p1 as usize].as_ref().unwrap();
    let mut p_c = p_c.borrow_mut();
    debug_assert!(p_c.e_cur_type == CURTYPE_BTREE);

    if let VdbeCursorCursorUnion::PCursor(cursor) = &mut p_c.uc {
        btree_cursor_unpin(cursor);
    }
    OpFlow::Next
}

#[cfg(not(feature = "sqlite_omit_shared_cache"))]
/// Opcode: TableLock P1 P2 P3 P4 *
/// Sinopse: iDb=P1 root=P2 write=P3
///
/// Obtém um bloqueio em uma tabela particular. Este opcode é usado apenas quando
/// o recurso de cache compartilhado está ativado.
///
/// P1 é o índice da base de dados em sqlite3.aDb[] da base de dados na qual
/// o bloqueio é adquirido. Um bloqueio de leitura é obtido se P3==0 ou um
/// bloqueio de escrita se P3==1.
///
/// P2 contém a raiz da página da tabela a bloquear.
///
/// P4 contém um ponteiro para o nome da tabela sendo bloqueada. Isto é usado
/// apenas para gerar uma mensagem de erro se o bloqueio não puder ser obtido.
pub fn op_table_lock(p: &mut Vdbe, p_op: &VdbeOp) -> OpFlow {
    let db = vdbe_db(p);
    let is_write_lock: u8 = p_op.p3 as u8;
    if is_write_lock != 0 || (db.borrow().flags & SQLITE_READ_UNCOMMIT) == 0 {
        let p1 = p_op.p1;
        debug_assert!(p1 >= 0 && (p1 as usize) < db.borrow().a_db.len());
        debug_assert!(db_mask_test(p.btree_mask, p1));
        debug_assert!(is_write_lock == 0 || is_write_lock == 1);

        let p_bt = db.borrow().a_db[p1 as usize].p_bt.clone();
        let rc = btree_lock_table(&p_bt, p_op.p2, is_write_lock);
        if rc != 0 {
            if (rc & 0xFF) == SQLITE_LOCKED {
                let z = match &p_op.p4 {
                    P4Value::Z(s) => s.as_deref().unwrap_or("?"),
                    _ => "?",
                };
                vdbe_error(p, &format!("database table is locked: {}", z));
            }
            p.rc = rc;
            return OpFlow::AbortDueToError;
        }
    }
    OpFlow::Next
}

#[cfg(not(feature = "sqlite_omit_virtualtable"))]
/// Opcode: VBegin * * * P4 *
///
/// P4 pode ser um ponteiro para uma estrutura sqlite3_vtab. Se for, chama
/// o método xBegin para aquela tabela.
///
/// Também, independentemente de P4 estar definido ou não, verifica que isto
/// não está sendo chamado de dentro de um callback para o método xSync() de
/// uma tabela virtual. Se estiver, o código de erro será definido para
/// SQLITE_LOCKED.
pub fn op_vbegin(p: &mut Vdbe, p_op: &VdbeOp) -> OpFlow {
    let p_v_tab = match &p_op.p4 {
        P4Value::VTab(vt) => Some(vt),
        _ => None,
    };

    let db = vdbe_db(p);
    let rc = vtab_begin(&db, p_v_tab);

    if let Some(vt) = p_v_tab {
        vtab_import_errmsg(p, vt);
    }

    if rc != 0 {
        p.rc = rc;
        return OpFlow::AbortDueToError;
    }
    OpFlow::Next
}

#[cfg(not(feature = "sqlite_omit_virtualtable"))]
/// Opcode: VCreate P1 P2 * * *
///
/// P2 é um registro que guarda o nome de uma tabela virtual na base de
/// dados P1. Chama o método xCreate para aquela tabela.
pub fn op_vcreate(p: &mut Vdbe, p_op: &VdbeOp) -> OpFlow {
    let db = vdbe_db(p);
    let mut s_mem = Mem::new();
    s_mem.db = p.db.clone();

    // P2 é sempre uma string estática, então a cópia não pode falhar.
    let a_mem_p2 = p.a_mem[p_op.p2 as usize].borrow();
    debug_assert!((a_mem_p2.flags & MEM_STR) != 0);
    debug_assert!((a_mem_p2.flags & MEM_STATIC) != 0);

    let mut rc = vdbe_mem_copy(&mut s_mem, &a_mem_p2);
    drop(a_mem_p2);
    debug_assert!(rc == SQLITE_OK);

    let z_tab = value_text(&s_mem).map(|z| z.to_vec());
    debug_assert!(z_tab.is_some() || db.borrow().malloc_failed != 0);

    if let Some(tab_name) = z_tab {
        rc = vtab_call_create(&db, p_op.p1, &tab_name, &mut p.z_err_msg);
    }

    vdbe_mem_release(&mut s_mem);
    if rc != 0 {
        p.rc = rc;
        return OpFlow::AbortDueToError;
    }
    OpFlow::Next
}

#[cfg(not(feature = "sqlite_omit_virtualtable"))]
/// Opcode: VDestroy P1 * * P4 *
///
/// P4 é o nome de uma tabela virtual na base de dados P1. Chama o método
/// xDestroy daquela tabela.
pub fn op_vdestroy(p: &mut Vdbe, p_op: &VdbeOp) -> OpFlow {
    let db = vdbe_db(p);
    db.borrow_mut().n_v_destroy += 1;

    let z = match &p_op.p4 {
        P4Value::Z(s) => s.as_deref().unwrap_or(""),
        _ => "",
    };

    let rc = vtab_call_destroy(&db, p_op.p1, z);
    db.borrow_mut().n_v_destroy -= 1;

    debug_assert!(p.error_action == OE_ABORT && p.uses_stmt_journal != 0);

    if rc != 0 {
        p.rc = rc;
        return OpFlow::AbortDueToError;
    }
    OpFlow::Next
}

#[cfg(not(feature = "sqlite_omit_virtualtable"))]
/// Opcode: VOpen P1 * * P4 *
///
/// P4 é um ponteiro para um objeto de tabela virtual, uma estrutura
/// sqlite3_vtab. P1 é um número de cursor. Este opcode abre um cursor
/// para a tabela virtual e armazena esse cursor em P1.
pub fn op_vopen(p: &mut Vdbe, p_op: &VdbeOp) -> OpFlow {
    debug_assert!(p.b_is_reader != 0);

    let p_v_tab_ref = match &p_op.p4 {
        P4Value::VTab(vt) => vt.as_ref(),
        _ => {
            p.rc = SQLITE_LOCKED;
            return OpFlow::AbortDueToError;
        }
    };

    let p_v_tab = match p_v_tab_ref {
        Some(vt) => &vt.p_v_tab,
        None => {
            p.rc = SQLITE_LOCKED;
            return OpFlow::AbortDueToError;
        }
    };

    if p_v_tab.is_none() {
        p.rc = SQLITE_LOCKED;
        return OpFlow::AbortDueToError;
    }

    let p_module = match p_v_tab.as_ref() {
        Some(vt) => {
            if vt.p_module.is_none() {
                p.rc = SQLITE_LOCKED;
                return OpFlow::AbortDueToError;
            }
            vt.p_module.as_ref()
        }
        None => {
            p.rc = SQLITE_LOCKED;
            return OpFlow::AbortDueToError;
        }
    };

    if p_module.is_none() {
        p.rc = SQLITE_LOCKED;
        return OpFlow::AbortDueToError;
    }

    let module = p_module.as_ref().unwrap();
    let mut p_v_cur: Option<Box<sqlite3_vtab_cursor>> = None;

    let rc = module.x_open(p_v_tab.as_ref().unwrap(), &mut p_v_cur);

    if let Some(vt) = p_v_tab.as_ref() {
        vtab_import_errmsg(p, vt);
    }

    if rc != 0 {
        p.rc = rc;
        return OpFlow::AbortDueToError;
    }

    if let Some(ref mut v_cur) = p_v_cur {
        if let Some(vt) = p_v_tab.as_ref() {
            v_cur.p_v_tab = Some(vt.clone());
        }
    }

    let p_cur = allocate_cursor(p, p_op.p1, 0, CURTYPE_VTAB);
    if let Some(cursor) = p_cur {
        if let Some(v_cur) = p_v_cur {
            cursor.borrow_mut().uc = VdbeCursorCursorUnion::PVCur(v_cur);
        }
        if let Some(vt) = p_v_tab.as_ref() {
            vt.n_ref.set(vt.n_ref.get() + 1);
        }
    } else {
        debug_assert!(vdbe_db(p).borrow().malloc_failed != 0);
        if let Some(v_cur) = p_v_cur {
            if let Some(mod_ref) = p_module.as_ref() {
                mod_ref.x_close(v_cur);
            }
        }
        return OpFlow::NoMem;
    }

    OpFlow::Next
}

#[cfg(not(feature = "sqlite_omit_virtualtable"))]
/// Opcode: VCheck P1 P2 P3 P4 *
///
/// P4 é um ponteiro para um objeto Table que é uma tabela virtual no
/// esquema P1 que suporta o método xIntegrity(). Este opcode executa o
/// método xIntegrity() para aquela tabela virtual, usando P3 como o
/// argumento inteiro. Se um erro é reportado de volta, o nome da tabela
/// é preposto à mensagem de erro e aquela mensagem é armazenada em P2.
/// Se nenhum erro é visto, o registro P2 é definido para NULL.
pub fn op_vcheck(p: &mut Vdbe, p_op: &VdbeOp) -> OpFlow {
    let p_out = p.a_mem[p_op.p2 as usize].clone();
    vdbe_mem_set_null(&mut p_out.borrow_mut());

    debug_assert!(p_op.p4type == P4_TABLEREF);

    let p_tab = match &p_op.p4 {
        P4Value::Tab(t) => t.as_ref(),
        _ => {
            p.rc = SQLITE_ERROR;
            return OpFlow::AbortDueToError;
        }
    };

    if p_tab.is_none() {
        p.rc = SQLITE_ERROR;
        return OpFlow::AbortDueToError;
    }

    let tab_ref = p_tab.as_ref().unwrap();
    debug_assert!(tab_ref.n_tab_ref > 0);
    debug_assert!(is_virtual(tab_ref));

    if tab_ref.u_vtab.p.is_none() {
        return OpFlow::Next;
    }

    let p_v_tab_ptr = &tab_ref.u_vtab.p;
    if p_v_tab_ptr.is_none() {
        return OpFlow::Next;
    }

    let p_v_tab = p_v_tab_ptr.as_ref().unwrap();

    if p_v_tab.p_module.is_none() {
        return OpFlow::Next;
    }

    let p_module = p_v_tab.p_module.as_ref().unwrap();
    debug_assert!(p_module.i_version >= 4);
    debug_assert!(p_module.x_integrity.is_some());

    vtab_lock(p_v_tab_ptr.as_ref());

    debug_assert!(p_op.p1 >= 0);
    let db = vdbe_db(p);
    debug_assert!((p_op.p1 as usize) < db.borrow().a_db.len());

    let z_db_s_name = db.borrow().a_db[p_op.p1 as usize].z_db_s_name.clone();
    let mut z_err: Option<String> = None;

    let mut rc = SQLITE_OK;
    if let Some(x_integrity) = &p_module.x_integrity {
        rc = x_integrity(
            p_v_tab,
            z_db_s_name.as_deref(),
            tab_ref.z_name.as_deref(),
            p_op.p3,
            &mut z_err,
        );
    }

    vtab_unlock(p_v_tab_ptr.as_ref());

    if rc != 0 {
        p.rc = rc;
        return OpFlow::AbortDueToError;
    }

    if let Some(err) = z_err {
        vdbe_mem_set_str(&mut p_out.borrow_mut(), err.as_bytes(), -1, SQLITE_UTF8, None);
    }

    OpFlow::Next
}

#[cfg(not(feature = "sqlite_omit_virtualtable"))]
/// Opcode: VInitIn P1 P2 P3 * *
/// Sinopse: r[P2]=ValueList(P1,P3)
///
/// Define o registro P2 para ser um ponteiro para um objeto ValueList para
/// o cursor P1 com registro de cache P3 e registro de saída P3+1. Este
/// objeto ValueList pode ser usado como o primeiro argumento para
/// sqlite3_vtab_in_first() e sqlite3_vtab_in_next() para extrair todos os
/// valores armazenados no cursor P1. O registro P3 é usado para guardar os
/// valores retornados por sqlite3_vtab_in_first() e sqlite3_vtab_in_next().
pub fn op_v_init_in(p: &mut Vdbe, p_op: &VdbeOp) -> OpFlow {
    let p_c = p.ap_csr[p_op.p1 as usize].clone().unwrap();
    let mut value_list = ValueList::new();
    if let VdbeCursorCursorUnion::PCursor(cursor) = &p_c.borrow().uc {
        value_list.p_csr = Some(cursor.clone());
    }
    value_list.p_out = p.a_mem[p_op.p3 as usize].clone();
    let p_rhs = Box::new(value_list);

    let p_out = out2_prerelease(p, p_op);
    p_out.borrow_mut().flags = MEM_NULL;
    vdbe_mem_set_pointer(&mut p_out.borrow_mut(), p_rhs, "ValueList", Some(vdbe_value_list_free));

    OpFlow::Next
}

#[cfg(not(feature = "sqlite_omit_virtualtable"))]
/// Opcode: VFilter P1 P2 P3 P4 *
/// Sinopse: iplan=r[P3] zplan='P4'
///
/// P1 é um cursor aberto usando VOpen. P2 é um endereço para o qual saltar
/// se o conjunto de resultados filtrado estiver vazio.
///
/// P4 é NULL ou uma string que foi gerada pelo método xBestIndex do módulo.
/// A interpretação da string P4 é deixada para a implementação do módulo.
///
/// Este opcode invoca o método xFilter na tabela virtual especificada por P1.
/// O parâmetro inteiro do plano de consulta para xFilter é armazenado no
/// registro P3. O registro P3+1 armazena o parâmetro argc a ser passado para
/// o método xFilter. Os registros P3+2..P3+1+argc são os argumentos argc
/// adicionais que são passados para xFilter como argv. O registro P3+2 vira
/// argv[0] quando passado para xFilter.
///
/// Um salto é feito para P2 se o conjunto de resultados após filtragem estaria
/// vazio.
pub fn op_v_filter(p: &mut Vdbe, p_op: &VdbeOp) -> OpFlow {
    let p_query = p.a_mem[p_op.p3 as usize].clone();
    let p_argc = p.a_mem[(p_op.p3 + 1) as usize].clone();

    let p_cur = p.ap_csr[p_op.p1 as usize].clone().unwrap();
    debug_assert!(p_cur.borrow().e_cur_type == CURTYPE_VTAB);

    debug_assert!((p_query.borrow().flags & MEM_INT) != 0 && p_argc.borrow().flags == MEM_INT);
    let n_arg = p_argc.borrow().u.i as i32;
    let i_query = p_query.borrow().u.i as i32;

    // apArg[i] = &pArgc[i+1]: os argumentos começam em P3+2.
    for i in 0..n_arg {
        p.ap_arg[i as usize] = p.a_mem[(p_op.p3 + 2 + i) as usize].clone();
    }

    let z_plan = match &p_op.p4 {
        P4Value::Z(s) => s.as_deref(),
        _ => None,
    };

    let mut cur = p_cur.borrow_mut();
    let rc = match &mut cur.uc {
        VdbeCursorCursorUnion::PVCur(v_cur) => {
            let v_tab = v_cur.p_v_tab.clone();
            let rc = v_tab.p_module.x_filter(v_cur, i_query, z_plan, n_arg, &p.ap_arg);
            vtab_import_errmsg(p, &v_tab);
            rc
        }
        _ => unreachable!(),
    };
    if rc != 0 {
        p.rc = rc;
        return OpFlow::AbortDueToError;
    }
    let res = match &cur.uc {
        VdbeCursorCursorUnion::PVCur(v_cur) => v_cur.p_v_tab.p_module.x_eof(v_cur),
        _ => unreachable!(),
    };
    cur.null_row = 0;

    if res != 0 {
        return OpFlow::JumpToP2;
    }
    OpFlow::Next
}


// ---- part_021.rs ----

/// Opcode: VColumn P1 P2 P3 * P5
/// Synopsis: r[P3]=vcolumn(P2)
///
/// Armazena no registro P3 o valor da coluna P2 da linha atual da tabela
/// virtual do cursor P1.
///
/// Se o opcode VColumn está sendo usado para buscar o valor de uma coluna
/// imutável durante uma operação UPDATE, então o valor P5 é OPFLAG_NOCHNG.
/// Isso fará com que a função sqlite3_vtab_nochange() retorne verdadeiro
/// dentro do método xColumn da implementação da tabela virtual. A coluna P5
/// também pode conter outros bits (OPFLAG_LENGTHARG ou OPFLAG_TYPEOFARG),
/// mas esses bits não são usados por OP_VColumn.
pub fn exec_op_v_column(
    p: &mut Vdbe,
    p_op: &VdbeOp,
) -> i32 {
    let p_cur = p.ap_csr[p_op.p1 as usize].clone();
    debug_assert!(p_cur.is_some());
    debug_assert!(p_op.p3 > 0 && p_op.p3 <= (p.n_mem + 1 - p.n_cursor) as i32);

    let p_dest = p.a_mem[p_op.p3 as usize].clone();
    let p_dest = &p_dest;
    mem_about_to_change(p, p_dest);

    let p_cur_ref = p_cur.as_ref().unwrap();
    let p_cur_borrow = p_cur_ref.borrow();

    if p_cur_borrow.null_row != 0 {
        {
            let mut p_dest_mut = p_dest.borrow_mut();
            vdbe_mem_set_null(&mut p_dest_mut);
        }
        return SQLITE_OK;
    }

    debug_assert_eq!(p_cur_borrow.e_cur_type, CURTYPE_VTAB);

    let (p_vtab, x_column_fn) = if let VdbeCursorCursorUnion::PVCur(p_vcur) = &p_cur_borrow.uc {
        let p_vtab_ref = &p_vcur.p_vtab;
        (p_vtab_ref, &p_vtab_ref.p_module.x_column)
    } else {
        unreachable!()
    };

    debug_assert!(x_column_fn.is_some());

    let mut s_context = sqlite3_context {
        p_out: p_dest.clone(),
        enc: p.enc,
        is_error: 0,
        p_func: None,
        ..Default::default()
    };

    let mut null_func = FuncDef::default();
    null_func.func_flags = SQLITE_RESULT_SUBTYPE;
    s_context.p_func = Some(Rc::new(null_func));

    if (p_op.p5 & OPFLAG_NOCHNG) != 0 {
        let mut p_dest_mut = p_dest.borrow_mut();
        vdbe_mem_set_null(&mut p_dest_mut);
        p_dest_mut.flags = MEM_NULL | MEM_ZERO;
        p_dest_mut.u.n_zero = 0;
    } else {
        let mut p_dest_mut = p_dest.borrow_mut();
        mem_set_type_flag(&mut p_dest_mut, MEM_NULL);
    }

    let mut rc = SQLITE_OK;

    if let Some(x_column) = x_column_fn {
        let p_vcur = if let VdbeCursorCursorUnion::PVCur(p_vcur) = &p_cur_borrow.uc {
            p_vcur
        } else {
            unreachable!()
        };
        rc = x_column(p_vcur, &mut s_context, p_op.p2);
    }

    vtab_import_errmsg(p, p_vtab);

    if s_context.is_error > 0 {
        let p_dest_borrow = p_dest.borrow();
        let err_text = sqlite3_value_text(&p_dest_borrow);
        vdbe_error(p, "%s", &err_text);
        rc = s_context.is_error;
    }

    {
        let mut p_dest_mut = p_dest.borrow_mut();
        vdbe_change_encoding(&mut p_dest_mut, p.enc);
    }

    drop(p_cur_borrow);

    register_trace(p_op.p3, p_dest);
    update_max_blobsize(p_dest);

    if rc != SQLITE_OK {
        return rc;
    }

    SQLITE_OK
}

/// Resultado de OP_VNext: o que o laço principal deve fazer depois.
pub enum VNextFlow {
    /// `break` simples (cursor em linha nula).
    Next,
    /// `goto jump_to_p2_and_check_for_interrupt`.
    JumpToP2CheckInterrupt,
    /// `goto check_for_interrupt`.
    CheckInterrupt,
    /// `goto abort_due_to_error` com o código de erro.
    Abort(i32),
}

/// Opcode: VNext P1 P2 * * *
///
/// Avança a tabela virtual P1 para a próxima linha em seu conjunto de
/// resultados e salta para a instrução P2. Ou, se a tabela virtual chegou
/// ao final de seu conjunto de resultados, executa a instrução seguinte.
pub fn exec_op_v_next(
    p: &mut Vdbe,
    p_op: &VdbeOp,
) -> VNextFlow {
    let p_cur = &p.ap_csr[p_op.p1 as usize];
    debug_assert!(p_cur.is_some());

    let p_cur_ref = p_cur.as_ref().unwrap();
    let p_cur_borrow = p_cur_ref.borrow();

    if p_cur_borrow.null_row != 0 {
        return VNextFlow::Next;
    }

    debug_assert_eq!(p_cur_borrow.e_cur_type, CURTYPE_VTAB);

    let (p_vtab, x_next_fn, x_eof_fn) = if let VdbeCursorCursorUnion::PVCur(p_vcur) = &p_cur_borrow.uc {
        let p_vtab_ref = &p_vcur.p_vtab;
        (p_vtab_ref, &p_vtab_ref.p_module.x_next, &p_vtab_ref.p_module.x_eof)
    } else {
        unreachable!()
    };

    debug_assert!(x_next_fn.is_some());

    let mut rc = SQLITE_OK;

    if let Some(x_next) = x_next_fn {
        let p_vcur = if let VdbeCursorCursorUnion::PVCur(p_vcur) = &p_cur_borrow.uc {
            p_vcur
        } else {
            unreachable!()
        };
        rc = x_next(p_vcur);
    }

    vtab_import_errmsg(p, p_vtab);

    if rc != SQLITE_OK {
        return VNextFlow::Abort(rc);
    }

    let res = if let Some(x_eof) = x_eof_fn {
        let p_vcur = if let VdbeCursorCursorUnion::PVCur(p_vcur) = &p_cur_borrow.uc {
            p_vcur
        } else {
            unreachable!()
        };
        x_eof(p_vcur)
    } else {
        0
    };

    vdbe_branch_taken(res == 0, 2);

    drop(p_cur_borrow);

    if res == 0 {
        VNextFlow::JumpToP2CheckInterrupt
    } else {
        VNextFlow::CheckInterrupt
    }
}

/// Opcode: VRename P1 * * P4 *
///
/// P4 é um ponteiro para um objeto de tabela virtual, uma estrutura
/// sqlite3_vtab. Este opcode invoca o método xRename correspondente.
/// O valor no registro P1 é passado como argumento zName para o método
/// xRename.
pub fn exec_op_v_rename(
    p: &mut Vdbe,
    p_op: &VdbeOp,
) -> i32 {
    let db = vdbe_db(p);

    let is_legacy = (db.borrow().flags & SQLITE_LEGACY_ALTER) != 0;
    db.borrow_mut().flags |= SQLITE_LEGACY_ALTER;

    let p_name = p.a_mem[p_op.p1 as usize].clone();
    debug_assert!(mem_is_valid(&p_name));
    debug_assert_eq!(p.read_only, 0);

    register_trace(p_op.p1, &p_name);

    debug_assert!((p_name.borrow().flags & MEM_STR) != 0);

    let mut rc = vdbe_change_encoding(&p_name, SQLITE_UTF8);

    if rc == SQLITE_OK {
        let p_name_z = p_name.borrow().z.clone();

        let p_vtab_ref_opt = p_op.p4.as_vtab();
        if let Some(p_vtab_ref) = p_vtab_ref_opt {
            let p_vtab = &p_vtab_ref.p_vtab;

            if let Some(x_rename) = &p_vtab.p_module.x_rename {
                rc = x_rename(p_vtab, &p_name_z);
            }

            if !is_legacy {
                db.borrow_mut().flags &= !(SQLITE_LEGACY_ALTER as u64);
            }
            vtab_import_errmsg(p, p_vtab);
            p.expired = 0;
        }
    }

    rc
}

/// Opcode: VUpdate P1 P2 P3 P4 P5
/// Synopsis: data=r[P3@P2]
///
/// P4 é um ponteiro para um objeto de tabela virtual, uma estrutura
/// sqlite3_vtab. Este opcode invoca o método xUpdate correspondente. P2
/// valores são células de memória contíguas começando em P3 para passar
/// para a invocação xUpdate. O valor no registro (P3+P2-1) corresponde ao
/// elemento p2 do array argv passado para xUpdate.
///
/// O método xUpdate fará um DELETE ou um INSERT ou ambos. O elemento
/// argv[0] (que corresponde à célula de memória P3) é o rowid de uma linha
/// a deletar. Se argv[0] é NULL, nenhuma deleção ocorre. O elemento argv[1]
/// é o rowid da nova linha. Isso pode ser NULL para que a tabela virtual
/// selecione o novo rowid por si mesma. Os elementos subsequentes no array
/// são os valores das colunas na nova linha.
///
/// Se P2==1, nenhuma inserção é realizada. argv[0] é o rowid de uma linha
/// a deletar.
///
/// P1 é um sinalizador booleano. Se estiver definido como verdadeiro e a
/// chamada xUpdate for bem sucedida, então o valor retornado por
/// sqlite3_last_insert_rowid() é definido como o valor do rowid da linha
/// recém inserida.
///
/// P5 são as ações de erro (OE_Replace, OE_Fail, OE_Ignore, etc) a
/// aplicar no caso de falha de restrição em uma inserção ou atualização.
pub fn exec_op_v_update(
    p: &mut Vdbe,
    p_op: &VdbeOp,
) -> i32 {
    debug_assert!(
        p_op.p2 == 1
            || p_op.p5 == OE_FAIL
            || p_op.p5 == OE_ROLLBACK
            || p_op.p5 == OE_ABORT
            || p_op.p5 == OE_IGNORE
            || p_op.p5 == OE_REPLACE
    );
    debug_assert_eq!(p.read_only, 0);

    let db = vdbe_db(p);

    if db.borrow().malloc_failed != 0 {
        return SQLITE_NOMEM;
    }

    vdbe_incr_write_counter(p, 0);

    let p_vtab_ref_opt = p_op.p4.as_vtab();
    if p_vtab_ref_opt.is_none() {
        return SQLITE_LOCKED;
    }

    let p_vtab_ref = p_vtab_ref_opt.unwrap();
    let p_vtab = &p_vtab_ref.p_vtab;

    if p_vtab.p_module.is_none() {
        return SQLITE_LOCKED;
    }

    debug_assert_eq!(p_op.p4type, P4_VTAB);

    let p_module = p_vtab.p_module.as_ref().unwrap();
    let n_arg = p_op.p2;

    if p_module.x_update.is_none() {
        return SQLITE_OK;
    }

    let vtab_on_conflict = db.borrow().vtab_on_conflict;

    let mut rowid: i64 = 0;

    for i in 0..n_arg {
        let p_x = p.a_mem[(p_op.p3 as usize) + i as usize].clone();
        debug_assert!(mem_is_valid(&p_x));
        mem_about_to_change(p, &p_x);
        p.ap_arg[i as usize] = p_x;
    }

    db.borrow_mut().vtab_on_conflict = p_op.p5;

    let mut rc = SQLITE_OK;

    if let Some(x_update) = &p_module.x_update {
        rc = x_update(p_vtab, n_arg as usize, &p.ap_arg, &mut rowid);
    }

    db.borrow_mut().vtab_on_conflict = vtab_on_conflict;

    vtab_import_errmsg(p, p_vtab);

    if rc == SQLITE_OK && p_op.p1 != 0 {
        debug_assert!(n_arg > 1);
        debug_assert!((p.ap_arg[0].borrow().flags & MEM_NULL) != 0);
        db.borrow_mut().last_rowid = rowid;
    }

    if (rc & 0xff) == SQLITE_CONSTRAINT && p_vtab_ref.b_constraint != 0 {
        if p_op.p5 == OE_IGNORE {
            rc = SQLITE_OK;
        } else {
            p.error_action = if p_op.p5 == OE_REPLACE {
                OE_ABORT
            } else {
                p_op.p5
            };
        }
    } else {
        p.n_change += 1;
    }

    rc
}

/// Opcode: Pagecount P1 P2 * * *
///
/// Escreve o número atual de páginas do banco de dados P1 na célula de
/// memória P2.
pub fn exec_op_pagecount(
    p: &mut Vdbe,
    p_op: &VdbeOp,
) -> i32 {
    let db = vdbe_db(p);

    let p_out = out2_prerelease(p, p_op);

    let last_page = btree_last_page(&db.borrow().a_db[p_op.p1 as usize].p_bt);

    p_out.borrow_mut().u.i = last_page as i64;

    SQLITE_OK
}

/// Opcode: MaxPgcnt P1 P2 P3 * *
///
/// Tenta definir a contagem máxima de páginas para o banco de dados P1
/// como o valor em P3. Não deixe a contagem máxima de páginas cair abaixo
/// da contagem atual de páginas e não mude o valor de contagem máxima de
/// páginas se P3==0.
///
/// Armazena a contagem máxima de páginas após a mudança no registro P2.
pub fn exec_op_max_pgcnt(
    p: &mut Vdbe,
    p_op: &VdbeOp,
) -> i32 {
    let db = vdbe_db(p);

    let p_out = out2_prerelease(p, p_op);

    let p_bt = db.borrow().a_db[p_op.p1 as usize].p_bt.clone();
    let p_bt = &p_bt;

    let mut new_max: u32 = 0;

    if p_op.p3 != 0 {
        let last_page = btree_last_page(p_bt);
        new_max = last_page;
        if new_max < (p_op.p3 as u32) {
            new_max = p_op.p3 as u32;
        }
    }

    let max_count = btree_max_page_count(p_bt, new_max);

    let mut p_out_mut = p_out.borrow_mut();
    p_out_mut.u.i = max_count as i64;

    SQLITE_OK
}

/// Opcode: Function P1 P2 P3 P4 *
/// Synopsis: r[P3]=func(r[P2@NP])
///
/// Invoca uma função de usuário (P4 é um ponteiro para um objeto
/// sqlite3_context que contém um ponteiro para a função a ser executada)
/// com argumentos tirados do registro P2 e sucessores. O número de argumentos
/// está no objeto sqlite3_context que P4 aponta. O resultado da função é
/// armazenado no registro P3. O registro P3 não deve ser uma das entradas
/// de função.
///
/// P1 é uma máscara de bits de 32 bits indicando se cada argumento para a
/// função foi determinado como constante em tempo de compilação. Se o
/// primeiro argumento era constante, o bit 0 de P1 é definido. Isso é usado
/// para determinar se metadados associados a um argumento de função de
/// usuário usando a API sqlite3_set_auxdata() podem ser retidos com segurança
/// até a próxima invocação deste opcode.
///
/// Ver também: AggStep, AggFinal, PureFunc
///
/// OP_PureFunc é o mesmo código: o despachante trata `OP_PURE_FUNC | OP_FUNCTION`
/// no mesmo braço (no C os dois `case` compartilham o corpo), sem função de repasse.
pub fn exec_op_function(
    p: &mut Vdbe,
    p_op: &VdbeOp,
) -> i32 {
    let p_ctx_opt = p_op.p4.as_func_ctx();
    if p_ctx_opt.is_none() {
        return SQLITE_OK;
    }

    let p_ctx = p_ctx_opt.unwrap();
    let p_out = p.a_mem[p_op.p3 as usize].clone();

    {
        let p_ctx_borrow = p_ctx.borrow();
        if p_ctx_borrow.p_out.as_ptr() != p_out.as_ptr() {
            drop(p_ctx_borrow);

            let mut p_ctx_mut = p_ctx.borrow_mut();
            p_ctx_mut.p_vdbe = p.get_weak_self();
            p_ctx_mut.p_out = p_out.clone();
            p_ctx_mut.enc = p.enc;

            for i in (0..p_ctx_mut.argc).rev() {
                p_ctx_mut.argv[i as usize] = p.a_mem[(p_op.p2 as usize) + i as usize].clone();
            }
        }
    }

    {
        let p_ctx_borrow = p_ctx.borrow();
        debug_assert!(p_ctx_borrow.p_vdbe.is_some());
    }

    mem_about_to_change(p, &p_out);

    {
        let p_ctx_borrow = p_ctx.borrow();
        for i in 0..p_ctx_borrow.argc {
            debug_assert!(mem_is_valid(&p_ctx_borrow.argv[i as usize]));
            register_trace(p_op.p2 + i, &p_ctx_borrow.argv[i as usize]);
        }
    }

    {
        let mut p_out_mut = p_out.borrow_mut();
        mem_set_type_flag(&mut p_out_mut, MEM_NULL);
    }

    // Chama a função sem segurar o empréstimo do contexto: ela escreve nele.
    let (x_s_func, argc, argv) = {
        let p_ctx_borrow = p_ctx.borrow();
        debug_assert_eq!(p_ctx_borrow.is_error, 0);
        (
            p_ctx_borrow.p_func.as_ref().and_then(|f| f.x_s_func),
            p_ctx_borrow.argc,
            p_ctx_borrow.argv.clone(),
        )
    };
    if let Some(x_s_func) = x_s_func {
        x_s_func(p_ctx, argc, &argv);
    }

    let mut rc = SQLITE_OK;

    // Se a função devolveu erro, lança a exceção.
    let is_error = p_ctx.borrow().is_error;
    if is_error != 0 {
        if is_error > 0 {
            let err_text = sqlite3_value_text(&p_out.borrow());
            vdbe_error(p, "%s", &err_text);
            rc = is_error;
        }

        let i_op = p_ctx.borrow().i_op;
        let db = vdbe_db(p);
        vdbe_delete_aux_data(&mut db.borrow_mut(), &mut p.p_aux_data, i_op, p_op.p1);

        p_ctx.borrow_mut().is_error = 0;
        if rc != 0 {
            return rc;
        }
    }

    {
        let p_out_borrow = p_out.borrow();
        debug_assert!(
            (p_out_borrow.flags & MEM_STR) == 0
                || p_out_borrow.enc == p.enc
                || vdbe_db(p).borrow().malloc_failed != 0
        );
        debug_assert!(!vdbe_mem_too_big(&p_out_borrow));
    }

    register_trace(p_op.p3, &p_out);
    update_max_blobsize(&p_out);

    SQLITE_OK
}

