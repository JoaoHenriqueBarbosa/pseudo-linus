// 1 milhão de decodeURI e de decodeURIComponent (o número solto que existia).
let total = 0;
for (let i = 0; i < 1000000; i++) total += decodeURI('%41%42%C3%A9%E2%82%AC' + (i & 7)).length;
for (let i = 0; i < 1000000; i++) total += decodeURIComponent('a%2Fb%20c' + (i & 7)).length;
console.log(total);
