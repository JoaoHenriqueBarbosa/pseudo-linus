import * as ns from "node:process";
import proc, { argv, env, nextTick, platform } from "node:process";
console.log("same", proc === process, ns.default === process);
console.log("named", argv === process.argv, env === process.env, nextTick === process.nextTick, platform);
console.log("ns keys", Object.keys(ns).includes("default"), Object.keys(ns).includes("argv"), Object.keys(ns).includes("cwd"));
nextTick(() => console.log("tick"));
console.log("version", typeof proc.version, proc.versions.bun);
await import("./main.cjs");
