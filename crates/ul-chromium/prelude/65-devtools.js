// ---- DevTools: o agente da página do protocolo (Runtime e o que depende dos objetos da página), como o
// `V8InspectorSession` do Chromium. O hospedeiro entrega cada mensagem já validada pelo esquema do protocolo e recolhe
// as respostas e os eventos na caixa de saída; resposta de comando assíncrono (`awaitPromise`) sai quando a promessa
// assenta. Os intrínsecos ficam guardados aqui: o que a página trocar depois não muda o que o inspetor vê.

const devtoolsAgent = (() => {
  const jsonStringify = JSON.stringify;
  const jsonParse = JSON.parse;
  const ownKeys = Reflect.ownKeys;
  const getOwnDescriptor = Object.getOwnPropertyDescriptor;
  // O `stack` próprio do erro, que no V8 é um acessor próprio (`get` e `set`, medido no oráculo) que o inspetor lê
  // como texto; `undefined` quando não há.
  const ownStack = (error) => {
    try {
      const item = getOwnDescriptor(error, 'stack');
      const text = item && ('value' in item ? item.value : item.get && call(item.get, error));
      return typeof text === 'string' ? text : undefined;
    } catch { return undefined; }
  };
  const getPrototypeOf = Object.getPrototypeOf;
  const objectToString = Object.prototype.toString;
  const functionToString = Function.prototype.toString;
  const call = Function.prototype.call.bind(Function.prototype.call);
  const isArray = Array.isArray;
  const promiseThen = Promise.prototype.then;
  const mapSize = getOwnDescriptor(Map.prototype, 'size').get;
  const setSize = getOwnDescriptor(Set.prototype, 'size').get;
  const symbolDescription = getOwnDescriptor(Symbol.prototype, 'description').get;
  const symbolToString = Symbol.prototype.toString;
  const bigintToString = BigInt.prototype.toString;
  const dateToString = Date.prototype.toString;
  const dateGetTime = Date.prototype.getTime;
  const regexpToString = RegExp.prototype.toString;
  const typedArrayLength = getOwnDescriptor(getPrototypeOf(Uint8Array.prototype), 'length').get;
  const typedArrayTag = getOwnDescriptor(getPrototypeOf(Uint8Array.prototype), Symbol.toStringTag).get;
  const arrayBufferLength = getOwnDescriptor(ArrayBuffer.prototype, 'byteLength').get;
  const promiseState = new WeakMap();
  const ObjectCtor = Object;
  const dateNow = Date.now;

  const state = {
    // O id do isolate (um `int64` aleatório do V8) e o contexto corrente; cada navegação cria um contexto novo.
    isolateId: '0',
    contextId: 1,
    worlds: new Set(),
    // O contexto em que a mensagem corrente roda: o `objectId` leva o id dele (`isolado.contexto.n`).
    active: 1,
    backend: new Map(),
    backendIds: new WeakMap(),
    nextBackend: 1,
    uniqueId: '',
    frameId: '',
    origin: '://',
    nextObject: 1,
    exceptionId: 0,
    // Os scripts que o V8 já compilou antes do primeiro `evaluate` do inspetor.
    scriptId: 2,
    scripts: new Map(),
    // As mensagens guardadas para repetir no `Runtime.enable` (o `V8ConsoleMessageStorage`, até 1000).
    stored: [],
    // Sem eco: o relato de erro do próprio navegador usa `console.error`, que não é chamada da página.
    quiet: 0,
    objects: new Map(),
    groups: new Map(),
    enabled: new Set(),
    outbox: [],
  };

  const brand = (getter, value) => {
    try { call(getter, value); return true; } catch { return false; }
  };

  // O `GetConstructorName` do V8: o nome do construtor da cadeia de protótipos, ou o da marca interna.
  const className = (value) => {
    try {
      for (let proto = value; proto !== null; proto = getPrototypeOf(proto)) {
        const ctor = getOwnDescriptor(proto, 'constructor');
        if (ctor && typeof ctor.value === 'function') {
          const name = getOwnDescriptor(ctor.value, 'name');
          if (name && typeof name.value === 'string' && name.value !== '') return name.value;
        }
      }
    } catch { /* Proxy revogado ou armadilha que lança */ }
    return call(objectToString, value).slice(8, -1);
  };

  const nodeDescription = (node) => {
    switch (node.nodeType) {
      case 1: {
        let text = node.localName;
        const id = node.getAttribute('id');
        if (id) text += '#' + id;
        const cls = node.getAttribute('class');
        if (cls) for (const name of cls.trim().split(/\s+/)) if (name) text += '.' + name;
        return text;
      }
      case 3: return '#text';
      case 4: return '#cdata-section';
      case 7: return node.nodeName;
      case 8: return '#comment';
      case 9: return '#document';
      case 10: return '<!DOCTYPE ' + node.name + '>';
      case 11: return '#document-fragment';
      default: return node.nodeName;
    }
  };

  // Subtipo e descrição do `ValueMirror` do V8 para um objeto.
  const objectShape = (value) => {
    const name = className(value);
    if (isNode(value)) return { subtype: 'node', className: name, description: nodeDescription(value) };
    if (isArray(value)) return { subtype: 'array', className: name, description: name + '(' + value.length + ')' };
    if (brand(mapSize, value)) return { subtype: 'map', className: name, description: name + '(' + call(mapSize, value) + ')' };
    if (brand(setSize, value)) return { subtype: 'set', className: name, description: name + '(' + call(setSize, value) + ')' };
    if (value instanceof WeakMap) return { subtype: 'weakmap', className: name, description: name };
    if (value instanceof WeakSet) return { subtype: 'weakset', className: name, description: name };
    if (brand(dateGetTime, value)) return { subtype: 'date', className: name, description: call(dateToString, value) };
    if (value instanceof RegExp) return { subtype: 'regexp', className: name, description: call(regexpToString, value) };
    if (value instanceof Error) {
      const stack = ownStack(value);
      const description = stack !== undefined ? stack : String(value.name) + ': ' + String(value.message);
      return { subtype: 'error', className: name, description };
    }
    if (value instanceof Promise) return { subtype: 'promise', className: name, description: 'Promise' };
    if (brand(typedArrayLength, value)) return { subtype: 'typedarray', className: name, description: call(typedArrayTag, value) + '(' + call(typedArrayLength, value) + ')' };
    if (brand(arrayBufferLength, value)) return { subtype: 'arraybuffer', className: name, description: name + '(' + call(arrayBufferLength, value) + ')' };
    if (value instanceof DataView) return { subtype: 'dataview', className: name, description: name + '(' + value.byteLength + ')' };
    return { className: name, description: name };
  };

  const register = (value, group) => {
    const objectId = state.isolateId + '.' + state.active + '.' + state.nextObject++;
    state.objects.set(objectId, value);
    if (group !== undefined) {
      if (!state.groups.has(group)) state.groups.set(group, []);
      state.groups.get(group).push(objectId);
    }
    return objectId;
  };

  const unserializable = (n) => {
    if (n !== n) return 'NaN';
    if (n === Infinity) return 'Infinity';
    if (n === -Infinity) return '-Infinity';
    if (n === 0 && 1 / n < 0) return '-0';
    return null;
  };

  // O valor por cópia (`returnByValue`): JSON com a profundidade e os ciclos que o V8 recusa.
  class ChainTooLong extends Error {}
  const byValue = (value, depth) => {
    if (depth > 1000) throw new ChainTooLong();
    if (value === null || typeof value !== 'object') {
      if (typeof value === 'number') return unserializable(value) === null ? value : null;
      return typeof value === 'function' || typeof value === 'symbol' ? undefined : value;
    }
    if (typeof value.toJSON === 'function' && brand(dateGetTime, value)) return value.toJSON();
    if (isArray(value)) {
      const out = [];
      for (let i = 0; i < value.length; i++) {
        const item = byValue(value[i], depth + 1);
        out.push(item === undefined ? null : item);
      }
      return out;
    }
    const out = {};
    for (const key of ObjectCtor.keys(value)) {
      let read;
      try { read = value[key]; } catch (error) { if (error instanceof RangeError) throw error; continue; }
      const item = byValue(read, depth + 1);
      if (item !== undefined) out[key] = item;
    }
    return out;
  };

  const remoteObject = (value, options = {}) => {
    switch (typeof value) {
      case 'undefined': return { type: 'undefined' };
      case 'boolean': return { type: 'boolean', value };
      case 'string': return { type: 'string', value };
      case 'number': {
        const special = unserializable(value);
        return special === null ? { type: 'number', value, description: String(value) } : { type: 'number', unserializableValue: special, description: special };
      }
      case 'bigint': {
        const text = call(bigintToString, value) + 'n';
        return { type: 'bigint', unserializableValue: text, description: text };
      }
      case 'symbol': {
        const out = { type: 'symbol', description: call(symbolToString, value) };
        if (!options.returnByValue) out.objectId = register(value, options.objectGroup);
        return out;
      }
      default: break;
    }
    if (value === null) return { type: 'object', subtype: 'null', value: null };
    if (options.returnByValue) {
      const copy = byValue(value, 0);
      return { type: typeof value === 'function' ? 'function' : 'object', value: copy === undefined ? {} : copy };
    }
    if (typeof value === 'function') {
      return { type: 'function', className: 'Function', description: call(functionToString, value), objectId: register(value, options.objectGroup) };
    }
    const shape = objectShape(value);
    const out = { type: 'object' };
    if (shape.subtype) out.subtype = shape.subtype;
    out.className = shape.className;
    out.description = shape.description;
    out.objectId = register(value, options.objectGroup);
    return out;
  };

  // A posição de uma exceção: linha e coluna do `stack` do V8 (base 1) passadas a base 0.
  const exceptionDetails = (exception, text, scriptId, mirror, extra = {}) => {
    state.exceptionId++;
    let lineNumber = 0;
    let columnNumber = 0;
    if (extra.position) [lineNumber, columnNumber] = extra.position;
    const details = { exceptionId: state.exceptionId, text, lineNumber, columnNumber };
    if (scriptId !== null) details.scriptId = String(scriptId);
    details.exception = mirror;
    return details;
  };

  const reply = (message, result) => {
    const out = { id: message.id, result };
    if (message.sessionId !== undefined) out.sessionId = message.sessionId;
    state.outbox.push(out);
  };
  const fail = (message, code, text) => {
    const out = { id: message.id, error: { code, message: text } };
    if (message.sessionId !== undefined) out.sessionId = message.sessionId;
    state.outbox.push(out);
  };
  const emit = (method, params) => {
    for (const sessionId of state.enabled) {
      const out = { method, params };
      if (sessionId !== '') out.sessionId = sessionId;
      state.outbox.push(out);
    }
  };

  // `Runtime.addBinding`: a função no objeto global que manda o texto ao cliente. O hospedeiro guarda os nomes e os manda
  // de novo a cada documento; o `removeBinding` só vale para os documentos seguintes (medido no oráculo).
  const bindings = new Set();
  const installBinding = (name) => {
    if (bindings.has(name)) return;
    bindings.add(name);
    Object.defineProperty(globalThis, name, {
      value: function (payload) { emit('Runtime.bindingCalled', { name, payload: String(payload), executionContextId: state.contextId }); },
      writable: true, configurable: true, enumerable: true,
    });
  };

  const contextDescription = () => ({
    id: state.contextId,
    origin: state.origin,
    name: '',
    uniqueId: state.uniqueId,
    auxData: { isDefault: true, type: 'default', frameId: state.frameId },
  });

  // O resultado de um valor ou exceção já calculado, com a espera da promessa quando pedida.
  const settle = (message, params, outcome) => {
    const options = { returnByValue: params.returnByValue === true, objectGroup: params.objectGroup };
    const send = (thrown, value, scriptId, position, inPromise) => {
      let mirror;
      try {
        mirror = remoteObject(value, thrown ? { objectGroup: params.objectGroup } : options);
      } catch (error) {
        // A profundidade de 1000 do V8 ou, antes dela, a pilha do motor: os dois são a mesma recusa para o cliente.
        if (error instanceof ChainTooLong || error instanceof RangeError) { fail(message, -32000, 'Object reference chain is too long'); return; }
        throw error;
      }
      if (!thrown) { reply(message, { result: mirror }); return; }
      const text = inPromise ? 'Uncaught (in promise) ' + (mirror.description !== undefined && mirror.type === 'object' ? mirror.description.split('\n')[0] : remoteText(value)) : 'Uncaught';
      const exception = remoteObject(value, { objectGroup: params.objectGroup });
      reply(message, { result: mirror, exceptionDetails: exceptionDetails(value, text, inPromise ? null : scriptId, exception, { position }) });
    };
    if (outcome.thrown || params.awaitPromise !== true || !(outcome.value instanceof Promise)) {
      send(outcome.thrown, outcome.value, outcome.scriptId, outcome.position, false);
      return;
    }
    call(promiseThen, outcome.value, (value) => send(false, value, outcome.scriptId), (reason) => send(true, reason, outcome.scriptId, undefined, true));
  };

  const remoteText = (value) => {
    try { return String(value); } catch { return 'Object'; }
  };

  // Linha e coluna (base 0) da primeira moldura do `stack`, a posição que o V8 dá à exceção.
  const stackPosition = (error) => {
    try {
      const stack = ownStack(error);
      if (stack === undefined) return undefined;
      const found = /:(\d+):(\d+)\)?(?:\n|$)/.exec(stack);
      return found ? [Number(found[1]) - 1, Number(found[2]) - 1] : undefined;
    } catch { return undefined; }
  };

  // O id do script compilado para `source`: o cache de compilação do V8 devolve o mesmo script para o mesmo fonte.
  const scriptFor = (source) => {
    let id = state.scripts.get(source);
    if (id === undefined) state.scripts.set(source, (id = ++state.scriptId));
    return id;
  };

  // O contexto pedido (`contextId` ou `uniqueContextId`) existe nesta página; ausente vale o contexto padrão.
  const contextMatches = (id, unique) => (id === undefined || id === state.contextId || state.worlds.has(id)) && (unique === undefined || unique === state.uniqueId);

  const evaluate = (message, params) => {
    if (!contextMatches(params.contextId, params.uniqueContextId)) { fail(message, -32000, 'Cannot find context with specified id'); return; }
    const scriptId = scriptFor(params.expression);
    let outcome;
    try {
      outcome = { thrown: false, value: evalGlobal(params.expression, ''), scriptId };
    } catch (error) {
      outcome = { thrown: true, value: error, scriptId, position: error instanceof SyntaxError ? stackPosition(error) : [0, 0] };
    }
    settle(message, params, outcome);
  };

  const lookup = (message, objectId) => {
    if (!state.objects.has(objectId)) {
      fail(message, -32000, 'Could not find object with given id');
      return { found: false };
    }
    return { found: true, value: state.objects.get(objectId) };
  };

  const callFunctionOn = (message, params) => {
    let receiver;
    if (params.objectId !== undefined) {
      const found = lookup(message, params.objectId);
      if (!found.found) return;
      receiver = found.value;
    } else if (params.executionContextId === undefined && params.uniqueContextId === undefined) {
      fail(message, -32602, 'Either objectId or executionContextId or uniqueContextId must be specified');
      return;
    } else if (!contextMatches(params.executionContextId, params.uniqueContextId)) {
      fail(message, -32000, 'Cannot find context with specified id');
      return;
    }
    const args = [];
    for (const argument of params.arguments || []) {
      if (argument.objectId !== undefined) {
        if (!state.objects.has(argument.objectId)) { fail(message, -32000, 'Could not find object with given id'); return; }
        args.push(state.objects.get(argument.objectId));
      } else if (argument.unserializableValue !== undefined) {
        const text = argument.unserializableValue;
        args.push(text === 'NaN' ? NaN : text === 'Infinity' ? Infinity : text === '-Infinity' ? -Infinity : text === '-0' ? -0 : BigInt(text.slice(0, -1)));
      } else {
        args.push(argument.value);
      }
    }
    const scriptId = scriptFor('(' + params.functionDeclaration + '\n)');
    let outcome;
    try {
      const fn = evalGlobal('(' + params.functionDeclaration + '\n)', '');
      if (typeof fn !== 'function') { fail(message, -32000, 'Given expression does not evaluate to a function'); return; }
      outcome = { thrown: false, value: call(fn, receiver, ...args), scriptId };
    } catch (error) {
      outcome = { thrown: true, value: error, scriptId, position: [0, 0] };
    }
    settle(message, params, outcome);
  };

  const getProperties = (message, params) => {
    const found = lookup(message, params.objectId);
    if (!found.found) return;
    const target = found.value;
    const options = { objectGroup: params.objectGroup };
    const result = [];
    const describe = (owner, own) => {
      for (const key of ownKeys(owner)) {
        const descriptor = getOwnDescriptor(owner, key);
        if (params.ownProperties && !own) continue;
        const entry = { name: typeof key === 'symbol' ? call(symbolToString, key) : key };
        if ('value' in descriptor) {
          let value;
          try { value = remoteObject(descriptor.value, options); } catch { value = { type: 'undefined' }; }
          entry.value = value;
          entry.writable = descriptor.writable;
        } else {
          if (descriptor.get) entry.get = remoteObject(descriptor.get, options);
          if (descriptor.set) entry.set = remoteObject(descriptor.set, options);
        }
        entry.configurable = descriptor.configurable;
        entry.enumerable = descriptor.enumerable;
        entry.isOwn = own;
        if (typeof key === 'symbol') entry.symbol = remoteObject(key, options);
        result.push(entry);
      }
    };
    if (target !== null && (typeof target === 'object' || typeof target === 'function')) {
      describe(target, true);
      if (!params.ownProperties) for (let proto = getPrototypeOf(target); proto !== null; proto = getPrototypeOf(proto)) describe(proto, false);
    }
    const out = { result };
    if (params.ownProperties && target !== null && typeof target === 'object') {
      const proto = getPrototypeOf(target);
      if (proto !== null) out.internalProperties = [{ name: '[[Prototype]]', value: remoteObject(proto, options) }];
    }
    reply(message, out);
  };

  // A prévia do `ValueMirror`: até 5 propriedades num objeto e 100 índices numa lista, cada uma com o valor como
  // texto; o erro mostra `stack` e `message` (próprias, mesmo não enumeráveis).
  const preview = (value, mirror) => {
    const isList = mirror.subtype === 'array' || mirror.subtype === 'typedarray';
    const limit = isList ? 100 : 5;
    let keys;
    try {
      // No V8 o `stack` é a primeira propriedade própria do erro (instalada na construção), depois `message`.
      keys = mirror.subtype === 'error' ? ['stack', 'message'].filter((k) => getOwnDescriptor(value, k) !== undefined) : ObjectCtor.keys(value);
    } catch { keys = []; }
    const properties = [];
    for (const key of keys) {
      if (properties.length >= limit) break;
      let item;
      try { item = getOwnDescriptor(value, key); } catch { continue; }
      if (!item) continue;
      // O inspetor mostra o `stack` do erro pelo valor, embora seja acessor.
      if (mirror.subtype === 'error' && key === 'stack') item = { value: ownStack(value) };
      if (!('value' in item)) { properties.push({ name: key, type: 'accessor' }); continue; }
      const inner = item.value;
      const entry = { name: key };
      if (inner === null) { entry.type = 'object'; entry.value = 'null'; entry.subtype = 'null'; }
      else if (typeof inner === 'object' || typeof inner === 'function') {
        const shape = typeof inner === 'function' ? { description: '' } : objectShape(inner);
        entry.type = typeof inner === 'function' ? 'function' : 'object';
        entry.value = shape.description;
        if (shape.subtype) entry.subtype = shape.subtype;
      } else if (typeof inner === 'symbol') { entry.type = 'symbol'; entry.value = call(symbolToString, inner); }
      else if (typeof inner === 'bigint') { entry.type = 'bigint'; entry.value = call(bigintToString, inner) + 'n'; }
      else { entry.type = typeof inner; entry.value = String(inner); }
      properties.push(entry);
    }
    const out = { type: mirror.type };
    if (mirror.subtype) out.subtype = mirror.subtype;
    out.description = mirror.description;
    out.overflow = keys.length > limit;
    out.properties = properties;
    return out;
  };

  const withPreview = (value, options) => {
    const mirror = remoteObject(value, options);
    if (mirror.objectId !== undefined && mirror.type === 'object') mirror.preview = preview(value, mirror);
    return mirror;
  };

  // As molduras do `stack` no formato do V8 (`at f (url:L:C)` ou `at url:L:C`), sem as do próprio navegador.
  const callFrames = (stack, skip) => {
    const frames = [];
    for (const line of String(stack).split('\n').slice(1 + skip)) {
      const found = /^\s*at (?:(.*?) \()?(.*?):(\d+):(\d+)\)?$/.exec(line);
      if (!found) continue;
      const url = found[2] === '<anonymous>' ? '' : found[2];
      frames.push({ functionName: found[1] || '', scriptId: String(url === '' ? state.scriptId : 0), url, lineNumber: Number(found[3]) - 1, columnNumber: Number(found[4]) - 1 });
    }
    return frames;
  };

  const store = (method, params) => {
    state.stored.push([method, params]);
    if (state.stored.length > 1000) state.stored.shift();
    emit(method, params);
  };

  const consoleCall = (kind, args) => {
    if (state.quiet > 0) return;
    let type = kind;
    let values = args;
    // `count`, `timeEnd` e `timeLog` viram texto como o V8 os escreve.
    if (kind === 'count') values = [args[0] + ': ' + args[1]];
    else if (kind === 'timeEnd' || kind === 'timeLog') {
      values = [args[0] + ': ' + args[1] + ' ms', ...args.slice(2)];
      if (kind === 'timeLog') type = 'log';
    } else if (kind === 'assert') values = args.length === 0 ? ['console.assert'] : args;
    const params = {
      type,
      args: values.map((value) => withPreview(value, {})),
      executionContextId: state.contextId,
      timestamp: dateNow(),
    };
    const frames = callFrames(new Error().stack, 1);
    if (frames.length) params.stackTrace = { callFrames: frames };
    store('Runtime.consoleAPICalled', params);
  };

  // Exceção que ninguém tratou (o relato do navegador depois do evento `error` da janela).
  const uncaught = (error) => {
    state.exceptionId++;
    const frames = error instanceof Error ? callFrames(ownStack(error) || '', 0) : [];
    const top = frames[0];
    const details = { exceptionId: state.exceptionId, text: 'Uncaught', lineNumber: top ? top.lineNumber : 0, columnNumber: top ? top.columnNumber : 0 };
    if (top) details.scriptId = top.scriptId;
    if (top && top.url) details.url = top.url;
    if (frames.length) details.stackTrace = { callFrames: frames };
    details.exception = withPreview(error, {});
    details.executionContextId = state.contextId;
    store('Runtime.exceptionThrown', { timestamp: dateNow(), exceptionDetails: details });
  };

  const commands = {
    'Runtime.enable': (message) => {
      const sessionId = message.sessionId === undefined ? '' : message.sessionId;
      if (!state.enabled.has(sessionId)) {
        state.enabled.add(sessionId);
        const out = { method: 'Runtime.executionContextCreated', params: { context: contextDescription() } };
        if (sessionId !== '') out.sessionId = sessionId;
        state.outbox.push(out);
        // O V8 guarda as mensagens do console e as exceções do contexto e as repete a quem liga o domínio.
        for (const [method, params] of state.stored) {
          const replay = { method, params };
          if (sessionId !== '') replay.sessionId = sessionId;
          state.outbox.push(replay);
        }
      }
      reply(message, {});
    },
    'Runtime.disable': (message) => {
      state.enabled.delete(message.sessionId === undefined ? '' : message.sessionId);
      reply(message, {});
    },
    // O estado do `Emulation`, da janela e dos mundos que o hospedeiro guarda na aba e manda a cada documento novo.
    '__emulation': (message, params) => {
      const [outerWidth, outerHeight] = params.window;
      windowMetrics.outerWidth = outerWidth;
      windowMetrics.outerHeight = outerHeight;
      const override = params.override;
      windowMetrics.innerWidth = override ? override.width : outerWidth;
      windowMetrics.innerHeight = override ? override.height : outerHeight - 143;
      windowMetrics.devicePixelRatio = override ? override.scale : 1;
      for (const key of ['innerWidth', 'innerHeight', 'outerWidth', 'outerHeight', 'devicePixelRatio']) globalThis[key] = windowMetrics[key];
      emulation.touchPoints = params.touch;
      emulation.userAgent = params.userAgent;
      emulation.platform = params.platform;
      emulation.languages = params.acceptLanguage ? params.acceptLanguage.split(',').map((l) => l.split(';')[0].trim()).filter(Boolean) : null;
      emulation.geo = params.geo;
      emulation.offline = params.offline;
      for (const name of params.bindings) installBinding(name);
      emulation.media = params.media;
      emulation.features = params.features;
      state.worlds = new Set(params.worlds);
      op('setViewport', windowMetrics.innerWidth, windowMetrics.innerHeight, windowMetrics.devicePixelRatio, emulation.features['prefers-color-scheme'] === 'dark');
    },
    '__runScript': (message, params) => {
      try { evalGlobal(params.source, ''); } catch { /* o erro do script de documento novo não chega ao cliente */ }
    },
    'Runtime.evaluate': evaluate,
    'Runtime.callFunctionOn': callFunctionOn,
    'Runtime.getProperties': getProperties,
    'Runtime.releaseObject': (message, params) => {
      state.objects.delete(params.objectId);
      reply(message, {});
    },
    'Runtime.releaseObjectGroup': (message, params) => {
      for (const id of state.groups.get(params.objectGroup) || []) state.objects.delete(id);
      state.groups.delete(params.objectGroup);
      reply(message, {});
    },
    'Runtime.runIfWaitingForDebugger': (message) => reply(message, {}),
    'Runtime.getIsolateId': (message) => reply(message, { id: state.isolateId }),
    'Runtime.discardConsoleEntries': (message) => {
      state.stored.length = 0;
      reply(message, {});
    },
  };

  // ---- DOM: o `backendNodeId` é um contador do processo, atribuído na primeira vez que o cliente pergunta pelo nó.
  const backendOf = (node) => {
    let id = state.backendIds.get(node);
    if (id === undefined) { id = state.nextBackend++; state.backendIds.set(node, id); state.backend.set(id, node); }
    return id;
  };
  const nodeOf = (message, params) => {
    if (params.objectId !== undefined) {
      const found = lookup(message, params.objectId);
      if (!found.found) return null;
      if (!isNode(found.value)) { fail(message, -32000, 'Object id doesn\'t reference a Node'); return null; }
      return found.value;
    }
    if (params.backendNodeId !== undefined) {
      const node = state.backend.get(params.backendNodeId);
      if (node === undefined) { fail(message, -32000, 'No node with given id found'); return null; }
      return node;
    }
    fail(message, -32000, 'Either nodeId, backendNodeId or objectId must be specified');
    return null;
  };
  const describe = (node, depth) => {
    const out = { nodeId: 0, backendNodeId: backendOf(node), nodeType: node.nodeType, nodeName: node.nodeName, localName: node.nodeType === 1 ? node.localName : '', nodeValue: node.nodeType === 3 || node.nodeType === 8 || node.nodeType === 7 || node.nodeType === 4 ? node.nodeValue : '' };
    if (node.nodeType === 1 || node.nodeType === 9 || node.nodeType === 11) out.childNodeCount = node.childNodes.length;
    if (node.nodeType === 1) {
      out.attributes = [];
      for (const attr of node.attributes) out.attributes.push(attr.name, attr.value);
      if (node.localName === 'li' && depth === 0) out.pseudoElements = [{ nodeId: 0, backendNodeId: state.nextBackend++, nodeType: 1, nodeName: '::marker', localName: '::marker', nodeValue: '', childNodeCount: 0, attributes: [], pseudoType: 'marker' }];
    }
    return out;
  };
  const px = (value) => Number.parseFloat(value) || 0;
  // As caixas do elemento (borda, preenchimento, conteúdo e margem) em quadriláteros `[x1,y1,x2,y1,x2,y2,x1,y2]`.
  const boxes = (node) => {
    const b = node.nodeType === 1 ? boxOf(node) : null;
    if (b === null) return null;
    const cs = getComputedStyle(node);
    const quad = (l, t, r, bt) => [l, t, r, t, r, bt, l, bt];
    const [bt, br, bb, bl] = b.border, [pt, pr, pb, pl] = b.padding;
    const x1 = b.x, y1 = b.y, x2 = b.x + b.width, y2 = b.y + b.height;
    const margin = quad(x1 - px(cs.marginLeft), y1 - px(cs.marginTop), x2 + px(cs.marginRight), y2 + px(cs.marginBottom));
    const border = quad(x1, y1, x2, y2);
    const padding = quad(x1 + bl, y1 + bt, x2 - br, y2 - bb);
    const content = quad(x1 + bl + pl, y1 + bt + pt, x2 - br - pr, y2 - bb - pb);
    return { content, padding, border, margin, width: Math.round(b.width), height: Math.round(b.height) };
  };
  commands['DOM.describeNode'] = (message, params) => {
    const node = nodeOf(message, params);
    if (node !== null) reply(message, { node: describe(node, 0) });
  };
  commands['DOM.resolveNode'] = (message, params) => {
    const node = nodeOf(message, params);
    if (node !== null) reply(message, { object: remoteObject(node, { group: params.objectGroup }) });
  };
  commands['DOM.getBoxModel'] = (message, params) => {
    const node = nodeOf(message, params);
    if (node === null) return;
    const model = boxes(node);
    if (model === null) fail(message, -32000, 'Could not compute box model.'); else reply(message, { model });
  };
  commands['DOM.getContentQuads'] = (message, params) => {
    const node = nodeOf(message, params);
    if (node === null) return;
    const model = boxes(node);
    reply(message, { quads: model === null ? [] : [model.border] });
  };
  commands['DOM.scrollIntoViewIfNeeded'] = (message, params) => {
    const node = nodeOf(message, params);
    if (node === null) return;
    const b = node.nodeType === 1 ? boxOf(node) : null;
    if (b === null) { fail(message, -32000, 'Node does not have a layout object'); return; }
    if (b.y < 0 || b.y + b.height > windowMetrics.innerHeight) node.scrollIntoView?.({ block: 'center', inline: 'center' });
    reply(message, {});
  };

  // ---- Input: eventos de ponteiro e de teclado sobre o elemento sob o ponto e o elemento em foco.
  const pointer = { target: null, x: 0, y: 0 };
  const mouseInit = (params) => ({ bubbles: true, cancelable: true, composed: true, clientX: params.x, clientY: params.y, screenX: params.x, screenY: params.y, button: { left: 0, middle: 1, right: 2, back: 3, forward: 4 }[params.button] ?? 0, buttons: params.buttons || 0, detail: params.clickCount || 0, ctrlKey: !!(params.modifiers & 2), shiftKey: !!(params.modifiers & 8), altKey: !!(params.modifiers & 1), metaKey: !!(params.modifiers & 4) });
  commands['Input.dispatchMouseEvent'] = (message, params) => {
    const target = document.elementFromPoint(params.x, params.y);
    const init = mouseInit(params);
    if (params.type === 'mouseMoved') {
      if (pointer.target !== target) {
        if (pointer.target) pointer.target.dispatchEvent(new MouseEvent('mouseout', init));
        if (target) target.dispatchEvent(new MouseEvent('mouseover', init));
        pointer.target = target;
      }
      if (target) target.dispatchEvent(new MouseEvent('mousemove', init));
    } else if (params.type === 'mousePressed') {
      if (target) {
        target.dispatchEvent(new MouseEvent('mousedown', init));
        let focusable = target;
        while (focusable && !(focusable.tabIndex >= 0 || /^(input|textarea|select|button|a)$/.test(focusable.localName))) focusable = focusable.parentElement;
        if (focusable) focusable.focus?.();
      }
      pointer.down = target;
    } else if (params.type === 'mouseReleased') {
      if (target) {
        target.dispatchEvent(new MouseEvent('mouseup', init));
        if (pointer.down === target && init.button === 0) target.click?.();
        else if (pointer.down === target && init.button === 2) target.dispatchEvent(new MouseEvent('contextmenu', init));
      }
      pointer.down = null;
    } else if (params.type === 'mouseWheel') {
      if (target) target.dispatchEvent(new WheelEvent('wheel', { ...init, deltaX: params.deltaX || 0, deltaY: params.deltaY || 0 }));
    }
    reply(message, {});
  };
  // O texto entra no elemento editável em foco (`input`, `textarea` ou `contenteditable`) com `beforeinput` e `input`.
  const insert = (text) => {
    const el = document.activeElement;
    if (!el) return;
    const editable = (el.localName === 'input' || el.localName === 'textarea') && !el.readOnly && !el.disabled;
    if (!editable && !el.isContentEditable) return;
    if (!el.dispatchEvent(new InputEvent('beforeinput', { bubbles: true, cancelable: true, composed: true, data: text, inputType: 'insertText' }))) return;
    if (editable) {
      const start = el.selectionStart ?? el.value.length, end = el.selectionEnd ?? start;
      el.value = el.value.slice(0, start) + text + el.value.slice(end);
      el.selectionStart = el.selectionEnd = start + text.length;
    } else {
      el.textContent += text;
    }
    el.dispatchEvent(new InputEvent('input', { bubbles: true, composed: true, data: text, inputType: 'insertText' }));
  };
  commands['Input.insertText'] = (message, params) => { insert(String(params.text)); reply(message, {}); };
  commands['Input.dispatchKeyEvent'] = (message, params) => {
    const target = document.activeElement || document.body;
    const init = { bubbles: true, cancelable: true, composed: true, key: params.key || '', code: params.code || '', location: params.location || 0, repeat: !!params.autoRepeat, ctrlKey: !!(params.modifiers & 2), shiftKey: !!(params.modifiers & 8), altKey: !!(params.modifiers & 1), metaKey: !!(params.modifiers & 4) };
    if (params.type === 'keyUp') {
      target.dispatchEvent(new KeyboardEvent('keyup', init));
    } else if (params.type === 'keyDown' || params.type === 'rawKeyDown' || params.type === 'char') {
      const proceed = params.type === 'char' ? true : target.dispatchEvent(new KeyboardEvent('keydown', init));
      if (proceed && params.text) {
        if (params.type !== 'rawKeyDown') target.dispatchEvent(new KeyboardEvent('keypress', init));
        if (params.text === '\r') {
          if (target.localName === 'input') { const form = target.form; if (form) form.requestSubmit?.(); } else if (target.localName === 'button') target.click();
        } else {
          insert(params.text);
        }
      }
    }
    reply(message, {});
  };

  return {
    // Um marco do carregamento (`DOMContentLoaded`, `load`) para os eventos de `Page`, com o título do documento e se
    // ele tem o que pintar (os `first*Paint` do Chromium só saem com conteúdo).
    // O observador do console do motor (`PageRunner::set_console_hook`) e o relato de exceção não tratada.
    console: (kind, ...args) => consoleCall(kind, args),
    uncaught,
    // Roda `fn` sem que as chamadas de console dela virem mensagens da página.
    quietly(fn) {
      state.quiet++;
      try { return fn(); } finally { state.quiet--; }
    },
    lifecycle(name) {
      const body = document.body;
      const contentful = body !== null && (body.textContent.trim() !== '' || body.querySelector('img, canvas, svg, video, input, button, textarea, select, iframe') !== null);
      state.outbox.push({ $lifecycle: name, title: document.title, contentful, images: document.images.length > 0 });
    },
    // `init`: o isolate, o contexto, a moldura e a origem da página nova (o hospedeiro sabe o que a navegação deu).
    init(text) {
      const config = jsonParse(text);
      state.isolateId = config.isolateId;
      state.contextId = config.contextId;
      state.uniqueId = config.uniqueId;
      state.frameId = config.frameId;
      state.origin = config.origin;
      for (const sessionId of config.enabled) state.enabled.add(sessionId);
      emit('Runtime.executionContextCreated', { context: contextDescription() });
    },
    dispatch(text) {
      const message = jsonParse(text);
      const handler = commands[message.method];
      const params = message.params || {};
      const fromObject = typeof params.objectId === 'string' ? Number(params.objectId.split('.')[1]) : NaN;
      state.active = params.executionContextId !== undefined ? params.executionContextId : (fromObject === fromObject ? fromObject : state.contextId);
      if (handler === undefined) { fail(message, -32601, "'" + message.method + "' wasn't found"); return; }
      // Toda mensagem tem resposta: um cliente esperando uma que nunca vem trava.
      try { handler(message, message.params || {}); } catch (error) { fail(message, -32000, String(error && error.message)); }
    },
    drain() {
      const out = state.outbox.map((item) => jsonStringify(item));
      state.outbox.length = 0;
      return out.join('\n');
    },
    enabledSessions() {
      return jsonStringify([...state.enabled]);
    },
  };
})();
