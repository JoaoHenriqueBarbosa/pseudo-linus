
SQLITE_PRIVATE Select *sqlite3MultiValues(Parse *pParse, Select *pLeft, ExprList *pRow);
SQLITE_PRIVATE void sqlite3MultiValuesEnd(Parse *pParse, Select *pVal);

#ifndef SQLITE_OMIT_WINDOWFUNC
SQLITE_PRIVATE void sqlite3WindowDelete(sqlite3*, Window*);
SQLITE_PRIVATE void sqlite3WindowUnlinkFromSelect(Window*);
SQLITE_PRIVATE void sqlite3WindowListDelete(sqlite3 *db, Window *p);
SQLITE_PRIVATE Window *sqlite3WindowAlloc(Parse*, int, int, Expr*, int , Expr*, u8);
SQLITE_PRIVATE void sqlite3WindowAttach(Parse*, Expr*, Window*);
SQLITE_PRIVATE void sqlite3WindowLink(Select *pSel, Window *pWin);
SQLITE_PRIVATE int sqlite3WindowCompare(const Parse*, const Window*, const Window*, int);
SQLITE_PRIVATE void sqlite3WindowCodeInit(Parse*, Select*);
SQLITE_PRIVATE void sqlite3WindowCodeStep(Parse*, Select*, WhereInfo*, int, int);
SQLITE_PRIVATE int sqlite3WindowRewrite(Parse*, Select*);
SQLITE_PRIVATE void sqlite3WindowUpdate(Parse*, Window*, Window*, FuncDef*);
SQLITE_PRIVATE Window *sqlite3WindowDup(sqlite3 *db, Expr *pOwner, Window *p);
SQLITE_PRIVATE Window *sqlite3WindowListDup(sqlite3 *db, Window *p);
SQLITE_PRIVATE void sqlite3WindowFunctions(void);
SQLITE_PRIVATE void sqlite3WindowChain(Parse*, Window*, Window*);
SQLITE_PRIVATE Window *sqlite3WindowAssemble(Parse*, Window*, ExprList*, ExprList*, Token*);
#else
# define sqlite3WindowDelete(a,b)
# define sqlite3WindowFunctions()
# define sqlite3WindowAttach(a,b,c)
#endif

/*
** Assuming zIn points to the first byte of a UTF-8 character,
** advance zIn to point to the first byte of the next UTF-8 character.
*/
#define SQLITE_SKIP_UTF8(zIn) {                        \
  if( (*(zIn++))>=0xc0 ){                              \
    while( (*zIn & 0xc0)==0x80 ){ zIn++; }             \
  }                                                    \
}

/*
** The SQLITE_*_BKPT macros are substitutes for the error codes with
** the same name but without the _BKPT suffix.  These macros invoke
** routines that report the line-number on which the error originated
** using sqlite3_log().  The routines also provide a convenient place
** to set a debugger breakpoint.
*/
SQLITE_PRIVATE int sqlite3ReportError(int iErr, int lineno, const char *zType);
SQLITE_PRIVATE int sqlite3CorruptError(int);
SQLITE_PRIVATE int sqlite3MisuseError(int);
SQLITE_PRIVATE int sqlite3CantopenError(int);
#define SQLITE_CORRUPT_BKPT sqlite3CorruptError(__LINE__)
#define SQLITE_MISUSE_BKPT sqlite3MisuseError(__LINE__)
#define SQLITE_CANTOPEN_BKPT sqlite3CantopenError(__LINE__)
#ifdef SQLITE_DEBUG
SQLITE_PRIVATE   int sqlite3NomemError(int);
SQLITE_PRIVATE   int sqlite3IoerrnomemError(int);
# define SQLITE_NOMEM_BKPT sqlite3NomemError(__LINE__)
# define SQLITE_IOERR_NOMEM_BKPT sqlite3IoerrnomemError(__LINE__)
#else
# define SQLITE_NOMEM_BKPT SQLITE_NOMEM
# define SQLITE_IOERR_NOMEM_BKPT SQLITE_IOERR_NOMEM
#endif
#if defined(SQLITE_DEBUG) || defined(SQLITE_ENABLE_CORRUPT_PGNO)
SQLITE_PRIVATE   int sqlite3CorruptPgnoError(int,Pgno);
# define SQLITE_CORRUPT_PGNO(P) sqlite3CorruptPgnoError(__LINE__,(P))
#else
# define SQLITE_CORRUPT_PGNO(P) sqlite3CorruptError(__LINE__)
#endif

/*
** FTS3 and FTS4 both require virtual table support
*/
#if defined(SQLITE_OMIT_VIRTUALTABLE)
# undef SQLITE_ENABLE_FTS3
# undef SQLITE_ENABLE_FTS4
#endif

/*
** FTS4 is really an extension for FTS3.  It is enabled using the
** SQLITE_ENABLE_FTS3 macro.  But to avoid confusion we also call
** the SQLITE_ENABLE_FTS4 macro to serve as an alias for SQLITE_ENABLE_FTS3.
*/
#if defined(SQLITE_ENABLE_FTS4) && !defined(SQLITE_ENABLE_FTS3)
# define SQLITE_ENABLE_FTS3 1
#endif

/*
** The ctype.h header is needed for non-ASCII systems.  It is also
** needed by FTS3 when FTS3 is included in the amalgamation.
*/
#if !defined(SQLITE_ASCII) || \
    (defined(SQLITE_ENABLE_FTS3) && defined(SQLITE_AMALGAMATION))
# include <ctype.h>
#endif

/*
** The following macros mimic the standard library functions toupper(),
** isspace(), isalnum(), isdigit() and isxdigit(), respectively. The
** sqlite versions only work for ASCII characters, regardless of locale.
*/
#ifdef SQLITE_ASCII
# define sqlite3Toupper(x)  ((x)&~(sqlite3CtypeMap[(unsigned char)(x)]&0x20))
# define sqlite3Isspace(x)   (sqlite3CtypeMap[(unsigned char)(x)]&0x01)
# define sqlite3Isalnum(x)   (sqlite3CtypeMap[(unsigned char)(x)]&0x06)
# define sqlite3Isalpha(x)   (sqlite3CtypeMap[(unsigned char)(x)]&0x02)
# define sqlite3Isdigit(x)   (sqlite3CtypeMap[(unsigned char)(x)]&0x04)
# define sqlite3Isxdigit(x)  (sqlite3CtypeMap[(unsigned char)(x)]&0x08)
# define sqlite3Tolower(x)   (sqlite3UpperToLower[(unsigned char)(x)])
# define sqlite3Isquote(x)   (sqlite3CtypeMap[(unsigned char)(x)]&0x80)
# define sqlite3JsonId1(x)   (sqlite3CtypeMap[(unsigned char)(x)]&0x42)
# define sqlite3JsonId2(x)   (sqlite3CtypeMap[(unsigned char)(x)]&0x46)
#else
# define sqlite3Toupper(x)   toupper((unsigned char)(x))
# define sqlite3Isspace(x)   isspace((unsigned char)(x))
# define sqlite3Isalnum(x)   isalnum((unsigned char)(x))
# define sqlite3Isalpha(x)   isalpha((unsigned char)(x))
# define sqlite3Isdigit(x)   isdigit((unsigned char)(x))
# define sqlite3Isxdigit(x)  isxdigit((unsigned char)(x))
# define sqlite3Tolower(x)   tolower((unsigned char)(x))
# define sqlite3Isquote(x)   ((x)=='"'||(x)=='\''||(x)=='['||(x)=='`')
# define sqlite3JsonId1(x)   (sqlite3IsIdChar(x)&&(x)<'0')
# define sqlite3JsonId2(x)   sqlite3IsIdChar(x)
#endif
SQLITE_PRIVATE int sqlite3IsIdChar(u8);

/*
** Internal function prototypes
*/
SQLITE_PRIVATE int sqlite3StrICmp(const char*,const char*);
SQLITE_PRIVATE int sqlite3Strlen30(const char*);
#define sqlite3Strlen30NN(C) (strlen(C)&0x3fffffff)
SQLITE_PRIVATE char *sqlite3ColumnType(Column*,char*);
#define sqlite3StrNICmp sqlite3_strnicmp

SQLITE_PRIVATE int sqlite3MallocInit(void);
SQLITE_PRIVATE void sqlite3MallocEnd(void);
SQLITE_PRIVATE void *sqlite3Malloc(u64);
SQLITE_PRIVATE void *sqlite3MallocZero(u64);
SQLITE_PRIVATE void *sqlite3DbMallocZero(sqlite3*, u64);
SQLITE_PRIVATE void *sqlite3DbMallocRaw(sqlite3*, u64);
SQLITE_PRIVATE void *sqlite3DbMallocRawNN(sqlite3*, u64);
SQLITE_PRIVATE char *sqlite3DbStrDup(sqlite3*,const char*);
SQLITE_PRIVATE char *sqlite3DbStrNDup(sqlite3*,const char*, u64);
SQLITE_PRIVATE char *sqlite3DbSpanDup(sqlite3*,const char*,const char*);
SQLITE_PRIVATE void *sqlite3Realloc(void*, u64);
SQLITE_PRIVATE void *sqlite3DbReallocOrFree(sqlite3 *, void *, u64);
SQLITE_PRIVATE void *sqlite3DbRealloc(sqlite3 *, void *, u64);
SQLITE_PRIVATE void sqlite3DbFree(sqlite3*, void*);
SQLITE_PRIVATE void sqlite3DbFreeNN(sqlite3*, void*);
SQLITE_PRIVATE void sqlite3DbNNFreeNN(sqlite3*, void*);
SQLITE_PRIVATE int sqlite3MallocSize(const void*);
SQLITE_PRIVATE int sqlite3DbMallocSize(sqlite3*, const void*);
SQLITE_PRIVATE void *sqlite3PageMalloc(int);
SQLITE_PRIVATE void sqlite3PageFree(void*);
SQLITE_PRIVATE void sqlite3MemSetDefault(void);
#ifndef SQLITE_UNTESTABLE
SQLITE_PRIVATE void sqlite3BenignMallocHooks(void (*)(void), void (*)(void));
#endif
SQLITE_PRIVATE int sqlite3HeapNearlyFull(void);

/*
** On systems with ample stack space and that support alloca(), make
** use of alloca() to obtain space for large automatic objects.  By default,
** obtain space from malloc().
**
** The alloca() routine never returns NULL.  This will cause code paths
** that deal with sqlite3StackAlloc() failures to be unreachable.
*/
#ifdef SQLITE_USE_ALLOCA
# define sqlite3StackAllocRaw(D,N)   alloca(N)
# define sqlite3StackAllocRawNN(D,N) alloca(N)
# define sqlite3StackFree(D,P)
# define sqlite3StackFreeNN(D,P)
#else
# define sqlite3StackAllocRaw(D,N)   sqlite3DbMallocRaw(D,N)
# define sqlite3StackAllocRawNN(D,N) sqlite3DbMallocRawNN(D,N)
# define sqlite3StackFree(D,P)       sqlite3DbFree(D,P)
# define sqlite3StackFreeNN(D,P)     sqlite3DbFreeNN(D,P)
#endif

/* Do not allow both MEMSYS5 and MEMSYS3 to be defined together.  If they
** are, disable MEMSYS3
*/
#ifdef SQLITE_ENABLE_MEMSYS5
SQLITE_PRIVATE const sqlite3_mem_methods *sqlite3MemGetMemsys5(void);
#undef SQLITE_ENABLE_MEMSYS3
#endif
#ifdef SQLITE_ENABLE_MEMSYS3
SQLITE_PRIVATE const sqlite3_mem_methods *sqlite3MemGetMemsys3(void);
#endif


#ifndef SQLITE_MUTEX_OMIT
SQLITE_PRIVATE   sqlite3_mutex_methods const *sqlite3DefaultMutex(void);
SQLITE_PRIVATE   sqlite3_mutex_methods const *sqlite3NoopMutex(void);
SQLITE_PRIVATE   sqlite3_mutex *sqlite3MutexAlloc(int);
SQLITE_PRIVATE   int sqlite3MutexInit(void);
SQLITE_PRIVATE   int sqlite3MutexEnd(void);
#endif
#if !defined(SQLITE_MUTEX_OMIT) && !defined(SQLITE_MUTEX_NOOP)
SQLITE_PRIVATE   void sqlite3MemoryBarrier(void);
#else
# define sqlite3MemoryBarrier()
#endif

SQLITE_PRIVATE sqlite3_int64 sqlite3StatusValue(int);
SQLITE_PRIVATE void sqlite3StatusUp(int, int);
SQLITE_PRIVATE void sqlite3StatusDown(int, int);
SQLITE_PRIVATE void sqlite3StatusHighwater(int, int);
SQLITE_PRIVATE int sqlite3LookasideUsed(sqlite3*,int*);

/* Access to mutexes used by sqlite3_status() */
SQLITE_PRIVATE sqlite3_mutex *sqlite3Pcache1Mutex(void);
SQLITE_PRIVATE sqlite3_mutex *sqlite3MallocMutex(void);

#if defined(SQLITE_ENABLE_MULTITHREADED_CHECKS) && !defined(SQLITE_MUTEX_OMIT)
SQLITE_PRIVATE void sqlite3MutexWarnOnContention(sqlite3_mutex*);
#else
# define sqlite3MutexWarnOnContention(x)
#endif

#ifndef SQLITE_OMIT_FLOATING_POINT
# define EXP754 (((u64)0x7ff)<<52)
# define MAN754 ((((u64)1)<<52)-1)
# define IsNaN(X) (((X)&EXP754)==EXP754 && ((X)&MAN754)!=0)
# define IsOvfl(X) (((X)&EXP754)==EXP754)
SQLITE_PRIVATE   int sqlite3IsNaN(double);
SQLITE_PRIVATE   int sqlite3IsOverflow(double);
#else
# define IsNaN(X)             0
# define sqlite3IsNaN(X)      0
# define sqlite3IsOVerflow(X) 0
#endif

/*
** An instance of the following structure holds information about SQL
** functions arguments that are the parameters to the printf() function.
*/
struct PrintfArguments {
  int nArg;                /* Total number of arguments */
  int nUsed;               /* Number of arguments used so far */
  sqlite3_value **apArg;   /* The argument values */
};

/*
** An instance of this object receives the decoding of a floating point
** value into an approximate decimal representation.
*/
struct FpDecode {
  char sign;           /* '+' or '-' */
  char isSpecial;      /* 1: Infinity  2: NaN */
  int n;               /* Significant digits in the decode */
  int iDP;             /* Location of the decimal point */
  char *z;             /* Start of significant digits */
  char zBuf[24];       /* Storage for significant digits */
};

SQLITE_PRIVATE void sqlite3FpDecode(FpDecode*,double,int,int);
SQLITE_PRIVATE char *sqlite3MPrintf(sqlite3*,const char*, ...);
SQLITE_PRIVATE char *sqlite3VMPrintf(sqlite3*,const char*, va_list);
#if defined(SQLITE_DEBUG) || defined(SQLITE_HAVE_OS_TRACE)
SQLITE_PRIVATE   void sqlite3DebugPrintf(const char*, ...);
#endif
#if defined(SQLITE_TEST)
SQLITE_PRIVATE   void *sqlite3TestTextToPtr(const char*);
#endif

#if defined(SQLITE_DEBUG)
SQLITE_PRIVATE   void sqlite3TreeViewLine(TreeView*, const char *zFormat, ...);
SQLITE_PRIVATE   void sqlite3TreeViewExpr(TreeView*, const Expr*, u8);
SQLITE_PRIVATE   void sqlite3TreeViewBareExprList(TreeView*, const ExprList*, const char*);
SQLITE_PRIVATE   void sqlite3TreeViewExprList(TreeView*, const ExprList*, u8, const char*);
SQLITE_PRIVATE   void sqlite3TreeViewBareIdList(TreeView*, const IdList*, const char*);
SQLITE_PRIVATE   void sqlite3TreeViewIdList(TreeView*, const IdList*, u8, const char*);
SQLITE_PRIVATE   void sqlite3TreeViewColumnList(TreeView*, const Column*, int, u8);
SQLITE_PRIVATE   void sqlite3TreeViewSrcList(TreeView*, const SrcList*);
SQLITE_PRIVATE   void sqlite3TreeViewSelect(TreeView*, const Select*, u8);
SQLITE_PRIVATE   void sqlite3TreeViewWith(TreeView*, const With*, u8);
SQLITE_PRIVATE   void sqlite3TreeViewUpsert(TreeView*, const Upsert*, u8);
#if TREETRACE_ENABLED
SQLITE_PRIVATE   void sqlite3TreeViewDelete(const With*, const SrcList*, const Expr*,
                             const ExprList*,const Expr*, const Trigger*);
SQLITE_PRIVATE   void sqlite3TreeViewInsert(const With*, const SrcList*,
                             const IdList*, const Select*, const ExprList*,
                             int, const Upsert*, const Trigger*);
SQLITE_PRIVATE   void sqlite3TreeViewUpdate(const With*, const SrcList*, const ExprList*,
                             const Expr*, int, const ExprList*, const Expr*,
                             const Upsert*, const Trigger*);
#endif
#ifndef SQLITE_OMIT_TRIGGER
SQLITE_PRIVATE   void sqlite3TreeViewTriggerStep(TreeView*, const TriggerStep*, u8, u8);
SQLITE_PRIVATE   void sqlite3TreeViewTrigger(TreeView*, const Trigger*, u8, u8);
#endif
#ifndef SQLITE_OMIT_WINDOWFUNC
SQLITE_PRIVATE   void sqlite3TreeViewWindow(TreeView*, const Window*, u8);
SQLITE_PRIVATE   void sqlite3TreeViewWinFunc(TreeView*, const Window*, u8);
#endif
SQLITE_PRIVATE   void sqlite3ShowExpr(const Expr*);
SQLITE_PRIVATE   void sqlite3ShowExprList(const ExprList*);
SQLITE_PRIVATE   void sqlite3ShowIdList(const IdList*);
SQLITE_PRIVATE   void sqlite3ShowSrcList(const SrcList*);
SQLITE_PRIVATE   void sqlite3ShowSelect(const Select*);
SQLITE_PRIVATE   void sqlite3ShowWith(const With*);
SQLITE_PRIVATE   void sqlite3ShowUpsert(const Upsert*);
#ifndef SQLITE_OMIT_TRIGGER
SQLITE_PRIVATE   void sqlite3ShowTriggerStep(const TriggerStep*);
SQLITE_PRIVATE   void sqlite3ShowTriggerStepList(const TriggerStep*);
SQLITE_PRIVATE   void sqlite3ShowTrigger(const Trigger*);
SQLITE_PRIVATE   void sqlite3ShowTriggerList(const Trigger*);
#endif
#ifndef SQLITE_OMIT_WINDOWFUNC
SQLITE_PRIVATE   void sqlite3ShowWindow(const Window*);
SQLITE_PRIVATE   void sqlite3ShowWinFunc(const Window*);
#endif
#endif

SQLITE_PRIVATE void sqlite3SetString(char **, sqlite3*, const char*);
SQLITE_PRIVATE void sqlite3ProgressCheck(Parse*);
SQLITE_PRIVATE void sqlite3ErrorMsg(Parse*, const char*, ...);
SQLITE_PRIVATE int sqlite3ErrorToParser(sqlite3*,int);
SQLITE_PRIVATE void sqlite3Dequote(char*);
SQLITE_PRIVATE void sqlite3DequoteExpr(Expr*);
SQLITE_PRIVATE void sqlite3DequoteToken(Token*);
SQLITE_PRIVATE void sqlite3DequoteNumber(Parse*, Expr*);
SQLITE_PRIVATE void sqlite3TokenInit(Token*,char*);
SQLITE_PRIVATE int sqlite3KeywordCode(const unsigned char*, int);
SQLITE_PRIVATE int sqlite3RunParser(Parse*, const char*);
SQLITE_PRIVATE void sqlite3FinishCoding(Parse*);
SQLITE_PRIVATE int sqlite3GetTempReg(Parse*);
SQLITE_PRIVATE void sqlite3ReleaseTempReg(Parse*,int);
SQLITE_PRIVATE int sqlite3GetTempRange(Parse*,int);
SQLITE_PRIVATE void sqlite3ReleaseTempRange(Parse*,int,int);
SQLITE_PRIVATE void sqlite3ClearTempRegCache(Parse*);
SQLITE_PRIVATE void sqlite3TouchRegister(Parse*,int);
#if defined(SQLITE_ENABLE_STAT4) || defined(SQLITE_DEBUG)
SQLITE_PRIVATE int sqlite3FirstAvailableRegister(Parse*,int);
#endif
#ifdef SQLITE_DEBUG
SQLITE_PRIVATE int sqlite3NoTempsInRange(Parse*,int,int);
#endif
SQLITE_PRIVATE Expr *sqlite3ExprAlloc(sqlite3*,int,const Token*,int);
SQLITE_PRIVATE Expr *sqlite3Expr(sqlite3*,int,const char*);
SQLITE_PRIVATE void sqlite3ExprAttachSubtrees(sqlite3*,Expr*,Expr*,Expr*);
SQLITE_PRIVATE Expr *sqlite3PExpr(Parse*, int, Expr*, Expr*);
SQLITE_PRIVATE void sqlite3PExprAddSelect(Parse*, Expr*, Select*);
SQLITE_PRIVATE Expr *sqlite3ExprAnd(Parse*,Expr*, Expr*);
SQLITE_PRIVATE Expr *sqlite3ExprSimplifiedAndOr(Expr*);
SQLITE_PRIVATE Expr *sqlite3ExprFunction(Parse*,ExprList*, const Token*, int);
SQLITE_PRIVATE void sqlite3ExprAddFunctionOrderBy(Parse*,Expr*,ExprList*);
SQLITE_PRIVATE void sqlite3ExprOrderByAggregateError(Parse*,Expr*);
SQLITE_PRIVATE void sqlite3ExprFunctionUsable(Parse*,const Expr*,const FuncDef*);
SQLITE_PRIVATE void sqlite3ExprAssignVarNumber(Parse*, Expr*, u32);
