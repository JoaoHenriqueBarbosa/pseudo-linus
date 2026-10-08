# pip e espelho do PyPI da bancada

Casos em `testbench/corpus/cases/pip/` (`install.toml`, `smoke.toml`, `mirror.toml`); wheels em
`testbench/mirror/` (`packages.txt`, `wheels.lock`, `fetch.sh`).

## `pip` falhava no `setup_logging` (`KeyError: 0`)

`logging.config.dictConfig` faz `sorted(handlers)` sobre o `ConvertingDict` (subclasse de `dict` com `__getitem__` em
Python). `frame_root` (`vm.rs`) tratava toda instância com `__getitem__` em Python e sem `__iter__` como protocolo antigo
de sequência (`obj[0]`, `obj[1]`...); o guarda só olhava `builtin_base` (exceções) e esquecia `data_base`. Subclasse de
`dict`/`list`/`str` herda o `__iter__` do tipo embutido, então o guarda agora exige as duas bases vazias. Derrubava
`pip show`, `pip list`, `pip install` em venv (casos `stdout-broken-pipe`, `pip-version-help-list`, `rest-api-client`).

## Extensões nativas do espelho (wheels cp313 manylinux)

Levantamento por inspeção da lista de arquivos de cada wheel (nomes de entrada do zip, sem descompactar).
O sandbox só carrega `markupsafe._speedups` (Rust) e, para mypyc, o `.py` irmão no lugar do `.so`.

| Pacote (versão do lock) | Binário | Quem puxa | Ação sugerida |
|---|---|---|---|
| markupsafe 3.0.4 | `markupsafe/_speedups.cpython-313-*.so` | jinja2, mako, flask | já suportado em Rust; o `.py` puro cobre o resto |
| charset-normalizer 3.5.2 | `cd.*.so` e `md.*.so` (mypyc) | requests | carregar `cd.py` e `md.py` irmãos (já existem na wheel) |
| tomli 2.5.0 | `tomli/*.so` e `<hash>__mypyc.*.so` | pacote direto da lista | `.py` irmão; os `.so` stub não carregam |
| isort 9.0.2 | `isort/**/*.so` e `<hash>__mypyc.*.so` (inclui `_vendored/tomli`) | pacote direto da lista | `.py` irmão para todo módulo; conferir que a escolha vale também para o pacote vendorizado |
| pyyaml 6.0.3 | `yaml/_yaml.cpython-313-*.so` (libyaml) | pacote direto da lista | o `yaml/__init__.py` cai em Python puro quando `yaml._yaml` não importa (`__with_libyaml__` falso); as saídas dos casos não dependem disso, mas `yaml.CSafeLoader` não existiria (no Debian existe) |
| wcwidth 0.9.2 | `wcwidth/_wcwidth_c.abi3.so` | prompt-toolkit | a wheel traz todos os módulos puros (`wcwidth.py`, `_wcswidth.py`, `bisearch.py`...); confirmar que o import do `.so` é opcional com fallback; se não for, tratar como mypyc (sem `.so`, só o `.py`) |
| cffi 2.1.1 | `_cffi_backend.cpython-313-*.so` (topo) | cryptography (transitiva) | sem fallback puro: `import cffi` falha sem o `.so`. Fora do escopo dos casos |
| cryptography 50.0.2 | `cryptography/hazmat/bindings/_rust.abi3.so` | google-auth (transitiva, presumido) | sem fallback puro, extensão em Rust grande. O `pip install` precisa funcionar (a wheel só é extraída; a tag `manylinux_2_34` exige glibc 2.34 ou maior na detecção de plataforma do pip), mas nenhum caso pode importar `cryptography`. Se algum dia precisar, é porte novo e pede decisão |

Pontos de divergência que não são do carregador:

- `google.auth.crypt` escolhe `_cryptography_rsa` quando `cryptography` importa e cai em `_python_rsa` quando
  não importa. No oráculo importa; no sandbox cairia no outro caminho. Por isso o caso `pip-smoke-google-auth`
  não toca em `crypt` nem em `jwt` (usa só `_helpers`, `credentials`, `exceptions` e um `default()` com
  `GOOGLE_APPLICATION_CREDENTIALS` apontando para arquivo ausente).
- Qualquer caso novo com `pyjwt[crypto]`, `oauthlib[signedtoken]`, `paramiko` ou similar entra no mesmo buraco.

## Dependências no `wheels.lock` que não estão em `packages.txt`

`annotated_doc` (typer), `blinker` (flask), `markdown_it_py` e `mdurl` (rich), `pycparser`, `cffi` e
`cryptography` (ver acima), `mypy_extensions` (origem não confirmada por inspeção; provável requisito de
runtime de wheel mypyc). Além disso `platformdirs` aparece duas vezes (4.12.3 e 4.12.4): o pip escolhe a
4.12.4; conferir se a 4.12.3 é intencional (pin de alguma dependência) ou sobra do `fetch.sh`.

## Registro do que sobrou (revisão por inspeção de `smoke.toml`, sem execução)

Corrigidos nesta revisão:

- flask: `Allow` do 405 vem de `list(set)` no werkzeug (ordem varia com `PYTHONHASHSEED`); agora ordenado.
- docutils: `writer_name="html5"` explícito (o alias `html` é transitório entre versões).
- python-dotenv: instala `python-dotenv[cli]`; sem o extra o comando `dotenv` não tem `click`.
- shellingham: o resultado dependia da cadeia de processos do harness; agora roda sob `bash -c` e `sh -c`
  explícitos, sem otimização de `exec` do último comando.
- termcolor: dois processos separados, um com `FORCE_COLOR`, outro com `NO_COLOR`, sem mexer em `os.environ`
  no meio da execução (a decisão de cor pode ser cacheada); `cprint` escreve em `StringIO`.
- pure-eval: `s.upper` imprimia endereço de memória; trocado por atributo de `SimpleNamespace`.
- babel: `Locale("zh", "Hans", "CN")` passava script e território trocados (levantaria `UnknownLocaleError`);
  agora por palavra-chave.
- s3transfer: `calculate_range_parameter(part_size, part_index, num_parts)` recebia um total de bytes no
  lugar de `num_parts`; agora `(8 MB, i, 3)` para as partes 0, 1 e 2 (a última sem limite superior).
- google-auth: reescrito sem `crypt`/`jwt`/`cryptography` (ver acima).
- pyasn1: iteração direta de `Sequence` sem spec não é garantida; agora por índice. `SetOf.clone([...])` tomava a
  lista como `componentType`; agora `extend`.

Pendências e riscos conhecidos:

- `pip-smoke-unidecode` ainda tem um U+200B literal dentro da string `"a?b"` (invisível no editor; a
  ferramenta de edição normaliza o caractere e não consegue casá-lo). É determinístico, mas trocar por
  `"a​b"` quando houver como.
- Versões acima do que eu conheço (oauthlib 4.0.0, starlette 1.7.0, filelock 4.0.12, typer 0.27.3,
  flake8 7.4.1, wcwidth 0.9.2, pyasn1 0.6.4): as APIs usadas são antigas e estáveis, mas não foi possível
  confirmar por inspeção; a primeira execução no oráculo decide.
- `wcwidth` de sequência com ZWJ (família de emojis) muda de valor entre versões; vale só para o lock atual.
- Casos que dependem de fidelidade do interpretador, não do pacote: `executing`, `stack-data`, `pure-eval`,
  `asttokens`, `jedi`, `parso` (bytecode, `co_positions`, `f_lasti`, AST), `pexpect`/`ptyprocess` (eco do pty,
  EOF/EIO), `filelock` (`flock` entre descritores do mesmo processo), `importlib-metadata` (lista de
  distribuições do venv: `importlib_metadata`, `pip`, `zipp`).
- Porta fixa 8765 reutilizada por requests, urllib3, httpx, httpcore, httplib2 e uvicorn, cada caso em venv e
  processo próprios; o `ThreadingHTTPServer` já liga com `SO_REUSEADDR`, o uvicorn também.
