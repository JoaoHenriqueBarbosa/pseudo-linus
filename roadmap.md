# PRD — pseudo-linus: Python completo e pip nativo

Oct 6, 2026 · @João Henrique Barbosa

## Visão e tese

Fechar o Python 3.13.5 do pseudo-linus ao ponto de o `pip` rodar sem alteração transforma o PyPI inteiro de Python puro em software do sandbox, sem escrever nenhum desses pacotes.

Hoje cada programa existe porque foi reimplementado em Rust. O `pip` é um programa Python puro: se o interpretador e a stdlib forem fiéis o suficiente, ele baixa, valida, descompacta e registra wheels exatamente como no Debian. A partir daí o ecossistema cresce sozinho.

O que não for Python puro (extensões nativas em `.so`) entra pelo mesmo caminho do Pillow: o módulo é reimplementado em Rust com a API idêntica, e o pip instala uma wheel que aponta para ele. Poucos pacotes concentram quase toda a demanda de agentes, então um conjunto pequeno de módulos nativos destrava análise de dados completa.

O resultado é o argumento definitivo de que o pseudo-linus é uma nova classe de ambiente: um contêiner semântico que, além de imitar o Debian, herda o software do Debian.

## Objetivos e métricas de sucesso

O marco central é: `python3 -m venv .venv && .venv/bin/pip install <pacote>` idêntico ao Debian para os 100 pacotes Python puro mais baixados do PyPI.

| Métrica | Alvo | Como medir |
| --- | --- | --- |
| Suíte `python-stdlib` na bancada | 100% dos casos dos módulos que o pip importa | pl-conformance contra o oráculo |
| `pip --version`, `pip list`, `pip show` | Saída byte a byte igual | Nova suíte `pip-cli` |
| `pip install` dos top 100 Python puro | 100 de 100 instalam; árvore de `site-packages` e `RECORD` idênticos | Nova suíte `pip-install` |
| Import e smoke test pós-instalação | 95 de 100 passam o teste do próprio pacote ou um script de uso | Suíte `pip-smoke` |
| Pacotes nativos reimplementados | Pillow, numpy, pandas e matplotlib (backend Agg) | Suítes por pacote, comparando arrays, imagens e CSV gerados |
| Tempo de `pip install requests` | Menor que no oráculo (sem rede real, índice local) | Cronômetro na bancada |
| Placar geral | Manter 97,4% ou mais com as novas suítes somadas | `docs/conformance.md` |

## Escopo e fora de escopo

O escopo é tudo que o agente precisa para instalar e usar pacotes Python dentro do sandbox, com o host controlando a origem dos arquivos.

**Dentro:**

- Interpretador e stdlib suficientes para o `pip` 25.x do Debian trixie rodar sem patch.
- `venv`, `ensurepip`, `site`, `sysconfig` e o comportamento do PEP 668 no Python do sistema.
- Instalação de wheels `py3-none-any` e de sdists com backend Python puro (setuptools, hatchling, flit-core, poetry-core).
- Espelho do PyPI servido pelo host dentro do sandbox, com cache e lista de permissão.
- Módulos nativos reimplementados em Rust para os pacotes de maior demanda.
- Pacotes Debian `python3-*` equivalentes via `apt`, onde fizer sentido para a fidelidade.

**Fora:**

- Executar `.so` ou qualquer código de máquina vindo de wheels.
- Compilar extensões C, Cython ou Rust dentro do sandbox.
- Acesso à internet real a partir do convidado.
- Pacotes nativos de cauda longa sem reimplementação.

## Fase 1: stdlib e mecanismo de import para o pip rodar

A fase termina quando `python3 -m pip --version` roda o pip vendorizado do Debian sem nenhum patch e imprime a mesma linha do oráculo.

O caminho prático é rodar o pip real no sandbox, registrar cada `ImportError`, `AttributeError` e divergência de comportamento, e fechar um por um. A lista abaixo é o que se espera encontrar.

| Área | Requisitos | Por que o pip precisa |
| --- | --- | --- |
| Import system | `importlib` completo: finders e loaders de `sys.meta_path`, `importlib.resources`, `importlib.util.spec_from_file_location`, imports de dentro de `.zip`, namespace packages (PEP 420), arquivos `.pth` | O pip vendoriza \~20 bibliotecas e carrega recursos de dentro delas |
| Metadados | `importlib.metadata`: `distributions()`, `entry_points()`, leitura de `.dist-info/METADATA`, `RECORD`, `entry_points.txt` | `pip list`, `pip show`, detecção de conflitos |
| Caminhos | `site`, `sysconfig` (esquemas `posix_prefix`, `posix_local`, `venv` do Debian), `sys.prefix`, `sys.base_prefix`, `/usr/lib/python3/dist-packages` | Onde instalar e o que já está instalado |
| Formatos | `zipfile` (incluindo leitura de wheels grandes), `tarfile`, `gzip`, `email.parser`, `tomllib`, `csv` | Wheel, sdist, METADATA, pyproject.toml, RECORD |
| Criptografia | `hashlib` (sha256, sha384, sha512), `hmac`, `secrets`, `base64` | Validação de hashes do índice e do RECORD |
| Rede | `ssl` com contexto aceitando a CA do espelho, `http.client`, `urllib`, `socket` com timeouts, `select`/`selectors` | Download do índice e dos arquivos |
| Processo | `subprocess` com captura e `env`, `shutil`, `tempfile`, `os.replace`, `os.chmod`, `stat`, `fcntl` | Build isolation, cópia atômica, permissões de scripts |
| Diversos | `logging`, `optparse`, `textwrap`, `locale`, `platform`, `sys.flags`, `warnings`, `contextvars`, `typing` completo, `dataclasses`, `enum`, `functools.cached_property` | Usados pelo pip e pelo `rich` vendorizado |

Critério de saída: suíte `python-stdlib` com 100% nos módulos acima e o pip respondendo `--version`, `--help` e `list` iguais ao oráculo.

## Fase 2: pip, venv, PEP 668 e espelho do PyPI

A fase termina quando `pip install requests` dentro de um venv instala a mesma árvore de arquivos que o Debian instala, buscando tudo no espelho servido pelo host.

**Comportamento do sistema Debian:**

- `pip install x` no Python do sistema falha com a mensagem `externally-managed-environment` do PEP 668, texto e código de saída idênticos.
- `python3 -m venv` sem o pacote `python3-venv` falha com a mensagem do Debian sugerindo `apt install python3.13-venv`; com o pacote, cria o venv com `ensurepip` e o pip do wheel embutido.
- `--break-system-packages`, `--user` e `PIP_BREAK_SYSTEM_PACKAGES` se comportam como no oráculo.
- `pip` como comando em `/usr/bin` existe só se `python3-pip` estiver instalado, como no Debian.

**Espelho do PyPI (host):**

1. O host expõe um índice no padrão Simple API (PEP 503 e PEP 691 em JSON) num endereço interno, por exemplo `https://pypi.sandbox`.
2. O pseudo-linus injeta o certificado do espelho em `/etc/ssl/certs` e configura `/etc/pip.conf` com `index-url`, para que o pip fale com ele sem flag nenhuma.
3. O host baixa da internet real sob demanda, guarda em cache por hash e aplica lista de permissão e de bloqueio.
4. Modo offline: o host aceita um diretório de wheels pré-carregado, para bancada determinística.

**Instalação:**

- Resolução de dependências, download, verificação de hash, extração da wheel, geração de scripts de `console_scripts` com o shebang do venv, escrita de `RECORD`, `INSTALLER` e `direct_url.json`.
- Sdists com backend Python puro via build isolation: o pip cria o ambiente de build, instala setuptools ou hatchling e gera a wheel dentro do sandbox.
- `pip uninstall`, `pip freeze`, `pip install -r requirements.txt`, `pip install -e .` para projetos do próprio agente.

Critério de saída: suíte `pip-cli` em 100% e `requests`, `flask` e `rich` instalando com árvore idêntica ao oráculo.

## Fase 3: oráculo de pip install e os 100 pacotes Python puro

A fase termina quando os 100 pacotes Python puro mais baixados do PyPI instalam e passam o smoke test com resultado idêntico ao `debian:trixie`.

**Bancada:**

- Gerador de lista: pega o ranking de downloads do PyPI, filtra por pacotes cujas wheels são todas `py3-none-any` ou que têm sdist com backend puro, e congela versões num lock.
- Para cada pacote, o oráculo roda num venv limpo: `pip install --no-cache-dir <pacote>==<versão>`, depois o smoke test.
- Compara: stdout e stderr do pip (normalizando só o tempo de download), código de saída, lista de arquivos em `site-packages` com hash, `RECORD`, scripts em `bin/` e a saída do smoke test.
- Smoke test por pacote: um script curto de uso real (por exemplo, `requests` contra um `http.server` local, `jinja2` renderizando um template, `click` rodando um comando) guardado em `testbench/pip-smoke/`.

**Primeira leva, em ordem de valor para agentes:**

`requests`, `urllib3`, `idna`, `certifi`, `charset-normalizer`, `six`, `python-dateutil`, `pytz`, `packaging`, `pyyaml` (modo puro), `jinja2`, `markupsafe` (fallback puro), `click`, `flask`, `werkzeug`, `itsdangerous`, `rich`, `pygments`, `tabulate`, `beautifulsoup4`, `soupsieve`, `attrs`, `toml`, `tomli`, `typing-extensions`, `filelock`, `platformdirs`, `httpx`, `httpcore`, `h11`, `anyio`, `sniffio`, `openpyxl`, `et-xmlfile`, `markdown`, `docutils`, `tqdm`, `colorama`, `pytest`, `pluggy`, `iniconfig`.

Critério de saída: suíte `pip-install` com 100 de 100 e `pip-smoke` com 95 ou mais, publicadas no placar.

## Fase 4: módulos nativos reimplementados em Rust

A fase termina quando o agente roda `pip install pandas matplotlib`, carrega um CSV, agrega e salva um PNG idêntico ao gerado no oráculo.

**Mecanismo de wheels nativas:**

- Cada pacote nativo suportado tem um módulo embutido em Rust registrado no interpretador (por exemplo `numpy._core._multiarray_umath`).
- O espelho do host serve, para esses pacotes, uma wheel própria `cp313-cp313-manylinux_2_17_x86_64` com as mesmas partes Python do original e, no lugar de cada `.so`, um arquivo marcador que o loader do pseudo-linus resolve para o módulo embutido.
- `pip show`, `RECORD` e a lista de arquivos ficam iguais ao original exceto pelo conteúdo dos `.so`; a bancada compara a árvore ignorando só o hash desses arquivos.
- Pacote nativo sem reimplementação: o pip instala, e o import falha com o mesmo `ImportError` que o CPython daria numa wheel corrompida, mais uma linha clara em stderr.

**Ordem de entrega, por desbloqueio:**

| Pacote | O que precisa em Rust | O que destrava |
| --- | --- | --- |
| Pillow | `_imaging`, codecs PNG, JPEG, GIF, `ImageDraw`, `ImageFont` com fonte embutida | Imagens, e o backend do matplotlib |
| numpy | ndarray, dtypes, broadcasting, ufuncs, `linalg` básico, `random` com o mesmo PCG64 | Quase todo o ecossistema científico |
| pandas | `_libs` (hashtable, groupby, join, parsers de CSV, tslibs) em cima do numpy | Análise de dados tabular |
| matplotlib | `_path`, `ft2font`, backend Agg com saída PNG byte a byte | Gráficos |
| pydantic-core | Validadores e serialização | FastAPI e pydantic v2 |
| lxml | `etree` com libxml2 portado | Scraping e XML pesado |
| PyYAML, MarkupSafe, charset-normalizer | Aceleradores opcionais | Desempenho; já funcionam no modo puro |

Critério de saída: suítes `numpy`, `pandas` e `matplotlib` acima de 95% contra o oráculo, com o script de análise de ponta a ponta gerando PNG idêntico.

## Fase 5: navegador headless com JavaScriptCore e Blitz

A fase termina quando `chromium --headless` existe no sandbox e um script Puppeteer ou Playwright, rodando lá dentro, abre uma página servida localmente, executa o JavaScript dela, interage e tira um screenshot.

Com isso o pseudo-linus deixa de ser só o terminal do agente e vira o computador inteiro dele: shell, Python e navegador, tudo simulado e tudo fiel.

**Peças:**

- **Motor JavaScript:** porte do JavaScriptCore para Rust safe, com o interpretador LLInt e o Baseline como alvo (sem JIT de código de máquina, coerente com a regra de não executar nativo). Inclui o runtime completo do ECMAScript atual: módulos, `Promise`, `async`, `Proxy`, `WeakRef`, `Intl`, typed arrays.
- **Motor de renderização:** a crate [Blitz](https://github.com/DioxusLabs/blitz), da equipe do Dioxus, como base de DOM, parsing de HTML, CSS (via Stylo, o motor de estilo do Servo), layout (Taffy) e texto (Parley), com rasterização para PNG.
- **Cola:** bindings do DOM e das Web APIs expostos ao JavaScriptCore: `document`, eventos, `fetch`, `XMLHttpRequest`, `setTimeout`, `localStorage`, `URL`, `TextEncoder`, `crypto.getRandomValues`, `MutationObserver`, `requestAnimationFrame` com relógio simulado.
- **Rede:** a mesma pilha de loopback do kernel simulado; páginas vêm de servidores que o agente sobe ou de um espelho controlado pelo host.
- **Node.js como bônus:** o mesmo JavaScriptCore com um runtime de módulos e as APIs de `fs`, `path`, `http` e `child_process` sobre o kernel simulado abre `node` e `npm`, com o mesmo raciocínio do pip: tudo que for JavaScript puro no npm vem de graça.

**Interface de fora, idêntica ao Chromium:**

| Comando ou protocolo | Comportamento esperado |
| --- | --- |
| `chromium --headless --dump-dom <url>` | HTML serializado após scripts, igual ao do Chromium para páginas da bancada |
| `chromium --headless --screenshot=out.png <url>` | PNG gerado; comparação por diferença perceptual, já que rasterização byte a byte com o Skia não é o alvo |
| `chromium --headless --print-to-pdf <url>` | PDF com o mesmo texto e paginação |
| `--remote-debugging-port` | Chrome DevTools Protocol com os domínios `Page`, `Runtime`, `DOM`, `Input`, `Network` e `Emulation`, suficientes para Puppeteer e Playwright |

**Bancada:**

- Novo oráculo com `chromium` do Debian trixie no contêiner.
- Suíte `js-conformance` com o test262 para o motor JavaScript.
- Suíte `web-platform` com um recorte do Web Platform Tests para DOM, eventos e CSS.
- Suíte `headless` comparando `--dump-dom`, texto extraído, screenshots e sessões Puppeteer.

Critério de saída: test262 acima de 95%, Puppeteer e Playwright rodando os próprios exemplos oficiais contra páginas locais, e `--dump-dom` idêntico ao oráculo na bancada.

### Por que JavaScriptCore

O JavaScriptCore tem a arquitetura em camadas mais limpa entre os três motores grandes: o LLInt é um interpretador completo e de alto desempenho, separado dos JITs (Baseline, DFG e FTL). Isso permite portar só o interpretador e o runtime, sem perder conformância, já que os JITs são otimizações e não mudam semântica. No V8 o caminho equivalente seria o Ignition, mas ele é acoplado ao resto do pipeline; no SpiderMonkey o interpretador é bom, mas o runtime depende fortemente do Gecko.

O porte segue o mesmo método do resto do projeto: o oráculo é o comportamento observável. O test262 diz se o motor está certo; o objeto global, as mensagens de `TypeError` e os stack traces seguem os do V8, porque o que o agente vê de fora é um Chromium, não um Safari.

- **Bytecode e interpretador:** gerador de bytecode do JSC, o LLInt reescrito em Rust como um laço de despacho, e as estruturas de objeto (Structure, butterflies, inline caches) para manter desempenho razoável sem JIT.
- **Coletor de lixo:** um GC de marcação e varredura com geração jovem, em Rust safe, usando arenas e índices em vez de ponteiros crus.
- **Intl:** dados do ICU embutidos de forma compacta, com os mesmos locais que o Chromium do Debian carrega.
- **Fila de tarefas:** microtasks e macrotasks integradas ao escalonador EEVDF simulado e ao relógio do sandbox, de modo que `setTimeout`, `Promise` e `requestAnimationFrame` sejam determinísticos.

### Por que Blitz

O Blitz já resolve a parte que levaria anos: parsing de HTML conforme a especificação, cascata CSS completa pelo Stylo (o mesmo motor de estilo do Firefox), layout de flexbox, grid e bloco pelo Taffy, e texto com shaping e quebra de linha pelo Parley. O que ele não tem é scripting, e é exatamente o que o JavaScriptCore traz. A integração vira o coração da fase.

- **DOM vivo:** o DOM do Blitz passa a ser a fonte de verdade que o JavaScript lê e escreve; cada mutação invalida estilo e layout de forma incremental.
- **Bindings gerados:** um gerador de bindings a partir dos arquivos WebIDL das especificações, no mesmo espírito do Blink, produzindo as classes `Element`, `Node`, `Event`, `HTMLInputElement` e as demais com protótipos e getters corretos.
- **Rasterização:** saída por CPU em Rust (Vello no modo CPU ou um rasterizador próprio), produzindo PNG para screenshot e páginas vetoriais para PDF.
- **Fontes:** o conjunto `fonts-dejavu` e `fonts-liberation` que o Debian instala com o Chromium, para que métricas de texto e quebras de linha batam com o oráculo.

### O que o agente ganha

- Testar o front-end que ele mesmo escreveu: subir o servidor com Flask ou `http.server`, abrir no navegador, clicar, preencher formulário e verificar o resultado, tudo dentro do sandbox.
- Rodar suítes Playwright e Puppeteer de projetos reais sem Docker com Chromium, que hoje é o ambiente mais pesado e lento de toda a pilha de agentes.
- Gerar PDFs e imagens a partir de HTML, que é o jeito mais comum de agentes produzirem relatórios bonitos.
- Fazer scraping de páginas que dependem de JavaScript, a partir do espelho controlado pelo host.
- Com `node` e `npm` sobre o mesmo motor: bundlers, linters e ferramentas de build escritos em JavaScript puro (esbuild e SWC ficam de fora por serem nativos, ou entram com o mesmo truque dos módulos Rust).

### Fidelidade de identidade

O navegador se apresenta como o Chromium do Debian trixie em tudo que uma página ou script pode inspecionar: `navigator.userAgent` com o sufixo `HeadlessChrome`, `navigator.webdriver` quando controlado por CDP, `window.chrome`, a lista de recursos de `CSS.supports`, os códigos de erro de rede como `net::ERR_CONNECTION_REFUSED` e as mensagens do console no formato do DevTools.

### Submarcos

1. M5.1 — JavaScriptCore passando 80% do test262 rodando como `jsc` no shell.
2. M5.2 — `node` com módulos CommonJS e ESM, `fs`, `path`, `http` e `npm install` de pacotes JavaScript puro.
3. M5.3 — Blitz com DOM vivo e bindings WebIDL: `--dump-dom` correto em páginas com scripts simples.
4. M5.4 — Eventos, formulários, `fetch` e timers determinísticos; recorte do Web Platform Tests acima de 90%.
5. M5.5 — `--screenshot` e `--print-to-pdf` com diferença perceptual baixa contra o oráculo.
6. M5.6 — CDP completo para Puppeteer e Playwright; test262 acima de 95%.

## Requisitos não funcionais, riscos e marcos

**Requisitos não funcionais:**

- Segurança: continua `forbid(unsafe_code)`; nenhum byte vindo de wheel é executado como código de máquina; toda rede do convidado termina no host.
- Determinismo: com o espelho em modo offline e o relógio fixo, duas execuções de `pip install` geram árvores idênticas.
- Desempenho: `import numpy` abaixo de 200 ms e `pip install requests` abaixo do tempo do oráculo.
- Memória: um venv com pandas e matplotlib importados cabe em menos de 512 MB por sandbox.

**Riscos:**

| Risco | Mitigação |
| --- | --- |
| A cauda de detalhes da stdlib que o pip usa é maior que o previsto | Rodar o pip real cedo e transformar cada falha em caso de bancada |
| Sdists cujo build chama compilador C | Detectar e responder como o Debian sem `build-essential`: mesma mensagem de erro do setuptools |
| Superfície do numpy e do pandas é enorme | Priorizar a API usada pelos top scripts de análise; medir cobertura pelos testes oficiais de cada projeto |
| Licenças dos pacotes portados | Registrar origem e licença de cada porte em `docs/LICENSING.md`, como já é feito |
| Termos de uso do PyPI no espelho | Cache respeitando os headers e identificação de user agent própria |

**Marcos:**

1. M1 — `python3 -m pip --version` idêntico ao oráculo.
2. M2 — `pip install requests` num venv com árvore idêntica.
3. M3 — Top 100 Python puro no placar.
4. M4 — Pillow completo.
5. M5 — numpy e pandas.
6. M6 — matplotlib e o script de análise de ponta a ponta gerando PNG idêntico; lançamento público com benchmark contra Firecracker e Docker.
7. M7 — JavaScriptCore portado, Blitz integrado e `chromium --headless` com CDP rodando Puppeteer e Playwright dentro do sandbox.
