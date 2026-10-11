/** @jsxRuntime classic */
/** @jsx h */
/** @jsxFrag Frag */
const Frag = Symbol('frag');
function h(type, props, ...children) { return { type: typeof type === 'string' ? type : String(type.description || type.name), props, children }; }
console.log(JSON.stringify(<ul class="l"><li>a</li><li>{1 + 1}</li></ul>));
console.log(JSON.stringify(<><i /></>));
