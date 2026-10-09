// Serializador compartilhado do golden de ordem de chaves: o bun (scripts/gen-key-order-golden.js) e o porte
// (tests/key_order_bun_golden.rs) rodam exatamente este texto. `ser(objeto, filtro)` devolve uma linha com cada
// chave própria na ordem de Reflect.ownKeys: `chave:wec:tipo`, separadas por `;`. Símbolo sai como `@@descrição`.
// `wec` são as flags writable/enumerable/configurable (`-` quando falta, ou `A` no lugar de w em acessor);
// tipo é `get`/`set`/`get+set` em acessor, `F<length>/<name>` em função, ou o typeof (com `null` à parte).
function ser(o, filter) {
  var keys = Reflect.ownKeys(o);
  var out = [];
  for (var i = 0; i < keys.length; i++) {
    var k = keys[i];
    var label = typeof k === "symbol" ? "@@" + k.description : k;
    if (filter && filter.indexOf(label) < 0) continue;
    var d = Object.getOwnPropertyDescriptor(o, k);
    var kind;
    var flags;
    if ("value" in d) {
      flags = (d.writable ? "w" : "-") + (d.enumerable ? "e" : "-") + (d.configurable ? "c" : "-");
      var v = d.value;
      if (typeof v === "function") {
        var ld = Object.getOwnPropertyDescriptor(v, "length");
        var nd = Object.getOwnPropertyDescriptor(v, "name");
        kind = "F" + (ld ? String(ld.value) : "?") + "/" + (nd ? (typeof nd.value === "string" ? nd.value : typeof nd.value) : "?");
      } else {
        kind = v === null ? "null" : typeof v;
      }
    } else {
      flags = "A" + (d.enumerable ? "e" : "-") + (d.configurable ? "c" : "-");
      kind = (d.get ? "get" : "") + (d.get && d.set ? "+" : "") + (d.set ? "set" : "");
    }
    out.push(label + ":" + flags + ":" + kind);
  }
  return out.join(";");
}
