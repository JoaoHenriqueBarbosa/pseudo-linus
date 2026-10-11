// Array.sort de números (comparador) e de strings, com um gerador congruencial fixo.
let seed = 12345;
const next = () => (seed = (seed * 1103515245 + 12345) & 0x7fffffff);
const numbers = Array.from({ length: 300000 }, next);
const strings = numbers.slice(0, 100000).map((n) => 'k' + n.toString(36));
numbers.sort((a, b) => a - b);
strings.sort();
console.log(numbers[0], numbers[numbers.length - 1], strings[0], strings[strings.length - 1]);
