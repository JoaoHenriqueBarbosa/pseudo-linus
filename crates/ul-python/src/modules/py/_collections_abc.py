"""`_collections_abc`: alias de `collections.abc` (as classes abstratas vivem em `collections.abc` no sandbox)."""

from collections.abc import *
from collections.abc import __dict__ as _ns

__all__ = [name for name in _ns if not name.startswith('_')]
