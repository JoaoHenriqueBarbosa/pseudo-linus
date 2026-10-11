// node:util/types num programa principal ES: o namespace de um módulo é ModuleNamespaceObject, o objeto do require não.
import * as path from "node:path";
import types from "node:util/types";
import { isModuleNamespaceObject, isProxy } from "node:util/types";
console.log(types.isModuleNamespaceObject(path), isModuleNamespaceObject(path.default), isModuleNamespaceObject(types), isProxy(path));
const dynamic = await import("node:path");
console.log(dynamic === path, isModuleNamespaceObject(dynamic));
