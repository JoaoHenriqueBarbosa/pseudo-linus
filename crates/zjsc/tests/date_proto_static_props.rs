//! `datePrototypeTable` (44 funções) reifica no primeiro acesso. Ordens medidas no bun 1.4.2: antes e depois
//! de acessar a lista é a mesma (a tabela primeiro, depois `toUTCString`, `toGMTString`, `toTemporalInstant`,
//! `constructor` e `[Symbol.toPrimitive]`); `delete` de um nome da tabela reifica tudo e a ordem passa a ser a
//! da `Structure` (eager, os já acessados, depois a tabela); `delete` fora da tabela não reifica.
use zjsc::api::eval::evaluate_script;
use zjsc::wtf::text::conversion_mode::ConversionMode;

fn run(program: &str) -> String {
    let value = evaluate_script(program).unwrap_or_else(|_| panic!("o programa lançou exceção"));
    assert!(value.is_string(), "o programa não devolveu string");
    String::from_utf8_lossy(&value.as_js_string().value().utf8(ConversionMode::LenientConversion)).into_owned()
}

const KEYS: &str = "var k = function () { return Reflect.ownKeys(Date.prototype).map(String).join(','); };";

const TABLE: &str = "toString,toISOString,toDateString,toTimeString,toLocaleString,toLocaleDateString,toLocaleTimeString,valueOf,getTime,\
getFullYear,getUTCFullYear,getMonth,getUTCMonth,getDate,getUTCDate,getDay,getUTCDay,getHours,getUTCHours,getMinutes,getUTCMinutes,\
getSeconds,getUTCSeconds,getMilliseconds,getUTCMilliseconds,getTimezoneOffset,getYear,setTime,setMilliseconds,setUTCMilliseconds,\
setSeconds,setUTCSeconds,setMinutes,setUTCMinutes,setHours,setUTCHours,setDate,setUTCDate,setMonth,setUTCMonth,setFullYear,\
setUTCFullYear,setYear,toJSON";

const EAGER: &str = "toUTCString,toGMTString,toTemporalInstant,constructor";

fn all_keys() -> String {
    format!("{TABLE},{EAGER},Symbol(Symbol.toPrimitive)")
}

#[test]
fn own_keys_before_access() {
    assert_eq!(run(&format!("{KEYS} k()")), all_keys());
}

#[test]
fn own_keys_after_access() {
    let program = format!("{KEYS} var a = [Date.prototype.getTime, Date.prototype.setYear, Date.prototype.toJSON, Date.prototype.toUTCString]; k()");
    assert_eq!(run(&program), all_keys());
}

#[test]
fn delete_in_table_reifies_all() {
    let program = format!(
        "{KEYS} var a = [Date.prototype.getTime, Date.prototype.setYear, Date.prototype.toJSON]; \
         var r = delete Date.prototype.toString; r + '|' + k()"
    );
    let rest = TABLE.strip_prefix("toString,").unwrap().replace("getTime,", "").replace("setYear,", "").replace(",toJSON", "");
    assert_eq!(
        run(&program),
        format!("true|{EAGER},getTime,setYear,toJSON,{rest},Symbol(Symbol.toPrimitive)")
    );
}

#[test]
fn accessed_order_then_delete() {
    let program = format!("{KEYS} var a = Date.prototype.setSeconds, b = Date.prototype.valueOf; var r = delete Date.prototype.getYear; r + '|' + k()");
    let rest = TABLE.replace("setSeconds,", "").replace("valueOf,", "").replace("getYear,", "");
    assert_eq!(
        run(&program),
        format!("true|{EAGER},setSeconds,valueOf,{rest},Symbol(Symbol.toPrimitive)")
    );
}

#[test]
fn delete_outside_table_does_not_reify() {
    let program = format!("{KEYS} var r = delete Date.prototype.toUTCString; r + '|' + k()");
    assert_eq!(
        run(&program),
        format!("true|{TABLE},toGMTString,toTemporalInstant,constructor,Symbol(Symbol.toPrimitive)")
    );
}

#[test]
fn descriptors_match_table_attributes() {
    let program = "var f = function (n) { var d = Object.getOwnPropertyDescriptor(Date.prototype, n); \
                   return [typeof d.value, d.writable, d.enumerable, d.configurable, d.value.length, d.value.name].join(','); }; \
                   ['toISOString', 'getTime', 'valueOf', 'setHours', 'toJSON', 'getMilliseconds'].map(f).join('|')";
    assert_eq!(
        run(program),
        "function,true,false,true,0,toISOString|function,true,false,true,0,getTime|function,true,false,true,0,valueOf|\
function,true,false,true,4,setHours|function,true,false,true,1,toJSON|function,true,false,true,0,getMilliseconds"
    );
}
