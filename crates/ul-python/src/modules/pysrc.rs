//! Módulos da stdlib escritos em Python, embutidos no binário e executados pela própria VM.
//!
//! Cada fonte vive em `modules/py/<nome>.py`. O módulo roda uma vez, numa `Vm` com globais próprias
//! (as funções definidas nele guardam essas globais em `FuncObj::globals`), e os nomes globais
//! resultantes viram os atributos do `ModuleObj`.
//!
//! Para acrescentar um módulo: crie o `.py` e registre o par `(nome, include_str!)` em [`SOURCES`].

use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap};
use std::rc::Rc;

use crate::object::{intern, ModuleObj, Value};
use crate::vm::Vm;

const SOURCES: &[(&str, &str)] = &[
    ("sys", include_str!("py/sys.py")),
    ("posixpath", include_str!("py/posixpath.py")),
    ("os", include_str!("py/os.py")),
    ("io", include_str!("py/io.py")),
    ("itertools", include_str!("py/itertools.py")),
    ("functools", include_str!("py/functools.py")),
    ("contextlib", include_str!("py/contextlib.py")),
    ("abc", include_str!("py/abc.py")),
    ("collections", include_str!("py/collections.py")),
    ("enum", include_str!("py/enum.py")),
    ("collections.abc", include_str!("py/collections_abc.py")),
    ("typing", include_str!("py/typing.py")),
    ("copy", include_str!("py/copy.py")),
    ("dataclasses", include_str!("py/dataclasses.py")),
    ("time", include_str!("py/time.py")),
    ("datetime", include_str!("py/datetime.py")),
    ("random", include_str!("py/random.py")),
    ("bisect", include_str!("py/bisect.py")),
    ("stat", include_str!("py/stat.py")),
    ("glob", include_str!("py/glob.py")),
    ("shutil", include_str!("py/shutil.py")),
    ("tempfile", include_str!("py/tempfile.py")),
    ("pathlib", include_str!("py/pathlib.py")),
    ("zlib", include_str!("py/zlib.py")),
    ("gzip", include_str!("py/gzip.py")),
    ("argparse", include_str!("py/argparse.py")),
    ("heapq", include_str!("py/heapq.py")),
    ("types", include_str!("py/types.py")),
    ("weakref", include_str!("py/weakref.py")),
    ("threading", include_str!("py/threading.py")),
    ("logging", include_str!("py/logging.py")),
    ("atexit", include_str!("py/atexit.py")),
    ("string", include_str!("py/string.py")),
    ("_string", include_str!("py/_string.py")),
    ("pyexpat", include_str!("py/pyexpat.py")),
    ("xml", include_str!("py/xml.py")),
    ("xml.etree", include_str!("py/xml_etree.py")),
    ("xml.etree.ElementPath", include_str!("py/xml_etree_ElementPath.py")),
    ("xml.etree.ElementTree", include_str!("py/xml_etree_ElementTree.py")),
    ("xml.parsers", include_str!("py/xml_parsers.py")),
    ("xml.parsers.expat", include_str!("py/xml_parsers_expat.py")),
    ("xml.dom", include_str!("py/xml_dom.py")),
    ("xml.dom.NodeFilter", include_str!("py/xml_dom_NodeFilter.py")),
    ("xml.dom.domreg", include_str!("py/xml_dom_domreg.py")),
    ("xml.dom.expatbuilder", include_str!("py/xml_dom_expatbuilder.py")),
    ("xml.dom.minicompat", include_str!("py/xml_dom_minicompat.py")),
    ("xml.dom.minidom", include_str!("py/xml_dom_minidom.py")),
    ("xml.dom.pulldom", include_str!("py/xml_dom_pulldom.py")),
    ("xml.dom.xmlbuilder", include_str!("py/xml_dom_xmlbuilder.py")),
    ("xml.sax", include_str!("py/xml_sax.py")),
    ("xml.sax._exceptions", include_str!("py/xml_sax__exceptions.py")),
    ("xml.sax.expatreader", include_str!("py/xml_sax_expatreader.py")),
    ("xml.sax.handler", include_str!("py/xml_sax_handler.py")),
    ("xml.sax.saxutils", include_str!("py/xml_sax_saxutils.py")),
    ("xml.sax.xmlreader", include_str!("py/xml_sax_xmlreader.py")),
    ("concurrent", include_str!("py/concurrent.py")),
    ("concurrent.futures", include_str!("py/concurrent_futures.py")),
    ("_thread", include_str!("py/_thread.py")),
    ("colorsys", include_str!("py/colorsys.py")),
    ("keyword", include_str!("py/keyword.py")),
    ("graphlib", include_str!("py/graphlib.py")),
    ("reprlib", include_str!("py/reprlib.py")),
    ("getopt", include_str!("py/getopt.py")),
    ("difflib", include_str!("py/difflib.py")),
    ("zipfile", include_str!("py/zipfile.py")),
    ("traceback", include_str!("py/traceback.py")),
    ("warnings", include_str!("py/warnings.py")),
    ("subprocess", include_str!("py/subprocess.py")),
    ("_complex", include_str!("py/_complex.py")),
    ("_match", include_str!("py/_match.py")),
    ("_memoryview", include_str!("py/_memoryview.py")),
    ("operator", include_str!("py/operator.py")),
    ("json", include_str!("py/json.py")),
    ("json.decoder", include_str!("py/json_decoder.py")),
    ("json.encoder", include_str!("py/json_encoder.py")),
    ("json.tool", include_str!("py/json_tool.py")),
    ("json.scanner", include_str!("py/json_scanner.py")),
    ("configparser", include_str!("py/configparser.py")),
    ("queue", include_str!("py/queue.py")),
    ("calendar", include_str!("py/calendar.py")),
    ("uuid", include_str!("py/uuid.py")),
    ("secrets", include_str!("py/secrets.py")),
    ("hmac", include_str!("py/hmac.py")),
    ("numbers", include_str!("py/numbers.py")),
    ("contextvars", include_str!("py/contextvars.py")),
    ("decimal", include_str!("py/decimal.py")),
    ("fractions", include_str!("py/fractions.py")),
    ("statistics", include_str!("py/statistics.py")),
    ("pprint", include_str!("py/pprint.py")),
    ("locale", include_str!("py/locale.py")),
    ("urllib", include_str!("py/urllib.py")),
    ("urllib.parse", include_str!("py/urllib_parse.py")),
    ("urllib.error", include_str!("py/urllib_error.py")),
    ("urllib.response", include_str!("py/urllib_response.py")),
    ("urllib.request", include_str!("py/urllib_request.py")),
    ("_csv", include_str!("py/_csv.py")),
    ("csv", include_str!("py/csv.py")),
    ("importlib", include_str!("py/importlib.py")),
    ("importlib.machinery", include_str!("py/importlib_machinery.py")),
    ("importlib.util", include_str!("py/importlib_util.py")),
    ("asyncio", include_str!("py/asyncio.py")),
    ("asyncio.base_events", include_str!("py/asyncio_base_events.py")),
    ("asyncio.base_futures", include_str!("py/asyncio_base_futures.py")),
    ("asyncio.base_tasks", include_str!("py/asyncio_base_tasks.py")),
    ("asyncio.constants", include_str!("py/asyncio_constants.py")),
    ("asyncio.coroutines", include_str!("py/asyncio_coroutines.py")),
    ("asyncio.events", include_str!("py/asyncio_events.py")),
    ("asyncio.exceptions", include_str!("py/asyncio_exceptions.py")),
    ("asyncio.format_helpers", include_str!("py/asyncio_format_helpers.py")),
    ("asyncio.futures", include_str!("py/asyncio_futures.py")),
    ("asyncio.locks", include_str!("py/asyncio_locks.py")),
    ("asyncio.log", include_str!("py/asyncio_log.py")),
    ("asyncio.mixins", include_str!("py/asyncio_mixins.py")),
    ("asyncio.queues", include_str!("py/asyncio_queues.py")),
    ("asyncio.runners", include_str!("py/asyncio_runners.py")),
    ("asyncio.taskgroups", include_str!("py/asyncio_taskgroups.py")),
    ("asyncio.tasks", include_str!("py/asyncio_tasks.py")),
    ("asyncio.threads", include_str!("py/asyncio_threads.py")),
    ("asyncio.timeouts", include_str!("py/asyncio_timeouts.py")),
    ("asyncio.unix_events", include_str!("py/asyncio_unix_events.py")),
    ("inspect", include_str!("py/inspect.py")),
    ("linecache", include_str!("py/linecache.py")),
    ("_excgroup", include_str!("py/_excgroup.py")),
    ("pkgutil", include_str!("py/pkgutil.py")),
    ("errno", include_str!("py/errno.py")),
    ("tarfile", include_str!("py/tarfile.py")),
    ("_archivefile", include_str!("py/_archivefile.py")),
    ("sqlite3", include_str!("py/sqlite3.py")),
    ("sqlite3.dbapi2", include_str!("py/sqlite3_dbapi2.py")),
    ("sqlite3.dump", include_str!("py/sqlite3_dump.py")),
    ("sysconfig", include_str!("py/sysconfig.py")),
    ("_colorize", include_str!("py/_colorize.py")),
    ("doctest", include_str!("py/doctest.py")),
    ("pdb", include_str!("py/pdb.py")),
    ("pwd", include_str!("py/pwd.py")),
    ("grp", include_str!("py/grp.py")),
    ("wave", include_str!("py/wave.py")),
    ("webbrowser", include_str!("py/webbrowser.py")),
    ("importlib.metadata", include_str!("py/importlib_metadata.py")),
    ("importlib.resources", include_str!("py/importlib_resources.py")),
    ("ssl", include_str!("py/ssl.py")),
    ("select", include_str!("py/select.py")),
    ("selectors", include_str!("py/selectors.py")),
    ("socketserver", include_str!("py/socketserver.py")),
    ("http.cookiejar", include_str!("py/http_cookiejar.py")),
    ("http.server", include_str!("py/http_server.py")),
    ("http.client", include_str!("py/http_client.py")),
    ("socket", include_str!("py/socket.py")),
    ("http.cookies", include_str!("py/http_cookies.py")),
    ("email.mime.message", include_str!("py/email_mime_message.py")),
    ("email.mime.image", include_str!("py/email_mime_image.py")),
    ("email.mime.audio", include_str!("py/email_mime_audio.py")),
    ("email.mime.application", include_str!("py/email_mime_application.py")),
    ("email.mime.nonmultipart", include_str!("py/email_mime_nonmultipart.py")),
    ("email.mime", include_str!("py/email_mime.py")),
    ("email._header_value_parser", include_str!("py/email__header_value_parser.py")),
    ("email.contentmanager", include_str!("py/email_contentmanager.py")),
    ("email.quoprimime", include_str!("py/email_quoprimime.py")),
    ("email.base64mime", include_str!("py/email_base64mime.py")),
    ("email._encoded_words", include_str!("py/email__encoded_words.py")),
    ("email._policybase", include_str!("py/email__policybase.py")),
    ("email._parseaddr", include_str!("py/email__parseaddr.py")),
    ("email.iterators", include_str!("py/email_iterators.py")),
    ("email.headerregistry", include_str!("py/email_headerregistry.py")),
    ("email.header", include_str!("py/email_header.py")),
    ("email.generator", include_str!("py/email_generator.py")),
    ("email.feedparser", include_str!("py/email_feedparser.py")),
    ("email.encoders", include_str!("py/email_encoders.py")),
    ("email.charset", include_str!("py/email_charset.py")),
    ("email.errors", include_str!("py/email_errors.py")),
    ("logging.handlers", include_str!("py/logging_handlers.py")),
    ("email.mime.base", include_str!("py/email_mime_base.py")),
    ("email.mime.multipart", include_str!("py/email_mime_multipart.py")),
    ("email.mime.text", include_str!("py/email_mime_text.py")),
    ("email.utils", include_str!("py/email_utils.py")),
    ("email.policy", include_str!("py/email_policy.py")),
    ("email.parser", include_str!("py/email_parser.py")),
    ("email.message", include_str!("py/email_message.py")),
    ("email", include_str!("py/email.py")),
    ("http", include_str!("py/http.py")),
    ("cmath", include_str!("py/cmath.py")),
    ("_tokenize", include_str!("py/_tokenize.py")),
    ("token", include_str!("py/token.py")),
    ("tokenize", include_str!("py/tokenize.py")),
    ("runpy", include_str!("py/runpy.py")),
    ("dbm.sqlite3", include_str!("py/dbm_sqlite3.py")),
    ("dbm.dumb", include_str!("py/dbm_dumb.py")),
    ("dbm", include_str!("py/dbm.py")),
    ("shelve", include_str!("py/shelve.py")),
    ("code", include_str!("py/code.py")),
    ("codeop", include_str!("py/codeop.py")),
    ("plistlib", include_str!("py/plistlib.py")),
    ("quopri", include_str!("py/quopri.py")),
    ("html.entities", include_str!("py/html_entities.py")),
    ("__future__", include_str!("py/__future__.py")),
    ("_compat_pickle", include_str!("py/_compat_pickle.py")),
    ("fileinput", include_str!("py/fileinput.py")),
    ("_markupbase", include_str!("py/_markupbase.py")),
    ("html.parser", include_str!("py/html_parser.py")),
    ("mimetypes", include_str!("py/mimetypes.py")),
    ("ipaddress", include_str!("py/ipaddress.py")),
    ("tomllib._types", include_str!("py/tomllib__types.py")),
    ("tomllib._re", include_str!("py/tomllib__re.py")),
    ("tomllib._parser", include_str!("py/tomllib__parser.py")),
    ("tomllib", include_str!("py/tomllib.py")),
    ("pickle", include_str!("py/pickle.py")),
    ("copyreg", include_str!("py/copyreg.py")),
    ("_ast", include_str!("py/_ast.py")),
    ("ast", include_str!("py/ast.py")),
    ("zoneinfo", include_str!("py/zoneinfo.py")),
    ("zoneinfo._tzpath", include_str!("py/zoneinfo_tzpath.py")),
    ("zoneinfo._common", include_str!("py/zoneinfo_common.py")),
    ("zoneinfo._zoneinfo", include_str!("py/zoneinfo_zoneinfo.py")),
    ("bz2", include_str!("py/bz2.py")),
    ("lzma", include_str!("py/lzma.py")),
    ("array", include_str!("py/array.py")),
    ("codecs", include_str!("py/codecs.py")),
    ("gc", include_str!("py/gc.py")),
    ("getpass", include_str!("py/getpass.py")),
    ("platform", include_str!("py/platform.py")),
    ("optparse", include_str!("py/optparse.py")),
    ("sched", include_str!("py/sched.py")),
    ("cmd", include_str!("py/cmd.py")),
    ("filecmp", include_str!("py/filecmp.py")),
    ("gettext", include_str!("py/gettext.py")),
    ("timeit", include_str!("py/timeit.py")),
    ("signal", include_str!("py/signal.py")),
    ("unittest", include_str!("py/unittest.py")),
    ("unittest.__main__", include_str!("py/unittest___main__.py")),
    ("unittest.mock", include_str!("py/unittest_mock.py")),
    ("unittest._log", include_str!("py/unittest__log.py")),
    ("unittest.case", include_str!("py/unittest_case.py")),
    ("unittest.loader", include_str!("py/unittest_loader.py")),
    ("unittest.main", include_str!("py/unittest_main.py")),
    ("unittest.result", include_str!("py/unittest_result.py")),
    ("unittest.runner", include_str!("py/unittest_runner.py")),
    ("unittest.signals", include_str!("py/unittest_signals.py")),
    ("unittest.suite", include_str!("py/unittest_suite.py")),
    ("unittest.util", include_str!("py/unittest_util.py")),
];

/// Nomes de módulo que são apelidos de outro.
fn alias(name: &str) -> &str {
    match name {
        "os.path" => "posixpath",
        other => other,
    }
}

/// O módulo em Python embutido chamado `name`, construído na primeira vez.
pub fn import(vm: &mut Vm, name: &str) -> Option<Rc<ModuleObj>> {
    let real = alias(name);
    let (_, src) = SOURCES.iter().find(|(n, _)| *n == real)?;
    // Como o CPython, `import pacote.sub` importa `pacote` antes de `sub`.
    if let Some((parent, _)) = real.rsplit_once('.') {
        crate::modules::import(vm, parent);
    }
    if let Some(m) = vm.modules.borrow().get(real) {
        return Some(m.clone());
    }
    let module = Rc::new(ModuleObj { name: intern(real), attrs: RefCell::new(BTreeMap::new()) });
    // Registrado antes de rodar, para que importações circulares enxerguem o módulo.
    vm.modules.borrow_mut().insert(real.to_string(), module.clone());
    let globals: Rc<RefCell<HashMap<String, Value>>> = Rc::new(RefCell::new(HashMap::new()));
    {
        let mut g = globals.borrow_mut();
        g.insert("__name__".to_string(), Value::str(real));
        g.insert("__doc__".to_string(), Value::None);
        let as_path = real.replace('.', "/");
        let file = if crate::modules::is_embedded_package(real) {
            format!("/usr/lib/python3.13/{as_path}/__init__.py")
        } else {
            format!("/usr/lib/python3.13/{as_path}.py")
        };
        g.insert("__file__".to_string(), Value::str(file));
        // Pacote: `__package__` é ele mesmo e `__path__` existe (vazio); módulo: o pacote pai.
        if crate::modules::is_embedded_package(real) {
            g.insert("__package__".to_string(), Value::str(real));
            g.insert("__path__".to_string(), Value::list(Vec::new()));
        } else {
            g.insert("__package__".to_string(), Value::str(real.rsplit_once('.').map_or("", |(p, _)| p)));
        }
    }
    vm.module_globals.borrow_mut().insert(module.name, globals.clone());
    let mut inner = vm.clone();
    inner.globals = globals.clone();
    let parsed = crate::parser::parse_module(src).unwrap_or_else(|e| panic!("módulo embutido {real}: {e:?}"));
    let mut code = crate::compile::compile_module(&parsed)
        .unwrap_or_else(|e| panic!("módulo embutido {real}: {}: {}", e.kind, e.msg));
    let filename = format!("/usr/lib/python3.13/{}.py", real.replace('.', "/"));
    code.set_filename(&filename);
    crate::vm::register_source(&filename, src);
    if let Err(e) = inner.run(&Rc::new(code)) {
        panic!("módulo embutido {real}:\n{}", crate::vm::format_traceback_in(&e, real, Some(src)));
    }
    let mut attrs = module.attrs.borrow_mut();
    for (k, v) in globals.borrow().iter() {
        attrs.insert(k.clone(), v.clone());
    }
    drop(attrs);
    // `import pacote.sub` deixa `sub` como atributo do módulo `pacote`.
    if let Some((parent, child)) = real.rsplit_once('.') {
        if let Some(p) = crate::modules::import(vm, parent) {
            if let Some(g) = vm.module_globals.borrow().get(p.name) {
                g.borrow_mut().insert(child.to_string(), Value::Module(module.clone()));
            }
            p.attrs.borrow_mut().insert(child.to_string(), Value::Module(module.clone()));
        }
    }
    Some(module)
}

/// O texto-fonte do módulo embutido `name`.
pub fn source(name: &str) -> Option<&'static str> {
    SOURCES.iter().find(|(n, _)| *n == name).map(|(_, s)| *s)
}

/// Nomes de todos os módulos embutidos em Python (para os testes).
pub fn names() -> Vec<&'static str> {
    SOURCES.iter().map(|(n, _)| *n).collect()
}
