import os
import time

# O estado do `get_clock` do libuuid (`gen_uuid.c`): o último instante entregue em `(segundos, microssegundos)`, o
# ajuste de 100 ns para pedidos no mesmo microssegundo, a sequência de relógio e o nó (6 bytes).
_state = {'last': (0, 0), 'adjustment': 0, 'clock_seq': None, 'node': None}
_MAX_ADJUSTMENT = 10


def _random_clock_seq():
    while True:
        clock_seq = int.from_bytes(os.urandom(2), 'little') & 0x3FFF
        if clock_seq != 0:
            return clock_seq


def generate_time_safe():
    """O `uuid_generate_time_safe` do libuuid: os 16 bytes de um UUID de versão 1 e o indicador de segurança
    (`-1` quando o arquivo de estado do relógio não pode ser usado, como num contêiner sem `/var/lib/libuuid`)."""
    state = _state
    if state['node'] is None:
        # Sem placa de rede com endereço, o libuuid sorteia o nó e liga o bit de multicast.
        node = bytearray(os.urandom(6))
        node[0] |= 0x01
        state['node'] = bytes(node)
    if state['clock_seq'] is None:
        state['clock_seq'] = _random_clock_seq()
        seconds, micros = divmod(time.time_ns() // 1000, 1_000_000)
        state['last'] = (seconds - 1, micros)
    while True:
        now = time.time_ns() // 1000
        seconds, micros = divmod(now, 1_000_000)
        last = state['last']
        if (seconds, micros) < last:
            state['clock_seq'] = (state['clock_seq'] + 1) & 0x3FFF
            if state['clock_seq'] == 0:
                state['clock_seq'] = 1
            state['adjustment'] = 0
            state['last'] = (seconds, micros)
        elif (seconds, micros) == last:
            if state['adjustment'] >= _MAX_ADJUSTMENT:
                continue
            state['adjustment'] += 1
        else:
            state['adjustment'] = 0
            state['last'] = (seconds, micros)
        break
    clock_reg = micros * 10 + state['adjustment'] + seconds * 10_000_000 + 0x01B21DD213814000
    clock_high = clock_reg >> 32
    time_low = clock_reg & 0xFFFFFFFF
    time_mid = clock_high & 0xFFFF
    time_hi_and_version = ((clock_high >> 16) & 0x0FFF) | 0x1000
    clock_seq = state['clock_seq'] | 0x8000
    data = (time_low.to_bytes(4, 'big') + time_mid.to_bytes(2, 'big') + time_hi_and_version.to_bytes(2, 'big')
            + clock_seq.to_bytes(2, 'big') + state['node'])
    return data, -1
