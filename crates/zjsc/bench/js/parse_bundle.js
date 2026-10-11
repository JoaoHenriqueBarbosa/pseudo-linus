// Compila (parse + geração de bytecode, sem executar) um arquivo grande: `arquivo` como argumento. O texto entra por
// `import(..., { with: { type: 'text' } })`, que os dois motores leem pelo carregador de módulos (sem `node:fs`).
const file = process.argv[2];
const source = (await import(file, { with: { type: 'text' } })).default;
let count = 0;
for (let i = 0; i < 3; i++) { new Function('require', 'module', 'exports', source); count++; }
console.log('compilou', count, 'vezes', source.length, 'caracteres');
