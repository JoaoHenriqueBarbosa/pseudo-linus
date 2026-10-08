// Mesclado das partes traduzidas de update_c (as partes quebram funções grandes no meio).
#![allow(unused_imports, ambiguous_glob_reexports)]
use crate::prelude::*;

// ---- part_001.rs ----

/// Localiza a tabela que se deseja atualizar.
fn locate_table() {
    // Seu código aqui
}

/// Inicializa o contexto de parsing.
fn initialize_parse_context() {
    // Seu código aqui
}


// ---- part_002.rs ----

            p_parse, i_eph, p_pk, p_changes, p_tab_list, p_where, p_order_by, p_limit
        );
#[cfg(not(sqlite_omit_subquery))]
{
    if is_view {
        i_data_cur = i_eph;
    }
}
    }
  }

  if n_change_from != 0 {
    multi_write(p_parse);
    e_one_pass = ONEPASS_OFF;
    n_key = n_pk;
    reg_key = i_pk;
  } else {
    if let Some(ref p_upsert) = p_upsert {
      // Se é um UPSERT, todos os cursores já foram abertos pelo INSERT
      // externo e o cursor de dados deve estar apontando para a linha que vai
      // ser atualizada. Então ignora o código que procura pelas linhas a
      // atualizar.
      p_winfo = None;
      e_one_pass = ONEPASS_SINGLE;
      expr_if_false(p_parse, p_where.as_deref(), label_break, SQLITE_JUMPIFNULL);
      b_finish_seek = 0;
    } else {
      // Comece a varredura do banco de dados.
      //
      // Não considere uma estratégia de passagem única para uma atualização
      // de múltiplas linhas se houver algo que possa atrapalhar o cursor
      // usado para fazer a UPDATE:
      //   (1) É uma UPDATE aninhada
      //   (2) Há gatilhos (triggers)
      //   (3) Há restrições FOREIGN KEY
      //   (4) Há manipuladores de conflito REPLACE
      //   (5) Há subconsultas na cláusula WHERE
      let mut flags = WHERE_ONEPASS_DESIRED;
      if !p_parse.nested
       && p_trigger.is_none()
       && !has_fk
       && !chng_key
       && !b_replace
       && (p_where.is_none() || !expr_has_property(p_where.as_deref(), EP_Subquery))
      {
        flags |= WHERE_ONEPASS_MULTIROW;
      }
      p_winfo = where_begin(p_parse, p_tab_list, p_where.as_deref(), None, None, None, flags, i_idx_cur);
      if p_winfo.is_none() {
          return; // goto update_cleanup;
      }

      // Uma estratégia de passagem única que pode atualizar mais de uma linha
      // não pode ser usada se qualquer coluna do índice usado para a varredura
      // está sendo atualizada. Caso contrário, se houver um índice em "b",
      // instruções como:
      //
      //   UPDATE t1 SET b=b+1 WHERE b>?
      //
      // poderiam criar um laço infinito.
      //
      // Volte a ONEPASS_OFF se where.c selecionou uma estratégia
      // ONEPASS_MULTI que usa um índice em que uma ou mais colunas estão
      // sendo atualizadas.
      e_one_pass = where_ok_one_pass(p_winfo.as_deref(), &ai_cur_one_pass);
      b_finish_seek = where_uses_deferred_seek(p_winfo.as_deref());
      if e_one_pass != ONEPASS_SINGLE {
        multi_write(p_parse);
        if e_one_pass == ONEPASS_MULTI {
          let i_cur = ai_cur_one_pass[1];
          if i_cur >= 0 && i_cur != i_data_cur && a_to_open[(i_cur - i_base_cur) as usize] != 0 {
            e_one_pass = ONEPASS_OFF;
          }
          debug_assert!(i_cur != i_data_cur || !has_rowid(p_tab));
        }
      }
    }

    if has_rowid(p_tab) {
      // Leia o rowid da linha atual da varredura WHERE. Em modo ONEPASS_OFF,
      // escreva o rowid na FIFO. Em qualquer um dos modos de passagem única,
      // deixe-o no registro reg_old_rowid.
      vdbe_add_op2(v, OP_Rowid, i_data_cur, reg_old_rowid);
      if e_one_pass == ONEPASS_OFF {
        a_reg_idx[n_all_idx as usize] = p_parse.n_mem + 1;
        p_parse.n_mem += 1;
        vdbe_add_op3(v, OP_Insert, i_eph, reg_row_set, reg_old_rowid);
      } else {
        if addr_open != 0 {
            vdbe_change_to_noop(v, addr_open);
        }
      }
    } else {
      // Leia a PK da linha atual em um arranjo de registros. Em modo
      // ONEPASS_OFF, serialize o arranjo em um registro e armazene-o na
      // tabela efêmera. Ou, em modo ONEPASS_SINGLE ou MULTI, mude a
      // instrução OP_OpenEphemeral para Noop (a tabela efêmera não é
      // necessária) e deixe os campos PK no arranjo de registros.
      for i in 0..n_pk {
        debug_assert!(p_pk.as_ref().map(|pk| pk.ai_column[i as usize] >= 0).unwrap_or(false));
        let i_col = p_pk.as_ref().map(|pk| pk.ai_column[i as usize]).unwrap_or(0);
        expr_code_get_column_of_table(v, p_tab, i_data_cur, i_col, i_pk + i);
      }
      if e_one_pass != ONEPASS_OFF {
        if addr_open != 0 {
            vdbe_change_to_noop(v, addr_open);
        }
        n_key = n_pk;
        reg_key = i_pk;
      } else {
        let affinity_str = index_affinity_str(db, p_pk.as_deref()).map(|s| s.as_bytes()).unwrap_or(&[]);
        vdbe_add_op4(v, OP_MakeRecord, i_pk, n_pk, reg_key, affinity_str, n_pk);
        vdbe_add_op4_int(v, OP_IdxInsert, i_eph, reg_key, i_pk, n_pk);
      }
    }
  }

  if p_upsert.is_none() {
    if n_change_from == 0 && e_one_pass != ONEPASS_MULTI {
      where_end(p_winfo.as_deref_mut());
    }

    if !is_view {
      let mut addr_once = 0;
      let mut i_not_used_1 = 0;
      let mut i_not_used_2 = 0;

      // Abra todo índice que precisa atualizar.
      if e_one_pass != ONEPASS_OFF {
        if ai_cur_one_pass[0] >= 0 {
            a_to_open[(ai_cur_one_pass[0] - i_base_cur) as usize] = 0;
        }
        if ai_cur_one_pass[1] >= 0 {
            a_to_open[(ai_cur_one_pass[1] - i_base_cur) as usize] = 0;
        }
      }

      if e_one_pass == ONEPASS_MULTI && (n_idx - if ai_cur_one_pass[1] >= 0 { 1 } else { 0 }) > 0 {
        addr_once = vdbe_add_op0(v, OP_Once);
      }
      open_table_and_indices(p_parse, p_tab, OP_OpenWrite, 0, i_base_cur,
                                 &mut a_to_open, &mut i_not_used_1, &mut i_not_used_2);
      if addr_once != 0 {
        vdbe_jump_here_or_pop_inst(v, addr_once);
      }
    }

    // Topo do laço de atualização
    if e_one_pass != ONEPASS_OFF {
      if ai_cur_one_pass[0] != i_data_cur
       && ai_cur_one_pass[1] != i_data_cur
#[cfg(sqlite_allow_rowid_in_view)]
       && !is_view
      {
        debug_assert!(p_pk.is_some());
        vdbe_add_op4_int(v, OP_NotFound, i_data_cur, label_break, reg_key, n_key);
      }
      if e_one_pass != ONEPASS_SINGLE {
        label_continue = vdbe_make_label(p_parse);
      }
      vdbe_add_op2(v, OP_IsNull, if p_pk.is_some() { reg_key } else { reg_old_rowid }, label_break);
    } else if p_pk.is_some() || n_change_from != 0 {
      label_continue = vdbe_make_label(p_parse);
      vdbe_add_op2(v, OP_Rewind, i_eph, label_break);
      addr_top = vdbe_current_addr(v);
      if n_change_from != 0 {
        if !is_view {
          if p_pk.is_some() {
            for i in 0..n_pk {
              vdbe_add_op3(v, OP_Column, i_eph, i, i_pk + i);
            }
            vdbe_add_op4_int(
                v, OP_NotFound, i_data_cur, label_continue, i_pk, n_pk
            );
          } else {
            vdbe_add_op2(v, OP_Rowid, i_eph, reg_old_rowid);
            vdbe_add_op3(
                v, OP_NotExists, i_data_cur, label_continue, reg_old_rowid
            );
          }
        }
      } else {
        vdbe_add_op2(v, OP_RowData, i_eph, reg_key);
        vdbe_add_op4_int(v, OP_NotFound, i_data_cur, label_continue, reg_key, 0);
      }
    } else {
      vdbe_add_op2(v, OP_Rewind, i_eph, label_break);
      label_continue = vdbe_make_label(p_parse);
      addr_top = vdbe_add_op2(v, OP_Rowid, i_eph, reg_old_rowid);
      vdbe_add_op3(v, OP_NotExists, i_data_cur, label_continue, reg_old_rowid);
    }
  }

  // Se o valor do rowid mudar, defina o registro reg_new_rowid para conter
  // o novo valor. Se o rowid não está sendo modificado, então reg_new_rowid
  // é o mesmo registro que reg_old_rowid, que já está preenchido.
  debug_assert!(chng_key || p_trigger.is_some() || has_fk || reg_old_rowid == reg_new_rowid);
  if chng_rowid {
    debug_assert!(i_rowid_expr >= 0);
    if n_change_from == 0 {
      expr_code(p_parse, p_rowid_expr.as_deref(), reg_new_rowid);
    } else {
      vdbe_add_op3(v, OP_Column, i_eph, i_rowid_expr, reg_new_rowid);
    }
    vdbe_add_op1(v, OP_MustBeInt, reg_new_rowid);
  }

  // Calcule o conteúdo antigo pré-UPDATE da linha sendo alterada, se essa
  // informação for necessária
  if chng_pk || has_fk || p_trigger.is_some() {
    let mut old_mask = if has_fk {
        fk_old_mask(p_parse, p_tab)
    } else {
        0u32
    };
    old_mask |= trigger_col_mask(p_parse,
        p_trigger.as_deref(), p_changes, 0, TRIGGER_BEFORE | TRIGGER_AFTER, p_tab, on_error
    );
    for i in 0..p_tab.n_col {
      let col_flags = p_tab.a_col[i as usize].col_flags;
      let k = table_column_to_storage(p_tab, i) + reg_old;
      if old_mask == 0xffffffff
       || (i < 32 && (old_mask & maskbit32(i)) != 0)
       || (col_flags & COLFLAG_PRIMKEY) != 0
      {
        expr_code_get_column_of_table(v, p_tab, i_data_cur, i, k);
      } else {
        vdbe_add_op2(v, OP_Null, 0, k);
      }
    }
    if chng_rowid == 0 && p_pk.is_none() {
#[cfg(sqlite_allow_rowid_in_view)]
      if is_view {
          vdbe_add_op2(v, OP_Null, 0, reg_old_rowid);
      }
      vdbe_add_op2(v, OP_Copy, reg_old_rowid, reg_new_rowid);
    }
  }

  // Preencha o arranjo de registros começando em reg_new com os dados da nova
  // linha. Este arranjo é usado para verificar constantes, criar os novos
  // registros de tabela e índice, e como os valores para qualquer novo.*
  // referência feita por gatilhos.
  //
  // Se há um ou mais gatilhos BEFORE, então não preencha os registros
  // associados às colunas que são (a) não modificadas por esta instrução
  // UPDATE e (b) não acessadas por novo.* referências. Os valores de
  // registros não modificados pelo UPDATE devem ser recarregados do banco de
  // dados após os gatilhos BEFORE dispararem de qualquer forma (pois o gatilho
  // pode tê-los modificado). Então não carregar aqueles que não vão ser
  // usados elimina alguns opcodes redundantes.
  let new_mask = trigger_col_mask(
      p_parse, p_trigger.as_deref(), p_changes, 1, TRIGGER_BEFORE, p_tab, on_error
  );
  let mut k = reg_new;
  for i in 0..p_tab.n_col {
    if i == p_tab.i_pkey {
      vdbe_add_op2(v, OP_Null, 0, k);
    } else if (p_tab.a_col[i as usize].col_flags & COLFLAG_GENERATED) != 0 {
      if (p_tab.a_col[i as usize].col_flags & COLFLAG_VIRTUAL) != 0 {
          k -= 1;
      }
    } else {
      let j = a_xref[i as usize];
      if j >= 0 {
        if n_change_from != 0 {
          let n_off = if is_view { p_tab.n_col } else { n_pk };
          debug_assert!(e_one_pass == ONEPASS_OFF);
          vdbe_add_op3(v, OP_Column, i_eph, n_off + j, k);
        } else if let Some(ref p_ch) = p_changes {
          expr_code(p_parse, p_ch.a[j as usize].p_expr.as_deref(), k);
        }
      } else if (tmask & TRIGGER_BEFORE) == 0 || i > 31 || (new_mask & maskbit32(i)) != 0 {
        // Este ramo carrega o valor de uma coluna que não será alterada em um
        // registro. Isto é feito se não há gatilhos BEFORE, ou se há um ou mais
        // gatilhos BEFORE que usam este valor por uma referência novo.* em um
        // programa de gatilho.
        expr_code_get_column_of_table(v, p_tab, i_data_cur, i, k);
        b_finish_seek = 0;
      } else {
        vdbe_add_op2(v, OP_Null, 0, k);
      }
    }
    k += 1;
  }
#[cfg(not(sqlite_omit_generated_columns))]
{
  if (p_tab.tab_flags & TF_HasGenerated) != 0 {
    compute_generated_columns(p_parse, reg_new, p_tab);
  }
}

  // Dispare gatilhos BEFORE UPDATE. Isto acontece antes da verificação de
  // restrições. Poderíamos argumentar que isto é errado.
  if (tmask & TRIGGER_BEFORE) != 0 {
    table_affinity(v, p_tab, reg_new);
    code_row_trigger(p_parse, p_trigger.as_deref(), TK_UPDATE, p_changes.as_deref(),
        TRIGGER_BEFORE, p_tab, reg_old_rowid, on_error, label_continue);

    if !is_view {
      // O row-trigger pode ter deletado a linha sendo atualizada. Neste
      // caso, pule para a próxima linha. Nenhuma atualização ou gatilho
      // AFTER é necessário. Este comportamento é deixado indefinido na
      // documentação.
      if p_pk.is_some() {
        vdbe_add_op4_int(v, OP_NotFound, i_data_cur, label_continue, reg_key, n_key);
      } else {
        vdbe_add_op3(v, OP_NotExists, i_data_cur, label_continue, reg_old_rowid);
      }

      // Laço pós-trigger-BEFORE-reload:
      // Se o gatilho não deletou, pode ainda ter modificado algumas colunas
      // da linha sendo atualizada. Carregue os valores para todas as colunas
      // não modificadas pelo comando update em seus registros caso isto tenha
      // acontecido. Apenas colunas não modificadas são recarregadas.
      // Os valores computados para colunas modificadas usam os valores antes
      // do gatilho BEFORE rodar. Veja caso de teste trigger1-18.0 (adicionado
      // 2018-04-26) para um exemplo.
      let mut k = reg_new;
      for i in 0..p_tab.n_col {
        if (p_tab.a_col[i as usize].col_flags & COLFLAG_GENERATED) != 0 {
          if (p_tab.a_col[i as usize].col_flags & COLFLAG_VIRTUAL) != 0 {
              k -= 1;
          }
        } else if a_xref[i as usize] < 0 && i != p_tab.i_pkey {
          expr_code_get_column_of_table(v, p_tab, i_data_cur, i, k);
        }
        k += 1;
      }
#[cfg(not(sqlite_omit_generated_columns))]
{
      if (p_tab.tab_flags & TF_HasGenerated) != 0 {
        compute_generated_columns(p_parse, reg_new, p_tab);
      }
}
    }
  }

  if !is_view {
    // Faça verificações de restrição.
    debug_assert!(reg_old_rowid > 0);
    generate_constraint_checks(p_parse, p_tab, &a_reg_idx, i_data_cur, i_idx_cur,
        reg_new_rowid, reg_old_rowid, chng_key, on_error, label_continue, &mut b_replace,
        &a_xref, 0);

    // Se a manipulação de conflito REPLACE pode ter sido usada, ou se a PK
    // da linha está mudando, o GenerateConstraintChecks() acima pode ter
    // movido o cursor i_data_cur. Re-procure.
    if b_replace || chng_key {
      if p_pk.is_some() {
        vdbe_add_op4_int(v, OP_NotFound, i_data_cur, label_continue, reg_key, n_key);
      } else {
        vdbe_add_op3(v, OP_NotExists, i_data_cur, label_continue, reg_old_rowid);
      }
    }

    // Faça verificações de restrição FK.
    if has_fk {
      fk_check(p_parse, p_tab, reg_old_rowid, 0, &a_xref, chng_key);
    }

    // Apague as entradas de índice associadas ao registro atual.
    generate_row_index_delete(p_parse, p_tab, i_data_cur, i_idx_cur, &a_reg_idx, -1);

    // Devemos executar o opcode OP_FinishSeek para resolver um
    // OP_DeferredSeek anterior se houver qualquer possibilidade de que não
    // tenha havido opcodes OP_Column desde que o OP_DeferredSeek foi emitido.
    // Mas queremos evitar o OP_FinishSeek se possível, pois executá-lo
    // custa ciclos de CPU.
    if b_finish_seek {
      vdbe_add_op1(v, OP_FinishSeek, i_data_cur);
    }

    // Se mudando o valor do rowid, ou se há restrições de chave estrangeira
    // a processar, apague o registro antigo. Caso contrário, adicione um noop
    // OP_Delete para invocar o gancho pré-atualização.
    //
    // Que (reg_new == reg_new_rowid + 1) é verdade também é importante para
    // o gancho pré-atualização. Se o chamador invocar preupdate_new(), o valor
    // retornado é copiado da célula de memória (reg_new_rowid + 1 + i_col),
    // onde i_col é o índice de coluna fornecido pelo usuário.
    debug_assert!(reg_new == reg_new_rowid + 1);
#[cfg(sqlite_enable_preupdate_hook)]
{
    vdbe_add_op3(v, OP_Delete, i_data_cur,
        OPFLAG_ISUPDATE | if (has_fk > 1) || chng_key { 0 } else { OPFLAG_ISNOOP },
        reg_new_rowid
    );
    if e_one_pass == ONEPASS_MULTI {
      debug_assert!(has_fk == 0 && chng_key == 0);
      vdbe_change_p5(v, OPFLAG_SAVEPOSITION);
    }
    if !p_parse.nested {
      vdbe_append_p4(v, p_tab, P4_TABLE);
    }
}
#[cfg(not(sqlite_enable_preupdate_hook))]
{
    if (has_fk > 1) || chng_key {
      vdbe_add_op2(v, OP_Delete, i_data_cur, 0);
    }
}

    if has_fk {
      fk_check(p_parse, p_tab, 0, reg_new_rowid, &a_xref, chng_key);
    }

    // Insira as novas entradas de índice e o novo registro.
    complete_insertion(
        p_parse, p_tab, i_data_cur, i_idx_cur, reg_new_rowid, &a_reg_idx,
        OPFLAG_ISUPDATE | if e_one_pass == ONEPASS_MULTI { OPFLAG_SAVEPOSITION } else { 0 },
        0, 0
    );

    // Faça qualquer operação ON CASCADE, SET NULL ou SET DEFAULT necessária
    // para manipular linhas (possivelmente em outras tabelas) que se referem
    // por chave estrangeira à linha apenas atualizada.
    if has_fk {
      fk_actions(p_parse, p_tab, p_changes.as_deref(), reg_old_rowid, &a_xref, chng_key);
    }
  }

  // Incremente o contador de linhas
  if reg_row_count != 0 {
    vdbe_add_op2(v, OP_AddImm, reg_row_count, 1);
  }

  if p_trigger.is_some() {
    code_row_trigger(p_parse, p_trigger.as_deref(), TK_UPDATE, p_changes.as_deref(),
        TRIGGER_AFTER, p_tab, reg_old_rowid, on_error, label_continue);
  }

  // Repita o acima com o próximo registro a atualizar, até que todos os
  // registros selecionados pela cláusula WHERE tenham sido atualizados.
  if e_one_pass == ONEPASS_SINGLE {
    // Nada a fazer no fim do laço para uma passagem única
  } else if e_one_pass == ONEPASS_MULTI {
    vdbe_resolve_label(v, label_continue);
    where_end(p_winfo.as_deref_mut());
  } else {
    vdbe_resolve_label(v, label_continue);
    vdbe_add_op2(v, OP_Next, i_eph, addr_top);
  }
  vdbe_resolve_label(v, label_break);

  // Atualize a tabela sqlite_sequence armazenando o conteúdo dos valores
  // máximos do contador de rowid gravados ao inserir em tabelas
  // autoincrement.
  if p_parse.nested == 0 && p_parse.p_trigger_tab.is_none() && p_upsert.is_none() {
    autoincrement_end(p_parse);
  }

  // Devolva o número de linhas que foram alteradas, se estamos rastreando
  // essa informação.
  if reg_row_count != 0 {
    code_change_count(v, reg_row_count, b"rows updated");
  }

  // update_cleanup:
  auth_context_pop(&mut s_context);
  // db_free() também liberta a_xref[], a_reg_idx[] e a_to_open[]
#[cfg(sqlite_enable_update_delete_limit)]
{
  // expr_list_delete(db, p_order_by);
  // expr_delete(db, p_limit);
}



// ---- part_003.rs ----

/// Gera código para um UPDATE de uma tabela virtual.
///
/// Existem duas estratégias possíveis: a padrão e a especial "onepass".
/// Onepass é usada apenas se a implementação da tabela virtual indicar
/// que pWhere pode corresponder a no máximo uma linha.
///
/// A estratégia padrão é criar uma tabela efêmera que contém, para cada linha
/// a ser modificada:
///
///   (A)  O rowid original dessa linha.
///   (B)  O rowid revisado para a linha.
///   (C)  O conteúdo de cada coluna na linha.
///
/// Então passa pelos conteúdos desta tabela efêmera executando um VUpdate
/// para cada linha. Quando terminar, a tabela efêmera é descartada.
///
/// A estratégia "onepass" não usa uma tabela efêmera. Em vez disso, armazena
/// os mesmos valores (A, B e C acima) em uma matriz de registros e faz uma
/// única invocação de VUpdate.
fn update_virtual_table(
    p_parse: &mut Parse,
    p_src: &SrcList,
    p_tab: &Table,
    p_changes: &ExprList,
    p_rowid: Option<&Expr>,
    a_xref: &[i32],
    p_where: Option<&Expr>,
    on_error: u8,
) {
    let v_opt = p_parse.p_vdbe.clone();
    if v_opt.is_none() {
        return;
    }

    let v = v_opt.unwrap();
    let mut v = v.borrow_mut();

    let ephemeral_table = p_parse.n_tab;
    p_parse.n_tab += 1;

    let p_v_tab: *const u8 = unsafe {
        get_v_table(&p_parse.db, p_tab) as *const u8
    };

    let mut p_winfo: Option<WhereInfoRef> = None;
    let n_arg: i32 = 2 + p_tab.n_col as i32;
    let reg_arg: i32 = p_parse.n_mem + 1;
    p_parse.n_mem += n_arg;
    let reg_rec: i32;
    let reg_rowid: i32;
    let i_csr = p_src.a[0].i_cursor;
    let mut a_dummy = [0i32; 2];
    let mut e_one_pass: i32;
    let mut addr: i32;

    addr = vdbe_add_op2(&mut v, OP_OPENEPHEMERAL as i32, ephemeral_table, n_arg);

    if p_src.n_src > 1 {
        let mut p_pk: Option<Rc<RefCell<Index>>> = None;
        let mut p_row: Option<Box<Expr>> = None;
        let mut p_list: Option<Box<ExprList>> = None;

        if has_rowid(p_tab) {
            if let Some(rowid_expr) = p_rowid {
                p_row = expr_dup(p_parse, rowid_expr, 0).map(Box::new);
            } else {
                p_row = Some(Box::new(Expr {
                    op: TK_ROW as u8,
                    ..Default::default()
                }));
            }
        } else {
            p_pk = primary_key_index(p_tab);
            if let Some(pk_ref) = &p_pk {
                let pk = pk_ref.borrow();
                let i_pk = pk.ai_column[0] as i32;
                if a_xref[i_pk as usize] >= 0 {
                    let idx = a_xref[i_pk as usize] as usize;
                    p_row = expr_dup(p_parse, &p_changes.a[idx].p_expr, 0).map(Box::new);
                } else {
                    p_row = expr_row_column(p_parse, i_pk).map(Box::new);
                }
            }
        }

        if let Some(row) = p_row {
            p_list = Some(Box::new(ExprList {
                n_expr: 1,
                a: vec![ExprListItem {
                    p_expr: row,
                    ..Default::default()
                }],
            }));
        }

        let mut i = 0;
        while i < p_tab.n_col as i32 {
            if a_xref[i as usize] >= 0 {
                let idx = a_xref[i as usize] as usize;
                if let Some(dup_expr) = expr_dup(p_parse, &p_changes.a[idx].p_expr, 0) {
                    if let Some(ref mut list) = p_list {
                        list.n_expr += 1;
                        list.a.push(ExprListItem {
                            p_expr: Box::new(dup_expr),
                            ..Default::default()
                        });
                    }
                }
            } else {
                if let Some(mut row_expr) = expr_row_column(p_parse, i) {
                    row_expr.op2 = OPFLAG_NOCHNG as u8;

                    if let Some(ref mut list) = p_list {
                        list.n_expr += 1;
                        list.a.push(ExprListItem {
                            p_expr: Box::new(row_expr),
                            ..Default::default()
                        });
                    }
                }
            }

            i += 1;
        }

        if let Some(list) = p_list {
            update_from_select(
                p_parse,
                ephemeral_table,
                p_pk.as_ref(),
                &list,
                p_src,
                p_where,
                0,
                0,
            );
            expr_list_delete(p_parse.db.upgrade().as_ref(), Some(list));
        }

        e_one_pass = ONEPASS_OFF;
    } else {
        reg_rec = p_parse.n_mem + 1;
        p_parse.n_mem += 1;
        reg_rowid = p_parse.n_mem + 1;
        p_parse.n_mem += 1;

        p_winfo = where_begin(
            p_parse,
            p_src,
            p_where,
            0,
            0,
            0,
            WHERE_ONEPASS_DESIRED,
            0,
        );

        if p_winfo.is_none() {
            return;
        }

        let mut i = 0;
        while i < p_tab.n_col as i32 {
            if a_xref[i as usize] >= 0 {
                let idx = a_xref[i as usize] as usize;
                expr_code(p_parse, Some(&p_changes.a[idx].p_expr), reg_arg + 2 + i);
            } else {
                vdbe_add_op3(&mut v, OP_VCOLUMN as i32, i_csr, i, reg_arg + 2 + i);
                vdbe_change_p5(&mut v, OPFLAG_NOCHNG as u16);
            }

            i += 1;
        }

        if has_rowid(p_tab) {
            vdbe_add_op2(&mut v, OP_ROWID as i32, i_csr, reg_arg);
            if let Some(rowid_expr) = p_rowid {
                expr_code(p_parse, Some(rowid_expr), reg_arg + 1);
            } else {
                vdbe_add_op2(&mut v, OP_ROWID as i32, i_csr, reg_arg + 1);
            }
        } else {
            if let Some(pk_ref) = primary_key_index(p_tab) {
                let pk = pk_ref.borrow();
                let i_pk = pk.ai_column[0] as i32;
                vdbe_add_op3(&mut v, OP_VCOLUMN as i32, i_csr, i_pk, reg_arg);
                vdbe_add_op2(&mut v, OP_SCOPY as i32, reg_arg + 2 + i_pk, reg_arg + 1);
            }
        }

        e_one_pass = where_ok_one_pass(p_winfo.as_ref(), &mut a_dummy);

        if e_one_pass != 0 {
            vdbe_change_to_noop(&mut v, addr);
            vdbe_add_op1(&mut v, OP_CLOSE as i32, i_csr);
        } else {
            multi_write(p_parse);
            vdbe_add_op3(&mut v, OP_MAKERECORD as i32, reg_arg, n_arg, reg_rec);

            vdbe_change_p5(&mut v, OPFLAG_NOCHNG_MAGIC as u16);

            vdbe_add_op2(&mut v, OP_NEWROWID as i32, ephemeral_table, reg_rowid);
            vdbe_add_op3(&mut v, OP_INSERT as i32, ephemeral_table, reg_rec, reg_rowid);
        }
    }

    if e_one_pass == ONEPASS_OFF {
        if p_src.n_src == 1 {
            if let Some(winfo) = p_winfo {
                where_end(&winfo);
            }
        }

        addr = vdbe_add_op1(&mut v, OP_REWIND as i32, ephemeral_table);

        let mut i = 0;
        while i < n_arg {
            vdbe_add_op3(&mut v, OP_COLUMN as i32, ephemeral_table, i, reg_arg + i);
            i += 1;
        }
    }

    vtab_make_writable(p_parse, p_tab);
    vdbe_add_op4(&mut v, OP_VUPDATE as i32, 0, n_arg, reg_arg, p_v_tab);
    vdbe_change_p5(&mut v, if on_error == OE_DEFAULT { OE_ABORT } else { on_error } as u16);
    may_abort(p_parse);

    if e_one_pass == ONEPASS_OFF {
        vdbe_add_op2(&mut v, OP_NEXT as i32, ephemeral_table, addr + 1);
        vdbe_jump_here(&mut v, addr);
        vdbe_add_op2(&mut v, OP_CLOSE as i32, ephemeral_table, 0);
    } else {
        if let Some(winfo) = p_winfo {
            where_end(&winfo);
        }
    }
}

