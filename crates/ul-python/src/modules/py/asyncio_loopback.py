"""Rede do laço asyncio no sandbox: transportes de stream sobre as pontas em loopback do `_net` e os
métodos do laço que abrem servidores e conexões (`create_server`, `create_connection`, as variantes
Unix). Não há seletor: quando o par escreve, fecha ou conecta, a ponta avisa por um gancho e o laço
agenda a entrega com `call_soon`, na mesma ordem em que o CPython a faria."""

import errno
import socket

import _socket

from . import constants
from . import events
from . import tasks
from . import transports
from . import trsock
from . import futures
from .log import logger

__all__ = ()


class _SelectorSocketTransport(transports.Transport):
    """`_SelectorSocketTransport` sem buffer de escrita: a escrita na ponta do par é imediata."""

    max_size = 256 * 1024

    def __init__(self, loop, sock, protocol, waiter=None, extra=None, server=None):
        super().__init__(extra)
        self._extra['socket'] = trsock.TransportSocket(sock)
        try:
            self._extra['sockname'] = sock.getsockname()
        except OSError:
            self._extra['sockname'] = None
        if 'peername' not in self._extra:
            try:
                self._extra['peername'] = sock.getpeername()
            except OSError:
                self._extra['peername'] = None
        self._loop = loop
        self._sock = sock
        self._endpoint = _socket._fds[sock.fileno()].endpoint
        self._protocol = protocol
        self._server = server
        self._closing = False
        self._conn_lost = 0
        self._eof = False
        self._eof_received = False
        self._paused = False
        self._scheduled = False
        self._low = 16 * 1024
        self._high = 64 * 1024
        self._loop.call_soon(self._protocol.connection_made, self)
        self._loop.call_soon(self._start)
        if waiter is not None:
            self._loop.call_soon(futures._set_result_unless_cancelled, waiter, None)
        if server is not None:
            server._attach(self)

    def __repr__(self):
        info = [self.__class__.__name__]
        if self._sock is None:
            info.append('closed')
        elif self._closing:
            info.append('closing')
        info.append(f'fd={self._sock.fileno()}' if self._sock is not None else 'fd=-1')
        if self._sock is not None:
            info.append('read=idle' if self._paused or self._closing else 'read=polling')
            info.append('write=<idle, bufsize=0>')
        return '<{}>'.format(' '.join(info))

    # --- leitura -------------------------------------------------------------------------------

    def _start(self):
        if self._conn_lost:
            return
        self._endpoint.hooks.append(self._wake)
        self._wake()

    def _wake(self):
        if self._scheduled or self._conn_lost or self._paused or self._loop is None or self._loop.is_closed():
            return
        ep = self._endpoint
        if not (ep.rx or ep.reset or (ep.rx_eof and not self._eof_received)):
            return
        self._scheduled = True
        self._loop._add_io(self._pump)

    def _pump(self):
        self._scheduled = False
        if self._conn_lost or self._paused:
            return
        ep = self._endpoint
        if ep.rx or ep.reset:
            try:
                data = ep.read(self.max_size)
            except (BlockingIOError, InterruptedError):
                return
            except (SystemExit, KeyboardInterrupt):
                raise
            except BaseException as exc:
                self._fatal_error(exc, 'Fatal read error on socket transport')
                return
            if data:
                # O seletor do CPython é disparado por nível: se ainda há o que ler, a próxima leitura é
                # enfileirada antes de entregar estes dados (e portanto antes do que eles provocarem).
                if ep.rx or ep.reset or (ep.rx_eof and not self._eof_received):
                    self._wake()
                try:
                    self._protocol.data_received(data)
                except (SystemExit, KeyboardInterrupt):
                    raise
                except BaseException as exc:
                    self._fatal_error(exc, 'Fatal error: protocol.data_received() call failed.')
                    return
            return
        if ep.rx_eof and not self._eof_received:
            self._eof_received = True
            try:
                keep_open = self._protocol.eof_received()
            except (SystemExit, KeyboardInterrupt):
                raise
            except BaseException as exc:
                self._fatal_error(exc, 'Fatal error: protocol.eof_received() call failed.')
                return
            if not keep_open:
                self.close()

    def pause_reading(self):
        if self._closing or self._paused:
            return
        self._paused = True

    def resume_reading(self):
        if self._closing or not self._paused:
            return
        self._paused = False
        self._wake()

    def is_reading(self):
        return not self._paused and not self._closing

    # --- escrita -------------------------------------------------------------------------------

    def write(self, data):
        if not isinstance(data, (bytes, bytearray, memoryview)):
            raise TypeError(f'data argument must be a bytes-like object, not {type(data).__name__!r}')
        if self._eof:
            raise RuntimeError('Cannot call write() after write_eof()')
        if not data:
            return
        if self._conn_lost:
            if self._conn_lost >= constants.LOG_THRESHOLD_FOR_CONNLOST_WRITES:
                logger.warning('socket.send() raised exception.')
            self._conn_lost += 1
            return
        try:
            self._endpoint.write(bytes(data))
        except (SystemExit, KeyboardInterrupt):
            raise
        except BaseException as exc:
            self._fatal_error(exc, 'Fatal write error on socket transport')

    def can_write_eof(self):
        return True

    def write_eof(self):
        if self._closing or self._eof:
            return
        self._eof = True
        self._endpoint.shutdown_write()

    def get_write_buffer_size(self):
        return 0

    def get_write_buffer_limits(self):
        return (self._low, self._high)

    def set_write_buffer_limits(self, high=None, low=None):
        if high is None:
            high = 64 * 1024 if low is None else 4 * low
        if low is None:
            low = high // 4
        if not high >= low >= 0:
            raise ValueError(f'high ({high!r}) must be >= low ({low!r}) must be >= 0')
        self._high = high
        self._low = low

    # --- encerramento --------------------------------------------------------------------------

    def get_protocol(self):
        return self._protocol

    def set_protocol(self, protocol):
        self._protocol = protocol

    def is_closing(self):
        return self._closing

    def close(self):
        if self._closing:
            return
        self._closing = True
        if not self._conn_lost:
            self._conn_lost += 1
            self._loop.call_soon(self._call_connection_lost, None)

    def abort(self):
        self._force_close(None)

    def _force_close(self, exc):
        if self._conn_lost:
            return
        self._closing = True
        self._conn_lost += 1
        self._loop.call_soon(self._call_connection_lost, exc)

    def _fatal_error(self, exc, message='Fatal error on transport'):
        if isinstance(exc, OSError):
            if self._loop.get_debug():
                logger.debug('%r: %s', self, message, exc_info=True)
        else:
            self._loop.call_exception_handler({
                'message': message,
                'exception': exc,
                'transport': self,
                'protocol': self._protocol,
            })
        self._force_close(exc)

    def _call_connection_lost(self, exc):
        try:
            if self._protocol is not None:
                self._protocol.connection_lost(exc)
        finally:
            try:
                self._endpoint.hooks.remove(self._wake)
            except ValueError:
                pass
            self._sock.close()
            self._sock = None
            self._protocol = None
            self._loop = None
            server = self._server
            if server is not None:
                server._detach(self)
                self._server = None


class NetworkMixin:
    """Métodos de rede do `BaseEventLoop`."""

    def _add_io(self, callback, *args):
        """Enfileira um evento de I/O: roda no início da próxima iteração, depois dos prontos."""
        self._io_ready.append(events.Handle(callback, args, self, None))

    def _make_socket_transport(self, sock, protocol, waiter=None, *, extra=None, server=None):
        return _SelectorSocketTransport(self, sock, protocol, waiter, extra, server)

    async def getaddrinfo(self, host, port, *, family=0, type=0, proto=0, flags=0):
        return socket.getaddrinfo(host, port, family, type, proto, flags)

    async def create_connection(
            self, protocol_factory, host=None, port=None, *, ssl=None, family=0, proto=0, flags=0, sock=None,
            local_addr=None, server_hostname=None, ssl_handshake_timeout=None, ssl_shutdown_timeout=None,
            happy_eyeballs_delay=None, interleave=None, all_errors=False):
        if server_hostname is not None and not ssl:
            raise ValueError('server_hostname is only meaningful with ssl')
        if server_hostname is None and ssl:
            if not host:
                raise ValueError('You must set server_hostname when using ssl without a host')
            server_hostname = host
        if ssl_handshake_timeout is not None and not ssl:
            raise ValueError('ssl_handshake_timeout is only meaningful with ssl')
        if ssl_shutdown_timeout is not None and not ssl:
            raise ValueError('ssl_shutdown_timeout is only meaningful with ssl')
        if ssl:
            raise NotImplementedError('TLS não é suportado pelo laço de eventos do sandbox')
        if happy_eyeballs_delay is not None and interleave is None:
            interleave = 1
        if host is not None or port is not None:
            if sock is not None:
                raise ValueError('host/port and sock can not be specified at the same time')
            infos = await self.getaddrinfo(host, port, family=family, type=socket.SOCK_STREAM, proto=proto,
                                           flags=flags)
            if not infos:
                raise OSError('getaddrinfo() returned empty list')
            errors = []
            for af, socktype, sock_proto, _, address in infos:
                candidate = None
                try:
                    candidate = socket.socket(af, socktype, sock_proto)
                    candidate.setblocking(False)
                    candidate.connect(address)
                except OSError as exc:
                    if candidate is not None:
                        candidate.close()
                    if exc.errno is not None and candidate is not None:
                        exc = OSError(exc.errno, f'Connect call failed {address}')
                    errors.append(exc)
                except:
                    if candidate is not None:
                        candidate.close()
                    raise
                else:
                    sock = candidate
                    await self._connect_hops()
                    break
                await self._connect_hops()
            if sock is None:
                if all_errors:
                    raise ExceptionGroup('create_connection failed', errors)
                if len(errors) == 1:
                    raise errors[0]
                model = str(errors[0])
                if all(str(exc) == model for exc in errors):
                    raise errors[0]
                raise OSError('Multiple exceptions: {}'.format(', '.join(str(exc) for exc in errors)))
        else:
            if sock is None:
                raise ValueError('host and port was not specified and no sock specified')
            if sock.type != socket.SOCK_STREAM:
                raise ValueError(f'A Stream Socket was expected, got {sock!r}')
        return await self._create_connection_transport(sock, protocol_factory)

    async def _connect_hops(self):
        # `sock_connect` do CPython: EINPROGRESS e, na iteração seguinte, o callback do escritor (um evento de
        # I/O, depois do aceite do servidor) completa o futuro; a tarefa só acorda na iteração depois.
        fut = self.create_future()
        self._add_io(futures._set_result_unless_cancelled, fut, None)
        await fut

    async def _create_connection_transport(self, sock, protocol_factory):
        sock.setblocking(False)
        protocol = protocol_factory()
        waiter = self.create_future()
        transport = self._make_socket_transport(sock, protocol, waiter)
        try:
            await waiter
        except:
            transport.close()
            raise
        return transport, protocol

    async def create_unix_connection(
            self, protocol_factory, path=None, *, ssl=None, sock=None, server_hostname=None,
            ssl_handshake_timeout=None, ssl_shutdown_timeout=None):
        if ssl:
            raise NotImplementedError('TLS não é suportado pelo laço de eventos do sandbox')
        if server_hostname is not None:
            raise ValueError('server_hostname is only meaningful with ssl')
        if path is not None:
            if sock is not None:
                raise ValueError('path and sock can not be specified at the same time')
            path = socket._fspath(path) if hasattr(socket, '_fspath') else path
            sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM, 0)
            try:
                sock.setblocking(False)
                sock.connect(path)
            except:
                sock.close()
                raise
            await self._connect_hops()
        else:
            if sock is None:
                raise ValueError('no path and sock were specified')
            if sock.family != socket.AF_UNIX or sock.type != socket.SOCK_STREAM:
                raise ValueError(f'A UNIX Domain Stream Socket was expected, got {sock!r}')
        return await self._create_connection_transport(sock, protocol_factory)

    async def create_server(
            self, protocol_factory, host=None, port=None, *, family=socket.AF_UNSPEC, flags=socket.AI_PASSIVE,
            sock=None, backlog=100, ssl=None, reuse_address=None, reuse_port=None, keep_alive=None,
            ssl_handshake_timeout=None, ssl_shutdown_timeout=None, start_serving=True):
        from . import base_events
        if isinstance(ssl, bool):
            raise TypeError('ssl argument must be an SSLContext or None')
        if ssl is not None:
            raise NotImplementedError('TLS não é suportado pelo laço de eventos do sandbox')
        if ssl_handshake_timeout is not None and ssl is None:
            raise ValueError('ssl_handshake_timeout is only meaningful with ssl')
        if ssl_shutdown_timeout is not None and ssl is None:
            raise ValueError('ssl_shutdown_timeout is only meaningful with ssl')
        if host is not None or port is not None:
            if sock is not None:
                raise ValueError('host/port and sock can not be specified at the same time')
            if reuse_address is None:
                reuse_address = True
            sockets = []
            if host == '':
                hosts = [None]
            elif isinstance(host, str) or not hasattr(host, '__iter__'):
                hosts = [host]
            else:
                hosts = host
            infos = []
            for one in hosts:
                for info in await self.getaddrinfo(one, port, family=family, type=socket.SOCK_STREAM, proto=0,
                                                   flags=flags):
                    if info not in infos:
                        infos.append(info)
            completed = False
            try:
                for res in infos:
                    af, socktype, proto, canonname, sa = res
                    try:
                        sock = socket.socket(af, socktype, proto)
                    except OSError:
                        continue
                    sockets.append(sock)
                    if reuse_address:
                        sock.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, True)
                    if reuse_port:
                        sock.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEPORT, True)
                    if keep_alive:
                        sock.setsockopt(socket.SOL_SOCKET, socket.SO_KEEPALIVE, True)
                    if af == socket.AF_INET6:
                        sock.setsockopt(socket.IPPROTO_IPV6, socket.IPV6_V6ONLY, True)
                    try:
                        sock.bind(sa)
                    except OSError as err:
                        if err.errno == errno.EADDRNOTAVAIL:
                            sockets.pop()
                            sock.close()
                            continue
                        msg = ('error while attempting to bind on address %r: %s' % (sa, str(err).lower()))
                        raise OSError(err.errno, msg) from None
                completed = True
            finally:
                if not completed:
                    for sock in sockets:
                        sock.close()
        else:
            if sock is None:
                raise ValueError('Neither host/port nor sock were specified')
            if sock.type != socket.SOCK_STREAM:
                raise ValueError(f'A Stream Socket was expected, got {sock!r}')
            sockets = [sock]
        for sock in sockets:
            sock.setblocking(False)
        server = base_events.Server(self, sockets, protocol_factory, ssl, backlog, ssl_handshake_timeout,
                                    ssl_shutdown_timeout)
        if start_serving:
            server._start_serving()
            await tasks.sleep(0)
        return server

    async def create_unix_server(
            self, protocol_factory, path=None, *, sock=None, backlog=100, ssl=None,
            ssl_handshake_timeout=None, ssl_shutdown_timeout=None, start_serving=True, cleanup_socket=True):
        from . import base_events
        if ssl is not None:
            raise NotImplementedError('TLS não é suportado pelo laço de eventos do sandbox')
        if path is not None:
            if sock is not None:
                raise ValueError('path and sock can not be specified at the same time')
            sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
            try:
                sock.bind(path)
            except OSError as exc:
                sock.close()
                if exc.errno == errno.EADDRINUSE:
                    raise OSError(errno.EADDRINUSE, f'Address {path!r} is already in use') from None
                raise
            except:
                sock.close()
                raise
        else:
            if sock is None:
                raise ValueError('path was not specified, and no sock specified')
            if sock.family != socket.AF_UNIX or sock.type != socket.SOCK_STREAM:
                raise ValueError(f'A UNIX Domain Stream Socket was expected, got {sock!r}')
        sock.setblocking(False)
        server = base_events.Server(self, [sock], protocol_factory, ssl, backlog, ssl_handshake_timeout,
                                    ssl_shutdown_timeout)
        if start_serving:
            server._start_serving()
            await tasks.sleep(0)
        return server

    # --- aceitação -----------------------------------------------------------------------------

    def _start_serving(self, protocol_factory, sock, sslcontext=None, server=None, backlog=100,
                       ssl_handshake_timeout=None, ssl_shutdown_timeout=None):
        listener = _socket._fds[sock.fileno()].listener

        def on_pending():
            if not self.is_closed():
                self._add_io(self._accept_connection, protocol_factory, sock, server, backlog)

        hooks = self.__dict__.setdefault('_listener_hooks', {})
        hooks[sock.fileno()] = (listener, on_pending)
        listener.hooks.append(on_pending)
        if listener.pending:
            on_pending()

    def _stop_serving(self, sock):
        hooks = self.__dict__.get('_listener_hooks', {})
        entry = hooks.pop(sock.fileno(), None)
        if entry is not None:
            listener, hook = entry
            if hook in listener.hooks:
                listener.hooks.remove(hook)
        sock.close()

    def _accept_connection(self, protocol_factory, sock, server, backlog):
        for _ in range(backlog):
            try:
                conn, addr = sock.accept()
                conn.setblocking(False)
            except (BlockingIOError, InterruptedError, ConnectionAbortedError):
                return
            except OSError as exc:
                self.call_exception_handler({
                    'message': 'socket.accept() out of system resource',
                    'exception': exc,
                    'socket': trsock.TransportSocket(sock),
                })
                return
            self.create_task(self._accept_connection2(protocol_factory, conn, {'peername': addr}, server))

    async def _accept_connection2(self, protocol_factory, conn, extra, server):
        protocol = None
        transport = None
        try:
            protocol = protocol_factory()
            waiter = self.create_future()
            transport = self._make_socket_transport(conn, protocol, waiter=waiter, extra=extra, server=server)
            try:
                await waiter
            except BaseException:
                transport.close()
                raise
        except (SystemExit, KeyboardInterrupt):
            raise
        except BaseException as exc:
            self.call_exception_handler({
                'message': 'Error on transport creation for incoming connection',
                'exception': exc,
            })
