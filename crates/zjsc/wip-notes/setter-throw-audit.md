# Golden de getter e setter que lançam

Golden: `tests/golden/setter_throw_bun.tsv` (2905 programas, 638 resultados distintos), gerado por
`scripts/gen-setter-throw-golden.js` no bun (um processo filho por programa, `vm.runInThisContext`, sem APIs de host,
captura `globalThis.R`), rodado por `tests/setter_throw_bun_golden.rs` (thread de 256 MiB,
`VM::set_thread_stack_budget`, `evaluate_script_sequence_result`, piso de 2500 programas). O teste NÃO foi rodado
(regra da tarefa: sem cargo).

## O que a grade cobre

- 96 operações: atribuição por nome, colchete e índice; destructuring de array e objeto com iterador com `return()`,
  sem ele e com `return()` que lança; compound (`+=`, `??=`, `||=`, `&&=`, `++`, `--`, `**=`); leitura por getter
  (spread, `Object.assign`, `JSON.stringify`, optional chaining); `super.x`; Proxy e `Reflect.set` com receiver;
  acessores estáticos e privados de classe.
- 32 posições: última instrução do try, meio, finally, catch, for-of com break e return, for-in, for clássico com
  continue, return sobrescrito pelo finally, `break` de rótulo no finally, generator (`next`, `return`, `throw`),
  async (só o trecho síncrono), switch, parâmetro padrão, bloco estático.
- 4 comportamentos (getter e setter lançam ou não) e variante estrita em parte da grade.
- A grade completa tem cerca de 11 mil programas; o gerador guarda 1 em cada 4, determinístico, para a geração caber
  em menos de um minuto. Descartados na geração: 83 (programa que lançou sem captura, caminho ou marca no resultado).

## Cuidados

- Todo efeito do log é síncrono, então o resultado não depende de o porte esvaziar microtarefas antes de ler `R`.
- Mensagens de erro do próprio motor aparecem no log (por exemplo `o.x.y.z` sobre `undefined`); divergência de texto
  ali é divergência real de mensagem.
