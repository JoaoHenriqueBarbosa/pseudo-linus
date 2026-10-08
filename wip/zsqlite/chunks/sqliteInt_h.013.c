SQLITE_PRIVATE int sqlite3FixTriggerStep(DbFixer*, TriggerStep*);

SQLITE_PRIVATE int sqlite3RealSameAsInt(double,sqlite3_int64);
SQLITE_PRIVATE i64 sqlite3RealToI64(double);
SQLITE_PRIVATE int sqlite3Int64ToText(i64,char*);
SQLITE_PRIVATE int sqlite3AtoF(const char *z, double*, int, u8);
SQLITE_PRIVATE int sqlite3GetInt32(const char *, int*);
SQLITE_PRIVATE int sqlite3GetUInt32(const char*, u32*);
SQLITE_PRIVATE int sqlite3Atoi(const char*);
#ifndef SQLITE_OMIT_UTF16
SQLITE_PRIVATE int sqlite3Utf16ByteLen(const void *pData, int nChar);
#endif
SQLITE_PRIVATE int sqlite3Utf8CharLen(const char *pData, int nByte);
SQLITE_PRIVATE u32 sqlite3Utf8Read(const u8**);
SQLITE_PRIVATE int sqlite3Utf8ReadLimited(const u8*, int, u32*);
SQLITE_PRIVATE LogEst sqlite3LogEst(u64);
SQLITE_PRIVATE LogEst sqlite3LogEstAdd(LogEst,LogEst);
SQLITE_PRIVATE LogEst sqlite3LogEstFromDouble(double);
SQLITE_PRIVATE u64 sqlite3LogEstToInt(LogEst);
SQLITE_PRIVATE VList *sqlite3VListAdd(sqlite3*,VList*,const char*,int,int);
SQLITE_PRIVATE const char *sqlite3VListNumToName(VList*,int);
SQLITE_PRIVATE int sqlite3VListNameToNum(VList*,const char*,int);

/*
** Routines to read and write variable-length integers.  These used to
** be defined locally, but now we use the varint routines in the util.c
** file.
*/
SQLITE_PRIVATE int sqlite3PutVarint(unsigned char*, u64);
SQLITE_PRIVATE u8 sqlite3GetVarint(const unsigned char *, u64 *);
SQLITE_PRIVATE u8 sqlite3GetVarint32(const unsigned char *, u32 *);
SQLITE_PRIVATE int sqlite3VarintLen(u64 v);

/*
** The common case is for a varint to be a single byte.  They following
** macros handle the common case without a procedure call, but then call
** the procedure for larger varints.
*/
#define getVarint32(A,B)  \
  (u8)((*(A)<(u8)0x80)?((B)=(u32)*(A)),1:sqlite3GetVarint32((A),(u32 *)&(B)))
#define getVarint32NR(A,B) \
  B=(u32)*(A);if(B>=0x80)sqlite3GetVarint32((A),(u32*)&(B))
#define putVarint32(A,B)  \
  (u8)(((u32)(B)<(u32)0x80)?(*(A)=(unsigned char)(B)),1:\
  sqlite3PutVarint((A),(B)))
#define getVarint    sqlite3GetVarint
#define putVarint    sqlite3PutVarint


SQLITE_PRIVATE const char *sqlite3IndexAffinityStr(sqlite3*, Index*);
SQLITE_PRIVATE char *sqlite3TableAffinityStr(sqlite3*,const Table*);
SQLITE_PRIVATE void sqlite3TableAffinity(Vdbe*, Table*, int);
SQLITE_PRIVATE char sqlite3CompareAffinity(const Expr *pExpr, char aff2);
SQLITE_PRIVATE int sqlite3IndexAffinityOk(const Expr *pExpr, char idx_affinity);
SQLITE_PRIVATE char sqlite3TableColumnAffinity(const Table*,int);
SQLITE_PRIVATE char sqlite3ExprAffinity(const Expr *pExpr);
SQLITE_PRIVATE int sqlite3ExprDataType(const Expr *pExpr);
SQLITE_PRIVATE int sqlite3Atoi64(const char*, i64*, int, u8);
SQLITE_PRIVATE int sqlite3DecOrHexToI64(const char*, i64*);
SQLITE_PRIVATE void sqlite3ErrorWithMsg(sqlite3*, int, const char*,...);
SQLITE_PRIVATE void sqlite3Error(sqlite3*,int);
SQLITE_PRIVATE void sqlite3ErrorClear(sqlite3*);
SQLITE_PRIVATE void sqlite3SystemError(sqlite3*,int);
#if !defined(SQLITE_OMIT_BLOB_LITERAL)
SQLITE_PRIVATE void *sqlite3HexToBlob(sqlite3*, const char *z, int n);
#endif
SQLITE_PRIVATE u8 sqlite3HexToInt(int h);
SQLITE_PRIVATE int sqlite3TwoPartName(Parse *, Token *, Token *, Token **);

#if defined(SQLITE_NEED_ERR_NAME)
SQLITE_PRIVATE const char *sqlite3ErrName(int);
#endif

#ifndef SQLITE_OMIT_DESERIALIZE
SQLITE_PRIVATE int sqlite3MemdbInit(void);
SQLITE_PRIVATE int sqlite3IsMemdb(const sqlite3_vfs*);
#else
# define sqlite3IsMemdb(X) 0
#endif

SQLITE_PRIVATE const char *sqlite3ErrStr(int);
SQLITE_PRIVATE int sqlite3ReadSchema(Parse *pParse);
SQLITE_PRIVATE CollSeq *sqlite3FindCollSeq(sqlite3*,u8 enc, const char*,int);
SQLITE_PRIVATE int sqlite3IsBinary(const CollSeq*);
SQLITE_PRIVATE CollSeq *sqlite3LocateCollSeq(Parse *pParse, const char*zName);
SQLITE_PRIVATE void sqlite3SetTextEncoding(sqlite3 *db, u8);
SQLITE_PRIVATE CollSeq *sqlite3ExprCollSeq(Parse *pParse, const Expr *pExpr);
SQLITE_PRIVATE CollSeq *sqlite3ExprNNCollSeq(Parse *pParse, const Expr *pExpr);
SQLITE_PRIVATE int sqlite3ExprCollSeqMatch(Parse*,const Expr*,const Expr*);
SQLITE_PRIVATE Expr *sqlite3ExprAddCollateToken(const Parse *pParse, Expr*, const Token*, int);
SQLITE_PRIVATE Expr *sqlite3ExprAddCollateString(const Parse*,Expr*,const char*);
SQLITE_PRIVATE Expr *sqlite3ExprSkipCollate(Expr*);
SQLITE_PRIVATE Expr *sqlite3ExprSkipCollateAndLikely(Expr*);
SQLITE_PRIVATE int sqlite3CheckCollSeq(Parse *, CollSeq *);
SQLITE_PRIVATE int sqlite3WritableSchema(sqlite3*);
SQLITE_PRIVATE int sqlite3CheckObjectName(Parse*, const char*,const char*,const char*);
SQLITE_PRIVATE void sqlite3VdbeSetChanges(sqlite3 *, i64);
SQLITE_PRIVATE int sqlite3AddInt64(i64*,i64);
SQLITE_PRIVATE int sqlite3SubInt64(i64*,i64);
SQLITE_PRIVATE int sqlite3MulInt64(i64*,i64);
SQLITE_PRIVATE int sqlite3AbsInt32(int);
#ifdef SQLITE_ENABLE_8_3_NAMES
SQLITE_PRIVATE void sqlite3FileSuffix3(const char*, char*);
#else
# define sqlite3FileSuffix3(X,Y)
#endif
SQLITE_PRIVATE u8 sqlite3GetBoolean(const char *z,u8);

SQLITE_PRIVATE const void *sqlite3ValueText(sqlite3_value*, u8);
SQLITE_PRIVATE int sqlite3ValueIsOfClass(const sqlite3_value*, void(*)(void*));
SQLITE_PRIVATE int sqlite3ValueBytes(sqlite3_value*, u8);
SQLITE_PRIVATE void sqlite3ValueSetStr(sqlite3_value*, int, const void *,u8,
                        void(*)(void*));
SQLITE_PRIVATE void sqlite3ValueSetNull(sqlite3_value*);
SQLITE_PRIVATE void sqlite3ValueFree(sqlite3_value*);
#ifndef SQLITE_UNTESTABLE
SQLITE_PRIVATE void sqlite3ResultIntReal(sqlite3_context*);
#endif
SQLITE_PRIVATE sqlite3_value *sqlite3ValueNew(sqlite3 *);
#ifndef SQLITE_OMIT_UTF16
SQLITE_PRIVATE char *sqlite3Utf16to8(sqlite3 *, const void*, int, u8);
#endif
SQLITE_PRIVATE int sqlite3ValueFromExpr(sqlite3 *, const Expr *, u8, u8, sqlite3_value **);
SQLITE_PRIVATE void sqlite3ValueApplyAffinity(sqlite3_value *, u8, u8);
#ifndef SQLITE_AMALGAMATION
SQLITE_PRIVATE const unsigned char sqlite3OpcodeProperty[];
SQLITE_PRIVATE const char sqlite3StrBINARY[];
SQLITE_PRIVATE const unsigned char sqlite3StdTypeLen[];
SQLITE_PRIVATE const char sqlite3StdTypeAffinity[];
SQLITE_PRIVATE const char *sqlite3StdType[];
SQLITE_PRIVATE const unsigned char sqlite3UpperToLower[];
SQLITE_PRIVATE const unsigned char *sqlite3aLTb;
SQLITE_PRIVATE const unsigned char *sqlite3aEQb;
SQLITE_PRIVATE const unsigned char *sqlite3aGTb;
SQLITE_PRIVATE const unsigned char sqlite3CtypeMap[];
SQLITE_PRIVATE SQLITE_WSD struct Sqlite3Config sqlite3Config;
SQLITE_PRIVATE FuncDefHash sqlite3BuiltinFunctions;
#ifndef SQLITE_OMIT_WSD
SQLITE_PRIVATE int sqlite3PendingByte;
#endif
#endif /* SQLITE_AMALGAMATION */
#ifdef VDBE_PROFILE
SQLITE_PRIVATE sqlite3_uint64 sqlite3NProfileCnt;
#endif
SQLITE_PRIVATE void sqlite3RootPageMoved(sqlite3*, int, Pgno, Pgno);
SQLITE_PRIVATE void sqlite3Reindex(Parse*, Token*, Token*);
SQLITE_PRIVATE void sqlite3AlterFunctions(void);
SQLITE_PRIVATE void sqlite3AlterRenameTable(Parse*, SrcList*, Token*);
SQLITE_PRIVATE void sqlite3AlterRenameColumn(Parse*, SrcList*, Token*, Token*);
SQLITE_PRIVATE int sqlite3GetToken(const unsigned char *, int *);
SQLITE_PRIVATE void sqlite3NestedParse(Parse*, const char*, ...);
SQLITE_PRIVATE void sqlite3ExpirePreparedStatements(sqlite3*, int);
SQLITE_PRIVATE void sqlite3CodeRhsOfIN(Parse*, Expr*, int);
SQLITE_PRIVATE int sqlite3CodeSubselect(Parse*, Expr*);
SQLITE_PRIVATE void sqlite3SelectPrep(Parse*, Select*, NameContext*);
SQLITE_PRIVATE int sqlite3ExpandSubquery(Parse*, SrcItem*);
SQLITE_PRIVATE void sqlite3SelectWrongNumTermsError(Parse *pParse, Select *p);
SQLITE_PRIVATE int sqlite3MatchEName(
  const struct ExprList_item*,
  const char*,
  const char*,
  const char*,
  int*
);
SQLITE_PRIVATE Bitmask sqlite3ExprColUsed(Expr*);
SQLITE_PRIVATE u8 sqlite3StrIHash(const char*);
SQLITE_PRIVATE int sqlite3ResolveExprNames(NameContext*, Expr*);
SQLITE_PRIVATE int sqlite3ResolveExprListNames(NameContext*, ExprList*);
SQLITE_PRIVATE void sqlite3ResolveSelectNames(Parse*, Select*, NameContext*);
SQLITE_PRIVATE int sqlite3ResolveSelfReference(Parse*,Table*,int,Expr*,ExprList*);
SQLITE_PRIVATE int sqlite3ResolveOrderGroupBy(Parse*, Select*, ExprList*, const char*);
SQLITE_PRIVATE void sqlite3ColumnDefault(Vdbe *, Table *, int, int);
SQLITE_PRIVATE void sqlite3AlterFinishAddColumn(Parse *, Token *);
SQLITE_PRIVATE void sqlite3AlterBeginAddColumn(Parse *, SrcList *);
SQLITE_PRIVATE void sqlite3AlterDropColumn(Parse*, SrcList*, const Token*);
SQLITE_PRIVATE const void *sqlite3RenameTokenMap(Parse*, const void*, const Token*);
SQLITE_PRIVATE void sqlite3RenameTokenRemap(Parse*, const void *pTo, const void *pFrom);
SQLITE_PRIVATE void sqlite3RenameExprUnmap(Parse*, Expr*);
SQLITE_PRIVATE void sqlite3RenameExprlistUnmap(Parse*, ExprList*);
SQLITE_PRIVATE CollSeq *sqlite3GetCollSeq(Parse*, u8, CollSeq *, const char*);
SQLITE_PRIVATE char sqlite3AffinityType(const char*, Column*);
SQLITE_PRIVATE void sqlite3Analyze(Parse*, Token*, Token*);
SQLITE_PRIVATE int sqlite3InvokeBusyHandler(BusyHandler*);
SQLITE_PRIVATE int sqlite3FindDb(sqlite3*, Token*);
SQLITE_PRIVATE int sqlite3FindDbName(sqlite3 *, const char *);
SQLITE_PRIVATE int sqlite3AnalysisLoad(sqlite3*,int iDB);
SQLITE_PRIVATE void sqlite3DeleteIndexSamples(sqlite3*,Index*);
SQLITE_PRIVATE void sqlite3DefaultRowEst(Index*);
SQLITE_PRIVATE void sqlite3RegisterLikeFunctions(sqlite3*, int);
SQLITE_PRIVATE int sqlite3IsLikeFunction(sqlite3*,Expr*,int*,char*);
SQLITE_PRIVATE void sqlite3SchemaClear(void *);
SQLITE_PRIVATE Schema *sqlite3SchemaGet(sqlite3 *, Btree *);
SQLITE_PRIVATE int sqlite3SchemaToIndex(sqlite3 *db, Schema *);
SQLITE_PRIVATE KeyInfo *sqlite3KeyInfoAlloc(sqlite3*,int,int);
SQLITE_PRIVATE void sqlite3KeyInfoUnref(KeyInfo*);
SQLITE_PRIVATE KeyInfo *sqlite3KeyInfoRef(KeyInfo*);
SQLITE_PRIVATE KeyInfo *sqlite3KeyInfoOfIndex(Parse*, Index*);
SQLITE_PRIVATE KeyInfo *sqlite3KeyInfoFromExprList(Parse*, ExprList*, int, int);
SQLITE_PRIVATE const char *sqlite3SelectOpName(int);
SQLITE_PRIVATE int sqlite3HasExplicitNulls(Parse*, ExprList*);

#ifdef SQLITE_DEBUG
SQLITE_PRIVATE int sqlite3KeyInfoIsWriteable(KeyInfo*);
#endif
SQLITE_PRIVATE int sqlite3CreateFunc(sqlite3 *, const char *, int, int, void *,
  void (*)(sqlite3_context*,int,sqlite3_value **),
  void (*)(sqlite3_context*,int,sqlite3_value **),
  void (*)(sqlite3_context*),
  void (*)(sqlite3_context*),
  void (*)(sqlite3_context*,int,sqlite3_value **),
  FuncDestructor *pDestructor
);
SQLITE_PRIVATE void sqlite3NoopDestructor(void*);
SQLITE_PRIVATE void *sqlite3OomFault(sqlite3*);
SQLITE_PRIVATE void sqlite3OomClear(sqlite3*);
SQLITE_PRIVATE int sqlite3ApiExit(sqlite3 *db, int);
SQLITE_PRIVATE int sqlite3OpenTempDatabase(Parse *);

SQLITE_PRIVATE char *sqlite3RCStrRef(char*);
SQLITE_PRIVATE void sqlite3RCStrUnref(void*);
SQLITE_PRIVATE char *sqlite3RCStrNew(u64);
SQLITE_PRIVATE char *sqlite3RCStrResize(char*,u64);

SQLITE_PRIVATE void sqlite3StrAccumInit(StrAccum*, sqlite3*, char*, int, int);
SQLITE_PRIVATE int sqlite3StrAccumEnlarge(StrAccum*, i64);
SQLITE_PRIVATE char *sqlite3StrAccumFinish(StrAccum*);
SQLITE_PRIVATE void sqlite3StrAccumSetError(StrAccum*, u8);
SQLITE_PRIVATE void sqlite3ResultStrAccum(sqlite3_context*,StrAccum*);
SQLITE_PRIVATE void sqlite3SelectDestInit(SelectDest*,int,int);
SQLITE_PRIVATE Expr *sqlite3CreateColumnExpr(sqlite3 *, SrcList *, int, int);
SQLITE_PRIVATE void sqlite3RecordErrorByteOffset(sqlite3*,const char*);
SQLITE_PRIVATE void sqlite3RecordErrorOffsetOfExpr(sqlite3*,const Expr*);

SQLITE_PRIVATE void sqlite3BackupRestart(sqlite3_backup *);
SQLITE_PRIVATE void sqlite3BackupUpdate(sqlite3_backup *, Pgno, const u8 *);

#ifndef SQLITE_OMIT_SUBQUERY
SQLITE_PRIVATE int sqlite3ExprCheckIN(Parse*, Expr*);
#else
# define sqlite3ExprCheckIN(x,y) SQLITE_OK
#endif

#ifdef SQLITE_ENABLE_STAT4
SQLITE_PRIVATE int sqlite3Stat4ProbeSetValue(
    Parse*,Index*,UnpackedRecord**,Expr*,int,int,int*);
SQLITE_PRIVATE int sqlite3Stat4ValueFromExpr(Parse*, Expr*, u8, sqlite3_value**);
SQLITE_PRIVATE void sqlite3Stat4ProbeFree(UnpackedRecord*);
SQLITE_PRIVATE int sqlite3Stat4Column(sqlite3*, const void*, int, int, sqlite3_value**);
SQLITE_PRIVATE char sqlite3IndexColumnAffinity(sqlite3*, Index*, int);
#endif

/*
** The interface to the LEMON-generated parser
*/
#ifndef SQLITE_AMALGAMATION
SQLITE_PRIVATE   void *sqlite3ParserAlloc(void*(*)(u64), Parse*);
SQLITE_PRIVATE   void sqlite3ParserFree(void*, void(*)(void*));
#endif
SQLITE_PRIVATE void sqlite3Parser(void*, int, Token);
SQLITE_PRIVATE int sqlite3ParserFallback(int);
#ifdef YYTRACKMAXSTACKDEPTH
SQLITE_PRIVATE   int sqlite3ParserStackPeak(void*);
#endif

SQLITE_PRIVATE void sqlite3AutoLoadExtensions(sqlite3*);
#ifndef SQLITE_OMIT_LOAD_EXTENSION
SQLITE_PRIVATE   void sqlite3CloseExtensions(sqlite3*);
#else
# define sqlite3CloseExtensions(X)
#endif

#ifndef SQLITE_OMIT_SHARED_CACHE
SQLITE_PRIVATE   void sqlite3TableLock(Parse *, int, Pgno, u8, const char *);
#else
  #define sqlite3TableLock(v,w,x,y,z)
#endif

#ifdef SQLITE_TEST
SQLITE_PRIVATE   int sqlite3Utf8To8(unsigned char*);
#endif

#ifdef SQLITE_OMIT_VIRTUALTABLE
#  define sqlite3VtabClear(D,T)
#  define sqlite3VtabSync(X,Y) SQLITE_OK
#  define sqlite3VtabRollback(X)
#  define sqlite3VtabCommit(X)
#  define sqlite3VtabInSync(db) 0
#  define sqlite3VtabLock(X)
#  define sqlite3VtabUnlock(X)
#  define sqlite3VtabModuleUnref(D,X)
#  define sqlite3VtabUnlockList(X)
#  define sqlite3VtabSavepoint(X, Y, Z) SQLITE_OK
#  define sqlite3GetVTable(X,Y)  ((VTable*)0)
#else
SQLITE_PRIVATE    void sqlite3VtabClear(sqlite3 *db, Table*);
SQLITE_PRIVATE    void sqlite3VtabDisconnect(sqlite3 *db, Table *p);
SQLITE_PRIVATE    int sqlite3VtabSync(sqlite3 *db, Vdbe*);
SQLITE_PRIVATE    int sqlite3VtabRollback(sqlite3 *db);
SQLITE_PRIVATE    int sqlite3VtabCommit(sqlite3 *db);
SQLITE_PRIVATE    void sqlite3VtabLock(VTable *);
SQLITE_PRIVATE    void sqlite3VtabUnlock(VTable *);
SQLITE_PRIVATE    void sqlite3VtabModuleUnref(sqlite3*,Module*);
SQLITE_PRIVATE    void sqlite3VtabUnlockList(sqlite3*);
SQLITE_PRIVATE    int sqlite3VtabSavepoint(sqlite3 *, int, int);
SQLITE_PRIVATE    void sqlite3VtabImportErrmsg(Vdbe*, sqlite3_vtab*);
SQLITE_PRIVATE    VTable *sqlite3GetVTable(sqlite3*, Table*);
SQLITE_PRIVATE    Module *sqlite3VtabCreateModule(
     sqlite3*,
     const char*,
     const sqlite3_module*,
     void*,
     void(*)(void*)
   );
#  define sqlite3VtabInSync(db) ((db)->nVTrans>0 && (db)->aVTrans==0)
#endif
SQLITE_PRIVATE int sqlite3ReadOnlyShadowTables(sqlite3 *db);
#ifndef SQLITE_OMIT_VIRTUALTABLE
SQLITE_PRIVATE   int sqlite3ShadowTableName(sqlite3 *db, const char *zName);
SQLITE_PRIVATE   int sqlite3IsShadowTableOf(sqlite3*,Table*,const char*);
SQLITE_PRIVATE   void sqlite3MarkAllShadowTablesOf(sqlite3*, Table*);
#else
# define sqlite3ShadowTableName(A,B) 0
# define sqlite3IsShadowTableOf(A,B,C) 0
# define sqlite3MarkAllShadowTablesOf(A,B)
#endif
SQLITE_PRIVATE int sqlite3VtabEponymousTableInit(Parse*,Module*);
SQLITE_PRIVATE void sqlite3VtabEponymousTableClear(sqlite3*,Module*);
SQLITE_PRIVATE void sqlite3VtabMakeWritable(Parse*,Table*);
SQLITE_PRIVATE void sqlite3VtabBeginParse(Parse*, Token*, Token*, Token*, int);
SQLITE_PRIVATE void sqlite3VtabFinishParse(Parse*, Token*);
SQLITE_PRIVATE void sqlite3VtabArgInit(Parse*);
SQLITE_PRIVATE void sqlite3VtabArgExtend(Parse*, Token*);
SQLITE_PRIVATE int sqlite3VtabCallCreate(sqlite3*, int, const char *, char **);
SQLITE_PRIVATE int sqlite3VtabCallConnect(Parse*, Table*);
SQLITE_PRIVATE int sqlite3VtabCallDestroy(sqlite3*, int, const char *);
SQLITE_PRIVATE int sqlite3VtabBegin(sqlite3 *, VTable *);

SQLITE_PRIVATE FuncDef *sqlite3VtabOverloadFunction(sqlite3 *,FuncDef*, int nArg, Expr*);
SQLITE_PRIVATE void sqlite3VtabUsesAllSchemas(Parse*);
SQLITE_PRIVATE sqlite3_int64 sqlite3StmtCurrentTime(sqlite3_context*);
SQLITE_PRIVATE int sqlite3VdbeParameterIndex(Vdbe*, const char*, int);
SQLITE_PRIVATE int sqlite3TransferBindings(sqlite3_stmt *, sqlite3_stmt *);
SQLITE_PRIVATE void sqlite3ParseObjectInit(Parse*,sqlite3*);
SQLITE_PRIVATE void sqlite3ParseObjectReset(Parse*);
SQLITE_PRIVATE void *sqlite3ParserAddCleanup(Parse*,void(*)(sqlite3*,void*),void*);
#ifdef SQLITE_ENABLE_NORMALIZE
SQLITE_PRIVATE char *sqlite3Normalize(Vdbe*, const char*);
#endif
SQLITE_PRIVATE int sqlite3Reprepare(Vdbe*);
SQLITE_PRIVATE void sqlite3ExprListCheckLength(Parse*, ExprList*, const char*);
