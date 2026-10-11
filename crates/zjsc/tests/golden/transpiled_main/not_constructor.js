const a = { b: 1 };
// comentário que o impressor tira
try { new a.b(); } catch (e) { console.log(e.message); }
try { new a.b(1,2); } catch (e) { console.log(e.message); }
try { new (a.b)(); } catch (e) { console.log(e.message); }
try { new a["class"](); } catch (e) { console.log(e.message); }
const k = "b";
try { new a[k](); } catch (e) { console.log(e.message); }
const g = 1;
try { new g(); } catch (e) { console.log(e.message); }
try { new 日(); } catch (e) { console.log(e.message); }
var 日 = 1;
try { a.x.y; } catch (e) { console.log(e.message); }
try { a.nope(); } catch (e) { console.log(e.message); }
try { a["não"](); } catch (e) { console.log(e.message); }
try { undefined(); } catch (e) { console.log(e.message); }
try { null.p; } catch (e) { console.log(e.message); }
function f() {
  const dead = 1;
  const t = `plain`;
  return typeof undefined === "undefined" ? void 0 : t;
}
try { new f.x(); } catch (e) { console.log(e.message); }
