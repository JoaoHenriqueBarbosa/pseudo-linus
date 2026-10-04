# Proveniência do crate `shell`

O projeto é MIT. Nenhum código-fonte do bash (GPLv3) foi aberto pra escrever este crate. As fontes
de comportamento são: o manual do bash 5.2, o POSIX, o oráculo da bancada (Debian 13 com bash
5.2.37) e os testes.

## Código de terceiros

- `vendor/brush-parser`: fork do brush-parser 0.4.0 (MIT, https://github.com/reubeno/brush).
  Mudanças descritas em `vendor/brush-parser/FORK.md`.

## Fatos de comportamento vindos de conhecimento prévio

Três detalhes foram implementados a partir do conhecimento prévio de como o bash se comporta, sem
leitura de fonte nesta implementação. Todos são observáveis e estão fixados por teste contra o
oráculo:

1. Ordem de iteração de array associativo: hash FNV-1 de 32 bits (algoritmo público), 1024 baldes,
   item novo no início do balde, crescimento por 4 quando há 2 itens por balde. Teste:
   `vars::tests::assoc_order_matches_bash` e o caso `array-assoc-iteration-order` do golden.
2. `RANDOM`: gerador Park-Miller "minimal standard" (algoritmo público) com a saída
   `(semente >> 16) ^ (semente & 65535)` limitada a 15 bits, sem repetir o valor anterior. Teste:
   `tests/provenance.rs` (sequência com semente conferida no oráculo).
3. Ordem das letras de `$-` (`abefhikmnptuvxBCEHPT`, mais `c`/`s`/`i` conforme a chamada). Teste:
   `tests/provenance.rs`.

## Módulos refeitos em sala limpa

- `src/arith.rs`: uma primeira versão foi escrita por um sub-agente depois de ler o `expr.c` do bash,
  antes da política de licença chegar. Ela foi descartada (nunca entrou no repositório) e o módulo foi
  reescrito por outro agente que nunca abriu o fonte, a partir do manual e de uma bateria de casos
  gerada rodando o bash real.
