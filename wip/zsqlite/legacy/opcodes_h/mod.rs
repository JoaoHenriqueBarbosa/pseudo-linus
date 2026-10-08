// Mesclado das partes traduzidas de opcodes_h (as partes quebram funções grandes no meio).
#![allow(unused_imports, ambiguous_glob_reexports)]
use crate::prelude::*;

// ---- part_000.rs ----

/// Operações da máquina virtual de bytecode do SQLite, geradas automaticamente.
/// Veja tool/mkopcodeh.tcl para detalhes.

pub const OP_SAVEPOINT: u8 = 0;
pub const OP_AUTOCOMMIT: u8 = 1;
pub const OP_TRANSACTION: u8 = 2;
pub const OP_CHECKPOINT: u8 = 3;
pub const OP_JOURNALMODE: u8 = 4;
pub const OP_VACUUM: u8 = 5;
pub const OP_VFILTER: u8 = 6;
pub const OP_VUPDATE: u8 = 7;
pub const OP_INIT: u8 = 8;
pub const OP_GOTO: u8 = 9;
pub const OP_GOSUB: u8 = 10;
pub const OP_INITCOROUTINE: u8 = 11;
pub const OP_YIELD: u8 = 12;
pub const OP_MUSTBEINT: u8 = 13;
pub const OP_JUMP: u8 = 14;
pub const OP_ONCE: u8 = 15;
pub const OP_IF: u8 = 16;
pub const OP_IFNOT: u8 = 17;
pub const OP_ISTYPE: u8 = 18;
pub const OP_NOT: u8 = 19;
pub const OP_IFNULLROW: u8 = 20;
pub const OP_SEEKLT: u8 = 21;
pub const OP_SEEKLE: u8 = 22;
pub const OP_SEEKGE: u8 = 23;
pub const OP_SEEKGT: u8 = 24;
pub const OP_IFNOTOPEN: u8 = 25;
pub const OP_IFNOHOPE: u8 = 26;
pub const OP_NOCONFLICT: u8 = 27;
pub const OP_NOTFOUND: u8 = 28;
pub const OP_FOUND: u8 = 29;
pub const OP_SEEKROWID: u8 = 30;
pub const OP_NOTEXISTS: u8 = 31;
pub const OP_LAST: u8 = 32;
pub const OP_IFSIZEBETWEEN: u8 = 33;
pub const OP_SORTERSORT: u8 = 34;
pub const OP_SORT: u8 = 35;
pub const OP_REWIND: u8 = 36;
pub const OP_SORTERNEXT: u8 = 37;
pub const OP_PREV: u8 = 38;
pub const OP_NEXT: u8 = 39;
pub const OP_IDXLE: u8 = 40;
pub const OP_IDXGT: u8 = 41;
pub const OP_IDXLT: u8 = 42;
pub const OP_OR: u8 = 43;
pub const OP_AND: u8 = 44;
pub const OP_IDXGE: u8 = 45;
pub const OP_ROWSETREAD: u8 = 46;
pub const OP_ROWSETTEST: u8 = 47;
pub const OP_PROGRAM: u8 = 48;
pub const OP_FKIFZERO: u8 = 49;
pub const OP_ISNULL: u8 = 50;
pub const OP_NOTNULL: u8 = 51;
pub const OP_NE: u8 = 52;
pub const OP_EQ: u8 = 53;
pub const OP_GT: u8 = 54;
pub const OP_LE: u8 = 55;
pub const OP_LT: u8 = 56;
pub const OP_GE: u8 = 57;
pub const OP_ELSEEQ: u8 = 58;
pub const OP_IFPOS: u8 = 59;
pub const OP_IFNOTZERO: u8 = 60;
pub const OP_DECRJUMPZERO: u8 = 61;
pub const OP_INCRVACUUM: u8 = 62;
pub const OP_VNEXT: u8 = 63;
pub const OP_FILTER: u8 = 64;
pub const OP_PUREFUNC: u8 = 65;
pub const OP_FUNCTION: u8 = 66;
pub const OP_RETURN: u8 = 67;
pub const OP_ENDCOROUTINE: u8 = 68;
pub const OP_HALTIFNULL: u8 = 69;
pub const OP_HALT: u8 = 70;
pub const OP_INTEGER: u8 = 71;
pub const OP_INT64: u8 = 72;
pub const OP_STRING: u8 = 73;
pub const OP_BEGINSUBRTN: u8 = 74;
pub const OP_NULL: u8 = 75;
pub const OP_SOFTNULL: u8 = 76;
pub const OP_BLOB: u8 = 77;
pub const OP_VARIABLE: u8 = 78;
pub const OP_MOVE: u8 = 79;
pub const OP_COPY: u8 = 80;
pub const OP_SCOPY: u8 = 81;
pub const OP_INTCOPY: u8 = 82;
pub const OP_FKCHECK: u8 = 83;
pub const OP_RESULTROW: u8 = 84;
pub const OP_COLLSEQ: u8 = 85;
pub const OP_ADDIMM: u8 = 86;
pub const OP_REALAFFINITY: u8 = 87;
pub const OP_CAST: u8 = 88;
pub const OP_PERMUTATION: u8 = 89;
pub const OP_COMPARE: u8 = 90;
pub const OP_ISTRUE: u8 = 91;
pub const OP_ZEROORNULL: u8 = 92;
pub const OP_OFFSET: u8 = 93;
pub const OP_COLUMN: u8 = 94;
pub const OP_TYPECHECK: u8 = 95;
pub const OP_AFFINITY: u8 = 96;
pub const OP_MAKERECORD: u8 = 97;
pub const OP_COUNT: u8 = 98;
pub const OP_READCOOKIE: u8 = 99;
pub const OP_SETCOOKIE: u8 = 100;
pub const OP_REOPENIDX: u8 = 101;
pub const OP_BITAND: u8 = 102;
pub const OP_BITOR: u8 = 103;
pub const OP_SHIFTLEFT: u8 = 104;
pub const OP_SHIFTRIGHT: u8 = 105;
pub const OP_ADD: u8 = 106;
pub const OP_SUBTRACT: u8 = 107;
pub const OP_MULTIPLY: u8 = 108;
pub const OP_DIVIDE: u8 = 109;
pub const OP_REMAINDER: u8 = 110;
pub const OP_CONCAT: u8 = 111;
pub const OP_OPENREAD: u8 = 112;
pub const OP_OPENWRITE: u8 = 113;
pub const OP_BITNOT: u8 = 114;
pub const OP_OPENDUP: u8 = 115;
pub const OP_OPENAUTOINDEX: u8 = 116;
pub const OP_STRING8: u8 = 117;
pub const OP_OPENEPHEMERAL: u8 = 118;
pub const OP_SORTEROPEN: u8 = 119;
pub const OP_SEQUENCETEST: u8 = 120;
pub const OP_OPENPSEUDO: u8 = 121;
pub const OP_CLOSE: u8 = 122;
pub const OP_COLUMNSUSED: u8 = 123;
pub const OP_SEEKSCAN: u8 = 124;
pub const OP_SEEKHIT: u8 = 125;
pub const OP_SEQUENCE: u8 = 126;
pub const OP_NEWROWID: u8 = 127;
pub const OP_INSERT: u8 = 128;
pub const OP_ROWCELL: u8 = 129;
pub const OP_DELETE: u8 = 130;
pub const OP_RESETCOUNT: u8 = 131;
pub const OP_SORTERCOMPARE: u8 = 132;
pub const OP_SORTERDATA: u8 = 133;
pub const OP_ROWDATA: u8 = 134;
pub const OP_ROWID: u8 = 135;
pub const OP_NULLROW: u8 = 136;
pub const OP_SEEKEND: u8 = 137;
pub const OP_IDXINSERT: u8 = 138;
pub const OP_SORTERINSERT: u8 = 139;
pub const OP_IDXDELETE: u8 = 140;
pub const OP_DEFERREDSEEK: u8 = 141;
pub const OP_IDXROWID: u8 = 142;
pub const OP_FINISHSEEK: u8 = 143;
pub const OP_DESTROY: u8 = 144;
pub const OP_CLEAR: u8 = 145;
pub const OP_RESETSORTER: u8 = 146;
pub const OP_CREATEBTREE: u8 = 147;
pub const OP_SQLEXEC: u8 = 148;
pub const OP_PARSESCHEMA: u8 = 149;
pub const OP_LOADANALYSIS: u8 = 150;
pub const OP_DROPTABLE: u8 = 151;
pub const OP_DROPINDEX: u8 = 152;
pub const OP_REAL: u8 = 153;
pub const OP_DROPTRIGGER: u8 = 154;
pub const OP_INTEGRITYCK: u8 = 155;
pub const OP_ROWSETADD: u8 = 156;
pub const OP_PARAM: u8 = 157;
pub const OP_FKCOUNTER: u8 = 158;
pub const OP_MEMMAX: u8 = 159;
pub const OP_OFFSETLIMIT: u8 = 160;
pub const OP_AGGINVERSE: u8 = 161;
pub const OP_AGGSTEP: u8 = 162;
pub const OP_AGGSTEP1: u8 = 163;
pub const OP_AGGVALUE: u8 = 164;
pub const OP_AGGFINAL: u8 = 165;
pub const OP_EXPIRE: u8 = 166;
pub const OP_CURSORLOCK: u8 = 167;
pub const OP_CURSORUNLOCK: u8 = 168;
pub const OP_TABLELOCK: u8 = 169;
pub const OP_VBEGIN: u8 = 170;
pub const OP_VCREATE: u8 = 171;
pub const OP_VDESTROY: u8 = 172;
pub const OP_VOPEN: u8 = 173;
pub const OP_VCHECK: u8 = 174;
pub const OP_VINITIN: u8 = 175;
pub const OP_VCOLUMN: u8 = 176;
pub const OP_VRENAME: u8 = 177;
pub const OP_PAGECOUNT: u8 = 178;
pub const OP_MAXPGCNT: u8 = 179;
pub const OP_CLRSUBTYPE: u8 = 180;
pub const OP_GETSUBTYPE: u8 = 181;
pub const OP_SETSUBTYPE: u8 = 182;
pub const OP_FILTERADD: u8 = 183;
pub const OP_TRACE: u8 = 184;
pub const OP_CURSORHINT: u8 = 185;
pub const OP_RELEASEREG: u8 = 186;
pub const OP_NOOP: u8 = 187;
pub const OP_EXPLAIN: u8 = 188;
pub const OP_ABORTABLE: u8 = 189;

/// Propriedades codificadas em vetores de bits, derivadas dos comentários
/// no vdbe.c, como "jump", "out2", "in1", etc.
pub const OPFLG_JUMP: u8 = 0x01;        // P2 contém destino do salto
pub const OPFLG_IN1: u8 = 0x02;         // P1 é entrada
pub const OPFLG_IN2: u8 = 0x04;         // P2 é entrada
pub const OPFLG_IN3: u8 = 0x08;         // P3 é entrada
pub const OPFLG_OUT2: u8 = 0x10;        // P2 é saída
pub const OPFLG_OUT3: u8 = 0x20;        // P3 é saída
pub const OPFLG_NCYCLE: u8 = 0x40;      // Ciclos contam contra P1
pub const OPFLG_JUMP0: u8 = 0x80;       // P2 pode ser zero

/// Tabela inicializadora com as propriedades de cada opcode.
/// Tem 190 entradas, uma por opcode (índices 0 a 189).
pub const OPFLG_INITIALIZER: &[u8] = &[
    /*   0 */ 0x00, 0x00, 0x00, 0x00, 0x10, 0x00, 0x41, 0x00,
    /*   8 */ 0x81, 0x01, 0x01, 0x81, 0x83, 0x83, 0x01, 0x01,
    /*  16 */ 0x03, 0x03, 0x01, 0x12, 0x01, 0xc9, 0xc9, 0xc9,
    /*  24 */ 0xc9, 0x01, 0x49, 0x49, 0x49, 0x49, 0xc9, 0x49,
    /*  32 */ 0xc1, 0x01, 0x41, 0x41, 0xc1, 0x01, 0x41, 0x41,
    /*  40 */ 0x41, 0x41, 0x41, 0x26, 0x26, 0x41, 0x23, 0x0b,
    /*  48 */ 0x81, 0x01, 0x03, 0x03, 0x0b, 0x0b, 0x0b, 0x0b,
    /*  56 */ 0x0b, 0x0b, 0x01, 0x03, 0x03, 0x03, 0x01, 0x41,
    /*  64 */ 0x01, 0x00, 0x00, 0x02, 0x02, 0x08, 0x00, 0x10,
    /*  72 */ 0x10, 0x10, 0x00, 0x10, 0x00, 0x10, 0x10, 0x00,
    /*  80 */ 0x00, 0x10, 0x10, 0x00, 0x00, 0x00, 0x02, 0x02,
    /*  88 */ 0x02, 0x00, 0x00, 0x12, 0x1e, 0x20, 0x40, 0x00,
    /*  96 */ 0x00, 0x00, 0x10, 0x10, 0x00, 0x40, 0x26, 0x26,
    /* 104 */ 0x26, 0x26, 0x26, 0x26, 0x26, 0x26, 0x26, 0x26,
    /* 112 */ 0x40, 0x00, 0x12, 0x40, 0x40, 0x10, 0x40, 0x00,
    /* 120 */ 0x00, 0x00, 0x40, 0x00, 0x40, 0x40, 0x10, 0x10,
    /* 128 */ 0x00, 0x00, 0x00, 0x00, 0x00, 0x40, 0x00, 0x50,
    /* 136 */ 0x00, 0x40, 0x04, 0x04, 0x00, 0x40, 0x50, 0x40,
    /* 144 */ 0x10, 0x00, 0x00, 0x10, 0x00, 0x00, 0x00, 0x00,
    /* 152 */ 0x00, 0x10, 0x00, 0x00, 0x06, 0x10, 0x00, 0x04,
    /* 160 */ 0x1a, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    /* 168 */ 0x00, 0x00, 0x00, 0x00, 0x00, 0x40, 0x10, 0x50,
    /* 176 */ 0x40, 0x00, 0x10, 0x10, 0x02, 0x12, 0x12, 0x00,
    /* 184 */ 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
];

/// Valor máximo do opcode de salto.
/// A rotina resolve3P2Values() executa mais rápido sabendo disso.
/// Quanto menor o máximo do opcode de salto, melhor (os opcodes de salto
/// são agrupados no início da lista pelo script mkopcodeh.tcl).
pub const SQLITE_MX_JUMP_OPCODE: u8 = 64;

