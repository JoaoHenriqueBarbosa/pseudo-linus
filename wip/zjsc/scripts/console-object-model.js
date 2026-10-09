// Modelo de referência do formatador de objetos do `console.log` do bun 1.4.2 (arrays e objetos simples).
// É a especificação executável de `src/runtime/console_format.rs`. A quebra de linha é a do fonte do bun
// (`src/jsc/ConsoleObject.rs`: `Formatter::print_array`, `print_object`, `PropertyIteratorCtx`, `WrappedWriter`), sem
// constantes ajustadas: o gerador `scripts/gen-console-object-golden.js` exige zero divergência.
//
// Regras medidas:
// - objeto vazio `{}`; com propriedades, uma por linha (`chave: valor,`), sempre em várias linhas; chave só sem aspas se for
//   `[A-Za-z_$][A-Za-z0-9_$]*` (`"3"`, `"a-b"`, `"é"` levam aspas); índices primeiro, depois as demais na ordem de criação,
//   depois símbolos (`[Symbol(s)]: 1`); protótipo nulo abre com `[Object: null prototype] `;
// - objeto a mais de duas camadas de profundidade vira `[Object ...]` (array nunca é cortado); referência circular vira
//   `[Circular]` (sem `<ref *1>`);
// - array vazio `[]`; buraco vira `empty item` ou `N x empty items`; propriedade não indexada entra no fim (`k: 1`); no
//   máximo 100 elementos, depois `... N more items`;
// - array abre em linha (`[ a, b ]`) e passa a várias linhas se tiver mais de 10 elementos, se o primeiro elemento for um
//   objeto/array, ou se a linha estimada já passa de 80 colunas; a quebra dentro da lista acontece depois da vírgula quando a
//   estimativa passa de 80, e então o `]` também vai para a própria linha;
// - a estimativa de largura da linha (`est`) soma o texto de cada valor (string sem as aspas, bigint sem o `n`), mais
//   constantes de pontuação, e vale para a chamada inteira de `console.log` (entre argumentos soma 1).
const EMPTY_ARRAY = 2;
const MAX_ITEMS = 100;
const MAX_DEPTH = 2;

function escapeString(text) {
  let out = "";
  for (const ch of text) {
    const code = ch.codePointAt(0);
    if (ch === "\n") out += "\\n";
    else if (ch === "\t") out += "\\t";
    else if (ch === "\r") out += "\\r";
    else if (ch === "\b") out += "\\b";
    else if (ch === "\f") out += "\\f";
    else if (ch === '"') out += '\\"';
    else if (ch === "\\") out += "\\\\";
    else if (code < 0x20) out += "\\u" + code.toString(16).toUpperCase().padStart(4, "0");
    else out += ch;
  }
  return out;
}

function keyText(key) {
  if (typeof key === "symbol") return "[" + key.toString() + "]";
  if (/^[A-Za-z_$][A-Za-z0-9_$]*$/.test(key)) return key;
  return '"' + escapeString(key) + '"';
}

function isIndex(key) {
  return /^(0|[1-9][0-9]*)$/.test(key) && Number(key) < 4294967295;
}

function ownKeys(value, isArray) {
  const keys = Reflect.ownKeys(value).filter((key) => Object.getOwnPropertyDescriptor(value, key).enumerable);
  const indices = keys.filter((key) => typeof key === "string" && isIndex(key));
  const rest = keys.filter((key) => !(typeof key === "string" && isIndex(key)));
  return isArray ? rest : indices.concat(rest);
}

function inspectArgs(args) {
  const state = { indent: 0, est: 0, depth: 0, seen: [] };
  const pad = () => "  ".repeat(state.indent);
  const reset = () => { state.est = state.indent * 2; };
  const add = (count) => { state.est += count; };

  function primitive(value, quoted) {
    if (typeof value === "string") {
      add(value.length);
      return quoted ? '"' + escapeString(value) + '"' : value;
    }
    let text;
    if (typeof value === "number") text = Object.is(value, -0) ? "-0" : String(value);
    else if (typeof value === "bigint") {
      text = value + "n";
      add(text.length - 1);
      return text;
    } else if (typeof value === "symbol") text = value.toString();
    else text = String(value);
    add(text.length);
    return text;
  }

  // `good_time_for_a_new_line` (ConsoleObject.rs, `impl Formatter`): se a linha estimada passa de 80, zera para
  // `indent * 2` e devolve true.
  function goodTime() {
    if (state.est > 80) {
      reset();
      return true;
    }
    return false;
  }

  // `write_property_key` (ConsoleObject.rs, `PropertyIteratorCtx`): identificador soma `key.len + 1`; chave com aspas soma
  // `key.len + 2`; símbolo soma `1 + "[Symbol()]:".len() + key.len`.
  function addKey(key) {
    if (typeof key === "symbol") add(1 + "[Symbol()]:".length + (key.description ?? "").length);
    else if (/^[A-Za-z_$][A-Za-z0-9_$]*$/.test(key)) add(key.length + 1);
    else add(key.length + 2);
  }

  // Funções e classes (`print_function`, `print_class`): sem propriedades próprias. O nome é o do executável
  // (`calculated_display_name`), não a propriedade `name`: um `static name = 'z'` ou um `defineProperty(f, 'name', ...)` não o
  // muda. Em JS o nome do executável sai do texto do fonte (`function f`, `class A`); sem nome declarado, só a função
  // nativa e a que não começa por `function` (seta, método, ligada) herdam o `name` visível, porque o nome vem da inferência.
  // O tipo da função vem do `@@toStringTag` do protótipo (`AsyncFunction`, `GeneratorFunction`, `AsyncGeneratorFunction`,
  // vazio para `Function.prototype`).
  const KINDS = ["AsyncFunction", "GeneratorFunction", "AsyncGeneratorFunction"];
  function callableName(fn) {
    if (typeof fn !== "function") return "";
    const source = Function.prototype.toString.call(fn);
    const declared = /^(?:async\s+)?function\b\s*\*?\s*([A-Za-z_$][\w$]*)\s*\(/.exec(source) || /^class\s+(?!extends\b)([A-Za-z_$][\w$]*)/.exec(source);
    if (declared) return declared[1];
    if (!source.includes("[native code]") && /^(?:async\s+)?function\b|^class\b/.test(source)) return "";
    const name = Object.getOwnPropertyDescriptor(fn, "name");
    return name && typeof name.value === "string" ? name.value.replace(/^(bound )+/, "") : "";
  }
  function isClass(fn) {
    if (typeof fn !== "function") return false;
    const source = Function.prototype.toString.call(fn);
    if (/^class\b/.test(source)) return true;
    return source.includes("[native code]") && "prototype" in fn && fn.prototype !== undefined && !/^bound /.test(Object.getOwnPropertyDescriptor(fn, "name")?.value ?? "");
  }
  function formatCallable(fn) {
    const name = callableName(fn);
    const proto = Object.getPrototypeOf(fn);
    if (isClass(fn)) {
      const parent = typeof proto === "function" && isClass(proto) ? callableName(proto) : "";
      add(name.length + parent.length);
      return "[class " + (name === "" ? "(anonymous)" : name) + (parent === "" ? "" : " extends " + parent) + "]";
    }
    const tag = proto !== null && typeof proto === "object" || typeof proto === "function" ? proto[Symbol.toStringTag] : undefined;
    const kind = KINDS.includes(tag) ? tag : "";
    if (name === "" || kind === name) return "[" + (kind === "" ? "Function" : kind) + "]";
    return "[" + (kind === "" ? "Function" : kind) + ": " + name + "]";
  }

  // Marca interna de `Map`, `Set`, `WeakMap`, `WeakSet` (o `@@toStringTag` próprio não engana).
  function brand(value) {
    const probes = [["Map", Map.prototype.has, 0], ["Set", Set.prototype.has, 0], ["WeakMap", WeakMap.prototype.has, {}], ["WeakSet", WeakSet.prototype.has, {}]];
    for (const [name, has, probe] of probes) {
      try { has.call(value, probe); return name; } catch (_) {}
    }
    return null;
  }
  function boxedKind(value) {
    for (const [name, valueOf] of [["Number", Number.prototype.valueOf], ["Boolean", Boolean.prototype.valueOf], ["String", String.prototype.valueOf]]) {
      try { valueOf.call(value); return name; } catch (_) {}
    }
    return null;
  }
  function isPrimitiveTag(value) {
    if (typeof value === "function") return false;
    if (typeof value !== "object" || value === null) return true;
    return boxedKind(value) !== null;
  }

  // `print_double`, `print_boolean`, `print_string`: a caixa de string na raiz leva o rótulo, aninhada vira a string entre aspas.
  // O texto é o `ToString` do objeto (`to_js_string_view`), que lança `No default value` se o protótipo for nulo.
  function formatBoxed(value, kind) {
    const text = String(value);
    if (kind === "String") {
      add(text.length);
      return state.depth === 0 ? '[String: "' + escapeString(text) + '"]' : '"' + escapeString(text) + '"';
    }
    const proto = Object.getPrototypeOf(value);
    // `calculatedClassName`: o `constructor` próprio primeiro (`Number.prototype`), depois o do protótipo.
    const holder = Object.prototype.hasOwnProperty.call(value, "constructor") ? value : proto;
    const own = holder !== null && holder.constructor && typeof holder.constructor.name === "string" ? holder.constructor.name : kind;
    const plain = own === kind;
    if (kind === "Number") add(own.length + text.length + (plain ? 4 : "[Number ():]".length));
    else add(plain ? text.length + "[Boolean: ]".length : text.length + own.length + "[Boolean (): ]".length);
    return "[" + kind + (plain ? "" : " (" + own + ")") + ": " + text + "]";
  }

  // `print_json` para `JSDate`: o `JSON.stringify` do objeto (chama `toJSON`, portanto o `toISOString` do usuário, e pode
  // lançar) sem as aspas; `null` é `Invalid Date`; texto de até duas unidades (o `{}` do protótipo nulo) sai como veio. A
  // estimativa soma o texto com aspas.
  function isDate(value) {
    try { Date.prototype.getTime.call(value); return true; } catch (_) { return false; }
  }
  function formatDate(value) {
    const json = JSON.stringify(value);
    if (json === undefined) return "";
    add(json.length);
    if (json === "null") return "Invalid Date";
    return json.length > 2 ? json.slice(1, -1) : json;
  }

  // `Tag::String` para `RegExpObject` (`print_string` sem aspas): o `ToString` do objeto, que chama o `toString` do usuário.
  function isRegExp(value) {
    if (value === RegExp.prototype) return false;
    try { Object.getOwnPropertyDescriptor(RegExp.prototype, "global").get.call(value); return true; } catch (_) { return false; }
  }
  function formatRegExp(value) {
    const text = String(value);
    add(text.length);
    return text;
  }

  // `print_promise`: `Promise { <pending> }`, `<resolved>` ou `<rejected>`; a escrita direta não soma à estimativa, só a
  // consulta de quebra que vem antes.
  function promiseStatus(value) {
    if (!require("util").types.isPromise(value)) return null;
    const shown = require("util").inspect(value, { depth: -1 });
    if (shown.startsWith("Promise { <pending>")) return "pending";
    return shown.startsWith("Promise { <rejected>") ? "rejected" : "resolved";
  }
  function formatPromise(status) {
    const lead = goodTime() ? "\n" + pad() : "";
    return lead + "Promise { <" + status + "> }";
  }

  // `get_object_name`: o nome de classe do objeto (o `calculatedClassName` do JSC: `constructor` próprio, `constructor` do
  // protótipo, `@@toStringTag` string, só dados, sem rodar getter), `[Object: null prototype]` para o protótipo nulo e nada
  // para o objeto comum. O nome vazio cai em `Object`.
  function dataProperty(object, key) {
    for (let holder = object; holder !== null; holder = Object.getPrototypeOf(holder)) {
      const descriptor = Object.getOwnPropertyDescriptor(holder, key);
      if (descriptor) return "value" in descriptor ? descriptor : null;
    }
    return null;
  }
  function constructorName(descriptor) {
    return descriptor !== null && typeof descriptor.value === "function" ? callableName(descriptor.value) : null;
  }
  function objectName(value) {
    let name = null;
    const own = Object.getOwnPropertyDescriptor(value, "constructor");
    if (own && "value" in own) name = constructorName(own);
    const proto = Object.getPrototypeOf(value);
    if (name === null && proto !== null) name = constructorName(dataProperty(proto, "constructor"));
    if (name === null || name === "Object") {
      const tag = dataProperty(value, Symbol.toStringTag);
      name = tag !== null && typeof tag.value === "string" ? tag.value : "Object";
    }
    if (name !== "" && name !== "Object") return name;
    return proto === null ? "[Object: null prototype]" : null;
  }

  // `Symbol` e `BigInt` encaixotados, `WeakRef` e `FinalizationRegistry`: o `forEachPropertyImpl` do bun desce ao protótipo e
  // lista todas as chaves dele (enumeráveis ou não), até cinco níveis e sem chegar ao `Object.prototype`, menos `constructor`,
  // `__proto__` e `@@toStringTag`.
  function printsPrototypeMembers(value) {
    const probes = [
      () => Symbol.prototype.valueOf.call(value),
      () => BigInt.prototype.valueOf.call(value),
      () => WeakRef.prototype.deref.call(value),
      () => FinalizationRegistry.prototype.unregister.call(value, {}),
    ];
    return probes.some((probe) => { try { probe(); return true; } catch (_) { return false; } });
  }
  function prototypeMembers(value, known) {
    const keys = [];
    let proto = Object.getPrototypeOf(value);
    for (let level = 0; level < 5 && proto !== null && Object.getPrototypeOf(proto) !== null; level++) {
      for (const key of Reflect.ownKeys(proto)) {
        if (key === "constructor" || key === "__proto__" || key === Symbol.toStringTag || known.includes(key) || keys.includes(key)) continue;
        keys.push(key);
      }
      proto = Object.getPrototypeOf(proto);
    }
    return keys;
  }

  // `print_map_like`, `print_set`: `Nome(n) {`, uma entrada por linha, indentação e vírgula; o tamanho vem de `size`.
  function formatCollection(value, name) {
    const size = value.size;
    const length = typeof size === "number" ? size | 0 : 0;
    if (length === 0) return name + " {}";
    const isSet = name === "Set" || name === "WeakSet";
    const entries = [];
    if (name === "Map") for (const entry of Map.prototype.entries.call(value)) entries.push(entry);
    else if (name === "Set") for (const entry of Set.prototype.values.call(value)) entries.push([entry]);
    let text = name + "(" + length + ") {\n";
    state.indent++;
    state.depth++;
    state.seen.push(value);
    for (const entry of entries) {
      text += pad() + format(entry[0]);
      if (!isSet) text += ": " + format(entry[1]);
      text += ",\n";
      add(1);
    }
    state.seen.pop();
    state.depth--;
    state.indent--;
    return text + pad() + "}";
  }

  // `print_map_iterator_like`: `Nome { `, por item `\n`, indentação, valor e vírgula. O bun lê o iterador sem consumir; em JS
  // não há como espiar, então o modelo consome o iterador que recebe (o gerador imprime cada iterador uma vez).
  function formatIterator(value, name) {
    state.indent++;
    state.depth++;
    let text = name + " { ";
    let count = 0;
    for (;;) {
      const step = Object.getPrototypeOf(value).next.call(value);
      if (step.done) break;
      count++;
      text += "\n" + pad();
      if (Array.isArray(step.value)) {
        state.depth++;
        text += formatArray(step.value);
        state.depth--;
      } else text += format(step.value);
      text += ",";
      add(1);
    }
    state.depth--;
    state.indent--;
    return text + (count > 0 ? "\n" : "") + pad() + "}";
  }

  function format(value) {
    if (typeof value === "function") return formatCallable(value);
    if (typeof value !== "object" || value === null) return primitive(value, true);
    if (state.seen.includes(value)) return "[Circular]";
    const boxed = boxedKind(value);
    if (boxed !== null) return formatBoxed(value, boxed);
    const kind = brand(value);
    if (kind !== null) return formatCollection(value, kind);
    if (isDate(value)) return formatDate(value);
    if (isRegExp(value)) return formatRegExp(value);
    const status = promiseStatus(value);
    if (status !== null) return formatPromise(status);
    const iteratorTag = Object.prototype.toString.call(value);
    if (iteratorTag === "[object Map Iterator]") return formatIterator(value, "MapIterator");
    if (iteratorTag === "[object Set Iterator]") return formatIterator(value, "SetIterator");
    const isArray = Array.isArray(value);
    if (!isArray) {
      // `print_object`: `always_newline = good_time_for_a_new_line()` roda antes do teste de profundidade (e zera a linha).
      goodTime();
      // `print_object_depth_exceeded`: a segunda chamada de `good_time_for_a_new_line` já não vê a linha acima de 80.
      if (state.depth > MAX_DEPTH) {
        if (goodTime()) return "\n" + pad() + "[Object ...]";
        return "[Object ...]";
      }
    }
    state.seen.push(value);
    state.depth++;
    const text = isArray ? formatArray(value) : formatObject(value);
    state.depth--;
    state.seen.pop();
    return text;
  }

  // `print_object` + `PropertyIteratorCtx::handle_first_property` + `print_object_tail`: a primeira propriedade fixa a linha em
  // `indent * 2 + 1` (indent ainda sem o incremento), cada propriedade seguinte vem depois de vírgula (+1), quebra e
  // `reset_line`; o `}` soma 1 sem zerar.
  function formatObject(value) {
    let keys = ownKeys(value, false);
    // O caminho rápido do bun descarta `@@toStringTag`; ele só aparece se for a única chave do objeto.
    if (keys.some((key) => key !== Symbol.toStringTag)) keys = keys.filter((key) => key !== Symbol.toStringTag);
    if (printsPrototypeMembers(value)) keys = keys.concat(prototypeMembers(value, keys));
    const name = objectName(value);
    const prefix = name === null ? "" : name + " ";
    if (keys.length === 0) return prefix + "{}";
    let text = prefix + "{\n";
    state.est = state.indent * 2 + 1;
    state.indent++;
    keys.forEach((key, position) => {
      if (position > 0) {
        text += ",\n";
        add(1);
        reset();
      }
      text += pad();
      addKey(key);
      text += keyText(key) + ": " + format(value[key]);
    });
    state.indent--;
    add(1); // `print_comma` do `print_object_tail` soma 1
    text += ",\n" + pad() + "}";
    add(1);
    return text;
  }

  // `print_array` (ConsoleObject.rs), porta linha a linha.
  function formatArray(value) {
    const length = value.length;
    if (length === 0) {
      add(EMPTY_ARRAY);
      return "[]";
    }
    let good = length > 10;
    state.indent++;
    add(2);
    const first = 0 in value ? value[0] : undefined;
    good = good || (0 in value && !isPrimitiveTag(first)) || goodTime();
    let text = "[";
    if (good) {
      reset();
      text += "\n" + pad();
      add(1);
    } else {
      text += " ";
      add(2);
    }
    const separator = () => {
      if (goodTime()) {
        good = true;
        text += "\n" + pad();
      } else {
        add(1);
        text += " ";
      }
    };
    const comma = () => {
      text += ",";
      add(1);
    };
    const hole = (count) => {
      if (count === 1) {
        add("empty item".length);
        text += "empty item";
      } else {
        add(String(count).length);
        add(" x empty items".length);
        text += count + " x empty items";
      }
    };
    let emptyStart = null;
    if (0 in value) text += format(value[0]);
    else emptyStart = 0;
    let index = 1;
    let nonEmpty = 1;
    while (index < length) {
      if (!(index in value)) {
        if (emptyStart === null) emptyStart = index;
        index++;
        continue;
      }
      if (nonEmpty >= MAX_ITEMS) {
        comma();
        text += "\n";
        state.est = 0;
        text += pad();
        add("... N more items".length);
        text += "... " + (length - index) + " more items";
        break;
      }
      nonEmpty++;
      if (emptyStart !== null) {
        if (emptyStart > 0) {
          comma();
          separator();
        }
        hole(index - emptyStart);
        emptyStart = null;
      }
      comma();
      separator();
      text += format(value[index]);
      index++;
    }
    if (emptyStart !== null) {
      if (emptyStart > 0) {
        comma();
        separator();
      }
      hole(length - emptyStart);
    }
    // Propriedades não indexadas: `always_newline` sai de `good_time_for_a_new_line` (e zera a linha se passou de 80).
    const always = goodTime();
    for (const key of ownKeys(value, true)) {
      comma();
      if (always || goodTime()) {
        text += "\n" + pad();
        reset();
      } else {
        add(1);
        text += " ";
      }
      addKey(key);
      text += keyText(key) + ": " + format(value[key]);
    }
    state.indent--;
    if (good || goodTime()) {
      reset();
      text += "\n" + pad() + "]";
      reset();
      add(1);
    } else {
      text += " ]";
      add(2);
    }
    return text;
  }

  // `write_with_formatting` (`%s`, `%o`, `%O`, `%j`): o primeiro argumento string com argumentos depois dele é o formato.
  // `%s` é o `ToString` (a caixa de string leva o rótulo), `%o` e `%O` passam pelo formatador (primitivo sem aspas), `%j` é o
  // `JSON.stringify` (`undefined` some). O texto literal soma à estimativa; o objeto, ele mesmo.
  function applyFormat(text, rest) {
    let out = "";
    let used = 0;
    let literal = 0;
    for (let at = 0; at < text.length; at++) {
      const spec = text[at + 1];
      if (text[at] === "%" && used < rest.length && spec !== undefined && "sOoj".includes(spec)) {
        const arg = rest[used++];
        at++;
        if (spec === "s") {
          if (typeof arg === "object" && arg !== null && boxedKind(arg) === "String") out += format(arg);
          else { const shown = String(arg); add(shown.length); out += shown; }
        } else if (spec === "j") {
          const json = JSON.stringify(arg);
          if (json !== undefined) { add(json.length); out += json; }
        } else out += typeof arg === "string" ? primitive(arg, false) : format(arg);
      } else {
        out += text[at];
        literal++;
      }
    }
    add(literal);
    return { out, used };
  }

  let out = "";
  for (let position = 0; position < args.length; position++) {
    const arg = args[position];
    if (position > 0) {
      out += " ";
      add(1);
    }
    if (typeof arg === "string" && position + 1 < args.length) {
      const applied = applyFormat(arg, args.slice(position + 1));
      out += applied.out;
      position += applied.used;
    } else out += typeof arg === "string" ? primitive(arg, false) : format(arg, false);
  }
  return out;
}

module.exports = { inspectArgs };
