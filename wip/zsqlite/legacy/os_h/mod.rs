// Mesclado das partes traduzidas de os_h (as partes quebram funções grandes no meio).
#![allow(unused_imports, ambiguous_glob_reexports)]
use crate::prelude::*;

// ---- part_000.rs ----

// Tentativa de detectar automaticamente o sistema operacional e configurar as
// macros do pré-processador necessárias.

// Se a macro SET_FULLSYNC não está definida acima, faça-a uma operação vazia.
#[inline]
pub fn set_fullsync(_x: i32, _y: i32) {
    // sem operação
}

// Comprimento máximo de caminho. Nota: FILENAME_MAX definido por stdio.h
pub const SQLITE_MAX_PATHLEN: usize = 4096;

// Número máximo de links simbólicos resolvidos ao tentar expandir um nome de
// arquivo em xFullPathname() do VFS.
pub const SQLITE_MAX_SYMLINK: u32 = 200;

// Tamanho padrão de um setor do disco.
pub const SQLITE_DEFAULT_SECTOR_SIZE: u32 = 4096;

// Arquivos temporários são nomeados começando com este prefixo seguido de 16
// caracteres alfanuméricos aleatórios e nenhuma extensão de arquivo. Eles são
// armazenados no diretório de arquivo temporário padrão do SO e são excluídos
// antes da saída. Se sqlite está sendo incorporado em outro programa, você pode
// desejar alterar o prefixo para refletir o nome do seu programa, de forma que
// se seu programa sair prematuramente, arquivos temporários antigos possam ser
// facilmente identificados.
pub const SQLITE_TEMP_FILE_PREFIX: &[u8] = b"etilqs_";

// Os seguintes valores podem ser passados como o segundo argumento para
// sqlite3OsLock(). Os vários bloqueios exibem a seguinte semântica:
//
// SHARED: qualquer número de processos pode manter um bloqueio SHARED simultaneamente.
// RESERVED: um único processo pode manter um bloqueio RESERVED em um arquivo em
//           qualquer tempo. Outros processos podem manter e obter novos bloqueios SHARED.
// PENDING: um único processo pode manter um bloqueio PENDING em um arquivo em
//          um único momento. Bloqueios SHARED existentes podem persistir, mas nenhum
//          novo bloqueio SHARED pode ser obtido por outros processos.
// EXCLUSIVE: um bloqueio EXCLUSIVE preclude todos os outros bloqueios.
//
// PENDING_LOCK não pode ser passado diretamente para sqlite3OsLock(). Em vez disso,
// um processo que solicita um bloqueio EXCLUSIVE pode realmente obter um bloqueio
// PENDING. Isso pode ser atualizado para um bloqueio EXCLUSIVE por uma chamada
// subsequente para sqlite3OsLock().

pub const NO_LOCK: i32 = 0;
pub const SHARED_LOCK: i32 = 1;
pub const RESERVED_LOCK: i32 = 2;
pub const PENDING_LOCK: i32 = 3;
pub const EXCLUSIVE_LOCK: i32 = 4;

// Notas de Bloqueio de Arquivo: (principalmente sobre windows mas também algumas
// informações para Unix)
//
// Não podemos usar LockFileEx() ou UnlockFileEx() no Win95/98/ME porque essas
// funções não estão disponíveis. Portanto, usamos apenas LockFile() e UnlockFile().
//
// LockFile() impede não apenas a escrita mas também a leitura por outros processos.
// Um SHARED_LOCK é obtido bloqueando um único byte escolhido aleatoriamente de um
// intervalo específico de bytes. O byte de bloqueio é obtido aleatoriamente para
// que dois leitores separados possam provavelmente acessar o arquivo ao mesmo tempo,
// a menos que sejam azarados e escolham o mesmo byte de bloqueio. Um EXCLUSIVE_LOCK
// é obtido bloqueando todos os bytes no intervalo. Pode haver apenas um escritor.
// Um RESERVED_LOCK é obtido bloqueando um único byte do arquivo designado como
// byte de bloqueio reservado. Um PENDING_LOCK é obtido bloqueando um byte designado
// diferente do byte RESERVED_LOCK.
//
// Em sistemas WinNT/2K/XP, LockFileEx() e UnlockFileEx() estão disponíveis, o que
// significa que podemos usar bloqueios leitor/escritor. Quando bloqueios leitor/escritor
// são usados, o bloqueio é colocado no mesmo intervalo de bytes usado para bloqueio
// probabilístico no Win95/98/ME. Portanto, o esquema de bloqueio suportará dois ou
// mais leitores Win95 ou dois ou mais leitores WinNT. Mas um único leitor Win95
// bloqueia todos os leitores WinNT e um único leitor WinNT bloqueia todos os outros
// leitores Win95.
//
// Os #defines a seguir especificam o intervalo de bytes usado para bloqueio.
// SHARED_SIZE é o número de bytes disponíveis no pool do qual um byte aleatório é
// selecionado para um bloqueio compartilhado. O pool de bytes para bloqueios
// compartilhados começa em SHARED_FIRST.
//
// A mesma estratégia de bloqueio e intervalos de bytes são usados para Unix. Isso
// deixa aberta a possibilidade de ter clientes no win95, winNT e unix todos falando
// com o mesmo arquivo compartilhado e todos bloqueando corretamente. Para fazer isso
// seria necessário que samba (ou qualquer ferramenta usada para compartilhamento de
// arquivo) implementasse bloqueios corretamente entre windows e unix. Estou chutando
// que isso provavelmente não vai acontecer, mas usando o mesmo intervalo de bloqueio
// estamos pelo menos abertos à possibilidade.
//
// O bloqueio no windows é obrigatório. Por essa razão, não podemos armazenar dados
// reais nos bytes usados para bloqueio. O pager nunca aloca as páginas envolvidas
// em bloqueio portanto. SHARED_SIZE é selecionado para que todos os bloqueios caibam
// em uma única página mesmo no tamanho mínimo de página. PENDING_BYTE define o início
// dos bloqueios. Por padrão, PENDING_BYTE é definido alto para que não precisemos
// alocar uma página não utilizada, exceto para bancos de dados muito grandes. Mas
// deve-se testar a lógica de pulo de página definindo PENDING_BYTE baixo e executando
// a suíte de regressão inteira.
//
// Alterar o valor de PENDING_BYTE resulta em um formato de arquivo levemente
// incompatível. Dependendo de como é alterado, você pode não notar a incompatibilidade
// imediatamente, mesmo executando um teste de regressão completo. O local padrão de
// PENDING_BYTE é o primeiro byte após a fronteira de 1GB.

// No Debian, SQLITE_OMIT_WSD não está definido: PENDING_BYTE é a variável global
// sqlite3PendingByte (padrão 0x40000000), alterável por sqlite3_test_control.
// O global vive em outro módulo (global.c) e é lido por esta função.
#[inline]
pub fn pending_byte() -> i64 {
    crate::prelude::pending_byte_global()
}

#[inline]
pub fn reserved_byte() -> i64 {
    pending_byte() + 1
}

#[inline]
pub fn shared_first() -> i64 {
    pending_byte() + 2
}

pub const SHARED_SIZE: i32 = 510;

pub const SQLITE_FCNTL_DB_UNCHANGED: u32 = 0xca093fa0;

