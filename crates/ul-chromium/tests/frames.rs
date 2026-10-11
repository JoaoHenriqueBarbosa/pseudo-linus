//! Documentos aninhados (`<iframe>`): o filho carrega com o seu próprio programa (realm) no mesmo laço de eventos, e
//! `contentWindow`, `contentDocument`, `parent`, `top`, `frameElement` e `frames` ligam os dois lados. As expectativas
//! vêm do Chromium 154 do oráculo (corpus `testbench/corpus/headless-storage/frames-basic.html`, página em
//! `http://127.0.0.1:8000`).

mod common;

use common::run;

const CHILD: &str = "<!doctype html><title>filho</title><body id=\"child-body\"><p id=\"p\">dentro</p>\
<script>parent.log('child-script ' + location.pathname + ' parent===window:' + (parent === window) + ' top===parent:' + (top === parent) + ' rs=' + document.readyState + ' frameElement=' + (frameElement && frameElement.id));\
window.addEventListener('load', () => parent.log('child-load'));\
document.addEventListener('DOMContentLoaded', () => parent.log('child-DOMContentLoaded'));</script>";

const PARENT: &str = "<!doctype html><title>pai</title><script>\
window.log = (m) => (window.entries = window.entries || []).push(m);\
log('parent-script-1 frames=' + frames.length);</script>\
<iframe id=\"fa\" src=\"/child.html\"></iframe>\
<iframe id=\"fb\" srcdoc=\"<title>sd</title><p id=q>x</p>\"></iframe>\
<script>log('parent-script-2 frames=' + frames.length + ' cw=' + (fa.contentWindow !== null) + ' cd=' + (fa.contentDocument !== null));\
window.addEventListener('load', () => { log('parent-load'); setTimeout(() => {\
const r = {};\
r.frames = frames.length;\
r.w0 = window[0] === fa.contentWindow;\
r.w1 = window[1] === fb.contentWindow;\
r.parentOfFrame = frames[0].parent === window;\
r.topOfFrame = frames[0].top === window;\
r.self = [window.top === window, window.parent === window, window.self === window, window.frames === window];\
r.href = fa.contentWindow.location.href;\
r.title = fa.contentDocument.title;\
r.srcdoc = [fb.contentDocument.title, fb.contentWindow.location.href];\
r.p = fa.contentDocument.getElementById('p').textContent;\
r.frameElement = frames[0].frameElement === fa;\
r.arrayRealms = frames[0].Array === Array;\
r.crossInstance = frames[0].eval('[]') instanceof Array;\
r.defaultView = fa.contentDocument.defaultView === fa.contentWindow;\
r.rs = [fa.contentDocument.readyState, fb.contentDocument.readyState];\
r.entries = window.entries;\
console.log(JSON.stringify(r)); }, 50); });</script>";

#[test]
fn an_iframe_with_src_and_srcdoc_loads_its_own_document() {
    let (_, log) = run(&[("/", PARENT), ("/child.html", CHILD)]);
    let result: serde_json::Value = serde_json::from_str(log.trim()).unwrap_or_else(|e| panic!("{e}: {log}"));
    assert_eq!(result["frames"], 2, "{log}");
    assert_eq!(result["w0"], true);
    assert_eq!(result["w1"], true);
    assert_eq!(result["parentOfFrame"], true);
    assert_eq!(result["topOfFrame"], true);
    assert_eq!(result["self"], serde_json::json!([true, true, true, true]));
    assert_eq!(result["href"], "http://t.test/child.html");
    assert_eq!(result["title"], "filho");
    assert_eq!(result["srcdoc"], serde_json::json!(["sd", "about:srcdoc"]));
    assert_eq!(result["p"], "dentro");
    assert_eq!(result["frameElement"], true);
    assert_eq!(result["arrayRealms"], false);
    assert_eq!(result["crossInstance"], false);
    assert_eq!(result["defaultView"], true);
    assert_eq!(result["rs"], serde_json::json!(["complete", "complete"]));
    let entries: Vec<&str> = result["entries"].as_array().unwrap().iter().map(|e| e.as_str().unwrap()).collect();
    // O filho existe já no segundo script do pai, e carrega (scripts, DOMContentLoaded, load) antes do `load` do pai.
    assert_eq!(entries[..2], ["parent-script-1 frames=0", "parent-script-2 frames=2 cw=true cd=true"]);
    assert_eq!(entries[2], "child-script /child.html parent===window:false top===parent:true rs=loading frameElement=fa");
    assert_eq!(entries[3..], ["child-DOMContentLoaded", "child-load", "parent-load"]);
}
