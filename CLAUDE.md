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
