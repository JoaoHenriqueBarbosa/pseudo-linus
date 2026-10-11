/* a */ foo(/* b */ 1 /* c */, /* d */ 2) /* e */;
function /* f */ g /* h */ (/* i */) /* j */ { /* k */ }
const o = { /* l */ a: 1, /* m */ b /* n */: 2 };
const arr = [ /* o */ 1, /* p */ 2 /* q */ ];
if (/* r */ a /* s */) /* t */ b; /* u */ else /* v */ c;
// leading
// multiple
foo(); // trailing
/* before */ bar(); /* after */
export /* w */ const x = 1;
/* @__PURE__ */ foo();
/* #__PURE__ */ bar();
/* @__NO_SIDE_EFFECTS__ */ function pure() {}
const y = /* @__PURE__ */ (() => 1)();
var a, b, c;
