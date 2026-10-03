# Licença: estado e decisão pendente

O workspace declara `license = "MIT"` por padrão, mas parte do código foi derivada de projetos GPL/LGPL
durante a implementação. Cada crate derivado declara a licença verdadeira no próprio `Cargo.toml` e tem
um `PROVENANCE.md` com a origem arquivo por arquivo.

## Inventário (atualizado pelos agentes)

| Crate | Origem derivada | Licença declarada |
|---|---|---|
| `sched` | porte do `kernel/sched/fair.c` do Linux 6.12.101 (GPL-2.0-only), mesmos nomes de função e mesma ordem de passos | GPL-2.0-only |
| `regex-posix` | parser traduzido do `regcomp.c` da glibc/gnulib (LGPL-2.1+), submatches do `regexec.c` (LGPL-2.1+), trechos do `dfa.c` (GPL-3.0+) | LGPL-2.1-or-later (os trechos do dfa.c pedem GPL-3.0+; ver PROVENANCE) |
| `ul-textproc` (grep) | porte do `grep.c`/`dfasearch.c` do GNU grep (GPL-3.0+) | GPL-3.0-or-later |
| `ul-textproc` (sed) | escrito a partir do manual e do oráculo, por quem leu partes do `compile.c`/`execute.c` antes da regra (não é sala limpa estrita) | GPL-3.0-or-later por precaução |
| `kernel`, `vfs` | inventário pedido ao agente; ver os PROVENANCE.md | a confirmar |
| demais `ul-*` | forks de projetos MIT/Apache (uutils, findutils, jaq, gix, posixutils-rs) ou escritos a partir de comportamento | MIT |

## O problema

- GPL-2.0-only (o `sched`, vindo do Linux) e GPL-3.0 (o grep) **não podem ir no mesmo binário**. O
  daemon e o `osh` linkam os dois.
- Com o resto MIT, o binário final seria GPL de qualquer forma enquanto houver código GPL dentro.

## Caminhos

1. **Sala limpa onde há derivação**: reescrever `sched` (a partir do artigo do EEVDF, da documentação
   do kernel e do diferencial contra o kernel do host que o E02 já mede), `regex-posix` e o grep (a
   partir do comportamento, com o corpus da bancada como rede: 682/682 e 9291/9291 no regex, 185/185 no
   grep), por agentes que nunca leram o código GPL. Mantém o projeto MIT.
2. **Projeto inteiro GPL-3.0-or-later**: exige trocar o `sched` (GPL-2.0-only) por versão em sala limpa
   ou obter o código sob licença compatível; o resto entra como está.
3. **Projeto inteiro GPL-2.0-or-later**: exige reescrever o grep e os trechos do dfa.c (GPL-3.0+).

Recomendação do coordenador: caminho 1, porque a bancada já tem as redes de segurança necessárias e o
resultado deixa todas as opções abertas. Decisão do dono.
