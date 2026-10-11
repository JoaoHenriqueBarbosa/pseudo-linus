import data from './data.json';
const d: { name: string; list: number[] } = data;
console.log(d.name, d.list.reduce((a, b) => a + b, 0), JSON.stringify(d));
