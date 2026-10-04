# ul-diff: estado

Dono: agente arquivos. Alvo: GNU diffutils 3.10 (`diff`, `cmp`, `diff3`, `sdiff`) e GNU patch 2.8.

## Pronto

- Esqueleto do crate e `programs()` com os cinco programas.
- `getopt`: getopt_long do glibc 2.41 (permutação, abreviação, mensagens exatas, conferidas no oráculo).
- `sysutil` (E/S sobre sysabi) e `tz` (fuso do sandbox via jiff com tzdata embutida, sem ler o host).

## Em andamento

- `diff`, `sdiff`, `diff3`, `cmp`: motor de alinhamento próprio (Myers de espaço linear) calibrado
  contra o oráculo.
- `patch`: localizador estilo GNU sobre o parser do diffy.

## Placar

A preencher na primeira rodada completa.
