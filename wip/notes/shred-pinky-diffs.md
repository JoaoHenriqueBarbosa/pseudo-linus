# shred e pinky: saída esperada do Debian real nos casos que divergem

## shred-zero-only

Entrada: `{"script": "shred -n 0 -z -x a.txt; od -An -tx1 a.txt", "files": {"a.txt": "segredo\n"}}`

```
stdout: ' 00 00 00 00 00 00 00 00\n'
stderr: ''
exit: 0
files: {"a.txt": {"kind": "file", "mode": 420, "size": 8, "sha256": "af5570f5a1810b7af78caf4bc70a660f0df51e42baf91d4de5b2328de0e83dfc", "data": "\u0000\u0000\u0000\u0000\u0000\u0000\u0000\u0000"}}
```

## shred-zero-verbose

Entrada: `{"script": "shred -v -n 0 -z -x a.txt; od -An -c a.txt", "files": {"a.txt": "abc"}}`

```
stdout: '  \\0  \\0  \\0\n'
stderr: 'shred: a.txt: pass 1/1 (000000)...\n'
exit: 0
files: {"a.txt": {"kind": "file", "mode": 420, "size": 3, "sha256": "709e80c88487a2411e1ee4dfb9f22a861492d20c4765150c0c794abd70f8147c", "data": "\u0000\u0000\u0000"}}
```

## shred-verbose-random-source

Entrada: `{"argv": ["shred", "-v", "-n", "1", "--random-source=/dev/zero", "-x", "a.txt"], "files": {"a.txt": "dados\n"}}`

```
stdout: ''
stderr: 'shred: a.txt: pass 1/1 (random)...\n'
exit: 0
files: {"a.txt": {"kind": "file", "mode": 420, "size": 6, "sha256": "b0f66adc83641586656866813fd9dd0b8ebb63796075661ba45d1aa8089e1d44", "data": "\u0000\u0000\u0000\u0000\u0000\u0000"}}
```

## shred-verbose-default-three-random

Entrada: `{"argv": ["shred", "-v", "--random-source=/dev/zero", "-x", "a.txt"], "files": {"a.txt": "x\n"}}`

```
stdout: ''
stderr: 'shred: a.txt: pass 1/3 (random)...\nshred: a.txt: pass 2/3 (random)...\nshred: a.txt: pass 3/3 (random)...\n'
exit: 0
files: {"a.txt": {"kind": "file", "mode": 420, "size": 2, "sha256": "96a296d224f285c67bee93c30f8a309157f0daa35dc5b87e410b78630a09cfc7", "data": "\u0000\u0000"}}
```

## shred-verbose-remove-unlink

Entrada: `{"script": "shred -v -n 0 --remove=unlink f.txt; ls", "files": {"f.txt": "x\n", "g.txt": "y\n"}}`

```
stdout: 'g.txt\n'
stderr: 'shred: f.txt: removing\nshred: f.txt: removed\n'
exit: 0
files: {"g.txt": {"kind": "file", "mode": 420, "size": 2, "sha256": "3bb2abb69ebb27fbfe63c7639624c6ec5e331b841a5bc8c3ebc10b9285e90877", "data": "y\n"}}
```

## shred-size-exact

Entrada: `{"script": "shred -n 0 -z -s 3 a.txt; od -An -c a.txt", "files": {"a.txt": "abcdef\n"}}`

```
stdout: '  \\0  \\0  \\0   d   e   f  \\n\n'
stderr: ''
exit: 0
files: {"a.txt": {"kind": "file", "mode": 420, "size": 7, "sha256": "3634422db40109bcffe04ea4da7786328ec97c8950e64f988b726f2ae82db85c", "data": "\u0000\u0000\u0000def\n"}}
```

## shred-nonexistent

Entrada: `{"argv": ["shred", "-n", "0", "nope.txt", "a.txt"], "files": {"a.txt": "x\n"}}`

```
stdout: ''
stderr: 'shred: nope.txt: failed to open for writing: No such file or directory\n'
exit: 1
files: {"a.txt": {"kind": "file", "mode": 420, "size": 2, "sha256": "73cb3858a687a8494ca3323053016282f3dad39d42cf62ca4e79dda2aac7d9ac", "data": "x\n"}}
```

## pinky-heading

Entrada: `{"argv": ["pinky"]}`

```
stdout: 'Login    Name                 TTY      Idle   When             Where\n'
stderr: ''
exit: 0
files: {}
```

## pinky-s

Entrada: `{"argv": ["pinky", "-s"]}`

```
stdout: 'Login    Name                 TTY      Idle   When             Where\n'
stderr: ''
exit: 0
files: {}
```

## pinky-user-filter

Entrada: `{"argv": ["pinky", "root", "nobody"]}`

```
stdout: 'Login    Name                 TTY      Idle   When             Where\n'
stderr: ''
exit: 0
files: {}
```

## pinky-l-no-user

Entrada: `{"argv": ["pinky", "-l"]}`

```
stdout: ''
stderr: "pinky: no username specified; at least one must be specified when using -l\nTry 'pinky --help' for more information.\n"
exit: 1
files: {}
```

## pinky-l-root

Entrada: `{"argv": ["pinky", "-l", "root"]}`

```
stdout: 'Login name: root                        In real life:  root\nDirectory: /root                        Shell:  /bin/bash\n\n'
stderr: ''
exit: 0
files: {}
```

## pinky-lb-root

Entrada: `{"argv": ["pinky", "-lb", "root"]}`

```
stdout: 'Login name: root                        In real life:  root\n\n'
stderr: ''
exit: 0
files: {}
```

## pinky-lhp-unknown

Entrada: `{"argv": ["pinky", "-lhp", "ninguem-aqui", "root"]}`

```
stdout: 'Login name: ninguem-aqui                In real life:  ???\nLogin name: root                        In real life:  root\nDirectory: /root                        Shell:  /bin/bash\n\n'
stderr: ''
exit: 0
files: {}
```

