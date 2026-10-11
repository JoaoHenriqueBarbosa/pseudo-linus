import { add, type Op } from './lib.ts';
import util from './util';
import { tag } from './tag.js';
const op: Op = add;
console.log(op(2, 3), util.name, tag('x'));
