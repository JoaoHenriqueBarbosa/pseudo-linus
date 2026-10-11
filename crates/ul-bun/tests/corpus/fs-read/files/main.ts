import { readFileSync, existsSync } from 'node:fs';
const text: string = readFileSync('./note.txt', 'utf8');
console.log(text.trim().toUpperCase(), existsSync('./note.txt'), existsSync('./nope'));
