// Mesclado das partes traduzidas de sqliteLimit_h (as partes quebram funções grandes no meio).
#![allow(unused_imports, ambiguous_glob_reexports)]
use crate::prelude::*;

// ---- part_000.rs ----

/// Tamanho máximo de um TEXT ou BLOB em bytes. Também limita o tamanho de uma linha
/// em uma tabela ou índice. O limite máximo é a capacidade de um inteiro assinado de 32 bits,
/// 2^31-1 ou 2147483647.
pub const SQLITE_MAX_LENGTH: i32 = 1000000000;

/// Número máximo de colunas em uma tabela, colunas em um índice, colunas em uma view,
/// termos na cláusula SET de um UPDATE, termos no conjunto de resultados de um SELECT,
/// termos nas cláusulas GROUP BY ou ORDER BY de um SELECT e termos na cláusula VALUES de um INSERT.
/// O limite superior máximo é 32676. A maioria dos especialistas de banco de dados dirá que
/// em um banco de dados bem normalizado, você normalmente não deve ter mais de uma dúzia
/// de colunas em nenhuma tabela.
pub const SQLITE_MAX_COLUMN: i32 = 2000;

/// Tamanho máximo de uma única declaração SQL em bytes. Anteriormente, definir este valor como zero
/// desativaria o limite. Isso não é mais verdade. Não é possível desativar este limite.
pub const SQLITE_MAX_SQL_LENGTH: i32 = 1000000000;

/// Profundidade máxima de uma árvore de expressão. Isso é limitado em certa medida por
/// SQLITE_MAX_SQL_LENGTH. Mas às vezes você pode querer impor limites mais severos à complexidade
/// de uma expressão. Um valor de 0 significa que não há limite.
pub const SQLITE_MAX_EXPR_DEPTH: i32 = 1000;

/// Número máximo de termos em uma declaração SELECT composta. O gerador de código para declarações
/// SELECT compostas faz um nível de recursão para cada termo. Um estouro de pilha pode resultar se
/// o número de termos for muito grande. Na prática, a maioria das SQL nunca tem mais de 3 ou 4 termos.
/// Use um valor de 0 para desativar qualquer limite no número de termos.
pub const SQLITE_MAX_COMPOUND_SELECT: i32 = 500;

/// Número máximo de opcodes em um programa VDBE. Não é atualmente forçado.
pub const SQLITE_MAX_VDBE_OP: i32 = 250000000;

/// Número máximo de argumentos para uma função SQL.
pub const SQLITE_MAX_FUNCTION_ARG: i32 = 127;

/// Número máximo sugerido de páginas em memória para usar para a tabela do banco de dados principal
/// e para tabelas temporárias.
/// IMPLEMENTATION-OF: R-30185-15359 O tamanho do cache sugerido padrão é -2000,
/// o que significa que o tamanho do cache é limitado a 2048000 bytes de memória.
/// IMPLEMENTATION-OF: R-48205-43578 O tamanho do cache sugerido padrão pode ser alterado
/// usando as opções de compilação SQLITE_DEFAULT_CACHE_SIZE.
pub const SQLITE_DEFAULT_CACHE_SIZE: i32 = -2000;

/// Número padrão de frames para acumular no arquivo de log antes de fazer um checkpoint
/// do banco de dados no modo WAL.
pub const SQLITE_DEFAULT_WAL_AUTOCHECKPOINT: i32 = 1000;

/// Número máximo de bancos de dados anexados. Deve estar entre 0 e 125. O limite superior de 125
/// é porque os bancos de dados anexados são contados usando um inteiro assinado de 8 bits, que tem
/// um valor máximo de 127, e precisamos permitir 2 contagens extras para os bancos de dados
/// "main" e "temp".
pub const SQLITE_MAX_ATTACHED: i32 = 10;

/// Valor máximo de um wildcard ?nnn que o analisador aceitará. Se o valor exceder 32767,
/// será necessário espaço extra para a estrutura Expr. Caso contrário, acreditamos que o número
/// pode ser tão grande quanto um inteiro assinado de 32 bits pode conter.
/// O Debian 13 compila com SQLITE_MAX_VARIABLE_NUMBER=250000 (o padrão do C é 32766).
pub const SQLITE_MAX_VARIABLE_NUMBER: i32 = 250000;

/// Tamanho máximo da página. O limite superior neste valor é 65536. Este é um limite imposto
/// pelo uso de deslocamentos de 16 bits dentro de cada página.
/// Versões anteriores do SQLite permitiam que o usuário alterasse este valor em tempo de compilação.
/// Isso não é mais permitido, pois cria uma biblioteca que é tecnicamente incompatível com uma
/// biblioteca SQLite compilada com um limite diferente. Se um processo operando em um banco de dados
/// com tamanho de página de 65536 bytes for interrompido, uma instância de SQLite compilada com o
/// limite de tamanho de página padrão não conseguirá fazer rollback da transação interrompida.
/// Isso pode levar à corrupção do banco de dados.
pub const SQLITE_MAX_PAGE_SIZE: i32 = 65536;

/// Tamanho padrão de uma página de banco de dados.
pub const SQLITE_DEFAULT_PAGE_SIZE: i32 = 4096;

/// Ordinariamente, se nenhum valor for explicitamente fornecido, o SQLite cria bancos de dados
/// com tamanho de página SQLITE_DEFAULT_PAGE_SIZE. No entanto, com base em certas características
/// do dispositivo (tamanho do setor e suporte a escrita atômica), o SQLite pode escolher um valor maior.
/// Essa constante é o valor máximo que o SQLite escolherá por conta própria.
pub const SQLITE_MAX_DEFAULT_PAGE_SIZE: i32 = 8192;

/// Número máximo de páginas em um arquivo de banco de dados.
/// Esse é realmente apenas o valor padrão para o pragma max_page_count. Esse valor pode ser
/// diminuído (ou aumentado) em tempo de execução usando esse pragma.
pub const SQLITE_MAX_PAGE_COUNT: u32 = 0xfffffffe;

/// Tamanho máximo (em bytes) do padrão em um operador LIKE ou GLOB.
pub const SQLITE_MAX_LIKE_PATTERN_LENGTH: i32 = 50000;

/// Profundidade máxima de recursão para triggers. Um valor de 1 significa que um programa de
/// trigger não será capaz de ativar outros triggers. Um valor de 0 significa que nenhum programa
/// de trigger pode ser executado.
pub const SQLITE_MAX_TRIGGER_DEPTH: i32 = 1000;

