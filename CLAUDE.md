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

Duas regras, impostas por hook do git (`.githooks/pre-commit`), não por disciplina. Instale uma vez
por clone com `scripts/install-hooks.sh`.

- **Código parecido não se repete.** O `similarity-rs` compara os corpos de função; par acima de
  0,90 que toque um arquivo do commit recusa o commit.
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

```sh
scripts/dry-check.sh              # o que o pre-commit roda: os .rs do índice
scripts/dry-check.sh ARQ...       # arquivos escolhidos
scripts/dry-check.sh --rebuild    # refaz a lista de dívida (varre tudo, leva minutos)
```

O limiar é mais frouxo que o 0,20 de outros projetos porque, a 0,20, este repositório tem mais de
255 mil pares e a varredura completa leva 2,5 minutos (medido em 2026-10-07, 732 arquivos).
`vendor/`, `staging/`, `tests/` e `benches/` ficam fora.
