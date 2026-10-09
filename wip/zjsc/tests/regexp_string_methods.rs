//! Os métodos de `RegExp` (`exec`, `Symbol.replace/split/match/matchAll/search`, `flags`, `lastIndex`, as
//! estáticas legadas `RegExp.$1`...) e de `String` (`replaceAll`, `at`, `isWellFormed`, `localeCompare`,
//! `normalize`, `raw`, `split`) ponta a ponta. Os valores esperados são os da especificação e do
//! `RegExpPrototype.cpp`/`StringPrototype.cpp`/`StringConstructor.cpp` do JSC; os casos de "a exceção
//! interrompe antes da próxima conversão" fixam a ordem do `RETURN_IF_EXCEPTION` do C++.
use zjsc::api::eval::evaluate_script;

/// Roda um programa e devolve o valor de conclusão como booleano.
fn is_true(source: &str) -> bool {
    let value = evaluate_script(source).unwrap_or_else(|_| panic!("lançou exceção: {source}"));
    value.is_true()
}

#[test]
fn exec_to_string_that_throws_stops_before_matching() {
    assert!(is_true("var r = /a/g; r.lastIndex = 1; try { r.exec({ toString() { throw 1 } }) } catch (e) {} r.lastIndex === 1"));
    assert!(is_true("var t; try { /a/.exec({ toString() { throw 5 } }) } catch (e) { t = e } t === 5"));
}

#[test]
fn last_index_conversion_that_throws_stops_exec() {
    assert!(is_true("var r = /a/g; r.lastIndex = { valueOf() { throw 7 } }; var t; try { r.exec('a') } catch (e) { t = e } t === 7"));
    assert!(is_true("var r = /a/y; r.lastIndex = { valueOf() { throw 7 } }; var t; try { r.test('a') } catch (e) { t = e } t === 7"));
}

#[test]
fn last_index_semantics() {
    assert!(is_true("var r = /a/g; r.exec('aa'); r.lastIndex === 1"));
    assert!(is_true("var r = /a/g; r.exec('aa'); r.exec('aa'); r.exec('aa') === null && r.lastIndex === 0"));
    assert!(is_true("var s = /a/y; s.test('ba') === false && s.lastIndex === 0"));
    assert!(is_true("var s = /a/y; s.lastIndex = 1; s.test('ba') === true && s.lastIndex === 2"));
    assert!(is_true("var r = /a/; r.lastIndex = 5; r.exec('a') !== null && r.lastIndex === 5"));
    assert!(is_true("var r = /a/g; r.lastIndex = 9; r.exec('a') === null && r.lastIndex === 0"));
}

#[test]
fn non_writable_last_index_throws_before_recording_legacy_statics() {
    assert!(is_true(
        "/(z)/.exec('z'); var r = /(q)/g; Object.defineProperty(r, 'lastIndex', { writable: false }); \
         var threw = false; try { r.exec('q') } catch (e) { threw = e instanceof TypeError } \
         threw && RegExp.$1 === 'z'"
    ));
}

#[test]
fn legacy_statics() {
    assert!(is_true(
        "/(a)(b)/.exec('xaby'); RegExp.$1 === 'a' && RegExp.$2 === 'b' && RegExp.lastMatch === 'ab' \
         && RegExp['$&'] === 'ab' && RegExp.leftContext === 'x' && RegExp['$`'] === 'x' \
         && RegExp.rightContext === 'y' && RegExp[\"$'\"] === 'y' && RegExp.lastParen === 'b' \
         && RegExp['$+'] === 'b' && RegExp.input === 'xaby' && RegExp.$_ === 'xaby' && RegExp.$3 === ''"
    ));
    assert!(is_true("RegExp.input = 'abc'; RegExp.input === 'abc' && RegExp.$_ === 'abc'"));
    assert!(is_true("RegExp.multiline = 1; RegExp.multiline === true && RegExp['$*'] === true"));
}

#[test]
fn flags_getter_honors_redefined_accessors() {
    assert!(is_true("/a/dgimsyu.flags === 'dgimsuy'"));
    assert!(is_true("/a/v.flags === 'v'"));
    assert!(is_true(
        "var re = /a/; var d = Object.getOwnPropertyDescriptor(RegExp.prototype, 'global'); \
         Object.defineProperty(RegExp.prototype, 'global', { get() { return true }, configurable: true }); \
         var f = re.flags; Object.defineProperty(RegExp.prototype, 'global', d); f === 'g'"
    ));
    assert!(is_true("var o = { hasIndices: 1, global: 1, sticky: 1 }; Object.getOwnPropertyDescriptor(RegExp.prototype, 'flags').get.call(o) === 'dgy'"));
    assert!(is_true("var t; try { Object.getOwnPropertyDescriptor(RegExp.prototype, 'flags').get.call(1) } catch (e) { t = e } t instanceof TypeError"));
}

#[test]
fn prototype_getters_and_source() {
    assert!(is_true("RegExp.prototype.source === '(?:)' && RegExp.prototype.flags === '' && RegExp.prototype.global === undefined"));
    assert!(is_true("/a\\/b/.source === 'a\\\\/b' && new RegExp('/').source === '\\\\/' && new RegExp('\\n').source === '\\\\n'"));
    assert!(is_true("new RegExp('').source === '(?:)' && new RegExp('[/]').source === '[/]'"));
    assert!(is_true("var t; try { Object.getOwnPropertyDescriptor(RegExp.prototype, 'global').get.call({}) } catch (e) { t = e } t instanceof TypeError"));
    assert!(is_true("String(/a/g) === '/a/g' && RegExp.prototype.toString.call({ source: 'x', flags: 'y' }) === '/x/y'"));
}

#[test]
fn constructor_converts_pattern_before_flags() {
    assert!(is_true("var calls = 0; try { new RegExp({ toString() { throw 1 } }, { toString() { calls++; return '' } }) } catch (e) {} calls === 0"));
    assert!(is_true("var t; try { new RegExp('a', 'gg') } catch (e) { t = e } t instanceof SyntaxError"));
    assert!(is_true("var t; try { RegExp(Symbol()) } catch (e) { t = e } t instanceof TypeError"));
    assert!(is_true("var r = /a/g; RegExp(r) === r && new RegExp(r) !== r && new RegExp(r).flags === 'g' && new RegExp(r, 'i').flags === 'i'"));
}

#[test]
fn compile() {
    assert!(is_true("var r = /a/g; r.lastIndex = 3; r.compile('b', 'i') === r && r.source === 'b' && r.flags === 'i' && r.lastIndex === 0"));
    assert!(is_true("var r = /a/g; r.compile(/c/y); r.source === 'c' && r.flags === 'y'"));
    assert!(is_true("var t; try { /a/.compile(/b/, 'g') } catch (e) { t = e } t instanceof TypeError"));
}

#[test]
fn symbol_replace() {
    assert!(is_true("/b/g[Symbol.replace]('abab', 'X') === 'aXaX'"));
    assert!(is_true("'2020-01'.replace(/(?<y>\\d+)-(?<m>\\d+)/, '$<m>/$<y>') === '01/2020'"));
    assert!(is_true("'abc'.replace(/(b)/, '[$1$2$&]') === 'a[b$2b]c'"));
    assert!(is_true("'abc'.replace(/(b)/, \"[$`|$']\") === 'a[a|c]c'"));
    assert!(is_true("'abc'.replace(/b/, (m, i, s) => m + i + s) === 'ab1abcc'"));
    assert!(is_true("'aaa'.replace(/a*?/g, '-') === '-a-a-a-'"));
    assert!(is_true("'\\ud83d\\ude00'.replace(/(?:)/gu, '-') === '-\\ud83d\\ude00-'"));
    assert!(is_true("var t; try { RegExp.prototype[Symbol.replace].call(1, 'a', 'b') } catch (e) { t = e } t instanceof TypeError"));
}

#[test]
fn symbol_replace_uses_overridden_exec() {
    assert!(is_true(
        "var r = /a/; r.exec = function () { return { 0: 'b', index: 1, length: 1 } }; \
         r[Symbol.replace]('abc', 'X') === 'aXc'"
    ));
    assert!(is_true(
        "var r = /a/; r.exec = function () { return 1 }; var t; try { r[Symbol.replace]('a', 'X') } catch (e) { t = e } t instanceof TypeError"
    ));
}

#[test]
fn symbol_match_and_match_all() {
    assert!(is_true("'a1b2'.match(/\\d/g).join() === '1,2'"));
    assert!(is_true("'abc'.match(/x/g) === null && 'abc'.match(/b/).index === 1"));
    assert!(is_true("[...'a1b2'.matchAll(/\\d/g)].map(m => m[0] + m.index).join() === '11,23'"));
    assert!(is_true("var t; try { 'a'.matchAll(/a/) } catch (e) { t = e } t instanceof TypeError"));
    assert!(is_true("var it = /a/g[Symbol.matchAll]('aa'); var a = it.next(), b = it.next(), c = it.next(); a.value[0] === 'a' && b.value.index === 1 && c.done === true"));
    assert!(is_true("var r = /a/g; r.lastIndex = 1; var it = r[Symbol.matchAll]('aa'); it.next().value.index === 1 && r.lastIndex === 1"));
}

#[test]
fn symbol_search_restores_last_index() {
    assert!(is_true("var r = /b/g; r.lastIndex = 2; r[Symbol.search]('abc') === 1 && r.lastIndex === 2"));
    assert!(is_true("'abc'.search(/c/) === 2 && 'abc'.search(/x/) === -1"));
}

#[test]
fn symbol_split() {
    assert!(is_true("'a1b'.split(/\\d/).join() === 'a,b'"));
    assert!(is_true("'a1b2c'.split(/\\d/, 2).join() === 'a,b'"));
    assert!(is_true("'abc'.split(/(b)/).join() === 'a,b,c'"));
    assert!(is_true("'abc'.split(/(x)?b/).length === 3 && 'abc'.split(/(x)?b/)[1] === undefined"));
    assert!(is_true("''.split(/a/).length === 1 && ''.split(/(?:)/).length === 0"));
    assert!(is_true("'ab'.split(/(?:)/).join() === 'a,b'"));
    assert!(is_true("'\\ud83d\\ude00'.split(/(?:)/u).length === 1 && '\\ud83d\\ude00'.split(/(?:)/).length === 2"));
    assert!(is_true("'a'.split(/a/y).join() === ','"));
}

#[test]
fn string_replace_all() {
    assert!(is_true("'ab'.replaceAll('', '-') === '-a-b-'"));
    assert!(is_true("'aaa'.replaceAll('a', '$&$&') === 'aaaaaa'"));
    assert!(is_true("'aXbX'.replaceAll('X', (m, i) => i) === 'a1b3'"));
    assert!(is_true("'abab'.replaceAll(/b/g, '1') === 'a1a1'"));
    assert!(is_true("'a.b'.replaceAll('.', '$$') === 'a$b'"));
    assert!(is_true("var t; try { 'a'.replaceAll(/a/, 'b') } catch (e) { t = e } t instanceof TypeError"));
    assert!(is_true("var t; try { String.prototype.replaceAll.call(null, 'a', 'b') } catch (e) { t = e } t instanceof TypeError"));
}

#[test]
fn string_at_and_well_formed() {
    assert!(is_true("'abc'.at(-1) === 'c' && 'abc'.at(0) === 'a' && 'abc'.at(3) === undefined && 'abc'.at(-4) === undefined"));
    assert!(is_true("'abc'.at(1.9) === 'b' && 'abc'.at(NaN) === 'a' && 'abc'.at(Infinity) === undefined"));
    assert!(is_true("'abc'.isWellFormed() && !'a\\ud800'.isWellFormed() && '\\ud83d\\ude00'.isWellFormed()"));
    assert!(is_true("'a\\ud800b'.toWellFormed() === 'a\\ufffdb'"));
    assert!(is_true("var n = 0; try { 'abc'.at({ valueOf() { throw 1 } }) } catch (e) { n++ } n === 1"));
}

#[test]
fn string_arguments_stop_at_the_first_exception() {
    assert!(is_true("var n = 0; try { 'abc'.slice({ valueOf() { throw 1 } }, { valueOf() { n++; return 1 } }) } catch (e) {} n === 0"));
    assert!(is_true("var n = 0; try { 'abc'.substring({ valueOf() { throw 1 } }, { valueOf() { n++; return 1 } }) } catch (e) {} n === 0"));
    assert!(is_true("var n = 0; try { 'abc'.indexOf({ toString() { throw 1 } }, { valueOf() { n++; return 1 } }) } catch (e) {} n === 0"));
    assert!(is_true("var n = 0; try { 'abc'.padStart({ valueOf() { throw 1 } }, { toString() { n++; return 'x' } }) } catch (e) {} n === 0"));
    assert!(is_true("var n = 0; try { 'abc'.includes('a', { valueOf() { throw 1 } }) } catch (e) { n++ } n === 1"));
    assert!(is_true("var n = 0; var o = { toString() { throw 1 } }; try { String.prototype.at.call(o, { valueOf() { n++; return 0 } }) } catch (e) {} n === 0"));
    assert!(is_true("var t; try { 'a'.startsWith(/a/) } catch (e) { t = e } t instanceof TypeError"));
}

#[test]
fn string_object_receiver_uses_user_to_string() {
    assert!(is_true("var s = new String('abc'); s.toString = function () { return 'xyz' }; String.prototype.at.call(s, 0) === 'x'"));
}

#[test]
fn string_locale_compare_and_normalize() {
    assert!(is_true("'x'.localeCompare('x') === 0 && 'a'.localeCompare('b') < 0 && 'b'.localeCompare('a') > 0"));
    assert!(is_true("var t; try { String.prototype.localeCompare.call(undefined, 'a') } catch (e) { t = e } t instanceof TypeError"));
    assert!(is_true("'\\u00e9'.normalize('NFD') === 'e\\u0301' && 'e\\u0301'.normalize() === '\\u00e9'"));
    assert!(is_true("var t; try { 'a'.normalize('nfc') } catch (e) { t = e } t instanceof RangeError"));
}

#[test]
fn string_split() {
    assert!(is_true("'a-b-c'.split('-', 2).join() === 'a,b'"));
    assert!(is_true("'abc'.split('').length === 3 && 'abc'.split(undefined).length === 1 && 'abc'.split(undefined, 0).length === 0"));
    assert!(is_true("''.split('').length === 0 && ''.split('a').length === 1"));
    assert!(is_true("'a,b'.split(',', undefined).length === 2 && 'a,b'.split(',', -1).length === 2 && 'a,b'.split(',', 4294967297).length === 1"));
    assert!(is_true("'abc'.split('abc').join('|') === '|'"));
    assert!(is_true("var o = { [Symbol.split](s, l) { return [s, l] } }; 'x'.split(o, 3).join() === 'x,3'"));
}

#[test]
fn string_raw() {
    assert!(is_true("String.raw({ raw: ['a', 'b', 'c'] }, 1, 2) === 'a1b2c'"));
    assert!(is_true("String.raw({ raw: { length: 0 } }) === ''"));
    assert!(is_true("String.raw({ raw: { length: 2, 0: 'x', 1: 'y' } }, 'S', 'T') === 'xSy'"));
    assert!(is_true("String.raw({ raw: ['a', 'b'] }) === 'ab' && String.raw({ raw: ['a'] }, 9) === 'a'"));
    assert!(is_true("function tag(s, ...v) { return String.raw(s, ...v) } tag`a${1}b${2}c\\n` === 'a1b2c\\\\n'"));
    assert!(is_true("var t; try { String.raw() } catch (e) { t = e } t instanceof TypeError"));
    assert!(is_true("var t; try { String.raw({}) } catch (e) { t = e } t instanceof TypeError"));
    // A substituição só é convertida se há um segmento depois dela.
    assert!(is_true("var n = 0; String.raw({ raw: ['a'] }, { toString() { n++; return 'z' } }); n === 0"));
    // A ordem do C++: segmento 0, substituição 0, segmento 1.
    assert!(is_true(
        "var log = []; String.raw({ raw: { length: 2, get 0() { log.push('s0'); return 'a' }, get 1() { log.push('s1'); return 'b' } } }, \
         { toString() { log.push('sub'); return '-' } }) === 'a-b' && log.join() === 's0,sub,s1'"
    ));
}

#[test]
fn string_from_code_point_and_char_code_stop_at_the_first_problem() {
    assert!(is_true("var called = false; try { String.fromCodePoint(-1, { valueOf() { called = true; return 1 } }) } catch (e) {} !called"));
    assert!(is_true("var t; try { String.fromCodePoint(1114112) } catch (e) { t = e } t instanceof RangeError"));
    assert!(is_true("String.fromCodePoint(0x1F600, 65) === '\\ud83d\\ude00A' && String.fromCodePoint() === ''"));
    assert!(is_true("var n = 0; try { String.fromCharCode({ valueOf() { n++; throw 1 } }, { valueOf() { n++; return 1 } }) } catch (e) {} n === 1"));
    assert!(is_true("String.fromCharCode(65, 0x10042) === 'AB'"));
}
