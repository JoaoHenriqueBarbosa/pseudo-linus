# pseudo-linus: regras do projeto

## REGRA SUPREMA: nunca entregar evidência de simulação

O pseudo-linus é um sandbox para agentes de IA que precisa ser indistinguível de um Debian 13 de
verdade. Conformidade com o Debian vence qualquer outra consideração: conveniência de
implementação, desempenho, elegância do código, prazo.

Nada que o agente consiga observar pode denunciar que a máquina é simulada. Isso inclui:

- conteúdo de arquivos (`cat /bin/ls` precisa parecer um ELF do Debian, não um stub com marcador,
  nome de crate, caminho de builtin ou a string `PSEUDO-LINUS`);
- tabela de processos (`ps` não mostra laço de controle, scripts internos, nomes de crate);
- mensagens de erro (iguais às do programa real, sem caminhos de arquivos de controle);
- `/proc`, `/sys`, `/etc`, variáveis de ambiente, `uname`, `hostname`, `os-release`;
- nomes de diretórios internos visíveis no sistema de arquivos;
- comportamento de sinais, zumbis, jobs, tempos e códigos de saída.

O que não existe no Debian mínimo responde exatamente como lá (`command not found`, exit 127).
O que existe e ainda não foi portado também não pode revelar a costura: melhor ausente como no
Debian do que presente com cara de imitação.

Achou um vazamento (num cenário com IA, num teste, lendo código)? É bug de prioridade máxima:
corrige antes de seguir, com teste de regressão que garanta que ele não volta.

## Conformidade é medida contra o oráculo

Se o oráculo (o Debian real em Docker da bancada) não tem algo para comparar, melhore o oráculo
antes; nunca exclua caso de teste por limitação dele.

## Fitas e artefatos públicos

O repositório é público. Nada da máquina de quem desenvolve (`CLAUDE.md`, caminhos do home, nome
de usuário, chaves) entra em fita, golden ou fixture versionada. O harness em `example/` recusa
gravar requisições com esse contexto.

## DRY

Duas regras, conferidas à mão com `scripts/dry-check.sh` e `scripts/dry-forwarders.py`.

- **Código parecido não se repete.** O `similarity-rs` compara os corpos de função; par acima de
  0,90 que toque um arquivo alterado é dívida nova.
- **Função de repasse não existe.** Função cujo corpo é só a chamada de outra com os próprios
  argumentos é intermediária inútil: o chamador chama o alvo direto, ou o nome vira reexportação
  (`pub use sysabi::sys::current as sys;`). Acessor de campo privado (`len`, `is_empty`) não é
  repasse; método que acrescenta argumento, prefixo ou conversão também não.

Quando a checagem reclamar, os caminhos que funcionam, em ordem: dar nome ao conceito e extrair a
função que faltava; fundir as duas numa só, com a diferença virando parâmetro (um `const` genérico
serve quando a diferença é de tipo, como em `parse_id::<GROUP>`); escrever a macro; mudar o tipo
para a repetição deixar de ser possível (um `Deref` no newtype, uma tabela no lugar de `match`).

Ajustar o código para a medida cair sem desfazer a repetição (renomear, reordenar, quebrar em dois
para passar raspando) é burlar a régua.

`scripts/dry-baseline.txt` é a dívida que já existia quando a régua chegou, sem números de linha.
Ela só encolhe: a checagem avisa a entrada que deixou de acontecer, e ela sai no mesmo commit que a
resolveu. Entrada nova não se acrescenta à mão; `scripts/dry-check.sh --rebuild` refaz a lista
inteira e só se usa depois de zerar o que ela acusa de novo.

### `ul-common`: o lugar do que mais de um programa usa

`crates/ul-common` é a crate compartilhada de todos os `ul-*`, do `shell` e do `host`. Antes de
escrever qualquer função utilitária, procure nela; existindo, use; faltando, ela nasce lá e não no
programa. DRY aqui é absoluto: a segunda cópia de uma lógica não chega a existir.

| Módulo | O que tem |
|---|---|
| `ctype` | classificação de bytes, `strtol`/`strtoull`/`strtod` com a semântica do glibc, parsers estritos, `cstr`/`cstr_at` |
| `getopt` | o `getopt_long` do glibc |
| `quote` | quotearg do gnulib (dez estilos); as diferenças por programa são `Rules`; `cat_v` |
| `fnmatch` | `fnmatch(3)` genérico no `Alphabet` (byte, `char`, o `Unit` do grep); diferenças por programa são `Flags` |
| `time` | calendário civil (`Civil`), `strftime` com as flags do glibc; `time::zone` (fusos via `jiff`) atrás da feature `zone` |
| `signal` | nomes e parse de sinais; o que cada programa aceita é uma `Table` |
| `codec`, `hash` | base64, crc32, hex; md5, sha1, sha224, sha256 (uma passada e incremental) |
| `width` | `wcwidth` do glibc 2.41 e largura de exibição |
| `fsutil` | caminhos, `mkdir -p`, strings de modo, `size_to_human_string` do util-linux |

Como prosseguir:

1. **Procure antes de escrever.** `grep -rn 'fn NOME' crates/ul-common/src` e, para achar irmãos
   espalhados, `grep -rnE 'fn (NOME|SINÔNIMO)' crates/*/src`. Achou a mesma lógica em outro programa?
   Ela sobe para o `ul-common` no mesmo commit, e as duas cópias passam a usá-la.
2. **A diferença entre programas vira parâmetro, nunca cópia.** Um tipo de opções (`Rules`, `Flags`,
   `Table`), um genérico, uma feature de Cargo para dependência pesada. A função comum reproduz cada
   variação que o Debian tem; a escolha de qual variação fica no chamador.
3. **Variação nova se confere no oráculo.** Quando duas cópias divergem, o Debian real decide qual está
   certa (às vezes as duas, em programas diferentes: o `\` no fim do padrão falha no `fnmatch` do glibc e
   casa literal no gnulib de tar, diff e grep). Um teste no `ul-common` fixa cada variação com o
   programa que a usa no comentário.
4. **Sem repasse.** O chamador importa do `ul-common` direto, ou reexporta com
   `pub use ul_common::x as y;`. Nada de função local de uma linha que só encaminha.
5. **O `ul-common` só depende do `sysabi`** (e de crate externa atrás de feature). Não importa nenhum
   `ul-*`; o que depende de um programa fica no programa. `vfs` e `kernel` não usam o `ul-common`.
6. **Utilitário que só um programa usa** fica no programa, até o segundo aparecer. No dia em que
   aparecer, sobe.

Para caçar duplicação que já existe, os nomes de função repetidos entre crates são um bom ponto de
partida:

```sh
grep -rhoE 'fn [a-z_0-9]+' crates/*/src | sort | uniq -c | sort -rn | head -60
```

```sh
scripts/dry-check.sh              # os .rs do índice
scripts/dry-check.sh ARQ...       # arquivos escolhidos
scripts/dry-check.sh --rebuild    # refaz a lista de dívida (varre tudo, leva minutos)
```

O limiar é mais frouxo que o 0,20 de outros projetos porque, a 0,20, este repositório tem mais de
255 mil pares e a varredura completa leva 2,5 minutos (medido em 2026-10-07, 732 arquivos).
`vendor/`, `staging/`, `tests/` e `benches/` ficam fora.
