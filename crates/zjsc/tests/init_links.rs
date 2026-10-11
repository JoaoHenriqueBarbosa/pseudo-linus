//! As ligações do `JSGlobalObject::init` que o PLAN.md listava como pendentes (Proxy, getters de `RegExp.prototype`,
//! `%AsyncGeneratorPrototype%` e `Array.prototype[Symbol.unscopables]`), conferidas no observável medido no bun 1.4.2.
//! O resultado de cada programa é uma linha de texto na global `R`.

mod common;

/// (programa, resultado do bun 1.4.2).
const CASES: &[(&str, &str)] = &[
    (
        "globalThis.R=Object.getOwnPropertyNames(Array.prototype[Symbol.unscopables]).join()+'|'+Object.getPrototypeOf(Array.prototype[Symbol.unscopables])+'|'+Object.keys(Array.prototype[Symbol.unscopables]).length",
        "at,copyWithin,entries,fill,find,findIndex,findLast,findLastIndex,flat,flatMap,includes,keys,toReversed,toSorted,toSpliced,values|null|16",
    ),
    (
        "globalThis.R=(()=>{var d=Object.getOwnPropertyDescriptor(Array.prototype,Symbol.unscopables);return d.writable+'|'+d.enumerable+'|'+d.configurable})()",
        "false|false|true",
    ),
    (
        "globalThis.R=Object.getOwnPropertyNames(RegExp.prototype).filter(k=>typeof Object.getOwnPropertyDescriptor(RegExp.prototype,k).get==='function').join()",
        "global,dotAll,hasIndices,ignoreCase,multiline,sticky,unicode,unicodeSets,source,flags",
    ),
    (
        "globalThis.R=(()=>{var d=Object.getOwnPropertyDescriptor(RegExp.prototype,'flags');return typeof d.get+'|'+d.set+'|'+d.enumerable+'|'+d.configurable})()",
        "function|undefined|false|true",
    ),
    (
        "globalThis.R=JSON.stringify(RegExp.prototype.flags)+'|'+String(RegExp.prototype.global)+'|'+RegExp.prototype.source+'|'+/a/gi.flags",
        "\"\"|undefined|(?:)|gi",
    ),
    (
        "globalThis.R=(()=>{var p=Object.getPrototypeOf(async function*(){}).prototype;return Object.getOwnPropertyNames(p).join()+'|'+p[Symbol.toStringTag]+'|'+(Object.getPrototypeOf(Object.getPrototypeOf(p))===Object.prototype)})()",
        "return,throw,next,constructor|AsyncGenerator|true",
    ),
    (
        "globalThis.R=typeof Proxy+'|'+Proxy.length+'|'+Object.getOwnPropertyNames(Proxy).join()+'|'+String(Proxy.prototype)+'|'+Object.getOwnPropertyDescriptor(globalThis,'Proxy').enumerable+'|'+new Proxy({a:1},{}).a+'|'+typeof Proxy.revocable({},{}).revoke",
        "function|2|length,name,revocable|undefined|false|1|function",
    ),
];

#[test]
fn init_links_match_bun() {
    for (program, expected) in CASES {
        let actual = common::guarded(|| common::EvalMode::IndirectEval.evaluate(*program, "init_links_case.js", "R"));
        assert_eq!(actual.as_deref(), Ok(*expected), "programa: {program}");
    }
}
