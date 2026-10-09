//! As 21 entradas de `regExpConstructorTable` (`input`/`$_`, `multiline`/`$*`, `lastMatch`/`$&`, `lastParen`/`$+`,
//! `leftContext`/`` $` ``, `rightContext`/`$'`, `$1` a `$9`) são `CustomAccessor` e não nascem na `Structure`.
//! Ordens medidas no bun 1.4.2: antes e depois de ler, escrever ou chamar o setter a lista é a mesma (tabela, depois
//! `length,name,prototype,escape` e o `@@species`); ler um acessor não o reifica; `delete` de um nome da tabela
//! reifica tudo e a ordem passa a ser a da `Structure` (`length,name,prototype,escape` primeiro e o resto da
//! tabela na ordem do `@begin`); `Object.defineProperty` reifica só aquele nome, que vai para depois de `escape`;
//! `delete RegExp.escape` (fora da tabela) não reifica.
use zjsc::api::eval::evaluate_script;
use zjsc::wtf::text::conversion_mode::ConversionMode;

fn run(program: &str) -> String {
    let value = evaluate_script(program).unwrap_or_else(|_| panic!("o programa lançou exceção"));
    assert!(value.is_string(), "o programa não devolveu string");
    String::from_utf8_lossy(&value.as_js_string().value().utf8(ConversionMode::LenientConversion)).into_owned()
}

const KEYS: &str = "var k = function () { return Reflect.ownKeys(RegExp).map(String).join(','); };";

const TABLE: &str = "input,$_,multiline,$*,lastMatch,$&,lastParen,$+,leftContext,$`,rightContext,$',$1,$2,$3,$4,$5,$6,$7,$8,$9";
const TAIL: &str = "length,name,prototype,escape,Symbol(Symbol.species)";

#[test]
fn own_keys_before_access() {
    assert_eq!(run(&format!("{KEYS} k()")), format!("{TABLE},{TAIL}"));
}

#[test]
fn own_keys_after_reads() {
    let program = format!("{KEYS} var a = [RegExp.input, RegExp.$1, RegExp.lastMatch, typeof RegExp.escape]; k()");
    assert_eq!(run(&program), format!("{TABLE},{TAIL}"));
}

#[test]
fn own_keys_after_writes() {
    let program = format!(
        "{KEYS} var a = [Reflect.set(RegExp, '$1', 'q'), Reflect.set(RegExp, 'multiline', true), RegExp.multiline, \
         Reflect.set(RegExp, '$_', 'abc'), RegExp.input]; a.join('|') + '|' + k()"
    );
    assert_eq!(run(&program), format!("false|true|true|true|abc|{TABLE},{TAIL}"));
}

#[test]
fn legacy_values_after_exec() {
    let program = "/(a)(b)/.exec('xaby'); \
                   [RegExp.$1, RegExp.$2, RegExp.lastMatch, RegExp['$&'], RegExp.leftContext, RegExp['$`'], \
                   RegExp.rightContext, RegExp[\"$'\"], RegExp.lastParen, RegExp['$+'], RegExp.input].join(',')";
    assert_eq!(run(program), "a,b,ab,ab,x,x,y,y,b,b,xaby");
}

#[test]
fn descriptor_matches_table_attributes() {
    let program = "var d = Object.getOwnPropertyDescriptor(RegExp, '$1'); \
                   var i = Object.getOwnPropertyDescriptor(RegExp, 'input'); \
                   [typeof d.get, typeof d.set, d.enumerable, d.configurable, typeof i.get, typeof i.set, i.enumerable, \
                   i.configurable].join(',')";
    assert_eq!(run(program), "function,undefined,false,true,function,function,false,true");
}

#[test]
fn has_property_counts_the_table() {
    let program = "[Object.keys(RegExp).length, 'multiline' in RegExp, RegExp.hasOwnProperty('$9'), \
                   Object.getOwnPropertyNames(RegExp).length].join(',')";
    assert_eq!(run(program), "0,true,true,25");
}

#[test]
fn own_keys_after_delete_last_match() {
    let program = format!("{KEYS} var a = [RegExp.input, RegExp.$1]; var r = delete RegExp.lastMatch; r + '|' + k() + '|' + typeof RegExp.lastMatch");
    assert_eq!(
        run(&program),
        "true|length,name,prototype,escape,input,$_,multiline,$*,$&,lastParen,$+,leftContext,$`,rightContext,$',\
         $1,$2,$3,$4,$5,$6,$7,$8,$9,Symbol(Symbol.species)|undefined"
    );
}

#[test]
fn own_keys_after_define_property() {
    let program = format!(
        "{KEYS} Object.defineProperty(RegExp, '$5', {{ value: 1, configurable: true }}); var r = delete RegExp.lastMatch; \
         r + '|' + k()"
    );
    assert_eq!(
        run(&program),
        "true|length,name,prototype,escape,$5,input,$_,multiline,$*,$&,lastParen,$+,leftContext,$`,rightContext,$',\
         $1,$2,$3,$4,$6,$7,$8,$9,Symbol(Symbol.species)"
    );
}

#[test]
fn object_assign_reifies_everything() {
    let program = format!("{KEYS} Object.assign({{}}, RegExp); k()");
    assert_eq!(run(&program), format!("length,name,prototype,escape,{TABLE},Symbol(Symbol.species)"));
}

#[test]
fn delete_outside_the_table_does_not_reify() {
    let program = format!("{KEYS} var r = delete RegExp.escape; r + '|' + k()");
    assert_eq!(run(&program), format!("true|{TABLE},length,name,prototype,Symbol(Symbol.species)"));
}
