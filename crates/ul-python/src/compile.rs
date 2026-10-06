//! Compilador do AST (`parser::parse_module`) para o bytecode próprio da VM (fatia 10 de
//! `docs/python3-port.md`).
//!
//! O bytecode é de pilha e não copia os opcodes do CPython (`dis` e `.pyc` estão fora do escopo),
//! mas a ordem de avaliação é a mesma: operandos da esquerda para a direita, valor antes do alvo na
//! atribuição, comparações encadeadas avaliando cada operando uma vez só. Cada instrução guarda a
//! linha do nó que a gerou, que é o `lineno` do traceback.
//!
//! Esta fatia cobre o nível de módulo: expressões aritméticas, lógicas e de comparação, nomes
//! globais, atribuição (simples, múltipla, desempacotamento e subscrição), atribuição aumentada,
//! `if`, `while` e `for` (com `else`, `break` e `continue`), `pass`, chamadas com argumentos
//! posicionais e nomeados, e os displays de `list`, `tuple`, `set` e `dict`. O que ainda não é
//! suportado (funções, classes, `import`, `try`, atributos, fatias, f-strings...) vira
//! `CompileError` do tipo `NotImplementedError`, que as próximas fatias vão eliminando.

use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use crate::ast::{
    Arguments, BoolOp, CmpOp, Comprehension, Constant, ExceptHandler, Expr, ExprContext, ExprKind as E, Keyword, Mod,
    Operator, Pos,
    Stmt, StmtKind as S, UnaryOp, WithItem,
};
use crate::object::Value;

/// Instrução da VM. Os operandos `u32` são índices em `consts`/`names` ou alvos de salto.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Op {
    LoadConst(u32),
    LoadName(u32),
    StoreName(u32),
    /// Descarta o topo.
    Pop,
    /// Duplica o topo.
    Dup,
    /// Duplica os dois do topo (`[a, b]` vira `[a, b, a, b]`).
    Dup2,
    /// Troca os dois do topo.
    Rot2,
    /// Leva o topo para a terceira posição (`[a, b, c]` vira `[c, a, b]`).
    Rot3,
    /// Operação binária; `inplace` é a forma da atribuição aumentada (`list += x` estende no lugar).
    Binary { op: Operator, inplace: bool },
    Unary(UnaryOp),
    Compare(CmpOp),
    Jump(u32),
    PopJumpIfFalse(u32),
    PopJumpIfTrue(u32),
    /// Salta mantendo o topo se ele for falso; senão o descarta (`and`).
    JumpIfFalseOrPop(u32),
    /// Salta mantendo o topo se ele for verdadeiro; senão o descarta (`or`).
    JumpIfTrueOrPop(u32),
    /// Troca o iterável do topo pelo iterador dele.
    GetIter,
    /// Empilha o próximo item do iterador do topo; esgotado, descarta o iterador e salta.
    ForIter(u32),
    /// Chamada: função, `argc` posicionais e, se `kwnames` aponta uma tupla de nomes em `consts`,
    /// os valores nomeados correspondentes por último.
    Call { argc: u32, kwnames: Option<u32> },
    BuildList(u32),
    BuildTuple(u32),
    BuildSet(u32),
    /// `n` pares chave e valor.
    BuildDict(u32),
    /// `container[index]`.
    Subscript,
    /// `container[index] = value`, com a pilha `[value, container, index]`.
    StoreSubscript,
    /// Desempacota o topo em `n` valores, o primeiro no topo.
    UnpackSequence(u32),
    /// Abre um bloco protegido; uma exceção salta para o alvo com a pilha restaurada.
    SetupTry(u32),
    /// Fecha o bloco protegido mais interno.
    PopBlock,
    /// Move a exceção do topo (mantendo-a) para a pilha de exceções tratadas.
    PushExc,
    /// Descarta a exceção tratada mais recente.
    PopExc,
    /// `[exc, cls]` vira `[exc, bool]`: a exceção é instância da classe (ou de alguma da tupla).
    ExcMatch,
    /// Levanta o topo da pilha.
    Raise,
    /// `raise X from Y`: pilha `[X, Y]`.
    RaiseFrom,
    /// `raise` sem argumento: levanta a exceção tratada mais recente.
    ReraiseCurrent,
    /// Relevanta a exceção do topo, descartando a tratada mais recente (fim de `except` sem casamento
    /// e de `finally`).
    Reraise,
    /// `del nome`, ignorando nome ausente.
    DeleteName(u32),
    /// `objeto.nome`.
    LoadAttr(u32),
    /// Variável local da função em execução (`UnboundLocalError` se ainda sem valor).
    LoadLocal(u32),
    /// `locals()` dentro de uma função: um dict com os locais ligados no momento.
    Locals,
    StoreLocal(u32),
    /// `[anotação]`: grava em `__annotations__[nome]` do módulo ou do corpo de classe.
    Annotate(u32),
    /// `from m import *`: desempilha o módulo e grava seus nomes públicos nas globais.
    ImportStar,
    /// Cria a função `functions[code]` com `ndefaults` valores padrão tirados da pilha e, se
    /// `kwdefaults` aponta uma tupla de nomes em `consts`, os padrões só-nomeados correspondentes
    /// (empilhados depois dos posicionais).
    MakeFunction { code: u32, ndefaults: u32, kwdefaults: Option<u32> },
    /// Depois de `MakeFunction`: desempilha um valor por nome da tupla em `consts` e grava o
    /// `__annotations__` da função que continua na pilha (`return` entra com esse nome).
    SetAnnotations(u32),
    /// Devolve o topo ao chamador.
    Return,
    /// `import nome`: empilha o módulo.
    Import(u32),
    /// `from .m import nome`: importa o módulo relativo (`level` pontos, `names[name]` pode ser vazio).
    ImportRel { name: u32, level: u32 },
    /// `from m import nome`: com o módulo no topo, empilha o atributo ou levanta `ImportError`.
    ImportName(u32),
    /// Nome declarado `global` dentro de uma função: sempre nas globais.
    LoadGlobal(u32),
    /// Nome declarado `nonlocal`: guarda no escopo de função externo mais próximo que o tem.
    StoreNonlocal(u32),
    /// Chamada com `*args` e `**kwargs`: `[func, lista]` ou, com `kwargs`, `[func, lista, dict]`.
    CallEx { kwargs: bool },
    /// `[lista, item]` vira `[lista]` com o item no fim.
    ListAppend,
    /// `[lista, iterável]` vira `[lista]` estendida.
    ListExtend,
    /// `[dict, chave, valor]` vira `[dict]` com o par.
    DictSet,
    /// `[dict, mapeamento]` vira `[dict]` atualizado (`**m`); chave repetida é `TypeError`.
    DictUpdate,
    /// `objeto.nome = valor`, com a pilha `[value, objeto]`.
    StoreAttr(u32),
    /// `del objeto.nome`.
    DeleteAttr(u32),
    /// `del container[index]`, com a pilha `[container, index]`.
    DeleteSubscript,
    /// `del x` em função: o nome precisa existir.
    DeleteLocal(u32),
    /// `del x` no módulo ou de nome `global`: o nome precisa existir.
    DeleteGlobal(u32),
    /// `[lo, hi, step]` vira um objeto `slice`.
    BuildSlice,
    /// Cria uma classe: `[nome-ignorado, bases...]` com `nbases`; executa o corpo `functions[code]`.
    BuildClass { code: u32, nbases: u32, kwnames: Option<u32> },
    /// Concatena `n` textos da pilha (f-strings).
    BuildString(u32),
    /// `[valor]` vira `[str]` por `format(valor, spec)`; com `has_spec` o topo é a especificação.
    /// `conv`: 0 nenhuma, 1 `!s`, 2 `!r`, 3 `!a`.
    FormatValue { conv: u8, has_spec: bool },
    /// Abre o `with`: `[mgr]` vira `[exit, valor]` (chama `__enter__`; o `__exit__` fica logo
    /// abaixo para o compilador guardar numa variável oculta).
    WithEnter,
    /// No tratador do `with`: `[exc, exit]` chama `exit(tipo, exc, None)` e vira `[exc, bool]` com o
    /// "suprimir" do `__exit__`.
    WithExcept,
    /// Desempacota com um alvo estrelado: `before` itens, o resto numa lista, `after` itens.
    UnpackEx { before: u32, after: u32 },
    /// `yield`: suspende a função geradora entregando o topo; ao retomar, empilha o valor enviado.
    Yield,
    /// `await` e `yield from`: `[iterador, enviado]`. Repassa o valor ao sub-iterador (`send`); se ele
    /// entrega algo, empilha o valor (o `Yield` seguinte o devolve); se termina, troca o iterador pelo
    /// valor de retorno e salta para o destino.
    Delegate(u32),
    /// Depois do `Yield` de uma delegação: volta ao `Delegate`. Marca o ponto onde `throw` é repassado.
    DelegateNext(u32),
    /// `await x`: o topo vira o iterador aguardável (`__await__`) ou a própria corrente.
    GetAwaitable,
    /// `async for`: o topo vira `type(x).__aiter__(x)`.
    GetAIter,
    /// `async for`: empilha `aiter.__anext__()` sem tirar o iterador assíncrono.
    GetANext,
    /// `async with`: `[mgr]` vira `[aexit, aenter()]` (o aguardável de `__aenter__`).
    AsyncWithEnter,
    /// `async with`, saída por exceção: `[exc, aexit]` vira `[exc, aexit(...)]` (o aguardável).
    AsyncWithExceptCall,
    /// `yield` num gerador assíncrono: entrega o valor embrulhado, distinto de um `await` suspenso.
    AsyncGenYield,
    /// Tratador do `async for`: `[aiter, exc]`; `StopAsyncIteration` encerra o laço (salta ao destino).
    AsyncForExcept(u32),
    /// Dentro de uma compreensão: `[acum, iteradores(d)..., item]`, acrescenta o item à lista.
    ListAppendAt(u32),
    /// Lista do topo vira tupla.
    ListToTuple,
    /// Lista do topo vira conjunto.
    ListToSet,
    /// Como `ListAppendAt`, para conjunto.
    SetAddAt(u32),
    /// Como `ListAppendAt`, com `[chave, valor]` no topo, para dicionário.
    MapAddAt(u32),
}

/// Corpo de uma função: instruções (`def`) ou uma expressão (`lambda`).
enum FnBody<'a> {
    Stmts(&'a [Stmt]),
    Expr(&'a Expr),
}

/// Intervalo de fonte de uma instrução (linhas de 1, colunas em bytes UTF-8, como o `co_positions` do
/// CPython). `lineno == 0`: posição desconhecida.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Span {
    pub lineno: u32,
    pub end_lineno: u32,
    pub col: u32,
    pub end_col: u32,
}

impl Span {
    pub fn of(pos: &crate::ast::Pos) -> Span {
        match (pos.end_lineno, pos.end_col_offset) {
            (Some(end_lineno), Some(end_col)) => Span {
                lineno: pos.lineno as u32,
                end_lineno: end_lineno as u32,
                col: pos.col_offset as u32,
                end_col: end_col as u32,
            },
            _ => Span::default(),
        }
    }
}

/// Código compilado de um módulo ou de uma função.
#[derive(Debug, Default)]
pub struct Code {
    pub ops: Vec<Op>,
    /// Linha de cada instrução, paralela a `ops`.
    pub lines: Vec<usize>,
    /// Intervalo de fonte de cada instrução, paralelo a `ops` (os carets do traceback).
    pub spans: Vec<Span>,
    pub consts: Vec<Value>,
    pub names: Vec<String>,
    /// Nome da função (`<module>` no nível de módulo, vazio por `Default`).
    pub name: String,
    /// Nome qualificado (`A.m`, `f.<locals>.g`); vazio quando é igual a `name`.
    pub qualname: String,
    /// Parâmetros posicionais (os `posonly` primeiros são só-posicionais).
    pub params: Vec<String>,
    pub posonly: usize,
    pub vararg: Option<String>,
    pub kwonly: Vec<String>,
    pub kwarg: Option<String>,
    pub is_function: bool,
    /// Corpo de classe: o resultado é o espaço de nomes, não um valor devolvido.
    pub is_class: bool,
    /// A função contém `yield`: chamá-la devolve um gerador.
    pub is_generator: bool,
    /// `async def`: chamar devolve uma corrente (coroutine), ou um gerador assíncrono se tiver `yield`.
    pub is_async: bool,
    pub functions: Vec<Rc<Code>>,
    /// Arquivo de origem; vazio para o script do usuário (módulos embutidos preenchem o deles).
    pub filename: String,
    /// Docstring (primeira instrução do corpo, se for um literal de texto).
    pub doc: Option<String>,
    /// Linha do `def` (`co_firstlineno`); 0 quando não se aplica.
    pub first_line: usize,
}

impl Code {
    /// `__qualname__` da função.
    pub fn qual(&self) -> &str {
        if self.qualname.is_empty() { &self.name } else { &self.qualname }
    }

    /// Grava `filename` neste código e em todas as funções aninhadas (só vale logo após compilar,
    /// quando cada `Rc` ainda tem um dono só).
    pub fn set_filename(&mut self, filename: &str) {
        self.filename = filename.to_string();
        for f in &mut self.functions {
            if let Some(inner) = Rc::get_mut(f) {
                inner.set_filename(filename);
            }
        }
    }
}

/// Erro de compilação: `SyntaxError` dos que o CPython detecta no compilador (`'break' outside
/// loop`) ou `NotImplementedError` para construções que esta fatia ainda não executa.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompileError {
    pub kind: &'static str,
    pub msg: String,
    pub lineno: usize,
}

/// Compila um `Mod::Module`.
/// `try/except*` reescrito como um `try/except` comum que reparte o grupo capturado entre os
/// tratadores com `_eg_split` e relança o que sobrou.
fn lower_try_star(body: &[Stmt], handlers: &[ExceptHandler], orelse: &[Stmt], finalbody: &[Stmt], pos: Pos) -> Stmt {
    let ex = |kind: E| Expr { kind, pos };
    let st = |kind: S| Stmt { kind, pos };
    let name = |id: &str, ctx: ExprContext| ex(E::Name { id: id.to_string(), ctx });
    let eg = format!(".eg{}_{}", pos.lineno, pos.col_offset);
    let rest = format!(".rest{}_{}", pos.lineno, pos.col_offset);
    let part = format!(".part{}_{}", pos.lineno, pos.col_offset);
    let mut inner = vec![st(S::Assign {
        targets: vec![name(&rest, ExprContext::Store)],
        value: Box::new(name(&eg, ExprContext::Load)),
        type_comment: None,
    })];
    for h in handlers {
        let kinds = h.r#type.clone().map(|t| *t).unwrap_or_else(|| name("BaseException", ExprContext::Load));
        inner.push(st(S::Assign {
            targets: vec![ex(E::Tuple {
                elts: vec![name(&part, ExprContext::Store), name(&rest, ExprContext::Store)],
                ctx: ExprContext::Store,
            })],
            value: Box::new(ex(E::Call {
                func: Box::new(name("_eg_split", ExprContext::Load)),
                args: vec![name(&rest, ExprContext::Load), kinds],
                keywords: Vec::new(),
            })),
            type_comment: None,
        }));
        let mut then = Vec::new();
        if let Some(n) = &h.name {
            then.push(st(S::Assign {
                targets: vec![name(n, ExprContext::Store)],
                value: Box::new(name(&part, ExprContext::Load)),
                type_comment: None,
            }));
        }
        then.extend(h.body.iter().cloned());
        inner.push(st(S::If {
            test: Box::new(ex(E::Compare {
                left: Box::new(name(&part, ExprContext::Load)),
                ops: vec![CmpOp::IsNot],
                comparators: vec![ex(E::Constant { value: Constant::None, kind: None })],
            })),
            body: then,
            orelse: Vec::new(),
        }));
    }
    inner.push(st(S::If {
        test: Box::new(ex(E::Compare {
            left: Box::new(name(&rest, ExprContext::Load)),
            ops: vec![CmpOp::IsNot],
            comparators: vec![ex(E::Constant { value: Constant::None, kind: None })],
        })),
        body: vec![st(S::Raise { exc: Some(Box::new(name(&rest, ExprContext::Load))), cause: None })],
        orelse: Vec::new(),
    }));
    let handler = ExceptHandler {
        r#type: Some(Box::new(name("BaseException", ExprContext::Load))),
        name: Some(eg),
        body: inner,
        pos,
    };
    st(S::Try { body: body.to_vec(), handlers: vec![handler], orelse: orelse.to_vec(), finalbody: finalbody.to_vec() })
}

/// Como o compilador do CPython 3.13: expande tabs e tira a indentação comum das linhas após a primeira
/// (linhas vazias no começo e no fim ficam, ao contrário de `inspect.cleandoc`).
fn clean_doc(doc: &str) -> String {
    if !doc.contains('\n') && !doc.contains('\t') {
        return doc.to_string();
    }
    let mut expanded = String::new();
    let mut col = 0;
    for ch in doc.chars() {
        match ch {
            '\t' => {
                let n = 8 - col % 8;
                expanded.extend(std::iter::repeat(' ').take(n));
                col += n;
            }
            '\n' => {
                expanded.push('\n');
                col = 0;
            }
            _ => {
                expanded.push(ch);
                col += 1;
            }
        }
    }
    let lines: Vec<&str> = expanded.split('\n').collect();
    let margin = lines
        .iter()
        .skip(1)
        .filter_map(|l| {
            let content = l.trim_start_matches(' ').len();
            (content > 0).then(|| l.len() - content)
        })
        .min();
    let mut out: Vec<String> = Vec::with_capacity(lines.len());
    for (i, l) in lines.iter().enumerate() {
        if i == 0 {
            out.push(l.trim_start_matches(' ').to_string());
        } else {
            let cut = margin.unwrap_or(0).min(l.len());
            out.push(l[cut..].to_string());
        }
    }
    out.join("\n")
}

/// O docstring de um corpo: a primeira instrução, se for só um literal de texto.
fn docstring(body: &[Stmt]) -> Option<String> {
    match body.first().map(|s| &s.kind) {
        Some(S::Expr { value }) => match &value.kind {
            E::Constant { value: Constant::Str(s), .. } => Some(clean_doc(s)),
            _ => None,
        },
        _ => None,
    }
}

/// Texto de uma anotação, para `from __future__ import annotations` (o que `ast.unparse` daria).
fn ann_text(e: &Expr) -> String {
    match &e.kind {
        E::Name { id, .. } => id.clone(),
        E::Attribute { value, attr, .. } => format!("{}.{attr}", ann_text(value)),
        E::Subscript { value, slice, .. } => {
            let inner = match &slice.kind {
                E::Tuple { elts, .. } if !elts.is_empty() => elts.iter().map(ann_text).collect::<Vec<_>>().join(", "),
                _ => ann_text(slice),
            };
            format!("{}[{inner}]", ann_text(value))
        }
        E::Tuple { elts, .. } => match elts.as_slice() {
            [one] => format!("({},)", ann_text(one)),
            _ => format!("({})", elts.iter().map(ann_text).collect::<Vec<_>>().join(", ")),
        },
        E::List { elts, .. } => format!("[{}]", elts.iter().map(ann_text).collect::<Vec<_>>().join(", ")),
        E::BinOp { left, op: Operator::BitOr, right } => format!("{} | {}", ann_text(left), ann_text(right)),
        E::Constant { value, .. } => match value {
            Constant::None => "None".to_string(),
            Constant::Bool(b) => if *b { "True" } else { "False" }.to_string(),
            Constant::Int(i) => i.clone(),
            Constant::Str(s) => crate::object::repr(&Value::str(s.clone())),
            Constant::Ellipsis => "...".to_string(),
            _ => "...".to_string(),
        },
        _ => "...".to_string(),
    }
}

pub fn compile_module(module: &Mod) -> Result<Code, CompileError> {
    let Mod::Module { body, .. } = module else {
        return Err(CompileError { kind: "NotImplementedError", msg: "only modules can be compiled".into(), lineno: 1 });
    };
    let mut c = Compiler::new(Code { name: "<module>".into(), ..Code::default() }, 1);
    c.future_annotations = body.iter().any(|s| {
        matches!(&s.kind, S::ImportFrom { module: Some(m), names, .. }
            if m == "__future__" && names.iter().any(|n| n.name == "annotations"))
    });
    // Sem docstring, `__doc__` fica como o chamador definiu (`None` ao criar o módulo).
    if let Some(doc) = docstring(body) {
        let k = c.constant(Value::str(doc));
        c.emit(Op::LoadConst(k));
        c.emit_store("__doc__");
    }
    c.block(body)?;
    Ok(c.code)
}

/// Nomes de um escopo de função: os ligados no corpo (alvos de atribuição, `for`, `with`, `except
/// as`, `import`, `def`, `class`, `del`, walrus) e os declarados `global` e `nonlocal`.
#[derive(Default)]
struct Scope {
    bound: HashSet<String>,
    globals: HashSet<String>,
    nonlocals: HashSet<String>,
    /// Há um `await` no que o escopo percorreu (uma compreensão assim é assíncrona).
    has_await: bool,
}

impl Scope {
    fn target(&mut self, e: &Expr) {
        match &e.kind {
            E::Name { id, .. } => {
                self.bound.insert(id.clone());
            }
            E::Tuple { elts, .. } | E::List { elts, .. } => elts.iter().for_each(|x| self.target(x)),
            E::Starred { value, .. } => self.target(value),
            _ => {}
        }
    }

    /// Procura `x := v` em uma expressão (o alvo liga no escopo da função que a contém). Não entra
    /// em `lambda`; entra nas compreensões, cujo walrus também liga no escopo externo.
    fn expr(&mut self, e: &Expr) {
        match &e.kind {
            E::NamedExpr { target, value } => {
                self.target(target);
                self.expr(value);
            }
            E::BinOp { left, right, .. } => {
                self.expr(left);
                self.expr(right);
            }
            E::UnaryOp { operand, .. } => self.expr(operand),
            E::BoolOp { values, .. } => values.iter().for_each(|v| self.expr(v)),
            E::IfExp { test, body, orelse } => {
                self.expr(test);
                self.expr(body);
                self.expr(orelse);
            }
            E::Compare { left, comparators, .. } => {
                self.expr(left);
                comparators.iter().for_each(|v| self.expr(v));
            }
            E::Call { func, args, keywords } => {
                self.expr(func);
                args.iter().for_each(|v| self.expr(v));
                keywords.iter().for_each(|k| self.expr(&k.value));
            }
            E::List { elts, .. } | E::Tuple { elts, .. } | E::Set { elts } => elts.iter().for_each(|v| self.expr(v)),
            E::Dict { keys, values } => {
                keys.iter().flatten().for_each(|v| self.expr(v));
                values.iter().for_each(|v| self.expr(v));
            }
            E::Subscript { value, slice, .. } => {
                self.expr(value);
                self.expr(slice);
            }
            E::Attribute { value, .. } | E::Starred { value, .. } => self.expr(value),
            E::Slice { lower, upper, step } => {
                for x in [lower, upper, step].into_iter().flatten() {
                    self.expr(x);
                }
            }
            E::ListComp { elt, generators } | E::SetComp { elt, generators } | E::GeneratorExp { elt, generators } => {
                self.expr(elt);
                for g in generators {
                    self.expr(&g.iter);
                    g.ifs.iter().for_each(|v| self.expr(v));
                }
            }
            E::DictComp { key, value, generators } => {
                self.expr(key);
                self.expr(value);
                for g in generators {
                    self.expr(&g.iter);
                    g.ifs.iter().for_each(|v| self.expr(v));
                }
            }
            E::JoinedStr { values } => values.iter().for_each(|v| self.expr(v)),
            E::FormattedValue { value, format_spec, .. } => {
                self.expr(value);
                if let Some(f) = format_spec {
                    self.expr(f);
                }
            }
            E::Yield { value: Some(v) } => self.expr(v),
            E::YieldFrom { value } => self.expr(value),
            E::Await { value } => {
                self.has_await = true;
                self.expr(value)
            }
            _ => {}
        }
    }

    fn block(&mut self, body: &[Stmt]) {
        for s in body {
            self.stmt(s);
        }
    }

    fn stmt(&mut self, s: &Stmt) {
        match &s.kind {
            S::Expr { value } => self.expr(value),
            S::Assign { targets, value, .. } => {
                targets.iter().for_each(|t| self.target(t));
                self.expr(value);
            }
            S::AugAssign { target, value, .. } => {
                self.target(target);
                self.expr(value);
            }
            S::AnnAssign { target, value, .. } => {
                self.target(target);
                if let Some(v) = value {
                    self.expr(v);
                }
            }
            S::For { target, iter, body, orelse, .. } | S::AsyncFor { target, iter, body, orelse, .. } => {
                self.target(target);
                self.expr(iter);
                self.block(body);
                self.block(orelse);
            }
            S::While { test, body, orelse } | S::If { test, body, orelse } => {
                self.expr(test);
                self.block(body);
                self.block(orelse);
            }
            S::With { items, body, .. } | S::AsyncWith { items, body, .. } => {
                for it in items {
                    self.expr(&it.context_expr);
                    if let Some(v) = &it.optional_vars {
                        self.target(v);
                    }
                }
                self.block(body);
            }
            S::TryStar { body, handlers, orelse, finalbody } => {
                let lowered = lower_try_star(body, handlers, orelse, finalbody, s.pos);
                self.stmt(&lowered);
            }
            S::Try { body, handlers, orelse, finalbody } => {
                self.block(body);
                for h in handlers {
                    if let Some(n) = &h.name {
                        self.bound.insert(n.clone());
                    }
                    self.block(&h.body);
                }
                self.block(orelse);
                self.block(finalbody);
            }
            S::Return { value: Some(v) } => self.expr(v),
            S::Raise { exc, cause } => {
                for x in [exc, cause].into_iter().flatten() {
                    self.expr(x);
                }
            }
            S::Assert { test, msg } => {
                self.expr(test);
                if let Some(m) = msg {
                    self.expr(m);
                }
            }
            S::Delete { targets } => targets.iter().for_each(|t| self.target(t)),
            S::FunctionDef { name, .. } | S::AsyncFunctionDef { name, .. } | S::ClassDef { name, .. } => {
                self.bound.insert(name.clone());
            }
            S::Import { names } => {
                for a in names {
                    let n = a.asname.clone().unwrap_or_else(|| a.name.split('.').next().unwrap_or("").to_string());
                    self.bound.insert(n);
                }
            }
            S::ImportFrom { names, .. } => {
                for a in names {
                    if a.name != "*" {
                        self.bound.insert(a.asname.clone().unwrap_or_else(|| a.name.clone()));
                    }
                }
            }
            S::Global { names } => self.globals.extend(names.iter().cloned()),
            S::Nonlocal { names } => self.nonlocals.extend(names.iter().cloned()),
            S::Match { subject, cases } => {
                self.expr(subject);
                for c in cases {
                    self.pattern(&c.pattern);
                    if let Some(g) = &c.guard {
                        self.expr(g);
                    }
                    self.block(&c.body);
                }
            }
            _ => {}
        }
    }

    /// Os nomes que um padrão de `match` captura.
    fn pattern(&mut self, p: &crate::ast::Pattern) {
        use crate::ast::PatternKind as P;
        match &p.kind {
            P::MatchSequence { patterns } | P::MatchOr { patterns } => patterns.iter().for_each(|x| self.pattern(x)),
            P::MatchMapping { patterns, rest, .. } => {
                patterns.iter().for_each(|x| self.pattern(x));
                if let Some(r) = rest {
                    self.bound.insert(r.clone());
                }
            }
            P::MatchClass { patterns, kwd_patterns, .. } => {
                patterns.iter().chain(kwd_patterns.iter()).for_each(|x| self.pattern(x));
            }
            P::MatchStar { name } => {
                if let Some(n) = name {
                    self.bound.insert(n.clone());
                }
            }
            P::MatchAs { pattern, name } => {
                if let Some(x) = pattern {
                    self.pattern(x);
                }
                if let Some(n) = name {
                    self.bound.insert(n.clone());
                }
            }
            P::MatchValue { .. } | P::MatchSingleton { .. } => {}
        }
    }

    /// As variáveis locais: o que o corpo liga, menos o declarado `global` ou `nonlocal`.
    fn locals(mut self) -> (HashSet<String>, HashSet<String>, HashSet<String>) {
        for g in self.globals.iter().chain(self.nonlocals.iter()) {
            self.bound.remove(g);
        }
        (self.bound, self.globals, self.nonlocals)
    }
}

struct LoopCtx {
    continue_target: usize,
    breaks: Vec<usize>,
    /// Laço `for`: o `break` descarta o iterador da pilha antes de sair.
    is_for: bool,
    /// Quantos `try` estavam abertos quando o laço começou.
    try_depth: usize,
}

/// `try` aberto durante a compilação: o `finally`, se houver, é repetido onde o controle sai. Num
/// `with`, `with_exit` é a variável oculta que guarda o `__exit__`, chamado no lugar do `finally`.
#[derive(Clone)]
struct TryCtx {
    finalbody: Vec<Stmt>,
    with_exit: Option<String>,
    /// `async with`: a saída é um aguardável, que `break`/`return` precisam aguardar.
    with_async: bool,
}

struct Compiler {
    code: Code,
    line: usize,
    /// Intervalo de fonte do nó que está sendo compilado (vira o `span` das instruções emitidas).
    span: Span,
    loops: Vec<LoopCtx>,
    name_index: HashMap<String, u32>,
    tries: Vec<TryCtx>,
    /// Variáveis locais da função sendo compilada (`None` no módulo).
    locals: Option<HashSet<String>>,
    /// Nomes declarados `global` na função sendo compilada.
    globals_decl: HashSet<String>,
    /// Nomes declarados `nonlocal` na função sendo compilada.
    nonlocals_decl: HashSet<String>,
    /// Contador para nomear as variáveis ocultas do `with`.
    hidden: usize,
    in_class_body: bool,
    /// Nome da classe cujo corpo este compilador compila.
    class_name: Option<String>,
    /// Prefixo do `__qualname__` dos filhos (`A.` num corpo de classe, `f.<locals>.` numa função).
    qual_prefix: String,
    /// Classe em cujo corpo a função sendo compilada foi definida (para o `super()` sem argumentos).
    enclosing_class: Option<String>,
    /// `from __future__ import annotations`: anotações viram texto em vez de serem avaliadas.
    future_annotations: bool,
}

impl Compiler {
    fn new(code: Code, line: usize) -> Compiler {
        Compiler {
            code,
            line,
            span: Span::default(),
            loops: Vec::new(),
            name_index: HashMap::new(),
            tries: Vec::new(),
            locals: None,
            globals_decl: HashSet::new(),
            nonlocals_decl: HashSet::new(),
            hidden: 0,
            in_class_body: false,
            class_name: None,
            qual_prefix: String::new(),
            enclosing_class: None,
            future_annotations: false,
        }
    }

    fn unsupported(&self, what: &str) -> CompileError {
        CompileError {
            kind: "NotImplementedError",
            msg: format!("{what} is not supported yet by this interpreter"),
            lineno: self.line,
        }
    }

    fn emit(&mut self, op: Op) -> usize {
        self.code.ops.push(op);
        self.code.lines.push(self.line);
        self.code.spans.push(self.span);
        self.code.ops.len() - 1
    }

    /// Passa a atribuir as instruções ao nó em `pos`.
    fn at(&mut self, pos: &crate::ast::Pos) {
        self.line = pos.lineno;
        self.span = Span::of(pos);
    }

    fn here(&self) -> usize {
        self.code.ops.len()
    }

    /// Aponta o salto em `at` para `target`.
    fn patch(&mut self, at: usize, target: usize) {
        let t = target as u32;
        self.code.ops[at] = match self.code.ops[at] {
            Op::Jump(_) => Op::Jump(t),
            Op::PopJumpIfFalse(_) => Op::PopJumpIfFalse(t),
            Op::PopJumpIfTrue(_) => Op::PopJumpIfTrue(t),
            Op::JumpIfFalseOrPop(_) => Op::JumpIfFalseOrPop(t),
            Op::JumpIfTrueOrPop(_) => Op::JumpIfTrueOrPop(t),
            Op::ForIter(_) => Op::ForIter(t),
            Op::SetupTry(_) => Op::SetupTry(t),
            Op::Delegate(_) => Op::Delegate(t),
            Op::AsyncForExcept(_) => Op::AsyncForExcept(t),
            other => other,
        };
    }

    fn constant(&mut self, value: Value) -> u32 {
        self.code.consts.push(value);
        (self.code.consts.len() - 1) as u32
    }

    fn name(&mut self, name: &str) -> u32 {
        if let Some(&i) = self.name_index.get(name) {
            return i;
        }
        let i = self.code.names.len() as u32;
        self.code.names.push(name.to_string());
        self.name_index.insert(name.to_string(), i);
        i
    }

    fn block(&mut self, body: &[Stmt]) -> Result<(), CompileError> {
        body.iter().try_for_each(|s| self.stmt(s))
    }

    fn stmt(&mut self, stmt: &Stmt) -> Result<(), CompileError> {
        self.at(&stmt.pos);
        if let Some(g) = crate::pep695::desugar(stmt) {
            // PEP 695: o escopo sintético roda na hora e o resultado entra no nome original.
            self.make_function(&g.scope_name, &crate::pep695::no_args(), FnBody::Stmts(&g.body), stmt.pos.lineno, false, None)?;
            self.emit(Op::Call { argc: 0, kwnames: None });
            self.emit_store(&g.target);
            return Ok(());
        }
        match &stmt.kind {
            S::Expr { value } => {
                self.expr(value)?;
                self.emit(Op::Pop);
            }
            S::Assign { targets, value, .. } => {
                self.expr(value)?;
                for (i, target) in targets.iter().enumerate() {
                    if i + 1 < targets.len() {
                        self.emit(Op::Dup);
                    }
                    self.store(target)?;
                }
            }
            S::AugAssign { target, op, value } => self.aug_assign(target, *op, value)?,
            S::Pass => {}
            // No nível de módulo `global x` não muda nada.
            S::Global { .. } => {}
            S::If { test, body, orelse } => {
                self.expr(test)?;
                let to_else = self.emit(Op::PopJumpIfFalse(0));
                self.block(body)?;
                if orelse.is_empty() {
                    let end = self.here();
                    self.patch(to_else, end);
                } else {
                    let to_end = self.emit(Op::Jump(0));
                    let else_start = self.here();
                    self.patch(to_else, else_start);
                    self.block(orelse)?;
                    let end = self.here();
                    self.patch(to_end, end);
                }
            }
            S::While { test, body, orelse } => {
                let top = self.here();
                self.at(&stmt.pos);
                self.expr(test)?;
                let to_else = self.emit(Op::PopJumpIfFalse(0));
                self.loops.push(LoopCtx { continue_target: top, breaks: Vec::new(), is_for: false, try_depth: self.tries.len() });
                self.block(body)?;
                self.at(&stmt.pos);
                self.emit(Op::Jump(top as u32));
                let ctx = self.loops.pop().unwrap_or(LoopCtx { continue_target: top, breaks: Vec::new(), is_for: false, try_depth: 0 });
                let else_start = self.here();
                self.patch(to_else, else_start);
                self.block(orelse)?;
                let end = self.here();
                for b in ctx.breaks {
                    self.patch(b, end);
                }
            }
            S::For { target, iter, body, orelse, .. } => {
                self.expr(iter)?;
                self.at(&stmt.pos);
                self.emit(Op::GetIter);
                let top = self.emit(Op::ForIter(0));
                self.store(target)?;
                self.loops.push(LoopCtx { continue_target: top, breaks: Vec::new(), is_for: true, try_depth: self.tries.len() });
                self.block(body)?;
                self.at(&stmt.pos);
                self.emit(Op::Jump(top as u32));
                let ctx = self.loops.pop().unwrap_or(LoopCtx { continue_target: top, breaks: Vec::new(), is_for: true, try_depth: 0 });
                let else_start = self.here();
                self.patch(top, else_start);
                self.block(orelse)?;
                let end = self.here();
                for b in ctx.breaks {
                    self.patch(b, end);
                }
            }
            S::Break => {
                let Some(ctx) = self.loops.last() else {
                    return Err(CompileError { kind: "SyntaxError", msg: "'break' outside loop".into(), lineno: self.line });
                };
                let (is_for, depth) = (ctx.is_for, ctx.try_depth);
                self.leave_tries(depth)?;
                if is_for {
                    self.emit(Op::Pop);
                }
                let at = self.emit(Op::Jump(0));
                if let Some(ctx) = self.loops.last_mut() {
                    ctx.breaks.push(at);
                }
            }
            S::Continue => {
                let Some(ctx) = self.loops.last() else {
                    return Err(CompileError {
                        kind: "SyntaxError",
                        msg: "'continue' not properly in loop".into(),
                        lineno: self.line,
                    });
                };
                let (target, depth) = (ctx.continue_target as u32, ctx.try_depth);
                self.leave_tries(depth)?;
                self.emit(Op::Jump(target));
            }
            S::Try { body, handlers, orelse, finalbody } => self.try_stmt(body, handlers, orelse, finalbody)?,
            S::Raise { exc, cause } => {
                match exc {
                    Some(e) => {
                        self.expr(e)?;
                        // `raise X from Y`: `Y` vira `__cause__` de `X`.
                        if let Some(c) = cause {
                            self.expr(c)?;
                            self.at(&stmt.pos);
                            self.emit(Op::RaiseFrom);
                        } else {
                            self.at(&stmt.pos);
                            self.emit(Op::Raise);
                        }
                    }
                    None => {
                        self.emit(Op::ReraiseCurrent);
                    }
                }
            }
            // `python -O` descarta os `assert`
            S::Assert { .. } if crate::OPTIMIZE.load(std::sync::atomic::Ordering::Relaxed) > 0 => {}
            S::Assert { test, msg } => {
                self.expr(test)?;
                let ok = self.emit(Op::PopJumpIfTrue(0));
                self.at(&test.pos);
                self.emit_load("AssertionError");
                let argc = if let Some(m) = msg {
                    self.expr(m)?;
                    1
                } else {
                    0
                };
                self.at(&test.pos);
                self.emit(Op::Call { argc, kwnames: None });
                self.emit(Op::Raise);
                let end = self.here();
                self.patch(ok, end);
            }
            S::FunctionDef { name, args, body, decorator_list, returns, .. }
            | S::AsyncFunctionDef { name, args, body, decorator_list, returns, .. } => {
                let is_async = matches!(stmt.kind, S::AsyncFunctionDef { .. });
                for d in decorator_list {
                    self.expr(d)?;
                }
                self.make_function(name, args, FnBody::Stmts(body), stmt.pos.lineno, is_async, returns.as_deref())?;
                for _ in decorator_list {
                    self.at(&stmt.pos);
                    self.emit(Op::Call { argc: 1, kwnames: None });
                }
                self.emit_store(name);
            }
            S::Return { value } => {
                if !self.code.is_function {
                    return Err(CompileError {
                        kind: "SyntaxError",
                        msg: "'return' outside function".into(),
                        lineno: self.line,
                    });
                }
                match value {
                    Some(v) => self.expr(v)?,
                    None => {
                        let c = self.constant_none();
                        self.emit(Op::LoadConst(c));
                    }
                }
                self.at(&stmt.pos);
                self.emit_return()?;
            }
            S::ClassDef { name, bases, keywords, body, decorator_list, .. } => {
                if keywords.iter().any(|k| k.arg.is_none()) {
                    return Err(self.unsupported("class keywords with **"));
                }
                if bases.iter().any(|b| matches!(b.kind, E::Starred { .. })) {
                    return Err(self.unsupported("starred class bases"));
                }
                for d in decorator_list {
                    self.expr(d)?;
                }
                self.class_def(name, bases, keywords, body, stmt.pos.lineno)?;
                for _ in decorator_list {
                    self.at(&stmt.pos);
                    self.emit(Op::Call { argc: 1, kwnames: None });
                }
                self.emit_store(name);
            }
            S::Import { names } => {
                for alias in names {
                    // `import a.b.c` liga `a` (depois de carregar `a.b.c`); com `as d`, liga `d` ao
                    // submódulo `a.b.c`.
                    let n = self.name(&alias.name);
                    self.emit(Op::Import(n));
                    let bound = match &alias.asname {
                        Some(a) => a.clone(),
                        None if alias.name.contains('.') => {
                            self.emit(Op::Pop);
                            let top = alias.name.split('.').next().unwrap_or("").to_string();
                            let t = self.name(&top);
                            self.emit(Op::Import(t));
                            top
                        }
                        None => alias.name.clone(),
                    };
                    self.emit_store(&bound);
                }
            }
            S::ImportFrom { module, names, level } => {
                let level = level.unwrap_or(0);
                if level == 0 && module.is_none() {
                    return Err(self.unsupported("import without a module name"));
                }
                let module = module.clone().unwrap_or_default();
                for alias in names {
                    let m = self.name(&module);
                    self.emit(if level == 0 { Op::Import(m) } else { Op::ImportRel { name: m, level: level as u32 } });
                    if alias.name == "*" {
                        self.emit(Op::ImportStar);
                        continue;
                    }
                    let n = self.name(&alias.name);
                    self.emit(Op::ImportName(n));
                    let bound = alias.asname.clone().unwrap_or_else(|| alias.name.clone());
                    self.emit_store(&bound);
                }
            }
            S::TryStar { body, handlers, orelse, finalbody } => {
                let lowered = lower_try_star(body, handlers, orelse, finalbody, stmt.pos);
                self.stmt(&lowered)?;
            }
            S::Delete { targets } => {
                for t in targets {
                    self.delete(t)?;
                }
            }
            S::With { items, body, .. } => self.with_stmt(items, body, false)?,
            S::AsyncWith { items, body, .. } => {
                if !self.code.is_async {
                    return Err(CompileError {
                        kind: "SyntaxError",
                        msg: "'async with' outside async function".into(),
                        lineno: stmt.pos.lineno,
                    });
                }
                self.with_stmt(items, body, true)?
            }
            S::Match { subject, cases } => self.match_stmt(subject, cases)?,
            S::AnnAssign { target, annotation, value, .. } => {
                if let Some(v) = value {
                    self.expr(v)?;
                    self.store(target)?;
                }
                // Anotações de nome simples no módulo e em corpo de classe viram `__annotations__`.
                if self.locals.is_none() || self.in_class_body {
                    if let E::Name { id, .. } = &target.kind {
                        self.emit_annotation(annotation)?;
                        let n = self.name(id);
                        self.emit(Op::Annotate(n));
                    }
                }
            }
            S::TypeAlias { .. } => {}
            S::Nonlocal { .. } => {}
            S::AsyncFor { target, iter, body, orelse, .. } => {
                if !self.code.is_async {
                    return Err(CompileError {
                        kind: "SyntaxError",
                        msg: "'async for' outside async function".into(),
                        lineno: stmt.pos.lineno,
                    });
                }
                self.expr(iter)?;
                self.at(&stmt.pos);
                self.emit(Op::GetAIter);
                let top = self.emit(Op::SetupTry(0));
                self.emit(Op::GetANext);
                self.emit(Op::GetAwaitable);
                self.await_delegate();
                self.emit(Op::PopBlock);
                self.store(target)?;
                self.loops.push(LoopCtx { continue_target: top, breaks: Vec::new(), is_for: true, try_depth: self.tries.len() });
                self.block(body)?;
                self.at(&stmt.pos);
                self.emit(Op::Jump(top as u32));
                let ctx = self.loops.pop().unwrap_or(LoopCtx { continue_target: top, breaks: Vec::new(), is_for: true, try_depth: 0 });
                let handler = self.here();
                self.patch(top, handler);
                let stop = self.emit(Op::AsyncForExcept(0));
                let else_start = self.here();
                self.patch(stop, else_start);
                self.block(orelse)?;
                let end = self.here();
                for b in ctx.breaks {
                    self.patch(b, end);
                }
            }
        }
        Ok(())
    }

    /// `try` completo: o `finally` embrulha o `try/except/else` por fora, e o corpo dele é repetido
    /// no caminho normal, no excepcional e em cada `break`/`continue` que atravessa o `try`.
    fn try_stmt(
        &mut self,
        body: &[Stmt],
        handlers: &[ExceptHandler],
        orelse: &[Stmt],
        finalbody: &[Stmt],
    ) -> Result<(), CompileError> {
        if finalbody.is_empty() {
            return self.try_except(body, handlers, orelse);
        }
        let setup = self.emit(Op::SetupTry(0));
        self.tries.push(TryCtx { finalbody: finalbody.to_vec(), with_exit: None, with_async: false });
        if handlers.is_empty() {
            self.block(body)?;
            self.block(orelse)?;
        } else {
            self.try_except(body, handlers, orelse)?;
        }
        self.tries.pop();
        self.emit(Op::PopBlock);
        self.block(finalbody)?;
        let to_end = self.emit(Op::Jump(0));
        let handler = self.here();
        self.patch(setup, handler);
        self.emit(Op::PushExc);
        self.block(finalbody)?;
        self.emit(Op::Reraise);
        let end = self.here();
        self.patch(to_end, end);
        Ok(())
    }

    fn try_except(&mut self, body: &[Stmt], handlers: &[ExceptHandler], orelse: &[Stmt]) -> Result<(), CompileError> {
        let setup = self.emit(Op::SetupTry(0));
        self.tries.push(TryCtx { finalbody: Vec::new(), with_exit: None, with_async: false });
        self.block(body)?;
        self.tries.pop();
        self.emit(Op::PopBlock);
        self.block(orelse)?;
        let mut ends = vec![self.emit(Op::Jump(0))];
        let handler = self.here();
        self.patch(setup, handler);
        self.emit(Op::PushExc);
        let mut pending: Option<usize> = None;
        for h in handlers {
            if let Some(at) = pending.take() {
                let here = self.here();
                self.patch(at, here);
            }
            self.at(&h.pos);
            if let Some(t) = &h.r#type {
                self.expr(t)?;
                self.at(&h.pos);
                self.emit(Op::ExcMatch);
                pending = Some(self.emit(Op::PopJumpIfFalse(0)));
            }
            match &h.name {
                Some(name) => {
                    self.emit_store(name);
                }
                None => {
                    self.emit(Op::Pop);
                }
            }
            self.block(&h.body)?;
            if let Some(name) = &h.name {
                let n = self.name(name);
                self.emit(Op::DeleteName(n));
            }
            self.emit(Op::PopExc);
            ends.push(self.emit(Op::Jump(0)));
        }
        if let Some(at) = pending.take() {
            let here = self.here();
            self.patch(at, here);
            self.emit(Op::Reraise);
        }
        let end = self.here();
        for e in ends {
            self.patch(e, end);
        }
        Ok(())
    }

    /// Sai dos `try` abertos acima de `depth` (por `break`/`continue`): fecha o bloco de cada um e
    /// repete o `finally` dele.
    /// Com o aguardável no topo, suspende até ele terminar e deixa o resultado dele no topo.
    fn await_delegate(&mut self) {
        let none = self.constant_none();
        self.emit(Op::LoadConst(none));
        let delegate = self.emit(Op::Delegate(0));
        self.emit(Op::Yield);
        self.emit(Op::DelegateNext(delegate as u32));
        let end = self.here();
        self.patch(delegate, end);
    }

    fn leave_tries(&mut self, depth: usize) -> Result<(), CompileError> {
        let all = std::mem::take(&mut self.tries);
        let mut result = Ok(());
        for i in (depth..all.len()).rev() {
            self.emit(Op::PopBlock);
            if let Some(name) = all[i].with_exit.clone() {
                self.emit_load(&name);
                let none = self.constant_none();
                for _ in 0..3 {
                    self.emit(Op::LoadConst(none));
                }
                self.emit(Op::Call { argc: 3, kwnames: None });
                if all[i].with_async {
                    self.emit(Op::GetAwaitable);
                    self.await_delegate();
                }
                self.emit(Op::Pop);
                continue;
            }
            if all[i].finalbody.is_empty() {
                continue;
            }
            self.tries = all[..i].to_vec();
            if let Err(e) = self.block(&all[i].finalbody) {
                result = Err(e);
                break;
            }
        }
        self.tries = all;
        result
    }

    fn emit_load(&mut self, id: &str) {
        let n = self.name(id);
        let local = self.locals.as_ref().is_some_and(|l| l.contains(id));
        self.emit(if local {
            Op::LoadLocal(n)
        } else if self.globals_decl.contains(id) {
            Op::LoadGlobal(n)
        } else {
            Op::LoadName(n)
        });
    }

    /// Empilha o valor de uma anotação: avaliada, ou o texto dela com `from __future__ import annotations`.
    fn emit_annotation(&mut self, e: &Expr) -> Result<(), CompileError> {
        if self.future_annotations {
            let k = self.constant(Value::str(ann_text(e)));
            self.emit(Op::LoadConst(k));
            Ok(())
        } else {
            self.expr(e)
        }
    }

    fn emit_store(&mut self, id: &str) {
        let n = self.name(id);
        let local = self.locals.as_ref().is_some_and(|l| l.contains(id));
        self.emit(if local {
            Op::StoreLocal(n)
        } else if self.nonlocals_decl.contains(id) {
            Op::StoreNonlocal(n)
        } else {
            Op::StoreName(n)
        });
    }

    /// Compila uma função (`def` ou `lambda`) num `Code` próprio e emite a criação dela: os padrões
    /// são avaliados aqui, no escopo de fora. Deixa a função na pilha; guardar é com o chamador.
    fn make_function(
        &mut self,
        name: &str,
        args: &Arguments,
        body: FnBody,
        line: usize,
        is_async: bool,
        returns: Option<&Expr>,
    ) -> Result<(), CompileError> {
        let mut params: Vec<String> = args.posonlyargs.iter().map(|a| a.arg.clone()).collect();
        let posonly = params.len();
        params.extend(args.args.iter().map(|a| a.arg.clone()));
        let kwonly: Vec<String> = args.kwonlyargs.iter().map(|a| a.arg.clone()).collect();
        let vararg = args.vararg.as_ref().map(|a| a.arg.clone());
        let kwarg = args.kwarg.as_ref().map(|a| a.arg.clone());
        let mut scope = Scope::default();
        scope.bound.extend(params.iter().cloned());
        scope.bound.extend(kwonly.iter().cloned());
        scope.bound.extend(vararg.iter().cloned());
        scope.bound.extend(kwarg.iter().cloned());
        match &body {
            FnBody::Stmts(b) => scope.block(b),
            FnBody::Expr(e) => scope.expr(e),
        }
        let (locals, globals, nonlocals) = scope.locals();
        let qualname = format!("{}{}", self.qual_prefix, name);
        let mut inner = Compiler::new(
            Code {
                name: name.to_string(),
                qualname: qualname.clone(),
                params,
                posonly,
                vararg,
                kwonly: kwonly.clone(),
                kwarg,
                is_function: true,
                is_async,
                first_line: line,
                ..Code::default()
            },
            line,
        );
        inner.locals = Some(locals);
        inner.qual_prefix = format!("{qualname}.<locals>.");
        inner.globals_decl = globals;
        inner.nonlocals_decl = nonlocals;
        inner.enclosing_class = if self.in_class_body { self.class_name.clone() } else { self.enclosing_class.clone() };
        inner.future_annotations = self.future_annotations;
        match body {
            FnBody::Stmts(b) => {
                inner.code.doc = docstring(b);
                inner.block(b)?;
                let c = inner.constant_none();
                inner.emit(Op::LoadConst(c));
            }
            FnBody::Expr(e) => inner.expr(e)?,
        }
        inner.emit(Op::Return);
        let code = Rc::new(inner.code);
        self.line = line;
        for d in &args.defaults {
            self.expr(d)?;
        }
        let mut kw_names = Vec::new();
        for (arg, default) in args.kwonlyargs.iter().zip(&args.kw_defaults) {
            if let Some(d) = default {
                kw_names.push(Value::str(arg.arg.clone()));
                self.expr(d)?;
            }
        }
        let kwdefaults = if kw_names.is_empty() { None } else { Some(self.constant(Value::tuple(kw_names))) };
        self.line = line;
        self.code.functions.push(code);
        let idx = (self.code.functions.len() - 1) as u32;
        self.emit(Op::MakeFunction { code: idx, ndefaults: args.defaults.len() as u32, kwdefaults });
        // Anotações de parâmetros e de retorno, na ordem da assinatura (como o CPython).
        let mut ann_names = Vec::new();
        let ordered = args
            .posonlyargs
            .iter()
            .chain(args.args.iter())
            .chain(args.vararg.as_deref())
            .chain(args.kwonlyargs.iter())
            .chain(args.kwarg.as_deref());
        for a in ordered {
            if let Some(ann) = &a.annotation {
                self.emit_annotation(ann)?;
                ann_names.push(Value::str(a.arg.clone()));
            }
        }
        if let Some(r) = returns {
            self.emit_annotation(r)?;
            ann_names.push(Value::str("return".to_string()));
        }
        if !ann_names.is_empty() {
            self.line = line;
            let k = self.constant(Value::tuple(ann_names));
            self.emit(Op::SetAnnotations(k));
        }
        Ok(())
    }

    /// `class Nome(Bases): corpo`: o corpo roda num escopo próprio e o resultado é a classe.
    fn class_def(
        &mut self,
        name: &str,
        bases: &[Expr],
        keywords: &[Keyword],
        body: &[Stmt],
        line: usize,
    ) -> Result<(), CompileError> {
        let mut scope = Scope::default();
        scope.block(body);
        let (locals, globals, nonlocals) = scope.locals();
        let qualname = format!("{}{}", self.qual_prefix, name);
        let mut inner = Compiler::new(Code { name: name.to_string(), qualname: qualname.clone(), is_class: true, ..Code::default() }, line);
        inner.qual_prefix = format!("{qualname}.");
        inner.locals = Some(locals);
        inner.globals_decl = globals;
        inner.nonlocals_decl = nonlocals;
        inner.in_class_body = true;
        inner.future_annotations = self.future_annotations;
        inner.class_name = Some(name.to_string());
        let k = inner.constant(Value::str(qualname));
        inner.emit(Op::LoadConst(k));
        if let Some(l) = &mut inner.locals {
            l.insert("__qualname__".to_string());
        }
        inner.emit_store("__qualname__");
        if let Some(doc) = docstring(body) {
            let k = inner.constant(Value::str(doc));
            inner.emit(Op::LoadConst(k));
            if let Some(l) = &mut inner.locals {
                l.insert("__doc__".to_string());
            }
            inner.emit_store("__doc__");
        }
        inner.block(body)?;
        let c = inner.constant_none();
        inner.emit(Op::LoadConst(c));
        inner.emit(Op::Return);
        let code = Rc::new(inner.code);
        self.line = line;
        self.exprs(bases)?;
        let mut kw_names = Vec::new();
        for k in keywords {
            self.expr(&k.value)?;
            kw_names.push(Value::str(k.arg.clone().unwrap_or_default()));
        }
        self.line = line;
        self.code.functions.push(code);
        let idx = (self.code.functions.len() - 1) as u32;
        let kwnames = if kw_names.is_empty() { None } else { Some(self.constant(Value::tuple(kw_names))) };
        self.emit(Op::BuildClass { code: idx, nbases: bases.len() as u32, kwnames });
        Ok(())
    }

    /// `with a as x, b as y: corpo`, aninhando um por item. O `__exit__` fica numa variável oculta
    /// para que `return`, `break` e `continue` consigam chamá-lo ao sair.
    /// Variável oculta que guarda o sujeito do padrão na profundidade `d`.
    fn match_tmp(&mut self, d: usize) -> String {
        let name = format!(".m{d}");
        if let Some(l) = &mut self.locals {
            l.insert(name.clone());
        }
        name
    }

    fn load_tmp(&mut self, d: usize) {
        let n = self.match_tmp(d);
        self.emit_load(&n);
    }

    fn store_tmp(&mut self, d: usize) {
        let n = self.match_tmp(d);
        self.emit_store(&n);
    }

    /// Chama o auxiliar `_match_<name>` com `argc` argumentos já empilhados depois dele.
    fn load_helper(&mut self, name: &str) {
        self.emit_load(&format!("_match_{name}"));
    }

    fn load_int(&mut self, n: i64) {
        let c = self.constant(Value::Int(n));
        self.emit(Op::LoadConst(c));
    }

    fn is_wildcard(p: &crate::ast::Pattern) -> bool {
        use crate::ast::PatternKind as P;
        matches!(&p.kind, P::MatchAs { pattern: None, name: None })
    }

    fn match_stmt(&mut self, subject: &Expr, cases: &[crate::ast::MatchCase]) -> Result<(), CompileError> {
        self.expr(subject)?;
        self.store_tmp(0);
        let mut ends = Vec::new();
        for case in cases {
            let mut fails = Vec::new();
            self.pattern(&case.pattern, 0, &mut fails)?;
            if let Some(g) = &case.guard {
                self.expr(g)?;
                fails.push(self.emit(Op::PopJumpIfFalse(0)));
            }
            self.block(&case.body)?;
            ends.push(self.emit(Op::Jump(0)));
            let next = self.here();
            for f in fails {
                self.patch(f, next);
            }
        }
        let end = self.here();
        for e in ends {
            self.patch(e, end);
        }
        Ok(())
    }

    /// Compila o teste do padrão contra a variável oculta `d`; cada falha salta por um item de `fails`.
    fn pattern(&mut self, p: &crate::ast::Pattern, d: usize, fails: &mut Vec<usize>) -> Result<(), CompileError> {
        use crate::ast::PatternKind as P;
        self.line = p.pos.lineno;
        self.span = Span { lineno: p.pos.lineno as u32, end_lineno: p.pos.end_lineno as u32, col: p.pos.col_offset as u32, end_col: p.pos.end_col_offset as u32 };
        match &p.kind {
            P::MatchValue { value } => {
                self.load_tmp(d);
                self.expr(value)?;
                self.emit(Op::Compare(CmpOp::Eq));
                fails.push(self.emit(Op::PopJumpIfFalse(0)));
            }
            P::MatchSingleton { value } => {
                self.load_tmp(d);
                let v = self.constant_value(value)?;
                let c = self.constant(v);
                self.emit(Op::LoadConst(c));
                self.emit(Op::Compare(CmpOp::Is));
                fails.push(self.emit(Op::PopJumpIfFalse(0)));
            }
            P::MatchAs { pattern, name } => {
                if let Some(inner) = pattern {
                    self.pattern(inner, d, fails)?;
                }
                if let Some(n) = name {
                    self.load_tmp(d);
                    self.emit_store(n);
                }
            }
            P::MatchOr { patterns } => {
                let mut ends = Vec::new();
                for (i, alt) in patterns.iter().enumerate() {
                    let mut f = Vec::new();
                    self.pattern(alt, d, &mut f)?;
                    if i + 1 == patterns.len() {
                        fails.extend(f);
                    } else {
                        ends.push(self.emit(Op::Jump(0)));
                        let next = self.here();
                        for x in f {
                            self.patch(x, next);
                        }
                    }
                }
                let end = self.here();
                for e in ends {
                    self.patch(e, end);
                }
            }
            P::MatchSequence { patterns } => {
                let star = patterns.iter().position(|x| matches!(x.kind, P::MatchStar { .. }));
                self.load_helper("seq");
                self.load_tmp(d);
                self.load_int(patterns.len() as i64);
                let flag = self.constant(Value::Bool(star.is_some()));
                self.emit(Op::LoadConst(flag));
                self.emit(Op::Call { argc: 3, kwnames: None });
                fails.push(self.emit(Op::PopJumpIfFalse(0)));
                let n = patterns.len();
                for (i, sub) in patterns.iter().enumerate() {
                    if let P::MatchStar { name } = &sub.kind {
                        if let Some(name) = name {
                            self.load_helper("star");
                            self.load_tmp(d);
                            self.load_int(i as i64);
                            self.load_int((n - i - 1) as i64);
                            self.emit(Op::Call { argc: 3, kwnames: None });
                            self.emit_store(name);
                        }
                        continue;
                    }
                    if Self::is_wildcard(sub) {
                        continue;
                    }
                    let idx = match star {
                        Some(s) if i > s => -((n - i) as i64),
                        _ => i as i64,
                    };
                    self.load_helper("item");
                    self.load_tmp(d);
                    self.load_int(idx);
                    self.emit(Op::Call { argc: 2, kwnames: None });
                    self.store_tmp(d + 2);
                    self.pattern(sub, d + 2, fails)?;
                }
            }
            P::MatchMapping { keys, patterns, rest } => {
                self.load_helper("map");
                self.load_tmp(d);
                self.emit(Op::Call { argc: 1, kwnames: None });
                fails.push(self.emit(Op::PopJumpIfFalse(0)));
                for k in keys {
                    self.expr(k)?;
                }
                self.emit(Op::BuildTuple(keys.len() as u32));
                self.store_tmp(d + 1);
                self.load_helper("vals");
                self.load_tmp(d);
                self.load_tmp(d + 1);
                self.emit(Op::Call { argc: 2, kwnames: None });
                self.store_tmp(d + 2);
                self.load_tmp(d + 2);
                let none = self.constant(Value::None);
                self.emit(Op::LoadConst(none));
                self.emit(Op::Compare(CmpOp::IsNot));
                fails.push(self.emit(Op::PopJumpIfFalse(0)));
                for (i, sub) in patterns.iter().enumerate() {
                    if Self::is_wildcard(sub) {
                        continue;
                    }
                    self.load_tmp(d + 2);
                    self.load_int(i as i64);
                    self.emit(Op::Subscript);
                    self.store_tmp(d + 3);
                    self.pattern(sub, d + 3, fails)?;
                }
                if let Some(r) = rest {
                    self.load_helper("rest");
                    self.load_tmp(d);
                    self.load_tmp(d + 1);
                    self.emit(Op::Call { argc: 2, kwnames: None });
                    self.emit_store(r);
                }
            }
            P::MatchClass { cls, patterns, kwd_attrs, kwd_patterns } => {
                self.load_helper("class");
                self.load_tmp(d);
                self.expr(cls)?;
                self.load_int(patterns.len() as i64);
                for a in kwd_attrs {
                    let c = self.constant(Value::str(a.clone()));
                    self.emit(Op::LoadConst(c));
                }
                self.emit(Op::BuildTuple(kwd_attrs.len() as u32));
                self.emit(Op::Call { argc: 4, kwnames: None });
                self.store_tmp(d + 1);
                self.load_tmp(d + 1);
                let none = self.constant(Value::None);
                self.emit(Op::LoadConst(none));
                self.emit(Op::Compare(CmpOp::IsNot));
                fails.push(self.emit(Op::PopJumpIfFalse(0)));
                for (i, sub) in patterns.iter().chain(kwd_patterns.iter()).enumerate() {
                    if Self::is_wildcard(sub) {
                        continue;
                    }
                    self.load_tmp(d + 1);
                    self.load_int(i as i64);
                    self.emit(Op::Subscript);
                    self.store_tmp(d + 2);
                    self.pattern(sub, d + 2, fails)?;
                }
            }
            P::MatchStar { .. } => return Err(self.unsupported("star pattern outside a sequence")),
        }
        Ok(())
    }

    fn with_stmt(&mut self, items: &[WithItem], body: &[Stmt], is_async: bool) -> Result<(), CompileError> {
        let Some((item, rest)) = items.split_first() else {
            return self.block(body);
        };
        self.hidden += 1;
        let hidden = format!(".with{}", self.hidden);
        if let Some(l) = &mut self.locals {
            l.insert(hidden.clone());
        }
        self.expr(&item.context_expr)?;
        if is_async {
            self.emit(Op::AsyncWithEnter);
            self.emit(Op::GetAwaitable);
            self.await_delegate();
        } else {
            self.emit(Op::WithEnter);
        }
        // `[exit, valor]`: o `__exit__` vai para a variável oculta, o valor para o alvo.
        self.emit(Op::Rot2);
        self.emit_store(&hidden);
        match &item.optional_vars {
            Some(t) => self.store(t)?,
            None => {
                self.emit(Op::Pop);
            }
        }
        let setup = self.emit(Op::SetupTry(0));
        self.tries.push(TryCtx { finalbody: Vec::new(), with_exit: Some(hidden.clone()), with_async: is_async });
        self.with_stmt(rest, body, is_async)?;
        self.tries.pop();
        self.emit(Op::PopBlock);
        self.emit_load(&hidden);
        let none = self.constant_none();
        for _ in 0..3 {
            self.emit(Op::LoadConst(none));
        }
        self.emit(Op::Call { argc: 3, kwnames: None });
        if is_async {
            self.emit(Op::GetAwaitable);
            self.await_delegate();
        }
        self.emit(Op::Pop);
        let to_end = self.emit(Op::Jump(0));
        let handler = self.here();
        self.patch(setup, handler);
        self.emit(Op::PushExc);
        self.emit_load(&hidden);
        if is_async {
            self.emit(Op::AsyncWithExceptCall);
            self.emit(Op::GetAwaitable);
            self.await_delegate();
        } else {
            self.emit(Op::WithExcept);
        }
        let suppressed = self.emit(Op::PopJumpIfTrue(0));
        self.emit(Op::Reraise);
        let ok = self.here();
        self.patch(suppressed, ok);
        self.emit(Op::Pop);
        self.emit(Op::PopExc);
        let end = self.here();
        self.patch(to_end, end);
        Ok(())
    }

    /// `del alvo`.
    fn delete(&mut self, target: &Expr) -> Result<(), CompileError> {
        match &target.kind {
            E::Name { id, .. } => {
                let n = self.name(id);
                let local = self.locals.as_ref().is_some_and(|l| l.contains(id.as_str()));
                self.emit(if local { Op::DeleteLocal(n) } else { Op::DeleteGlobal(n) });
            }
            E::Attribute { value, attr, .. } => {
                self.expr(value)?;
                let n = self.name(attr);
                self.emit(Op::DeleteAttr(n));
            }
            E::Subscript { value, slice, .. } => {
                self.expr(value)?;
                self.slice_or_expr(slice)?;
                self.emit(Op::DeleteSubscript);
            }
            E::Tuple { elts, .. } | E::List { elts, .. } => {
                for e in elts {
                    self.delete(e)?;
                }
            }
            _ => return Err(self.unsupported("this del target")),
        }
        Ok(())
    }

    /// O índice de um subscript: uma fatia vira um objeto `slice`.
    fn slice_or_expr(&mut self, slice: &Expr) -> Result<(), CompileError> {
        if let E::Slice { lower, upper, step } = &slice.kind {
            for part in [lower, upper, step] {
                match part {
                    Some(e) => self.expr(e)?,
                    None => {
                        let c = self.constant_none();
                        self.emit(Op::LoadConst(c));
                    }
                }
            }
            self.emit(Op::BuildSlice);
            Ok(())
        } else {
            self.expr(slice)
        }
    }

    /// Compreensão: o corpo vira uma função `<listcomp>` etc. chamada com o primeiro iterável, como
    /// no CPython (o alvo do laço não vaza para fora). `kind`: 0 lista, 1 conjunto, 2 dicionário,
    /// 3 gerador.
    fn comprehension(
        &mut self,
        kind: u8,
        elt: &Expr,
        value: Option<&Expr>,
        generators: &[Comprehension],
        line: usize,
    ) -> Result<(), CompileError> {
        let name = ["<listcomp>", "<setcomp>", "<dictcomp>", "<genexpr>"][kind as usize];
        let mut scope = Scope::default();
        scope.bound.insert(".0".to_string());
        for g in generators {
            scope.target(&g.target);
            scope.expr(&g.iter);
            g.ifs.iter().for_each(|i| scope.expr(i));
        }
        scope.expr(elt);
        if let Some(v) = value {
            scope.expr(v);
        }
        let is_async = generators.iter().any(|g| g.is_async != 0) || scope.has_await;
        if is_async && !self.code.is_async {
            return Err(CompileError {
                kind: "SyntaxError",
                msg: "asynchronous comprehension outside of an asynchronous function".into(),
                lineno: line,
            });
        }
        let (locals, globals, nonlocals) = scope.locals();
        let mut inner = Compiler::new(
            Code { name: name.to_string(), qualname: format!("{}{}", self.qual_prefix, name), params: vec![".0".to_string()], is_function: true, ..Code::default() },
            line,
        );
        inner.qual_prefix = format!("{}{}.<locals>.", self.qual_prefix, name);
        inner.locals = Some(locals);
        inner.globals_decl = globals;
        inner.nonlocals_decl = nonlocals;
        inner.code.is_generator = kind == 3;
        inner.code.is_async = is_async;
        match kind {
            0 => {
                inner.emit(Op::BuildList(0));
            }
            1 => {
                inner.emit(Op::BuildSet(0));
            }
            2 => {
                inner.emit(Op::BuildDict(0));
            }
            _ => {}
        }
        inner.comp_loops(kind, elt, value, generators, 0)?;
        if kind == 3 {
            let c = inner.constant_none();
            inner.emit(Op::LoadConst(c));
        }
        inner.emit(Op::Return);
        let code = Rc::new(inner.code);
        self.line = line;
        self.code.functions.push(code);
        let idx = (self.code.functions.len() - 1) as u32;
        self.emit(Op::MakeFunction { code: idx, ndefaults: 0, kwdefaults: None });
        self.expr(&generators[0].iter)?;
        self.line = line;
        self.emit(Op::Call { argc: 1, kwnames: None });
        if is_async && kind != 3 {
            // a compreensão é uma corotina: quem a escreveu espera o resultado
            self.emit(Op::GetAwaitable);
            self.await_delegate();
        }
        Ok(())
    }

    /// Laços aninhados de uma compreensão; no mais interno, acrescenta o elemento.
    fn comp_loops(
        &mut self,
        kind: u8,
        elt: &Expr,
        value: Option<&Expr>,
        generators: &[Comprehension],
        depth: usize,
    ) -> Result<(), CompileError> {
        let g = &generators[depth];
        if depth == 0 {
            self.emit_load(".0");
        } else {
            self.expr(&g.iter)?;
        }
        self.at(&g.iter.pos);
        let is_async_loop = g.is_async != 0;
        let top = if is_async_loop {
            self.emit(Op::GetAIter);
            let top = self.emit(Op::SetupTry(0));
            self.emit(Op::GetANext);
            self.emit(Op::GetAwaitable);
            self.await_delegate();
            self.emit(Op::PopBlock);
            top
        } else {
            self.emit(Op::GetIter);
            self.emit(Op::ForIter(0))
        };
        self.store(&g.target)?;
        let mut skips = Vec::new();
        for cond in &g.ifs {
            self.expr(cond)?;
            skips.push(self.emit(Op::PopJumpIfFalse(0)));
        }
        if depth + 1 < generators.len() {
            self.comp_loops(kind, elt, value, generators, depth + 1)?;
        } else {
            match kind {
                0 => {
                    self.expr(elt)?;
                    self.emit(Op::ListAppendAt(depth as u32 + 1));
                }
                1 => {
                    self.expr(elt)?;
                    self.emit(Op::SetAddAt(depth as u32 + 1));
                }
                2 => {
                    self.expr(elt)?;
                    self.expr(value.ok_or_else(|| self.unsupported("dict comprehension value"))?)?;
                    self.emit(Op::MapAddAt(depth as u32 + 1));
                }
                _ => {
                    self.expr(elt)?;
                    self.emit(if self.code.is_async { Op::AsyncGenYield } else { Op::Yield });
                    self.emit(Op::Pop);
                }
            }
        }
        let next = self.here();
        for s in skips {
            self.patch(s, next);
        }
        self.emit(Op::Jump(top as u32));
        let end = self.here();
        self.patch(top, end);
        if is_async_loop {
            let stop = self.emit(Op::AsyncForExcept(0));
            let after = self.here();
            self.patch(stop, after);
        }
        Ok(())
    }

    fn constant_none(&mut self) -> u32 {
        self.constant(Value::None)
    }

    /// `return` com o valor já na pilha: fecha os `try` abertos (repetindo os `finally`).
    fn emit_return(&mut self) -> Result<(), CompileError> {
        self.leave_tries(0)?;
        self.emit(Op::Return);
        Ok(())
    }

    fn aug_assign(&mut self, target: &Expr, op: Operator, value: &Expr) -> Result<(), CompileError> {
        match &target.kind {
            E::Name { id, .. } => {
                self.emit_load(id);
                self.expr(value)?;
                self.emit(Op::Binary { op, inplace: true });
                self.emit_store(id);
            }
            E::Subscript { value: container, slice, .. } => {
                self.expr(container)?;
                self.slice_or_expr(slice)?;
                self.emit(Op::Dup2);
                self.emit(Op::Subscript);
                self.expr(value)?;
                self.emit(Op::Binary { op, inplace: true });
                self.emit(Op::Rot3);
                self.emit(Op::StoreSubscript);
            }
            E::Attribute { value: obj, attr, .. } => {
                self.expr(obj)?;
                self.emit(Op::Dup);
                let n = self.name(attr);
                self.emit(Op::LoadAttr(n));
                self.expr(value)?;
                self.emit(Op::Binary { op, inplace: true });
                self.emit(Op::Rot2);
                self.emit(Op::StoreAttr(n));
            }
            _ => return Err(self.unsupported("this augmented assignment target")),
        }
        Ok(())
    }

    /// Guarda o topo da pilha no alvo.
    fn store(&mut self, target: &Expr) -> Result<(), CompileError> {
        let saved = (self.line, self.span);
        self.at(&target.pos);
        match &target.kind {
            E::Name { id, .. } => {
                self.emit_store(id);
            }
            E::Subscript { value, slice, .. } => {
                self.expr(value)?;
                self.slice_or_expr(slice)?;
                self.emit(Op::StoreSubscript);
            }
            E::Attribute { value, attr, .. } => {
                self.expr(value)?;
                let n = self.name(attr);
                self.emit(Op::StoreAttr(n));
            }
            E::Tuple { elts, .. } | E::List { elts, .. } => {
                let stars: Vec<usize> =
                    elts.iter().enumerate().filter(|(_, e)| matches!(e.kind, E::Starred { .. })).map(|(i, _)| i).collect();
                match stars.as_slice() {
                    [] => {
                        self.emit(Op::UnpackSequence(elts.len() as u32));
                    }
                    [i] => {
                        self.emit(Op::UnpackEx { before: *i as u32, after: (elts.len() - i - 1) as u32 });
                    }
                    _ => {
                        return Err(CompileError {
                            kind: "SyntaxError",
                            msg: "multiple starred expressions in assignment".into(),
                            lineno: self.line,
                        })
                    }
                }
                for elt in elts {
                    match &elt.kind {
                        E::Starred { value, .. } => self.store(value)?,
                        _ => self.store(elt)?,
                    }
                }
            }
            _ => return Err(self.unsupported("this assignment target")),
        }
        (self.line, self.span) = saved;
        Ok(())
    }

    fn expr(&mut self, expr: &Expr) -> Result<(), CompileError> {
        let saved = (self.line, self.span);
        self.at(&expr.pos);
        self.expr_inner(expr)?;
        (self.line, self.span) = saved;
        Ok(())
    }

    /// Avalia as expressões em ordem, cada uma deixando um valor (sem `*x`: ver `build_sequence`).
    fn exprs(&mut self, exprs: &[Expr]) -> Result<(), CompileError> {
        exprs.iter().try_for_each(|e| self.expr(e))
    }

    /// Lista com os elementos de `elts` no topo da pilha, expandindo os `*x`. Sem estrelas é o
    /// `BuildList` direto; com estrelas, acrescenta item a item.
    fn build_list(&mut self, elts: &[Expr]) -> Result<(), CompileError> {
        if !elts.iter().any(|e| matches!(e.kind, E::Starred { .. })) {
            self.exprs(elts)?;
            self.emit(Op::BuildList(elts.len() as u32));
            return Ok(());
        }
        self.emit(Op::BuildList(0));
        for e in elts {
            match &e.kind {
                E::Starred { value, .. } => {
                    self.expr(value)?;
                    self.emit(Op::ListExtend);
                }
                _ => {
                    self.expr(e)?;
                    self.emit(Op::ListAppend);
                }
            }
        }
        Ok(())
    }

    fn expr_inner(&mut self, expr: &Expr) -> Result<(), CompileError> {
        match &expr.kind {
            E::Constant { value: Constant::Complex(re, im), .. } => {
                // `3j` é `complex(0.0, 3.0)`: o tipo vive em `modules/py/_complex.py`.
                self.emit_load("complex");
                for part in [*re, *im] {
                    let i = self.constant(Value::Float(part));
                    self.emit(Op::LoadConst(i));
                }
                self.emit(Op::Call { argc: 2, kwnames: None });
            }
            E::Constant { value, .. } => {
                let v = self.constant_value(value)?;
                let i = self.constant(v);
                self.emit(Op::LoadConst(i));
            }
            E::Name { id, .. } => {
                self.emit_load(id);
            }
            E::BinOp { left, op, right } => {
                self.expr(left)?;
                self.expr(right)?;
                self.emit(Op::Binary { op: *op, inplace: false });
            }
            E::UnaryOp { op, operand } => {
                self.expr(operand)?;
                self.emit(Op::Unary(*op));
            }
            E::BoolOp { op, values } => {
                let mut jumps = Vec::new();
                for (i, value) in values.iter().enumerate() {
                    self.expr(value)?;
                    if i + 1 < values.len() {
                        jumps.push(self.emit(match op {
                            BoolOp::And => Op::JumpIfFalseOrPop(0),
                            BoolOp::Or => Op::JumpIfTrueOrPop(0),
                        }));
                    }
                }
                let end = self.here();
                for j in jumps {
                    self.patch(j, end);
                }
            }
            E::IfExp { test, body, orelse } => {
                self.expr(test)?;
                let to_else = self.emit(Op::PopJumpIfFalse(0));
                self.expr(body)?;
                let to_end = self.emit(Op::Jump(0));
                let else_start = self.here();
                self.patch(to_else, else_start);
                self.expr(orelse)?;
                let end = self.here();
                self.patch(to_end, end);
            }
            E::Compare { left, ops, comparators } => self.compare(left, ops, comparators)?,
            E::Call { func, args, keywords } => {
                // `super()` sem argumentos dentro de um método: `super(self, "Classe")`.
                if let (E::Name { id, .. }, true, true, Some(class), Some(first)) =
                    (&func.kind, args.is_empty(), keywords.is_empty(), self.enclosing_class.clone(), self.code.params.first().cloned())
                    && id == "super"
                {
                    self.emit_load("super");
                    self.emit_load(&first);
                    let c = self.constant(Value::str(class));
                    self.emit(Op::LoadConst(c));
                    self.at(&expr.pos);
                    self.emit(Op::Call { argc: 2, kwnames: None });
                    return Ok(());
                }
                if let (E::Name { id, .. }, true, true, true) = (&func.kind, args.is_empty(), keywords.is_empty(), self.code.is_function)
                    && id == "locals"
                {
                    self.at(&expr.pos);
                    self.emit(Op::Locals);
                    return Ok(());
                }
                self.expr(func)?;
                let star_args = args.iter().any(|a| matches!(a.kind, E::Starred { .. }));
                let star_kw = keywords.iter().any(|k| k.arg.is_none());
                if !star_args && !star_kw {
                    self.exprs(args)?;
                    let mut names = Vec::new();
                    for kw in keywords {
                        let name = kw.arg.clone().unwrap_or_default();
                        names.push(Value::str(name));
                        self.expr(&kw.value)?;
                    }
                    let kwnames = if names.is_empty() { None } else { Some(self.constant(Value::tuple(names))) };
                    self.at(&expr.pos);
                    self.emit(Op::Call { argc: (args.len() + keywords.len()) as u32, kwnames });
                } else {
                    self.build_list(args)?;
                    if !keywords.is_empty() {
                        self.emit(Op::BuildDict(0));
                        for kw in keywords {
                            match &kw.arg {
                                Some(name) => {
                                    let c = self.constant(Value::str(name.clone()));
                                    self.emit(Op::LoadConst(c));
                                    self.expr(&kw.value)?;
                                    self.emit(Op::DictSet);
                                }
                                None => {
                                    self.expr(&kw.value)?;
                                    self.emit(Op::DictUpdate);
                                }
                            }
                        }
                    }
                    self.at(&expr.pos);
                    self.emit(Op::CallEx { kwargs: !keywords.is_empty() });
                }
            }
            E::List { elts, .. } => self.build_list(elts)?,
            E::Tuple { elts, .. } => {
                if elts.iter().any(|e| matches!(e.kind, E::Starred { .. })) {
                    self.build_list(elts)?;
                    self.emit(Op::ListToTuple);
                } else {
                    self.exprs(elts)?;
                    self.emit(Op::BuildTuple(elts.len() as u32));
                }
            }
            E::Set { elts } => {
                if elts.iter().any(|e| matches!(e.kind, E::Starred { .. })) {
                    self.build_list(elts)?;
                    self.emit(Op::ListToSet);
                } else {
                    self.exprs(elts)?;
                    self.emit(Op::BuildSet(elts.len() as u32));
                }
            }
            E::Dict { keys, values } => {
                if keys.iter().all(Option::is_some) {
                    for (key, value) in keys.iter().zip(values) {
                        if let Some(key) = key {
                            self.expr(key)?;
                        }
                        self.expr(value)?;
                    }
                    self.emit(Op::BuildDict(values.len() as u32));
                } else {
                    self.emit(Op::BuildDict(0));
                    for (key, value) in keys.iter().zip(values) {
                        match key {
                            Some(k) => {
                                self.expr(k)?;
                                self.expr(value)?;
                                self.emit(Op::DictSet);
                            }
                            None => {
                                self.expr(value)?;
                                self.emit(Op::DictUpdate);
                            }
                        }
                    }
                }
            }
            E::Subscript { value, slice, .. } => {
                self.expr(value)?;
                self.slice_or_expr(slice)?;
                self.at(&expr.pos);
                self.emit(Op::Subscript);
            }
            E::Attribute { value, attr, .. } => {
                self.expr(value)?;
                let n = self.name(attr);
                self.at(&expr.pos);
                self.emit(Op::LoadAttr(n));
            }
            E::Lambda { args, body } => self.make_function("<lambda>", args, FnBody::Expr(body), expr.pos.lineno, false, None)?,
            E::NamedExpr { target, value } => {
                self.expr(value)?;
                self.emit(Op::Dup);
                self.store(target)?;
            }
            E::ListComp { elt, generators } => self.comprehension(0, elt, None, generators, expr.pos.lineno)?,
            E::SetComp { elt, generators } => self.comprehension(1, elt, None, generators, expr.pos.lineno)?,
            E::DictComp { key, value, generators } => {
                self.comprehension(2, key, Some(value), generators, expr.pos.lineno)?
            }
            E::GeneratorExp { elt, generators } => self.comprehension(3, elt, None, generators, expr.pos.lineno)?,
            E::JoinedStr { values } => {
                for v in values {
                    self.expr(v)?;
                }
                self.emit(Op::BuildString(values.len() as u32));
            }
            E::FormattedValue { value, conversion, format_spec } => {
                self.expr(value)?;
                if let Some(spec) = format_spec {
                    self.expr(spec)?;
                }
                let conv = match conversion {
                    115 => 1,
                    114 => 2,
                    97 => 3,
                    _ => 0,
                };
                self.at(&expr.pos);
                self.emit(Op::FormatValue { conv, has_spec: format_spec.is_some() });
            }
            E::Yield { value } => {
                self.code.is_generator = true;
                match value {
                    Some(v) => self.expr(v)?,
                    None => {
                        let c = self.constant_none();
                        self.emit(Op::LoadConst(c));
                    }
                }
                self.at(&expr.pos);
                self.emit(if self.code.is_async { Op::AsyncGenYield } else { Op::Yield });
            }
            E::YieldFrom { value } => {
                self.code.is_generator = true;
                self.expr(value)?;
                self.at(&expr.pos);
                self.emit(Op::GetIter);
                self.await_delegate();
            }
            E::Await { value } => {
                if !self.code.is_async {
                    return Err(CompileError {
                        kind: "SyntaxError",
                        msg: "'await' outside async function".into(),
                        lineno: expr.pos.lineno,
                    });
                }
                self.expr(value)?;
                self.at(&expr.pos);
                self.emit(Op::GetAwaitable);
                self.await_delegate();
            }
            E::Starred { .. } => {
                return Err(CompileError {
                    kind: "SyntaxError",
                    msg: "can't use starred expression here".into(),
                    lineno: self.line,
                })
            }
            E::Slice { .. } => return Err(self.unsupported("slicing outside a subscript")),
        }
        Ok(())
    }

    /// `a op1 b op2 c`: cada operando é avaliado uma vez, e a cadeia para no primeiro falso.
    fn compare(&mut self, left: &Expr, ops: &[CmpOp], comparators: &[Expr]) -> Result<(), CompileError> {
        self.expr(left)?;
        if ops.len() == 1 {
            self.expr(&comparators[0])?;
            self.emit(Op::Compare(ops[0]));
            return Ok(());
        }
        let mut cleanups = Vec::new();
        for (i, (op, right)) in ops.iter().zip(comparators).enumerate() {
            self.expr(right)?;
            if i + 1 < ops.len() {
                self.emit(Op::Dup);
                self.emit(Op::Rot3);
                self.emit(Op::Compare(*op));
                cleanups.push(self.emit(Op::JumpIfFalseOrPop(0)));
            } else {
                self.emit(Op::Compare(*op));
            }
        }
        let to_end = self.emit(Op::Jump(0));
        let cleanup = self.here();
        for c in cleanups {
            self.patch(c, cleanup);
        }
        self.emit(Op::Rot2);
        self.emit(Op::Pop);
        let end = self.here();
        self.patch(to_end, end);
        Ok(())
    }

    fn constant_value(&self, c: &Constant) -> Result<Value, CompileError> {
        Ok(match c {
            Constant::None => Value::None,
            Constant::Bool(b) => Value::Bool(*b),
            Constant::Int(digits) => match digits.parse::<i64>() {
                Ok(i) => Value::Int(i),
                Err(_) => match crate::bigint::parse(digits, 10) {
                    Some(big) => crate::bigint::norm(big),
                    None => return Err(self.unsupported("integer literal")),
                },
            },
            Constant::Float(x) => Value::Float(*x),
            Constant::Str(s) => Value::str(s.clone()),
            Constant::Bytes(b) => Value::bytes(b.clone()),
            Constant::Complex(..) => return Err(self.unsupported("complex numbers")),
            Constant::Ellipsis => Value::Builtin("Ellipsis"),
        })
    }
}
