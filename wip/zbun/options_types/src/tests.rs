use crate::jsx::{MemberList, Pragma, RUNTIME_MAP, Runtime};
use crate::schema::api;
use crate::{Format, LoaderExt, ModuleType, TargetExt};
use bun_ast::{Loader, Target};

#[test]
fn pragma_defaults_and_runtime_map() {
    let pragma = Pragma::default();
    assert_eq!(pragma.factory.iter().collect::<Vec<_>>(), [&b"React"[..], b"createElement"]);
    assert_eq!(pragma.import_source(), b"react/jsx-dev-runtime");
    assert_eq!(RUNTIME_MAP.get(b"react-jsx").map(|p| (p.runtime, p.development)), Some((Runtime::Automatic, Some(false))));
}

#[test]
fn pragma_from_api_splits_members_and_sets_import_source() {
    let pragma = Pragma::from_api(api::Jsx {
        factory: b"h".to_vec().into(),
        fragment: b"React.Fragment".to_vec().into(),
        runtime: api::JsxRuntime::Classic,
        import_source: b"preact".to_vec().into(),
        development: false,
        side_effects: false,
    });
    assert!(matches!(pragma.fragment, MemberList::Static(_)));
    assert_eq!(pragma.factory.iter().collect::<Vec<_>>(), [&b"h"[..]]);
    assert_eq!(pragma.runtime, Runtime::Classic);
    assert_eq!(pragma.import_source(), b"preact/jsx-runtime");
    assert!(Pragma::member_list_to_components_if_different(&pragma.factory, b".").is_none());
}

#[test]
fn bundle_enums_round_trip() {
    assert_eq!(Format::from_string(b"cjs"), Some(Format::Cjs));
    assert_eq!(ModuleType::LIST.get(b"module").copied(), Some(ModuleType::Esm));
    assert_eq!(Loader::from_api(Loader::Tsx.to_api()), Loader::Tsx);
    assert_eq!(Target::from_api(Some(Target::ServerComponentsSsr.to_api())), Target::Bun);
}
