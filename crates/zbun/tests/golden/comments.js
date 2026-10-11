// line comment
/* block comment */
/** jsdoc */
function f(/* inline */ a, b /* after */) {
  // inside
  return a /* mid */ + b; // trailing
}
const x = /* pre */ 1;
/* istanbul ignore next */
const y = 2;
// @__PURE__
const z = /* @__PURE__ */ f(1, 2);
/*! legal */
export {};
