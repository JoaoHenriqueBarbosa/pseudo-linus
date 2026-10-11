// concatenação de strings (rope), join, split e indexOf sobre texto de 2 MB.
let s = '';
for (let i = 0; i < 400000; i++) s += 'item' + i + ',';
const parts = s.split(',');
let total = 0;
for (let i = 0; i < parts.length; i++) total += parts[i].length;
const joined = parts.join(';');
let found = 0;
for (let i = 0; i < 2000; i++) found += joined.indexOf('item' + (i * 150) + ';') >= 0 ? 1 : 0;
console.log(s.length, parts.length, total, joined.length, found);
