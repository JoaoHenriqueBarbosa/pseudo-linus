//! `gix` alto nível: onde ele exige diretório real.
//!
//! Não existe trait de FS no `gix`: `gix::open`/`gix::init` recebem um `Path` e o repositório guarda
//! `gix_odb::Handle` (loose + packs lidos do disco, mmap), `gix_ref::file::Store` (refs como arquivos)
//! e `gix_index::File::at` (índice lido do disco). Aqui a gente prova isso medindo: o mesmo repositório
//! do `MemTree` não abre por caminho nenhum sem materializar, e abre quando materializado num
//! diretório de verdade.

use serde_json::{Value as Json, json};

use harness::MemTree;

/// Tenta abrir um repositório que só existe no `MemTree` e depois o mesmo materializado em disco.
pub fn probe(tree: &MemTree) -> Json {
    // 1) Sem disco: o único jeito de apontar o gix pro repositório é um caminho; o `MemTree` não tem.
    let virtual_path = "/pseudo-linus-memtree/work/case";
    let open_virtual = match gix::open(virtual_path) {
        Ok(_) => "abriu (inesperado)".to_string(),
        Err(e) => format!("{e}"),
    };
    let init_virtual = match gix::init(virtual_path) {
        Ok(_) => "criou no disco do host (é exatamente o problema)".to_string(),
        Err(e) => format!("{e}"),
    };
    // 2) Materializado: funciona, lendo do disco do host.
    let dir = harness::paths::scratch_dir("f12-gix-high");
    let case = dir.join("repo");
    let _ = std::fs::remove_dir_all(&case);
    let mtime = std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(harness::FIXTURE_MTIME);
    let materialized = tree.materialize(&case, mtime);
    let opened = materialized.map_err(|e| e.to_string()).and_then(|_| {
        let repo = gix::open(&case).map_err(|e| e.to_string())?;
        let head = repo.head_id().map_err(|e| e.to_string())?;
        let commit = head.object().map_err(|e| e.to_string())?.into_commit();
        let msg = commit.message_raw().map_err(|e| e.to_string())?.to_string();
        Ok(json!({"head": head.to_string(), "message": msg.trim()}))
    });
    let _ = std::fs::remove_dir_all(&case);
    json!({
        "open_memtree_without_disk": open_virtual,
        "init_memtree_without_disk": init_virtual,
        "open_after_materializing_to_host_dir": opened.unwrap_or_else(|e| json!({"error": e})),
        "entry_points": ["gix::open(path)", "gix::init(path)", "gix::discover(path)", "gix::ThreadSafeRepository::open_opts(path, opts)"],
        "storage_types": ["gix_odb::Store::at(objects_dir)", "gix_ref::file::Store::at(git_dir)", "gix_index::File::at(path)"],
    })
}
