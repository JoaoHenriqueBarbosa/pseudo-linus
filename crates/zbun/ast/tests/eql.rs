//! `Data::eql`: a tabela de igualdade do upstream (https://dorey.github.io/JavaScript-Equality-Table/) sobre o
//! `Store` do arquivo.
use bun_ast::expr::{Data, EqlParser, Equality, LooseEql, StrictEql};
use bun_ast::{E, NodeStore, Ref, RefTag, Store};

struct P {
    store: Store,
    module_ref: Ref,
}

impl EqlParser for P {
    fn store_mut(&mut self) -> &mut Store {
        &mut self.store
    }
    fn module_ref(&self) -> Ref {
        self.module_ref
    }
}

fn string(p: &mut P, text: &[u8]) -> Data {
    let s = p.store.alloc_str(text);
    Data::EString(p.store.alloc(E::EString::init(s)))
}

#[test]
fn equality_table() {
    let mut p = P { store: Store::default(), module_ref: Ref::new(4, 0, RefTag::Symbol) };
    let one = Data::ENumber(E::Number::new(1.0));
    let t = Data::EBoolean(E::Boolean { value: true });
    let null = Data::ENull(E::Null {});
    let undef = Data::EUndefined(E::Undefined {});
    assert_eq!(Data::eql::<P, LooseEql>(&t, &one, &mut p), Equality::Equal);
    assert_eq!(Data::eql::<P, StrictEql>(&t, &one, &mut p), Equality::NotEqual);
    assert_eq!(Data::eql::<P, LooseEql>(&null, &undef, &mut p), Equality::Equal);
    assert_eq!(Data::eql::<P, StrictEql>(&null, &undef, &mut p), Equality::NotEqual);

    let a = string(&mut p, b"1");
    let b = string(&mut p, b"1");
    let c = string(&mut p, b"0x1");
    assert_eq!(Data::eql::<P, StrictEql>(&a, &b, &mut p), Equality::Equal);
    assert_eq!(Data::eql::<P, LooseEql>(&a, &one, &mut p), Equality::Equal);
    assert_eq!(Data::eql::<P, LooseEql>(&c, &one, &mut p), Equality::Unknown);
    assert_eq!(Data::eql::<P, StrictEql>(&a, &one, &mut p), Equality::NotEqual);

    let module = Data::EIdentifier(E::Identifier::init(p.module_ref));
    assert_eq!(Data::eql::<P, StrictEql>(&module, &Data::ERequireMain, &mut p), Equality::RequireMainAndModule);
}
