// JSON.stringify e JSON.parse de um documento de cerca de 1 MB, 5 voltas.
const doc = [];
for (let i = 0; i < 6000; i++) {
  doc.push({ id: i, name: 'usuario-' + i, tags: ['a', 'b', 'c' + (i % 7)], score: i * 1.5, nested: { ok: i % 2 === 0, text: 'linha ' + i + ' com "aspas" e \\ barra', list: [1, 2, 3, i] } });
}
let size = 0;
let check = 0;
for (let round = 0; round < 5; round++) {
  const text = JSON.stringify(doc);
  size = text.length;
  check += JSON.parse(text).length;
}
console.log(size, check);
