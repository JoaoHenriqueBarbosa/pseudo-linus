# zjsc: regexp sticky e lastIndex (24 divergências do golden contra o bun)

## Causa raiz

`lastIndex` do `RegExpObject` vive num campo da célula (`last_index`), não na `Structure`. O
`JSObject::get_own_property_slot` despachava para `RegExpObject::get_own_property_slot`, mas
`JSObject::put`, `JSObject::delete_property` e `JSObject::define_own_property` não despachavam para os
overrides já portados em `reg_exp_object.rs` (`put`, `delete_property`, `define_own_property`).

Efeito: `r.lastIndex = 1` num script gravava uma propriedade comum no objeto, e os nativos
(`exec`, `test`, `Symbol.search`, `Symbol.replace`, `Symbol.matchAll`...) liam o campo, que seguia em 0.
Explica quase tudo da lista: sticky com `lastIndex` diferente de zero (`/a/y`, `/a|b/y`, `/(?<=a)b/y`,
`/^a/my`, `/b/y`), `lastIndex="1"` e `1.9` em `/g`, `lastIndex=5` com `/a/` (que deve ficar 5), `search`
restaurando o `lastIndex` anterior, `split` com `lastIndex=3`, `matchAll` sticky, `/./g` com
`lastIndex=1` sobre par substituto.

Os três casos de `lastIndex` não gravável (`defineProperty` com `writable:false`, `Object.freeze`) também
dependem do `define_own_property` chegar ao `RegExpObject` (liga a flag `LAST_INDEX_IS_NOT_WRITABLE_FLAG`
e o `put` seguinte lança `TypeError`).

## Correção

`src/runtime/js_object.rs`: despacho para `RegExpObject::put`, `delete_property` e `define_own_property`
quando o tipo é `RegExpObjectType` e o nome é `lastIndex`, no mesmo molde do despacho de leitura.

## Proxy de RegExp em replace/match/split/search/test

`RegExp.prototype[Symbol.replace].call(new Proxy(/b/g, ...))` lançava `TypeError: Attempting to define
property on object that is not extensible`; esperado é o log `flags, exec, exec, exec`.

Causa: `set_object_property` (`string_regexp_support.rs`), o `Set(obj, "lastIndex", v, true)` do caminho
genérico, chamava `JSObject::put` direto no `Proxy`. O `JSObject::put` do porte não despacha o próprio
`Proxy` (só o encontrado na cadeia de protótipos, em `put_inline_slow`), então a escrita caía na célula-casca
do Proxy, que não é extensível. No C++ o `put` é virtual (`ProxyObject::put`).

Correção: se o objeto é `ProxyObjectType`, `set_object_property` usa `proxy_object::put_from_proxy` (mesmo
molde de `js_scope.rs` e do slow path do LLInt). Nada compilado (cargo proibido): conferir no golden.

Risco residual: outros chamadores de `JSObject::put` com receptor possivelmente Proxy podem ter a mesma
falha; o `get_object_property` usa `object.get`, que já despacha.

## Despacho de Proxy na entrada dos métodos de `JSObject` (follow-up)

O furo era geral: no C++ `put`, `putByIndex`, `deleteProperty`, `deletePropertyByIndex` e
`defineOwnProperty` são virtuais, e o porte só despachava o `Proxy` encontrado na cadeia
(`put_inline_slow`). Agora `JSObject::put`, `put_by_index`, `delete_property`, `delete_property_by_index`
e `define_own_property` (`src/runtime/js_object.rs`) despacham logo na entrada, quando
`type_() == ProxyObjectType`, para `put_from_proxy`, `put_by_index_from_proxy` (this = o próprio Proxy),
`delete_from_proxy` e `define_own_property_from_proxy` (os dois últimos novos em `proxy_object.rs`, com a
conversão `Thrown` para `PutError` e o realm da `Structure`).

Sem recursão: `perform_put` sem trap chama `object_set` no alvo, `perform_delete` e
`perform_define_own_property` operam no alvo; nenhum volta a `JSObject::put` do Proxy. O laço de
`put_inline_slow` já excluía `self` (`!ptr::eq(obj, self)`), então não duplica.

Já cobertos: `get_property_slot` (despacha o Proxy na primeira volta, logo `has_property` também),
`get_prototype`, `is_extensible`. `get_own_property_names` não existe em `JSObject` (o
`ProxyObject::get_own_property_names` é chamado pelo despacho de chaves), sem furo achado.

O remendo de `set_object_property` (`string_regexp_support.rs`) ficou redundante e foi removido
(`lookup.put` direto). Nada compilado (cargo proibido): conferir com `regexp_edge_bun_golden` e os testes
de Proxy.

## Não coberto (não são da mesma raiz)
- `RegExp.$1 + RegExp.lastMatch + RegExp.input`: o "esperado" do golden é a própria expressão do teste
  (artefato do harness, não do motor); o porte devolve `""`, o que é o certo sem match prévio.

Nada foi compilado nem rodado (cargo proibido nesta tarefa): conferir com
`regexp_edge_bun_golden` na próxima rodada.
