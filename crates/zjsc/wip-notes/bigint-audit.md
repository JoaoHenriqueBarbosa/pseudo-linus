# Auditoria de BigInt contra o bun 1.4.2

## O que foi criado

- `scripts/gen-bigint-bun-golden.js`: gera `tests/golden/bigint_bun.tsv` rodando no bun.
  O nome tem `-bun-` porque `scripts/gen-bigint-golden.js` já existe e gera `tests/golden/bigint.tsv` (outro golden,
  de aritmética aleatória em várias bases, usado por `tests/bigint_golden.rs`); os dois convivem.
- `tests/golden/bigint_bun.tsv`: 11289 programas (uma linha cada: fonte em JSON, tab, texto de `R` em JSON).
- `tests/bigint_bun_golden.rs`: no padrão de `function_error_bun_golden.rs`. Resultados com mais de 4000 unidades
  UTF-16 saem como `#len<N>` dos dois lados.

Cobertura: literais (hex, octal, binário, separadores, erros de sintaxe pelo `eval` interno), `+ - * / % **` nos
tamanhos de borda de 1 a 1000 bits com negativos e zero, divisão por zero, expoente negativo, shifts `<< >> >>>`,
bitwise em complemento de dois, comparação com Number e String, mistura de tipos (mensagens de TypeError),
`BigInt(valor)`, `asIntN`/`asUintN` (bits 0 a 200 e argumentos estranhos), `toString(radix)`, `toLocaleString` (en,
pt-BR, de, ar, hi e outros), Number(bigint) com arredondamento, parseInt, typeof, TypedArrays de 64 bits (set, wrap,
Atomics, DataView), JSON, `++`/`--`, `Math.*`, `Object.is`, Map/Set, `1n << 1000000n`, `10n ** 100000n` e
`BigInt('9'.repeat(100000))` (só comprimento e pontas).

## Cuidados do gerador

- `BigInt.asUintN(1073741823, -1n)` e variantes com 2^30 bits ou mais travam o bun por mais de um minuto; ficaram de
  fora (o limite exato fica coberto por `1n << 4294967296n`, `asUintN(4294967296, ...)` e `asIntN(2**53 - 1, ...)`).
- Programas que lançam fora do `try` (erro de sintaxe) não gravam `R` e são descartados; os literais passam por
  `eval` para o erro ser capturado e comparado.
- `BigInt.prototype.toJSON` do caso de JSON é apagado depois de cada programa para não vazar.
- Nenhum resultado contém caminho da máquina (conferido com grep de `/home`).

## Leitura contra `upstream/JavaScriptCore/runtime/JSBigInt.cpp`

Tempo curto (limite de 5 minutos), então a leitura foi por amostragem dos pontos onde o bun e o porte costumam
divergir. O porte (`js_big_int*.rs`, 8480 linhas) é tradução linha a linha. Conferidos e iguais:

- `MAX_LENGTH_BITS = 1 << 30` e `MAX_LENGTH` (JSBigInt.h:527).
- `exponentiate`: `exp_value >= MAX_LENGTH_BITS` falha com RangeError (cpp:672).
- Conversão de contagem de shift/asIntN (`value > MAX_LENGTH_BITS`, cpp:7420).
- `tryAllocateCell` aceita `resultLength == maxLength + 1` (cpp:2760, comentado no porte).

Nenhuma divergência óbvia encontrada, nenhum código de runtime alterado. Os testes não foram rodados (regra desta
tarefa: sem cargo); o primeiro `cargo test --test bigint_bun_golden` dirá onde o porte difere do bun.
