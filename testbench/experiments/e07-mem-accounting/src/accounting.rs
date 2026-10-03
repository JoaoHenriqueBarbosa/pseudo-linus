//! O contrato que cada candidato implementa.
//!
//! Semântica comum: "bytes vivos de um processo" são os bytes pedidos (`Layout::size`) que o processo
//! alocou desde que começou e que ainda não foram liberados, segundo a contabilidade do candidato. Cada
//! adaptador devolve esse número relativo ao início do processo, pra que os cenários comparem
//! candidatos sem saber como cada um guarda a conta.

use serde::{Deserialize, Serialize};

/// Identificador do pseudo-processo para o candidato (id de grupo, índice de thread, etc.).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Pid(pub u64);

/// O que o candidato oferece, declarado pelo adaptador. Os cenários medem se a oferta funciona.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Capabilities {
    /// Atribui alocações a um grupo (processo), e não só a um contador global.
    pub per_group: bool,
    /// O próprio processo lê os bytes vivos dele, no meio da execução (serve pro checkpoint).
    pub self_read: bool,
    /// Outra thread (o kernel, um vigia) lê os bytes vivos de um processo.
    pub remote_read: bool,
    /// Existe um escopo pra tirar da conta do processo as alocações feitas pelo kernel em nome dele.
    pub kernel_scope: bool,
    /// O próprio allocator marca que o processo passou do limite (o checkpoint só lê uma flag).
    pub limit_flag: bool,
    /// O allocator recusa a alocação acima do limite (limite duro dentro do allocator).
    pub hard_limit: bool,
    /// Só existe contador global (do processo host inteiro).
    pub global_only: bool,
}

/// Um candidato a allocator contabilizado, visto pela bancada.
///
/// Os métodos de leitura não podem alocar na thread que chama (senão a leitura polui a medida);
/// quem precisa alocar pra ler (alloc-track) delega a uma thread auxiliar.
pub trait Accounting: Sync {
    fn name(&self) -> &'static str;

    fn caps(&self) -> Capabilities;

    /// Roda `body` como um pseudo-processo na thread atual: abre o grupo, mede, fecha.
    ///
    /// `limit` é o teto em bytes vivos do processo, contado desde o início dele. Candidatos com limite
    /// no allocator (flag ou recusa) o aplicam; os outros ignoram e o cenário compara por conta própria.
    fn run_process<R>(&self, limit: Option<i64>, body: impl FnOnce(Pid) -> R) -> R;

    /// Bytes vivos do processo corrente, lidos de dentro dele.
    fn self_live(&self) -> Option<i64> {
        None
    }

    /// Bytes vivos de `pid`, lidos de outra thread.
    fn remote_live(&self, _pid: Pid) -> Option<i64> {
        None
    }

    /// Bytes vivos do host inteiro, quando só existe contador global.
    fn global_live(&self) -> Option<i64> {
        None
    }

    /// Bytes vivos que o último `run_process` desta thread deixou atribuídos ao processo, lidos no fim
    /// dele. Serve pros candidatos que só entregam o número quando a medição termina.
    fn last_process_net(&self) -> Option<i64> {
        None
    }

    /// Roda `f` com as alocações fora da conta do processo corrente (código do kernel).
    fn kernel_scope<R>(&self, f: impl FnOnce() -> R) -> R {
        f()
    }

    /// `Some(true)` se o allocator já marcou que `pid` passou do limite.
    fn over_limit(&self, _pid: Pid) -> Option<bool> {
        None
    }
}

/// Allocator sem contabilidade nenhuma (System, mimalloc puro): só roda as cargas.
#[derive(Debug)]
pub struct NoAccounting {
    pub name: &'static str,
}

impl Accounting for NoAccounting {
    fn name(&self) -> &'static str {
        self.name
    }

    fn caps(&self) -> Capabilities {
        Capabilities::default()
    }

    fn run_process<R>(&self, _limit: Option<i64>, body: impl FnOnce(Pid) -> R) -> R {
        body(Pid(0))
    }
}

/// Lê os bytes vivos do processo corrente pelo melhor caminho que o candidato oferece.
pub fn best_self_read<A: Accounting>(acct: &A, pid: Pid) -> Option<i64> {
    acct.self_live().or_else(|| acct.remote_live(pid))
}
