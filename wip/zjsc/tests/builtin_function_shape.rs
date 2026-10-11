//! A forma das funções dos módulos embutidos escritos em JavaScript contra o bun 1.4.2. No bun elas são builtins do JSC:
//! nascem sem `prototype` próprio (só o ganha quem o atribui, como `EventEmitter.prototype = {}` ou `$toClass`) e o
//! `Function.prototype.toString` mostra o fonte empacotado do módulo. `tests/golden/builtin_function_shape_main.js`
//! roda como `/app/main.js` e anda as exportações de url, events, stream, querystring, timers, string_decoder,
//! async_hooks e util; a saída tem de ser a do bun do oráculo (`builtin_function_shape_main.expected.txt`, gerada com
//! `bun /app/main.js` na imagem do oráculo): por função, o `prototype` próprio, `name`, `length`, as chaves próprias e o
//! comprimento, o hash e o começo do texto.
mod main_golden;

const PROBE: &str = include_str!("golden/builtin_function_shape_main.js");

#[test]
fn builtin_function_shapes_match_bun() {
    main_golden::assert_main_script_matches(PROBE, include_str!("golden/builtin_function_shape_main.expected.txt"));
}

/// O mesmo passeio só pelo `URLPattern` (`url.URLPattern`), que o zjsc ainda não tem: a dívida fica registrada aqui, e
/// não fora do golden.
#[test]
fn url_pattern_shapes_match_bun() {
    let probe = PROBE.replace("const URL_PATTERN = \"skip\";", "const URL_PATTERN = \"only\";");
    main_golden::assert_main_script_matches(&probe, include_str!("golden/builtin_function_shape_url_pattern.expected.txt"));
}
