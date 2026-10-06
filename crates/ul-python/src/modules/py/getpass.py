"""Nome do usuário e leitura de senha sem eco (aqui, lida da entrada padrão)."""

import os
import sys

__all__ = ['getpass', 'getuser', 'GetPassWarning']


class GetPassWarning(UserWarning):
    pass


def getuser():
    for name in ('LOGNAME', 'USER', 'LNAME', 'USERNAME'):
        user = os.environ.get(name)
        if user:
            return user
    try:
        import pwd
        return pwd.getpwuid(os.getuid())[0]
    except Exception:
        return str(os.getuid())


def getpass(prompt='Password: ', stream=None):
    out = stream or sys.stderr
    out.write(prompt)
    out.flush()
    line = sys.stdin.readline()
    if not line:
        raise EOFError
    out.write('\n')
    return line.rstrip('\n')
