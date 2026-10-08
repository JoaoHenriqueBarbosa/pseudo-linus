// Mesclado das partes traduzidas de vdbeaux_c (as partes quebram funções grandes no meio).
#![allow(unused_imports, ambiguous_glob_reexports)]
use crate::prelude::*;

// ---- part_000.rs ----


// Este trecho de vdbeaux.c contém o código usado para criar, destruir e popular uma VDBE (ou um
// "sqlite3_stmt", como ela é conhecida no mundo externo).
//
// Opções do Debian 13 resolvidas neste trecho: SQLITE_ENABLE_NORMALIZE, SQLITE_ENABLE_EXPLAIN_COMMENTS,
// SQLITE_ENABLE_STMT_SCANSTATUS, VDBE_PROFILE, SQLITE_VDBE_COVERAGE, SQLITE_TEST_REALLOC_STRESS e
// SQLITE_DEBUG não estão ligadas, então `vdbe_add_dblquote_str`, `vdbe_uses_double_quoted_string`,
// `test_addop_breakpoint`, o rastreio de AddOp e os campos opcionais de VdbeOp não existem aqui.
//
// As referências adiante do C (`freeEphemeralFunction`, `vdbeFreeOpArray`) são só protótipos e
// não geram código; cada função é traduzida no trecho onde é definida.

/// Cria uma nova máquina virtual de banco de dados.
pub fn vdbe_create(p_parse: &ParseRef) -> Option<VdbeRef> {
    let db = p_parse
        .borrow()
        .db
        .upgrade()
        .expect("a conexão precisa estar viva enquanto o Parse existe");
    // sqlite3DbMallocRawNN devolve 0 quando db->mallocFailed já está ligado.
    if db.borrow().malloc_failed != 0 {
        return None;
    }
    // O memset do C zera tudo a partir de aOp; os campos anteriores são inicializados por
    // vdbe_make_ready() antes do uso, aqui nascem zerados.
    let p: VdbeRef = Rc::new(RefCell::new(Vdbe {
        db: Rc::downgrade(&db),
        pp_v_prev: None,
        p_v_next: None,
        p_parse: Some(Rc::downgrade(p_parse)),
        n_var: 0,
        n_mem: 0,
        n_cursor: 0,
        cache_ctr: 0,
        pc: 0,
        rc: 0,
        n_change: 0,
        i_statement: 0,
        i_current_time: 0,
        n_fk_constraint: 0,
        n_stmt_def_cons: 0,
        n_stmt_def_imm_cons: 0,
        a_mem: Vec::new(),
        ap_arg: Vec::new(),
        ap_csr: Vec::new(),
        a_var: Vec::new(),
        a_op: Vec::new(),
        n_op: 0,
        n_op_alloc: 0,
        a_col_name: Vec::new(),
        p_result_row: None,
        z_err_msg: None,
        p_v_list: None,
        start_time: 0,
        n_res_column: 0,
        n_res_alloc: 0,
        error_action: 0,
        min_write_file_format: 0,
        prep_flags: 0,
        e_vdbe_state: 0,
        expired: 0,
        explain: 0,
        change_cnt_on: 0,
        uses_stmt_journal: 0,
        read_only: 0,
        b_is_reader: 0,
        have_eqp_ops: 0,
        btree_mask: 0,
        lock_mask: 0,
        a_counter: [0; 9],
        z_sql: None,
        p_frame: None,
        p_del_frame: None,
        n_frame: 0,
        expmask: 0,
        p_program: None,
        p_aux_data: None,
    }));
    {
        // Encaixa a nova VM na cabeça da lista db->pVdbe. O ppVPrev do C (ponteiro para o
        // ponteiro que aponta para este nó) é `None` quando o elo anterior é o próprio db e
        // `Some(nó anterior)` caso contrário.
        let mut db_mut = db.borrow_mut();
        if let Some(old) = db_mut.p_vdbe.take() {
            old.borrow_mut().pp_v_prev = Some(Rc::downgrade(&p));
            p.borrow_mut().p_v_next = Some(old);
        }
        db_mut.p_vdbe = Some(p.clone());
    }
    debug_assert!(p.borrow().e_vdbe_state == VDBE_INIT_STATE);
    {
        let mut parse = p_parse.borrow_mut();
        parse.p_vdbe = Some(p.clone());
        debug_assert!(parse.a_label.is_empty());
        debug_assert!(parse.n_label == 0);
        debug_assert!(p.borrow().n_op_alloc == 0);
        debug_assert!(parse.sz_op_alloc == 0);
    }
    vdbe_add_op2(&mut p.borrow_mut(), OP_INIT as i32, 0, 1);
    Some(p)
}

/// Devolve o objeto Parse dono de um objeto Vdbe.
pub fn vdbe_parser(p: &Vdbe) -> Option<ParseRef> {
    p.p_parse.as_ref().and_then(|w| w.upgrade())
}

/// Muda a string de erro guardada em Vdbe.zErrMsg.
pub fn vdbe_error(p: &mut Vdbe, z_format: &[u8], ap: &[Value]) {
    let db = p
        .db
        .upgrade()
        .expect("a conexão precisa estar viva enquanto a VM existe");
    p.z_err_msg = None;
    p.z_err_msg = vm_printf(&db, z_format, ap);
}

/// Lembra a string SQL de uma declaração preparada.
pub fn vdbe_set_sql(p: Option<&mut Vdbe>, z: &[u8], n: i32, prep_flags: u8) {
    let p = match p {
        Some(p) => p,
        None => return,
    };
    p.prep_flags = prep_flags;
    if (prep_flags as u32 & SQLITE_PREPARE_SAVESQL) == 0 {
        p.expmask = 0;
    }
    debug_assert!(p.z_sql.is_none());
    let db = p
        .db
        .upgrade()
        .expect("a conexão precisa estar viva enquanto a VM existe");
    p.z_sql = db_str_n_dup(&db, z, n as u64);
}

/// Troca o bytecode entre duas estruturas VDBE.
///
/// Isto acontece depois que pB foi executada e devolveu SQLITE_SCHEMA. A declaração foi então
/// repreparada em pA. Esta rotina transfere o bytecode novo de pA para pB, para que pB possa
/// rodar de novo. O bytecode antigo de pB volta para pA, para ser limpo quando pA for
/// finalizada.
pub fn vdbe_swap(p_a: &mut Vdbe, p_b: &mut Vdbe) {
    debug_assert!(std::rc::Weak::ptr_eq(&p_a.db, &p_b.db));
    std::mem::swap(p_a, p_b);
    std::mem::swap(&mut p_a.p_v_next, &mut p_b.p_v_next);
    std::mem::swap(&mut p_a.pp_v_prev, &mut p_b.pp_v_prev);
    std::mem::swap(&mut p_a.z_sql, &mut p_b.z_sql);
    p_b.expmask = p_a.expmask;
    p_b.prep_flags = p_a.prep_flags;
    p_b.a_counter = p_a.a_counter;
    let i = SQLITE_STMTSTATUS_REPREPARE as usize;
    p_b.a_counter[i] = p_b.a_counter[i].wrapping_add(1);
}

/// Redimensiona o array Vdbe.aOp para que fique pelo menos nOp elementos maior que o tamanho
/// atual. nOp é garantidamente menor ou igual a 1024/sizeof(Op).
///
/// Se ocorrer um erro de falta de memória ao redimensionar, devolve SQLITE_NOMEM. Neste caso
/// Vdbe.aOp e Vdbe.nOpAlloc ficam inalterados (para que os opcodes já alocados possam ser
/// desalocados corretamente junto com o resto do Vdbe).
fn grow_op_array(v: &mut Vdbe, n_op: i32) -> i32 {
    // sizeof(Op) do C sem EXPLAIN_COMMENTS, STMT_SCANSTATUS e VDBE_PROFILE: opcode, p4type,
    // p5, p1, p2, p3 e a união p4 somam 24 bytes.
    const SIZEOF_OP: i64 = 24;
    // Limite de sqlite3Realloc(): pedidos a partir daqui falham.
    const MAX_ALLOC: i64 = 0x7fffff00;

    let p_parse = v
        .p_parse
        .as_ref()
        .and_then(|w| w.upgrade())
        .expect("o Parse precisa estar vivo enquanto o programa é construído");
    let db = p_parse
        .borrow()
        .db
        .upgrade()
        .expect("a conexão precisa estar viva enquanto o Parse existe");

    // Sem SQLITE_TEST_REALLOC_STRESS: dobra o tamanho atual do array de ops, ou aloca 1KB de
    // espaço na primeira vez. O parâmetro nOp só é usado nas asserções.
    let n_new: i64 = if v.n_op_alloc != 0 {
        2 * v.n_op_alloc as i64
    } else {
        1024 / SIZEOF_OP
    };

    // Garante que o tamanho de uma VDBE não cresça demais.
    if n_new > db.borrow().a_limit[SQLITE_LIMIT_VDBE_OP as usize] as i64 {
        oom_fault(&db);
        return SQLITE_NOMEM;
    }

    debug_assert!(n_op as i64 <= 1024 / SIZEOF_OP);
    debug_assert!(n_new >= v.n_op_alloc as i64 + n_op as i64);

    // sqlite3DbRealloc: não tenta nada se mallocFailed já está ligado, falha para pedidos de
    // 0x7fffff00 bytes ou mais, e chama sqlite3OomFault quando a alocação falha.
    let n_bytes = n_new * SIZEOF_OP;
    let failed_before = db.borrow().malloc_failed != 0;
    let target = n_new as usize;
    let grown = !failed_before
        && n_bytes < MAX_ALLOC
        && v.a_op
            .try_reserve_exact(target.saturating_sub(v.a_op.len()))
            .is_ok();
    if !grown {
        if !failed_before {
            oom_fault(&db);
        }
        return SQLITE_NOMEM_BKPT;
    }
    // As posições novas nascem como instruções zeradas (o C deixa o conteúdo indefinido).
    v.a_op.resize_with(target, || VdbeOp {
        opcode: 0,
        p4type: P4_NOTUSED,
        p5: 0,
        p1: 0,
        p2: 0,
        p3: 0,
        p4: P4Value::NotUsed,
    });
    // sqlite3DbMallocSize devolve exatamente n_new*sizeof(Op) para estes tamanhos (múltiplos
    // de 24 bytes cabem justos no bloco utilizável do malloc), logo nOpAlloc vale n_new.
    let sz_op_alloc = (n_new * SIZEOF_OP) as i32;
    p_parse.borrow_mut().sz_op_alloc = sz_op_alloc;
    v.n_op_alloc = sz_op_alloc / SIZEOF_OP as i32;
    SQLITE_OK
}

/// Caminhos lentos de vdbe_add_op3() e vdbe_add_op4_int() para o caso incomum em que é preciso
/// aumentar o tamanho do array Vdbe.aOp[] antes de acrescentar o novo opcode.
fn grow_op3(p: &mut Vdbe, op: i32, p1: i32, p2: i32, p3: i32) -> i32 {
    debug_assert!(p.n_op_alloc <= p.n_op);
    if grow_op_array(p, 1) != 0 {
        return 1;
    }
    debug_assert!(p.n_op_alloc > p.n_op);
    vdbe_add_op3(p, op, p1, p2, p3)
}

fn add_op4_int_slow(
    p: &mut Vdbe, // Acrescenta o opcode a esta VM
    op: i32,      // O novo opcode
    p1: i32,      // O operando P1
    p2: i32,      // O operando P2
    p3: i32,      // O operando P3
    p4: i32,      // O operando P4 como inteiro
) -> i32 {
    let addr = vdbe_add_op3(p, op, p1, p2, p3);
    let db = p
        .db
        .upgrade()
        .expect("a conexão precisa estar viva enquanto a VM existe");
    let malloc_failed = db.borrow().malloc_failed;
    if malloc_failed == 0 {
        let p_op = &mut p.a_op[addr as usize];
        p_op.p4type = P4_INT32;
        p_op.p4 = P4Value::Int32(p4);
    }
    addr
}

/// Acrescenta uma nova instrução à lista de instruções atuais da VDBE. Devolve o endereço da
/// nova instrução.
///
/// Parâmetros:
///
///    p               A VDBE
///
///    op              O opcode desta instrução
///
///    p1, p2, p3, p4  Operandos
pub fn vdbe_add_op0(p: &mut Vdbe, op: i32) -> i32 {
    vdbe_add_op3(p, op, 0, 0, 0)
}

pub fn vdbe_add_op1(p: &mut Vdbe, op: i32, p1: i32) -> i32 {
    vdbe_add_op3(p, op, p1, 0, 0)
}

pub fn vdbe_add_op2(p: &mut Vdbe, op: i32, p1: i32, p2: i32) -> i32 {
    vdbe_add_op3(p, op, p1, p2, 0)
}

pub fn vdbe_add_op3(p: &mut Vdbe, op: i32, p1: i32, p2: i32, p3: i32) -> i32 {
    let i = p.n_op;
    debug_assert!(p.e_vdbe_state == VDBE_INIT_STATE);
    debug_assert!(op >= 0 && op < 0xff);
    if p.n_op_alloc <= i {
        return grow_op3(p, op, p1, p2, p3);
    }
    debug_assert!(!p.a_op.is_empty());
    p.n_op += 1;
    let p_op = &mut p.a_op[i as usize];
    p_op.opcode = op as u8;
    p_op.p5 = 0;
    p_op.p1 = p1;
    p_op.p2 = p2;
    p_op.p3 = p3;
    p_op.p4 = P4Value::NotUsed;
    p_op.p4type = P4_NOTUSED;

    // Replicar esta lógica em vdbe_add_op4_int()
    // (sem EXPLAIN_COMMENTS, STMT_SCANSTATUS, VDBE_PROFILE, DEBUG nem VDBE_COVERAGE não há
    // mais nada a inicializar)

    i
}

pub fn vdbe_add_op4_int(
    p: &mut Vdbe, // Acrescenta o opcode a esta VM
    op: i32,      // O novo opcode
    p1: i32,      // O operando P1
    p2: i32,      // O operando P2
    p3: i32,      // O operando P3
    p4: i32,      // O operando P4 como inteiro
) -> i32 {
    let i = p.n_op;
    if p.n_op_alloc <= i {
        return add_op4_int_slow(p, op, p1, p2, p3, p4);
    }
    p.n_op += 1;
    let p_op = &mut p.a_op[i as usize];
    p_op.opcode = op as u8;
    p_op.p5 = 0;
    p_op.p1 = p1;
    p_op.p2 = p2;
    p_op.p3 = p3;
    p_op.p4 = P4Value::Int32(p4);
    p_op.p4type = P4_INT32;

    // Replicar esta lógica em vdbe_add_op3()
    // (sem EXPLAIN_COMMENTS, STMT_SCANSTATUS, VDBE_PROFILE, DEBUG nem VDBE_COVERAGE não há
    // mais nada a inicializar)

    i
}


// ---- part_001.rs ----


// Notas desta parte (vdbeaux.c, parte 1):
//  - Convenção de empréstimo, igual à das outras partes de vdbeaux.c: as rotinas que recebem
//    `Vdbe*` recebem `&mut Vdbe` (ou `&Vdbe` quando só leem) e as que recebem `Parse*` recebem
//    `&mut Parse` (ou `&Parse`). Quando a rotina precisa do outro lado do par (o Vdbe de um
//    Parse, ou o Parse de um Vdbe), sobe a referência fraca/forte e pega o `borrow()` só pelo
//    tempo do uso, nunca durante a chamada a outra rotina que também o pegue.
//  - Os `assert()` do C (compilados só com SQLITE_DEBUG) não são traduzidos.
//  - Falha de alocação não existe em Rust: os ramos `if( p==0 )` que só tratam malloc falho somem,
//    e a liberação que o C faz à mão (`freeEphemeralFunction`) é o `Drop`.
//  - `sqlite3VdbeScanStatus` é macro vazia sem SQLITE_ENABLE_STMT_SCANSTATUS, e `IS_STMT_SCANSTATUS`
//    vale 0, então só `pParse->explain==2` ativa o OP_Explain.
//  - `sqlite3ExplainBreakpoint` (macro vazia fora de SQLITE_DEBUG) e `VdbeOpIter`/`opIterNext`
//    (só com SQLITE_DEBUG) são omitidos, junto com suas chamadas.

/// Argumento variádico de `vdbe_multi_load`: um "s" do `zTypes` consome `Str`, um "i" consome `Int`.
pub enum MultiLoadArg<'a> {
    /// Texto, ou `None` quando o ponteiro do C é nulo (gera OP_Null).
    Str(Option<&'a [u8]>),
    /// Inteiro de 32 bits.
    Int(i32),
}

/// Devolve o `ParseRef` que criou o Vdbe (`v->pParse`).
fn vdbe_parse_ref(v: &Vdbe) -> ParseRef {
    v.p_parse
        .as_ref()
        .and_then(|w| w.upgrade())
        .expect("Vdbe sem Parse associado")
}

/// Gera código para um salto incondicional para a instrução `i_dest`.
pub fn vdbe_goto(p: &mut Vdbe, i_dest: i32) -> i32 {
    vdbe_add_op3(p, OP_GOTO as i32, 0, i_dest, 0)
}

/// Gera código que carrega a string `z_str` no registrador `i_dest`.
pub fn vdbe_load_string(p: &mut Vdbe, i_dest: i32, z_str: &[u8]) -> i32 {
    vdbe_add_op4(
        p,
        OP_STRING8 as i32,
        0,
        i_dest,
        0,
        P4Value::Static(z_str.to_vec()),
        0,
    )
}

/// Gera código que inicializa vários registradores com constantes de texto ou inteiras, a partir
/// de `i_dest`, um por caractere de `z_types`. Cada "s" vira texto (ou OP_Null se o argumento for
/// nulo) e cada "i" vira inteiro. Se `z_types` não termina em "X" (qualquer caractere que não seja
/// "s" nem "i" interrompe a geração), um OP_ResultRow é gerado para os valores inseridos.
pub fn vdbe_multi_load(p: &mut Vdbe, i_dest: i32, z_types: &[u8], args: &[MultiLoadArg]) {
    let mut ap = args.iter();
    'skip_op_resultrow: {
        let mut i: i32 = 0;
        loop {
            // O C termina o laço no byte NUL; aqui também no fim da fatia.
            let c = match z_types.get(i as usize) {
                Some(&c) if c != 0 => c,
                _ => break,
            };
            if c == b's' {
                let z = match ap.next() {
                    Some(MultiLoadArg::Str(z)) => *z,
                    _ => panic!("vdbe_multi_load: argumento 's' ausente ou de outro tipo"),
                };
                match z {
                    Some(z) => {
                        vdbe_add_op4(
                            p,
                            OP_STRING8 as i32,
                            0,
                            i_dest + i,
                            0,
                            P4Value::Static(z.to_vec()),
                            0,
                        );
                    }
                    None => {
                        vdbe_add_op4(p, OP_NULL as i32, 0, i_dest + i, 0, P4Value::NotUsed, 0);
                    }
                }
            } else if c == b'i' {
                let n = match ap.next() {
                    Some(MultiLoadArg::Int(n)) => *n,
                    _ => panic!("vdbe_multi_load: argumento 'i' ausente ou de outro tipo"),
                };
                vdbe_add_op2(p, OP_INTEGER as i32, n, i_dest + i);
            } else {
                break 'skip_op_resultrow;
            }
            i += 1;
        }
        vdbe_add_op2(p, OP_RESULTROW as i32, i_dest, i);
    }
}

/// Adiciona um opcode que inclui o valor P4 como ponteiro.
pub fn vdbe_add_op4(
    p: &mut Vdbe,
    op: i32,
    p1: i32,
    p2: i32,
    p3: i32,
    z_p4: P4Value,
    p4type: i8,
) -> i32 {
    let addr = vdbe_add_op3(p, op, p1, p2, p3);
    vdbe_change_p4(p, addr, z_p4, p4type as i32);
    addr
}

/// Adiciona um OP_Function ou OP_PureFunc. `e_call_ctx` descreve o contexto da chamada (em geral
/// vem de `Expr.op2`): 0 é uma chamada comum, NC_IsCheck vem de uma restrição CHECK, NC_IdxExpr de
/// uma expressão de índice, NC_PartIdx do WHERE de um índice parcial e NC_GenCol do cálculo de uma
/// coluna gerada.
pub fn vdbe_add_function_call(
    p_parse: &mut Parse,
    p1: i32,
    p2: i32,
    p3: i32,
    n_arg: i32,
    p_func: Rc<FuncDef>,
    e_call_ctx: i32,
) -> i32 {
    let v = p_parse
        .p_vdbe
        .clone()
        .expect("vdbe_add_function_call: Parse sem Vdbe");
    let mut vb = v.borrow_mut();
    let i_op = vdbe_current_addr(&vb);
    // O C deixa pOut nulo; aqui o campo não é opcional, então entra um Mem vazio que o
    // OP_Function substitui antes de usar.
    let p_out = Rc::new(RefCell::new(Mem {
        u: MemValue::default(),
        z: Vec::new(),
        n: 0,
        flags: MEM_UNDEFINED,
        enc: 0,
        e_subtype: 0,
        db: None,
        sz_malloc: 0,
        u_temp: 0,
        z_malloc: Vec::new(),
        x_del: None,
    }));
    let p_ctx = Box::new(sqlite3_context {
        p_out,
        p_func,
        p_mem: None,
        p_vdbe: Weak::new(),
        i_op,
        is_error: 0,
        enc: 0,
        skip_flag: 0,
        argc: n_arg as u8,
        argv: Vec::with_capacity(n_arg.max(0) as usize),
    });
    let addr = vdbe_add_op4(
        &mut vb,
        if e_call_ctx != 0 {
            OP_PUREFUNC as i32
        } else {
            OP_FUNCTION as i32
        },
        p1,
        p2,
        p3,
        P4Value::FuncCtx(p_ctx),
        P4_FUNCCTX,
    );
    vdbe_change_p5(&mut vb, (e_call_ctx & NC_SELFREF) as u16);
    drop(vb);
    may_abort(p_parse);
    addr
}

/// Adiciona um opcode que inclui o valor P4 com tipo P4_INT64 ou P4_REAL. `z_p4` são os 8 bytes
/// do valor, na ordem nativa de bytes, como a cópia de 8 bytes do C.
pub fn vdbe_add_op4_dup8(
    p: &mut Vdbe,
    op: i32,
    p1: i32,
    p2: i32,
    p3: i32,
    z_p4: &[u8],
    p4type: i8,
) -> i32 {
    let mut p4copy = [0u8; 8];
    p4copy.copy_from_slice(&z_p4[..8]);
    let p4 = match p4type {
        P4_INT64 => P4Value::Int64(i64::from_ne_bytes(p4copy)),
        P4_REAL => P4Value::Real(f64::from_ne_bytes(p4copy)),
        _ => P4Value::Static(p4copy.to_vec()),
    };
    vdbe_add_op4(p, op, p1, p2, p3, p4, p4type)
}

/// Devolve o endereço da base atual do EXPLAIN QUERY PLAN. 0 significa "nenhuma".
pub fn vdbe_explain_parent(p_parse: &Parse) -> i32 {
    if p_parse.addr_explain == 0 {
        return 0;
    }
    let v = p_parse
        .p_vdbe
        .as_ref()
        .expect("vdbe_explain_parent: Parse sem Vdbe");
    // sqlite3VdbeGetOp(v, addr_explain)->p2, com addr_explain sempre positivo aqui.
    let p2 = v.borrow().a_op[p_parse.addr_explain as usize].p2;
    p2
}

/// Adiciona um novo OP_Explain. Se `b_push` for verdadeiro, este opcode vira o pai dos Explain
/// seguintes até `vdbe_explain_pop()`. A mensagem chega já formatada pelo chamador (o
/// `sqlite3VMPrintf(zFmt, ...)` do C), e o Vdbe fica dono dela (P4_DYNAMIC).
pub fn vdbe_explain(p_parse: &mut Parse, b_push: u8, z_msg: Vec<u8>) -> i32 {
    let mut addr = 0;
    if p_parse.explain == 2 {
        let v = p_parse
            .p_vdbe
            .clone()
            .expect("vdbe_explain: Parse sem Vdbe");
        let mut vb = v.borrow_mut();
        let i_this = vb.n_op;
        addr = vdbe_add_op4(
            &mut vb,
            OP_EXPLAIN as i32,
            i_this,
            p_parse.addr_explain,
            0,
            P4Value::Dynamic(z_msg),
            P4_DYNAMIC,
        );
        if b_push != 0 {
            p_parse.addr_explain = i_this;
        }
    }
    addr
}

/// Desempilha um nível da pilha do EXPLAIN QUERY PLAN.
pub fn vdbe_explain_pop(p_parse: &mut Parse) {
    p_parse.addr_explain = vdbe_explain_parent(p_parse);
}

/// Adiciona um OP_ParseSchema. Separado de `vdbe_add_op4()` porque também precisa marcar todos os
/// btrees como usados. `z_where` passa a pertencer ao opcode.
pub fn vdbe_add_parse_schema_op(p: &mut Vdbe, i_db: i32, z_where: Option<Vec<u8>>, p5: u16) {
    vdbe_add_op4(
        p,
        OP_PARSESCHEMA as i32,
        i_db,
        0,
        0,
        match z_where {
            Some(z) => P4Value::Dynamic(z),
            None => P4Value::NotUsed,
        },
        P4_DYNAMIC,
    );
    vdbe_change_p5(p, p5);
    let db = p.db.upgrade().expect("Vdbe sem conexão");
    let mut j: i32 = 0;
    while j < db.borrow().n_db {
        vdbe_uses_btree(p, j);
        j += 1;
    }
    let parse = vdbe_parse_ref(p);
    may_abort(&mut parse.borrow_mut());
}

/// Insere o fim de uma co-rotina.
pub fn vdbe_end_coroutine(v: &mut Vdbe, reg_yield: i32) {
    vdbe_add_op1(v, OP_ENDCOROUTINE as i32, reg_yield);

    // Zera o cache de registradores temporários, garantindo que cada co-rotina tenha seu próprio
    // conjunto independente de registradores: uma co-rotina pode esperar que os seus sejam
    // preservados através de um OP_Yield, e isso daria problema se duas ou mais co-rotinas
    // usassem o mesmo registrador temporário.
    let parse = vdbe_parse_ref(v);
    let mut pp = parse.borrow_mut();
    pp.n_temp_reg = 0;
    pp.n_range_reg = 0;
}

/// Cria um novo rótulo simbólico para uma instrução ainda não codificada. O rótulo é só um número
/// negativo que pode ser usado como P2 de uma operação. Quando o rótulo é resolvido para um
/// endereço, o VDBE troca todos os P2 que casam com ele pelo endereço resolvido. Um P2 negativo
/// é um rótulo não resolvido (só vale para opcodes com a propriedade OPFLG_JUMP).
///
/// `Parse.a_label[x]` guarda o endereço em que o x-ésimo rótulo se resolve, `Parse.n_label_alloc`
/// o número de posições alocadas em a_label e `Parse.n_label` o NEGATIVO do número de rótulos
/// emitidos (o negativo dá ganho de desempenho em relação ao positivo equivalente).
pub fn vdbe_make_label(p_parse: &mut Parse) -> i32 {
    p_parse.n_label -= 1;
    p_parse.n_label
}

/// Resolve o rótulo "x" como o endereço da próxima instrução a inserir (ramo de crescimento do
/// array de rótulos).
fn resize_resolve_label(p: &mut Parse, v: &Vdbe, j: i32) {
    let n_new_size = 10 - p.n_label;
    // sqlite3DbReallocOrFree: o ramo de falha (aLabel==0, nLabelAlloc=0) não existe em Rust.
    // As posições novas ficam com -1, "rótulo ainda não resolvido".
    p.a_label.resize(n_new_size as usize, -1);
    if n_new_size >= 100 && (n_new_size / 100) > (p.n_label_alloc / 100) {
        progress_check(p);
    }
    p.n_label_alloc = n_new_size;
    p.a_label[j as usize] = v.n_op;
}

/// Resolve o rótulo "x" como o endereço da próxima instrução a inserir. O parâmetro "x" deve ter
/// vindo de uma chamada anterior a `vdbe_make_label()`.
pub fn vdbe_resolve_label(v: &mut Vdbe, x: i32) {
    let parse = vdbe_parse_ref(v);
    let mut p = parse.borrow_mut();
    let j = addr(x);
    if p.n_label_alloc + p.n_label < 0 {
        resize_resolve_label(&mut p, v, j);
    } else {
        // Os rótulos só podem ser resolvidos uma vez.
        p.a_label[j as usize] = v.n_op;
    }
}

/// Marca o VDBE como um que só pode rodar uma vez.
pub fn vdbe_run_only_once(p: &mut Vdbe) {
    vdbe_add_op2(p, OP_EXPIRE as i32, 1, 1);
}

/// Marca o VDBE como um que pode rodar várias vezes.
pub fn vdbe_reusable(p: &mut Vdbe) {
    let n_op = p.n_op as usize;
    let mut i: usize = 1;
    while i < n_op {
        if p.a_op[i].opcode == OP_EXPIRE {
            p.a_op[1].opcode = OP_NOOP;
            break;
        }
        i += 1;
    }
}


// ---- part_002.rs ----

// As funções sqlite3VdbeAssertMayAbort, sqlite3VdbeIncrWriteCounter,
// sqlite3VdbeAssertAbortable, sqlite3VdbeNoJumpsOutsideSubrtn,
// sqlite3VdbeVerifyNoMallocRequired, sqlite3VdbeVerifyNoResultRow e
// sqlite3VdbeVerifyAbortable existem só sob SQLITE_DEBUG e por isso não são
// traduzidas (o Debian compila sem SQLITE_DEBUG).

/// Chamada depois que todos os opcodes foram inseridos. Percorre todos os opcodes
/// e acerta alguns detalhes:
///
/// (1) Para cada instrução de salto com P2 negativo (um label), resolve P2 para um
///     endereço real.
///
/// (2) Calcula o número máximo de argumentos usado por qualquer função SQL e guarda
///     o valor em `*p_max_func_args`.
///
/// (3) Atualiza as flags Vdbe.read_only e Vdbe.b_is_reader para indicar com
///     exatidão o que a declaração preparada de fato faz.
///
/// (4) (descontinuado)
///
/// (5) Recupera a memória alocada para guardar os labels.
///
/// Esta rotina só funciona se o gerador mkopcodeh.tcl numerar os opcodes
/// corretamente. Mudanças aqui precisam ser coordenadas com o mkopcodeh.tcl.
pub fn resolve_p2_values(p: &mut Vdbe, p_max_func_args: &mut i32) {
    let mut n_max_args: i32 = *p_max_func_args;
    let p_parse_rc = p
        .p_parse
        .as_ref()
        .and_then(|w| w.upgrade())
        .expect("Vdbe.p_parse deve apontar para um Parse vivo");
    let mut p_parse = p_parse_rc.borrow_mut();

    p.read_only = 1;
    p.b_is_reader = 0;
    let mut i_op: usize = (p.n_op - 1) as usize;
    // O laço termina ao chegar no opcode OP_Init.
    loop {
        // Só os opcodes de salto e a lista curta de opcodes especiais do switch
        // abaixo precisam ser considerados. O gerador mkopcodeh.tcl agrupa todos
        // esses opcodes perto do início da lista. Pular qualquer opcode maior que
        // SQLITE_MX_JUMP_OPCODE é uma otimização de desempenho.
        if p.a_op[i_op].opcode <= SQLITE_MX_JUMP_OPCODE {
            // Ramo "default" do switch do C, alcançado também pela queda a partir de
            // OP_VFilter.
            let mut run_default = false;
            match p.a_op[i_op].opcode {
                OP_TRANSACTION => {
                    if p.a_op[i_op].p2 != 0 {
                        p.read_only = 0;
                    }
                    // Queda deliberada para o caso seguinte.
                    p.b_is_reader = 1;
                }
                OP_AUTOCOMMIT | OP_SAVEPOINT => {
                    p.b_is_reader = 1;
                }
                OP_CHECKPOINT | OP_VACUUM | OP_JOURNALMODE => {
                    p.read_only = 0;
                    p.b_is_reader = 1;
                }
                OP_INIT => {
                    break;
                }
                OP_VUPDATE => {
                    if p.a_op[i_op].p2 > n_max_args {
                        n_max_args = p.a_op[i_op].p2;
                    }
                }
                OP_VFILTER => {
                    let n: i32 = p.a_op[i_op - 1].p1;
                    if n > n_max_args {
                        n_max_args = n;
                    }
                    // Queda para o caso default.
                    run_default = true;
                }
                _ => {
                    run_default = true;
                }
            }
            if run_default {
                if p.a_op[i_op].p2 < 0 {
                    // O mkopcodeh.tcl arranjou as coisas de modo que os únicos
                    // opcodes que não são de salto e ficam abaixo de
                    // SQLITE_MX_JUMP_CODE têm P2 não negativo.
                    let i_label = addr(p.a_op[i_op].p2) as usize;
                    p.a_op[i_op].p2 = p_parse.a_label[i_label];
                }
            }
        }
        i_op -= 1;
    }
    if !p_parse.a_label.is_empty() {
        // sqlite3DbNNFreeNN(p->db, pParse->aLabel): o Vec é dono da memória.
        p_parse.a_label = Vec::new();
    }
    p_parse.n_label = 0;
    *p_max_func_args = n_max_args;
}

/// Devolve o endereço da próxima instrução a ser inserida.
pub fn vdbe_current_addr(p: &Vdbe) -> i32 {
    p.n_op
}


// ---- part_003.rs ----


/// Devolve o array de opcodes associado ao Vdbe `p`. O chamador passa a ser dono do array e deve
/// descartá-lo com `vdbe_free_op_array()`.
///
/// Antes de retornar, `*pn_op` recebe o número de entradas do array devolvido e `*pn_max_arg`
/// recebe o maior entre o valor atual e o número de entradas de `Vdbe.ap_arg[]` necessárias para
/// executar o programa devolvido.
pub fn vdbe_take_op_array(p: &mut Vdbe, pn_op: &mut i32, pn_max_arg: &mut i32) -> Vec<VdbeOp> {
    // Confere que vdbe_uses_btree() não foi chamada nesta VM (assert do C: btree_mask zerado).
    resolve_p2_values(p, pn_max_arg);
    *pn_op = p.n_op;
    std::mem::take(&mut p.a_op)
}

/// Acrescenta uma lista inteira de operações à pilha de operações. Devolve o índice em `p.a_op`
/// da primeira operação inserida (o ponteiro do C), ou `None` se faltou memória.
///
/// Os argumentos P2 não nulos das instruções de salto são ajustados automaticamente para que o
/// destino do salto seja relativo à primeira operação inserida.
///
/// `p.a_op` pode ter comprimento igual a `n_op` ou a `n_op_alloc`, conforme `grow_op_array()`;
/// por isso cada instrução nova sobrescreve o slot se ele existir e é acrescentada se não existir.
pub fn vdbe_add_op_list(
    p: &mut Vdbe,
    n_op: i32,
    a_op: &[VdbeOpList],
    i_lineno: i32,
) -> Option<usize> {
    if p.n_op + n_op > p.n_op_alloc && grow_op_array(p, n_op) != 0 {
        return None;
    }
    let first = p.n_op as usize;
    for (i, src) in a_op.iter().take(n_op as usize).enumerate() {
        let mut p2 = src.p2 as i32;
        if (OPFLG_INITIALIZER[src.opcode as usize] & OPFLG_JUMP) != 0 && src.p2 > 0 {
            p2 += p.n_op;
        }
        let out = VdbeOp {
            opcode: src.opcode,
            p4type: P4_NOTUSED,
            p5: 0,
            p1: src.p1 as i32,
            p2,
            p3: src.p3 as i32,
            p4: P4Value::NotUsed,
        };
        // Sem SQLITE_VDBE_COVERAGE o número da linha de origem não é guardado.
        let _ = i_lineno;
        if first + i < p.a_op.len() {
            p.a_op[first + i] = out;
        } else {
            p.a_op.push(out);
        }
    }
    p.n_op += n_op;
    Some(first)
}

// As rotinas vdbe_scan_status(), vdbe_scan_status_range() e vdbe_scan_status_counters() só
// existem com SQLITE_ENABLE_STMT_SCANSTATUS, que o Debian 13 não liga: não são traduzidas.

/// Muda o valor do opcode da instrução em `addr`.
pub fn vdbe_change_opcode(p: &mut Vdbe, addr: i32, i_new_opcode: u8) {
    vdbe_get_op(p, addr).opcode = i_new_opcode;
}

/// Muda o operando P1 da instrução em `addr`.
pub fn vdbe_change_p1(p: &mut Vdbe, addr: i32, val: i32) {
    vdbe_get_op(p, addr).p1 = val;
}

/// Muda o operando P2 da instrução em `addr`.
pub fn vdbe_change_p2(p: &mut Vdbe, addr: i32, val: i32) {
    vdbe_get_op(p, addr).p2 = val;
}

/// Muda o operando P3 da instrução em `addr`.
pub fn vdbe_change_p3(p: &mut Vdbe, addr: i32, val: i32) {
    vdbe_get_op(p, addr).p3 = val;
}

/// Muda o operando P5 da última instrução inserida.
pub fn vdbe_change_p5(p: &mut Vdbe, p5: u16) {
    if p.n_op > 0 {
        let last = (p.n_op - 1) as usize;
        p.a_op[last].p5 = p5;
    }
}

/// Se o opcode anterior é um OP_Column que entrega o resultado no registrador `i_dest`, liga a
/// flag OPFLAG_TYPEOFARG nesse opcode.
pub fn vdbe_typeof_column(p: &mut Vdbe, i_dest: i32) {
    let p_op = vdbe_get_last_op(p);
    if p_op.p3 == i_dest && p_op.opcode == OP_COLUMN {
        p_op.p5 |= OPFLAG_TYPEOFARG as u16;
    }
}

/// Muda o operando P2 da instrução em `addr` para que aponte para o endereço da próxima
/// instrução a ser codificada.
pub fn vdbe_jump_here(p: &mut Vdbe, addr: i32) {
    let next = p.n_op;
    vdbe_change_p2(p, addr, next);
}

/// Muda o operando P2 da instrução de salto em `addr` para que o salto caia no próximo opcode.
/// Ou, se a instrução de salto era o opcode anterior (e portanto é um no-op), apenas recua o
/// contador da próxima instrução em uma posição, de modo que o salto seja sobrescrito pelo
/// próximo opcode inserido.
///
/// É uma otimização de `vdbe_jump_here()` que evita bytecode inútil como este:
///
/// ```text
///        7   Once 0 8 0
///        8   ...
/// ```
pub fn vdbe_jump_here_or_pop_inst(p: &mut Vdbe, addr: i32) {
    if addr == p.n_op - 1 {
        p.n_op -= 1;
    } else {
        let next = p.n_op;
        vdbe_change_p2(p, addr, next);
    }
}

/// Se a estrutura FuncDef recebida é efêmera, libera-a. Se não é efêmera, não faz nada. No
/// modelo sem ponteiros a memória é devolvida quando a referência é descartada.
fn free_ephemeral_function(_db: &Sqlite3, p_def: Rc<FuncDef>) {
    if (p_def.func_flags & SQLITE_FUNC_EPHEM) != 0 {
        drop(p_def);
    }
}

/// Descarta um valor P4 do tipo Mem quando a contagem de bytes liberados está ligada. A
/// alocação `z_malloc` e o próprio Mem são devolvidos ao serem descartados.
fn free_p4_mem(_db: &Sqlite3, p: Box<Mem>) {
    drop(p);
}

/// Descarta um contexto de função P4: libera a função efêmera e depois o próprio contexto.
fn free_p4_func_ctx(db: &Sqlite3, p: Box<sqlite3_context>) {
    let ctx = *p;
    free_ephemeral_function(db, ctx.p_func);
}

/// Libera o valor P4 se necessário.
fn free_p4(db: &mut Sqlite3, p4type: i8, p4: P4Value) {
    match p4type {
        P4_FUNCCTX => {
            if let P4Value::FuncCtx(ctx) = p4 {
                free_p4_func_ctx(db, ctx);
            }
        }
        P4_REAL | P4_INT64 | P4_DYNAMIC | P4_INTARRAY => {
            drop(p4);
        }
        P4_KEYINFO => {
            if db.pn_bytes_freed.is_none() {
                if let P4Value::KeyInfo(key_info) = p4 {
                    key_info_unref(key_info);
                }
            }
        }
        P4_FUNCDEF => {
            if let P4Value::FuncDef(def) = p4 {
                free_ephemeral_function(db, def);
            }
        }
        P4_MEM => {
            if let P4Value::Mem(mem) = p4 {
                if db.pn_bytes_freed.is_none() {
                    value_free(mem);
                } else {
                    free_p4_mem(db, mem);
                }
            }
        }
        P4_VTAB => {
            if db.pn_bytes_freed.is_none() {
                if let P4Value::VTab(vtab) = p4 {
                    vtab_unlock(vtab);
                }
            }
        }
        P4_TABLEREF => {
            if db.pn_bytes_freed.is_none() {
                if let P4Value::Table(table) = p4 {
                    delete_table(db, table);
                }
            }
        }
        _ => {}
    }
}

/// Libera o espaço alocado para `a_op` e quaisquer valores P4 alocados para os opcodes contidos
/// nele. Se `a_op` não é vazio, assume-se que contém `n_op` entradas.
fn vdbe_free_op_array(db: &mut Sqlite3, a_op: Vec<Op>, n_op: i32) {
    if !a_op.is_empty() {
        let mut a_op = a_op;
        let n = (n_op.max(0) as usize).min(a_op.len());
        for p_op in a_op[..n].iter_mut().rev() {
            if p_op.p4type <= P4_FREE_IF_LE {
                let p4 = std::mem::replace(&mut p_op.p4, P4Value::NotUsed);
                free_p4(db, p_op.p4type, p4);
            }
        }
        // Ao sair do escopo o array inteiro é devolvido (sqlite3DbNNFreeNN do C).
    }
}

/// Encadeia o objeto SubProgram `p` na lista em `Vdbe.p_program`. Essa lista serve para apagar
/// todos os objetos de subprograma quando a VM não é mais necessária.
pub fn vdbe_link_sub_program(p_vdbe: &mut Vdbe, p: SubProgramRef) {
    p.borrow_mut().p_next = p_vdbe.p_program.take();
    p_vdbe.p_program = Some(p);
}

/// Devolve verdadeiro se o Vdbe dado tem algum SubProgram.
pub fn vdbe_has_sub_program(p_vdbe: &Vdbe) -> i32 {
    p_vdbe.p_program.is_some() as i32
}

/// Troca o opcode em `addr` por OP_Noop.
pub fn vdbe_change_to_noop(p: &mut Vdbe, addr: i32) -> i32 {
    let db_ref = p.db.upgrade().expect("a conexão dona do Vdbe foi encerrada");
    let mut db = db_ref.borrow_mut();
    if db.malloc_failed != 0 {
        return 0;
    }
    let p_op = &mut p.a_op[addr as usize];
    let p4type = p_op.p4type;
    let p4 = std::mem::replace(&mut p_op.p4, P4Value::NotUsed);
    free_p4(&mut *db, p4type, p4);
    p_op.p4type = P4_NOTUSED;
    p_op.opcode = OP_NOOP;
    1
}


// ---- part_004.rs ----

// Trecho 4 de vdbeaux.c (sqlite 3.46.1). Os blocos sob SQLITE_DEBUG,
// SQLITE_ENABLE_EXPLAIN_COMMENTS, SQLITE_ENABLE_CURSOR_HINTS e SQLITE_VDBE_COVERAGE
// não entram: nenhum deles está nas opções de compilação do Debian 13 listadas
// em CONVENTIONS.md (sqlite3VdbeReleaseRegisters, vdbeVComment, sqlite3VdbeComment,
// sqlite3VdbeNoopComment, sqlite3VdbeSetLineNumber, translateP,
// sqlite3VdbeDisplayComment e displayP4Expr).
//
// Modelo do P4 (sem ponteiros): o argumento `void*`/`const char*` do C chega como
// um `P4Value` já tipado (`NotUsed`, `Int32(i32)`, `Static(Vec<u8>)`, `KeyInfo(..)`,
// `VTab(..)`, ...), o mesmo tipo do campo `VdbeOp::p4` (vdbe.h).

/// Remove o último opcode se ele for `op` e não for destino de salto.
/// Devolve verdadeiro se, e somente se, um opcode foi removido.
pub fn vdbe_delete_prior_opcode(p: &mut Vdbe, op: u8) -> i32 {
    if p.n_op > 0 && p.a_op[(p.n_op - 1) as usize].opcode == op {
        let addr = p.n_op - 1;
        vdbe_change_to_noop(p, addr)
    } else {
        0
    }
}

/// Parte lenta de `vdbe_change_p4`: o operando P4 já estava definido, ou `n>=0`
/// pede cópia dinâmica da string. `addr` é o índice da instrução em `p.a_op`
/// (o `pOp - p->aOp` do C).
fn vdbe_change_p4_full(p: &mut Vdbe, addr: i32, z_p4: P4Value, n: i32) {
    {
        let p_op = &mut p.a_op[addr as usize];
        if p_op.p4type != 0 {
            debug_assert!(p_op.p4type > P4_FREE_IF_LE);
            p_op.p4type = 0;
            p_op.p4 = P4Value::NotUsed;
        }
    }
    if n < 0 {
        vdbe_change_p4(p, addr, z_p4, n);
    } else {
        let z: Vec<u8> = match z_p4 {
            P4Value::Static(z) | P4Value::Dynamic(z) => z,
            _ => Vec::new(),
        };
        let n = if n == 0 { strlen30(Some(&z)) } else { n };
        let dup = match p.db.upgrade() {
            Some(db_ref) => db_str_n_dup(&mut db_ref.borrow_mut(), Some(&z), n as u64),
            None => None,
        };
        let p_op = &mut p.a_op[addr as usize];
        p_op.p4 = match dup {
            Some(d) => P4Value::Dynamic(d.cstr().to_vec()),
            None => P4Value::NotUsed,
        };
        p_op.p4type = P4_DYNAMIC;
    }
}

/// Muda o valor do operando P4 de uma instrução específica. Útil quando um
/// programa grande é carregado de um vetor estático por `vdbe_add_op_list` e
/// poucas mudanças são necessárias.
///
/// Se `n>=0` o P4 é dinâmico: uma cópia da string é feita (`n==0` copia até o
/// primeiro byte nulo, `n>0` copia `n` bytes). Os outros valores de `n`
/// (P4_STATIC, P4_COLLSEQ etc.) indicam que o valor vive tanto quanto o Vdbe e
/// basta guardá-lo. Se `addr<0`, muda o P4 da última instrução inserida.
pub fn vdbe_change_p4(p: &mut Vdbe, addr: i32, z_p4: P4Value, n: i32) {
    debug_assert!(p.e_vdbe_state == VDBE_INIT_STATE);
    let db = p.db.upgrade();
    let malloc_failed = db.as_ref().map_or(false, |d| d.borrow().malloc_failed != 0);
    if malloc_failed {
        if n != P4_VTAB as i32 {
            if let Some(d) = &db {
                free_p4(&mut d.borrow_mut(), n as i8, z_p4);
            }
        }
        return;
    }
    debug_assert!(p.n_op > 0);
    debug_assert!(addr < p.n_op);
    let addr = if addr < 0 { p.n_op - 1 } else { addr };
    if n >= 0 || p.a_op[addr as usize].p4type != 0 {
        vdbe_change_p4_full(p, addr, z_p4, n);
        return;
    }
    if n == P4_INT32 as i32 {
        // O dado de origem era um int convertido em ponteiro (SQLITE_PTR_TO_INT).
        let i = match z_p4 {
            P4Value::Int32(i) => i,
            _ => 0,
        };
        let p_op = &mut p.a_op[addr as usize];
        p_op.p4 = P4Value::Int32(i);
        p_op.p4type = P4_INT32;
    } else if !matches!(z_p4, P4Value::NotUsed) {
        debug_assert!(n < 0);
        if n == P4_VTAB as i32 {
            if let P4Value::VTab(v) = &z_p4 {
                vtab_lock(&mut v.borrow_mut());
            }
        }
        let p_op = &mut p.a_op[addr as usize];
        p_op.p4 = z_p4;
        p_op.p4type = n as i8;
    }
}

/// Muda o P4 da instrução mais recente para o valor dado. Versão rápida de
/// `vdbe_change_p4`: o P4 não pode ter sido definido antes e o novo não pode ser
/// P4_INT32 nem P4_VTAB.
pub fn vdbe_append_p4(p: &mut Vdbe, p_p4: P4Value, n: i32) {
    debug_assert!(n != P4_INT32 as i32 && n != P4_VTAB as i32);
    debug_assert!(n <= 0);
    let db = p.db.upgrade();
    let malloc_failed = db.as_ref().map_or(false, |d| d.borrow().malloc_failed != 0);
    if malloc_failed {
        if let Some(d) = &db {
            free_p4(&mut d.borrow_mut(), n as i8, p_p4);
        }
    } else {
        debug_assert!(!matches!(p_p4, P4Value::NotUsed) || n == P4_DYNAMIC as i32);
        debug_assert!(p.n_op > 0);
        let last = (p.n_op - 1) as usize;
        let p_op = &mut p.a_op[last];
        debug_assert!(p_op.p4type == P4_NOTUSED);
        p_op.p4type = n as i8;
        p_op.p4 = p_p4;
    }
}

/// Define o P4 do último opcode adicionado como o KeyInfo do índice dado.
pub fn vdbe_set_p4_key_info(p_parse: &mut Parse, p_idx: &IndexRef) {
    let v = p_parse.p_vdbe.clone();
    debug_assert!(v.is_some());
    let p_key_info = key_info_of_index(p_parse, p_idx);
    if let (Some(v), Some(ki)) = (v, p_key_info) {
        vdbe_append_p4(&mut v.borrow_mut(), P4Value::KeyInfo(ki), P4_KEYINFO as i32);
    }
}

/// Devolve o opcode de um endereço (não negativo). Para o mais recente use
/// `vdbe_get_last_op`.
///
/// Se houve falha de alocação antes, devolve um VdbeOp fictício (campo
/// `dummy_op` do Vdbe, todo zerado). Ele é legível, nunca escrito: com
/// `opcode==0` nenhum chamador o modifica.
pub fn vdbe_get_op(p: &mut Vdbe, addr: i32) -> &mut VdbeOp {
    debug_assert!(p.e_vdbe_state == VDBE_INIT_STATE);
    let malloc_failed = p.db.upgrade().map_or(false, |d| d.borrow().malloc_failed != 0);
    debug_assert!((addr >= 0 && addr < p.n_op) || malloc_failed);
    if malloc_failed {
        &mut p.dummy_op
    } else {
        &mut p.a_op[addr as usize]
    }
}

/// Devolve o opcode adicionado mais recentemente.
pub fn vdbe_get_last_op(p: &mut Vdbe) -> &mut VdbeOp {
    let addr = p.n_op - 1;
    vdbe_get_op(p, addr)
}


// ---- part_005.rs ----

// Trecho 5 de vdbeaux.c (sqlite 3.46.1).
//
// Fora deste trecho por não existirem na build do Debian 13 (CONVENTIONS.md): displayP4Expr
// e o caso P4_EXPR de vdbe_display_p4 (SQLITE_ENABLE_CURSOR_HINTS), sqlite3VdbePrintOp
// (VDBE_PROFILE e SQLITE_DEBUG), sqlite3VdbeFrameIsValid (SQLITE_DEBUG) e o ramo eMode==2 de
// sqlite3VdbeNextOpcode (SQLITE_ENABLE_BYTECODE_VTAB). O cache compartilhado e SQLITE_THREADSAFE=1
// estão ligados, então vdbe_enter e vdbe_leave existem sem condição.

/// Anexa ao acumulador o resultado de um formato com argumentos (o
/// `sqlite3_str_appendf` do C, que recebe os argumentos variádicos em `va_list`).
fn appendf(x: &mut StrAccum, z_format: &[u8], args: Vec<VaArg>) {
    let mut ap = VaList::new();
    ap.args.extend(args);
    str_appendf(x, z_format, &mut ap);
}

/// Bytes de um texto C: tudo até o primeiro NUL.
fn c_text(z: &[u8]) -> &[u8] {
    let n = z.iter().position(|&c| c == 0).unwrap_or(z.len());
    &z[..n]
}

/// Calcula uma string que descreve o parâmetro P4 de um opcode.
pub fn vdbe_display_p4(db: &Sqlite3Ref, p_op: &Op) -> Option<Vec<u8>> {
    let mut z_p4: Option<Vec<u8>> = None;
    let mut x = StrAccum::default();
    str_accum_init(&mut x, None, 0, SQLITE_MAX_LENGTH);
    match p_op.p4type {
        P4_KEYINFO => {
            if let P4Value::KeyInfo(key_info) = &p_op.p4 {
                let ki = key_info.borrow();
                debug_assert!(!ki.a_sort_flags.is_empty());
                appendf(&mut x, b"k(%d", vec![VaArg::Int(ki.n_key_field as i32)]);
                for j in 0..(ki.n_key_field as usize) {
                    let mut z_coll: Vec<u8> = match &ki.a_coll[j] {
                        Some(p_coll) => p_coll.borrow().z_name.clone(),
                        None => Vec::new(),
                    };
                    if c_text(&z_coll) == b"BINARY" {
                        z_coll = b"B".to_vec();
                    }
                    let flags = ki.a_sort_flags[j];
                    let z_desc: &[u8] = if (flags & KEYINFO_ORDER_DESC) != 0 { b"-" } else { b"" };
                    let z_big_null: &[u8] = if (flags & KEYINFO_ORDER_BIGNULL) != 0 { b"N." } else { b"" };
                    appendf(
                        &mut x,
                        b",%s%s%s",
                        vec![
                            VaArg::Text(Some(z_desc.to_vec())),
                            VaArg::Text(Some(z_big_null.to_vec())),
                            VaArg::Text(Some(z_coll)),
                        ],
                    );
                }
                str_append(&mut x, b")", 1);
            }
        }
        P4_COLLSEQ => {
            const ENCNAMES: [&[u8]; 4] = [b"?", b"8", b"16LE", b"16BE"];
            if let P4Value::CollSeq(p_coll) = &p_op.p4 {
                let coll = p_coll.borrow();
                debug_assert!(coll.enc < 4);
                appendf(
                    &mut x,
                    b"%.18s-%s",
                    vec![
                        VaArg::Text(Some(coll.z_name.clone())),
                        VaArg::Text(Some(ENCNAMES[coll.enc as usize].to_vec())),
                    ],
                );
            }
        }
        P4_FUNCDEF => {
            if let P4Value::FuncDef(p_def) = &p_op.p4 {
                appendf(
                    &mut x,
                    b"%s(%d)",
                    vec![VaArg::Text(Some(p_def.z_name.clone())), VaArg::Int(p_def.n_arg as i32)],
                );
            }
        }
        P4_FUNCCTX => {
            if let P4Value::FuncCtx(p_ctx) = &p_op.p4 {
                let p_def = &p_ctx.p_func;
                appendf(
                    &mut x,
                    b"%s(%d)",
                    vec![VaArg::Text(Some(p_def.z_name.clone())), VaArg::Int(p_def.n_arg as i32)],
                );
            }
        }
        P4_INT64 => {
            if let P4Value::Int64(i) = &p_op.p4 {
                appendf(&mut x, b"%lld", vec![VaArg::Long(*i)]);
            }
        }
        P4_INT32 => {
            if let P4Value::Int32(i) = &p_op.p4 {
                appendf(&mut x, b"%d", vec![VaArg::Int(*i)]);
            }
        }
        P4_REAL => {
            if let P4Value::Real(r) = &p_op.p4 {
                appendf(&mut x, b"%.16g", vec![VaArg::Double(*r)]);
            }
        }
        P4_MEM => {
            if let P4Value::Mem(p_mem) = &p_op.p4 {
                if (p_mem.flags & MEM_STR) != 0 {
                    z_p4 = Some(c_text(&p_mem.z).to_vec());
                } else if (p_mem.flags & (MEM_INT | MEM_INTREAL)) != 0 {
                    appendf(&mut x, b"%lld", vec![VaArg::Long(p_mem.u.i)]);
                } else if (p_mem.flags & MEM_REAL) != 0 {
                    appendf(&mut x, b"%.16g", vec![VaArg::Double(p_mem.u.r)]);
                } else if (p_mem.flags & MEM_NULL) != 0 {
                    z_p4 = Some(b"NULL".to_vec());
                } else {
                    debug_assert!((p_mem.flags & MEM_BLOB) != 0);
                    z_p4 = Some(b"(blob)".to_vec());
                }
            }
        }
        P4_VTAB => {
            if let P4Value::VTab(p_vtable) = &p_op.p4 {
                // O endereço do sqlite3_vtab, impresso por %p como no C.
                let addr: u64 = match &p_vtable.borrow().p_vtab {
                    Some(p_vtab) => Rc::as_ptr(p_vtab) as *const u8 as usize as u64,
                    None => 0,
                };
                appendf(&mut x, b"vtab:%p", vec![VaArg::ULong(addr)]);
            }
        }
        P4_INTARRAY => {
            if let P4Value::IntArray(ai) = &p_op.p4 {
                // O primeiro elemento de um INTARRAY é sempre a contagem dos que seguem.
                let n = ai[0];
                let mut i: u32 = 1;
                while i <= n {
                    appendf(
                        &mut x,
                        b"%c%u",
                        vec![VaArg::Int(if i == 1 { b'[' } else { b',' } as i32), VaArg::UInt(ai[i as usize])],
                    );
                    i += 1;
                }
                str_append(&mut x, b"]", 1);
            }
        }
        P4_SUBPROGRAM => {
            z_p4 = Some(b"program".to_vec());
        }
        P4_TABLE => {
            if let P4Value::Table(p_tab) = &p_op.p4 {
                z_p4 = Some(p_tab.borrow().z_name.clone());
            }
        }
        _ => {
            if let P4Value::Static(z) | P4Value::Dynamic(z) = &p_op.p4 {
                z_p4 = Some(c_text(z).to_vec());
            }
        }
    }
    if let Some(z) = &z_p4 {
        str_appendall(&mut x, z);
    }
    if (x.acc_error as i32 & SQLITE_NOMEM) != 0 {
        oom_fault(&mut db.borrow_mut());
    }
    str_accum_finish(&mut x)
}

/// Declara ao Vdbe que a árvore B em db->a_db[i] é usada.
///
/// As declarações preparadas precisam saber antecipadamente o conjunto
/// completo de bancos anexados que serão usados. Uma máscara desses bancos
/// é mantida em p->btree_mask. O valor p->lock_mask é o subconjunto de
/// p->btree_mask de bancos que exigem bloqueio.
pub fn vdbe_uses_btree(p: &mut Vdbe, i: i32) {
    let db_ref = p.db.upgrade().expect("Vdbe sem conexão");
    let db = db_ref.borrow();
    debug_assert!(i >= 0 && i < db.n_db && i < (std::mem::size_of::<yDbMask>() * 8) as i32);
    db_mask_set(&mut p.btree_mask, i as u32);
    if i != 1 {
        if let Some(p_bt) = &db.a_db[i as usize].p_bt {
            if btree_sharable(&p_bt.borrow()) != 0 {
                db_mask_set(&mut p.lock_mask, i as u32);
            }
        }
    }
}

/// As árvores B de lock_mask, na ordem de a_db[] (o laço comum a vdbe_enter e
/// vdbe_leave). O banco 1 nunca entra: é o do `temp`, que não é compartilhável.
fn locked_btrees(p: &Vdbe) -> Vec<BtreeRef> {
    let mut out = Vec::new();
    let db_ref = match p.db.upgrade() {
        Some(d) => d,
        None => return out,
    };
    let db = db_ref.borrow();
    for i in 0..db.n_db {
        if i != 1 && db_mask_test(p.lock_mask, i as u32) {
            if let Some(p_bt) = &db.a_db[i as usize].p_bt {
                out.push(p_bt.clone());
            }
        }
    }
    out
}

/// Obtém o mutex associado a cada estrutura BtShared que a VM pode acessar.
/// Ao fazer isso também define BtShared.db de cada uma, garantindo que o
/// callback de busy correto seja invocado se necessário.
///
/// O campo p->btree_mask é uma máscara de bits de todas as btrees que a
/// declaração preparada p jamais usará. Seja N o número de bits em
/// p->btree_mask correspondentes a btrees que usam cache compartilhado. Então
/// o tempo de execução desta rotina é N*N. Mas como N raramente é mais que 1,
/// isso não deve ser um problema.
pub fn vdbe_enter(p: &Vdbe) {
    if db_mask_all_zero(p.lock_mask) {
        return; // O caso comum.
    }
    for p_bt in locked_btrees(p) {
        btree_enter(&mut p_bt.borrow_mut());
    }
}

/// Desbloqueia todas as btrees previamente bloqueadas por `vdbe_enter()`.
fn vdbe_leave_locked(p: &Vdbe) {
    for p_bt in locked_btrees(p) {
        btree_leave(&mut p_bt.borrow_mut());
    }
}

pub fn vdbe_leave(p: &Vdbe) {
    if db_mask_all_zero(p.lock_mask) {
        return; // O caso comum.
    }
    vdbe_leave_locked(p);
}

/// Inicializa um array de N elementos Mem.
///
/// Esta é uma rotina de alto desempenho, portanto apenas os campos que
/// realmente precisam ser inicializados são definidos:
///
///    Mem.flags = flags
///    Mem.db = db
///    Mem.sz_malloc = 0
///
/// Todos os outros campos de Mem podem ficar com segurança sem inicializar
/// por enquanto. Eles serão inicializados antes do uso.
pub fn init_mem_array(p: &[MemRef], db: &Sqlite3Ref, flags: u16) {
    for mem in p.iter() {
        let mut mem = mem.borrow_mut();
        mem.flags = flags;
        mem.db = Some(Rc::downgrade(db));
        mem.sz_malloc = 0;
    }
}

/// Libera memória auxiliar mantida em um array de N elementos Mem.
///
/// Após esta rotina retornar, todos os elementos Mem no array ainda serão
/// válidos. Aqueles que não estavam mantendo recursos auxiliares permanecem
/// inalterados. Elementos Mem que tiveram algo liberado ficam como MEM_Undefined.
pub fn release_mem_array(p: &[MemRef]) {
    if p.is_empty() {
        return;
    }
    let db_ref = p[0].borrow().db.as_ref().and_then(|w| w.upgrade());
    let measuring = db_ref.as_ref().map_or(false, |d| d.borrow().p_n_bytes_freed.is_some());
    if measuring {
        // Só mede o que seria liberado (sqlite3_db_status): o Mem fica intacto.
        let db_ref = db_ref.as_ref().unwrap();
        for mem in p.iter() {
            let mem = mem.borrow();
            if mem.sz_malloc != 0 {
                db_free(Some(&mut db_ref.borrow_mut()), Some(DbBlock::heap(mem.z_malloc.clone())));
            }
        }
        return;
    }
    for mem in p.iter() {
        let mut mem = mem.borrow_mut();
        let mem = &mut *mem;
        debug_assert!(vdbe_check_mem_invariants(mem));

        // Versão embutida de vdbe_mem_release() que se aproveita de a célula
        // virar NULL depois de liberar os recursos dinâmicos.
        if (mem.flags & (MEM_AGG | MEM_DYN)) != 0 {
            vdbe_mem_release(mem);
            mem.flags = MEM_UNDEFINED;
        } else if mem.sz_malloc != 0 {
            let z_malloc = std::mem::take(&mut mem.z_malloc);
            match &db_ref {
                Some(d) => db_free_nn(Some(&mut d.borrow_mut()), DbBlock::heap(z_malloc)),
                None => db_free_nn(None, DbBlock::heap(z_malloc)),
            }
            mem.sz_malloc = 0;
            mem.flags = MEM_UNDEFINED;
        }
    }
}

/// Destrutor de um objeto Mem (na verdade um sqlite3_value) que apaga o objeto
/// Frame anexado a ele como blob.
///
/// Esta rotina não apaga o Frame na hora. Apenas acrescenta o frame a uma lista
/// de frames a apagar quando a Vdbe parar (`p_del_frame`, encadeada por `p_parent`).
pub fn vdbe_frame_mem_del(p_frame: &VdbeFrameRef) {
    let v = p_frame.borrow().v.as_ref().and_then(|w| w.upgrade());
    if let Some(v) = v {
        let mut vdbe = v.borrow_mut();
        p_frame.borrow_mut().p_parent = vdbe.p_del_frame.take();
        vdbe.p_del_frame = Some(p_frame.clone());
    }
}

/// Localiza o próximo opcode a exibir em EXPLAIN ou EXPLAIN QUERY PLAN.
///
/// Devolve SQLITE_OK em sucesso e SQLITE_DONE se não há mais opcodes a exibir.
///
/// Desvios do C (sem ponteiros):
///  * `p_sub` é a lista de subprogramas já vistos. No C ela vive como blob de
///    ponteiros em `pSub->z`; aqui é um `Vec<SubProgramRef>` do chamador (`None`
///    quando o C passaria `pSub==0`). Como o blob some, o `sqlite3VdbeMemGrow`
///    desse trecho (e seu SQLITE_ERROR por falta de memória) também some.
///  * `pa_op` recebe `None` quando o opcode está no programa principal
///    (`p.a_op`) e `Some(sub)` quando está em `sub.borrow().a_op`; `pi_addr` é o
///    índice dentro desse vetor.
pub fn vdbe_next_opcode(
    p: &mut Vdbe,
    mut p_sub: Option<&mut Vec<SubProgramRef>>,
    e_mode: i32,
    pi_pc: &mut i32,
    pi_addr: &mut i32,
    pa_op: &mut Option<SubProgramRef>,
) -> i32 {
    let mut rc = SQLITE_OK;
    let mut a_op: Option<SubProgramRef> = None;
    let mut i: i32;

    // Quando o número de linhas de saída chega a n_row, a listagem acabou e
    // sqlite3_step() devolve SQLITE_DONE. n_row é o número de linhas do programa
    // principal mais o de todos os subprogramas de trigger encontrados até agora.
    // Ele cresce conforme novos subprogramas aparecem, mas p.pc acaba alcançando.
    let mut n_row = p.n_op;
    if let Some(subs) = p_sub.as_ref() {
        for sub in subs.iter() {
            n_row += sub.borrow().n_op;
        }
    }
    let mut i_pc = *pi_pc;
    loop {
        // O laço sai por break.
        i = i_pc;
        i_pc += 1;
        if i >= n_row {
            p.rc = SQLITE_OK;
            rc = SQLITE_DONE;
            break;
        }
        if i < p.n_op {
            // O rowid é pequeno o bastante para ainda estarmos no programa principal.
            a_op = None;
        } else {
            // Estamos listando subprogramas: descobre qual e pega o opcode certo.
            i -= p.n_op;
            let subs = p_sub.as_ref().expect("subprograma sem lista");
            debug_assert!(!subs.is_empty());
            let mut j = 0usize;
            while i >= subs[j].borrow().n_op {
                i -= subs[j].borrow().n_op;
                j += 1;
                debug_assert!(i < subs[j - 1].borrow().n_op || j < subs.len());
            }
            a_op = Some(subs[j].clone());
        }

        // Instrução em exame: opcode, p4type e, se for P4_SUBPROGRAM, o subprograma.
        let (opcode, p4type, p_program) = {
            let view = |ops: &[Op]| -> (u8, i8, Option<SubProgramRef>) {
                let op = &ops[i as usize];
                let prog = match &op.p4 {
                    P4Value::SubProgram(sp) => Some(sp.clone()),
                    _ => None,
                };
                (op.opcode, op.p4type, prog)
            };
            match &a_op {
                None => view(&p.a_op),
                Some(sp) => view(&sp.borrow().a_op),
            }
        };

        // Ao encontrar um OP_Program (o único opcode com argumento P4_SUBPROGRAM),
        // acrescenta o novo subprograma à lista, se ainda não foi visto.
        if let Some(subs) = p_sub.as_mut() {
            if p4type == P4_SUBPROGRAM {
                let prog = p_program.expect("P4_SUBPROGRAM sem subprograma");
                if !subs.iter().any(|s| Rc::ptr_eq(s, &prog)) {
                    n_row += prog.borrow().n_op;
                    subs.push(prog);
                }
            }
        }
        if e_mode == 0 {
            break;
        }
        debug_assert!(e_mode == 1);
        if opcode == OP_EXPLAIN {
            break;
        }
        if opcode == OP_INIT && i_pc > 1 {
            break;
        }
    }
    *pi_pc = i_pc;
    *pi_addr = i;
    *pa_op = a_op;
    rc
}


// ---- part_006.rs ----

// Trecho 6 de vdbeaux.c (sqlite 3.46.1).
//
// Fora deste trecho: sqlite3VdbePrintSql (SQLITE_DEBUG), sqlite3VdbeIOTraceSql
// (SQLITE_ENABLE_IOTRACE) e os blocos de VDBE_PROFILE/SQLITE_DEBUG de sqlite3VdbeRewind, que
// não existem na build do Debian 13. SQLITE_ENABLE_EXPLAIN_COMMENTS também não está ligado, então
// a coluna de comentário do EXPLAIN (pMem+7) é NULL.
//
// `struct ReusableSpace` e `allocSpace()` (que repartem a sobra do vetor de opcodes por
// aritmética de ponteiro entre aMem, aVar, apArg e apCsr) não têm tradução: sem ponteiros, cada
// vetor é alocado direto em `vdbe_make_ready`, com o mesmo tamanho que o C pediria.
//
// Estado novo que o Vdbe precisa carregar (o C o escondia em ponteiros):
//  * `Vdbe::ap_sub: Vec<SubProgramRef>`: a lista de subprogramas que o C guarda como blob em
//    `aMem[9]`. Quem faz `releaseMemArray(p->aMem, p->nMem)` (close_all_cursors) deve limpá-la.
//  * `VdbeFrame::a_frame_csr: Vec<Option<VdbeCursorRef>>`: os cursores do quadro filho, que o C
//    guarda logo depois dos registradores em `&aMem[p->nChildMem]`.

/// Apaga um objeto VdbeFrame e seu conteúdo. Os VdbeFrame são alocados pelo opcode
/// OP_Program em vdbe_exec().
///
/// O `Vdbe` dono do quadro vem como `v` (no C é `p->v`): os chamadores já o seguram
/// emprestado, então reabrir `p.v` entraria em pânico.
pub fn vdbe_frame_delete(v: &mut Vdbe, p: &VdbeFrameRef) {
    let (a_csr, a_mem, n_child_mem, mut p_aux_data) = {
        let mut f = p.borrow_mut();
        let n_child_csr = f.n_child_csr as usize;
        let mut a_csr = std::mem::take(&mut f.a_frame_csr);
        a_csr.truncate(n_child_csr);
        (a_csr, std::mem::take(&mut f.a_frame_mem), f.n_child_mem as usize, f.p_aux_data.take())
    };
    for p_csr in a_csr.into_iter().flatten() {
        vdbe_free_cursor_nn(v, &mut p_csr.borrow_mut());
    }
    release_mem_array(&a_mem[..n_child_mem]);
    let db = v.db.upgrade().expect("Vdbe sem conexão");
    vdbe_delete_aux_data(&db, &mut p_aux_data, -1, 0);
}

/// Dá uma listagem do programa na máquina virtual.
///
/// A interface é a mesma de vdbe_exec(). Mas em vez de rodar o código, ela
/// invoca o callback uma vez para cada instrução. Este recurso implementa o "EXPLAIN".
///
/// Com p.explain==1, cada instrução é listada. Com p.explain==2, só as instruções
/// OP_Explain são listadas, num formato diferente; p.explain==2 implementa
/// EXPLAIN QUERY PLAN. Em 2018-04-24, no modo p.explain==2 os OP_Init dos
/// triggers também aparecem, para deixar claras as fronteiras entre o programa
/// principal e cada trigger.
///
/// Com p.explain==1, primeiro lista-se o programa principal, depois cada
/// subprograma de trigger, um a um.
pub fn vdbe_list(p: &mut Vdbe) -> i32 {
    let db_ref = p.db.upgrade().expect("Vdbe sem conexão");
    let b_list_subprogs =
        p.explain == 1 || (db_ref.borrow().flags & SQLITE_TRIGGER_EQP) != 0;

    debug_assert!(p.explain != 0);
    debug_assert!(p.e_vdbe_state == VDBE_RUN_STATE);
    debug_assert!(p.rc == SQLITE_OK || p.rc == SQLITE_BUSY || p.rc == SQLITE_NOMEM);

    // Mesmo que este opcode não use strings dinâmicas no resultado, as colunas
    // podem virar dinâmicas se o usuário chamar sqlite3_column_text16(), que
    // traduz para UTF-16.
    release_mem_array(&p.a_mem[1..9]);

    if p.rc == SQLITE_NOMEM {
        // Acontece se um malloc() dentro de sqlite3_column_text() ou
        // sqlite3_column_text16() falhou.
        oom_fault(&mut db_ref.borrow_mut());
        return SQLITE_ERROR;
    }

    // As 8 primeiras células de memória são o conjunto de resultados; a 9ª guarda
    // o vetor de subprogramas de trigger (aqui, `p.ap_sub`).
    let mut ap_sub = std::mem::take(&mut p.ap_sub);
    if b_list_subprogs {
        debug_assert!(p.n_mem > 9);
    }

    // Descobre qual opcode mostrar em seguida.
    let mut i: i32 = 0;
    let mut a_op: Option<SubProgramRef> = None;
    let mut pc = p.pc;
    let explain = p.explain;
    let mut rc = vdbe_next_opcode(
        p,
        if b_list_subprogs { Some(&mut ap_sub) } else { None },
        (explain == 2) as i32,
        &mut pc,
        &mut i,
        &mut a_op,
    );
    p.pc = pc;
    p.ap_sub = ap_sub;

    if rc == SQLITE_OK {
        if db_ref.borrow().is_interrupted != 0 {
            p.rc = SQLITE_INTERRUPT;
            rc = SQLITE_ERROR;
            let z = err_str(p.rc);
            vdbe_error(p, z, &[]);
        } else {
            // Valores do opcode e o texto de P4, vistos no vetor certo.
            let view = |op: &Op| -> (u8, i32, i32, i32, u16, Option<Vec<u8>>) {
                (op.opcode, op.p1, op.p2, op.p3, op.p5, vdbe_display_p4(&db_ref, op))
            };
            let (opcode, p1, p2, p3, p5, z_p4) = match &a_op {
                None => view(&p.a_op[i as usize]),
                Some(sp) => view(&sp.borrow().a_op[i as usize]),
            };
            let z_p4_ref: Option<&[u8]> = z_p4.as_deref();
            let p_mem = p.a_mem[1..9].to_vec();
            if p.explain == 2 {
                vdbe_mem_set_int64(&mut p_mem[0].borrow_mut(), p1 as i64);
                vdbe_mem_set_int64(&mut p_mem[1].borrow_mut(), p2 as i64);
                vdbe_mem_set_int64(&mut p_mem[2].borrow_mut(), p3 as i64);
                vdbe_mem_set_str(&mut p_mem[3].borrow_mut(), z_p4_ref, -1, SQLITE_UTF8, SQLITE_TRANSIENT);
                debug_assert!(p.n_res_column == 4);
            } else {
                vdbe_mem_set_int64(&mut p_mem[0].borrow_mut(), i as i64);
                vdbe_mem_set_str(
                    &mut p_mem[1].borrow_mut(),
                    Some(opcode_name(opcode as i32).as_bytes()),
                    -1,
                    SQLITE_UTF8,
                    SQLITE_STATIC,
                );
                vdbe_mem_set_int64(&mut p_mem[2].borrow_mut(), p1 as i64);
                vdbe_mem_set_int64(&mut p_mem[3].borrow_mut(), p2 as i64);
                vdbe_mem_set_int64(&mut p_mem[4].borrow_mut(), p3 as i64);
                // pMem+5, do p4, é feito por último.
                vdbe_mem_set_int64(&mut p_mem[6].borrow_mut(), p5 as i64);
                vdbe_mem_set_null(&mut p_mem[7].borrow_mut());
                vdbe_mem_set_str(&mut p_mem[5].borrow_mut(), z_p4_ref, -1, SQLITE_UTF8, SQLITE_TRANSIENT);
                debug_assert!(p.n_res_column == 8);
            }
            p.p_result_row = Some(p_mem[0].clone());
            if db_ref.borrow().malloc_failed != 0 {
                p.rc = SQLITE_NOMEM;
                rc = SQLITE_ERROR;
            } else {
                p.rc = SQLITE_OK;
                rc = SQLITE_ROW;
            }
        }
    }
    rc
}

/// Volta o VDBE ao começo, em preparação para rodá-lo.
pub fn vdbe_rewind(p: &mut Vdbe) {
    debug_assert!(
        p.e_vdbe_state == VDBE_INIT_STATE
            || p.e_vdbe_state == VDBE_READY_STATE
            || p.e_vdbe_state == VDBE_HALT_STATE
    );

    // Precisa haver pelo menos um opcode.
    debug_assert!(p.n_op > 0);

    p.e_vdbe_state = VDBE_READY_STATE;

    p.pc = -1;
    p.rc = SQLITE_OK;
    p.error_action = OE_ABORT;
    p.n_change = 0;
    p.cache_ctr = 1;
    p.min_write_file_format = 255;
    p.i_statement = 0;
    p.n_fk_constraint = 0;
}

/// Prepara uma máquina virtual para execução pela primeira vez depois de criada.
/// Isso envolve alocar registradores e inicializar o contador de programa.
/// Depois de preparado, o VDBE pode ser executado por uma ou mais chamadas a
/// vdbe_exec().
///
/// Esta função pode ser chamada exatamente uma vez em cada máquina virtual.
/// Depois dela a VM está "empacotada" e pronta para rodar, e novas chamadas a
/// vdbe_add_op() são proibidas. A rotina desconecta o Vdbe do objeto Parse que
/// o gerou, para o Vdbe virar uma entidade independente e o Parse poder ser
/// destruído.
///
/// Use vdbe_rewind() para devolver uma máquina virtual ao estado inicial depois
/// de rodada.
pub fn vdbe_make_ready(p: &mut Vdbe, p_parse: &mut Parse) {
    debug_assert!(p.n_op > 0);
    debug_assert!(p.e_vdbe_state == VDBE_INIT_STATE);
    let v_list = std::mem::take(&mut p_parse.p_vlist);
    p.p_v_list = if v_list.is_empty() { None } else { Some(v_list) };
    let db = p.db.upgrade().expect("Vdbe sem conexão");
    debug_assert!(db.borrow().malloc_failed == 0);
    let mut n_var = p_parse.n_var as i32;
    let mut n_mem = p_parse.n_mem;
    let n_cursor = p_parse.n_tab;
    let mut n_arg = p_parse.n_max_arg;

    // Cada cursor usa uma célula de memória. O primeiro (cursor 0) pode usar
    // aMem[0], que o programa do VDBE não usa de outro modo. Reserva espaço no
    // fim de aMem[] para os cursores 1 em diante. Veja também allocate_cursor().
    n_mem += n_cursor;
    if n_cursor == 0 && n_mem > 0 {
        n_mem += 1; // Espaço para aMem[0] mesmo sem uso.
    }

    resolve_p2_values(p, &mut n_arg);
    p.uses_stmt_journal = (p_parse.is_multi_write != 0 && p_parse.may_abort != 0) as _;
    if p_parse.explain != 0 {
        if n_mem < 10 {
            n_mem = 10;
        }
        p.explain = p_parse.explain as _;
        p.n_res_column = (12 - 4 * p_parse.explain as i32) as u16;
    }
    p.expired = 0;

    // Memória de registradores, parâmetros, cursores etc. No C, vem em uma ou duas
    // passadas que reaproveitam a sobra do vetor de opcodes; aqui cada vetor é
    // alocado direto.
    let new_cells = |n: i32| -> Vec<MemRef> {
        (0..n.max(0)).map(|_| Rc::new(RefCell::new(Mem::default()))).collect()
    };
    p.a_mem = new_cells(n_mem);
    p.a_var = new_cells(n_var);
    p.ap_arg = Vec::with_capacity(n_arg.max(0) as usize);
    p.ap_csr = vec![None; n_cursor.max(0) as usize];

    if db.borrow().malloc_failed != 0 {
        n_var = 0;
        p.n_var = 0;
        p.n_cursor = 0;
        p.n_mem = 0;
    } else {
        p.n_cursor = n_cursor;
        p.n_var = n_var as YnVar;
        init_mem_array(&p.a_var, &db, MEM_NULL);
        p.n_mem = n_mem;
        init_mem_array(&p.a_mem, &db, MEM_UNDEFINED);
        // apCsr já nasce todo None (o memset do C).
    }
    let _ = n_var;
    vdbe_rewind(p);
}


// ---- part_007.rs ----

// Trecho 7 de vdbeaux.c (sqlite 3.46.1).
//
// Desvios do C por causa do modelo sem ponteiros:
//  * `vdbe_frame_restore` e `vdbe_frame_delete` (esta no trecho 6) recebem o `&mut Vdbe` dono
//    do quadro como primeiro argumento. No C o quadro chega ao Vdbe por `pFrame->v`; aqui os
//    chamadores já seguram o Vdbe emprestado (`RefCell`) e reabrir `pFrame.v` entraria em pânico.
//  * `vdbe_commit` recebe a conexão como `&Sqlite3Ref`: os hooks e os métodos de tabela virtual
//    voltam a olhar a conexão, então nenhum empréstimo de `db` pode ficar aberto durante eles.

/// Fecha um cursor do VDBE e libera todos os recursos que o cursor mantém.
pub fn vdbe_free_cursor(p: &mut Vdbe, p_cx: Option<&mut VdbeCursor>) {
    if let Some(p_cx) = p_cx {
        vdbe_free_cursor_nn(p, p_cx);
    }
}

/// Parte do `vdbe_free_cursor_nn` para cursores com cache de coluna.
fn free_cursor_with_cache(p: &mut Vdbe, p_cx: &mut VdbeCursor) {
    let p_cache = p_cx.p_cache.take();
    debug_assert!(p_cx.col_cache != 0);
    p_cx.col_cache = 0;
    if let Some(mut cache) = p_cache {
        // rc_str_unref(pCValue): a contagem de referências é a do Rc, o drop a diminui.
        cache.p_c_value = None;
        // sqlite3DbFree(p->db, pCache): o Box cai aqui.
    }
    vdbe_free_cursor_nn(p, p_cx);
}

/// Fecha um cursor do VDBE.
/// Esta rotina sempre é chamada quando um cursor é liberado, quer ele tenha sido
/// aberto ou não. Se o cursor nunca foi aberto, os campos são todos nulos ou zero
/// e a rotina é uma no-op.
pub fn vdbe_free_cursor_nn(p: &mut Vdbe, p_cx: &mut VdbeCursor) {
    if p_cx.col_cache != 0 {
        free_cursor_with_cache(p, p_cx);
        return;
    }
    match p_cx.e_cur_type {
        CURTYPE_SORTER => {
            let db = p.db.upgrade().expect("Vdbe sem conexão");
            vdbe_sorter_close(&db, p_cx);
        }
        CURTYPE_BTREE => {
            if let VdbeCursorCursorUnion::PCursor(p_cursor) = &mut p_cx.uc {
                btree_close_cursor(p_cursor);
            } else {
                debug_assert!(false, "CURTYPE_BTREE sem cursor");
            }
        }
        CURTYPE_VTAB => {
            if let VdbeCursorCursorUnion::PVCur(p_v_cur) = &mut p_cx.uc {
                let p_vtab = p_v_cur.p_vtab.clone().expect("cursor de vtab sem vtab");
                let x_close = {
                    let mut vtab = p_vtab.borrow_mut();
                    debug_assert!(vtab.n_ref > 0);
                    vtab.n_ref -= 1;
                    vtab.p_module.as_ref().and_then(|m| m.x_close).expect("módulo sem xClose")
                };
                x_close(p_v_cur);
            }
        }
        _ => {}
    }
}

/// Fecha todos os cursores do quadro corrente.
fn close_cursors_in_frame(p: &mut Vdbe) {
    for i in 0..p.n_cursor as usize {
        if let Some(p_c) = p.ap_csr[i].take() {
            vdbe_free_cursor_nn(p, &mut p_c.borrow_mut());
        }
    }
}

/// Copia os valores guardados na estrutura VdbeFrame para o seu Vdbe. Isto é
/// usado, por exemplo, quando um subprograma de trigger termina, para devolver o
/// controle ao programa principal.
pub fn vdbe_frame_restore(v: &mut Vdbe, p_frame: &VdbeFrameRef) -> i32 {
    close_cursors_in_frame(v);
    let db = v.db.upgrade().expect("Vdbe sem conexão");
    let mut f = p_frame.borrow_mut();
    v.a_op = std::mem::take(&mut f.a_op);
    v.n_op = f.n_op;
    v.a_mem = std::mem::take(&mut f.a_mem);
    v.n_mem = f.n_mem;
    v.ap_csr = std::mem::take(&mut f.ap_csr);
    v.n_cursor = f.n_cursor;
    {
        let mut d = db.borrow_mut();
        d.last_rowid = f.last_rowid;
        d.n_change = f.n_db_change;
    }
    v.n_change = f.n_change;
    vdbe_delete_aux_data(&db, &mut v.p_aux_data, -1, 0);
    v.p_aux_data = f.p_aux_data.take();
    f.pc
}

/// Fecha todos os cursores.
///
/// Também libera qualquer memória dinâmica mantida pela VM no vetor de células
/// Vdbe.a_mem. Isto é necessário porque o vetor pode conter ponteiros para
/// objetos VdbeFrame, que por sua vez podem conter ponteiros para cursores abertos.
pub fn close_all_cursors(p: &mut Vdbe) {
    if let Some(top) = p.p_frame.clone() {
        let mut p_frame = top;
        loop {
            let parent = p_frame.borrow().p_parent.clone();
            match parent {
                Some(pa) => p_frame = pa,
                None => break,
            }
        }
        vdbe_frame_restore(p, &p_frame);
        p.p_frame = None;
        p.n_frame = 0;
    }
    debug_assert!(p.n_frame == 0);
    close_cursors_in_frame(p);
    release_mem_array(&p.a_mem[..p.n_mem as usize]);
    // O blob de subprogramas do EXPLAIN vivia em aMem[9] e acabou de ser liberado.
    p.ap_sub.clear();
    while let Some(p_del) = p.p_del_frame.clone() {
        let parent = p_del.borrow().p_parent.clone();
        p.p_del_frame = parent;
        vdbe_frame_delete(p, &p_del);
    }

    // Apaga qualquer alocação auxdata feita pela VM.
    if p.p_aux_data.is_some() {
        let db = p.db.upgrade().expect("Vdbe sem conexão");
        vdbe_delete_aux_data(&db, &mut p.p_aux_data, -1, 0);
    }
    debug_assert!(p.p_aux_data.is_none());
}

/// Define o número de colunas de resultado que a declaração SQL devolverá. Isto
/// agora é definido na compilação, e não durante a execução do programa, para que
/// sqlite3_column_count() possa ser chamada antes de sqlite3_step().
pub fn vdbe_set_num_cols(p: &mut Vdbe, n_res_column: i32) {
    let db = p.db.upgrade().expect("Vdbe sem conexão");
    if p.n_res_alloc != 0 {
        let n_old = p.n_res_alloc as usize * COLNAME_N as usize;
        release_mem_array(&p.a_col_name[..n_old]);
        p.a_col_name = Vec::new();
    }
    let n = n_res_column as usize * COLNAME_N as usize;
    p.n_res_column = n_res_column as u16;
    p.n_res_alloc = n_res_column as u16;
    p.a_col_name = (0..n).map(|_| Rc::new(RefCell::new(Mem::default()))).collect();
    init_mem_array(&p.a_col_name, &db, MEM_NULL);
}

/// Define o nome da coluna `idx` a devolver pela declaração SQL. `z_name` é um
/// texto terminado em nulo.
///
/// Esta chamada deve vir depois de vdbe_set_num_cols().
///
/// O último parâmetro, `x_del`, diz como o texto é guardado: SQLITE_STATIC ou
/// SQLITE_TRANSIENT (o SQLITE_DYNAMIC do C é uma cópia que o Mem passa a possuir).
pub fn vdbe_set_col_name(
    p: &mut Vdbe,
    idx: i32,
    var: i32,
    z_name: Option<&[u8]>,
    x_del: Destructor,
) -> i32 {
    debug_assert!(idx < p.n_res_alloc as i32);
    debug_assert!(var < COLNAME_N as i32);
    let db = p.db.upgrade().expect("Vdbe sem conexão");
    if db.borrow().malloc_failed != 0 {
        return SQLITE_NOMEM_BKPT;
    }
    debug_assert!(!p.a_col_name.is_empty());
    let p_col_name = p.a_col_name[(idx + var * p.n_res_alloc as i32) as usize].clone();
    let mut p_col_name = p_col_name.borrow_mut();
    let rc = vdbe_mem_set_str(&mut p_col_name, z_name, -1, SQLITE_UTF8, x_del);
    debug_assert!(rc != 0 || z_name.is_none() || (p_col_name.flags & MEM_TERM) != 0);
    rc
}

/// Uma transação de leitura ou escrita pode ou não estar ativa na conexão `db`.
/// Se houver uma ativa, faz o commit. Se houver uma transação de escrita que
/// cobre mais de um arquivo de banco, esta rotina cuida do truque do super-journal.
pub fn vdbe_commit(db: &Sqlite3Ref, p: &mut Vdbe) -> i32 {
    // Quantos bancos têm transação de escrita ativa e são candidatos a commit em
    // duas fases com super-journal.
    let mut n_trans = 0;
    let mut need_xcommit = false;

    // Antes de tudo, chama xSync() das tabelas virtuais escritas nesta transação.
    // Isso precisa vir antes de decidir se um super-journal é necessário, porque
    // um xSync() pode anexar um banco à transação.
    let mut rc = vtab_sync(db, p);

    // Este laço decide (a) se o hook de commit deve ser chamado e (b) quantos
    // arquivos de banco, fora o temp, têm transação de escrita aberta. O (b)
    // importa porque, com mais de um, o commit atômico exige um super-journal.
    let n_db = db.borrow().n_db as usize;
    let mut i = 0;
    while rc == SQLITE_OK && i < n_db {
        let p_bt = db.borrow().a_db[i].p_bt.clone();
        if btree_txn_state(p_bt.as_ref()) == SQLITE_TXN_WRITE {
            // Se um banco precisa de super-journal depende do modo de journal (entre
            // outras coisas). Esta matriz diz quais modos usam e quais não.
            const A_MJ_NEEDED: [u8; 6] = [
                1, // DELETE
                1, // PERSIST
                0, // OFF
                1, // TRUNCATE
                0, // MEMORY
                0, // WAL
            ];
            let p_bt = p_bt.expect("transação de escrita sem Btree");
            need_xcommit = true;
            btree_enter(&mut p_bt.borrow_mut());
            let p_pager = btree_pager(&p_bt.borrow());
            let safety_level = db.borrow().a_db[i].safety_level;
            let journal_mode = pager_get_journal_mode(&p_pager.borrow());
            if safety_level != PAGER_SYNCHRONOUS_OFF
                && A_MJ_NEEDED[journal_mode as usize] != 0
                && pager_is_memdb(&p_pager.borrow()) == 0
            {
                debug_assert!(i != 1);
                n_trans += 1;
            }
            rc = pager_exclusive_lock(&mut p_pager.borrow_mut());
            btree_leave(&mut p_bt.borrow_mut());
        }
        i += 1;
    }
    if rc != SQLITE_OK {
        return rc;
    }

    // Se houve alguma transação de escrita, chama o hook de commit.
    let x_commit_callback = db.borrow().x_commit_callback.clone();
    if need_xcommit {
        if let Some(x_commit) = x_commit_callback {
            let p_commit_arg = db.borrow().p_commit_arg.clone();
            rc = x_commit(&p_commit_arg);
            if rc != 0 {
                return SQLITE_CONSTRAINT_COMMITHOOK;
            }
        }
    }

    // Caso simples: no máximo um arquivo de banco (sem contar o TEMP) tem
    // transação ativa, e não precisa de super-journal.
    //
    // Se o nome de arquivo do banco principal é vazio, o banco é :memory: ou um
    // arquivo temporário. Então não há commit atômico de vários arquivos e vale
    // o caso simples também.
    let p_main_bt = db.borrow().a_db[0].p_bt.clone();
    let z_main_file = match &p_main_bt {
        Some(b) => btree_get_filename(b),
        None => Vec::new(),
    };
    if 0 == strlen30(Some(&z_main_file)) || n_trans <= 1 {
        let mut i = 0;
        while rc == SQLITE_OK && i < n_db {
            let p_bt = db.borrow().a_db[i].p_bt.clone();
            if let Some(p_bt) = p_bt {
                rc = btree_commit_phase_one(&mut p_bt.borrow_mut(), None);
            }
            i += 1;
        }

        // Só faz o commit se todos os bancos terminaram a fase 1. Se um
        // btree_commit_phase_one() falha, houve erro de E/S ao apagar ou truncar
        // um journal, o que é raro mas possível: abandona e devolve o erro.
        let mut i = 0;
        while rc == SQLITE_OK && i < n_db {
            let p_bt = db.borrow().a_db[i].p_bt.clone();
            if let Some(p_bt) = p_bt {
                rc = btree_commit_phase_two(&mut p_bt.borrow_mut(), 0);
            }
            i += 1;
        }
        if rc == SQLITE_OK {
            vtab_commit(db);
        }
    } else {
        // Caso complexo: há uma transação de escrita em vários arquivos. Ela exige
        // um super-journal para ser confirmada atomicamente.
        let p_vfs = db.borrow().p_vfs.clone().expect("conexão sem VFS");
        let n_main_file = z_main_file.len();
        let mut retry_count = 0;
        let mut z_super: Vec<u8> = z_main_file.clone(); // Nome do super-journal.
        let mut res: i32 = 0;

        // Escolhe um nome para o super-journal.
        loop {
            if retry_count != 0 {
                if retry_count > 100 {
                    let mut ap = VaList::new();
                    ap.args.push_back(VaArg::Text(Some(z_super.clone())));
                    log(SQLITE_FULL, b"MJ delete: %s", &mut ap);
                    os_delete(&*p_vfs, &z_super, 0);
                    break;
                } else if retry_count == 1 {
                    let mut ap = VaList::new();
                    ap.args.push_back(VaArg::Text(Some(z_super.clone())));
                    log(SQLITE_FULL, b"MJ collide: %s", &mut ap);
                }
            }
            retry_count += 1;
            let mut buf = [0u8; 4];
            randomness(4, &mut buf);
            let i_random = u32::from_ne_bytes(buf);
            z_super.truncate(n_main_file);
            z_super.extend_from_slice(
                format!("-mj{:06X}9{:02X}", (i_random >> 8) & 0xffffff, i_random & 0xff).as_bytes(),
            );
            // O antepenúltimo caractere do nome do super-journal deve ser "9" para
            // evitar colisões de nome com arquivos 8+3.
            debug_assert!(z_super[z_super.len() - 3] == b'9');
            file_suffix3(&z_main_file, &mut z_super);
            rc = os_access(&*p_vfs, &z_super, SQLITE_ACCESS_EXISTS, &mut res);
            if !(rc == SQLITE_OK && res != 0) {
                break;
            }
        }
        let mut p_super_jrnl: Option<Box<Sqlite3File>> = None;
        if rc == SQLITE_OK {
            // Abre o super-journal.
            rc = os_open_malloc(
                &*p_vfs,
                Some(&z_super),
                &mut p_super_jrnl,
                SQLITE_OPEN_READWRITE | SQLITE_OPEN_CREATE | SQLITE_OPEN_EXCLUSIVE | SQLITE_OPEN_SUPER_JOURNAL,
                None,
            );
        }
        if rc != SQLITE_OK {
            return rc;
        }
        let mut jrnl = p_super_jrnl.take().expect("super-journal aberto sem arquivo");

        // Escreve no super-journal o nome do arquivo de cada banco da transação.
        // Se der erro aqui, fecha e apaga o super-journal. Os journals individuais
        // ainda têm 'nulo' como ponteiro de super-journal, então desfazem sozinhos.
        let mut offset: i64 = 0;
        for i in 0..n_db {
            let p_bt = db.borrow().a_db[i].p_bt.clone();
            if btree_txn_state(p_bt.as_ref()) == SQLITE_TXN_WRITE {
                let z_file = btree_get_journalname(p_bt.as_ref().expect("transação sem Btree"));
                if z_file.is_empty() {
                    continue; // Ignora os bancos TEMP e :memory:.
                }
                debug_assert!(z_file[0] != 0);
                let mut data = z_file.clone();
                data.push(0);
                rc = os_write(&mut jrnl, &data, offset);
                offset += data.len() as i64;
                if rc != SQLITE_OK {
                    os_close_free(jrnl);
                    os_delete(&*p_vfs, &z_super, 0);
                    return rc;
                }
            }
        }

        // Sincroniza o super-journal. Se a flag IOCAP_SEQUENTIAL do dispositivo
        // está ligada, isso não é necessário.
        if 0 == (os_device_characteristics(&mut jrnl) & SQLITE_IOCAP_SEQUENTIAL) {
            rc = os_sync(&mut jrnl, SQLITE_SYNC_NORMAL);
            if rc != SQLITE_OK {
                os_close_free(jrnl);
                os_delete(&*p_vfs, &z_super, 0);
                return rc;
            }
        }

        // Sincroniza todos os arquivos de banco da transação. A mesma chamada grava
        // o ponteiro do super-journal em cada journal individual. Se der erro aqui,
        // não apaga o super-journal.
        //
        // Se o erro vier da primeira chamada a btree_commit_phase_one(), o
        // super-journal pode ficar órfão. Mas não dá para apagá-lo, caso o nome
        // dele já tenha sido gravado no journal antes da falha.
        let mut i = 0;
        while rc == SQLITE_OK && i < n_db {
            let p_bt = db.borrow().a_db[i].p_bt.clone();
            if let Some(p_bt) = p_bt {
                rc = btree_commit_phase_one(&mut p_bt.borrow_mut(), Some(&z_super));
            }
            i += 1;
        }
        os_close_free(jrnl);
        debug_assert!(rc != SQLITE_BUSY);
        if rc != SQLITE_OK {
            return rc;
        }

        // Apaga o super-journal. Isso confirma a transação. Depois disso o
        // diretório é sincronizado de novo antes de apagar os arquivos individuais.
        rc = os_delete(&*p_vfs, &z_super, 1);
        if rc != 0 {
            return rc;
        }

        // Todos os arquivos e diretórios já foram sincronizados, então as chamadas
        // a btree_commit_phase_two() abaixo só fecham arquivos e apagam ou truncam
        // journals. Se algo falhar aqui não importa: a integridade da transação já
        // está garantida, só podem sobrar journals 'frios'. Devolver erro não ajuda.
        disable_simulated_io_errors();
        begin_benign_malloc();
        for i in 0..n_db {
            let p_bt = db.borrow().a_db[i].p_bt.clone();
            if let Some(p_bt) = p_bt {
                btree_commit_phase_two(&mut p_bt.borrow_mut(), 1);
            }
        }
        end_benign_malloc();
        enable_simulated_io_errors();

        vtab_commit(db);
    }

    rc
}


// ---- part_008.rs ----

// A função checkActiveVdbeCnt() só faz asserções (e é macro vazia sob NDEBUG,
// que é como o Debian compila). Os asserts do C não são traduzidos, então ela
// não existe aqui e cada chamada some.

/// Obtém a conexão dona da VM. O ponteiro de volta é fraco (ver `Vdbe.db`).
#[inline]
fn halt_db(p: &Vdbe) -> Sqlite3Ref {
    p.db.upgrade().expect("Vdbe.db deve apontar para uma conexão viva")
}

/// Se o Vdbe passado como primeiro argumento abriu uma transação de declaração,
/// fecha-a agora. O argumento `e_op` deve ser SAVEPOINT_ROLLBACK ou
/// SAVEPOINT_RELEASE. Com SAVEPOINT_ROLLBACK a transação de declaração sofre
/// rollback; com SAVEPOINT_RELEASE ela é confirmada.
///
/// Se ocorrer erro de E/S, devolve um código SQLITE_IOERR_XXX. Senão SQLITE_OK.
fn vdbe_close_statement_static(p: &mut Vdbe, e_op: i32) -> i32 {
    let db_rc = halt_db(p);
    let mut rc = SQLITE_OK;
    let i_savepoint = p.i_statement - 1;

    let n_db = db_rc.borrow().n_db;
    for i in 0..n_db {
        let mut rc2 = SQLITE_OK;
        let p_bt = db_rc.borrow().a_db[i as usize].p_bt.clone();
        if let Some(p_bt) = p_bt {
            if e_op == SAVEPOINT_ROLLBACK {
                rc2 = btree_savepoint(&p_bt, SAVEPOINT_ROLLBACK, i_savepoint);
            }
            if rc2 == SQLITE_OK {
                rc2 = btree_savepoint(&p_bt, SAVEPOINT_RELEASE, i_savepoint);
            }
            if rc == SQLITE_OK {
                rc = rc2;
            }
        }
    }
    db_rc.borrow_mut().n_statement -= 1;
    p.i_statement = 0;

    if rc == SQLITE_OK {
        if e_op == SAVEPOINT_ROLLBACK {
            rc = vtab_savepoint(&mut db_rc.borrow_mut(), SAVEPOINT_ROLLBACK, i_savepoint);
        }
        if rc == SQLITE_OK {
            rc = vtab_savepoint(&mut db_rc.borrow_mut(), SAVEPOINT_RELEASE, i_savepoint);
        }
    }

    // Se a transação de declaração sofre rollback, restaura também o contador de
    // restrições diferidas do handle de banco de dados para o valor que tinha
    // quando a transação de declaração foi aberta.
    if e_op == SAVEPOINT_ROLLBACK {
        let mut db = db_rc.borrow_mut();
        db.n_deferred_cons = p.n_stmt_def_cons;
        db.n_deferred_imm_cons = p.n_stmt_def_imm_cons;
    }
    rc
}

pub fn vdbe_close_statement(p: &mut Vdbe, e_op: i32) -> i32 {
    let db_rc = halt_db(p);
    let has_stmt = db_rc.borrow().n_statement != 0;
    if has_stmt && p.i_statement != 0 {
        return vdbe_close_statement_static(p, e_op);
    }
    SQLITE_OK
}

/// Chamada quando uma transação aberta pelo handle de banco de dados associado à
/// VM passada como argumento está prestes a sofrer commit. Se há violações
/// pendentes de chave estrangeira diferida, devolve SQLITE_ERROR. Senão,
/// SQLITE_OK.
///
/// Se há violações de FK pendentes e a função devolve SQLITE_ERROR, define o
/// resultado da VM como SQLITE_CONSTRAINT_FOREIGNKEY e escreve uma mensagem de
/// erro nela. Depois devolve SQLITE_ERROR.
pub fn vdbe_check_fk(p: &mut Vdbe, deferred: i32) -> i32 {
    let (n_deferred_cons, n_deferred_imm_cons) = {
        let db_rc = halt_db(p);
        let db = db_rc.borrow();
        (db.n_deferred_cons, db.n_deferred_imm_cons)
    };
    if (deferred != 0 && (n_deferred_cons + n_deferred_imm_cons) > 0)
        || (deferred == 0 && p.n_fk_constraint > 0)
    {
        p.rc = SQLITE_CONSTRAINT_FOREIGNKEY;
        p.error_action = OE_ABORT;
        vdbe_error(p, b"FOREIGN KEY constraint failed");
        if (p.prep_flags as u32 & SQLITE_PREPARE_SAVESQL) == 0 {
            return SQLITE_ERROR;
        }
        return SQLITE_CONSTRAINT_FOREIGNKEY;
    }
    SQLITE_OK
}

/// Chamada quando uma VDBE tenta parar (halt). Se a VDBE fez mudanças e está em
/// modo autocommit, faz commit delas. Se é preciso rollback, faz o rollback.
///
/// Esta rotina é o único jeito de mover o eOpenState de uma VM de
/// SQLITE_STATE_RUN para SQLITE_STATE_HALT. É inofensivo chamá-la numa VM que já
/// está em SQLITE_STATE_HALT.
///
/// Devolve um código de erro. Se o commit não pôde terminar por contenção de
/// bloqueio, devolve SQLITE_BUSY. Se SQLITE_BUSY é devolvido, o fechamento não
/// aconteceu e precisa ser repetido.
pub fn vdbe_halt(p: &mut Vdbe) -> i32 {
    let db_rc = halt_db(p);
    let mut rc: i32;

    // Esta função contém a lógica que decide se uma declaração ou transação sofre
    // commit ou rollback como resultado da execução desta máquina virtual.
    //
    // Se ocorrer qualquer um dos erros abaixo:
    //
    //     SQLITE_NOMEM
    //     SQLITE_IOERR
    //     SQLITE_FULL
    //     SQLITE_INTERRUPT
    //
    // então o cache interno pode ter ficado inconsistente. É preciso fazer
    // rollback da transação de declaração, se houver, ou da transação completa se
    // não houver transação de declaração.

    if db_rc.borrow().malloc_failed != 0 {
        p.rc = SQLITE_NOMEM_BKPT;
    }
    close_all_cursors(p);

    // Nenhum commit ou rollback é necessário se o programa nunca começou ou se o
    // SQL não lê nem escreve arquivo de banco de dados.
    if p.b_is_reader != 0 {
        let mrc: i32; // Código de erro primário de p.rc
        let mut e_statement_op: i32 = 0;
        let is_special_error: bool; // Verdadeiro se for um erro "especial"

        // Bloqueia todas as btrees usadas pela declaração
        vdbe_enter(p);

        // Verifica se é um dos erros especiais
        if p.rc != 0 {
            mrc = p.rc & 0xff;
            is_special_error = mrc == SQLITE_NOMEM
                || mrc == SQLITE_IOERR
                || mrc == SQLITE_INTERRUPT
                || mrc == SQLITE_FULL;
        } else {
            mrc = 0;
            is_special_error = false;
        }
        if is_special_error {
            // Se a consulta era somente leitura e o código de erro é
            // SQLITE_INTERRUPT, nenhum rollback é necessário. Senão, ao menos uma
            // transação de savepoint precisa sofrer rollback para restaurar o banco
            // de dados a um estado consistente.
            //
            // Mesmo que a declaração seja somente leitura, é importante fazer uma
            // operação de rollback de declaração ou de transação. Se o erro
            // ocorreu ao escrever no journal, no sub-journal ou no arquivo de banco
            // de dados como parte do esforço de liberar espaço de cache (ver a
            // função pagerStress() em pager.c), o rollback é necessário para
            // restaurar o pager a um estado consistente.
            if p.read_only == 0 || mrc != SQLITE_INTERRUPT {
                if (mrc == SQLITE_NOMEM || mrc == SQLITE_FULL) && p.uses_stmt_journal != 0 {
                    e_statement_op = SAVEPOINT_ROLLBACK;
                } else {
                    // Somos forçados a fazer rollback da transação ativa. Antes
                    // disso, aborta qualquer outra declaração ativa deste handle.
                    {
                        let mut db = db_rc.borrow_mut();
                        rollback_all(&mut db, SQLITE_ABORT_ROLLBACK);
                        close_savepoints(&mut db);
                        db.auto_commit = 1;
                    }
                    p.n_change = 0;
                }
            }
        }

        // Verifica violações imediatas de chave estrangeira.
        if p.rc == SQLITE_OK || (p.error_action == OE_FAIL && !is_special_error) {
            let _ = vdbe_check_fk(p, 0);
        }

        // Se o flag de auto-commit está ligado e esta é a única VM escritora
        // ativa, faz commit ou rollback da transação atual.
        //
        // Nota: este bloco também roda se ocorreu um dos erros especiais tratados
        // acima.
        let (in_sync, auto_commit, n_vdbe_write) = {
            let db = db_rc.borrow();
            (vtab_in_sync(&db), db.auto_commit, db.n_vdbe_write)
        };
        if !in_sync && auto_commit != 0 && n_vdbe_write == (p.read_only == 0) as i32 {
            if p.rc == SQLITE_OK || (p.error_action == OE_FAIL && !is_special_error) {
                rc = vdbe_check_fk(p, 1);
                if rc != SQLITE_OK {
                    if p.read_only != 0 {
                        vdbe_leave(p);
                        return SQLITE_ERROR;
                    }
                    rc = SQLITE_CONSTRAINT_FOREIGNKEY;
                } else if (db_rc.borrow().flags & SQLITE_CORRUPT_RD_ONLY) != 0 {
                    rc = SQLITE_CORRUPT;
                    db_rc.borrow_mut().flags &= !SQLITE_CORRUPT_RD_ONLY;
                } else {
                    // O flag de auto-commit é verdadeiro, o programa da vdbe teve
                    // sucesso ou bateu numa restrição 'OR FAIL' e não há chaves
                    // estrangeiras diferidas segurando a transação. Isso significa
                    // que um commit é necessário.
                    rc = vdbe_commit(&mut db_rc.borrow_mut(), p);
                }
                if rc == SQLITE_BUSY && p.read_only != 0 {
                    vdbe_leave(p);
                    return SQLITE_BUSY;
                } else if rc != SQLITE_OK {
                    {
                        let mut db = db_rc.borrow_mut();
                        system_error(&mut db, rc);
                        p.rc = rc;
                        rollback_all(&mut db, SQLITE_OK);
                    }
                    p.n_change = 0;
                } else {
                    let mut db = db_rc.borrow_mut();
                    db.n_deferred_cons = 0;
                    db.n_deferred_imm_cons = 0;
                    db.flags &= !SQLITE_DEFER_FKS;
                    commit_internal_changes(&mut db);
                }
            } else if p.rc == SQLITE_SCHEMA && db_rc.borrow().n_vdbe_active > 1 {
                p.n_change = 0;
            } else {
                rollback_all(&mut db_rc.borrow_mut(), SQLITE_OK);
                p.n_change = 0;
            }
            db_rc.borrow_mut().n_statement = 0;
        } else if e_statement_op == 0 {
            if p.rc == SQLITE_OK || p.error_action == OE_FAIL {
                e_statement_op = SAVEPOINT_RELEASE;
            } else if p.error_action == OE_ABORT {
                e_statement_op = SAVEPOINT_ROLLBACK;
            } else {
                {
                    let mut db = db_rc.borrow_mut();
                    rollback_all(&mut db, SQLITE_ABORT_ROLLBACK);
                    close_savepoints(&mut db);
                    db.auto_commit = 1;
                }
                p.n_change = 0;
            }
        }

        // Se e_statement_op é diferente de zero, uma transação de declaração
        // precisa sofrer commit ou rollback. Chama vdbe_close_statement() para
        // isso. Se a operação devolver erro, e o código de erro atual da
        // declaração é SQLITE_OK ou SQLITE_CONSTRAINT, promove o código de erro
        // atual da declaração.
        if e_statement_op != 0 {
            rc = vdbe_close_statement(p, e_statement_op);
            if rc != 0 {
                if p.rc == SQLITE_OK || (p.rc & 0xff) == SQLITE_CONSTRAINT {
                    p.rc = rc;
                    p.z_err_msg = None;
                }
                {
                    let mut db = db_rc.borrow_mut();
                    rollback_all(&mut db, SQLITE_ABORT_ROLLBACK);
                    close_savepoints(&mut db);
                    db.auto_commit = 1;
                }
                p.n_change = 0;
            }
        }

        // Se foi um INSERT, UPDATE ou DELETE e nenhuma transação de declaração
        // sofreu rollback, atualiza o contador de mudanças da conexão de banco de
        // dados.
        if p.change_cnt_on != 0 {
            if e_statement_op != SAVEPOINT_ROLLBACK {
                vdbe_set_changes(&mut db_rc.borrow_mut(), p.n_change);
            } else {
                vdbe_set_changes(&mut db_rc.borrow_mut(), 0);
            }
            p.n_change = 0;
        }

        // Libera os bloqueios
        vdbe_leave(p);
    }

    // Paramos e fechamos a VM com sucesso. Registra esse fato.
    {
        let mut db = db_rc.borrow_mut();
        db.n_vdbe_active -= 1;
        if p.read_only == 0 {
            db.n_vdbe_write -= 1;
        }
        if p.b_is_reader != 0 {
            db.n_vdbe_read -= 1;
        }
    }
    p.e_vdbe_state = VDBE_HALT_STATE;
    if db_rc.borrow().malloc_failed != 0 {
        p.rc = SQLITE_NOMEM_BKPT;
    }

    // Se o flag de auto-commit está ligado, quaisquer bloqueios mantidos pela
    // conexão db foram liberados. Chama connection_unlocked() para invocar os
    // callbacks de unlock-notify necessários.
    if db_rc.borrow().auto_commit != 0 {
        connection_unlocked(&mut db_rc.borrow_mut());
    }

    if p.rc == SQLITE_BUSY {
        SQLITE_BUSY
    } else {
        SQLITE_OK
    }
}

/// Cada VDBE guarda o resultado da chamada mais recente a sqlite3_step() em
/// p.rc. Esta rotina volta esse resultado para SQLITE_OK.
pub fn vdbe_reset_step_result(p: &mut Vdbe) {
    p.rc = SQLITE_OK;
}

/// Copia o código de erro e a mensagem de erro pertencentes à VDBE passada como
/// primeiro argumento para o seu handle de banco de dados (para que sejam
/// devolvidos por chamadas a sqlite3_errcode() e sqlite3_errmsg()).
///
/// Esta função não limpa o código de erro nem a mensagem da VDBE, só os copia
/// para o handle de banco de dados.
pub fn vdbe_transfer_error(p: &mut Vdbe) -> i32 {
    let db_rc = halt_db(p);
    let mut db = db_rc.borrow_mut();
    let rc = p.rc;
    if let Some(z_err_msg) = &p.z_err_msg {
        db.b_benign_malloc = db.b_benign_malloc.wrapping_add(1);
        begin_benign_malloc();
        if db.p_err.is_none() {
            db.p_err = value_new(&mut db).map(|v| Rc::new(RefCell::new(*v)));
        }
        if let Some(p_err) = &db.p_err {
            value_set_str(&mut p_err.borrow_mut(), -1, z_err_msg, SQLITE_UTF8 as u8, None);
        }
        end_benign_malloc();
        db.b_benign_malloc = db.b_benign_malloc.wrapping_sub(1);
    } else if let Some(p_err) = &db.p_err {
        value_set_null(&mut p_err.borrow_mut());
    }
    db.err_code = rc;
    db.err_byte_offset = -1;
    rc
}


// ---- part_011.rs ----

// Trecho 11 de vdbeaux.c (sqlite 3.46.1).
//
// `vdbeAssertFieldCountWithinLimits` existe só sob SQLITE_DEBUG; no build do
// Debian 13 ela é a macro vazia `#define vdbeAssertFieldCountWithinLimits(A,B,C)`,
// então a função some e quem a chamaria não a chama. Os assert(), testcase() e
// o `vdbeRecordCompareDebug` também somem (NDEBUG), assim como `doubleLt` e
// `doubleEq`, que só existem sob SQLITE_COVERAGE_TEST ou SQLITE_DEBUG.

/// memcmp(3) sobre os `n` primeiros bytes: negativo, zero ou positivo conforme
/// a primeira diferença byte a byte (sem sinal), como no glibc.
pub fn memcmp(a: &[u8], b: &[u8], n: usize) -> i32 {
    for i in 0..n {
        if a[i] != b[i] {
            return (a[i] as i32) - (b[i] as i32);
        }
    }
    0
}

/// Os dois valores *pMem1 e *pMem2 são strings. Compara-os com a sequência de
/// ordenação pColl. Devolve negativo, zero ou positivo se *pMem1 for menor,
/// igual ou maior que *pMem2. `prc_err` recebe SQLITE_NOMEM em caso de falta
/// de memória.
fn vdbe_compare_mem_string(
    p_mem1: &Mem,
    p_mem2: &Mem,
    p_coll: &CollSeq,
    prc_err: Option<&mut u8>,
) -> i32 {
    if p_mem1.enc == p_coll.enc {
        // As strings já estão na codificação certa: chama a função de
        // comparação direto.
        match &p_coll.x_cmp {
            Some(x_cmp) => x_cmp(
                &p_coll.p_user,
                &p_mem1.z[..p_mem1.n as usize],
                &p_mem2.z[..p_mem2.n as usize],
            ),
            None => 0,
        }
    } else {
        let mut c1 = Mem::default();
        let mut c2 = Mem::default();
        vdbe_mem_init(&mut c1, p_mem1.db.clone(), MEM_NULL);
        vdbe_mem_init(&mut c2, p_mem1.db.clone(), MEM_NULL);
        vdbe_mem_shallow_copy(&mut c1, p_mem1, MEM_EPHEM);
        vdbe_mem_shallow_copy(&mut c2, p_mem2, MEM_EPHEM);
        let v1: Option<Vec<u8>> = value_text(Some(&mut c1), p_coll.enc).map(|s| s.to_vec());
        let v2: Option<Vec<u8>> = value_text(Some(&mut c2), p_coll.enc).map(|s| s.to_vec());
        let rc;
        match (v1, v2) {
            (Some(v1), Some(v2)) => {
                rc = match &p_coll.x_cmp {
                    Some(x_cmp) => x_cmp(
                        &p_coll.p_user,
                        &v1[..c1.n as usize],
                        &v2[..c2.n as usize],
                    ),
                    None => 0,
                };
            }
            _ => {
                if let Some(err) = prc_err {
                    *err = SQLITE_NOMEM_BKPT as u8;
                }
                rc = 0;
            }
        }
        vdbe_mem_release_malloc(&mut c1);
        vdbe_mem_release_malloc(&mut c2);
        rc
    }
}

/// O blob de entrada não tem MEM_ZERO. Devolve verdadeiro se ele pode ser um
/// zero-blob (os `n` primeiros bytes de `z` são todos zero).
fn is_all_zero(z: &[u8], n: i32) -> bool {
    for i in 0..n as usize {
        if z[i] != 0 {
            return false;
        }
    }
    true
}

/// Compara dois blobs. Devolve negativo, zero ou positivo se o primeiro for
/// menor, igual ou maior que o segundo. Se um for prefixo do outro, o mais
/// curto é o menor.
pub fn blob_compare(p_b1: &Mem, p_b2: &Mem) -> i32 {
    let n1 = p_b1.n;
    let n2 = p_b2.n;

    // Pode existir um Blob com conteúdo não zero seguido de conteúdo zero, mas
    // isso só ocorre em Blobs montados pelo OP_MakeRecord, que nunca chegam em
    // mem_compare().
    if ((p_b1.flags | p_b2.flags) & MEM_ZERO) != 0 {
        if (p_b1.flags & p_b2.flags & MEM_ZERO) != 0 {
            return p_b1.u.n_zero - p_b2.u.n_zero;
        } else if (p_b1.flags & MEM_ZERO) != 0 {
            if !is_all_zero(&p_b2.z, p_b2.n) {
                return -1;
            }
            return p_b1.u.n_zero - n2;
        } else {
            if !is_all_zero(&p_b1.z, p_b1.n) {
                return 1;
            }
            return n1 - p_b2.u.n_zero;
        }
    }
    let c = memcmp(&p_b1.z, &p_b2.z, (if n1 > n2 { n2 } else { n1 }) as usize);
    if c != 0 {
        return c;
    }
    n1 - n2
}

/// Compara um inteiro de 64 bits com um número de ponto flutuante. Devolve
/// negativo, zero ou positivo se o primeiro (i64) for menor, igual ou maior
/// que o segundo (double).
pub fn int_float_compare(i: i64, r: f64) -> i32 {
    if r.is_nan() {
        // O SQLite trata NaN como NULL, e todo inteiro é maior que NULL.
        return 1;
    }
    if SQLITE_CONFIG.b_use_long_double != 0 {
        // `long double` de 80 bits guarda qualquer i64 com exatidão, então a
        // comparação (x<r) ? -1 : (x>r) é a comparação matemática exata entre
        // i e r. r está dentro de i64 ou fora; a parte fracionária decide o
        // empate com a parte inteira truncada.
        if r < -9223372036854775808.0 {
            return 1;
        }
        if r >= 9223372036854775808.0 {
            return -1;
        }
        let y = r as i64;
        if i < y {
            return -1;
        }
        if i > y {
            return 1;
        }
        let frac = r - (y as f64);
        return if frac > 0.0 {
            -1
        } else if frac < 0.0 {
            1
        } else {
            0
        };
    } else {
        if r < -9223372036854775808.0 {
            return 1;
        }
        if r >= 9223372036854775808.0 {
            return -1;
        }
        let y = r as i64;
        if i < y {
            return -1;
        }
        if i > y {
            return 1;
        }
        let d = i as f64;
        if d < r {
            -1
        } else if d > r {
            1
        } else {
            0
        }
    }
}

/// Compara os valores de duas células de memória, devolvendo negativo, zero
/// ou positivo se pMem1 for menor, igual ou maior que pMem2. Ordem: NULLs
/// primeiro, depois números (inteiros e reais) por valor, depois texto pela
/// sequência de ordenação pColl e por fim blobs por memcmp(). Dois NULLs são
/// considerados iguais.
pub fn mem_compare(p_mem1: &Mem, p_mem2: &Mem, p_coll: Option<&CollSeq>) -> i32 {
    let f1 = p_mem1.flags;
    let f2 = p_mem2.flags;
    let combined_flags = f1 | f2;

    // Se um valor é NULL ele é menor que o outro. Se ambos são NULL, 0.
    if (combined_flags & MEM_NULL) != 0 {
        return ((f2 & MEM_NULL) as i32) - ((f1 & MEM_NULL) as i32);
    }

    // Pelo menos um dos dois é número.
    if (combined_flags & (MEM_INT | MEM_REAL | MEM_INTREAL)) != 0 {
        if (f1 & f2 & (MEM_INT | MEM_INTREAL)) != 0 {
            if p_mem1.u.i < p_mem2.u.i {
                return -1;
            }
            if p_mem1.u.i > p_mem2.u.i {
                return 1;
            }
            return 0;
        }
        if (f1 & f2 & MEM_REAL) != 0 {
            if p_mem1.u.r < p_mem2.u.r {
                return -1;
            }
            if p_mem1.u.r > p_mem2.u.r {
                return 1;
            }
            return 0;
        }
        if (f1 & (MEM_INT | MEM_INTREAL)) != 0 {
            if (f2 & MEM_REAL) != 0 {
                return int_float_compare(p_mem1.u.i, p_mem2.u.r);
            } else if (f2 & (MEM_INT | MEM_INTREAL)) != 0 {
                if p_mem1.u.i < p_mem2.u.i {
                    return -1;
                }
                if p_mem1.u.i > p_mem2.u.i {
                    return 1;
                }
                return 0;
            } else {
                return -1;
            }
        }
        if (f1 & MEM_REAL) != 0 {
            if (f2 & (MEM_INT | MEM_INTREAL)) != 0 {
                return -int_float_compare(p_mem2.u.i, p_mem1.u.r);
            } else {
                return -1;
            }
        }
        return 1;
    }

    // Se um valor é string e o outro é blob, a string é menor. Se ambos são
    // strings, compara pelas funções de ordenação.
    if (combined_flags & MEM_STR) != 0 {
        if (f1 & MEM_STR) == 0 {
            return 1;
        }
        if (f2 & MEM_STR) == 0 {
            return -1;
        }

        // A sequência de ordenação precisa estar definida neste ponto, mesmo
        // que o usuário a apague depois de o programa VDBE ser compilado.
        if let Some(p_coll) = p_coll {
            return vdbe_compare_mem_string(p_mem1, p_mem2, p_coll, None);
        }
        // Se a função de ordenação veio nula, cai no caso de blob e usa
        // memcmp().
    }

    // Os dois valores são blobs. Compara com memcmp().
    blob_compare(p_mem1, p_mem2)
}

/// O primeiro argumento é um tipo serial que corresponde a um inteiro (de 1 a
/// 9, exceto 7). O segundo aponta o buffer com o inteiro serializado conforme
/// serial_type. Desserializa e devolve o valor.
fn vdbe_record_decode_int(serial_type: u32, a_key: &[u8]) -> i64 {
    match serial_type {
        0 | 1 => {
            // ONE_BYTE_INT
            return (a_key[0] as i8) as i64;
        }
        2 => {
            // TWO_BYTE_INT: 256*(i8)x[0] | x[1]
            return (256 * ((a_key[0] as i8) as i32) | (a_key[1] as i32)) as i64;
        }
        3 => {
            // THREE_BYTE_INT: 65536*(i8)x[0] | (x[1]<<8) | x[2]
            return (65536 * ((a_key[0] as i8) as i32)
                | ((a_key[1] as i32) << 8)
                | (a_key[2] as i32)) as i64;
        }
        4 => {
            // FOUR_BYTE_UINT lido como int com sinal
            let y: u32 = ((a_key[0] as u32) << 24)
                | ((a_key[1] as u32) << 16)
                | ((a_key[2] as u32) << 8)
                | (a_key[3] as u32);
            return (y as i32) as i64;
        }
        5 => {
            // FOUR_BYTE_UINT(aKey+2) + (((i64)1)<<32)*TWO_BYTE_INT(aKey)
            let lo: u32 = ((a_key[2] as u32) << 24)
                | ((a_key[3] as u32) << 16)
                | ((a_key[4] as u32) << 8)
                | (a_key[5] as u32);
            let two = (256 * ((a_key[0] as i8) as i32) | (a_key[1] as i32)) as i64;
            return (lo as i64).wrapping_add((1i64 << 32).wrapping_mul(two));
        }
        6 => {
            let mut x: u64 = (((a_key[0] as u32) << 24)
                | ((a_key[1] as u32) << 16)
                | ((a_key[2] as u32) << 8)
                | (a_key[3] as u32)) as u64;
            let lo: u32 = ((a_key[4] as u32) << 24)
                | ((a_key[5] as u32) << 16)
                | ((a_key[6] as u32) << 8)
                | (a_key[7] as u32);
            x = (x << 32) | (lo as u64);
            return x as i64;
        }
        _ => {}
    }

    serial_type.wrapping_sub(8) as i64
}

/// Compara duas linhas de tabela ou registros de índice: {nKey1, pKey1} contra
/// pPKey2. Devolve negativo, zero ou positivo se key1 for menor, igual ou
/// maior que key2. A chave {nKey1, pKey1} é um blob criado pelo OP_MakeRecord
/// do VDBE; pPKey2 é uma chave já separada em campos, como a obtida por
/// vdbe_parse_record().
///
/// Se `b_skip` for diferente de zero, supõe-se que o chamador já determinou
/// que os primeiros campos das chaves são iguais.
///
/// As chaves não precisam ter o mesmo número de campos. Se todos os campos
/// presentes nas duas forem iguais, devolve pPKey2->default_rc.
///
/// Se achar corrupção, põe SQLITE_CORRUPT em pPKey2->err_code e devolve 0. Se
/// faltar memória, põe SQLITE_NOMEM em pPKey2->err_code.
pub fn vdbe_record_compare_with_skip(
    n_key1: i32,
    p_key1: &[u8],
    p_pkey2: &mut UnpackedRecord,
    b_skip: i32,
) -> i32 {
    let mut d1: u32; // Deslocamento em aKey[] do próximo elemento de dados
    let mut i: i32; // Índice do próximo campo a comparar
    let mut sz_hdr1: u32; // Tamanho do cabeçalho do registro, em bytes
    let mut idx1: u32; // Deslocamento do primeiro tipo no cabeçalho
    let mut rc: i32 = 0; // Valor de retorno
    let mut rhs: usize = 0; // Próximo campo de pPKey2 a comparar (índice em a_mem)
    let a_key1: &[u8] = p_key1;
    let mut mem1 = Mem::default();
    let p_key_info = match p_pkey2.p_key_info.clone() {
        Some(k) => k,
        None => return 0,
    };

    // Se b_skip é verdadeiro, o chamador já determinou que os dois primeiros
    // elementos das chaves são iguais. Ajusta as variáveis para que esta rotina
    // comece a comparar no segundo campo.
    if b_skip != 0 {
        let mut s1: u32 = a_key1[1] as u32;
        if s1 < 0x80 {
            idx1 = 2;
        } else {
            idx1 = 1 + get_varint32(&a_key1[1..], &mut s1) as u32;
        }
        sz_hdr1 = a_key1[0] as u32;
        d1 = sz_hdr1.wrapping_add(vdbe_serial_type_len(s1));
        i = 1;
        rhs += 1;
    } else {
        sz_hdr1 = a_key1[0] as u32;
        if sz_hdr1 < 0x80 {
            idx1 = 1;
        } else {
            idx1 = get_varint32(a_key1, &mut sz_hdr1) as u32;
        }
        d1 = sz_hdr1;
        i = 0;
    }
    if d1 > n_key1 as u32 {
        p_pkey2.err_code = SQLITE_CORRUPT_BKPT as u8;
        return 0; // Corrupção
    }

    loop {
        let mut serial_type: u32;
        let rhs_flags = p_pkey2.a_mem[rhs].flags;

        // O RHS é inteiro
        if (rhs_flags & (MEM_INT | MEM_INTREAL)) != 0 {
            serial_type = a_key1[idx1 as usize] as u32;
            if serial_type >= 10 {
                rc = if serial_type == 10 { -1 } else { 1 };
            } else if serial_type == 0 {
                rc = -1;
            } else if serial_type == 7 {
                serial_get7(&a_key1[d1 as usize..], &mut mem1);
                rc = -int_float_compare(p_pkey2.a_mem[rhs].u.i, mem1.u.r);
            } else {
                let lhs: i64 = vdbe_record_decode_int(serial_type, &a_key1[d1 as usize..]);
                let rhs_i: i64 = p_pkey2.a_mem[rhs].u.i;
                if lhs < rhs_i {
                    rc = -1;
                } else if lhs > rhs_i {
                    rc = 1;
                }
            }
        }
        // O RHS é real
        else if (rhs_flags & MEM_REAL) != 0 {
            serial_type = a_key1[idx1 as usize] as u32;
            if serial_type >= 10 {
                // Tipos seriais 12 ou maiores são strings e blobs (maiores que
                // números). Os tipos 10 e 11 são "reservados para uso futuro",
                // então o resultado da comparação com valores numéricos não
                // importa.
                rc = if serial_type == 10 { -1 } else { 1 };
            } else if serial_type == 0 {
                rc = -1;
            } else {
                if serial_type == 7 {
                    if serial_get7(&a_key1[d1 as usize..], &mut mem1) != 0 {
                        rc = -1; // mem1 é NaN
                    } else if mem1.u.r < p_pkey2.a_mem[rhs].u.r {
                        rc = -1;
                    } else if mem1.u.r > p_pkey2.a_mem[rhs].u.r {
                        rc = 1;
                    }
                } else {
                    vdbe_serial_get(&a_key1[d1 as usize..], serial_type, &mut mem1);
                    rc = int_float_compare(mem1.u.i, p_pkey2.a_mem[rhs].u.r);
                }
            }
        }
        // O RHS é string
        else if (rhs_flags & MEM_STR) != 0 {
            serial_type = 0;
            get_varint32_nr(&a_key1[idx1 as usize..], &mut serial_type);
            if serial_type < 12 {
                rc = -1;
            } else if (serial_type & 0x01) == 0 {
                rc = 1;
            } else {
                mem1.n = ((serial_type - 12) / 2) as i32;
                if (d1 + mem1.n as u32) > n_key1 as u32
                    || p_key_info.borrow().n_all_field as i32 <= i
                {
                    p_pkey2.err_code = SQLITE_CORRUPT_BKPT as u8;
                    return 0; // Corrupção
                }
                let coll = p_key_info.borrow().a_coll[i as usize].clone();
                if let Some(coll) = coll {
                    mem1.enc = p_key_info.borrow().enc;
                    mem1.db = Some(p_key_info.borrow().db.clone());
                    mem1.flags = MEM_STR;
                    mem1.z = a_key1[d1 as usize..d1 as usize + mem1.n as usize].to_vec();
                    rc = vdbe_compare_mem_string(
                        &mem1,
                        &p_pkey2.a_mem[rhs],
                        &coll.borrow(),
                        Some(&mut p_pkey2.err_code),
                    );
                } else {
                    let n_cmp = if mem1.n < p_pkey2.a_mem[rhs].n {
                        mem1.n
                    } else {
                        p_pkey2.a_mem[rhs].n
                    };
                    rc = memcmp(&a_key1[d1 as usize..], &p_pkey2.a_mem[rhs].z, n_cmp as usize);
                    if rc == 0 {
                        rc = mem1.n - p_pkey2.a_mem[rhs].n;
                    }
                }
            }
        }
        // O RHS é blob
        else if (rhs_flags & MEM_BLOB) != 0 {
            serial_type = 0;
            get_varint32_nr(&a_key1[idx1 as usize..], &mut serial_type);
            if serial_type < 12 || (serial_type & 0x01) != 0 {
                rc = -1;
            } else {
                let n_str: i32 = ((serial_type - 12) / 2) as i32;
                if (d1 + n_str as u32) > n_key1 as u32 {
                    p_pkey2.err_code = SQLITE_CORRUPT_BKPT as u8;
                    return 0; // Corrupção
                } else if (rhs_flags & MEM_ZERO) != 0 {
                    if !is_all_zero(&a_key1[d1 as usize..], n_str) {
                        rc = 1;
                    } else {
                        rc = n_str - p_pkey2.a_mem[rhs].u.n_zero;
                    }
                } else {
                    let n_cmp = if n_str < p_pkey2.a_mem[rhs].n {
                        n_str
                    } else {
                        p_pkey2.a_mem[rhs].n
                    };
                    rc = memcmp(&a_key1[d1 as usize..], &p_pkey2.a_mem[rhs].z, n_cmp as usize);
                    if rc == 0 {
                        rc = n_str - p_pkey2.a_mem[rhs].n;
                    }
                }
            }
        }
        // O RHS é nulo
        else {
            serial_type = a_key1[idx1 as usize] as u32;
            if serial_type == 0
                || serial_type == 10
                || (serial_type == 7 && serial_get7(&a_key1[d1 as usize..], &mut mem1) != 0)
            {
                // rc continua 0
            } else {
                rc = 1;
            }
        }

        if rc != 0 {
            let sort_flags = p_key_info.borrow().a_sort_flags[i as usize];
            if sort_flags != 0 {
                if (sort_flags & KEYINFO_ORDER_BIGNULL) == 0
                    || ((sort_flags & KEYINFO_ORDER_DESC) as i32
                        != (serial_type == 0 || (p_pkey2.a_mem[rhs].flags & MEM_NULL) != 0)
                            as i32)
                {
                    rc = -rc;
                }
            }
            return rc;
        }

        i += 1;
        if i == p_pkey2.n_field as i32 {
            break;
        }
        rhs += 1;
        d1 = d1.wrapping_add(vdbe_serial_type_len(serial_type));
        if d1 > n_key1 as u32 {
            break;
        }
        idx1 += varint_len(serial_type as u64) as u32;
        if idx1 >= sz_hdr1 {
            p_pkey2.err_code = SQLITE_CORRUPT_BKPT as u8;
            return 0; // Índice corrompido
        }
    }

    // rc==0 aqui significa que uma das chaves, ou as duas, ficou sem campos e
    // todos os campos até esse ponto eram iguais. Devolve o valor default_rc.
    p_pkey2.eq_seen = 1;
    p_pkey2.default_rc as i32
}


// ---- part_012.rs ----

/// Compara a chave serializada com a chave desempacotada.
pub fn vdbe_record_compare(n_key1: i32, p_key1: &[u8], p_key2: &mut UnpackedRecord) -> i32 {
    vdbe_record_compare_with_skip(n_key1, p_key1, p_key2, 0)
}

/// Versão otimizada de vdbe_record_compare() em que (a) o primeiro campo de
/// p_key2 é inteiro e (b) o varint de tamanho do cabeçalho no início de
/// (p_key1/n_key1) cabe em um único byte (menor que 128).
///
/// Para evitar leituras além do buffer, só é usada em esquemas em que o tamanho
/// máximo válido do cabeçalho é de 63 bytes ou menos.
fn vdbe_record_compare_int(n_key1: i32, p_key1: &[u8], p_key2: &mut UnpackedRecord) -> i32 {
    let a_key = &p_key1[(p_key1[0] & 0x3F) as usize..];
    let serial_type = p_key1[1] as i32;
    let mut res: i32;
    let lhs: i64;

    match serial_type {
        1 => {
            // inteiro de 1 byte com sinal
            lhs = one_byte_int(a_key) as i64;
        }
        2 => {
            // inteiro de 2 bytes com sinal
            lhs = two_byte_int(a_key) as i64;
        }
        3 => {
            // inteiro de 3 bytes com sinal
            lhs = three_byte_int(a_key) as i64;
        }
        4 => {
            // inteiro de 4 bytes com sinal
            let y: u32 = four_byte_uint(a_key);
            lhs = (y as i32) as i64;
        }
        5 => {
            // inteiro de 6 bytes com sinal
            lhs = (four_byte_uint(&a_key[2..]) as i64)
                .wrapping_add((1i64 << 32).wrapping_mul(two_byte_int(a_key) as i64));
        }
        6 => {
            // inteiro de 8 bytes com sinal
            let mut x: u64 = four_byte_uint(a_key) as u64;
            x = (x << 32) | four_byte_uint(&a_key[4..]) as u64;
            lhs = x as i64;
        }
        8 => {
            lhs = 0;
        }
        9 => {
            lhs = 1;
        }
        // Os casos 0 e 7 e o padrão delegam à comparação geral.
        _ => {
            return vdbe_record_compare(n_key1, p_key1, p_key2);
        }
    }

    let v: i64 = p_key2.u.i;
    if v > lhs {
        res = p_key2.r1;
    } else if v < lhs {
        res = p_key2.r2;
    } else if p_key2.n_field > 1 {
        // Os primeiros campos das duas chaves são iguais. Compara os campos
        // restantes.
        res = vdbe_record_compare_with_skip(n_key1, p_key1, p_key2, 1);
    } else {
        // Os primeiros campos são iguais e não há campos restantes. Devolve
        // default_rc neste caso.
        res = p_key2.default_rc;
        p_key2.eq_seen = 1;
    }
    res
}

/// Versão otimizada de vdbe_record_compare() em que (a) o primeiro campo de
/// p_key2 é texto, (b) o primeiro campo usa a collation BINARY e (c) o varint
/// de tamanho do cabeçalho no início de (p_key1/n_key1) cabe em um único byte.
fn vdbe_record_compare_string(n_key1: i32, p_key1: &[u8], p_key2: &mut UnpackedRecord) -> i32 {
    let a_key1 = p_key1;
    let mut serial_type: i32 = a_key1[1] as i8 as i32;
    let res: i32;

    // vrcs_restart
    loop {
        if serial_type < 12 {
            if serial_type < 0 {
                let mut st: u32 = 0;
                get_varint32(&a_key1[1..], &mut st);
                serial_type = st as i32;
                if serial_type >= 12 {
                    continue;
                }
            }
            res = p_key2.r1; // (p_key1/n_key1) é um número ou nulo
        } else if (serial_type & 0x01) == 0 {
            res = p_key2.r2; // (p_key1/n_key1) é um blob
        } else {
            let sz_hdr = a_key1[0] as i32;
            let n_str = (serial_type - 12) / 2;
            if (sz_hdr + n_str) > n_key1 {
                p_key2.err_code = sqlite_corrupt_bkpt() as u8;
                return 0; // Corrupção
            }
            let n_cmp = if p_key2.n < n_str { p_key2.n } else { n_str } as usize;
            let lhs = &a_key1[sz_hdr as usize..sz_hdr as usize + n_cmp];
            let rhs = &p_key2.u.z[..n_cmp];
            let cmp = lhs.cmp(rhs);

            if cmp == std::cmp::Ordering::Greater {
                res = p_key2.r2;
            } else if cmp == std::cmp::Ordering::Less {
                res = p_key2.r1;
            } else {
                let diff = n_str - p_key2.n;
                if diff == 0 {
                    if p_key2.n_field > 1 {
                        res = vdbe_record_compare_with_skip(n_key1, p_key1, p_key2, 1);
                    } else {
                        res = p_key2.default_rc;
                        p_key2.eq_seen = 1;
                    }
                } else if diff > 0 {
                    res = p_key2.r2;
                } else {
                    res = p_key2.r1;
                }
            }
        }
        break;
    }
    res
}

/// Devolve uma função compatível com vdbe_record_compare() adequada para
/// comparar registros serializados com o registro desempacotado passado como
/// único argumento.
pub fn vdbe_find_compare(p: &mut UnpackedRecord) -> RecordCompare {
    // vdbe_record_compare_int() e vdbe_record_compare_string() supõem que o
    // varint de tamanho do cabeçalho no início de cada registro cabe em um
    // único byte (127 ou menos). vdbe_record_compare_int() também supõe que é
    // seguro ler além do buffer pelo tamanho máximo legal de cabeçalho mais 8
    // bytes. Como há ao menos 74 (mas não 136) bytes de preenchimento depois de
    // cada buffer, convém limitar o cabeçalho a 64 bytes quando o primeiro
    // campo é inteiro.
    //
    // O jeito mais fácil de impor o limite é considerar só registros com 13
    // campos ou menos. Se o primeiro campo é inteiro, o maior cabeçalho legal é
    // (12*5 + 1 + 1) bytes.
    if p.p_key_info.n_all_field <= 13 {
        let flags = p.a_mem[0].flags;
        if p.p_key_info.a_sort_flags[0] != 0 {
            if (p.p_key_info.a_sort_flags[0] & KEYINFO_ORDER_BIGNULL) != 0 {
                return vdbe_record_compare;
            }
            p.r1 = 1;
            p.r2 = -1;
        } else {
            p.r1 = -1;
            p.r2 = 1;
        }
        if (flags & MEM_INT) != 0 {
            p.u.i = p.a_mem[0].u.i;
            return vdbe_record_compare_int;
        }
        if (flags & (MEM_REAL | MEM_INTREAL | MEM_NULL | MEM_BLOB)) == 0
            && p.p_key_info.a_coll[0].is_none()
        {
            p.u.z = p.a_mem[0].z.clone();
            p.n = p.a_mem[0].n;
            return vdbe_record_compare_string;
        }
    }

    vdbe_record_compare
}

/// p_cur aponta para uma entrada de índice criada com o opcode OP_MakeRecord.
/// Lê a rowid (o último campo do registro) e a guarda em *rowid. Devolve
/// SQLITE_OK se tudo der certo, ou um código de erro.
///
/// p_cur pode apontar para texto obtido de um arquivo de banco corrompido,
/// então o conteúdo não é confiável. Faz as verificações apropriadas.
pub fn vdbe_idx_rowid(db: &Sqlite3Ref, p_cur: &mut BtCursor, rowid: &mut i64) -> i32 {
    let mut sz_hdr: u32 = 0; // Tamanho do cabeçalho
    let mut type_rowid: u32 = 0; // Tipo serial da rowid
    let mut m = Mem::default();
    let mut v = Mem::default();

    // Obtém o tamanho da entrada de índice. Só há suporte a entradas menores
    // que 2GiB; qualquer coisa maior é corrupção do banco. A corrupção é
    // detectada em btree_parse_cell_ptr(), então aqui nCellKey cabe em 32 bits.
    let n_cell_key: i64 = btree_payload_size(p_cur) as i64;

    // Lê o conteúdo completo da entrada de índice
    vdbe_mem_init(&mut m, db, 0);
    let rc = vdbe_mem_from_btree_zero_offset(p_cur, n_cell_key as u32, &mut m);
    if rc != 0 {
        return rc;
    }

    let corrupted: bool = 'check: {
        // A entrada de índice começa com o tamanho do cabeçalho
        get_varint32_nr(&m.z, &mut sz_hdr);
        if sz_hdr < 3 || sz_hdr > m.n as u32 {
            break 'check true;
        }

        // O último campo do índice deve ser um inteiro, a ROWID. Verifica que
        // a última entrada é de fato um inteiro.
        get_varint32_nr(&m.z[sz_hdr as usize - 1..], &mut type_rowid);
        if type_rowid < 1 || type_rowid > 9 || type_rowid == 7 {
            break 'check true;
        }
        let len_rowid: u32 = small_type_sizes[type_rowid as usize] as u32;
        if (m.n as u32) < sz_hdr + len_rowid {
            break 'check true;
        }

        // Lê o inteiro no fim do registro do índice
        vdbe_serial_get(&m.z[m.n as usize - len_rowid as usize..], type_rowid, &mut v);
        *rowid = v.u.i;
        false
    };

    // idx_rowid_corruption: libera m e devolve SQLITE_CORRUPT em caso de
    // corrupção detectada depois de m alocado.
    vdbe_mem_release_malloc(&mut m);
    if corrupted {
        return sqlite_corrupt_bkpt();
    }
    SQLITE_OK
}

/// Compara a chave da entrada de índice para a qual o cursor p_c aponta com a
/// chave em p_unpacked. Escreve em *res um número negativo, zero ou positivo
/// conforme p_c seja menor, igual ou maior que p_unpacked. Devolve SQLITE_OK em
/// caso de sucesso.
///
/// p_unpacked é criado sem rowid ou truncado para omiti-la no fim. A rowid no
/// fim da entrada de índice também é ignorada. Assim, esta rotina compara só os
/// prefixos das chaves anteriores à rowid final, não a chave inteira.
pub fn vdbe_idx_key_compare(
    db: &Sqlite3Ref,
    p_c: &mut VdbeCursor,
    p_unpacked: &mut UnpackedRecord,
    res: &mut i32,
) -> i32 {
    let mut m = Mem::default();

    let p_cur = &mut p_c.uc.p_cursor;
    let n_cell_key: i64 = btree_payload_size(p_cur) as i64;
    // n_cell_key estará sempre entre 0 e 0xffffffff pela forma como
    // btree_parse_cell_ptr() e get_varint32() são implementados
    if n_cell_key <= 0 || n_cell_key > 0x7fffffff {
        *res = 0;
        return sqlite_corrupt_bkpt();
    }
    vdbe_mem_init(&mut m, db, 0);
    let rc = vdbe_mem_from_btree_zero_offset(p_cur, n_cell_key as u32, &mut m);
    if rc != 0 {
        return rc;
    }
    *res = vdbe_record_compare_with_skip(m.n, &m.z, p_unpacked, 0);
    vdbe_mem_release_malloc(&mut m);
    SQLITE_OK
}

/// Define o valor devolvido pelas chamadas seguintes a sqlite3_changes() no
/// manipulador de banco 'db'.
pub fn vdbe_set_changes(db: &mut Sqlite3, n_change: i64) {
    db.n_change = n_change;
    db.n_total_change = db.n_total_change.wrapping_add(n_change);
}


// ---- part_013.rs ----


/// Aciona um sinalizador na VM para atualizar o contador de mudanças quando ela
/// é finalizada ou resetada.
pub fn vdbe_count_changes(v: &mut Vdbe) {
    v.change_cnt_on = 1;
}

/// Marca toda declaração preparada associada a uma conexão de banco de dados
/// como expirada.
///
/// Uma declaração expirada significa que a recompilação dela é recomendada. As
/// declarações expiram quando acontece algo que torna seus programas obsoletos:
/// remover funções definidas pelo usuário ou sequências de collation, ou mudar
/// uma função de autorização.
///
/// Se i_code é 1, a expiração é consultiva: a declaração deve ser preparada de
/// novo antes de reiniciar, mas se já estiver em execução pode rodar até o fim.
///
/// Internamente, só aciona o campo Vdbe.expired em todas as declarações
/// preparadas: 1 para expiração imediata e 2 para consultiva.
pub fn expire_prepared_statements(db: &Sqlite3Ref, i_code: i32) {
    let mut p = db.borrow().p_vdbe.clone();
    while let Some(p_ref) = p {
        p_ref.borrow_mut().expired = (i_code + 1) as _;
        p = p_ref.borrow().p_v_next.clone();
    }
}

/// Devolve o banco de dados associado ao Vdbe.
pub fn vdbe_db(v: &Vdbe) -> Sqlite3Ref {
    v.db.upgrade().expect("Vdbe.db deve estar vivo")
}

/// Devolve os sinalizadores SQLITE_PREPARE de um Vdbe.
pub fn vdbe_prepare_flags(v: &Vdbe) -> u8 {
    v.prep_flags
}

/// Devolve um valor (sqlite3_value) com o valor do parâmetro de ligação i_var
/// da VM v. Exceto se o valor for NULL de SQL: aí devolve None. A menos que
/// seja NULL, aplica a afinidade aff (uma das constantes SQLITE_AFF_*) ao valor
/// antes de devolvê-lo.
///
/// O chamador libera o valor devolvido (basta descartá-lo).
pub fn vdbe_get_bound_value(v: Option<&Vdbe>, i_var: i32, aff: u8) -> Option<Box<Mem>> {
    if let Some(v) = v {
        let p_mem = &v.a_var[(i_var - 1) as usize];
        if (p_mem.flags & MEM_NULL) == 0 {
            let mut p_ret = value_new(&v.db.upgrade().expect("Vdbe.db deve estar vivo"));
            if let Some(ret) = p_ret.as_mut() {
                vdbe_mem_copy(ret, p_mem);
                value_apply_affinity(ret, aff, SQLITE_UTF8);
            }
            return p_ret;
        }
    }
    None
}

/// Configura a variável SQL i_var de modo que ligar um novo valor a ela avise
/// sqlite3_reoptimize() de que preparar a declaração de novo pode resultar em
/// um plano de consulta melhor.
pub fn vdbe_set_varmask(v: &mut Vdbe, i_var: i32) {
    if i_var >= 32 {
        v.expmask |= 0x80000000;
    } else {
        v.expmask |= 1u32 << (i_var - 1);
    }
}

/// Faz uma função lançar erro se foi chamada por OP_PureFunc em vez de
/// OP_Function.
///
/// OP_PureFunc significa que a função deve ser determinística e deve lançar erro
/// se receber entradas que a tornariam não determinística. Esta rotina é
/// invocada por funções de data e hora que usam recursos não determinísticos
/// como 'now'.
pub fn not_pure_func(p_ctx: &mut Sqlite3Context) -> i32 {
    let p_vdbe = p_ctx.p_vdbe.clone();
    let p_op = p_vdbe.borrow().a_op[p_ctx.i_op as usize].clone();
    if p_op.opcode == OP_PUREFUNC {
        let z_context: &[u8] = if (p_op.p5 & NC_ISCHECK) != 0 {
            b"a CHECK constraint"
        } else if (p_op.p5 & NC_GENCOL) != 0 {
            b"a generated column"
        } else {
            b"an index"
        };
        let mut z_msg: Vec<u8> = b"non-deterministic use of ".to_vec();
        z_msg.extend_from_slice(&p_ctx.p_func.z_name);
        z_msg.extend_from_slice(b"() in ");
        z_msg.extend_from_slice(z_context);
        api::result_error(p_ctx, &z_msg);
        return 0;
    }
    1
}

/// Transfere o texto de mensagem de erro de sqlite3_vtab.z_err_msg (texto da
/// memória de sqlite3_malloc) para Vdbe.z_err_msg (texto da memória de
/// db_malloc).
pub fn vtab_import_errmsg(p: &mut Vdbe, p_vtab: &mut Sqlite3Vtab) {
    if let Some(msg) = p_vtab.z_err_msg.take() {
        // Substitui a mensagem antiga (a anterior é liberada pelo descarte).
        p.z_err_msg = Some(msg);
    }
}

/// Se o terceiro argumento não for None, libera as alocações associadas às
/// células de memória do vetor p.a_mem[]. Também libera a própria estrutura
/// UnpackedRecord.
///
/// É usada para liberar estruturas UnpackedRecord alocadas por
/// vdbe_unpack_record(), de vdbeapi.c.
fn vdbe_free_unpacked(_db: &Sqlite3Ref, n_field: i32, p: Option<Box<UnpackedRecord>>) {
    if let Some(mut p) = p {
        for i in 0..n_field as usize {
            let p_mem = &mut p.a_mem[i];
            if !p_mem.z_malloc.is_empty() {
                vdbe_mem_release_malloc(p_mem);
            }
        }
    }
}

/// Invoca o hook de pré-atualização. Se for uma chamada de pré-atualização de
/// UPDATE ou DELETE, o cursor passado como segundo argumento deve apontar para a
/// linha prestes a ser atualizada ou apagada. Se a aplicação chamar
/// sqlite3_preupdate_old(), o valor necessário é lido da linha para a qual o
/// cursor aponta.
pub fn vdbe_pre_update_hook(
    v: &VdbeRef,
    p_csr: &VdbeCursorRef,
    op: i32,
    z_db: &[u8],
    p_tab: &TableRef,
    mut i_key1: i64,
    i_reg: i32,
    i_blob_write: i32,
) {
    let db = v.borrow().db.upgrade().expect("Vdbe.db deve estar vivo");
    let i_key2: i64;
    let z_tbl: Vec<u8> = p_tab.borrow().z_name.clone();
    let mut preupdate = PreUpdate::default();

    if !has_rowid(&p_tab.borrow()) {
        i_key1 = 0;
        i_key2 = 0;
        preupdate.p_pk = primary_key_index(&p_tab.borrow());
    } else if op == SQLITE_UPDATE {
        i_key2 = v.borrow().a_mem[i_reg as usize].u.i;
    } else {
        i_key2 = i_key1;
    }

    preupdate.v = Some(v.clone());
    preupdate.p_csr = Some(p_csr.clone());
    preupdate.op = op;
    preupdate.i_new_reg = i_reg;
    preupdate.keyinfo.db = Rc::downgrade(&db);
    preupdate.keyinfo.enc = enc(&db.borrow());
    preupdate.keyinfo.n_key_field = p_tab.borrow().n_col as _;
    preupdate.keyinfo.a_sort_flags = vec![0u8]; // fakeSortOrder
    preupdate.i_key1 = i_key1;
    preupdate.i_key2 = i_key2;
    preupdate.p_tab = Some(p_tab.clone());
    preupdate.i_blob_write = i_blob_write;

    let n_key_field = preupdate.keyinfo.n_key_field as i32;
    let p_preupdate: PreUpdateRef = Rc::new(RefCell::new(preupdate));

    // O callback pode chamar sqlite3_preupdate_old() etc., que lê db e
    // p_pre_update: nenhum empréstimo pode estar ativo durante a chamada.
    db.borrow_mut().p_pre_update = Some(p_preupdate.clone());
    let x_callback = db.borrow().x_pre_update_callback.clone();
    if let Some(cb) = x_callback {
        cb(&db, op, z_db, &z_tbl, i_key1, i_key2);
    }
    db.borrow_mut().p_pre_update = None;

    let mut pu = p_preupdate.borrow_mut();
    pu.a_record = Vec::new();
    let p_unpacked = pu.p_unpacked.take();
    vdbe_free_unpacked(&db, n_key_field + 1, p_unpacked);
    let p_new_unpacked = pu.p_new_unpacked.take();
    vdbe_free_unpacked(&db, n_key_field + 1, p_new_unpacked);
    if !pu.a_new.is_empty() {
        let n_field = p_csr.borrow().n_field as usize;
        for p_mem in pu.a_new.iter_mut().take(n_field) {
            vdbe_mem_release(p_mem);
        }
        pu.a_new = Vec::new();
    }
}

