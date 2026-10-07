"""_frozen_importlib: o núcleo do sistema de importação (os importadores de `sys.meta_path`)."""


class BuiltinImporter:
    """Meta path import for built-in modules.

    All methods are either class or static methods to avoid the need to
    instantiate the class.

    """

    _ORIGIN = "built-in"

    @classmethod
    def find_spec(cls, fullname, path=None, target=None):
        import _sys
        if _sys._is_builtin_module(fullname):
            from importlib.machinery import ModuleSpec
            return ModuleSpec(fullname, cls, origin=cls._ORIGIN)
        return None

    @staticmethod
    def create_module(spec):
        return None

    @staticmethod
    def exec_module(module):
        pass

    @classmethod
    def get_code(cls, fullname):
        """Return None as built-in modules do not have code objects."""
        return None

    @classmethod
    def get_source(cls, fullname):
        """Return None as built-in modules do not have source code."""
        return None

    @classmethod
    def is_package(cls, fullname):
        """Return False as built-in modules are never packages."""
        return False


class FrozenImporter:
    """Meta path import for frozen modules.

    All methods are either class or static methods to avoid the need to
    instantiate the class.

    """

    _ORIGIN = "frozen"

    @classmethod
    def find_spec(cls, fullname, path=None, target=None):
        return None

    @staticmethod
    def create_module(spec):
        return None

    @staticmethod
    def exec_module(module):
        pass

    @classmethod
    def get_code(cls, fullname):
        raise ImportError('{!r} is not a frozen module'.format(fullname), name=fullname)

    @classmethod
    def get_source(cls, fullname):
        return None

    @classmethod
    def is_package(cls, fullname):
        raise ImportError('{!r} is not a frozen module'.format(fullname), name=fullname)
