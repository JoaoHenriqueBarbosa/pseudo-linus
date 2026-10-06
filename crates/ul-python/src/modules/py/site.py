"""site: caminhos de pacotes do Debian 13. O sandbox não processa `.pth` nem sitecustomize; as funções
públicas devolvem o que o python3 do Debian devolve."""

import os
import sys

__all__ = ['main', 'addsitedir', 'getsitepackages', 'getuserbase', 'getusersitepackages', 'removeduppaths']

PREFIXES = [sys.prefix, sys.exec_prefix]
ENABLE_USER_SITE = True
USER_SITE = None
USER_BASE = None


def removeduppaths():
    seen = []
    for p in sys.path:
        if p not in seen:
            seen.append(p)
    sys.path[:] = seen
    return set(seen)


def getuserbase():
    global USER_BASE
    if USER_BASE is None:
        env = os.environ.get('PYTHONUSERBASE')
        USER_BASE = env if env else os.path.join(os.path.expanduser('~'), '.local')
    return USER_BASE


def getusersitepackages():
    global USER_SITE
    if USER_SITE is None:
        USER_SITE = os.path.join(getuserbase(), 'lib', 'python3.13', 'site-packages')
    return USER_SITE


def getsitepackages(prefixes=None):
    return ['/usr/local/lib/python3.13/dist-packages', '/usr/lib/python3/dist-packages',
            '/usr/lib/python3.13/dist-packages']


def addsitedir(sitedir, known_paths=None):
    sitedir = os.path.abspath(sitedir)
    if sitedir not in sys.path:
        sys.path.append(sitedir)
    return known_paths


def check_enableusersite():
    return True


def main():
    getuserbase()
    getusersitepackages()
    return None


def _script():
    args = sys.argv[1:]
    if not args:
        user_base = getuserbase()
        user_site = getusersitepackages()
        print('sys.path = [')
        for dir in sys.path:
            print('    %r,' % (dir,))
        print(']')

        def exists(path):
            return 'exists' if os.path.isdir(path) else "doesn't exist"
        print('USER_BASE: %r (%s)' % (user_base, exists(user_base)))
        print('USER_SITE: %r (%s)' % (user_site, exists(user_site)))
        print('ENABLE_USER_SITE: %r' % ENABLE_USER_SITE)
        return 0
    buffer = []
    if '--user-base' in args:
        buffer.append(getuserbase())
    if '--user-site' in args:
        buffer.append(getusersitepackages())
    if buffer:
        print(os.pathsep.join(buffer))
        return 0
    sys.stderr.write('usage: %s [--user-base] [--user-site]\n' % sys.argv[0] if False else
                     'usage: site.py [--user-base] [--user-site]\n')
    return 2


if __name__ == '__main__':
    sys.exit(_script())
