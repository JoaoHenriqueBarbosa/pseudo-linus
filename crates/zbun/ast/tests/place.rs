//! O lugar tipado (`bun_ast::place`): ler e escrever um campo de nó do `Store` sem guardar empréstimo entre as duas
//! operações, na ordem em que o upstream escrevia pelo ponteiro para o campo.
use bun_ast::store::Store;
use bun_ast::{E, ExprPlace, G, Loc, NodeStore, Place, PropertyListPlace, S, StoreSlice, Stmt};

fn at(start: i32) -> bun_ast::Expr {
    bun_ast::Expr { loc: Loc { start }, ..Default::default() }
}

fn start_of(store: &Store, place: ExprPlace) -> i32 {
    place.get(store).loc.start
}

#[test]
fn each_expr_field_reads_and_writes_only_its_own_slot() {
    let mut store = Store::default();
    let dot = store.alloc(E::Dot { target: at(1), ..Default::default() });
    let binary = store.alloc(E::Binary { left: at(2), right: at(3), op: bun_ast::OpCode::BinAdd });
    let s_expr = store.alloc(S::SExpr { value: at(4), ..Default::default() });
    let throw = store.alloc(S::Throw { value: at(5) });
    let ret = store.alloc(S::Return { value: Some(at(6)) });
    let if_stmt = store.alloc(S::If { test: at(7), yes: Stmt::default(), no: None });
    let switch = store.alloc(S::Switch { test: at(8), body_loc: Loc::EMPTY, cases: StoreSlice::EMPTY });
    let decls = store.alloc_slice([
        G::Decl { binding: Default::default(), value: Some(at(9)) },
        G::Decl { binding: Default::default(), value: Some(at(10)) },
    ]);

    let places = [
        (ExprPlace::DotTarget(dot), 1),
        (ExprPlace::BinaryLeft(binary), 2),
        (ExprPlace::SExprValue(s_expr), 4),
        (ExprPlace::ThrowValue(throw), 5),
        (ExprPlace::ReturnValue(ret), 6),
        (ExprPlace::IfTest(if_stmt), 7),
        (ExprPlace::SwitchTest(switch), 8),
        (ExprPlace::DeclValue(decls, 0), 9),
        (ExprPlace::DeclValue(decls, 1), 10),
    ];
    for (place, start) in places {
        assert_eq!(start_of(&store, place), start);
    }
    for (index, (place, _)) in places.iter().enumerate() {
        place.set(&mut store, at(100 + index as i32));
        for (other, (other_place, start)) in places.iter().enumerate() {
            let expected = if other <= index { 100 + other as i32 } else { *start };
            assert_eq!(start_of(&store, *other_place), expected, "depois de escrever o lugar {index}");
        }
    }
    // O campo vizinho do mesmo nó não muda.
    assert_eq!(store[binary].right.loc.start, 3);
}

/// A caminhada pela cadeia de `E::Dot` do `maybe_defined_helper`: escreve o lugar, aloca o próximo nó e desce para o
/// campo dele, sem empréstimo de pé entre uma alocação e outra.
#[test]
fn a_chain_walk_can_allocate_between_writes() {
    let mut store = Store::default();
    let head = store.alloc(E::Dot { target: at(0), ..Default::default() });
    let mut current = ExprPlace::DotTarget(head);
    let mut nodes = vec![head];
    for start in 1..=3 {
        let next = store.alloc(E::Dot { target: at(0), ..Default::default() });
        nodes.push(next);
        current.set(&mut store, bun_ast::Expr { loc: Loc { start }, data: bun_ast::ExprData::EDot(next) });
        current = ExprPlace::DotTarget(next);
    }
    current.set(&mut store, at(99));
    let mut place = ExprPlace::DotTarget(head);
    for expected in 1..=3 {
        let value = place.get(&store);
        assert_eq!(value.loc.start, expected);
        place = ExprPlace::DotTarget(value.data.e_dot().expect("é um E::Dot"));
    }
    assert_eq!(start_of(&store, place), 99);
}

#[test]
fn property_list_push_and_take() {
    let mut store = Store::default();
    let object = store.alloc(E::Object::default());
    let place = PropertyListPlace::Object(object);
    assert!(place.get(&store).is_empty());

    let property = |start: i32| G::Property { key: Some(at(start)), ..Default::default() };
    place.push(&mut store, property(1));
    place.push(&mut store, property(2));
    // Uma alocação no meio: a lista deixa de terminar no fim do vetor e o `append` copia, sem perder os elementos.
    let _unrelated = store.alloc_slice([property(50)]);
    place.push(&mut store, property(3));
    let list = place.get(&store);
    let starts: Vec<i32> = store[list].iter().map(|p| p.key.expect("tem chave").loc.start).collect();
    assert_eq!(starts, [1, 2, 3]);

    let taken = place.take(&mut store);
    assert_eq!(taken.len(), 3);
    assert!(place.get(&store).is_empty());
    assert_eq!(store[object].properties.len(), 0);

    let jsx = store.alloc(E::JSXElement::default());
    let jsx_place = PropertyListPlace::JsxElement(jsx);
    jsx_place.push(&mut store, property(7));
    assert_eq!(jsx_place.get(&store).len(), 1);
    assert!(place.get(&store).is_empty());
}
