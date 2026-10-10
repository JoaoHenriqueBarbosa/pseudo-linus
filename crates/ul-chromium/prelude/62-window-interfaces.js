// ---- Interfaces da janela que o Chromium expõe em toda página: as barras (`BarProp`), `external`, `styleMedia`,
// `event`, `clientInformation`, `frameElement`, os legados (`captureEvents`, `releaseEvents`, `find`, os
// `webkit*AnimationFrame`), `credentialless`, `fence`, `requestResize` e o objeto `chrome` (`loadTimes`, `csi` e
// `app`). A forma (acessor ou valor, nome e aridade das funções, membros enumeráveis do protótipo) e os retornos
// são os medidos no oráculo.

// Os membros do protótipo enumeráveis, como o WebIDL do Blink os instala, e a etiqueta do `toString`.
const webInterface = (cls, name) => {
  for (const key of Reflect.ownKeys(cls.prototype)) if (key !== 'constructor') Object.defineProperty(cls.prototype, key, { enumerable: true });
  return tag(cls, name);
};
// O receptor de um membro de interface: outro objeto é `Illegal invocation`, como no binding do Blink.
const receiver = (self, brand) => {
  if (self === null || (typeof self !== 'object' && typeof self !== 'function') || self[brand] === undefined) throw new TypeError('Illegal invocation');
};
// Aridade mínima de uma operação da janela, com a mensagem do binding.
const requireArguments = (operation, needed, given) => {
  if (given < needed) throw new TypeError(`Failed to execute '${operation}' on 'Window': ${needed} argument${needed === 1 ? '' : 's'} required, but only ${given} present.`);
};

class BarProp {
  constructor(key) {
    if (key !== ILLEGAL) throw illegalConstructor('BarProp');
    hidden(this, '__barProp', true);
  }
  // O headless desenha uma janela comum: toda barra está visível.
  get visible() { receiver(this, P.__barProp); return true; }
}
webInterface(BarProp, 'BarProp');

class External {
  constructor(key) {
    if (key !== ILLEGAL) throw illegalConstructor('External');
    hidden(this, '__external', true);
  }
  // As duas operações viraram vazias na especificação (sem efeito e sem retorno).
  AddSearchProvider() { receiver(this, P.__external); }
  IsSearchProviderInstalled() { receiver(this, P.__external); }
}
webInterface(External, 'External');

// O `StyleMedia` não é interface exposta: o protótipo do objeto não tem `constructor` próprio (herda o de Object).
const styleMediaPrototype = Object.create(Object.prototype, {
  type: { get: Object.getOwnPropertyDescriptor({ get type() { receiver(this, P.__styleMedia); return 'screen'; } }, 'type').get, enumerable: true, configurable: true },
  matchMedium: { value: { matchMedium(query) { receiver(this, P.__styleMedia); return matchMedia(String(query)).matches; } }.matchMedium, writable: true, enumerable: true, configurable: true },
  [Symbol.toStringTag]: { value: 'StyleMedia', configurable: true },
});
const styleMedia = Object.create(styleMediaPrototype);
hidden(styleMedia, '__styleMedia', true);

const external = new External(ILLEGAL);
const bars = {};
for (const name of ['locationbar', 'menubar', 'personalbar', 'scrollbars', 'statusbar', 'toolbar']) bars[name] = new BarProp(ILLEGAL);

// O tempo e o tipo da navegação que o `chrome.loadTimes` e o `chrome.csi` relatam: os instantes do Navigation Timing
// e a transição do `NavigationType` do Blink (o navegador só carrega navegações do tipo outra).
const loadTimes = [function () {
  const timing = performance.timing;
  const seconds = (ms) => ms / 1000;
  return {
    requestTime: seconds(timing.navigationStart),
    startLoadTime: seconds(timing.navigationStart),
    commitLoadTime: seconds(timing.responseStart),
    finishDocumentLoadTime: seconds(timing.domContentLoadedEventEnd),
    finishLoadTime: seconds(timing.loadEventEnd),
    firstPaintTime: 0,
    firstPaintAfterLoadTime: 0,
    navigationType: 'Other',
    wasFetchedViaSpdy: false,
    wasNpnNegotiated: false,
    npnNegotiatedProtocol: 'unknown',
    wasAlternateProtocolAvailable: false,
    connectionInfo: performance.getEntriesByType('navigation')[0]?.nextHopProtocol || 'unknown',
  };
}][0];
const csi = [function () {
  const timing = performance.timing;
  return { startE: timing.navigationStart, onloadT: timing.loadEventEnd, pageT: performance.now(), tran: 15 };
}][0];
const appInstallState = { DISABLED: 'disabled', INSTALLED: 'installed', NOT_INSTALLED: 'not_installed' };
const appRunningState = { CANNOT_RUN: 'cannot_run', READY_TO_RUN: 'ready_to_run', RUNNING: 'running' };
const chromeApp = {
  isInstalled: false,
  getDetails() { return null; },
  getIsInstalled() { return false; },
  // A resposta chega depois, pela mensagem ao processo do navegador; argumento que não é função é recusado.
  installState: [function () {
    const callback = arguments[0];
    if (callback !== undefined && typeof callback !== 'function') throw new TypeError('Error in invocation of app.installState(optional function callback): ');
    if (callback) setTimeout(() => callback('not_installed'), 0);
  }][0],
  runningState() { return 'cannot_run'; },
  InstallState: appInstallState,
  RunningState: appRunningState,
};
Object.defineProperty(chromeApp.installState, 'name', { value: 'installState' });
const chromeObject = { loadTimes, csi, app: chromeApp };

const windowLegacy = {
  captureEvents() {},
  releaseEvents() {},
  find() { return false; },
  webkitRequestAnimationFrame(callback) {
    requireArguments('webkitRequestAnimationFrame', 1, arguments.length);
    if (typeof callback !== 'function') throw new TypeError("Failed to execute 'webkitRequestAnimationFrame' on 'Window': parameter 1 is not of type 'Function'.");
    return requestAnimationFrame(callback);
  },
  webkitCancelAnimationFrame(handle) {
    requireArguments('webkitCancelAnimationFrame', 1, arguments.length);
    cancelAnimationFrame(Number(handle) | 0);
  },
  // O documento de topo nunca é um embutido responsivo: o pedido é sempre recusado (medido com e sem a meta).
  requestResize() {
    throw domException("Failed to execute 'requestResize' on 'Window': Requesting resize is allowed only from HTML documents with `responsive-embedded-sizing` meta tags.", 'NotAllowedError');
  },
};
for (const [key, value] of Object.entries(windowLegacy)) Object.defineProperty(globalThis, key, { value, writable: true, configurable: true, enumerable: true });
Object.defineProperty(globalThis, 'chrome', { value: chromeObject, writable: true, configurable: true, enumerable: true });

// Acessores da janela. Os `[Replaceable]` (`event`, `clientInformation`, as barras, `external`, `styleMedia`) viram
// propriedade de dados ao receber valor; os demais só têm leitura.
const replaceable = (key, read) => {
  const accessors = { get [key]() { return read(); }, set [key](value) { Object.defineProperty(globalThis, key, { value, writable: true, configurable: true, enumerable: true }); } };
  const descriptor = Object.getOwnPropertyDescriptor(accessors, key);
  Object.defineProperty(globalThis, key, { get: descriptor.get, set: descriptor.set, configurable: true, enumerable: true });
};
const readonly = (key, read) => {
  const descriptor = Object.getOwnPropertyDescriptor({ get [key]() { return read(); } }, key);
  Object.defineProperty(globalThis, key, { get: descriptor.get, configurable: true, enumerable: true });
};
for (const name of Object.keys(bars)) replaceable(name, () => bars[name]);
replaceable('external', () => external);
replaceable('event', () => currentEvent);
replaceable('clientInformation', () => navigator);
readonly('styleMedia', () => styleMedia);
readonly('frameElement', () => null);
readonly('credentialless', () => false);
readonly('fence', () => null);
defineHandlerProperties(globalThis, ['gamepadconnected', 'gamepaddisconnected']);

Object.assign(globalThis, { BarProp, External });
for (const key of ['BarProp', 'External']) Object.defineProperty(globalThis, key, { enumerable: false });
