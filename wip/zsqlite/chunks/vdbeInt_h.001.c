
/*
** The "context" argument for an installable function.  A pointer to an
** instance of this structure is the first argument to the routines used
** implement the SQL functions.
**
** There is a typedef for this structure in sqlite.h.  So all routines,
** even the public interface to SQLite, can use a pointer to this structure.
** But this file is the only place where the internal details of this
** structure are known.
**
** This structure is defined inside of vdbeInt.h because it uses substructures
** (Mem) which are only defined there.
*/
struct sqlite3_context {
  Mem *pOut;              /* The return value is stored here */
  FuncDef *pFunc;         /* Pointer to function information */
  Mem *pMem;              /* Memory cell used to store aggregate context */
  Vdbe *pVdbe;            /* The VM that owns this context */
  int iOp;                /* Instruction number of OP_Function */
  int isError;            /* Error code returned by the function. */
  u8 enc;                 /* Encoding to use for results */
  u8 skipFlag;            /* Skip accumulator loading if true */
  u8 argc;                /* Number of arguments */
  sqlite3_value *argv[1]; /* Argument set */
};

/* A bitfield type for use inside of structures.  Always follow with :N where
** N is the number of bits.
*/
typedef unsigned bft;  /* Bit Field Type */

/* The ScanStatus object holds a single value for the
** sqlite3_stmt_scanstatus() interface.
**
** aAddrRange[]:
**   This array is used by ScanStatus elements associated with EQP
**   notes that make an SQLITE_SCANSTAT_NCYCLE value available. It is
**   an array of up to 3 ranges of VM addresses for which the Vdbe.anCycle[]
**   values should be summed to calculate the NCYCLE value. Each pair of
**   integer addresses is a start and end address (both inclusive) for a range
**   instructions. A start value of 0 indicates an empty range.
*/
typedef struct ScanStatus ScanStatus;
struct ScanStatus {
  int addrExplain;                /* OP_Explain for loop */
  int aAddrRange[6];
  int addrLoop;                   /* Address of "loops" counter */
  int addrVisit;                  /* Address of "rows visited" counter */
  int iSelectID;                  /* The "Select-ID" for this loop */
  LogEst nEst;                    /* Estimated output rows per loop */
  char *zName;                    /* Name of table or index */
};

/* The DblquoteStr object holds the text of a double-quoted
** string for a prepared statement.  A linked list of these objects
** is constructed during statement parsing and is held on Vdbe.pDblStr.
** When computing a normalized SQL statement for an SQL statement, that
** list is consulted for each double-quoted identifier to see if the
** identifier should really be a string literal.
*/
typedef struct DblquoteStr DblquoteStr;
struct DblquoteStr {
  DblquoteStr *pNextStr;   /* Next string literal in the list */
  char z[8];               /* Dequoted value for the string */
};

/*
** An instance of the virtual machine.  This structure contains the complete
** state of the virtual machine.
**
** The "sqlite3_stmt" structure pointer that is returned by sqlite3_prepare()
** is really a pointer to an instance of this structure.
*/
struct Vdbe {
  sqlite3 *db;            /* The database connection that owns this statement */
  Vdbe **ppVPrev,*pVNext; /* Linked list of VDBEs with the same Vdbe.db */
  Parse *pParse;          /* Parsing context used to create this Vdbe */
  ynVar nVar;             /* Number of entries in aVar[] */
  int nMem;               /* Number of memory locations currently allocated */
  int nCursor;            /* Number of slots in apCsr[] */
  u32 cacheCtr;           /* VdbeCursor row cache generation counter */
  int pc;                 /* The program counter */
  int rc;                 /* Value to return */
  i64 nChange;            /* Number of db changes made since last reset */
  int iStatement;         /* Statement number (or 0 if has no opened stmt) */
  i64 iCurrentTime;       /* Value of julianday('now') for this statement */
  i64 nFkConstraint;      /* Number of imm. FK constraints this VM */
  i64 nStmtDefCons;       /* Number of def. constraints when stmt started */
  i64 nStmtDefImmCons;    /* Number of def. imm constraints when stmt started */
  Mem *aMem;              /* The memory locations */
  Mem **apArg;            /* Arguments to currently executing user function */
  VdbeCursor **apCsr;     /* One element of this array for each open cursor */
  Mem *aVar;              /* Values for the OP_Variable opcode. */

  /* When allocating a new Vdbe object, all of the fields below should be
  ** initialized to zero or NULL */

  Op *aOp;                /* Space to hold the virtual machine's program */
  int nOp;                /* Number of instructions in the program */
  int nOpAlloc;           /* Slots allocated for aOp[] */
  Mem *aColName;          /* Column names to return */
  Mem *pResultRow;        /* Current output row */
  char *zErrMsg;          /* Error message written here */
  VList *pVList;          /* Name of variables */
#ifndef SQLITE_OMIT_TRACE
  i64 startTime;          /* Time when query started - used for profiling */
#endif
#ifdef SQLITE_DEBUG
  int rcApp;              /* errcode set by sqlite3_result_error_code() */
  u32 nWrite;             /* Number of write operations that have occurred */
#endif
  u16 nResColumn;         /* Number of columns in one row of the result set */
  u16 nResAlloc;          /* Column slots allocated to aColName[] */
  u8 errorAction;         /* Recovery action to do in case of an error */
  u8 minWriteFileFormat;  /* Minimum file format for writable database files */
  u8 prepFlags;           /* SQLITE_PREPARE_* flags */
  u8 eVdbeState;          /* On of the VDBE_*_STATE values */
  bft expired:2;          /* 1: recompile VM immediately  2: when convenient */
  bft explain:2;          /* 0: normal, 1: EXPLAIN, 2: EXPLAIN QUERY PLAN */
  bft changeCntOn:1;      /* True to update the change-counter */
  bft usesStmtJournal:1;  /* True if uses a statement journal */
  bft readOnly:1;         /* True for statements that do not write */
  bft bIsReader:1;        /* True for statements that read */
  bft haveEqpOps:1;       /* Bytecode supports EXPLAIN QUERY PLAN */
  yDbMask btreeMask;      /* Bitmask of db->aDb[] entries referenced */
  yDbMask lockMask;       /* Subset of btreeMask that requires a lock */
  u32 aCounter[9];        /* Counters used by sqlite3_stmt_status() */
  char *zSql;             /* Text of the SQL statement that generated this */
#ifdef SQLITE_ENABLE_NORMALIZE
  char *zNormSql;         /* Normalization of the associated SQL statement */
  DblquoteStr *pDblStr;   /* List of double-quoted string literals */
#endif
  void *pFree;            /* Free this when deleting the vdbe */
  VdbeFrame *pFrame;      /* Parent frame */
  VdbeFrame *pDelFrame;   /* List of frame objects to free on VM reset */
  int nFrame;             /* Number of frames in pFrame list */
  u32 expmask;            /* Binding to these vars invalidates VM */
  SubProgram *pProgram;   /* Linked list of all sub-programs used by VM */
  AuxData *pAuxData;      /* Linked list of auxdata allocations */
#ifdef SQLITE_ENABLE_STMT_SCANSTATUS
  int nScan;              /* Entries in aScan[] */
  ScanStatus *aScan;      /* Scan definitions for sqlite3_stmt_scanstatus() */
#endif
};

/*
** The following are allowed values for Vdbe.eVdbeState
*/
#define VDBE_INIT_STATE     0   /* Prepared statement under construction */
#define VDBE_READY_STATE    1   /* Ready to run but not yet started */
#define VDBE_RUN_STATE      2   /* Run in progress */
#define VDBE_HALT_STATE     3   /* Finished.  Need reset() or finalize() */

/*
** Structure used to store the context required by the
** sqlite3_preupdate_*() API functions.
*/
struct PreUpdate {
  Vdbe *v;
  VdbeCursor *pCsr;               /* Cursor to read old values from */
  int op;                         /* One of SQLITE_INSERT, UPDATE, DELETE */
  u8 *aRecord;                    /* old.* database record */
  KeyInfo keyinfo;
  UnpackedRecord *pUnpacked;      /* Unpacked version of aRecord[] */
  UnpackedRecord *pNewUnpacked;   /* Unpacked version of new.* record */
  int iNewReg;                    /* Register for new.* values */
  int iBlobWrite;                 /* Value returned by preupdate_blobwrite() */
  i64 iKey1;                      /* First key value passed to hook */
  i64 iKey2;                      /* Second key value passed to hook */
  Mem *aNew;                      /* Array of new.* values */
  Table *pTab;                    /* Schema object being updated */
  Index *pPk;                     /* PK index if pTab is WITHOUT ROWID */
};

/*
** An instance of this object is used to pass an vector of values into
** OP_VFilter, the xFilter method of a virtual table.  The vector is the
** set of values on the right-hand side of an IN constraint.
**
** The value as passed into xFilter is an sqlite3_value with a "pointer"
** type, such as is generated by sqlite3_result_pointer() and read by
** sqlite3_value_pointer.  Such values have MEM_Term|MEM_Subtype|MEM_Null
** and a subtype of 'p'.  The sqlite3_vtab_in_first() and _next() interfaces
** know how to use this object to step through all the values in the
** right operand of the IN constraint.
*/
typedef struct ValueList ValueList;
struct ValueList {
  BtCursor *pCsr;          /* An ephemeral table holding all values */
  sqlite3_value *pOut;     /* Register to hold each decoded output value */
};

/* Size of content associated with serial types that fit into a
** single-byte varint.
*/
#ifndef SQLITE_AMALGAMATION
SQLITE_PRIVATE const u8 sqlite3SmallTypeSizes[];
#endif

/*
** Function prototypes
*/
SQLITE_PRIVATE void sqlite3VdbeError(Vdbe*, const char *, ...);
SQLITE_PRIVATE void sqlite3VdbeFreeCursor(Vdbe *, VdbeCursor*);
SQLITE_PRIVATE void sqlite3VdbeFreeCursorNN(Vdbe*,VdbeCursor*);
void sqliteVdbePopStack(Vdbe*,int);
SQLITE_PRIVATE int SQLITE_NOINLINE sqlite3VdbeHandleMovedCursor(VdbeCursor *p);
SQLITE_PRIVATE int SQLITE_NOINLINE sqlite3VdbeFinishMoveto(VdbeCursor*);
SQLITE_PRIVATE int sqlite3VdbeCursorRestore(VdbeCursor*);
SQLITE_PRIVATE u32 sqlite3VdbeSerialTypeLen(u32);
SQLITE_PRIVATE u8 sqlite3VdbeOneByteSerialTypeLen(u8);
#ifdef SQLITE_MIXED_ENDIAN_64BIT_FLOAT
SQLITE_PRIVATE   u64 sqlite3FloatSwap(u64 in);
# define swapMixedEndianFloat(X)  X = sqlite3FloatSwap(X)
#else
# define swapMixedEndianFloat(X)
#endif
SQLITE_PRIVATE void sqlite3VdbeSerialGet(const unsigned char*, u32, Mem*);
SQLITE_PRIVATE void sqlite3VdbeDeleteAuxData(sqlite3*, AuxData**, int, int);

int sqlite2BtreeKeyCompare(BtCursor *, const void *, int, int, int *);
SQLITE_PRIVATE int sqlite3VdbeIdxKeyCompare(sqlite3*,VdbeCursor*,UnpackedRecord*,int*);
SQLITE_PRIVATE int sqlite3VdbeIdxRowid(sqlite3*, BtCursor*, i64*);
SQLITE_PRIVATE int sqlite3VdbeExec(Vdbe*);
#if !defined(SQLITE_OMIT_EXPLAIN) || defined(SQLITE_ENABLE_BYTECODE_VTAB)
SQLITE_PRIVATE int sqlite3VdbeNextOpcode(Vdbe*,Mem*,int,int*,int*,Op**);
SQLITE_PRIVATE char *sqlite3VdbeDisplayP4(sqlite3*,Op*);
#endif
#if defined(SQLITE_ENABLE_EXPLAIN_COMMENTS)
SQLITE_PRIVATE char *sqlite3VdbeDisplayComment(sqlite3*,const Op*,const char*);
#endif
#if !defined(SQLITE_OMIT_EXPLAIN)
SQLITE_PRIVATE int sqlite3VdbeList(Vdbe*);
#endif
SQLITE_PRIVATE int sqlite3VdbeHalt(Vdbe*);
SQLITE_PRIVATE int sqlite3VdbeChangeEncoding(Mem *, int);
SQLITE_PRIVATE int sqlite3VdbeMemTooBig(Mem*);
SQLITE_PRIVATE int sqlite3VdbeMemCopy(Mem*, const Mem*);
SQLITE_PRIVATE void sqlite3VdbeMemShallowCopy(Mem*, const Mem*, int);
SQLITE_PRIVATE void sqlite3VdbeMemMove(Mem*, Mem*);
SQLITE_PRIVATE int sqlite3VdbeMemNulTerminate(Mem*);
SQLITE_PRIVATE int sqlite3VdbeMemSetStr(Mem*, const char*, i64, u8, void(*)(void*));
SQLITE_PRIVATE void sqlite3VdbeMemSetInt64(Mem*, i64);
#ifdef SQLITE_OMIT_FLOATING_POINT
# define sqlite3VdbeMemSetDouble sqlite3VdbeMemSetInt64
#else
SQLITE_PRIVATE   void sqlite3VdbeMemSetDouble(Mem*, double);
#endif
SQLITE_PRIVATE void sqlite3VdbeMemSetPointer(Mem*, void*, const char*, void(*)(void*));
SQLITE_PRIVATE void sqlite3VdbeMemInit(Mem*,sqlite3*,u16);
SQLITE_PRIVATE void sqlite3VdbeMemSetNull(Mem*);
#ifndef SQLITE_OMIT_INCRBLOB
SQLITE_PRIVATE void sqlite3VdbeMemSetZeroBlob(Mem*,int);
#else
SQLITE_PRIVATE int sqlite3VdbeMemSetZeroBlob(Mem*,int);
#endif
#ifdef SQLITE_DEBUG
SQLITE_PRIVATE int sqlite3VdbeMemIsRowSet(const Mem*);
#endif
SQLITE_PRIVATE int sqlite3VdbeMemSetRowSet(Mem*);
SQLITE_PRIVATE void sqlite3VdbeMemZeroTerminateIfAble(Mem*);
SQLITE_PRIVATE int sqlite3VdbeMemMakeWriteable(Mem*);
SQLITE_PRIVATE int sqlite3VdbeMemStringify(Mem*, u8, u8);
SQLITE_PRIVATE int sqlite3IntFloatCompare(i64,double);
SQLITE_PRIVATE i64 sqlite3VdbeIntValue(const Mem*);
SQLITE_PRIVATE int sqlite3VdbeMemIntegerify(Mem*);
SQLITE_PRIVATE double sqlite3VdbeRealValue(Mem*);
SQLITE_PRIVATE int sqlite3VdbeBooleanValue(Mem*, int ifNull);
SQLITE_PRIVATE void sqlite3VdbeIntegerAffinity(Mem*);
SQLITE_PRIVATE int sqlite3VdbeMemRealify(Mem*);
SQLITE_PRIVATE int sqlite3VdbeMemNumerify(Mem*);
SQLITE_PRIVATE int sqlite3VdbeMemCast(Mem*,u8,u8);
SQLITE_PRIVATE int sqlite3VdbeMemFromBtree(BtCursor*,u32,u32,Mem*);
SQLITE_PRIVATE int sqlite3VdbeMemFromBtreeZeroOffset(BtCursor*,u32,Mem*);
SQLITE_PRIVATE void sqlite3VdbeMemRelease(Mem *p);
SQLITE_PRIVATE void sqlite3VdbeMemReleaseMalloc(Mem*p);
SQLITE_PRIVATE int sqlite3VdbeMemFinalize(Mem*, FuncDef*);
#ifndef SQLITE_OMIT_WINDOWFUNC
SQLITE_PRIVATE int sqlite3VdbeMemAggValue(Mem*, Mem*, FuncDef*);
#endif
#if !defined(SQLITE_OMIT_EXPLAIN) || defined(SQLITE_ENABLE_BYTECODE_VTAB)
SQLITE_PRIVATE const char *sqlite3OpcodeName(int);
#endif
SQLITE_PRIVATE int sqlite3VdbeMemGrow(Mem *pMem, int n, int preserve);
SQLITE_PRIVATE int sqlite3VdbeMemClearAndResize(Mem *pMem, int n);
SQLITE_PRIVATE int sqlite3VdbeCloseStatement(Vdbe *, int);
#ifdef SQLITE_DEBUG
SQLITE_PRIVATE int sqlite3VdbeFrameIsValid(VdbeFrame*);
#endif
SQLITE_PRIVATE void sqlite3VdbeFrameMemDel(void*);      /* Destructor on Mem */
SQLITE_PRIVATE void sqlite3VdbeFrameDelete(VdbeFrame*); /* Actually deletes the Frame */
SQLITE_PRIVATE int sqlite3VdbeFrameRestore(VdbeFrame *);
#ifdef SQLITE_ENABLE_PREUPDATE_HOOK
SQLITE_PRIVATE void sqlite3VdbePreUpdateHook(
    Vdbe*,VdbeCursor*,int,const char*,Table*,i64,int,int);
#endif
SQLITE_PRIVATE int sqlite3VdbeTransferError(Vdbe *p);

SQLITE_PRIVATE int sqlite3VdbeSorterInit(sqlite3 *, int, VdbeCursor *);
SQLITE_PRIVATE void sqlite3VdbeSorterReset(sqlite3 *, VdbeSorter *);
SQLITE_PRIVATE void sqlite3VdbeSorterClose(sqlite3 *, VdbeCursor *);
SQLITE_PRIVATE int sqlite3VdbeSorterRowkey(const VdbeCursor *, Mem *);
SQLITE_PRIVATE int sqlite3VdbeSorterNext(sqlite3 *, const VdbeCursor *);
SQLITE_PRIVATE int sqlite3VdbeSorterRewind(const VdbeCursor *, int *);
SQLITE_PRIVATE int sqlite3VdbeSorterWrite(const VdbeCursor *, Mem *);
SQLITE_PRIVATE int sqlite3VdbeSorterCompare(const VdbeCursor *, Mem *, int, int *);

SQLITE_PRIVATE void sqlite3VdbeValueListFree(void*);

#ifdef SQLITE_DEBUG
SQLITE_PRIVATE   void sqlite3VdbeIncrWriteCounter(Vdbe*, VdbeCursor*);
SQLITE_PRIVATE   void sqlite3VdbeAssertAbortable(Vdbe*);
#else
# define sqlite3VdbeIncrWriteCounter(V,C)
# define sqlite3VdbeAssertAbortable(V)
#endif

#if !defined(SQLITE_OMIT_SHARED_CACHE)
SQLITE_PRIVATE   void sqlite3VdbeEnter(Vdbe*);
#else
# define sqlite3VdbeEnter(X)
#endif

#if !defined(SQLITE_OMIT_SHARED_CACHE) && SQLITE_THREADSAFE>0
SQLITE_PRIVATE   void sqlite3VdbeLeave(Vdbe*);
#else
# define sqlite3VdbeLeave(X)
#endif

#ifdef SQLITE_DEBUG
SQLITE_PRIVATE void sqlite3VdbeMemAboutToChange(Vdbe*,Mem*);
SQLITE_PRIVATE int sqlite3VdbeCheckMemInvariants(Mem*);
#endif

#ifndef SQLITE_OMIT_FOREIGN_KEY
SQLITE_PRIVATE int sqlite3VdbeCheckFk(Vdbe *, int);
#else
# define sqlite3VdbeCheckFk(p,i) 0
#endif

#ifdef SQLITE_DEBUG
SQLITE_PRIVATE   void sqlite3VdbePrintSql(Vdbe*);
SQLITE_PRIVATE   void sqlite3VdbeMemPrettyPrint(Mem *pMem, StrAccum *pStr);
#endif
#ifndef SQLITE_OMIT_UTF16
SQLITE_PRIVATE   int sqlite3VdbeMemTranslate(Mem*, u8);
SQLITE_PRIVATE   int sqlite3VdbeMemHandleBom(Mem *pMem);
#endif

