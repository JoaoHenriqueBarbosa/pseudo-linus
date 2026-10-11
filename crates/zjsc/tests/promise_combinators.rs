//! `Promise` ponta a ponta (`JSPromiseConstructor.cpp`, `JSPromisePrototype.cpp`, `JSMicrotask.cpp`,
//! `PromiseConstructor.js`): os combinadores `all`, `allSettled`, `any`, `race`, mais `withResolvers`,
//! `try` e `finally`. Cada programa grava um booleano na global `result`, que é lida depois da
//! drenagem das microtasks. Os valores esperados são os do C++ (o `cause` explícito do
//! `AggregateError` de `Promise.any`, o formato `{ status, value | reason }` do `allSettled`, a ordem
//! das chaves de `withResolvers`).
use zjsc::api::eval::evaluate_named_script_result;

/// Roda `source`, esvazia as microtasks e devolve a global `result` como booleano.
fn result_of(source: &str) -> bool {
    let value = evaluate_named_script_result(source, "promise_combinators.js", "result")
        .unwrap_or_else(|_| panic!("lançou exceção: {source}"));
    value.is_true()
}

#[test]
fn any_without_elements_rejects_with_an_aggregate_error_that_has_an_explicit_undefined_cause() {
    assert!(result_of(
        "var result = false; \
         Promise.any([]).catch(function (e) { \
           var cause = Object.getOwnPropertyDescriptor(e, 'cause'); \
           var errors = Object.getOwnPropertyDescriptor(e, 'errors'); \
           result = e instanceof AggregateError \
             && cause !== undefined && cause.value === undefined && cause.enumerable === false \
             && errors !== undefined && errors.enumerable === false \
             && Array.isArray(e.errors) && e.errors.length === 0; \
         });"
    ));
}

#[test]
fn any_collects_the_reasons_in_order_when_every_element_rejects() {
    assert!(result_of(
        "var result = false; \
         Promise.any([Promise.reject(1), Promise.reject(2), Promise.reject(3)]).catch(function (e) { \
           result = e.errors.join() === '1,2,3'; \
         });"
    ));
}

#[test]
fn any_fulfills_with_the_first_fulfillment() {
    assert!(result_of(
        "var result = false; \
         Promise.any([Promise.reject(1), new Promise(function () {}), Promise.resolve(7)]).then(function (v) { \
           result = v === 7; \
         });"
    ));
}

#[test]
fn all_keeps_the_input_order_and_waits_for_every_element() {
    assert!(result_of(
        "var result = false; \
         var late = new Promise(function (resolve) { Promise.resolve().then(function () { resolve('late'); }); }); \
         Promise.all([late, 2, Promise.resolve(3)]).then(function (v) { \
           result = v.length === 3 && v[0] === 'late' && v[1] === 2 && v[2] === 3; \
         });"
    ));
}

#[test]
fn all_of_an_empty_iterable_fulfills_with_an_empty_array() {
    assert!(result_of(
        "var result = false; \
         Promise.all([]).then(function (v) { result = Array.isArray(v) && v.length === 0; });"
    ));
}

#[test]
fn all_rejects_with_the_first_rejection() {
    assert!(result_of(
        "var result = false; \
         Promise.all([new Promise(function () {}), Promise.reject(4), Promise.reject(5)]).catch(function (e) { \
           result = e === 4; \
         });"
    ));
}

#[test]
fn all_does_not_run_indexed_setters_of_array_prototype() {
    // `putDirectIndex` define a propriedade própria; um setter indexado herdado não é chamado.
    assert!(result_of(
        "var result = false; var hit = false; \
         var source = [1, 2]; \
         Object.defineProperty(Array.prototype, 0, { set: function (v) { hit = true; }, configurable: true }); \
         Promise.all(source).then(function (v) { \
           var own = Object.getOwnPropertyDescriptor(v, 0); \
           delete Array.prototype[0]; \
           result = !hit && own !== undefined && own.value === 1 && v[1] === 2; \
         });"
    ));
}

#[test]
fn all_settled_reports_status_value_and_reason_with_a_fixed_key_order() {
    assert!(result_of(
        "var result = false; \
         Promise.allSettled([Promise.resolve(1), Promise.reject(2), 3]).then(function (r) { \
           result = r.length === 3 \
             && r[0].status === 'fulfilled' && r[0].value === 1 && Object.keys(r[0]).join() === 'status,value' \
             && r[1].status === 'rejected' && r[1].reason === 2 && Object.keys(r[1]).join() === 'status,reason' \
             && r[2].status === 'fulfilled' && r[2].value === 3; \
         });"
    ));
}

#[test]
fn race_settles_with_the_first_settled_element() {
    assert!(result_of(
        "var result = false; \
         Promise.race([new Promise(function () {}), Promise.resolve(4), Promise.reject(5)]).then(function (v) { \
           result = v === 4; \
         });"
    ));
    assert!(result_of(
        "var result = false; \
         Promise.race([new Promise(function () {}), Promise.reject(5), Promise.resolve(4)]).catch(function (e) { \
           result = e === 5; \
         });"
    ));
}

#[test]
fn combinators_reject_when_the_iterable_is_not_iterable() {
    assert!(result_of(
        "var result = false; \
         Promise.all(1).catch(function (e) { result = e instanceof TypeError; });"
    ));
    assert!(result_of(
        "var result = false; \
         Promise.race(undefined).catch(function (e) { result = e instanceof TypeError; });"
    ));
}

#[test]
fn combinators_with_a_subclass_use_the_slow_path_and_its_capability() {
    assert!(result_of(
        "var result = false; var made = 0; \
         class Sub extends Promise { constructor(executor) { made++; super(executor); } } \
         var p = Sub.all([1, Sub.resolve(2)]); \
         p.then(function (v) { result = p instanceof Sub && v.join() === '1,2' && made > 0; });"
    ));
    assert!(result_of(
        "var result = false; \
         class Sub extends Promise {} \
         Sub.allSettled([Sub.reject(1)]).then(function (r) { result = r[0].status === 'rejected' && r[0].reason === 1; });"
    ));
    assert!(result_of(
        "var result = false; \
         class Sub extends Promise {} \
         Sub.any([Sub.reject(1), Sub.resolve(2)]).then(function (v) { result = v === 2; });"
    ));
}

#[test]
fn combinators_reject_with_a_type_error_when_promise_resolve_is_not_callable() {
    assert!(result_of(
        "var result = false; \
         function Fake(executor) { return new Promise(executor); } \
         Fake.resolve = 1; \
         Promise.all.call(Fake, [1]).catch(function (e) { result = e instanceof TypeError; });"
    ));
}

#[test]
fn with_resolvers_returns_the_capability_object_in_the_structure_order() {
    assert!(result_of(
        "var result = false; \
         var w = Promise.withResolvers(); \
         w.resolve(9); \
         w.promise.then(function (v) { result = v === 9 && Object.keys(w).join() === 'resolve,reject,promise'; });"
    ));
}

#[test]
fn try_runs_the_callback_synchronously_and_captures_its_throw() {
    assert!(result_of(
        "var result = false; var ran = false; \
         var p = Promise.try(function (a, b) { ran = true; return a + b; }, 1, 2); \
         p.then(function (v) { result = ran && v === 3; });"
    ));
    assert!(result_of(
        "var result = false; \
         Promise.try(function () { throw 6; }).catch(function (e) { result = e === 6; });"
    ));
}

#[test]
fn finally_passes_the_settlement_through_and_ignores_the_callback_value() {
    assert!(result_of(
        "var result = false; \
         Promise.resolve(5).finally(function () { return 99; }).then(function (v) { result = v === 5; });"
    ));
    assert!(result_of(
        "var result = false; \
         Promise.reject(7).finally(function () { return 99; }).catch(function (e) { result = e === 7; });"
    ));
}

#[test]
fn finally_rejects_with_the_callback_throw_and_waits_for_a_returned_promise() {
    assert!(result_of(
        "var result = false; \
         Promise.resolve(5).finally(function () { throw 8; }).catch(function (e) { result = e === 8; });"
    ));
    assert!(result_of(
        "var result = false; var order = []; \
         Promise.resolve(5).finally(function () { return Promise.resolve().then(function () { order.push('inner'); }); }) \
           .then(function (v) { order.push('outer'); result = v === 5 && order.join() === 'inner,outer'; });"
    ));
}

#[test]
fn finally_with_a_non_callable_argument_behaves_like_then_with_it_twice() {
    assert!(result_of(
        "var result = false; \
         Promise.resolve(1).finally(undefined).then(function (v) { result = v === 1; });"
    ));
}

#[test]
fn promise_resolved_with_itself_rejects_with_a_type_error() {
    assert!(result_of(
        "var result = false; var resolveIt; \
         var p = new Promise(function (resolve) { resolveIt = resolve; }); \
         resolveIt(p); \
         p.catch(function (e) { result = e instanceof TypeError && e.message === 'Cannot resolve a promise with itself'; });"
    ));
}

#[test]
fn resolve_function_of_the_executor_ignores_calls_after_the_first() {
    assert!(result_of(
        "var result = false; \
         new Promise(function (resolve, reject) { resolve(1); reject(2); resolve(3); }).then(function (v) { result = v === 1; });"
    ));
}

#[test]
fn reaction_order_follows_registration_order_for_a_pending_promise() {
    assert!(result_of(
        "var result = false; var log = []; var resolveIt; \
         var p = new Promise(function (resolve) { resolveIt = resolve; }); \
         p.then(function () { log.push(1); }); \
         p.then(function () { log.push(2); }); \
         p.finally(function () { log.push(3); }); \
         p.then(function () { log.push(4); result = log.join() === '1,2,3,4'; }); \
         resolveIt(0);"
    ));
}

#[test]
fn promise_constructor_requires_a_callable_executor() {
    assert!(result_of(
        "var result = false; \
         try { new Promise(1); } catch (e) { result = e instanceof TypeError; }"
    ));
}
