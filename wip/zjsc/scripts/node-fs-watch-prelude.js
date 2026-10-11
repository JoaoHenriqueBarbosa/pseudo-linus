  // O `internal/fs/binding` do bun (`watch` e `watchFile` nativos) sobre o `node:fs`: o sysabi não tem inotify, então o
  // observador compara `stat` e `readdir` a cada passo. O que se vê: `change` quando o conteúdo ou os atributos mudam,
  // `rename` quando um nome aparece ou some; a quantidade de eventos de um mesmo gesto é a de uma comparação por
  // passo, não a de uma fila do kernel (declarado em `NOTICE` e no `PLAN`).
  const fsModule = host.requireBuiltin("node:fs");
  const pathModule = host.requireBuiltin("node:path");
  const WATCH_STEP_MS = 50;
  // O `ignore` por texto casa como o `Bun.Glob` do bun: aqui pelo minimatch do `fs.glob`.
  const globMatcher = pattern => host.asset("glob").createMatcher(pattern);
  const uvErrors = new Map([
    [-1, ["EPERM", "operation not permitted"]],
    [-2, ["ENOENT", "no such file or directory"]],
    [-13, ["EACCES", "permission denied"]],
    [-20, ["ENOTDIR", "not a directory"]],
  ]);
  const processBindingUv = { getErrorMap: () => uvErrors };

  const signatureOf = stat => [stat.ino, stat.size, stat.mtimeMs, stat.ctimeMs, stat.mode, stat.nlink].join(":");

  // `{ nome: assinatura }` do que há debaixo de `path` (o próprio arquivo sob o nome vazio), ou null se não existe.
  function snapshot(path, recursive) {
    let stat;
    try {
      stat = fsModule.lstatSync(path);
    } catch {
      return null;
    }
    const entries = { "": signatureOf(stat) };
    if (!stat.isDirectory()) return entries;
    const walk = (directory, prefix) => {
      let names;
      try {
        names = fsModule.readdirSync(directory, { withFileTypes: true });
      } catch {
        return;
      }
      for (const entry of names) {
        const name = prefix + entry.name;
        try {
          entries[name] = signatureOf(fsModule.lstatSync(directory + "/" + entry.name));
        } catch {
          continue;
        }
        if (recursive && entry.isDirectory()) walk(directory + "/" + entry.name, name + "/");
      }
    };
    walk(path, "");
    return entries;
  }

  function watchError(code, path) {
    const entry = [...uvErrors.entries()].find(([, value]) => value[0] === code);
    const error = new Error(`${code}: ${entry[1][1]}, watch '${path}'`);
    error.code = code;
    error.path = path;
    error.syscall = "watch";
    error.errno = entry[0];
    return error;
  }

  const binding = {
    watch(path, options, onEvent) {
      const { persistent = true, recursive = false, encoding = "utf8", signal } = options;
      const resolved = pathModule.resolve(path);
      let before = snapshot(resolved, recursive);
      if (before === null) throw watchError("ENOENT", path);
      const label = name => (encoding === "buffer" ? Buffer.from(name) : name);
      const baseName = pathModule.basename(resolved);
      const directory = fsModule.lstatSync(resolved).isDirectory();
      let closed = false;
      let timer;
      const step = () => {
        const after = snapshot(resolved, recursive);
        if (after === null) {
          if (before !== null) onEvent("rename", label(baseName));
          before = null;
          return;
        }
        if (before === null) {
          onEvent("rename", label(baseName));
          before = after;
          return;
        }
        const names = [...new Set([...Object.keys(before), ...Object.keys(after)])].sort();
        for (const name of names) {
          if (closed) return;
          // O diretório em si não relata a mudança dos filhos.
          if (name === "" && directory) continue;
          const shown = label(name === "" ? baseName : name);
          if (!(name in after) || !(name in before)) onEvent("rename", shown);
          else if (before[name] !== after[name]) onEvent("change", shown);
        }
        before = after;
      };
      const close = () => {
        if (closed) return;
        closed = true;
        clearInterval(timer);
        signal?.removeEventListener("abort", abort);
        onEvent("close");
      };
      const abort = () => {
        if (closed) return;
        onEvent("error", signal.reason, true);
        close();
      };
      timer = setInterval(step, WATCH_STEP_MS);
      if (!persistent) timer.unref?.();
      if (signal) {
        if (signal.aborted) queueMicrotask(abort);
        else signal.addEventListener("abort", abort, { once: true });
      }
      return {
        close,
        ref: () => timer.ref?.(),
        unref: () => timer.unref?.(),
      };
    },

    watchFile(path, options, onChange) {
      const { persistent = true, interval = 5007, bigint = false } = options || {};
      const StatsClass = fsModule.Stats;
      const zeroed = bigint
        ? () => new (fsModule.statSync("/", { bigint: true }).constructor)(...new Array(18).fill(0n))
        : () => new StatsClass(...new Array(14).fill(0));
      const read = () => {
        try {
          return fsModule.statSync(path, { bigint });
        } catch {
          return zeroed();
        }
      };
      const keyOf = stat => [stat.dev, stat.ino, stat.mode, stat.nlink, stat.uid, stat.gid, stat.size, stat.mtimeMs, stat.ctimeMs].join(":");
      let previous = read();
      let first = previous.ino === 0 || previous.ino === 0n;
      const timer = setInterval(() => {
        const current = read();
        // Um arquivo que não existe dá uma chamada com tudo zerado no primeiro passo (como o libuv e o Node).
        if (first || keyOf(current) !== keyOf(previous)) {
          first = false;
          const before = previous;
          previous = current;
          onChange(current, before);
        }
      }, interval);
      if (!persistent) timer.unref?.();
      return {
        close: () => clearInterval(timer),
        ref: () => timer.ref?.(),
        unref: () => timer.unref?.(),
      };
    },
  };
