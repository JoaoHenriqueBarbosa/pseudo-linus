//! O banco (`db/db_impl.cc`): abertura e recuperação do log, escrita, leitura, a memtable que vira tabela quando passa
//! do `write_buffer_size`, e a limpeza dos arquivos que a versão não usa mais. O texto do `LOG` segue o do leveldb do
//! Chromium (as mesmas mensagens, o mesmo formato de hora), porque quem abre o diretório do perfil o lê.
//!
//! Tudo roda na thread de quem chama: no leveldb a memtable cheia vira tabela numa thread de fundo, aqui logo depois
//! da escrita que a encheu; os arquivos que restam no disco são os mesmos.

use std::cell::Cell;
use std::cmp::Ordering;
use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use ul_common::time::Civil;

use crate::batch::{Op, WriteBatch};
use crate::compaction::{self, Compaction};
use crate::comparator;
use crate::dbformat::*;
use crate::env::Env;
use crate::log::{read_records, Writer};
use crate::memtable::{Lookup, MemTable};
use crate::table::{Table, TableBuilder, TableOptions};
use crate::version::{set_current_file, VersionSet};
use crate::version_edit::{FileMeta, VersionEdit};
use crate::{Error, Result};

/// As opções que mudam o que o banco faz e os bytes que ele grava.
#[derive(Debug, Clone)]
pub struct Options {
    pub create_if_missing: bool,
    pub write_buffer_size: usize,
    pub max_file_size: usize,
    pub reuse_logs: bool,
    pub table: TableOptions,
    /// A ordem das chaves de usuário: o bytewise, ou o do formato que o banco guarda (`idb_cmp1` no IndexedDB).
    pub comparator: comparator::Shared,
    /// O id da thread que aparece em cada linha do `LOG`, e o da thread de fundo, que escreve as linhas dos flushes
    /// de memtable em execução e das compactações.
    pub thread_id: u64,
    pub background_thread_id: u64,
}

impl Options {
    /// As do leveldb do Chromium para o `Local Storage` (medidas no oráculo): log e MANIFEST reaproveitados e
    /// tabelas com snappy. A thread de fundo tem um id diferente da que abriu o banco (numa medição, `29a` e `29c`).
    pub fn chromium(thread_id: u64) -> Options {
        Options {
            create_if_missing: true,
            write_buffer_size: 64 * 1024,
            max_file_size: 2 << 20,
            reuse_logs: true,
            table: TableOptions::default(),
            comparator: comparator::bytewise(),
            thread_id,
            background_thread_id: thread_id + 2,
        }
    }
}

#[derive(Debug)]
struct LogFile {
    path: String,
    writer: Writer,
}

pub struct Db {
    env: Arc<dyn Env>,
    dir: String,
    options: Options,
    mem: MemTable,
    versions: VersionSet,
    log: Option<LogFile>,
    logfile_number: u64,
    tables: Mutex<HashMap<u64, Arc<Table>>>,
    /// Verdadeiro enquanto o trabalho que no leveldb roda na thread de fundo está em andamento: as linhas do `LOG`
    /// saem com o id dela.
    background: Cell<bool>,
}


/// `InternalKey::DebugString`: `'<chave escapada>' @ <sequência> : <tipo>`.
fn debug_internal_key(key: &[u8]) -> String {
    let (user, sequence, value_type) = match parse_internal_key(key) {
        Some((user, sequence, value_type)) => (user, sequence, value_type as u8),
        None => (key, 0, 0),
    };
    let escaped: String = user.iter().map(|byte| if (b' '..=b'~').contains(byte) { (*byte as char).to_string() } else { format!("\\x{byte:02x}") }).collect();
    format!("'{escaped}' @ {sequence} : {value_type}")
}

fn corruption(message: impl Into<String>) -> Error {
    Error::Corruption(message.into())
}

impl Db {
    pub fn open(env: Arc<dyn Env>, dir: &str, options: Options) -> Result<Db> {
        let _scope = comparator::scoped(&options.comparator);
        env.create_dir_all(dir)?;
        let info_log = info_log_file_name(dir);
        let _ = env.rename(&info_log, &old_info_log_file_name(dir));
        env.write_file(&info_log, b"")?;
        env.lock_file(&lock_file_name(dir))?;
        let mut db = Db {
            env: env.clone(),
            dir: dir.to_owned(),
            versions: VersionSet::new(dir, options.max_file_size, options.reuse_logs),
            options,
            mem: MemTable::new(),
            log: None,
            logfile_number: 0,
            tables: Mutex::new(HashMap::new()),
            background: Cell::new(false),
        };
        db.recover_and_start()?;
        Ok(db)
    }

    /// Uma linha do `LOG`: `AAAA/MM/DD-HH:MM:SS.mmm <thread em hexa> <texto>`.
    fn info(&self, message: &str) {
        let micros = self.env.now_micros();
        let civil = Civil::from_secs((micros / 1_000_000) as i64);
        let thread = if self.background.get() { self.options.background_thread_id } else { self.options.thread_id };
        let line = format!(
            "{:04}/{:02}/{:02}-{:02}:{:02}:{:02}.{:03} {:x} {}",
            civil.year,
            civil.mon,
            civil.mday,
            civil.hour,
            civil.min,
            civil.sec,
            (micros % 1_000_000) / 1000,
            thread,
            message
        );
        let line = if line.ends_with('\n') { line } else { line + "\n" };
        let _ = self.env.append_file(&info_log_file_name(&self.dir), line.as_bytes());
    }

    /// Roda `work` como o leveldb roda o trabalho de fundo: noutra thread, que é a que assina as linhas do `LOG`.
    fn in_background<T>(&mut self, work: impl FnOnce(&mut Db) -> T) -> T {
        self.background.set(true);
        let result = work(self);
        self.background.set(false);
        result
    }

    fn recover_and_start(&mut self) -> Result<()> {
        let mut edit = VersionEdit::default();
        let save_manifest = self.recover(&mut edit)?;
        if self.log.is_none() {
            let number = self.versions.new_file_number();
            let path = log_file_name(&self.dir, number);
            self.env.write_file(&path, b"")?;
            edit.log_number = Some(number);
            self.logfile_number = number;
            self.log = Some(LogFile { path, writer: Writer::new() });
            self.mem = MemTable::new();
        }
        if save_manifest {
            edit.prev_log_number = Some(0);
            edit.log_number = Some(self.logfile_number);
            self.versions.log_and_apply(self.env.as_ref(), &mut edit)?;
        }
        self.delete_obsolete_files();
        self.in_background(Db::compact_if_needed)
    }

    /// `DBImpl::Recover`: o `CURRENT`, o MANIFEST e os logs que a versão ainda não cobre. Devolve se o MANIFEST precisa
    /// de uma entrada nova.
    fn recover(&mut self, edit: &mut VersionEdit) -> Result<bool> {
        if !self.env.exists(&current_file_name(&self.dir)) {
            if !self.options.create_if_missing {
                return Err(Error::InvalidArgument(format!("{} does not exist (create_if_missing is false)", self.dir)));
            }
            self.info(&format!("Creating DB {} since it was missing.", self.dir));
            self.new_db()?;
        }
        let mut messages = Vec::new();
        let mut save_manifest = self.versions.recover(self.env.as_ref(), &mut |message| messages.push(message.to_owned()))?;
        for message in messages {
            self.info(&message);
        }
        let min_log = self.versions.log_number();
        let prev_log = self.versions.prev_log_number();
        let mut expected = self.versions.live_files();
        let mut logs = Vec::new();
        for name in self.env.list_dir(&self.dir)? {
            if let Some((number, file_type)) = parse_file_name(&name) {
                expected.remove(&number);
                if file_type == FileType::Log && (number >= min_log || number == prev_log) {
                    logs.push(number);
                }
            }
        }
        if let Some(first) = expected.iter().next() {
            return Err(corruption(format!("{} missing files; e.g. {}", expected.len(), table_file_name(&self.dir, *first))));
        }
        logs.sort_unstable();
        let mut max_sequence = 0;
        for (index, number) in logs.iter().enumerate() {
            self.recover_log_file(*number, index == logs.len() - 1, &mut save_manifest, edit, &mut max_sequence)?;
            self.versions.mark_file_number_used(*number);
        }
        if self.versions.last_sequence() < max_sequence {
            self.versions.set_last_sequence(max_sequence);
        }
        Ok(save_manifest)
    }

    fn new_db(&mut self) -> Result<()> {
        let edit = VersionEdit { comparator: Some(comparator::name().to_vec()), log_number: Some(0), next_file_number: Some(2), last_sequence: Some(0), ..VersionEdit::default() };
        let manifest = descriptor_file_name(&self.dir, 1);
        self.env.write_file(&manifest, &Writer::new().add_record(&edit.encode()))?;
        if let Err(error) = set_current_file(self.env.as_ref(), &self.dir, 1) {
            let _ = self.env.remove_file(&manifest);
            return Err(error);
        }
        Ok(())
    }

    fn recover_log_file(&mut self, number: u64, last_log: bool, save_manifest: &mut bool, edit: &mut VersionEdit, max_sequence: &mut u64) -> Result<()> {
        let path = log_file_name(&self.dir, number);
        self.info(&format!("Recovering log #{number}"));
        let contents = self.env.read_file(&path)?;
        let mut mem: Option<MemTable> = None;
        let mut compactions = 0;
        for record in read_records(&contents)? {
            if record.len() < 12 {
                return Err(corruption("log record too small"));
            }
            let batch = WriteBatch::from_contents(&record)?;
            let table = mem.get_or_insert_with(MemTable::new);
            insert_into(&batch, table)?;
            let last = batch.sequence() + u64::from(batch.count()).saturating_sub(1);
            if last > *max_sequence {
                *max_sequence = last;
            }
            if table.approximate_memory_usage() > self.options.write_buffer_size {
                compactions += 1;
                *save_manifest = true;
                if let Some(full) = mem.take() {
                    self.write_level0_table(&full, edit, false)?;
                }
            }
        }
        if self.options.reuse_logs && last_log && compactions == 0 {
            if let Ok(size) = self.env.file_size(&path) {
                self.info(&format!("Reusing old log {path} "));
                self.log = Some(LogFile { path, writer: Writer::resuming(size) });
                self.logfile_number = number;
                self.mem = mem.take().unwrap_or_default();
            }
        }
        if let Some(rest) = mem {
            *save_manifest = true;
            self.write_level0_table(&rest, edit, false)?;
        }
        Ok(())
    }

    /// `DBImpl::WriteLevel0Table`: a memtable vira uma tabela; `pick_level` escolhe o nível pelas sobreposições (na
    /// recuperação ela sempre vai para o nível 0).
    fn write_level0_table(&mut self, mem: &MemTable, edit: &mut VersionEdit, pick_level: bool) -> Result<()> {
        let number = self.versions.new_file_number();
        self.info(&format!("Level-0 table #{number}: started"));
        let mut builder = TableBuilder::new(self.options.table);
        let (mut smallest, mut largest) = (None, Vec::new());
        for (key, value) in mem.iter() {
            if smallest.is_none() {
                smallest = Some(key.to_vec());
            }
            largest = key.to_vec();
            builder.add(key, value);
        }
        let Some(smallest) = smallest else {
            self.info(&format!("Level-0 table #{number}: 0 bytes OK"));
            return Ok(());
        };
        let file = builder.finish();
        let file_size = file.len() as u64;
        self.env.write_file(&table_file_name(&self.dir, number), &file)?;
        self.info(&format!("Level-0 table #{number}: {file_size} bytes OK"));
        let level = if pick_level { self.versions.current().pick_level_for_memtable_output(user_key(&smallest), user_key(&largest), self.options.max_file_size) } else { 0 };
        edit.new_files.push((level as u32, FileMeta { number, file_size, smallest, largest }));
        Ok(())
    }

    /// `DBImpl::DeleteObsoleteFiles`: apaga logs, MANIFESTs e tabelas que a versão não usa mais.
    fn delete_obsolete_files(&mut self) {
        let live = self.versions.live_files();
        let Ok(names) = self.env.list_dir(&self.dir) else { return };
        for name in names {
            let Some((number, file_type)) = parse_file_name(&name) else { continue };
            let keep = match file_type {
                FileType::Log => number >= self.versions.log_number() || number == self.versions.prev_log_number(),
                FileType::Descriptor => number >= self.versions.manifest_file_number(),
                FileType::Table | FileType::Temp => live.contains(&number),
                FileType::Current | FileType::DbLock | FileType::InfoLog => true,
            };
            if !keep {
                if file_type == FileType::Table {
                    self.cache().remove(&number);
                }
                self.info(&format!("Delete type={} #{}", file_type as i32, number));
                let _ = self.env.remove_file(&format!("{}/{}", self.dir, name));
            }
        }
    }

    pub fn put(&mut self, key: &[u8], value: &[u8]) -> Result<()> {
        let mut batch = WriteBatch::new();
        batch.put(key, value);
        self.write(batch)
    }

    pub fn delete(&mut self, key: &[u8]) -> Result<()> {
        let mut batch = WriteBatch::new();
        batch.delete(key);
        self.write(batch)
    }

    /// `DB::Write`: o lote vai para o log, depois para a memtable; antes, `MakeRoomForWrite`.
    pub fn write(&mut self, mut batch: WriteBatch) -> Result<()> {
        let _scope = comparator::scoped(&self.options.comparator);
        self.make_room_for_write()?;
        let mut last_sequence = self.versions.last_sequence();
        batch.set_sequence(last_sequence + 1);
        last_sequence += u64::from(batch.count());
        if let Some(log) = &mut self.log {
            let bytes = log.writer.add_record(batch.contents());
            self.env.append_file(&log.path, &bytes)?;
        }
        insert_into(&batch, &mut self.mem)?;
        self.versions.set_last_sequence(last_sequence);
        Ok(())
    }

    /// `DBImpl::MakeRoomForWrite`: com a memtable cheia, troca o log e leva a memtable para uma tabela.
    fn make_room_for_write(&mut self) -> Result<()> {
        if self.mem.approximate_memory_usage() <= self.options.write_buffer_size {
            return Ok(());
        }
        let number = self.versions.new_file_number();
        let path = log_file_name(&self.dir, number);
        self.env.write_file(&path, b"")?;
        self.logfile_number = number;
        self.log = Some(LogFile { path, writer: Writer::new() });
        let imm = std::mem::take(&mut self.mem);
        self.in_background(|db| {
            let mut edit = VersionEdit::default();
            db.write_level0_table(&imm, &mut edit, true)?;
            edit.prev_log_number = Some(0);
            edit.log_number = Some(db.logfile_number);
            db.versions.log_and_apply(db.env.as_ref(), &mut edit)?;
            db.delete_obsolete_files();
            db.compact_if_needed()
        })
    }

    /// `DB::CompactRange`: leva a memtable para uma tabela (trocando o log, mesmo vazia) e compacta a faixa
    /// `[begin, end]` (chaves do usuário) em cada nível abaixo do último que tinha tabelas nela, como o leveldb faz.
    pub fn compact_range(&mut self, begin: &[u8], end: &[u8]) -> Result<()> {
        let _scope = comparator::scoped(&self.options.comparator);
        let max_level_with_files = (1..NUM_LEVELS).filter(|level| self.versions.current().overlap_in_level(*level, begin, end)).max().unwrap_or(1);
        self.flush_memtable()?;
        for level in 0..max_level_with_files {
            self.in_background(|db| db.manual_compaction(level, begin, end))?;
        }
        Ok(())
    }

    /// `TEST_CompactMemTable`: um log novo e a memtable antiga numa tabela.
    fn flush_memtable(&mut self) -> Result<()> {
        let number = self.versions.new_file_number();
        let path = log_file_name(&self.dir, number);
        self.env.write_file(&path, b"")?;
        self.logfile_number = number;
        self.log = Some(LogFile { path, writer: Writer::new() });
        let imm = std::mem::take(&mut self.mem);
        self.in_background(|db| {
            let mut edit = VersionEdit::default();
            db.write_level0_table(&imm, &mut edit, true)?;
            edit.prev_log_number = Some(0);
            edit.log_number = Some(db.logfile_number);
            db.versions.log_and_apply(db.env.as_ref(), &mut edit)?;
            db.delete_obsolete_files();
            db.compact_if_needed()
        })
    }

    /// `TEST_CompactRange` de um nível: pedaço a pedaço até a faixa acabar, cada um com a linha do `LOG`.
    fn manual_compaction(&mut self, level: usize, begin: &[u8], end: &[u8]) -> Result<()> {
        let mut from = internal_key(begin, MAX_SEQUENCE_NUMBER, VALUE_TYPE_FOR_SEEK);
        let until = internal_key(end, 0, ValueType::Deletion);
        loop {
            let mut messages = Vec::new();
            let picked = compaction::manual(self.versions.current(), level, user_key(&from), end, self.options.max_file_size, &mut |message| messages.push(message.to_owned()));
            let stop = picked.as_ref().map_or_else(|| "(end)".to_owned(), |compaction| debug_internal_key(&compaction.inputs[0][compaction.inputs[0].len() - 1].largest));
            // O `Expanding` sai dentro do `VersionSet::CompactRange`, antes da linha da compactação manual.
            for message in messages {
                self.info(&message);
            }
            self.info(&format!("Manual compaction at level-{level} from {} .. {}; will stop at {stop}", debug_internal_key(&from), debug_internal_key(&until)));
            let Some(mut compaction) = picked else { return Ok(()) };
            let largest = compaction.inputs[0][compaction.inputs[0].len() - 1].largest.clone();
            self.do_compaction_work(&mut compaction)?;
            self.delete_obsolete_files();
            from = largest;
        }
    }

    /// `DBImpl::MaybeScheduleCompaction` e `BackgroundCompaction`: enquanto algum nível passa de nota 1, compacta.
    fn compact_if_needed(&mut self) -> Result<()> {
        loop {
            let mut messages = Vec::new();
            let picked = compaction::pick(self.versions.current(), self.versions.compact_pointers(), self.options.max_file_size, &mut |message| messages.push(message.to_owned()));
            for message in messages {
                self.info(&message);
            }
            let Some(mut compaction) = picked else { return Ok(()) };
            if compaction.is_trivial_move() {
                self.move_to_next_level(&mut compaction)?;
            } else {
                self.do_compaction_work(&mut compaction)?;
                self.delete_obsolete_files();
            }
        }
    }

    /// A compactação trivial: a tabela muda de nível sem ser reescrita.
    fn move_to_next_level(&mut self, compaction: &mut Compaction) -> Result<()> {
        let file = compaction.inputs[0][0].clone();
        let level = compaction.level;
        compaction.edit.deleted_files.push((level as u32, file.number));
        compaction.edit.new_files.push(((level + 1) as u32, file.clone()));
        self.versions.log_and_apply(self.env.as_ref(), &mut compaction.edit)?;
        let summary = self.versions.current().level_summary();
        self.info(&format!("Moved #{} to level-{} {} bytes OK: {summary}", file.number, level + 1, file.file_size));
        Ok(())
    }

    /// `DBImpl::DoCompactionWork`: junta as tabelas de entrada em ordem de chave interna, descarta as versões
    /// encobertas (e as remoções que nada mais abaixo pode contradizer) e grava as tabelas de saída no nível seguinte.
    fn do_compaction_work(&mut self, compaction: &mut Compaction) -> Result<()> {
        let level = compaction.level;
        self.info(&format!("Compacting {}@{level} + {}@{} files", compaction.inputs[0].len(), compaction.inputs[1].len(), level + 1));
        // Sem snapshots, a menor sequência visível é a última.
        let smallest_snapshot = self.versions.last_sequence();
        let mut input = Vec::new();
        for file in compaction.inputs.iter().flatten() {
            input.extend(self.table(file)?.entries()?);
        }
        input.sort_by(|a, b| compare_internal(&a.0, &b.0));
        let mut outputs: Vec<FileMeta> = Vec::new();
        let mut open: Option<(FileMeta, TableBuilder)> = None;
        let mut current_user_key: Option<Vec<u8>> = None;
        let mut last_sequence_for_key = MAX_SEQUENCE_NUMBER;
        for (key, value) in input {
            if compaction.should_stop_before(&key) {
                if let Some(output) = open.take() {
                    outputs.push(self.finish_compaction_output(output, level)?);
                }
            }
            let mut drop = false;
            match parse_internal_key(&key) {
                None => {
                    current_user_key = None;
                    last_sequence_for_key = MAX_SEQUENCE_NUMBER;
                }
                Some((user, sequence, value_type)) => {
                    if current_user_key.as_deref().is_none_or(|current| comparator::compare(current, user) != Ordering::Equal) {
                        current_user_key = Some(user.to_vec());
                        last_sequence_for_key = MAX_SEQUENCE_NUMBER;
                    }
                    if last_sequence_for_key <= smallest_snapshot {
                        // Encoberta por uma versão mais nova da mesma chave.
                        drop = true;
                    } else if value_type == ValueType::Deletion && sequence <= smallest_snapshot && compaction.is_base_level_for_key(user) {
                        drop = true;
                    }
                    last_sequence_for_key = sequence;
                }
            }
            if drop {
                continue;
            }
            let (meta, builder) = open.get_or_insert_with(|| {
                let number = self.versions.new_file_number();
                (FileMeta { number, file_size: 0, smallest: key.clone(), largest: Vec::new() }, TableBuilder::new(self.options.table))
            });
            meta.largest = key.clone();
            builder.add(&key, &value);
            if builder.file_size() >= compaction.max_output_file_size() as u64 {
                if let Some(output) = open.take() {
                    outputs.push(self.finish_compaction_output(output, level)?);
                }
            }
        }
        if let Some(output) = open.take() {
            outputs.push(self.finish_compaction_output(output, level)?);
        }
        let total: u64 = outputs.iter().map(|file| file.file_size).sum();
        self.info(&format!("Compacted {}@{level} + {}@{} files => {total} bytes", compaction.inputs[0].len(), compaction.inputs[1].len(), level + 1));
        compaction.add_input_deletions();
        for file in outputs {
            compaction.edit.new_files.push(((level + 1) as u32, file));
        }
        self.versions.log_and_apply(self.env.as_ref(), &mut compaction.edit)?;
        let summary = self.versions.current().level_summary();
        self.info(&format!("compacted to: {summary}"));
        Ok(())
    }

    /// `FinishCompactionOutputFile`: grava a tabela de saída e anota no `LOG`.
    fn finish_compaction_output(&mut self, (mut meta, builder): (FileMeta, TableBuilder), level: usize) -> Result<FileMeta> {
        let entries = builder.num_entries();
        let file = builder.finish();
        meta.file_size = file.len() as u64;
        self.env.write_file(&table_file_name(&self.dir, meta.number), &file)?;
        self.info(&format!("Generated table #{}@{level}: {entries} keys, {} bytes", meta.number, meta.file_size));
        Ok(meta)
    }

    /// As tabelas já abertas, por número de arquivo.
    fn cache(&self) -> MutexGuard<'_, HashMap<u64, Arc<Table>>> {
        self.tables.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn table(&self, file: &FileMeta) -> Result<Arc<Table>> {
        if let Some(table) = self.cache().get(&file.number) {
            return Ok(table.clone());
        }
        let table = Arc::new(Table::open(self.env.read_file(&table_file_name(&self.dir, file.number))?)?);
        self.cache().insert(file.number, table.clone());
        Ok(table)
    }

    /// `DB::Get`: a memtable e depois as tabelas, do nível 0 (a mais nova primeiro) ao último.
    pub fn get(&self, key: &[u8]) -> Result<Option<Vec<u8>>> {
        let _scope = comparator::scoped(&self.options.comparator);
        let sequence = self.versions.last_sequence();
        match self.mem.get(key, sequence) {
            Lookup::Found(value) => return Ok(Some(value)),
            Lookup::Deleted => return Ok(None),
            Lookup::Missing => {}
        }
        let version = self.versions.current();
        let target = internal_key(key, sequence, VALUE_TYPE_FOR_SEEK);
        for level in 0..NUM_LEVELS {
            let mut candidates: Vec<&FileMeta> = version.files[level].iter().filter(|file| comparator::compare(user_key(&file.smallest), key) != Ordering::Greater && comparator::compare(key, user_key(&file.largest)) != Ordering::Greater).collect();
            if level == 0 {
                candidates.sort_by(|a, b| b.number.cmp(&a.number));
            }
            for file in candidates {
                if let Some((found, value)) = self.table(file)?.seek(&target)? {
                    if let Some((found_key, _, value_type)) = parse_internal_key(&found) {
                        if found_key == key {
                            return Ok(if value_type == ValueType::Value { Some(value) } else { None });
                        }
                    }
                }
            }
        }
        Ok(None)
    }

    /// Todas as chaves vivas com os valores, em ordem (o que um iterador do leveldb percorre).
    pub fn entries(&self) -> Result<Vec<(Vec<u8>, Vec<u8>)>> {
        let _scope = comparator::scoped(&self.options.comparator);
        let mut all: Vec<(Vec<u8>, Vec<u8>)> = self.mem.iter().map(|(key, value)| (key.to_vec(), value.to_vec())).collect();
        for files in &self.versions.current().files {
            for file in files {
                all.extend(self.table(file)?.entries()?);
            }
        }
        all.sort_by(|a, b| compare_internal(&a.0, &b.0));
        let mut live: Vec<(Vec<u8>, Vec<u8>)> = Vec::new();
        let mut previous: Option<Vec<u8>> = None;
        for (key, value) in all {
            let Some((user, _, value_type)) = parse_internal_key(&key) else { continue };
            if previous.as_deref().is_some_and(|previous| comparator::compare(previous, user) == Ordering::Equal) {
                continue;
            }
            previous = Some(user.to_vec());
            if value_type == ValueType::Value {
                live.push((user.to_vec(), value));
            }
        }
        Ok(live)
    }

    /// O diretório do banco.
    pub fn dir(&self) -> &str {
        &self.dir
    }
}

impl Drop for Db {
    fn drop(&mut self) {
        self.env.unlock_file(&lock_file_name(&self.dir));
    }
}

/// `WriteBatchInternal::InsertInto`.
fn insert_into(batch: &WriteBatch, mem: &mut MemTable) -> Result<()> {
    let mut sequence = batch.sequence();
    for op in batch.ops()? {
        match op {
            Op::Put { key, value } => mem.add(sequence, ValueType::Value, &key, &value),
            Op::Delete { key } => mem.add(sequence, ValueType::Deletion, &key, b""),
        }
        sequence += 1;
    }
    Ok(())
}
