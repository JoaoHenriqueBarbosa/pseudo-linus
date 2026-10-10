# Desenho: o `node:fs` do bun no pseudo-linus (trait de arquivos com escrita)

Pergunta da coordenação: qual trait de hospedeiro dá escrita em arquivo ao `fs` do bun sobre o zjsc, sem mexer às
cegas em kernel e vfs. Esta nota responde com medição e trade-offs; nada foi implementado. Contexto geral em
`bun-on-zjsc-plan.md` (seções 7 e 8).

## 1. Achado: não falta API de arquivo no kernel nem no vfs

O `sysabi::Syscalls` (`crates/sysabi/src/sys.rs`, 995 linhas) já expõe a superfície POSIX que o `node:fs` usa, e o
processo corrente da thread vem de `sysabi::sys::current()` (um `Arc<dyn Syscalls>` instalado pelo kernel ou pelo
testkit). Cobertura medida, por grupo do `fs` do bun (108 chaves entre síncrono, callback e `promises`):

| Grupo do `fs` | Syscall do `sysabi` | Situação |
|---|---|---|
| `open`, `close`, `read`, `write`, `readv`, `writev` | `openat`, `close`, `read`, `write`, `pread`, `pwrite`, `lseek` | pronto (`readv`/`writev` por laço de `pread`/`pwrite`) |
| `stat`, `lstat`, `fstat`, `statfs` | `fstatat`, `fstat`, `statfs`, `fstatfs` | pronto; `Stat` tem `atime`, `mtime`, `ctime` e `btime: Option` |
| `readdir` (com `withFileTypes`), `opendir` | `getdents` (`DirEntry` com `kind`) | pronto |
| `mkdir` (com `recursive`), `rmdir`, `rm`, `unlink` | `mkdirat`, `unlinkat` (com `AT_REMOVEDIR`) | pronto; `ul_common::fsutil::mkdir_p` já existe |
| `rename`, `link`, `symlink`, `readlink` | `renameat2`, `linkat`, `symlinkat`, `readlinkat` | pronto |
| `chmod`, `fchmod`, `chown`, `lchown`, `utimes`, `futimes`, `lutimes` | `fchmodat`, `fchmod`, `fchownat`, `utimensat`, `futimens` | pronto |
| `truncate`, `ftruncate`, `fsync`, `fdatasync`, `fallocate` | `ftruncate`, `fsync`, `fallocate` | pronto (`fdatasync` usa `fsync`) |
| `access`, `exists` | `faccessat` | pronto |
| `realpath` | nenhuma | falta; derivar por `readlinkat` ao longo do caminho (componente a componente) em helper do `ul-common::fsutil`, sem syscall nova |
| `copyFile`, `cp` | nenhuma (`copy_file_range`, `sendfile`, `splice` não existem) | por laço de `read`/`write` com `fstat` para o modo |
| `mkdtemp` | `mkdirat` | pronto (sufixo aleatório no zjsc) |
| `watch`, `watchFile` | nenhuma (`inotify` não existe) | falta; `watchFile` por polling de `fstatat` é viável; `watch` pede `inotify` no kernel |
| `createReadStream`, `createWriteStream` | `read`/`write` com fd | pronto sobre `node:stream` (já no zjsc) |

Também já existem, para os degraus seguintes: `pipe2`, `spawn`, `wait4`, `kill`, `poll`, `epoll_*`, sockets TCP, UDP e
Unix, `net_connect` com a política de rede do sandbox, `chdir`, `getcwd`, `umask`, `getrlimit`.

Conclusão: o primeiro `fs` não toca kernel nem vfs. As lacunas (`realpath`, `copyFile`, `watch`) são resolvidas em
userland, exceto `watch`, que é a única que pediria feature de kernel (`inotify`), e que fica fora do primeiro corte.

## 2. As opções de desenho

**A. O zjsc chama `sysabi::sys::current()` direto nas funções nativas do `node:fs` (recomendada).** O crate `zjsc`
passa a depender do `sysabi` por caminho (ele já depende do `ul-common`, que só depende do `sysabi`, então o
`sysabi` já está no grafo). Os testes rodam dentro do pseudo-processo do `testkit`
(`sysabi = { features = ["testkit"] }` como dev-dependency), do mesmo jeito que o `ul-chromium` e o `ul-python`
testam. Consequências:
- Cumpre a regra DRY do projeto: não nasce um trait espelho de 35 métodos cujo único implementador só repassa
  para o `Syscalls` (isso seria exatamente a "função de repasse" que o `CLAUDE.md` proíbe).
- Erros e fds são os do kernel simulado, sem camada de tradução: `fs.openSync` devolve o próprio número do fd do
  pseudo-processo, então `fs.writeSync(1, ...)` e `process.stdout` enxergam o mesmo descritor.
- O zjsc deixa de ser um motor sem noção de processo: quem o embute sem pseudo-processo (um teste unitário
  isolado) precisa instalar um. Mitigação: o registro de módulos é por perfil (ver 4), e o perfil de navegador,
  que não tem `fs`, continua sem depender do processo.

**B. Um trait novo `HostFs` no zjsc, implementado pelo `ul-bun` sobre o `Syscalls`.** Mantém o zjsc independente de
processo e deixa testar com um `MemoryFs` com escrita. Consequências: um trait de uns 35 métodos duplicando o
`Syscalls`; cada método do implementador é um repasse (viola a regra DRY); duas superfícies para manter em
sincronia a cada syscall nova; os erros passam por uma tradução a mais. O ganho (testar sem pseudo-processo) já é
atendido pelo `testkit` na opção A.

**C. Crescer o `ModuleFs` existente com métodos de escrita.** Descartada: o `ModuleFs` é por texto UTF-8 ("não UTF-8
vira `None`"), sem fd, sem modo, sem `errno`. O `node:fs` precisa de caminhos em bytes, fds, modos e do código de erro,
então ele viraria o trait B por outro nome.

`ModuleFs` e `ConsoleHost` ficam como estão para o carregador de módulos e o `console`; no bun do pseudo-linus o
`ul-bun` os implementa sobre `Syscalls` (`ConsoleHost::write_stdout` vira `write(1, ...)`, o que também faz
`bun x.js > out.txt` funcionar dentro do sandbox).

## 3. Semântica que o desenho precisa fixar (a medir no bun 1.4.2 antes de codar)

- **Mensagens de erro são as do libuv, não as da glibc.** `Errno::message` devolve o `strerror` da glibc ("No such
  file or directory"); o bun devolve o texto do libuv em minúsculas: `ENOENT: no such file or directory, open
  '/nope/x'` com `errno -2`, `code "ENOENT"`, `syscall "open"` e `path "/nope/x"` (medido). Precisa de uma tabela
  `errno` para (`code`, `errno` negativo, texto libuv) e de um golden por erro comum (`ENOENT`, `EACCES`, `EEXIST`,
  `ENOTDIR`, `EISDIR`, `ENOTEMPTY`, `EBADF`, `EINVAL`, `EMFILE`, `EPERM`, `EXDEV`, `ELOOP`, `ENAMETOOLONG`).
- **Caminho em bytes.** Aceitar string, `Buffer`, `Uint8Array` e `file:` URL; string com par substituto solto: medir
  a codificação que o bun usa antes de decidir (UTF-8 com substituição, ou WTF-8).
- **Assíncrono.** Callback e `promises` não podem rodar a syscall numa thread concorrente sem perder
  determinismo. Opção recomendada: executar a syscall no ato da chamada e enfileirar a conclusão como macrotarefa na
  fase de entrada e saída do laço, depois dos timers vencidos (como o libuv). A ordem entre `fs` assíncrono e
  `setTimeout(0)`/`setImmediate` é o que o golden mede; se divergir do bun, a fase muda sem mexer no desenho. Para
  `child_process` e `net`, o laço passa a bloquear em `Syscalls::poll`/`epoll_wait` no lugar do `park` de hoje.
- **`process.cwd()` e caminhos relativos** passam a vir do `getcwd` e do `Fd::CWD` do kernel, em vez de variáveis
  do embedder.
- **Permissão e `umask`** são as do kernel simulado, o que casa com o oráculo (o Debian real).
- **`Bun.file`/`Bun.write`** usam a mesma camada, com `Blob` (já no zjsc) apoiado em arquivo.

## 4. Perfil: `fs` só no bun, não no navegador

O mesmo zjsc é embutido pelo `ul-chromium` (que já depende de `zjsc` e `sysabi`) e pelo futuro `ul-bun`. O
`require("fs")` não existe no navegador. O `BUILTINS` hoje é uma tabela estática única (`builtin_modules.rs`); precisa
virar por perfil (`Bun` e `Browser`), com o perfil escolhido pelo embedder antes do primeiro programa. Sem isso o
`ul-chromium` ganharia um `require("fs")` visível, o que seria vazamento de simulação.

## 5. O que muda onde (se a opção A for aprovada)

- `wip/zjsc/Cargo.toml`: dependência `sysabi` por caminho; `sysabi` com `testkit` em `dev-dependencies`.
- `wip/zjsc/src/runtime/node_fs/` (novo): tabela `errno` do libuv, conversão de argumentos de caminho, as funções
  síncronas, depois callback e `promises`, depois `createReadStream`/`createWriteStream`. Instalador em
  `BUILTINS` (`fs`, `fs/promises`) no perfil `Bun`.
- `crates/ul-common/src/fsutil.rs`: `realpath` e `copy_file` sobre o `sysabi`. Conferido: o único `canonicalize`
  do repositório é o do uucore vendorizado (`crates/ul-coreutils/vendor/src/uucore/src/lib/features/fs.rs`), que usa
  `std::env::current_dir` e `std::fs`, não o processo simulado, então não serve ao zjsc como está. O `realpath` novo
  nasce no `ul-common` e fica disponível para o `realpath`/`readlink -f` do coreutils se um dia saírem do uucore.
- Kernel e vfs: nada no primeiro corte. `inotify` só quando o `fs.watch` for pedido.
- Oráculo: o bun 1.4.2 já está no oráculo (`ff5d46fb`); os goldens de `fs` saem de lá, com arquivos num diretório
  temporário do próprio oráculo (nada do home de quem desenvolve em fita ou golden).

## 6. Pergunta que preciso que a coordenação decida

Opção A (recomendada) ou B. A decide se o zjsc passa a depender do `sysabi`. O resto do desenho (erros do libuv,
macrotarefa para o assíncrono, perfis) vale para as duas. Enquanto isso não houver decisão, sigo com os `Bun.*`
pequenos e puros e com `util`, `assert`, `url`, `querystring` e `timers`, que não dependem desta escolha.
