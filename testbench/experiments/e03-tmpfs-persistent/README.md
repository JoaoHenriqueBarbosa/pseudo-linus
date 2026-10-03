# E03: tmpfs persistente

Responde o H17 do `hypotheses.toml` e escolhe a estrutura do tmpfs do design v2 (seção "Sistema de
arquivos"): tabela de inodes como mapa persistente `ino -> Arc<Inode>`, diretório como mapa persistente
`nome -> ino`, conteúdo em blocos de 4 KiB com cópia na escrita, e snapshot, restore e sandbox nova
como clone da raiz.

```sh
cargo test --release --manifest-path experiments/e03-tmpfs-persistent/Cargo.toml
cargo run --release --manifest-path experiments/e03-tmpfs-persistent/Cargo.toml   # grava results/e03-tmpfs-persistent.json
```

O binário refaz tudo sozinho em 1,5 a 2,5 minutos (memória, conformidade, tempos, vazão, depscan) e
grava o JSON com um `CandidateResult` por candidato e o veredito do H17. Os números abaixo são da
última rodada; a máquina estava dividida com outros agentes (load average entre 17 e 34 em 16 núcleos,
registrado em `metrics.loadavg_*`), por isso os critérios se apoiam em medidas determinísticas.

## Hipóteses

**H17** (v1): "tmpfs persistente dá snapshot, restore e sandbox nova em O(1)". Critério: snapshot e
restore com custo constante (não cresce de 1k pra 100k arquivos); custo de escrita pós-snapshot
proporcional à profundidade, não ao tamanho; memória da imagem compartilhada entre sandboxes.

Afirmações do design conferidas junto: hardlink natural (dois nomes, mesmo ino), arquivo desvinculado e
aberto vivo "pelo Arc", blocos de 4 KiB com cópia na escrita pra que 1 byte num arquivo grande não copie
o arquivo todo.

## Método

**Modelo de tmpfs** (`src/fs.rs`, `src/vfs.rs`): criar, ler, escrever em offset, truncar, unlink, rmdir,
rename, link, mkdir, readdir e stat com `nlink` e `size`, mais `.`, `..`, barra final, fds
(`open`/`pread`/`pwrite`/`fstat`/`close`), snapshot e restore. Semântica do tmpfs do Linux 6.12, inclusive
a ordem dos errnos (`rename` checa ancestralidade antes do tipo do destino, `link` dá `EEXIST` antes de
`EPERM`), `nlink` de diretório = 2 + subdiretórios e tamanho de diretório = 40 + 20 por entrada (o
`BOGO_DIRENT_SIZE` do shmem). As operações são funções genéricas sobre um trait `Txn` (acesso à tabela) e
sobre um `Flavor` (tabela, diretório, conteúdo); cada candidato é um flavor.

**Correção**, tudo verde em release:

- `tests/semantics.rs`: 8 cenários com o resultado de cada passo escrito à mão (hardlink, unlink de
  arquivo aberto, rename sobre destino existente em todas as combinações, `.`/`..`, buracos e
  truncamento, erros, raiz, snapshot com fds abertos). Os cenários sem snapshot rodam também no tmpfs de
  verdade do host (`/dev/shm`), o que prova que a expectativa é a do Linux; depois rodam no modelo de
  referência e em todos os 18 flavors, com `fsck` no fim.
- `tests/differential.rs` (proptest, 2048 casos cada, sequências de até 80 operações num universo de 39
  caminhos): modelo de referência (um `BTreeMap` de caminhos) contra o tmpfs do host; cada flavor contra o
  modelo, com snapshot e restore no meio e `fsck` dos invariantes no fim; cada flavor contra o tmpfs do
  host.
- `tests/concurrency.rs`: 8 threads escrevendo em diretórios diferentes da mesma sandbox, em cada uma das
  três estratégias de trava, com uma nona thread tirando snapshots o tempo todo e conferindo os
  invariantes de cada um; no fim o estado tem que bater com o que cada thread espera.
- O binário repete a comparação com 400 sequências de 60 operações: modelo x tmpfs do host deu **0
  divergências** (exercitando ENOENT, ENOTDIR, EISDIR, EEXIST, ENOTEMPTY, EPERM e EINVAL), e cada flavor x
  modelo com snapshots deu 0 divergências e 0 falhas de `fsck`.

**Medições** (`src/measure.rs`, binário principal, allocator do sistema):

- Imagens com 1k, 10k e 100k arquivos em `/d<i>/s<j>/f<k>` (64 por diretório), tamanho log-uniforme de 1
  a 8192 bytes (86,6 MiB de conteúdo nos 100k), mais uma cadeia `/deep/l2/.../l32` com um arquivo por
  nível.
- Snapshot e sandbox nova (clone + drop, em lotes); restore depois de 1 escrita no mesmo arquivo
  ("quente") e depois de 10 escritas em arquivos sorteados ("frio"); primeira escrita de 1 byte depois
  de snapshot (quente: sempre o mesmo arquivo; frio: arquivo sorteado) e a segunda escrita no lugar;
  escrita em função da profundidade (1, 4, 8, 16, 32); diretório com 10k entradas (criar depois de
  snapshot, criar no lugar, lookup, readdir ordenado); arquivo de 100 MiB (1 byte depois de snapshot,
  no lugar, leitura de 4 KiB, leitura sequencial); 100 sandboxes derivadas da imagem de 100k com
  "poucas alterações" (diretório de trabalho com 10 arquivos de 1 KiB, 10 escritas de 100 bytes em
  arquivos da imagem, 2 remoções, 1 rename).
- Vazão com 1, 4 e 16 threads, cada uma no seu `/w<t>` da mesma sandbox (60% escrita de 4 KiB, 20%
  criar e escrever 512 bytes, 20% stat; 20 mil operações por thread), nas estratégias `RwLock` na raiz,
  `arc-swap` com RCU e 64 fatias da tabela com `RwLock` cada (resolução de caminho com trava de leitura
  momentânea, mutação travando em ordem só as fatias tocadas, refaz se faltar trava). Referência: uma
  sandbox por thread. Estratégias intercaladas, mediana de 5 rodadas.

**Memória** (`src/memory.rs`, binário `e03-mem` rodado como subprocesso, com `stats_alloc` como
allocator global): bytes vivos da imagem, bytes alocados por snapshot e por sandbox nova, bytes retidos
pela primeira escrita depois de snapshot (com o snapshot vivo), bytes liberados pelo restore, custo de
100 sandboxes derivadas. Fica em processo separado porque o contador atômico global distorceria os
tempos com 16 threads.

**Critérios** (calculados no `main.rs` e gravados em `metrics.checks` de cada candidato). A razão entre
100k e 1k arquivos é o que separa "constante ou logarítmico" de "proporcional ao tamanho": uma árvore de
grau 32 vai de 2 pra 4 níveis nesse intervalo, linear seria 100x.

| critério | regra |
|---|---|
| snapshot constante | snapshot e sandbox nova alocam 0 bytes; tempo 100k/1k <= 5 e < 1 µs |
| restore constante | bytes liberados por escrita 100k/1k <= 2,5; tempo quente 100k/1k <= 5 |
| escrita não proporcional ao tamanho | bytes copiados 100k/1k <= 2,5; tempo quente 100k/1k <= 5; frio < 100 µs |
| memória compartilhada | sandbox derivada <= 5% da imagem |
| diretório grande | criar em diretório de 10k entradas depois de snapshot < 20 µs |
| conformidade | 0 divergências do modelo e 0 falhas de `fsck` |

Encaixe: "não serve" se falha um dos cinco primeiros critérios ou se a crate não tem manutenção; "encaixa
com trabalho" se tem parte feita à mão (código nosso pra manter) ou falha só o de diretório grande;
"encaixa" no resto.

## Candidatos

Estrutura (tabela + diretório), todos com o mesmo conteúdo (`Vec<Arc<bloco>>`) pra que a diferença venha
só da estrutura:

| chave | tabela | diretório | versão |
|---|---|---|---|
| `imbl-hamt` | `imbl::GenericHashMap<_, _, RandomState, ArcK>` | idem | imbl 7.0.2 |
| `imbl-hamt-triomphe` | idem com `ArcTK` (`triomphe::Arc`) | idem | imbl 7.0.2, triomphe 0.1.16 |
| `imbl-btree` | `imbl::GenericOrdMap<_, _, ArcK>` | idem | imbl 7.0.2 |
| `im-hamt` | `im::HashMap` | idem | im 15.1.0 |
| `rpds-hamt` | `rpds::HashTrieMapSync` | idem | rpds 1.2.1 |
| `rpds-rbtree` | `rpds::RedBlackTreeMapSync` | idem | rpds 1.2.1 |
| `chunkmap` | `immutable_chunkmap::map::MapM` (AVL de blocos de até 512) | idem | immutable-chunkmap 2.1.4 |
| `hand-radix` | trie de raiz 64 à mão (`src/radix.rs`, só `Arc` e `Arc::make_mut`) | `Arc<BTreeMap>` à mão | à mão |
| `hand-flat` | `Arc<BTreeMap>` da tabela inteira (controle negativo) | `Arc<BTreeMap>` à mão | à mão |

Conteúdo (sobre tabela e diretório `imbl::HashMap`): `Arc<Vec<u8>>`; `Vec<Arc<bloco>>`;
`imbl::Vector<Arc<bloco>>`; `rpds::VectorSync<Arc<bloco>>`; e uma sequência à mão que guarda até 32
blocos num `Vec` e passa pra trie de raiz 64 acima disso. Bloco é `Arc<[u8]>` de 4 KiB, com o último do
tamanho exato do resto do arquivo (arquivo de 100 bytes ocupa 100 bytes) e buracos apontando pra um bloco
zerado compartilhado.

Combinações finais, medidas de ponta a ponta (estrutura, conteúdo e vazão): `final-imbl` (imbl `HashMap`
+ `HashMap` + `Vector`), `final-imbl-ord` (imbl `OrdMap` + `OrdMap` + `Vector`), `final-hand` (trie à
mão + imbl `HashMap` + blocos híbridos à mão) e `final-hand-ord` (trie à mão + imbl `OrdMap` + blocos
híbridos à mão).

Como o imbl 7 escolhe o ponteiro: todo tipo é genérico em `P: archery::SharedPointerKind`; os apelidos
`imbl::HashMap`, `OrdMap` e `Vector` usam `DefaultSharedPtr`, que é `ArcK` (std `Arc`) por padrão e vira
`ArcTK` (`triomphe::Arc`) com a feature `triomphe`. Não existe variante `Rc` por padrão (`RcK` só
explícito, e aí o mapa não é `Send`). Como features são unificadas no build inteiro, ligar `triomphe`
troca o ponteiro de todos os apelidos; por isso o experimento usa os tipos `Generic*` com `P` explícito.
O rpds nas versões `*Sync` usa `ArcTK`.

Outros considerados: o `im` 15 entrou medido, mas está sem manutenção e tem três advisories no RustSec
(RUSTSEC-2020-0096, RUSTSEC-2023-0126, RUSTSEC-2026-0248). O depscan dá categoria (b) pro imbl e pro rpds
por artefatos que não tocam o caminho usado: `println!` em código `#[cfg(test)]` do imbl, `getrandom` que
o `cargo metadata` puxa pela unificação de features com o proptest (fora da árvore normal, como mostra
`cargo tree -p imbl -e normal`) e `std::process::abort` do triomphe em estouro de contador.

## Resultado

### Estrutura

Tempos medianos; "copiado" é o que a primeira escrita de 1 byte depois do snapshot aloca e retém (cópia
de caminho na tabela, inode e bloco); "liberado" é o que o restore depois de 1 escrita devolve.

| candidato | snapshot | 1a escrita quente 1k / 100k | copiado 1k / 100k | restore 1 escrita quente 1k / 100k | liberado 100k | criar em dir 10k pós-snapshot | bytes/inode | por sandbox | encaixe |
|---|---|---|---|---|---|---|---|---|---|
| imbl-hamt | 6 ns | 421 ns / 601 ns | 2.6 KiB / 3.8 KiB | 220 ns / 431 ns | 3.5 KiB | 1.6 µs (6.0 KiB) | 541 | 75.9 KiB | encaixa |
| imbl-hamt-triomphe | 7 ns | 451 ns / 582 ns | 2.6 KiB / 3.8 KiB | 210 ns / 301 ns | 3.5 KiB | 1.4 µs (5.9 KiB) | 540 | 76.9 KiB | encaixa |
| imbl-btree | 7 ns | 361 ns / 581 ns | 1.7 KiB / 2.6 KiB | 100 ns / 141 ns | 2.3 KiB | 1.1 µs (4.4 KiB) | 321 | 43.2 KiB | encaixa |
| im-hamt | 10 ns | 401 ns / 981 ns | 2.9 KiB / 4.2 KiB | 180 ns / 311 ns | 3.9 KiB | 2.2 µs (8.9 KiB) | 843 | 87.0 KiB | não serve |
| rpds-hamt | 7 ns | 511 ns / 741 ns | 1.6 KiB / 2.5 KiB | 211 ns / 341 ns | 2.2 KiB | 2.1 µs (3.3 KiB) | 450 | 46.0 KiB | encaixa |
| rpds-rbtree | 7 ns | 281 ns / 471 ns | 1.3 KiB / 2.0 KiB | 110 ns / 170 ns | 1.7 KiB | 2.3 µs (3.0 KiB) | 395 | 37.3 KiB | encaixa |
| chunkmap | 7 ns | 2.4 µs / 2.6 µs | 8.9 KiB / 9.5 KiB | 1.6 µs / 1.7 µs | 9.2 KiB | 5.8 µs (29.3 KiB) | 436 | 202.1 KiB | encaixa |
| hand-radix | 7 ns | 320 ns / 481 ns | 1.8 KiB / 2.7 KiB | 210 ns / 370 ns | 2.3 KiB | 73.2 µs (446.3 KiB) | 269 | 46.4 KiB | encaixa com trabalho |
| hand-flat | 6 ns | 5.5 µs / 2.1 ms | 37.0 KiB / 3.3 MiB | 6.1 µs / 2.1 ms | 3.3 MiB | 181.2 µs (778.6 KiB) | 295 | 3.4 MiB | não serve |
| final-imbl | 7 ns | 420 ns / 561 ns | 2.6 KiB / 3.8 KiB | 191 ns / 271 ns | 3.5 KiB | 1.9 µs (6.0 KiB) | 502 | 75.7 KiB | encaixa |
| final-hand | 7 ns | 360 ns / 551 ns | 1.8 KiB / 2.7 KiB | 210 ns / 350 ns | 2.4 KiB | 1.4 µs (5.5 KiB) | 435 | 44.2 KiB | encaixa com trabalho |
| final-hand-ord | 7 ns | 351 ns / 561 ns | 1.8 KiB / 2.7 KiB | 210 ns / 351 ns | 2.4 KiB | 1.5 µs (5.0 KiB) | 296 | 40.4 KiB | encaixa com trabalho |
| final-imbl-ord | 7 ns | 381 ns / 591 ns | 1.7 KiB / 2.6 KiB | 101 ns / 141 ns | 2.3 KiB | 1.3 µs (4.5 KiB) | 290 | 43.2 KiB | encaixa |

"bytes/inode" é a memória da imagem de 100k arquivos menos o conteúdo, dividida pelos 101.677 inodes
(tabela, diretórios, nomes, inode e vetor de blocos). "por sandbox" é o custo médio de cada uma das 100
sandboxes derivadas, dominado pelos dados que elas escreveram (10 KiB novos mais os blocos copiados).
O `chunkmap` cumpre os critérios, mas é dominado: copia 9 KiB por escrita (3 a 5x os outros, porque o
bloco da AVL tem até 512 entradas) e cada sandbox derivada custa 202 KiB.

Os mesmos cenários com cache frio (arquivo sorteado a cada amostra), que é o custo realista:

| candidato | 1a escrita fria 1k / 10k / 100k | restore de 10 escritas 1k / 100k | stat 100k | readdir 10k ordenado | lookup em dir 10k |
|---|---|---|---|---|---|
| imbl-hamt | 601 ns / 1.0 µs / 2.4 µs | 1.8 µs / 3.9 µs | 911 ns | 942.0 µs | 201 ns |
| imbl-btree | 591 ns / 1.0 µs / 2.3 µs | 902 ns / 1.9 µs | 1.3 µs | 201.8 µs | 400 ns |
| rpds-hamt | 701 ns / 1.5 µs / 2.3 µs | 1.2 µs / 3.4 µs | 1.1 µs | 1.5 ms | 581 ns |
| rpds-rbtree | 761 ns / 1.8 µs / 3.4 µs | 1.3 µs / 2.9 µs | 2.3 µs | 327.4 µs | 891 ns |
| chunkmap | 2.6 µs / 3.6 µs / 5.9 µs | 3.2 µs / 18.7 µs | 1.8 µs | 173.4 µs | 591 ns |
| hand-radix | 531 ns / 1.0 µs / 2.1 µs | 1.7 µs / 3.9 µs | 992 ns | 170.2 µs | 221 ns |
| hand-flat | 5.6 µs / 73.8 µs / 1.7 ms | 6.7 µs / 2.3 ms | 1.5 µs | 171.3 µs | 301 ns |
| final-hand-ord | 591 ns / 1.1 µs / 2.1 µs | 1.7 µs / 3.9 µs | 992 ns | 245.2 µs | 350 ns |
| final-imbl-ord | 591 ns / 1.0 µs / 2.4 µs | 872 ns / 1.8 µs | 1.6 µs | 240.0 µs | 531 ns |

No frio o tempo cresce 3 a 4x de 1k pra 100k porque a imagem de 100k (115 a 168 MiB) não cabe no cache e
a leitura do caminho falta cache; o `stat` (só leitura) cresce na mesma proporção. Os bytes copiados, que
medem o trabalho do algoritmo, crescem 1,5x, que é a profundidade da árvore indo de 2 pra 3 ou 4 níveis.

Profundidade de diretório (imagem de 100k, primeira escrita depois de snapshot, quente):

| candidato | prof. 1 | 4 | 8 | 16 | 32 | copiado prof. 1 / 32 |
|---|---|---|---|---|---|---|
| imbl-btree | 531 ns (stat 211 ns) | 762 ns | 1.0 µs | 1.5 µs | 2.5 µs (stat 2.1 µs) | 1.7 KiB / 1.7 KiB |
| hand-radix | 471 ns (stat 100 ns) | 571 ns | 672 ns | 832 ns | 1.1 µs (stat 611 ns) | 1.7 KiB / 1.7 KiB |
| final-imbl-ord | 531 ns (stat 221 ns) | 761 ns | 1.0 µs | 1.5 µs | 2.6 µs (stat 2.2 µs) | 1.7 KiB / 1.7 KiB |
| hand-flat | 1.5 ms | 1.6 ms | 1.7 ms | 1.3 ms | 2.0 ms | 3.3 MiB / 3.3 MiB |

A cópia não depende da profundidade de diretório: com a tabela de inodes, escrever num arquivo copia o
caminho na tabela (log do número de inodes), o inode e um bloco, nunca a cadeia de diretórios. O tempo
que cresce com a profundidade é a resolução do caminho (o `stat` entre parênteses), que é só leitura.

O controle negativo deixa claro o que "Arc à mão" ingênuo custa: `hand-flat` também tem snapshot de 6 ns,
mas a primeira escrita copia a tabela inteira (37 KiB com 1k arquivos, 3,3 MiB com 100k: 92x), cada
sandbox derivada custa 3,4 MiB e a imagem mais 100 sandboxes ocupam 450,7 MiB, contra 118,9 MiB em
`final-imbl-ord`. O mesmo vale pro diretório: `Arc<BTreeMap>` por diretório (`hand-radix`) copia o
diretório inteiro na primeira inserção depois do snapshot (446 KiB e 73 µs num diretório de 10k
entradas), enquanto `imbl::OrdMap` copia 4,5 KiB em 1,3 µs.

Compartilhamento: imagem de 100k mais 100 sandboxes derivadas ocupa de 1,03x a 1,05x a imagem sozinha nos
candidatos que passam (1,15x no `chunkmap`, que copia blocos de até 512 entradas), ou seja, um fator de
96 a 98 contra 101 cópias fundas; snapshot (6 a 10 ns) e sandbox nova (12 a 17 ns) não alocam nada. Montar a imagem de 100k leva de 100 a 210 ms; soltar a última referência dela leva de 5 a 16 ms
(isso é O(n), por natureza).

### Conteúdo

Arquivo de 100 MiB:

| candidato | 1 byte pós-snapshot | copiado | no lugar | ler 4 KiB | overhead | bytes/inode (100k pequenos) | encaixe |
|---|---|---|---|---|---|---|---|
| `Arc<Vec<u8>>` | 12.5 ms | 100.0 MiB | 2.3 µs | 371 ns | 28.0 MiB | 498 | não serve |
| `Vec<Arc<bloco>>` | 91.6 µs | 404.9 KiB | 221 ns | 451 ns | 914.0 KiB | 542 | encaixa com trabalho |
| `imbl::Vector<Arc<bloco>>` | 2.2 µs | 9.0 KiB | 161 ns | 581 ns | 832.1 KiB | 501 | encaixa |
| `rpds::VectorSync<Arc<bloco>>` | 1.4 µs | 5.7 KiB | 140 ns | 641 ns | 1.2 MiB | 574 | encaixa |
| híbrido à mão (Vec até 32 + trie) | 1.8 µs | 8.0 KiB | 131 ns | 541 ns | 822.8 KiB | 548 | encaixa com trabalho |
| final-imbl-ord (`imbl::Vector`) | 2.3 µs | 8.5 KiB | 120 ns | 561 ns | 831.0 KiB | 290 | combinação |
| final-hand-ord (híbrido) | 1.7 µs | 7.7 KiB | 90 ns | 481 ns | 821.9 KiB | 296 | combinação |

`Arc<Vec<u8>>` copia o arquivo inteiro pra mudar 1 byte, e ainda desperdiça 28 MiB num arquivo de 100 MiB
pela capacidade dobrada do `Vec`. `Vec<Arc<bloco>>` copia o vetor de ponteiros inteiro (16 bytes por bloco
de 4 KiB, 400 KiB pra 100 MiB): melhor, mas ainda proporcional ao tamanho. Os vetores persistentes copiam
só o caminho (de 5,7 a 9 KiB, contando o bloco de 4 KiB).

### Vazão concorrente

Operações por segundo com cada thread no seu diretório da mesma sandbox (`final-imbl-ord`, mediana de 5
rodadas):

| threads | RwLock na raiz | arc-swap (RCU) | 64 fatias | sandbox por thread (referência) |
|---|---|---|---|---|
| 1 | 2.35 M | 0.72 M | 2.54 M | 2.40 M |
| 4 | 1.23 M | 0.61 M | 5.98 M | 8.39 M |
| 16 | 0.96 M | 0.43 M | 7.00 M | 14.95 M |

Os valores absolutos variaram até 3x entre rodadas com a carga da máquina, mas a ordem foi a mesma em
todas as rodadas e em todos os flavors medidos: com 4 ou mais threads, sandbox por thread > fatias >
`RwLock` > RCU. O `RwLock` na raiz não escala (cai de 2,4 M pra 1 M ops/s). O RCU com `arc-swap` é o pior
mesmo com 1 thread: toda escrita copia o caminho porque a versão publicada continua viva, e com 16
threads perde o compare-and-swap cerca de 6 vezes por operação (1,9 milhão de refeitas em 320 mil
operações); com diretório em `Arc<BTreeMap>` (`hand-radix`) ele cai pra 0,1 M ops/s, porque cada criação
copia o diretório todo. As fatias chegam a 7 M ops/s com 16 threads nas combinações finais (2 a 4,5 M
nas outras três estruturas medidas, nesta rodada). A diferença que sobra pra sandbox por thread vem do que ainda é
compartilhado: toda resolução de caminho pega trava de leitura nas fatias de `/` e de `/w<t>` (não medido
separadamente).

### Correções no design que o experimento encontrou

- **O fd guarda o ino, não `Arc<Inode>`.** Com estrutura persistente, um `Arc<Inode>` é um valor
  congelado: um fd que o segurasse não veria escritas feitas por outro fd ou pelo caminho. O "arquivo
  desvinculado e aberto continua vivo" fica com a tabela: o inode com `nlink == 0` e fd aberto fica nela
  marcado como órfão e sai no último `close` (testado contra o Linux, inclusive `fstat` com `nlink` 0).
- **Restore não pode voltar o contador de inos.** Senão um fd aberto antes do restore passa a apontar pra
  outro arquivo que reaproveitou o mesmo ino. Depois do restore, fd cujo ino existe no estado restaurado
  vê a versão restaurada; fd cujo ino não existe dá `ESTALE`.
- **"Restore O(1)" é O(o que mudou desde o snapshot)**: trocar a raiz é O(1), mas soltar a versão
  abandonada libera o que só ela tinha (2,3 KiB e 141 ns por escrita em `final-imbl-ord`). Soltar a
  última referência de uma imagem inteira é O(n) (5 a 16 ms com 100k arquivos).
- **readdir**: o tmpfs do host devolve as entradas do mais novo pro mais antigo (conferido com `ls -f`
  em `/dev/shm`); aqui o readdir sai em ordem de nome. Reproduzir a ordem do tmpfs exige um índice por
  ordem de criação no diretório (o tmpfs usa offsets estáveis num maple tree). Fica registrado como
  decisão pendente do VFS, fora do H17.

## Veredito

**H17: confirmada.** Com qualquer mapa persistente de verdade na tabela e nos diretórios, snapshot custa 6
a 10 ns e sandbox nova a partir da imagem 12 a 17 ns (clone mais drop), sem alocar nada, com 1k ou 100k
arquivos. Restore custa o que mudou desde o
snapshot (141 ns e 2,3 KiB por escrita em `final-imbl-ord`, sem crescer com o tamanho da imagem). A
primeira escrita depois do snapshot copia de 1,7 a 2,6 KiB indo de 1k pra 100k arquivos (logarítmico),
contra 37 KiB a 3,3 MiB do controle linear, e não depende da profundidade de diretório (1,7 KiB na
profundidade 1 e na 32). 100 sandboxes derivadas da imagem de 100k custam 43 KiB cada sobre 115 MiB. Onze
estruturas e combinações cumprem o critério e servem; o `im` também cumpre, mas fica de fora por falta de
manutenção e advisories; `hand-flat` e `Arc<Vec<u8>>` mostram que a versão "Arc à mão" ingênua não serve.

**Recomendação de estrutura** (o que vai pro design):

- **Tabela de inodes e diretórios: `imbl` 7 `OrdMap`** (com `ArcK`). É a combinação com menos memória entre
  as crates (290 bytes por inode na combinação final), readdir já sai ordenado (240 µs pra 10k entradas
  contra 1 ms ordenando um `HashMap`), e diretório grande continua barato depois do snapshot (1,3 µs). A trie
  de raiz 64 à mão na tabela (`final-hand-ord`) passa em tudo e faz o `stat` cerca de 1,5x mais rápido com
  inos densos; é uma otimização medida e disponível, não uma necessidade.
- **Conteúdo: `imbl::Vector<Arc<[u8]>>` de blocos de 4 KiB**, com o último bloco do tamanho exato e buraco
  apontando pro bloco zerado compartilhado. 1 byte num arquivo de 100 MiB depois de snapshot: 2,3 µs e
  8,5 KiB. O `rpds::VectorSync` copia um pouco menos, mas gasta mais por inode e não traz o `OrdMap` junto.
- **Trava: `RwLock` por sandbox** como padrão (2,4 M ops/s com 1 thread), trocando pra **fatias da tabela**
  quando houver escritores paralelos na mesma sandbox (até 7 M ops/s com 16 threads). **RCU com
  `arc-swap` não serve pra escrita** (pior em todos os cenários). Processos em sandboxes diferentes não
  compartilham nada mutável e escalam sozinhos.
- O fd guarda o ino, o tmpfs mantém órfãos abertos na tabela, e o restore preserva o contador de inos.

## Arquivos

- `src/radix.rs`: trie de raiz 64 persistente à mão.
- `src/maps.rs`: traits `IntMap`/`NameMap`/`Flavor` e as implementações sobre cada crate.
- `src/content.rs`: `ArcBytes`, `Blocks<S>` e as sequências de blocos.
- `src/fs.rs`, `src/vfs.rs`: inodes, operações, snapshot, restore, fds, `fsck`.
- `src/concurrent.rs`: `LockedSandbox`, `RcuSandbox`, `ShardedSandbox`.
- `src/check.rs`: modelo de referência, alvo real (`/dev/shm`) e comparação.
- `src/flavors.rs`: os candidatos e as macros que iteram sobre eles.
- `src/measure.rs`, `src/memory.rs`, `src/bin/e03-mem.rs`, `src/main.rs`: medições e o JSON.
- `tests/semantics.rs`, `tests/differential.rs`, `tests/concurrency.rs`.
