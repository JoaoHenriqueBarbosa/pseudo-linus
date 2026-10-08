// Mesclado das partes traduzidas de vdbeblob_c (as partes quebram funções grandes no meio).
#![allow(unused_imports, ambiguous_glob_reexports)]
use crate::prelude::*;

// ---- part_000.rs ----

/// Alças `sqlite3_blob*` válidas apontam para estruturas Incrblob.
pub struct Incrblob {
    /// Tamanho do blob aberto, em bytes.
    pub n_byte: i32,
    /// Deslocamento em bytes do blob nos dados do cursor.
    pub i_offset: i32,
    /// Coluna da tabela em que esta alça está aberta.
    pub i_col: u16,
    /// Cursor apontando para a linha do blob.
    pub p_csr: Option<BtCursorRef>,
    /// Instrução que mantém o cursor aberto.
    pub p_stmt: Option<VdbeRef>,
    /// O banco de dados associado.
    pub db: Sqlite3Ref,
    /// Nome do banco de dados.
    pub z_db: Vec<u8>,
    /// Objeto da tabela.
    pub p_tab: Option<TableRef>,
}

/// Usada por blob_open() e blob_reopen(). Posiciona o cursor da b-tree associado à
/// alça de blob `p` na linha `i_row`.
///
/// Se for bem-sucedida, retorna SQLITE_OK e as chamadas seguintes a blob_read() ou
/// blob_write() acessam a linha especificada. Se ocorrer um erro, ou se a linha não
/// existir ou não tiver um valor TEXT ou BLOB na coluna indicada ao abrir a alça, um
/// código de erro é retornado e `pz_err` pode receber uma mensagem de erro.
///
/// Se ocorrer um erro, o cursor da b-tree é fechado. Todas as chamadas seguintes a
/// blob_read(), blob_write() ou blob_reopen() retornam imediatamente SQLITE_ABORT.
fn blob_seek_to_row(p: &mut Incrblob, i_row: i64, pz_err: &mut Option<Vec<u8>>) -> i32 {
    let mut rc: i32; // Código de erro.
    let mut z_err: Option<Vec<u8>> = None; // Mensagem de erro.
    let v: VdbeRef = p.p_stmt.clone().unwrap();

    // Grava o inteiro i_row no registrador r[1] da instrução SQL. É feito direto
    // como otimização de desempenho.
    vdbe_mem_set_int64(&mut v.borrow_mut().a_mem[1], i_row);

    // Se a instrução já rodou (e está parada no OP_ResultRow), volta até o ponto em
    // que executa o OP_NotExists. Poderia ter sido feito com um OP_Goto extra, mas
    // ajustar o contador de programa é mais rápido.
    if v.borrow().pc > 4 {
        v.borrow_mut().pc = 4;
        debug_assert!(v.borrow().a_op[4].opcode == OP_NOT_EXISTS);
        rc = vdbe_exec(&v);
    } else {
        rc = api::step(p.p_stmt.as_ref().unwrap());
    }
    if rc == SQLITE_ROW {
        let p_c: VdbeCursorRef = v.borrow().ap_csr[0].clone().unwrap();
        let ty: u32 = {
            let c = p_c.borrow();
            debug_assert!(c.e_cur_type == CURTYPE_BTREE);
            if (c.n_hdr_parsed as i32) > (p.i_col as i32) {
                c.a_type[p.i_col as usize]
            } else {
                0
            }
        };
        if ty < 12 {
            let nome: &[u8] = if ty == 0 {
                b"null"
            } else if ty == 7 {
                b"real"
            } else {
                b"integer"
            };
            z_err = Some([&b"cannot open value of type "[..], nome].concat());
            rc = SQLITE_ERROR;
            api::finalize(p.p_stmt.as_ref().unwrap());
            p.p_stmt = None;
        } else {
            {
                let c = p_c.borrow();
                p.i_offset = c.a_type[p.i_col as usize + c.n_field as usize] as i32;
                p.n_byte = vdbe_serial_type_len(ty) as i32;
                p.p_csr = c.uc.p_cursor.clone();
            }
            btree_incrblob_cursor(&mut p.p_csr.as_ref().unwrap().borrow_mut());
        }
    }

    if rc == SQLITE_ROW {
        rc = SQLITE_OK;
    } else if p.p_stmt.is_some() {
        rc = api::finalize(p.p_stmt.as_ref().unwrap());
        p.p_stmt = None;
        if rc == SQLITE_OK {
            z_err = Some([&b"no such rowid: "[..], i_row.to_string().as_bytes()].concat());
            rc = SQLITE_ERROR;
        } else {
            z_err = Some(api::errmsg(&p.db));
        }
    }

    debug_assert!(rc != SQLITE_OK || z_err.is_none());
    debug_assert!(rc != SQLITE_ROW && rc != SQLITE_DONE);

    *pz_err = z_err;
    rc
}

/// Abre uma alça de blob.
pub fn api_blob_open(
    db: &Sqlite3Ref,                  // A conexão com o banco de dados.
    z_db: Option<&[u8]>,              // O banco anexado que contém o blob.
    z_table: &[u8],                   // A tabela que contém o blob.
    z_column: &[u8],                  // A coluna que contém o blob.
    i_row: i64,                       // A linha que contém o blob.
    wr_flag: i32,                     // Verdadeiro: leitura e escrita; falso: só leitura.
    pp_blob: &mut Option<Box<Incrblob>>, // A alça do blob é devolvida aqui.
) -> i32 {
    let mut n_attempt: i32 = 0;
    let mut i_col: i32; // Índice de z_column no registro da linha.
    let mut rc: i32 = SQLITE_OK;
    let mut z_err: Option<Vec<u8>> = None;
    let mut s_parse = Parse::default();

    *pp_blob = None;
    let wr_flag: i32 = if wr_flag != 0 { 1 } else { 0 };

    mutex_enter(db.borrow().mutex);

    // O C aloca com sqlite3DbMallocZero; o campo db já nasce preenchido aqui.
    let mut p_blob: Option<Box<Incrblob>> = Some(Box::new(Incrblob {
        n_byte: 0,
        i_offset: 0,
        i_col: 0,
        p_csr: None,
        p_stmt: None,
        db: db.clone(),
        z_db: Vec::new(),
        p_tab: None,
    }));

    'blob_open_out: {
        loop {
            parse_object_init(&mut s_parse, db);
            if p_blob.is_none() {
                break 'blob_open_out;
            }
            z_err = None;

            btree_enter_all(db);
            let mut p_tab: Option<TableRef> = locate_table(&mut s_parse, 0, z_table, z_db);
            if p_tab.is_some() && is_virtual(&p_tab.as_ref().unwrap().borrow()) {
                p_tab = None;
                error_msg(&mut s_parse, &[&b"cannot open virtual table: "[..], z_table].concat());
            }
            if p_tab.is_some() && !has_rowid(&p_tab.as_ref().unwrap().borrow()) {
                p_tab = None;
                error_msg(
                    &mut s_parse,
                    &[&b"cannot open table without rowid: "[..], z_table].concat(),
                );
            }
            if p_tab.is_some() && is_view(&p_tab.as_ref().unwrap().borrow()) {
                p_tab = None;
                error_msg(&mut s_parse, &[&b"cannot open view: "[..], z_table].concat());
            }
            let p_tab: TableRef = match p_tab {
                Some(t) => t,
                None => {
                    if s_parse.z_err_msg.is_some() {
                        z_err = s_parse.z_err_msg.take();
                    }
                    rc = SQLITE_ERROR;
                    btree_leave_all(db);
                    break 'blob_open_out;
                }
            };
            let i_db_tab: i32 = schema_to_index(db, p_tab.borrow().p_schema.as_ref().unwrap());
            {
                let b = p_blob.as_mut().unwrap();
                b.p_tab = Some(p_tab.clone());
                b.z_db = db.borrow().a_db[i_db_tab as usize].z_db_s_name.clone();
            }

            // Agora procura a coluna exata em p_tab.
            i_col = 0;
            while i_col < p_tab.borrow().n_col as i32 {
                if str_i_cmp(&p_tab.borrow().a_col[i_col as usize].z_cn_name, z_column) == 0 {
                    break;
                }
                i_col += 1;
            }
            if i_col == p_tab.borrow().n_col as i32 {
                z_err = Some([&b"no such column: \""[..], z_column, &b"\""[..]].concat());
                rc = SQLITE_ERROR;
                btree_leave_all(db);
                break 'blob_open_out;
            }

            // Se o valor está sendo aberto para escrita, confere que a coluna não é
            // indexada e que não faz parte de uma chave estrangeira.
            if wr_flag != 0 {
                let mut z_fault: Option<&'static [u8]> = None;
                if db.borrow().flags & SQLITE_FOREIGNKEYS != 0 {
                    // Confere que a coluna não faz parte da definição da chave filha
                    // de uma chave estrangeira. Não é preciso conferir a chave pai,
                    // pois colunas de chave pai precisam ser indexadas. A conferência
                    // abaixo cobre esse caso.
                    debug_assert!(is_ordinary_table(&p_tab.borrow()));
                    let mut p_f_key: Option<FKeyRef> = p_tab.borrow().u.tab.p_f_key.clone();
                    while let Some(fk) = p_f_key {
                        let fkb = fk.borrow();
                        for j in 0..fkb.n_col as usize {
                            if fkb.a_col[j].i_from == i_col {
                                z_fault = Some(b"foreign key");
                            }
                        }
                        let next = fkb.p_next_from.clone();
                        drop(fkb);
                        p_f_key = next;
                    }
                }
                let mut p_idx: Option<IndexRef> = p_tab.borrow().p_index.clone();
                while let Some(ix) = p_idx {
                    let ixb = ix.borrow();
                    for j in 0..ixb.n_key_col as usize {
                        // FIXME: ser mais esperto com índices que usam expressões.
                        if ixb.a_i_column[j] as i32 == i_col || ixb.a_i_column[j] == XN_EXPR {
                            z_fault = Some(b"indexed");
                        }
                    }
                    let next = ixb.p_next.clone();
                    drop(ixb);
                    p_idx = next;
                }
                if let Some(f) = z_fault {
                    z_err = Some([&b"cannot open "[..], f, &b" column for writing"[..]].concat());
                    rc = SQLITE_ERROR;
                    btree_leave_all(db);
                    break 'blob_open_out;
                }
            }

            p_blob.as_mut().unwrap().p_stmt = vdbe_create(&mut s_parse);
            debug_assert!(p_blob.as_ref().unwrap().p_stmt.is_some() || db.borrow().malloc_failed != 0);
            if p_blob.as_ref().unwrap().p_stmt.is_some() {
                // Este programa VDBE posiciona um cursor da b-tree na entrada
                // banco/tabela/linha identificada. Usa-se um programa VDBE em vez de
                // escrever código que use a b-tree direto porque o programa aproveita
                // a infraestrutura de transação, travas e tratamento de erro do vdbe.
                //
                // Depois de posicionar o cursor, o vdbe executa um OP_ResultRow. O
                // código externo ao Vdbe então "empresta" o cursor da b-tree e o usa
                // para implementar blob_read(), blob_write() e blob_bytes().
                //
                // sqlite3_blob_close() finaliza o programa vdbe, o que fecha o cursor
                // da b-tree e (possivelmente) confirma a transação.
                const I_LN: i32 = 0; // VDBE_OFFSET_LINENO(2) sem cobertura é 0.
                const OPEN_BLOB: [VdbeOpList; 6] = [
                    VdbeOpList { opcode: OP_TABLE_LOCK, p1: 0, p2: 0, p3: 0 }, // 0: trava de leitura ou escrita
                    VdbeOpList { opcode: OP_OPEN_READ, p1: 0, p2: 0, p3: 0 },  // 1: abre um cursor
                    // blob_seek_to_row() inicializa r[1] com o rowid desejado.
                    VdbeOpList { opcode: OP_NOT_EXISTS, p1: 0, p2: 5, p3: 1 }, // 2: posiciona em rowid=r[1]
                    VdbeOpList { opcode: OP_COLUMN, p1: 0, p2: 0, p3: 1 },     // 3
                    VdbeOpList { opcode: OP_RESULT_ROW, p1: 1, p2: 0, p3: 0 }, // 4
                    VdbeOpList { opcode: OP_HALT, p1: 0, p2: 0, p3: 0 },       // 5
                ];
                let v: VdbeRef = p_blob.as_ref().unwrap().p_stmt.clone().unwrap();
                let i_db: i32 = schema_to_index(db, p_tab.borrow().p_schema.as_ref().unwrap());

                let (cookie, generation) = {
                    let t = p_tab.borrow();
                    let sc = t.p_schema.as_ref().unwrap().borrow();
                    (sc.schema_cookie, sc.i_generation)
                };
                vdbe_add_op4_int(&v, OP_TRANSACTION, i_db, wr_flag, cookie, generation);
                vdbe_change_p5(&v, 1);
                debug_assert!(vdbe_current_addr(&v) == 2 || db.borrow().malloc_failed != 0);
                // a_op é o índice base do bloco inserido em v.a_op (o ponteiro do C).
                let a_op: Option<usize> = vdbe_add_op_list(&v, OPEN_BLOB.len() as i32, &OPEN_BLOB, I_LN);

                // Garante que há um mutex na tabela a ser acessada.
                vdbe_uses_btree(&v, i_db);

                if db.borrow().malloc_failed == 0 {
                    let base = a_op.unwrap();
                    // Configura a instrução OP_TableLock.
                    {
                        let mut vb = v.borrow_mut();
                        let tnum = p_tab.borrow().tnum as i32;
                        vb.a_op[base].p1 = i_db;
                        vb.a_op[base].p2 = tnum;
                        vb.a_op[base].p3 = wr_flag;
                    }
                    let z_name = p_tab.borrow().z_name.clone();
                    vdbe_change_p4(&v, 2, &z_name, P4_TRANSIENT);
                }
                if db.borrow().malloc_failed == 0 {
                    let base = a_op.unwrap();
                    // Remove o OP_OpenWrite ou o OpenRead. Define o parâmetro P2 do
                    // outro como p_tab.tnum.
                    {
                        let mut vb = v.borrow_mut();
                        let tnum = p_tab.borrow().tnum as i32;
                        let n_col = p_tab.borrow().n_col as i32;
                        if wr_flag != 0 {
                            vb.a_op[base + 1].opcode = OP_OPEN_WRITE;
                        }
                        vb.a_op[base + 1].p2 = tnum;
                        vb.a_op[base + 1].p3 = i_db;

                        // Configura o número de colunas. Configura o cursor para achar
                        // que a tabela tem uma coluna a mais do que tem de fato. Um
                        // OP_Column para obter essa coluna imaginária sempre devolve
                        // NULL. Isso é útil porque permite invocar OP_Column para
                        // preencher o cache de tipo e deslocamento do cursor do vdbe
                        // sem causar nenhum E/S.
                        vb.a_op[base + 1].p4type = P4_INT32;
                        vb.a_op[base + 1].p4.i = n_col + 1;
                        vb.a_op[base + 3].p2 = n_col;
                    }

                    s_parse.n_var = 0;
                    s_parse.n_mem = 1;
                    s_parse.n_tab = 1;
                    vdbe_make_ready(&v, &mut s_parse);
                }
            }

            {
                let b = p_blob.as_mut().unwrap();
                b.i_col = i_col as u16;
                b.db = db.clone();
            }
            btree_leave_all(db);
            if db.borrow().malloc_failed != 0 {
                break 'blob_open_out;
            }
            rc = blob_seek_to_row(p_blob.as_mut().unwrap(), i_row, &mut z_err);
            n_attempt += 1;
            if n_attempt >= SQLITE_MAX_SCHEMA_RETRY || rc != SQLITE_SCHEMA {
                break;
            }
            parse_object_reset(&mut s_parse);
        }
    }

    // blob_open_out:
    if rc == SQLITE_OK && db.borrow().malloc_failed == 0 {
        *pp_blob = p_blob;
    } else {
        if let Some(b) = p_blob.as_ref() {
            if let Some(st) = b.p_stmt.as_ref() {
                vdbe_finalize(st);
            }
        }
        // sqlite3DbFree(db, pBlob): o Box é liberado ao sair de escopo.
        drop(p_blob);
    }
    // No C: sqlite3ErrorWithMsg(db, rc, (zErr ? "%s" : 0), zErr).
    error_with_msg(&mut db.borrow_mut(), rc, z_err.as_deref());
    drop(z_err);
    parse_object_reset(&mut s_parse);
    rc = api_exit(db, rc);
    mutex_leave(db.borrow().mutex);
    rc
}

/// Fecha uma alça de blob criada antes com sqlite3_blob_open().
pub fn api_blob_close(p_blob: Option<Box<Incrblob>>) -> i32 {
    match p_blob {
        Some(p) => {
            let p_stmt: Option<VdbeRef> = p.p_stmt.clone();
            let db: Sqlite3Ref = p.db.clone();
            mutex_enter(db.borrow().mutex);
            drop(p);
            mutex_leave(db.borrow().mutex);
            match p_stmt {
                Some(s) => api::finalize(&s),
                None => SQLITE_OK,
            }
        }
        None => SQLITE_OK,
    }
}


// ---- part_001.rs ----

/// Realiza uma operação de leitura ou escrita em um blob.
///
/// `x_call` é `btree_payload_checked` (leitura) ou `btree_put_data` (escrita); o buffer
/// `z` é o `void *z` do C, por isso os dois recebem `&mut [u8]`. `p_blob` ausente
/// corresponde ao ponteiro nulo do C.
fn blob_read_write(
    p_blob: Option<&mut Incrblob>,
    z: &mut [u8],
    n: i32,
    i_offset: i32,
    x_call: fn(&mut BtCursor, u32, u32, &mut [u8]) -> i32,
) -> i32 {
    let mut rc: i32;
    let p = match p_blob {
        None => return misuse_bkpt(),
        Some(p) => p,
    };
    let db = p.db.clone();
    mutex_enter(db.borrow().mutex);
    let v: Option<VdbeRef> = p.p_stmt.clone();

    if n < 0 || i_offset < 0 || (i_offset as i64 + n as i64) > p.n_byte as i64 {
        // Pedido fora do intervalo. Retorna um erro transitório.
        rc = SQLITE_ERROR;
    } else if v.is_none() {
        // Se não há alça de instrução, a alça do blob já foi invalidada.
        // Retorna SQLITE_ABORT neste caso.
        rc = SQLITE_ABORT;
    } else {
        // Chama btree_payload_checked() ou btree_put_data(). Se SQLITE_ABORT for
        // retornado, limpa a alça da instrução.
        let v = v.unwrap();
        debug_assert!(v.borrow().db.upgrade().map_or(false, |d| std::rc::Rc::ptr_eq(&d, &db)));
        let p_csr = p.p_csr.clone().unwrap();
        btree_enter_cursor(&mut p_csr.borrow_mut());

        if x_call as usize == btree_put_data as usize && db.borrow().x_pre_update_callback.is_some() {
            // Se um gancho de pré-atualização está registrado e este é um cursor de
            // escrita, invoca-o aqui.
            //
            // TODO: o gancho de pré-atualização recebe SQLITE_DELETE, ainda que esta
            // operação devesse ser na verdade um SQLITE_UPDATE. Isso provavelmente está
            // incorreto, mas é conveniente porque neste ponto os valores new.* não são
            // facilmente obtidos. E para o módulo de sessões, um SQLITE_UPDATE em que as
            // colunas da chave primária não mudam é tratado do mesmo jeito que um
            // SQLITE_DELETE (o código SQLITE_DELETE é na verdade um pouco mais
            // eficiente). Como não se pode escrever numa coluna da chave primária pela
            // API de blob incremental, isso funciona. Para o módulo de sessões, de
            // qualquer forma.
            let i_key: i64 = btree_integer_key(&mut p_csr.borrow_mut());
            debug_assert!(v.borrow().ap_csr[0].is_some());
            let p_csr0 = v.borrow().ap_csr[0].clone().unwrap();
            debug_assert!(p_csr0.borrow().e_cur_type == CURTYPE_BTREE);
            vdbe_pre_update_hook(
                &mut v.borrow_mut(),
                &p_csr0,
                SQLITE_DELETE,
                &p.z_db,
                p.p_tab.as_ref().unwrap(),
                i_key,
                -1,
                p.i_col as i32,
            );
        }

        rc = x_call(
            &mut p_csr.borrow_mut(),
            (i_offset + p.i_offset) as u32,
            n as u32,
            z,
        );
        btree_leave_cursor(&mut p_csr.borrow_mut());
        if rc == SQLITE_ABORT {
            vdbe_finalize(&v);
            p.p_stmt = None;
        } else {
            v.borrow_mut().rc = rc;
        }
    }
    error(&mut db.borrow_mut(), rc);
    rc = api_exit(&db, rc);
    mutex_leave(db.borrow().mutex);
    rc
}

/// Lê dados de uma alça de blob.
pub fn api_blob_read(p_blob: Option<&mut Incrblob>, z: &mut [u8], n: i32, i_offset: i32) -> i32 {
    blob_read_write(p_blob, z, n, i_offset, btree_payload_checked)
}

/// Escreve dados numa alça de blob.
pub fn api_blob_write(p_blob: Option<&mut Incrblob>, z: &mut [u8], n: i32, i_offset: i32) -> i32 {
    blob_read_write(p_blob, z, n, i_offset, btree_put_data)
}

/// Consulta uma alça de blob pelo tamanho dos dados.
///
/// O campo Incrblob.n_byte é fixo durante toda a vida do Incrblob, então nenhum
/// mutex é necessário para o acesso.
pub fn api_blob_bytes(p_blob: Option<&Incrblob>) -> i32 {
    match p_blob {
        Some(p) if p.p_stmt.is_some() => p.n_byte,
        _ => 0,
    }
}

/// Move uma alça de blob existente para apontar para outra linha da mesma tabela
/// do banco de dados.
///
/// Se ocorrer um erro, ou se a linha especificada não existir ou não contiver um
/// valor do tipo TEXT ou BLOB, um código de erro é retornado e o código e a mensagem
/// de erro do banco de dados são definidos. Se isso acontecer, todas as chamadas
/// seguintes às funções blob_xxx() (exceto blob_close()) retornam imediatamente
/// SQLITE_ABORT.
pub fn api_blob_reopen(p_blob: Option<&mut Incrblob>, i_row: i64) -> i32 {
    let mut rc: i32;
    let p = match p_blob {
        None => return misuse_bkpt(),
        Some(p) => p,
    };
    let db = p.db.clone();
    mutex_enter(db.borrow().mutex);

    if p.p_stmt.is_none() {
        // Se não há alça de instrução, a alça do blob já foi invalidada.
        // Retorna SQLITE_ABORT neste caso.
        rc = SQLITE_ABORT;
    } else {
        let mut z_err: Option<Vec<u8>> = None;
        p.p_stmt.as_ref().unwrap().borrow_mut().rc = SQLITE_OK;
        rc = blob_seek_to_row(p, i_row, &mut z_err);
        if rc != SQLITE_OK {
            // No C: sqlite3ErrorWithMsg(db, rc, (zErr ? "%s" : 0), zErr).
            error_with_msg(&mut db.borrow_mut(), rc, z_err.as_deref());
            // sqlite3DbFree(db, zErr): o Vec é liberado ao sair de escopo.
            drop(z_err);
        }
        debug_assert!(rc != SQLITE_SCHEMA);
    }

    rc = api_exit(&db, rc);
    debug_assert!(rc == SQLITE_OK || p.p_stmt.is_none());
    mutex_leave(db.borrow().mutex);
    rc
}

