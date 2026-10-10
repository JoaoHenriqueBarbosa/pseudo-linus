//! IndexedDB no formato do disco do Chromium: o esquema de chaves sobre o LevelDB (`coding`) e o `ValueSerializer`
//! do V8 com o envelope do Blink (`v8`). Os bytes aqui são os que o Chromium 154 do oráculo grava; cada teste cita
//! o trecho medido (`testbench/oracle/measure-idb.sh`, `leveldb-hex.py`, fixtures em `tests/fixtures/idb`).

pub mod coding;
pub mod js_value;
pub mod v8;
