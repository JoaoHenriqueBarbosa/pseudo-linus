"""readline do sandbox: sem terminal interativo, então só guarda o histórico em memória e aceita a configuração."""

_history = []
_completer = None
_delims = ' \t\n`~!@#$%^&*()-=+[{]}\\|;:\'",<>/?'
_history_length = -1

__doc__ = 'readline sem terminal: histórico em memória, sem edição de linha.'


def parse_and_bind(string):
    pass


def read_init_file(filename=None):
    pass


def get_line_buffer():
    return ''


def insert_text(string):
    pass


def redisplay():
    pass


def read_history_file(filename=None):
    if filename is None:
        filename = '/root/.history'
    with open(filename, encoding='utf-8') as f:
        for line in f.read().splitlines():
            _history.append(line)


def write_history_file(filename=None):
    if filename is None:
        filename = '/root/.history'
    items = _history if _history_length < 0 else _history[-_history_length:]
    with open(filename, 'w', encoding='utf-8') as f:
        for line in items:
            f.write(line + '\n')


def append_history_file(nelements, filename=None):
    if filename is None:
        filename = '/root/.history'
    with open(filename, 'a', encoding='utf-8') as f:
        for line in _history[-nelements:] if nelements else []:
            f.write(line + '\n')


def get_history_length():
    return _history_length


def set_history_length(length):
    global _history_length
    _history_length = length


def clear_history():
    del _history[:]


def get_current_history_length():
    return len(_history)


def get_history_item(index):
    if 1 <= index <= len(_history):
        return _history[index - 1]
    return None


def remove_history_item(pos):
    if not 0 <= pos < len(_history):
        raise ValueError('No history item at position %d' % pos)
    del _history[pos]


def replace_history_item(pos, line):
    if not 0 <= pos < len(_history):
        raise ValueError('No history item at position %d' % pos)
    _history[pos] = line


def add_history(line):
    _history.append(line)


def set_auto_history(enabled):
    pass


def set_startup_hook(function=None):
    pass


def set_pre_input_hook(function=None):
    pass


def set_completer(function=None):
    global _completer
    _completer = function


def get_completer():
    return _completer


def get_completion_type():
    return 0


def get_begidx():
    return 0


def get_endidx():
    return 0


def set_completer_delims(string):
    global _delims
    _delims = string


def get_completer_delims():
    return _delims


def set_completion_display_matches_hook(function=None):
    pass
