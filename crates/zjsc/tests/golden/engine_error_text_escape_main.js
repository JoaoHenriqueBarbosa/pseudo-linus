// O fonte de um arquivo passa pelo transpilador do bun, que o reimprime (dobra `const`, escapa o que não é ASCII).
// Só o que passa por `eval` e `new Function` chega ao JSC como foi escrito, então a sonda do motor vai por aí.
const t = (code) => {
  try {
    (0, eval)(code);
  } catch (e) {
    console.log(JSON.stringify(e.message));
  }
};
t("var 日 = 1; 日();");
t("var o = {}; o.日本();");
t("var o = {}; o['日本']();");
t("var 日 = undefined; for (var x of 日) {}");
t("var 日 = {}; new 日();");
t("var 日 = {}; 日.x.y;");
t("var f = 'é'; ({})[f]();");
t("var é = 1; é();");
t("var o = { é: 1 }; o.é();");
t("var o = {}; o.a日();");
t("var 日 = null; 日.x;");
t("var o = {}; o[1](); ");
