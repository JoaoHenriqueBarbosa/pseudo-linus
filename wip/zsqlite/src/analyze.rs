//! `analyze.c`: o comando ANALYZE, as funções SQL internas `stat_init()`, `stat_push()` e
//! `stat_get()` e a carga do `sqlite_stat1` no esquema (`sqlite3AnalysisLoad`).
//!
//! Opções do Debian 13: `STAT4` está DESLIGADO, então só existe o `sqlite_stat1`. Somem, como os
//! `#ifdef SQLITE_ENABLE_STAT4` do C, `StatSample` e toda a amostragem, `loadStat4`, `loadStatTbl`,
//! `initAvgEq`, `findIndexOrPrimaryKey` e `sqlite3DeleteIndexSamples` (cujo corpo sem STAT4 é só
//! `UNUSED_PARAMETER`; por isso a função não existe). `IsStat4` vale 0: `stat_push` tem 2
//! argumentos e `stat_get` tem 1.
//!
//! Desvios do C, decorrentes do modelo v2 (CONVENTIONS.md):
//!
//! - as três funções SQL não são registradas por nome: o C as usa como `FuncDef` estáticas
//!   postas direto no `OP_Function` (não existem para o SQL do usuário). Aqui cada uma é um
//!   `Rc<FuncDef>` montado no ponto de uso;
//! - o `StatAccum*` que o C devolve como BLOB é um valor ponteiro (`Mem.pointer`), que o `Drop`
//!   do último dono libera no lugar do `statAccumDestructor`. O estado muda por `RefCell`
//!   porque o ponteiro é compartilhado entre as células (a cópia do registrador o duplica);
//! - o esquema é de `Rc<Table>` e `Rc<Index>` imutáveis: a carga das estatísticas altera cada
//!   entrada com `Rc::make_mut`, sobre a tabela hash do próprio esquema;
//! - `sqlite3DefaultRowEst` mora em `build3`; este módulo o reexporta para `build2`;
//! - a iteração sobre a tabela hash de tabelas, no `ANALYZE`, trabalha sobre uma cópia do vetor
//!   de `Rc<Table>` (a geração de código consulta o esquema e o `Parse`);
//! - `VdbeComment`, `VdbeCoverage` e as asserções de depuração somem (`SQLITE_ENABLE_EXPLAIN_COMMENTS`
//!   e `SQLITE_DEBUG` estão desligados).

use std::cell::RefCell;
use std::rc::Rc;

use crate::build::{
    find_db, find_index, find_table, locate_table, name_from_token, nested_parse,
    primary_key_index, table_lock, two_part_name,
};
use crate::build3::begin_write_operation;
use crate::callback::locate_coll_seq;
use crate::connection::{
    Connection, Context, FuncDef, Parse, ScalarFn, OPFLAG_APPEND, OPFLAG_P2ISREG,
};
use crate::consts::{
    OP_CLEAR, OP_COLUMN, OP_COUNT, OP_EXPIRE, OP_GOTO, OP_IF, OP_IFNOT, OP_INSERT, OP_INTEGER,
    OP_ISNULL, OP_LOADANALYSIS, OP_MAKERECORD, OP_NE, OP_NEWROWID, OP_NEXT, OP_NOOP, OP_NOTNULL,
    OP_NULL, OP_OPENREAD, OP_OPENWRITE, OP_REWIND, OP_SEEKGT, SQLITE_ANALYZE, SQLITE_NOMEM,
    SQLITE_NOMEM_BKPT, SQLITE_NULLEQ, SQLITE_OK, SQLITE_STAT4, SQLITE_UTF8, TF_HAS_STAT1,
};
use crate::expr_code2::touch_register;
use crate::func::{strglob, strlike};
use crate::hash::{hash_data_mut, hash_find_mut, hash_first, hash_iter, hash_next};
use crate::insert::open_table;
use crate::legacy::exec;
use crate::main::db_printf;
use crate::mem::Mem;
use crate::mem2::{mem_set_pointer, value_pointer};
use crate::pager::cstr;
use crate::prepare::{read_schema, schema_to_index};
use crate::printf::{result_str_accum, PrintfArg, StrAccum};
use crate::select::get_vdbe;
use crate::sqlite_int::{Index, Table, Token};
use crate::util::{at, atoi, log_est, oom_fault, str_icmp, LogEst};
use crate::vdbe_types::P4;
use crate::vdbeapi::{result_int, result_null, value_int, value_int64};
use crate::vdbeaux::{
    add_function_call, add_op0, add_op1, add_op2, add_op3, add_op4, add_op4_int, change_p4,
    change_p5, current_addr, jump_here, load_string, make_label, resolve_label, set_p4_key_info,
    vdbe_goto, vdbe_of_parse,
};
use crate::auth::auth_check;

use crate::build3::default_row_est;

/// O tipo do valor ponteiro que carrega o `StatAccum` entre `stat_init`, `stat_push` e
/// `stat_get`.
const STAT_ACCUM_TYPE: &[u8] = b"StatAccum";

/// `StatAccum` sem STAT4: o estado que as três funções compartilham.
struct StatAccum {
    /// `nEst`: número estimado de linhas.
    n_est: u64,
    /// `nRow`: linhas visitadas até agora.
    n_row: u64,
    /// `nLimit`: limite de linhas a varrer, ou 0 para nenhum.
    n_limit: i32,
    /// `nCol`: colunas do índice mais a chave primária ou rowid.
    n_col: i32,
    /// `nKeyCol`: colunas do índice sem a chave primária ou rowid.
    n_key_col: i32,
    /// `nSkipAhead`: quantas vezes o salto adiante rodou.
    n_skip_ahead: u8,
    /// `current.anDLt`: um contador de valores distintos por coluna.
    an_dlt: Vec<u64>,
}

/// O `FuncDef` estático (`statInitFuncdef` e companhia) de uma das funções internas.
fn stat_funcdef(z_name: &[u8], n_arg: i8, x_s_func: ScalarFn) -> Rc<FuncDef> {
    Rc::new(FuncDef {
        n_arg,
        func_flags: SQLITE_UTF8 as u32,
        x_s_func: Some(x_s_func),
        z_name: z_name.to_vec(),
        ..FuncDef::default()
    })
}

/// O `StatAccum` do argumento 0 de `stat_push` e `stat_get`.
fn stat_accum_of(arg: &Mem) -> Option<&RefCell<StatAccum>> {
    value_pointer(arg, STAT_ACCUM_TYPE)?.downcast_ref::<RefCell<StatAccum>>()
}

/// `openStatTable`: gera o código que abre as tabelas `sqlite_statN`. A `sqlite_stat1` é criada se
/// não existe; as `sqlite_stat3` e `sqlite_stat4` só são limpas se existem (o Debian não as
/// escreve). `z_where` é, se houver, o par (nome, coluna `tbl` ou `idx`) das entradas a apagar;
/// sem ele apaga tudo.
fn open_stat_table(
    db: &mut Connection,
    parse: &mut Parse,
    i_db: i32,
    i_stat_cur: i32,
    z_where: Option<(&[u8], &[u8])>,
) {
    // Nome e colunas de cada tabela; as colunas só existem para a que pode ser criada.
    const A_TABLE: [(&[u8], Option<&[u8]>); 3] = [
        (b"sqlite_stat1" as &[u8], Some(b"tbl,idx,stat" as &[u8])),
        (b"sqlite_stat4" as &[u8], None),
        (b"sqlite_stat3" as &[u8], None),
    ];
    // Sem STAT4 só a `sqlite_stat1` é aberta.
    const N_TO_OPEN: usize = 1;
    let mut a_root = [0u32; 3];
    let mut a_create_tbl = [0u16; 3];

    // Sem Vdbe o C retorna aqui; o `get_vdbe` do modelo v2 sempre entrega um.
    get_vdbe(db, parse);
    let z_db_s_name = db.dbs[i_db as usize].z_db_s_name.clone();

    // Cria as tabelas de estatística que não existem ou limpa as que já existem.
    for (i, (z_tab, z_cols)) in A_TABLE.iter().enumerate() {
        a_create_tbl[i] = 0;
        match find_table(db, z_tab, Some(&z_db_s_name)) {
            None => {
                if i < N_TO_OPEN {
                    // A tabela não existe: cria. Um efeito colateral do CREATE TABLE é deixar a
                    // página raiz da nova tabela em `parse.reg_root`, o que o OpenWrite abaixo
                    // vai precisar.
                    nested_parse(
                        db,
                        parse,
                        b"CREATE TABLE %Q.%s(%s)",
                        &[
                            PrintfArg::Text(Some(z_db_s_name.clone())),
                            PrintfArg::Text(Some(z_tab.to_vec())),
                            PrintfArg::Text(z_cols.map(<[u8]>::to_vec)),
                        ],
                    );
                    a_root[i] = parse.reg_root as u32;
                    a_create_tbl[i] = OPFLAG_P2ISREG;
                }
            }
            Some(p_stat) => {
                // A tabela já existe. Com `z_where` apaga só as entradas da tabela ou do índice;
                // sem ele apaga tudo.
                a_root[i] = p_stat.tnum;
                table_lock(db, parse, i_db, a_root[i], true, z_tab);
                if let Some((z_name, z_where_type)) = z_where {
                    nested_parse(
                        db,
                        parse,
                        b"DELETE FROM %Q.%s WHERE %s=%Q",
                        &[
                            PrintfArg::Text(Some(z_db_s_name.clone())),
                            PrintfArg::Text(Some(z_tab.to_vec())),
                            PrintfArg::Text(Some(z_where_type.to_vec())),
                            PrintfArg::Text(Some(z_name.to_vec())),
                        ],
                    );
                } else if db.x_pre_update_callback.is_some() {
                    nested_parse(
                        db,
                        parse,
                        b"DELETE FROM %Q.%s",
                        &[
                            PrintfArg::Text(Some(z_db_s_name.clone())),
                            PrintfArg::Text(Some(z_tab.to_vec())),
                        ],
                    );
                } else {
                    // A sqlite_stat[134] já existe: apaga todas as linhas.
                    add_op2(vdbe_of_parse(parse), OP_CLEAR, a_root[i] as i32, i_db);
                }
            }
        }
    }

    // Abre as tabelas sqlite_stat[134] para escrita.
    for i in 0..N_TO_OPEN {
        let v = vdbe_of_parse(parse);
        add_op4_int(v, OP_OPENWRITE, i_stat_cur + i as i32, a_root[i] as i32, i_db, 3);
        change_p5(v, a_create_tbl[i]);
    }
}

/// `statInit`: implementação de `stat_init(N,K,C,L)`.
///
/// - N: colunas do índice com o rowid ou a chave primária (nota: no índice de cobertura que
///   implementa uma tabela WITHOUT ROWID, N é o número de colunas da PRIMARY KEY);
/// - K: colunas do índice sem o rowid ou a chave primária;
/// - C: número estimado de linhas do índice;
/// - L: limite de linhas a varrer, ou 0 para nenhum.
///
/// Devolve o `StatAccum` como valor ponteiro.
fn stat_init(ctx: &mut Context<'_>, argv: &[Mem]) {
    let n_col = value_int(&argv[0]);
    debug_assert!(n_col > 0);
    let n_key_col = value_int(&argv[1]);
    debug_assert!(n_key_col <= n_col);
    debug_assert!(n_key_col > 0);
    let p = StatAccum {
        n_est: value_int64(&argv[2]) as u64,
        n_row: 0,
        n_limit: value_int64(&argv[3]) as i32,
        n_col,
        n_key_col,
        n_skip_ahead: 0,
        an_dlt: vec![0; n_col.max(0) as usize],
    };
    // `mem_set_pointer` exige a célula de resultado limpa (NULL).
    result_null(ctx);
    mem_set_pointer(&mut ctx.out, Box::new(RefCell::new(p)), STAT_ACCUM_TYPE);
}

/// `statPush`: implementação de `stat_push(P,C)`. P é o `StatAccum` criado por `stat_init()` e C
/// o índice da coluna mais à esquerda que difere da linha anterior. Costuma devolver NULL; devolve
/// um inteiro quando o código de bytes precisa de um tratamento especial (o salto adiante do
/// `PRAGMA analysis_limit`).
fn stat_push(ctx: &mut Context<'_>, argv: &[Mem]) {
    let i_chng = value_int(&argv[1]);
    let Some(cell) = stat_accum_of(&argv[0]) else {
        return;
    };
    let skip_ahead = {
        let mut p = cell.borrow_mut();
        debug_assert!(p.n_col > 0);
        debug_assert!(i_chng < p.n_col);

        // Na primeira chamada não há o que atualizar; as seguintes acrescentam um valor distinto
        // a cada coluna a partir da que mudou.
        if p.n_row != 0 {
            for slot in p.an_dlt.iter_mut().skip(i_chng.max(0) as usize) {
                *slot = slot.wrapping_add(1);
            }
        }
        p.n_row += 1;
        if p.n_limit != 0
            && p.n_row
                > (p.n_limit as i64 as u64).wrapping_mul(u64::from(p.n_skip_ahead) + 1)
        {
            p.n_skip_ahead = p.n_skip_ahead.wrapping_add(1);
            Some(p.an_dlt[0] > 0)
        } else {
            None
        }
    };
    if let Some(distinct_seen) = skip_ahead {
        result_int(ctx, distinct_seen as i32);
    }
}

/// `statGet`: implementação de `stat_get(P)`. Devolve o valor da coluna `stat` da linha do índice
/// no `sqlite_stat1`: o número de entradas do índice seguido de uma estimativa de linhas
/// casadas por consulta de igualdade para cada coluna. Com D valores distintos entre K linhas,
/// cada estimativa é I = (K+D-1)/D; mas se I está entre 1,0 e 1,1 (perto de 1,0 e só um pouco
/// acima) fica 1,0 em vez de arredondar para cima.
fn stat_get(ctx: &mut Context<'_>, argv: &[Mem]) {
    debug_assert!(argv.len() == 1);
    let Some(cell) = stat_accum_of(&argv[0]) else {
        return;
    };
    let mut s_stat = {
        let p = cell.borrow();
        let mut s_stat = StrAccum::new(((p.n_key_col + 1) * 100) as u32);
        let first = if p.n_skip_ahead != 0 { p.n_est } else { p.n_row };
        s_stat.appendf(b"%llu", &[PrintfArg::Int(first as i64)]);
        for i in 0..p.n_key_col as usize {
            let n_distinct = p.an_dlt[i].wrapping_add(1);
            let mut i_val = p.n_row.wrapping_add(n_distinct).wrapping_sub(1) / n_distinct;
            if i_val == 2 && p.n_row.wrapping_mul(10) <= n_distinct.wrapping_mul(11) {
                i_val = 1;
            }
            s_stat.appendf(b" %llu", &[PrintfArg::Int(i_val as i64)]);
        }
        s_stat
    };
    result_str_accum(ctx, &mut s_stat);
}

/// `callStatGet`: chama `stat_get(P)` sobre o registrador `reg_stat` e põe o resultado em
/// `reg_out`. Sem STAT4 o único `J` possível é `STAT_GET_STAT1`, por isso ele não é argumento.
fn call_stat_get(parse: &mut Parse, reg_stat: i32, reg_out: i32) {
    debug_assert!(reg_out != reg_stat && reg_out != reg_stat + 1);
    add_function_call(parse, 0, reg_stat, reg_out, 1, &stat_funcdef(b"stat_get", 1, stat_get), 0);
}

/// `analyzeOneTable`: gera o código que analisa todos os índices de uma tabela. `p_only_idx`, se
/// houver, é o único índice a analisar.
fn analyze_one_table(
    db: &mut Connection,
    parse: &mut Parse,
    p_tab: &Table,
    p_only_idx: Option<&Rc<Index>>,
    i_stat_cur: i32,
    i_mem: i32,
    i_tab: i32,
) {
    let mut i_mem = i_mem;
    let mut i_tab = i_tab;
    let mut need_table_cnt = true; // verdadeiro para contar a tabela
    let reg_new_rowid = i_mem; // rowid da linha inserida
    i_mem += 1;
    let reg_stat = i_mem; // registrador do StatAccum
    i_mem += 1;
    let reg_chng = i_mem; // índice do campo do índice que mudou
    i_mem += 1;
    let reg_rowid = i_mem; // rowid passado ao stat_push()
    i_mem += 1;
    let reg_temp = i_mem; // registrador temporário
    i_mem += 1;
    let reg_temp2 = i_mem; // segundo registrador temporário
    i_mem += 1;
    let reg_tabname = i_mem; // registrador com o nome da tabela
    i_mem += 1;
    let reg_idxname = i_mem; // registrador com o nome do índice
    i_mem += 1;
    let reg_stat1 = i_mem; // valor da coluna stat do sqlite_stat1
    i_mem += 1;
    let reg_prev = i_mem; // PRECISA SER O ÚLTIMO (ver abaixo)

    touch_register(parse, i_mem);
    get_vdbe(db, parse);
    if !p_tab.is_ordinary_table() {
        // Não colhe estatística de views nem de tabelas virtuais.
        return;
    }
    if strlike(Some(&b"sqlite\\_%"[..]), Some(&p_tab.z_name), u32::from(b'\\')) == 0 {
        // Não colhe estatística das tabelas do sistema.
        return;
    }
    let i_db = schema_to_index(db, p_tab.p_schema);
    debug_assert!(i_db >= 0);
    let z_db_s_name = db.dbs[i_db as usize].z_db_s_name.clone();
    if auth_check(db, parse, SQLITE_ANALYZE, Some(&p_tab.z_name), None, Some(&z_db_s_name)) != 0 {
        return;
    }

    // Com o gancho de pré-atualização o INSERT no sqlite_stat1 precisa de um `Table` para
    // descrevê-lo ao gancho.
    let p_stat1: Option<Rc<Table>> = if db.x_pre_update_callback.is_some() {
        let stat1 = Rc::new(Table {
            z_name: b"sqlite_stat1".to_vec(),
            n_col: 3,
            i_p_key: -1,
            ..Table::default()
        });
        add_op4(vdbe_of_parse(parse), OP_NOOP, 0, 0, 0, P4::Table(Rc::clone(&stat1)));
        Some(stat1)
    } else {
        None
    };

    // Trava a tabela no nível do cache compartilhado e abre um cursor de leitura sobre ela. Também
    // reserva um número de cursor para varrer os índices (`i_idx_cur`), sem abri-lo ainda.
    table_lock(db, parse, i_db, p_tab.tnum, false, &p_tab.z_name);
    let i_tab_cur = i_tab; // cursor da tabela
    i_tab += 1;
    let i_idx_cur = i_tab; // cursor do índice em análise
    i_tab += 1;
    parse.n_tab = parse.n_tab.max(i_tab);
    open_table(db, parse, i_tab_cur, i_db, p_tab, OP_OPENREAD);
    load_string(vdbe_of_parse(parse), reg_tabname, &p_tab.z_name);

    for p_idx in p_tab.p_index.iter() {
        if let Some(only) = p_only_idx {
            if !Rc::ptr_eq(only, p_idx) {
                continue;
            }
        }
        if p_idx.p_partial_idx_where.is_none() {
            need_table_cnt = false;
        }
        let n_col: i32; // número de colunas de `p_idx`: "N"
        let z_idx_name: &[u8]; // nome do índice
        let n_col_test: i32; // colunas a testar por mudança
        if !p_tab.has_rowid() && p_idx.is_primary_key_index() {
            n_col = i32::from(p_idx.n_key_col);
            z_idx_name = &p_tab.z_name;
            n_col_test = n_col - 1;
        } else {
            n_col = i32::from(p_idx.n_column);
            z_idx_name = &p_idx.z_name;
            n_col_test = if p_idx.uniq_not_null { i32::from(p_idx.n_key_col) - 1 } else { n_col - 1 };
        }

        // Carrega o registrador com o nome do índice.
        load_string(vdbe_of_parse(parse), reg_idxname, z_idx_name);

        // Pseudocódigo do laço que chama stat_push():
        //
        //   regChng = 0
        //   Rewind csr
        //   if eof(csr){
        //      stat_init() com count = 0;
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

        // Garante células de memória para o vetor `regPrev` e para o rowid final.
        touch_register(parse, reg_prev + n_col_test);

        // Abre um cursor de leitura sobre o índice em análise.
        debug_assert!(i_db == schema_to_index(db, p_tab.p_schema));
        add_op3(vdbe_of_parse(parse), OP_OPENREAD, i_idx_cur, p_idx.tnum as i32, i_db);
        set_p4_key_info(parse, db, p_idx);

        // Implementa:
        //
        //   regChng = 0
        //   Rewind csr
        //   if eof(csr){
        //      stat_init() com count = 0;
        //      goto end_of_scan;
        //   }
        //   count()
        //   stat_init()
        //   goto chng_addr_0;
        debug_assert!(reg_temp2 == reg_stat + 4);
        add_op2(vdbe_of_parse(parse), OP_INTEGER, db.n_analysis_limit, reg_temp2);

        // Argumentos do stat_init():
        //    (1) o número de colunas do índice com o rowid (ou, numa tabela WITHOUT ROWID, as
        //        colunas da chave primária),
        //    (2) o número de colunas da chave sem o rowid ou a chave primária,
        //    (3) o número estimado de linhas do índice.
        add_op2(vdbe_of_parse(parse), OP_INTEGER, n_col, reg_stat + 1);
        debug_assert!(reg_rowid == reg_stat + 2);
        add_op2(vdbe_of_parse(parse), OP_INTEGER, i32::from(p_idx.n_key_col), reg_rowid);
        add_op3(
            vdbe_of_parse(parse),
            OP_COUNT,
            i_idx_cur,
            reg_temp,
            i32::from(db.optimization_disabled(SQLITE_STAT4)),
        );
        add_function_call(
            parse,
            0,
            reg_stat + 1,
            reg_stat,
            4,
            &stat_funcdef(b"stat_init", 4, stat_init),
            0,
        );
        let mut addr_goto_end = add_op1(vdbe_of_parse(parse), OP_REWIND, i_idx_cur);

        add_op2(vdbe_of_parse(parse), OP_INTEGER, 0, reg_chng);
        let mut addr_next_row = current_addr(parse);

        if n_col_test > 0 {
            let end_distinct_test = make_label(parse);
            let mut a_goto_chng: Vec<i32> = Vec::with_capacity(n_col_test as usize);

            //  next_row:
            //   regChng = 0
            //   if( idx(0) != regPrev(0) ) goto chng_addr_0
            //   regChng = 1
            //   if( idx(1) != regPrev(1) ) goto chng_addr_1
            //   ...
            //   regChng = N
            //   goto endDistinctTest
            add_op0(vdbe_of_parse(parse), OP_GOTO);
            addr_next_row = current_addr(parse);
            if n_col_test == 1 && p_idx.n_key_col == 1 && p_idx.is_unique_index() {
                // Num índice UNIQUE de uma coluna, achada uma linha não NULL, todas as demais
                // são distintas: pula os testes de distinção seguintes.
                add_op2(vdbe_of_parse(parse), OP_NOTNULL, reg_prev, end_distinct_test);
            }
            for i in 0..n_col_test {
                let p_coll = locate_coll_seq(db, parse, &p_idx.az_coll[i as usize]);
                let v = vdbe_of_parse(parse);
                add_op2(v, OP_INTEGER, i, reg_chng);
                add_op3(v, OP_COLUMN, i_idx_cur, i, reg_temp);
                a_goto_chng.push(add_op4(v, OP_NE, reg_temp, 0, reg_prev + i, P4::Coll(p_coll)));
                change_p5(v, u16::from(SQLITE_NULLEQ));
            }
            add_op2(vdbe_of_parse(parse), OP_INTEGER, n_col_test, reg_chng);
            vdbe_goto(vdbe_of_parse(parse), end_distinct_test);

            //  chng_addr_0:
            //   regPrev(0) = idx(0)
            //  chng_addr_1:
            //   regPrev(1) = idx(1)
            //  ...
            jump_here(vdbe_of_parse(parse), addr_next_row - 1);
            for (i, addr_chng) in a_goto_chng.iter().enumerate() {
                let v = vdbe_of_parse(parse);
                jump_here(v, *addr_chng);
                add_op3(v, OP_COLUMN, i_idx_cur, i as i32, reg_prev + i as i32);
            }
            resolve_label(parse, db, end_distinct_test);
        }

        //  chng_addr_N:
        //   stat_push(P, regChng)
        //   Next csr
        //   if !eof(csr) goto next_row;
        debug_assert!(reg_chng == reg_stat + 1);
        add_function_call(
            parse,
            1,
            reg_stat,
            reg_temp,
            2,
            &stat_funcdef(b"stat_push", 2, stat_push),
            0,
        );
        if db.n_analysis_limit != 0 {
            let v = vdbe_of_parse(parse);
            let j1 = add_op1(v, OP_ISNULL, reg_temp);
            let j2 = add_op1(v, OP_IF, reg_temp);
            let j3 = add_op4_int(v, OP_SEEKGT, i_idx_cur, 0, reg_prev, 1);
            jump_here(v, j1);
            add_op2(v, OP_NEXT, i_idx_cur, addr_next_row);
            jump_here(v, j2);
            jump_here(v, j3);
        } else {
            add_op2(vdbe_of_parse(parse), OP_NEXT, i_idx_cur, addr_next_row);
        }

        // Acrescenta a entrada na tabela stat1.
        if p_idx.p_partial_idx_where.is_some() {
            // Índices parciais podem ganhar uma entrada zerada no sqlite_stat1, mas uma tabela
            // vazia é omitida dele.
            jump_here(vdbe_of_parse(parse), addr_goto_end);
            addr_goto_end = 0;
        }
        call_stat_get(parse, reg_stat, reg_stat1);
        let v = vdbe_of_parse(parse);
        add_op4(v, OP_MAKERECORD, reg_tabname, 3, reg_temp, P4::Text(b"BBB".to_vec()));
        add_op2(v, OP_NEWROWID, i_stat_cur, reg_new_rowid);
        add_op3(v, OP_INSERT, i_stat_cur, reg_temp, reg_new_rowid);
        if let Some(stat1) = &p_stat1 {
            change_p4(v, -1, P4::Table(Rc::clone(stat1)));
        }
        change_p5(v, OPFLAG_APPEND);

        // Fim da análise.
        if addr_goto_end != 0 {
            jump_here(vdbe_of_parse(parse), addr_goto_end);
        }
    }

    // Cria uma única entrada no sqlite_stat1 com NULL como nome do índice e a contagem de linhas
    // como conteúdo.
    if p_only_idx.is_none() && need_table_cnt {
        let v = vdbe_of_parse(parse);
        add_op2(v, OP_COUNT, i_tab_cur, reg_stat1);
        let j_zero_rows = add_op1(v, OP_IFNOT, reg_stat1);
        add_op2(v, OP_NULL, 0, reg_idxname);
        add_op4(v, OP_MAKERECORD, reg_tabname, 3, reg_temp, P4::Text(b"BBB".to_vec()));
        add_op2(v, OP_NEWROWID, i_stat_cur, reg_new_rowid);
        add_op3(v, OP_INSERT, i_stat_cur, reg_temp, reg_new_rowid);
        change_p5(v, OPFLAG_APPEND);
        if let Some(stat1) = &p_stat1 {
            change_p4(v, -1, P4::Table(Rc::clone(stat1)));
        }
        jump_here(v, j_zero_rows);
    }
}

/// `loadAnalysis`: gera o código que carrega a análise mais recente nas tabelas hash internas,
/// onde o planejador a usa.
fn load_analysis(db: &mut Connection, parse: &mut Parse, i_db: i32) {
    add_op1(get_vdbe(db, parse), OP_LOADANALYSIS, i_db);
}

/// `analyzeDatabase`: gera o código que analisa um banco inteiro.
fn analyze_database(db: &mut Connection, parse: &mut Parse, i_db: i32) {
    begin_write_operation(db, parse, 0, i_db);
    let i_stat_cur = parse.n_tab;
    parse.n_tab += 3;
    open_stat_table(db, parse, i_db, i_stat_cur, None);
    let i_mem = parse.n_mem + 1;
    let i_tab = parse.n_tab;
    let tables: Vec<Rc<Table>> = hash_iter(&db.dbs[i_db as usize].schema.tbl_hash)
        .map(|(_, p_tab)| Rc::clone(p_tab))
        .collect();
    for p_tab in &tables {
        analyze_one_table(db, parse, p_tab, None, i_stat_cur, i_mem, i_tab);
        // Sem STAT4 o registrador livre seguinte não muda (`iMem` fica como está).
    }
    load_analysis(db, parse, i_db);
}

/// `analyzeTable`: gera o código que analisa uma tabela de um banco. `p_only_idx`, se houver, é o
/// único índice de `p_tab` a analisar.
fn analyze_table(
    db: &mut Connection,
    parse: &mut Parse,
    p_tab: &Rc<Table>,
    p_only_idx: Option<&Rc<Index>>,
) {
    let i_db = schema_to_index(db, p_tab.p_schema);
    begin_write_operation(db, parse, 0, i_db);
    let i_stat_cur = parse.n_tab;
    parse.n_tab += 3;
    if let Some(p_idx) = p_only_idx {
        open_stat_table(db, parse, i_db, i_stat_cur, Some((p_idx.z_name.as_slice(), &b"idx"[..])));
    } else {
        open_stat_table(db, parse, i_db, i_stat_cur, Some((p_tab.z_name.as_slice(), &b"tbl"[..])));
    }
    let i_mem = parse.n_mem + 1;
    let i_tab = parse.n_tab;
    analyze_one_table(db, parse, p_tab, p_only_idx, i_stat_cur, i_mem, i_tab);
    load_analysis(db, parse, i_db);
}

/// `sqlite3Analyze`: gera o código do comando ANALYZE; o analisador a chama ao reconhecê-lo.
///
/// ```text
///   ANALYZE                            -- 1
///   ANALYZE  <database>                -- 2
///   ANALYZE  ?<database>.?<tablename>  -- 3
/// ```
///
/// A forma 1 analisa todos os índices de todos os bancos anexados; a 2, todos os do banco
/// nomeado; a 3, todos os da tabela (ou só o índice) nomeada.
pub fn analyze(
    db: &mut Connection,
    parse: &mut Parse,
    p_name1: Option<&Token>,
    p_name2: Option<&Token>,
) {
    // Lê o esquema. Se houver erro, a mensagem e o código ficam em `parse`.
    if SQLITE_OK != read_schema(db, parse) {
        return;
    }

    debug_assert!(p_name2.is_some() || p_name1.is_none());
    match (p_name1, p_name2) {
        (Some(name1), Some(name2)) => {
            let i_db_named = if name2.z.is_empty() { find_db(db, name1) } else { -1 };
            if i_db_named >= 0 {
                // Analisa o esquema que o argumento nomeia.
                analyze_database(db, parse, i_db_named);
            } else if let Some((i_db, p_table_name)) = two_part_name(db, parse, name1, name2) {
                // Forma 3: analisa a tabela ou o índice nomeado.
                let z_db = if name2.z.is_empty() {
                    None
                } else {
                    Some(db.dbs[i_db as usize].z_db_s_name.clone())
                };
                if let Some(z) = name_from_token(Some(p_table_name)) {
                    if let Some((p_tab, p_idx)) = find_index(db, &z, z_db.as_deref()) {
                        analyze_table(db, parse, &p_tab, Some(&p_idx));
                    } else if let Some(p_tab) = locate_table(db, parse, 0, &z, z_db.as_deref()) {
                        analyze_table(db, parse, &p_tab, None);
                    }
                }
            }
        }
        _ => {
            // Forma 1: analisa tudo, menos o banco TEMP.
            let mut i = 0usize;
            while i < db.dbs.len() {
                if i != 1 {
                    analyze_database(db, parse, i as i32);
                }
                i += 1;
            }
        }
    }
    if db.n_sql_exec == 0 {
        add_op0(get_vdbe(db, parse), OP_EXPIRE);
    }
}

/// `decodeIntArray`: lê os primeiros `n_out` inteiros da lista separada por espaços em
/// `z_int_array` e grava o `LogEst` de cada um em `a_log`. Depois trata as palavras de controle
/// (`unordered`, `sz=N`, `noskipscan`) e a marca de baixa qualidade de `p_index`.
fn decode_int_array(z_int_array: &[u8], n_out: usize, a_log: &mut [LogEst], p_index: &mut Index) {
    let z = cstr(z_int_array);
    let mut pos = 0usize;
    let mut i = 0usize;
    while at(z, pos) != 0 && i < n_out {
        let mut v: u64 = 0;
        loop {
            let c = at(z, pos);
            if !c.is_ascii_digit() {
                break;
            }
            v = v.wrapping_mul(10).wrapping_add(u64::from(c - b'0'));
            pos += 1;
        }
        if let Some(slot) = a_log.get_mut(i) {
            *slot = log_est(v);
        }
        if at(z, pos) == b' ' {
            pos += 1;
        }
        i += 1;
    }
    p_index.b_unordered = false;
    p_index.no_skip_scan = false;
    while at(z, pos) != 0 {
        let rest = &z[pos..];
        if strglob(Some(&b"unordered*"[..]), Some(rest)) == 0 {
            p_index.b_unordered = true;
        } else if strglob(Some(&b"sz=[0-9]*"[..]), Some(rest)) == 0 {
            let sz = atoi(&rest[3..]).max(2);
            p_index.sz_idx_row = log_est(sz as u64);
        } else if strglob(Some(&b"noskipscan*"[..]), Some(rest)) == 0 {
            p_index.no_skip_scan = true;
        }
        while at(z, pos) != 0 && at(z, pos) != b' ' {
            pos += 1;
        }
        while at(z, pos) == b' ' {
            pos += 1;
        }
    }

    // Liga `b_low_qual` se o pico de linhas de uma igualdade completa é tão grande que a varredura
    // da tabela provavelmente ganha do índice.
    if n_out > 0 {
        let first = a_log.first().copied().unwrap_or(0);
        let last = a_log.get(n_out - 1).copied().unwrap_or(0);
        if first > 66 /* o índice tem mais de 100 linhas */ && first <= last
        /* e só um valor foi visto */
        {
            p_index.b_low_qual = true;
        }
    }
}

/// `analysisLoader`: chamada uma vez por linha do `sqlite_stat1`, com `argv[0]` o nome da tabela,
/// `argv[1]` o do índice (pode ser NULL) e `argv[2]` o resultado da análise, um inteiro por coluna.
/// A linha com `argv[1]` NULL só registra o número de linhas da tabela.
fn analysis_loader(
    db: &mut Connection,
    i_db: usize,
    z_database: &[u8],
    argv: &[Option<Vec<u8>>],
) -> i32 {
    debug_assert!(argv.len() == 3);
    let (Some(z_tbl), Some(z)) = (
        argv.first().and_then(|a| a.as_deref()),
        argv.get(2).and_then(|a| a.as_deref()),
    ) else {
        return 0;
    };
    let Some(p_table) = find_table(db, z_tbl, Some(z_database)) else {
        return 0;
    };
    let z_idx = argv.get(1).and_then(|a| a.as_deref());
    // O dono do índice, o nome dele e o número de colunas da chave; nada de `Rc` fica vivo
    // durante a alteração do esquema, para `Rc::make_mut` não copiar à toa.
    let p_index: Option<(Vec<u8>, Vec<u8>)> = match z_idx {
        None => None,
        Some(z_idx) if str_icmp(z_tbl, z_idx) == 0 => {
            primary_key_index(&p_table).map(|i| (p_table.z_name.clone(), i.z_name.clone()))
        }
        Some(z_idx) => {
            find_index(db, z_idx, Some(z_database)).map(|(t, i)| (t.z_name.clone(), i.z_name.clone()))
        }
    };
    let z_table_name = p_table.z_name.clone();
    let sz_tab_row = p_table.sz_tab_row;
    let mut n_row_log_est = p_table.n_row_log_est;
    drop(p_table);

    let schema = &mut db.dbs[i_db].schema;
    if let Some((z_owner, z_index_name)) = p_index {
        let mut row_log_est0: Option<LogEst> = None;
        if let Some(owner) = hash_find_mut(&mut schema.tbl_hash, &z_owner) {
            let owner = Rc::make_mut(owner);
            if let Some(slot) =
                owner.p_index.iter_mut().find(|x| str_icmp(&x.z_name, &z_index_name) == 0)
            {
                let p_idx = Rc::make_mut(slot);
                let n_col = usize::from(p_idx.n_key_col) + 1;
                p_idx.b_unordered = false;
                let mut a_log = std::mem::take(&mut p_idx.ai_row_log_est);
                if a_log.len() < n_col {
                    a_log.resize(n_col, 0);
                }
                decode_int_array(z, n_col, &mut a_log, p_idx);
                p_idx.ai_row_log_est = a_log;
                p_idx.has_stat1 = true;
                if p_idx.p_partial_idx_where.is_none() {
                    row_log_est0 = p_idx.ai_row_log_est.first().copied();
                }
            }
        }
        if let Some(n_row) = row_log_est0 {
            if let Some(p_tab) = hash_find_mut(&mut schema.tbl_hash, &z_table_name) {
                let p_tab = Rc::make_mut(p_tab);
                p_tab.n_row_log_est = n_row;
                p_tab.tab_flags |= TF_HAS_STAT1;
            }
        }
    } else {
        let mut fake_idx = Index { sz_idx_row: sz_tab_row, ..Index::default() };
        decode_int_array(z, 1, std::slice::from_mut(&mut n_row_log_est), &mut fake_idx);
        if let Some(p_tab) = hash_find_mut(&mut schema.tbl_hash, &z_table_name) {
            let p_tab = Rc::make_mut(p_tab);
            p_tab.n_row_log_est = n_row_log_est;
            p_tab.sz_tab_row = fake_idx.sz_idx_row;
            p_tab.tab_flags |= TF_HAS_STAT1;
        }
    }
    0
}

/// `sqlite3AnalysisLoad`: carrega o conteúdo do `sqlite_stat1` do banco `i_db` nas estimativas
/// dos índices (`ai_row_log_est`) e das tabelas. Devolve `SQLITE_OK`, ou o erro da leitura do
/// `sqlite_stat1`; os índices que ficam sem entrada recebem as estimativas padrão.
pub fn analysis_load(db: &mut Connection, i_db: i32) -> i32 {
    let i_db = i_db as usize;
    debug_assert!(i_db < db.dbs.len());
    debug_assert!(db.dbs[i_db].bt.is_some());
    let mut rc = SQLITE_OK;

    // Apaga as estatísticas anteriores.
    {
        let schema = &mut db.dbs[i_db].schema;
        let mut cur = hash_first(&schema.tbl_hash);
        while let Some(elem) = cur {
            let next = hash_next(&schema.tbl_hash, elem);
            let entry = hash_data_mut(&mut schema.tbl_hash, elem);
            if entry.tab_flags & TF_HAS_STAT1 != 0 || entry.p_index.iter().any(|i| i.has_stat1) {
                let p_tab = Rc::make_mut(entry);
                p_tab.tab_flags &= !TF_HAS_STAT1;
                for slot in p_tab.p_index.iter_mut().filter(|i| i.has_stat1) {
                    Rc::make_mut(slot).has_stat1 = false;
                }
            }
            cur = next;
        }
    }

    // Carrega as estatísticas novas da tabela sqlite_stat1.
    let z_database = db.dbs[i_db].z_db_s_name.clone();
    let has_stat1 = find_table(db, b"sqlite_stat1", Some(&z_database))
        .is_some_and(|p_stat1| p_stat1.is_ordinary_table());
    if has_stat1 {
        match db_printf(
            db,
            b"SELECT tbl,idx,stat FROM %Q.sqlite_stat1",
            &[PrintfArg::Text(Some(z_database.clone()))],
        ) {
            None => rc = SQLITE_NOMEM_BKPT,
            Some(z_sql) => {
                let mut x_callback =
                    |db: &mut Connection, argv: &[Option<Vec<u8>>], _cols: &[Vec<u8>]| {
                        analysis_loader(db, i_db, &z_database, argv)
                    };
                rc = exec(db, &z_sql, Some(&mut x_callback));
            }
        }
    }

    // Põe as estimativas padrão em todo índice que não está no sqlite_stat1.
    {
        let schema = &mut db.dbs[i_db].schema;
        let mut cur = hash_first(&schema.tbl_hash);
        while let Some(elem) = cur {
            let next = hash_next(&schema.tbl_hash, elem);
            let entry = hash_data_mut(&mut schema.tbl_hash, elem);
            if entry.p_index.iter().any(|i| !i.has_stat1) {
                let Table { n_row_log_est, p_index, .. } = Rc::make_mut(entry);
                for slot in p_index.iter_mut().filter(|i| !i.has_stat1) {
                    default_row_est(Rc::make_mut(slot), n_row_log_est);
                }
            }
            cur = next;
        }
    }

    if rc == SQLITE_NOMEM {
        oom_fault(db);
    }
    rc
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decode_reads_counts_and_flags() {
        let mut idx = Index::default();
        let mut a_log: [LogEst; 3] = [0; 3];
        decode_int_array(b"10000 100 1 unordered noskipscan sz=64", 3, &mut a_log, &mut idx);
        assert_eq!(a_log, [log_est(10000), log_est(100), log_est(1)]);
        assert!(idx.b_unordered);
        assert!(idx.no_skip_scan);
        assert_eq!(idx.sz_idx_row, log_est(64));
        assert!(!idx.b_low_qual);
    }

    #[test]
    fn decode_marks_low_quality_when_one_value_seen() {
        let mut idx = Index::default();
        let mut a_log: [LogEst; 2] = [0; 2];
        decode_int_array(b"1000 1000", 2, &mut a_log, &mut idx);
        assert!(idx.b_low_qual);
    }

    #[test]
    fn decode_clamps_row_size_to_two() {
        let mut idx = Index::default();
        let mut a_log: [LogEst; 1] = [0; 1];
        decode_int_array(b"5 sz=1", 1, &mut a_log, &mut idx);
        assert_eq!(idx.sz_idx_row, log_est(2));
    }
}
