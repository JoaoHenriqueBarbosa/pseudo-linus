struct LookasideSlot {
  LookasideSlot *pNext;    /* Next buffer in the list of free buffers */
};

#define DisableLookaside  db->lookaside.bDisable++;db->lookaside.sz=0
#define EnableLookaside   db->lookaside.bDisable--;\
   db->lookaside.sz=db->lookaside.bDisable?0:db->lookaside.szTrue

/* Size of the smaller allocations in two-size lookaside */
#ifdef SQLITE_OMIT_TWOSIZE_LOOKASIDE
#  define LOOKASIDE_SMALL           0
#else
#  define LOOKASIDE_SMALL         128
#endif

/*
** A hash table for built-in function definitions.  (Application-defined
** functions use a regular table table from hash.h.)
**
** Hash each FuncDef structure into one of the FuncDefHash.a[] slots.
** Collisions are on the FuncDef.u.pHash chain.  Use the SQLITE_FUNC_HASH()
** macro to compute a hash on the function name.
*/
#define SQLITE_FUNC_HASH_SZ 23
struct FuncDefHash {
  FuncDef *a[SQLITE_FUNC_HASH_SZ];       /* Hash table for functions */
};
#define SQLITE_FUNC_HASH(C,L) (((C)+(L))%SQLITE_FUNC_HASH_SZ)

#if defined(SQLITE_USER_AUTHENTICATION)
# warning  "The SQLITE_USER_AUTHENTICATION extension is deprecated. \
 See ext/userauth/user-auth.txt for details."
#endif
#ifdef SQLITE_USER_AUTHENTICATION
/*
** Information held in the "sqlite3" database connection object and used
** to manage user authentication.
*/
typedef struct sqlite3_userauth sqlite3_userauth;
struct sqlite3_userauth {
  u8 authLevel;                 /* Current authentication level */
  int nAuthPW;                  /* Size of the zAuthPW in bytes */
  char *zAuthPW;                /* Password used to authenticate */
  char *zAuthUser;              /* User name used to authenticate */
};

/* Allowed values for sqlite3_userauth.authLevel */
#define UAUTH_Unknown     0     /* Authentication not yet checked */
#define UAUTH_Fail        1     /* User authentication failed */
#define UAUTH_User        2     /* Authenticated as a normal user */
#define UAUTH_Admin       3     /* Authenticated as an administrator */

/* Functions used only by user authorization logic */
SQLITE_PRIVATE int sqlite3UserAuthTable(const char*);
SQLITE_PRIVATE int sqlite3UserAuthCheckLogin(sqlite3*,const char*,u8*);
SQLITE_PRIVATE void sqlite3UserAuthInit(sqlite3*);
SQLITE_PRIVATE void sqlite3CryptFunc(sqlite3_context*,int,sqlite3_value**);

#endif /* SQLITE_USER_AUTHENTICATION */

/*
** typedef for the authorization callback function.
*/
#ifdef SQLITE_USER_AUTHENTICATION
  typedef int (*sqlite3_xauth)(void*,int,const char*,const char*,const char*,
                               const char*, const char*);
#else
  typedef int (*sqlite3_xauth)(void*,int,const char*,const char*,const char*,
                               const char*);
#endif

#ifndef SQLITE_OMIT_DEPRECATED
/* This is an extra SQLITE_TRACE macro that indicates "legacy" tracing
** in the style of sqlite3_trace()
*/
#define SQLITE_TRACE_LEGACY          0x40     /* Use the legacy xTrace */
#define SQLITE_TRACE_XPROFILE        0x80     /* Use the legacy xProfile */
#else
#define SQLITE_TRACE_LEGACY          0
#define SQLITE_TRACE_XPROFILE        0
#endif /* SQLITE_OMIT_DEPRECATED */
#define SQLITE_TRACE_NONLEGACY_MASK  0x0f     /* Normal flags */

/*
** Maximum number of sqlite3.aDb[] entries.  This is the number of attached
** databases plus 2 for "main" and "temp".
*/
#define SQLITE_MAX_DB (SQLITE_MAX_ATTACHED+2)

/*
** Each database connection is an instance of the following structure.
*/
struct sqlite3 {
  sqlite3_vfs *pVfs;            /* OS Interface */
  struct Vdbe *pVdbe;           /* List of active virtual machines */
  CollSeq *pDfltColl;           /* BINARY collseq for the database encoding */
  sqlite3_mutex *mutex;         /* Connection mutex */
  Db *aDb;                      /* All backends */
  int nDb;                      /* Number of backends currently in use */
  u32 mDbFlags;                 /* flags recording internal state */
  u64 flags;                    /* flags settable by pragmas. See below */
  i64 lastRowid;                /* ROWID of most recent insert (see above) */
  i64 szMmap;                   /* Default mmap_size setting */
  u32 nSchemaLock;              /* Do not reset the schema when non-zero */
  unsigned int openFlags;       /* Flags passed to sqlite3_vfs.xOpen() */
  int errCode;                  /* Most recent error code (SQLITE_*) */
  int errByteOffset;            /* Byte offset of error in SQL statement */
  int errMask;                  /* & result codes with this before returning */
  int iSysErrno;                /* Errno value from last system error */
  u32 dbOptFlags;               /* Flags to enable/disable optimizations */
  u8 enc;                       /* Text encoding */
  u8 autoCommit;                /* The auto-commit flag. */
  u8 temp_store;                /* 1: file 2: memory 0: default */
  u8 mallocFailed;              /* True if we have seen a malloc failure */
  u8 bBenignMalloc;             /* Do not require OOMs if true */
  u8 dfltLockMode;              /* Default locking-mode for attached dbs */
  signed char nextAutovac;      /* Autovac setting after VACUUM if >=0 */
  u8 suppressErr;               /* Do not issue error messages if true */
  u8 vtabOnConflict;            /* Value to return for s3_vtab_on_conflict() */
  u8 isTransactionSavepoint;    /* True if the outermost savepoint is a TS */
  u8 mTrace;                    /* zero or more SQLITE_TRACE flags */
  u8 noSharedCache;             /* True if no shared-cache backends */
  u8 nSqlExec;                  /* Number of pending OP_SqlExec opcodes */
  u8 eOpenState;                /* Current condition of the connection */
  int nextPagesize;             /* Pagesize after VACUUM if >0 */
  i64 nChange;                  /* Value returned by sqlite3_changes() */
  i64 nTotalChange;             /* Value returned by sqlite3_total_changes() */
  int aLimit[SQLITE_N_LIMIT];   /* Limits */
  int nMaxSorterMmap;           /* Maximum size of regions mapped by sorter */
  struct sqlite3InitInfo {      /* Information used during initialization */
    Pgno newTnum;               /* Rootpage of table being initialized */
    u8 iDb;                     /* Which db file is being initialized */
    u8 busy;                    /* TRUE if currently initializing */
    unsigned orphanTrigger : 1; /* Last statement is orphaned TEMP trigger */
    unsigned imposterTable : 1; /* Building an imposter table */
    unsigned reopenMemdb : 1;   /* ATTACH is really a reopen using MemDB */
    const char **azInit;        /* "type", "name", and "tbl_name" columns */
  } init;
  int nVdbeActive;              /* Number of VDBEs currently running */
  int nVdbeRead;                /* Number of active VDBEs that read or write */
  int nVdbeWrite;               /* Number of active VDBEs that read and write */
  int nVdbeExec;                /* Number of nested calls to VdbeExec() */
  int nVDestroy;                /* Number of active OP_VDestroy operations */
  int nExtension;               /* Number of loaded extensions */
  void **aExtension;            /* Array of shared library handles */
  union {
    void (*xLegacy)(void*,const char*);   /* mTrace==SQLITE_TRACE_LEGACY */
    int (*xV2)(u32,void*,void*,void*);    /* All other mTrace values */
  } trace;
  void *pTraceArg;                        /* Argument to the trace function */
#ifndef SQLITE_OMIT_DEPRECATED
  void (*xProfile)(void*,const char*,u64);  /* Profiling function */
  void *pProfileArg;                        /* Argument to profile function */
#endif
  void *pCommitArg;                 /* Argument to xCommitCallback() */
  int (*xCommitCallback)(void*);    /* Invoked at every commit. */
  void *pRollbackArg;               /* Argument to xRollbackCallback() */
  void (*xRollbackCallback)(void*); /* Invoked at every commit. */
  void *pUpdateArg;
  void (*xUpdateCallback)(void*,int, const char*,const char*,sqlite_int64);
  void *pAutovacPagesArg;           /* Client argument to autovac_pages */
  void (*xAutovacDestr)(void*);     /* Destructor for pAutovacPAgesArg */
  unsigned int (*xAutovacPages)(void*,const char*,u32,u32,u32);
  Parse *pParse;                /* Current parse */
#ifdef SQLITE_ENABLE_PREUPDATE_HOOK
  void *pPreUpdateArg;          /* First argument to xPreUpdateCallback */
  void (*xPreUpdateCallback)(   /* Registered using sqlite3_preupdate_hook() */
    void*,sqlite3*,int,char const*,char const*,sqlite3_int64,sqlite3_int64
  );
  PreUpdate *pPreUpdate;        /* Context for active pre-update callback */
#endif /* SQLITE_ENABLE_PREUPDATE_HOOK */
#ifndef SQLITE_OMIT_WAL
  int (*xWalCallback)(void *, sqlite3 *, const char *, int);
  void *pWalArg;
#endif
  void(*xCollNeeded)(void*,sqlite3*,int eTextRep,const char*);
  void(*xCollNeeded16)(void*,sqlite3*,int eTextRep,const void*);
  void *pCollNeededArg;
  sqlite3_value *pErr;          /* Most recent error message */
  union {
    volatile int isInterrupted; /* True if sqlite3_interrupt has been called */
    double notUsed1;            /* Spacer */
  } u1;
  Lookaside lookaside;          /* Lookaside malloc configuration */
#ifndef SQLITE_OMIT_AUTHORIZATION
  sqlite3_xauth xAuth;          /* Access authorization function */
  void *pAuthArg;               /* 1st argument to the access auth function */
#endif
#ifndef SQLITE_OMIT_PROGRESS_CALLBACK
  int (*xProgress)(void *);     /* The progress callback */
  void *pProgressArg;           /* Argument to the progress callback */
  unsigned nProgressOps;        /* Number of opcodes for progress callback */
#endif
#ifndef SQLITE_OMIT_VIRTUALTABLE
  int nVTrans;                  /* Allocated size of aVTrans */
  Hash aModule;                 /* populated by sqlite3_create_module() */
  VtabCtx *pVtabCtx;            /* Context for active vtab connect/create */
  VTable **aVTrans;             /* Virtual tables with open transactions */
  VTable *pDisconnect;          /* Disconnect these in next sqlite3_prepare() */
#endif
  Hash aFunc;                   /* Hash table of connection functions */
  Hash aCollSeq;                /* All collating sequences */
  BusyHandler busyHandler;      /* Busy callback */
  Db aDbStatic[2];              /* Static space for the 2 default backends */
  Savepoint *pSavepoint;        /* List of active savepoints */
  int nAnalysisLimit;           /* Number of index rows to ANALYZE */
  int busyTimeout;              /* Busy handler timeout, in msec */
  int nSavepoint;               /* Number of non-transaction savepoints */
  int nStatement;               /* Number of nested statement-transactions  */
  i64 nDeferredCons;            /* Net deferred constraints this transaction. */
  i64 nDeferredImmCons;         /* Net deferred immediate constraints */
  int *pnBytesFreed;            /* If not NULL, increment this in DbFree() */
  DbClientData *pDbData;        /* sqlite3_set_clientdata() content */
#ifdef SQLITE_ENABLE_UNLOCK_NOTIFY
  /* The following variables are all protected by the STATIC_MAIN
  ** mutex, not by sqlite3.mutex. They are used by code in notify.c.
  **
  ** When X.pUnlockConnection==Y, that means that X is waiting for Y to
  ** unlock so that it can proceed.
  **
  ** When X.pBlockingConnection==Y, that means that something that X tried
  ** tried to do recently failed with an SQLITE_LOCKED error due to locks
  ** held by Y.
  */
  sqlite3 *pBlockingConnection; /* Connection that caused SQLITE_LOCKED */
  sqlite3 *pUnlockConnection;           /* Connection to watch for unlock */
  void *pUnlockArg;                     /* Argument to xUnlockNotify */
  void (*xUnlockNotify)(void **, int);  /* Unlock notify callback */
  sqlite3 *pNextBlocked;        /* Next in list of all blocked connections */
#endif
#ifdef SQLITE_USER_AUTHENTICATION
  sqlite3_userauth auth;        /* User authentication information */
#endif
};

/*
** A macro to discover the encoding of a database.
*/
#define SCHEMA_ENC(db) ((db)->aDb[0].pSchema->enc)
#define ENC(db)        ((db)->enc)

/*
** A u64 constant where the lower 32 bits are all zeros.  Only the
** upper 32 bits are included in the argument.  Necessary because some
** C-compilers still do not accept LL integer literals.
*/
#define HI(X)  ((u64)(X)<<32)

/*
** Possible values for the sqlite3.flags.
**
** Value constraints (enforced via assert()):
**      SQLITE_FullFSync     == PAGER_FULLFSYNC
**      SQLITE_CkptFullFSync == PAGER_CKPT_FULLFSYNC
**      SQLITE_CacheSpill    == PAGER_CACHE_SPILL
*/
#define SQLITE_WriteSchema    0x00000001  /* OK to update SQLITE_SCHEMA */
#define SQLITE_LegacyFileFmt  0x00000002  /* Create new databases in format 1 */
#define SQLITE_FullColNames   0x00000004  /* Show full column names on SELECT */
#define SQLITE_FullFSync      0x00000008  /* Use full fsync on the backend */
#define SQLITE_CkptFullFSync  0x00000010  /* Use full fsync for checkpoint */
#define SQLITE_CacheSpill     0x00000020  /* OK to spill pager cache */
#define SQLITE_ShortColNames  0x00000040  /* Show short columns names */
#define SQLITE_TrustedSchema  0x00000080  /* Allow unsafe functions and
                                          ** vtabs in the schema definition */
#define SQLITE_NullCallback   0x00000100  /* Invoke the callback once if the */
                                          /*   result set is empty */
#define SQLITE_IgnoreChecks   0x00000200  /* Do not enforce check constraints */
#define SQLITE_StmtScanStatus 0x00000400  /* Enable stmt_scanstats() counters */
#define SQLITE_NoCkptOnClose  0x00000800  /* No checkpoint on close()/DETACH */
#define SQLITE_ReverseOrder   0x00001000  /* Reverse unordered SELECTs */
#define SQLITE_RecTriggers    0x00002000  /* Enable recursive triggers */
#define SQLITE_ForeignKeys    0x00004000  /* Enforce foreign key constraints  */
#define SQLITE_AutoIndex      0x00008000  /* Enable automatic indexes */
#define SQLITE_LoadExtension  0x00010000  /* Enable load_extension */
#define SQLITE_LoadExtFunc    0x00020000  /* Enable load_extension() SQL func */
#define SQLITE_EnableTrigger  0x00040000  /* True to enable triggers */
#define SQLITE_DeferFKs       0x00080000  /* Defer all FK constraints */
#define SQLITE_QueryOnly      0x00100000  /* Disable database changes */
#define SQLITE_CellSizeCk     0x00200000  /* Check btree cell sizes on load */
#define SQLITE_Fts3Tokenizer  0x00400000  /* Enable fts3_tokenizer(2) */
#define SQLITE_EnableQPSG     0x00800000  /* Query Planner Stability Guarantee*/
#define SQLITE_TriggerEQP     0x01000000  /* Show trigger EXPLAIN QUERY PLAN */
#define SQLITE_ResetDatabase  0x02000000  /* Reset the database */
#define SQLITE_LegacyAlter    0x04000000  /* Legacy ALTER TABLE behaviour */
#define SQLITE_NoSchemaError  0x08000000  /* Do not report schema parse errors*/
#define SQLITE_Defensive      0x10000000  /* Input SQL is likely hostile */
#define SQLITE_DqsDDL         0x20000000  /* dbl-quoted strings allowed in DDL*/
#define SQLITE_DqsDML         0x40000000  /* dbl-quoted strings allowed in DML*/
#define SQLITE_EnableView     0x80000000  /* Enable the use of views */
#define SQLITE_CountRows      HI(0x00001) /* Count rows changed by INSERT, */
                                          /*   DELETE, or UPDATE and return */
                                          /*   the count using a callback. */
#define SQLITE_CorruptRdOnly  HI(0x00002) /* Prohibit writes due to error */
#define SQLITE_ReadUncommit   HI(0x00004) /* READ UNCOMMITTED in shared-cache */
#define SQLITE_FkNoAction     HI(0x00008) /* Treat all FK as NO ACTION */

/* Flags used only if debugging */
#ifdef SQLITE_DEBUG
#define SQLITE_SqlTrace       HI(0x0100000) /* Debug print SQL as it executes */
#define SQLITE_VdbeListing    HI(0x0200000) /* Debug listings of VDBE progs */
#define SQLITE_VdbeTrace      HI(0x0400000) /* True to trace VDBE execution */
#define SQLITE_VdbeAddopTrace HI(0x0800000) /* Trace sqlite3VdbeAddOp() calls */
#define SQLITE_VdbeEQP        HI(0x1000000) /* Debug EXPLAIN QUERY PLAN */
#define SQLITE_ParserTrace    HI(0x2000000) /* PRAGMA parser_trace=ON */
#endif

/*
** Allowed values for sqlite3.mDbFlags
*/
#define DBFLAG_SchemaChange   0x0001  /* Uncommitted Hash table changes */
#define DBFLAG_PreferBuiltin  0x0002  /* Preference to built-in funcs */
#define DBFLAG_Vacuum         0x0004  /* Currently in a VACUUM */
#define DBFLAG_VacuumInto     0x0008  /* Currently running VACUUM INTO */
#define DBFLAG_SchemaKnownOk  0x0010  /* Schema is known to be valid */
#define DBFLAG_InternalFunc   0x0020  /* Allow use of internal functions */
#define DBFLAG_EncodingFixed  0x0040  /* No longer possible to change enc. */

/*
** Bits of the sqlite3.dbOptFlags field that are used by the
** sqlite3_test_control(SQLITE_TESTCTRL_OPTIMIZATIONS,...) interface to
** selectively disable various optimizations.
*/
#define SQLITE_QueryFlattener 0x00000001 /* Query flattening */
#define SQLITE_WindowFunc     0x00000002 /* Use xInverse for window functions */
#define SQLITE_GroupByOrder   0x00000004 /* GROUPBY cover of ORDERBY */
#define SQLITE_FactorOutConst 0x00000008 /* Constant factoring */
#define SQLITE_DistinctOpt    0x00000010 /* DISTINCT using indexes */
#define SQLITE_CoverIdxScan   0x00000020 /* Covering index scans */
#define SQLITE_OrderByIdxJoin 0x00000040 /* ORDER BY of joins via index */
#define SQLITE_Transitive     0x00000080 /* Transitive constraints */
#define SQLITE_OmitNoopJoin   0x00000100 /* Omit unused tables in joins */
#define SQLITE_CountOfView    0x00000200 /* The count-of-view optimization */
#define SQLITE_CursorHints    0x00000400 /* Add OP_CursorHint opcodes */
#define SQLITE_Stat4          0x00000800 /* Use STAT4 data */
   /* TH3 expects this value  ^^^^^^^^^^ to be 0x0000800. Don't change it */
#define SQLITE_PushDown       0x00001000 /* WHERE-clause push-down opt */
#define SQLITE_SimplifyJoin   0x00002000 /* Convert LEFT JOIN to JOIN */
#define SQLITE_SkipScan       0x00004000 /* Skip-scans */
#define SQLITE_PropagateConst 0x00008000 /* The constant propagation opt */
#define SQLITE_MinMaxOpt      0x00010000 /* The min/max optimization */
#define SQLITE_SeekScan       0x00020000 /* The OP_SeekScan optimization */
#define SQLITE_OmitOrderBy    0x00040000 /* Omit pointless ORDER BY */
   /* TH3 expects this value  ^^^^^^^^^^ to be 0x40000. Coordinate any change */
#define SQLITE_BloomFilter    0x00080000 /* Use a Bloom filter on searches */
#define SQLITE_BloomPulldown  0x00100000 /* Run Bloom filters early */
#define SQLITE_BalancedMerge  0x00200000 /* Balance multi-way merges */
#define SQLITE_ReleaseReg     0x00400000 /* Use OP_ReleaseReg for testing */
#define SQLITE_FlttnUnionAll  0x00800000 /* Disable the UNION ALL flattener */
   /* TH3 expects this value  ^^^^^^^^^^ See flatten04.test */
#define SQLITE_IndexedExpr    0x01000000 /* Pull exprs from index when able */
#define SQLITE_Coroutines     0x02000000 /* Co-routines for subqueries */
#define SQLITE_NullUnusedCols 0x04000000 /* NULL unused columns in subqueries */
#define SQLITE_OnePass        0x08000000 /* Single-pass DELETE and UPDATE */
#define SQLITE_AllOpts        0xffffffff /* All optimizations */

/*
** Macros for testing whether or not optimizations are enabled or disabled.
*/
#define OptimizationDisabled(db, mask)  (((db)->dbOptFlags&(mask))!=0)
#define OptimizationEnabled(db, mask)   (((db)->dbOptFlags&(mask))==0)

/*
** Return true if it OK to factor constant expressions into the initialization
** code. The argument is a Parse object for the code generator.
*/
#define ConstFactorOk(P) ((P)->okConstFactor)

/* Possible values for the sqlite3.eOpenState field.
** The numbers are randomly selected such that a minimum of three bits must
** change to convert any number to another or to zero
*/
#define SQLITE_STATE_OPEN     0x76  /* Database is open */
#define SQLITE_STATE_CLOSED   0xce  /* Database is closed */
#define SQLITE_STATE_SICK     0xba  /* Error and awaiting close */
#define SQLITE_STATE_BUSY     0x6d  /* Database currently in use */
#define SQLITE_STATE_ERROR    0xd5  /* An SQLITE_MISUSE error occurred */
#define SQLITE_STATE_ZOMBIE   0xa7  /* Close with last statement close */

/*
** Each SQL function is defined by an instance of the following
** structure.  For global built-in functions (ex: substr(), max(), count())
** a pointer to this structure is held in the sqlite3BuiltinFunctions object.
** For per-connection application-defined functions, a pointer to this
** structure is held in the db->aHash hash table.
**
** The u.pHash field is used by the global built-ins.  The u.pDestructor
** field is used by per-connection app-def functions.
*/
struct FuncDef {
  i8 nArg;             /* Number of arguments.  -1 means unlimited */
  u32 funcFlags;       /* Some combination of SQLITE_FUNC_* */
  void *pUserData;     /* User data parameter */
  FuncDef *pNext;      /* Next function with same name */
  void (*xSFunc)(sqlite3_context*,int,sqlite3_value**); /* func or agg-step */
  void (*xFinalize)(sqlite3_context*);                  /* Agg finalizer */
  void (*xValue)(sqlite3_context*);                     /* Current agg value */
  void (*xInverse)(sqlite3_context*,int,sqlite3_value**); /* inverse agg-step */
  const char *zName;   /* SQL name of the function. */
  union {
    FuncDef *pHash;      /* Next with a different name but the same hash */
    FuncDestructor *pDestructor;   /* Reference counted destructor function */
  } u; /* pHash if SQLITE_FUNC_BUILTIN, pDestructor otherwise */
};
