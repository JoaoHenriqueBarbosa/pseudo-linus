"""Módulo `_queue`: a `SimpleQueue` e a exceção `Empty` que o `queue` importa."""

from collections import deque as _deque
from types import GenericAlias as _GenericAlias


class Empty(Exception):
    """Exception raised by Queue.get(block=0)/get_nowait()."""


class SimpleQueue:
    """Simple, unbounded, reentrant FIFO queue."""

    __slots__ = ('_items', '_count')

    def __init__(self):
        import threading
        self._items = _deque()
        self._count = threading.Semaphore(0)

    def put(self, item, block=True, timeout=None):
        """Put the item on the queue.

The optional 'block' and 'timeout' arguments are ignored, as this method
never blocks.  They are provided for compatibility with the queue.Queue
class."""
        self._items.append(item)
        self._count.release()

    def get(self, block=True, timeout=None):
        """Remove and return an item from the queue.

If optional args 'block' is true and 'timeout' is None (the default),
block if necessary until an item is available. If 'timeout' is
a non-negative number, it blocks at most 'timeout' seconds and raises
the Empty exception if no item was available within that time.
Otherwise ('block' is false), return an item if one is immediately
available, else raise the Empty exception ('timeout' is ignored
in that case)."""
        if timeout is not None and timeout < 0:
            raise ValueError("'timeout' must be a non-negative number")
        if not self._count.acquire(block, timeout):
            raise Empty
        return self._items.popleft()

    def put_nowait(self, item):
        """Put an item into the queue without blocking.

This is exactly equivalent to `put(item, block=False)` and is only provided
for compatibility with the Queue class."""
        return self.put(item, block=False)

    def get_nowait(self):
        """Remove and return an item from the queue without blocking.

Only get an item if one is immediately available. Otherwise
raise the Empty exception."""
        return self.get(block=False)

    def empty(self):
        """Return True if the queue is empty, False otherwise (not reliable!)."""
        return len(self._items) == 0

    def qsize(self):
        """Return the approximate size of the queue (not reliable!)."""
        return len(self._items)

    __class_getitem__ = classmethod(_GenericAlias)
