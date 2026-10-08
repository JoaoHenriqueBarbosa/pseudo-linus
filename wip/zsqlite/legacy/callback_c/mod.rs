// Mesclado das partes traduzidas de callback_c (as partes quebram funções grandes no meio).
#![allow(unused_imports, ambiguous_glob_reexports)]
use crate::prelude::*;

// ---- part_000.rs ----

/// Pontuação de uma correspondência perfeita entre função e pedido.
pub const FUNC_PERFECT_MATCH: i32 = 6;

/// Invoca o callback "collation needed" para pedir uma sequência de ordenação
/// de nome `z_name` na codificação `enc`.
fn call_coll_needed(db: &Sqlite3Ref, enc: i32, z_name: &[u8]) {
    {
        let d = db.borrow();
        debug_assert!(d.x_coll_needed.is_none() || d.x_coll_needed16.is_none());
    }
    let cb = db.borrow().x_coll_needed.clone();
    if let Some(cb) = cb {
        let z_external = match db_str_dup(db, z_name) {
            Some(z) => z,
            None => return,
        };
        let arg = db.borrow().p_coll_needed_arg.clone();
        cb(arg, db, enc, &z_external);
    }
    let cb16 = db.borrow().x_coll_needed16.clone();
    if let Some(cb16) = cb16 {
        let p_tmp = value_new(db);
        value_set_str(&p_tmp, -1, z_name, SQLITE_UTF8, SQLITE_STATIC);
        let z_external = value_text(&p_tmp, SQLITE_UTF16NATIVE);
        if let Some(z_external) = z_external {
            let arg = db.borrow().p_coll_needed_arg.clone();
            let enc_db = db.borrow().enc as i32;
            cb16(arg, db, enc_db, &z_external);
        }
        value_free(p_tmp);
    }
}

/// Chamada quando a fábrica de ordenações não entrega a função na melhor
/// codificação, mas pode haver versões em outras codificações. Usa uma delas
/// se existir, evitando conversão UTF-8 <-> UTF-16 quando possível.
fn synth_coll_seq(db: &Sqlite3Ref, p_coll: &CollSeqRef) -> i32 {
    const A_ENC: [u8; 3] = [SQLITE_UTF16BE, SQLITE_UTF16LE, SQLITE_UTF8];
    let z = p_coll.get().z_name.clone();
    for i in 0..3 {
        let p_coll2 = find_coll_seq(db, A_ENC[i], Some(&z), false)
            .expect("find_coll_seq com nome nunca retorna nulo sem create");
        if p_coll2.get().x_cmp.is_some() {
            // memcpy(pColl, pColl2, sizeof(CollSeq))
            let copia = p_coll2.get();
            p_coll.set(copia);
            // Não copia o destrutor.
            p_coll.with_mut(|c| c.x_del = None);
            return SQLITE_OK;
        }
    }
    SQLITE_ERROR
}

/// Chamada numa sequência de ordenação antes do uso, para conferir que ela
/// está definida. Se preciso, chama o callback "collation needed" e, se isso
/// não resolver, substitui por uma equivalente em outra codificação.
pub fn check_coll_seq(p_parse: &mut Parse, p_coll: Option<&CollSeqRef>) -> i32 {
    if let Some(p_coll) = p_coll {
        if p_coll.get().x_cmp.is_none() {
            let z_name = p_coll.get().z_name.clone();
            let db = p_parse.db.clone();
            let enc_db = enc(&db);
            let p = get_coll_seq(p_parse, enc_db, Some(p_coll.clone()), &z_name);
            match p {
                None => return SQLITE_ERROR,
                Some(p) => debug_assert!(p.same_as(p_coll)),
            }
        }
    }
    SQLITE_OK
}

/// Localiza e retorna uma entrada da tabela hash `db.a_coll_seq`. Se a entrada
/// não existe e `create` é verdadeiro, cria uma nova; senão retorna `None`.
///
/// Cada valor guardado na hash é um vetor de três CollSeq: UTF-8, UTF-16le e
/// UTF-16be, todos com uma cópia do nome.
fn find_coll_seq_entry(db: &Sqlite3Ref, z_name: &[u8], create: bool) -> Option<CollSeqSetRef> {
    let mut p_coll = hash_find(&db.borrow().a_coll_seq, z_name);

    if p_coll.is_none() && create {
        let mut set: [CollSeq; 3] = [CollSeq::default(), CollSeq::default(), CollSeq::default()];
        set[0].z_name = z_name.to_vec();
        set[0].enc = SQLITE_UTF8;
        set[1].z_name = z_name.to_vec();
        set[1].enc = SQLITE_UTF16LE;
        set[2].z_name = z_name.to_vec();
        set[2].enc = SQLITE_UTF16BE;
        let novo: CollSeqSetRef = Rc::new(RefCell::new(set));
        let p_del = hash_insert(&mut db.borrow_mut().a_coll_seq, z_name, Some(Rc::clone(&novo)));

        // Se houve falha de malloc em hash_insert, ele devolve o ponteiro que
        // deve ser descartado (porque não entrou na tabela).
        debug_assert!(p_del.is_none() || p_del.as_ref().map(|d| Rc::ptr_eq(d, &novo)).unwrap_or(false));
        if p_del.is_some() {
            oom_fault(db);
            p_coll = None;
        } else {
            p_coll = Some(novo);
        }
    }
    p_coll
}

/// Retorna a CollSeq de nome `z_name` para a codificação `enc` no banco `db`.
///
/// Se a entrada não existe e `create` é verdadeiro, cria; senão retorna `None`.
/// `locate_coll_seq` é um invólucro que invoca a fábrica de ordenações e gera
/// mensagem de erro se a sequência não for encontrada.
pub fn find_coll_seq(db: &Sqlite3Ref, enc: u8, z_name: Option<&[u8]>, create: bool) -> Option<CollSeqRef> {
    debug_assert!(SQLITE_UTF8 == 1 && SQLITE_UTF16LE == 2 && SQLITE_UTF16BE == 3);
    debug_assert!(enc >= SQLITE_UTF8 && enc <= SQLITE_UTF16BE);
    if let Some(z_name) = z_name {
        find_coll_seq_entry(db, z_name, create).map(|set| CollSeqRef { set, idx: (enc - 1) as usize })
    } else {
        db.borrow().p_dflt_coll.clone()
    }
}

/// Muda a codificação de texto de uma conexão. A `p_dflt_coll` muda junto.
pub fn set_text_encoding(db: &Sqlite3Ref, enc: u8) {
    debug_assert!(enc == SQLITE_UTF8 || enc == SQLITE_UTF16LE || enc == SQLITE_UTF16BE);
    db.borrow_mut().enc = enc;
    // A função de ordenação padrão para todas as strings é BINARY.
    let dflt = find_coll_seq(db, enc, Some(STR_BINARY), false);
    db.borrow_mut().p_dflt_coll = dflt;
    expire_prepared_statements(db, 1);
}

/// Responsável por invocar o callback da fábrica de ordenações ou substituir
/// por uma sequência de outra codificação quando a pedida não existe na
/// codificação desejada.
///
/// Se `p_coll` não é nulo, aponta a sequência na codificação nativa do banco.
/// Retorna a sequência a usar ou `None`; sem sequência, deixa mensagem de erro.
pub fn get_coll_seq(p_parse: &mut Parse, enc: u8, p_coll: Option<CollSeqRef>, z_name: &[u8]) -> Option<CollSeqRef> {
    let db = p_parse.db.clone();

    let mut p = p_coll;
    if p.is_none() {
        p = find_coll_seq(&db, enc, Some(z_name), false);
    }
    if p.as_ref().map(|c| c.get().x_cmp.is_none()).unwrap_or(true) {
        // Nenhuma sequência deste tipo registrada nesta codificação. Chama a
        // fábrica para ver se ela fornece uma.
        call_coll_needed(&db, enc as i32, z_name);
        p = find_coll_seq(&db, enc, Some(z_name), false);
    }
    if let Some(pp) = &p {
        if pp.get().x_cmp.is_none() && synth_coll_seq(&db, pp) != 0 {
            p = None;
        }
    }
    debug_assert!(p.as_ref().map(|c| c.get().x_cmp.is_some()).unwrap_or(true));
    if p.is_none() {
        error_msg(p_parse, &[b"no such collation sequence: ".as_slice(), z_name].concat());
        p_parse.rc = SQLITE_ERROR_MISSING_COLLSEQ;
    }
    p
}

/// Retorna a sequência de ordenação, na codificação nativa do banco,
/// identificada por `z_name`. É um invólucro de `find_coll_seq` que invoca a
/// fábrica se o nome não for encontrado e gera mensagem de erro.
pub fn locate_coll_seq(p_parse: &mut Parse, z_name: &[u8]) -> Option<CollSeqRef> {
    let db = p_parse.db.clone();
    let enc = enc(&db);
    let initbusy = db.borrow().init.busy;

    let mut p_coll = find_coll_seq(&db, enc, Some(z_name), initbusy != 0);
    if initbusy == 0 && p_coll.as_ref().map(|c| c.get().x_cmp.is_none()).unwrap_or(true) {
        p_coll = get_coll_seq(p_parse, enc, p_coll, z_name);
    }

    p_coll
}

/// Durante a busca da melhor definição de função, avalia o quão bem a função
/// `p` atende ao pedido de `n_arg` argumentos na codificação `enc`.
///
/// Se `n_arg` é -1, só há correspondência (não zero) se `p.n_arg` também for -1.
/// Se `n_arg` é -2, procura qualquer função independente do número de
/// argumentos: `x_s_func` não nulo é correspondência perfeita, nulo não casa.
///
/// Retorno de 0 a 6: 0 sem correspondência; 1 conversão UTF8/16 e qualquer
/// número de argumentos; 2 troca de ordem de bytes UTF16 e qualquer número;
/// 3 codificação casa e qualquer número; 4 conversão UTF8/16, argumentos
/// exatos; 5 conversão de ordem UTF16, argumentos exatos; 6 perfeita.
fn match_quality(p: &FuncDefRef, n_arg: i32, enc: u8) -> i32 {
    let p = p.borrow();
    debug_assert!(p.n_arg >= -1);

    // Número de argumentos errado significa "sem correspondência".
    if p.n_arg as i32 != n_arg {
        if n_arg == -2 {
            return if p.x_s_func.is_none() { 0 } else { FUNC_PERFECT_MATCH };
        }
        if p.n_arg >= 0 {
            return 0;
        }
    }

    // Função com número específico de argumentos pontua mais que a variádica.
    let mut matched = if p.n_arg as i32 == n_arg { 4 } else { 1 };

    // Pontos extras se a codificação de texto casa.
    if enc as u32 == (p.func_flags & SQLITE_FUNC_ENCMASK) {
        matched += 2; // Codificação exata.
    } else if (enc as u32 & p.func_flags & 2) != 0 {
        matched += 1; // Ambas UTF16, mas com ordem de bytes diferente.
    }

    matched
}

/// Busca numa FuncDefHash uma função com o nome dado. Retorna a FuncDef
/// correspondente ou `None`.
pub fn function_search(h: usize, z_func: &[u8]) -> Option<FuncDefRef> {
    let mut p = builtin_functions_get(h);
    while let Some(cur) = p {
        debug_assert!(cur.borrow().func_flags & SQLITE_FUNC_BUILTIN != 0);
        if str_i_cmp(&cur.borrow().z_name, z_func) == 0 {
            return Some(cur);
        }
        let next = cur.borrow().u_p_hash.clone();
        p = next;
    }
    None
}

/// Insere novas FuncDef numa tabela hash FuncDefHash.
pub fn insert_builtin_funcs(a_def: &[FuncDefRef], n_def: usize) {
    for i in 0..n_def {
        let z_name = a_def[i].borrow().z_name.clone();
        let n_name = strlen30(&z_name);
        let h = func_hash(z_name.first().copied().unwrap_or(0), n_name);
        debug_assert!(a_def[i].borrow().func_flags & SQLITE_FUNC_BUILTIN != 0);
        let p_other = function_search(h, &z_name);
        if let Some(p_other) = p_other {
            debug_assert!(!Rc::ptr_eq(&p_other, &a_def[i]));
            debug_assert!(p_other
                .borrow()
                .p_next
                .as_ref()
                .map(|n| !Rc::ptr_eq(n, &a_def[i]))
                .unwrap_or(true));
            let next = p_other.borrow().p_next.clone();
            a_def[i].borrow_mut().p_next = next;
            p_other.borrow_mut().p_next = Some(Rc::clone(&a_def[i]));
        } else {
            a_def[i].borrow_mut().p_next = None;
            a_def[i].borrow_mut().u_p_hash = builtin_functions_get(h);
            builtin_functions_set(h, Some(Rc::clone(&a_def[i])));
        }
    }
}


// ---- part_001.rs ----

/// Localiza uma função de usuário por nome, número de argumentos e preferência
/// de codificação (UTF-16 ou UTF-8). Retorna a FuncDef que define a função, ou
/// `None` se ela não existe.
///
/// Se `create_flag` é verdadeiro, cria uma FuncDef em branco e a liga ao `db`
/// quando nenhuma função correspondente existia.
///
/// Se `n_arg` é -2, retorna a primeira função válida (`x_s_func` não nulo); nesse
/// caso `create_flag` deve ser falso.
///
/// Se `create_flag` é falso, pode retornar função com o nome e o número de
/// argumentos pedidos mesmo que `enc` não case com o pedido.
pub fn find_function(db: &Sqlite3Ref, z_name: &[u8], n_arg: i32, enc: u8, create_flag: bool) -> Option<FuncDefRef> {
    let mut p_best: Option<FuncDefRef> = None; // Melhor correspondência até agora.
    let mut best_score = 0; // Pontuação da melhor correspondência.

    debug_assert!(n_arg >= -2);
    debug_assert!(n_arg >= -1 || !create_flag);
    let n_name = strlen30(z_name);

    // Primeiro busca entre as funções definidas pela aplicação.
    let mut p = hash_find(&db.borrow().a_func, z_name);
    while let Some(cur) = p {
        let score = match_quality(&cur, n_arg, enc);
        if score > best_score {
            p_best = Some(Rc::clone(&cur));
            best_score = score;
        }
        let next = cur.borrow().p_next.clone();
        p = next;
    }

    // Se não houve correspondência, busca as funções embutidas.
    //
    // Se DBFLAG_PREFERBUILTIN está ligada, busca as embutidas mesmo que já
    // tenha achado uma da aplicação, e dá prioridade às embutidas.
    //
    // Exceto se `create_flag` é verdadeiro: a FuncDef devolvida terá campos
    // sobrescritos, e as FuncDef embutidas são somente leitura.
    if !create_flag && (p_best.is_none() || (db.borrow().m_db_flags & DBFLAG_PREFERBUILTIN) != 0) {
        best_score = 0;
        let c0 = z_name.first().copied().unwrap_or(0);
        let h = func_hash(UPPER_TO_LOWER[c0 as usize], n_name);
        let mut p = function_search(h, z_name);
        while let Some(cur) = p {
            let score = match_quality(&cur, n_arg, enc);
            if score > best_score {
                p_best = Some(Rc::clone(&cur));
                best_score = score;
            }
            let next = cur.borrow().p_next.clone();
            p = next;
        }
    }

    // Se `create_flag` é verdadeiro e a busca não achou correspondência exata
    // de nome, número de argumentos e codificação, acrescenta nova entrada na
    // tabela hash e a retorna.
    if create_flag && best_score < FUNC_PERFECT_MATCH {
        let mut novo = FuncDef::default();
        novo.n_arg = n_arg as u16 as i16;
        novo.func_flags = enc as u32;
        novo.z_name = z_name.iter().map(|&c| UPPER_TO_LOWER[c as usize]).collect();
        let key = novo.z_name.clone();
        let p_new: FuncDefRef = Rc::new(RefCell::new(novo));
        p_best = Some(Rc::clone(&p_new));
        let p_other = hash_insert(&mut db.borrow_mut().a_func, &key, Some(Rc::clone(&p_new)));
        match p_other {
            Some(o) if Rc::ptr_eq(&o, &p_new) => {
                oom_fault(db);
                return None;
            }
            o => {
                p_new.borrow_mut().p_next = o;
            }
        }
    }

    if let Some(best) = p_best {
        if best.borrow().x_s_func.is_some() || create_flag {
            return Some(best);
        }
    }
    None
}

/// Libera todos os recursos mantidos pela estrutura Schema. Não libera o
/// próprio Schema, só limpa os recursos subsidiários (o conteúdo das tabelas
/// hash do schema).
///
/// A variável `Schema.cache_size` não é limpa.
pub fn schema_clear(p_schema: &mut Schema) {
    // memset(&xdb, 0, sizeof(xdb))
    let xdb: Sqlite3Ref = Rc::new(RefCell::new(Sqlite3::default()));
    let mut temp1 = std::mem::take(&mut p_schema.tbl_hash);
    let mut temp2 = std::mem::take(&mut p_schema.trig_hash);
    hash_init(&mut p_schema.trig_hash);
    hash_clear(&mut p_schema.idx_hash);
    for p_trig in hash_data_list(&temp2) {
        delete_trigger(&xdb, p_trig);
    }
    hash_clear(&mut temp2);
    hash_init(&mut p_schema.tbl_hash);
    for p_tab in hash_data_list(&temp1) {
        delete_table(&xdb, p_tab);
    }
    hash_clear(&mut temp1);
    hash_clear(&mut p_schema.fkey_hash);
    p_schema.p_seq_tab = None;
    if p_schema.schema_flags & DB_SCHEMALOADED != 0 {
        p_schema.i_generation = p_schema.i_generation.wrapping_add(1);
    }
    p_schema.schema_flags &= !(DB_SCHEMALOADED | DB_RESETWANTED);
}

/// Localiza e retorna o schema associado a uma Btree. Cria um novo se preciso.
pub fn schema_get(db: &Sqlite3Ref, p_bt: Option<&BtreeRef>) -> Option<SchemaRef> {
    let p: Option<SchemaRef> = if let Some(p_bt) = p_bt {
        btree_schema(p_bt, std::mem::size_of::<Schema>(), Some(schema_clear))
    } else {
        Some(Rc::new(RefCell::new(Schema::default())))
    };
    match p {
        None => {
            oom_fault(db);
            None
        }
        Some(p) => {
            if p.borrow().file_format == 0 {
                let mut s = p.borrow_mut();
                hash_init(&mut s.tbl_hash);
                hash_init(&mut s.idx_hash);
                hash_init(&mut s.trig_hash);
                hash_init(&mut s.fkey_hash);
                s.enc = SQLITE_UTF8;
            }
            Some(p)
        }
    }
}

