Fixtures da suíte `greenlet/tests`

Origem e licenças:

- `greenlet/tests/`: os testes do greenlet 3.5.6, copiados da wheel do PyPI (a que o oráculo instala).
  Licença do projeto em `LICENSE.greenlet` (MIT) e `LICENSE.greenlet.PSF` (PSF), as mesmas do dist-info da wheel.
- `objgraph.py`: o objgraph 3.6.2 do PyPI, sem alteração, que o `leakcheck.py` do greenlet importa.
  Licença em `LICENSE.objgraph` (MIT, Marius Gedminas).
- `psutil.py`: NÃO é o pacote psutil. É um mínimo próprio, escrito para este repositório, que cobre só o que a suíte
  usa (`Process().memory_full_info().uss`) lendo `/proc/<pid>/smaps_rollup`.

O teste `greenlet_test_suite_runs_like_the_oracle` (`crates/host/tests/greenlet.rs`) escreve esta árvore na sandbox e
roda `unittest discover`; o resultado de referência (172 testes, 6 pulados) foi medido no CPython 3.13.5 do oráculo.
