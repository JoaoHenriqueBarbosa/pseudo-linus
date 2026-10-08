
/*
** Information about a RETURNING clause
*/
struct Returning {
  Parse *pParse;        /* The parse that includes the RETURNING clause */
  ExprList *pReturnEL;  /* List of expressions to return */
  Trigger retTrig;      /* The transient trigger that implements RETURNING */
  TriggerStep retTStep; /* The trigger step */
  int iRetCur;          /* Transient table holding RETURNING results */
  int nRetCol;          /* Number of in pReturnEL after expansion */
  int iRetReg;          /* Register array for holding a row of RETURNING */
  char zName[40];       /* Name of trigger: "sqlite_returning_%p" */
};

/*
** An objected used to accumulate the text of a string where we
** do not necessarily know how big the string will be in the end.
*/
struct sqlite3_str {
  sqlite3 *db;         /* Optional database for lookaside.  Can be NULL */
  char *zText;         /* The string collected so far */
  u32  nAlloc;         /* Amount of space allocated in zText */
  u32  mxAlloc;        /* Maximum allowed allocation.  0 for no malloc usage */
  u32  nChar;          /* Length of the string so far */
  u8   accError;       /* SQLITE_NOMEM or SQLITE_TOOBIG */
  u8   printfFlags;    /* SQLITE_PRINTF flags below */
};
#define SQLITE_PRINTF_INTERNAL 0x01  /* Internal-use-only converters allowed */
#define SQLITE_PRINTF_SQLFUNC  0x02  /* SQL function arguments to VXPrintf */
#define SQLITE_PRINTF_MALLOCED 0x04  /* True if xText is allocated space */

#define isMalloced(X)  (((X)->printfFlags & SQLITE_PRINTF_MALLOCED)!=0)

/*
** The following object is the header for an "RCStr" or "reference-counted
** string".  An RCStr is passed around and used like any other char*
** that has been dynamically allocated.  The important interface
** differences:
**
**   1.  RCStr strings are reference counted.  They are deallocated
**       when the reference count reaches zero.
**
**   2.  Use sqlite3RCStrUnref() to free an RCStr string rather than
**       sqlite3_free()
**
**   3.  Make a (read-only) copy of a read-only RCStr string using
**       sqlite3RCStrRef().
**
** "String" is in the name, but an RCStr object can also be used to hold
** binary data.
*/
struct RCStr {
  u64 nRCRef;            /* Number of references */
  /* Total structure size should be a multiple of 8 bytes for alignment */
};

/*
** A pointer to this structure is used to communicate information
** from sqlite3Init and OP_ParseSchema into the sqlite3InitCallback.
*/
typedef struct {
  sqlite3 *db;        /* The database being initialized */
  char **pzErrMsg;    /* Error message stored here */
  int iDb;            /* 0 for main database.  1 for TEMP, 2.. for ATTACHed */
  int rc;             /* Result code stored here */
  u32 mInitFlags;     /* Flags controlling error messages */
  u32 nInitRow;       /* Number of rows processed */
  Pgno mxPage;        /* Maximum page number.  0 for no limit. */
} InitData;

/*
** Allowed values for mInitFlags
*/
#define INITFLAG_AlterMask     0x0003  /* Types of ALTER */
#define INITFLAG_AlterRename   0x0001  /* Reparse after a RENAME */
#define INITFLAG_AlterDrop     0x0002  /* Reparse after a DROP COLUMN */
#define INITFLAG_AlterAdd      0x0003  /* Reparse after an ADD COLUMN */

/* Tuning parameters are set using SQLITE_TESTCTRL_TUNE and are controlled
** on debug-builds of the CLI using ".testctrl tune ID VALUE".  Tuning
** parameters are for temporary use during development, to help find
** optimal values for parameters in the query planner.  The should not
** be used on trunk check-ins.  They are a temporary mechanism available
** for transient development builds only.
**
** Tuning parameters are numbered starting with 1.
*/
#define SQLITE_NTUNE  6             /* Should be zero for all trunk check-ins */
#ifdef SQLITE_DEBUG
# define Tuning(X)  (sqlite3Config.aTune[(X)-1])
#else
# define Tuning(X)  0
#endif

/*
** Structure containing global configuration data for the SQLite library.
**
** This structure also contains some state information.
*/
struct Sqlite3Config {
  int bMemstat;                     /* True to enable memory status */
  u8 bCoreMutex;                    /* True to enable core mutexing */
  u8 bFullMutex;                    /* True to enable full mutexing */
  u8 bOpenUri;                      /* True to interpret filenames as URIs */
  u8 bUseCis;                       /* Use covering indices for full-scans */
  u8 bSmallMalloc;                  /* Avoid large memory allocations if true */
  u8 bExtraSchemaChecks;            /* Verify type,name,tbl_name in schema */
  u8 bUseLongDouble;                /* Make use of long double */
#ifdef SQLITE_DEBUG
  u8 bJsonSelfcheck;                /* Double-check JSON parsing */
#endif
  int mxStrlen;                     /* Maximum string length */
  int neverCorrupt;                 /* Database is always well-formed */
  int szLookaside;                  /* Default lookaside buffer size */
  int nLookaside;                   /* Default lookaside buffer count */
  int nStmtSpill;                   /* Stmt-journal spill-to-disk threshold */
  sqlite3_mem_methods m;            /* Low-level memory allocation interface */
  sqlite3_mutex_methods mutex;      /* Low-level mutex interface */
  sqlite3_pcache_methods2 pcache2;  /* Low-level page-cache interface */
  void *pHeap;                      /* Heap storage space */
  int nHeap;                        /* Size of pHeap[] */
  int mnReq, mxReq;                 /* Min and max heap requests sizes */
  sqlite3_int64 szMmap;             /* mmap() space per open file */
  sqlite3_int64 mxMmap;             /* Maximum value for szMmap */
  void *pPage;                      /* Page cache memory */
  int szPage;                       /* Size of each page in pPage[] */
  int nPage;                        /* Number of pages in pPage[] */
  int mxParserStack;                /* maximum depth of the parser stack */
  int sharedCacheEnabled;           /* true if shared-cache mode enabled */
  u32 szPma;                        /* Maximum Sorter PMA size */
  /* The above might be initialized to non-zero.  The following need to always
  ** initially be zero, however. */
  int isInit;                       /* True after initialization has finished */
  int inProgress;                   /* True while initialization in progress */
  int isMutexInit;                  /* True after mutexes are initialized */
  int isMallocInit;                 /* True after malloc is initialized */
  int isPCacheInit;                 /* True after malloc is initialized */
  int nRefInitMutex;                /* Number of users of pInitMutex */
  sqlite3_mutex *pInitMutex;        /* Mutex used by sqlite3_initialize() */
  void (*xLog)(void*,int,const char*); /* Function for logging */
  void *pLogArg;                       /* First argument to xLog() */
#ifdef SQLITE_ENABLE_SQLLOG
  void(*xSqllog)(void*,sqlite3*,const char*, int);
  void *pSqllogArg;
#endif
#ifdef SQLITE_VDBE_COVERAGE
  /* The following callback (if not NULL) is invoked on every VDBE branch
  ** operation.  Set the callback using SQLITE_TESTCTRL_VDBE_COVERAGE.
  */
  void (*xVdbeBranch)(void*,unsigned iSrcLine,u8 eThis,u8 eMx);  /* Callback */
  void *pVdbeBranchArg;                                     /* 1st argument */
#endif
#ifndef SQLITE_OMIT_DESERIALIZE
  sqlite3_int64 mxMemdbSize;        /* Default max memdb size */
#endif
#ifndef SQLITE_UNTESTABLE
  int (*xTestCallback)(int);        /* Invoked by sqlite3FaultSim() */
#endif
#ifdef SQLITE_ALLOW_ROWID_IN_VIEW
  u32 mNoVisibleRowid;              /* TF_NoVisibleRowid if the ROWID_IN_VIEW
                                    ** feature is disabled.  0 if rowids can
                                    ** occur in views. */
#endif
  int bLocaltimeFault;              /* True to fail localtime() calls */
  int (*xAltLocaltime)(const void*,void*); /* Alternative localtime() routine */
  int iOnceResetThreshold;          /* When to reset OP_Once counters */
  u32 szSorterRef;                  /* Min size in bytes to use sorter-refs */
  unsigned int iPrngSeed;           /* Alternative fixed seed for the PRNG */
  /* vvvv--- must be last ---vvv */
#ifdef SQLITE_DEBUG
  sqlite3_int64 aTune[SQLITE_NTUNE]; /* Tuning parameters */
#endif
};

/*
** This macro is used inside of assert() statements to indicate that
** the assert is only valid on a well-formed database.  Instead of:
**
**     assert( X );
**
** One writes:
**
**     assert( X || CORRUPT_DB );
**
** CORRUPT_DB is true during normal operation.  CORRUPT_DB does not indicate
** that the database is definitely corrupt, only that it might be corrupt.
** For most test cases, CORRUPT_DB is set to false using a special
** sqlite3_test_control().  This enables assert() statements to prove
** things that are always true for well-formed databases.
*/
#define CORRUPT_DB  (sqlite3Config.neverCorrupt==0)

/*
** Context pointer passed down through the tree-walk.
*/
struct Walker {
  Parse *pParse;                            /* Parser context.  */
  int (*xExprCallback)(Walker*, Expr*);     /* Callback for expressions */
  int (*xSelectCallback)(Walker*,Select*);  /* Callback for SELECTs */
  void (*xSelectCallback2)(Walker*,Select*);/* Second callback for SELECTs */
  int walkerDepth;                          /* Number of subqueries */
  u16 eCode;                                /* A small processing code */
  u16 mWFlags;                              /* Use-dependent flags */
  union {                                   /* Extra data for callback */
    NameContext *pNC;                         /* Naming context */
    int n;                                    /* A counter */
    int iCur;                                 /* A cursor number */
    SrcList *pSrcList;                        /* FROM clause */
    struct CCurHint *pCCurHint;               /* Used by codeCursorHint() */
    struct RefSrcList *pRefSrcList;           /* sqlite3ReferencesSrcList() */
    int *aiCol;                               /* array of column indexes */
    struct IdxCover *pIdxCover;               /* Check for index coverage */
    ExprList *pGroupBy;                       /* GROUP BY clause */
    Select *pSelect;                          /* HAVING to WHERE clause ctx */
    struct WindowRewrite *pRewrite;           /* Window rewrite context */
    struct WhereConst *pConst;                /* WHERE clause constants */
    struct RenameCtx *pRename;                /* RENAME COLUMN context */
    struct Table *pTab;                       /* Table of generated column */
    struct CoveringIndexCheck *pCovIdxCk;     /* Check for covering index */
    SrcItem *pSrcItem;                        /* A single FROM clause item */
    DbFixer *pFix;                            /* See sqlite3FixSelect() */
    Mem *aMem;                                /* See sqlite3BtreeCursorHint() */
  } u;
};

/*
** The following structure contains information used by the sqliteFix...
** routines as they walk the parse tree to make database references
** explicit.
*/
struct DbFixer {
  Parse *pParse;      /* The parsing context.  Error messages written here */
  Walker w;           /* Walker object */
  Schema *pSchema;    /* Fix items to this schema */
  u8 bTemp;           /* True for TEMP schema entries */
  const char *zDb;    /* Make sure all objects are contained in this database */
  const char *zType;  /* Type of the container - used for error messages */
  const Token *pName; /* Name of the container - used for error messages */
};

/* Forward declarations */
SQLITE_PRIVATE int sqlite3WalkExpr(Walker*, Expr*);
SQLITE_PRIVATE int sqlite3WalkExprNN(Walker*, Expr*);
SQLITE_PRIVATE int sqlite3WalkExprList(Walker*, ExprList*);
SQLITE_PRIVATE int sqlite3WalkSelect(Walker*, Select*);
SQLITE_PRIVATE int sqlite3WalkSelectExpr(Walker*, Select*);
SQLITE_PRIVATE int sqlite3WalkSelectFrom(Walker*, Select*);
SQLITE_PRIVATE int sqlite3ExprWalkNoop(Walker*, Expr*);
SQLITE_PRIVATE int sqlite3SelectWalkNoop(Walker*, Select*);
SQLITE_PRIVATE int sqlite3SelectWalkFail(Walker*, Select*);
SQLITE_PRIVATE int sqlite3WalkerDepthIncrease(Walker*,Select*);
SQLITE_PRIVATE void sqlite3WalkerDepthDecrease(Walker*,Select*);
SQLITE_PRIVATE void sqlite3WalkWinDefnDummyCallback(Walker*,Select*);

#ifdef SQLITE_DEBUG
SQLITE_PRIVATE void sqlite3SelectWalkAssert2(Walker*, Select*);
#endif

#ifndef SQLITE_OMIT_CTE
SQLITE_PRIVATE void sqlite3SelectPopWith(Walker*, Select*);
#else
# define sqlite3SelectPopWith 0
#endif

/*
** Return code from the parse-tree walking primitives and their
** callbacks.
*/
#define WRC_Continue    0   /* Continue down into children */
#define WRC_Prune       1   /* Omit children but continue walking siblings */
#define WRC_Abort       2   /* Abandon the tree walk */

/*
** A single common table expression
*/
struct Cte {
  char *zName;            /* Name of this CTE */
  ExprList *pCols;        /* List of explicit column names, or NULL */
  Select *pSelect;        /* The definition of this CTE */
  const char *zCteErr;    /* Error message for circular references */
  CteUse *pUse;           /* Usage information for this CTE */
  u8 eM10d;               /* The MATERIALIZED flag */
};

/*
** Allowed values for the materialized flag (eM10d):
*/
#define M10d_Yes       0  /* AS MATERIALIZED */
#define M10d_Any       1  /* Not specified.  Query planner's choice */
#define M10d_No        2  /* AS NOT MATERIALIZED */

/*
** An instance of the With object represents a WITH clause containing
** one or more CTEs (common table expressions).
*/
struct With {
  int nCte;               /* Number of CTEs in the WITH clause */
  int bView;              /* Belongs to the outermost Select of a view */
  With *pOuter;           /* Containing WITH clause, or NULL */
  Cte a[1];               /* For each CTE in the WITH clause.... */
};

/*
** The Cte object is not guaranteed to persist for the entire duration
** of code generation.  (The query flattener or other parser tree
** edits might delete it.)  The following object records information
** about each Common Table Expression that must be preserved for the
** duration of the parse.
**
** The CteUse objects are freed using sqlite3ParserAddCleanup() rather
** than sqlite3SelectDelete(), which is what enables them to persist
** until the end of code generation.
*/
struct CteUse {
  int nUse;              /* Number of users of this CTE */
  int addrM9e;           /* Start of subroutine to compute materialization */
  int regRtn;            /* Return address register for addrM9e subroutine */
  int iCur;              /* Ephemeral table holding the materialization */
  LogEst nRowEst;        /* Estimated number of rows in the table */
  u8 eM10d;              /* The MATERIALIZED flag */
};


/* Client data associated with sqlite3_set_clientdata() and
** sqlite3_get_clientdata().
*/
struct DbClientData {
  DbClientData *pNext;        /* Next in a linked list */
  void *pData;                /* The data */
  void (*xDestructor)(void*); /* Destructor.  Might be NULL */
  char zName[1];              /* Name of this client data. MUST BE LAST */
};

#ifdef SQLITE_DEBUG
/*
** An instance of the TreeView object is used for printing the content of
** data structures on sqlite3DebugPrintf() using a tree-like view.
*/
struct TreeView {
  int iLevel;             /* Which level of the tree we are on */
  u8  bLine[100];         /* Draw vertical in column i if bLine[i] is true */
};
#endif /* SQLITE_DEBUG */

/*
** This object is used in various ways, most (but not all) related to window
** functions.
**
**   (1) A single instance of this structure is attached to the
**       the Expr.y.pWin field for each window function in an expression tree.
**       This object holds the information contained in the OVER clause,
**       plus additional fields used during code generation.
**
**   (2) All window functions in a single SELECT form a linked-list
**       attached to Select.pWin.  The Window.pFunc and Window.pExpr
**       fields point back to the expression that is the window function.
**
**   (3) The terms of the WINDOW clause of a SELECT are instances of this
**       object on a linked list attached to Select.pWinDefn.
**
**   (4) For an aggregate function with a FILTER clause, an instance
**       of this object is stored in Expr.y.pWin with eFrmType set to
**       TK_FILTER. In this case the only field used is Window.pFilter.
**
** The uses (1) and (2) are really the same Window object that just happens
** to be accessible in two different ways.  Use case (3) are separate objects.
*/
struct Window {
  char *zName;            /* Name of window (may be NULL) */
  char *zBase;            /* Name of base window for chaining (may be NULL) */
  ExprList *pPartition;   /* PARTITION BY clause */
  ExprList *pOrderBy;     /* ORDER BY clause */
  u8 eFrmType;            /* TK_RANGE, TK_GROUPS, TK_ROWS, or 0 */
  u8 eStart;              /* UNBOUNDED, CURRENT, PRECEDING or FOLLOWING */
  u8 eEnd;                /* UNBOUNDED, CURRENT, PRECEDING or FOLLOWING */
  u8 bImplicitFrame;      /* True if frame was implicitly specified */
  u8 eExclude;            /* TK_NO, TK_CURRENT, TK_TIES, TK_GROUP, or 0 */
  Expr *pStart;           /* Expression for "<expr> PRECEDING" */
  Expr *pEnd;             /* Expression for "<expr> FOLLOWING" */
  Window **ppThis;        /* Pointer to this object in Select.pWin list */
  Window *pNextWin;       /* Next window function belonging to this SELECT */
  Expr *pFilter;          /* The FILTER expression */
  FuncDef *pWFunc;        /* The function */
  int iEphCsr;            /* Partition buffer or Peer buffer */
  int regAccum;           /* Accumulator */
  int regResult;          /* Interim result */
  int csrApp;             /* Function cursor (used by min/max) */
  int regApp;             /* Function register (also used by min/max) */
  int regPart;            /* Array of registers for PARTITION BY values */
  Expr *pOwner;           /* Expression object this window is attached to */
  int nBufferCol;         /* Number of columns in buffer table */
  int iArgCol;            /* Offset of first argument for this function */
  int regOne;             /* Register containing constant value 1 */
  int regStartRowid;
  int regEndRowid;
  u8 bExprArgs;           /* Defer evaluation of window function arguments
                          ** due to the SQLITE_SUBTYPE flag */
};
