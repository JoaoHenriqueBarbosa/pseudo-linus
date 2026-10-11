"""greenlet.getcurrent() chamado do __del__ de um ciclo que só a finalização do interpretador coleta: o C do 3.5.6
responde RuntimeError, porque o greenlet principal já não existe."""

import gc
import sys

import greenlet

gc.disable()


class Cleanup:
    def __del__(self):
        try:
            print('finalizing', sys.is_finalizing())
            sys.stdout.flush()
            greenlet.getcurrent()
        except BaseException as e:
            print('raised', type(e).__name__, e)


a = Cleanup()
b = {'ref': a}
a._cycle = b
del a, b
