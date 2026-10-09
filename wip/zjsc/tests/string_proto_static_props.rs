//! `stringPrototypeTable` (os 13 métodos HTML, `anchor` a `sup`) reifica no primeiro acesso. Ordens medidas no
//! bun 1.4.2: antes e depois de acessar a lista é a mesma (`length`, os nomes da tabela, depois o que o
//! `finishCreation` põe); `delete` de um nome da tabela reifica tudo e a ordem passa a ser a da `Structure`
//! (o que já estava nela, depois a tabela, depois `Symbol.iterator`); `delete` fora da tabela não reifica.
use zjsc::api::eval::evaluate_script;
use zjsc::wtf::text::conversion_mode::ConversionMode;

fn run(program: &str) -> String {
    let value = evaluate_script(program).unwrap_or_else(|_| panic!("o programa lançou exceção"));
    assert!(value.is_string(), "o programa não devolveu string");
    String::from_utf8_lossy(&value.as_js_string().value().utf8(ConversionMode::LenientConversion)).into_owned()
}

const KEYS: &str = "var k = function () { return Reflect.ownKeys(String.prototype).map(String).join(','); };";

const HTML: &str = "anchor,big,bold,blink,fixed,fontcolor,fontsize,italics,link,small,strike,sub,sup";
const HEAD: &str = "toString,valueOf,charAt,charCodeAt,codePointAt,concat,indexOf,lastIndexOf,replace,replaceAll,repeat,padStart,padEnd,\
slice,substr,at,substring,toLowerCase,toUpperCase,localeCompare,toLocaleLowerCase,toLocaleUpperCase,trim,startsWith,endsWith,includes,\
match,search,matchAll,split,normalize,trimStart,trimLeft,trimEnd,trimRight,isWellFormed,toWellFormed,constructor";

fn all_keys() -> String {
    format!("length,{HTML},{HEAD},Symbol(Symbol.iterator)")
}

#[test]
fn own_keys_before_access() {
    assert_eq!(run(&format!("{KEYS} k()")), all_keys());
}

#[test]
fn own_keys_after_access() {
    let program = format!("{KEYS} var a = [String.prototype.anchor, String.prototype.sup, String.prototype.charAt]; k()");
    assert_eq!(run(&program), all_keys());
}

#[test]
fn delete_in_table_reifies_all() {
    let program = format!("{KEYS} var r = delete String.prototype.big; r + '|' + k()");
    assert_eq!(
        run(&program),
        "true|length,toString,valueOf,charAt,charCodeAt,codePointAt,concat,indexOf,lastIndexOf,replace,replaceAll,repeat,padStart,padEnd,\
slice,substr,at,substring,toLowerCase,toUpperCase,localeCompare,toLocaleLowerCase,toLocaleUpperCase,trim,startsWith,endsWith,includes,\
match,search,matchAll,split,normalize,trimStart,trimLeft,trimEnd,trimRight,isWellFormed,toWellFormed,constructor,\
anchor,bold,blink,fixed,fontcolor,fontsize,italics,link,small,strike,sub,sup,Symbol(Symbol.iterator)"
    );
}

#[test]
fn accessed_then_delete() {
    let program = format!("{KEYS} var a = String.prototype.link; var r = delete String.prototype.sub; r + '|' + k()");
    assert_eq!(
        run(&program),
        "true|length,toString,valueOf,charAt,charCodeAt,codePointAt,concat,indexOf,lastIndexOf,replace,replaceAll,repeat,padStart,padEnd,\
slice,substr,at,substring,toLowerCase,toUpperCase,localeCompare,toLocaleLowerCase,toLocaleUpperCase,trim,startsWith,endsWith,includes,\
match,search,matchAll,split,normalize,trimStart,trimLeft,trimEnd,trimRight,isWellFormed,toWellFormed,constructor,\
link,anchor,big,bold,blink,fixed,fontcolor,fontsize,italics,small,strike,sup,Symbol(Symbol.iterator)"
    );
}

#[test]
fn delete_outside_table_does_not_reify() {
    let program = format!("{KEYS} var r = delete String.prototype.trim; r + '|' + k()");
    assert_eq!(
        run(&program),
        format!("true|length,{HTML},{}", all_keys().replace(&format!("length,{HTML},"), "").replace("trim,", ""))
    );
}

#[test]
fn descriptors_match_table_attributes() {
    let program = "var f = function (n) { var d = Object.getOwnPropertyDescriptor(String.prototype, n); \
                   return [typeof d.value, d.writable, d.enumerable, d.configurable, d.value.length, d.value.name].join(','); }; \
                   [f('anchor'), f('fontcolor'), f('big'), f('sup')].join('|')";
    assert_eq!(
        run(program),
        "function,true,false,true,1,anchor|function,true,false,true,1,fontcolor|function,true,false,true,0,big|function,true,false,true,0,sup"
    );
}

#[test]
fn html_methods_still_work() {
    assert_eq!(run("'x'.anchor('a\"b') + '|' + 'x'.sup()"), "<a name=\"a&quot;b\">x</a>|<sup>x</sup>");
}
