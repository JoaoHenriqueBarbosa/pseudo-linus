# E04: confinamento e semântica de caminho do hostfs

## Hipótese

**H18** (v1): `openat2` com `RESOLVE_BENEATH` ou `cap-std` impedem qualquer fuga do hostfs.

Critério: zero fugas na suíte (inclusive corrida de troca de diretório por symlink com 1 milhão de
tentativas) pra cada candidato, e errno igual ao Linux.

## Método

Tudo dentro de `testbench/scratch/e04/run` (gitignored), recriado a cada execução (`src/fixture.rs`):

- `outside/canary.txt`: o canário. Ler esse conteúdo a partir do sandbox é fuga.
- `jail/`: o diretório do host montado em `/work` no sandbox, com armadilhas: symlink absoluto e
  relativo pra fora, `..` encadeado, symlink pra `/etc/passwd` e pra `../etc/passwd`, symlink absoluto
  que aponta pra dentro do namespace do sandbox (`/work/file.txt`), magic links (`/proc/self/root/...`,
  `/proc/self/fd/N`), loop, cadeias de 40 e 41 symlinks, nome de 256 bytes, caminho de 5200 bytes,
  arquivo usado como diretório, barra final, e um hardlink pré-existente pro canário.
- `refroot/`: **a referência do que o Linux responde**. É o namespace do sandbox materializado de
  verdade (`etc/passwd` do sandbox, `tmp/`, cópia da jaula em `work/`) e resolvido pelo próprio kernel
  com `RESOLVE_IN_ROOT`, que é a semântica de chroot. Assim o "errno igual ao Linux" é medido contra o
  kernel, não contra uma tabela escrita à mão.

`src/vfs.rs` tem um VFS de brinquedo com o namespace do sandbox (tmpfs com `/etc/passwd` próprio e o
hostfs em `/work`, cwd = `/work`) e cinco implementações de hostfs:

| Candidato | Como resolve dentro da montagem |
|---|---|
| `cap-std` 4.0.3 | delega o resto do caminho pro `Dir::open` (openat2 BENEATH por baixo) |
| `openat2` BENEATH | delega pro kernel com `RESOLVE_BENEATH \| RESOLVE_NO_MAGICLINKS` |
| `openat2` IN_ROOT | delega pro kernel com `RESOLVE_IN_ROOT \| RESOLVE_NO_MAGICLINKS` |
| namei manual | o nosso namei resolve componente por componente; o hostfs só faz `openat(O_PATH\|O_NOFOLLOW)` de um nome, `fstat`, e `readlinkat` no próprio fd; symlink volta pro namei do sandbox; `..` desempilha o fd pelo qual a resolução passou |
| namei híbrido | igual ao manual, mas antes tenta o resto do caminho numa syscall com `RESOLVE_BENEATH \| RESOLVE_NO_SYMLINKS \| RESOLVE_NO_MAGICLINKS`; se o kernel recusar (ELOOP por symlink, EXDEV por `..` acima do ponto de partida), segue componente a componente |

Corrida: uma thread troca `jail/race` (diretório com um `canary.txt` legítimo) e `jail/race_alt`
(symlink pro diretório de fora) com `renameat2(RENAME_EXCHANGE)` em laço, enquanto o candidato tenta ler
`race/canary.txt` 1 milhão de vezes.

## Resultado (rodada de 2026-10-02, máquina carregada por outros builds)

| Candidato | Fugas (estáticas + corrida) | Igual ao Linux | Latência 1 / 4 / 16 componentes |
|---|---|---|---|
| cap-std | 0 + 0 | 18/29 | 6,8 / 8,8 / 12,0 µs |
| openat2 BENEATH | 0 + 0 | 18/29 | 4,3 / 4,7 / 5,7 µs |
| openat2 IN_ROOT | 0 + 0 | 25/29 | 4,3 / 4,7 / 5,7 µs |
| namei manual | 0 + 0 | **29/29** | 6,4 / 12,2 / 36,2 µs |
| **namei híbrido** | 0 + 0 | **29/29** | 4,4 / 4,7 / 5,8 µs |

As latências incluem o VFS de brinquedo e a leitura do arquivo; servem pra comparar os candidatos entre
si.

Onde os candidatos que delegam erram (todos sem fuga, todos com semântica errada):

- **cap-std**: 11 casos viram um erro próprio, "a path led outside of the filesystem", **sem errno**:
  `..` acima da montagem, qualquer symlink absoluto, symlink relativo que sobe da montagem, magic links.
  Inclui casos em que o Linux devolveria **conteúdo**: `dir/../../etc/passwd`, um symlink pra
  `/etc/passwd` e um symlink pra `/work/file.txt` deveriam ler o `/etc/passwd` do sandbox e o próprio
  arquivo da montagem.
- **openat2 BENEATH**: os mesmos 11 casos dão `EXDEV`, um errno que um processo no Linux nunca recebe
  num `open`.
- **openat2 IN_ROOT**: trata a montagem como raiz de um chroot. Acerta os `ENOENT` por coincidência, mas
  erra os 4 casos que deveriam sair da montagem pro resto do namespace: os dois symlinks pra
  `/etc/passwd`, o `..` até `/etc/passwd` e o symlink absoluto pra `/work/file.txt`.

## Veredito

**H18: parcial.**

- Contra fuga, a hipótese se confirma: zero leituras do canário em todos os candidatos, inclusive com 1
  milhão de tentativas de corrida cada um.
- Pelo critério de errno igual ao Linux, ela é refutada pra cap-std e `openat2` BENEATH, e também pro
  IN_ROOT.

O problema não é de segurança, é de desenho. Quando a resolução é delegada ao kernel do host, symlinks e
`..` passam a ter a semântica do host. Mas o hostfs é só uma montagem dentro do namespace do sandbox, e
um symlink lá dentro tem que ser resolvido no namespace do sandbox.

**Recomendação para o design:** o hostfs não segue symlink. Ele faz lookup de um nome por vez
(`O_PATH | O_NOFOLLOW`) e devolve symlinks pro namei do nosso VFS, que mantém a pilha de diretórios pra
`..` e o limite de 40 links. No caminho rápido, um único `openat2` com `RESOLVE_BENEATH |
RESOLVE_NO_SYMLINKS | RESOLVE_NO_MAGICLINKS` resolve o caso comum (sem symlink) numa syscall; se o kernel
recusar, o namei componente a componente assume. Tudo isso com API segura do `rustix`, sem cap-std.

**Hardlink pré-existente pro canário** é lido por todos e não conta como fuga. É um objeto que o dono
colocou dentro do diretório montado, e a resolução de caminho não tem como saber disso. A defesa é de
política: não montar diretório que contenha hardlink pra fora, ou montar só leitura.
