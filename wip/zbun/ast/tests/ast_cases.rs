//! Bloco 4 do porte dos nós: a AST dos oito casos do plano (`wip/notes/bun-transpiler-t0.md`), montada à mão no
//! formato que o parser do upstream produz para cada entrada, e percorrida em pré-ordem. O teste fixa que os tipos
//! de nó portados se compõem (lista dentro de nó dentro de lista, cordas, decorators, `StmtOrExpr`) e que a visita
//! pelo `Store` alcança cada nó uma vez. A fidelidade de texto contra o bun do oráculo entra com o parser.

use bun_ast::{
    B, Binding, ClauseItem, E, EnumValue, Expr, ExprData, G, LocRef, NodeStore, OpCode, Ref, S, Stmt, StmtData,
    StmtOrExpr, Store, StoreSlice, flags,
};

fn loc(start: i32) -> bun_ast::Loc {
    bun_ast::Loc { start }
}

fn sym(n: u32) -> Ref {
    Ref::new(n, 0, bun_ast::RefTag::Symbol)
}

fn string(store: &mut Store, text: &[u8]) -> Expr {
    let data = store.alloc_str(text);
    Expr::init(store, E::String::init(data), loc(0))
}

fn number(store: &mut Store, value: f64) -> Expr {
    Expr::init(store, E::Number::new(value), loc(0))
}

fn ident_binding(store: &mut Store, n: u32) -> Binding {
    Binding::alloc(store, B::Identifier { r#ref: sym(n) }, loc(0))
}

fn local(store: &mut Store, kind: S::Kind, name: u32, value: Expr, is_export: bool) -> Stmt {
    let binding = ident_binding(store, name);
    let decls = store.alloc_slice([G::Decl { binding, value: Some(value) }]);
    Stmt::alloc(store, S::Local { kind, decls, is_export, ..Default::default() }, loc(0))
}

/// Os nomes dos nós em pré-ordem, como os `tag()` do upstream os escrevem.
struct Walk<'s> {
    store: &'s Store,
    out: Vec<String>,
}

impl Walk<'_> {
    fn stmts(&mut self, stmts: StoreSlice<Stmt>) {
        for i in 0..stmts.len() {
            self.stmt(self.store[stmts][i]);
        }
    }

    fn stmt(&mut self, stmt: Stmt) {
        let store = self.store;
        self.out.push(<&'static str>::from(stmt.data.tag()).to_owned());
        match stmt.data {
            StmtData::SLocal(s) => {
                for decl in store[store[s].decls].iter() {
                    self.binding(decl.binding);
                    if let Some(value) = decl.value {
                        self.expr(value);
                    }
                }
            }
            StmtData::SExpr(s) => self.expr(store[s].value),
            StmtData::SEnum(s) => {
                for value in store[store[s].values].iter() {
                    self.out.push(format!("enum_value {}", String::from_utf8_lossy(store.str(value.name))));
                    if let Some(v) = value.value {
                        self.expr(v);
                    }
                }
            }
            StmtData::SNamespace(s) => self.stmts(store[s].stmts),
            StmtData::SImport(s) => {
                for item in store[store[s].items].iter() {
                    self.out.push(format!("clause {}", String::from_utf8_lossy(store.str(item.alias))));
                }
            }
            StmtData::SClass(s) => self.class(&store[s].class),
            StmtData::SExportDefault(s) => match store[s].value {
                StmtOrExpr::Stmt(inner) => self.stmt(inner),
                StmtOrExpr::Expr(inner) => self.expr(inner),
            },
            StmtData::SFunction(s) => self.stmts(store[s].func.body.stmts),
            StmtData::SReturn(s) => {
                if let Some(value) = store[s].value {
                    self.expr(value);
                }
            }
            _ => {}
        }
    }

    fn class(&mut self, class: &G::Class) {
        let store = self.store;
        self.exprs(class.ts_decorators);
        for property in store[class.properties].iter() {
            self.out.push(format!("property {}", <&'static str>::from(property.kind)));
            self.exprs(property.ts_decorators);
            for e in [property.key, property.value, property.initializer].into_iter().flatten() {
                self.expr(e);
            }
        }
    }

    fn exprs(&mut self, exprs: StoreSlice<Expr>) {
        for i in 0..exprs.len() {
            self.expr(self.store[exprs][i]);
        }
    }

    fn expr(&mut self, expr: Expr) {
        let store = self.store;
        self.out.push(expr.data.tag_name().to_owned());
        match expr.data {
            ExprData::EBinary(b) => {
                self.expr(store[b].left);
                self.expr(store[b].right);
            }
            ExprData::EObject(o) => {
                for property in store[store[o].properties].iter() {
                    for e in [property.key, property.value].into_iter().flatten() {
                        self.expr(e);
                    }
                }
            }
            ExprData::ECall(c) => {
                self.expr(store[c].target);
                self.exprs(store[c].args);
            }
            ExprData::EArray(a) => self.exprs(store[a].items),
            ExprData::EString(s) => {
                let s = store[s].flattened_view(store);
                self.out.push(format!("\"{}\"", String::from_utf8_lossy(&s)));
            }
            ExprData::EFunction(f) => self.stmts(store[f].func.body.stmts),
            ExprData::EClass(c) => self.class(&store[c]),
            _ => {}
        }
    }

    fn binding(&mut self, binding: Binding) {
        self.out.push(<&'static str>::from(binding.data.tag()).to_owned());
    }
}

/// Lê uma `EString` (corda ou não) sem gravar no `Store`, para o teste poder percorrer com `&Store`.
trait FlattenedView {
    fn flattened_view(&self, store: &Store) -> Vec<u8>;
}

impl FlattenedView for E::String {
    fn flattened_view(&self, store: &Store) -> Vec<u8> {
        let mut bytes = store.str(self.data).to_vec();
        let mut next = self.next;
        while let Some(part) = next {
            bytes.extend_from_slice(store.str(store[part].data));
            next = store[part].next;
        }
        bytes
    }
}

fn walk(store: &Store, stmts: StoreSlice<Stmt>) -> Vec<String> {
    let mut w = Walk { store, out: Vec::new() };
    w.stmts(stmts);
    w.out
}

fn assert_walk(store: &Store, stmts: StoreSlice<Stmt>, expected: &[&str]) {
    assert_eq!(walk(store, stmts), expected);
}

/// `const x = 1 + 2;`
#[test]
fn base_case() {
    let mut store = Store::default();
    let (one, two) = (number(&mut store, 1.0), number(&mut store, 2.0));
    let sum = Expr::init(&mut store, E::Binary { left: one, right: two, op: OpCode::BinAdd }, loc(10));
    let stmt = local(&mut store, S::Kind::KConst, 1, sum, false);
    let stmts = store.alloc_slice([stmt]);
    assert_walk(&store, stmts, &["s_local", "b_identifier", "binary", "number", "number"]);
}

/// `enum Color { Red, Green = 4, Blue }`: o upstream guarda os valores no `S::Enum`, e os ausentes ficam `None`
/// até a visita numerar.
#[test]
fn enum_case() {
    let mut store = Store::default();
    let four = number(&mut store, 4.0);
    let values: Vec<EnumValue> = [(&b"Red"[..], None), (b"Green", Some(four)), (b"Blue", None)]
        .into_iter()
        .enumerate()
        .map(|(i, (name, value))| EnumValue {
            loc: loc(i as i32),
            ref_: sym(10 + i as u32),
            name: store.alloc_str(name),
            value,
        })
        .collect();
    let values = store.alloc_slice(values);
    let name = LocRef { loc: loc(5), ref_: sym(1) };
    let stmt = Stmt::alloc(&mut store, S::Enum { name, arg: sym(2), values, is_export: false }, loc(0));
    let stmts = store.alloc_slice([stmt]);
    assert_walk(&store, stmts, &["s_enum", "enum_value Red", "enum_value Green", "number", "enum_value Blue"]);

    // `name_as_e_string` reencoda o nome (ASCII aqui) como o printer pede.
    let green = store[values][1];
    let green = green.name_as_e_string(&mut store);
    assert!(green.eql_comptime(&store, b"Green"));
}

/// `namespace NS { export const a = 1; }`
#[test]
fn namespace_case() {
    let mut store = Store::default();
    let one = number(&mut store, 1.0);
    let inner = local(&mut store, S::Kind::KConst, 3, one, true);
    let inner = store.alloc_slice([inner]);
    let name = LocRef { loc: loc(10), ref_: sym(1) };
    let stmt = Stmt::alloc(&mut store, S::Namespace { name, arg: sym(2), stmts: inner, is_export: false }, loc(0));
    let stmts = store.alloc_slice([stmt]);
    assert_walk(&store, stmts, &["s_namespace", "s_local", "b_identifier", "number"]);
    let StmtData::SNamespace(ns) = store[stmts][0].data else { unreachable!() };
    let StmtData::SLocal(l) = store[store[ns].stmts][0].data else { panic!("o corpo do namespace perdeu o s_local") };
    assert!(store[l].is_export);
}

/// `import type { T } from "./t"; import { v } from "./v";`: a importação só de tipo vira `s_type_script`, a
/// outra fica com a cláusula.
#[test]
fn import_type_case() {
    let mut store = Store::default();
    let type_only = Stmt::alloc(&mut store, S::TypeScript {}, loc(0));
    let alias = store.alloc_str(b"v");
    let items = store.alloc_slice([ClauseItem {
        alias,
        alias_loc: loc(40),
        name: LocRef { loc: loc(40), ref_: sym(3) },
        original_name: alias,
    }]);
    let import = Stmt::alloc(
        &mut store,
        S::Import { namespace_ref: sym(2), items, import_record_index: 0, ..Default::default() },
        loc(31),
    );
    let stmts = store.alloc_slice([type_only, import]);
    assert_walk(&store, stmts, &["s_type_script", "s_import", "clause v"]);
}

/// `const cfg = { port: 1 } satisfies Config;`: o `satisfies` não deixa nó, sobra o objeto.
#[test]
fn satisfies_case() {
    let mut store = Store::default();
    let object = store.alloc(E::Object::default());
    let one = number(&mut store, 1.0);
    let object_expr = Expr { loc: loc(12), data: ExprData::EObject(object) };
    E::Object::put(&mut store, object, b"port", one);
    let stmt = local(&mut store, S::Kind::KConst, 1, object_expr, false);
    let stmts = store.alloc_slice([stmt]);
    assert_walk(&store, stmts, &["s_local", "b_identifier", "object", "string", "\"port\"", "number"]);
}

/// `@sealed class Greeter { @log greet() { return "hi"; } }`
#[test]
fn decorators_case() {
    let mut store = Store::default();
    let sealed = Expr::init_identifier(sym(5), loc(1));
    let log = Expr::init_identifier(sym(6), loc(30));
    let hi = string(&mut store, b"hi");
    let ret = Stmt::alloc(&mut store, S::Return { value: Some(hi) }, loc(50));
    let body = store.alloc_slice([ret]);
    let mut func = G::Fn::default();
    func.body = G::FnBody { loc: loc(45), stmts: body };
    func.flags = flags::Function::IsUniqueFormalParameters.into();
    let method = Expr::init(&mut store, E::Function { func }, loc(35));
    let key = string(&mut store, b"greet");
    let method_decorators = store.alloc_slice([log]);
    let properties = store.alloc_slice([G::Property {
        key: Some(key),
        value: Some(method),
        flags: flags::Property::IsMethod.into(),
        ts_decorators: method_decorators,
        ..Default::default()
    }]);
    let class_decorators = store.alloc_slice([sealed]);
    let class = G::Class {
        class_name: Some(LocRef { loc: loc(15), ref_: sym(1) }),
        ts_decorators: class_decorators,
        properties,
        has_decorators: true,
        ..Default::default()
    };
    let stmt = Stmt::alloc(&mut store, S::Class { class, is_export: false }, loc(0));
    let stmts = store.alloc_slice([stmt]);
    assert_walk(
        &store,
        stmts,
        &[
            "s_class", "identifier", "property normal", "identifier", "string", "\"greet\"", "function", "s_return",
            "string", "\"hi\"",
        ],
    );
    // Com decorators a classe não pode sair do invólucro do módulo.
    let StmtData::SClass(s) = store[stmts][0].data else { unreachable!() };
    assert!(!store[s].class.can_be_moved(&store));
}

/// O JSX automático que os dois casos de JSX produzem: `import { jsx } from "<fonte>/jsx-runtime";` e
/// `export default () => jsx("div", { className: "a", children: "hi" })`, aqui com a função de exportação padrão.
fn jsx_automatic(store: &mut Store, import_source: &[u8]) -> StoreSlice<Stmt> {
    let jsx_alias = store.alloc_str(b"jsx");
    let items = store.alloc_slice([ClauseItem {
        alias: jsx_alias,
        alias_loc: loc(0),
        name: LocRef { loc: loc(0), ref_: sym(7) },
        original_name: jsx_alias,
    }]);
    let path = [import_source, b"/jsx-runtime"].concat();
    let _ = store.alloc_str(&path);
    let import = Stmt::alloc(store, S::Import { namespace_ref: sym(8), items, import_record_index: 0, ..Default::default() }, loc(0));

    let target = Expr::init(store, E::ImportIdentifier::new(sym(7), true), loc(20));
    let tag = string(store, b"div");
    let props = store.alloc(E::Object::default());
    E::Object::put_string(store, props, b"className", b"a");
    E::Object::put_string(store, props, b"children", b"hi");
    let props = Expr { loc: loc(25), data: ExprData::EObject(props) };
    let args = store.alloc_slice([tag, props]);
    let call = Expr::init(store, E::Call { target, args, was_jsx_element: true, ..Default::default() }, loc(20));
    let ret = Stmt::alloc(store, S::Return { value: Some(call) }, loc(20));
    let body = store.alloc_slice([ret]);
    let mut func = G::Fn::default();
    func.body = G::FnBody { loc: loc(15), stmts: body };
    let function = Stmt::alloc(store, S::Function { func }, loc(15));
    let export = Stmt::alloc(
        store,
        S::ExportDefault { default_name: LocRef { loc: loc(15), ref_: sym(9) }, value: StmtOrExpr::Stmt(function) },
        loc(0),
    );
    store.alloc_slice([import, export])
}

const JSX_WALK: &[&str] = &[
    "s_import", "clause jsx", "s_export_default", "s_function", "s_return", "call", "import identifier", "string",
    "\"div\"", "object", "string", "\"className\"", "string", "\"a\"", "string", "\"children\"", "string", "\"hi\"",
];

/// `export default () => <div className="a">hi</div>` com `jsx: react-jsx`.
#[test]
fn jsx_automatic_case() {
    let mut store = Store::default();
    let stmts = jsx_automatic(&mut store, b"react");
    assert_walk(&store, stmts, JSX_WALK);
    // `export default function` pode ser movida; e vira expressão quando o printer pede.
    let StmtData::SExportDefault(d) = store[stmts][1].data else { unreachable!() };
    assert!(store[d].can_be_moved(&store));
    let as_expr = store[d].value.to_expr(&mut store);
    assert!(matches!(as_expr.data, ExprData::EFunction(_)));
}

/// O mesmo arquivo com `tsconfig.json` mandando `jsxImportSource: "preact"` e `verbatimModuleSyntax`: a árvore é
/// a mesma, só o caminho do registro de importação muda.
#[test]
fn tsconfig_case() {
    let mut store = Store::default();
    let stmts = jsx_automatic(&mut store, b"preact");
    assert_walk(&store, stmts, JSX_WALK);
}

/// A dobra de template e a corda passam pela mesma visita: "`a${'b'}c`" vira uma string só.
#[test]
fn template_fold_in_tree() {
    let mut store = Store::default();
    let b = string(&mut store, b"b");
    let head = E::String::init(store.alloc_str(b"a"));
    let tail = E::String::init(store.alloc_str(b"c"));
    let parts = store.alloc_slice([E::TemplatePart { value: b, tail_loc: loc(5), tail: E::TemplateContents::Cooked(tail) }]);
    let template = store.alloc(E::Template { tag: None, parts, head: E::TemplateContents::Cooked(head) });
    let folded = E::Template::fold(&mut store, template, loc(0));
    let stmt = local(&mut store, S::Kind::KLet, 1, folded, false);
    let stmts = store.alloc_slice([stmt]);
    assert_walk(&store, stmts, &["s_local", "b_identifier", "string", "\"abc\""]);
}
