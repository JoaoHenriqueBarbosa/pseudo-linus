//! Módulos da stdlib escritos em Python, embutidos no binário e executados pela própria VM.
//!
//! Cada fonte vive em `modules/py/<nome>.py`. O módulo roda uma vez, numa `Vm` com globais próprias
//! (as funções definidas nele guardam essas globais em `FuncObj::globals`), e os nomes globais
//! resultantes viram os atributos do `ModuleObj`.
//!
//! Para acrescentar um módulo: crie o `.py` e registre o par `(nome, include_str!)` em [`SOURCES`].

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;

use crate::object::{intern, ModuleObj, Value};
use crate::vm::Vm;

const SOURCES: &[(&str, &str)] = &[
    ("sys", include_str!("py/sys.py")),
    ("posixpath", include_str!("../../../kernel/image/usr/lib/python3.13/posixpath.py")),
    ("os", include_str!("py/os.py")),
    ("io", include_str!("py/io.py")),
    ("itertools", include_str!("py/itertools.py")),
    ("functools", include_str!("py/functools.py")),
    ("contextlib", include_str!("py/contextlib.py")),
    ("abc", include_str!("py/abc.py")),
    ("collections", include_str!("py/collections.py")),
    ("enum", include_str!("py/enum.py")),
    ("collections.abc", include_str!("py/collections_abc.py")),
    ("_collections_abc", include_str!("py/_collections_abc.py")),
    ("typing", include_str!("py/typing.py")),
    ("copy", include_str!("py/copy.py")),
    ("dataclasses", include_str!("py/dataclasses.py")),
    ("time", include_str!("py/time.py")),
    ("datetime", include_str!("py/datetime.py")),
    ("random", include_str!("../../../kernel/image/usr/lib/python3.13/random.py")),
    ("_random", include_str!("py/_random.py")),
    ("_tracemalloc", include_str!("py/_tracemalloc.py")),
    ("mmap", include_str!("py/mmap.py")),
    ("_lsprof", include_str!("py/_lsprof.py")),
    ("bisect", include_str!("../../../kernel/image/usr/lib/python3.13/bisect.py")),
    // Congelado no CPython; o texto é o do `stat.py` do Debian, que o boot precisa antes do `sys.path`.
    ("stat", include_str!("../../../kernel/image/usr/lib/python3.13/stat.py")),
    ("glob", include_str!("../../../kernel/image/usr/lib/python3.13/glob.py")),
    ("shutil", include_str!("py/shutil.py")),
    ("tempfile", include_str!("../../../kernel/image/usr/lib/python3.13/tempfile.py")),
    ("pathlib", include_str!("../../../kernel/image/usr/lib/python3.13/pathlib/__init__.py")),
    ("zlib", include_str!("py/zlib.py")),
    ("gzip", include_str!("py/gzip.py")),
    ("argparse", include_str!("py/argparse.py")),
    ("heapq", include_str!("../../../kernel/image/usr/lib/python3.13/heapq.py")),
    ("types", include_str!("py/types.py")),
    ("weakref", include_str!("py/weakref.py")),
    ("threading", include_str!("py/threading.py")),
    ("logging", include_str!("../../../kernel/image/usr/lib/python3.13/logging/__init__.py")),
    ("atexit", include_str!("py/atexit.py")),
    ("string", include_str!("../../../kernel/image/usr/lib/python3.13/string.py")),
    ("_string", include_str!("py/_string.py")),
    ("pyexpat", include_str!("py/pyexpat.py")),
    ("xml", include_str!("../../../kernel/image/usr/lib/python3.13/xml/__init__.py")),
    ("xml.etree", include_str!("../../../kernel/image/usr/lib/python3.13/xml/etree/__init__.py")),
    ("xml.etree.ElementPath", include_str!("../../../kernel/image/usr/lib/python3.13/xml/etree/ElementPath.py")),
    ("xml.etree.ElementTree", include_str!("../../../kernel/image/usr/lib/python3.13/xml/etree/ElementTree.py")),
    ("xml.parsers", include_str!("../../../kernel/image/usr/lib/python3.13/xml/parsers/__init__.py")),
    ("xml.parsers.expat", include_str!("../../../kernel/image/usr/lib/python3.13/xml/parsers/expat.py")),
    ("xml.dom", include_str!("../../../kernel/image/usr/lib/python3.13/xml/dom/__init__.py")),
    ("xml.dom.NodeFilter", include_str!("../../../kernel/image/usr/lib/python3.13/xml/dom/NodeFilter.py")),
    ("xml.dom.domreg", include_str!("../../../kernel/image/usr/lib/python3.13/xml/dom/domreg.py")),
    ("xml.dom.expatbuilder", include_str!("../../../kernel/image/usr/lib/python3.13/xml/dom/expatbuilder.py")),
    ("xml.dom.minicompat", include_str!("../../../kernel/image/usr/lib/python3.13/xml/dom/minicompat.py")),
    ("xml.dom.minidom", include_str!("../../../kernel/image/usr/lib/python3.13/xml/dom/minidom.py")),
    ("xml.dom.pulldom", include_str!("../../../kernel/image/usr/lib/python3.13/xml/dom/pulldom.py")),
    ("xml.dom.xmlbuilder", include_str!("../../../kernel/image/usr/lib/python3.13/xml/dom/xmlbuilder.py")),
    ("xml.sax", include_str!("../../../kernel/image/usr/lib/python3.13/xml/sax/__init__.py")),
    ("xml.sax._exceptions", include_str!("../../../kernel/image/usr/lib/python3.13/xml/sax/_exceptions.py")),
    ("xml.sax.expatreader", include_str!("../../../kernel/image/usr/lib/python3.13/xml/sax/expatreader.py")),
    ("xml.sax.handler", include_str!("../../../kernel/image/usr/lib/python3.13/xml/sax/handler.py")),
    ("xml.sax.saxutils", include_str!("py/xml_sax_saxutils.py")),
    ("xml.sax.xmlreader", include_str!("py/xml_sax_xmlreader.py")),
    ("concurrent", include_str!("../../../kernel/image/usr/lib/python3.13/concurrent/__init__.py")),
    ("concurrent.futures", include_str!("py/concurrent_futures.py")),
    ("_thread", include_str!("py/_thread.py")),
    ("colorsys", include_str!("../../../kernel/image/usr/lib/python3.13/colorsys.py")),
    ("keyword", include_str!("../../../kernel/image/usr/lib/python3.13/keyword.py")),
    ("dis", include_str!("py/dis.py")),
    ("graphlib", include_str!("../../../kernel/image/usr/lib/python3.13/graphlib.py")),
    ("reprlib", include_str!("../../../kernel/image/usr/lib/python3.13/reprlib.py")),
    ("getopt", include_str!("../../../kernel/image/usr/lib/python3.13/getopt.py")),
    ("difflib", include_str!("../../../kernel/image/usr/lib/python3.13/difflib.py")),
    ("zipfile", include_str!("py/zipfile.py")),
    ("traceback", include_str!("py/traceback.py")),
    ("warnings", include_str!("../../../kernel/image/usr/lib/python3.13/warnings.py")),
    ("subprocess", include_str!("py/subprocess.py")),
    ("_complex", include_str!("py/_complex.py")),
    ("_match", include_str!("py/_match.py")),
    ("_memoryview", include_str!("py/_memoryview.py")),
    ("operator", include_str!("py/operator.py")),
    ("_json", include_str!("py/_json.py")),
    ("_anext", include_str!("py/_anext.py")),
    ("json", include_str!("../../../kernel/image/usr/lib/python3.13/json/__init__.py")),
    ("json.decoder", include_str!("../../../kernel/image/usr/lib/python3.13/json/decoder.py")),
    ("json.encoder", include_str!("../../../kernel/image/usr/lib/python3.13/json/encoder.py")),
    ("json.tool", include_str!("../../../kernel/image/usr/lib/python3.13/json/tool.py")),
    ("json.scanner", include_str!("../../../kernel/image/usr/lib/python3.13/json/scanner.py")),
    ("configparser", include_str!("../../../kernel/image/usr/lib/python3.13/configparser.py")),
    ("queue", include_str!("py/queue.py")),
    ("calendar", include_str!("py/calendar.py")),
    ("uuid", include_str!("../../../kernel/image/usr/lib/python3.13/uuid.py")),
    ("secrets", include_str!("../../../kernel/image/usr/lib/python3.13/secrets.py")),
    ("hmac", include_str!("py/hmac.py")),
    ("numbers", include_str!("../../../kernel/image/usr/lib/python3.13/numbers.py")),
    ("contextvars", include_str!("py/contextvars.py")),
    ("decimal", include_str!("py/decimal.py")),
    ("fractions", include_str!("py/fractions.py")),
    ("statistics", include_str!("py/statistics.py")),
    ("pprint", include_str!("../../../kernel/image/usr/lib/python3.13/pprint.py")),
    ("locale", include_str!("py/locale.py")),
    ("urllib", include_str!("../../../kernel/image/usr/lib/python3.13/urllib/__init__.py")),
    ("urllib.parse", include_str!("../../../kernel/image/usr/lib/python3.13/urllib/parse.py")),
    ("urllib.error", include_str!("../../../kernel/image/usr/lib/python3.13/urllib/error.py")),
    ("urllib.response", include_str!("../../../kernel/image/usr/lib/python3.13/urllib/response.py")),
    ("urllib.request", include_str!("py/urllib_request.py")),
    ("_csv", include_str!("py/_csv.py")),
    ("csv", include_str!("../../../kernel/image/usr/lib/python3.13/csv.py")),
    ("importlib", include_str!("py/importlib.py")),
    ("importlib.machinery", include_str!("py/importlib_machinery.py")),
    ("importlib.abc", include_str!("py/importlib_abc.py")),
    ("importlib.util", include_str!("py/importlib_util.py")),
    ("asyncio", include_str!("../../../kernel/image/usr/lib/python3.13/asyncio/__init__.py")),
    ("asyncio.base_events", include_str!("py/asyncio_base_events.py")),
    ("asyncio.base_futures", include_str!("py/asyncio_base_futures.py")),
    ("asyncio.base_tasks", include_str!("../../../kernel/image/usr/lib/python3.13/asyncio/base_tasks.py")),
    ("asyncio.constants", include_str!("../../../kernel/image/usr/lib/python3.13/asyncio/constants.py")),
    ("asyncio.coroutines", include_str!("../../../kernel/image/usr/lib/python3.13/asyncio/coroutines.py")),
    ("asyncio.events", include_str!("py/asyncio_events.py")),
    ("asyncio.exceptions", include_str!("../../../kernel/image/usr/lib/python3.13/asyncio/exceptions.py")),
    ("asyncio.format_helpers", include_str!("../../../kernel/image/usr/lib/python3.13/asyncio/format_helpers.py")),
    ("asyncio.futures", include_str!("py/asyncio_futures.py")),
    ("asyncio.locks", include_str!("py/asyncio_locks.py")),
    ("asyncio.loopback", include_str!("py/asyncio_loopback.py")),
    ("asyncio.log", include_str!("../../../kernel/image/usr/lib/python3.13/asyncio/log.py")),
    ("asyncio.mixins", include_str!("py/asyncio_mixins.py")),
    ("asyncio.queues", include_str!("py/asyncio_queues.py")),
    ("asyncio.runners", include_str!("py/asyncio_runners.py")),
    ("asyncio.subprocess", include_str!("py/asyncio_subprocess.py")),
    ("asyncio.taskgroups", include_str!("py/asyncio_taskgroups.py")),
    ("asyncio.tasks", include_str!("py/asyncio_tasks.py")),
    ("asyncio.threads", include_str!("py/asyncio_threads.py")),
    ("asyncio.timeouts", include_str!("py/asyncio_timeouts.py")),
    ("asyncio.unix_events", include_str!("py/asyncio_unix_events.py")),
    ("inspect", include_str!("py/inspect.py")),
    ("linecache", include_str!("../../../kernel/image/usr/lib/python3.13/linecache.py")),
    ("_excgroup", include_str!("py/_excgroup.py")),
    ("_imp", include_str!("py/_imp.py")),
    ("errno", include_str!("py/errno.py")),
    ("posix", include_str!("py/posix.py")),
    ("tarfile", include_str!("../../../kernel/image/usr/lib/python3.13/tarfile.py")),
    ("_archivefile", include_str!("py/_archivefile.py")),
    ("sqlite3", include_str!("py/sqlite3.py")),
    ("sqlite3.dbapi2", include_str!("py/sqlite3_dbapi2.py")),
    ("sqlite3.dump", include_str!("../../../kernel/image/usr/lib/python3.13/sqlite3/dump.py")),
    ("bdb", include_str!("../../../kernel/image/usr/lib/python3.13/bdb.py")),
    ("trace", include_str!("../../../kernel/image/usr/lib/python3.13/trace.py")),
    ("cProfile", include_str!("../../../kernel/image/usr/lib/python3.13/cProfile.py")),
    ("profile", include_str!("../../../kernel/image/usr/lib/python3.13/profile.py")),
    ("pydoc", include_str!("py/pydoc.py")),
    ("tracemalloc", include_str!("../../../kernel/image/usr/lib/python3.13/tracemalloc.py")),
    ("fnmatch", include_str!("../../../kernel/image/usr/lib/python3.13/fnmatch.py")),
    ("pathlib._local", include_str!("py/pathlib__local.py")),
    ("pathlib._abc", include_str!("../../../kernel/image/usr/lib/python3.13/pathlib/_abc.py")),
    ("pickletools", include_str!("../../../kernel/image/usr/lib/python3.13/pickletools.py")),
    ("nturl2path", include_str!("../../../kernel/image/usr/lib/python3.13/nturl2path.py")),
    ("ntpath", include_str!("../../../kernel/image/usr/lib/python3.13/ntpath.py")),
    ("genericpath", include_str!("../../../kernel/image/usr/lib/python3.13/genericpath.py")),
    ("asyncio.trsock", include_str!("../../../kernel/image/usr/lib/python3.13/asyncio/trsock.py")),
    ("asyncio.streams", include_str!("py/asyncio_streams.py")),
    ("asyncio.transports", include_str!("../../../kernel/image/usr/lib/python3.13/asyncio/transports.py")),
    ("asyncio.protocols", include_str!("../../../kernel/image/usr/lib/python3.13/asyncio/protocols.py")),
    ("pstats", include_str!("py/pstats.py")),
    ("mailbox", include_str!("../../../kernel/image/usr/lib/python3.13/mailbox.py")),
    ("wsgiref.validate", include_str!("../../../kernel/image/usr/lib/python3.13/wsgiref/validate.py")),
    ("wsgiref.types", include_str!("../../../kernel/image/usr/lib/python3.13/wsgiref/types.py")),
    ("wsgiref.util", include_str!("../../../kernel/image/usr/lib/python3.13/wsgiref/util.py")),
    ("wsgiref.headers", include_str!("../../../kernel/image/usr/lib/python3.13/wsgiref/headers.py")),
    ("wsgiref.handlers", include_str!("../../../kernel/image/usr/lib/python3.13/wsgiref/handlers.py")),
    ("wsgiref.simple_server", include_str!("../../../kernel/image/usr/lib/python3.13/wsgiref/simple_server.py")),
    ("wsgiref", include_str!("../../../kernel/image/usr/lib/python3.13/wsgiref/__init__.py")),
    ("xmlrpc.client", include_str!("../../../kernel/image/usr/lib/python3.13/xmlrpc/client.py")),
    ("xmlrpc", include_str!("../../../kernel/image/usr/lib/python3.13/xmlrpc/__init__.py")),
    ("netrc", include_str!("../../../kernel/image/usr/lib/python3.13/netrc.py")),
    ("ftplib", include_str!("../../../kernel/image/usr/lib/python3.13/ftplib.py")),
    ("poplib", include_str!("../../../kernel/image/usr/lib/python3.13/poplib.py")),
    ("imaplib", include_str!("../../../kernel/image/usr/lib/python3.13/imaplib.py")),
    ("smtplib", include_str!("py/smtplib.py")),
    ("pyclbr", include_str!("../../../kernel/image/usr/lib/python3.13/pyclbr.py")),
    ("shlex", include_str!("../../../kernel/image/usr/lib/python3.13/shlex.py")),
    ("tabnanny", include_str!("../../../kernel/image/usr/lib/python3.13/tabnanny.py")),
    ("logging.config", include_str!("../../../kernel/image/usr/lib/python3.13/logging/config.py")),
    ("sqlite3.__main__", include_str!("../../../kernel/image/usr/lib/python3.13/sqlite3/__main__.py")),
    ("zipimport", include_str!("py/zipimport.py")),
    ("zipapp", include_str!("../../../kernel/image/usr/lib/python3.13/zipapp.py")),
    ("this", include_str!("../../../kernel/image/usr/lib/python3.13/this.py")),
    ("struct", include_str!("py/struct.py")),
    ("py_compile", include_str!("py/py_compile.py")),
    ("compileall", include_str!("../../../kernel/image/usr/lib/python3.13/compileall.py")),
    ("multiprocessing", include_str!("py/multiprocessing.py")),
    ("faulthandler", include_str!("py/faulthandler.py")),
    ("re", include_str!("py/re.py")),
    ("base64", include_str!("../../../kernel/image/usr/lib/python3.13/base64.py")),
    ("_colorize", include_str!("../../../kernel/image/usr/lib/python3.13/_colorize.py")),
    ("doctest", include_str!("../../../kernel/image/usr/lib/python3.13/doctest.py")),
    ("pdb", include_str!("py/pdb.py")),
    ("pwd", include_str!("py/pwd.py")),
    ("grp", include_str!("py/grp.py")),
    ("wave", include_str!("../../../kernel/image/usr/lib/python3.13/wave.py")),
    ("webbrowser", include_str!("../../../kernel/image/usr/lib/python3.13/webbrowser.py")),
    ("importlib.metadata", include_str!("../../../kernel/image/usr/lib/python3.13/importlib/metadata/__init__.py")),
    ("importlib.metadata._adapters", include_str!("../../../kernel/image/usr/lib/python3.13/importlib/metadata/_adapters.py")),
    ("importlib.metadata._collections", include_str!("../../../kernel/image/usr/lib/python3.13/importlib/metadata/_collections.py")),
    ("importlib.metadata._functools", include_str!("../../../kernel/image/usr/lib/python3.13/importlib/metadata/_functools.py")),
    ("importlib.metadata._itertools", include_str!("../../../kernel/image/usr/lib/python3.13/importlib/metadata/_itertools.py")),
    ("importlib.metadata._meta", include_str!("../../../kernel/image/usr/lib/python3.13/importlib/metadata/_meta.py")),
    ("importlib.metadata._text", include_str!("../../../kernel/image/usr/lib/python3.13/importlib/metadata/_text.py")),
    ("importlib.metadata.diagnose", include_str!("../../../kernel/image/usr/lib/python3.13/importlib/metadata/diagnose.py")),
    ("_frozen_importlib", include_str!("py/_frozen_importlib.py")),
    ("_frozen_importlib_external", include_str!("py/_frozen_importlib_external.py")),
    ("importlib.resources", include_str!("py/importlib_resources.py")),
    ("_ssl", include_str!("py/_ssl.py")),
    ("marshal", include_str!("py/marshal.py")),
    ("resource", include_str!("py/resource.py")),
    ("fcntl", include_str!("py/fcntl.py")),
    ("readline", include_str!("py/readline.py")),
    ("select", include_str!("py/select.py")),
    ("selectors", include_str!("../../../kernel/image/usr/lib/python3.13/selectors.py")),
    ("socketserver", include_str!("py/socketserver.py")),
    ("http.cookiejar", include_str!("../../../kernel/image/usr/lib/python3.13/http/cookiejar.py")),
    ("http.server", include_str!("py/http_server.py")),
    ("http.client", include_str!("py/http_client.py")),
    ("socket", include_str!("../../../kernel/image/usr/lib/python3.13/socket.py")),
    ("_socket", include_str!("py/_socket.py")),
    ("_net", include_str!("py/_net.py")),
    ("http.cookies", include_str!("../../../kernel/image/usr/lib/python3.13/http/cookies.py")),
    ("email.mime.message", include_str!("../../../kernel/image/usr/lib/python3.13/email/mime/message.py")),
    ("email.mime.image", include_str!("../../../kernel/image/usr/lib/python3.13/email/mime/image.py")),
    ("email.mime.audio", include_str!("../../../kernel/image/usr/lib/python3.13/email/mime/audio.py")),
    ("email.mime.application", include_str!("../../../kernel/image/usr/lib/python3.13/email/mime/application.py")),
    ("email.mime.nonmultipart", include_str!("../../../kernel/image/usr/lib/python3.13/email/mime/nonmultipart.py")),
    ("email.mime", include_str!("../../../kernel/image/usr/lib/python3.13/email/mime/__init__.py")),
    ("email._header_value_parser", include_str!("../../../kernel/image/usr/lib/python3.13/email/_header_value_parser.py")),
    ("email.contentmanager", include_str!("../../../kernel/image/usr/lib/python3.13/email/contentmanager.py")),
    ("email.quoprimime", include_str!("../../../kernel/image/usr/lib/python3.13/email/quoprimime.py")),
    ("email.base64mime", include_str!("../../../kernel/image/usr/lib/python3.13/email/base64mime.py")),
    ("email._encoded_words", include_str!("../../../kernel/image/usr/lib/python3.13/email/_encoded_words.py")),
    ("email._policybase", include_str!("py/email__policybase.py")),
    ("email._parseaddr", include_str!("../../../kernel/image/usr/lib/python3.13/email/_parseaddr.py")),
    ("email.iterators", include_str!("../../../kernel/image/usr/lib/python3.13/email/iterators.py")),
    ("email.headerregistry", include_str!("../../../kernel/image/usr/lib/python3.13/email/headerregistry.py")),
    ("email.header", include_str!("../../../kernel/image/usr/lib/python3.13/email/header.py")),
    ("email.generator", include_str!("../../../kernel/image/usr/lib/python3.13/email/generator.py")),
    ("email.feedparser", include_str!("py/email_feedparser.py")),
    ("email.encoders", include_str!("../../../kernel/image/usr/lib/python3.13/email/encoders.py")),
    ("email.charset", include_str!("../../../kernel/image/usr/lib/python3.13/email/charset.py")),
    ("email.errors", include_str!("../../../kernel/image/usr/lib/python3.13/email/errors.py")),
    ("logging.handlers", include_str!("../../../kernel/image/usr/lib/python3.13/logging/handlers.py")),
    ("email.mime.base", include_str!("../../../kernel/image/usr/lib/python3.13/email/mime/base.py")),
    ("email.mime.multipart", include_str!("../../../kernel/image/usr/lib/python3.13/email/mime/multipart.py")),
    ("email.mime.text", include_str!("../../../kernel/image/usr/lib/python3.13/email/mime/text.py")),
    ("email.utils", include_str!("py/email_utils.py")),
    ("email.policy", include_str!("../../../kernel/image/usr/lib/python3.13/email/policy.py")),
    ("email.parser", include_str!("py/email_parser.py")),
    ("email.message", include_str!("../../../kernel/image/usr/lib/python3.13/email/message.py")),
    ("email", include_str!("../../../kernel/image/usr/lib/python3.13/email/__init__.py")),
    ("http", include_str!("../../../kernel/image/usr/lib/python3.13/http/__init__.py")),
    ("cmath", include_str!("py/cmath.py")),
    ("_tokenize", include_str!("py/_tokenize.py")),
    ("token", include_str!("../../../kernel/image/usr/lib/python3.13/token.py")),
    ("tokenize", include_str!("../../../kernel/image/usr/lib/python3.13/tokenize.py")),
    ("dbm.sqlite3", include_str!("../../../kernel/image/usr/lib/python3.13/dbm/sqlite3.py")),
    ("dbm.dumb", include_str!("../../../kernel/image/usr/lib/python3.13/dbm/dumb.py")),
    ("dbm", include_str!("../../../kernel/image/usr/lib/python3.13/dbm/__init__.py")),
    ("shelve", include_str!("../../../kernel/image/usr/lib/python3.13/shelve.py")),
    ("code", include_str!("../../../kernel/image/usr/lib/python3.13/code.py")),
    ("codeop", include_str!("../../../kernel/image/usr/lib/python3.13/codeop.py")),
    ("plistlib", include_str!("py/plistlib.py")),
    ("quopri", include_str!("../../../kernel/image/usr/lib/python3.13/quopri.py")),
    ("html.entities", include_str!("../../../kernel/image/usr/lib/python3.13/html/entities.py")),
    ("__future__", include_str!("../../../kernel/image/usr/lib/python3.13/__future__.py")),
    ("_compat_pickle", include_str!("../../../kernel/image/usr/lib/python3.13/_compat_pickle.py")),
    ("fileinput", include_str!("../../../kernel/image/usr/lib/python3.13/fileinput.py")),
    ("_markupbase", include_str!("../../../kernel/image/usr/lib/python3.13/_markupbase.py")),
    ("html.parser", include_str!("../../../kernel/image/usr/lib/python3.13/html/parser.py")),
    ("mimetypes", include_str!("../../../kernel/image/usr/lib/python3.13/mimetypes.py")),
    ("ipaddress", include_str!("../../../kernel/image/usr/lib/python3.13/ipaddress.py")),
    ("tomllib._types", include_str!("../../../kernel/image/usr/lib/python3.13/tomllib/_types.py")),
    ("tomllib._re", include_str!("py/tomllib__re.py")),
    ("tomllib._parser", include_str!("py/tomllib__parser.py")),
    ("tomllib", include_str!("py/tomllib.py")),
    ("pickle", include_str!("py/pickle.py")),
    ("copyreg", include_str!("py/copyreg.py")),
    ("_ast", include_str!("py/_ast.py")),
    ("ast", include_str!("py/ast.py")),
    ("zoneinfo", include_str!("../../../kernel/image/usr/lib/python3.13/zoneinfo/__init__.py")),
    ("zoneinfo._tzpath", include_str!("../../../kernel/image/usr/lib/python3.13/zoneinfo/_tzpath.py")),
    ("zoneinfo._common", include_str!("../../../kernel/image/usr/lib/python3.13/zoneinfo/_common.py")),
    ("zoneinfo._zoneinfo", include_str!("py/zoneinfo_zoneinfo.py")),
    ("bz2", include_str!("py/bz2.py")),
    ("lzma", include_str!("py/lzma.py")),
    ("array", include_str!("py/array.py")),
    ("codecs", include_str!("py/codecs.py")),
    ("gc", include_str!("py/gc.py")),
    ("getpass", include_str!("../../../kernel/image/usr/lib/python3.13/getpass.py")),
    ("platform", include_str!("../../../kernel/image/usr/lib/python3.13/platform.py")),
    ("optparse", include_str!("../../../kernel/image/usr/lib/python3.13/optparse.py")),
    ("sched", include_str!("../../../kernel/image/usr/lib/python3.13/sched.py")),
    ("cmd", include_str!("../../../kernel/image/usr/lib/python3.13/cmd.py")),
    ("filecmp", include_str!("../../../kernel/image/usr/lib/python3.13/filecmp.py")),
    ("gettext", include_str!("py/gettext.py")),
    ("timeit", include_str!("../../../kernel/image/usr/lib/python3.13/timeit.py")),
    ("signal", include_str!("py/signal.py")),
    ("unittest", include_str!("../../../kernel/image/usr/lib/python3.13/unittest/__init__.py")),
    ("unittest.__main__", include_str!("py/unittest___main__.py")),
    ("unittest.mock", include_str!("py/unittest_mock.py")),
    ("unittest._log", include_str!("../../../kernel/image/usr/lib/python3.13/unittest/_log.py")),
    ("unittest.case", include_str!("py/unittest_case.py")),
    ("unittest.loader", include_str!("py/unittest_loader.py")),
    ("unittest.main", include_str!("py/unittest_main.py")),
    ("unittest.result", include_str!("py/unittest_result.py")),
    ("unittest.runner", include_str!("py/unittest_runner.py")),
    ("unittest.signals", include_str!("../../../kernel/image/usr/lib/python3.13/unittest/signals.py")),
    ("unittest.suite", include_str!("py/unittest_suite.py")),
    ("unittest.util", include_str!("../../../kernel/image/usr/lib/python3.13/unittest/util.py")),
    // olefile 0.47 (o `python3-olefile` do Debian 13), que o Pillow usa para FPX e MIC.
    ("olefile", include_str!("py/olefile/__init__.py")),
    ("olefile.olefile", include_str!("py/olefile/olefile.py")),
    // Pillow 11.1.0 (o `python3-pil` do Debian 13), camada Python sem alterações; o núcleo C
    // (`PIL._imaging`) é o módulo nativo `imaging`.
    ("PIL", include_str!("py/PIL/__init__.py")),
    ("PIL._version", include_str!("py/PIL/_version.py")),
    ("PIL._binary", include_str!("py/PIL/_binary.py")),
    ("PIL._deprecate", include_str!("py/PIL/_deprecate.py")),
    ("PIL._typing", include_str!("py/PIL/_typing.py")),
    ("PIL._util", include_str!("py/PIL/_util.py")),
    ("PIL.ExifTags", include_str!("py/PIL/ExifTags.py")),
    ("PIL.Image", include_str!("py/PIL/Image.py")),
    ("PIL.ImageMode", include_str!("py/PIL/ImageMode.py")),
    ("PIL.ImageColor", include_str!("py/PIL/ImageColor.py")),
    ("PIL.ImageDraw", include_str!("py/PIL/ImageDraw.py")),
    ("PIL.ImageFile", include_str!("py/PIL/ImageFile.py")),
    ("PIL.ImagePalette", include_str!("py/PIL/ImagePalette.py")),
    ("PIL.ImageChops", include_str!("py/PIL/ImageChops.py")),
    ("PIL.ImageSequence", include_str!("py/PIL/ImageSequence.py")),
    ("PIL.PngImagePlugin", include_str!("py/PIL/PngImagePlugin.py")),
    ("PIL.TiffTags", include_str!("py/PIL/TiffTags.py")),
    ("PIL.TiffImagePlugin", include_str!("py/PIL/TiffImagePlugin.py")),
    ("PIL.ImageFont", include_str!("py/PIL/ImageFont.py")),
    ("PIL.ImageOps", include_str!("py/PIL/ImageOps.py")),
    ("PIL.ImageFilter", include_str!("py/PIL/ImageFilter.py")),
    ("PIL.ImageStat", include_str!("py/PIL/ImageStat.py")),
    ("PIL.ImageEnhance", include_str!("py/PIL/ImageEnhance.py")),
    ("PIL.ImageMath", include_str!("py/PIL/ImageMath.py")),
    ("PIL.JpegImagePlugin", include_str!("py/PIL/JpegImagePlugin.py")),
    ("PIL.JpegPresets", include_str!("py/PIL/JpegPresets.py")),
    ("PIL.BmpImagePlugin", include_str!("py/PIL/BmpImagePlugin.py")),
    ("PIL.GifImagePlugin", include_str!("py/PIL/GifImagePlugin.py")),
    ("PIL.PpmImagePlugin", include_str!("py/PIL/PpmImagePlugin.py")),
    ("PIL.features", include_str!("py/PIL/features.py")),
    ("PIL.BdfFontFile", include_str!("py/PIL/BdfFontFile.py")),
    ("PIL.BlpImagePlugin", include_str!("py/PIL/BlpImagePlugin.py")),
    ("PIL.BufrStubImagePlugin", include_str!("py/PIL/BufrStubImagePlugin.py")),
    ("PIL.ContainerIO", include_str!("py/PIL/ContainerIO.py")),
    ("PIL.CurImagePlugin", include_str!("py/PIL/CurImagePlugin.py")),
    ("PIL.DcxImagePlugin", include_str!("py/PIL/DcxImagePlugin.py")),
    ("PIL.DdsImagePlugin", include_str!("py/PIL/DdsImagePlugin.py")),
    ("PIL.EpsImagePlugin", include_str!("py/PIL/EpsImagePlugin.py")),
    ("PIL.FitsImagePlugin", include_str!("py/PIL/FitsImagePlugin.py")),
    ("PIL.FliImagePlugin", include_str!("py/PIL/FliImagePlugin.py")),
    ("PIL.FontFile", include_str!("py/PIL/FontFile.py")),
    ("PIL.FpxImagePlugin", include_str!("py/PIL/FpxImagePlugin.py")),
    ("PIL.FtexImagePlugin", include_str!("py/PIL/FtexImagePlugin.py")),
    ("PIL.GbrImagePlugin", include_str!("py/PIL/GbrImagePlugin.py")),
    ("PIL.GdImageFile", include_str!("py/PIL/GdImageFile.py")),
    ("PIL.GimpGradientFile", include_str!("py/PIL/GimpGradientFile.py")),
    ("PIL.GimpPaletteFile", include_str!("py/PIL/GimpPaletteFile.py")),
    ("PIL.GribStubImagePlugin", include_str!("py/PIL/GribStubImagePlugin.py")),
    ("PIL.Hdf5StubImagePlugin", include_str!("py/PIL/Hdf5StubImagePlugin.py")),
    ("PIL.IcnsImagePlugin", include_str!("py/PIL/IcnsImagePlugin.py")),
    ("PIL.IcoImagePlugin", include_str!("py/PIL/IcoImagePlugin.py")),
    ("PIL.ImageCms", include_str!("py/PIL/ImageCms.py")),
    ("PIL.ImageDraw2", include_str!("py/PIL/ImageDraw2.py")),
    ("PIL.ImageGrab", include_str!("py/PIL/ImageGrab.py")),
    ("PIL.ImageMorph", include_str!("py/PIL/ImageMorph.py")),
    ("PIL.ImagePath", include_str!("py/PIL/ImagePath.py")),
    ("PIL.ImageQt", include_str!("py/PIL/ImageQt.py")),
    ("PIL.ImageShow", include_str!("py/PIL/ImageShow.py")),
    ("PIL.ImageTransform", include_str!("py/PIL/ImageTransform.py")),
    ("PIL.ImageWin", include_str!("py/PIL/ImageWin.py")),
    ("PIL.ImImagePlugin", include_str!("py/PIL/ImImagePlugin.py")),
    ("PIL.ImtImagePlugin", include_str!("py/PIL/ImtImagePlugin.py")),
    ("PIL.IptcImagePlugin", include_str!("py/PIL/IptcImagePlugin.py")),
    ("PIL.Jpeg2KImagePlugin", include_str!("py/PIL/Jpeg2KImagePlugin.py")),
    ("PIL.__main__", include_str!("py/PIL/__main__.py")),
    ("PIL.McIdasImagePlugin", include_str!("py/PIL/McIdasImagePlugin.py")),
    ("PIL.MicImagePlugin", include_str!("py/PIL/MicImagePlugin.py")),
    ("PIL.MpegImagePlugin", include_str!("py/PIL/MpegImagePlugin.py")),
    ("PIL.MpoImagePlugin", include_str!("py/PIL/MpoImagePlugin.py")),
    ("PIL.MspImagePlugin", include_str!("py/PIL/MspImagePlugin.py")),
    ("PIL.PaletteFile", include_str!("py/PIL/PaletteFile.py")),
    ("PIL.PalmImagePlugin", include_str!("py/PIL/PalmImagePlugin.py")),
    ("PIL.PcdImagePlugin", include_str!("py/PIL/PcdImagePlugin.py")),
    ("PIL.PcfFontFile", include_str!("py/PIL/PcfFontFile.py")),
    ("PIL.PcxImagePlugin", include_str!("py/PIL/PcxImagePlugin.py")),
    ("PIL.PdfImagePlugin", include_str!("py/PIL/PdfImagePlugin.py")),
    ("PIL.PdfParser", include_str!("py/PIL/PdfParser.py")),
    ("PIL.PixarImagePlugin", include_str!("py/PIL/PixarImagePlugin.py")),
    ("PIL.PsdImagePlugin", include_str!("py/PIL/PsdImagePlugin.py")),
    ("PIL.PSDraw", include_str!("py/PIL/PSDraw.py")),
    ("PIL.QoiImagePlugin", include_str!("py/PIL/QoiImagePlugin.py")),
    ("PIL.report", include_str!("py/PIL/report.py")),
    ("PIL.SgiImagePlugin", include_str!("py/PIL/SgiImagePlugin.py")),
    ("PIL.SpiderImagePlugin", include_str!("py/PIL/SpiderImagePlugin.py")),
    ("PIL.SunImagePlugin", include_str!("py/PIL/SunImagePlugin.py")),
    ("PIL.TarIO", include_str!("py/PIL/TarIO.py")),
    ("PIL.TgaImagePlugin", include_str!("py/PIL/TgaImagePlugin.py")),
    ("PIL._tkinter_finder", include_str!("py/PIL/_tkinter_finder.py")),
    ("PIL.WalImageFile", include_str!("py/PIL/WalImageFile.py")),
    ("PIL.WebPImagePlugin", include_str!("py/PIL/WebPImagePlugin.py")),
    ("PIL.WmfImagePlugin", include_str!("py/PIL/WmfImagePlugin.py")),
    ("PIL.XbmImagePlugin", include_str!("py/PIL/XbmImagePlugin.py")),
    ("PIL.XpmImagePlugin", include_str!("py/PIL/XpmImagePlugin.py")),
    ("PIL.XVThumbImagePlugin", include_str!("py/PIL/XVThumbImagePlugin.py")),
];

/// Nomes de módulo que são apelidos de outro.
fn alias(name: &str) -> &str {
    match name {
        "os.path" => "posixpath",
        other => other,
    }
}

thread_local! {
    /// A exceção do último módulo embutido que falhou ao rodar.
    static IMPORT_ERROR: RefCell<Option<crate::vm::PyException>> = const { RefCell::new(None) };
}

/// Recolhe a exceção deixada por um `import` que devolveu `None` porque o módulo falhou ao rodar.
pub fn take_error() -> Option<crate::vm::PyException> {
    IMPORT_ERROR.with(|c| c.borrow_mut().take())
}

/// Diretório de instalação de um módulo embutido: o Pillow do Debian mora em dist-packages, fora
/// da stdlib.
fn embedded_base(real: &str) -> &'static str {
    let top = real.split('.').next().unwrap_or(real);
    if matches!(top, "PIL" | "olefile") { "/usr/lib/python3/dist-packages" } else { "/usr/lib/python3.13" }
}

fn embedded_dir(real: &str) -> String {
    format!("{}/{}", embedded_base(real), real.replace('.', "/"))
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
    // Só os módulos de arquivo têm `__builtins__` nas globais (os de C do CPython não).
    let builtins = if crate::object::BUILTIN_MODULES.contains(&real) { None } else { crate::modules::builtins_dict(vm) };
    let globals: Rc<RefCell<crate::object::VarMap>> = Rc::new(RefCell::new(Default::default()));
    {
        let mut g = globals.borrow_mut();
        if let Some(b) = builtins {
            g.insert("__builtins__".into(), b);
        }
        g.insert("__name__".into(), Value::str(real));
        g.insert("__doc__".into(), Value::None);
        let as_path = real.replace('.', "/");
        let base = embedded_base(real);
        let file = if crate::modules::is_embedded_package(real) {
            format!("{base}/{as_path}/__init__.py")
        } else {
            format!("{base}/{as_path}.py")
        };
        // Módulo que no Debian é C embutido no executável não tem `__file__`.
        if !crate::object::BUILTIN_MODULES.contains(&real) {
            if !crate::object::FROZEN_MODULES.contains(&real) {
                if let Some(cached) = crate::modules::cached_path(&file) {
                    g.insert("__cached__".into(), Value::str(cached));
                }
            }
            g.insert("__file__".into(), Value::str(file));
        }
        // Pacote: `__package__` é ele mesmo e `__path__` aponta o diretório dele; módulo: o pacote pai.
        if crate::modules::is_embedded_package(real) {
            g.insert("__package__".into(), Value::str(real));
            g.insert("__path__".into(), Value::list(vec![Value::str(format!("{base}/{as_path}"))]));
        } else {
            g.insert("__package__".into(), Value::str(real.rsplit_once('.').map_or("", |(p, _)| p)));
        }
    }
    vm.module_globals.borrow_mut().insert(module.name, globals.clone());
    let mut inner = vm.clone();
    inner.globals = globals.clone();
    let mut parsed = crate::parser::parse_module(src).unwrap_or_else(|e| panic!("módulo embutido {real}: {e:?}"));
    let filename = if crate::modules::is_embedded_package(real) {
        format!("{}/__init__.py", embedded_dir(real))
    } else {
        format!("{}.py", embedded_dir(real))
    };
    // As docstrings são as do CPython que está no disco, nunca as do fonte embutido (sem processo,
    // como nos testes de unidade, valem só as da tabela).
    let cpython = sysabi::sys::try_current()
        .and_then(|_| sysabi::sys::read_file(filename.as_bytes()).ok())
        .and_then(|b| crate::parser::parse_module(&String::from_utf8_lossy(&b)).ok());
    crate::modules::cpydocs::align(&mut parsed, real, cpython.as_ref());
    let mut code = crate::compile::compile_module(&parsed)
        .unwrap_or_else(|e| panic!("módulo embutido {real}: {}: {}", e.kind, e.msg));
    code.set_filename(&filename);
    code.mark_internal();
    crate::vm::register_source(&filename, src);
    if let Err(e) = inner.run(&Rc::new(code)) {
        // Como no CPython, o módulo que falhou ao rodar sai de `sys.modules` e a exceção sobe para
        // quem importou (o `import_checked` a recolhe com `take_error`).
        vm.modules.borrow_mut().remove(real);
        IMPORT_ERROR.with(|c| *c.borrow_mut() = Some(e.exc));
        return None;
    }
    // Módulo que no Debian é C embutido: o programa só enxerga os nomes que o `dir()` do CPython
    // lista. As funções do shim seguem com as globais completas, como o C, que não consulta o
    // dicionário do módulo (trocar `time.time` de fora não muda o que o `time` usa por dentro).
    if let Some(visible) = builtin_dir(real) {
        let public: crate::object::VarMap = globals
            .borrow()
            .iter()
            .filter(|(k, _)| visible.contains(&k.to_string().as_str()))
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        vm.module_globals.borrow_mut().insert(module.name, Rc::new(RefCell::new(public)));
        PRIVATE.with(|p| p.borrow_mut().insert(module.name, globals.clone()));
    }
    let mut attrs = module.attrs.borrow_mut();
    let shown = vm.module_globals.borrow().get(module.name).cloned().unwrap_or_else(|| globals.clone());
    for (k, v) in shown.borrow().iter() {
        attrs.insert(k.to_string(), v.clone());
    }
    drop(attrs);
    // `import pacote.sub` deixa `sub` como atributo do módulo `pacote`.
    if let Some((parent, child)) = real.rsplit_once('.') {
        if let Some(p) = crate::modules::import(vm, parent) {
            if let Some(g) = vm.module_globals.borrow().get(p.name) {
                g.borrow_mut().insert(child.into(), Value::Module(module.clone()));
            }
            p.attrs.borrow_mut().insert(child.to_string(), Value::Module(module.clone()));
        }
    }
    Some(module)
}

thread_local! {
    /// As globais completas dos módulos embutidos cujo lado visível foi filtrado por `builtin_dir`.
    static PRIVATE: RefCell<std::collections::HashMap<&'static str, Rc<RefCell<crate::object::VarMap>>>> =
        RefCell::new(std::collections::HashMap::new());
}

/// Atributo escondido de um módulo embutido, para o código embutido que lê os auxiliares de outro
/// shim (`_socket._fds` no `select`); o programa nunca chega aqui.
pub fn private_attr(module: &str, name: &str) -> Option<Value> {
    PRIVATE.with(|p| p.borrow().get(module).and_then(|g| g.borrow().get(name).cloned()))
}

/// Os nomes do `dir()` do módulo embutido `name` no CPython 3.13 do Debian (`builtin-dir.tsv`,
/// gerado no oráculo). `sys` e `builtins` ficam de fora: o programa religa nomes públicos deles
/// (`sys.stdout = ...`) e o interpretador precisa ver a troca.
fn builtin_dir(name: &str) -> Option<Vec<&'static str>> {
    const TABLE: &str = include_str!("../../data/cpython-docs/builtin-dir.tsv");
    if matches!(name, "sys" | "builtins") {
        return None;
    }
    TABLE.lines().find_map(|line| {
        let (module, names) = line.split_once('\t')?;
        (module == name).then(|| names.split(' ').collect())
    })
}

/// O texto-fonte do módulo embutido `name`.
pub fn source(name: &str) -> Option<&'static str> {
    SOURCES.iter().find(|(n, _)| *n == name).map(|(_, s)| *s)
}

/// Nomes de todos os módulos embutidos em Python (para os testes).
pub fn names() -> Vec<&'static str> {
    SOURCES.iter().map(|(n, _)| *n).collect()
}
