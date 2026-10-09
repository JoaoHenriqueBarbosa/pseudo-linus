//! Conformidade de `Proxy`, `Reflect`, `JSON`, `Symbol` e `BigInt` com o C++ (`ProxyObject.cpp`,
//! `ProxyConstructor.cpp`, `ReflectObject.cpp`, `JSONObject.cpp`, `SymbolConstructor.cpp`,
//! `SymbolPrototype.cpp`, `BigIntConstructor.cpp`, `BigIntPrototype.cpp`).
//!
//! Cada teste roda um programa que devolve várias linhas separadas por `|||` e compara linha a linha
//! com o texto que o JavaScriptCore produz: as mensagens de erro são as literais do C++ e a ordem de
//! leitura é a do algoritmo da especificação que o C++ implementa.
use zjsc::api::eval::evaluate_script;
use zjsc::wtf::text::conversion_mode::ConversionMode;

/// O texto comum dos programas: `msg(f)` devolve o resultado de `f` ou `Nome: mensagem` do erro.
const PRELUDE: &str = r#"
  function msg(f) {
    try { return String(f()); } catch (e) { return e.constructor.name + ": " + e.message; }
  }
  var out = [];
"#;

/// Roda `body` (que usa `msg` e `out`) e devolve as linhas que ele acumulou em `out`.
fn run_lines(body: &str) -> Vec<String> {
    let program = format!("(function () {{ {PRELUDE} {body} return out.join(\"|||\"); }})()");
    match evaluate_script(&program) {
        Ok(value) if value.is_string() => {
            let bytes = value.as_js_string().value().utf8(ConversionMode::LenientConversion);
            String::from_utf8_lossy(&bytes).split("|||").map(str::to_string).collect()
        }
        Ok(_) => panic!("o programa não devolveu string"),
        Err(_) => panic!("o programa lançou exceção"),
    }
}

/// Compara as linhas do programa com as esperadas, uma a uma, para o erro apontar a divergente.
fn check(body: &str, expected: &[&str]) {
    let actual = run_lines(body);
    assert_eq!(actual.len(), expected.len(), "quantidade de linhas: {actual:#?}");
    for (index, (actual, expected)) in actual.iter().zip(expected).enumerate() {
        assert_eq!(actual, expected, "linha {index}");
    }
}

fn not_callable(trap: &str) -> String {
    format!("TypeError: '{trap}' property of a Proxy's handler should be callable")
}

#[test]
fn proxy_trap_that_is_a_non_callable_primitive_throws() {
    // `JSObject::getMethod` (e `getHandlerTrap`) lança para qualquer valor que não é `undefined`, `null`
    // nem chamável: número, booleano e string também.
    let expected = [
        not_callable("getPrototypeOf"),
        not_callable("deleteProperty"),
        not_callable("isExtensible"),
        not_callable("preventExtensions"),
        not_callable("defineProperty"),
        not_callable("setPrototypeOf"),
        not_callable("get"),
        not_callable("apply"),
        not_callable("construct"),
    ];
    let expected: Vec<&str> = expected.iter().map(String::as_str).collect();
    check(
        r#"
        out.push(msg(function () { return Object.getPrototypeOf(new Proxy({}, { getPrototypeOf: 1 })); }));
        out.push(msg(function () { return Reflect.deleteProperty(new Proxy({}, { deleteProperty: true }), "a"); }));
        out.push(msg(function () { return Reflect.isExtensible(new Proxy({}, { isExtensible: "x" })); }));
        out.push(msg(function () { return Reflect.preventExtensions(new Proxy({}, { preventExtensions: 0 })); }));
        out.push(msg(function () { return Reflect.defineProperty(new Proxy({}, { defineProperty: 1 }), "a", {}); }));
        out.push(msg(function () { return Reflect.setPrototypeOf(new Proxy({}, { setPrototypeOf: 1 }), null); }));
        out.push(msg(function () { return Reflect.get(new Proxy({}, { get: 1 }), "a"); }));
        out.push(msg(function () { return (new Proxy(function () {}, { apply: 1 }))(); }));
        out.push(msg(function () { return new (new Proxy(function () {}, { construct: 1 }))(); }));
        "#,
        &expected,
    );
}

#[test]
fn proxy_invariants_on_a_non_configurable_non_writable_target_property() {
    check(
        r#"
        var target = {};
        Object.defineProperty(target, "x", { value: 1, writable: false, configurable: false });
        out.push(msg(function () { return Reflect.get(new Proxy(target, { get() { return 2; } }), "x"); }));
        out.push(msg(function () { return Reflect.get(new Proxy(target, { get() { return 1; } }), "x"); }));
        out.push(msg(function () { return Reflect.has(new Proxy(target, { has() { return false; } }), "x"); }));
        out.push(msg(function () { return Reflect.deleteProperty(new Proxy(target, { deleteProperty() { return true; } }), "x"); }));
        out.push(msg(function () { return Reflect.ownKeys(new Proxy(target, { ownKeys() { return []; } })); }));
        out.push(msg(function () { return Reflect.set(new Proxy(target, { set() { return true; } }), "x", 2); }));
        out.push(msg(function () { return Reflect.set(new Proxy(target, { set() { return false; } }), "x", 2); }));
        "#,
        &[
            "TypeError: Proxy handler's 'get' result of a non-configurable and non-writable property should be the same value as the target's property",
            "1",
            "TypeError: Proxy 'has' must return 'true' for non-configurable properties",
            "TypeError: Proxy handler's 'deleteProperty' method should return false when the target's property is not configurable",
            "TypeError: Proxy object's 'target' has the non-configurable property 'x' that was not in the result from the 'ownKeys' trap",
            "TypeError: Proxy handler's 'set' on a non-configurable and non-writable property on 'target' should either return false or be the same value already on the 'target'",
            "false",
        ],
    );
}

#[test]
fn proxy_own_keys_result_is_validated() {
    check(
        r#"
        out.push(msg(function () { return Reflect.ownKeys(new Proxy({}, { ownKeys() { return ["a", "a"]; } })); }));
        out.push(msg(function () { return Reflect.ownKeys(new Proxy({}, { ownKeys() { return 1; } })); }));
        out.push(msg(function () { return Reflect.ownKeys(new Proxy({}, { ownKeys() { return [1]; } })); }));
        var keys = Reflect.ownKeys(new Proxy({}, { ownKeys() { return ["b", "a", Symbol.iterator]; } }));
        out.push(keys[0] + keys[1] + String(keys[2]) + keys.length);
        var frozen = Object.preventExtensions({ a: 1 });
        out.push(msg(function () { return Reflect.ownKeys(new Proxy(frozen, { ownKeys() { return ["a", "b"]; } })); }));
        out.push(msg(function () { return Reflect.ownKeys(new Proxy(frozen, { ownKeys() { return []; } })); }));
        "#,
        &[
            "TypeError: Proxy handler's 'ownKeys' trap result must not contain any duplicate names",
            "TypeError: Proxy handler's 'ownKeys' method must return an object",
            "TypeError: Proxy handler's 'ownKeys' method must return an array-like object containing only Strings and Symbols",
            "baSymbol(Symbol.iterator)3",
            "TypeError: Proxy handler's 'ownKeys' method returned a key that was not present in its non-extensible target",
            "TypeError: Proxy object's non-extensible 'target' has configurable property 'a' that was not in the result from the 'ownKeys' trap",
        ],
    );
}

#[test]
fn proxy_extensibility_and_prototype_traps_are_validated() {
    check(
        r#"
        out.push(msg(function () { return Reflect.isExtensible(new Proxy({}, { isExtensible() { return false; } })); }));
        out.push(msg(function () { return Reflect.isExtensible(new Proxy(Object.preventExtensions({}), { isExtensible() { return true; } })); }));
        out.push(msg(function () { return Reflect.preventExtensions(new Proxy({}, { preventExtensions() { return true; } })); }));
        out.push(msg(function () { return Reflect.getPrototypeOf(new Proxy({}, { getPrototypeOf() { return 1; } })); }));
        out.push(msg(function () { return Reflect.getPrototypeOf(new Proxy(Object.preventExtensions({}), { getPrototypeOf() { return null; } })); }));
        out.push(msg(function () { return Reflect.setPrototypeOf(new Proxy({}, { setPrototypeOf() { return false; } }), null); }));
        out.push(msg(function () { return Object.setPrototypeOf(new Proxy({}, { setPrototypeOf() { return false; } }), null); }));
        out.push(msg(function () { return Reflect.setPrototypeOf(new Proxy(Object.preventExtensions({}), { setPrototypeOf() { return true; } }), null); }));
        "#,
        &[
            "TypeError: Proxy object's 'isExtensible' trap returned false when the target is extensible. It should have returned true",
            "TypeError: Proxy object's 'isExtensible' trap returned true when the target is non-extensible. It should have returned false",
            "TypeError: Proxy's 'preventExtensions' trap returned true even though its target is extensible. It should have returned false",
            "TypeError: Proxy handler's 'getPrototypeOf' trap should either return an object or null",
            "TypeError: Proxy's 'getPrototypeOf' trap for a non-extensible target should return the same value as the target's prototype",
            "false",
            "TypeError: Proxy 'setPrototypeOf' returned false indicating it could not set the prototype value. The operation was expected to succeed",
            "TypeError: Proxy 'setPrototypeOf' trap returned true when its target is non-extensible and the new prototype value is not the same as the current prototype value. It should have returned false",
        ],
    );
}

#[test]
fn proxy_define_property_trap_is_validated() {
    check(
        r#"
        var falsy = { defineProperty() { return false; } };
        out.push(msg(function () { return Reflect.defineProperty(new Proxy({}, falsy), "x", { value: 1 }); }));
        out.push(msg(function () { return Object.defineProperty(new Proxy({}, falsy), "x", { value: 1 }); }));
        var truthy = { defineProperty() { return true; } };
        out.push(msg(function () { return Reflect.defineProperty(new Proxy(Object.preventExtensions({}), truthy), "x", { value: 1 }); }));
        out.push(msg(function () { return Reflect.defineProperty(new Proxy({}, truthy), "x", { value: 1, configurable: false }); }));
        out.push(msg(function () { return Reflect.defineProperty(new Proxy({}, truthy), "x", { value: 1 }); }));
        "#,
        &[
            "false",
            "TypeError: Proxy's 'defineProperty' trap returned falsy value for property 'x'",
            "TypeError: Proxy's 'defineProperty' trap returned true even though getOwnPropertyDescriptor of the Proxy's target returned undefined and the target is non-extensible",
            "TypeError: Proxy's 'defineProperty' trap returned true for a non-configurable field even though getOwnPropertyDescriptor of the Proxy's target returned undefined",
            "true",
        ],
    );
}

#[test]
fn proxy_get_own_property_descriptor_trap_is_validated() {
    check(
        r#"
        out.push(msg(function () { return Reflect.getOwnPropertyDescriptor(new Proxy({}, { getOwnPropertyDescriptor() { return 1; } }), "x"); }));
        out.push(msg(function () { return Reflect.getOwnPropertyDescriptor(new Proxy({ x: 1 }, { getOwnPropertyDescriptor() { return { value: 1, configurable: false }; } }), "x"); }));
        var fixed = {};
        Object.defineProperty(fixed, "y", { value: 1, configurable: false });
        out.push(msg(function () { return Reflect.getOwnPropertyDescriptor(new Proxy(fixed, { getOwnPropertyDescriptor() { return undefined; } }), "y"); }));
        out.push(msg(function () {
          var d = Reflect.getOwnPropertyDescriptor(new Proxy({}, { getOwnPropertyDescriptor() { return { value: 7, configurable: true }; } }), "z");
          return d.value + "," + d.writable + "," + d.enumerable + "," + d.configurable;
        }));
        "#,
        &[
            "TypeError: result of 'getOwnPropertyDescriptor' call should either be an Object or undefined",
            "TypeError: Result from 'getOwnPropertyDescriptor' can't be non-configurable when the 'target' doesn't have it as an own property or if it is a configurable own property on 'target'",
            "TypeError: When the result of 'getOwnPropertyDescriptor' is undefined the target must be configurable",
            "7,false,false,true",
        ],
    );
}

#[test]
fn proxy_revoked_and_constructor_errors() {
    let revoked = "TypeError: Proxy has already been revoked. No more operations are allowed to be performed on it";
    check(
        r#"
        var r = Proxy.revocable({}, {});
        r.revoke();
        out.push(msg(function () { return r.proxy.a; }));
        out.push(msg(function () { return Reflect.ownKeys(r.proxy); }));
        out.push(msg(function () { return Reflect.has(r.proxy, "a"); }));
        out.push(msg(function () { return new Proxy(1, {}); }));
        out.push(msg(function () { return new Proxy({}, 1); }));
        out.push(msg(function () { return Proxy({}, {}); }));
        out.push(msg(function () { return Proxy.revocable({}); }));
        out.push(Object.keys(Proxy.revocable({}, {})).join());
        "#,
        &[
            revoked,
            revoked,
            revoked,
            "TypeError: A Proxy's 'target' should be an Object",
            "TypeError: A Proxy's 'handler' should be an Object",
            "TypeError: calling Proxy constructor without new is invalid",
            "TypeError: Proxy.revocable needs to be called with two arguments: the target and the handler",
            "proxy,revoke",
        ],
    );
}

#[test]
fn proxy_trap_lookup_order_and_receiver() {
    check(
        r#"
        var log = [];
        var handler = new Proxy({}, { get(target, key) { log.push("h." + String(key)); return undefined; } });
        var proxy = new Proxy({ a: 1 }, handler);
        Reflect.has(proxy, "a");
        Reflect.ownKeys(proxy);
        Reflect.getPrototypeOf(proxy);
        out.push(log.join());

        var seen;
        var observed = new Proxy({}, { get(target, key, receiver) { seen = [target, key, receiver]; return 5; } });
        var child = Object.create(observed);
        out.push(String(child.z) + "," + (seen[1]) + "," + (seen[2] === child));
        "#,
        &["h.has,h.ownKeys,h.getPrototypeOf", "5,z,true"],
    );
}

#[test]
fn reflect_argument_errors() {
    check(
        r#"
        out.push(msg(function () { return Reflect.construct(function () {}, 1); }));
        out.push(msg(function () { return Reflect.construct(1, []); }));
        out.push(msg(function () { return Reflect.construct(function () {}, [], 1); }));
        out.push(msg(function () { return Reflect.apply(1); }));
        out.push(msg(function () { return Reflect.apply(function () {}, null, 1); }));
        out.push(msg(function () { return Reflect.get(1, "a"); }));
        out.push(msg(function () { return Reflect.getPrototypeOf(1); }));
        out.push(msg(function () { return Reflect.setPrototypeOf({}, 1); }));
        out.push(msg(function () { return Reflect.defineProperty(1, "a", {}); }));
        out.push(msg(function () { return Reflect.has(1, "a"); }));
        out.push(msg(function () { return Reflect.ownKeys(1); }));
        "#,
        &[
            "TypeError: Reflect.construct requires the second argument be an object",
            "TypeError: Reflect.construct requires the first argument be a constructor",
            "TypeError: Reflect.construct requires the third argument be a constructor if present",
            "TypeError: Reflect.apply requires the first argument be a function",
            "TypeError: Reflect.apply requires the third argument be an object",
            "TypeError: Reflect.get requires the first argument be an object",
            "TypeError: Reflect.getPrototypeOf requires the first argument be an object",
            "TypeError: Reflect.setPrototypeOf requires the second argument be either an object or null",
            "TypeError: Reflect.defineProperty requires the first argument be an object",
            "TypeError: Reflect.has requires the first argument be an object",
            "TypeError: Reflect.ownKeys requires the first argument be an object",
        ],
    );
}

#[test]
fn reflect_operations_use_the_receiver_and_the_argument_list() {
    check(
        r#"
        var receiver = {};
        out.push(Reflect.set({}, "a", 1, receiver) + "," + receiver.a);
        var Constructor = function () { this.v = arguments[1]; };
        out.push(String(Reflect.construct(Constructor, [1, 2]).v));
        out.push(String(Reflect.apply(function (a, b) { return a + b; }, null, { length: 2, 0: 3, 1: 4 })));
        out.push(String(Reflect.deleteProperty(Object.freeze({ a: 1 }), "a")));
        out.push(String(Reflect.set(Object.freeze({ a: 1 }), "a", 2)));
        out.push(Object.keys(Reflect).length + "," + String(Reflect[Symbol.toStringTag]));
        "#,
        &["true,1", "2", "7", "false", "false", "0,Reflect"],
    );
}

#[test]
fn json_stringify_gap_to_json_and_replacer() {
    check(
        r#"
        out.push(JSON.stringify([1], null, 20));
        out.push(JSON.stringify([1], null, "abcdefghijklmnop"));
        out.push(JSON.stringify([1], null, new Number(2)));
        out.push(JSON.stringify([1], null, 0.9));
        out.push(JSON.stringify({ a: { toJSON(key) { return "k=" + key; } } }));
        out.push(JSON.stringify([{ toJSON(key) { return typeof key + key; } }]));
        out.push(JSON.stringify({ toJSON(key) { return "r[" + key + "]"; } }));
        out.push(JSON.stringify({ a: 1, b: 2, c: 3 }, ["c", "a", "c"]));
        out.push(JSON.stringify({ a: 1, 2: 2 }, [2, new String("a"), {}]));
        var holder;
        JSON.stringify(5, function (key, value) { holder = this; return value; });
        out.push(Object.keys(holder).join() + "|" + holder[""]);
        out.push(JSON.stringify({ a: Symbol(), b: undefined, c: 1 }));
        out.push(JSON.stringify([Symbol(), undefined, function () {}]));
        out.push(String(JSON.stringify(Symbol())));
        out.push(JSON.stringify([1.5, -0, 1e21, NaN, Infinity]));
        out.push(JSON.stringify([new Number(3), new String("s"), new Boolean(false)]));
        "#,
        &[
            "[\n          1\n]",
            "[\nabcdefghij1\n]",
            "[\n  1\n]",
            "[1]",
            "{\"a\":\"k=a\"}",
            "[\"string0\"]",
            "\"r[]\"",
            "{\"c\":3,\"a\":1}",
            "{\"2\":2,\"a\":1}",
            "|5",
            "{\"c\":1}",
            "[null,null,null]",
            "undefined",
            "[1.5,0,1e+21,null,null]",
            "[3,\"s\",false]",
        ],
    );
}

#[test]
fn json_stringify_cycles_bigint_proxy_and_raw_json() {
    check(
        r#"
        var cyclic = {};
        cyclic.self = cyclic;
        out.push(msg(function () { return JSON.stringify(cyclic); }));
        var cyclicArray = [];
        cyclicArray.push([cyclicArray]);
        out.push(msg(function () { return JSON.stringify(cyclicArray); }));
        out.push(msg(function () { return JSON.stringify(1n); }));
        BigInt.prototype.toJSON = function () { return "big" + this; };
        var withToJson = JSON.stringify({ a: 1n });
        delete BigInt.prototype.toJSON;
        out.push(withToJson);
        out.push(JSON.stringify(new Proxy({ a: 1, b: [2] }, {})));
        out.push(JSON.stringify(new Proxy([1, 2], {})));
        var revocable = Proxy.revocable([], {});
        revocable.revoke();
        out.push(msg(function () { return JSON.stringify(revocable.proxy); }));
        out.push(JSON.stringify({ a: JSON.rawJSON("1e1000") }));
        out.push(String(JSON.isRawJSON(JSON.rawJSON("1"))) + "," + String(JSON.isRawJSON({})));
        out.push(msg(function () { return JSON.rawJSON(" 1"); }));
        out.push(msg(function () { return JSON.rawJSON(""); }));
        "#,
        &[
            "TypeError: JSON.stringify cannot serialize cyclic structures.",
            "TypeError: JSON.stringify cannot serialize cyclic structures.",
            "TypeError: JSON.stringify cannot serialize BigInt.",
            "{\"a\":\"big1\"}",
            "{\"a\":1,\"b\":[2]}",
            "[1,2]",
            "TypeError: Proxy has already been revoked. No more operations are allowed to be performed on it",
            "{\"a\":1e1000}",
            "true,false",
            "SyntaxError: JSON.rawJSON cannot accept string starting with ' '",
            "SyntaxError: JSON.rawJSON cannot accept empty string",
        ],
    );
}

#[test]
fn json_parse_reviver_order_holder_and_source() {
    check(
        r#"
        var log = [];
        JSON.parse('{"a":[1,2],"b":{"c":3}}', function (key, value) { log.push(key); return value; });
        out.push(log.join());

        var removed = JSON.parse('{"a":1,"b":2}', function (key, value) { return key === "a" ? undefined : value; });
        out.push(("a" in removed) + "," + removed.b);
        var removedElement = JSON.parse("[1,2,3]", function (key, value) { return key === "1" ? undefined : value; });
        out.push((1 in removedElement) + "," + removedElement.length);

        out.push(JSON.parse("5", function (key, value) { return typeof this + ":" + JSON.stringify(key) + ":" + value; }));
        var mutated = JSON.parse('{"a":1,"b":2}', function (key, value) { if (key === "a") this.b = 3; return value; });
        out.push(String(mutated.b));

        var sources = [];
        JSON.parse('[1.0, "x", {"y": 2e0}]', function (key, value, context) {
          if (typeof value !== "object") sources.push(context.source);
          else sources.push(String("source" in context));
          return value;
        });
        out.push(sources.join("|"));
        out.push(msg(function () { return JSON.parse("[1,"); }));
        out.push(String(JSON.parse("[1,2]", 5).length));
        "#,
        &[
            "0,1,a,c,b,",
            "false,2",
            "false,3",
            "object:\"\":5",
            "3",
            "1.0|\"x\"|2e0|false|false",
            "SyntaxError: JSON Parse error: Unexpected EOF",
            "2",
        ],
    );
}

#[test]
fn symbol_errors_and_registry() {
    check(
        r#"
        out.push(Symbol("x").toString());
        out.push(Symbol.keyFor(Symbol.for("k")) + "," + String(Symbol.keyFor(Symbol("k"))));
        out.push(msg(function () { return Symbol.keyFor(1); }));
        out.push(msg(function () { return Symbol.prototype.valueOf.call(1); }));
        out.push(msg(function () { return Symbol.prototype.toString.call({}); }));
        out.push(String(Symbol().description) + "," + JSON.stringify(Symbol("").description));
        "#,
        &[
            "Symbol(x)",
            "k,undefined",
            "TypeError: Symbol.keyFor requires that the first argument be a symbol",
            "TypeError: Symbol.prototype.valueOf requires that |this| be a symbol or a symbol object",
            "TypeError: Symbol.prototype.toString requires that |this| be a symbol or a symbol object",
            "undefined,\"\"",
        ],
    );
}

#[test]
fn bigint_constructor_and_prototype() {
    check(
        r#"
        out.push(msg(function () { return BigInt(1.5); }));
        out.push(msg(function () { return BigInt.asUintN(-1, 1n); }));
        out.push(String(BigInt.asUintN(8, 257n)) + "," + String(BigInt.asIntN(8, 255n)));
        out.push(msg(function () { return BigInt.prototype.toString.call(1); }));
        out.push((255n).toString(16));
        var hits = 0;
        try { BigInt.asUintN(Symbol(), { valueOf() { hits++; return 1n; } }); } catch (e) {}
        out.push(String(hits));
        "#,
        &[
            "RangeError: Not an integer",
            "RangeError: number of bits cannot be negative",
            "1,-1",
            "TypeError: 'this' value must be a BigInt or BigIntObject",
            "ff",
            "0",
        ],
    );
}
