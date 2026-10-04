# Procedência do código: crate `vfs`

Nenhum arquivo deste crate tem texto copiado do Linux nem da glibc, e nenhum foi traduzido linha a linha.
Não é sala limpa: a ordem das checagens de cada syscall foi escrita por quem conhece o código do kernel e,
em `namei.rs` e `ops.rs`, a divisão em funções segue a do `fs/namei.c` e do `fs/open.c` de memória (os
comentários citam os nomes: `link_path_walk`, `lookup_last`, `filename_create`, `may_delete`, `may_open`,
`do_renameat2`, `vfs_rename`). Toda essa ordem foi conferida contra o kernel real pelo teste diferencial
(`tests/differential.rs`, ~207 mil operações contra o tmpfs do host), que é a especificação independente.

| Arquivo | Classe | Fontes |
|---|---|---|
| `src/lib.rs`, `src/fs.rs`, `src/mount.rs` | original | design v2 |
| `src/types.rs` | comportamento | constantes de limits.h e do E05; `makedev`/`major`/`minor` reproduzem a codificação do `dev_t` da ABI do glibc (fórmula de bits, makedev(3)) |
| `src/perm.rs` | comportamento | path_resolution(7), capabilities(7) (CAP_DAC_OVERRIDE, CAP_DAC_READ_SEARCH, CAP_FOWNER, CAP_FSETID), E05 (root com modo 000); estrutura própria |
| `src/namei.rs` | comportamento, estrutura perto do kernel | path_resolution(7), symlink(7) (40 links), E04 e E05; a divisão `walk_parent`/`resolve_last` corresponde à do `fs/namei.c` |
| `src/ops.rs` | comportamento, ordem de errnos perto do kernel | open(2), stat(2), access(2), mkdir(2), mknod(2), symlink(2), link(2), unlink(2), rmdir(2), rename(2), readlink(2), chmod(2), chown(2), utimensat(2), truncate(2), statfs(2), chdir(2), getcwd(3); a sequência de checagens de `open`, `rename`, `unlink` e `link` segue a das funções do kernel citadas nos comentários |
| `src/tmpfs.rs` | original (estrutura do E03) + comportamento | E03 (imbl, blocos de 4 KiB, snapshot), tmpfs(5); tamanho de diretório 40 + 20 por entrada e ordem do readdir medidos no host |
| `src/procfs.rs` | comportamento | proc(5), formatos de `testbench/golden/linux-facts/proc/` |
| `tests/*` | original | |

## Recomendação

O crate fica MIT por enquanto (decisão do dono). Se o dono quiser sala limpa estrita, `ops.rs` e `namei.rs`
podem ser reescritos a partir das man pages com o teste diferencial e os cenários de `tests/basic.rs` como
especificação (eles medem o kernel real, não leem o código dele).
