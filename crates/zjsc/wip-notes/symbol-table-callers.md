# Usos de SymbolTable fora dos arquivos já corrigidos

API nova em `src/runtime/symbol_table.rs`: `SymbolTableEntry::new(VarOffset, attributes)` e
`from_var_offset(VarOffset)`; `add_locked(NO_LOCKING_NECESSARY, UniquedKey, entry)`;
`take_next_scope_offset_locked(NO_LOCKING_NECESSARY)`; `get(&UniquedKey) -> Fast` (sem locker);
`cell_id()` e `clone_scope_part(&VM, Propagate)` são métodos de `SymbolTable`, então sobre
`SymbolTableRef` (`Rc<RefCell<..>>`) precisam de `.borrow()`. `Identifier::impl_()` devolve
`Option<UniquedKey>`: passe `&x.impl_().unwrap()` para `get` e `x.impl_().unwrap()` para `add_locked`/`set`.

## bytecode_generator_cpp1.rs
- 775, 817, 843, 851: `SymbolTableEntry::new(var_offset)` vira `SymbolTableEntry::from_var_offset(var_offset)`.
- 776, 815, 841, 849-851: `set(name, ..)` precisa de `UniquedKey` (hoje passa `name`); conferir o tipo de `name`.

## bytecode_generator_cpp2.rs
- 156: `module_environment_symbol_table.cell_id()` vira `.borrow().cell_id()`; 165: `cloned.cell_id()` vira `cloned.borrow().cell_id()`.
- 426: `take_next_scope_offset(NO_LOCKING_NECESSARY)` vira `take_next_scope_offset_locked(NO_LOCKING_NECESSARY)` (idem 446).
- 437: `take_next_scope_offset_locked()` falta o argumento `NO_LOCKING_NECESSARY`.
- 427, 438, 447: `.add(NO_LOCKING_NECESSARY, ..)` vira `.add_locked(NO_LOCKING_NECESSARY, ..)`.
- 429, 440, 449: `.impl_()` vira `.impl_().unwrap()` (a chave é `UniquedKey`, não `Option`).
- 430, 441, 450: `SymbolTableEntry::new(VarOffset::from_scope_offset(offset))` vira `SymbolTableEntry::from_var_offset(..)`.

## bytecode_generator_cpp5.rs
- 703: `symbol_table.borrow().get(property.impl_())` vira `.get(&property.impl_().unwrap())`.

## bytecode_generator_part3.rs
- 86: `LexicalScopeStackEntry` precisa de `#[derive(Clone)]` (cpp3 e cpp5 chamam `.clone()` na entrada inteira).
  O campo `symbol_table` fica `Option<SymbolTableRef>` (None no with-scope); o cpp3 já foi ajustado com `unwrap`.

## Resolvidos
- bytecode_generator_cpp3.rs: todos os usos (SymbolTableRef, `borrow()`, `_locked`, `UniquedKey`, `get_fast()`).
- bytecode_generator.rs e bytecode_generator_part2.rs: sem uso a corrigir (import de `SymbolTable`, `SymbolTableEntry`,
  `NO_LOCKING_NECESSARY` em bytecode_generator.rs:43 continua necessário ao cpp1 e cpp2).
