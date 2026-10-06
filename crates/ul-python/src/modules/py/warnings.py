"""warnings: filtros e avisos, com o local do chamador vindo de `sys._getframe`."""

import sys

__all__ = ['warn', 'warn_explicit', 'showwarning', 'formatwarning', 'filterwarnings', 'simplefilter',
           'resetwarnings', 'catch_warnings']

filters = []
_defaultaction = 'default'
_onceregistry = {}
_filters_mutated_count = 0


def _init_filters():
    filters[:] = [
        ('default', None, DeprecationWarning, '__main__', 0),
        ('ignore', None, DeprecationWarning, None, 0),
        ('ignore', None, PendingDeprecationWarning, None, 0),
        ('ignore', None, ImportWarning, None, 0),
        ('ignore', None, ResourceWarning, None, 0),
    ]


_init_filters()


def _source_line(filename, lineno):
    if filename.startswith('<'):
        return None
    try:
        with open(filename, encoding='utf-8') as f:
            for i, line in enumerate(f, 1):
                if i == lineno:
                    return line
    except (OSError, ValueError, UnicodeDecodeError):
        return None
    return None


def formatwarning(message, category, filename, lineno, line=None):
    s = '%s:%s: %s: %s\n' % (filename, lineno, category.__name__, message)
    if line is None:
        line = _source_line(filename, lineno)
    if line:
        s += '  %s\n' % line.strip()
    return s


def showwarning(message, category, filename, lineno, file=None, line=None):
    if file is None:
        file = sys.stderr
        if file is None:
            return
    try:
        file.write(formatwarning(message, category, filename, lineno, line))
    except OSError:
        pass


def _is_name_match(pattern, text):
    if pattern is None:
        return True
    import re
    return re.match(pattern, text) is not None


def filterwarnings(action, message='', category=Warning, module='', lineno=0, append=False):
    _registry.clear()
    import re
    if action not in ('error', 'ignore', 'always', 'default', 'module', 'once'):
        raise ValueError('invalid action: %r' % (action,))
    if not isinstance(message, str):
        raise TypeError('message must be a string')
    if not isinstance(module, str):
        raise TypeError('module must be a string')
    if not isinstance(lineno, int) or lineno < 0:
        raise ValueError('lineno must be an int >= 0')
    pattern = re.compile(message, re.I) if message else None
    mod = re.compile(module) if module else None
    item = (action, pattern, category, mod, lineno)
    if item in filters:
        filters.remove(item)
    if append:
        filters.append(item)
    else:
        filters.insert(0, item)


def simplefilter(action, category=Warning, lineno=0, append=False):
    _registry.clear()
    if action not in ('error', 'ignore', 'always', 'default', 'module', 'once'):
        raise ValueError('invalid action: %r' % (action,))
    item = (action, None, category, None, lineno)
    if item in filters:
        filters.remove(item)
    if append:
        filters.append(item)
    else:
        filters.insert(0, item)


def resetwarnings():
    filters[:] = []
    _registry.clear()


class WarningMessage:
    def __init__(self, message, category, filename, lineno, file=None, line=None, source=None):
        self.message = message
        self.category = category
        self.filename = filename
        self.lineno = lineno
        self.file = file
        self.line = line
        self.source = source
        self._category_name = category.__name__ if category else None

    def __str__(self):
        return '{message : %r, category : %r, filename : %r, lineno : %s, line : %r}' % (
            self.message, self._category_name, self.filename, self.lineno, self.line)


def _match_filter(text, category, module, lineno):
    for item in filters:
        action, msg, cat, mod, ln = item
        if ((msg is None or msg.match(text)) and issubclass(category, cat) and
                (mod is None or (mod == module if isinstance(mod, str) else mod.match(module))) and
                (ln == 0 or lineno == ln)):
            return action
    return _defaultaction


def warn_explicit(message, category, filename, lineno, module=None, registry=None,
                  module_globals=None, source=None):
    if module is None:
        module = filename or '<unknown>'
        if module[-3:].lower() == '.py':
            module = module[:-3]
    if registry is None:
        registry = {}
    if isinstance(message, Warning):
        text = str(message)
        category = message.__class__
    else:
        text = message
        message = category(message)
    key = (text, category, lineno)
    if registry.get(key):
        return
    action = _match_filter(text, category, module, lineno)
    if action == 'ignore':
        return
    if action == 'error':
        raise message
    if action == 'once':
        registry[key] = 1
        oncekey = (text, category)
        if _onceregistry.get(oncekey):
            return
        _onceregistry[oncekey] = 1
    elif action == 'always':
        pass
    elif action == 'module':
        registry[key] = 1
        altkey = (text, category, 0)
        if registry.get(altkey):
            return
        registry[altkey] = 1
    elif action == 'default':
        registry[key] = 1
    else:
        raise RuntimeError('Unrecognized action (%r) in warnings.filters:\n %s' % (action, item))
    _state['show'](message, category, filename, lineno)


_registry = {}
_state = {'show': showwarning}


def warn(message, category=None, stacklevel=1, source=None, *, skip_file_prefixes=()):
    if isinstance(message, Warning):
        category = message.__class__
    if category is None:
        category = UserWarning
    if not (isinstance(category, type) and issubclass(category, Warning)):
        raise TypeError('category must be a Warning subclass, not %r' % type(category).__name__)
    try:
        frame = sys._getframe(stacklevel)
        filename = frame.f_code.co_filename
        lineno = frame.f_lineno
    except ValueError:
        filename = '<sys>'
        lineno = 0
    module = '__main__' if (not filename.startswith('<') or filename == '<string>') else filename
    if filename != '<string>' and filename != '<sys>' and filename != (sys.argv[0] if sys.argv else ''):
        module = filename
    warn_explicit(message, category, filename, lineno, module, _registry)


class catch_warnings:
    def __init__(self, *, record=False, module=None, action=None, category=Warning, lineno=0, append=False):
        self._record = record
        self._entered = False
        self._action = action
        self._category = category
        self._lineno = lineno
        self._append = append

    def __repr__(self):
        args = []
        if self._record:
            args.append('record=True')
        name = type(self).__name__
        return '%s(%s)' % (name, ', '.join(args))

    def __enter__(self):
        if self._entered:
            raise RuntimeError('Cannot enter %r twice' % self)
        self._entered = True
        self._filters = filters[:]
        self._showwarning = _state['show']
        self._registry_backup = dict(_registry)
        _registry.clear()
        if self._action is not None:
            simplefilter(self._action, self._category, self._lineno, self._append)
        if self._record:
            log = []

            def _record(message, category, filename, lineno, file=None, line=None):
                log.append(WarningMessage(message, category, filename, lineno, file, line))
            _state['show'] = _record
            return log
        return None

    def __exit__(self, *exc_info):
        if not self._entered:
            raise RuntimeError('Cannot exit %r without entering first' % self)
        filters[:] = self._filters
        _state['show'] = self._showwarning
        _registry.clear()
        _registry.update(self._registry_backup)
