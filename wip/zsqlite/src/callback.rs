//! Tradução de callback.c: acesso às tabelas hash de funções definidas pelo usuário e de
//! sequências de colação, mais a limpeza (`sqlite3SchemaClear`) e a criação (`sqlite3SchemaGet`)
//! do esquema de um banco.
//!
//! Decisões do modelo v2 que aparecem aqui (ver CONVENTIONS.md e `connection.rs`):
//!
//! * As três `CollSeq` de um nome (UTF-8, UTF-16LE, UTF-16BE), que o C aloca juntas com o nome
//!   logo depois, são o valor `[Option<Rc<CollSeq>>; 3]` de `Connection.a_coll_seq`. O `CollSeq`
//!   de `mem.rs` NÃO tem o estado "em branco" (`xCmp == 0`) que o C usa: aqui uma vaga em branco
//!   é `None` e a existência da entrada no hash é o que distingue "nome conhecido, sem função
//!   nesta codificação" de "nome desconhecido". Por isso `find_coll_seq` devolve `None` onde o C
//!   devolveria uma `CollSeq` em branco, e `check_coll_seq` não tem o que conferir (um
//!   `Rc<CollSeq>` sempre tem comparação). Ver as dúvidas no relatório da fatia.
//! * As funções embutidas (`sqlite3BuiltinFunctions`, `FuncDefHash`) são um estado global do
//!   processo no C. Como um `Rc<FuncDef>` não é `Sync`, a tabela é `thread_local!` (um `Cell`
//!   com `take`/`set`, sem `RefCell`) e o registro (`register_builtin_functions`) precisa
//!   rodar em cada thread que abre conexões. A tabela é `[balde][nome][cadeia]`: o balde é a
//!   lista `u.pHash` (um elemento por nome, o mais novo na frente) e a cadeia é a lista `pNext`
//!   (as sobrecargas do mesmo nome, na ordem do C).
//! * `sqlite3FindFunction` com `createFlag` insere uma `FuncDef` em branco na cadeia da conexão
//!   e a devolve (`Rc`). Como `FuncDef` é imutável depois de compartilhada, quem a preenche
//!   (`sqlite3_create_function`) troca o `Rc` na posição em que ele está na cadeia
//!   (`Rc::ptr_eq`), como diz `connection.rs`.
//! * O `Schema` é por valor em `Connection.dbs[i].schema`; `sqlite3SchemaClear` trabalha sobre o
//!   banco `i_db` da conexão. `sqlite3SchemaGet` devolve um `Schema` novo.
//! * O `sqlite3 xdb` zerado que o C passa a `sqlite3DeleteTable`/`sqlite3DeleteTrigger` some:
//!   `delete_table` recebe a conexão de verdade (a desconexão de tabelas virtuais precisa dela)
//!   e o `Drop` de `Rc<Trigger>` desfaz os gatilhos.

use std::cell::Cell;
use std::rc::Rc;

use crate::build::{delete_table, text_arg};
use crate::btree_types::Btree;
use crate::connection::{Connection, FuncDef, Parse};
use crate::consts::{
    DBFLAG_PREFER_BUILTIN, DB_RESETWANTED, DB_SCHEMALOADED, SQLITE_ERROR,
    SQLITE_ERROR_MISSING_COLLSEQ, SQLITE_FUNC_BUILTIN, SQLITE_FUNC_ENCMASK, SQLITE_FUNC_HASH_SZ,
    SQLITE_OK, SQLITE_UTF16BE, SQLITE_UTF16LE, SQLITE_UTF16NATIVE, SQLITE_UTF8,
};
use crate::ctype::UPPER_TO_LOWER;
use crate::hash::{
    hash_clear, hash_data, hash_find, hash_find_mut, hash_first, hash_init, hash_insert, hash_next,
};
use crate::mem::CollSeq;
use crate::sqlite_int::Schema;
use crate::util::{at, error_msg, str_icmp, strlen30};
use crate::utf::translate_bytes;
use crate::vdbeaux3::expire_prepared_statements;

/// O nome da colação padrão (`sqlite3StrBINARY`).
const STR_BINARY: &[u8] = b"BINARY";

/// `FUNC_PERFECT_MATCH`: a pontuação de uma correspondência perfeita.
const FUNC_PERFECT_MATCH: i32 = 6;

/// As três vagas de uma colação, na ordem de `enc - 1`: UTF-8, UTF-16LE, UTF-16BE.
pub type CollSlots = [Option<Rc<CollSeq>>; 3];

/// O trecho de `z` até o primeiro NUL (o C trata o nome como cadeia terminada em NUL).
fn cstr(z: &[u8]) -> &[u8] {
    &z[..strlen30(z) as usize]
}

/// `callCollNeeded`: chama o gancho "collation needed" para pedir a colação `z_name` na
/// codificação `enc`. O gancho recebe a conexão, então é retirado do `Option` durante a chamada
/// e devolvido depois, a menos que ele mesmo tenha instalado outro.
fn call_coll_needed(db: &mut Connection, enc: u8, z_name: Option<&[u8]>) {
    let Some(z_name) = z_name else {
        // O C duplica `zName` (nulo dá nulo e retorna) ou converte para UTF-16 (nulo dá nulo e
        // o gancho não é chamado).
        return;
    };
    let z_name = cstr(z_name);
    let Some(mut hook) = db.x_coll_needed.take() else {
        return;
    };
    if db.coll_needed_16 {
        let external = translate_bytes(z_name, SQLITE_UTF8 as u8, SQLITE_UTF16NATIVE as u8);
        let enc_db = db.enc as i32;
        hook(db, enc_db, &external);
    } else {
        hook(db, enc as i32, z_name);
    }
    if db.x_coll_needed.is_none() {
        db.x_coll_needed = Some(hook);
    }
}

/// `synthCollSeq`: chamada quando a fábrica de colações não entrega a função na melhor
/// codificação mas pode haver outras versões da mesma colação (em outras codificações). Usa uma
/// delas se existir, evitando a conversão UTF-8 <-> UTF-16 sempre que possível. O `memcpy` do C
/// vira a vaga `enc - 1` passar a apontar para a mesma `CollSeq`.
fn synth_coll_seq(db: &mut Connection, enc: u8, z_name: &[u8]) -> i32 {
    const A_ENC: [i32; 3] = [SQLITE_UTF16BE, SQLITE_UTF16LE, SQLITE_UTF8];
    for e in A_ENC {
        if let Some(p_coll2) = find_coll_seq(db, e as u8, Some(z_name), 0) {
            if let Some(slots) = find_coll_seq_entry(db, z_name, 0) {
                slots[(enc - 1) as usize] = Some(p_coll2);
            }
            return SQLITE_OK;
        }
    }
    SQLITE_ERROR
}

/// `sqlite3CheckCollSeq`: confere uma colação antes de usá-la. No C, uma colação sem `xCmp`
/// (carregada de um banco que a cita sem que ninguém a tenha registrado) dispara
/// `sqlite3GetCollSeq`. Aqui um `Rc<CollSeq>` sempre tem comparação (a vaga em branco é `None`,
/// ver o cabeçalho do módulo), então não há o que conferir e o resultado é sempre `SQLITE_OK`.
pub fn check_coll_seq(_db: &mut Connection, _parse: &mut Parse, _p_coll: Option<&Rc<CollSeq>>) -> i32 {
    SQLITE_OK
}

/// `findCollSeqEntry`: localiza a entrada de `db.a_coll_seq` com o nome `z_name`. Se não existe e
/// `create` é verdadeiro, cria uma entrada com as três vagas em branco. Devolve as vagas.
///
/// É `pub(crate)` (no C é `static`) porque `sqlite3CreateCollation` preenche a vaga devolvida:
/// no modelo v2 isso é trocar `None` por `Some(Rc<CollSeq>)` na vaga `enc - 1`.
pub(crate) fn find_coll_seq_entry<'a>(
    db: &'a mut Connection,
    z_name: &[u8],
    create: i32,
) -> Option<&'a mut CollSlots> {
    let z_name = cstr(z_name);
    if create != 0 && hash_find(&db.a_coll_seq, z_name).is_none() {
        hash_insert(&mut db.a_coll_seq, z_name, Some([None, None, None]));
    }
    hash_find_mut(&mut db.a_coll_seq, z_name)
}

/// `sqlite3FindCollSeq`: a `CollSeq` da colação `z_name` na codificação `enc` (um dos
/// `SQLITE_UTF8`, `SQLITE_UTF16LE`, `SQLITE_UTF16BE`). Se a entrada não existe e `create` é
/// verdadeiro, cria uma em branco. Sem nome devolve a colação padrão da conexão. `None` também
/// é o que se devolve para uma vaga em branco (o C devolveria a `CollSeq` com `xCmp == 0`).
///
/// A função `sqlite3LocateCollSeq` embrulha esta e chama a fábrica de colações se for preciso.
pub fn find_coll_seq(
    db: &mut Connection,
    enc: u8,
    z_name: Option<&[u8]>,
    create: i32,
) -> Option<Rc<CollSeq>> {
    debug_assert!(SQLITE_UTF8 == 1 && SQLITE_UTF16LE == 2 && SQLITE_UTF16BE == 3);
    debug_assert!(enc >= SQLITE_UTF8 as u8 && enc <= SQLITE_UTF16BE as u8);
    match z_name {
        Some(name) => find_coll_seq_entry(db, name, create)
            .and_then(|slots| slots[(enc - 1) as usize].clone()),
        None => db.p_dflt_coll.clone(),
    }
}

/// `sqlite3SetTextEncoding`: troca a codificação de texto da conexão, e com ela a colação
/// padrão (o BINARY da nova codificação).
pub fn set_text_encoding(db: &mut Connection, enc: u8) {
    debug_assert!(
        enc == SQLITE_UTF8 as u8 || enc == SQLITE_UTF16LE as u8 || enc == SQLITE_UTF16BE as u8
    );
    db.enc = enc;
    // EVIDENCE-OF: R-08308-17224 A colação padrão de todas as cadeias é BINARY.
    db.p_dflt_coll = find_coll_seq(db, enc, Some(STR_BINARY), 0);
    expire_prepared_statements(db, 1);
}

/// `sqlite3GetCollSeq`: invoca a fábrica de colações ou substitui por uma colação de outra
/// codificação quando a pedida não existe na codificação desejada.
///
/// `p_coll`, se existe, é a colação na codificação nativa do banco com o nome `z_name`. Devolve
/// a colação a usar, ou `None` se não há nenhuma, caso em que deixa a mensagem de erro em
/// `parse`.
///
/// Ver também `locate_coll_seq` e `find_coll_seq`.
pub fn get_coll_seq(
    db: &mut Connection,
    parse: &mut Parse,
    enc: u8,
    p_coll: Option<Rc<CollSeq>>,
    z_name: Option<&[u8]>,
) -> Option<Rc<CollSeq>> {
    let mut p = p_coll;
    if p.is_none() {
        p = find_coll_seq(db, enc, z_name, 0);
    }
    if p.is_none() {
        // Nenhuma colação deste tipo está registrada para esta codificação. Pergunta à
        // fábrica de colações se ela fornece uma.
        call_coll_needed(db, enc, z_name);
        p = find_coll_seq(db, enc, z_name, 0);
    }
    if p.is_none() {
        // A entrada existe (em branco) mas sem função: tenta outra codificação do mesmo nome.
        if let Some(name) = z_name {
            let name = cstr(name);
            if find_coll_seq_entry(db, name, 0).is_some()
                && synth_coll_seq(db, enc, name) == SQLITE_OK
            {
                p = find_coll_seq(db, enc, Some(name), 0);
            }
        }
    }
    if p.is_none() {
        error_msg(
            db,
            parse,
            b"no such collation sequence: %s",
            &[text_arg(z_name.map(cstr).unwrap_or(&[]))],
        );
        parse.rc = SQLITE_ERROR_MISSING_COLLSEQ;
    }
    p
}

/// `sqlite3LocateCollSeq`: a colação do banco com o nome `z_name`, na codificação nativa. Se não
/// está disponível ou não existe nessa codificação, invoca a fábrica de colações; se ainda assim
/// não há uma colação em alguma codificação, devolve `None` e deixa a mensagem em `parse`.
///
/// Embrulha `find_coll_seq`: invoca a fábrica se o nome não é achado e gera a mensagem de erro.
pub fn locate_coll_seq(db: &mut Connection, parse: &mut Parse, z_name: &[u8]) -> Option<Rc<CollSeq>> {
    let enc = db.enc;
    let initbusy = db.init.busy;
    let mut p_coll = find_coll_seq(db, enc, Some(z_name), initbusy as i32);
    if initbusy == 0 && p_coll.is_none() {
        p_coll = get_coll_seq(db, parse, enc, p_coll, Some(z_name));
    }
    p_coll
}

/// `SQLITE_FUNC_HASH(C, L)`: o balde de uma função, pelo primeiro byte do nome e pelo tamanho.
#[inline]
fn func_hash(c: u8, l: i32) -> usize {
    (c as usize + l as usize) % SQLITE_FUNC_HASH_SZ
}

/// `matchQuality`: durante a busca da melhor definição de função, mede o quanto `p` atende ao
/// pedido de uma função com `n_arg` argumentos num sistema que usa a codificação `enc`. Um valor
/// maior é um casamento melhor.
///
/// Se `n_arg` é -1 só casa (valor diferente de zero) se `p.n_arg` também é -1: a busca é por uma
/// função de número variável de argumentos. Se é -2, procura qualquer função, seja qual for o
/// número de argumentos: qualquer `p` com `x_s_func` casa perfeitamente e sem `x_s_func` não casa.
///
/// O resultado fica entre 0 e 6:
///
/// * 0: não casa.
/// * 1: precisa de conversão UTF-8/16 e a função aceita qualquer número de argumentos.
/// * 2: precisa trocar a ordem dos bytes do UTF-16 e aceita qualquer número de argumentos.
/// * 3: a codificação casa e a função aceita qualquer número de argumentos.
/// * 4: precisa de conversão UTF-8/16, com o número de argumentos exato.
/// * 5: precisa trocar a ordem dos bytes do UTF-16, com o número de argumentos exato.
/// * 6: casamento perfeito: codificação e número de argumentos exatos.
fn match_quality(p: &FuncDef, n_arg: i32, enc: u8) -> i32 {
    let p_n_arg = p.n_arg as i32;
    debug_assert!(p_n_arg >= -1);

    // Número de argumentos errado quer dizer "sem casamento".
    if p_n_arg != n_arg {
        if n_arg == -2 {
            return if p.x_s_func.is_none() { 0 } else { FUNC_PERFECT_MATCH };
        }
        if p_n_arg >= 0 {
            return 0;
        }
    }

    // Uma função com número específico de argumentos pontua mais que uma que aceita qualquer
    // número.
    let mut match_ = if p_n_arg == n_arg { 4 } else { 1 };

    // Pontos de bônus se a codificação de texto casa.
    if enc as u32 == (p.func_flags & SQLITE_FUNC_ENCMASK) {
        match_ += 2; // Casamento exato da codificação.
    } else if (enc as u32 & p.func_flags & 2) != 0 {
        match_ += 1; // As duas são UTF-16, mas com ordem de bytes diferente.
    }

    match_
}

/// A tabela de funções embutidas (`sqlite3BuiltinFunctions`): `[balde][nome][cadeia]`, ver o
/// cabeçalho do módulo.
#[derive(Default)]
struct BuiltinFuncs {
    a: Vec<Vec<Vec<Rc<FuncDef>>>>,
}

impl BuiltinFuncs {
    /// Garante os `SQLITE_FUNC_HASH_SZ` baldes (o `FuncDefHash` do C é zerado estaticamente).
    fn ensure(&mut self) {
        if self.a.is_empty() {
            self.a.resize_with(SQLITE_FUNC_HASH_SZ, Vec::new);
        }
    }
}

thread_local! {
    /// `sqlite3BuiltinFunctions`.
    static BUILTIN_FUNCTIONS: Cell<BuiltinFuncs> = Cell::new(BuiltinFuncs::default());
}

/// Roda `f` sobre a tabela de funções embutidas da thread.
fn with_builtin<R>(f: impl FnOnce(&mut BuiltinFuncs) -> R) -> R {
    BUILTIN_FUNCTIONS.with(|cell| {
        let mut table = cell.take();
        table.ensure();
        let r = f(&mut table);
        cell.set(table);
        r
    })
}

/// `sqlite3FunctionSearch`: procura numa tabela de funções embutidas a função de nome `z_func`
/// (o balde é `h`, o hash do nome). Devolve a cadeia de sobrecargas do nome, ou `None`.
fn function_search(t: &BuiltinFuncs, h: usize, z_func: &[u8]) -> Option<Vec<Rc<FuncDef>>> {
    for chain in &t.a[h] {
        let head = &chain[0];
        debug_assert!(head.func_flags & SQLITE_FUNC_BUILTIN != 0);
        if str_icmp(&head.z_name, z_func) == 0 {
            return Some(chain.clone());
        }
    }
    None
}

/// `sqlite3InsertBuiltinFuncs`: insere na tabela de funções embutidas a lista `defs`. Um nome
/// novo entra na frente do balde; uma sobrecarga de nome já presente entra logo depois da
/// primeira definição do nome (como `pOther->pNext = &aDef[i]` do C).
pub fn insert_builtin_funcs(defs: Vec<FuncDef>) {
    with_builtin(|t| {
        for def in defs {
            let n_name = strlen30(&def.z_name);
            let h = func_hash(at(&def.z_name, 0), n_name);
            debug_assert!(def.func_flags & SQLITE_FUNC_BUILTIN != 0);
            let name = cstr(&def.z_name).to_vec();
            let def = Rc::new(def);
            let bucket = &mut t.a[h];
            match bucket.iter().position(|c| str_icmp(&c[0].z_name, &name) == 0) {
                Some(pos) => {
                    debug_assert!(!bucket[pos].iter().any(|d| Rc::ptr_eq(d, &def)));
                    bucket[pos].insert(1, def);
                }
                None => bucket.insert(0, vec![def]),
            }
        }
    });
}

/// Todas as funções embutidas na ordem em que o `PRAGMA function_list` do C as percorre: os
/// baldes de 0 a `SQLITE_FUNC_HASH_SZ - 1`, em cada um os nomes (`u.pHash`) e em cada nome as
/// sobrecargas (`pNext`).
pub fn builtin_functions_in_order() -> Vec<Rc<FuncDef>> {
    with_builtin(|t| t.a.iter().flatten().flatten().cloned().collect())
}

/// `sqlite3FindFunction`: localiza uma função de usuário pelo nome, número de argumentos e
/// codificação preferida de texto. Devolve a definição, ou `None` se a função não existe.
///
/// Se `create_flag` é verdadeiro e não havia uma função com o nome, argumentos e codificação
/// exatos, cria uma `FuncDef` em branco, a liga na frente da cadeia do nome em `db.a_func` e a
/// devolve (quem a preenche troca o `Rc` na cadeia, ver o cabeçalho do módulo).
///
/// Se `n_arg` é -2, devolve a primeira função válida (com `x_s_func`): serve para ver se
/// `z_name` é um nome de função válido para algum número de argumentos. Com `n_arg == -2`,
/// `create_flag` tem de ser zero.
///
/// Se `create_flag` é zero, uma função com o nome e número de argumentos pedidos pode ser
/// devolvida mesmo que a codificação não seja a pedida.
pub fn find_function(
    db: &mut Connection,
    z_name: &[u8],
    n_arg: i32,
    enc: u8,
    create_flag: u8,
) -> Option<Rc<FuncDef>> {
    let mut p_best: Option<Rc<FuncDef>> = None; // Melhor casamento até agora.
    let mut best_score = 0; // Pontuação do melhor casamento.

    debug_assert!(n_arg >= -2);
    debug_assert!(n_arg >= -1 || create_flag == 0);
    let z_name = cstr(z_name);
    let n_name = strlen30(z_name);

    // Primeiro procura entre as funções definidas pela aplicação.
    if let Some(chain) = hash_find(&db.a_func, z_name) {
        for p in chain {
            let score = match_quality(p, n_arg, enc);
            if score > best_score {
                p_best = Some(p.clone());
                best_score = score;
            }
        }
    }

    // Se não achou, procura entre as embutidas.
    //
    // Se o DBFLAG_PreferBuiltin está ligado, procura nas embutidas mesmo que uma função da
    // aplicação tenha sido achada, e dá prioridade às embutidas.
    //
    // Mas se `create_flag` é verdadeiro estamos instalando uma função nova, e as `FuncDef` das
    // embutidas são somente de leitura: então não se procura nelas.
    if create_flag == 0 && (p_best.is_none() || (db.m_db_flags & DBFLAG_PREFER_BUILTIN) != 0) {
        best_score = 0;
        let h = func_hash(UPPER_TO_LOWER[at(z_name, 0) as usize], n_name);
        let chain = with_builtin(|t| function_search(t, h, z_name));
        if let Some(chain) = chain {
            for p in &chain {
                let score = match_quality(p, n_arg, enc);
                if score > best_score {
                    p_best = Some(p.clone());
                    best_score = score;
                }
            }
        }
    }

    // Se `create_flag` é verdadeiro e a busca não achou um casamento exato de nome, número de
    // argumentos e codificação, acrescenta uma entrada nova à tabela hash e a devolve.
    if create_flag != 0 && best_score < FUNC_PERFECT_MATCH {
        let lowered: Vec<u8> = z_name.iter().map(|&c| UPPER_TO_LOWER[c as usize]).collect();
        let new = Rc::new(FuncDef {
            n_arg: n_arg as i8,
            func_flags: enc as u32,
            z_name: lowered.clone(),
            ..FuncDef::default()
        });
        match hash_find_mut(&mut db.a_func, &lowered) {
            Some(chain) => chain.insert(0, new.clone()),
            None => {
                hash_insert(&mut db.a_func, &lowered, Some(vec![new.clone()]));
            }
        }
        p_best = Some(new);
    }

    match p_best {
        Some(b) if b.x_s_func.is_some() || create_flag != 0 => Some(b),
        _ => None,
    }
}

/// `sqlite3SchemaClear`: libera tudo o que o esquema do banco `i_db` guarda (as tabelas hash de
/// tabelas, índices, gatilhos e chaves estrangeiras). `Schema.cache_size` não é zerado.
pub fn schema_clear(db: &mut Connection, i_db: usize) {
    let schema = &mut db.dbs[i_db].schema;
    let mut temp1 = std::mem::replace(&mut schema.tbl_hash, hash_init());
    let mut temp2 = std::mem::replace(&mut schema.trig_hash, hash_init());
    hash_clear(&mut schema.idx_hash);
    // Os gatilhos se desfazem com o `Drop` dos `Rc<Trigger>`.
    hash_clear(&mut temp2);
    // `p_seq_tab` também é um `Rc` da tabela sqlite_sequence: soltá-lo antes deixa o `delete_table`
    // ver a última referência (o C só zera o ponteiro, sem contagem).
    schema.p_seq_tab = None;

    let mut tables = Vec::new();
    let mut elem = hash_first(&temp1);
    while let Some(e) = elem {
        tables.push(hash_data(&temp1, e).clone());
        elem = hash_next(&temp1, e);
    }
    hash_clear(&mut temp1);
    drop(temp1);
    for p_tab in tables {
        delete_table(db, Some(p_tab));
    }

    let schema = &mut db.dbs[i_db].schema;
    hash_clear(&mut schema.fkey_hash);
    if schema.schema_flags & DB_SCHEMALOADED != 0 {
        schema.i_generation += 1;
    }
    schema.schema_flags &= !(DB_SCHEMALOADED | DB_RESETWANTED);
}

/// `sqlite3SchemaGet`: o esquema associado a uma árvore-b. No C devolve o `Schema` guardado
/// no `Btree` (criando se preciso, para que as conexões que compartilham o cache vejam o mesmo);
/// sem cache compartilhado só há um dono, então é um `Schema` novo, vazio, em UTF-8.
pub fn schema_get(_db: &mut Connection, _p_bt: Option<&Btree>) -> Schema {
    let mut p = Schema::new();
    debug_assert!(p.file_format == 0);
    p.enc = SQLITE_UTF8 as u8;
    p
}
