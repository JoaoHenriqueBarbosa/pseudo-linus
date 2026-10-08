# zjsc: convenções do porte do JavaScriptCore para Rust

Objetivo: o motor JavaScript do Bun 1.4.2 em Rust seguro, com comportamento observável idêntico ao
do JavaScriptCore que o Bun usa (WebKit do fork, commit `bec044e92d`). O mesmo motor serve depois ao
Chromium em Rust, com as diferenças observáveis em relação ao V8 como parâmetro ("personalidade").

Fontes:

- `upstream/JavaScriptCore`, `upstream/WTF`, `upstream/bmalloc`: o C++ original.
- `derived/`: os arquivos gerados pelo build do Bun (`Bytecodes.h`, `*.lut.h`, `JSCBuiltins.cpp`,
  `KeywordLookup.h`, `cmakeconfig.h` com as flags `ENABLE_*` valendo).

Oráculo: o `bun` 1.4.2 (`bun -e '...'`). Saída, mensagens de erro, formato de pilha e ordem de
propriedades se conferem nele.

## Regras gerais

- `#![forbid(unsafe_code)]`. Nenhum `unsafe`, nenhuma FFI, nenhuma crate de motor JavaScript.
- Tradução fiel, função por função, na mesma ordem do C++. Nada de simplificar, trocar algoritmo,
  pular ramo de erro ou deixar `todo!()`/`unimplemented!()`. Ramo que só existe em outra plataforma
  (Windows, Cocoa, `OS(DARWIN)`), sob `ASSERT_ENABLED`, `ENABLE(ASSERT)` ou de depuração, some.
- `#if ENABLE(X)` se resolve pelo `derived/cmakeconfig.h` e pelos padrões de `WTF/wtf/PlatformEnable.h`
  para Linux x86_64, com estas exceções que NÃO têm comportamento observável e não se portam:
  - **Camadas de JIT** (`jit/`, `dfg/`, `ftl/`, `b3/`, `assembler/`, `disassembler/`, `domjit/`,
    `offlineasm/`, os tiers JIT do `wasm/`): Rust seguro não executa código gerado. O código roda no
    interpretador (`llint/` + `interpreter/`), portado como interpretador de bytecode em Rust, com a
    semântica do `LowLevelInterpreter*.asm` e dos slow paths. Em `#if ENABLE(JIT)`, vale o ramo
    `#else`/C_LOOP.
  - **bmalloc/libpas e Gigacage**: o alocador do Rust os substitui.
  - **Inspector remoto, `fuzzilli/`, `testmem/`, `tools/` de teste, `API/` de Cocoa/GLib.**
- Comentários em português acentuado, sem travessão. Identificadores em inglês.
- Sem `.unwrap()`/`.expect()` que possa disparar em entrada de usuário. `unwrap` só onde o C++ tem
  `ASSERT`/`RELEASE_ASSERT` de invariante.

## Nomes (determinísticos)

- Cada arquivo C++ vira um módulo com o caminho do diretório em snake_case: `parser/Lexer.cpp` e
  `parser/Lexer.h` viram `crate::parser::lexer`; `WTF/wtf/text/StringImpl.h` vira
  `crate::wtf::text::string_impl`; `WTF/wtf/dtoa/fast-dtoa.cc` vira `crate::wtf::dtoa::fast_dtoa`.
  O `.h` e o `.cpp` do mesmo nome são um módulo só. `*Inlines.h` entra no módulo do `.h` base.
- Classes e structs mantêm o nome do C++ (`StringImpl`, `Lexer`, `JSObject`). Métodos e funções livres
  em snake_case (`isIdentStart` vira `is_ident_start`). Campo `m_fooBar` vira `foo_bar`.
- `enum class` vira `enum` com as mesmas variantes; enum com valores numéricos usados em contas vira
  `#[repr(u8/u32)]` com os valores do C++.
- Constantes e `#define` de constante viram `pub const`. Template vira genérico; especialização por
  `CharType` (`LChar`/`UChar`) vira genérico sobre o trait `crate::wtf::text::CharType`
  (`LChar = u8`, `UChar = u16`).
- Nada de glob `prelude`: cada módulo importa o que usa. Item que outro módulo precisa é `pub`.

## Modelo de dados (fechado, não se inventa outro)

1. **Strings da WTF**: `StringImpl` guarda `Latin1(Box<[u8]>)` ou `Utf16(Box<[u16]>)`, hash
   calculado preguiçoso com o mesmo algoritmo (`SuperFastHash`/`StringHasher`, aparece na ordem de
   nada observável, mas fica fiel). `String` é `Option<Rc<StringImpl>>` (nulo do C++ é `None`).
   `AtomString` é `String` internada numa tabela por thread. Texto JS é sempre UTF-16 semântico,
   nunca `std::String`.
2. **Heap do JS por índice**: todo `JSCell` vive numa arena do `Heap` (`crate::heap`), endereçado
   por `CellId(u32)`. `JSValue` é `enum JSValue { Empty, Undefined, Null, Bool(bool), Int32(i32),
   Double(f64), Cell(CellId) }`, com a semântica do `JSCJSValue.h`. Coleta por marcação e varredura,
   raízes explícitas (pilha do interpretador, handles, `MarkedArgumentBuffer`). Sem `Rc` para células.
3. **Árvore sintática compartilhada**: no C++ os `Node` do `parser/Nodes.h` vivem na arena do
   parser e o `Parser`/`ASTBuilder` guardam ponteiros para nós que já estão na árvore e os alteram
   depois (`setIsOptionalChainBase`, `setEcmaName`, `m_next`). Por isso todo ponteiro de nó é
   `NodeRef<T> = Rc<RefCell<T>>` (em `crate::parser::nodes`, com `node(x)` para criar), nunca `Box`;
   igualdade de ponteiro é `Rc::ptr_eq`. A herança vira `enum` por família (`Expression`,
   `Statement`) com uma variante `NodeRef<Struct>` por classe concreta; o acesso à base comum é
   por `expr.base() -> Ref<ExpressionNode>` e `base_mut()`, não `Deref`. Ponteiro nulo é `Option`.
   O `TreeBuilder` do `Parser` (template sobre `ASTBuilder`/`SyntaxChecker`) vira trait com tipos
   associados; no `ASTBuilder` eles são os próprios `Expression`, `Statement`, `NodeRef<..>`.
4. **Interpretador**: o bytecode é o mesmo do `derived/JavaScriptCore/Bytecodes.h` e
   `bytecode/BytecodeList.rb` (mesmos opcodes, mesmos operandos, mesma geração pelo
   `bytecompiler/`). A execução segue o `llint/LowLevelInterpreter*.asm` e os slow paths do
   `llint/LLIntSlowPaths.cpp` e `runtime/CommonSlowPaths.cpp`.
5. **Funções nativas**: `type NativeFunction = fn(&mut JSGlobalObject, &mut CallFrame) -> EncodedJSValue`
   como no C++ (`JSC_DEFINE_HOST_FUNCTION`); exceção pelo `ThrowScope` do `VM`, como no C++.
6. **Builtins em JavaScript** (`builtins/*.js`): o texto do `derived/JavaScriptCore/JSCBuiltins.cpp`
   é embutido como está e compilado pelo próprio motor, como o C++ faz.
7. **Tabelas geradas** (`*.lut.h`, `KeywordLookup.h`, `Bytecodes.h`): regeradas por script versionado
   em `scripts/` a partir do `derived/`, nunca copiadas à mão.
8. **Números**: `f64` com as regras do C++; formatação de número sempre pela tradução de
   `WTF/wtf/dtoa` (`numberToString`), nunca `format!` do Rust.
9. **Erros internos**: como no C++ (exceção JS pelo `VM`); `Result` só em helper local.

## Personalidade

Tudo que difere de forma observável entre JavaScriptCore e V8 (mensagens de erro, formato de
`Error.stack`, `Error.captureStackTrace`, limites, `Date.parse` lenient, `Function.prototype.toString`
de nativas) passa por `crate::personality::Personality`. Nesta fase só existe a personalidade
`JavaScriptCore`; o código chama a tabela em vez de escrever a constante no lugar, para a personalidade
V8 entrar depois sem cópia.

## Ordem de fechamento (camadas)

0. `wtf`: `ascii_ctype`, `text` (`StringImpl`, `String`, `StringBuilder`, `AtomString`, conversões
   UTF-8/UTF-16), `dtoa` (double-conversion), `math_extras`, `unicode` (propriedades usadas pelo lexer).
1. `parser`: tokens, `Lexer`, `Nodes`, `ASTBuilder`, `SyntaxChecker`, `Parser`, `VariableEnvironment`.
2. `bytecode` e `bytecompiler`.
3. `heap` e `runtime` (objetos, estruturas, protótipos, conversões).
4. `interpreter` + `llint` (interpretador de bytecode).
5. `builtins`, `yarr` (expressões regulares), `runtime/Intl*`, `wasm` (interpretador IPInt).
6. Shell `zjsc` (equivalente ao `jsc`) e, depois, a camada do Bun.
