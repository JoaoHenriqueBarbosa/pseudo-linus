// `Bun.listen` e `Bun.connect` do bun 1.4.2 (ver `bun_http.rs` e `net_loop.rs`): sockets TCP de loopback sobre as funções `net*`
// do `Host`. Programa escondido: recebe o `host` e devolve `{ listen, connect }`. O socket e o ouvinte são objetos comuns
// (`constructor.name` é `Object`, as chaves próprias são vazias) cujos métodos moram no protótipo, o estado fica num WeakMap.
// Medido no oráculo bun 1.4.2: os handlers `open`, `data`, `drain`, `end`, `close`, `error` e `connectError`, `write` devolve
// os bytes aceitos, `end` termina depois de escrever e dispara `close`, o `connect` chama `connectError` e rejeita.
(function (host) {
  "use strict";

  const defer = setImmediate;
  const EAGAIN = 11;
  const ECONNREFUSED = 111;
  const kReadyOpen = 1;
  const kReadyClosed = -1;

  const states = new WeakMap();
  const listeners = new WeakMap();
  const noop = () => {};

  function fail(kind, code, message) {
    host.throwNodeError(kind, code, message);
  }

  function loopback(hostname) {
    return hostname === undefined || hostname === "localhost" || hostname === "0.0.0.0" || hostname === "::1" || /^127(\.\d{1,3}){3}$/.test(hostname);
  }

  // `SocketOptions.socket` e as outras validações das opções (mensagens medidas).
  function validate(options, what) {
    if (options === null || typeof options !== "object") fail("TypeError", "ERR_INVALID_ARG_TYPE", what === "listen" ? "Expected object" : "Expected options object");
    const handlers = options.socket;
    if (handlers === undefined || handlers === null || typeof handlers !== "object") fail("TypeError", "ERR_INVALID_ARG_TYPE", "SocketOptions.socket is required");
    if (typeof handlers.data !== "function" && typeof handlers.drain !== "function") fail("TypeError", "ERR_INVALID_ARG_TYPE", 'Expected at least "data" or "drain" callback');
    const port = options.port;
    if (port !== undefined) {
      const number = Number(port);
      if (!(number >= 0 && number <= 65535)) {
        fail("RangeError", "ERR_OUT_OF_RANGE", `SocketOptions.port must be in the range [0, 65535] (received ${number})`);
      }
    }
    return handlers;
  }

  function bytesOf(data, offset, length) {
    let bytes;
    if (typeof data === "string") bytes = globalThis.Buffer.from(data, "utf8");
    else if (ArrayBuffer.isView(data)) bytes = new Uint8Array(data.buffer, data.byteOffset, data.byteLength);
    else if (data instanceof ArrayBuffer || (typeof SharedArrayBuffer !== "undefined" && data instanceof SharedArrayBuffer)) bytes = new Uint8Array(data);
    else fail("TypeError", "ERR_INVALID_ARG_TYPE", "Expected an ArrayBuffer, TypedArray, DataView or string");
    const start = offset === undefined ? 0 : offset;
    const end = length === undefined ? bytes.length : Math.min(bytes.length, start + length);
    return start === 0 && end === bytes.length ? bytes : bytes.subarray(start, end);
  }

  // Um handler que lança: o erro vai ao handler `error` se o socket tem, senão é exceção não tratada.
  function call(state, name, ...args) {
    const handler = state.handlers[name];
    if (typeof handler !== "function") return undefined;
    try {
      return handler.call(state.handlers, state.socket, ...args);
    } catch (error) {
      if (name !== "error" && typeof state.handlers.error === "function") {
        try {
          state.handlers.error.call(state.handlers, state.socket, error);
          return undefined;
        } catch (inner) {
          error = inner;
        }
      }
      queueMicrotask(() => {
        throw error;
      });
      return undefined;
    }
  }

  function rewatch(state) {
    if (state.fd < 0) return;
    host.netWatch(state.fd, !state.paused, state.needsDrain, (readable, writable, hangup, error) => {
      if (state.fd < 0) return;
      if (writable && state.needsDrain) {
        state.needsDrain = false;
        call(state, "drain");
      }
      if (readable || hangup || error) pump(state);
      if (state.fd >= 0) rewatch(state);
    });
    host.netRef(state.fd, state.refed);
  }

  // O que o descritor tem agora: cada pedaço vai ao `data`, o fim da conexão ao `end` e ao `close`.
  function pump(state) {
    for (;;) {
      if (state.fd < 0 || state.paused) return;
      const result = host.netRecv(state.fd, 65536);
      if (typeof result === "number") {
        if (result === -EAGAIN) return;
        if (result === -104 || result === -32) {
          finish(state, "end", undefined);
          return;
        }
        const error = new Error("socket error");
        error.errno = result;
        error.code = result === -104 ? "ECONNRESET" : "EIO";
        finish(state, "error", error);
        return;
      }
      if (result.length === 0) {
        finish(state, "end", undefined);
        return;
      }
      state.received += result.length;
      call(state, "data", globalThis.Buffer.from(result.buffer, result.byteOffset, result.length));
    }
  }

  // Fecha o descritor e avisa o programa uma vez: `end` (se veio do outro lado) ou `error`, depois `close`.
  function finish(state, why, error) {
    if (state.fd < 0) return;
    const fd = state.fd;
    state.fd = -1;
    state.readyState = kReadyClosed;
    host.netClose(fd);
    if (why === "end") call(state, "end");
    if (why === "error") call(state, "error", error);
    if (!state.closeCalled) {
      state.closeCalled = true;
      call(state, "close", error);
    }
    if (state.owner !== undefined) state.owner.sockets.delete(state.socket);
    states.delete(state.socket);
  }

  function stateOf(socket) {
    const state = states.get(socket);
    if (state === undefined) {
      const error = new TypeError(`Expected this to be instanceof Socket`);
      error.code = "ERR_INVALID_THIS";
      throw error;
    }
    return state;
  }

  // Os métodos do socket. Os de TLS não têm o que fazer sobre TCP puro: a mesma resposta vazia do bun sem sessão.
  const SocketPrototype = {};
  const method = (name, length, body) => {
    const fn = {
      [name](...args) {
        return body.apply(this, args);
      },
    }[name];
    Object.defineProperty(fn, "length", { value: length });
    Object.defineProperty(SocketPrototype, name, { value: fn, writable: true, configurable: true, enumerable: false });
  };
  const getter = (name, read, write) => {
    Object.defineProperty(SocketPrototype, name, { get: read, set: write, configurable: true, enumerable: false });
  };

  method("write", 3, function (data, offset, length) {
    const state = stateOf(this);
    if (state.fd < 0) return -1;
    const bytes = bytesOf(data, offset, length);
    let sent = 0;
    while (sent < bytes.length) {
      const result = host.netSend(state.fd, sent === 0 ? bytes : bytes.subarray(sent));
      if (result < 0) {
        if (result === -EAGAIN) {
          state.needsDrain = true;
          rewatch(state);
          break;
        }
        queueMicrotask(() => finish(state, "error", Object.assign(new Error("write failed"), { errno: result, code: "EPIPE" })));
        return -1;
      }
      sent += result;
    }
    state.written += sent;
    return sent;
  });
  method("end", 3, function (data, offset, length) {
    const state = stateOf(this);
    if (state.fd < 0) return 0;
    const sent = data === undefined ? 0 : this.write(data, offset, length);
    state.ending = true;
    queueMicrotask(() => finish(state, "close", undefined));
    return sent;
  });
  method("shutdown", 1, function () {
    const state = stateOf(this);
    if (state.fd >= 0) host.netShutdown(state.fd, false, true);
  });
  method("close", 0, function () {
    const state = stateOf(this);
    queueMicrotask(() => finish(state, "close", undefined));
  });
  method("terminate", 0, function () {
    const state = stateOf(this);
    finish(state, "close", undefined);
  });
  method("flush", 0, function () {});
  method("ref", 0, function () {
    const state = stateOf(this);
    state.refed = true;
    if (state.fd >= 0) host.netRef(state.fd, true);
  });
  method("unref", 0, function () {
    const state = stateOf(this);
    state.refed = false;
    if (state.fd >= 0) host.netRef(state.fd, false);
  });
  method("pause", 0, function () {
    const state = stateOf(this);
    state.paused = true;
    rewatch(state);
  });
  method("resume", 0, function () {
    const state = stateOf(this);
    state.paused = false;
    rewatch(state);
  });
  method("timeout", 1, function (seconds) {
    stateOf(this).timeoutSeconds = seconds;
  });
  method("setNoDelay", 1, function () {
    return true;
  });
  method("setKeepAlive", 2, function () {
    return true;
  });
  method("reload", 1, function (options) {
    const state = stateOf(this);
    if (options !== null && typeof options === "object" && options.socket !== undefined) state.handlers = options.socket;
  });
  for (const name of [
    "disableRenegotiation", "exportKeyingMaterial", "getAuthorizationError", "getCertificate", "getCipher", "getEphemeralKeyInfo",
    "getPeerCertificate", "getServername", "getSession", "getSharedSigalgs", "getTLSFinishedMessage", "getTLSPeerFinishedMessage",
    "getTLSTicket", "getTLSVersion", "getTypeOfService", "isSessionReused", "renegotiate", "resumeSNI", "setKeyCert",
    "setMaxSendFragment", "setServername", "setSession", "setTypeOfService", "setVerifyMode", "upgradeTLS",
  ]) {
    method(name, 0, function () {
      return undefined;
    });
  }
  getter("alpnProtocol", () => null);
  getter("authorized", () => false);
  getter("bytesWritten", function () {
    return stateOf(this).written;
  });
  getter("data", function () {
    return stateOf(this).data;
  }, function (value) {
    stateOf(this).data = value;
  });
  getter("fd", function () {
    return stateOf(this).fd;
  });
  getter("listener", function () {
    return stateOf(this).listener;
  });
  getter("readyState", function () {
    const state = states.get(this);
    return state === undefined ? kReadyClosed : state.readyState;
  });
  getter("localAddress", function () {
    return "127.0.0.1";
  });
  getter("localFamily", function () {
    return "IPv4";
  });
  getter("localPort", function () {
    return stateOf(this).localPort;
  });
  getter("remoteAddress", function () {
    return "127.0.0.1";
  });
  getter("remoteFamily", function () {
    return "IPv4";
  });
  getter("remotePort", function () {
    return stateOf(this).remotePort;
  });

  function createSocket(fd, handlers, data, listener, localPort, remotePort) {
    const socket = Object.create(SocketPrototype);
    const state = {
      socket, fd, handlers, data, listener, localPort, remotePort, readyState: kReadyOpen, paused: false, refed: true,
      needsDrain: false, closeCalled: false, ending: false, written: 0, received: 0, timeoutSeconds: 0,
    };
    states.set(socket, state);
    return state;
  }

  // ---- ouvinte ----

  const ListenerPrototype = {};
  const listenerMethod = (name, length, body) => {
    const fn = {
      [name](...args) {
        const state = listeners.get(this);
        if (state === undefined) {
          const error = new TypeError("Expected this to be instanceof SocketListener");
          error.code = "ERR_INVALID_THIS";
          throw error;
        }
        return body.call(this, state, ...args);
      },
    }[name];
    Object.defineProperty(fn, "length", { value: length });
    Object.defineProperty(ListenerPrototype, name, { value: fn, writable: true, configurable: true, enumerable: false });
  };
  const listenerGetter = (name, read, write) => {
    Object.defineProperty(ListenerPrototype, name, {
      get() {
        return read(listeners.get(this));
      },
      set: write && function (value) {
        write(listeners.get(this), value);
      },
      configurable: true,
      enumerable: false,
    });
  };
  listenerMethod("stop", 1, function (state, closeActive) {
    if (state.fd < 0) return;
    host.netClose(state.fd);
    state.fd = -1;
    if (closeActive === true) for (const open of [...state.sockets]) open.terminate();
  });
  listenerMethod("ref", 0, function (state) {
    state.refed = true;
    if (state.fd >= 0) host.netRef(state.fd, true);
  });
  listenerMethod("unref", 0, function (state) {
    state.refed = false;
    if (state.fd >= 0) host.netRef(state.fd, false);
  });
  listenerMethod("reload", 1, function (state, options) {
    if (options !== null && typeof options === "object" && options.socket !== undefined) state.handlers = options.socket;
    return this;
  });
  listenerMethod("getsockname", 1, function (state, out) {
    const info = { family: "IPv4", address: state.hostname, port: state.port };
    if (out !== null && typeof out === "object") return Object.assign(out, info);
    return info;
  });
  listenerGetter("data", state => state.data, (state, value) => {
    state.data = value;
  });
  listenerGetter("fd", state => state.fd);
  listenerGetter("hostname", state => state.hostname);
  listenerGetter("port", state => state.port);
  listenerGetter("unix", () => undefined);

  function accept(state) {
    for (;;) {
      if (state.fd < 0) return;
      const accepted = host.netAccept(state.fd);
      if (typeof accepted === "number") return;
      const [fd, peer] = accepted;
      const connection = createSocket(fd, state.handlers, state.data, state.listener, state.port, peer);
      state.sockets.add(connection.socket);
      connection.owner = state;
      call(connection, "open");
      rewatch(connection);
    }
  }

  function listen(options) {
    const handlers = validate(options, "listen");
    if (options.unix !== undefined) fail("Error", "ERR_SOCKET_UNSUPPORTED", "unix sockets are not supported");
    const hostname = options.hostname ?? options.host ?? "0.0.0.0";
    const result = host.netListen(options.port === undefined ? 0 : Number(options.port), 512);
    if (typeof result === "number") {
      const error = new Error(`Failed to listen at ${hostname}`);
      error.code = result === -98 ? "EADDRINUSE" : "ERR_SOCKET_LISTEN_FAILED";
      error.errno = result;
      error.syscall = "listen";
      throw error;
    }
    const [fd, port] = result;
    const listener = Object.create(ListenerPrototype);
    const state = { fd, port, hostname, handlers, data: options.data, refed: true, sockets: new Set(), listener };
    listeners.set(listener, state);
    host.netWatch(fd, true, false, () => accept(state));
    return listener;
  }

  // ---- cliente ----

  function connect(options) {
    const handlers = validate(options, "connect");
    return new Promise((resolve, reject) => {
      const hostname = options.hostname ?? options.host ?? "localhost";
      const port = Number(options.port);
      const refuse = errno => {
        const error = new Error("Failed to connect");
        error.code = errno === ECONNREFUSED ? "ECONNREFUSED" : "ENETUNREACH";
        error.errno = -errno;
        error.syscall = "connect";
        const stub = createSocket(-1, handlers, options.data, undefined, 0, port);
        stub.readyState = kReadyClosed;
        if (typeof handlers.connectError === "function") {
          try {
            handlers.connectError.call(handlers, stub.socket, error);
          } catch (thrown) {
            queueMicrotask(() => {
              throw thrown;
            });
          }
        }
        states.delete(stub.socket);
        reject(error);
      };
      // No bun a conexão se completa num turno do laço de eventos, depois do accept do listener: o `open` e a
      // recusa esperam o `poll` (a recusa, o `defer`, que é o `setImmediate`, depois dele).
      if (!loopback(hostname)) {
        defer(() => refuse(ECONNREFUSED));
        return;
      }
      const result = host.netConnect(port);
      if (typeof result === "number") {
        defer(() => refuse(-result));
        return;
      }
      const [fd, localPort] = result;
      host.netWatch(fd, false, true, () => {
        const connection = createSocket(fd, handlers, options.data, undefined, localPort, port);
        call(connection, "open");
        rewatch(connection);
        resolve(connection.socket);
      });
    });
  }

  return { listen, connect };
});
