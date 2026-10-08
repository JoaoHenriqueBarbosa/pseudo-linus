//! `json.c` (parte 2): as funções SQL `json_*` e `jsonb_*`, os agregados `json_group_array` e
//! `json_group_object`, as tabelas virtuais `json_each` e `json_tree` e o registro de tudo isso.
//! A base (JSONB, analisador, busca por caminho) está em [`crate::json`], que reexporta este
//! módulo.
//!
//! Desvios do C, todos decorrentes do modelo v2 (ver também o cabeçalho de `json.rs`):
//!
//! * O acumulador dos agregados é um `Option<JsonString>` no contexto do agregado (o `zBuf==0`
//!   do C é `None`); como o `JsonString` não guarda o contexto, o passo tira o acumulador do
//!   contexto, trabalha com ele e o devolve. `jsonArrayStep`/`jsonObjectStep` e
//!   `jsonArrayCompute`/`jsonObjectCompute` do C são idênticos a menos do caractere de abertura,
//!   do de fechamento e do resultado vazio, e aqui são uma rotina só (DRY).
//! * As macros `JFUNCTION` e `WAGGREGATE` de `sqliteInt.h` viram [`jfunction`] e [`waggregate`]
//!   (o `make_def` de `func.rs` é privado).
//! * `jsonEachPathLength` troca o byte por NUL no buffer do caminho para delimitar a cadeia C; aqui
//!   a busca recebe a fatia do caminho já cortada.

use std::any::Any;
use std::rc::Rc;

use crate::connection::{
    Connection, Context, FinalFn, FuncDef, IndexInfo, ModuleCaps, ScalarFn, UserData, Vtab,
    VtabCursor, VtabModule,
};
use crate::consts::{
    SQLITE_CONSTRAINT, SQLITE_DETERMINISTIC, SQLITE_ERROR, SQLITE_FUNC_BUILTIN,
    SQLITE_FUNC_CONSTANT, SQLITE_FUNC_RUNONLY, SQLITE_INDEX_CONSTRAINT_EQ, SQLITE_INTEGER,
    SQLITE_NOMEM, SQLITE_NULL, SQLITE_OK, SQLITE_RESULT_SUBTYPE, SQLITE_SUBTYPE, SQLITE_TEXT,
    SQLITE_UTF8, SQLITE_VTAB_INNOCUOUS,
};
use crate::ctype::{is_alnum, is_alpha};
use crate::vdbeapi::text_of;
use crate::json::{
    blob_of, cstr, json_after_edit_size_adjust, json_append_sql_value, json_array_count,
    json_bad_path_error, json_blob_edit, json_convert_text_to_blob, json_func_arg_might_be_binary,
    json_function_arg_to_blob, json_label_compare, json_lookup_is_error, json_lookup_step,
    json_parse_func_arg, json_parse_reset, json_payload_size, json_return_from_blob,
    json_return_parse, json_return_string, json_return_string_as_blob,
    json_translate_blob_to_pretty_text, json_translate_blob_to_text, json_validity_check, off,
    sub, user_flags, JsonParse, JsonPretty, JsonString, JEDIT_DEL, JEDIT_INS, JEDIT_REPL,
    JEDIT_SET, JSONB_ARRAY, JSONB_OBJECT, JSONB_TEXT, JSONB_TEXTRAW, JSONB_TYPE, JSON_ABPATH,
    JSON_BLOB, JSON_EDITABLE, JSON_ISSET, JSON_JSON, JSON_KEEPERROR, JSON_LOOKUP_ERROR,
    JSON_LOOKUP_NOTFOUND, JSON_LOOKUP_PATHERROR, JSON_SQL, JSON_SUBTYPE,
};
use crate::mem::{value_type, Mem, StrDtor};
use crate::util::{at, atoi64};
use crate::vdbeapi::{
    aggregate_context, result_error, result_error_nomem, result_int, result_int64,
    result_subtype, result_text, result_text64, value_int64,
};
use crate::vtab::{create_module, declare_vtab, vtab_config};

// ---------------------------------------------------------------------------------------------
// Funções escalares
// ---------------------------------------------------------------------------------------------

/// `jsonWrongNumArgs`: número errado de argumentos de `json_insert()`, `json_replace()` ou
/// `json_set()`.
fn json_wrong_num_args(ctx: &mut Context<'_>, z_func_name: &str) {
    let msg = format!("json_{}() needs an odd number of arguments", z_func_name);
    result_error(ctx, msg.as_bytes(), -1);
}

/// `jsonInsertIntoBlob`: `argv[0]` é um JSON; os argumentos seguintes vêm em pares de caminho e
/// conteúdo a inserir ou pôr nesse caminho. Faz as edições e devolve o resultado. A operação é
/// `e_edit`: `JEDIT_INS`, `JEDIT_REPL` ou `JEDIT_SET`.
fn json_insert_into_blob(ctx: &mut Context<'_>, argv: &[Mem], e_edit: u8) {
    let argc = argv.len();
    let mut rc: u32 = 0;
    let mut z_path_keep: Vec<u8> = Vec::new();
    let flgs = if argc == 1 { 0 } else { JSON_EDITABLE };
    let Some(mut p) = json_parse_func_arg(ctx, &argv[0], flgs) else {
        return;
    };
    let mut i = 1usize;
    while i + 1 < argc {
        if value_type(&argv[i]) == SQLITE_NULL {
            i += 2;
            continue;
        }
        let Some(zp_raw) = text_of(&argv[i]) else {
            result_error_nomem(ctx);
            return;
        };
        let z_path = cstr(&zp_raw);
        z_path_keep = z_path.to_vec();
        let mut patherror = at(z_path, 0) != b'$';
        if !patherror {
            let Some(ax) = json_function_arg_to_blob(ctx, &argv[i + 1]) else {
                return;
            };
            if at(z_path, 1) == 0 {
                if e_edit == JEDIT_REPL || e_edit == JEDIT_SET {
                    let n = p.n_blob();
                    json_blob_edit(&mut p, 0, n, Some(ax.blob()), ax.n_blob());
                }
                rc = 0;
            } else {
                p.e_edit = e_edit;
                p.a_ins = ax.a_blob.clone();
                p.delta = 0;
                rc = json_lookup_step(&mut p, 0, off(z_path, 1), 0);
            }
            if rc == JSON_LOOKUP_NOTFOUND {
                i += 2;
                continue;
            }
            if json_lookup_is_error(rc) {
                patherror = true;
            }
        }
        if patherror {
            if rc == JSON_LOOKUP_ERROR {
                result_error(ctx, b"malformed JSON", -1);
            } else {
                json_bad_path_error(Some(&mut *ctx), &z_path_keep);
            }
            return;
        }
        i += 2;
    }
    json_return_parse(ctx, &mut p);
}

/// `json_quote(VALUE)`: o valor JSON que corresponde ao valor SQL.
fn json_quote_func(ctx: &mut Context<'_>, argv: &[Mem]) {
    let mut jx = JsonString::new();
    json_append_sql_value(ctx, &mut jx, &argv[0]);
    json_return_string(ctx, &mut jx, None);
    result_subtype(ctx, JSON_SUBTYPE);
}

/// `json_array(VALUE,...)`: um vetor JSON com todos os valores dados.
fn json_array_func(ctx: &mut Context<'_>, argv: &[Mem]) {
    let mut jx = JsonString::new();
    jx.append_char(b'[');
    for a in argv {
        jx.append_separator();
        json_append_sql_value(ctx, &mut jx, a);
    }
    jx.append_char(b']');
    json_return_string(ctx, &mut jx, None);
    result_subtype(ctx, JSON_SUBTYPE);
}

/// `json_array_length(JSON)` e `json_array_length(JSON, PATH)`: o número de elementos do vetor.
fn json_array_length_func(ctx: &mut Context<'_>, argv: &[Mem]) {
    let mut cnt: i64 = 0;
    let mut e_err = false;
    let Some(mut p) = json_parse_func_arg(ctx, &argv[0], 0) else {
        return;
    };
    let mut i: u32 = 0;
    if argv.len() == 2 {
        let Some(zp_raw) = text_of(&argv[1]) else {
            return;
        };
        let z_path = cstr(&zp_raw);
        let tail: &[u8] = if at(z_path, 0) == b'$' { off(z_path, 1) } else { &b"@"[..] };
        i = json_lookup_step(&mut p, 0, tail, 0);
        if json_lookup_is_error(i) {
            if i == JSON_LOOKUP_NOTFOUND {
                // nada
            } else if i == JSON_LOOKUP_PATHERROR {
                json_bad_path_error(Some(&mut *ctx), z_path);
            } else {
                result_error(ctx, b"malformed JSON", -1);
            }
            e_err = true;
            i = 0;
        }
    }
    if (p.blob_at(i) & 0x0f) == JSONB_ARRAY {
        cnt = json_array_count(&p, i) as i64;
    }
    if !e_err {
        result_int64(ctx, cnt);
    }
}

/// `jsonAllAlphanum`: verdadeiro se a cadeia só tem alfanuméricos e sublinhados.
fn json_all_alphanum(z: &[u8]) -> bool {
    z.iter().all(|&c| is_alnum(c) || c == b'_')
}

/// `json_extract(JSON, PATH, ...)`, `->(JSON,PATH)` e `->>(JSON,PATH)`: o elemento descrito
/// por PATH, ou NULL se não existe. Com `JSON_JSON` ou mais de um PATH o resultado é sempre JSON;
/// com `JSON_SQL` é sempre um valor SQL; sem nenhum dos dois e com `argc==2`, JSON para objetos e
/// vetores e SQL para o resto. Com vários PATH o resultado é um vetor JSON com o de cada um.
/// Caminhos abreviados valem com `JSON_ABPATH`, por compatibilidade com o PostgreSQL.
fn json_extract_func(ctx: &mut Context<'_>, argv: &[Mem]) {
    let argc = argv.len();
    if argc < 2 {
        return;
    }
    let Some(mut p) = json_parse_func_arg(ctx, &argv[0], 0) else {
        return;
    };
    let flags = user_flags(ctx);
    let mut jx = JsonString::new();
    if argc > 2 {
        jx.append_char(b'[');
    }
    for i in 1..argc {
        // Com um único argumento de caminho.
        let Some(zp_raw) = text_of(&argv[i]) else {
            return;
        };
        let z_path = cstr(&zp_raw);
        let j: u32;
        if at(z_path, 0) == b'$' {
            j = json_lookup_step(&mut p, 0, off(z_path, 1), 0);
        } else if (flags & JSON_ABPATH) != 0 {
            // Os operadores -> e ->> aceitam caminhos abreviados:
            //
            //     NUMBER   ==>  $[NUMBER]     // compatível com o PG
            //     LABEL    ==>  $.LABEL       // compatível com o PG
            //     [NUMBER] ==>  $[NUMBER]     // não é do PG, só por conveniência
            let mut ab = JsonString::new();
            if value_type(&argv[i]) == SQLITE_INTEGER {
                ab.append_raw(b"[");
                ab.append_raw(z_path);
                ab.append_raw(b"]");
            } else if json_all_alphanum(z_path) {
                ab.append_raw(b".");
                ab.append_raw(z_path);
            } else if at(z_path, 0) == b'[' && z_path.len() >= 3 && z_path[z_path.len() - 1] == b']' {
                ab.append_raw(z_path);
            } else {
                ab.append_raw(b".\"");
                ab.append_raw(z_path);
                ab.append_raw(b"\"");
            }
            j = json_lookup_step(&mut p, 0, &ab.buf, 0);
        } else {
            json_bad_path_error(Some(&mut *ctx), z_path);
            return;
        }
        if j < p.n_blob() {
            if argc == 2 {
                if (flags & JSON_JSON) != 0 {
                    let mut s = JsonString::new();
                    json_translate_blob_to_text(&p, j, &mut s);
                    json_return_string(ctx, &mut s, None);
                    result_subtype(ctx, JSON_SUBTYPE);
                } else {
                    json_return_from_blob(ctx, &p, j, false);
                    if (flags & (JSON_SQL | JSON_BLOB)) == 0 && (p.blob_at(j) & 0x0f) >= JSONB_ARRAY {
                        result_subtype(ctx, JSON_SUBTYPE);
                    }
                }
            } else {
                jx.append_separator();
                json_translate_blob_to_text(&p, j, &mut jx);
            }
        } else if j == JSON_LOOKUP_NOTFOUND {
            if argc == 2 {
                return; // Devolve NULL se não achou
            }
            jx.append_separator();
            jx.append_raw(b"null");
        } else if j == JSON_LOOKUP_ERROR {
            result_error(ctx, b"malformed JSON", -1);
            return;
        } else {
            json_bad_path_error(Some(&mut *ctx), z_path);
            return;
        }
    }
    if argc > 2 {
        jx.append_char(b']');
        json_return_string(ctx, &mut jx, None);
        if (flags & JSON_BLOB) == 0 {
            result_subtype(ctx, JSON_SUBTYPE);
        }
    }
}

/// Códigos de retorno de `jsonMergePatch()`.
const JSON_MERGE_OK: i32 = 0;
const JSON_MERGE_BADTARGET: i32 = 1;
const JSON_MERGE_BADPATCH: i32 = 2;
const JSON_MERGE_OOM: i32 = 3;

/// Copia `src` sobre o JSONB de `t` a partir de `at_pos` (o espaço já foi aberto por
/// `json_blob_edit`).
fn json_copy_into(t: &mut JsonParse, at_pos: usize, src: &[u8]) {
    let v = t.blob_mut();
    if at_pos + src.len() <= v.len() {
        v[at_pos..at_pos + src.len()].copy_from_slice(src);
    }
}

/// `jsonMergePatch`: o MergePatch da RFC-7396 para dois JSONB. O alvo (`t`) é atualizado no
/// lugar e o remendo (`patch`) é só leitura.
///
/// O algoritmo original da RFC-7396:
///
/// ```text
///   define MergePatch(Target, Patch):
///     if Patch is an Object:
///       if Target is not an Object:
///         Target = {} # Ignore the contents and set it to an empty Object
///     for each Name/Value pair in Patch:
///         if Value is null:
///           if Name exists in Target:
///             remove the Name/Value pair from Target
///         else:
///           Target[Name] = MergePatch(Target[Name], Value)
///       return Target
///     else:
///       return Patch
/// ```
fn json_merge_patch(t: &mut JsonParse, i_target: u32, patch: &JsonParse, i_patch: u32) -> i32 {
    let mut x = patch.blob_at(i_patch) & 0x0f;
    if x != JSONB_OBJECT {
        // Algoritmo, linha 02
        let (n, sz) = json_payload_size(patch, i_patch);
        let sz_patch = n + sz;
        let (n, sz) = json_payload_size(t, i_target);
        let sz_target = n + sz;
        json_blob_edit(t, i_target, sz_target, Some(sub(patch.blob(), i_patch, sz_patch)), sz_patch);
        return if t.oom { JSON_MERGE_OOM } else { JSON_MERGE_OK }; // Linha 03
    }
    x = t.blob_at(i_target) & 0x0f;
    if x != JSONB_OBJECT {
        // Algoritmo, linha 05
        let (n, sz) = json_payload_size(t, i_target);
        json_blob_edit(t, i_target + n, sz, None, 0);
        let xb = t.blob_at(i_target);
        let v = t.blob_mut();
        if let Some(b) = v.get_mut(i_target as usize) {
            *b = (xb & 0xf0) | JSONB_OBJECT;
        }
    }
    let (n, sz) = json_payload_size(patch, i_patch);
    if n == 0 {
        return JSON_MERGE_BADPATCH;
    }
    let mut i_p_cursor = i_patch + n;
    let i_p_end = i_p_cursor + sz;
    let (n, sz) = json_payload_size(t, i_target);
    if n == 0 {
        return JSON_MERGE_BADTARGET;
    }
    let i_t_start = i_target + n;
    let i_t_end_be = i_t_start + sz;

    while i_p_cursor < i_p_end {
        // Algoritmo, linha 07
        let i_p_label = i_p_cursor;
        let e_p_label = patch.blob_at(i_p_cursor) & 0x0f;
        if e_p_label < JSONB_TEXT || e_p_label > JSONB_TEXTRAW {
            return JSON_MERGE_BADPATCH;
        }
        let (n_p_label, sz_p_label) = json_payload_size(patch, i_p_cursor);
        if n_p_label == 0 {
            return JSON_MERGE_BADPATCH;
        }
        let i_p_value = i_p_cursor + n_p_label + sz_p_label;
        if i_p_value >= i_p_end {
            return JSON_MERGE_BADPATCH;
        }
        let (n_p_value, sz_p_value) = json_payload_size(patch, i_p_value);
        if n_p_value == 0 {
            return JSON_MERGE_BADPATCH;
        }
        i_p_cursor = i_p_value + n_p_value + sz_p_value;
        if i_p_cursor > i_p_end {
            return JSON_MERGE_BADPATCH;
        }

        let mut i_t_cursor = i_t_start;
        let i_t_end = i_t_end_be.wrapping_add(t.delta as u32);
        let mut i_t_label = 0u32;
        let mut n_t_label = 0u32;
        let mut sz_t_label = 0u32;
        let mut i_t_value = 0u32;
        let mut n_t_value = 0u32;
        let mut sz_t_value = 0u32;
        while i_t_cursor < i_t_end {
            i_t_label = i_t_cursor;
            let e_t_label = t.blob_at(i_t_cursor) & 0x0f;
            if e_t_label < JSONB_TEXT || e_t_label > JSONB_TEXTRAW {
                return JSON_MERGE_BADTARGET;
            }
            let (a, b) = json_payload_size(t, i_t_cursor);
            n_t_label = a;
            sz_t_label = b;
            if n_t_label == 0 {
                return JSON_MERGE_BADTARGET;
            }
            i_t_value = i_t_label + n_t_label + sz_t_label;
            if i_t_value >= i_t_end {
                return JSON_MERGE_BADTARGET;
            }
            let (a, b) = json_payload_size(t, i_t_value);
            n_t_value = a;
            sz_t_value = b;
            if n_t_value == 0 {
                return JSON_MERGE_BADTARGET;
            }
            if i_t_value + n_t_value + sz_t_value > i_t_end {
                return JSON_MERGE_BADTARGET;
            }
            let is_equal = json_label_compare(
                sub(patch.blob(), i_p_label + n_p_label, sz_p_label),
                e_p_label == JSONB_TEXT || e_p_label == JSONB_TEXTRAW,
                sub(t.blob(), i_t_label + n_t_label, sz_t_label),
                e_t_label == JSONB_TEXT || e_t_label == JSONB_TEXTRAW,
            );
            if is_equal {
                break;
            }
            i_t_cursor = i_t_value + n_t_value + sz_t_value;
        }
        x = patch.blob_at(i_p_value) & 0x0f;
        if i_t_cursor < i_t_end {
            // Achou um par. Algoritmo, linha 08
            if x == 0 {
                // O valor do remendo é NULL. Linha 09
                json_blob_edit(
                    t,
                    i_t_label,
                    n_t_label + sz_t_label + n_t_value + sz_t_value,
                    None,
                    0,
                );
            } else {
                // Linha 12
                let saved_delta = t.delta;
                t.delta = 0;
                let rc = json_merge_patch(t, i_t_value, patch, i_p_value);
                if rc != 0 {
                    return rc;
                }
                t.delta += saved_delta;
            }
        } else if x > 0 {
            // Algoritmo, linha 13: sem par e o valor do remendo não é NULL.
            let sz_new = sz_p_label + n_p_label;
            if (patch.blob_at(i_p_value) & 0x0f) != JSONB_OBJECT {
                // Linha 14
                json_blob_edit(t, i_t_end, 0, None, sz_p_value + n_p_value + sz_new);
                json_copy_into(t, i_t_end as usize, sub(patch.blob(), i_p_label, sz_new));
                json_copy_into(
                    t,
                    (i_t_end + sz_new) as usize,
                    sub(patch.blob(), i_p_value, sz_p_value + n_p_value),
                );
            } else {
                json_blob_edit(t, i_t_end, 0, None, sz_new + 1);
                json_copy_into(t, i_t_end as usize, sub(patch.blob(), i_p_label, sz_new));
                json_copy_into(t, (i_t_end + sz_new) as usize, &[0x00]);
                let saved_delta = t.delta;
                t.delta = 0;
                let rc = json_merge_patch(t, i_t_end + sz_new, patch, i_p_value);
                if rc != 0 {
                    return rc;
                }
                t.delta += saved_delta;
            }
        }
    }
    if t.delta != 0 {
        json_after_edit_size_adjust(t, i_target);
    }
    if t.oom { JSON_MERGE_OOM } else { JSON_MERGE_OK }
}

/// `json_mergepatch(JSON1,JSON2)`: o objeto que resulta do MergePatch da RFC 7396.
fn json_patch_func(ctx: &mut Context<'_>, argv: &[Mem]) {
    let Some(mut target) = json_parse_func_arg(ctx, &argv[0], JSON_EDITABLE) else {
        return;
    };
    if let Some(patch) = json_parse_func_arg(ctx, &argv[1], 0) {
        let rc = json_merge_patch(&mut target, 0, &patch, 0);
        if rc == JSON_MERGE_OK {
            json_return_parse(ctx, &mut target);
        } else if rc == JSON_MERGE_OOM {
            result_error_nomem(ctx);
        } else {
            result_error(ctx, b"malformed JSON", -1);
        }
    }
}

/// `json_object(NAME,VALUE,...)`: o objeto JSON com todos os pares dados.
fn json_object_func(ctx: &mut Context<'_>, argv: &[Mem]) {
    let argc = argv.len();
    if argc & 1 != 0 {
        result_error(ctx, b"json_object() requires an even number of arguments", -1);
        return;
    }
    let mut jx = JsonString::new();
    jx.append_char(b'{');
    let mut i = 0usize;
    while i < argc {
        if value_type(&argv[i]) != SQLITE_TEXT {
            result_error(ctx, b"json_object() labels must be TEXT", -1);
            jx.reset();
            return;
        }
        jx.append_separator();
        if let Some(z) = text_of(&argv[i]) {
            jx.append_string(&z);
        }
        jx.append_char(b':');
        json_append_sql_value(ctx, &mut jx, &argv[i + 1]);
        i += 2;
    }
    jx.append_char(b'}');
    json_return_string(ctx, &mut jx, None);
    result_subtype(ctx, JSON_SUBTYPE);
}

/// `json_remove(JSON, PATH, ...)`: tira os elementos nomeados. JSON ou PATH mal formados dão erro.
fn json_remove_func(ctx: &mut Context<'_>, argv: &[Mem]) {
    let argc = argv.len();
    if argc < 1 {
        return;
    }
    let Some(mut p) = json_parse_func_arg(ctx, &argv[0], if argc > 1 { JSON_EDITABLE } else { 0 })
    else {
        return;
    };
    for a in &argv[1..] {
        let Some(zp_raw) = text_of(a) else {
            return;
        };
        let z_path = cstr(&zp_raw);
        if at(z_path, 0) != b'$' {
            json_bad_path_error(Some(&mut *ctx), z_path);
            return;
        }
        if at(z_path, 1) == 0 {
            // json_remove(j,'$') devolve NULL.
            return;
        }
        p.e_edit = JEDIT_DEL;
        p.delta = 0;
        let rc = json_lookup_step(&mut p, 0, off(z_path, 1), 0);
        if json_lookup_is_error(rc) {
            if rc == JSON_LOOKUP_NOTFOUND {
                continue; // Nada a fazer
            } else if rc == JSON_LOOKUP_PATHERROR {
                json_bad_path_error(Some(&mut *ctx), z_path);
            } else {
                result_error(ctx, b"malformed JSON", -1);
            }
            return;
        }
    }
    json_return_parse(ctx, &mut p);
}

/// `json_replace(JSON, PATH, VALUE, ...)`: troca o valor em PATH por VALUE; se PATH não existe,
/// não faz nada.
fn json_replace_func(ctx: &mut Context<'_>, argv: &[Mem]) {
    if argv.is_empty() {
        return;
    }
    if (argv.len() & 1) == 0 {
        json_wrong_num_args(ctx, "replace");
        return;
    }
    json_insert_into_blob(ctx, argv, JEDIT_REPL);
}

/// `json_set(JSON, PATH, VALUE, ...)` e `json_insert(JSON, PATH, VALUE, ...)`: o primeiro põe
/// VALUE em PATH (criando o caminho e sobrescrevendo); o segundo cria PATH e não faz nada se ele
/// já existe.
fn json_set_func(ctx: &mut Context<'_>, argv: &[Mem]) {
    let flags = user_flags(ctx);
    let b_is_set = (flags & JSON_ISSET) != 0;
    if argv.is_empty() {
        return;
    }
    if (argv.len() & 1) == 0 {
        json_wrong_num_args(ctx, if b_is_set { "set" } else { "insert" });
        return;
    }
    json_insert_into_blob(ctx, argv, if b_is_set { JEDIT_SET } else { JEDIT_INS });
}

/// `json_type(JSON)` e `json_type(JSON, PATH)`: o "tipo" do elemento. Dá erro se JSON ou PATH
/// estão mal formados.
fn json_type_func(ctx: &mut Context<'_>, argv: &[Mem]) {
    let Some(mut p) = json_parse_func_arg(ctx, &argv[0], 0) else {
        return;
    };
    let mut i: u32 = 0;
    if argv.len() == 2 {
        let Some(zp_raw) = text_of(&argv[1]) else {
            return;
        };
        let z_path = cstr(&zp_raw);
        if at(z_path, 0) != b'$' {
            json_bad_path_error(Some(&mut *ctx), z_path);
            return;
        }
        i = json_lookup_step(&mut p, 0, off(z_path, 1), 0);
        if json_lookup_is_error(i) {
            if i == JSON_LOOKUP_NOTFOUND {
                // nada
            } else if i == JSON_LOOKUP_PATHERROR {
                json_bad_path_error(Some(&mut *ctx), z_path);
            } else {
                result_error(ctx, b"malformed JSON", -1);
            }
            return;
        }
    }
    let name = JSONB_TYPE[(p.blob_at(i) & 0x0f) as usize];
    result_text(ctx, Some(name.as_bytes()), -1, StrDtor::Static);
}

/// `json_pretty(JSON)` e `json_pretty(JSON, INDENT)`: o JSON com indentação para leitura. INDENT
/// é o texto da indentação (quatro espaços se omitido, como no PostgreSQL).
fn json_pretty_func(ctx: &mut Context<'_>, argv: &[Mem]) {
    let Some(p) = json_parse_func_arg(ctx, &argv[0], 0) else {
        return;
    };
    let mut s = JsonString::new();
    let indent: Vec<u8> = if argv.len() == 1 {
        b"    ".to_vec()
    } else {
        match text_of(&argv[1]) {
            Some(z) => cstr(&z).to_vec(),
            None => b"    ".to_vec(),
        }
    };
    {
        let mut x = JsonPretty { p_parse: &p, p_out: &mut s, z_indent: indent, n_indent: 0 };
        json_translate_blob_to_pretty_text(&mut x, 0);
    }
    json_return_string(ctx, &mut s, None);
}

/// `json_valid(JSON)` e `json_valid(JSON, FLAGS)`: confere se o argumento é bem formado. FLAGS
/// codifica as restrições:
///
/// ```text
///     0x01      Canonical RFC-8259 JSON text
///     0x02      JSON text with optional JSON-5 extensions
///     0x04      Superficially appears to be JSONB
///     0x08      Strictly well-formed JSONB
/// ```
///
/// Só os quatro bits baixos de FLAGS valem; um bit mais alto é erro. Sem FLAGS vale 1. Devolve
/// um erro se FLAGS está fora de 1 a 15, NULL se a entrada é NULL, 1 se é bem formada e 0 se não.
fn json_valid_func(ctx: &mut Context<'_>, argv: &[Mem]) {
    let mut flags: u8 = 1;
    let mut res: u8 = 0;
    if argv.len() == 2 {
        let f = value_int64(&argv[1]);
        if !(1..=15).contains(&f) {
            result_error(ctx, b"FLAGS parameter to json_valid() must be between 1 and 15", -1);
            return;
        }
        flags = (f & 0x0f) as u8;
    }
    match value_type(&argv[0]) {
        SQLITE_NULL => return,
        crate::consts::SQLITE_BLOB if json_func_arg_might_be_binary(&argv[0]) => {
            if flags & 0x04 != 0 {
                // Só a conferência superficial, que o json_func_arg_might_be_binary() já fez.
                res = 1;
            } else if flags & 0x08 != 0 {
                // Conferência estrita.
                let blob = blob_of(&argv[0]);
                let i_err = json_validity_check(&blob, 0, blob.len() as u32, 1);
                res = (i_err == 0) as u8;
            }
        }
        _ => {
            // Vale também para o BLOB que não parece JSONB: lê-se como texto.
            if (flags & 0x3) != 0 {
                match json_parse_func_arg(ctx, &argv[0], JSON_KEEPERROR) {
                    Some(p) => {
                        if p.oom {
                            result_error_nomem(ctx);
                        } else if p.n_err != 0 {
                            // nada
                        } else if (flags & 0x02) != 0 || !p.has_nonstd {
                            res = 1;
                        }
                    }
                    None => result_error_nomem(ctx),
                }
            }
        }
    }
    result_int(ctx, res as i32);
}

/// `json_error_position(JSON)`: NULL se o argumento é NULL. Com um BLOB, faz a conferência
/// completa e devolve diferente de zero se falha (o deslocamento aproximado, a partir de 1, do
/// elemento com o primeiro erro). Senão lê o argumento como texto e devolve a posição (em
/// caracteres, a partir de 1) em que o analisador viu que não era JSON, ou 0 se o texto está bem.
fn json_error_func(ctx: &mut Context<'_>, argv: &[Mem]) {
    let mut i_err_pos: i64 = 0;
    if json_func_arg_might_be_binary(&argv[0]) {
        let blob = blob_of(&argv[0]);
        i_err_pos = json_validity_check(&blob, 0, blob.len() as u32, 1) as i64;
    } else {
        let Some(z) = text_of(&argv[0]) else {
            return; // Entrada NULL ou falta de memória
        };
        let zj: Rc<Vec<u8>> = Rc::new(z.into_owned());
        let mut s = JsonParse::default();
        s.z_json = Some(zj.clone());
        if json_convert_text_to_blob(&mut s, None) != 0 {
            if s.oom {
                i_err_pos = -1;
            } else {
                // Converte o deslocamento em bytes (`s.i_err`) em deslocamento em caracteres.
                let mut k = 0u32;
                while k < s.i_err && at(&zj, k as usize) != 0 {
                    if (at(&zj, k as usize) & 0xc0) != 0x80 {
                        i_err_pos += 1;
                    }
                    k += 1;
                }
                i_err_pos += 1;
            }
        }
    }
    if i_err_pos < 0 {
        result_error_nomem(ctx);
    } else {
        result_int64(ctx, i_err_pos);
    }
}

// ---------------------------------------------------------------------------------------------
// Funções agregadas
// ---------------------------------------------------------------------------------------------

/// Tira do contexto do agregado o acumulador (a posse volta com [`json_group_put`]); cria a
/// cadeia com o caractere `open` na primeira chamada e põe a vírgula nas seguintes.
fn json_group_begin(ctx: &mut Context<'_>, open: u8) -> Option<JsonString> {
    let slot = aggregate_context::<Option<JsonString>>(ctx, true)?;
    let s = match slot.take() {
        None => {
            let mut n = JsonString::new();
            n.append_char(open);
            n
        }
        Some(mut x) => {
            if x.buf.len() > 1 {
                x.append_char(b',');
            }
            x
        }
    };
    Some(s)
}

/// Devolve o acumulador ao contexto do agregado.
fn json_group_put(ctx: &mut Context<'_>, s: JsonString) {
    if let Some(slot) = aggregate_context::<Option<JsonString>>(ctx, true) {
        *slot = Some(s);
    }
}

/// `json_group_array(VALUE)`: o passo.
fn json_array_step(ctx: &mut Context<'_>, argv: &[Mem]) {
    let Some(mut s) = json_group_begin(ctx, b'[') else {
        return;
    };
    json_append_sql_value(ctx, &mut s, &argv[0]);
    json_group_put(ctx, s);
}

/// `json_group_obj(NAME,VALUE)`: o passo.
fn json_object_step(ctx: &mut Context<'_>, argv: &[Mem]) {
    let Some(mut s) = json_group_begin(ctx, b'{') else {
        return;
    };
    if let Some(z) = text_of(&argv[0]) {
        s.append_string(crate::json::cstr(&z));
    }
    s.append_char(b':');
    json_append_sql_value(ctx, &mut s, &argv[1]);
    json_group_put(ctx, s);
}

/// `jsonArrayCompute` e `jsonObjectCompute`: o resultado do agregado. `close` fecha a cadeia e
/// `empty` é o resultado de um grupo vazio. Sem `is_final` (função de janela) a cadeia continua
/// aberta para os próximos passos.
fn json_group_compute(ctx: &mut Context<'_>, is_final: bool, close: u8, empty: &'static [u8]) {
    let state = aggregate_context::<Option<JsonString>>(ctx, false).map(|s| s.take());
    match state {
        Some(Some(mut s)) => {
            s.append_char(close);
            let flags = user_flags(ctx);
            if s.e_err != 0 {
                json_return_string(ctx, &mut s, None);
                json_group_put(ctx, s);
                return;
            } else if (flags & JSON_BLOB) != 0 {
                json_return_string_as_blob(ctx, &mut s);
                if !is_final {
                    s.trim_one_char();
                }
                json_group_put(ctx, s);
                return;
            } else {
                result_text(ctx, Some(&s.buf), s.buf.len() as i32, StrDtor::Transient);
                if !is_final {
                    s.trim_one_char();
                }
            }
            json_group_put(ctx, s);
        }
        _ => {
            result_text(ctx, Some(empty), 2, StrDtor::Static);
        }
    }
    result_subtype(ctx, JSON_SUBTYPE);
}

/// `jsonArrayValue`.
fn json_array_value(ctx: &mut Context<'_>) {
    json_group_compute(ctx, false, b']', b"[]");
}

/// `jsonArrayFinal`.
fn json_array_final(ctx: &mut Context<'_>) {
    json_group_compute(ctx, true, b']', b"[]");
}

/// `jsonObjectValue`.
fn json_object_value(ctx: &mut Context<'_>) {
    json_group_compute(ctx, false, b'}', b"{}");
}

/// `jsonObjectFinal`.
fn json_object_final(ctx: &mut Context<'_>) {
    json_group_compute(ctx, true, b'}', b"{}");
}

/// `jsonGroupInverse`: serve a `json_group_array()` e a `json_group_object()`. Tira o primeiro
/// elemento do grupo: procura a primeira vírgula que não está dentro de uma cadeia e apaga todo
/// o texto até ela.
fn json_group_inverse(ctx: &mut Context<'_>, _argv: &[Mem]) {
    let Some(slot) = aggregate_context::<Option<JsonString>>(ctx, false) else {
        return;
    };
    let Some(s) = slot.as_mut() else {
        return;
    };
    let mut in_str = false;
    let mut n_nest: i32 = 0;
    let n_used = s.buf.len();
    let mut i = 1usize;
    while i < n_used {
        let c = s.buf[i];
        if c == b',' && !in_str && n_nest == 0 {
            break;
        }
        if c == b'"' {
            in_str = !in_str;
        } else if c == b'\\' {
            i += 1;
        } else if !in_str {
            if c == b'{' || c == b'[' {
                n_nest += 1;
            }
            if c == b'}' || c == b']' {
                n_nest -= 1;
            }
        }
        i += 1;
    }
    if i < n_used {
        s.buf.drain(1..i + 1);
    } else {
        s.buf.truncate(1);
    }
}

// ---------------------------------------------------------------------------------------------
// As tabelas virtuais json_each e json_tree
// ---------------------------------------------------------------------------------------------

/// Número das colunas.
const JEACH_KEY: i32 = 0;
const JEACH_VALUE: i32 = 1;
const JEACH_TYPE: i32 = 2;
const JEACH_ATOM: i32 = 3;
const JEACH_ID: i32 = 4;
const JEACH_PARENT: i32 = 5;
const JEACH_FULLKEY: i32 = 6;
const JEACH_PATH: i32 = 7;
// O `xBestIndex` supõe que JSON e ROOT são as duas últimas colunas da tabela.
const JEACH_JSON: i32 = 8;
const JEACH_ROOT: i32 = 9;

/// `struct JsonParent`: um elemento pai do elemento corrente.
#[derive(Clone, Copy, Default)]
struct JsonParent {
    /// Início do objeto ou vetor.
    i_head: u32,
    /// Início do valor.
    i_value: u32,
    /// O primeiro byte depois do fim.
    i_end: u32,
    /// Comprimento do caminho.
    n_path: u32,
    /// A chave de um `JSONB_ARRAY`.
    i_key: i64,
}

/// `struct JsonEachCursor`: o cursor de `json_each` e `json_tree`.
struct JsonEachCursor {
    /// O rowid.
    i_rowid: u32,
    /// Índice, em `s_parse.a_blob`, da linha corrente.
    i: u32,
    /// Fim do arquivo quando `i` iguala ou passa disto.
    i_end: u32,
    /// Tamanho do caminho raiz, em bytes.
    n_root: u32,
    /// O tipo do contêiner do elemento `i`.
    e_type: u8,
    /// Verdadeiro em `json_tree()`, falso em `json_each()`.
    b_recursive: bool,
    /// Os pais do elemento `i` (`aParent[0..nParent]`).
    a_parent: Vec<JsonParent>,
    /// O caminho corrente.
    path: JsonString,
    /// A análise do JSON de entrada.
    s_parse: JsonParse,
}

impl JsonEachCursor {
    /// `jsonEachOpenEach` e `jsonEachOpenTree`.
    fn new(b_recursive: bool) -> JsonEachCursor {
        JsonEachCursor {
            i_rowid: 0,
            i: 0,
            i_end: 0,
            n_root: 0,
            e_type: 0,
            b_recursive,
            a_parent: Vec::new(),
            path: JsonString::new(),
            s_parse: JsonParse::default(),
        }
    }

    /// `jsonEachCursorReset`: o cursor volta ao estado original.
    fn reset(&mut self) {
        json_parse_reset(&mut self.s_parse);
        self.path.reset();
        self.a_parent.clear();
        self.i_rowid = 0;
        self.i = 0;
        self.i_end = 0;
        self.e_type = 0;
    }

    /// `jsonSkipLabel`: se o cursor está no rótulo de uma entrada de objeto, o índice do valor;
    /// senão, a posição corrente, que é o valor.
    fn skip_label(&self) -> u32 {
        if self.e_type == JSONB_OBJECT {
            let (n, sz) = json_payload_size(&self.s_parse, self.i);
            self.i + n + sz
        } else {
            self.i
        }
    }

    /// `jsonAppendPathName`: acrescenta o nome do caminho do elemento corrente.
    fn append_path_name(&mut self) {
        if self.e_type == JSONB_ARRAY {
            let k = self.a_parent.last().map_or(0, |x| x.i_key);
            self.path.append_raw(format!("[{}]", k).as_bytes());
        } else {
            let (n, sz) = json_payload_size(&self.s_parse, self.i);
            let k = self.i + n;
            let z = sub(self.s_parse.blob(), k, sz);
            let mut need_quote = false;
            if sz == 0 || !is_alpha(at(z, 0)) {
                need_quote = true;
            } else {
                for &c in z.iter() {
                    if !is_alnum(c) {
                        need_quote = true;
                        break;
                    }
                }
            }
            // `%.*s` do printf do SQLite para no primeiro NUL.
            let shown = cstr(z);
            if need_quote {
                self.path.append_raw(b".\"");
                self.path.append_raw(shown);
                self.path.append_raw(b"\"");
            } else {
                self.path.append_raw(b".");
                self.path.append_raw(shown);
            }
        }
    }

    /// `jsonEachNext`: avança o cursor para o próximo elemento.
    fn next_row(&mut self) -> i32 {
        if self.b_recursive {
            let mut level_change = false;
            let i = self.skip_label();
            let x = self.s_parse.blob_at(i) & 0x0f;
            let (n, sz) = json_payload_size(&self.s_parse, i);
            if x == JSONB_OBJECT || x == JSONB_ARRAY {
                level_change = true;
                let parent = JsonParent {
                    i_head: self.i,
                    i_value: i,
                    i_end: i + n + sz,
                    i_key: -1,
                    n_path: self.path.buf.len() as u32,
                };
                if self.e_type != 0 && !self.a_parent.is_empty() {
                    self.append_path_name();
                }
                self.a_parent.push(parent);
                self.i = i + n;
            } else {
                self.i = i + n + sz;
            }
            while let Some(top) = self.a_parent.last().copied() {
                if self.i < top.i_end {
                    break;
                }
                self.a_parent.pop();
                self.path.buf.truncate(top.n_path as usize);
                level_change = true;
            }
            if level_change {
                self.e_type = match self.a_parent.last() {
                    Some(pr) => self.s_parse.blob_at(pr.i_value) & 0x0f,
                    None => 0,
                };
            }
        } else {
            let i = self.skip_label();
            let (n, sz) = json_payload_size(&self.s_parse, i);
            self.i = i + n + sz;
        }
        if self.e_type == JSONB_ARRAY {
            if let Some(top) = self.a_parent.last_mut() {
                top.i_key += 1;
            }
        }
        self.i_rowid += 1;
        SQLITE_OK
    }

    /// `jsonEachPathLength`: o comprimento do caminho da linha de rowid 0 no modo recursivo.
    fn path_length(&mut self) -> u32 {
        let mut n = self.path.buf.len() as u32;
        if self.i_rowid == 0 && self.b_recursive && n >= 2 {
            while n > 1 {
                n -= 1;
                let c = self.path.buf[n as usize];
                if c == b'[' || c == b'.' {
                    let x = json_lookup_step(&mut self.s_parse, 0, &self.path.buf[1..n as usize], 0);
                    if json_lookup_is_error(x) {
                        continue;
                    }
                    let (nn, _sz) = json_payload_size(&self.s_parse, x);
                    if x + nn == self.i {
                        break;
                    }
                }
            }
        }
        n
    }
}

/// `struct JsonEachConnection`: a tabela virtual `json_each` ou `json_tree`.
struct JsonEachVtab {
    /// `zErrMsg`.
    z_err_msg: Option<Vec<u8>>,
    /// Verdadeiro em `json_tree()`.
    b_recursive: bool,
}

impl Vtab for JsonEachVtab {
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn err_msg_mut(&mut self) -> &mut Option<Vec<u8>> {
        &mut self.z_err_msg
    }

    /// `jsonEachBestIndex`: a estratégia é procurar uma restrição de igualdade na coluna `json`.
    /// Sem ela a tabela não opera. `idxNum` é 1 se a restrição existe, 3 se existem ela e
    /// `root`, e 0 senão.
    fn best_index(&mut self, _db: &mut Connection, info: &mut IndexInfo) -> i32 {
        let mut a_idx: [i32; 2] = [-1, -1]; // Índice das restrições de JSON e ROOT
        let mut unusable_mask = 0; // Máscara das restrições inutilizáveis de JSON e ROOT
        let mut idx_mask = 0; // Máscara das restrições == utilizáveis de JSON e ROOT

        // Esta implementação supõe que JSON e ROOT são as duas últimas colunas da tabela.
        for (i, c) in info.a_constraint.iter().enumerate() {
            if c.i_column < JEACH_JSON {
                continue;
            }
            let i_col = (c.i_column - JEACH_JSON) as usize;
            if i_col > 1 {
                continue;
            }
            let i_mask = 1 << i_col;
            if !c.usable {
                unusable_mask |= i_mask;
            } else if c.op as i32 == SQLITE_INDEX_CONSTRAINT_EQ {
                a_idx[i_col] = i as i32;
                idx_mask |= i_mask;
            }
        }
        if let Some(o) = info.a_order_by.first() {
            if o.i_column < 0 && !o.desc {
                info.order_by_consumed = 1;
            }
        }

        if (unusable_mask & !idx_mask) != 0 {
            // Qualquer restrição inutilizável em JSON ou ROOT rejeita o plano inteiro.
            return SQLITE_CONSTRAINT;
        }
        if a_idx[0] < 0 {
            // Sem a entrada JSON. O `estimatedCost` fica no valor enorme com que começou, para
            // que o planejador evite este plano.
            info.idx_num = 0;
        } else {
            info.estimated_cost = 1.0;
            let i = a_idx[0] as usize;
            info.a_constraint_usage[i].argv_index = 1;
            info.a_constraint_usage[i].omit = true;
            if a_idx[1] < 0 {
                info.idx_num = 1; // Só JSON. Plano 1
            } else {
                let i = a_idx[1] as usize;
                info.a_constraint_usage[i].argv_index = 2;
                info.a_constraint_usage[i].omit = true;
                info.idx_num = 3; // JSON e ROOT. Plano 3
            }
        }
        SQLITE_OK
    }

    /// `jsonEachDisconnect`.
    fn disconnect(&mut self, _db: &mut Connection) -> i32 {
        SQLITE_OK
    }

    /// `xDestroy` é nulo no módulo (a tabela é epônima e não se destrói).
    fn destroy(&mut self, _db: &mut Connection) -> i32 {
        SQLITE_OK
    }

    /// `jsonEachOpenEach` e `jsonEachOpenTree`.
    fn open(&mut self, _db: &mut Connection) -> Result<Box<dyn VtabCursor>, i32> {
        Ok(Box::new(JsonEachCursor::new(self.b_recursive)))
    }
}

impl VtabCursor for JsonEachCursor {
    /// `jsonEachClose`.
    fn close(&mut self, _db: &mut Connection, _vtab: &mut dyn Vtab) -> i32 {
        self.reset();
        SQLITE_OK
    }

    /// `jsonEachFilter`: começa uma busca num JSON novo.
    fn filter(
        &mut self,
        _db: &mut Connection,
        vtab: &mut dyn Vtab,
        idx_num: i32,
        _idx_str: Option<&[u8]>,
        argv: &[Mem],
    ) -> i32 {
        self.reset();
        if idx_num == 0 {
            return SQLITE_OK;
        }
        self.s_parse = JsonParse::default();
        if json_func_arg_might_be_binary(&argv[0]) {
            self.s_parse.a_blob = Rc::new(blob_of(&argv[0]).into_owned());
        } else {
            let Some(z) = text_of(&argv[0]) else {
                self.i = 0;
                self.i_end = 0;
                return SQLITE_OK;
            };
            self.s_parse.z_json = Some(Rc::new(z.into_owned()));
            if json_convert_text_to_blob(&mut self.s_parse, None) != 0 {
                if self.s_parse.oom {
                    return SQLITE_NOMEM;
                }
                // json_each_malformed_input
                let msg = crate::printf::mprintf(b"malformed JSON", &[]);
                *vtab.err_msg_mut() = msg.clone();
                self.reset();
                return if msg.is_some() { SQLITE_ERROR } else { SQLITE_NOMEM };
            }
        }
        let i: u32;
        if idx_num == 3 {
            let Some(z_root_raw) = text_of(&argv[1]) else {
                return SQLITE_OK;
            };
            let z_root = cstr(&z_root_raw);
            if at(z_root, 0) != b'$' {
                let msg = json_bad_path_error(None, z_root);
                *vtab.err_msg_mut() = msg.clone();
                self.reset();
                return if msg.is_some() { SQLITE_ERROR } else { SQLITE_NOMEM };
            }
            self.n_root = z_root.len() as u32;
            if at(z_root, 1) == 0 {
                i = 0;
                self.i = 0;
                self.e_type = 0;
            } else {
                i = json_lookup_step(&mut self.s_parse, 0, off(z_root, 1), 0);
                if json_lookup_is_error(i) {
                    if i == JSON_LOOKUP_NOTFOUND {
                        self.i = 0;
                        self.e_type = 0;
                        self.i_end = 0;
                        return SQLITE_OK;
                    }
                    let msg = json_bad_path_error(None, z_root);
                    *vtab.err_msg_mut() = msg.clone();
                    self.reset();
                    return if msg.is_some() { SQLITE_ERROR } else { SQLITE_NOMEM };
                }
                if self.s_parse.i_label != 0 {
                    self.i = self.s_parse.i_label;
                    self.e_type = JSONB_OBJECT;
                } else {
                    self.i = i;
                    self.e_type = JSONB_ARRAY;
                }
            }
            self.path.append_raw(z_root);
        } else {
            i = 0;
            self.i = 0;
            self.e_type = 0;
            self.n_root = 1;
            self.path.append_raw(b"$");
        }
        self.a_parent.clear();
        let (n, sz) = json_payload_size(&self.s_parse, i);
        self.i_end = i + n + sz;
        if (self.s_parse.blob_at(i) & 0x0f) >= JSONB_ARRAY && !self.b_recursive {
            self.i = i + n;
            self.e_type = self.s_parse.blob_at(i) & 0x0f;
            self.a_parent.push(JsonParent {
                i_head: self.i,
                i_value: i,
                i_end: self.i_end,
                n_path: 0,
                i_key: 0,
            });
        }
        SQLITE_OK
    }

    /// `jsonEachNext`.
    fn next(&mut self, _db: &mut Connection, _vtab: &mut dyn Vtab) -> i32 {
        self.next_row()
    }

    /// `jsonEachEof`.
    fn eof(&mut self, _vtab: &mut dyn Vtab) -> i32 {
        (self.i >= self.i_end) as i32
    }

    /// `jsonEachColumn`: o valor de uma coluna.
    fn column(&mut self, _vtab: &mut dyn Vtab, ctx: &mut Context<'_>, i_column: i32) -> i32 {
        match i_column {
            JEACH_KEY => {
                if self.a_parent.is_empty() {
                    if self.n_root == 1 {
                        return SQLITE_OK;
                    }
                    let j = self.path_length();
                    let n = self.n_root.wrapping_sub(j);
                    if n == 0 {
                    } else if at(&self.path.buf, j as usize) == b'[' {
                        let mut x: i64 = 0;
                        atoi64(
                            off(&self.path.buf, j as usize + 1),
                            &mut x,
                            n.wrapping_sub(1) as i32,
                            SQLITE_UTF8 as u8,
                        );
                        result_int64(ctx, x);
                    } else if at(&self.path.buf, j as usize + 1) == b'"' {
                        let part = sub(&self.path.buf, j + 2, n.wrapping_sub(3));
                        result_text(ctx, Some(part), part.len() as i32, StrDtor::Transient);
                    } else {
                        let part = sub(&self.path.buf, j + 1, n.wrapping_sub(1));
                        result_text(ctx, Some(part), part.len() as i32, StrDtor::Transient);
                    }
                    return SQLITE_OK;
                }
                if self.e_type == JSONB_OBJECT {
                    json_return_from_blob(ctx, &self.s_parse, self.i, true);
                } else if let Some(top) = self.a_parent.last() {
                    result_int64(ctx, top.i_key);
                }
            }
            JEACH_VALUE => {
                let i = self.skip_label();
                json_return_from_blob(ctx, &self.s_parse, i, true);
                if (self.s_parse.blob_at(i) & 0x0f) >= JSONB_ARRAY {
                    result_subtype(ctx, JSON_SUBTYPE);
                }
            }
            JEACH_TYPE => {
                let i = self.skip_label();
                let e_type = self.s_parse.blob_at(i) & 0x0f;
                result_text(ctx, Some(JSONB_TYPE[e_type as usize].as_bytes()), -1, StrDtor::Static);
            }
            JEACH_ATOM => {
                let i = self.skip_label();
                if (self.s_parse.blob_at(i) & 0x0f) < JSONB_ARRAY {
                    json_return_from_blob(ctx, &self.s_parse, i, true);
                }
            }
            JEACH_ID => {
                result_int64(ctx, self.i as i64);
            }
            JEACH_PARENT => {
                if self.b_recursive {
                    if let Some(top) = self.a_parent.last() {
                        result_int64(ctx, top.i_head as i64);
                    }
                }
            }
            JEACH_FULLKEY => {
                let n_base = self.path.buf.len();
                if !self.a_parent.is_empty() {
                    self.append_path_name();
                }
                result_text64(
                    ctx,
                    Some(&self.path.buf),
                    self.path.buf.len() as u64,
                    StrDtor::Transient,
                    SQLITE_UTF8 as u8,
                );
                self.path.buf.truncate(n_base);
            }
            JEACH_PATH => {
                let n = self.path_length();
                result_text64(
                    ctx,
                    Some(sub(&self.path.buf, 0, n)),
                    n as u64,
                    StrDtor::Transient,
                    SQLITE_UTF8 as u8,
                );
            }
            JEACH_JSON => {
                match &self.s_parse.z_json {
                    None => {
                        let blob = self.s_parse.blob();
                        crate::vdbeapi::result_blob(
                            ctx,
                            Some(blob),
                            blob.len() as i32,
                            StrDtor::Transient,
                        );
                    }
                    Some(zj) => {
                        result_text(ctx, Some(zj.as_slice()), -1, StrDtor::Transient);
                    }
                }
            }
            _ => {
                // JEACH_ROOT e o que sobrar.
                let part = sub(&self.path.buf, 0, self.n_root);
                result_text(ctx, Some(part), part.len() as i32, StrDtor::Static);
            }
        }
        SQLITE_OK
    }

    /// `jsonEachRowid`.
    fn rowid(&mut self, _vtab: &mut dyn Vtab, rowid: &mut i64) -> i32 {
        *rowid = self.i_rowid as i64;
        SQLITE_OK
    }
}

/// Os módulos `json_each` e `json_tree` (`jsonEachModule` e `jsonTreeModule`): sem `xCreate`,
/// portanto epônimos, e com `iVersion` 0.
struct JsonEachModule {
    /// Verdadeiro em `json_tree`.
    b_recursive: bool,
}

impl VtabModule for JsonEachModule {
    fn i_version(&self) -> i32 {
        0
    }

    fn caps(&self) -> ModuleCaps {
        ModuleCaps::default()
    }

    /// `jsonEachConnect`.
    fn x_connect(
        &self,
        db: &mut Connection,
        _aux: &Option<Rc<dyn Any>>,
        _argv: &[Vec<u8>],
        _err: &mut Option<Vec<u8>>,
    ) -> Result<Box<dyn Vtab>, i32> {
        let rc = declare_vtab(
            db,
            b"CREATE TABLE x(key,value,type,atom,id,parent,fullkey,path,json HIDDEN,root HIDDEN)",
        );
        if rc == SQLITE_OK {
            vtab_config(db, SQLITE_VTAB_INNOCUOUS, 0);
            Ok(Box::new(JsonEachVtab { z_err_msg: None, b_recursive: self.b_recursive }))
        } else {
            Err(rc)
        }
    }
}

/// `sqlite3JsonTableFunctions`: registra as funções de tabela `json_each` e `json_tree`.
pub fn json_table_functions(db: &mut Connection) -> i32 {
    let mut rc = SQLITE_OK;
    let a_mod: [(&[u8], bool); 2] = [(b"json_each", false), (b"json_tree", true)];
    for (z_name, b_recursive) in a_mod {
        if rc != SQLITE_OK {
            break;
        }
        let module: Rc<dyn VtabModule> = Rc::new(JsonEachModule { b_recursive });
        rc = create_module(db, z_name, Some(module), None, None);
    }
    rc
}

// ---------------------------------------------------------------------------------------------
// Registro das funções
// ---------------------------------------------------------------------------------------------

/// Macro `JFUNCTION` de `sqliteInt.h`: `b_use_cache` (a função usa o cache), `b_ws` (grava
/// `sqlite3_result_subtype()`), `b_rs` (lê `sqlite3_value_subtype()`) e `b_json_b` (devolve
/// JSONB); `i_arg` são os `JSON_*` do `pUserData`.
#[allow(clippy::too_many_arguments)]
fn jfunction(
    name: &str,
    n_arg: i8,
    b_use_cache: bool,
    b_ws: bool,
    b_rs: bool,
    b_json_b: bool,
    i_arg: i32,
    x_func: ScalarFn,
) -> FuncDef {
    let mut flags = SQLITE_FUNC_BUILTIN
        | SQLITE_DETERMINISTIC as u32
        | SQLITE_FUNC_CONSTANT
        | SQLITE_UTF8 as u32;
    if b_use_cache {
        flags |= SQLITE_FUNC_RUNONLY;
    }
    if b_rs {
        flags |= SQLITE_SUBTYPE as u32;
    }
    if b_ws {
        flags |= SQLITE_RESULT_SUBTYPE as u32;
    }
    let arg = i_arg | if b_json_b { JSON_BLOB } else { 0 };
    FuncDef {
        n_arg,
        func_flags: flags,
        p_user_data: UserData::Int(arg as isize),
        x_s_func: Some(x_func),
        x_finalize: None,
        x_value: None,
        x_inverse: None,
        z_name: name.as_bytes().to_vec(),
        p_destructor: None,
    }
}

/// Macro `WAGGREGATE` de `sqliteInt.h` (sem `NEEDCOLL`).
fn waggregate(
    name: &str,
    n_arg: i8,
    arg: i32,
    x_step: ScalarFn,
    x_final: FinalFn,
    x_value: FinalFn,
    x_inverse: ScalarFn,
    f: u32,
) -> FuncDef {
    FuncDef {
        n_arg,
        func_flags: SQLITE_FUNC_BUILTIN | SQLITE_UTF8 as u32 | f,
        p_user_data: UserData::Int(arg as isize),
        x_s_func: Some(x_step),
        x_finalize: Some(x_final),
        x_value: Some(x_value),
        x_inverse: Some(x_inverse),
        z_name: name.as_bytes().to_vec(),
        p_destructor: None,
    }
}

/// `sqlite3RegisterJsonFunctions`: registra as funções JSON na tabela das funções embutidas
/// (chamada por `register_builtin_functions`).
pub fn register_json_functions() {
    let agg_flags = (SQLITE_SUBTYPE
        | SQLITE_RESULT_SUBTYPE
        | SQLITE_UTF8
        | SQLITE_DETERMINISTIC) as u32;
    let defs = vec![
        //          nome            nArg cache ws    rs    jsonb iArg
        jfunction("json", 1, true, true, false, false, 0, json_remove_func),
        jfunction("jsonb", 1, true, false, false, true, 0, json_remove_func),
        jfunction("json_array", -1, false, true, true, false, 0, json_array_func),
        jfunction("jsonb_array", -1, false, true, true, true, 0, json_array_func),
        jfunction("json_array_length", 1, true, false, false, false, 0, json_array_length_func),
        jfunction("json_array_length", 2, true, false, false, false, 0, json_array_length_func),
        jfunction("json_error_position", 1, true, false, false, false, 0, json_error_func),
        jfunction("json_extract", -1, true, true, false, false, 0, json_extract_func),
        jfunction("jsonb_extract", -1, true, false, false, true, 0, json_extract_func),
        jfunction("->", 2, true, true, false, false, JSON_JSON, json_extract_func),
        jfunction("->>", 2, true, false, false, false, JSON_SQL, json_extract_func),
        jfunction("json_insert", -1, true, true, true, false, 0, json_set_func),
        jfunction("jsonb_insert", -1, true, false, true, true, 0, json_set_func),
        jfunction("json_object", -1, false, true, true, false, 0, json_object_func),
        jfunction("jsonb_object", -1, false, true, true, true, 0, json_object_func),
        jfunction("json_patch", 2, true, true, false, false, 0, json_patch_func),
        jfunction("jsonb_patch", 2, true, false, false, true, 0, json_patch_func),
        jfunction("json_pretty", 1, true, false, false, false, 0, json_pretty_func),
        jfunction("json_pretty", 2, true, false, false, false, 0, json_pretty_func),
        jfunction("json_quote", 1, false, true, true, false, 0, json_quote_func),
        jfunction("json_remove", -1, true, true, false, false, 0, json_remove_func),
        jfunction("jsonb_remove", -1, true, false, false, true, 0, json_remove_func),
        jfunction("json_replace", -1, true, true, true, false, 0, json_replace_func),
        jfunction("jsonb_replace", -1, true, false, true, true, 0, json_replace_func),
        jfunction("json_set", -1, true, true, true, false, JSON_ISSET, json_set_func),
        jfunction("jsonb_set", -1, true, false, true, true, JSON_ISSET, json_set_func),
        jfunction("json_type", 1, true, false, false, false, 0, json_type_func),
        jfunction("json_type", 2, true, false, false, false, 0, json_type_func),
        jfunction("json_valid", 1, true, false, false, false, 0, json_valid_func),
        jfunction("json_valid", 2, true, false, false, false, 0, json_valid_func),
        waggregate(
            "json_group_array",
            1,
            0,
            json_array_step,
            json_array_final,
            json_array_value,
            json_group_inverse,
            agg_flags,
        ),
        waggregate(
            "jsonb_group_array",
            1,
            JSON_BLOB,
            json_array_step,
            json_array_final,
            json_array_value,
            json_group_inverse,
            agg_flags,
        ),
        waggregate(
            "json_group_object",
            2,
            0,
            json_object_step,
            json_object_final,
            json_object_value,
            json_group_inverse,
            agg_flags,
        ),
        waggregate(
            "jsonb_group_object",
            2,
            JSON_BLOB,
            json_object_step,
            json_object_final,
            json_object_value,
            json_group_inverse,
            agg_flags,
        ),
    ];
    crate::callback::insert_builtin_funcs(defs);
}
