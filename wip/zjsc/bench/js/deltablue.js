// Solucionador de restrições incremental no estilo do DeltaBlue (Octane): cadeia de igualdades e projeção de
// escala, com o plano extraído por propagação para frente. Versão reduzida, escrita para este baseline (não é o
// DeltaBlue completo do Octane: não tem a remoção incremental de restrições nem o replanejamento).
function Strength(value) { this.value = value; }
const REQUIRED = new Strength(0), STRONG = new Strength(2), NORMAL = new Strength(4), WEAKEST = new Strength(6);
const stronger = (a, b) => a.value < b.value;

function Variable(name, value) {
  this.name = name; this.value = value; this.constraints = []; this.determinedBy = null; this.mark = 0;
}
function Constraint(strength) { this.strength = strength; }
function EqualityConstraint(v1, v2, strength) { Constraint.call(this, strength); this.v1 = v1; this.v2 = v2; this.direction = 0; }
EqualityConstraint.prototype = Object.create(Constraint.prototype);
function ScaleConstraint(src, scale, offset, dest, strength) {
  Constraint.call(this, strength); this.src = src; this.scale = scale; this.offset = offset; this.dest = dest; this.direction = 0;
}
ScaleConstraint.prototype = Object.create(Constraint.prototype);
function StayConstraint(v, strength) { Constraint.call(this, strength); this.v = v; }
StayConstraint.prototype = Object.create(Constraint.prototype);
function EditConstraint(v, strength) { Constraint.call(this, strength); this.v = v; }
EditConstraint.prototype = Object.create(Constraint.prototype);

EqualityConstraint.prototype.output = function () { return this.direction === 1 ? this.v2 : this.v1; };
EqualityConstraint.prototype.input = function () { return this.direction === 1 ? this.v1 : this.v2; };
EqualityConstraint.prototype.execute = function () { this.output().value = this.input().value; };
ScaleConstraint.prototype.output = function () { return this.direction === 1 ? this.dest : this.src; };
ScaleConstraint.prototype.input = function () { return this.direction === 1 ? this.src : this.dest; };
ScaleConstraint.prototype.execute = function () {
  if (this.direction === 1) this.dest.value = this.src.value * this.scale.value + this.offset.value;
  else this.src.value = (this.dest.value - this.offset.value) / this.scale.value;
};
StayConstraint.prototype.output = function () { return this.v; };
StayConstraint.prototype.input = function () { return null; };
StayConstraint.prototype.execute = function () {};
EditConstraint.prototype.output = function () { return this.v; };
EditConstraint.prototype.input = function () { return null; };
EditConstraint.prototype.execute = function () {};

function Planner() { this.mark = 0; }
Planner.prototype.add = function (c, vars) {
  for (const v of vars) v.constraints.push(c);
  c.direction = 1;
  const out = c.output();
  if (out.determinedBy === null || stronger(c.strength, out.determinedBy.strength)) out.determinedBy = c;
};
Planner.prototype.makePlan = function (sources) {
  const mark = ++this.mark, plan = [], todo = sources.slice();
  while (todo.length > 0) {
    const c = todo.pop(), out = c.output();
    if (out.mark === mark) continue;
    out.mark = mark; plan.push(c);
    for (const next of out.constraints) if (next !== c && next.input && next.input() === out) todo.push(next);
  }
  return plan;
};

function chainTest(n) {
  const planner = new Planner(), vars = [];
  for (let i = 0; i <= n; i++) vars.push(new Variable('v' + i, 0));
  for (let i = 0; i < n; i++) planner.add(new EqualityConstraint(vars[i], vars[i + 1], REQUIRED), [vars[i], vars[i + 1]]);
  const first = vars[0], last = vars[n];
  const stay = new StayConstraint(last, STRONG), edit = new EditConstraint(first, STRONG);
  planner.add(stay, [last]); planner.add(edit, [first]);
  const plan = planner.makePlan([edit]);
  for (let i = 0; i < 100; i++) {
    first.value = i;
    for (const c of plan) c.execute();
    if (last.value !== i) throw new Error('Chain test falhou em ' + n + ': ' + last.value + ' != ' + i);
  }
}
function projectionTest(n) {
  const planner = new Planner(), scale = new Variable('scale', 10), offset = new Variable('offset', 1000);
  let src = null, dst = null;
  for (let i = 0; i < n; i++) {
    src = new Variable('src' + i, i); dst = new Variable('dst' + i, i);
    planner.add(new StayConstraint(src, NORMAL), [src]);
    planner.add(new ScaleConstraint(src, scale, offset, dst, REQUIRED), [src, scale, offset, dst]);
  }
  const edit = new EditConstraint(src, STRONG); planner.add(edit, [src]);
  const plan = planner.makePlan([edit]);
  src.value = 17; for (const c of plan) c.execute();
  if (dst.value !== 1170) throw new Error('Projection falhou: ' + dst.value);
}
for (let i = 0; i < 60; i++) { chainTest(100); projectionTest(100); chainTest(2000); }
console.log('deltablue ok');
