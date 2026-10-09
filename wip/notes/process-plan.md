# Plano do global `process` (porte do JavaScriptCore, zjsc)

Golden: `tests/golden/process_bun.tsv` (659 casos, prelúdio em `tests/golden/process.preludes.json`), gerado por
`scripts/gen-process-golden.js` contra o bun 1.4.2. Duas rodadas idênticas, 0 descartados.

## Formato e como o teste deve rodar

- Cada linha é um programa que roda como `/app/main.js` (ou `/app/main.mjs` se o fonte tem `/*mjs*/`), num processo
  próprio, com cwd `/app`, argv extra `x --flag`, argv0 `bun` e ambiente
  `{PATH, HOME, NO_COLOR=1, FOO=bar, EMPTY=, mixedCase=1}`.
- O resultado é `stdout + "#stderr\n" + stderr + "#exit " + código` (ou `signal:SIGTERM`), com o pid trocado por
  `<pid>` em `(node:<pid>)`. O lado Rust é um `tests/process_bun_golden.rs` irmão de `uncaught_bun_golden.rs`,
  que compara stdout, stderr e código de saída.
- O prelúdio usa só `console.log` e `JSON.stringify`, então o console do porte precisa funcionar antes de tudo.
- Valores do host real entram por forma: pid, ppid, execPath (basename `bun`), uptime, hrtime, memoryUsage.
  Literais: `platform` 'linux', `arch` 'x64', `version` 'v26.3.0', `versions.bun` '1.4.2', `title` 'bun'.
  A versão do node (v26.3.0) é a que o bun finge ser.

## Regras surpreendentes medidas

nextTick e ordem:
1. Depois do script principal os ticks rodam ANTES de todas as microtarefas (promessa, queueMicrotask, await),
   mesmo quando o tick foi agendado depois. Vale também em módulo ESM (diferente do Node).
2. Tick agendado de dentro de uma microtarefa roda logo depois DESSA microtarefa, antes das irmãs já enfileiradas
   (`t1 | tick | t2 | t3`). Mas se o esvaziamento das microtarefas começou de dentro de um tick, as microtarefas
   novas rodam primeiro e o tick novo só depois (`m | m2 | t2`). O porte precisa de dois modos de drenagem.
3. Dentro de callback de timer e de immediate, os ticks rodam depois de CADA callback (`a ta b tb c`), e antes das
   microtarefas (`a | b | m` no caso do timer).
4. Ticks recursivos (100000 níveis) funcionam e deixam timer, immediate e promessa esperando até o fim.
5. `setImmediate` roda antes de `setTimeout(0)` no script principal (`imm | timeout`), nos dois lados do ESM.
6. `this` do callback é `undefined` mesmo em função sloppy (`typeof this` vale 'undefined'). Argumentos extras
   passam inteiros (6 argumentos funcionam). Retorna `undefined`. `new process.nextTick(f)` não lança.
7. Validação síncrona: `TypeError` com `code` 'ERR_INVALID_ARG_TYPE' e mensagem
   `The "callback" argument must be of type function. Received undefined` (variantes `Received type string ('str')`,
   `an instance of Object`, `null`). `code` NÃO é propriedade própria (`getOwnPropertyDescriptor` devolve undefined);
   o `toString` mostra `TypeError [ERR_INVALID_ARG_TYPE]: ...` mas `e.stack` e `e.name` não.
8. Classe passada ao nextTick não lança na chamada: lança depois, dentro do tick (`Cannot call a class constructor`).
9. Erro dentro de tick sem handler: os OUTROS ticks já enfileirados ainda rodam, depois o processo morre com código 1.
   Com handler de uncaughtException os ticks e microtarefas seguintes continuam.

exit e exitCode:
10. `exitCode` é acessor NÃO configurável (`{enumerable:true, configurable:false}`): `delete process.exitCode` lança
    `TypeError: Unable to delete property.`. Setar `undefined` depois de um valor NÃO limpa (continua 3).
    Setar `null` ou `undefined` direto fica `undefined`. Valor numérico em string ('4') vira número 4. 300 vira 44 já
    na leitura, -1 vira 255, 1.5 lança `RangeError ERR_OUT_OF_RANGE`, 'abc' lança `ERR_INVALID_ARG_TYPE`.
11. `process.exit(2**32+3)` sai 3, `exit(256)` sai 0, `exit(null/undefined)` sai 0 (ou o exitCode), `exit('3')` sai 3,
    `exit(1.5)`, `exit(NaN)`, `exit(Infinity)` lançam RangeError, `exit(true/'abc'/{}/1n)` lançam TypeError.
12. Handler de `exit` recebe o código (1 argumento). Mudar `process.exitCode` dentro do handler muda o código final.
    Timer, immediate, tick, promessa agendados dentro do handler de exit NÃO rodam. `process.exit()` dentro do handler
    não reentra (o handler roda uma vez). Throw dentro do handler de exit dá código 1.
13. `process.exit()` dentro de um tick roda o `exit` e descarta os ticks seguintes. Tick pendente, microtarefa,
    timer e immediate pendentes são descartados quando `exit()` é chamado em seguida, no mesmo script.
14. Exceção não tratada: o handler de exit vê código 1. Rejeição não tratada: o handler de exit vê código 0 e
    `process.exitCode` undefined, mas o processo sai 1 (e o stderr sai antes ou depois conforme o caso, medir no teste).
15. `beforeExit`: emitido com o exitCode atual; timer ou immediate agendado dentro dele revive o laço e o
    evento repete (3 vezes no caso medido). nextTick agendado dentro roda uma vez e NÃO re-emite; promessa e
    queueMicrotask agendados dentro do beforeExit NUNCA rodam. Não é emitido após `exit()`, nem após exceção fatal.
    No ESM com top-level await o `beforeExit` vem depois do `await`.
16. Sinal: `kill(pid,'SIGTERM')` sem handler mata sem emitir `exit` (`signal:SIGTERM`); com handler recebe
    `('SIGUSR2', 12)`. `abort()` mata com SIGABRT sem `exit`.

uncaughtException e unhandledRejection:
17. Throw síncrono no módulo principal com handler de uncaughtException chama o handler com origem
    `'unhandledRejection'` (o módulo principal roda como promessa), e o monitor também vê `'unhandledRejection'`.
    Throw em timer, immediate, queueMicrotask, tick: origem `'uncaughtException'`, e o resto continua.
18. Com SÓ `uncaughtException` registrado, uma rejeição não tratada (`Promise.reject`, throw em `.then`, async
    function, executor) NÃO chega ao handler: o processo morre com código 1. Com `unhandledRejection` registrado ela
    chega com `(reason, promise)`; throw dentro desse handler vai para o `uncaughtException` se existir, senão
    imprime o erro e sai 1.
19. Throw dentro de handler de uncaughtException: sai com código 7 (stderr com o novo erro).
20. unhandledRejection dispara depois de tick e microtarefas, antes de immediate e timer (`tick | then | unhandled | imm`).
    Rejeição tratada tarde: `rejectionHandled` com a promessa. `process.once('unhandledRejection')` deixa a segunda
    rejeição cair no padrão (crash).
21. Saída fatal no stderr: trecho do fonte com `^`, `error: msg` (Error comum mostra `error:` minúsculo, TypeError
    mostra `TypeError: msg`), props extras do erro, `at ...`, linha em branco e `Bun v1.4.2 (Linux x64)`;
    valores não-Error: `error: str` + o valor, objeto como `error` + literal. Já coberto por `uncaught_bun_golden`.
22. `process.emit('uncaughtException', e)` sem listener não faz nada (não derruba). `setUncaughtExceptionCaptureCallback`
    tem prioridade sobre o handler; duas chamadas lançam `ERR_UNCAUGHT_EXCEPTION_CAPTURE_ALREADY_SET`.

forma, env e demais:
23. `process` tem o próprio `Symbol.toStringTag` = 'process' (enumerável, gravável). O protótipo tem `constructor`
    `EventEmitter` e NÃO é o `EventEmitter.prototype` do `require('events')`, mas `process instanceof EventEmitter` vale.
    `eventNames()` já começa com `['warning']`. `Object.keys(process)` tem as mesmas ~80 chaves de
    `getOwnPropertyNames` (tudo enumerável); ordem fixa medida no golden. Acessores próprios: `_eval`, `argv`,
    `connected`, `debugPort`, `execArgv`, `exitCode`, `ppid`, `title`. `process.stdout/stderr/stdin` são
    propriedades de dados. `isBun` true, `browser` false.
24. `process.env`: objeto de protótipo `Object.prototype` (não Proxy), chaves distintas por caixa (sensível), valores
    sempre string (`env.U = undefined` vira 'undefined', número vira '1', objeto '[object Object]'). Descritores
    sempre `{writable, enumerable, configurable: true}`; `defineProperty` só aceita esse formato
    (`ERR_INVALID_OBJECT_DEFINE_PROPERTY`), `Object.freeze/seal(process.env)` lançam, `preventExtensions` funciona
    e depois `env.Q = 'x'` lança "not extensible". Chave '' é ignorada; símbolo como chave lança no set.
    `Reflect.ownKeys(process.env)` traz 15 nomes (BUN_CONFIG_VERBOSE_FETCH, HTTPS_PROXY, TZ... não enumeráveis) enquanto
    `Object.keys` traz os 6 reais, na ordem do ambiente (PATH,HOME,NO_COLOR,FOO,EMPTY,mixedCase). `Bun.env === process.env`.
    Mudança em `process.env` aparece no ambiente de processo filho.
25. `argv[0]` é o execPath absoluto, `argv0` é 'bun' (diferente de argv[0]). `execArgv` `[]`. `argv[1]` é o caminho
    do script. `process.argv = [...]` é gravável. `chdir` erro: `ENOENT: no such file or directory, chdir '/app' -> '/x'`
    com `syscall 'chdir'`, `errno -2`, `path` = cwd antigo. `platform`/`arch`/`version`/`pid` são dados graváveis
    (`process.platform = 'win32'` pega). `process.pid = 1` não pega (silencioso).
26. `hrtime(prev)` valida: não-Array `ERR_INVALID_ARG_TYPE`, tamanho != 2 `ERR_OUT_OF_RANGE` ('It must be 2'),
    `['a','b']` e `null` tratados diferente (`null` lança, `undefined` não). `hrtime.bigint()` não tem relação com
    `uptime()` (menor que uptime em ns). `memoryUsage()` chaves `rss, heapTotal, heapUsed, external, arrayBuffers`,
    nem todas > 0 (alguma pode ser 0), `memoryUsage.rss()` número.
27. `emitWarning`: emite o evento `warning` num tick (depois do síncrono, antes de tick/then agendados depois, antes de
    immediate), imprime `(node:PID) Warning: msg` (ATENÇÃO: prefixo `node:`, não `bun:`) e na primeira vez a linha
    `(Use \`bun --trace-warnings ...\` to show where the warning was created)`; com `code`: `[CODE] Name: msg`;
    `detail` na linha seguinte. Warning é `Error` comum com `name` trocado. `noDeprecation` suprime só
    DeprecationWarning (e o evento também). `throwDeprecation` lança no tick seguinte (vira uncaught).
    Argumento ruim: `ERR_INVALID_ARG_TYPE` ('warning' / 'type' / 'code'). Nenhum aviso por ter mais de 10 ouvintes
    no próprio `process` (diferente de um EventEmitter comum, que avisa `MaxListenersExceededWarning`).

## Fatias para implementar (~5 min cada), nesta ordem

1. **nextTick (fila e ordem)**: fila FIFO própria no loop de eventos; drenar depois do script principal e depois de
   cada callback de timer/immediate/microtarefa de entrada (regras 1 a 4); dois modos de drenagem (regra 2);
   `this` undefined, args extras, retorno undefined (regra 6). Casos 147 a 188 e 189 a 211, 239 a 249 do golden
   (índices na ordem do tsv). Parar quando a fila vira vazia sem tocar em timers.
2. **nextTick (validação e erro)**: `ERR_INVALID_ARG_TYPE` sem `code` próprio, classe adiada, erro dentro de tick
   mantém os ticks restantes (regras 7 a 9). Casos 212 a 238.
3. **exit, exitCode, eventos exit/beforeExit**: acessor não configurável, coerção mod 256, validação, o laço de
   saída (`beforeExit` com revive por timer/immediate, descarte do que agendar em `exit`), código final
   (regras 10 a 16). Casos 250 a 334 e 644 a 657.
4. **uncaughtException/unhandledRejection/monitor/capture**: origem 'unhandledRejection' no módulo principal,
   política "só uncaught não pega rejeição", código 7, rejectionHandled, momento da entrega (regras 17 a 22).
   Casos 335 a 426.
5. **forma de process e EventEmitter**: toStringTag próprio, protótipo EventEmitter, lista de chaves na ordem do
   golden, descritores por chave, `eventNames` com 'warning' (regra 23). Casos 0 a 146 (cada chave tem uma linha).
6. **env**: objeto com coerção para string, descritores, `Reflect.ownKeys` com os 15 nomes, herança para filhos
   (regra 24). Casos 477 a 539.
7. **argv, cwd, chdir, platform, version, versions, release, config, features, title, pid, umask, kill, stdout**:
   valores literais do Debian 13 fingido e erros de chdir/kill (regra 25). Casos 540 a 600.
8. **hrtime, hrtime.bigint, memoryUsage, uptime, cpuUsage, resourceUsage** (regra 26). Casos 601 a 636.
9. **emitWarning e evento warning** (regra 27). Casos 427 a 476.
10. **stdout/stderr entrelaçados com ticks** (`write` com callback síncrono, ordem com `console.log`). Casos 637 a 643.

Dependências: console (já tem golden), timers (golden pronto), `events` (EventEmitter), `child_process` só para 3 casos de
env herdado (pode ficar por último).

## Pendências do gerador

- Os índices acima são aproximados (a ordem do tsv segue a ordem de `add` em `gen-process-golden.js`); conferir com
  `readRows("process", ...)` antes de ligar o teste.
- O dir temporário é normalizado para `/app`; stderr com `(node:PID)` troca o pid por `<pid>`.
