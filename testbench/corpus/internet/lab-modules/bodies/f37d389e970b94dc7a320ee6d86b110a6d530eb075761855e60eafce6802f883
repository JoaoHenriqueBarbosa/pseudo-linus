// Registro único por URL: app.mjs e o inline importam o mesmo estado.
export let counter = 0;
export function bump() {
  counter += 1;
}
import { ping } from './cycle-a.mjs';
log('state.mjs avaliado uma vez; ciclo: ' + ping());
