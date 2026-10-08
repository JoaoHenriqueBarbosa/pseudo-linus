// Mesclado das partes traduzidas de backup_c (as partes quebram funções grandes no meio).
#![allow(unused_imports, ambiguous_glob_reexports)]
use crate::prelude::*;

// ---- part_000.rs ----

/// Estrutura alocada para cada operação de cópia de segurança (sqlite3_backup).
///
/// O objeto é compartilhado com a lista `p_backup` do paginador de origem, por
/// isso vive atrás de `BackupRef`. `p_dest_db` é `None` depois que o destino é
/// desvinculado (como o ponteiro nulo do C).
pub struct Backup {
    /// Identificador do banco de dados de destino
    pub p_dest_db: Option<Sqlite3Ref>,
    /// Arquivo b-tree de destino
    pub p_dest: BtreeRef,
    /// Cookie de esquema original no destino
    pub i_dest_schema: u32,
    /// Verdadeiro depois que uma transação de escrita foi aberta em p_dest
    pub b_dest_locked: i32,

    /// Número da página da próxima página de origem a copiar
    pub i_next: Pgno,
    /// Identificador do banco de dados de origem
    pub p_src_db: Sqlite3Ref,
    /// Arquivo b-tree de origem
    pub p_src: BtreeRef,

    /// Código de erro do processo de cópia de segurança
    pub rc: i32,

    /// Estas duas variáveis são definidas a cada chamada de backup_step(). São
    /// lidas pelas chamadas de backup_remaining() e backup_pagecount().
    /// Número de páginas que faltam copiar
    pub n_remaining: Pgno,
    /// Número total de páginas a copiar
    pub n_pagecount: Pgno,

    /// Verdadeiro depois que a cópia foi registrada no paginador
    pub is_attached: i32,
    /// Próxima cópia associada ao paginador de origem
    pub p_next: Option<BackupRef>,
}

/// Referência compartilhada a um `Backup` (o objeto também está na lista do paginador).
pub type BackupRef = std::rc::Rc<std::cell::RefCell<Backup>>;

// NOTAS DE SEGURANÇA DE THREAD:
//
//   Depois de criada por backup_init(), uma única estrutura sqlite3_backup pode
//   ser acessada por dois grupos de pontos de entrada seguros para threads:
//
//     * Pelas funções da API backup_step() e backup_finish(). As duas obtêm o
//       mutex do identificador do banco de origem e o mutex da estrutura
//       BtShared de origem, nessa ordem.
//
//     * Por BackupUpdate() e BackupRestart(), invocadas pela camada do
//       paginador para relatar mudanças de estado no cache de páginas do banco
//       de origem. O mutex da BtShared de origem sempre está retido quando
//       qualquer das duas é invocada.
//
//   As outras funções backup_remaining() e backup_pagecount() não são seguras
//   para threads. Se forem chamadas enquanto outra thread executa backup_step()
//   ou backup_finish(), os valores devolvidos podem ser inválidos. Não há como
//   BackupUpdate() ou BackupRestart() interferirem nelas.
//
//   Dependendo da configuração do SQLite, os identificadores de banco e/ou os
//   objetos Btree podem ter mutexes próprios. Btrees não compartilháveis (bancos
//   em memória, por exemplo) não têm mutexes associados.

/// Devolve o Btree correspondente ao banco z_db ("main", "temp") na conexão
/// p_db. Se o banco não existir, devolve `None` e escreve uma mensagem de erro
/// em p_error_db.
///
/// Se o banco "temp" for pedido, esta função pode precisar abri-lo. Se um erro
/// ocorrer ao fazer isso, devolve `None` e escreve a mensagem em p_error_db.
fn find_btree(p_error_db: &Sqlite3Ref, p_db: &Sqlite3Ref, z_db: &[u8]) -> Option<BtreeRef> {
    let i = find_db_name(&p_db.borrow(), z_db);

    if i == 1 {
        let mut s_parse = Parse::default();
        let mut rc = 0;
        parse_object_init(&mut s_parse, p_db);
        if open_temp_database(&mut s_parse) != 0 {
            // "%s" com a mensagem do Parse; texto nulo sai vazio no printf do SQLite.
            let z_msg = s_parse.z_err_msg.clone().unwrap_or_default();
            error_with_msg(&mut p_error_db.borrow_mut(), s_parse.rc, Some(&z_msg));
            rc = SQLITE_ERROR;
        }
        // sqlite3DbFree(pErrorDb, sParse.zErrMsg): o Vec é dono da memória.
        s_parse.z_err_msg = None;
        parse_object_reset(&mut s_parse);
        if rc != 0 {
            return None;
        }
    }

    if i < 0 {
        let mut z_msg = b"unknown database ".to_vec();
        z_msg.extend_from_slice(z_db);
        error_with_msg(&mut p_error_db.borrow_mut(), SQLITE_ERROR, Some(&z_msg));
        return None;
    }

    p_db.borrow().a_db[i as usize].p_bt.clone()
}

/// Tenta ajustar o tamanho de página do destino para o da origem.
fn set_dest_pgsz(p: &Backup) -> i32 {
    btree_set_page_size(&p.p_dest, btree_get_page_size(&p.p_src), 0, 0)
}

/// Verifica que não há transação de leitura aberta na b-tree passada como
/// segundo argumento. Se não houver, devolve SQLITE_OK. Se houver, devolve
/// SQLITE_ERROR e deixa uma mensagem de erro no identificador db.
fn check_read_transaction(db: &Sqlite3Ref, p: &BtreeRef) -> i32 {
    if btree_txn_state(p) != SQLITE_TXN_NONE {
        error_with_msg(&mut db.borrow_mut(), SQLITE_ERROR, Some(b"destination database is in use"));
        return SQLITE_ERROR;
    }
    SQLITE_OK
}

/// Cria um processo sqlite3_backup para copiar o conteúdo de z_src_db da
/// conexão p_src_db para z_dest_db em p_dest_db. Em caso de sucesso devolve o
/// novo objeto.
///
/// Se ocorrer um erro, devolve `None` e guarda o código e a mensagem de erro
/// no identificador p_dest_db.
pub fn backup_init(
    p_dest_db: &Sqlite3Ref, // Banco de dados onde escrever
    z_dest_db: &[u8],       // Nome do banco dentro de p_dest_db
    p_src_db: &Sqlite3Ref,  // Conexão de onde ler
    z_src_db: &[u8],        // Nome do banco dentro de p_src_db
) -> Option<BackupRef> {
    let mut p: Option<BackupRef> = None; // Valor a devolver

    // Trava o identificador do banco de origem. O identificador de destino não
    // é travado nesta rotina, mas é travado em backup_step(). O usuário precisa
    // garantir que nenhuma outra thread acesse o identificador de destino
    // durante a cópia. Qualquer uso da conexão de destino enquanto a cópia está
    // em andamento pode causar mau funcionamento ou deadlock.
    mutex_enter(&p_src_db.borrow().mutex);
    mutex_enter(&p_dest_db.borrow().mutex);

    if std::rc::Rc::ptr_eq(p_src_db, p_dest_db) {
        error_with_msg(
            &mut p_dest_db.borrow_mut(),
            SQLITE_ERROR,
            Some(b"source and destination must be distinct"),
        );
    } else {
        // Aloca o novo objeto sqlite3_backup (a falha de alocação do C não
        // existe aqui, então o ramo sqlite3Error(pDestDb, SQLITE_NOMEM_BKPT) some).
        // EVIDENCE-OF: R-64852-21591 O objeto sqlite3_backup é criado por uma
        // chamada a sqlite3_backup_init() e destruído por uma chamada a
        // sqlite3_backup_finish().
        //
        // Com o objeto alocado, preenche-o. As duas buscas sempre executam, na
        // ordem origem e depois destino.
        let p_src = find_btree(p_dest_db, p_src_db, z_src_db);
        let p_dest = find_btree(p_dest_db, p_dest_db, z_dest_db);

        // Se um (ou os dois) bancos nomeados não existem, ou houve OOM, ou há
        // transação aberta no destino, o erro já está no p_dest_db e só resta
        // descartar o objeto.
        if let (Some(p_src), Some(p_dest)) = (p_src, p_dest) {
            if check_read_transaction(p_dest_db, &p_dest) == SQLITE_OK {
                p = Some(std::rc::Rc::new(std::cell::RefCell::new(Backup {
                    p_dest_db: Some(p_dest_db.clone()),
                    p_dest,
                    i_dest_schema: 0,
                    b_dest_locked: 0,
                    i_next: 1,
                    p_src_db: p_src_db.clone(),
                    p_src,
                    rc: SQLITE_OK,
                    n_remaining: 0,
                    n_pagecount: 0,
                    is_attached: 0,
                    p_next: None,
                })));
            }
        }
    }
    if let Some(p) = &p {
        p.borrow().p_src.borrow_mut().n_backup += 1;
    }

    mutex_leave(&p_dest_db.borrow().mutex);
    mutex_leave(&p_src_db.borrow().mutex);
    p
}

/// O argumento rc é um código de erro do SQLite. Devolve verdadeiro se o erro é
/// considerado fatal numa operação de cópia. Todos os erros são fatais, exceto
/// SQLITE_BUSY e SQLITE_LOCKED.
fn is_fatal_error(rc: i32) -> bool {
    rc != SQLITE_OK && rc != SQLITE_BUSY && always(rc != SQLITE_LOCKED)
}

/// O parâmetro z_src_data é um buffer com os dados da página i_src_pg do banco
/// de origem. Copia esses dados para o banco de destino.
fn backup_one_page(
    p: &Backup,         // Identificador da cópia
    i_src_pg: Pgno,     // Página do banco de origem a copiar
    z_src_data: &[u8],  // Dados da página de origem
    b_update: i32,      // Verdadeiro para uma atualização, falso caso contrário
) -> i32 {
    let p_dest_pager = btree_pager(&p.p_dest);
    let n_src_pgsz = btree_get_page_size(&p.p_src);
    let n_dest_pgsz = btree_get_page_size(&p.p_dest);
    let n_copy = min(n_src_pgsz, n_dest_pgsz);
    let i_end = (i_src_pg as i64) * (n_src_pgsz as i64);
    let mut rc = SQLITE_OK;
    let mut i_off: i64;
    let bt_src = p.p_src.borrow().p_bt.clone().unwrap();
    let bt_dest = p.p_dest.borrow().p_bt.clone().unwrap();

    debug_assert!(btree_get_reserve_no_mutex(&p.p_src) >= 0);
    debug_assert!(p.b_dest_locked != 0);
    debug_assert!(!is_fatal_error(p.rc));
    debug_assert!(i_src_pg != pending_byte_page(&bt_src.borrow()));
    debug_assert!(!z_src_data.is_empty());
    debug_assert!(n_src_pgsz == n_dest_pgsz || pager_is_memdb(&p_dest_pager) == 0);

    // Este laço executa uma vez para cada página de destino coberta pela página
    // de origem. A cada iteração, i_off é o deslocamento em bytes da página de
    // destino.
    i_off = i_end - (n_src_pgsz as i64);
    while rc == SQLITE_OK && i_off < i_end {
        let mut p_dest_pg: Option<PgHdrRef> = None;
        let i_dest: Pgno = ((i_off / n_dest_pgsz as i64) as Pgno).wrapping_add(1);
        // O "continue" do C pula também o pager_unref, que lá seria sobre nulo.
        if i_dest != pending_byte_page(&bt_dest.borrow()) {
            rc = pager_get(Some(&p_dest_pager), i_dest, &mut p_dest_pg, 0);
            if rc == SQLITE_OK {
                rc = pager_write(p_dest_pg.as_ref().unwrap());
            }
            if rc == SQLITE_OK {
                let z_in = (i_off % n_src_pgsz as i64) as usize;
                let z_out = (i_off % n_dest_pgsz as i64) as usize;
                let n = n_copy as usize;
                let mut pg = p_dest_pg.as_ref().unwrap().borrow_mut();

                // Copia os dados da página de origem para a de destino. Depois
                // limpa o flag MemPage.is_init da camada Btree. Este módulo e o
                // código do paginador usam o mesmo truque (zerar o primeiro byte
                // do espaço "extra" da página para invalidar a análise em cache
                // da camada Btree). MemPage.is_init é marcado "DEVE SER O
                // PRIMEIRO" para esse fim.
                pg.p_data[z_out..z_out + n].copy_from_slice(&z_src_data[z_in..z_in + n]);
                pg.p_extra[0] = 0;
                if i_off == 0 && b_update == 0 {
                    put4byte(&mut pg.p_data[z_out + 28..z_out + 32], btree_last_page(&p.p_src));
                }
            }
            pager_unref(p_dest_pg.as_ref());
        }
        i_off += n_dest_pgsz as i64;
    }

    rc
}

/// Se p_file for maior que i_size bytes, trunca para exatamente i_size bytes.
/// Se não for maior, a função não faz nada.
///
/// Devolve SQLITE_OK se tudo der certo, ou um código de erro do SQLite.
fn backup_truncate_file(p_file: &mut Sqlite3File, i_size: i64) -> i32 {
    let mut i_current: i64 = 0;
    let mut rc = os_file_size(p_file, &mut i_current);
    if rc == SQLITE_OK && i_current > i_size {
        rc = os_truncate(p_file, i_size);
    }
    rc
}

/// Registra este objeto de cópia no paginador de origem associado, para receber
/// chamadas quando páginas mudam ou o cache é invalidado.
fn attach_backup_object(p: &BackupRef) {
    let p_src = p.borrow().p_src.clone();
    debug_assert!(btree_holds_mutex(&p_src));
    let p_pager = btree_pager(&p_src);
    let mut pager = p_pager.borrow_mut();
    let pp = pager_backup_ptr(&mut pager);
    p.borrow_mut().p_next = pp.take();
    *pp = Some(p.clone());
    p.borrow_mut().is_attached = 1;
}

/// Copia n_page páginas da b-tree de origem para o destino.
pub fn backup_step(p: &BackupRef, n_page: i32) -> i32 {
    let mut rc: i32;
    let dest_mode: i32; // Modo de journal do destino
    let pgsz_src: i32; // Tamanho de página da origem
    let pgsz_dest: i32; // Tamanho de página do destino

    let p_src_db = p.borrow().p_src_db.clone();
    let p_dest_db = p.borrow().p_dest_db.clone();
    let p_src = p.borrow().p_src.clone();
    let p_dest = p.borrow().p_dest.clone();

    mutex_enter(&p_src_db.borrow().mutex);
    btree_enter(&mut p_src.borrow_mut());
    if let Some(db) = &p_dest_db {
        mutex_enter(&db.borrow().mutex);
    }

    rc = p.borrow().rc;
    if !is_fatal_error(rc) {
        let p_src_pager = btree_pager(&p_src); // Paginador de origem
        let p_dest_pager = btree_pager(&p_dest); // Paginador de destino
        let mut ii: i32; // Variável de iteração
        let mut n_src_page: i32; // Tamanho do banco de origem em páginas
        let mut b_close_trans = false; // Verdadeiro se o banco de origem precisa ser destravado
        let bt_src = p_src.borrow().p_bt.clone().unwrap();
        let bt_dest = p_dest.borrow().p_bt.clone().unwrap();

        // Se o paginador de origem está numa transação de escrita, devolve
        // SQLITE_BUSY imediatamente.
        if p_dest_db.is_some() && bt_src.borrow().in_transaction == TRANS_WRITE {
            rc = SQLITE_BUSY;
        } else {
            rc = SQLITE_OK;
        }

        // Se não há transação de leitura aberta no banco de origem, abre uma
        // agora. Se uma transação for aberta aqui, ela será fechada antes de a
        // função sair.
        if rc == SQLITE_OK && SQLITE_TXN_NONE == btree_txn_state(&p_src) {
            rc = btree_begin_trans(&p_src, 0, None);
            b_close_trans = true;
        }

        // Se o banco de destino ainda não foi travado (ou seja, se esta é a
        // primeira chamada de backup_step() desta cópia), tenta ajustar seu
        // tamanho de página ao da origem. Isso é especialmente importante em
        // sistemas ZipVFS, onde não é possível criar um arquivo de banco com um
        // tamanho de página escrevendo nele com outro.
        if p.borrow().b_dest_locked == 0
            && rc == SQLITE_OK
            && set_dest_pgsz(&p.borrow()) == SQLITE_NOMEM
        {
            rc = SQLITE_NOMEM;
        }

        // Trava o banco de destino, se ainda não estiver travado.
        if SQLITE_OK == rc && p.borrow().b_dest_locked == 0 {
            let mut i_dest_schema: i32 = 0;
            rc = btree_begin_trans(&p_dest, 2, Some(&mut i_dest_schema));
            if SQLITE_OK == rc {
                p.borrow_mut().i_dest_schema = i_dest_schema as u32;
                p.borrow_mut().b_dest_locked = 1;
            }
        }

        // Não permite a cópia se o destino está em modo WAL e os tamanhos de
        // página de origem e destino são diferentes.
        pgsz_src = btree_get_page_size(&p_src);
        pgsz_dest = btree_get_page_size(&p_dest);
        dest_mode = pager_get_journal_mode(&btree_pager(&p_dest).borrow());
        if SQLITE_OK == rc
            && (dest_mode == PAGER_JOURNALMODE_WAL || pager_is_memdb(&p_dest_pager) != 0)
            && pgsz_src != pgsz_dest
        {
            rc = SQLITE_READONLY;
        }

        // Agora que há um bloqueio de leitura no banco de origem, pergunta ao
        // paginador de origem quantas páginas o banco tem.
        n_src_page = btree_last_page(&p_src) as i32;
        debug_assert!(n_src_page >= 0);
        ii = 0;
        while (n_page < 0 || ii < n_page) && p.borrow().i_next <= (n_src_page as Pgno) && rc == 0 {
            let i_src_pg: Pgno = p.borrow().i_next; // Número da página de origem
            if i_src_pg != pending_byte_page(&bt_src.borrow()) {
                let mut p_src_pg: Option<PgHdrRef> = None; // Objeto da página de origem
                rc = pager_get(Some(&p_src_pager), i_src_pg, &mut p_src_pg, PAGER_GET_READONLY);
                if rc == SQLITE_OK {
                    rc = backup_one_page(
                        &p.borrow(),
                        i_src_pg,
                        &p_src_pg.as_ref().unwrap().borrow().p_data,
                        0,
                    );
                    pager_unref(p_src_pg.as_ref());
                }
            }
            p.borrow_mut().i_next += 1;
            ii += 1;
        }
        if rc == SQLITE_OK {
            let i_next = p.borrow().i_next;
            p.borrow_mut().n_pagecount = n_src_page as Pgno;
            p.borrow_mut().n_remaining = ((n_src_page + 1) as Pgno).wrapping_sub(i_next);
            if i_next > (n_src_page as Pgno) {
                rc = SQLITE_DONE;
            } else if p.borrow().is_attached == 0 {
                attach_backup_object(p);
            }
        }

        // Atualiza o campo de versão do esquema no banco de destino. Isso
        // garante que a versão do esquema realmente mude quando origem e destino
        // têm a mesma versão.
        if rc == SQLITE_DONE {
            if n_src_page == 0 {
                rc = btree_new_db(&p_dest);
                n_src_page = 1;
            }
            if rc == SQLITE_OK || rc == SQLITE_DONE {
                rc = btree_update_meta(&p_dest, 1, p.borrow().i_dest_schema.wrapping_add(1));
            }
            if rc == SQLITE_OK {
                if let Some(db) = &p_dest_db {
                    reset_all_schemas_of_connection(&mut db.borrow_mut());
                }
                if dest_mode == PAGER_JOURNALMODE_WAL {
                    rc = btree_set_version(&p_dest, 2);
                }
            }
            if rc == SQLITE_OK {
                let n_dest_truncate: i32;
                // Define n_dest_truncate como o número final de páginas do banco
                // de destino. A complicação é que o tamanho de página do destino
                // pode ser diferente do da origem.
                //
                // Se o tamanho de página da origem é menor que o do destino,
                // arredonda para cima. Nesse caso a chamada a os_truncate() mais
                // abaixo acerta o tamanho do arquivo. Mesmo assim é importante
                // chamar pager_truncate_image() aqui, para que as páginas do
                // arquivo de destino além da marca n_dest_truncate sejam gravadas
                // no journal por pager_commit_phase_one() antes de serem
                // destruídas pelo truncamento do arquivo.
                debug_assert!(pgsz_src == btree_get_page_size(&p_src));
                debug_assert!(pgsz_dest == btree_get_page_size(&p_dest));
                if pgsz_src < pgsz_dest {
                    let ratio = pgsz_dest / pgsz_src;
                    let mut n = (n_src_page + ratio - 1) / ratio;
                    if n == pending_byte_page(&bt_dest.borrow()) as i32 {
                        n -= 1;
                    }
                    n_dest_truncate = n;
                } else {
                    n_dest_truncate = n_src_page * (pgsz_src / pgsz_dest);
                }
                debug_assert!(n_dest_truncate > 0);

                if pgsz_src < pgsz_dest {
                    // Se o tamanho de página da origem é menor que o do destino,
                    // duas coisas extras podem precisar acontecer:
                    //
                    //   * o destino pode precisar ser truncado, e
                    //
                    //   * dados guardados nas páginas logo após a página do byte
                    //     pendente no banco de origem podem precisar ser copiados
                    //     para o banco de destino.
                    let i_size: i64 = (pgsz_src as i64) * (n_src_page as i64);
                    let mut i_pg: Pgno;
                    let mut n_dst_page: i32 = 0;
                    let mut i_off: i64;
                    let i_end: i64;

                    debug_assert!(pager_file(&p_dest_pager.borrow()).is_some());
                    debug_assert!(
                        n_dest_truncate == 0
                            || (n_dest_truncate as i64) * (pgsz_dest as i64) >= i_size
                            || (n_dest_truncate == (pending_byte_page(&bt_dest.borrow()) as i32 - 1)
                                && i_size >= pending_byte()
                                && i_size <= pending_byte() + (pgsz_dest as i64))
                    );

                    // Este bloco garante que todos os dados necessários para
                    // recriar o banco original foram guardados no journal do
                    // p_dest_pager e que o journal foi sincronizado com o disco.
                    // Daqui em diante é seguro modificar o arquivo do banco de
                    // qualquer modo, sabendo que, se faltar energia, o banco
                    // original será reconstruído a partir do journal.
                    pager_pagecount(&p_dest_pager, &mut n_dst_page);
                    i_pg = n_dest_truncate as Pgno;
                    while rc == SQLITE_OK && i_pg <= (n_dst_page as Pgno) {
                        if i_pg != pending_byte_page(&bt_dest.borrow()) {
                            let mut p_pg: Option<PgHdrRef> = None;
                            rc = pager_get(Some(&p_dest_pager), i_pg, &mut p_pg, 0);
                            if rc == SQLITE_OK {
                                rc = pager_write(p_pg.as_ref().unwrap());
                                pager_unref(p_pg.as_ref());
                            }
                        }
                        i_pg = i_pg.wrapping_add(1);
                    }
                    if rc == SQLITE_OK {
                        rc = pager_commit_phase_one(&p_dest_pager, None, 1);
                    }

                    // Escreve as páginas extras e trunca o arquivo do banco, se
                    // necessário.
                    i_end = min(pending_byte() + (pgsz_dest as i64), i_size);
                    i_off = pending_byte() + (pgsz_src as i64);
                    while rc == SQLITE_OK && i_off < i_end {
                        let mut p_src_pg: Option<PgHdrRef> = None;
                        let i_src_pg: Pgno = ((i_off / pgsz_src as i64) + 1) as Pgno;
                        rc = pager_get(Some(&p_src_pager), i_src_pg, &mut p_src_pg, 0);
                        if rc == SQLITE_OK {
                            let z_data = p_src_pg.as_ref().unwrap().borrow();
                            let mut dest = p_dest_pager.borrow_mut();
                            rc = os_write(
                                dest.fd.as_mut().unwrap(),
                                &z_data.p_data[..pgsz_src as usize],
                                i_off,
                            );
                        }
                        pager_unref(p_src_pg.as_ref());
                        i_off += pgsz_src as i64;
                    }
                    if rc == SQLITE_OK {
                        rc = backup_truncate_file(p_dest_pager.borrow_mut().fd.as_mut().unwrap(), i_size);
                    }

                    // Sincroniza o arquivo do banco com o disco.
                    if rc == SQLITE_OK {
                        rc = pager_sync(&p_dest_pager, None);
                    }
                } else {
                    pager_truncate_image(&p_dest_pager, n_dest_truncate as Pgno);
                    rc = pager_commit_phase_one(&p_dest_pager, None, 0);
                }

                // Termina o commit da transação no banco de destino.
                if SQLITE_OK == rc {
                    rc = btree_commit_phase_two(&p_dest, 0);
                    if SQLITE_OK == rc {
                        rc = SQLITE_DONE;
                    }
                }
            }
        }

        // Se b_close_trans é verdadeiro, esta função abriu uma transação de
        // leitura no banco de origem. Fecha-a aqui. Não é preciso conferir os
        // retornos dos métodos da b-tree, pois "confirmar" uma transação
        // somente de leitura não pode falhar.
        if b_close_trans {
            btree_commit_phase_one(&p_src, None);
            btree_commit_phase_two(&p_src, 0);
        }

        if rc == SQLITE_IOERR_NOMEM {
            rc = SQLITE_NOMEM_BKPT;
        }
        p.borrow_mut().rc = rc;
    }
    if let Some(db) = &p_dest_db {
        mutex_leave(&db.borrow().mutex);
    }
    btree_leave(&mut p_src.borrow_mut());
    mutex_leave(&p_src_db.borrow().mutex);
    rc
}


// ---- part_001.rs ----

// Convenções assumidas (a integração fica com o tech lead):
//   - `Backup` e `BackupRef = Rc<RefCell<Backup>>` vêm da parte 000: o backup é
//     compartilhado entre o chamador da API e a lista encadeada `p_next` mantida
//     pelo pager de origem.
//   - `Sqlite3Ref`, `BtreeRef`, `PagerRef` são `Rc<RefCell<_>>`; `db.mutex` é um `MutexRef`.
//   - `is_fatal_error`, `backup_one_page` e `backup_step` vêm da parte 000.
//   - `pager_backup_ptr` devolve `&mut Option<BackupRef>` (cabeça da lista do pager).

/// Libera todos os recursos associados a um identificador `sqlite3_backup`.
/// Corresponde a `sqlite3_backup_finish`.
pub fn backup_finish(p: Option<BackupRef>) -> i32 {
    // Entra nos mutexes
    let p = match p {
        None => return SQLITE_OK,
        Some(p) => p,
    };
    let p_src_db = p.borrow().p_src_db.clone();
    let p_src = p.borrow().p_src.clone();
    let p_dest_db = p.borrow().p_dest_db.clone();
    mutex_enter(&p_src_db.borrow().mutex);
    btree_enter(&mut p_src.borrow_mut());
    if let Some(dest_db) = &p_dest_db {
        mutex_enter(&dest_db.borrow().mutex);
    }

    // Desanexa este backup do pager de origem
    if p_dest_db.is_some() {
        p_src.borrow_mut().n_backup -= 1;
    }
    if p.borrow().is_attached != 0 {
        let pager = btree_pager(&p_src);
        let mut pager_mut = pager.borrow_mut();
        let pp = pager_backup_ptr(&mut pager_mut);
        let p_next = p.borrow().p_next.clone();
        let head_is_p = pp.as_ref().map_or(false, |head| Rc::ptr_eq(head, &p));
        if head_is_p {
            // *pp = p->pNext, com pp apontando para a cabeça da lista
            *pp = p_next;
        } else if let Some(head) = pp.clone() {
            // while( *pp!=p ) pp = &(*pp)->pNext;  *pp = p->pNext;
            let mut cur = head;
            loop {
                let next = cur.borrow().p_next.clone();
                match next {
                    Some(n) if Rc::ptr_eq(&n, &p) => {
                        cur.borrow_mut().p_next = p_next;
                        break;
                    }
                    Some(n) => cur = n,
                    None => break,
                }
            }
        }
    }

    // Se uma transação ainda está aberta no Btree, desfaz.
    let p_dest = p.borrow().p_dest.clone();
    btree_rollback(&p_dest, SQLITE_OK, 0);

    // Define o código de erro do identificador do banco de destino.
    let p_rc = p.borrow().rc;
    let rc = if p_rc == SQLITE_DONE { SQLITE_OK } else { p_rc };
    if let Some(dest_db) = &p_dest_db {
        error(&mut dest_db.borrow_mut(), rc);

        // Sai dos mutexes e libera a estrutura de contexto do backup.
        leave_mutex_and_close_zombie(dest_db);
    }
    btree_leave(&mut p_src.borrow_mut());
    if p_dest_db.is_some() {
        // EVIDENCE-OF: R-64852-21591 O objeto sqlite3_backup é criado por uma
        // chamada a sqlite3_backup_init() e destruído por uma chamada a
        // sqlite3_backup_finish().
        // O sqlite3_free(p) do C vira o descarte do último dono.
        drop(p);
    }
    leave_mutex_and_close_zombie(&p_src_db);
    rc
}

/// Retorna o número de páginas ainda por copiar na chamada mais recente a
/// `sqlite3_backup_step()`.
pub fn backup_remaining(p: &BackupRef) -> i32 {
    p.borrow().n_remaining as i32
}

/// Retorna o número total de páginas do banco de origem na chamada mais recente
/// a `sqlite3_backup_step()`.
pub fn backup_pagecount(p: &BackupRef) -> i32 {
    p.borrow().n_pagecount as i32
}

/// Chamada depois que o conteúdo da página `i_page` do banco de origem foi
/// modificado. Se a página já foi copiada para o destino, a cópia ficou
/// inválida e precisa ser atualizada antes do fim do backup.
///
/// Presume-se que o mutex do BtShared do banco de origem esteja seguro por
/// quem chama. (A `backupUpdate` estática do C; o nome `backup_update` já é da
/// `sqlite3BackupUpdate`, então esta leva o sufixo `_list`.)
fn backup_update_list(p: &BackupRef, i_page: Pgno, a_data: &[u8]) {
    let mut cur = Some(p.clone());
    while let Some(p) = cur {
        let (p_rc, i_next, p_dest_db) = {
            let b = p.borrow();
            (b.rc, b.i_next, b.p_dest_db.clone())
        };
        if !is_fatal_error(p_rc) && i_page < i_next {
            // O processo de backup p já copiou a página i_page, mas ela foi
            // modificada por uma transação no pager de origem. Copia os novos
            // dados para o backup.
            let dest_db = p_dest_db.expect("p_dest_db");
            mutex_enter(&dest_db.borrow().mutex);
            let rc = backup_one_page(&p.borrow(), i_page, a_data, 1);
            mutex_leave(&dest_db.borrow().mutex);
            debug_assert!(rc != SQLITE_BUSY && rc != SQLITE_LOCKED);
            if rc != SQLITE_OK {
                p.borrow_mut().rc = rc;
            }
        }
        cur = p.borrow().p_next.clone();
    }
}

/// `sqlite3BackupUpdate`: repassa a atualização à lista de backups, se houver.
pub fn backup_update(p_backup: Option<&BackupRef>, i_page: Pgno, a_data: &[u8]) {
    if let Some(p_backup) = p_backup {
        backup_update_list(p_backup, i_page, a_data);
    }
}

/// Reinicia o processo de backup. Chamada quando a camada do pager detecta que
/// o banco foi modificado por outra conexão: não há como saber quais páginas
/// já copiadas continuam válidas, então tudo recomeça.
///
/// Presume-se que o mutex do BtShared do banco de origem esteja seguro por
/// quem chama.
pub fn backup_restart(p_backup: Option<&BackupRef>) {
    let mut cur = p_backup.cloned();
    while let Some(p) = cur {
        p.borrow_mut().i_next = 1;
        cur = p.borrow().p_next.clone();
    }
}

/// Copia o conteúdo completo de `p_from` para `p_to`. Uma transação precisa
/// estar ativa nos dois arquivos.
///
/// O tamanho do arquivo de `p_to` pode diminuir. Se algo falhar, a transação em
/// `p_to` é desfeita. Se der certo, a transação é confirmada antes de retornar.
pub fn btree_copy_file(p_to: &BtreeRef, p_from: &BtreeRef) -> i32 {
    btree_enter(&mut p_to.borrow_mut());
    btree_enter(&mut p_from.borrow_mut());

    debug_assert!(btree_txn_state(p_to) == SQLITE_TXN_WRITE);
    let rc: i32 = 'copy_finished: {
        let mut rc: i32;
        let p_pager = btree_pager(p_to);
        let has_methods = p_pager.borrow().fd.as_ref().map_or(false, |f| f.p_methods.is_some());
        if has_methods {
            let mut n_byte: i64 =
                (btree_get_page_size(p_from) as i64) * (btree_last_page(p_from) as i64);
            rc = os_file_control(
                p_pager.borrow_mut().fd.as_mut().unwrap(),
                SQLITE_FCNTL_OVERWRITE,
                &mut n_byte,
            );
            if rc == SQLITE_NOTFOUND {
                rc = SQLITE_OK;
            }
            if rc != SQLITE_OK {
                break 'copy_finished rc;
            }
        }

        // Monta um objeto sqlite3_backup. `p_dest_db` precisa ser 0: é assim que
        // as implementações de sqlite3_backup_step() e sqlite3_backup_finish()
        // detectam que foram chamadas por esta função e não direto pelo usuário.
        let p_src_db = p_from
            .borrow()
            .db
            .as_ref()
            .and_then(|w| w.upgrade())
            .expect("db");
        let b: BackupRef = Rc::new(RefCell::new(Backup {
            p_dest_db: None,
            p_dest: p_to.clone(),
            i_dest_schema: 0,
            b_dest_locked: 0,
            i_next: 1,
            p_src_db,
            p_src: p_from.clone(),
            rc: 0,
            n_remaining: 0,
            n_pagecount: 0,
            is_attached: 0,
            p_next: None,
        }));

        // 0x7FFFFFFF é o limite rígido de páginas de um arquivo de banco. Ao
        // passá-lo como o número de páginas a copiar em sqlite3_backup_step(),
        // a cópia termina numa única chamada (salvo erro): b.rc fica em
        // SQLITE_DONE ou num código de erro.
        backup_step(&b, 0x7FFFFFFF);
        debug_assert!(b.borrow().rc != SQLITE_OK);

        rc = backup_finish(Some(b.clone()));
        if rc == SQLITE_OK {
            let p_bt = p_to.borrow().p_bt.clone().expect("p_bt");
            p_bt.borrow_mut().bts_flags &= !BTS_PAGESIZE_FIXED;
        } else {
            let p_dest = b.borrow().p_dest.clone();
            let pager = btree_pager(&p_dest);
            pager_clear_cache(&mut pager.borrow_mut());
        }

        debug_assert!(btree_txn_state(p_to) != SQLITE_TXN_WRITE);
        rc
    };

    // copy_finished:
    btree_leave(&mut p_from.borrow_mut());
    btree_leave(&mut p_to.borrow_mut());
    rc
}

