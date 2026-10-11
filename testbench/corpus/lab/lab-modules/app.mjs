import { counter, bump } from './state.mjs';
import { describe } from './lib/describe.mjs';

bump();
log('app.mjs: counter=' + counter + ' ' + describe(import.meta.url));
const config = await (await fetch('./config.json')).json();
log('app.mjs: config.name=' + config.name + ' (await de topo com fetch)');
