SQLITE_PRIVATE CollSeq *sqlite3ExprCompareCollSeq(Parse*,const Expr*);
SQLITE_PRIVATE CollSeq *sqlite3BinaryCompareCollSeq(Parse *, const Expr*, const Expr*);
SQLITE_PRIVATE int sqlite3TempInMemory(const sqlite3*);
SQLITE_PRIVATE const char *sqlite3JournalModename(int);
#ifndef SQLITE_OMIT_WAL
SQLITE_PRIVATE   int sqlite3Checkpoint(sqlite3*, int, int, int*, int*);
SQLITE_PRIVATE   int sqlite3WalDefaultHook(void*,sqlite3*,const char*,int);
#endif
#ifndef SQLITE_OMIT_CTE
SQLITE_PRIVATE   Cte *sqlite3CteNew(Parse*,Token*,ExprList*,Select*,u8);
SQLITE_PRIVATE   void sqlite3CteDelete(sqlite3*,Cte*);
SQLITE_PRIVATE   With *sqlite3WithAdd(Parse*,With*,Cte*);
SQLITE_PRIVATE   void sqlite3WithDelete(sqlite3*,With*);
SQLITE_PRIVATE   void sqlite3WithDeleteGeneric(sqlite3*,void*);
SQLITE_PRIVATE   With *sqlite3WithPush(Parse*, With*, u8);
#else
# define sqlite3CteNew(P,T,E,S)   ((void*)0)
# define sqlite3CteDelete(D,C)
# define sqlite3CteWithAdd(P,W,C) ((void*)0)
# define sqlite3WithDelete(x,y)
# define sqlite3WithPush(x,y,z) ((void*)0)
#endif
#ifndef SQLITE_OMIT_UPSERT
SQLITE_PRIVATE   Upsert *sqlite3UpsertNew(sqlite3*,ExprList*,Expr*,ExprList*,Expr*,Upsert*);
SQLITE_PRIVATE   void sqlite3UpsertDelete(sqlite3*,Upsert*);
SQLITE_PRIVATE   Upsert *sqlite3UpsertDup(sqlite3*,Upsert*);
SQLITE_PRIVATE   int sqlite3UpsertAnalyzeTarget(Parse*,SrcList*,Upsert*,Upsert*);
SQLITE_PRIVATE   void sqlite3UpsertDoUpdate(Parse*,Upsert*,Table*,Index*,int);
SQLITE_PRIVATE   Upsert *sqlite3UpsertOfIndex(Upsert*,Index*);
SQLITE_PRIVATE   int sqlite3UpsertNextIsIPK(Upsert*);
#else
#define sqlite3UpsertNew(u,v,w,x,y,z) ((Upsert*)0)
#define sqlite3UpsertDelete(x,y)
#define sqlite3UpsertDup(x,y)         ((Upsert*)0)
#define sqlite3UpsertOfIndex(x,y)     ((Upsert*)0)
#define sqlite3UpsertNextIsIPK(x)     0
#endif


/* Declarations for functions in fkey.c. All of these are replaced by
** no-op macros if OMIT_FOREIGN_KEY is defined. In this case no foreign
** key functionality is available. If OMIT_TRIGGER is defined but
** OMIT_FOREIGN_KEY is not, only some of the functions are no-oped. In
** this case foreign keys are parsed, but no other functionality is
** provided (enforcement of FK constraints requires the triggers sub-system).
*/
#if !defined(SQLITE_OMIT_FOREIGN_KEY) && !defined(SQLITE_OMIT_TRIGGER)
SQLITE_PRIVATE   void sqlite3FkCheck(Parse*, Table*, int, int, int*, int);
SQLITE_PRIVATE   void sqlite3FkDropTable(Parse*, SrcList *, Table*);
SQLITE_PRIVATE   void sqlite3FkActions(Parse*, Table*, ExprList*, int, int*, int);
SQLITE_PRIVATE   int sqlite3FkRequired(Parse*, Table*, int*, int);
SQLITE_PRIVATE   u32 sqlite3FkOldmask(Parse*, Table*);
SQLITE_PRIVATE   FKey *sqlite3FkReferences(Table *);
SQLITE_PRIVATE   void sqlite3FkClearTriggerCache(sqlite3*,int);
#else
  #define sqlite3FkActions(a,b,c,d,e,f)
  #define sqlite3FkCheck(a,b,c,d,e,f)
  #define sqlite3FkDropTable(a,b,c)
  #define sqlite3FkOldmask(a,b)         0
  #define sqlite3FkRequired(a,b,c,d)    0
  #define sqlite3FkReferences(a)        0
  #define sqlite3FkClearTriggerCache(a,b)
#endif
#ifndef SQLITE_OMIT_FOREIGN_KEY
SQLITE_PRIVATE   void sqlite3FkDelete(sqlite3 *, Table*);
SQLITE_PRIVATE   int sqlite3FkLocateIndex(Parse*,Table*,FKey*,Index**,int**);
#else
  #define sqlite3FkDelete(a,b)
  #define sqlite3FkLocateIndex(a,b,c,d,e)
#endif


/*
** Available fault injectors.  Should be numbered beginning with 0.
*/
#define SQLITE_FAULTINJECTOR_MALLOC     0
#define SQLITE_FAULTINJECTOR_COUNT      1

/*
** The interface to the code in fault.c used for identifying "benign"
** malloc failures. This is only present if SQLITE_UNTESTABLE
** is not defined.
*/
#ifndef SQLITE_UNTESTABLE
SQLITE_PRIVATE   void sqlite3BeginBenignMalloc(void);
SQLITE_PRIVATE   void sqlite3EndBenignMalloc(void);
#else
  #define sqlite3BeginBenignMalloc()
  #define sqlite3EndBenignMalloc()
#endif

/*
** Allowed return values from sqlite3FindInIndex()
*/
#define IN_INDEX_ROWID        1   /* Search the rowid of the table */
#define IN_INDEX_EPH          2   /* Search an ephemeral b-tree */
#define IN_INDEX_INDEX_ASC    3   /* Existing index ASCENDING */
#define IN_INDEX_INDEX_DESC   4   /* Existing index DESCENDING */
#define IN_INDEX_NOOP         5   /* No table available. Use comparisons */
/*
** Allowed flags for the 3rd parameter to sqlite3FindInIndex().
*/
#define IN_INDEX_NOOP_OK     0x0001  /* OK to return IN_INDEX_NOOP */
#define IN_INDEX_MEMBERSHIP  0x0002  /* IN operator used for membership test */
#define IN_INDEX_LOOP        0x0004  /* IN operator used as a loop */
SQLITE_PRIVATE int sqlite3FindInIndex(Parse *, Expr *, u32, int*, int*, int*);

SQLITE_PRIVATE int sqlite3JournalOpen(sqlite3_vfs *, const char *, sqlite3_file *, int, int);
SQLITE_PRIVATE int sqlite3JournalSize(sqlite3_vfs *);
#if defined(SQLITE_ENABLE_ATOMIC_WRITE) \
 || defined(SQLITE_ENABLE_BATCH_ATOMIC_WRITE)
SQLITE_PRIVATE   int sqlite3JournalCreate(sqlite3_file *);
#endif

SQLITE_PRIVATE int sqlite3JournalIsInMemory(sqlite3_file *p);
SQLITE_PRIVATE void sqlite3MemJournalOpen(sqlite3_file *);

SQLITE_PRIVATE void sqlite3ExprSetHeightAndFlags(Parse *pParse, Expr *p);
#if SQLITE_MAX_EXPR_DEPTH>0
SQLITE_PRIVATE   int sqlite3SelectExprHeight(const Select *);
SQLITE_PRIVATE   int sqlite3ExprCheckHeight(Parse*, int);
#else
  #define sqlite3SelectExprHeight(x) 0
  #define sqlite3ExprCheckHeight(x,y)
#endif
SQLITE_PRIVATE void sqlite3ExprSetErrorOffset(Expr*,int);

SQLITE_PRIVATE u32 sqlite3Get4byte(const u8*);
SQLITE_PRIVATE void sqlite3Put4byte(u8*, u32);

#ifdef SQLITE_ENABLE_UNLOCK_NOTIFY
SQLITE_PRIVATE   void sqlite3ConnectionBlocked(sqlite3 *, sqlite3 *);
SQLITE_PRIVATE   void sqlite3ConnectionUnlocked(sqlite3 *db);
SQLITE_PRIVATE   void sqlite3ConnectionClosed(sqlite3 *db);
#else
  #define sqlite3ConnectionBlocked(x,y)
  #define sqlite3ConnectionUnlocked(x)
  #define sqlite3ConnectionClosed(x)
#endif

#ifdef SQLITE_DEBUG
SQLITE_PRIVATE   void sqlite3ParserTrace(FILE*, char *);
#endif
#if defined(YYCOVERAGE)
SQLITE_PRIVATE   int sqlite3ParserCoverage(FILE*);
#endif

/*
** If the SQLITE_ENABLE IOTRACE exists then the global variable
** sqlite3IoTrace is a pointer to a printf-like routine used to
** print I/O tracing messages.
*/
#ifdef SQLITE_ENABLE_IOTRACE
# define IOTRACE(A)  if( sqlite3IoTrace ){ sqlite3IoTrace A; }
SQLITE_PRIVATE   void sqlite3VdbeIOTraceSql(Vdbe*);
SQLITE_API SQLITE_EXTERN void (SQLITE_CDECL *sqlite3IoTrace)(const char*,...);
#else
# define IOTRACE(A)
# define sqlite3VdbeIOTraceSql(X)
#endif

/*
** These routines are available for the mem2.c debugging memory allocator
** only.  They are used to verify that different "types" of memory
** allocations are properly tracked by the system.
**
** sqlite3MemdebugSetType() sets the "type" of an allocation to one of
** the MEMTYPE_* macros defined below.  The type must be a bitmask with
** a single bit set.
**
** sqlite3MemdebugHasType() returns true if any of the bits in its second
** argument match the type set by the previous sqlite3MemdebugSetType().
** sqlite3MemdebugHasType() is intended for use inside assert() statements.
**
** sqlite3MemdebugNoType() returns true if none of the bits in its second
** argument match the type set by the previous sqlite3MemdebugSetType().
**
** Perhaps the most important point is the difference between MEMTYPE_HEAP
** and MEMTYPE_LOOKASIDE.  If an allocation is MEMTYPE_LOOKASIDE, that means
** it might have been allocated by lookaside, except the allocation was
** too large or lookaside was already full.  It is important to verify
** that allocations that might have been satisfied by lookaside are not
** passed back to non-lookaside free() routines.  Asserts such as the
** example above are placed on the non-lookaside free() routines to verify
** this constraint.
**
** All of this is no-op for a production build.  It only comes into
** play when the SQLITE_MEMDEBUG compile-time option is used.
*/
#ifdef SQLITE_MEMDEBUG
SQLITE_PRIVATE   void sqlite3MemdebugSetType(void*,u8);
SQLITE_PRIVATE   int sqlite3MemdebugHasType(const void*,u8);
SQLITE_PRIVATE   int sqlite3MemdebugNoType(const void*,u8);
#else
# define sqlite3MemdebugSetType(X,Y)  /* no-op */
# define sqlite3MemdebugHasType(X,Y)  1
# define sqlite3MemdebugNoType(X,Y)   1
#endif
#define MEMTYPE_HEAP       0x01  /* General heap allocations */
#define MEMTYPE_LOOKASIDE  0x02  /* Heap that might have been lookaside */
#define MEMTYPE_PCACHE     0x04  /* Page cache allocations */

/*
** Threading interface
*/
#if SQLITE_MAX_WORKER_THREADS>0
SQLITE_PRIVATE int sqlite3ThreadCreate(SQLiteThread**,void*(*)(void*),void*);
SQLITE_PRIVATE int sqlite3ThreadJoin(SQLiteThread*, void**);
#endif

#if defined(SQLITE_ENABLE_DBPAGE_VTAB) || defined(SQLITE_TEST)
SQLITE_PRIVATE int sqlite3DbpageRegister(sqlite3*);
#endif
#if defined(SQLITE_ENABLE_DBSTAT_VTAB) || defined(SQLITE_TEST)
SQLITE_PRIVATE int sqlite3DbstatRegister(sqlite3*);
#endif

SQLITE_PRIVATE int sqlite3ExprVectorSize(const Expr *pExpr);
SQLITE_PRIVATE int sqlite3ExprIsVector(const Expr *pExpr);
SQLITE_PRIVATE Expr *sqlite3VectorFieldSubexpr(Expr*, int);
SQLITE_PRIVATE Expr *sqlite3ExprForVectorField(Parse*,Expr*,int,int);
SQLITE_PRIVATE void sqlite3VectorErrorMsg(Parse*, Expr*);

#ifndef SQLITE_OMIT_COMPILEOPTION_DIAGS
SQLITE_PRIVATE const char **sqlite3CompileOptions(int *pnOpt);
#endif

#if SQLITE_OS_UNIX && defined(SQLITE_OS_KV_OPTIONAL)
SQLITE_PRIVATE int sqlite3KvvfsInit(void);
#endif

#if defined(VDBE_PROFILE) \
 || defined(SQLITE_PERFORMANCE_TRACE) \
 || defined(SQLITE_ENABLE_STMT_SCANSTATUS)
SQLITE_PRIVATE sqlite3_uint64 sqlite3Hwtime(void);
#endif

#ifdef SQLITE_ENABLE_STMT_SCANSTATUS
# define IS_STMT_SCANSTATUS(db) (db->flags & SQLITE_StmtScanStatus)
#else
# define IS_STMT_SCANSTATUS(db) 0
#endif

#endif /* SQLITEINT_H */

