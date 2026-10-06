"""The asyncio package, tracking PEP 3156."""

# flake8: noqa

import sys

# This relies on each of the submodules having an __all__ variable.
from .base_events import *
from .coroutines import *
from .events import *
from .exceptions import *
from .futures import *
from .locks import *
from .queues import *
from .runners import *
from .subprocess import *
from .tasks import *
from .taskgroups import *
from .timeouts import *
from .threads import *
from .unix_events import *

__all__ = (base_events.__all__ +
           coroutines.__all__ +
           events.__all__ +
           exceptions.__all__ +
           futures.__all__ +
           locks.__all__ +
           queues.__all__ +
           runners.__all__ +
           subprocess.__all__ +
           tasks.__all__ +
           taskgroups.__all__ +
           threads.__all__ +
           timeouts.__all__ +
           unix_events.__all__)

_DefaultEventLoopPolicy = unix_events.DefaultEventLoopPolicy
