//! Gerenciamento explícito de recursos ponta a ponta (`JSDisposableStack`, `JSAsyncDisposableStack`,
//! `DisposableStackPrototype.js`, `AsyncDisposableStackPrototype.js`, `SuppressedError`, `Symbol.dispose`,
//! `Symbol.asyncDispose`, e os `using`/`await using` do `BytecodeGenerator::emitUsingBodyScope`), mais o
//! `WeakRef` e o `FinalizationRegistry`. Cada programa grava um booleano na global `result`, lida depois da
//! drenagem das microtasks. Os valores esperados (ordem de disposição, cadeia de `SuppressedError` com
//! `error` o mais novo e `suppressed` o anterior, mensagens) são os do C++ e dos builtins JS.
use zjsc::api::eval::evaluate_named_script_result;

/// Roda `source`, esvazia as microtasks e devolve a global `result` como booleano.
fn result_of(source: &str) -> bool {
    let value = evaluate_named_script_result(source, "explicit_resource_management.js", "result")
        .unwrap_or_else(|thrown| panic!("lançou exceção ({}): {source}", zjsc::api::eval::describe_exception(&thrown)));
    value.is_true()
}

#[test]
fn symbol_dispose_and_async_dispose_are_well_known_symbols() {
    assert!(result_of(
        r#"var result = typeof Symbol.dispose === 'symbol' && typeof Symbol.asyncDispose === 'symbol'
             && Symbol.dispose.description === 'Symbol.dispose'
             && Symbol.asyncDispose.description === 'Symbol.asyncDispose'
             && Symbol.dispose !== Symbol.asyncDispose;"#
    ));
}

#[test]
fn dispose_runs_in_reverse_order_and_is_idempotent() {
    assert!(result_of(
        r#"var log = [];
           var stack = new DisposableStack();
           stack.defer(function () { log.push('a'); });
           stack.defer(function () { log.push('b'); });
           stack.defer(function () { log.push('c'); });
           var first = stack.dispose();
           var second = stack.dispose();
           var result = first === undefined && second === undefined && log.join() === 'c,b,a' && stack.disposed === true;"#
    ));
}

#[test]
fn disposed_getter_reflects_the_state_and_checks_the_receiver() {
    assert!(result_of(
        r#"var stack = new DisposableStack();
           var before = stack.disposed;
           stack.dispose();
           var getter = Object.getOwnPropertyDescriptor(DisposableStack.prototype, 'disposed').get;
           var message = '';
           try { getter.call({}); } catch (e) { message = e instanceof TypeError ? e.message : 'wrong'; }
           var result = before === false && stack.disposed === true
             && message === 'DisposableStack.prototype.disposed getter requires that |this| be a DisposableStack object';"#
    ));
}

#[test]
fn async_disposed_getter_keeps_the_disposable_stack_wording_of_the_cpp() {
    assert!(result_of(
        r#"var getter = Object.getOwnPropertyDescriptor(AsyncDisposableStack.prototype, 'disposed').get;
           var message = '';
           try { getter.call({}); } catch (e) { message = e instanceof TypeError ? e.message : 'wrong'; }
           var result = message === 'AsyncDisposableStack.prototype.disposed getter requires that |this| be a DisposableStack object'
             && new AsyncDisposableStack().disposed === false;"#
    ));
}

#[test]
fn two_throwing_disposers_chain_a_suppressed_error_with_the_newest_as_error() {
    assert!(result_of(
        r#"var e1 = new Error('1'), e2 = new Error('2');
           var stack = new DisposableStack();
           stack.defer(function () { throw e1; });
           stack.defer(function () { throw e2; });
           var caught;
           try { stack.dispose(); } catch (e) { caught = e; }
           var result = caught instanceof SuppressedError && caught.error === e1 && caught.suppressed === e2
             && stack.disposed === true;"#
    ));
}

#[test]
fn three_throwing_disposers_nest_the_suppressed_errors() {
    assert!(result_of(
        r#"var a = 'a', b = 'b', c = 'c';
           var stack = new DisposableStack();
           stack.defer(function () { throw a; });
           stack.defer(function () { throw b; });
           stack.defer(function () { throw c; });
           var caught;
           try { stack.dispose(); } catch (e) { caught = e; }
           var result = caught instanceof SuppressedError && caught.error === a
             && caught.suppressed instanceof SuppressedError && caught.suppressed.error === b
             && caught.suppressed.suppressed === c;"#
    ));
}

#[test]
fn a_single_throwing_disposer_rethrows_the_original_value() {
    assert!(result_of(
        r#"var boom = { boom: true };
           var ran = [];
           var stack = new DisposableStack();
           stack.defer(function () { ran.push('first'); });
           stack.defer(function () { throw boom; });
           stack.defer(function () { ran.push('last'); });
           var caught;
           try { stack.dispose(); } catch (e) { caught = e; }
           var result = caught === boom && ran.join() === 'last,first';"#
    ));
}

#[test]
fn adopt_returns_the_value_and_calls_on_dispose_with_it() {
    assert!(result_of(
        r#"var seen = [];
           var stack = new DisposableStack();
           var value = { id: 7 };
           var returned = stack.adopt(value, function (v) { seen.push(v === value, arguments.length); });
           stack.dispose();
           var result = returned === value && seen.join() === 'true,1';"#
    ));
}

#[test]
fn defer_returns_undefined_and_use_returns_the_value() {
    assert!(result_of(
        r#"var log = [];
           var stack = new DisposableStack();
           var resource = {};
           resource[Symbol.dispose] = function () { log.push(this === resource); };
           var deferred = stack.defer(function () { log.push('deferred'); });
           var used = stack.use(resource);
           var nothing = stack.use(null);
           var alsoNothing = stack.use(undefined);
           stack.dispose();
           var result = deferred === undefined && used === resource && nothing === null && alsoNothing === undefined
             && log.join() === 'true,deferred';"#
    ));
}

#[test]
fn use_rejects_values_without_a_dispose_method() {
    assert!(result_of(
        r#"var stack = new DisposableStack();
           var messages = [];
           var attempt = function (value) {
             try { stack.use(value); messages.push('no throw'); }
             catch (e) { messages.push(e instanceof TypeError ? e.message : 'wrong'); }
           };
           attempt(1);
           attempt({});
           var notCallable = {}; notCallable[Symbol.dispose] = 1;
           attempt(notCallable);
           var result = messages[0] === 'Disposable value must be an object'
             && messages[1] === '@@dispose must not be undefined or null'
             && messages[2] === '@@dispose must be callable';"#
    ));
}

#[test]
fn adopt_and_defer_validate_the_callback() {
    assert!(result_of(
        r#"var stack = new DisposableStack();
           var messages = [];
           try { stack.adopt(1, 2); } catch (e) { messages.push(e instanceof TypeError ? e.message : 'wrong'); }
           try { stack.defer(2); } catch (e) { messages.push(e instanceof TypeError ? e.message : 'wrong'); }
           var result = messages[0] === 'DisposableStack.prototype.adopt requires that onDispose argument be a callable'
             && messages[1] === 'DisposableStack.prototype.defer requires that onDispose argument be a callable';"#
    ));
}

#[test]
fn methods_reject_a_receiver_that_is_not_a_disposable_stack() {
    assert!(result_of(
        r#"var names = ['adopt', 'defer', 'dispose', 'move', 'use'];
           var ok = true;
           for (var i = 0; i < names.length; i++) {
             var name = names[i];
             try { DisposableStack.prototype[name].call({}, 1, function () {}); ok = false; }
             catch (e) {
               ok = ok && e instanceof TypeError
                 && e.message === 'DisposableStack.prototype.' + name + ' requires that |this| be a DisposableStack object';
             }
           }
           var result = ok;"#
    ));
}

#[test]
fn a_disposed_stack_throws_reference_error_on_adopt_defer_use_and_move() {
    assert!(result_of(
        r#"var stack = new DisposableStack();
           stack.dispose();
           var names = ['adopt', 'defer', 'move', 'use'];
           var ok = true;
           for (var i = 0; i < names.length; i++) {
             var name = names[i];
             try { stack[name]({}, function () {}); ok = false; }
             catch (e) {
               ok = ok && e instanceof ReferenceError
                 && e.message === 'DisposableStack.prototype.' + name + ' requires that |this| be a pending DisposableStack object';
             }
           }
           var result = ok;"#
    ));
}

#[test]
fn move_transfers_the_resources_and_disposes_the_source() {
    assert!(result_of(
        r#"var log = [];
           var stack = new DisposableStack();
           stack.defer(function () { log.push('x'); });
           stack.defer(function () { log.push('y'); });
           var moved = stack.move();
           var states = [stack.disposed, moved.disposed];
           stack.dispose();
           var afterSource = log.length;
           moved.dispose();
           var result = moved !== stack && moved instanceof DisposableStack
             && states.join() === 'true,false' && afterSource === 0 && log.join() === 'y,x';"#
    ));
}

#[test]
fn builtin_function_names_lengths_and_aliases() {
    assert!(result_of(
        r#"var proto = DisposableStack.prototype;
           var asyncProto = AsyncDisposableStack.prototype;
           var result = proto.defer.name === 'defer' && proto.defer.length === 1
             && proto.adopt.length === 2 && proto.use.length === 1 && proto.move.length === 0 && proto.dispose.length === 0
             && proto[Symbol.dispose] === proto.dispose
             && asyncProto[Symbol.asyncDispose] === asyncProto.disposeAsync
             && asyncProto.disposeAsync.name === 'disposeAsync' && asyncProto.defer.name === 'defer'
             && proto[Symbol.toStringTag] === 'DisposableStack'
             && asyncProto[Symbol.toStringTag] === 'AsyncDisposableStack'
             && Object.getOwnPropertyDescriptor(proto, 'dispose').enumerable === false
             && Object.getOwnPropertyDescriptor(proto, 'disposed').set === undefined;"#
    ));
}

#[test]
fn constructors_require_new_and_have_length_zero() {
    assert!(result_of(
        r#"var messages = [];
           try { DisposableStack(); } catch (e) { messages.push(e instanceof TypeError ? e.message : 'wrong'); }
           try { AsyncDisposableStack(); } catch (e) { messages.push(e instanceof TypeError ? e.message : 'wrong'); }
           var result = messages[0] === 'calling DisposableStack constructor without new is invalid'
             && messages[1] === 'calling AsyncDisposableStack constructor without new is invalid'
             && DisposableStack.length === 0 && AsyncDisposableStack.length === 0
             && DisposableStack.name === 'DisposableStack' && AsyncDisposableStack.name === 'AsyncDisposableStack'
             && DisposableStack.prototype.constructor === DisposableStack
             && Object.getOwnPropertyDescriptor(DisposableStack, 'prototype').writable === false;"#
    ));
}

#[test]
fn subclasses_get_their_own_prototype() {
    assert!(result_of(
        r#"class Mine extends DisposableStack {}
           var instance = new Mine();
           var result = Object.getPrototypeOf(instance) === Mine.prototype && instance instanceof DisposableStack
             && instance.disposed === false;"#
    ));
}

#[test]
fn suppressed_error_properties_and_shape() {
    assert!(result_of(
        r#"var error = new Error('inner'), suppressed = new Error('outer');
           var instance = new SuppressedError(error, suppressed, 'msg');
           var errorDescriptor = Object.getOwnPropertyDescriptor(instance, 'error');
           var suppressedDescriptor = Object.getOwnPropertyDescriptor(instance, 'suppressed');
           var messageDescriptor = Object.getOwnPropertyDescriptor(instance, 'message');
           var result = instance instanceof SuppressedError && instance instanceof Error
             && instance.error === error && instance.suppressed === suppressed && instance.message === 'msg'
             && errorDescriptor.enumerable === false && suppressedDescriptor.enumerable === false
             && errorDescriptor.writable === true && errorDescriptor.configurable === true
             && messageDescriptor.enumerable === false
             && Object.prototype.hasOwnProperty.call(instance, 'cause') === false
             && SuppressedError.length === 3 && SuppressedError.name === 'SuppressedError'
             && Object.getPrototypeOf(SuppressedError) === Error
             && Object.getPrototypeOf(SuppressedError.prototype) === Error.prototype
             && SuppressedError.prototype.name === 'SuppressedError' && SuppressedError.prototype.message === ''
             && SuppressedError.prototype.constructor === SuppressedError
             && Object.prototype.toString.call(instance) === '[object Error]'
             && String(instance) === 'SuppressedError: msg';"#
    ));
}

#[test]
fn suppressed_error_without_message_has_no_own_message_and_works_without_new() {
    assert!(result_of(
        r#"var withoutNew = SuppressedError(1, 2);
           var result = withoutNew instanceof SuppressedError
             && Object.prototype.hasOwnProperty.call(withoutNew, 'message') === false
             && withoutNew.error === 1 && withoutNew.suppressed === 2
             && Object.prototype.hasOwnProperty.call(new SuppressedError(), 'error')
             && new SuppressedError().error === undefined
             && Object.keys(new SuppressedError(1, 2, 'm')).length === 0;"#
    ));
}

#[test]
fn suppressed_error_converts_the_message_and_propagates_its_failure() {
    assert!(result_of(
        r#"var coerced = new SuppressedError(1, 2, 42).message === '42';
           var threw = false;
           try { new SuppressedError(1, 2, Symbol()); } catch (e) { threw = e instanceof TypeError; }
           class Mine extends SuppressedError {}
           var mine = new Mine(1, 2, 'x');
           var result = coerced && threw && Object.getPrototypeOf(mine) === Mine.prototype && mine.error === 1;"#
    ));
}

#[test]
fn using_disposes_in_reverse_order_after_the_body() {
    assert!(result_of(
        r#"var log = [];
           function make(name) { var o = {}; o[Symbol.dispose] = function () { log.push(name); }; return o; }
           function run() {
             using a = make('a');
             using b = make('b');
             log.push('body');
             return 'done';
           }
           var returned = run();
           var result = returned === 'done' && log.join() === 'body,b,a';"#
    ));
}

#[test]
fn using_skips_null_and_undefined_values() {
    assert!(result_of(
        r#"var log = [];
           function run() {
             using a = null;
             using b = undefined;
             log.push('body');
           }
           run();
           var result = log.join() === 'body';"#
    ));
}

#[test]
fn using_a_throwing_disposer_with_a_throwing_body_makes_a_suppressed_error() {
    assert!(result_of(
        r#"function disposable(fn) { var o = {}; o[Symbol.dispose] = fn; return o; }
           function run() {
             using a = disposable(function () { throw 'dispose'; });
             throw 'body';
           }
           var caught;
           try { run(); } catch (e) { caught = e; }
           var result = caught instanceof SuppressedError && caught.error === 'dispose' && caught.suppressed === 'body';"#
    ));
}

#[test]
fn using_chains_the_errors_of_every_disposer_in_disposal_order() {
    assert!(result_of(
        r#"function disposable(fn) { var o = {}; o[Symbol.dispose] = fn; return o; }
           function run() {
             using a = disposable(function () { throw 'a'; });
             using b = disposable(function () { throw 'b'; });
             return 1;
           }
           var caught;
           try { run(); } catch (e) { caught = e; }
           var result = caught instanceof SuppressedError && caught.error === 'a' && caught.suppressed === 'b';"#
    ));
}

#[test]
fn using_still_disposes_the_earlier_resources_when_a_later_initializer_throws() {
    assert!(result_of(
        r#"var log = [];
           function make(name) { var o = {}; o[Symbol.dispose] = function () { log.push(name); }; return o; }
           function run() {
             using a = make('a');
             using b = (function () { throw 'init'; })();
             log.push('unreachable');
           }
           var caught;
           try { run(); } catch (e) { caught = e; }
           var result = caught === 'init' && log.join() === 'a';"#
    ));
}

#[test]
fn using_a_value_without_dispose_throws_type_error() {
    assert!(result_of(
        r#"var messages = [];
           try { (function () { using x = 1; })(); } catch (e) { messages.push(e instanceof TypeError ? e.message : 'wrong'); }
           try { (function () { using x = {}; })(); } catch (e) { messages.push(e instanceof TypeError ? e.message : 'wrong'); }
           var result = messages[0] === 'Disposable value must be an object, null, or undefined'
             && messages[1] === '@@dispose must not be undefined or null';"#
    ));
}

#[test]
fn using_in_a_block_loop_and_switch_disposes_at_the_end_of_each_scope() {
    assert!(result_of(
        r#"var log = [];
           function make(name) { var o = {}; o[Symbol.dispose] = function () { log.push(name); }; return o; }
           function run() {
             { using a = make('block'); log.push('in block'); }
             for (var i = 0; i < 2; i++) { using b = make('loop' + i); log.push('iter' + i); }
             switch (1) { case 1: using c = make('switch'); log.push('in switch'); }
             for (using d of [make('of0'), make('of1')]) { log.push('body-of'); }
           }
           run();
           var result = log.join() === 'in block,block,iter0,loop0,iter1,loop1,in switch,switch,body-of,of0,body-of,of1';"#
    ));
}

#[test]
fn using_disposes_on_break_continue_and_return() {
    assert!(result_of(
        r#"var log = [];
           function make(name) { var o = {}; o[Symbol.dispose] = function () { log.push(name); }; return o; }
           function run() {
             for (var i = 0; i < 3; i++) {
               using a = make('a' + i);
               if (i === 0) continue;
               if (i === 1) break;
             }
             using z = make('z');
             return 'r';
           }
           var returned = run();
           var result = returned === 'r' && log.join() === 'a0,a1,z';"#
    ));
}

#[test]
fn async_dispose_runs_in_reverse_order_and_resolves_undefined() {
    assert!(result_of(
        r#"var result = false;
           var log = [];
           var stack = new AsyncDisposableStack();
           stack.defer(async function () { log.push('a'); });
           var resource = {};
           resource[Symbol.asyncDispose] = function () { log.push('b'); return Promise.resolve(); };
           stack.use(resource);
           stack.adopt(5, async function (v) { log.push('c' + v); });
           var promise = stack.disposeAsync();
           var pending = stack.disposed;
           promise.then(function (v) {
             result = promise instanceof Promise && pending === true && v === undefined && log.join() === 'c5,b,a';
           });"#
    ));
}

#[test]
fn async_dispose_falls_back_to_the_sync_dispose_method() {
    assert!(result_of(
        r#"var result = false;
           var log = [];
           var stack = new AsyncDisposableStack();
           var resource = {};
           resource[Symbol.dispose] = function () { log.push(this === resource); };
           stack.use(resource);
           stack.disposeAsync().then(function () { result = log.join() === 'true'; });"#
    ));
}

#[test]
fn async_dispose_chains_suppressed_errors_across_sync_and_async_failures() {
    assert!(result_of(
        r#"var result = false;
           var e1 = new Error('1'), e2 = new Error('2');
           var stack = new AsyncDisposableStack();
           stack.defer(function () { throw e1; });
           stack.defer(function () { return Promise.reject(e2); });
           stack.disposeAsync().then(function () {}, function (e) {
             result = e instanceof SuppressedError && e.error === e1 && e.suppressed === e2;
           });"#
    ));
}

#[test]
fn async_dispose_is_idempotent_and_rejects_a_wrong_receiver_through_the_promise() {
    assert!(result_of(
        r#"var result = false;
           var stack = new AsyncDisposableStack();
           var first = stack.disposeAsync();
           var second = stack.disposeAsync();
           var wrong = AsyncDisposableStack.prototype.disposeAsync.call({});
           var outcome = [];
           first.then(function (v) { outcome.push(v === undefined); });
           second.then(function (v) { outcome.push(v === undefined); });
           wrong.catch(function (e) {
             outcome.push(e instanceof TypeError
               && e.message === 'AsyncDisposableStack.prototype.disposeAsync requires that |this| be a AsyncDisposableStack object');
             result = outcome.join() === 'true,true,true' && first !== second;
           });"#
    ));
}

#[test]
fn async_move_transfers_the_resources() {
    assert!(result_of(
        r#"var result = false;
           var log = [];
           var stack = new AsyncDisposableStack();
           stack.defer(function () { log.push('moved'); });
           var moved = stack.move();
           var states = [stack.disposed, moved.disposed];
           stack.disposeAsync().then(function () {
             var before = log.length;
             return moved.disposeAsync().then(function () {
               result = states.join() === 'true,false' && before === 0 && log.join() === 'moved' && moved instanceof AsyncDisposableStack;
             });
           });"#
    ));
}

#[test]
fn await_using_awaits_each_disposer_in_reverse_order() {
    assert!(result_of(
        r#"var result = false;
           var log = [];
           function make(name) {
             var o = {};
             o[Symbol.asyncDispose] = function () { log.push(name + ':start'); return Promise.resolve().then(function () { log.push(name + ':end'); }); };
             return o;
           }
           async function run() {
             await using a = make('a');
             await using b = make('b');
             await using c = null;
             log.push('body');
             return 1;
           }
           run().then(function (v) {
             result = v === 1 && log.join() === 'body,b:start,b:end,a:start,a:end';
           });"#
    ));
}

#[test]
fn await_using_chains_a_rejected_disposer_with_the_body_error() {
    assert!(result_of(
        r#"var result = false;
           function disposable(fn) { var o = {}; o[Symbol.asyncDispose] = fn; return o; }
           async function run() {
             await using a = disposable(function () { return Promise.reject('dispose'); });
             throw 'body';
           }
           run().then(function () {}, function (e) {
             result = e instanceof SuppressedError && e.error === 'dispose' && e.suppressed === 'body';
           });"#
    ));
}

#[test]
fn await_using_mixed_with_using_disposes_sync_resources_in_order() {
    assert!(result_of(
        r#"var result = false;
           var log = [];
           function syncResource(name) { var o = {}; o[Symbol.dispose] = function () { log.push(name); }; return o; }
           function asyncResource(name) {
             var o = {};
             o[Symbol.asyncDispose] = function () { log.push(name); return Promise.resolve(); };
             return o;
           }
           async function run() {
             using a = syncResource('a');
             await using b = asyncResource('b');
             using c = syncResource('c');
             log.push('body');
           }
           run().then(function () { result = log.join() === 'body,c,b,a'; });"#
    ));
}

#[test]
fn weak_ref_derefs_its_target_and_validates_the_argument() {
    assert!(result_of(
        r#"var target = {};
           var reference = new WeakRef(target);
           var symbolRef = new WeakRef(Symbol('s'));
           var messages = [];
           try { new WeakRef(1); } catch (e) { messages.push(e instanceof TypeError ? e.message : 'wrong'); }
           try { new WeakRef(Symbol.for('registered')); } catch (e) { messages.push(e instanceof TypeError ? e.message : 'wrong'); }
           try { WeakRef({}); } catch (e) { messages.push(e instanceof TypeError ? e.message : 'wrong'); }
           try { WeakRef.prototype.deref.call({}); } catch (e) { messages.push(e instanceof TypeError ? e.message : 'wrong'); }
           try { WeakRef.prototype.deref.call(1); } catch (e) { messages.push(e instanceof TypeError ? e.message : 'wrong'); }
           var result = reference.deref() === target && typeof symbolRef.deref() === 'symbol'
             && messages[0] === 'First argument to WeakRef should be an object or a non-registered symbol'
             && messages[1] === messages[0]
             && messages[2] === 'calling WeakRef constructor without new is invalid'
             && messages[3] === 'Called WeakRef function on a non-WeakRef object'
             && messages[4] === 'Called WeakRef function on non-object'
             && WeakRef.length === 1 && WeakRef.prototype.deref.length === 0
             && WeakRef.prototype[Symbol.toStringTag] === 'WeakRef';"#
    ));
}

#[test]
fn finalization_registry_register_and_unregister() {
    assert!(result_of(
        r#"var registry = new FinalizationRegistry(function () {});
           var target = {}, token = {};
           var registered = registry.register(target, 'held', token);
           var noToken = registry.register({}, 'held2');
           var first = registry.unregister(token);
           var second = registry.unregister(token);
           var result = registered === undefined && noToken === undefined && first === true && second === false
             && registry.register.length === 2 && registry.unregister.length === 1
             && FinalizationRegistry.length === 1
             && FinalizationRegistry.prototype[Symbol.toStringTag] === 'FinalizationRegistry'
             && typeof registry.cleanupSome === 'undefined';"#
    ));
}

#[test]
fn finalization_registry_validates_its_arguments() {
    assert!(result_of(
        r#"var registry = new FinalizationRegistry(function () {});
           var messages = [];
           var attempt = function (fn) {
             try { fn(); messages.push('no throw'); }
             catch (e) { messages.push(e instanceof TypeError ? e.message : 'wrong'); }
           };
           attempt(function () { new FinalizationRegistry(1); });
           attempt(function () { FinalizationRegistry(function () {}); });
           attempt(function () { registry.register(1, 'x'); });
           var same = {};
           attempt(function () { registry.register(same, same); });
           attempt(function () { registry.register({}, 'x', 1); });
           attempt(function () { registry.unregister(1); });
           attempt(function () { FinalizationRegistry.prototype.register.call({}, {}, 'x'); });
           attempt(function () { FinalizationRegistry.prototype.register.call(1, {}, 'x'); });
           var result = messages[0] === 'First argument to FinalizationRegistry should be a function'
             && messages[1] === 'calling FinalizationRegistry constructor without new is invalid'
             && messages[2] === 'register requires an object or a non-registered symbol as the target'
             && messages[3] === 'register expects the target object and the holdings parameter are not the same. Otherwise, the target can never be collected'
             && messages[4] === 'register requires an object or a non-registered symbol as the unregistration token'
             && messages[5] === 'unregister requires an object or a non-registered symbol as the unregistration token'
             && messages[6] === 'Called FinalizationRegistry function on a non-FinalizationRegistry object'
             && messages[7] === 'Called FinalizationRegistry function on non-object';"#
    ));
}
