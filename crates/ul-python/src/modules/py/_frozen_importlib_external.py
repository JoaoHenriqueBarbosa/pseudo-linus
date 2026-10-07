"""_frozen_importlib_external: a importação a partir de caminhos (`PathFinder` de `sys.meta_path`)."""


class PathFinder:
    """Meta path finder for sys.path and package __path__ attributes."""

    @staticmethod
    def invalidate_caches():
        """Call the invalidate_caches() method on all path entry finders
        stored in sys.path_importer_caches (where implemented)."""
        import sys
        for finder in list(sys.path_importer_cache.values()):
            if finder is not None and hasattr(finder, 'invalidate_caches'):
                finder.invalidate_caches()

    @classmethod
    def find_spec(cls, fullname, path=None, target=None):
        """Try to find a spec for 'fullname' on sys.path or 'path'."""
        import importlib.util
        if path is None:
            try:
                return importlib.util.find_spec(fullname)
            except (ImportError, ValueError):
                return None
        import os.path
        from importlib.machinery import ModuleSpec, SourceFileLoader
        leaf = fullname.rpartition('.')[2]
        for entry in path:
            init = os.path.join(entry, leaf, '__init__.py')
            if os.path.isfile(init):
                spec = ModuleSpec(fullname, SourceFileLoader(fullname, init), origin=init, is_package=True)
                spec.submodule_search_locations = [os.path.join(entry, leaf)]
                spec._set_fileattr = True
                return spec
            file = os.path.join(entry, leaf + '.py')
            if os.path.isfile(file):
                spec = ModuleSpec(fullname, SourceFileLoader(fullname, file), origin=file)
                spec._set_fileattr = True
                return spec
        return None

    @staticmethod
    def find_distributions(*args, **kwargs):
        """
        Find distributions.

        Return an iterable of all Distribution instances capable of
        loading the metadata for packages matching ``context.name``
        (or all names if ``None`` indicated) along the paths in the list
        of directories ``context.path``.
        """
        from importlib.metadata import MetadataPathFinder
        return MetadataPathFinder.find_distributions(*args, **kwargs)
