"""venv enxuto, no comportamento do python3 do Debian 13: o ambiente é montado (diretórios, `pyvenv.cfg`, links do
interpretador, `activate`), mas sem `--without-pip` o `ensurepip` não existe e a criação termina com a mesma
mensagem de erro do Debian (pacote `python3-venv` ausente)."""

import logging
import os
import shlex
import sys

logger = logging.getLogger(__name__)

_ACTIVATE = '''# This file must be used with "source bin/activate" *from bash*
# You cannot run it directly

deactivate () {
    # reset old environment variables
    if [ -n "${_OLD_VIRTUAL_PATH:-}" ] ; then
        PATH="${_OLD_VIRTUAL_PATH:-}"
        export PATH
        unset _OLD_VIRTUAL_PATH
    fi
    if [ -n "${_OLD_VIRTUAL_PYTHONHOME:-}" ] ; then
        PYTHONHOME="${_OLD_VIRTUAL_PYTHONHOME:-}"
        export PYTHONHOME
        unset _OLD_VIRTUAL_PYTHONHOME
    fi

    # Call hash to forget past locations. Without forgetting
    # past locations the $PATH changes we made may not be respected.
    # See "man bash" for more details. hash is usually a builtin of your shell
    hash -r 2> /dev/null

    if [ -n "${_OLD_VIRTUAL_PS1:-}" ] ; then
        PS1="${_OLD_VIRTUAL_PS1:-}"
        export PS1
        unset _OLD_VIRTUAL_PS1
    fi

    unset VIRTUAL_ENV
    unset VIRTUAL_ENV_PROMPT
    if [ ! "${1:-}" = "nondestructive" ] ; then
    # Self destruct!
        unset -f deactivate
    fi
}

# unset irrelevant variables
deactivate nondestructive

# on Windows, a path can contain colons and backslashes and has to be converted:
case "$(uname)" in
    CYGWIN*|MSYS*|MINGW*)
        # transform D:\\path\\to\\venv to /d/path/to/venv on MSYS and MINGW
        # and to /cygdrive/d/path/to/venv on Cygwin
        VIRTUAL_ENV=$(cygpath __VENV_DIR__)
        export VIRTUAL_ENV
        ;;
    *)
        # use the path as-is
        export VIRTUAL_ENV=__VENV_DIR__
        ;;
esac

_OLD_VIRTUAL_PATH="$PATH"
PATH="$VIRTUAL_ENV/"__VENV_BIN_NAME__":$PATH"
export PATH

VIRTUAL_ENV_PROMPT=__VENV_PROMPT__
export VIRTUAL_ENV_PROMPT

# unset PYTHONHOME if set
# this will fail if PYTHONHOME is set to the empty string (which is bad anyway)
# could use `if (set -u; : $PYTHONHOME) ;` in bash
if [ -n "${PYTHONHOME:-}" ] ; then
    _OLD_VIRTUAL_PYTHONHOME="${PYTHONHOME:-}"
    unset PYTHONHOME
fi

if [ -z "${VIRTUAL_ENV_DISABLE_PROMPT:-}" ] ; then
    _OLD_VIRTUAL_PS1="${PS1:-}"
    PS1="("__VENV_PROMPT__") ${PS1:-}"
    export PS1
fi

# Call hash to forget past commands. Without forgetting
# past commands the $PATH changes we made may not be respected
hash -r 2> /dev/null
'''

_NO_PIP = '''The virtual environment was not created successfully because ensurepip is not
available.  On Debian/Ubuntu systems, you need to install the python3-venv
package using the following command.

    apt install python3.13-venv

You may need to use sudo with that command.  After installing the python3-venv
package, recreate your virtual environment.

Failing command: %s

'''


class EnvBuilder:
    def __init__(self, system_site_packages=False, clear=False, symlinks=False, upgrade=False, with_pip=False,
                 prompt=None, upgrade_deps=False, *, scm_ignore_files=frozenset(['git'])):
        self.system_site_packages = system_site_packages
        self.clear = clear
        self.symlinks = symlinks
        self.upgrade = upgrade
        self.with_pip = with_pip
        self.orig_prompt = prompt
        if prompt == '.':
            prompt = os.path.basename(os.getcwd())
        self.prompt = prompt
        self.upgrade_deps = upgrade_deps
        self.scm_ignore_files = frozenset(map(str.lower, scm_ignore_files))
        self.command = None

    def create(self, env_dir):
        env_dir = os.path.abspath(env_dir)
        context = self.ensure_directories(env_dir)
        for scm in self.scm_ignore_files:
            getattr(self, 'create_%s_ignore_file' % scm)(context)
        self.create_configuration(context)
        self.setup_python(context)
        if self.with_pip:
            # O Debian imprime o aviso no stdout (é um `print` no patch do venv).
            sys.stdout.write(_NO_PIP % context.env_exec_cmd)
            sys.stdout.flush()
            raise SystemExit(1)
        if not self.upgrade:
            self.setup_scripts(context)
            self.post_setup(context)

    def clear_directory(self, path):
        import shutil
        for fn in os.listdir(path):
            fn = os.path.join(path, fn)
            if os.path.islink(fn) or os.path.isfile(fn):
                os.remove(fn)
            elif os.path.isdir(fn):
                shutil.rmtree(fn)

    def ensure_directories(self, env_dir):
        def create_if_needed(d):
            if not os.path.exists(d):
                os.makedirs(d)
            elif os.path.islink(d) or os.path.isfile(d):
                raise ValueError('Unable to create directory %r' % d)

        if os.pathsep in os.fspath(env_dir):
            raise ValueError(f'Refusing to create a venv in {env_dir} because it contains the PATH separator {os.pathsep}.')
        if os.path.exists(env_dir) and self.clear:
            self.clear_directory(env_dir)
        context = type('Context', (), {})()
        context.env_dir = env_dir
        context.env_name = os.path.split(env_dir)[1]
        context.prompt = self.prompt if self.prompt is not None else context.env_name
        create_if_needed(env_dir)
        executable = sys.executable
        dirname, exename = os.path.split(os.path.abspath(executable))
        context.executable = executable
        context.python_dir = dirname
        context.python_exe = exename
        binname = 'bin'
        incpath = os.path.join(env_dir, 'include', 'python3.13')
        libpath = os.path.join(env_dir, 'lib', 'python3.13', 'site-packages')
        context.inc_path = incpath
        create_if_needed(incpath)
        context.lib_path = libpath
        create_if_needed(libpath)
        if os.path.exists(os.path.join(env_dir, 'lib64')) is False and not os.path.islink(os.path.join(env_dir, 'lib64')):
            os.symlink('lib', os.path.join(env_dir, 'lib64'))
        context.bin_path = binpath = os.path.join(env_dir, binname)
        context.bin_name = binname
        context.env_exe = os.path.join(binpath, exename)
        context.env_exec_cmd = context.env_exe
        create_if_needed(binpath)
        context.cfg_path = os.path.join(env_dir, 'pyvenv.cfg')
        return context

    def create_configuration(self, context):
        context.cfg_path = path = os.path.join(context.env_dir, 'pyvenv.cfg')
        with open(path, 'w', encoding='utf-8') as f:
            f.write('home = %s\n' % context.python_dir)
            if self.system_site_packages:
                f.write('include-system-site-packages = true\n')
            else:
                f.write('include-system-site-packages = false\n')
            f.write('version = 3.13.5\n')
            f.write('executable = %s\n' % '/usr/bin/python3.13')
            args = [context.executable, '-m', 'venv']
            args += self.command if self.command is not None else [context.env_dir]
            f.write('command = %s\n' % ' '.join(args))

    def create_git_ignore_file(self, context):
        gitignore_path = os.path.join(context.env_dir, '.gitignore')
        with open(gitignore_path, 'w', encoding='utf-8') as file:
            file.write('# Created by venv; see https://docs.python.org/3/library/venv.html\n*\n')

    def setup_python(self, context):
        binpath = context.bin_path
        path = context.env_exe
        exename = context.python_exe
        if not os.path.islink(path):
            os.symlink(context.executable, path)
        for suffix in ('python', 'python3', 'python3.13'):
            link = os.path.join(binpath, suffix)
            if link != path and not os.path.lexists(link):
                os.symlink(exename, link)

    def setup_scripts(self, context):
        with open(os.path.join(context.bin_path, 'activate'), 'w', encoding='utf-8', newline='') as f:
            text = _ACTIVATE
            text = text.replace('__VENV_DIR__', shlex.quote(context.env_dir))
            text = text.replace('__VENV_NAME__', context.env_name)
            text = text.replace('__VENV_PROMPT__', context.prompt)
            text = text.replace('__VENV_BIN_NAME__', context.bin_name)
            text = text.replace('__VENV_PYTHON__', context.env_exe)
            f.write(text)

    def post_setup(self, context):
        pass

    def replace_variables(self, text, context):
        return text


def create(env_dir, system_site_packages=False, clear=False, symlinks=False, with_pip=False, prompt=None,
           upgrade_deps=False, *, scm_ignore_files=frozenset(['git'])):
    builder = EnvBuilder(system_site_packages=system_site_packages, clear=clear, symlinks=symlinks,
                         with_pip=with_pip, prompt=prompt, upgrade_deps=upgrade_deps,
                         scm_ignore_files=scm_ignore_files)
    builder.create(env_dir)


def main(args=None):
    import argparse
    parser = argparse.ArgumentParser(prog='venv', description='Creates virtual Python environments in one or more target directories.')
    parser.add_argument('dirs', metavar='ENV_DIR', nargs='+', help='A directory to create the environment in.')
    parser.add_argument('--system-site-packages', default=False, action='store_true', dest='system_site',
                        help='Give the virtual environment access to the system site-packages dir.')
    group = parser.add_mutually_exclusive_group()
    group.add_argument('--symlinks', default=True, action='store_true', dest='symlinks',
                       help='Try to use symlinks rather than copies, when symlinks are not the default for the platform.')
    group.add_argument('--copies', default=False, action='store_false', dest='symlinks',
                       help='Try to use copies rather than symlinks, even when symlinks are the default for the platform.')
    parser.add_argument('--clear', default=False, action='store_true', dest='clear',
                        help='Delete the contents of the environment directory if it already exists, before environment creation.')
    parser.add_argument('--upgrade', default=False, action='store_true', dest='upgrade',
                        help='Upgrade the environment directory to use this version of Python, assuming Python has been upgraded in-place.')
    parser.add_argument('--without-pip', dest='with_pip', default=True, action='store_false',
                        help='Skips installing or upgrading pip in the virtual environment (pip is bootstrapped by default)')
    parser.add_argument('--prompt', help='Provides an alternative prompt prefix for this environment.')
    parser.add_argument('--upgrade-deps', default=False, action='store_true', dest='upgrade_deps',
                        help='Upgrade core dependencies (pip) to the latest version in PyPI')
    parser.add_argument('--without-scm-ignore-files', dest='scm_ignore_files', action='store_const', const=frozenset(),
                        default=frozenset(['git']), help='Skips adding SCM ignore files to the environment directory (Git is supported by default).')
    options = parser.parse_args(args)
    if options.upgrade and options.clear:
        raise ValueError('you cannot supply --upgrade and --clear together.')
    builder = EnvBuilder(system_site_packages=options.system_site, clear=options.clear, symlinks=options.symlinks,
                         upgrade=options.upgrade, with_pip=options.with_pip, prompt=options.prompt,
                         upgrade_deps=options.upgrade_deps, scm_ignore_files=options.scm_ignore_files)
    raw = list(sys.argv[1:]) if args is None else list(args)
    for d in options.dirs:
        builder.command = raw
        builder.create(d)


if __name__ == '__main__':
    rc = 1
    try:
        main()
        rc = 0
    except SystemExit:
        raise
    except Exception as e:
        print('Error: %s' % e, file=sys.stderr)
    sys.exit(rc)
