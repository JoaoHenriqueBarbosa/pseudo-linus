// Carrega e executa o bundle como o Playwright o faz (`require`): `arquivo` como argumento. Diz onde falhou.
const file = process.argv[2];
try {
  const exported = require(file);
  console.log('carregou', typeof exported, Object.keys(exported || {}).length, 'chaves');
} catch (error) {
  console.log('falhou em require:', String(error && error.message).split('\n')[0].slice(0, 200));
}
