// Mesclado das partes traduzidas de insert_c (as partes quebram funções grandes no meio).
#![allow(unused_imports, ambiguous_glob_reexports)]
use crate::prelude::*;

// ---- part_000.rs ----

// Tradução de insert.c (SQLite 3.46.1), trecho 000: sqlite3OpenTable até
// sqlite3ComputeGeneratedColumns. Notas para o integrador:
//  - VdbeComment() só existe com SQLITE_ENABLE_EXPLAIN_COMMENTS (ausente no Debian), some.
//  - As strings de afinidade (Index.zColAff, Table.zColAff) ficam em `Vec<u8>` SEM o
//    terminador nulo do C. Vazio significa "ainda não calculada" (para a tabela, uma
//    string toda BLOB também vira vazia, então é recalculada, com o mesmo resultado).
//  - Falha de alocação (sqlite3OomFault) não existe: `Vec` não falha.

/// Gera código que vai
/// (1) adquirir uma trava para a tabela pTab e depois
/// (2) abrir pTab como cursor iCur.
///
/// Se pTab é uma tabela WITHOUT ROWID, então é o índice PRIMARY KEY
/// da tabela que é efetivamente aberto.
pub fn open_table(
    p_parse: &mut Parse,
    i_cur: i32,
    i_db: i32,
    p_tab: &TableRef,
    opcode: i32,
) {
    debug_assert!(!is_virtual(&p_tab.borrow()));
    debug_assert!(p_parse.p_vdbe.is_some());
    let v = p_parse.p_vdbe.clone().unwrap();
    debug_assert!(opcode == OP_OPENWRITE as i32 || opcode == OP_OPENREAD as i32);
    let db = p_parse.db.upgrade().expect("conexão encerrada");
    let no_shared_cache = db.borrow().no_shared_cache;
    let (tnum, n_nv_col, z_name) = {
        let t = p_tab.borrow();
        (t.tnum, t.n_nv_col, t.z_name.clone())
    };
    if no_shared_cache == 0 {
        table_lock(
            p_parse,
            i_db,
            tnum,
            if opcode == OP_OPENWRITE as i32 { 1 } else { 0 },
            &z_name,
        );
    }
    let has_rowid_tab = has_rowid(&p_tab.borrow());
    if has_rowid_tab {
        vdbe_add_op4_int(&mut v.borrow_mut(), opcode, i_cur, tnum as i32, i_db, n_nv_col as i32);
    } else {
        let p_pk = primary_key_index(&p_tab.borrow());
        debug_assert!(p_pk.is_some());
        let p_pk = p_pk.unwrap();
        let pk_tnum = p_pk.borrow().tnum;
        debug_assert!(pk_tnum == tnum || corrupt_db(&db));
        vdbe_add_op3(&mut v.borrow_mut(), opcode, i_cur, pk_tnum as i32, i_db);
        vdbe_set_p4_key_info(p_parse, &p_pk);
    }
}

/// Calcula (na primeira vez) e guarda na estrutura Index a string de afinidade de
/// coluna. Uma string de afinidade de coluna tem um caractere para cada coluna da
/// tabela, de acordo com a afinidade da coluna:
///
///  Caractere      Afinidade da coluna
///  'A'            BLOB
///  'B'            TEXT
///  'C'            NUMERIC
///  'D'            INTEGER
///  'F'            REAL
///
/// Um 'D' extra é anexado no fim da string para cobrir o rowid que aparece como
/// a última coluna em todo índice. A memória é gerenciada junto com o resto da
/// estrutura Index e liberada em sqlite3DeleteIndex().
fn compute_index_aff_str(_db: &Sqlite3Ref, p_idx: &IndexRef) -> Option<Vec<u8>> {
    let p_tab = p_idx.borrow().p_table.upgrade()?;
    let n_column = p_idx.borrow().n_column as usize;
    let mut z_col_aff: Vec<u8> = Vec::with_capacity(n_column + 1);
    for n in 0..n_column {
        let x: i16 = p_idx.borrow().ai_column[n];
        let mut aff: u8;
        if x >= 0 {
            aff = p_tab.borrow().a_col[x as usize].affinity;
        } else if x == XN_ROWID {
            aff = SQLITE_AFF_INTEGER;
        } else {
            debug_assert!(x == XN_EXPR);
            let idx = p_idx.borrow();
            debug_assert!(idx.b_has_expr);
            debug_assert!(idx.a_col_expr.is_some());
            let p_expr = idx.a_col_expr.as_ref().unwrap().a[n].p_expr.as_deref().unwrap();
            aff = expr_affinity(p_expr);
        }
        if aff < SQLITE_AFF_BLOB {
            aff = SQLITE_AFF_BLOB;
        }
        if aff > SQLITE_AFF_NUMERIC {
            aff = SQLITE_AFF_NUMERIC;
        }
        z_col_aff.push(aff);
    }
    p_idx.borrow_mut().z_col_aff = z_col_aff.clone();
    Some(z_col_aff)
}

/// Devolve a string de afinidade de coluna associada ao índice pIdx, calculando-a
/// na primeira chamada.
pub fn index_affinity_str(db: &Sqlite3Ref, p_idx: &IndexRef) -> Option<Vec<u8>> {
    if p_idx.borrow().z_col_aff.is_empty() {
        return compute_index_aff_str(db, p_idx);
    }
    Some(p_idx.borrow().z_col_aff.clone())
}

/// Calcula uma string de afinidade para uma tabela. O chamador é o dono do
/// resultado.
pub fn table_affinity_str(_db: Option<&Sqlite3Ref>, p_tab: &Table) -> Option<Vec<u8>> {
    let mut z_col_aff: Vec<u8> = Vec::with_capacity(p_tab.n_col as usize + 1);
    for i in 0..p_tab.n_col as usize {
        if (p_tab.a_col[i].col_flags & COLFLAG_VIRTUAL) == 0 {
            z_col_aff.push(p_tab.a_col[i].affinity);
        }
    }
    // O laço do C escreve o terminador na posição seguinte à última coluna e depois
    // recua enquanto a afinidade final for <= BLOB (omite as BLOB à direita).
    while let Some(&last) = z_col_aff.last() {
        if last <= SQLITE_AFF_BLOB {
            z_col_aff.pop();
        } else {
            break;
        }
    }
    Some(z_col_aff)
}

/// Faz mudanças no bytecode em evolução para fazer transformações de afinidade
/// de valores que estão prestes a ser reunidos em uma linha para a tabela pTab.
///
/// Para tabelas ordinárias (legado, não estritas): calcula a string de afinidade
/// da tabela, se ainda não foi calculada, omitindo as afinidades BLOB à direita.
/// Se a string ficou vazia, a rotina é um sem-op. Senão, se iReg>0, codifica um
/// OP_Affinity que define as afinidades do registro iReg em diante; se iReg==0,
/// apenas define o P4 do opcode anterior (que deveria ser um OP_MakeRecord) com a
/// string de afinidade.
///
/// Para tabelas STRICT: gera um OP_TypeCheck que verifica os tipos contra as
/// definições de coluna de pTab. Se iReg==0, um OP_MakeRecord já foi gerado e é o
/// último opcode; o novo OP_TypeCheck é inserido antes dele, com o mesmo conjunto
/// de registros. Se iReg>0, é o primeiro de uma série de registros do novo registro.
pub fn table_affinity(v: &mut Vdbe, p_tab: &TableRef, i_reg: i32) {
    let tab_flags = p_tab.borrow().tab_flags;
    if (tab_flags & TF_STRICT) != 0 {
        if i_reg == 0 {
            // Move o opcode anterior (que deveria ser OP_MakeRecord) uma posição
            // adiante e insere um novo OP_TypeCheck onde o OP_MakeRecord estava.
            vdbe_append_p4(v, P4Value::Table(p_tab.clone()), P4_TABLE as i32);
            let (p1, p2, p3) = {
                let p_prev = vdbe_get_last_op(v);
                debug_assert!(
                    p_prev.opcode == OP_MAKERECORD
                        || vdbe_db(v).borrow().malloc_failed != 0
                );
                p_prev.opcode = OP_TYPECHECK;
                (p_prev.p1, p_prev.p2, p_prev.p3)
            };
            vdbe_add_op3(v, OP_MAKERECORD as i32, p1, p2, p3);
        } else {
            // Insere um OP_TypeCheck isolado
            let n_nv_col = p_tab.borrow().n_nv_col as i32;
            vdbe_add_op2(v, OP_TYPECHECK as i32, i_reg, n_nv_col);
            vdbe_append_p4(v, P4Value::Table(p_tab.clone()), P4_TABLE as i32);
        }
        return;
    }
    let mut z_col_aff = p_tab.borrow().z_col_aff.clone();
    if z_col_aff.is_empty() {
        z_col_aff = table_affinity_str(None, &p_tab.borrow()).unwrap_or_default();
        p_tab.borrow_mut().z_col_aff = z_col_aff.clone();
    }
    let i = z_col_aff.len() as i32;
    if i != 0 {
        if i_reg != 0 {
            // sqlite3VdbeAddOp4(v, OP_Affinity, iReg, i, 0, zColAff, i)
            let addr = vdbe_add_op3(v, OP_AFFINITY as i32, i_reg, i, 0);
            vdbe_change_p4(v, addr, P4Value::Dynamic(z_col_aff), i);
        } else {
            debug_assert!(
                vdbe_get_last_op(v).opcode == OP_MAKERECORD
                    || vdbe_db(v).borrow().malloc_failed != 0
            );
            vdbe_change_p4(v, -1, P4Value::Dynamic(z_col_aff), i);
        }
    }
}

/// Retorna diferente de zero se a tabela pTab no banco iDb ou qualquer um de seus
/// índices foi aberta em qualquer ponto no programa VDBE. Usado para ver se uma
/// instrução "INSERT INTO <iDb, pTab> SELECT ..." pode rodar sem usar uma tabela
/// temporária para os resultados do SELECT.
fn reads_table(p: &mut Parse, i_db: i32, p_tab: &TableRef) -> i32 {
    let v = get_vdbe(p).expect("sem Vdbe");
    let i_end = vdbe_current_addr(&v.borrow());
    let p_vtab = if is_virtual(&p_tab.borrow()) {
        let db = p.db.upgrade().expect("conexão encerrada");
        get_v_table(&db, p_tab)
    } else {
        None
    };
    let tnum_tab = p_tab.borrow().tnum;

    for i in 1..i_end {
        let (opcode, p2, p3, p4_vtab) = {
            let mut vb = v.borrow_mut();
            let p_op = vdbe_get_op(&mut vb, i);
            let p4_vtab = match &p_op.p4 {
                P4Value::VTab(vt) => Some(vt.clone()),
                _ => None,
            };
            (p_op.opcode, p_op.p2, p_op.p3, p4_vtab)
        };
        if opcode == OP_OPENREAD && p3 == i_db {
            let tnum = p2 as Pgno;
            if tnum == tnum_tab {
                return 1;
            }
            let mut p_index = p_tab.borrow().p_index.clone();
            while let Some(idx) = p_index {
                if tnum == idx.borrow().tnum {
                    return 1;
                }
                p_index = idx.borrow().p_next.clone();
            }
        }
        if opcode == OP_VOPEN {
            if let (Some(a), Some(b)) = (&p4_vtab, &p_vtab) {
                if Rc::ptr_eq(a, b) {
                    return 1;
                }
            }
        }
    }
    0
}

/// Este callback do walker calcula a união das flags colFlags de todas as colunas
/// referenciadas numa restrição CHECK ou numa expressão de coluna gerada.
fn expr_column_flag_union(p_walker: &mut Walker, p_expr: &mut Expr) -> i32 {
    if p_expr.op == TK_COLUMN && p_expr.i_column >= 0 {
        if let WalkerU::Tab(p_tab) = &p_walker.u {
            debug_assert!((p_expr.i_column as usize) < p_tab.borrow().n_col as usize);
            p_walker.e_code |= p_tab.borrow().a_col[p_expr.i_column as usize].col_flags;
        }
    }
    WRC_CONTINUE
}

/// Todas as colunas regulares da tabela pTab foram colocadas em registros a partir
/// de iRegStore. Os registros que correspondem a colunas STORED ou VIRTUAL ainda
/// não foram inicializados. Esta rotina volta atrás e calcula os valores dessas
/// colunas com base nas colunas normais previamente calculadas.
pub fn compute_generated_columns(p_parse: &mut Parse, i_reg_store: i32, p_tab: &TableRef) {
    debug_assert!((p_tab.borrow().tab_flags & TF_HAS_GENERATED) != 0);
    let v = p_parse.p_vdbe.clone().unwrap();

    // Antes de calcular as colunas geradas, garante que a afinidade apropriada foi
    // aplicada às colunas regulares.
    table_affinity(&mut v.borrow_mut(), p_tab, i_reg_store);
    if (p_tab.borrow().tab_flags & TF_HAS_STORED) != 0 {
        let mut vb = v.borrow_mut();
        let p_op = vdbe_get_last_op(&mut vb);
        if p_op.opcode == OP_AFFINITY {
            // Muda o argumento do OP_Affinity para '@' (NONE) em todas as colunas
            // STORED. '@' é a afinidade sem efeito e essas colunas ainda não foram
            // calculadas.
            debug_assert!(p_op.p4type == P4_DYNAMIC);
            if let P4Value::Dynamic(z_p4) = &mut p_op.p4 {
                let tab = p_tab.borrow();
                let mut ii: usize = 0;
                let mut jj: usize = 0;
                while jj < z_p4.len() {
                    if (tab.a_col[ii].col_flags & COLFLAG_VIRTUAL) != 0 {
                        ii += 1;
                        continue;
                    }
                    if (tab.a_col[ii].col_flags & COLFLAG_STORED) != 0 {
                        z_p4[jj] = SQLITE_AFF_NONE;
                    }
                    jj += 1;
                    ii += 1;
                }
            } else {
                debug_assert!(false);
            }
        } else if p_op.opcode == OP_TYPECHECK {
            // Se um OP_TypeCheck foi gerado porque a tabela é STRICT, define o
            // operando P3 para indicar que as colunas geradas não devem ser
            // verificadas.
            p_op.p3 = 1;
        }
    }

    // Como várias colunas geradas podem se referir umas às outras, o algoritmo tem
    // duas passadas. Na primeira, marca todas as colunas geradas como "não
    // disponíveis".
    let n_col = p_tab.borrow().n_col as usize;
    for i in 0..n_col {
        let mut tab = p_tab.borrow_mut();
        if (tab.a_col[i].col_flags & COLFLAG_GENERATED) != 0 {
            tab.a_col[i].col_flags |= COLFLAG_NOTAVAIL;
        }
    }

    let mut w = Walker {
        p_parse: None,
        x_expr_callback: Some(expr_column_flag_union),
        x_select_callback: None,
        x_select_callback2: None,
        walker_depth: 0,
        e_code: 0,
        m_w_flags: 0,
        u: WalkerU::Tab(p_tab.clone()),
    };

    // Na segunda passada, calcula o valor de cada coluna NOT-AVAILABLE. O código
    // companheiro do caso TK_COLUMN de sqlite3ExprCodeTarget() calcula as
    // dependências e remove a marca COLFLAG_NOTAVAIL conforme são necessárias.
    p_parse.i_self_tab = -i_reg_store;
    let mut p_redo: Option<usize>;
    loop {
        let mut e_progress = false;
        p_redo = None;
        for i in 0..n_col {
            let flags = p_tab.borrow().a_col[i].col_flags;
            if (flags & COLFLAG_NOTAVAIL) != 0 {
                p_tab.borrow_mut().a_col[i].col_flags |= COLFLAG_BUSY;
                w.e_code = 0;
                {
                    let tab = p_tab.borrow();
                    let col = tab.a_col[i].clone();
                    walk_expr(&mut w, column_expr(&tab, &col));
                }
                p_tab.borrow_mut().a_col[i].col_flags &= !COLFLAG_BUSY;
                if (w.e_code & COLFLAG_NOTAVAIL) != 0 {
                    p_redo = Some(i);
                    continue;
                }
                e_progress = true;
                let col = p_tab.borrow().a_col[i].clone();
                debug_assert!((col.col_flags & COLFLAG_GENERATED) != 0);
                let x = table_column_to_storage(&p_tab.borrow(), i as i16) as i32 + i_reg_store;
                expr_code_generated_column(p_parse, p_tab, &col, x);
                p_tab.borrow_mut().a_col[i].col_flags &= !COLFLAG_NOTAVAIL;
            }
        }
        if !(p_redo.is_some() && e_progress) {
            break;
        }
    }
    if let Some(i_redo) = p_redo {
        let z_cn_name = p_tab.borrow().a_col[i_redo].z_cn_name.clone();
        error_msg(
            p_parse,
            b"generated column loop on \"%s\"",
            &[PrintfArg::Text(z_cn_name)],
        );
    }
    p_parse.i_self_tab = 0;
}


// ---- part_001.rs ----

// Tradução de insert.c (SQLite 3.46.1), trecho 001: autoincremento e VALUES de
// várias linhas. SQLITE_OMIT_AUTOINCREMENT não está definido no Debian, então o
// ramo #else (macros vazias) some. Notas para o integrador:
//  - A lista AutoincInfo (`Parse.p_ainc`) pertence ao Parse de nível superior; o
//    cleanup `sqlite3DbFree` registrado por sqlite3ParserAddCleanup vira o Drop do Box.
//  - `select_new` precisa devolver um Select cujo `p_src` já tem o slot `a[0]`
//    alocado (como o sqlite3SrcListAppend do C), pois multi_values faz `n_src = 1`.

/// Localiza ou cria uma estrutura AutoincInfo associada à tabela pTab que está no
/// banco iDb. Retorna o número do registro que contém o rowid máximo. Retorna zero
/// se pTab não é uma tabela AUTOINCREMENT (também retorna zero durante um VACUUM,
/// já que não queremos atualizar os contadores AUTOINCREMENT nesse caso).
///
/// Existe no máximo uma estrutura AutoincInfo por tabela, mesmo que a mesma tabela
/// seja autoincrementada várias vezes por inserções dentro de triggers. Uma nova é
/// criada no primeiro uso de pTab; do 2o uso em diante a original é reaproveitada.
///
/// Quatro registros consecutivos são alocados:
///   (1) O nome da tabela pTab.
///   (2) O maior ROWID de pTab.
///   (3) O rowid em sqlite_sequence de pTab.
///   (4) O valor original do maior ROWID em pTab, ou NULL se nenhum.
///
/// O 2o registro é o que é retornado. É tudo que a rotina de inserção precisa saber.
fn auto_inc_begin(p_parse: &mut Parse, i_db: i32, p_tab: &TableRef) -> i32 {
    let mut mem_id: i32 = 0;
    let db = p_parse.db.upgrade().expect("conexão encerrada");
    debug_assert!(db.borrow().a_db[i_db as usize].p_schema.is_some());
    if (p_tab.borrow().tab_flags & TF_AUTOINCREMENT) != 0
        && (db.borrow().m_db_flags & DBFLAG_VACUUM) == 0
    {
        let p_seq_tab = db.borrow().a_db[i_db as usize]
            .p_schema
            .as_ref()
            .unwrap()
            .borrow()
            .p_seq_tab
            .clone();

        // Verifica que a tabela sqlite_sequence existe e é uma tabela rowid
        // ordinária com exatamente duas colunas.
        // Ticket d8dc2b3a58cd5dc2918a1d4acb 2018-05-23
        let invalid = match &p_seq_tab {
            None => true,
            Some(t) => {
                let t = t.borrow();
                !has_rowid(&t) || is_virtual(&t) || t.n_col != 2
            }
        };
        if invalid {
            p_parse.n_err += 1;
            p_parse.rc = SQLITE_CORRUPT_SEQUENCE;
            return 0;
        }

        // Procura (ou cria) o AutoincInfo na lista do Parse de nível superior.
        let malloc_failed = db.borrow().malloc_failed != 0;
        let mut find_or_add = |p_toplevel: &mut Parse| -> i32 {
            let mut found: Option<i32> = None;
            {
                let mut p_info = p_toplevel.p_ainc.as_deref();
                while let Some(info) = p_info {
                    if Rc::ptr_eq(&info.p_tab, p_tab) {
                        found = Some(info.reg_ctr);
                        break;
                    }
                    p_info = info.p_next.as_deref();
                }
            }
            if let Some(reg_ctr) = found {
                return reg_ctr;
            }
            if malloc_failed {
                return 0;
            }
            p_toplevel.n_mem += 1; // Registro que guarda o nome da tabela
            p_toplevel.n_mem += 1;
            let reg_ctr = p_toplevel.n_mem; // Registro do rowid máximo
            p_toplevel.n_mem += 2; // Rowid em sqlite_sequence + valor máximo original
            let p_info = Box::new(AutoincInfo {
                p_next: p_toplevel.p_ainc.take(),
                p_tab: p_tab.clone(),
                i_db,
                reg_ctr,
            });
            p_toplevel.p_ainc = Some(p_info);
            reg_ctr
        };
        let p_top = p_parse.p_toplevel.as_ref().and_then(|w| w.upgrade());
        mem_id = match p_top {
            Some(top) => find_or_add(&mut top.borrow_mut()),
            None => find_or_add(p_parse),
        };
    }
    mem_id
}

/// Esta rotina gera código que inicializará todos os registros usados pelo
/// rastreador de autoincremento.
pub fn autoincrement_begin(p_parse: &mut Parse) {
    let db = p_parse.db.upgrade().expect("conexão encerrada");
    let v = p_parse.p_vdbe.clone();

    // Esta rotina nunca é chamada durante a geração de trigger. Só é chamada do
    // nível superior.
    debug_assert!(p_parse.p_trigger_tab.is_none());
    debug_assert!(is_toplevel(p_parse));

    debug_assert!(v.is_some()); // Já teríamos falhado muito antes se não fosse assim
    let v = v.unwrap();

    // Junta os dados de cada AutoincInfo antes, pois open_table precisa de
    // `&mut Parse`, que também é dono da lista.
    let mut a_info: Vec<(i32, i32, Vec<u8>)> = Vec::new();
    {
        let mut p = p_parse.p_ainc.as_deref();
        while let Some(info) = p {
            a_info.push((info.i_db, info.reg_ctr, info.p_tab.borrow().z_name.clone()));
            p = info.p_next.as_deref();
        }
    }

    for (p_i_db, mem_id, z_name) in a_info {
        let i_ln: i32 = vdbe_offset_lineno(2);
        const AUTO_INC: [VdbeOpList; 12] = [
            /* 0  */ VdbeOpList { opcode: OP_NULL, p1: 0, p2: 0, p3: 0 },
            /* 1  */ VdbeOpList { opcode: OP_REWIND, p1: 0, p2: 10, p3: 0 },
            /* 2  */ VdbeOpList { opcode: OP_COLUMN, p1: 0, p2: 0, p3: 0 },
            /* 3  */ VdbeOpList { opcode: OP_NE, p1: 0, p2: 9, p3: 0 },
            /* 4  */ VdbeOpList { opcode: OP_ROWID, p1: 0, p2: 0, p3: 0 },
            /* 5  */ VdbeOpList { opcode: OP_COLUMN, p1: 0, p2: 1, p3: 0 },
            /* 6  */ VdbeOpList { opcode: OP_ADDIMM, p1: 0, p2: 0, p3: 0 },
            /* 7  */ VdbeOpList { opcode: OP_COPY, p1: 0, p2: 0, p3: 0 },
            /* 8  */ VdbeOpList { opcode: OP_GOTO, p1: 0, p2: 11, p3: 0 },
            /* 9  */ VdbeOpList { opcode: OP_NEXT, p1: 0, p2: 2, p3: 0 },
            /* 10 */ VdbeOpList { opcode: OP_INTEGER, p1: 0, p2: 0, p3: 0 },
            /* 11 */ VdbeOpList { opcode: OP_CLOSE, p1: 0, p2: 0, p3: 0 },
        ];
        let p_seq_tab = db.borrow().a_db[p_i_db as usize]
            .p_schema
            .as_ref()
            .unwrap()
            .borrow()
            .p_seq_tab
            .clone()
            .unwrap();
        open_table(p_parse, 0, p_i_db, &p_seq_tab, OP_OPENREAD as i32);
        vdbe_load_string(&mut v.borrow_mut(), mem_id - 1, &z_name);
        let a_op = vdbe_add_op_list(&mut v.borrow_mut(), AUTO_INC.len() as i32, &AUTO_INC, i_ln);
        let base = match a_op {
            Some(b) => b,
            None => break,
        };
        {
            let mut vb = v.borrow_mut();
            vb.a_op[base].p2 = mem_id;
            vb.a_op[base].p3 = mem_id + 2;
            vb.a_op[base + 2].p3 = mem_id;
            vb.a_op[base + 3].p1 = mem_id - 1;
            vb.a_op[base + 3].p3 = mem_id;
            vb.a_op[base + 3].p5 = SQLITE_JUMPIFNULL as u16;
            vb.a_op[base + 4].p2 = mem_id + 1;
            vb.a_op[base + 5].p3 = mem_id;
            vb.a_op[base + 6].p1 = mem_id;
            vb.a_op[base + 7].p2 = mem_id + 2;
            vb.a_op[base + 7].p1 = mem_id;
            vb.a_op[base + 10].p2 = mem_id;
        }
        if p_parse.n_tab == 0 {
            p_parse.n_tab = 1;
        }
    }
}

/// Atualiza o rowid máximo para um cálculo de autoincremento.
///
/// Esta rotina deve ser chamada quando o registro regRowid contém um rowid novo que
/// está prestes a ser inserido. Se esse rowid é maior que o máximo na célula de
/// memória memId, a célula de memória é atualizada.
fn auto_inc_step(p_parse: &mut Parse, mem_id: i32, reg_rowid: i32) {
    if mem_id > 0 {
        let v = p_parse.p_vdbe.clone().unwrap();
        vdbe_add_op2(&mut v.borrow_mut(), OP_MEMMAX as i32, mem_id, reg_rowid);
    }
}

/// Esta rotina gera o código necessário para escrever os valores máximos de rowid de
/// autoincremento de volta na tabela sqlite_sequence. Toda instrução que possa fazer
/// um INSERT em uma tabela de autoincremento (diretamente ou por triggers) precisa
/// chamar esta rotina logo antes do código de "saída".
fn auto_increment_end(p_parse: &mut Parse) {
    let v = p_parse.p_vdbe.clone();
    let db = p_parse.db.upgrade().expect("conexão encerrada");

    debug_assert!(v.is_some());
    let v = v.unwrap();

    let mut a_info: Vec<(i32, i32)> = Vec::new();
    {
        let mut p = p_parse.p_ainc.as_deref();
        while let Some(info) = p {
            a_info.push((info.i_db, info.reg_ctr));
            p = info.p_next.as_deref();
        }
    }

    for (p_i_db, mem_id) in a_info {
        let i_ln: i32 = vdbe_offset_lineno(2);
        const AUTO_INC_END: [VdbeOpList; 5] = [
            /* 0 */ VdbeOpList { opcode: OP_NOTNULL, p1: 0, p2: 2, p3: 0 },
            /* 1 */ VdbeOpList { opcode: OP_NEWROWID, p1: 0, p2: 0, p3: 0 },
            /* 2 */ VdbeOpList { opcode: OP_MAKERECORD, p1: 0, p2: 2, p3: 0 },
            /* 3 */ VdbeOpList { opcode: OP_INSERT, p1: 0, p2: 0, p3: 0 },
            /* 4 */ VdbeOpList { opcode: OP_CLOSE, p1: 0, p2: 0, p3: 0 },
        ];
        let p_seq_tab = db.borrow().a_db[p_i_db as usize]
            .p_schema
            .as_ref()
            .unwrap()
            .borrow()
            .p_seq_tab
            .clone()
            .unwrap();

        let i_rec = get_temp_reg(p_parse);
        let addr_here = vdbe_current_addr(&v.borrow());
        vdbe_add_op3(&mut v.borrow_mut(), OP_LE as i32, mem_id + 2, addr_here + 7, mem_id);
        open_table(p_parse, 0, p_i_db, &p_seq_tab, OP_OPENWRITE as i32);
        let a_op = vdbe_add_op_list(
            &mut v.borrow_mut(),
            AUTO_INC_END.len() as i32,
            &AUTO_INC_END,
            i_ln,
        );
        let base = match a_op {
            Some(b) => b,
            None => break,
        };
        {
            let mut vb = v.borrow_mut();
            vb.a_op[base].p1 = mem_id + 1;
            vb.a_op[base + 1].p2 = mem_id + 1;
            vb.a_op[base + 2].p1 = mem_id - 1;
            vb.a_op[base + 2].p3 = i_rec;
            vb.a_op[base + 3].p2 = i_rec;
            vb.a_op[base + 3].p3 = mem_id + 1;
            vb.a_op[base + 3].p5 = OPFLAG_APPEND as u16;
        }
        release_temp_reg(p_parse, i_rec);
    }
}

/// Gera o código de fim do autoincremento se há alguma tabela AUTOINCREMENT.
pub fn autoincrement_end(p_parse: &mut Parse) {
    if p_parse.p_ainc.is_some() {
        auto_increment_end(p_parse);
    }
}

/// Se o argumento pVal é um objeto Select devolvido por sqlite3MultiValues() que
/// conseguiu usar a otimização de co-rotina, termina a codificação da co-rotina.
pub fn multi_values_end(p_parse: &mut Parse, p_val: &Select) {
    if p_val.p_src.as_ref().map_or(false, |s| s.n_src > 0) {
        let (reg_return, addr_fill_sub) = {
            let p_item = &p_val.p_src.as_ref().unwrap().a[0];
            (p_item.reg_return, p_item.addr_fill_sub)
        };
        let v = p_parse.p_vdbe.clone().unwrap();
        vdbe_end_coroutine(&mut v.borrow_mut(), reg_return);
        vdbe_jump_here(&mut v.borrow_mut(), addr_fill_sub - 1);
    }
}

/// Retorna verdadeiro se todas as expressões da lista passada como único argumento
/// são constantes.
fn expr_list_is_constant(p_parse: &mut Parse, p_row: &ExprList) -> i32 {
    for ii in 0..p_row.n_expr as usize {
        if 0 == expr_is_constant(p_parse, p_row.a[ii].p_expr.as_deref().unwrap()) {
            return 0;
        }
    }
    1
}

/// Retorna verdadeiro se todas as expressões da lista passada como único argumento
/// são constantes e não têm afinidade.
fn expr_list_is_no_affinity(p_parse: &mut Parse, p_row: &ExprList) -> i32 {
    if expr_list_is_constant(p_parse, p_row) == 0 {
        return 0;
    }
    for ii in 0..p_row.n_expr as usize {
        let p_expr = p_row.a[ii].p_expr.as_deref().unwrap();
        debug_assert!(p_expr.op != TK_RAISE);
        debug_assert!(p_expr.aff_expr == 0);
        if 0 != expr_affinity(p_expr) {
            return 0;
        }
    }
    1
}

/// Esta função é chamada pelo parser para a segunda linha e as seguintes de uma
/// cláusula VALUES de várias linhas. O argumento pLeft é a parte da cláusula VALUES
/// já analisada, e pRow é o vetor de valores da nova linha. O objeto Select devolvido
/// representa a cláusula VALUES completa, incluindo a nova linha.
///
/// Há duas maneiras de fazer isso: codificação incremental de uma co-rotina (o método
/// "co-rotina") ou devolver um Select equivalente a "pLeft UNION ALL SELECT pRow" (o
/// método "UNION ALL"). Com muitas linhas o Select composto pode consumir muita
/// memória.
///
/// No método co-rotina, cada linha devolvida pela cláusula VALUES é codificada numa
/// parte da co-rotina assim que passa por esta função. O Select devolvido é
/// equivalente a "SELECT * FROM (Select que lê a co-rotina)".
///
/// O método co-rotina é usado na maioria dos casos. As exceções são:
///
///    a) A instrução atual tem uma cláusula WITH. Isso evita instruções como
///       "WITH cte AS ( VALUES('x'), ('y') ... ) SELECT * FROM cte AS a, cte AS b;",
///       que não funcionariam: a co-rotina usa um registro fixo para os OP_Yield, e
///       dois cursores não podem percorrê-la ao mesmo tempo.
///
///    b) O schema está sendo analisado (a cláusula VALUES faz parte de um item de
///       schema como VIEW ou TRIGGER). Nesse caso não há VM sendo gerada.
///
///    c) Há expressões não constantes na cláusula VALUES (por exemplo, numa
///       subconsulta correlacionada).
///
///    d) Um ou mais valores da primeira linha têm afinidade (são expressões CAST).
///       Isso causa problemas porque as regras complexas de
///       sqlite3SubqueryColumnTypes() (select.c) para a afinidade efetiva da coluna
///       em todas as linhas exigem acesso a todos os valores da coluna ao mesmo
///       tempo.
pub fn multi_values(
    p_parse: &mut Parse,
    mut p_left: Box<Select>,
    p_row: Box<ExprList>,
) -> Box<Select> {
    let db = p_parse.db.upgrade().expect("conexão encerrada");
    let init_busy = db.borrow().init.busy != 0;
    let left_n_src = p_left.p_src.as_ref().unwrap().n_src;
    if p_parse.b_has_with != 0                          /* condição (a) acima */
        || init_busy                                    /* condição (b) acima */
        || expr_list_is_constant(p_parse, &p_row) == 0  /* condição (c) acima */
        || (left_n_src == 0
            && expr_list_is_no_affinity(p_parse, p_left.p_elist.as_deref().unwrap()) == 0)
                                                        /* condição (d) acima */
        || in_special_parse(p_parse)
    {
        // O método co-rotina não pode ser usado. Volta para UNION ALL.
        let mut f: u32 = SF_VALUES | SF_MULTIVALUE;
        if left_n_src != 0 {
            multi_values_end(p_parse, &p_left);
            f = SF_VALUES;
        } else if p_left.p_prior.is_some() {
            // Neste caso define SF_MultiValue só se estava definida em pLeft
            f &= p_left.sel_flags;
        }
        let p_select = select_new(p_parse, Some(p_row), None, None, None, None, None, f, None);
        p_left.sel_flags &= !SF_MULTIVALUE;
        if let Some(mut p_select) = p_select {
            p_select.op = TK_ALL;
            p_select.p_prior = Some(p_left);
            p_left = p_select;
        }
    } else {
        // `p_ok` diz se `p` (o SrcItem que lê da co-rotina, sempre a[0] de pLeft ao
        // fim do bloco) foi definido; fica falso só se a alocação de pRet falhou.
        let mut p_ok = true;

        if left_n_src == 0 {
            // A co-rotina ainda não foi iniciada e o Select especial que acessa a
            // co-rotina ainda não foi criado. Este bloco faz as duas coisas.
            let v = get_vdbe(p_parse).expect("sem Vdbe");
            let p_ret = select_new(p_parse, None, None, None, None, None, None, 0, None);

            // Garante que o schema do banco foi lido, para termos a codificação de
            // texto correta.
            if (db.borrow().m_db_flags & DBFLAG_SCHEMA_KNOWN_OK) == 0 {
                read_schema(p_parse);
            }

            match p_ret {
                Some(mut p_ret) => {
                    p_ret.p_src.as_mut().unwrap().n_src = 1;
                    p_ret.p_prior = p_left.p_prior.take();
                    p_ret.op = p_left.op;
                    if p_ret.p_prior.is_some() {
                        p_ret.sel_flags |= SF_VALUES;
                    }
                    p_left.op = TK_SELECT;
                    debug_assert!(p_left.p_next.is_none());
                    debug_assert!(p_ret.p_next.is_none());

                    let addr_fill_sub = vdbe_current_addr(&v.borrow()) + 1;
                    p_parse.n_mem += 1;
                    let reg_return = p_parse.n_mem;
                    vdbe_add_op3(
                        &mut v.borrow_mut(),
                        OP_INITCOROUTINE as i32,
                        reg_return,
                        0,
                        addr_fill_sub,
                    );
                    let mut dest = SelectDest {
                        e_dest: 0,
                        i_sdparm: 0,
                        i_sdparm2: 0,
                        i_sdst: 0,
                        n_sdst: 0,
                        z_aff_sdst: Vec::new(),
                        p_order_by: None,
                    };
                    select_dest_init(&mut dest, SRT_COROUTINE, reg_return);

                    // Aloca registros para a saída da co-rotina, de modo que haja
                    // dois registros não usados imediatamente antes dos usados pela
                    // co-rotina. Assim o código de sqlite3Insert() usa esses
                    // registros diretamente, sem copiar a saída da co-rotina para um
                    // vetor separado.
                    dest.i_sdst = p_parse.n_mem + 3;
                    dest.n_sdst = p_left.p_elist.as_ref().unwrap().n_expr;
                    p_parse.n_mem += 2 + dest.n_sdst;

                    p_left.sel_flags |= SF_MULTIVALUE;
                    select(p_parse, &mut p_left, &mut dest);
                    debug_assert!(p_parse.n_err != 0 || dest.i_sdst > 0);

                    // O item guarda o Select p_left (dono único) e os registros.
                    {
                        let p_item = &mut p_ret.p_src.as_mut().unwrap().a[0];
                        p_item.p_select = Some(p_left);
                        p_item.fg.via_coroutine = 1;
                        p_item.addr_fill_sub = addr_fill_sub;
                        p_item.reg_return = reg_return;
                        p_item.i_cursor = -1;
                        p_item.u1 = SrcItemU1::NRow(2);
                        p_item.reg_result = dest.i_sdst;
                    }
                    p_left = p_ret;
                }
                None => {
                    p_ok = false;
                }
            }
        } else {
            let p_item = &mut p_left.p_src.as_mut().unwrap().a[0];
            debug_assert!(p_item.fg.is_tab_func == 0 && p_item.fg.is_indexed_by == 0);
            if let SrcItemU1::NRow(n_row) = &mut p_item.u1 {
                *n_row += 1;
            }
        }

        if p_parse.n_err == 0 {
            debug_assert!(p_ok);
            let (reg_result, reg_return, n_expr_sel) = {
                let p_item = &p_left.p_src.as_ref().unwrap().a[0];
                (
                    p_item.reg_result,
                    p_item.reg_return,
                    p_item.p_select.as_ref().unwrap().p_elist.as_ref().unwrap().n_expr,
                )
            };
            if n_expr_sel != p_row.n_expr {
                let p_item = &p_left.p_src.as_ref().unwrap().a[0];
                select_wrong_num_terms_error(p_parse, p_item.p_select.as_ref().unwrap());
            } else {
                expr_code_expr_list(p_parse, &p_row, reg_result, 0, 0);
                let v = p_parse.p_vdbe.clone().unwrap();
                vdbe_add_op1(&mut v.borrow_mut(), OP_YIELD as i32, reg_return);
            }
        }
        let _ = p_ok;
        drop(p_row); // sqlite3ExprListDelete(pParse->db, pRow)
    }

    p_left
}


// ---- part_002.rs ----

// Tradução de insert.c (SQLite 3.46.1), trecho 002.
//
// O trecho C 002 contém a declaração antecipada de xferOptimization() (em Rust não
// existe declaração antecipada: a função fica no módulo, trecho 007) e o INÍCIO de
// sqlite3Insert(), que o corte em fronteira de linhas deixou pela metade (o corte caiu
// dentro da função, depois de "regData = regFromSelect; regRowid = regData - 1;").
// Como uma `fn` não pode atravessar arquivos, a tradução completa de sqlite3Insert()
// (trechos 002 e 003 do C) fica em part_003.rs, com o comentário de cabeçalho abaixo.
//
// Esta rotina é chamada para tratar SQL das seguintes formas:
//
//    insert into TABLE (IDLIST) values(EXPRLIST),(EXPRLIST),...
//    insert into TABLE (IDLIST) select
//    insert into TABLE (IDLIST) default values
//
// A IDLIST após o nome da tabela é sempre opcional. Se omitida, é substituída pela
// lista de todas as colunas (não ocultas) da tabela. A IDLIST aparece no parâmetro
// pColumn, que é NULL se a IDLIST foi omitida.
//
// O parâmetro pSelect contém os valores a inserir nas duas primeiras formas. Uma
// cláusula VALUES é só uma abreviação de um SELECT sem FROM e sem o que vem depois.
// Se pSelect é NULL, a forma pretendida é DEFAULT VALUES.
//
// O código gerado segue um de quatro modelos. Para um INSERT simples com dados de uma
// cláusula VALUES de uma linha, o código executa uma vez, de cima a baixo (1o modelo):
//
//         abre cursor de escrita em <table> e seus índices
//         põe as expressões da cláusula VALUES em registros
//         escreve o registro resultante em <table>
//         limpeza
//
// Os três modelos restantes supõem a forma "INSERT INTO <table> SELECT ...".
//
// Se o SELECT é da forma restrita "SELECT * FROM <table2>" (puxa todas as colunas de
// uma única tabela, sem WHERE, LIMIT, GROUP BY nem ORDER BY) e <table2> e <table1> são
// tabelas distintas com esquemas idênticos, inclusive os mesmos índices, uma otimização
// especial copia os registros brutos de <table2> para <table1>. Veja xferOptimization().
// Este é o 2o modelo.
//
//         abre cursor de escrita em <table>
//         abre cursor de leitura em <table2>
//         transfere todos os registros de <table2> para <table>
//         fecha cursores
//         para cada índice de <table>
//           abre cursor de escrita no índice de <table>
//           abre cursor de leitura no índice correspondente de <table2>
//           transfere todos os registros do cursor de leitura para o de escrita
//           fecha cursores
//         fim para cada
//
// O 3o modelo vale quando o 2o não se aplica e o SELECT nunca lê de <table>:
//
//         X <- A
//         goto B
//      A: prepara o SELECT
//         laço sobre as linhas do SELECT
//           carrega valores nos registros R..R+n
//           yield X
//         fim do laço
//         limpeza do SELECT
//         fim da co-rotina X
//      B: abre cursor de escrita em <table> e seus índices
//      C: yield X, no EOF goto D
//         insere o resultado do select em <table> a partir de R..R+n
//         goto C
//      D: limpeza
//
// O 4o modelo vale se o INSERT toma os valores de um SELECT mas a tabela de destino
// também é lida pelo SELECT. Então se usa uma tabela intermediária:
//
//         X <- A
//         goto B
//      A: prepara o SELECT
//         laço sobre as tabelas do SELECT
//           carrega valor nos registros R..R+n
//           yield X
//         fim do laço
//         limpeza do SELECT
//         fim da co-rotina R
//      B: abre tabela temporária
//      L: yield X, no EOF goto M
//         insere a linha de R..R+n na tabela temporária
//         goto L
//      M: abre cursor de escrita em <table> e seus índices
//         rebobina a tabela temporária
//      C: laço sobre as linhas da tabela intermediária
//           transfere os valores da tabela intermediária para <table>
//         fim do laço
//      D: limpeza


// ---- part_003.rs ----

// Tradução de insert.c (SQLite 3.46.1), trechos 002 (parte final) e 003: sqlite3Insert().
// Notas para o integrador:
//  - Os modelos de código gerado estão descritos no comentário de part_002.rs.
//  - Os parâmetros SrcList, Select, IdList e Upsert chegam por valor (`Option<Box<..>>`):
//    o `insert_cleanup` do C (sqlite3SrcListDelete & cia.) vira o Drop no fim da função.
//    `goto insert_cleanup` é `break 'insert_cleanup`; `goto insert_end` é `break 'insert_end`.
//  - sNC.pParse = pParse: um NameContext não guarda o Parse (seria um ponteiro de volta
//    para um `&mut`), então o Parse viaja como argumento de resolve_expr_list_names().
//  - Em C todos os Upsert da lista apontam para o MESMO SrcList (pUpsertSrc). Aqui cada
//    cláusula recebe uma cópia (src_list_dup) feita depois de definir a[0].iCursor, que
//    é o que os consumidores já fazem com sqlite3SrcListDup. A análise de alvo recebe como
//    `p_all` a fatia das cláusulas ANTERIORES (é só o que sqlite3UpsertAnalyzeTarget lê).
//  - SQLITE_ALLOW_ROWID_IN_VIEW e os blocos SQLITE_DEBUG/TREETRACE/VdbeCoverage não existem
//    no Debian e foram omitidos.

/// Texto do argumento `%S` do printf do SQLite para um SrcItem (sem a flag "!"): o alias
/// se houver; senão `[banco.]nome`; senão "(subquery-N)".
fn src_item_text(p_item: &SrcItem) -> Vec<u8> {
    if !p_item.z_alias.is_empty() {
        return p_item.z_alias.clone();
    }
    if !p_item.z_name.is_empty() {
        let mut z = Vec::new();
        if !p_item.z_database.is_empty() {
            z.extend_from_slice(&p_item.z_database);
            z.push(b'.');
        }
        z.extend_from_slice(&p_item.z_name);
        return z;
    }
    let sel_id = p_item.p_select.as_ref().map_or(0, |s| s.sel_id);
    format!("(subquery-{})", sel_id).into_bytes()
}

/// Gera o código que carrega o valor DEFAULT da coluna `i` de pTab no registro `i_reg`
/// (o trecho `sqlite3ExprCodeFactorable(pParse, sqlite3ColumnExpr(pTab, &pTab->aCol[i]), iRegStore)`
/// que se repete quatro vezes no laço de colunas de sqlite3Insert).
fn code_column_default(p_parse: &mut Parse, p_tab: &TableRef, i: usize, i_reg: i32) {
    let tab = p_tab.borrow();
    let col = tab.a_col[i].clone();
    expr_code_factorable(p_parse, column_expr(&tab, &col), i_reg);
}

pub fn insert(
    p_parse: &mut Parse,
    mut p_tab_list: Option<Box<SrcList>>,
    mut p_select: Option<Box<Select>>,
    mut p_column: Option<Box<IdList>>,
    on_error: i32,
    mut p_upsert: Option<Box<Upsert>>,
) {
    let mut i_data_cur: i32 = 0; // Cursor VDBE que é o repositório principal de dados
    let mut i_idx_cur: i32 = 0; // Primeiro cursor de índice
    let mut ipk_column: i32 = -1; // Coluna que é o INTEGER PRIMARY KEY
    let mut src_tab: i32 = 0; // Dados vêm deste cursor temporário se >=0
    let mut addr_ins_top: i32 = 0; // Salta para o rótulo "D"
    let mut addr_cont: i32 = 0; // Topo do laço. Rótulo "C" nos modelos 3 e 4
    let mut use_temp_table = false; // Guarda o SELECT numa tabela intermediária
    let mut append_flag: u8 = 0; // Verdadeiro se a inserção provavelmente é um append
    let mut p_list: Option<Box<ExprList>> = None; // Lista de VALUES() a inserir
    let mut n_hidden: i32 = 0; // Colunas ocultas se a tabela é virtual

    // Alocações de registros
    let mut reg_from_select: i32 = 0; // Registro base dos dados vindos do SELECT
    let mut reg_row_count: i32 = 0; // Célula de memória do contador de linhas
    let mut a_reg_idx: Vec<i32> = Vec::new(); // Um registro por índice

    let mut dest = SelectDest {
        e_dest: 0,
        i_sdparm: 0,
        i_sdparm2: 0,
        i_sdst: 0,
        n_sdst: 0,
        z_aff_sdst: Vec::new(),
        p_order_by: None,
    };

    let db = p_parse.db.upgrade().expect("conexão encerrada");

    'insert_cleanup: {
        if p_parse.n_err != 0 {
            break 'insert_cleanup;
        }
        debug_assert!(db.borrow().malloc_failed == 0);
        dest.i_sdparm = 0; // Suprime um aviso inofensivo do compilador

        // Se o Select é só uma lista VALUES() de uma linha (o caso comum), guarda essa
        // linha de valores e descarta as outras partes (não usadas) do Select.
        let is_single_values = p_select
            .as_ref()
            .map_or(false, |s| (s.sel_flags & SF_VALUES) != 0 && s.p_prior.is_none());
        if is_single_values {
            let mut sel = p_select.take().unwrap();
            p_list = sel.p_elist.take();
            drop(sel); // sqlite3SelectDelete(db, pSelect)
        }

        // Localiza a tabela na qual vamos inserir as novas informações.
        debug_assert!(p_tab_list.as_ref().unwrap().n_src == 1);
        let p_tab: TableRef = match src_list_lookup(p_parse, p_tab_list.as_mut().unwrap()) {
            Some(t) => t,
            None => break 'insert_cleanup,
        };
        let i_db = schema_to_index(&db, p_tab.borrow().p_schema.clone());
        debug_assert!(i_db < db.borrow().n_db);
        {
            let z_tab_name = p_tab.borrow().z_name.clone();
            let z_db_s_name = db.borrow().a_db[i_db as usize].z_db_sname.clone();
            if auth_check(
                p_parse,
                SQLITE_INSERT,
                Some(&z_tab_name),
                None,
                z_db_s_name.as_deref(),
            ) != 0
            {
                break 'insert_cleanup;
            }
        }
        let without_rowid = !has_rowid(&p_tab.borrow());
        let n_col = p_tab.borrow().n_col as i32;

        // Descobre se há triggers e se a tabela de destino é uma view.
        let mut tmask: u32 = 0; // Máscara dos tempos de trigger
        let p_trigger: Option<TriggerRef> =
            triggers_exist(p_parse, &p_tab, TK_INSERT, None, Some(&mut tmask));
        let is_view_tab = is_view(&p_tab.borrow());
        debug_assert!((p_trigger.is_some() && tmask != 0) || (p_trigger.is_none() && tmask == 0));

        // Se pTab é de fato uma view, garante que foi inicializada.
        // ViewGetColumnNames() não faz nada se pTab não é uma view.
        if view_get_column_names(p_parse, &p_tab) != 0 {
            break 'insert_cleanup;
        }

        // Não se pode inserir numa tabela somente leitura.
        if is_read_only(p_parse, &p_tab, p_trigger.as_ref()) != 0 {
            break 'insert_cleanup;
        }

        // Aloca uma VDBE
        let v: VdbeRef = match get_vdbe(p_parse) {
            Some(v) => v,
            None => break 'insert_cleanup,
        };
        if p_parse.nested == 0 {
            vdbe_count_changes(&mut v.borrow_mut());
        }
        begin_write_operation(
            p_parse,
            (p_select.is_some() || p_trigger.is_some()) as i32,
            i_db,
        );

        'insert_end: {
            // Se a instrução tem a forma
            //
            //       INSERT INTO <table1> SELECT * FROM <table2>;
            //
            // aplicam-se otimizações especiais que tornam a transferência muito rápida e
            // reduzem a fragmentação dos índices. Este é o 2o modelo.
            if p_column.is_none()
                && p_select.is_some()
                && p_trigger.is_none()
                && xfer_optimization(p_parse, &p_tab, p_select.as_mut().unwrap(), on_error, i_db) != 0
            {
                debug_assert!(p_trigger.is_none());
                debug_assert!(p_list.is_none());
                break 'insert_end;
            }

            // Se é uma tabela AUTOINCREMENT, procura o número de sequência na tabela
            // sqlite_sequence e o guarda na célula de memória regAutoinc.
            let reg_autoinc = auto_inc_begin(p_parse, i_db, &p_tab);

            // Aloca um bloco de registros para o rowid e os valores de todas as colunas
            // da nova linha.
            let mut reg_ins = p_parse.n_mem + 1;
            let mut reg_rowid = reg_ins;
            p_parse.n_mem += n_col + 1;
            if is_virtual(&p_tab.borrow()) {
                reg_rowid += 1;
                p_parse.n_mem += 1;
            }
            let mut reg_data = reg_rowid + 1;

            // Se o INSERT incluiu uma IDLIST, garante que todos os seus elementos são
            // colunas da tabela e lembra os índices das colunas.
            //
            // Se a tabela tem uma coluna INTEGER PRIMARY KEY e ela é nomeada na IDLIST,
            // registra em ipkColumn o índice dessa coluna NA IDLIST (não na tabela
            // original, onde o índice é pTab->iPKey). Depois do laço, ipkColumn==(-1)
            // significa que a chave primária inteira não foi especificada, e a tabela é
            // WITHOUT ROWID ou vai gerar uma chave inteira automaticamente.
            //
            // bIdListInOrder é verdadeiro se as colunas da IDLIST estão na ordem de
            // armazenamento. Isso habilita uma otimização que evita embaralhar as colunas
            // para a ordem de armazenamento. Falsos negativos são inofensivos, mas falsos
            // positivos corrompem o banco.
            let mut b_id_list_in_order =
                (p_tab.borrow().tab_flags & (TF_OOO_HIDDEN | TF_HAS_STORED)) == 0;
            if let Some(col) = p_column.as_mut() {
                debug_assert!(col.e_u4 != EU4_EXPR);
                col.e_u4 = EU4_IDX;
                for i in 0..col.n_id as usize {
                    col.a[i].u4_idx = -1;
                }
                for i in 0..col.n_id as usize {
                    let mut j: i32 = 0;
                    while j < n_col {
                        let (z_cn_name, col_flags, i_p_key) = {
                            let tab = p_tab.borrow();
                            (
                                tab.a_col[j as usize].z_cn_name.clone(),
                                tab.a_col[j as usize].col_flags,
                                tab.i_p_key,
                            )
                        };
                        if str_i_cmp(&col.a[i].z_name, &z_cn_name) == 0 {
                            col.a[i].u4_idx = j;
                            if i as i32 != j {
                                b_id_list_in_order = false;
                            }
                            if j == i_p_key as i32 {
                                ipk_column = i as i32;
                                debug_assert!(!without_rowid);
                            }
                            if (col_flags & (COLFLAG_STORED | COLFLAG_VIRTUAL)) != 0 {
                                error_msg(
                                    p_parse,
                                    b"cannot INSERT into generated column \"%s\"",
                                    &[PrintfArg::Text(z_cn_name)],
                                );
                                break 'insert_cleanup;
                            }
                            break;
                        }
                        j += 1;
                    }
                    if j >= n_col {
                        if is_rowid(&col.a[i].z_name) != 0 && !without_rowid {
                            ipk_column = i as i32;
                            b_id_list_in_order = false;
                        } else {
                            error_msg(
                                p_parse,
                                b"table %S has no column named %s",
                                &[
                                    PrintfArg::Text(src_item_text(&p_tab_list.as_ref().unwrap().a[0])),
                                    PrintfArg::Text(col.a[i].z_name.clone()),
                                ],
                            );
                            p_parse.check_schema = 1;
                            break 'insert_cleanup;
                        }
                    }
                }
            }

            // Descobre quantas colunas de dados foram fornecidas. Se os dados vêm de um
            // SELECT, gera uma co-rotina que produz uma linha do SELECT a cada invocação.
            // A co-rotina é o cabeçalho comum dos modelos 3 e 4.
            let n_column: i32;
            if let Some(sel) = p_select.as_mut() {
                // Os dados vêm de um SELECT ou de um VALUES de várias linhas. Gera uma
                // co-rotina para rodar o SELECT.
                let via_coroutine = {
                    let src = sel.p_src.as_ref().unwrap();
                    src.n_src == 1 && src.a[0].fg.via_coroutine != 0 && sel.p_prior.is_none()
                };
                if via_coroutine {
                    let (reg_return, reg_result, n_expr, z_item) = {
                        let p_item = &sel.p_src.as_ref().unwrap().a[0];
                        (
                            p_item.reg_return,
                            p_item.reg_result,
                            p_item.p_select.as_ref().unwrap().p_elist.as_ref().unwrap().n_expr,
                            src_item_text(p_item),
                        )
                    };
                    dest.i_sdparm = reg_return;
                    reg_from_select = reg_result;
                    n_column = n_expr;
                    let mut z_msg = b"SCAN ".to_vec();
                    z_msg.extend_from_slice(&z_item);
                    vdbe_explain(p_parse, 0, z_msg); // ExplainQueryPlan((pParse, 0, "SCAN %S", pItem))
                    if b_id_list_in_order && n_column == n_col {
                        reg_data = reg_from_select;
                        reg_rowid = reg_data - 1;
                        reg_ins = reg_rowid - if is_virtual(&p_tab.borrow()) { 1 } else { 0 };
                    }
                } else {
                    p_parse.n_mem += 1;
                    let reg_yield = p_parse.n_mem;
                    let addr_top = vdbe_current_addr(&v.borrow()) + 1; // Topo da co-rotina
                    vdbe_add_op3(&mut v.borrow_mut(), OP_INITCOROUTINE as i32, reg_yield, 0, addr_top);
                    select_dest_init(&mut dest, SRT_COROUTINE, reg_yield);
                    dest.i_sdst = if b_id_list_in_order { reg_data } else { 0 };
                    dest.n_sdst = n_col;
                    let rc = select(p_parse, sel, &mut dest);
                    reg_from_select = dest.i_sdst;
                    if rc != 0 || p_parse.n_err != 0 {
                        break 'insert_cleanup;
                    }
                    debug_assert!(db.borrow().malloc_failed == 0);
                    vdbe_end_coroutine(&mut v.borrow_mut(), reg_yield);
                    vdbe_jump_here(&mut v.borrow_mut(), addr_top - 1); // rótulo B:
                    debug_assert!(sel.p_elist.is_some());
                    n_column = sel.p_elist.as_ref().unwrap().n_expr;
                }

                // Define useTempTable como VERDADEIRO se o resultado do SELECT deve ser
                // escrito numa tabela temporária (modelo 4) e FALSO se cada linha de
                // saída pode ser escrita direto na tabela de destino (modelo 3).
                //
                // Uma tabela temporária é obrigatória se a tabela atualizada também é
                // uma das lidas pelo SELECT. Também se usa em caso de triggers de linha.
                if p_trigger.is_some() || reads_table(p_parse, i_db, &p_tab) != 0 {
                    use_temp_table = true;
                }

                if use_temp_table {
                    // Invoca a co-rotina para extrair informação do SELECT e a adiciona
                    // a uma tabela transitória srcTab. O código gerado aqui é do 4o
                    // modelo:
                    //
                    //      B: abre a tabela temporária
                    //      L: yield X, goto M no EOF
                    //         insere a linha de R..R+n na tabela temporária
                    //         goto L
                    //      M: ...
                    src_tab = p_parse.n_tab;
                    p_parse.n_tab += 1;
                    let reg_rec = get_temp_reg(p_parse); // Registro do registro empacotado
                    let reg_temp_rowid = get_temp_reg(p_parse); // Registro do ROWID temporário
                    vdbe_add_op2(&mut v.borrow_mut(), OP_OPENEPHEMERAL as i32, src_tab, n_column);
                    let addr_l = vdbe_add_op1(&mut v.borrow_mut(), OP_YIELD as i32, dest.i_sdparm);
                    vdbe_add_op3(&mut v.borrow_mut(), OP_MAKERECORD as i32, reg_from_select, n_column, reg_rec);
                    vdbe_add_op2(&mut v.borrow_mut(), OP_NEWROWID as i32, src_tab, reg_temp_rowid);
                    vdbe_add_op3(&mut v.borrow_mut(), OP_INSERT as i32, src_tab, reg_rec, reg_temp_rowid);
                    vdbe_goto(&mut v.borrow_mut(), addr_l);
                    vdbe_jump_here(&mut v.borrow_mut(), addr_l);
                    release_temp_reg(p_parse, reg_rec);
                    release_temp_reg(p_parse, reg_temp_rowid);
                }
            } else {
                // Este é o caso em que os dados do INSERT vêm de um VALUES de uma linha.
                let mut s_nc = NameContext::default();
                src_tab = -1;
                debug_assert!(!use_temp_table);
                if let Some(list) = p_list.as_mut() {
                    n_column = list.n_expr;
                    if resolve_expr_list_names(p_parse, &mut s_nc, list) != 0 {
                        break 'insert_cleanup;
                    }
                } else {
                    n_column = 0;
                }
            }

            // Se não há IDLIST mas a tabela tem uma chave primária inteira, define
            // ipkColumn como o índice dessa coluna na definição original da tabela.
            if p_column.is_none() && n_column > 0 {
                ipk_column = p_tab.borrow().i_p_key as i32;
                if ipk_column >= 0 && (p_tab.borrow().tab_flags & TF_HAS_GENERATED) != 0 {
                    let mut i = ipk_column - 1;
                    while i >= 0 {
                        if (p_tab.borrow().a_col[i as usize].col_flags & COLFLAG_GENERATED) != 0 {
                            ipk_column -= 1;
                        }
                        i -= 1;
                    }
                }

                // Garante que o número de colunas dos dados de origem bate com o número
                // de colunas a inserir na tabela.
                debug_assert!(TF_HAS_HIDDEN == COLFLAG_HIDDEN as u32);
                debug_assert!(TF_HAS_GENERATED == COLFLAG_GENERATED as u32);
                debug_assert!(COLFLAG_NOINSERT == (COLFLAG_GENERATED | COLFLAG_HIDDEN));
                if (p_tab.borrow().tab_flags & (TF_HAS_GENERATED | TF_HAS_HIDDEN)) != 0 {
                    for i in 0..n_col as usize {
                        if (p_tab.borrow().a_col[i].col_flags & COLFLAG_NOINSERT) != 0 {
                            n_hidden += 1;
                        }
                    }
                }
                if n_column != (n_col - n_hidden) {
                    error_msg(
                        p_parse,
                        b"table %S has %d columns but %d values were supplied",
                        &[
                            PrintfArg::Text(src_item_text(&p_tab_list.as_ref().unwrap().a[0])),
                            PrintfArg::Int((n_col - n_hidden) as i64),
                            PrintfArg::Int(n_column as i64),
                        ],
                    );
                    break 'insert_cleanup;
                }
            }
            if let Some(col) = p_column.as_ref() {
                if n_column != col.n_id {
                    error_msg(
                        p_parse,
                        b"%d values for %d columns",
                        &[PrintfArg::Int(n_column as i64), PrintfArg::Int(col.n_id as i64)],
                    );
                    break 'insert_cleanup;
                }
            }

            // Inicializa a contagem de linhas a inserir
            if (db.borrow().flags & SQLITE_COUNT_ROWS) != 0
                && p_parse.nested == 0
                && p_parse.p_trigger_tab.is_none()
                && p_parse.b_returning == 0
            {
                p_parse.n_mem += 1;
                reg_row_count = p_parse.n_mem;
                vdbe_add_op2(&mut v.borrow_mut(), OP_INTEGER as i32, 0, reg_row_count);
            }

            // Se não é uma view, abre a tabela e todos os índices
            if !is_view_tab {
                let n_idx = open_table_and_indices(
                    p_parse,
                    &p_tab,
                    OP_OPENWRITE as i32,
                    0,
                    -1,
                    None,
                    &mut i_data_cur,
                    &mut i_idx_cur,
                );
                a_reg_idx = vec![0i32; n_idx as usize + 2];
                let mut p_idx = p_tab.borrow().p_index.clone();
                let mut i: usize = 0;
                while (i as i32) < n_idx {
                    let idx = p_idx.expect("índice ausente");
                    p_parse.n_mem += 1;
                    a_reg_idx[i] = p_parse.n_mem;
                    p_parse.n_mem += idx.borrow().n_column as i32;
                    p_idx = idx.borrow().p_next.clone();
                    i += 1;
                }
                p_parse.n_mem += 1;
                a_reg_idx[i] = p_parse.n_mem; // Registro que guarda o registro da tabela
            }
            if p_upsert.is_some() {
                if is_virtual(&p_tab.borrow()) {
                    let z_name = p_tab.borrow().z_name.clone();
                    error_msg(
                        p_parse,
                        b"UPSERT not implemented for virtual table \"%s\"",
                        &[PrintfArg::Text(z_name)],
                    );
                    break 'insert_cleanup;
                }
                if is_view(&p_tab.borrow()) {
                    error_msg(p_parse, b"cannot UPSERT a view", &[]);
                    break 'insert_cleanup;
                }
                if has_explicit_nulls(
                    p_parse,
                    p_upsert.as_ref().unwrap().p_upsert_target.as_deref(),
                ) != 0
                {
                    break 'insert_cleanup;
                }
                p_tab_list.as_mut().unwrap().a[0].i_cursor = i_data_cur;

                // Desencadeia a lista de cláusulas para que a análise de cada alvo possa
                // ler as anteriores (pAll) enquanto altera a atual (pNx).
                let mut chain: Vec<Box<Upsert>> = Vec::new();
                let mut cur = p_upsert.take();
                while let Some(mut n) = cur {
                    cur = n.p_next_upsert.take();
                    chain.push(n);
                }
                for k in 0..chain.len() {
                    let (p_prior, p_rest) = chain.split_at_mut(k);
                    let p_nx = &mut p_rest[0];
                    p_nx.p_upsert_src = src_list_dup(&db, p_tab_list.as_ref().unwrap(), 0);
                    p_nx.reg_data = reg_data;
                    p_nx.i_data_cur = i_data_cur;
                    p_nx.i_idx_cur = i_idx_cur;
                    if p_nx.p_upsert_target.is_some() {
                        if upsert_analyze_target(p_parse, p_tab_list.as_ref().unwrap(), p_nx, p_prior) != 0 {
                            break 'insert_cleanup;
                        }
                    }
                }
                let mut head: Option<Box<Upsert>> = None;
                while let Some(mut n) = chain.pop() {
                    n.p_next_upsert = head;
                    head = Some(n);
                }
                p_upsert = head;
            }

            // Este é o topo do laço principal de inserção
            if use_temp_table {
                // Este bloco codifica só o topo do laço. O laço completo é o pseudocódigo
                // a seguir (modelo 4):
                //
                //         rebobina a tabela temporária, se vazia goto D
                //      C: laço sobre as linhas da tabela intermediária
                //           transfere os valores da intermediária para <table>
                //         fim do laço
                //      D: ...
                addr_ins_top = vdbe_add_op1(&mut v.borrow_mut(), OP_REWIND as i32, src_tab);
                addr_cont = vdbe_current_addr(&v.borrow());
            } else if p_select.is_some() {
                // Este bloco codifica só o topo do laço. O laço completo é o pseudocódigo
                // a seguir (modelo 3):
                //
                //      C: yield X, no EOF goto D
                //         insere o resultado do select em <table> a partir de R..R+n
                //         goto C
                //      D: ...
                vdbe_release_registers(&mut *p_parse, reg_data, n_col, 0, 0);
                addr_ins_top = vdbe_add_op1(&mut v.borrow_mut(), OP_YIELD as i32, dest.i_sdparm);
                addr_cont = addr_ins_top;
                if ipk_column >= 0 {
                    // tag-20191021-001: Se o INTEGER PRIMARY KEY é gerado pelo SELECT,
                    // copia o valor para o slot do rowid já, para que não seja
                    // sobrescrito por um NULL na tag-20191021-002.
                    vdbe_add_op2(
                        &mut v.borrow_mut(),
                        OP_COPY as i32,
                        reg_from_select + ipk_column,
                        reg_rowid,
                    );
                }
            }

            // Calcula os dados das colunas ordinárias da nova entrada. Os valores são
            // escritos em ordem de armazenamento em registros a partir de regData. Só as
            // colunas ordinárias são calculadas neste laço. O rowid (se houver) é
            // calculado depois, e as colunas geradas depois do rowid, pois podem depender
            // dele.
            n_hidden = 0;
            let mut i_reg_store = reg_data;
            debug_assert!(reg_data == reg_rowid + 1);
            for i in 0..n_col {
                'column: {
                    debug_assert!(i >= n_hidden);
                    if i == p_tab.borrow().i_p_key as i32 {
                        // tag-20191021-002: As referências ao INTEGER PRIMARY KEY são
                        // preenchidas com o rowid. Então põe um NULL no slot do IPK do
                        // registro para não gastar espaço. A definição do formato de
                        // arquivo exige esse NULL extra: não dá para otimizar pulando a
                        // coluna.
                        vdbe_add_op1(&mut v.borrow_mut(), OP_SOFTNULL as i32, i_reg_store);
                        break 'column;
                    }
                    let col_flags = p_tab.borrow().a_col[i as usize].col_flags;
                    if (col_flags & COLFLAG_NOINSERT) != 0 {
                        n_hidden += 1;
                        if (col_flags & COLFLAG_VIRTUAL) != 0 {
                            // Colunas virtuais não participam do OP_MakeRecord. Então
                            // recua iRegStore uma posição para compensar o iRegStore++
                            // do laço externo.
                            i_reg_store -= 1;
                            break 'column;
                        } else if (col_flags & COLFLAG_STORED) != 0 {
                            // Colunas stored são calculadas depois. Mas, se há triggers
                            // BEFORE, os slots das colunas stored serão copiados (OP_Copy)
                            // para um segundo bloco de registros, então o registro precisa
                            // ser inicializado com NULL para evitar leitura de registro
                            // não inicializado.
                            if (tmask & TRIGGER_BEFORE as u32) != 0 {
                                vdbe_add_op1(&mut v.borrow_mut(), OP_SOFTNULL as i32, i_reg_store);
                            }
                            break 'column;
                        } else if p_column.is_none() {
                            // Colunas ocultas não nomeadas no INSERT recebem o valor padrão
                            code_column_default(p_parse, &p_tab, i as usize, i_reg_store);
                            break 'column;
                        }
                    }
                    let k: i32;
                    if let Some(col) = p_column.as_ref() {
                        debug_assert!(col.e_u4 == EU4_IDX);
                        let mut j: i32 = 0;
                        while j < col.n_id && col.a[j as usize].u4_idx != i {
                            j += 1;
                        }
                        if j >= col.n_id {
                            // Uma coluna não nomeada na lista do INSERT recebe o valor
                            // padrão
                            code_column_default(p_parse, &p_tab, i as usize, i_reg_store);
                            break 'column;
                        }
                        k = j;
                    } else if n_column == 0 {
                        // É INSERT INTO ... DEFAULT VALUES. Carrega o valor padrão.
                        code_column_default(p_parse, &p_tab, i as usize, i_reg_store);
                        break 'column;
                    } else {
                        k = i - n_hidden;
                    }

                    if use_temp_table {
                        vdbe_add_op3(&mut v.borrow_mut(), OP_COLUMN as i32, src_tab, k, i_reg_store);
                    } else if p_select.is_some() {
                        if reg_from_select != reg_data {
                            vdbe_add_op2(&mut v.borrow_mut(), OP_SCOPY as i32, reg_from_select + k, i_reg_store);
                        }
                    } else {
                        let p_x = p_list.as_ref().unwrap().a[k as usize].p_expr.as_deref();
                        let y = expr_code_target(p_parse, p_x, i_reg_store);
                        if y != i_reg_store {
                            let op = if expr_has_property(p_x.unwrap(), EP_SUBQUERY) {
                                OP_COPY
                            } else {
                                OP_SCOPY
                            };
                            vdbe_add_op2(&mut v.borrow_mut(), op as i32, y, i_reg_store);
                        }
                    }
                }
                i_reg_store += 1;
            }

            // Roda os triggers BEFORE e INSTEAD OF, se houver
            let end_of_loop = vdbe_make_label(p_parse);
            if (tmask & TRIGGER_BEFORE as u32) != 0 {
                let reg_cols = get_temp_range(p_parse, n_col + 1);

                // Monta a linha de referência NEW.*. Se há um INTEGER PRIMARY KEY no qual
                // se insere NULL, esse NULL vira um ID único da linha. Mas num trigger
                // BEFORE não sabemos qual será o ID (o insert ainda não aconteceu), então
                // se substitui por um rowid -1.
                if ipk_column < 0 {
                    vdbe_add_op2(&mut v.borrow_mut(), OP_INTEGER as i32, -1, reg_cols);
                } else {
                    debug_assert!(!without_rowid);
                    if use_temp_table {
                        vdbe_add_op3(&mut v.borrow_mut(), OP_COLUMN as i32, src_tab, ipk_column, reg_cols);
                    } else {
                        debug_assert!(p_select.is_none()); // Senão useTempTable é verdadeiro
                        let p_ipk = p_list.as_ref().unwrap().a[ipk_column as usize].p_expr.as_deref();
                        expr_code(p_parse, p_ipk, reg_cols);
                    }
                    let addr1 = vdbe_add_op1(&mut v.borrow_mut(), OP_NOTNULL as i32, reg_cols);
                    vdbe_add_op2(&mut v.borrow_mut(), OP_INTEGER as i32, -1, reg_cols);
                    vdbe_jump_here(&mut v.borrow_mut(), addr1);
                    vdbe_add_op1(&mut v.borrow_mut(), OP_MUSTBEINT as i32, reg_cols);
                }

                // Copia os novos dados já gerados.
                let n_nv_col = p_tab.borrow().n_nv_col as i32;
                debug_assert!(n_nv_col > 0 || p_parse.n_err > 0);
                vdbe_add_op3(&mut v.borrow_mut(), OP_COPY as i32, reg_rowid + 1, reg_cols + 1, n_nv_col - 1);

                // Calcula o novo valor das colunas geradas depois de todas as outras
                // colunas. Deve ser feito depois de calcular o ROWID, caso uma coluna
                // gerada se refira ao ROWID.
                if (p_tab.borrow().tab_flags & TF_HAS_GENERATED) != 0 {
                    compute_generated_columns(p_parse, reg_cols + 1, &p_tab);
                }

                // Se é um INSERT numa view com trigger INSTEAD OF INSERT, não tenta
                // nenhuma conversão antes de montar o registro. Se é uma tabela real,
                // tenta as conversões exigidas pelas afinidades das colunas.
                if !is_view_tab {
                    table_affinity(&mut v.borrow_mut(), &p_tab, reg_cols + 1);
                }

                // Dispara os triggers BEFORE ou INSTEAD OF
                code_row_trigger(
                    p_parse,
                    p_trigger.as_ref().unwrap(),
                    TK_INSERT,
                    None,
                    TRIGGER_BEFORE,
                    &p_tab,
                    reg_cols - n_col - 1,
                    on_error as u8,
                    end_of_loop,
                );

                release_temp_range(p_parse, reg_cols, n_col + 1);
            }

            if !is_view_tab {
                if is_virtual(&p_tab.borrow()) {
                    // A linha que o VUpdate apagará: nenhuma
                    vdbe_add_op2(&mut v.borrow_mut(), OP_NULL as i32, 0, reg_ins);
                }
                if ipk_column >= 0 {
                    // Calcula o novo rowid
                    if use_temp_table {
                        vdbe_add_op3(&mut v.borrow_mut(), OP_COLUMN as i32, src_tab, ipk_column, reg_rowid);
                    } else if p_select.is_some() {
                        // Rowid já inicializado na tag-20191021-001
                    } else {
                        let p_ipk = p_list.as_ref().unwrap().a[ipk_column as usize].p_expr.as_deref();
                        if p_ipk.unwrap().op == TK_NULL && !is_virtual(&p_tab.borrow()) {
                            vdbe_add_op3(&mut v.borrow_mut(), OP_NEWROWID as i32, i_data_cur, reg_rowid, reg_autoinc);
                            append_flag = 1;
                        } else {
                            expr_code(p_parse, p_ipk, reg_rowid);
                        }
                    }
                    // Se a expressão da PRIMARY KEY é NULL, usa OP_NewRowid para gerar um
                    // valor único de chave primária.
                    if append_flag == 0 {
                        if !is_virtual(&p_tab.borrow()) {
                            let addr1 = vdbe_add_op1(&mut v.borrow_mut(), OP_NOTNULL as i32, reg_rowid);
                            vdbe_add_op3(&mut v.borrow_mut(), OP_NEWROWID as i32, i_data_cur, reg_rowid, reg_autoinc);
                            vdbe_jump_here(&mut v.borrow_mut(), addr1);
                        } else {
                            let addr1 = vdbe_current_addr(&v.borrow());
                            vdbe_add_op2(&mut v.borrow_mut(), OP_ISNULL as i32, reg_rowid, addr1 + 2);
                        }
                        vdbe_add_op1(&mut v.borrow_mut(), OP_MUSTBEINT as i32, reg_rowid);
                    }
                } else if is_virtual(&p_tab.borrow()) || without_rowid {
                    vdbe_add_op2(&mut v.borrow_mut(), OP_NULL as i32, 0, reg_rowid);
                } else {
                    vdbe_add_op3(&mut v.borrow_mut(), OP_NEWROWID as i32, i_data_cur, reg_rowid, reg_autoinc);
                    append_flag = 1;
                }
                auto_inc_step(p_parse, reg_autoinc, reg_rowid);

                // Calcula o novo valor das colunas geradas depois de todas as outras
                // colunas. Deve ser feito depois de calcular o ROWID, caso uma coluna
                // gerada derive do INTEGER PRIMARY KEY.
                if (p_tab.borrow().tab_flags & TF_HAS_GENERATED) != 0 {
                    compute_generated_columns(p_parse, reg_rowid + 1, &p_tab);
                }

                // Gera código para verificar as restrições, gerar as chaves de índice e
                // fazer a inserção.
                if is_virtual(&p_tab.borrow()) {
                    let p_vtab = get_v_table(&db, &p_tab).expect("tabela virtual sem VTable");
                    vtab_make_writable(p_parse, &p_tab);
                    vdbe_add_op4(
                        &mut v.borrow_mut(),
                        OP_VUPDATE as i32,
                        1,
                        n_col + 2,
                        reg_ins,
                        P4Value::VTab(p_vtab),
                        P4_VTAB,
                    );
                    vdbe_change_p5(
                        &mut v.borrow_mut(),
                        (if on_error == OE_DEFAULT as i32 { OE_ABORT as i32 } else { on_error }) as u16,
                    );
                    may_abort(p_parse);
                } else {
                    let mut is_replace: i32 = 0; // Verdadeiro se as restrições podem causar replace
                    generate_constraint_checks(
                        p_parse,
                        &p_tab,
                        &a_reg_idx,
                        i_data_cur,
                        i_idx_cur,
                        reg_ins,
                        0,
                        (ipk_column >= 0) as u8,
                        on_error as u8,
                        end_of_loop,
                        &mut is_replace,
                        &[],
                        p_upsert.as_deref(),
                    );
                    if (db.borrow().flags & SQLITE_FOREIGN_KEYS) != 0 {
                        fk_check(p_parse, &p_tab, 0, reg_ins, None, 0);
                    }

                    // Define a flag OPFLAG_USESEEKRESULT se (a) não há restrições REPLACE
                    // ou (b) não há triggers e a tabela não é pai numa chave estrangeira.
                    // É seguro no segundo caso porque, se uma restrição REPLACE for
                    // atingida, um OP_Delete ou OP_IdxDelete será executado em cada cursor
                    // perturbado, e ambos limpam VdbeCursor.seekResult, desabilitando o
                    // OPFLAG_USESEEKRESULT.
                    let b_use_seek = is_replace == 0 || vdbe_has_sub_program(&v.borrow()) == 0;
                    complete_insertion(
                        p_parse,
                        &p_tab,
                        i_data_cur,
                        i_idx_cur,
                        reg_ins,
                        &a_reg_idx,
                        0,
                        append_flag as i32,
                        b_use_seek as i32,
                    );
                }
            }

            // Atualiza a contagem de linhas inseridas
            if reg_row_count != 0 {
                vdbe_add_op2(&mut v.borrow_mut(), OP_ADDIMM as i32, reg_row_count, 1);
            }

            if p_trigger.is_some() {
                // Código dos triggers AFTER
                code_row_trigger(
                    p_parse,
                    p_trigger.as_ref().unwrap(),
                    TK_INSERT,
                    None,
                    TRIGGER_AFTER,
                    &p_tab,
                    reg_data - 2 - n_col,
                    on_error as u8,
                    end_of_loop,
                );
            }

            // O fim do laço principal de inserção, se a fonte dos dados é um SELECT.
            vdbe_resolve_label(&mut v.borrow_mut(), end_of_loop);
            if use_temp_table {
                vdbe_add_op2(&mut v.borrow_mut(), OP_NEXT as i32, src_tab, addr_cont);
                vdbe_jump_here(&mut v.borrow_mut(), addr_ins_top);
                vdbe_add_op1(&mut v.borrow_mut(), OP_CLOSE as i32, src_tab);
            } else if p_select.is_some() {
                vdbe_goto(&mut v.borrow_mut(), addr_cont);
                vdbe_jump_here(&mut v.borrow_mut(), addr_ins_top);
            }
        } // insert_end:

        // Atualiza a tabela sqlite_sequence gravando o conteúdo dos contadores de rowid
        // máximo registrados durante as inserções em tabelas autoincrement.
        if p_parse.nested == 0 && p_parse.p_trigger_tab.is_none() {
            autoincrement_end(p_parse);
        }

        // Devolve o número de linhas inseridas. Se esta rotina está gerando código por
        // causa de uma chamada a sqlite3NestedParse(), não invoca a função de retorno.
        if reg_row_count != 0 {
            code_change_count(&mut v.borrow_mut(), reg_row_count, b"rows inserted");
        }
    } // insert_cleanup:

    // sqlite3SrcListDelete, sqlite3ExprListDelete, sqlite3UpsertDelete,
    // sqlite3SelectDelete, sqlite3IdListDelete e a liberação de aRegIdx: o Drop dos
    // valores abaixo (p_tab_list, p_list, p_upsert, p_select, p_column, a_reg_idx).
    let _ = (&p_tab_list, &p_list, &p_upsert, &p_select, &p_column, &a_reg_idx);
}


// ---- part_004.rs ----

// Nota de porte: o trecho C 004 a 007 contém UMA função só, `sqlite3GenerateConstraintChecks`
// (mais o que vem depois dela). Uma função Rust não atravessa arquivos, então ela inteira vive
// aqui, em `generate_constraint_checks`; as partes 005 e 006 ficam sem itens.
//
// Opções do Debian 13 resolvidas neste trecho:
//  - SQLITE_DEBUG ausente: `VdbeComment`, `VdbeNoopComment`, `VdbeModuleComment`, `VdbeCoverage`,
//    `testcase`, `sqlite3VdbeVerifyAbortable` e `sqlite3VdbeReleaseRegisters` são macros vazias e
//    somem. `sqlite3SetMakeRecordP5` também é macro vazia (SQLITE_ENABLE_NULL_TRIM ausente).
//  - SQLITE_ENABLE_PREUPDATE_HOOK presente: o atalho "colisão omitida" do índice PRIMARY KEY de
//    WITHOUT ROWID (`#ifndef SQLITE_ENABLE_PREUPDATE_HOOK`) some, e o OP_Delete com
//    OPFLAG_ISNOOP do ramo REPLACE do rowid fica.
//  - Constantes `OE_*` são `i32` (o `int onError` do C). `sqlite3OpcodeProperty` vira
//    `OPCODE_PROPERTY`. `UpsertRef` e `TriggerRef` são `Rc<RefCell<_>>` do prelude.

/// Bit de `p_walker.e_code`: o CHECK usa uma coluna que está mudando
pub const CKCNSTRNT_COLUMN: u16 = 0x01;

/// Bit de `p_walker.e_code`: o CHECK referencia o ROWID
pub const CKCNSTRNT_ROWID: u16 = 0x02;

/// Callback do Walker de `expr_references_updated_column`. Liga o bit 0x01 de `e_code` se este nó
/// da expressão referencia alguma das colunas modificadas pelo UPDATE (e o 0x02 se referencia o
/// rowid).
fn check_constraint_expr_node(p_walker: &mut Walker, p_expr: &Expr) -> i32 {
    if p_expr.op == TK_COLUMN {
        debug_assert!(p_expr.i_column >= 0 || p_expr.i_column == -1);
        if p_expr.i_column >= 0 {
            let flagged = match &p_walker.u {
                WalkerU::AiCol(ai_col) => ai_col[p_expr.i_column as usize] >= 0,
                _ => false,
            };
            if flagged {
                p_walker.e_code |= CKCNSTRNT_COLUMN;
            }
        } else {
            p_walker.e_code |= CKCNSTRNT_ROWID;
        }
    }
    WRC_CONTINUE
}

/// `p_expr` é uma restrição CHECK de uma linha que está sendo UPDATE-ada. As únicas colunas
/// modificadas pelo UPDATE são aquelas com `ai_chng[i] >= 0`, e o ROWID também muda se
/// `chng_rowid` for verdadeiro.
///
/// Devolve verdadeiro se o CHECK usa alguma coluna que muda (ou o rowid, se ele muda). Ou seja,
/// se esta restrição precisa ser validada para a nova linha do UPDATE.
///
/// 2018-09-15: `p_expr` também pode ser uma expressão de índice sobre expressões. A operação é a
/// mesma: verdadeiro se e só se a expressão usa uma ou mais colunas identificadas pelo segundo e
/// terceiro argumentos.
pub fn expr_references_updated_column(p_expr: &Expr, ai_chng: &[i32], chng_rowid: i32) -> i32 {
    let mut w = Walker::default();
    w.e_code = 0;
    w.x_expr_callback = Some(check_constraint_expr_node);
    w.u = WalkerU::AiCol(ai_chng.to_vec());
    walk_expr(&mut w, p_expr);
    if chng_rowid == 0 {
        w.e_code &= !CKCNSTRNT_ROWID;
    }
    (w.e_code != 0) as i32
}

/// Elemento do vetor de índices reordenado, usado quando `IndexIterator.e_type == 1`
pub struct IndexListTerm {
    /// O índice
    pub p: IndexRef,
    /// Qual entrada da lista `Table.p_index` original é este índice
    pub ix: i32,
}

/// Iterador que percorre os índices de uma tabela na ordem de `Index.p_next` ou numa outra ordem
/// dada por um vetor de `IndexListTerm`. A união do C (`u.lx` e `u.ax`) vira dois campos: o
/// `p_idx` de `lx` e o vetor `a_idx` de `ax` (cujo tamanho `n_idx` é `a_idx.len()`).
pub struct IndexIterator {
    /// 0 para a lista `Index.p_next`, 1 para o vetor de `IndexListTerm`
    pub e_type: i32,
    /// Índice do item atual na lista
    pub i: i32,
    /// `u.lx.pIdx`: o índice atual (e_type == 0)
    pub p_idx: Option<IndexRef>,
    /// `u.ax.aIdx`: vetor de termos (e_type == 1)
    pub a_idx: Vec<IndexListTerm>,
}

/// Devolve o primeiro índice da lista
fn index_iterator_first(p_iter: &IndexIterator, p_ix: &mut i32) -> Option<IndexRef> {
    debug_assert!(p_iter.i == 0);
    if p_iter.e_type != 0 {
        *p_ix = p_iter.a_idx[0].ix;
        Some(p_iter.a_idx[0].p.clone())
    } else {
        *p_ix = 0;
        p_iter.p_idx.clone()
    }
}

/// Devolve o próximo índice da lista. Devolve `None` quando os índices acabam
fn index_iterator_next(p_iter: &mut IndexIterator, p_ix: &mut i32) -> Option<IndexRef> {
    if p_iter.e_type != 0 {
        p_iter.i += 1;
        let i = p_iter.i;
        if i >= p_iter.a_idx.len() as i32 {
            *p_ix = i;
            return None;
        }
        *p_ix = p_iter.a_idx[i as usize].ix;
        Some(p_iter.a_idx[i as usize].p.clone())
    } else {
        *p_ix += 1;
        let p_next = p_iter.p_idx.as_ref().unwrap().borrow().p_next.clone();
        p_iter.p_idx = p_next;
        p_iter.p_idx.clone()
    }
}

/// Comparação de ponteiros `pPk==pIdx` do C, com `pPk` possivelmente nulo
fn index_is(p_a: &Option<IndexRef>, p_b: &IndexRef) -> bool {
    match p_a {
        Some(a) => Rc::ptr_eq(a, p_b),
        None => false,
    }
}

/// Comparação de ponteiros `pUpsertClause==pUpsert` do C, com o primeiro possivelmente nulo
fn upsert_is(p_a: &Option<UpsertRef>, p_b: &UpsertRef) -> bool {
    match p_a {
        Some(a) => Rc::ptr_eq(a, p_b),
        None => false,
    }
}

/// Gera o código que faz as verificações de restrição antes de um INSERT ou UPDATE na tabela
/// `p_tab`.
///
/// `reg_new_data` é o primeiro registro de um intervalo com os dados a inserir (ou os dados após
/// o UPDATE): são `p_tab.n_col + 1` registros. O primeiro tem o novo rowid, ou NULL numa tabela
/// WITHOUT ROWID; o segundo tem o conteúdo da primeira coluna, e assim por diante.
///
/// `reg_old_data` é parecido, mas com os dados anteriores ao UPDATE. É zero num INSERT, e é assim
/// que a rotina distingue UPDATE de INSERT.
///
/// Num UPDATE, `pk_chng` é verdadeiro se a chave primária real (rowid, ou a PRIMARY KEY de uma
/// WITHOUT ROWID) pode ser modificada. Num INSERT, indica se o rowid foi dado explicitamente
/// (só é verdadeiro se o INSERT fornece um inteiro para a coluna rowid ou seu alias INTEGER
/// PRIMARY KEY).
///
/// O código gerado guarda as novas entradas de índice nos registros `a_reg_idx[]`; índices com
/// `a_reg_idx[i] == 0` não geram entrada. A ordem é a de `p_tab.p_index`. (2019-05-07) O código
/// também cria o registro da tabela principal, se ela tem rowid, em `a_reg_idx[n_idx]`, a
/// primeira entrada depois do último índice. Por isso `a_reg_idx` precisa ter `n_idx + 1`
/// posições.
///
/// O chamador já abriu cursores de escrita na tabela principal e em todos os índices aplicáveis.
/// `i_data_cur` é o cursor da tabela principal (ou do índice PRIMARY KEY numa WITHOUT ROWID);
/// `i_idx_cur` é o cursor do primeiro índice de `p_tab.p_index`, e os demais ficam em
/// `i_idx_cur + N`.
///
/// Verifica NOT NULL, CHECK e UNIQUE. Ações possíveis: ROLLBACK, ABORT, FAIL, REPLACE e IGNORE.
/// A ação vem de `override_error`; se for `OE_DEFAULT`, de `p_parse.on_error`; se também for
/// `OE_DEFAULT`, do `onError` da própria restrição.
pub fn generate_constraint_checks(
    p_parse: &mut Parse,
    p_tab: &TableRef,
    a_reg_idx: &[i32],
    i_data_cur: i32,
    i_idx_cur: i32,
    reg_new_data: i32,
    reg_old_data: i32,
    pk_chng: u8,
    override_error: u8,
    ignore_dest: i32,
    pb_may_replace: &mut i32,
    ai_chng: Option<&[i32]>,
    mut p_upsert: Option<UpsertRef>,
) {
    let mut override_error: i32 = override_error as i32;
    let mut on_error: i32 = 0;
    let mut seen_replace: i32 = 0; // Verdadeiro se REPLACE resolve conflito da PK inteira
    let mut p_upsert_clause: Option<UpsertRef> = None; // A cláusula ON CONFLICT do índice atual
    let mut b_affinity_done = false; // Verdadeiro se o OP_Affinity já foi gerado
    let mut upsert_ipk_return: i32 = 0; // Endereço do Goto ao fim da checagem do IPK
    let mut upsert_ipk_delay: i32 = 0; // Endereço do Goto que pula a checagem inicial do IPK
    let mut ipk_top: i32 = 0; // Topo da checagem de unicidade do IPK
    let mut ipk_bottom: i32 = 0; // OP_Goto ao fim da checagem de unicidade do IPK
    let mut addr_recheck: i32 = 0; // Pula aqui para reverificar todas as restrições de unicidade
    let mut lbl_recheck_ok: i32 = 0; // Cada reverificação pula aqui se passa
    let mut n_replace_trig: i32 = 0; // Número de triggers de REPLACE codificados

    let is_update = reg_old_data != 0;
    let db = p_parse.db.upgrade().unwrap();
    let v = p_parse.p_vdbe.clone().unwrap();
    let tab = p_tab.borrow();
    debug_assert!(!is_view(&tab)); // Esta tabela não é uma VIEW
    let n_col = tab.n_col as i32;

    // `p_pk` é o índice PRIMARY KEY das tabelas WITHOUT ROWID e `None` nas tabelas normais.
    // `n_pk_field` é o número de campos de chave de `p_pk`, ou 1 numa tabela com rowid. Ou seja, o
    // número de campos da chave primária real da tabela.
    let (p_pk, n_pk_field): (Option<IndexRef>, i32) = if has_rowid(&tab) {
        (None, 1)
    } else {
        let p_pk = primary_key_index(&tab).unwrap();
        let n = p_pk.borrow().n_key_col as i32;
        (Some(p_pk), n)
    };

    // Testa todas as restrições NOT NULL.
    if (tab.tab_flags & TF_HASNOTNULL) != 0 {
        let mut b_2nd_pass = false; // Verdadeiro se está rodando a segunda passada
        let mut n_seen_replace: i32 = 0; // Número de operações ON CONFLICT REPLACE
        let mut n_generated: i32 = 0; // Número de colunas geradas com NOT NULL
        loop {
            // Faz 2 passadas sobre as colunas. Sai do laço por "break"
            for i in 0..n_col {
                let p_col = &tab.a_col[i as usize]; // A coluna a testar quanto a NOT NULL
                on_error = p_col.not_null as i32;
                if on_error == OE_NONE {
                    continue; // Sem NOT NULL nesta coluna
                }
                if i == tab.i_p_key as i32 {
                    continue; // O ROWID nunca é NULL
                }
                let is_generated = (p_col.col_flags & COLFLAG_GENERATED) != 0;
                if is_generated && !b_2nd_pass {
                    n_generated += 1;
                    continue; // Colunas geradas são processadas na segunda passada
                }
                if let Some(a) = ai_chng {
                    if a[i as usize] < 0 && !is_generated {
                        // Não testa NOT NULL em colunas que não mudam
                        continue;
                    }
                }
                if override_error != OE_DEFAULT {
                    on_error = override_error;
                } else if on_error == OE_DEFAULT {
                    on_error = OE_ABORT;
                }
                if on_error == OE_REPLACE {
                    if b_2nd_pass /* REPLACE vira ABORT na segunda passada */
                        || p_col.i_dflt == 0 /* REPLACE é ABORT se não há valor DEFAULT */
                    {
                        on_error = OE_ABORT;
                    } else {
                        debug_assert!(!is_generated);
                    }
                } else if b_2nd_pass && !is_generated {
                    continue;
                }
                debug_assert!(
                    on_error == OE_ROLLBACK
                        || on_error == OE_ABORT
                        || on_error == OE_FAIL
                        || on_error == OE_IGNORE
                        || on_error == OE_REPLACE
                );
                let i_reg = table_column_to_storage(&tab, i) + reg_new_data + 1;
                match on_error {
                    OE_REPLACE => {
                        let addr1 = vdbe_add_op1(&mut v.borrow_mut(), OP_NOTNULL as i32, i_reg);
                        debug_assert!((p_col.col_flags & COLFLAG_GENERATED) == 0);
                        n_seen_replace += 1;
                        expr_code_copy(p_parse, column_expr(&tab, p_col), i_reg);
                        vdbe_jump_here(&mut v.borrow_mut(), addr1);
                    }
                    OE_ABORT | OE_ROLLBACK | OE_FAIL => {
                        if on_error == OE_ABORT {
                            may_abort(p_parse);
                            // queda deliberada para o tratamento de ROLLBACK e FAIL
                        }
                        // sqlite3MPrintf(db, "%s.%s", ...) com dois %s só concatena
                        let mut z_msg: Vec<u8> = tab.z_name.clone();
                        z_msg.push(b'.');
                        z_msg.extend_from_slice(&p_col.z_cn_name);
                        vdbe_add_op3(
                            &mut v.borrow_mut(),
                            OP_HALTIFNULL as i32,
                            SQLITE_CONSTRAINT_NOTNULL,
                            on_error,
                            i_reg,
                        );
                        vdbe_append_p4(&mut v.borrow_mut(), P4Union::Text(z_msg), P4_DYNAMIC as i32);
                        vdbe_change_p5(&mut v.borrow_mut(), P5_CONSTRAINTNOTNULL as u16);
                    }
                    _ => {
                        debug_assert!(on_error == OE_IGNORE);
                        vdbe_add_op2(&mut v.borrow_mut(), OP_ISNULL as i32, i_reg, ignore_dest);
                    }
                } // fim do switch(onError)
            } // fim do laço de i sobre as colunas
            if n_generated == 0 && n_seen_replace == 0 {
                // Se não há colunas geradas com NOT NULL nem restrições NOT NULL ON CONFLICT
                // REPLACE, uma única passada basta
                break;
            }
            if b_2nd_pass {
                break; // Nunca precisa de mais de 2 passadas
            }
            b_2nd_pass = true;
            if n_seen_replace > 0 && (tab.tab_flags & TF_HASGENERATED) != 0 {
                // Se alguma restrição NOT NULL ON CONFLICT REPLACE disparou na primeira passada,
                // recalcula os valores de todas as colunas geradas, pois eles podem depender de
                // colunas afetadas pelo REPLACE.
                compute_generated_columns(p_parse, reg_new_data + 1, p_tab);
            }
        } // fim do laço de 2 passadas
    } // fim do if (há restrições NOT NULL)

    // Testa todas as restrições CHECK
    if let Some(p_check) = tab.p_check.as_ref() {
        if (db.borrow().flags & SQLITE_IGNORECHECKS) == 0 {
            p_parse.i_self_tab = -(reg_new_data + 1);
            on_error = if override_error != OE_DEFAULT { override_error } else { OE_ABORT };
            for i in 0..p_check.n_expr as usize {
                let p_expr = p_check.a[i].p_expr.as_ref().unwrap();
                if let Some(a) = ai_chng {
                    if expr_references_updated_column(p_expr, a, pk_chng as i32) == 0 {
                        // As restrições CHECK não referenciam nenhuma das colunas atualizadas,
                        // então não adianta verificar a restrição
                        continue;
                    }
                }
                if !b_affinity_done {
                    table_affinity(&mut v.borrow_mut(), &tab, reg_new_data + 1);
                    b_affinity_done = true;
                }
                let all_ok = vdbe_make_label(p_parse);
                let p_copy = expr_dup(&db.borrow(), Some(p_expr), 0);
                if db.borrow().malloc_failed == 0 {
                    expr_if_true(p_parse, p_copy.as_deref(), all_ok, SQLITE_JUMPIFNULL);
                }
                drop(p_copy); // sqlite3ExprDelete(db, pCopy)
                if on_error == OE_IGNORE {
                    vdbe_goto(&mut v.borrow_mut(), ignore_dest);
                } else {
                    let z_name = p_check.a[i].z_e_name.clone();
                    debug_assert!(z_name.is_some() || db.borrow().malloc_failed != 0);
                    if on_error == OE_REPLACE {
                        on_error = OE_ABORT; // IMP: R-26383-51744
                    }
                    halt_constraint(
                        p_parse,
                        SQLITE_CONSTRAINT_CHECK,
                        on_error,
                        z_name,
                        P4_TRANSIENT,
                        P5_CONSTRAINTCHECK,
                    );
                }
                vdbe_resolve_label(&mut v.borrow_mut(), all_ok);
            }
            p_parse.i_self_tab = 0;
        }
    }

    // As restrições UNIQUE e PRIMARY KEY devem ser tratadas nesta ordem:
    //
    //   (1)  OE_Update
    //   (2)  OE_Abort, OE_Fail, OE_Rollback, OE_Ignore
    //   (3)  OE_Replace
    //
    // OE_Fail e OE_Ignore têm de acontecer antes de qualquer mudança. OE_Update garante que uma
    // única linha muda, então vem antes de OE_Replace. Tecnicamente OE_Abort e OE_Rollback
    // poderiam vir em qualquer ordem, mas ficam agrupados na frente por conveniência.
    //
    // 2018-08-14: Ticket https://www.sqlite.org/src/info/908f001483982c43
    // A ordem antiga tinha OE_Update como (2) e OE_Abort e afins como (1). Mas o PostgreSQL
    // verifica a restrição OE_Update antes das outras, então ela foi movida.
    //
    // O código das restrições é gerado nesta ordem:
    //   (A)  A restrição do rowid
    //   (B)  Restrições de índice único que não têm OE_Replace como resolução padrão
    //   (C)  Índices únicos que usam OE_Replace por padrão
    //
    // A ordem de (2) e (3) vem de a lista de índices da tabela pôr todos os índices OE_Replace no
    // fim. Ver `create_index` para saber onde isso acontece.
    let mut s_idx_iter = IndexIterator {
        e_type: 0,
        i: 0,
        p_idx: tab.p_index.clone(),
        a_idx: Vec::new(),
    };
    if let Some(ups) = p_upsert.clone() {
        if ups.borrow().p_upsert_target.is_none() {
            // Há só uma cláusula ON CONFLICT e ela não tem alvo de restrição
            debug_assert!(ups.borrow().p_next_upsert.is_none());
            if ups.borrow().is_do_update == 0 {
                // Um único ON CONFLICT DO NOTHING sem alvo de restrição. Toda resolução de
                // restrição única vira OE_Ignore
                override_error = OE_IGNORE;
                p_upsert = None;
            } else {
                // Um único ON CONFLICT DO UPDATE. Toda resolução vira OE_Update
                override_error = OE_UPDATE;
            }
        } else if tab.p_index.is_some() {
            // Senão é preciso usar a versão do iterador com o vetor de IndexListTerm, para
            // garantir que todas as condições ON CONFLICT sejam verificadas primeiro e em ordem.
            let mut n_idx: usize = 0;
            let mut p_idx_opt = tab.p_index.clone();
            while let Some(p_idx_ref) = p_idx_opt {
                debug_assert!(a_reg_idx[n_idx] > 0);
                n_idx += 1;
                p_idx_opt = p_idx_ref.borrow().p_next.clone();
            }
            s_idx_iter.e_type = 1;
            // O `pToFree` do C guardava o vetor para liberar junto com o Upsert; aqui o vetor
            // é dono de si mesmo. `b_used` é o vetor de bytes que o C punha depois dos termos.
            let mut b_used: Vec<u8> = vec![0u8; n_idx];
            let mut p_term_opt = Some(ups.clone());
            while let Some(p_term) = p_term_opt {
                let p_next_upsert = p_term.borrow().p_next_upsert.clone();
                if p_term.borrow().p_upsert_target.is_none() {
                    break;
                }
                let p_upsert_idx = p_term.borrow().p_upsert_idx.clone();
                if let Some(p_upsert_idx) = p_upsert_idx {
                    // (o ON CONFLICT do IPK, sem índice, é pulado)
                    let mut jj: usize = 0;
                    let mut p_idx_opt = tab.p_index.clone();
                    while let Some(p_idx_ref) = p_idx_opt.clone() {
                        if Rc::ptr_eq(&p_idx_ref, &p_upsert_idx) {
                            break;
                        }
                        p_idx_opt = p_idx_ref.borrow().p_next.clone();
                        jj += 1;
                    }
                    if b_used[jj] == 0 {
                        // (cláusula ON CONFLICT duplicada é ignorada)
                        b_used[jj] = 1;
                        s_idx_iter.a_idx.push(IndexListTerm {
                            p: p_idx_opt.unwrap(),
                            ix: jj as i32,
                        });
                    }
                }
                p_term_opt = p_next_upsert;
            }
            let mut jj: usize = 0;
            let mut p_idx_opt = tab.p_index.clone();
            while let Some(p_idx_ref) = p_idx_opt {
                if b_used[jj] == 0 {
                    s_idx_iter.a_idx.push(IndexListTerm {
                        p: p_idx_ref.clone(),
                        ix: jj as i32,
                    });
                }
                p_idx_opt = p_idx_ref.borrow().p_next.clone();
                jj += 1;
            }
            debug_assert!(s_idx_iter.a_idx.len() == n_idx);
        }
    }

    // Determina se triggers (explícitos ou ações de resolução de FK) podem rodar por causa de
    // deletes que acontecem na resolução de conflito OE_Replace. (Chamados "replace triggers".)
    // Se algum replace trigger roda, é preciso reverificar todas as restrições de unicidade
    // depois que todos rodarem. Mas na reverificação a resolução é OE_Abort em vez de OE_Replace.
    //
    // Se replace triggers são uma possibilidade, então
    //
    //   (1) Aloca o registro `reg_trig_cnt` e o zera. Esse registro conta quantos replace
    //       triggers disparam. A reverificação só ocorre se o número é positivo.
    //   (2) Inicializa `p_trigger` com a lista de todos os triggers DELETE de `p_tab`.
    //   (3) Inicializa `addr_recheck` e `lbl_recheck_ok`
    //
    // O código de reverificação cria uma série de testes para rodar numa segunda passada.
    // `addr_recheck` e `lbl_recheck_ok` ligam esses testes, que ficam separados entre si no
    // bytecode gerado.
    let p_trigger: Option<TriggerRef>;
    let mut reg_trig_cnt: i32;
    let flags = db.borrow().flags;
    if (flags & (SQLITE_RECTRIGGERS | SQLITE_FOREIGNKEYS)) == 0 {
        // Não há triggers DELETE nem restrições de FK. Nenhuma reverificação é necessária.
        p_trigger = None;
        reg_trig_cnt = 0;
    } else {
        if (flags & SQLITE_RECTRIGGERS) != 0 {
            p_trigger = triggers_exist(p_parse, p_tab, TK_DELETE, None, None);
            reg_trig_cnt = (p_trigger.is_some() || fk_required(p_parse, p_tab, None, 0) != 0) as i32;
        } else {
            p_trigger = None;
            reg_trig_cnt = fk_required(p_parse, p_tab, None, 0);
        }
        if reg_trig_cnt != 0 {
            // Podem existir replace triggers. Aloca o contador e o inicializa com zero.
            p_parse.n_mem += 1;
            reg_trig_cnt = p_parse.n_mem;
            vdbe_add_op2(&mut v.borrow_mut(), OP_INTEGER as i32, 0, reg_trig_cnt);
            lbl_recheck_ok = vdbe_make_label(p_parse);
            addr_recheck = lbl_recheck_ok;
        }
    }

    // Se o rowid está mudando, garante que o novo rowid não existe antes na tabela.
    if pk_chng != 0 && p_pk.is_none() {
        let addr_rowid_ok = vdbe_make_label(p_parse);

        // Descobre que ação tomar numa colisão de rowid
        on_error = tab.key_conf as i32;
        if override_error != OE_DEFAULT {
            on_error = override_error;
        } else if on_error == OE_DEFAULT {
            on_error = OE_ABORT;
        }

        // Descobre se o upsert se aplica neste caso
        if let Some(ups) = p_upsert.as_ref() {
            p_upsert_clause = upsert_of_index(ups, None);
            if let Some(clause) = p_upsert_clause.as_ref() {
                if clause.borrow().is_do_update == 0 {
                    on_error = OE_IGNORE; // DO NOTHING é o mesmo que INSERT OR IGNORE
                } else {
                    on_error = OE_UPDATE; // DO UPDATE
                }
            }
            if !upsert_is(&p_upsert_clause, ups) {
                // A primeira cláusula ON CONFLICT tem um alvo de conflito diferente do IPK. É
                // preciso saltar para essa primeira cláusula e depois voltar aqui para tratar o
                // IPK.
                upsert_ipk_delay = vdbe_add_op0(&mut v.borrow_mut(), OP_GOTO as i32);
            }
        }

        // Se a resposta a um conflito de rowid é REPLACE mas a resposta a alguma outra restrição
        // UNIQUE é FAIL ou IGNORE, é preciso adiar a checagem do conflito de rowid para depois
        // de as restrições UNIQUE rodarem.
        if on_error == OE_REPLACE /* A regra do IPK é REPLACE */
            && on_error != override_error /* As regras das outras restrições são diferentes */
            && tab.p_index.is_some() /* Existem outras restrições */
            && upsert_ipk_delay == 0 /* A checagem do IPK já foi adiada pelo UPSERT */
        {
            ipk_top = vdbe_add_op0(&mut v.borrow_mut(), OP_GOTO as i32) + 1;
        }

        if is_update {
            // pkChng!=0 não quer dizer que o rowid mudou, só que pode ter mudado. Pula a lógica
            // de conflito abaixo se o rowid não mudou.
            vdbe_add_op3(&mut v.borrow_mut(), OP_EQ as i32, reg_new_data, addr_rowid_ok, reg_old_data);
            vdbe_change_p5(&mut v.borrow_mut(), SQLITE_NOTNULL as u16);
        }

        // Vê se o novo rowid já existe na tabela. Pula a lógica de conflito abaixo se não existe.
        vdbe_add_op3(&mut v.borrow_mut(), OP_NOTEXISTS as i32, i_data_cur, addr_rowid_ok, reg_new_data);

        match on_error {
            OE_ROLLBACK | OE_ABORT | OE_FAIL => {
                rowid_constraint(p_parse, on_error, p_tab);
            }
            OE_REPLACE => {
                // Se há triggers DELETE na tabela e o flag de triggers recursivos está ligado,
                // chama `generate_row_delete` para remover a linha conflitante da tabela. Isso
                // dispara os triggers e remove as entradas da b-tree da tabela e dos índices.
                //
                // Senão, se não há triggers ou o flag de triggers recursivos está desligado, mas a
                // tabela tem um ou mais índices, chama `generate_row_index_delete`. Isso remove só
                // as entradas de índice. A entrada da b-tree da tabela é substituída pela nova
                // entrada quando ela é inserida.
                //
                // Se `generate_row_delete` ou `generate_row_index_delete` é chamada, também chama
                // `multi_write` para indicar que este VDBE pode precisar de rollback de
                // instrução (se a instrução for abortada depois do delete). Versões antigas
                // chamavam `multi_write` sempre, mas ser mais seletivo aqui permite que
                // instruções como
                //
                //   REPLACE INTO t(rowid) VALUES($newrowid)
                //
                // rodem sem journal de instrução se a tabela não tem índices.
                if reg_trig_cnt != 0 {
                    multi_write(p_parse);
                    generate_row_delete(
                        p_parse,
                        p_tab,
                        p_trigger.as_ref(),
                        i_data_cur,
                        i_idx_cur,
                        reg_new_data,
                        1,
                        0,
                        OE_REPLACE,
                        1,
                        -1,
                    );
                    vdbe_add_op2(&mut v.borrow_mut(), OP_ADDIMM as i32, reg_trig_cnt, 1); // incrementa o contador
                    n_replace_trig += 1;
                } else {
                    debug_assert!(has_rowid(&tab));
                    // Este OP_Delete só dispara o pre-update-hook. Não modifica a b-tree. É mais
                    // eficiente deixar o OP_Insert seguinte substituir a entrada existente do que
                    // apagá-la e inserir outra.
                    vdbe_add_op2(&mut v.borrow_mut(), OP_DELETE as i32, i_data_cur, OPFLAG_ISNOOP as i32);
                    vdbe_append_p4(&mut v.borrow_mut(), P4Union::Table(p_tab.clone()), P4_TABLE as i32);
                    if tab.p_index.is_some() {
                        multi_write(p_parse);
                        generate_row_index_delete(p_parse, p_tab, i_data_cur, i_idx_cur, None, -1);
                    }
                }
                seen_replace = 1;
            }
            OE_UPDATE | OE_IGNORE => {
                if on_error == OE_UPDATE {
                    upsert_do_update(p_parse, p_upsert.as_ref().unwrap(), p_tab, None, i_data_cur);
                    // queda deliberada para OE_Ignore
                }
                vdbe_goto(&mut v.borrow_mut(), ignore_dest);
            }
            _ => {
                on_error = OE_ABORT;
                rowid_constraint(p_parse, on_error, p_tab);
            }
        }
        vdbe_resolve_label(&mut v.borrow_mut(), addr_rowid_ok);
        if p_upsert.is_some() && !upsert_is(&p_upsert_clause, p_upsert.as_ref().unwrap()) {
            upsert_ipk_return = vdbe_add_op0(&mut v.borrow_mut(), OP_GOTO as i32);
        } else if ipk_top != 0 {
            ipk_bottom = vdbe_add_op0(&mut v.borrow_mut(), OP_GOTO as i32);
            vdbe_jump_here(&mut v.borrow_mut(), ipk_top - 1);
        }
    }

    // Testa todas as restrições UNIQUE criando entradas para cada índice UNIQUE e garantindo que
    // entradas duplicadas ainda não existem. Calcula as entradas de registro revisadas dos índices
    // pelo caminho.
    //
    // Este laço também trata o caso do índice PRIMARY KEY de uma tabela WITHOUT ROWID.
    let mut ix: i32 = 0; // Contador do laço de índices
    let mut p_idx_next = index_iterator_first(&s_idx_iter, &mut ix);
    loop {
        let p_idx_ref = match p_idx_next {
            Some(x) => x,
            None => break,
        };
        // O `continue` do C vira `break 'body`: o avanço do iterador fica depois do bloco.
        'body: {
            let p_idx = p_idx_ref.borrow();
            let reg_idx: i32; // Faixa de registros com o conteúdo de p_idx
            let reg_r: i32; // Faixa de registros com a PK conflitante
            let addr_unique_ok: i32; // Pula aqui se a restrição UNIQUE está satisfeita
            let addr_conflict_ck: i32; // Primeiro opcode da lógica de checagem de conflito

            if a_reg_idx[ix as usize] == 0 {
                break 'body; // Pula índices que não mudam
            }
            if let Some(ups) = p_upsert.as_ref() {
                p_upsert_clause = upsert_of_index(ups, Some(&p_idx_ref));
                if upsert_ipk_delay != 0 && upsert_is(&p_upsert_clause, ups) {
                    vdbe_jump_here(&mut v.borrow_mut(), upsert_ipk_delay);
                }
            }
            addr_unique_ok = vdbe_make_label(p_parse);
            if !b_affinity_done {
                table_affinity(&mut v.borrow_mut(), &tab, reg_new_data + 1);
                b_affinity_done = true;
            }
            let i_this_cur = i_idx_cur + ix; // Cursor deste índice UNIQUE

            // Pula índices parciais cuja cláusula WHERE não é verdadeira
            if p_idx.p_part_idx_where.is_some() {
                vdbe_add_op2(&mut v.borrow_mut(), OP_NULL as i32, 0, a_reg_idx[ix as usize]);
                p_parse.i_self_tab = -(reg_new_data + 1);
                expr_if_false_dup(
                    p_parse,
                    p_idx.p_part_idx_where.as_deref(),
                    addr_unique_ok,
                    SQLITE_JUMPIFNULL,
                );
                p_parse.i_self_tab = 0;
            }

            // Cria um registro para esta entrada de índice como ela deve aparecer depois do
            // INSERT ou UPDATE. Guarda esse registro no registro `a_reg_idx[ix]`
            reg_idx = a_reg_idx[ix as usize] + 1;
            for i in 0..p_idx.n_column as i32 {
                let i_field = p_idx.ai_column[i as usize] as i32;
                if i_field == XN_EXPR {
                    p_parse.i_self_tab = -(reg_new_data + 1);
                    expr_code_copy(
                        p_parse,
                        p_idx.a_col_expr.as_ref().unwrap().a[i as usize].p_expr.as_deref(),
                        reg_idx + i,
                    );
                    p_parse.i_self_tab = 0;
                } else if i_field == XN_ROWID || i_field == tab.i_p_key as i32 {
                    let x = reg_new_data;
                    vdbe_add_op2(&mut v.borrow_mut(), OP_INTCOPY as i32, x, reg_idx + i);
                } else {
                    let x = table_column_to_storage(&tab, i_field) + reg_new_data + 1;
                    vdbe_add_op2(&mut v.borrow_mut(), OP_SCOPY as i32, x, reg_idx + i);
                }
            }
            vdbe_add_op3(
                &mut v.borrow_mut(),
                OP_MAKERECORD as i32,
                reg_idx,
                p_idx.n_column as i32,
                a_reg_idx[ix as usize],
            );

            // Num UPDATE, se este índice é o índice PRIMARY KEY de uma tabela WITHOUT ROWID e a
            // chave primária não mudou, nenhuma colisão é possível. Toda a lógica de detecção de
            // colisão abaixo pode ser pulada.
            if is_update && index_is(&p_pk, &p_idx_ref) && pk_chng == 0 {
                vdbe_resolve_label(&mut v.borrow_mut(), addr_unique_ok);
                break 'body;
            }

            // Descobre que ação tomar se há um conflito de unicidade
            on_error = p_idx.on_error as i32;
            if on_error == OE_NONE {
                vdbe_resolve_label(&mut v.borrow_mut(), addr_unique_ok);
                break 'body; // p_idx não é um índice UNIQUE
            }
            if override_error != OE_DEFAULT {
                on_error = override_error;
            } else if on_error == OE_DEFAULT {
                on_error = OE_ABORT;
            }

            // Descobre se a cláusula upsert se aplica a este índice
            if let Some(clause) = p_upsert_clause.as_ref() {
                if clause.borrow().is_do_update == 0 {
                    on_error = OE_IGNORE; // DO NOTHING é o mesmo que INSERT OR IGNORE
                } else {
                    on_error = OE_UPDATE; // DO UPDATE
                }
            }

            // (O atalho que omitia a detecção de colisão para REPLACE no índice PRIMARY KEY único
            // de uma WITHOUT ROWID não existe em compilações com SQLITE_ENABLE_PREUPDATE_HOOK,
            // pois a linha precisa ser apagada explicitamente para o hook de pré-update rodar.)
            debug_assert!(is_ordinary_table(&tab));

            // Vê se a nova entrada de índice será única
            addr_conflict_ck = vdbe_add_op4_int(
                &mut v.borrow_mut(),
                OP_NOCONFLICT as i32,
                i_this_cur,
                addr_unique_ok,
                reg_idx,
                p_idx.n_key_col as i32,
            );

            // Gera o código que trata colisões
            reg_r = if index_is(&p_pk, &p_idx_ref) {
                reg_idx
            } else {
                get_temp_range(p_parse, n_pk_field)
            };
            if is_update || on_error == OE_REPLACE {
                if has_rowid(&tab) {
                    vdbe_add_op2(&mut v.borrow_mut(), OP_IDXROWID as i32, i_this_cur, reg_r);
                    // Só há conflito se o rowid da entrada de índice existente é diferente do
                    // rowid antigo
                    if is_update {
                        vdbe_add_op3(
                            &mut v.borrow_mut(),
                            OP_EQ as i32,
                            reg_r,
                            addr_unique_ok,
                            reg_old_data,
                        );
                        vdbe_change_p5(&mut v.borrow_mut(), SQLITE_NOTNULL as u16);
                    }
                } else {
                    let p_pk_ref = p_pk.as_ref().unwrap();
                    let pk = p_pk_ref.borrow();
                    // Extrai a PRIMARY KEY do fim da entrada de índice e a guarda nos registros
                    // reg_r..reg_r+nPk-1
                    if !Rc::ptr_eq(p_pk_ref, &p_idx_ref) {
                        for i in 0..pk.n_key_col as i32 {
                            debug_assert!(pk.ai_column[i as usize] >= 0);
                            let x = table_column_to_index(&p_idx, pk.ai_column[i as usize] as i32) as i32;
                            vdbe_add_op3(
                                &mut v.borrow_mut(),
                                OP_COLUMN as i32,
                                i_this_cur,
                                x,
                                reg_r + i,
                            );
                        }
                    }
                    if is_update {
                        // Se está processando a PRIMARY KEY de uma tabela WITHOUT ROWID, só há
                        // conflito se os novos valores da PRIMARY KEY são de fato diferentes dos
                        // antigos. Ver TH3 withoutrowid04.test.
                        //
                        // Para um índice UNIQUE, só há conflito se os valores da PRIMARY KEY da
                        // linha de índice encontrada são diferentes dos valores originais da
                        // PRIMARY KEY desta linha antes do UPDATE.
                        let mut addr_jump = vdbe_current_addr(&v.borrow()) + pk.n_key_col as i32;
                        let mut op: i32 = OP_NE as i32;
                        let reg_cmp = if is_primary_key_index(&p_idx) { reg_idx } else { reg_r };
                        for i in 0..pk.n_key_col as i32 {
                            let p4 = locate_coll_seq(p_parse, &pk.az_coll[i as usize]);
                            let mut x = pk.ai_column[i as usize] as i32;
                            debug_assert!(x >= 0);
                            if i == (pk.n_key_col as i32 - 1) {
                                addr_jump = addr_unique_ok;
                                op = OP_EQ as i32;
                            }
                            x = table_column_to_storage(&tab, x);
                            vdbe_add_op4(
                                &mut v.borrow_mut(),
                                op,
                                reg_old_data + 1 + x,
                                addr_jump,
                                reg_cmp + i,
                                P4Union::CollSeq(p4),
                                P4_COLLSEQ as i32,
                            );
                            vdbe_change_p5(&mut v.borrow_mut(), SQLITE_NOTNULL as u16);
                        }
                    }
                }
            }

            // Gera o código que executa se a nova entrada de índice não é única
            debug_assert!(
                on_error == OE_ROLLBACK
                    || on_error == OE_ABORT
                    || on_error == OE_FAIL
                    || on_error == OE_IGNORE
                    || on_error == OE_REPLACE
                    || on_error == OE_UPDATE
            );
            match on_error {
                OE_ROLLBACK | OE_ABORT | OE_FAIL => {
                    unique_constraint(p_parse, on_error, &p_idx_ref);
                }
                OE_UPDATE | OE_IGNORE => {
                    if on_error == OE_UPDATE {
                        upsert_do_update(
                            p_parse,
                            p_upsert.as_ref().unwrap(),
                            p_tab,
                            Some(&p_idx_ref),
                            i_idx_cur + ix,
                        );
                        // queda deliberada para OE_Ignore
                    }
                    vdbe_goto(&mut v.borrow_mut(), ignore_dest);
                }
                _ => {
                    debug_assert!(on_error == OE_REPLACE);
                    // Número de opcodes na lógica de checagem de conflito
                    let mut n_conflict_ck = vdbe_current_addr(&v.borrow()) - addr_conflict_ck;
                    let mut addr_conflict_ck = addr_conflict_ck;
                    if reg_trig_cnt != 0 {
                        multi_write(p_parse);
                        n_replace_trig += 1;
                    }
                    if p_trigger.is_some() && is_update {
                        vdbe_add_op1(&mut v.borrow_mut(), OP_CURSORLOCK as i32, i_data_cur);
                    }
                    generate_row_delete(
                        p_parse,
                        p_tab,
                        p_trigger.as_ref(),
                        i_data_cur,
                        i_idx_cur,
                        reg_r,
                        n_pk_field,
                        0,
                        OE_REPLACE,
                        if index_is(&p_pk, &p_idx_ref) { ONEPASS_SINGLE } else { ONEPASS_OFF },
                        i_this_cur,
                    );
                    if p_trigger.is_some() && is_update {
                        vdbe_add_op1(&mut v.borrow_mut(), OP_CURSORUNLOCK as i32, i_data_cur);
                    }
                    if reg_trig_cnt != 0 {
                        vdbe_add_op2(&mut v.borrow_mut(), OP_ADDIMM as i32, reg_trig_cnt, 1); // incrementa o contador
                        let addr_bypass = vdbe_add_op0(&mut v.borrow_mut(), OP_GOTO as i32); // Contorna a reverificação

                        // Aqui entra o código que será invocado depois de todas as verificações
                        // de restrição terem rodado, se e só se um ou mais replace triggers
                        // dispararam.
                        vdbe_resolve_label(&mut v.borrow_mut(), lbl_recheck_ok);
                        lbl_recheck_ok = vdbe_make_label(p_parse);
                        if p_idx.p_part_idx_where.is_some() {
                            // Contorna a reverificação se este índice parcial não está definido
                            // para a linha atual
                            vdbe_add_op2(
                                &mut v.borrow_mut(),
                                OP_ISNULL as i32,
                                reg_idx - 1,
                                lbl_recheck_ok,
                            );
                        }
                        // Copia o código de checagem de restrição de cima, mas troca o destino
                        // de salto "restrição ok" para o endereço do próximo bloco de reteste
                        while n_conflict_ck > 0 {
                            // A chamada de `vdbe_add_op4` pode realocar o vetor de opcodes. Por
                            // isso se faz uma cópia completa do opcode, e não uma referência.
                            let x = vdbe_get_op(&v.borrow(), addr_conflict_ck).clone();
                            if x.opcode != OP_IDXROWID {
                                // Novo valor de P2 do opcode copiado
                                let p2 = if (OPCODE_PROPERTY[x.opcode as usize] & OPFLG_JUMP) != 0 {
                                    lbl_recheck_ok
                                } else {
                                    x.p2
                                };
                                // No C, `zP4` escolhia entre `p4.i` (P4_INT32) e `p4.z`: a união
                                // `P4Union` já carrega as duas variantes, então clona-se inteira.
                                vdbe_add_op4(
                                    &mut v.borrow_mut(),
                                    x.opcode as i32,
                                    x.p1,
                                    p2,
                                    x.p3,
                                    x.p4.clone(),
                                    x.p4type as i32,
                                );
                                vdbe_change_p5(&mut v.borrow_mut(), x.p5);
                            }
                            n_conflict_ck -= 1;
                            addr_conflict_ck += 1;
                        }
                        // Se o reteste falha, emite um abort
                        unique_constraint(p_parse, OE_ABORT, &p_idx_ref);

                        vdbe_jump_here(&mut v.borrow_mut(), addr_bypass); // Termina o contorno da reverificação
                    }
                    seen_replace = 1;
                }
            }
            vdbe_resolve_label(&mut v.borrow_mut(), addr_unique_ok);
            if reg_r != reg_idx {
                release_temp_range(p_parse, reg_r, n_pk_field);
            }
            if let Some(clause) = p_upsert_clause.as_ref() {
                if upsert_ipk_return != 0 && upsert_next_is_ipk(clause) {
                    vdbe_goto(&mut v.borrow_mut(), upsert_ipk_delay + 1);
                    vdbe_jump_here(&mut v.borrow_mut(), upsert_ipk_return);
                    upsert_ipk_return = 0;
                }
            }
        }
        p_idx_next = index_iterator_next(&mut s_idx_iter, &mut ix);
    }

    // Se a restrição do IPK é um REPLACE, roda por último
    if ipk_top != 0 {
        vdbe_goto(&mut v.borrow_mut(), ipk_top);
        debug_assert!(ipk_bottom > 0);
        vdbe_jump_here(&mut v.borrow_mut(), ipk_bottom);
    }

    // Reverifica todas as restrições de unicidade depois de os replace triggers rodarem
    debug_assert!(reg_trig_cnt != 0 || n_replace_trig == 0);
    if n_replace_trig != 0 {
        vdbe_add_op2(&mut v.borrow_mut(), OP_IFNOT as i32, reg_trig_cnt, lbl_recheck_ok);
        if p_pk.is_none() {
            if is_update {
                vdbe_add_op3(
                    &mut v.borrow_mut(),
                    OP_EQ as i32,
                    reg_new_data,
                    addr_recheck,
                    reg_old_data,
                );
                vdbe_change_p5(&mut v.borrow_mut(), SQLITE_NOTNULL as u16);
            }
            vdbe_add_op3(
                &mut v.borrow_mut(),
                OP_NOTEXISTS as i32,
                i_data_cur,
                addr_recheck,
                reg_new_data,
            );
            rowid_constraint(p_parse, OE_ABORT, p_tab);
        } else {
            vdbe_goto(&mut v.borrow_mut(), addr_recheck);
        }
        vdbe_resolve_label(&mut v.borrow_mut(), lbl_recheck_ok);
    }

    // Gera o registro da tabela. Aqui `ix` vale o número de índices (o iterador o deixa assim ao
    // acabar), então `a_reg_idx[ix]` é a posição que sobra depois do último índice.
    if has_rowid(&tab) {
        let reg_rec = a_reg_idx[ix as usize];
        vdbe_add_op3(
            &mut v.borrow_mut(),
            OP_MAKERECORD as i32,
            reg_new_data + 1,
            tab.n_nv_col as i32,
            reg_rec,
        );
        if !b_affinity_done {
            table_affinity(&mut v.borrow_mut(), &tab, 0);
        }
    }

    *pb_may_replace = seen_replace;
}


// ---- part_005.rs ----

// Nota de porte: o trecho C 005 é a continuação de `sqlite3GenerateConstraintChecks`, que começa no
// trecho 004. Uma função Rust não atravessa arquivos, então toda a tradução (NOT NULL, CHECK,
// iterador de índices, upsert, replace triggers e a checagem do rowid) está em
// `generate_constraint_checks`, em `part_004.rs`. Esta parte não tem itens.


// ---- part_006.rs ----

// Nota de porte: o trecho C 006 é o fim de `sqlite3GenerateConstraintChecks` (fim do switch do
// rowid, laço dos índices UNIQUE, reverificação após replace triggers e o registro da tabela).
// Toda a tradução está em `generate_constraint_checks`, em `part_004.rs`, porque uma função Rust
// não atravessa arquivos. Esta parte não tem itens.


// ---- part_007.rs ----

// Nota de porte: sqlite3SetMakeRecordP5 só existe sob SQLITE_ENABLE_NULL_TRIM, que o
// Debian 13 não liga, então a função some. SQLITE_ENABLE_PREUPDATE_HOOK está ligado, então só
// vale o ramo com o hook. SQLITE_TEST, SQLITE_DEBUG (VdbeCoverage, VdbeComment, testcase) e
// SQLITE_ENABLE_HIDDEN_COLUMNS não existem na compilação do Debian e somem.

/// A tabela `p_tab` é uma tabela WITHOUT ROWID que está sendo escrita. O número do cursor é
/// `i_cur` e o registro `reg_data` contém o novo registro do índice da chave primária. Esta
/// função acrescenta o código que invoca o hook de pré-atualização, se houver um registrado.
fn code_without_rowid_preupdate(
    p_parse: &mut Parse,
    p_tab: &TableRef,
    i_cur: i32,
    reg_data: i32,
) {
    let v = p_parse.p_vdbe.clone().unwrap();
    let r = get_temp_reg(p_parse);
    assert!(!has_rowid(&p_tab.borrow()));
    vdbe_add_op2(&mut v.borrow_mut(), OP_INTEGER as i32, 0, r);
    vdbe_add_op4(
        &mut v.borrow_mut(),
        OP_INSERT as i32,
        i_cur,
        reg_data,
        r,
        P4Union::Table(p_tab.clone()),
        P4_TABLE,
    );
    vdbe_change_p5(&mut v.borrow_mut(), OPFLAG_ISNOOP as u16);
    release_temp_reg(p_parse, r);
}

/// Esta rotina gera o código que termina a operação INSERT ou UPDATE iniciada por uma chamada
/// anterior a `generate_constraint_checks`. Um intervalo consecutivo de registros que começa em
/// `reg_new_data` contém o rowid e o conteúdo a inserir.
///
/// Os argumentos desta rotina devem ser os mesmos dos seis primeiros argumentos de
/// `generate_constraint_checks`.
pub fn complete_insertion(
    p_parse: &mut Parse,
    p_tab: &TableRef,       // A tabela em que se insere
    i_data_cur: i32,        // Cursor da fonte canônica de dados
    i_idx_cur: i32,         // Primeiro cursor de índice
    reg_new_data: i32,      // Intervalo do conteúdo
    a_reg_idx: &[i32],      // Registro usado por cada índice. 0 para índices não usados
    update_flags: u8,       // Verdadeiro para UPDATE, falso para INSERT
    append_bias: i32,       // Verdadeiro se provavelmente é um append
    use_seek_result: i32,   // Liga USESEEKRESULT em OP_[Idx]Insert
) {
    assert!(
        update_flags == 0
            || update_flags == OPFLAG_ISUPDATE
            || update_flags == (OPFLAG_ISUPDATE | OPFLAG_SAVEPOSITION)
    );

    let v = p_parse.p_vdbe.clone().unwrap();
    assert!(!is_view(&p_tab.borrow())); // Esta tabela não é uma VIEW
    let mut i: usize = 0;
    let mut p_idx_opt = p_tab.borrow().p_index.clone();
    while let Some(p_idx_ref) = p_idx_opt {
        // Todos os índices REPLACE ficam no fim da lista
        assert!(
            p_idx_ref.borrow().on_error != OE_REPLACE
                || p_idx_ref.borrow().p_next.is_none()
                || p_idx_ref.borrow().p_next.as_ref().unwrap().borrow().on_error == OE_REPLACE
        );
        if a_reg_idx[i] != 0 {
            if p_idx_ref.borrow().p_part_idx_where.is_some() {
                let addr = vdbe_current_addr(&v.borrow()) + 2;
                vdbe_add_op2(&mut v.borrow_mut(), OP_ISNULL as i32, a_reg_idx[i], addr);
            }
            let mut pik_flags: u8 = if use_seek_result != 0 {
                OPFLAG_USESEEKRESULT
            } else {
                0
            };
            if is_primary_key_index(&p_idx_ref.borrow()) && !has_rowid(&p_tab.borrow()) {
                pik_flags |= OPFLAG_NCHANGE;
                pik_flags |= update_flags & OPFLAG_SAVEPOSITION;
                if update_flags == 0 {
                    code_without_rowid_preupdate(p_parse, p_tab, i_idx_cur + i as i32, a_reg_idx[i]);
                }
            }
            let n_p4 = {
                let p_idx = p_idx_ref.borrow();
                if p_idx.uniq_not_null {
                    p_idx.n_key_col
                } else {
                    p_idx.n_column
                }
            };
            vdbe_add_op4_int(
                &mut v.borrow_mut(),
                OP_IDXINSERT as i32,
                i_idx_cur + i as i32,
                a_reg_idx[i],
                a_reg_idx[i] + 1,
                n_p4 as i32,
            );
            vdbe_change_p5(&mut v.borrow_mut(), pik_flags as u16);
        }
        let p_next = p_idx_ref.borrow().p_next.clone();
        p_idx_opt = p_next;
        i += 1;
    }
    if !has_rowid(&p_tab.borrow()) {
        return;
    }
    let mut pik_flags: u8;
    if p_parse.nested != 0 {
        pik_flags = 0;
    } else {
        pik_flags = OPFLAG_NCHANGE;
        pik_flags |= if update_flags != 0 {
            update_flags
        } else {
            OPFLAG_LASTROWID
        };
    }
    if append_bias != 0 {
        pik_flags |= OPFLAG_APPEND;
    }
    if use_seek_result != 0 {
        pik_flags |= OPFLAG_USESEEKRESULT;
    }
    vdbe_add_op3(
        &mut v.borrow_mut(),
        OP_INSERT as i32,
        i_data_cur,
        a_reg_idx[i],
        reg_new_data,
    );
    if p_parse.nested == 0 {
        vdbe_append_p4(&mut v.borrow_mut(), P4Union::Table(p_tab.clone()), P4_TABLE as i32);
    }
    vdbe_change_p5(&mut v.borrow_mut(), pik_flags as u16);
}

/// Aloca cursores para a tabela `p_tab` e todos os seus índices e gera o código que abre e
/// inicializa esses cursores.
///
/// O cursor do objeto que contém os dados completos (normalmente a própria tabela, mas o índice
/// PRIMARY KEY no caso de uma tabela WITHOUT ROWID) volta em `pi_data_cur`. O primeiro cursor de
/// índice volta em `pi_idx_cur`. O número de índices é o valor devolvido.
///
/// Usa `i_base` como primeiro cursor (o `pi_data_cur` das tabelas com rowid, ou o primeiro
/// índice das tabelas WITHOUT ROWID) se ele não for negativo. Se `i_base` for negativo, aloca o
/// próximo cursor disponível.
///
/// Numa tabela com rowid, `pi_data_cur` fica exatamente um abaixo de `pi_idx_cur`. Numa tabela
/// WITHOUT ROWID, `pi_data_cur` fica em algum ponto da faixa de `pi_idx_cur`, conforme o lugar
/// do índice PRIMARY KEY na lista `p_tab.p_index`.
///
/// Se `p_tab` é uma tabela virtual, esta rotina não faz nada e `pi_data_cur` e `pi_idx_cur`
/// ficam sem inicializar.
pub fn open_table_and_indices(
    p_parse: &mut Parse,
    p_tab: &TableRef,           // Tabela a abrir
    op: i32,                    // OP_OpenRead ou OP_OpenWrite
    mut p5: u8,                 // Valor de P5 dos opcodes Open* (exceto em WITHOUT ROWID)
    mut i_base: i32,            // Usa este para o cursor da tabela, se houver
    a_to_open: Option<&[u8]>,   // Se presente: booleano para cada tabela e índice
    pi_data_cur: &mut i32,      // Escreve aqui o cursor da fonte de dados
    pi_idx_cur: &mut i32,       // Escreve aqui o primeiro cursor de índice
) -> i32 {
    assert!(op == OP_OPENREAD as i32 || op == OP_OPENWRITE as i32);
    assert!(op == OP_OPENWRITE as i32 || p5 == 0);
    if is_virtual(&p_tab.borrow()) {
        // Esta rotina não faz nada para tabelas virtuais. Deixa as variáveis de saída
        // *piDataCur e *piIdxCur com números de cursor ilegais, para detectar erro melhor.
        *pi_data_cur = -999;
        *pi_idx_cur = -999;
        return 0;
    }
    let db = p_parse.db.upgrade().unwrap();
    let i_db = {
        let tab = p_tab.borrow();
        let p_schema = tab.p_schema.as_ref().and_then(|w| w.upgrade());
        let schema_guard = p_schema.as_ref().map(|s| s.borrow());
        schema_to_index(&db.borrow(), schema_guard.as_deref())
    };
    let v = p_parse.p_vdbe.clone().unwrap();
    if i_base < 0 {
        i_base = p_parse.n_tab;
    }
    let i_data_cur = i_base;
    i_base += 1;
    *pi_data_cur = i_data_cur;
    if has_rowid(&p_tab.borrow()) && (a_to_open.is_none() || a_to_open.unwrap()[0] != 0) {
        open_table(p_parse, i_data_cur, i_db, &p_tab.borrow(), op);
    } else if db.borrow().no_shared_cache == 0 {
        let (tnum, z_name) = {
            let tab = p_tab.borrow();
            (tab.tnum, tab.z_name.clone())
        };
        table_lock(
            p_parse,
            i_db,
            tnum,
            (op == OP_OPENWRITE as i32) as u8,
            Some(z_name),
        );
    }
    *pi_idx_cur = i_base;
    let mut i: usize = 0;
    let mut p_idx_opt = p_tab.borrow().p_index.clone();
    while let Some(p_idx_ref) = p_idx_opt {
        let i_idx_cur = i_base;
        i_base += 1;
        assert!(xfer_same_schema(
            &p_idx_ref.borrow().p_schema,
            &p_tab.borrow().p_schema
        ));
        if is_primary_key_index(&p_idx_ref.borrow()) && !has_rowid(&p_tab.borrow()) {
            *pi_data_cur = i_idx_cur;
            p5 = 0;
        }
        if a_to_open.is_none() || a_to_open.unwrap()[i + 1] != 0 {
            let tnum = p_idx_ref.borrow().tnum;
            vdbe_add_op3(&mut v.borrow_mut(), op, i_idx_cur, tnum as i32, i_db);
            vdbe_set_p4_key_info(p_parse, &p_idx_ref);
            vdbe_change_p5(&mut v.borrow_mut(), p5 as u16);
        }
        let p_next = p_idx_ref.borrow().p_next.clone();
        p_idx_opt = p_next;
        i += 1;
    }
    if i_base > p_parse.n_tab {
        p_parse.n_tab = i_base;
    }
    i as i32
}

/// Verdadeiro se os dois schemas são o mesmo objeto (ou ambos ausentes). Equivale à comparação
/// de ponteiros `pSrc->pSchema==pDest->pSchema` do C.
fn xfer_same_schema(
    p_a: &Option<Weak<RefCell<Schema>>>,
    p_b: &Option<Weak<RefCell<Schema>>>,
) -> bool {
    match (p_a, p_b) {
        (None, None) => true,
        (Some(a), Some(b)) => Weak::ptr_eq(a, b),
        _ => false,
    }
}

/// Verifica se o índice `p_src` é compatível como fonte de dados do índice `p_dest` numa
/// otimização de transferência de INSERT. As regras de um índice compatível:
///
///    *   O índice é sobre o mesmo conjunto de colunas
///    *   As mesmas marcações DESC e ASC ocorrem em todas as colunas
///    *   O mesmo tratamento de onError (OE_Abort, OE_Ignore etc)
///    *   A mesma sequência de ordenação em cada coluna
///    *   O índice tem exatamente a mesma cláusula WHERE
fn xfer_compatible_index(p_dest: &Index, p_src: &Index) -> bool {
    assert!(!Weak::ptr_eq(&p_dest.p_table, &p_src.p_table));
    if p_dest.n_key_col != p_src.n_key_col || p_dest.n_column != p_src.n_column {
        return false; // Número de colunas diferente
    }
    if p_dest.on_error != p_src.on_error {
        return false; // Estratégias de resolução de conflito diferentes
    }
    for i in 0..p_src.n_key_col as usize {
        if p_src.ai_column[i] != p_dest.ai_column[i] {
            return false; // Colunas indexadas diferentes
        }
        if p_src.ai_column[i] == XN_EXPR {
            assert!(p_src.a_col_expr.is_some() && p_dest.a_col_expr.is_some());
            if expr_compare(
                None,
                p_src.a_col_expr.as_ref().unwrap().a[i].p_expr.as_deref(),
                p_dest.a_col_expr.as_ref().unwrap().a[i].p_expr.as_deref(),
                -1,
            ) != 0
            {
                return false; // Expressões diferentes no índice
            }
        }
        if p_src.a_sort_order[i] != p_dest.a_sort_order[i] {
            return false; // Ordens de classificação diferentes
        }
        if stricmp(Some(&p_src.az_coll[i][..]), Some(&p_dest.az_coll[i][..])) != 0 {
            return false; // Sequências de ordenação diferentes
        }
    }
    if expr_compare(
        None,
        p_src.p_part_idx_where.as_deref(),
        p_dest.p_part_idx_where.as_deref(),
        -1,
    ) != 0
    {
        return false; // Cláusulas WHERE diferentes
    }

    // Se nenhum teste acima falha, os índices são compatíveis
    true
}

/// Acha, na lista de índices de `p_src`, o primeiro índice compatível com `p_dest_idx` (ver
/// `xfer_compatible_index`). É o laço `for(pSrcIdx=pSrc->pIndex; ...)` que aparece duas vezes em
/// xferOptimization.
fn xfer_find_src_index(p_dest_idx: &Index, p_src: &Table) -> Option<IndexRef> {
    let mut p_src_idx = p_src.p_index.clone();
    while let Some(p_src_idx_ref) = p_src_idx {
        if xfer_compatible_index(p_dest_idx, &p_src_idx_ref.borrow()) {
            return Some(p_src_idx_ref);
        }
        let p_next = p_src_idx_ref.borrow().p_next.clone();
        p_src_idx = p_next;
    }
    None
}

/// Tenta a otimização de transferência em INSERTs da forma
///
///     INSERT INTO tab1 SELECT * FROM tab2;
///
/// A otimização de transferência passa os registros brutos de tab2 para tab1. As colunas não são
/// decodificadas e remontadas, o que melhora muito o desempenho. Os registros brutos de índice
/// são transferidos do mesmo jeito.
///
/// A otimização só é tentada se tab1 e tab2 são compatíveis. Há muitas regras de compatibilidade;
/// veja os comentários no código.
///
/// Esta rotina devolve verdadeiro se a otimização com certeza será usada. Às vezes ela só funciona
/// se a tabela de destino está vazia, fato que só se sabe na execução. Nesse caso a rotina gera o
/// código da otimização e também um teste de destino vazio que pula o código da otimização se o
/// teste falha, e devolve falso, para que o chamador gere uma transferência sem otimização. A
/// rotina também devolve falso se não há chance de a otimização se aplicar.
///
/// Esta otimização é particularmente útil para fazer o VACUUM rodar mais rápido.
fn xfer_optimization(
    p_parse: &mut Parse,
    p_dest: &TableRef,      // A tabela em que se insere
    p_select: &Select,      // O SELECT usado como fonte de dados
    mut on_error: i32,      // Como tratar erros de restrição
    i_db_dest: i32,         // O banco de p_dest
) -> i32 {
    let db = p_parse.db.upgrade().unwrap();
    let mut addr1: i32;
    let mut addr2: i32;
    let mut empty_dest_test: i32 = 0; // Endereço do teste de p_dest vazio
    let mut empty_src_test: i32 = 0; // Endereço do teste de p_src vazio
    let mut dest_has_unique_idx = false; // Verdadeiro se p_dest tem índice UNIQUE

    if p_parse.p_with.is_some() || p_select.p_with.is_some() {
        // Não tenta processar esta consulta se há cláusulas WITH ligadas a ela. Prosseguir pode
        // gerar um falso erro "no such table: xxx" se o SELECT lê de uma CTE chamada "xxx".
        return 0;
    }
    if is_virtual(&p_dest.borrow()) {
        return 0; // tab1 não pode ser tabela virtual
    }
    if on_error == OE_DEFAULT as i32 {
        if p_dest.borrow().i_p_key >= 0 {
            on_error = p_dest.borrow().key_conf as i32;
        }
        if on_error == OE_DEFAULT as i32 {
            on_error = OE_ABORT as i32;
        }
    }
    assert!(p_select.p_src.is_some()); // alocado mesmo sem cláusula FROM
    let p_select_src = p_select.p_src.as_ref().unwrap();
    if p_select_src.n_src != 1 {
        return 0; // A cláusula FROM deve ter exatamente um termo
    }
    if p_select_src.a[0].p_select.is_some() {
        return 0; // A cláusula FROM não pode conter subconsulta
    }
    if p_select.p_where.is_some() {
        return 0; // O SELECT não pode ter cláusula WHERE
    }
    if p_select.p_order_by.is_some() {
        return 0; // O SELECT não pode ter cláusula ORDER BY
    }
    // Não precisa testar a cláusula HAVING. Se HAVING existe mas não há ORDER BY, sai erro.
    if p_select.p_group_by.is_some() {
        return 0; // O SELECT não pode ter cláusula GROUP BY
    }
    if p_select.p_limit.is_some() {
        return 0; // O SELECT não pode ter cláusula LIMIT
    }
    if p_select.p_prior.is_some() {
        return 0; // O SELECT não pode ser consulta composta
    }
    if p_select.sel_flags & SF_DISTINCT != 0 {
        return 0; // O SELECT não pode ser DISTINCT
    }
    let p_e_list = p_select.p_elist.as_ref().unwrap();
    if p_e_list.n_expr != 1 {
        return 0; // O conjunto de resultado deve ter exatamente uma coluna
    }
    assert!(p_e_list.a[0].p_expr.is_some());
    if p_e_list.a[0].p_expr.as_ref().unwrap().op != TK_ASTERISK {
        return 0; // O conjunto de resultado deve ser o operador especial "*"
    }

    // Neste ponto está estabelecido que a instrução tem a forma sintática correta para
    // participar da otimização. Agora é preciso conferir a semântica.
    let p_item = &p_select_src.a[0];
    let p_src_ref = match locate_table_item(p_parse, 0, p_item) {
        Some(t) => t,
        None => return 0, // A cláusula FROM não contém uma tabela de verdade
    };
    let p_dest_b = p_dest.borrow();
    let p_src = p_src_ref.borrow();
    if p_src.tnum == p_dest_b.tnum && xfer_same_schema(&p_src.p_schema, &p_dest_b.p_schema) {
        return 0; // tab1 e tab2 não podem ser a mesma tabela
    }
    if has_rowid(&p_dest_b) != has_rowid(&p_src) {
        return 0; // origem e destino devem ser ambos WITHOUT ROWID ou ambos não
    }
    if !is_ordinary_table(&p_src) {
        return 0; // tab2 não pode ser view nem tabela virtual
    }
    if p_dest_b.n_col != p_src.n_col {
        return 0; // O número de colunas deve ser igual em tab1 e tab2
    }
    if p_dest_b.i_p_key != p_src.i_p_key {
        return 0; // As duas tabelas devem ter o mesmo INTEGER PRIMARY KEY
    }
    if (p_dest_b.tab_flags & TF_STRICT) != 0 && (p_src.tab_flags & TF_STRICT) == 0 {
        return 0; // Não se alimenta tabela STRICT a partir de uma não STRICT
    }
    for i in 0..p_dest_b.n_col as usize {
        let p_dest_col = &p_dest_b.a_col[i];
        let p_src_col = &p_src.a_col[i];
        // Mesmo que as tabelas t1 e t2 tenham schemas idênticos, se contêm colunas geradas esta
        // instrução é semanticamente incorreta:
        //
        //     INSERT INTO t2 SELECT * FROM t1;
        //
        // O motivo é que os valores das colunas geradas voltam do SELECT à direita, mas o INSERT
        // à esquerda quer que sejam omitidos.
        //
        // Mesmo assim, esta é uma notação abreviada útil para mandar o SQLite fazer uma
        // transferência em bloco de todo o conteúdo de t1 para t2.
        //
        // Em teoria dava para desligar isto (exceto para o uso interno do VACUUM, onde é
        // necessário). Mas para quê? Parece inofensivo e presta um serviço útil.
        if (p_dest_col.col_flags & COLFLAG_GENERATED) != (p_src_col.col_flags & COLFLAG_GENERATED) {
            return 0; // As duas colunas têm o mesmo tipo de coluna gerada
        }
        // Mas a transferência só é permitida se a origem e o destino têm exatamente as mesmas
        // expressões nas colunas geradas. Esta exigência poderia ser relaxada para colunas
        // VIRTUAL, suponho.
        if (p_dest_col.col_flags & COLFLAG_GENERATED) != 0 {
            if expr_compare(
                None,
                column_expr(&p_src, p_src_col),
                column_expr(&p_dest_b, p_dest_col),
                -1,
            ) != 0
            {
                return 0; // Expressões geradoras diferentes
            }
        }
        if p_dest_col.affinity != p_src_col.affinity {
            return 0; // A afinidade deve ser a mesma em todas as colunas
        }
        if stricmp(column_coll(p_dest_col), column_coll(p_src_col)) != 0 {
            return 0; // A sequência de ordenação deve ser a mesma em todas as colunas
        }
        if p_dest_col.not_null != 0 && p_src_col.not_null == 0 {
            return 0; // tab2 deve ser NOT NULL se tab1 é
        }
        // Os valores default da segunda coluna em diante precisam coincidir.
        if (p_dest_col.col_flags & COLFLAG_GENERATED) == 0 && i > 0 {
            let p_dest_expr = column_expr(&p_dest_b, p_dest_col);
            let p_src_expr = column_expr(&p_src, p_src_col);
            assert!(p_dest_expr.map_or(true, |e| e.op == TK_SPAN));
            assert!(p_dest_expr.map_or(true, |e| !expr_has_property(e, EP_INTVALUE)));
            assert!(p_src_expr.map_or(true, |e| e.op == TK_SPAN));
            assert!(p_src_expr.map_or(true, |e| !expr_has_property(e, EP_INTVALUE)));
            if p_dest_expr.is_none() != p_src_expr.is_none()
                || (p_dest_expr.is_some()
                    && p_dest_expr.unwrap().u.z_token != p_src_expr.unwrap().u.z_token)
            {
                return 0; // Os defaults devem ser iguais em todas as colunas
            }
        }
    }
    let mut p_dest_idx_opt = p_dest_b.p_index.clone();
    while let Some(p_dest_idx_ref) = p_dest_idx_opt {
        let p_dest_idx = p_dest_idx_ref.borrow();
        if is_unique_index(&p_dest_idx) {
            dest_has_unique_idx = true;
        }
        let p_src_idx_ref = match xfer_find_src_index(&p_dest_idx, &p_src) {
            Some(x) => x,
            None => return 0, // p_dest_idx não tem índice correspondente em p_src
        };
        let p_src_idx = p_src_idx_ref.borrow();
        if p_src_idx.tnum == p_dest_idx.tnum
            && xfer_same_schema(&p_src.p_schema, &p_dest_b.p_schema)
            && fault_sim(411) == SQLITE_OK
        {
            // A chamada a fault_sim() permite contornar este teste de corrupção durante os
            // testes, para exercitar outros testes de corrupção mais adiante.
            return 0; // Schema corrompido: dois índices na mesma b-tree
        }
        let p_next = p_dest_idx.p_next.clone();
        drop(p_src_idx);
        drop(p_dest_idx);
        p_dest_idx_opt = p_next;
    }
    let m_db_flags = db.borrow().m_db_flags;
    if p_dest_b.p_check.is_some()
        && (m_db_flags & DBFLAG_VACUUM) == 0
        && expr_list_compare(p_src.p_check.as_deref(), p_dest_b.p_check.as_deref(), -1) != 0
    {
        return 0; // As tabelas têm restrições CHECK diferentes. Ticket #2252
    }
    // Proíbe a otimização de transferência se a tabela de destino contém chaves estrangeiras.
    // Isto é mais restritivo que o necessário. Mas o principal beneficiário da otimização é o
    // comando VACUUM, e o VACUUM desliga as chaves estrangeiras. Então a complicação extra para
    // afrouxar esta regra provavelmente não vale o esforço. Ticket
    // [6284df89debdfa61db8073e062908af0c9b6118e]
    assert!(is_ordinary_table(&p_dest_b));
    if (db.borrow().flags & SQLITE_FOREIGNKEYS) != 0 {
        if let TableU::Tab(t) = &p_dest_b.u {
            if t.p_fkey.is_some() {
                return 0;
            }
        }
    }
    if (db.borrow().flags & SQLITE_COUNTROWS) != 0 {
        return 0; // A transferência não combina com PRAGMA count_changes
    }

    // Chegando aqui, a otimização é pelo menos uma possibilidade, embora possa só funcionar se a
    // tabela de destino (tab1) estiver inicialmente vazia.
    let i_db_src = {
        let p_schema = p_src.p_schema.as_ref().and_then(|w| w.upgrade());
        let schema_guard = p_schema.as_ref().map(|s| s.borrow());
        schema_to_index(&db.borrow(), schema_guard.as_deref())
    };
    let v = get_vdbe(p_parse);
    code_verify_schema(p_parse, i_db_src);
    let i_src = p_parse.n_tab;
    p_parse.n_tab += 1;
    let i_dest = p_parse.n_tab;
    p_parse.n_tab += 1;
    let reg_autoinc = auto_inc_begin(p_parse, i_db_dest, p_dest);
    let reg_data = get_temp_reg(p_parse);
    vdbe_add_op2(&mut v.borrow_mut(), OP_NULL as i32, 0, reg_data);
    let reg_rowid = get_temp_reg(p_parse);
    open_table(p_parse, i_dest, i_db_dest, &p_dest_b, OP_OPENWRITE as i32);
    assert!(has_rowid(&p_dest_b) || dest_has_unique_idx);
    if (m_db_flags & DBFLAG_VACUUM) == 0
        && ((p_dest_b.i_p_key < 0 && p_dest_b.p_index.is_some())          // (1)
            || dest_has_unique_idx                                        // (2)
            || (on_error != OE_ABORT as i32 && on_error != OE_ROLLBACK as i32)) // (3)
    {
        // Em algumas circunstâncias só se pode rodar a otimização de transferência se a tabela
        // de destino está inicialmente vazia. A menos que o flag DBFLAG_Vacuum esteja ligado,
        // este bloco gera o código que faz essa determinação. Se DBFLAG_Vacuum está ligado, a
        // tabela de destino sempre está vazia.
        //
        // Condições em que o destino deve estar vazio:
        //
        // (1) Não há INTEGER PRIMARY KEY mas há índices. (Se o destino não está inicialmente
        //     vazio, os campos rowid das entradas de índice talvez precisem mudar.)
        //
        // (2) O destino tem um índice UNIQUE. (A otimização não consegue testar unicidade.)
        //
        // (3) onError é diferente de OE_Abort e OE_Rollback.
        addr1 = vdbe_add_op2(&mut v.borrow_mut(), OP_REWIND as i32, i_dest, 0);
        empty_dest_test = vdbe_add_op0(&mut v.borrow_mut(), OP_GOTO as i32);
        vdbe_jump_here(&mut v.borrow_mut(), addr1);
    }
    if has_rowid(&p_src) {
        let mut ins_flags: u8;
        open_table(p_parse, i_src, i_db_src, &p_src, OP_OPENREAD as i32);
        empty_src_test = vdbe_add_op2(&mut v.borrow_mut(), OP_REWIND as i32, i_src, 0);
        if p_dest_b.i_p_key >= 0 {
            addr1 = vdbe_add_op2(&mut v.borrow_mut(), OP_ROWID as i32, i_src, reg_rowid);
            if (m_db_flags & DBFLAG_VACUUM) == 0 {
                addr2 = vdbe_add_op3(&mut v.borrow_mut(), OP_NOTEXISTS as i32, i_dest, 0, reg_rowid);
                rowid_constraint(p_parse, on_error, p_dest);
                vdbe_jump_here(&mut v.borrow_mut(), addr2);
            }
            auto_inc_step(p_parse, reg_autoinc, reg_rowid);
        } else if p_dest_b.p_index.is_none() && (m_db_flags & DBFLAG_VACUUMINTO) == 0 {
            addr1 = vdbe_add_op2(&mut v.borrow_mut(), OP_NEWROWID as i32, i_dest, reg_rowid);
        } else {
            addr1 = vdbe_add_op2(&mut v.borrow_mut(), OP_ROWID as i32, i_src, reg_rowid);
            assert!((p_dest_b.tab_flags & TF_AUTOINCREMENT) == 0);
        }

        if (m_db_flags & DBFLAG_VACUUM) != 0 {
            vdbe_add_op1(&mut v.borrow_mut(), OP_SEEKEND as i32, i_dest);
            ins_flags = OPFLAG_APPEND | OPFLAG_USESEEKRESULT | OPFLAG_PREFORMAT;
        } else {
            ins_flags = OPFLAG_NCHANGE | OPFLAG_LASTROWID | OPFLAG_APPEND | OPFLAG_PREFORMAT;
        }
        if (m_db_flags & DBFLAG_VACUUM) == 0 {
            vdbe_add_op3(&mut v.borrow_mut(), OP_ROWDATA as i32, i_src, reg_data, 1);
            ins_flags &= !OPFLAG_PREFORMAT;
        } else {
            vdbe_add_op3(&mut v.borrow_mut(), OP_ROWCELL as i32, i_dest, i_src, reg_rowid);
        }
        vdbe_add_op3(&mut v.borrow_mut(), OP_INSERT as i32, i_dest, reg_data, reg_rowid);
        if (m_db_flags & DBFLAG_VACUUM) == 0 {
            vdbe_change_p4(
                &mut v.borrow_mut(),
                -1,
                P4Union::Table(p_dest.clone()),
                P4_TABLE as i32,
            );
        }
        vdbe_change_p5(&mut v.borrow_mut(), ins_flags as u16);

        vdbe_add_op2(&mut v.borrow_mut(), OP_NEXT as i32, i_src, addr1);
        vdbe_add_op2(&mut v.borrow_mut(), OP_CLOSE as i32, i_src, 0);
        vdbe_add_op2(&mut v.borrow_mut(), OP_CLOSE as i32, i_dest, 0);
    } else {
        table_lock(p_parse, i_db_dest, p_dest_b.tnum, 1, Some(p_dest_b.z_name.clone()));
        table_lock(p_parse, i_db_src, p_src.tnum, 0, Some(p_src.z_name.clone()));
    }
    let mut p_dest_idx_opt = p_dest_b.p_index.clone();
    while let Some(p_dest_idx_ref) = p_dest_idx_opt {
        let p_dest_idx = p_dest_idx_ref.borrow();
        let mut idx_ins_flags: u8 = 0;
        let p_src_idx_ref = xfer_find_src_index(&p_dest_idx, &p_src);
        assert!(p_src_idx_ref.is_some());
        let p_src_idx_ref = p_src_idx_ref.unwrap();
        let p_src_idx = p_src_idx_ref.borrow();
        vdbe_add_op3(
            &mut v.borrow_mut(),
            OP_OPENREAD as i32,
            i_src,
            p_src_idx.tnum as i32,
            i_db_src,
        );
        vdbe_set_p4_key_info(p_parse, &p_src_idx_ref);
        vdbe_add_op3(
            &mut v.borrow_mut(),
            OP_OPENWRITE as i32,
            i_dest,
            p_dest_idx.tnum as i32,
            i_db_dest,
        );
        vdbe_set_p4_key_info(p_parse, &p_dest_idx_ref);
        vdbe_change_p5(&mut v.borrow_mut(), OPFLAG_BULKCSR as u16);
        addr1 = vdbe_add_op2(&mut v.borrow_mut(), OP_REWIND as i32, i_src, 0);
        if (m_db_flags & DBFLAG_VACUUM) != 0 {
            // Este INSERT faz parte de uma operação VACUUM, que garante que a tabela de destino
            // está vazia. Se todas as colunas indexadas usam a sequência de ordenação BINARY,
            // pode-se supor também que o índice será preenchido inserindo as chaves em ordem
            // estritamente crescente. Nesse caso, em vez de buscar dentro da b-tree em cada
            // opcode OP_IdxInsert, um OP_SeekEnd é posto antes do OP_IdxInsert para buscar o
            // ponto da b-tree onde cada chave deve entrar. Isso é mais rápido.
            //
            // Se alguma coluna indexada usa uma sequência de ordenação diferente de BINARY, esta
            // otimização fica desligada. Isso porque o usuário pode mudar a definição de uma
            // sequência de ordenação e depois rodar um VACUUM. Nesse caso as chaves podem não ser
            // gravadas em ordem estritamente crescente.
            let mut i: usize = 0;
            while i < p_src_idx.n_column as usize {
                let z_coll = &p_src_idx.az_coll[i];
                if stricmp(Some(SQLITE_STR_BINARY), Some(&z_coll[..])) != 0 {
                    break;
                }
                i += 1;
            }
            if i == p_src_idx.n_column as usize {
                idx_ins_flags = OPFLAG_USESEEKRESULT | OPFLAG_PREFORMAT;
                vdbe_add_op1(&mut v.borrow_mut(), OP_SEEKEND as i32, i_dest);
                vdbe_add_op2(&mut v.borrow_mut(), OP_ROWCELL as i32, i_dest, i_src);
            }
        } else if !has_rowid(&p_src) && p_dest_idx.idx_type == SQLITE_IDXTYPE_PRIMARYKEY {
            idx_ins_flags |= OPFLAG_NCHANGE;
        }
        if idx_ins_flags != (OPFLAG_USESEEKRESULT | OPFLAG_PREFORMAT) {
            vdbe_add_op3(&mut v.borrow_mut(), OP_ROWDATA as i32, i_src, reg_data, 1);
            if (m_db_flags & DBFLAG_VACUUM) == 0
                && !has_rowid(&p_dest_b)
                && is_primary_key_index(&p_dest_idx)
            {
                code_without_rowid_preupdate(p_parse, p_dest, i_dest, reg_data);
            }
        }
        vdbe_add_op2(&mut v.borrow_mut(), OP_IDXINSERT as i32, i_dest, reg_data);
        vdbe_change_p5(&mut v.borrow_mut(), (idx_ins_flags | OPFLAG_APPEND) as u16);
        vdbe_add_op2(&mut v.borrow_mut(), OP_NEXT as i32, i_src, addr1 + 1);
        vdbe_jump_here(&mut v.borrow_mut(), addr1);
        vdbe_add_op2(&mut v.borrow_mut(), OP_CLOSE as i32, i_src, 0);
        vdbe_add_op2(&mut v.borrow_mut(), OP_CLOSE as i32, i_dest, 0);
        let p_next = p_dest_idx.p_next.clone();
        drop(p_src_idx);
        drop(p_dest_idx);
        p_dest_idx_opt = p_next;
    }
    if empty_src_test != 0 {
        vdbe_jump_here(&mut v.borrow_mut(), empty_src_test);
    }
    release_temp_reg(p_parse, reg_rowid);
    release_temp_reg(p_parse, reg_data);
    if empty_dest_test != 0 {
        autoincrement_end(p_parse);
        vdbe_add_op2(&mut v.borrow_mut(), OP_HALT as i32, SQLITE_OK, 0);
        vdbe_jump_here(&mut v.borrow_mut(), empty_dest_test);
        vdbe_add_op2(&mut v.borrow_mut(), OP_CLOSE as i32, i_dest, 0);
        0
    } else {
        1
    }
}


// ---- part_008.rs ----

