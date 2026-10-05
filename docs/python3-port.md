# Port do python3 (CPython 3.13 do Debian 13) para `crates/ul-python`

Plano, sem código. Objetivo: um `python3` em Rust com `#![forbid(unsafe_code)]` cuja saída (stdout,
stderr, código de saída) bata byte a byte com o CPython 3.13 do trixie, começando pelos casos de
corpus que hoje dependem dele.

## 1. Escopo da primeira versão útil

### O que o corpus usa hoje

`testbench/corpus/cases/csv/rfc4180.toml` (21 casos, tool `csv`) usa só dois programas `-c`, sempre
os mesmos (o front-end atual os reconhece literalmente; o port troca esse reconhecimento por
interpretação de verdade):

Leitura (16 casos):

```python
import csv, sys, json
for row in csv.reader(open(sys.argv[1], newline='', encoding='utf-8')):
    print(json.dumps(row, ensure_ascii=False))
```

Escrita (5 casos, um deles sem `lineterminator`, saindo em `\r\n`):

```python
import csv, sys, json
w = csv.writer(sys.stdout, lineterminator='\n')
for line in sys.stdin:
    w.writerow(json.loads(line))
```

`testbench/corpus/cases/file/text.toml` NÃO executa python3: todos os casos rodam `file`. A única
relação é o fixture `tool.py` (`#!/usr/bin/env python3\nprint('hi')\n`), que o `file` classifica como
script Python pelo shebang. Isso não exige interpretador, mas `print('hi')` vira caso de corpus
natural para o `python3` também (fatia 6).

### Linguagem necessária

- Linha de comando: `python3 -c PROG args...` (`sys.argv == ['-c', *args]`); depois `python3 arquivo.py`.
- `import a, b, c` (só módulos embutidos, sem sistema de arquivos de módulos).
- Comando `for ... in ...:` com bloco indentado; atribuição simples (`w = ...`).
- Expressões: chamada com argumentos posicionais e nomeados (`newline=''`, `encoding='utf-8'`,
  `ensure_ascii=False`, `lineterminator='\n'`), acesso a atributo (`csv.reader`, `sys.stdout`),
  subscrição (`sys.argv[1]`), literais `str` (aspas simples, escapes `\n`), `int`, `False`.
- Protocolo de iteração (`__iter__`/`__next__`) sobre leitor csv e sobre arquivo texto (por linha).

### Builtins

`print` (com `sep`, `end`, `file`, `flush` desde já, pois é barato), `open` (modos `r`/`w`,
`encoding`, `newline`), `iter`/`next` implícitos, e os tipos `str`, `int`, `bool`, `list`, `dict`,
`float`, `NoneType` (os quatro últimos chegam pelo `json.loads`).

### Módulos

- `sys`: `argv`, `stdin`, `stdout`, `stderr`, `exit`.
- `io` (implícito via `open` e `sys.std*`): `TextIOWrapper` sobre `BufferedReader/Writer` com
  tradução universal de nova linha (`newline=None` em stdin, `newline=''` no `open` dos casos),
  decodificação UTF-8 estrita, buffer de stdout descarregado na saída do interpretador.
- `csv` (port do `Modules/_csv.c` + `Lib/csv.py`): `reader` com o dialeto `excel` (máquina de estados
  `parse_process_char` inteira, inclusive `strict=False` e o erro `_csv.Error: unexpected end of
  data` do caso `csv-read-unterminated-quote`, com traceback), `writer` com `QUOTE_MINIMAL`,
  `lineterminator` padrão `\r\n`, e o caso especial de linha com um único campo vazio (`""`).
- `json`: `dumps` (com `ensure_ascii`, separadores padrão `', '`/`': '`) e `loads` (port do
  `scanner.c`/`decoder.py`, mensagens de `JSONDecodeError` iguais).

Observação sobre `csv-read-bom`: com `encoding='utf-8'` o BOM vira `﻿` dentro do primeiro campo
e o `json.dumps(..., ensure_ascii=False)` imprime o caractere cru; a fatia de `io` tem de preservar
isso (não é `utf-8-sig`).

## 2. Arquitetura

| Camada | Escolha | Justificativa |
|---|---|---|
| Tokenizer | Port linha a linha do `Parser/tokenizer.c` (e do `lexer/` de 3.13) | INDENT/DEDENT, continuação de linha, f-strings em 3.12+ e, sobretudo, as mensagens `SyntaxError`/`IndentationError` com coluna e o `^` do traceback só batem se a máquina de estados for a mesma. |
| Parser | PEG gerado à mão a partir de `Grammar/python.gram`, com as regras `invalid_*` | O CPython escolhe a mensagem de erro de sintaxe pelas alternativas `invalid_*` da segunda passada; um parser LL ou Pratt não reproduz "Did you mean...?" nem as posições. Memoização por (regra, posição) em `Vec`, sem ponteiros. |
| AST | Enum Rust espelhando `Parser/Python.asdl`, com posições (lineno, col, end_lineno, end_col) | Tracebacks de 3.13 sublinham o trecho (`~~~^^^`) usando as posições finais; precisa delas desde o início. |
| Execução | Compilador para bytecode próprio (não o de CPython) + VM de pilha | Avaliador de AST seria mais rápido de escrever, mas: (a) checkpoint do escalonador do pseudo-linus fica natural a cada instrução, como no ul-jq por nó; (b) laços profundos sem recursão Rust evitam estouro de pilha e permitem `RecursionError` idêntico (limite 1000 contado por frame Python); (c) a tabela de linhas por instrução dá o `lineno` do traceback de graça. Não copiamos os opcodes do CPython porque `dis` e `.pyc` estão fora do escopo. |
| Objetos | `Rc<RefCell<...>>` com enum `Value` (int pequeno inline, `BigInt` próprio, str, etc.) e tipos como objetos com MRO | `forbid unsafe` impede o modelo `PyObject*`. Contagem de referência por `Rc` reproduz a ordem determinística de finalização (fechamento de arquivo, flush) que o CPython tem; ciclos ficam para um coletor simples depois. `str` guarda código-pontos (com representação compacta latin1/ucs2/ucs4 opcional) porque `len`, indexação e `repr` são por código-ponto. |
| Inteiros e floats | `int` arbitrário próprio; `float.__repr__` com o algoritmo de menor representação (port do `dtoa.c` `_Py_dg_dtoa` modo 0) | Saída byte a byte de `repr(0.1)` e `json.dumps` de floats. Reaproveitar a formatação de float que o ul-jq ou ul-awk já tenham, se for a mesma (`%.17g` NÃO é). |
| Exceções e tracebacks | Exceções são objetos Python; o traceback é montado pelo port de `Python/traceback.c` lendo o texto fonte | Formato `Traceback (most recent call last):`, `File "<string>", line N, in <module>`, linha de código e marcadores de 3.13. Para `-c` o CPython 3.13 mostra a linha do fonte; precisa reter o texto. Código de saída 1 em exceção, 2 em erro de uso, 120 em falha de flush do stdout. |
| Módulos embutidos | Escritos em Rust (sys, io, csv, json), registrados numa tabela de nomes | Evita depender de `Lib/` em Python, que exigiria a linguagem completa. `csv.py`/`json/` que são finos em Python viram Rust também. |
| I/O | Tudo via `sysabi` (`Ctx`), como os outros crates | Mesmo sandbox, mesmos sinais, mesmo relógio. |

Registro no userland, igual ao `ul-jq` (`crates/ul-jq/src/lib.rs`):

```rust
pub fn programs() -> Vec<Program> {
    vec![Program::bin("python3", python3_main), Program::bin("python3.13", python3_main)]
}
```

e `ul_python::programs()` adicionado no mesmo agregador que hoje junta `ul_jq::programs()` e
`ul_awk::programs()` (mais a dependência no `Cargo.toml` do workspace). O front-end que hoje reconhece
os dois programas literais do `csv` passa a delegar para `ul-python` quando a fatia 15 estiver verde.

## 3. Fila de fatias

Cada fatia: arquivos novos, dependência, como testar. "Caso novo" significa um `.toml` em
`testbench/corpus/cases/python/` com saída gerada pelo oráculo.

1. **Esqueleto do crate.** `crates/ul-python/Cargo.toml`, `src/lib.rs` com `programs()` e um
   `python3_main` que trata `-c`, `-V`/`--version` (`Python 3.13.5`, confirmar no oráculo) e erro de
   uso. Sem dependência. Teste: caso `python-version`.
2. **Tokens.** `src/token.rs` (enum de tipos de token, igual a `Lib/token.py`). Depende de 1. Teste
   unitário de nomes.
3. **Tokenizer básico.** `src/tokenizer.rs`: nomes, números, operadores, NEWLINE, INDENT/DEDENT,
   comentários, continuação. Depende de 2. Teste: tabela comparada com `python3 -m tokenize` no oráculo.
4. **Literais de string.** `src/tokenizer/strings.rs`: prefixos, aspas triplas, escapes, bytes. Depende
   de 3. Teste igual a 3.
5. **AST.** `src/ast.rs` espelhando `Python.asdl` com posições. Depende de 1. Teste: compila.
6. **Parser PEG, expressões.** `src/parser/expr.rs`: precedência completa, chamadas com nomeados,
   atributo, subscrição. Depende de 3, 5. Teste: `python3 -c "print('hi')"` após a fatia 10.
7. **Parser PEG, comandos.** `src/parser/stmt.rs`: atribuição, `import`, `for`, `if`, `while`, `def`,
   `return`, `try`. Depende de 6.
8. **Erros de sintaxe.** `src/parser/invalid.rs` (regras `invalid_*` mais comuns) e formato de
   `SyntaxError`. Depende de 7. Teste: casos `python-syntax-*` (parêntese aberto, indentação).
9. **Modelo de objetos.** `src/object/{mod,str,int,list,dict}.rs`: `Value`, tipos, `repr`/`str`,
   igualdade, hash. Depende de 1. Teste unitário de `repr` contra tabela do oráculo.
10. **Compilador e VM mínimos.** `src/compile.rs`, `src/vm.rs`: expressões, chamadas, nomes globais,
    `for` sobre iteradores, builtin `print`. Depende de 7, 9. Teste: `print('hi')`, `print(1, 2,
    sep='-')`.
11. **Exceções e traceback.** `src/exc.rs`, `src/traceback.rs`: hierarquia básica, `raise`, `try`,
    impressão do traceback com a linha fonte. Depende de 10. Teste: `python3 -c "1/0"`,
    `python3 -c "x"` (NameError).
12. **Funções e escopos.** `def`, closures, `return`, argumentos nomeados e padrão. Depende de 10.
    Teste: casos `python-def-*`.
13. **`sys` e `io`.** `src/modules/{sys,io}.rs`: `argv`, `stdin` iterável por linha com nova linha
    universal, `stdout` com buffer, `open` com `encoding`/`newline`, `import`. Depende de 10. Teste:
    `python3 -c "import sys\nfor l in sys.stdin: print(l, end='')"`.
14. **`json`.** `src/modules/json.rs`: `dumps` (`ensure_ascii`, escapes, floats) e `loads` com erros.
    Depende de 9, 11. Teste: casos `python-json-*` contra o oráculo.
15. **`csv.reader`.** `src/modules/csv/reader.rs`: port de `parse_process_char` e `_csv.Error`.
    Depende de 13, 11. Teste: os 16 casos `csv-read-*` de `rfc4180.toml` rodando pelo interpretador.
16. **`csv.writer`.** `src/modules/csv/writer.rs`: `QUOTE_MINIMAL`, `lineterminator`, campo vazio
    único. Depende de 13. Teste: os 5 casos `csv-write-*`.
17. **Troca do front-end.** Remover o reconhecimento literal e apontar `csv` para `ul-python`.
    Depende de 15, 16. Teste: `rfc4180.toml` inteiro verde.
18. **Execução de arquivo e shebang.** `python3 arquivo.py`, `sys.argv[0]`, erro `can't open file`.
    Depende de 13. Teste: rodar o `tool.py` do `file/text.toml` como caso novo `python-script-hi`.
19. **`int` arbitrário e `float` repr.** `src/object/{bigint,float}.rs` com o `dtoa` modo 0.
    Depende de 9. Teste: `print(2**100, 0.1, 1e22)`.
20. **Métodos de `str`.** `split`, `join`, `strip`, `format` simples, f-strings. Depende de 4, 10.
    Teste: casos `python-str-*`.
21. **Comprehensions e `range`/`enumerate`/`zip`/`sorted`/`len`.** Depende de 12. Teste: casos
    `python-builtins-*`.
22. **Checkpoint do escalonador e `RecursionError`.** Checkpoint por instrução na VM, limite 1000.
    Depende de 10. Teste: laço infinito morto por `timeout`, recursão profunda com mensagem igual.
23. **Saída e flush.** `sys.exit` com int/str/None, código 120 em `BrokenPipe` ao descarregar,
    `KeyboardInterrupt` em SIGINT (código 130). Depende de 11, 13. Teste: `python3 -c "print('x')" |
    head -c0` e `sys.exit('msg')`.

Fatias que podem andar em paralelo desde o início: 2-4, 5, 9 (e 19 depois de 9). A cadeia crítica é
3 → 6 → 7 → 10 → 13 → 15/16 → 17.
