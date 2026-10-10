//! O "lugar" tipado: o campo de um nó do `Store` que o upstream apontava com `&mut Expr` ou `&mut PropertyList`.
//!
//! No upstream, `StoreRef::from_bump(&mut node.campo)` guarda um ponteiro para o campo e o parser escreve por ele
//! enquanto aloca outros nós (a caminhada pela cadeia de `E::Dot`, a troca do alvo de uma substituição de uso
//! único, a lista de propriedades de um `<div {...{...x}}>`). Com índices não há ponteiro para campo: o lugar é o
//! par (nó dono, qual campo), resolvido contra o `Store` a cada leitura e a cada escrita. Nenhum empréstimo fica de
//! pé entre os dois, então o parser pode alocar, visitar e reentrar no meio, e a ordem das leituras e das escritas é
//! a do upstream.
//!
//! Regra de escolha no parser (README, "Lugar tipado"): onde o upstream lê o campo, escreve e segue sem que nada
//! mais alcance o mesmo campo, copiar o valor para fora e gravar de volta basta; onde um ponteiro para o campo vive
//! por mais de uma alocação (a caminhada pela cadeia, que escreve no campo e depois desce para o filho recém
//! criado), o lugar é o tipo certo.

use crate::new_store::NodeStore;
use crate::nodes::{StoreRef, StoreSlice};
use crate::store::Store;
use crate::{E, Expr, G, S};

/// Um campo do `Store` que se lê e se escreve por valor.
pub trait Place: Copy {
    type Value: Copy;

    /// O valor do campo agora.
    fn get(self, store: &Store) -> Self::Value;

    /// Grava `value` no campo.
    fn set(self, store: &mut Store, value: Self::Value);
}

/// Um campo `Expr` de um nó.
#[derive(Clone, Copy, Debug)]
pub enum ExprPlace {
    /// `E::Dot.target`.
    DotTarget(StoreRef<E::Dot>),
    /// `E::Binary.left`.
    BinaryLeft(StoreRef<E::Binary>),
    /// `S::SExpr.value`.
    SExprValue(StoreRef<S::SExpr>),
    /// `S::Throw.value`.
    ThrowValue(StoreRef<S::Throw>),
    /// `S::Return.value`, só quando o `return` tem valor.
    ReturnValue(StoreRef<S::Return>),
    /// `S::If.test`.
    IfTest(StoreRef<S::If>),
    /// `S::Switch.test`.
    SwitchTest(StoreRef<S::Switch>),
    /// `value` da declaração `index` de uma lista, só quando ela tem valor.
    DeclValue(G::DeclList, u32),
}

impl Place for ExprPlace {
    type Value = Expr;

    fn get(self, store: &Store) -> Expr {
        match self {
            ExprPlace::DotTarget(node) => store[node].target,
            ExprPlace::BinaryLeft(node) => store[node].left,
            ExprPlace::SExprValue(node) => store[node].value,
            ExprPlace::ThrowValue(node) => store[node].value,
            ExprPlace::ReturnValue(node) => store[node].value.expect("ExprPlace::ReturnValue: return sem valor"),
            ExprPlace::IfTest(node) => store[node].test,
            ExprPlace::SwitchTest(node) => store[node].test,
            ExprPlace::DeclValue(decls, index) => {
                store[decls][index as usize].value.expect("ExprPlace::DeclValue: declaração sem valor")
            }
        }
    }

    fn set(self, store: &mut Store, value: Expr) {
        match self {
            ExprPlace::DotTarget(node) => store[node].target = value,
            ExprPlace::BinaryLeft(node) => store[node].left = value,
            ExprPlace::SExprValue(node) => store[node].value = value,
            ExprPlace::ThrowValue(node) => store[node].value = value,
            ExprPlace::ReturnValue(node) => store[node].value = Some(value),
            ExprPlace::IfTest(node) => store[node].test = value,
            ExprPlace::SwitchTest(node) => store[node].test = value,
            ExprPlace::DeclValue(decls, index) => store[decls][index as usize].value = Some(value),
        }
    }
}

/// Um campo `PropertyList` de um nó.
#[derive(Clone, Copy, Debug)]
pub enum PropertyListPlace {
    /// `E::JSXElement.properties`.
    JsxElement(StoreRef<E::JSXElement>),
    /// `E::Object.properties`.
    Object(StoreRef<E::Object>),
}

impl Place for PropertyListPlace {
    type Value = StoreSlice<G::Property>;

    fn get(self, store: &Store) -> StoreSlice<G::Property> {
        match self {
            PropertyListPlace::JsxElement(node) => store[node].properties,
            PropertyListPlace::Object(node) => store[node].properties,
        }
    }

    fn set(self, store: &mut Store, value: StoreSlice<G::Property>) {
        match self {
            PropertyListPlace::JsxElement(node) => store[node].properties = value,
            PropertyListPlace::Object(node) => store[node].properties = value,
        }
    }
}

impl PropertyListPlace {
    /// O `props.push(property)` do upstream: a lista do campo ganha `property` no fim.
    pub fn push(self, store: &mut Store, property: G::Property) {
        let list = self.get(store);
        let list = store.append(list, property);
        self.set(store, list);
    }

    /// O `std::mem::take(props)` do upstream: devolve a lista do campo e deixa o campo vazio.
    pub fn take(self, store: &mut Store) -> StoreSlice<G::Property> {
        let list = self.get(store);
        self.set(store, StoreSlice::EMPTY);
        list
    }
}
