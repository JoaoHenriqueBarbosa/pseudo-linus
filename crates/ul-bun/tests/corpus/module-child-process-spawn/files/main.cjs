const cp = require("node:child_process");
const log = [];
const done = (label, ...rest) => console.log(label, ...rest);

const echo = cp.spawn("echo", ["hello", "world"]);
echo.stdout.on("data", d => done("echo data", JSON.stringify(String(d))));
echo.on("spawn", () => done("echo spawn", typeof echo.pid));
echo.on("exit", (code, sig) => done("echo exit", code, sig));
echo.on("close", (code, sig) => {
  done("echo close", code, sig, echo.exitCode, echo.signalCode, echo.killed, echo.spawnfile, echo.spawnargs.join(" "));
  step2();
});

function step2() {
  cp.exec("echo out; echo err >&2; exit 2", (error, stdout, stderr) => {
    done("exec", error && error.code, JSON.stringify(stdout), JSON.stringify(stderr), error && error.killed, error && error.cmd);
    step3();
  });
}

function step3() {
  const cat = cp.spawn("cat");
  let out = "";
  cat.stdout.setEncoding("utf8");
  cat.stdout.on("data", d => (out += d));
  cat.on("close", code => {
    done("cat close", code, JSON.stringify(out));
    step4();
  });
  cat.stdin.write("line1\n");
  cat.stdin.end("line2\n");
}

function step4() {
  const missing = cp.spawn("nope-xyz-cmd");
  missing.on("error", e => {
    done("missing error", e.code, e.errno, e.syscall, e.path, e.spawnargs, e.message);
  });
  missing.on("close", (code, sig) => {
    done("missing close", code, sig);
    step5();
  });
}

function step5() {
  const sleeper = cp.spawn("sleep", ["5"]);
  sleeper.on("exit", (code, sig) => {
    done("sleeper exit", code, sig, sleeper.killed, sleeper.signalCode);
    step6();
  });
  done("kill", sleeper.kill());
}

function step6() {
  done("execSync", JSON.stringify(String(cp.execSync("echo sync"))));
  done("execFileSync", JSON.stringify(String(cp.execFileSync("echo", ["a", "b"]))));
  const r = cp.spawnSync("sh", ["-c", "echo o; echo e >&2; exit 4"]);
  done("spawnSync", r.status, r.signal, String(r.stdout), String(r.stderr), r.pid > 0, r.output.length, r.error);
  try {
    cp.execSync("exit 3", { stdio: "pipe" });
  } catch (e) {
    done("execSync fail", e.status, e.signal, e.message.split("\n")[0]);
  }
  const m = cp.spawnSync("nope-xyz-cmd");
  done("spawnSync missing", m.status, m.error && m.error.code, m.error && m.error.syscall, m.error && m.error.path);
  cp.execFile("echo", ["file"], (err, out) => done("execFile", err, JSON.stringify(out)));
}
