# ul-git: proveniência

O crate é MIT. Nenhum arquivo do git (GPL-2.0) foi aberto. As fontes permitidas são estas:

- A documentação do git: as man pages git-*(1), gitignore(5), gitattributes(5), gitglossary(7),
  gitcli(7) e git-config(1).
- Os formatos documentados: gitformat-index, gitformat-pack e o formato dos objetos.
- O comportamento do oráculo (git 2.47.3 do Debian 13), medido com scripts em
  `$SCRATCHPAD/git-*.sh` e registrado nos testes.
- As crates `gix-*` (MIT/Apache).

## Ressalva sobre a autoria

O autor (o agente git) conhecia a implementação do git de memória e escreveu a primeira versão de
alguns trechos a partir dessa lembrança, antes da regra de licença chegar. Esses trechos foram
reescritos com estrutura própria, e as regras numéricas foram re-derivadas por medição no oráculo.
Mesmo assim, não é sala limpa estrita, porque é o mesmo autor. A decisão fica com o dono (ver
`docs/LICENSING.md`).

## Por arquivo

| Arquivo | Origem |
|---|---|
| `hash.rs` | Formato de objeto documentado (`<tipo> <tamanho>\0<dados>`), SHA-1 do crate `sha1`. |
| `object.rs` | Formatos de tree, commit e tag documentados e observados com `cat-file -p`. `%s`/`%b` seguem git-log(1) ("PRETTY FORMATS") e o oráculo. A leitura de identidade é uma leitura direta de `Nome <email> segundos fuso`. |
| `odb.rs` | gitformat-pack (idx v1/v2, entradas, OFS/REF delta) e o formato de objeto solto. zlib pelo `miniz_oxide`. |
| `index.rs` | gitformat-index (v2/v3/v4, extensões opcionais). A comparação de `stat` e o caso "racy" são desenho próprio, com a ideia descrita na documentação técnica (racy-git). |
| `config.rs` | Sintaxe do git-config(1) ("CONFIGURATION FILE"). O parser de valor foi reescrito a partir do texto da documentação depois da regra. A edição (onde inserir, remoção de seção vazia, aspas na escrita, mensagens) foi medida no oráculo. |
| `repo.rs` | git(1) (`GIT_DIR`, `GIT_WORK_TREE`, `GIT_CEILING_DIRECTORIES`, `GIT_COMMON_DIR`) e gitrepository-layout(5). Saídas de `rev-parse` medidas no oráculo. |
| `refs.rs` | git-check-ref-format(1) pras regras de nome, gitrepository-layout(5) pro `packed-refs` e o reflog. O colapso de espaço na mensagem do reflog foi medido no oráculo. |
| `ident.rs` | git-commit-tree(1) e git-var(1) (precedência ambiente, config, `EMAIL`). Os caracteres removidos nas pontas e a mensagem "Author identity unknown" foram medidos no oráculo. |
| `date.rs` | Formatos do git-commit(1) ("DATE FORMATS") e do git-log(1) (`--date`). O parser estrito foi reescrito com tokenizador próprio depois da regra e validado contra a tabela medida no oráculo (o teste `strict_parse_matches_oracle`). As faixas do `relative` foram re-derivadas por bisseção no oráculo (`git-reldate.sh`). Calendário pelos algoritmos de domínio público de H. Hinnant. TZif pelo tzfile(5). |
| `wildmatch.rs` | `gix-glob` (MIT/Apache). |
| `ignore.rs` | gitignore(5). |
| `pathspec.rs` | gitglossary(7) ("pathspec") e o oráculo. |
| `quote.rs` | git-config(1), `core.quotePath`. |
| `opts.rs` | gitcli(7) e as mensagens de erro observadas no oráculo. |
| `usage/*.txt` | Saída de `git <cmd> -h` capturada do oráculo. |
| `msg.rs` | git-stripspace(1). |
| `re.rs` | POSIX.1-2017 "Regular Expressions" e o manual do GNU grep (extensões). Motor do crate `regex` (MIT/Apache). |
| `graph.rs` | Definição de "best common ancestor" do git-merge-base(1). Reescrito depois da regra: conjuntos de ancestrais, sem a pintura por data do git. |
| `diff/text.rs` | Mudanças pelo `gix-imara-diff` (Apache-2.0, Myers com heurística de indentação). Agrupamento de hunks reescrito depois da regra, a partir de git-diff(1) (`--unified`, `--inter-hunk-context`). Linha de função pela regra de gitattributes(5); o corte em 80 bytes foi medido. |
| `diff/rename.rs` | Desenho próprio, depois da regra: semelhança = bytes de linhas preservadas sobre o maior tamanho, atribuição gulosa. Não reproduz a métrica do git; a concordância está no STATUS.md. |
| `diff/mod.rs` | Formato de patch do git-diff(1) ("GENERATING PATCH TEXT WITH -P"). O layout do `--stat` foi reescrito depois da regra a partir de medições no oráculo (`git-stat-probe.sh`, `git-stat-probe2.sh`): larguras, escala e corte de nome. O `pprint_rename` foi reescrito a partir das saídas observadas. |
| `worktree.rs` | git-status(1) (`--untracked-files`, `--ignored`) e o oráculo. |
| `cmd/mod.rs` | Opções globais do git(1). A sugestão de comando parecido é distância de edição ponderada (Damerau-Levenshtein, algoritmo de livro-texto), com pesos lembrados da implementação e confirmados pela tabela de 40 erros de digitação medida no oráculo (teste `typo_suggestions_match_oracle`). Lista de comandos de `git --list-cmds`. |
| `cmd/*.rs` | Man pages de cada comando e as saídas do oráculo. |
| `cmd/status.rs`, `cmd/commit.rs`, `cmd/log.rs`, `cmd/diff_cmd.rs` | git-status(1), git-commit(1), git-log(1), git-show(1), git-rev-list(1), git-shortlog(1), git-diff(1) e as saídas medidas no oráculo (git 2.47.3). Nenhum arquivo do git foi aberto: a ordem de decoração, a linha em branco antes do diff, os rótulos e larguras do status e os separadores de registro vêm de sondas no oráculo. |
| `cmd/branch.rs`, `cmd/tag.rs`, `cmd/for_each_ref.rs`, `cmd/reffmt.rs` | git-branch(1), git-tag(1), git-for-each-ref(1) ("FIELD NAMES") e sondas no oráculo (`b*.sh`, `t1.sh`, `f1.sh`): mensagens de criação, remoção, renomeação, acompanhamento (`set up to track`), o texto do `-vv`, o reflog (`branch: Created from`, `Branch: renamed`, `Branch: copied`), os átomos e a ordenação do `--format`/`--sort`. O desenho do motor de refs (árvore de nós, `if`/`align`, filtros por grafo) é próprio. |
| `cmd/checkout.rs`, `cmd/unpack.rs`, `cmd/restore.rs`, `cmd/reset.rs` | git-checkout(1), git-switch(1), git-restore(1), git-reset(1), a tabela de casos do "Two Tree Merge" do git-read-tree(1) e sondas no oráculo (`c*.sh`, `r1.sh`, `o1.sh`, `d1.sh`): mensagens de troca, aviso de HEAD destacado e de commits deixados pra trás, rejeições por mudança local ou arquivo não rastreado, reflog (`checkout: moving from`, `reset: moving to`), `ORIG_HEAD`. |
| `cmd/rm.rs`, `cmd/mv.rs` | git-rm(1), git-mv(1) e sondas no oráculo (`rm1.sh`, `mv1.sh`, `mv2.sh`): as recusas de `rm` por modificações locais e as mensagens de `mv` (`bad source`, `destination exists`...). |
