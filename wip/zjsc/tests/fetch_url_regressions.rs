//! Regressões de Fetch API e de URL, medidas no bun 1.4.2. Cada caso fixa uma divergência dos goldens
//! `fetch_types` e `url` que já fechou.
use zjsc::api::module::evaluate_module_map;

fn log_of(source: &str) -> String {
    let outcome = evaluate_module_map(&[("main.mjs".to_owned(), source.to_owned())], "main.mjs");
    assert_eq!(outcome.error, None);
    outcome.log_json
}

/// O esquema em maiúsculas é baixado e reconhecido como especial: host minúsculo, porta padrão fora.
#[test]
fn uppercase_scheme_is_special() {
    let log = log_of("L(new URL('HTTP://EXAMPLE.com:80/A b').href); L(new Request('HTTP://EXAMPLE.com:80/A b?c d#e').url);");
    assert_eq!(log, r#"["http://example.com/A%20b","http://example.com/A%20b?c%20d#e"]"#);
}

/// Um array de verdade no corpo são as partes de um `Blob`, sem `Content-Type`; `Set` vira texto.
#[test]
fn array_body_is_blob_parts() {
    let log = log_of(
        "const r = new Response([1, [2, 3]]); L(r.headers.get('content-type')); L(await r.text()); \
         L(await new Response(new Set([1])).text());",
    );
    assert_eq!(log, r#"["null","12,3","[object Set]"]"#);
}

/// O `blob()` leva o `Content-Type` do cabeçalho como está, sem baixar a caixa.
#[test]
fn blob_keeps_header_case() {
    let log = log_of(
        "L((await new Response(new URLSearchParams('a=1')).blob()).type); \
         L((await new Response('x', { headers: { 'content-type': 'Text/HTML;Charset=UTF-8' } }).blob()).type);",
    );
    assert_eq!(log, r#"["application/x-www-form-urlencoded;charset=UTF-8","Text/HTML;Charset=UTF-8"]"#);
}

/// `URL`, `URLSearchParams` e `FormData` convertem texto como `USVString` do WebIDL (o Symbol tem a mensagem dos
/// bindings); `Headers` segue com a do JSC.
#[test]
fn usv_string_symbol_message() {
    let log = log_of(
        "const t = (f) => { try { f(); return 'ok'; } catch (e) { return e.message; } }; \
         L(t(() => new URL(Symbol()))); L(t(() => { new URL('http://a/').search = Symbol(); })); \
         L(t(() => new URLSearchParams().append('a', Symbol()))); L(t(() => new URLSearchParams([['a', Symbol()]]))); \
         L(t(() => new URLSearchParams({ a: Symbol() }))); L(t(() => new FormData().append(Symbol(), 'x'))); \
         L(t(() => new Headers().append(Symbol(), 'x')));",
    );
    let usv = "Cannot convert a Symbol value to a string";
    let expected: Vec<&str> = vec![usv, usv, usv, usv, usv, usv, "Cannot convert a symbol to a string"];
    assert_eq!(log, format!("[{}]", expected.iter().map(|text| format!("\"{text}\"")).collect::<Vec<_>>().join(",")));
}

/// `Response.json` de um BigInt de topo tem a mensagem própria; dentro de objeto, a do `JSON.stringify`.
#[test]
fn response_json_bigint_message() {
    let log = log_of(
        "for (const v of [1n, { a: 1n }]) { try { Response.json(v); } catch (e) { L(e.name + ': ' + e.message); } }",
    );
    assert_eq!(log, r#"["TypeError: Do not know how to serialize a BigInt","TypeError: JSON.stringify cannot serialize BigInt."]"#);
}

/// O `body` lido pela primeira vez depois de uma leitura nasce travado; o que já tinha sido lido antes segue
/// destravado.
#[test]
fn body_created_after_read_is_locked() {
    let log = log_of(
        "const x = new Response(new URLSearchParams('a=1')); await x.formData(); L(x.body.locked); \
         const y = new Response('abc'); const s = y.body; await y.text(); L(s.locked); L(y.body === s);",
    );
    assert_eq!(log, r#"["true","false","true"]"#);
}
