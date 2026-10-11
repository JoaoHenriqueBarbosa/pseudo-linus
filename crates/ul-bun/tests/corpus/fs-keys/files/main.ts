import fs from 'node:fs';
import fsp from 'node:fs/promises';
console.log(fsp === fs.promises, Object.keys(fs).join());
console.log(Object.keys(fs.promises).join());
