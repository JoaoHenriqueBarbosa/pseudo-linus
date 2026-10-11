// RegExp: busca global, replace com função, grupos nomeados e backtracking leve sobre 1 MB de texto.
let text = '';
for (let i = 0; i < 40000; i++) text += 'user' + i + '@example' + (i % 13) + '.com 2026-01-' + (10 + (i % 18)) + ' foo_bar-' + i + '\n';
const emails = text.match(/[a-z0-9]+@[a-z0-9]+\.com/g).length;
const dates = [...text.matchAll(/(?<y>\d{4})-(?<m>\d{2})-(?<d>\d{2})/g)].length;
const replaced = text.replace(/foo_(\w+)-(\d+)/g, (_, a, b) => a.toUpperCase() + b).length;
const lines = text.split(/\r?\n/).filter((line) => /^user\d*7@/.test(line)).length;
console.log(text.length, emails, dates, replaced, lines);
