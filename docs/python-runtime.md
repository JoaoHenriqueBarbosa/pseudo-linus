# Runtime do `ul-python`: como acrescentar módulos, métodos e builtins

O interpretador (`crates/ul-python`) mira o CPython 3.13.5 do Debian 13: saída, mensagens de erro e
códigos de saída iguais. Aqui está o contrato para estender a stdlib sem tocar em `vm.rs` ou
`compile.rs` (que têm dono único).

## Assinatura de toda função nativa

```rust
use crate::object::{Kw, NativeFnPtr, Value};
use crate::vm::{exc, type_error, PyResult, Vm};

fn minha_funcao(vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> { ... }
```

- Nos métodos de tipo (`methods/*.rs`) o receptor vem em `args[0]`; os argumentos reais em `args[1..]`.
- Erros: `Err(exc("ValueError", "mensagem"))` ou `Err(type_error("..."))`. As classes disponíveis
  estão em `object::EXC_CLASSES` (acrescente ali uma classe nova se faltar, com o pai certo). A
  mensagem tem de ser a do CPython, letra por letra.
- `crate::native_util` tem `bind` (liga posicionais e nomeados por nome, com os erros do CPython),
  `no_kwargs`, `exactly`, `want_str`, `want_int`, `value_error`.
- `vm.call_value(&f, args, kw)` chama um callable do Python (para `key=`, callbacks).
- `crate::vm::iterate(&v)` devolve os itens de qualquer iterável; `vm.getattr(&obj, "nome")`;
  `crate::vm::py_lt(a, b)` e `crate::vm::py_binary("+", a, b)` aplicam a semântica do Python.
- `crate::object`: `Value`, `repr(&v)`, `to_str(&v)`, `py_eq(&a, &b)`, `hash(&v)`, `Dict`, `Set`,
  `PyStr` (`as_str()`, `len()` em pontos de código), `Value::str(..)`, `Value::list(vec)`,
  `Value::tuple(vec)`. `Value::Int` é `i64` (inteiro arbitrário ainda não existe: use `i64` e
  levante `OverflowError` onde estourar).

## Onde cada coisa vai

| O quê | Onde |
|---|---|
| Métodos de `str`, `list`, `dict`, `set`, `tuple`, `bytes`, `int`/`float` | `methods/<tipo>m.rs`: `pub const TABLE: &[(&str, NativeFnPtr)]` |
| Funções embutidas (`isinstance`, `map`...) | `builtins.rs`: `TABLE` (tem precedência sobre as antigas em `vm.rs`) |
| Módulo `x` | `modules/x.rs` com `pub fn build(vm: &mut Vm) -> Rc<ModuleObj>` usando `ModuleBuilder::new("x").func("nome", f).value("K", v).build()`; registrar em `modules::import` |
| Objeto com estado/métodos (`re.Pattern`, `ZipFile`...) | `impl ExtObject for MeuTipo` e `Value::Ext(Rc::new(...))`; estado em `Cell`/`RefCell` dentro do tipo |

`ExtObject` (em `object/mod.rs`): `type_name`, `repr`, `methods` (nomes estáticos dos métodos),
`call_method`, `getattr` (atributos de dado), `is_iterable`/`iter_next`, `len`, `getitem`,
`is_true`, `binop` (operadores com o objeto de um lado), `richcmp` (comparações).

## Limitações do núcleo hoje (não tente contornar; não use)

Sem classes de usuário, geradores, `with`, comprehensions, `*args`/`**kwargs`, fatias (`x[1:3]`),
f-strings completas, `%` em `str`. Funções nativas que no CPython devolvem iteradores preguiçosos
(`map`, `filter`, `zip`, `dict.items`...) devolvem **listas** aqui. Isso será revisto quando os
geradores existirem.

## Regras de trabalho

- `#![forbid(unsafe_code)]`; sem dependências externas novas; `std` apenas (e `sysabi` onde o
  módulo falar com o sistema de arquivos).
- Português acentuado em comentários e docstrings; identificadores em inglês. Sem travessão.
- Cada arquivo seu leva testes `#[cfg(test)]` que rodam o Python de verdade:
  `let o = crate::run_source("print('x'.upper())"); assert_eq!(String::from_utf8(o.stdout).unwrap(), "X\n");`
  (`o.stderr` e `o.status` também). Compare sempre com o que o CPython 3.13 imprime (se tiver
  dúvida do comportamento, escolha o caso que você tem certeza e deixe o resto de fora, em vez de
  chutar).
- Você NÃO compila nem roda nada (só lê e escreve arquivos), então escreva Rust conservador e releia
  o que escreveu procurando erros de tipo, empréstimo e `match` não exaustivo. Quem compilar
  devolve os erros para você corrigir.
- Mexa SOMENTE nos arquivos que o seu pedido lista. Se precisar de algo em `vm.rs`, `compile.rs` ou
  `object/mod.rs`, não edite: descreva no relatório final o que falta.
