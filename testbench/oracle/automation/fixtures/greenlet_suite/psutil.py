"""O mínimo do psutil que a suíte do greenlet usa (`Process().memory_full_info().uss`), lido do /proc como o psutil
faz no Linux: Private_Clean + Private_Dirty de /proc/self/smaps_rollup."""
import collections
import os

_Full = collections.namedtuple('pfullmem', 'rss vms shared text lib data dirty uss pss swap')


class Process:
    def __init__(self, pid=None):
        self.pid = os.getpid() if pid is None else pid

    def memory_full_info(self):
        fields = {}
        with open('/proc/%d/smaps_rollup' % self.pid) as f:
            for line in f:
                key, _, rest = line.partition(':')
                parts = rest.split()
                if parts and parts[-1] == 'kB':
                    fields[key] = int(parts[0]) * 1024
        uss = fields.get('Private_Clean', 0) + fields.get('Private_Dirty', 0)
        return _Full(fields.get('Rss', 0), 0, fields.get('Shared_Clean', 0) + fields.get('Shared_Dirty', 0), 0, 0, 0, 0,
                     uss, fields.get('Pss', 0), fields.get('Swap', 0))
