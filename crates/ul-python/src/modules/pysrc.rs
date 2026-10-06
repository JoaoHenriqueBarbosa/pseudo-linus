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
    ("_collections_abc", include_str!("py/_collections_abc.py")),
    ("typing", include_str!("py/typing.py")),
    ("copy", include_str!("py/copy.py")),
    ("dataclasses", include_str!("py/dataclasses.py")),
    ("time", include_str!("py/time.py")),
    ("datetime", include_str!("py/datetime.py")),
    ("random", include_str!("py/random.py")),
    ("_random", include_str!("py/_random.py")),
    ("_tracemalloc", include_str!("py/_tracemalloc.py")),
    ("mmap", include_str!("py/mmap.py")),
    ("_lsprof", include_str!("py/_lsprof.py")),
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
    ("dis", include_str!("py/dis.py")),
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
    ("asyncio.loopback", include_str!("py/asyncio_loopback.py")),
    ("asyncio.log", include_str!("py/asyncio_log.py")),
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
    ("bdb", include_str!("py/bdb.py")),
    ("trace", include_str!("py/trace.py")),
    ("cProfile", include_str!("py/cProfile.py")),
    ("profile", include_str!("py/profile.py")),
    ("pydoc", include_str!("py/pydoc.py")),
    ("tracemalloc", include_str!("py/tracemalloc.py")),
    ("fnmatch", include_str!("py/fnmatch.py")),
    ("pathlib._local", include_str!("py/pathlib__local.py")),
    ("pathlib._abc", include_str!("py/pathlib__abc.py")),
    ("pickletools", include_str!("py/pickletools.py")),
    ("nturl2path", include_str!("py/nturl2path.py")),
    ("ntpath", include_str!("py/ntpath.py")),
    ("genericpath", include_str!("py/genericpath.py")),
    ("asyncio.trsock", include_str!("py/asyncio_trsock.py")),
    ("asyncio.streams", include_str!("py/asyncio_streams.py")),
    ("asyncio.transports", include_str!("py/asyncio_transports.py")),
    ("asyncio.protocols", include_str!("py/asyncio_protocols.py")),
    ("pstats", include_str!("py/pstats.py")),
    ("mailbox", include_str!("py/mailbox.py")),
    ("wsgiref.validate", include_str!("py/wsgiref_validate.py")),
    ("wsgiref.types", include_str!("py/wsgiref_types.py")),
    ("wsgiref.util", include_str!("py/wsgiref_util.py")),
    ("wsgiref.headers", include_str!("py/wsgiref_headers.py")),
    ("wsgiref.handlers", include_str!("py/wsgiref_handlers.py")),
    ("wsgiref.simple_server", include_str!("py/wsgiref_simple_server.py")),
    ("wsgiref", include_str!("py/wsgiref.py")),
    ("xmlrpc.client", include_str!("py/xmlrpc_client.py")),
    ("xmlrpc", include_str!("py/xmlrpc.py")),
    ("netrc", include_str!("py/netrc.py")),
    ("ftplib", include_str!("py/ftplib.py")),
    ("poplib", include_str!("py/poplib.py")),
    ("imaplib", include_str!("py/imaplib.py")),
    ("smtplib", include_str!("py/smtplib.py")),
    ("pyclbr", include_str!("py/pyclbr.py")),
    ("shlex", include_str!("py/shlex.py")),
    ("tabnanny", include_str!("py/tabnanny.py")),
    ("logging.config", include_str!("py/logging_config.py")),
    ("sqlite3.__main__", include_str!("py/sqlite3___main__.py")),
    ("zipimport", include_str!("py/zipimport.py")),
    ("zipapp", include_str!("py/zipapp.py")),
    ("this", include_str!("py/this.py")),
    ("struct", include_str!("py/struct.py")),
    ("site", include_str!("py/site.py")),
    ("venv", include_str!("py/venv.py")),
    ("py_compile", include_str!("py/py_compile.py")),
    ("compileall", include_str!("py/compileall.py")),
    ("multiprocessing", include_str!("py/multiprocessing.py")),
    ("faulthandler", include_str!("py/faulthandler.py")),
    ("re", include_str!("py/re.py")),
    ("base64", include_str!("py/base64.py")),
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
    ("marshal", include_str!("py/marshal.py")),
    ("resource", include_str!("py/resource.py")),
    ("fcntl", include_str!("py/fcntl.py")),
    ("readline", include_str!("py/readline.py")),
    ("select", include_str!("py/select.py")),
    ("selectors", include_str!("py/selectors.py")),
    ("socketserver", include_str!("py/socketserver.py")),
    ("http.cookiejar", include_str!("py/http_cookiejar.py")),
    ("http.server", include_str!("py/http_server.py")),
    ("http.client", include_str!("py/http_client.py")),
    ("socket", include_str!("py/socket.py")),
    ("_socket", include_str!("py/_socket.py")),
    ("_net", include_str!("py/_net.py")),
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
    let globals: Rc<RefCell<crate::object::VarMap>> = Rc::new(RefCell::new(Default::default()));
    {
        let mut g = globals.borrow_mut();
        g.insert("__name__".into(), Value::str(real));
        g.insert("__doc__".into(), Value::None);
        let as_path = real.replace('.', "/");
        let base = embedded_base(real);
        let file = if crate::modules::is_embedded_package(real) {
            format!("{base}/{as_path}/__init__.py")
        } else {
            format!("{base}/{as_path}.py")
        };
        g.insert("__file__".into(), Value::str(file));
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
    let parsed = crate::parser::parse_module(src).unwrap_or_else(|e| panic!("módulo embutido {real}: {e:?}"));
    let mut code = crate::compile::compile_module(&parsed)
        .unwrap_or_else(|e| panic!("módulo embutido {real}: {}: {}", e.kind, e.msg));
    let filename = if crate::modules::is_embedded_package(real) {
        format!("{}/__init__.py", embedded_dir(real))
    } else {
        format!("{}.py", embedded_dir(real))
    };
    code.set_filename(&filename);
    crate::vm::register_source(&filename, src);
    if let Err(e) = inner.run(&Rc::new(code)) {
        // Como no CPython, o módulo que falhou ao rodar sai de `sys.modules` e a exceção sobe para
        // quem importou (o `import_checked` a recolhe com `take_error`).
        vm.modules.borrow_mut().remove(real);
        IMPORT_ERROR.with(|c| *c.borrow_mut() = Some(e.exc));
        return None;
    }
    let mut attrs = module.attrs.borrow_mut();
    for (k, v) in globals.borrow().iter() {
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

/// O texto-fonte do módulo embutido `name`.
pub fn source(name: &str) -> Option<&'static str> {
    SOURCES.iter().find(|(n, _)| *n == name).map(|(_, s)| *s)
}

/// Nomes de todos os módulos embutidos em Python (para os testes).
pub fn names() -> Vec<&'static str> {
    SOURCES.iter().map(|(n, _)| *n).collect()
}
