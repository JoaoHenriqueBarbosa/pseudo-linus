# Auditoria do golden de Math (2026-10-08)

Golden: `tests/golden/math_bun.tsv`, gerado por `scripts/gen-math-golden.js` no bun 1.4.2
(`bun scripts/gen-math-golden.js > tests/golden/math_bun.tsv`). Agora 21078 casos (eram 9496).

## Cobertura por função (antes: 200 cada, só uma lista comum de 200 valores)

O que faltava: potências de dois sistemáticas, múltiplos de pi/2, valores no redutor de argumento,
pontos de corte do glibc por função, domínios próprios (asin/acos/atanh em [-1,1], acosh >= 1, log1p > -1).

Casos adicionados, depois dos antigos (o prefixo de 9496 linhas é idêntico ao golden anterior):

- sin, cos, tan (1001 cada): 2^k de -1074 a 1023, k*pi/2 para k de -40 a 40 e mais de 25 valores grandes
  (2^19 pi/2, 2^20 pi/2, 1e6 a 2^52 vezes pi/2, caminho Payne-Hanek), cada um com vizinho acima e abaixo
  em ulp, limiares do rem_pio2 (pi/4, 3pi/4, 5pi/4, 9pi/4, 2^27, 2^28), faixas aleatórias até 1e308.
- asin, acos, atanh, acosh, atan, asinh, sinh, cosh, tanh, exp, expm1, log, log2, log10, log1p, cbrt, fround
  (500 a 800 cada): 300 casos aleatórios por domínio e escala mais os cortes: exp em 709.78 e -745.13,
  expm1 em 56 ln2, sinh/cosh em 22 e 710.47, tanh em 22 e 2^-55, acosh em 2^28, atanh em 1-2^-53, log1p em
  sqrt(2)-1 e 2^-54, log com 2^k e 0.7071, log10 com 10^k, cbrt com 2^k e cubos exatos, fround nos limites
  de float32 (overflow, subnormal, meio de ulp).
- clz32 (687): 2^k, 2^k-1, 2^k+1, negativos, não inteiros, NaN, infinitos, 2^32 e vizinhos.
- atan2 (977): 13 x 17 de eixos, subnormais, razões extremas, mais 300 aleatórios em escalas de 2^-100 a 2^100.
- pow (1763): 400 aleatórios (base 2^k, positiva, negativa; expoente inteiro e fracionário), mais
  base perto de 1 com expoente grande (2^53, 1e15), limites de overflow e underflow (308 a 1074).
- hypot (1376): 300 pares em escalas de 2^-1074 a 2^1023, 300 trios, 150 de quatro e cinco argumentos,
  mais 0, 1, NaN, infinito, 9 argumentos.
- imul (756): 300 casos com 32 bits, 2^k até 2^39, fracionários e 0xffffffff.
- sumPrecise (213, eram 80): 120 listas de 0 a 11 elementos (com e sem pares de sinal oposto, subnormais,
  1e308 e -1e308) e 13 casos fixos (vazio, -0, overflow intermediário, 1e100 cancelando, Infinity e -Infinity).

Todos com no mínimo 200. O teste (`tests/math_bun_golden.rs`) passou a aceitar coluna de argumentos vazia
(`hypot()` e `sumPrecise([])`): o filtro `!hex.is_empty()`.

NÃO rodei cargo (regra da tarefa): os casos novos nunca foram comparados com o porte; divergências que o
teste mostrar na próxima rodada são bugs reais nas funções `glibc_*.rs` ou em `operations`.

## Auditoria de delegação a f64/std (2026-10-08, por leitura)

Resultado: nenhuma função libm de `Math.*` delega mais ao `f64` do Rust. Mapa:

- `exp`, `log`, `log2`, `pow`: `glibc_math.rs` (e_exp, e_log, e_log2, e_pow, variante FMA, tabelas em
  `glibc_math_data.rs`). Nada a portar.
- `expm1`, `log1p`, `sinh`, `cosh`, `tanh`, `cbrt`, `log10`, `asinh`, `acosh`, `atanh`: `glibc_hyper.rs`.
- `sin`, `cos`: `glibc_trig.rs`; `tan`: `glibc_tan.rs`; `atan`, `atan2`: `glibc_atan.rs`; `asin`, `acos`:
  `glibc_asin.rs`; `hypot` (2 argumentos): `glibc_hypot.rs` (a nota abaixo sobre `f64::hypot` está obsoleta).
- `fround`: `x as f32 as f64`, conversão IEEE com arredondamento ao mais próximo, exata (não é libm).
- Ordem de avaliação: `Math.pow` (`math_proto_func_pow`) e `**` (`js_pow`) chamam ambos
  `operation_math_pow`, que segue `operationMathPow` (NaN, |base|==1 com expoente infinito, +-0.5 por
  sqrt, expoente inteiro pequeno por multiplicação, senão `glibc_math::pow`). Mesmo caminho, sem divergência.
- Único resíduo corrigido agora: as constantes `E`, `LOG2E`, `LOG10E`, `LN2`, `LN10` de `Math` eram
  calculadas com `f64::exp/ln/log2/log10` (libm do host); passaram a usar `glibc_math::exp/log/log2` e
  `glibc_hyper::log10`, como o upstream (`Math::exp(1.0)` etc.). Valores no bun: E 2.718281828459045,
  LOG2E 1.4426950408889634 (= `log2(E)`), LOG10E 0.4342944819032518 (= `log10(E)`).
- Fora de Math (não afeta o golden): `powf/powi/log10` em `heap.rs`, `date_math.rs`, `intl_plural_rules.rs`
  e `js_big_int_ops.rs` ainda usam std; conferir só se virarem observáveis.
- Sem rede, tabelas: não foi preciso, já existem. Nada compilado nem rodado.

## Math.hypot e Math.sumPrecise

O bun 1.4.2 tem `Math.sumPrecise` (medido: `typeof` é `function`).

`operations::hypot` em `src/runtime/math_object.rs` foi comparado linha a linha com
`upstream/JavaScriptCore/runtime/MathObject.cpp` (`mathProtoFuncHypot`): 0 argumentos devolve 0; 1 devolve
`fabs`; 2 devolve infinito se algum for infinito, senão `hypot`; 3 devolve infinito, depois NaN, depois
`std::hypot(x, y, z)` (ramo `#else`, libc++ >= 20.1 ou libstdc++); 4 ou mais é o laço com `maxAbs` e
`sum`, igual ao upstream (inclusive o `fma` nas duas atualizações). Nenhuma diferença por leitura.

Pontos a vigiar quando o teste rodar:

- O ramo de dois argumentos usa `f64::hypot` do Rust, que chama a libm do processo, não um `glibc_hypot.rs`
  (não existe). Se a libm do host divergir do glibc do bun em algum ulp, o golden de hypot (1376 casos) acusa.
- O de três argumentos reproduz o `__hypot3` do libstdc++ (escala pelo maior, soma dos quadrados, `sqrt`).
  Se o bun usar uma versão com `fma`, os trios novos revelam a diferença de 1 ulp.

`sum_precise_body` espelha as três mensagens de erro do upstream (nulo, estouro de iterações, não número).
O C++ escolhe `XsumSmall` ou `XsumLarge` pelo comprimento; o porte usa um acumulador exato único, o que dá
o mesmo resultado, já que ambos são exatos e arredondam uma vez só.

## Porte glibc de `tan` (`src/runtime/glibc_tan.rs`)

- Feito: `tan` a partir de `s_tan.c`, `utan.h` e `utan.tbl` do glibc 2.41, ligado em `math_object.rs`
  (`Math.tan`). A 2.41 não tem mais o caminho lento com `mpa` em `s_tan.c`; todo argumento termina nos ramos
  I a XI.
- Tabela `xfg` (186 linhas, sem a coluna FFi, que `__tan` não usa) em `glibc_tan_table.rs`, GERADA POR SCRIPT
  (Bash, geração em massa): `scripts/gen-glibc-tan-table.py /tmp/glibc-src/utan.tbl
  src/runtime/glibc_tan_table.rs`.
- DRY: reusa `emulv`/`eadd` de `glibc_atan.rs` e `reduce_sincos`/`branred` de `glibc_trig.rs`, que passaram a
  `pub(super)` (um `sed -i` de visibilidade por arquivo, via Bash). A redução do algoritmo ii do tan é
  aritmeticamente a de `reduce_sincos`, seguida de EADD. `DIV2` de `dla.h` virou `div2`; os ramos VI a XI
  compartilham `tan_reduced`.
- Testes: 24 casos de `tests/golden/math_bun.tsv` (linhas `tan`) mais especiais. Nada compilado nem rodado
  (regra da tarefa). Mesma ressalva do FMA dos outros portes: sem contração nos polinômios.

## Porte glibc de `hypot` de dois argumentos e conferência do `%`

- Feito: `__ieee754_hypot` (`e_hypot.c` do glibc 2.41) em `src/runtime/glibc_hypot.rs`, ligado em
  `operations::hypot` (ramo de dois argumentos, depois do teste de infinito). O build genérico do x86_64
  não define `__FP_FAST_FMA`, então foi portado o `kernel` do `#else` (correção de Borges), sem `fma`. Se o
  golden de hypot acusar 1 ulp em algum par, a hipótese a testar é o bun rodar uma variante `fma` (a
  multiarch do x86_64 tem ifunc para outras funções, não confirmei para `hypot`). Os fatores `SCALE`,
  `LARGE_VAL`, `TINY_VAL` e `EPS` são construídos com `f64::from_bits`.
- Teste: `matches_bun_golden` lê as linhas `hypot` de `tests/golden/math_bun.tsv` (2 argumentos). Nada
  compilado nem rodado (regra da tarefa).
- `%`: `js_remainder` (`runtime/operations.rs`) usa `left % right` em `f64`, que o Rust compila como
  `fmod` exato do C (o resultado de `fmod` é exato por definição, sem divergência de libm). Medido no bun:
  `5e-324 % 3` = `5e-324`; `1e308 % 1e-308` = `3.498445546245627e-309` (0x00028401cf53d610);
  `-0 % 5` = `-0`; `Infinity % 3` = `NaN`; `5 % Infinity` = `5`; `5.5 % -0` = `NaN`; `-5 % 3` = `-2`;
  `-5e-324 % 5e-324` = `-0`. O sinal segue o dividendo, como no `%` do Rust. Conferência só por leitura
  (sem teste novo).
