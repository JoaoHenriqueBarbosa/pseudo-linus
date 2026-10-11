const a = <div>  leading and trailing  </div>;
const b = <div>
  multi
  line
</div>;
const c = <div>&nbsp;&amp;&lt;&gt;&quot;&#65;&#x41;&copy;</div>;
const d = <a:b c:d="e" />;
const e = <A.B.C x />;
const f = <div {...a} {...b} />;
const g = <div>{...children}</div>;
const h = <div>{/* comment */}</div>;
const i = <div>{}</div>;
const j = <>{a}{b}</>;
const k = <div a="1" b='2' c={3} d={"4"} e />;
const l = <div key="k" {...p} />;
const m = <div {...p} key="k" />;
const n = cond ? <A /> : <B />, o = cond && <A />;
const q = <input value="a&quot;b" title='it&apos;s' />;
const r = <div>{a}{" "}{b}</div>;
const s = <div>
  {a}
  {b}
</div>;
