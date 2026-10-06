"""importlib.metadata enxuto: o sandbox não instala distribuições, então a busca nunca acha nenhuma."""

__all__ = ['PackageNotFoundError', 'Distribution', 'distribution', 'distributions', 'metadata', 'version',
           'entry_points', 'files', 'requires', 'packages_distributions']


class PackageNotFoundError(ModuleNotFoundError):
    """A distribution was not found."""

    def __str__(self):
        return "No package metadata was found for %s" % self.name

    @property
    def name(self):
        return self.args[0] if self.args else None


class Distribution:
    @classmethod
    def from_name(cls, name):
        raise PackageNotFoundError(name)

    @classmethod
    def discover(cls, **kwargs):
        return iter(())


def distribution(distribution_name):
    return Distribution.from_name(distribution_name)


def distributions(**kwargs):
    return iter(())


def metadata(distribution_name):
    raise PackageNotFoundError(distribution_name)


def version(distribution_name):
    raise PackageNotFoundError(distribution_name)


def entry_points(**params):
    return [] if not params else ()


def files(distribution_name):
    raise PackageNotFoundError(distribution_name)


def requires(distribution_name):
    raise PackageNotFoundError(distribution_name)


def packages_distributions():
    return {}
