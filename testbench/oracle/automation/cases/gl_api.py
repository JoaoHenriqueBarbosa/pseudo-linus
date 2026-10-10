"""greenlet 3.5.6: o contrato que o módulo nativo do ul-python precisa reproduzir (superfície, `switch`, `throw`,
`parent`, `dead`, `run`, `settrace`, `GreenletExit`, erros e o padrão que o Playwright usa). Sem endereços na saída."""
import gc, sys, threading
import greenlet
from greenlet import GreenletExit, getcurrent, greenlet as G, gettrace, settrace
from common import step_sync


def case(label, function):
    step_sync(label, function)


def surface():
    return [sorted(n for n in dir(greenlet) if not n.startswith("_")), greenlet.__version__,
            sorted(n for n in dir(G) if not n.startswith("_")), G.__mro__[1:], GreenletExit.__mro__[1:3], greenlet.error.__mro__[1:3]]


def ping_pong():
    log = []

    def body(first):
        log.append(("start", first))
        value = main.switch("a")
        log.append(("got", value))
        value = main.switch("b", "c")
        log.append(("got2", value))
        return "fim"

    main = getcurrent()
    g = G(body)
    log.append(g.switch(1))
    log.append(g.switch(2))
    log.append(g.switch())
    log.append(g.dead)
    return log


def args_shapes():
    main = getcurrent()
    seen = []

    def body(*args, **kwargs):
        seen.append([args, kwargs])
        seen.append(main.switch())
        seen.append(main.switch(1))
        return None

    g = G(body)
    first = g.switch(1, 2, x=3)
    second = g.switch()
    third = g.switch(5, 6)
    return [first, second, third, seen]


def raised(function):
    """Tipo e mensagem da exceção que a chamada levanta (ou `"sem erro"`)."""
    try:
        function()
    except BaseException as error:
        return [type(error).__name__, str(error)]
    return "sem erro"


def parents():
    main = getcurrent()
    g = G(lambda: None)
    out = [main.parent, g.parent is main, g.dead, bool(g), g.gr_frame, g.run is not None]
    h = G(lambda: None, parent=g)
    out.append(h.parent is g)

    def set_parent(target, value):
        target.parent = value

    out.append(raised(lambda: set_parent(g, h)))
    out.append(raised(lambda: set_parent(main, g)))
    out.append(raised(lambda: set_parent(g, 5)))
    out.append(raised(lambda: set_parent(g, g)))
    return out


def return_to_parent():
    main = getcurrent()
    g = G(lambda: "valor")
    out = [g.switch(), g.dead, bool(g)]
    out.append(g.switch())
    child = G(lambda: (_ for _ in ()).throw(KeyError("k")))
    try:
        child.switch()
    except KeyError as error:
        out.append(["propagou", repr(error)])
    return out


def throw_cases():
    main = getcurrent()
    out = []

    def body():
        try:
            main.switch("dentro")
        except ValueError as error:
            out.append(["capturou", str(error)])
            return "terminou"

    g = G(body)
    out.append(g.switch())
    out.append(g.throw(ValueError("x")))
    fresh = G(lambda: "nunca roda")
    try:
        fresh.throw(RuntimeError("r"))
    except RuntimeError as error:
        out.append(["fresh", str(error), fresh.dead])
    exit_greenlet = G(lambda: main.switch("preso"))
    exit_greenlet.switch()
    result = exit_greenlet.throw()
    out.append([type(result).__name__, exit_greenlet.dead])
    return out


def trace():
    events = []
    main = getcurrent()
    old = settrace(lambda event, args: events.append([event, args[0] is main, args[1] is main]))
    g = G(lambda: main.switch("t"))
    g.switch()
    g.throw(GreenletExit)
    settrace(old)
    return [events, gettrace() is old]


def subclass_run():
    class Mine(G):
        def run(self, *args):
            return ["run", args]

    return Mine().switch(1, 2)


def errors():
    out = []
    g = G(lambda: None)
    g.switch()

    def set_run():
        g.run = 1

    out.append(raised(set_run))
    holder = []

    def other():
        holder.append(G(lambda: None))

    thread = threading.Thread(target=other)
    thread.start()
    thread.join()
    out.append(raised(lambda: holder[0].switch()))
    out.append(raised(lambda: G(5)))
    out.append(raised(lambda: G(5).switch()))
    out.append(raised(lambda: G().switch()))
    out.append(raised(lambda: G(lambda: None, parent=5)))
    out.append(raised(lambda: getcurrent().throw(ValueError("principal"))))
    out.append(raised(lambda: G(lambda: 1 / 0).switch()))
    out.append(raised(lambda: getcurrent().switch("a", "b")))
    return out


def garbage_collection():
    main = getcurrent()
    events = []

    def body():
        try:
            main.switch()
        finally:
            events.append("finally")

    g = G(body)
    g.switch()
    del g
    gc.collect()
    return events


def dispatcher_pattern():
    """O padrão do `playwright.sync_api`: um greenlet despachante, o principal chamando `switch` e voltando."""
    main = getcurrent()
    log = []

    def dispatcher():
        for item in ["a", "b", "c"]:
            log.append(item)
            main.switch(item.upper())

    d = G(dispatcher)
    out = [d.switch(), d.switch(), d.switch()]
    return [out, log, d.dead, d.switch() is None, d.dead]


def context_and_frames():
    main = getcurrent()
    g = G(lambda: main.switch())
    g.switch()
    return [g.gr_frame is not None, type(g.gr_context).__name__ if g.gr_context is not None else None, g.dead, bool(g)]


for label, function in [("surface", surface), ("ping-pong", ping_pong), ("args", args_shapes), ("parents", parents),
                        ("return", return_to_parent), ("throw", throw_cases), ("trace", trace), ("subclass", subclass_run),
                        ("errors", errors), ("gc", garbage_collection), ("dispatcher", dispatcher_pattern),
                        ("frames", context_and_frames)]:
    case(label, function)
