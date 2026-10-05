# dc: casos que divergem do oráculo

## dc-strings

Entrada: `{"argv": ["dc", "-e", "[hello]p [world]n 10an [abc]Zp [x[y]z]p 3.25Xp 123.45Zp 0Zp"]}`

Esperado (Debian real):
```
stdout: 'hello\nworld\n3\nx[y]z\n2\n5\n1\n'
stderr: ''
exit: 0
```

## dc-line-wrap

Entrada: `{"argv": ["dc", "-e", "2 400^p 3 200^ d p f"]}`

Esperado (Debian real):
```
stdout: '258224987808690858965591917200301187432970579282922351283065935654064\\\n7622016841194629645353280137831435903171972747493376\n265613988875874769338781322035779626829233452653394495974574961739092\\\n490901302182994384699044001\n265613988875874769338781322035779626829233452653394495974574961739092\\\n490901302182994384699044001\n265613988875874769338781322035779626829233452653394495974574961739092\\\n490901302182994384699044001\n258224987808690858965591917200301187432970579282922351283065935654064\\\n7622016841194629645353280137831435903171972747493376\n'
stderr: ''
exit: 0
```

## dc-errors

Entrada: `{"argv": ["dc", "-e", "p + 1 0/ 1 0% c 1 0~ c _4v f c lz Lz 1k _1k 1i 1o g [x]1+ zp"]}`

Esperado (Debian real):
```
stdout: "'g' (0147) unimplemented\n3\n"
stderr: "dc: stack empty\ndc: stack empty\ndc: divide by zero\ndc: remainder by zero\ndc: divide by zero\ndc: square root of negative number\ndc: stack register 'z' (0172) is empty\ndc: scale must be a nonnegative number\ndc: input base must be a number between 2 and 16 (inclusive)\ndc: output base must be a number greater than 1\ndc: dc: non-numeric value\n"
exit: 0
```

## dc-missing-file

Entrada: `{"argv": ["dc", "nope.dc"]}`

Esperado (Debian real):
```
stdout: ''
stderr: 'dc: Could not open file nope.dc\n'
exit: 0
```

## dc-version-help

Entrada: `{"script": "dc -V; dc --help; dc -x; echo $?"}`

Esperado (Debian real):
```
stdout: 'dc (GNU bc 1.07.1) 1.4.1\n\nCopyright 1994, 1997, 1998, 2000, 2001, 2003-2006, 2008, 2010, 2012-2017 Free Software Foundation, Inc.\nThis is free software; see the source for copying conditions.  There is NO\nwarranty; not even for MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE,\nto the extent permitted by law.\nUsage: dc [OPTION] [file ...]\n  -e, --expression=EXPR    evaluate expression\n  -f, --file=FILE          evaluate contents of file\n  -h, --help               display this help and exit\n  -V, --version            output version information and exit\n\nEmail bug reports to:  bug-dc@gnu.org .\nEmail bug reports to:  bug-dc@gnu.org .\n1\n'
stderr: "dc: invalid option -- 'x'\nUsage: dc [OPTION] [file ...]\n  -e, --expression=EXPR    evaluate expression\n  -f, --file=FILE          evaluate contents of file\n  -h, --help               display this help and exit\n  -V, --version            output version information and exit\n\n"
exit: 0
```

