// Mesclado das partes traduzidas de analyze_c (as partes quebram funções grandes no meio).
#![allow(unused_imports, ambiguous_glob_reexports)]
use crate::prelude::*;

// ---- part_000.rs ----

// A build do Debian 13 NÃO define SQLITE_ENABLE_STAT4 (ver CONVENTIONS.md). Os #ifdef ficam
// resolvidos: sem sampleClear/sampleSetRowid/sampleSetRowidInt64/sampleCopy, sem os campos
// anEq/anLt/u/nRowid/isPSample/iCol/iHash em StatSample e sem os campos de amostragem em StatAccum.

/// `IsStat4` sem STAT4.
pub const IS_STAT4: i32 = 0;

/// Sem STAT4 o C faz `#undef SQLITE_STAT4_SAMPLES` e redefine para 1; o `#ifndef ... 24` que vem
/// depois não vale mais.
pub const SQLITE_STAT4_SAMPLES: i32 = 1;

/// Estado compartilhado de stat_init(), stat_push() e stat_get() (campo `anDLt` sem STAT4).
pub struct StatSample {
    /// sqlite_stat4.nDLt: número de valores distintos por prefixo de coluna
    pub an_d_lt: Vec<tRowcnt>,
}

/// Acumulador que stat_init(), stat_push() e stat_get() compartilham.
pub struct StatAccum {
    /// Conexão de banco de dados, para malloc()
    pub db: Sqlite3Ref,
    /// Número estimado de linhas
    pub n_est: tRowcnt,
    /// Número de linhas visitadas até agora
    pub n_row: tRowcnt,
    /// Limite de linhas da análise
    pub n_limit: i32,
    /// Número de colunas do índice mais pk/rowid
    pub n_col: i32,
    /// Número de colunas do índice sem pk/rowid
    pub n_key_col: i32,
    /// Número de vezes que houve skip-ahead
    pub n_skip_ahead: u8,
    /// Linha atual como StatSample
    pub current: StatSample,
}

/// O objeto vive no heap do SQLite e passa por um BLOB; aqui é um `Rc<RefCell<_>>`.
pub type StatAccumRef = Rc<RefCell<StatAccum>>;

/// Gera o código que abre as tabelas sqlite_statN. A sqlite_stat1 é sempre relevante; sqlite_stat2
/// é obsoleta; sqlite_stat3 e sqlite_stat4 só abrem com as opções de compilação apropriadas.
/// Se as tabelas não existirem, são criadas. `z_where` nomeado apaga só as entradas daquela
/// tabela ou índice; `None` apaga tudo.
pub fn open_stat_table(
    p_parse: &mut Parse,
    i_db: i32,
    i_stat_cur: i32,
    z_where: Option<&[u8]>,
    z_where_type: Option<&[u8]>,
) {
    // { zName, zCols } sem STAT4: a segunda e a terceira não têm colunas
    const A_TABLE: [(&str, Option<&str>); 3] = [
        ("sqlite_stat1", Some("tbl,idx,stat")),
        ("sqlite_stat4", None),
        ("sqlite_stat3", None),
    ];
    let db = p_parse.db.upgrade().expect("open_stat_table: conexão encerrada");
    let v = match get_vdbe(p_parse) {
        Some(v) => v,
        None => return,
    };
    let mut a_root = [0u32; A_TABLE.len()];
    let mut a_create_tbl = [0u8; A_TABLE.len()];
    let n_to_open: usize = 1;

    debug_assert!(btree_holds_all_mutexes(&db.borrow()));
    debug_assert!(vdbe_db(&v.borrow()) == db);
    let z_db_s_name: Vec<u8> = db.borrow().a_db[i_db as usize].z_db_s_name.clone();
    let z_db_str = String::from_utf8_lossy(&z_db_s_name).into_owned();

    // Cria as tabelas de estatística se não existirem, ou limpa se já existirem.
    for i in 0..A_TABLE.len() {
        let z_tab = A_TABLE[i].0;
        a_create_tbl[i] = 0;
        let p_stat = find_table(&db.borrow(), z_tab.as_bytes(), Some(&z_db_s_name[..]));
        match p_stat {
            None => {
                if i < n_to_open {
                    // A tabela sqlite_statN não existe: cria. Efeito colateral do CREATE TABLE:
                    // a página raiz da nova tabela fica em pParse->regRoot, que o OpenWrite
                    // abaixo vai precisar.
                    let z_cols = A_TABLE[i].1.unwrap_or("");
                    nested_parse(p_parse, "CREATE TABLE %Q.%s(%s)", &[&z_db_str, z_tab, z_cols]);
                    a_root[i] = p_parse.reg_root as u32;
                    a_create_tbl[i] = OPFLAG_P2ISREG as u8;
                }
            }
            Some(p_stat) => {
                // A tabela já existe. Com zWhere, apaga as entradas da tabela zWhere; sem ele,
                // apaga o conteúdo inteiro.
                a_root[i] = p_stat.borrow().tnum as u32;
                table_lock(p_parse, i_db, a_root[i] as i32, 1, z_tab.as_bytes());
                if let Some(z_where) = z_where {
                    let z_where_str = String::from_utf8_lossy(z_where).into_owned();
                    let z_type_str =
                        String::from_utf8_lossy(z_where_type.unwrap_or(b"")).into_owned();
                    nested_parse(
                        p_parse,
                        "DELETE FROM %Q.%s WHERE %s=%Q",
                        &[&z_db_str, z_tab, &z_type_str, &z_where_str],
                    );
                } else if db.borrow().x_pre_update_callback.is_some() {
                    // SQLITE_ENABLE_PREUPDATE_HOOK vale no Debian 13
                    nested_parse(p_parse, "DELETE FROM %Q.%s", &[&z_db_str, z_tab]);
                } else {
                    // A tabela sqlite_stat[134] já existe: apaga todas as linhas.
                    vdbe_add_op2(&mut v.borrow_mut(), OP_CLEAR as i32, a_root[i] as i32, i_db);
                }
            }
        }
    }

    // Abre as tabelas sqlite_stat[134] para escrita.
    for i in 0..n_to_open {
        debug_assert!(i < A_TABLE.len());
        vdbe_add_op4_int(
            &mut v.borrow_mut(),
            OP_OPENWRITE as i32,
            i_stat_cur + i as i32,
            a_root[i] as i32,
            i_db,
            3,
        );
        vdbe_change_p5(&mut v.borrow_mut(), a_create_tbl[i] as u16);
        // VdbeComment(v, aTable[i].zName) some: o Debian não liga SQLITE_ENABLE_EXPLAIN_COMMENTS
    }
}


// ---- part_001.rs ----

/// Libera toda a memória de um StatAccum. Sem STAT4 não há amostras para limpar (`sampleClear`
/// não existe); o `sqlite3DbFree(p->db, p)` do C é o `drop` do `Rc`, que acontece quando o
/// último dono (o resultado BLOB) se desfaz.
pub fn stat_accum_destructor(p_old: StatAccumRef) {
    drop(p_old);
}

/// Implementação da função SQL stat_init(N,K,C,L). Os quatro parâmetros:
///   N: número de colunas do índice incluindo rowid/pk (nota 1)
///   K: número de colunas do índice sem rowid/pk
///   C: número estimado de linhas do índice
///   L: limite de linhas a varrer, ou 0 para sem limite
///
/// Nota 1: no caso especial do índice de cobertura que implementa uma tabela WITHOUT ROWID, N é o
/// número de colunas da PRIMARY KEY. Em índices de tabelas comuns, N==K+1; em WITHOUT ROWID,
/// N=K+P (P colunas da PK), e o índice de cobertura da própria tabela tem N==K.
///
/// Aloca o StatAccum no heap e devolve o ponteiro como BLOB.
fn stat_init(context: &mut Sqlite3Context, argc: i32, argv: &[Sqlite3ValueRef]) {
    let db = context_db_handle(context);
    let _ = argc;
    let n_col = value_int(&argv[0].borrow());
    debug_assert!(n_col > 0);
    // nColUp: nCol arredondado para cima por alinhamento
    let n_col_up = if std::mem::size_of::<tRowcnt>() < 8 {
        ((n_col + 1) & !1) as usize
    } else {
        n_col as usize
    };
    let n_key_col = value_int(&argv[1].borrow());
    debug_assert!(n_key_col <= n_col);
    debug_assert!(n_key_col > 0);

    // O malloc do C (e o ramo sqlite3_result_error_nomem) não existe: alocação em Rust não falha.
    let p = StatAccum {
        db,
        n_est: value_int64(&argv[2].borrow()) as tRowcnt,
        n_row: 0,
        n_limit: value_int64(&argv[3].borrow()) as i32,
        n_col,
        n_key_col,
        n_skip_ahead: 0,
        current: StatSample {
            an_d_lt: vec![0; n_col_up],
        },
    };

    // Devolve o objeto ao chamador. Só o ponteiro importa; o destrutor é `stat_accum_destructor`.
    result_stat_accum(context, Rc::new(RefCell::new(p)));
}

/// `statInitFuncdef`
fn stat_init_funcdef() -> Rc<FuncDef> {
    Rc::new(FuncDef {
        n_arg: 4,
        func_flags: SQLITE_UTF8 as u32,
        p_user_data: CallbackArg::None,
        p_next: None,
        x_s_func: Some(Rc::new(stat_init)),
        x_finalize: None,
        x_value: None,
        x_inverse: None,
        z_name: b"stat_init".to_vec(),
        u: FuncDefU::PHash(None),
    })
}

/// Implementação da função SQL stat_push(P,C,R).
///   P: ponteiro para o StatAccum criado por stat_init()
///   C: índice da coluna mais à esquerda que difere da linha anterior
///   R: rowid da linha atual (só STAT4 usa)
///
/// Coleta os dados estatísticos do índice no StatAccum; stat_get() lê depois. Em geral devolve
/// NULL, mas pode devolver um inteiro quando o bytecode precisa fazer processamento especial.
fn stat_push(context: &mut Sqlite3Context, argc: i32, argv: &[Sqlite3ValueRef]) {
    let p_ref = value_stat_accum(&argv[0].borrow());
    let i_chng = value_int(&argv[1].borrow());
    let _ = argc;
    let mut p = p_ref.borrow_mut();
    debug_assert!(p.n_col > 0);
    debug_assert!(i_chng < p.n_col);

    if p.n_row == 0 {
        // Primeira chamada: no C só o STAT4 inicializa anEq aqui; sem STAT4 não há nada a fazer.
    } else {
        // Segunda chamada em diante: atualiza anDLt[] para a linha atual do índice.
        for i in (i_chng as usize)..(p.n_col as usize) {
            p.current.an_d_lt[i] = p.current.an_d_lt[i].wrapping_add(1);
        }
    }

    p.n_row = p.n_row.wrapping_add(1);
    if p.n_limit != 0
        && p.n_row
            > (p.n_limit as tRowcnt).wrapping_mul((p.n_skip_ahead as i32 + 1) as tRowcnt)
    {
        p.n_skip_ahead += 1;
        let r = (p.current.an_d_lt[0] > 0) as i32;
        drop(p);
        result_int(context, r);
    }
}


// ---- part_002.rs ----

/// `statPushFuncdef`: `nArg` é `2+IsStat4`
fn stat_push_funcdef() -> Rc<FuncDef> {
    Rc::new(FuncDef {
        n_arg: (2 + IS_STAT4) as i8,
        func_flags: SQLITE_UTF8 as u32,
        p_user_data: CallbackArg::None,
        p_next: None,
        x_s_func: Some(Rc::new(stat_push)),
        x_finalize: None,
        x_value: None,
        x_inverse: None,
        z_name: b"stat_push".to_vec(),
        u: FuncDefU::PHash(None),
    })
}

/// Coluna "stat" da tabela stat1
pub const STAT_GET_STAT1: i32 = 0;
/// Coluna "rowid" de uma entrada stat[34]
pub const STAT_GET_ROWID: i32 = 1;
/// Coluna "neq" de uma entrada stat[34]
pub const STAT_GET_NEQ: i32 = 2;
/// Coluna "nlt" de uma entrada stat[34]
pub const STAT_GET_NLT: i32 = 3;
/// Coluna "ndlt" de uma entrada stat[34]
pub const STAT_GET_NDLT: i32 = 4;

/// Implementação da função SQL stat_get(P,J). Consulta a informação estatística que stat_push()
/// acumulou no StatAccum. P é BLOB mas na verdade é o ponteiro do objeto. J seria um dos valores
/// STAT_GET_xxxx; sem STAT4 J é sempre STAT_GET_STAT1 e some, e a rotina vira stat_get(P), que
/// sempre devolve a entrada da tabela stat1. Não está disponível ao SQL genérico: entra num
/// programa de bytecode montado à mão (ver `call_stat_get`).
fn stat_get(context: &mut Sqlite3Context, argc: i32, argv: &[Sqlite3ValueRef]) {
    let p_ref = value_stat_accum(&argv[0].borrow());
    let p = p_ref.borrow();
    debug_assert!(argc == 1);
    let _ = argc;

    // Valor que vai na coluna "stat" de sqlite_stat1 para este índice: uma lista de inteiros. O
    // primeiro é o total de entradas do índice; depois vem um inteiro por coluna indexada, a
    // estimativa de linhas casadas por uma igualdade com aquele número de campos. Para um índice
    // em (a,b) com stat "100 10 2": o índice tem 100 linhas, "WHERE a=?" casa 10 e "WHERE a=? AND
    // b=?" casa 2. Com D valores distintos e K linhas, cada estimativa é I = (K+D-1)/D, ou seja,
    // K/D arredondado para cima; mas se I está entre 1.0 e 1.1 (perto de 1.0 e um pouco maior),
    // não arredonda para cima e mantém 1.0.
    let mut s_stat = StrAccum::default();
    str_accum_init(&mut s_stat, None, 0, (p.n_key_col + 1) * 100);
    let first: u64 = if p.n_skip_ahead != 0 {
        p.n_est as u64
    } else {
        p.n_row as u64
    };
    str_appendall(&mut s_stat, first.to_string().as_bytes());
    for i in 0..(p.n_key_col as usize) {
        let n_distinct: u64 = p.current.an_d_lt[i].wrapping_add(1) as u64;
        let mut i_val: u64 = (p.n_row as u64)
            .wrapping_add(n_distinct)
            .wrapping_sub(1)
            / n_distinct;
        if i_val == 2 && (p.n_row as u64).wrapping_mul(10) <= n_distinct.wrapping_mul(11) {
            i_val = 1;
        }
        str_appendall(&mut s_stat, format!(" {}", i_val).as_bytes());
    }
    result_str_accum(context, &mut s_stat);
}

/// `statGetFuncdef`: `nArg` é `1+IsStat4`
fn stat_get_funcdef() -> Rc<FuncDef> {
    Rc::new(FuncDef {
        n_arg: (1 + IS_STAT4) as i8,
        func_flags: SQLITE_UTF8 as u32,
        p_user_data: CallbackArg::None,
        p_next: None,
        x_s_func: Some(Rc::new(stat_get)),
        x_finalize: None,
        x_value: None,
        x_inverse: None,
        z_name: b"stat_get".to_vec(),
        u: FuncDefU::PHash(None),
    })
}

fn call_stat_get(p_parse: &mut Parse, reg_stat: i32, i_param: i32, reg_out: i32) {
    // Sem STAT4 o OP_Integer que carrega iParam em regStat+1 não é emitido.
    debug_assert!(i_param == STAT_GET_STAT1);
    debug_assert!(reg_out != reg_stat && reg_out != reg_stat + 1);
    vdbe_add_function_call(
        p_parse,
        0,
        reg_stat,
        reg_out,
        1 + IS_STAT4,
        stat_get_funcdef(),
        0,
    );
}

// analyzeVdbeCommentIndexWithColumnName some: o Debian não liga SQLITE_ENABLE_EXPLAIN_COMMENTS,
// o macro vazio do C. O mesmo vale para VdbeComment e VdbeCoverage nesta rotina.

/// Gera código para analisar todos os índices associados a uma única tabela.
fn analyze_one_table(
    p_parse: &mut Parse,
    p_tab: &TableRef,
    p_only_idx: Option<&IndexRef>,
    i_stat_cur: i32,
    mut i_mem: i32,
    mut i_tab: i32,
) {
    let db = p_parse.db.upgrade().expect("analyze_one_table: conexão encerrada");
    let mut need_table_cnt: u8 = 1;
    let reg_new_rowid = i_mem;
    i_mem += 1;
    let reg_stat = i_mem;
    i_mem += 1;
    let reg_chng = i_mem;
    i_mem += 1;
    let reg_rowid = i_mem;
    i_mem += 1;
    let reg_temp = i_mem;
    i_mem += 1;
    let reg_temp2 = i_mem;
    i_mem += 1;
    let reg_tabname = i_mem;
    i_mem += 1;
    let reg_idxname = i_mem;
    i_mem += 1;
    let reg_stat1 = i_mem;
    i_mem += 1;
    let reg_prev = i_mem; // PRECISA SER O ÚLTIMO (ver abaixo)
    let mut p_stat1: Option<TableRef> = None;

    touch_register(p_parse, i_mem);
    debug_assert!(no_temps_in_range(p_parse, reg_new_rowid, i_mem));
    let v = match get_vdbe(p_parse) {
        Some(v) => v,
        None => return,
    };
    // NEVER(pTab==0) não existe: TableRef não é nulo
    if !is_ordinary_table(&p_tab.borrow()) {
        // Não coleta estatística de views nem de tabelas virtuais
        return;
    }
    if strlike(b"sqlite\\_%", &p_tab.borrow().z_name, b'\\') == 0 {
        // Não coleta estatística de tabelas do sistema
        return;
    }
    debug_assert!(btree_holds_all_mutexes(&db.borrow()));
    let i_db = schema_to_index(&db.borrow(), p_tab.borrow().p_schema.as_ref().map(|s| s.borrow()).as_deref());
    debug_assert!(i_db >= 0);
    debug_assert!(schema_mutex_held(&db.borrow(), i_db, None) != 0);
    // SQLITE_OMIT_AUTHORIZATION não vale: a autorização está compilada
    if auth_check(
        p_parse,
        SQLITE_ANALYZE,
        Some(&p_tab.borrow().z_name[..]),
        None,
        Some(&db.borrow().a_db[i_db as usize].z_db_s_name[..]),
    ) != 0
    {
        return;
    }

    // SQLITE_ENABLE_PREUPDATE_HOOK vale no Debian 13
    if db.borrow().x_pre_update_callback.is_some() {
        let mut t = Table::default();
        t.z_name = b"sqlite_stat1".to_vec();
        t.n_col = 3;
        t.i_p_key = -1;
        let t = Rc::new(RefCell::new(t));
        // O C usa P4_DYNAMIC para o Noop ser o dono; aqui o dono é o Rc e o P4 vai direto
        // como P4_TABLE (que depois é regravado no OP_Insert).
        vdbe_add_op4(
            &mut v.borrow_mut(),
            OP_NOOP as i32,
            0,
            0,
            0,
            P4Value::Table(t.clone()),
            P4_TABLE,
        );
        p_stat1 = Some(t);
    }
    let p4_stat1 = || match &p_stat1 {
        Some(t) => P4Value::Table(t.clone()),
        None => P4Value::NotUsed,
    };

    // Trava de leitura na tabela no nível de shared-cache. Abre um cursor só de leitura na tabela.
    // Também reserva um número de cursor para varrer índices (iIdxCur), sem abrir cursor de índice
    // agora.
    table_lock(p_parse, i_db, p_tab.borrow().tnum as i32, 0, &p_tab.borrow().z_name);
    let i_tab_cur = i_tab;
    i_tab += 1;
    let i_idx_cur = i_tab;
    i_tab += 1;
    p_parse.n_tab = std::cmp::max(p_parse.n_tab, i_tab);
    open_table(p_parse, i_tab_cur, i_db, p_tab, OP_OPENREAD as i32);
    vdbe_load_string(&mut v.borrow_mut(), reg_tabname, &p_tab.borrow().z_name);

    let mut p_idx_opt = p_tab.borrow().p_index.clone();
    while let Some(p_idx) = p_idx_opt {
        let next = p_idx.borrow().p_next.clone();
        p_idx_opt = next;
        let n_col: i32; // Número de colunas de pIdx. "N"
        let mut addr_goto_end: i32; // Endereço do "OP_Rewind iIdxCur"
        let mut addr_next_row: i32; // Endereço de "next_row:"
        let z_idx_name: Vec<u8>; // Nome do índice
        let n_col_test: i32; // Número de colunas a testar por mudança

        if let Some(only) = p_only_idx {
            if !Rc::ptr_eq(only, &p_idx) {
                continue;
            }
        }
        let (n_key_col, n_column, uniq_not_null, has_part_where, tnum) = {
            let ix = p_idx.borrow();
            (
                ix.n_key_col as i32,
                ix.n_column as i32,
                ix.uniq_not_null != 0,
                ix.p_part_idx_where.is_some(),
                ix.tnum,
            )
        };
        if !has_part_where {
            need_table_cnt = 0;
        }
        if !has_rowid(&p_tab.borrow()) && is_primary_key_index(&p_idx.borrow()) {
            n_col = n_key_col;
            z_idx_name = p_tab.borrow().z_name.clone();
            n_col_test = n_col - 1;
        } else {
            n_col = n_column;
            z_idx_name = p_idx.borrow().z_name.clone();
            n_col_test = if uniq_not_null { n_key_col - 1 } else { n_col - 1 };
        }

        // Preenche o registro com o nome do índice.
        vdbe_load_string(&mut v.borrow_mut(), reg_idxname, &z_idx_name);

        // Pseudo-código do laço que chama stat_push():
        //
        //   regChng = 0
        //   Rewind csr
        //   if eof(csr){
        //      stat_init() with count = 0;
        //      goto end_of_scan;
        //   }
        //   count()
        //   stat_init()
        //   goto chng_addr_0;
        //
        //  next_row:
        //   regChng = 0
        //   if( idx(0) != regPrev(0) ) goto chng_addr_0
        //   regChng = 1
        //   if( idx(1) != regPrev(1) ) goto chng_addr_1
        //   ...
        //   regChng = N
        //   goto chng_addr_N
        //
        //  chng_addr_0:
        //   regPrev(0) = idx(0)
        //  chng_addr_1:
        //   regPrev(1) = idx(1)
        //  ...
        //
        //  endDistinctTest:
        //   regRowid = idx(rowid)
        //   stat_push(P, regChng, regRowid)
        //   Next csr
        //   if !eof(csr) goto next_row;
        //
        //  end_of_scan:

        // Garante registros suficientes para o array regPrev e um rowid final (o espaço do rowid
        // é exigido ao montar o registro da coluna sample de sqlite_stat4).
        touch_register(p_parse, reg_prev + n_col_test);

        // Abre um cursor só de leitura no índice analisado.
        debug_assert!(
            i_db == schema_to_index(
                &db.borrow(),
                p_idx.borrow().p_schema.as_ref().map(|s| s.borrow()).as_deref()
            )
        );
        vdbe_add_op3(&mut v.borrow_mut(), OP_OPENREAD as i32, i_idx_cur, tnum as i32, i_db);
        vdbe_set_p4_key_info(p_parse, &p_idx);

        // Implementa: regChng = 0; Rewind csr; se eof, stat_init() com count = 0 e vai para
        // end_of_scan; senão count(); stat_init(); goto chng_addr_0.
        debug_assert!(reg_temp2 == reg_stat + 4);
        let n_analysis_limit = db.borrow().n_analysis_limit;
        vdbe_add_op2(&mut v.borrow_mut(), OP_INTEGER as i32, n_analysis_limit, reg_temp2);

        // Argumentos de stat_init(): (1) número de colunas do índice incluindo o rowid (ou, em
        // WITHOUT ROWID, o número de colunas da PK), (2) número de colunas da chave sem o
        // rowid/pk, (3) número estimado de linhas do índice.
        vdbe_add_op2(&mut v.borrow_mut(), OP_INTEGER as i32, n_col, reg_stat + 1);
        debug_assert!(reg_rowid == reg_stat + 2);
        vdbe_add_op2(&mut v.borrow_mut(), OP_INTEGER as i32, n_key_col, reg_rowid);
        let stat4_disabled = optimization_disabled(&db.borrow(), SQLITE_STAT4) as i32;
        vdbe_add_op3(
            &mut v.borrow_mut(),
            OP_COUNT as i32,
            i_idx_cur,
            reg_temp,
            stat4_disabled,
        );
        vdbe_add_function_call(p_parse, 0, reg_stat + 1, reg_stat, 4, stat_init_funcdef(), 0);
        addr_goto_end = vdbe_add_op1(&mut v.borrow_mut(), OP_REWIND as i32, i_idx_cur);

        vdbe_add_op2(&mut v.borrow_mut(), OP_INTEGER as i32, 0, reg_chng);
        addr_next_row = vdbe_current_addr(&v.borrow());

        if n_col_test > 0 {
            let end_distinct_test = vdbe_make_label(p_parse);
            let mut a_goto_chng: Vec<i32> = Vec::with_capacity(n_col_test as usize);

            //  next_row:
            //   regChng = 0
            //   if( idx(0) != regPrev(0) ) goto chng_addr_0
            //   regChng = 1
            //   if( idx(1) != regPrev(1) ) goto chng_addr_1
            //   ...
            //   regChng = N
            //   goto endDistinctTest
            vdbe_add_op0(&mut v.borrow_mut(), OP_GOTO as i32);
            addr_next_row = vdbe_current_addr(&v.borrow());
            if n_col_test == 1 && n_key_col == 1 && is_unique_index(&p_idx.borrow()) {
                // Num índice UNIQUE de coluna única, achada uma linha não NULL, todas as demais
                // serão distintas, então pula os testes de distinção seguintes.
                vdbe_add_op2(&mut v.borrow_mut(), OP_NOTNULL as i32, reg_prev, end_distinct_test);
            }
            for i in 0..n_col_test {
                let z_coll = p_idx.borrow().az_coll[i as usize].clone();
                let p_coll = locate_coll_seq(p_parse, &z_coll);
                vdbe_add_op2(&mut v.borrow_mut(), OP_INTEGER as i32, i, reg_chng);
                vdbe_add_op3(&mut v.borrow_mut(), OP_COLUMN as i32, i_idx_cur, i, reg_temp);
                let p4 = match p_coll {
                    Some(c) => P4Value::CollSeq(c),
                    None => P4Value::NotUsed,
                };
                a_goto_chng.push(vdbe_add_op4(
                    &mut v.borrow_mut(),
                    OP_NE as i32,
                    reg_temp,
                    0,
                    reg_prev + i,
                    p4,
                    P4_COLLSEQ,
                ));
                vdbe_change_p5(&mut v.borrow_mut(), SQLITE_NULLEQ as u16);
            }
            vdbe_add_op2(&mut v.borrow_mut(), OP_INTEGER as i32, n_col_test, reg_chng);
            vdbe_goto(&mut v.borrow_mut(), end_distinct_test);

            //  chng_addr_0:
            //   regPrev(0) = idx(0)
            //  chng_addr_1:
            //   regPrev(1) = idx(1)
            //  ...
            vdbe_jump_here(&mut v.borrow_mut(), addr_next_row - 1);
            for i in 0..n_col_test {
                vdbe_jump_here(&mut v.borrow_mut(), a_goto_chng[i as usize]);
                vdbe_add_op3(&mut v.borrow_mut(), OP_COLUMN as i32, i_idx_cur, i, reg_prev + i);
            }
            vdbe_resolve_label(&mut v.borrow_mut(), end_distinct_test);
        }

        //  chng_addr_N:
        //   regRowid = idx(rowid)            // só STAT4
        //   stat_push(P, regChng, regRowid)  // 3º parâmetro só STAT4
        //   Next csr
        //   if !eof(csr) goto next_row;
        // (o bloco STAT4 que carrega regRowid some)
        debug_assert!(reg_chng == reg_stat + 1);
        {
            vdbe_add_function_call(
                p_parse,
                1,
                reg_stat,
                reg_temp,
                2 + IS_STAT4,
                stat_push_funcdef(),
                0,
            );
            if n_analysis_limit != 0 {
                let j1 = vdbe_add_op1(&mut v.borrow_mut(), OP_ISNULL as i32, reg_temp);
                let j2 = vdbe_add_op1(&mut v.borrow_mut(), OP_IF as i32, reg_temp);
                let j3 = vdbe_add_op4_int(
                    &mut v.borrow_mut(),
                    OP_SEEKGT as i32,
                    i_idx_cur,
                    0,
                    reg_prev,
                    1,
                );
                vdbe_jump_here(&mut v.borrow_mut(), j1);
                vdbe_add_op2(&mut v.borrow_mut(), OP_NEXT as i32, i_idx_cur, addr_next_row);
                vdbe_jump_here(&mut v.borrow_mut(), j2);
                vdbe_jump_here(&mut v.borrow_mut(), j3);
            } else {
                vdbe_add_op2(&mut v.borrow_mut(), OP_NEXT as i32, i_idx_cur, addr_next_row);
            }
        }

        // Acrescenta a entrada na tabela stat1.
        if has_part_where {
            // Índices parciais podem receber uma entrada zerada em sqlite_stat1, mas uma tabela
            // vazia é omitida de sqlite_stat1.
            vdbe_jump_here(&mut v.borrow_mut(), addr_goto_end);
            addr_goto_end = 0;
        }
        call_stat_get(p_parse, reg_stat, STAT_GET_STAT1, reg_stat1);
        debug_assert!(b"BBB"[0] == SQLITE_AFF_TEXT as u8);
        vdbe_add_op4(
            &mut v.borrow_mut(),
            OP_MAKERECORD as i32,
            reg_tabname,
            3,
            reg_temp,
            P4Value::Static(b"BBB".to_vec()),
            P4_STATIC,
        );
        vdbe_add_op2(&mut v.borrow_mut(), OP_NEWROWID as i32, i_stat_cur, reg_new_rowid);
        vdbe_add_op3(
            &mut v.borrow_mut(),
            OP_INSERT as i32,
            i_stat_cur,
            reg_temp,
            reg_new_rowid,
        );
        vdbe_change_p4(&mut v.borrow_mut(), -1, p4_stat1(), P4_TABLE as i32);
        vdbe_change_p5(&mut v.borrow_mut(), OPFLAG_APPEND as u16);

        // Acrescenta as entradas da tabela stat4: bloco STAT4 some.

        // Fim da análise
        if addr_goto_end != 0 {
            vdbe_jump_here(&mut v.borrow_mut(), addr_goto_end);
        }
    }

    // Cria uma única entrada em sqlite_stat1 com NULL como nome do índice e a contagem de linhas
    // como conteúdo.
    if p_only_idx.is_none() && need_table_cnt != 0 {
        vdbe_add_op2(&mut v.borrow_mut(), OP_COUNT as i32, i_tab_cur, reg_stat1);
        let j_zero_rows = vdbe_add_op1(&mut v.borrow_mut(), OP_IFNOT as i32, reg_stat1);
        vdbe_add_op2(&mut v.borrow_mut(), OP_NULL as i32, 0, reg_idxname);
        debug_assert!(b"BBB"[0] == SQLITE_AFF_TEXT as u8);
        vdbe_add_op4(
            &mut v.borrow_mut(),
            OP_MAKERECORD as i32,
            reg_tabname,
            3,
            reg_temp,
            P4Value::Static(b"BBB".to_vec()),
            P4_STATIC,
        );
        vdbe_add_op2(&mut v.borrow_mut(), OP_NEWROWID as i32, i_stat_cur, reg_new_rowid);
        vdbe_add_op3(
            &mut v.borrow_mut(),
            OP_INSERT as i32,
            i_stat_cur,
            reg_temp,
            reg_new_rowid,
        );
        vdbe_change_p5(&mut v.borrow_mut(), OPFLAG_APPEND as u16);
        vdbe_change_p4(&mut v.borrow_mut(), -1, p4_stat1(), P4_TABLE as i32);
        vdbe_jump_here(&mut v.borrow_mut(), j_zero_rows);
    }
}


// ---- part_003.rs ----

/// Gera o código que carrega a análise de índice mais recente nas tabelas hash internas, onde o
/// planejador pode usá-la.
fn load_analysis(p_parse: &mut Parse, i_db: i32) {
    if let Some(v) = get_vdbe(p_parse) {
        vdbe_add_op1(&mut v.borrow_mut(), OP_LOADANALYSIS as i32, i_db);
    }
}

/// Gera o código que analisa um banco de dados inteiro.
fn analyze_database(p_parse: &mut Parse, i_db: i32) {
    let db = p_parse.db.upgrade().expect("analyze_database: conexão encerrada");
    let p_schema = db.borrow().a_db[i_db as usize]
        .p_schema
        .clone()
        .expect("analyze_database: banco sem schema");

    begin_write_operation(p_parse, 0, i_db);
    let i_stat_cur = p_parse.n_tab;
    p_parse.n_tab += 3;
    open_stat_table(p_parse, i_db, i_stat_cur, None, None);
    let i_mem = p_parse.n_mem + 1;
    let i_tab = p_parse.n_tab;
    debug_assert!(schema_mutex_held(&db.borrow(), i_db, None) != 0);
    // Copia a lista antes do laço: analyze_one_table consulta o mesmo Schema por empréstimo.
    let mut tables: Vec<TableRef> = Vec::new();
    let mut k = hash_first(&p_schema.borrow().tbl_hash);
    while let Some(e) = k {
        let p_tab = hash_data(&e.borrow())
            .downcast_ref::<TableRef>()
            .expect("tblHash só guarda Table")
            .clone();
        tables.push(p_tab);
        k = hash_next(&e.borrow());
    }
    for p_tab in tables.iter() {
        analyze_one_table(p_parse, p_tab, None, i_stat_cur, i_mem, i_tab);
        // Sem STAT4 iMem não avança
        debug_assert!(i_mem == first_available_register(p_parse, i_mem));
    }
    load_analysis(p_parse, i_db);
}

/// Gera o código que analisa uma única tabela de um banco de dados. Se `p_only_idx` não é nulo,
/// é o único índice de pTab a analisar.
fn analyze_table(p_parse: &mut Parse, p_tab: &TableRef, p_only_idx: Option<&IndexRef>) {
    let db = p_parse.db.upgrade().expect("analyze_table: conexão encerrada");
    debug_assert!(btree_holds_all_mutexes(&db.borrow()));
    let i_db = schema_to_index(
        &db.borrow(),
        p_tab.borrow().p_schema.as_ref().map(|s| s.borrow()).as_deref(),
    );
    begin_write_operation(p_parse, 0, i_db);
    let i_stat_cur = p_parse.n_tab;
    p_parse.n_tab += 3;
    if let Some(p_idx) = p_only_idx {
        let z_name = p_idx.borrow().z_name.clone();
        open_stat_table(p_parse, i_db, i_stat_cur, Some(&z_name[..]), Some(b"idx"));
    } else {
        let z_name = p_tab.borrow().z_name.clone();
        open_stat_table(p_parse, i_db, i_stat_cur, Some(&z_name[..]), Some(b"tbl"));
    }
    let i_mem = p_parse.n_mem + 1;
    let i_tab = p_parse.n_tab;
    analyze_one_table(p_parse, p_tab, p_only_idx, i_stat_cur, i_mem, i_tab);
    load_analysis(p_parse, i_db);
}

/// Gera o código do comando ANALYZE. O analisador sintático chama esta rotina ao reconhecer o
/// comando.
///
///        ANALYZE                            -- 1
///        ANALYZE  <database>                -- 2
///        ANALYZE  ?<database>.?<tablename>  -- 3
///
/// A forma 1 analisa todos os índices de todos os bancos anexados. A forma 2 analisa todos os
/// índices do banco nomeado. A forma 3 analisa todos os índices da tabela nomeada.
pub fn analyze(p_parse: &mut Parse, p_name1: Option<&Token>, p_name2: Option<&Token>) {
    let db = p_parse.db.upgrade().expect("analyze: conexão encerrada");

    // Lê o schema. Se der erro, deixa mensagem e código em pParse e retorna.
    debug_assert!(btree_holds_all_mutexes(&db.borrow()));
    if read_schema(p_parse) != SQLITE_OK {
        return;
    }

    debug_assert!(p_name2.is_some() || p_name1.is_none());
    match p_name1 {
        None => {
            // Forma 1: analisa tudo
            let n_db = db.borrow().n_db;
            for i in 0..n_db {
                if i == 1 {
                    continue; // Não analisa o banco TEMP
                }
                analyze_database(p_parse, i);
            }
        }
        Some(name1) => {
            let name2 = p_name2.expect("analyze: pName2 ausente com pName1");
            let i_db_named = if name2.n == 0 { find_db(&db.borrow(), name1) } else { -1 };
            if name2.n == 0 && i_db_named >= 0 {
                // Analisa o schema nomeado no argumento
                analyze_database(p_parse, i_db_named);
            } else {
                // Forma 3: analisa a tabela ou índice nomeado no argumento
                let mut p_table_name = Token::default();
                let i_db = two_part_name(p_parse, name1, name2, &mut p_table_name);
                if i_db >= 0 {
                    let z_db: Option<Vec<u8>> = if name2.n != 0 {
                        Some(db.borrow().a_db[i_db as usize].z_db_s_name.clone())
                    } else {
                        None
                    };
                    let z = name_from_token(&db.borrow(), &p_table_name);
                    if let Some(z) = z {
                        let p_idx = find_index(&db.borrow(), &z, z_db.as_deref());
                        if let Some(p_idx) = p_idx {
                            let p_table = p_idx
                                .borrow()
                                .p_table
                                .upgrade()
                                .expect("analyze: índice sem tabela");
                            analyze_table(p_parse, &p_table, Some(&p_idx));
                        } else if let Some(p_tab) =
                            locate_table(p_parse, 0, &z, z_db.as_deref())
                        {
                            analyze_table(p_parse, &p_tab, None);
                        }
                    }
                }
            }
        }
    }
    if db.borrow().n_sql_exec == 0 {
        if let Some(v) = get_vdbe(p_parse) {
            vdbe_add_op0(&mut v.borrow_mut(), OP_EXPIRE as i32);
        }
    }
}

/// Passa informação do leitor de análise ao callback.
pub struct AnalysisInfo {
    pub db: Sqlite3Ref,
    pub z_database: Vec<u8>,
}

/// O primeiro argumento é uma string com uma lista de inteiros separados por espaço. Lê os
/// primeiros `n_out` em `a_log[]` (sem STAT4 `aOut` é sempre nulo e some).
///
/// No C, `aLog` é o próprio `pIndex->aiRowLogEst`; aqui o chamador retira o vetor do índice com
/// `mem::take`, passa como `a_log` e o devolve depois, para não haver dois empréstimos mutáveis.
fn decode_int_array(z_int_array: &[u8], n_out: i32, a_log: &mut [LogEst], p_index: &mut Index) {
    // Leitura com NUL implícito além do fim, como a string do C
    let at = |z: usize| -> u8 { z_int_array.get(z).copied().unwrap_or(0) };
    let mut z: usize = 0;
    let mut i: i32 = 0;
    while at(z) != 0 && i < n_out {
        let mut v: tRowcnt = 0;
        loop {
            let c = at(z);
            if !(c >= b'0' && c <= b'9') {
                break;
            }
            v = v.wrapping_mul(10).wrapping_add((c - b'0') as tRowcnt);
            z += 1;
        }
        a_log[i as usize] = log_est(v as u64);
        if at(z) == b' ' {
            z += 1;
        }
        i += 1;
    }
    p_index.b_unordered = 0;
    p_index.no_skip_scan = 0;
    while at(z) != 0 {
        if strglob(b"unordered*", &z_int_array[z..]) == 0 {
            p_index.b_unordered = 1;
        } else if strglob(b"sz=[0-9]*", &z_int_array[z..]) == 0 {
            let mut sz = atoi(&z_int_array[z + 3..]);
            if sz < 2 {
                sz = 2;
            }
            p_index.sz_idx_row = log_est(sz as u64);
        } else if strglob(b"noskipscan*", &z_int_array[z..]) == 0 {
            p_index.no_skip_scan = 1;
        }
        // O ramo costmult= só existe com SQLITE_ENABLE_COSTMULT, que o Debian não liga.
        while at(z) != 0 && at(z) != b' ' {
            z += 1;
        }
        while at(z) == b' ' {
            z += 1;
        }
    }

    // Liga bLowQual se o pico de linhas de uma igualdade completa é tão grande que uma varredura
    // da tabela provavelmente é mais rápida que o índice.
    if a_log[0] > 66 // O índice tem mais de 100 linhas
        && a_log[0] <= a_log[(n_out - 1) as usize]
    // E só um valor foi visto
    {
        p_index.b_low_qual = 1;
    }
}

/// Callback invocado uma vez por índice ao ler a tabela sqlite_stat1.
///
///     argv[0] = nome da tabela
///     argv[1] = nome do índice (pode ser NULL)
///     argv[2] = resultado da análise: um inteiro por coluna
///
/// Entradas com argv[1]==NULL só registram o número de linhas da tabela.
fn analysis_loader(
    p_info: &AnalysisInfo,
    argc: i32,
    argv: &[Option<Vec<u8>>],
    _not_used: &[Option<Vec<u8>>],
) -> i32 {
    debug_assert!(argc == 3);
    let _ = argc;

    if argv.is_empty() || argv[0].is_none() || argv[2].is_none() {
        return 0;
    }
    let argv0 = argv[0].as_deref().unwrap();
    let z = argv[2].as_deref().unwrap();
    let p_table = match find_table(&p_info.db.borrow(), argv0, Some(&p_info.z_database[..])) {
        Some(t) => t,
        None => return 0,
    };
    let p_index: Option<IndexRef> = match argv[1].as_deref() {
        None => None,
        Some(argv1) => {
            if stricmp(Some(argv0), Some(argv1)) == 0 {
                primary_key_index(&p_table)
            } else {
                find_index(&p_info.db.borrow(), argv1, Some(&p_info.z_database[..]))
            }
        }
    };

    if let Some(p_index) = p_index {
        let n_col = p_index.borrow().n_key_col as i32 + 1;
        let mut ix = p_index.borrow_mut();
        ix.b_unordered = 0;
        let mut a_log = std::mem::take(&mut ix.ai_row_log_est);
        decode_int_array(z, n_col, &mut a_log, &mut ix);
        ix.ai_row_log_est = a_log;
        ix.has_stat1 = 1;
        if ix.p_part_idx_where.is_none() {
            let mut t = p_table.borrow_mut();
            t.n_row_log_est = ix.ai_row_log_est[0];
            t.tab_flags |= TF_HASSTAT1;
        }
    } else {
        let mut fake_idx = Index::default();
        fake_idx.sz_idx_row = p_table.borrow().sz_tab_row;
        let mut a_log = [p_table.borrow().n_row_log_est];
        decode_int_array(z, 1, &mut a_log, &mut fake_idx);
        let mut t = p_table.borrow_mut();
        t.n_row_log_est = a_log[0];
        t.sz_tab_row = fake_idx.sz_idx_row;
        t.tab_flags |= TF_HASSTAT1;
    }
    0
}

/// Apaga o array Index.aSample[] e seu conteúdo. Sem STAT4 (`SQLITE_ENABLE_STAT4`) o corpo do C
/// só marca os parâmetros como não usados, e a função fica vazia.
pub fn delete_index_samples(_db: &Sqlite3, _p_idx: &Index) {}


// ---- part_004.rs ----

/// Procura um índice pelo nome. Ou, se o nome de uma tabela WITHOUT ROWID for fornecido,
/// procura o índice PRIMARY KEY dessa tabela.
fn find_index_or_primary_key(db: &Sqlite3Ref, z_name: &[u8], z_db: &[u8]) -> Option<IndexRef> {
    let mut p_idx = find_index(db, z_name, z_db);
    if p_idx.is_none() {
        let p_tab = find_table(db, z_name, z_db);
        if let Some(tab_ref) = p_tab {
            let sem_rowid = !has_rowid(&tab_ref.borrow());
            if sem_rowid {
                p_idx = primary_key_index(&tab_ref);
            }
        }
    }
    p_idx
}

/// Carrega o conteúdo de sqlite_stat4 nos arrays IndexSample relevantes.
///
/// Os argumentos z_sql1 e z_sql2 precisam ser instruções SQL que retornam
/// dados equivalentes aos seguintes:
///
///    z_sql1: SELECT idx,count(*) FROM %Q.sqlite_stat4 GROUP BY idx
///    z_sql2: SELECT idx,neq,nlt,ndlt,sample FROM %Q.sqlite_stat4
///
/// onde %Q é substituído pelo nome do banco antes da execução do SQL.
#[cfg(feature = "SQLITE_ENABLE_STAT4")]
fn load_stat_tbl(db: &Sqlite3Ref, z_sql1: &[u8], z_sql2: &[u8], z_db: &[u8]) -> i32 {
    let mut rc: i32;
    let mut p_stmt: Option<VdbeRef> = None;
    let mut p_prev_idx: Option<IndexRef> = None;

    assert!(db.borrow().lookaside.b_disable != 0);
    let z_sql = m_printf(Some(db), z_sql1, Some(z_db));
    if z_sql.is_none() {
        return SQLITE_NOMEM_BKPT;
    }
    rc = api::prepare(db, z_sql.as_ref().unwrap(), -1, &mut p_stmt, None);
    // o texto SQL é dono do Vec: o db_free do C é o drop
    drop(z_sql);
    if rc != 0 {
        return rc;
    }

    while api::step(&p_stmt) == SQLITE_ROW {
        let z_index = match api::column_text(&p_stmt, 0) {
            None => continue,
            Some(z) => z,
        };
        let n_sample = api::column_int(&p_stmt, 1);
        let p_idx = match find_index_or_primary_key(db, &z_index, z_db) {
            None => continue,
            Some(i) => i,
        };
        let mut idx = p_idx.borrow_mut();
        assert!(idx.n_sample == 0);
        if !idx.a_sample.is_empty() {
            // O mesmo índice aparece em sqlite_stat4 sob vários nomes
            continue;
        }
        let p_table = idx.p_table.upgrade().unwrap();
        let n_idx_col: i32;
        {
            let tab = p_table.borrow();
            assert!(has_rowid(&tab) || idx.n_column == idx.n_key_col + 1);
            if !has_rowid(&tab) && is_primary_key_index(&idx) {
                n_idx_col = idx.n_key_col as i32;
            } else {
                n_idx_col = idx.n_column as i32;
            }
        }
        idx.n_sample_col = n_idx_col;
        idx.mx_sample = n_sample;

        // O C aloca um bloco único (amostras, anEq/anLt/anDLt e aAvgEq); aqui cada
        // array é um Vec próprio, com o mesmo tamanho lógico.
        let n_col_usize = n_idx_col as usize;
        let mut a_sample: Vec<IndexSample> = Vec::new();
        for _ in 0..n_sample.max(0) {
            let mut s = IndexSample::default();
            s.an_eq = vec![0; n_col_usize];
            s.an_lt = vec![0; n_col_usize];
            s.an_d_lt = vec![0; n_col_usize];
            a_sample.push(s);
        }
        idx.a_sample = a_sample;
        idx.a_avg_eq = vec![0; n_col_usize];
        p_table.borrow_mut().tab_flags |= TF_HASSTAT4;
    }
    rc = api::finalize(&p_stmt);
    if rc != 0 {
        return rc;
    }

    let z_sql = m_printf(Some(db), z_sql2, Some(z_db));
    if z_sql.is_none() {
        return SQLITE_NOMEM_BKPT;
    }
    rc = api::prepare(db, z_sql.as_ref().unwrap(), -1, &mut p_stmt, None);
    drop(z_sql);
    if rc != 0 {
        return rc;
    }

    while api::step(&p_stmt) == SQLITE_ROW {
        let z_index = match api::column_text(&p_stmt, 0) {
            None => continue,
            Some(z) => z,
        };
        let p_idx = match find_index_or_primary_key(db, &z_index, z_db) {
            None => continue,
            Some(i) => i,
        };
        {
            let idx = p_idx.borrow();
            if idx.n_sample >= idx.mx_sample {
                // Slots demais usados porque o mesmo índice aparece em
                // sqlite_stat4 sob vários nomes
                continue;
            }
        }
        // Esta próxima condição é verdadeira se os dados já foram carregados
        // da tabela sqlite_stat4.
        let n_col = p_idx.borrow().n_sample_col;
        let mudou = match &p_prev_idx {
            None => true,
            Some(prev) => !Rc::ptr_eq(prev, &p_idx),
        };
        if mudou {
            init_avg_eq(&p_prev_idx);
            p_prev_idx = Some(p_idx.clone());
        }
        let mut idx = p_idx.borrow_mut();
        let i_sample = idx.n_sample as usize;
        let z1 = api::column_text(&p_stmt, 1).unwrap_or_default();
        decode_int_array(&z1, n_col, &mut idx.a_sample[i_sample].an_eq, None, None);
        let z2 = api::column_text(&p_stmt, 2).unwrap_or_default();
        decode_int_array(&z2, n_col, &mut idx.a_sample[i_sample].an_lt, None, None);
        let z3 = api::column_text(&p_stmt, 3).unwrap_or_default();
        decode_int_array(&z3, n_col, &mut idx.a_sample[i_sample].an_d_lt, None, None);

        // Faz uma cópia da amostra. Acrescenta 8 bytes 0x00 extras no fim do buffer.
        // Isso é para o caso de o registro da amostra estar corrompido. Nesse caso,
        // vdbe_record_compare() pode ler até dois varints além do fim do buffer
        // alocado antes de perceber que o registro está corrompido. Ou pode tentar
        // ler um inteiro grande do buffer. De qualquer forma, oito bytes 0x00
        // evitam uma leitura além do buffer.
        let n = api::column_bytes(&p_stmt, 4);
        let mut p_data = vec![0u8; n as usize + 8];
        if n != 0 {
            if let Some(blob) = api::column_blob(&p_stmt, 4) {
                let copy_len = (n as usize).min(blob.len());
                p_data[..copy_len].copy_from_slice(&blob[..copy_len]);
            }
        }
        idx.a_sample[i_sample].n = n;
        idx.a_sample[i_sample].p = p_data;
        idx.n_sample += 1;
    }
    rc = api::finalize(&p_stmt);
    if rc == SQLITE_OK {
        init_avg_eq(&p_prev_idx);
    }
    rc
}

/// Carrega o conteúdo da tabela sqlite_stat4 nos arrays Index.aSample[]
/// de todos os índices.
#[cfg(feature = "SQLITE_ENABLE_STAT4")]
fn load_stat4(db: &Sqlite3Ref, z_db: &[u8]) -> i32 {
    let mut rc: i32 = SQLITE_OK;

    assert!(db.borrow().lookaside.b_disable != 0);
    if optimization_enabled(&db.borrow(), SQLITE_STAT4) {
        if let Some(p_stat4) = find_table(db, b"sqlite_stat4", z_db) {
            if is_ordinary_table(&p_stat4.borrow()) {
                rc = load_stat_tbl(
                    db,
                    b"SELECT idx,count(*) FROM %Q.sqlite_stat4 GROUP BY idx COLLATE nocase",
                    b"SELECT idx,neq,nlt,ndlt,sample FROM %Q.sqlite_stat4",
                    z_db,
                );
            }
        }
    }
    rc
}

/// Carrega o conteúdo das tabelas sqlite_stat1 e sqlite_stat4. O conteúdo
/// de sqlite_stat1 é usado para preencher os arrays Index.aiRowEst[].
/// O conteúdo de sqlite_stat4 é usado para preencher os arrays Index.aSample[].
///
/// Se a tabela sqlite_stat1 não estiver presente no banco, SQLITE_ERROR
/// é retornado. Nesse caso, mesmo que SQLITE_ENABLE_STAT4 tenha sido definido
/// na compilação e a tabela sqlite_stat4 esteja presente, nenhum dado é lido dela.
///
/// Se SQLITE_ENABLE_STAT4 foi definido na compilação e a tabela
/// sqlite_stat4 não está presente no banco, SQLITE_ERROR é retornado. Porém,
/// nesse caso, os dados são lidos da tabela sqlite_stat1 (se presente) antes de retornar.
///
/// Se ocorrer um erro de OOM, esta função sempre marca db->mallocFailed.
/// Isso significa que, se o chamador não se importa com outros erros, o código
/// de retorno pode ser ignorado.
pub fn analysis_load(db: &Sqlite3Ref, i_db: i32) -> i32 {
    let mut rc: i32 = SQLITE_OK;
    let p_schema: SchemaRef;
    let z_database: Vec<u8>;

    {
        let d = db.borrow();
        assert!(i_db >= 0 && i_db < d.n_db);
        assert!(d.a_db[i_db as usize].p_bt.is_some());
        p_schema = d.a_db[i_db as usize].p_schema.clone();
        z_database = d.a_db[i_db as usize].z_db_s_name.clone();
    }

    // Limpa as estatísticas anteriores
    assert!(schema_mutex_held(db, i_db, None));
    let tabs: Vec<TableRef> = p_schema.borrow().tbl_hash.values().cloned().collect();
    for p_tab in tabs.iter() {
        p_tab.borrow_mut().tab_flags &= !TF_HASSTAT1;
    }
    let idxs: Vec<IndexRef> = p_schema.borrow().idx_hash.values().cloned().collect();
    for p_idx in idxs.iter() {
        p_idx.borrow_mut().has_stat1 = false;
        #[cfg(feature = "SQLITE_ENABLE_STAT4")]
        {
            delete_index_samples(db, p_idx);
            p_idx.borrow_mut().a_sample = Vec::new();
        }
    }

    // Carrega as estatísticas novas da tabela sqlite_stat1
    let s_info = AnalysisInfo {
        db: db.clone(),
        z_database: z_database.clone(),
    };
    if let Some(p_stat1) = find_table(db, b"sqlite_stat1", &s_info.z_database) {
        if is_ordinary_table(&p_stat1.borrow()) {
            let z_sql = m_printf(
                Some(db),
                b"SELECT tbl,idx,stat FROM %Q.sqlite_stat1",
                Some(&s_info.z_database),
            );
            match z_sql {
                None => rc = SQLITE_NOMEM_BKPT,
                Some(z) => {
                    rc = api::exec(db, &z, Some(analysis_loader), Some(&s_info), None);
                }
            }
        }
    }

    // Define os padrões apropriados em todos os índices que não estão em sqlite_stat1
    assert!(schema_mutex_held(db, i_db, None));
    let idxs: Vec<IndexRef> = p_schema.borrow().idx_hash.values().cloned().collect();
    for p_idx in idxs.iter() {
        let tem_stat1 = p_idx.borrow().has_stat1;
        if !tem_stat1 {
            default_row_est(p_idx);
        }
    }

    // Carrega as estatísticas da tabela sqlite_stat4.
    #[cfg(feature = "SQLITE_ENABLE_STAT4")]
    {
        if rc == SQLITE_OK {
            disable_lookaside(&mut db.borrow_mut());
            rc = load_stat4(db, &s_info.z_database);
            enable_lookaside(&mut db.borrow_mut());
        }
        let idxs: Vec<IndexRef> = p_schema.borrow().idx_hash.values().cloned().collect();
        for p_idx in idxs.iter() {
            p_idx.borrow_mut().ai_row_est = Vec::new();
        }
    }

    if rc == SQLITE_NOMEM {
        oom_fault(db);
    }
    rc
}

