# Plano: espaço único de `cell_id` (registro central de células)

## Levantamento (src/runtime, grep de `cell_id`, `CELL_ID_TAG`, `thread_local`)

| Tipo | Arquivo:linha | Tag (3 bits baixos) | Layout do id |
|---|---|---|---|
| JSString | js_string.rs:40,47 | 0b000 | `(idx+1)<<3` |
| RegExp | reg_exp.rs:78,118 | 0b001 | `((idx+1)<<3)\|1` |
| SymbolTable | symbol_table.rs:263,305 | 0b001 | `((idx+1)<<3)\|1` |
| JSCellButterfly | js_cell_butterfly.rs:100,121 | 0b100 | `((idx+1)<<3)\|4` |
| JSTemplateObjectDescriptor | js_template_object_descriptor.rs:23,48 | 0b100 | `((idx+1)<<3)\|4` |
| Symbol | symbol.rs:49,66 | 0b101 | `((idx+1)<<3)\|5` |
| JSScope e subclasses | js_scope.rs:45,60 (`encode_cell_id`/`decode_cell_id`) | 0b101 + 4 bits de `CellIdKind::Scope=0` | `((((idx+1)<<4)\|kind)<<3)\|5` |
| JSCallee/JSFunction | js_callee.rs:64,95 (reusa o de js_scope), js_function.rs:113 | 0b101 + `CellIdKind::Callee=1` | idem |
| JSObject/JSFinalObject | js_object.rs ainda não existe; js_cell.rs:15,222 já reserva 0b101 e chama `JSObject::from_cell_id` | 0b101 (reservada) | a definir |

Outros `thread_local!` em runtime (structure.rs:118, options.rs:288, vm.rs) não são registros de célula. Consumidores do id: `JSValue::Cell(usize)` (js_value.rs:89,143,149,157,317, teste com 0x7f00_1234_5670), `cell_type_from_cell_id` (js_cell.rs:218, só conhece JSString e JSObject), code_block.rs:508-519 (grava `cell_id as u32` em metadata: o id precisa caber em 32 bits), structure.rs:770 (`PointerKey::Object(cell_id)`), bytecode_generator_cpp2/3/4 (`from_cell(x.cell_id())`).

## Colisões e esgotamento

- 0b001: RegExp x SymbolTable. `RegExp::from_cell_id` nunca é chamado hoje, mas `SymbolTable::from_cell_id(regexp_id)` já devolve outra coisa ou `None` conforme o índice (bug latente: `from_cell_id` só confere o índice, não o tipo).
- 0b100: JSCellButterfly x JSTemplateObjectDescriptor (já admitida em js_cell.rs).
- 0b101: Symbol x JSScope/JSCallee x JSObject reservada. O id de Symbol e o de um Scope podem coincidir bit a bit.
- Esgotamento: o próprio js_cell.rs fixa o bit 1 limpo, então só existem 4 tags (0,1,4,5) para 9+ tipos, e o JSScope já remendou com 4 bits de "kind" dentro da tag. Não escala para JSMap, JSPromise, Structure, ProxyObject, etc.

## Desenho único

`runtime/cell_registry.rs` (um só `thread_local! static CELLS: RefCell<Vec<Option<CellEntry>>>`):

- `cell_id = (index + 1) << 3` (3 bits baixos sempre 0: ponteiro alinhado, igual ao C++; cabe em u32 até 2^29 células, o que mantém `as u32` do code_block válido). Zero nunca é id válido.
- `enum CellEntry { String(JSStringRef), RegExp(RegExpRef), Symbol(SymbolRef), SymbolTable(SymbolTableRef), CellButterfly(..), TemplateObjectDescriptor(..), Scope(JSScopeRef), Callee(JSCalleeRef), Object(JSObjectRef), ... }` (ou `Rc<dyn Any>` + `JSType` ao lado; prefira o enum: despacho sem downcast e exaustivo). Cada entrada guarda o `JSType` do cabeçalho `JSCell`, lido por `cell_type(id) -> Option<JSType>` (substitui `cell_type_from_cell_id`, que passa a ser função única sem varrer registros).
- API: `allocate(entry) -> usize`, `get(id) -> Option<CellEntry>` (clone de Rc), e `get_as::<T>` por tipo: `JSString::from_cell_id(id)` vira `match cell_registry::get(id) { Some(CellEntry::String(s)) => Some(s), _ => None }`. A conferência de tipo passa a ser exata, o que elimina as colisões por construção.
- Para tipos criados com `Rc::new_cyclic`/dois passos (RegExp com `cell_id: 0`, JSScope com `register_scope`/`register_callee`), `allocate` reserva o id e `set(id, entry)` preenche depois.
- Compatível com CONVENTIONS: é o degrau antes do `Heap`/`CellId(u32)`; quando o `Heap` chegar, `cell_registry` troca `Vec<CellEntry>` pela arena e o id vira `CellId` sem mexer nos chamadores.

## Migração (cada fatia ~5 min, um tipo por vez, build em background entre elas)

1. Criar `cell_registry.rs` com `CellEntry`, `allocate`/`set`/`get`/`cell_type`, testes (ids distintos entre tipos, `from_cell_id` cruzado dá `None`). Registrar no `runtime/mod.rs`.
2. JSString (tag 0 já é a forma final; só trocar `REGISTRY` pelo central) e remover `CELL_ID_SHIFT`.
3. Symbol e SymbolTable (resolve 0b101 e 0b001).
4. RegExp e JSCellButterfly e JSTemplateObjectDescriptor (resolve 0b001 e 0b100).
5. JSScope + JSCallee + JSFunction: apagar `CellIdKind`, `encode_cell_id`, `decode_cell_id`, `SCOPE_REGISTRY`, `CALLEE_REGISTRY`; `register_scope`/`register_callee` viram `cell_registry::set`.
6. `cell_type_from_cell_id` em js_cell.rs vira `cell_registry::cell_type`; corrigir o quadro de tags no doc comment de js_cell.rs; `JSValue::is_string`/`as_js_string` usam `CellEntry::String`.
7. JSObject/JSFinalObject (e demais tipos novos) já nascem no registro central; rodar `grep -rn 'CELL_ID_TAG\|CELL_ID_SHIFT\|CellIdKind' src` até zerar e checar o teste de js_value com `Cell(0x7f00_1234_5670)` (id arbitrário, não deve consultar o registro).

Regra para agentes novos: proibido `thread_local` de célula e tag própria; todo tipo novo adiciona uma variante em `CellEntry`.
