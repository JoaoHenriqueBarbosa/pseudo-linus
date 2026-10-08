SQLITE_PRIVATE void sqlite3ExprDelete(sqlite3*, Expr*);
SQLITE_PRIVATE void sqlite3ExprDeleteGeneric(sqlite3*,void*);
SQLITE_PRIVATE int sqlite3ExprDeferredDelete(Parse*, Expr*);
SQLITE_PRIVATE void sqlite3ExprUnmapAndDelete(Parse*, Expr*);
SQLITE_PRIVATE ExprList *sqlite3ExprListAppend(Parse*,ExprList*,Expr*);
SQLITE_PRIVATE ExprList *sqlite3ExprListAppendVector(Parse*,ExprList*,IdList*,Expr*);
SQLITE_PRIVATE Select *sqlite3ExprListToValues(Parse*, int, ExprList*);
SQLITE_PRIVATE void sqlite3ExprListSetSortOrder(ExprList*,int,int);
SQLITE_PRIVATE void sqlite3ExprListSetName(Parse*,ExprList*,const Token*,int);
SQLITE_PRIVATE void sqlite3ExprListSetSpan(Parse*,ExprList*,const char*,const char*);
SQLITE_PRIVATE void sqlite3ExprListDelete(sqlite3*, ExprList*);
SQLITE_PRIVATE void sqlite3ExprListDeleteGeneric(sqlite3*,void*);
SQLITE_PRIVATE u32 sqlite3ExprListFlags(const ExprList*);
SQLITE_PRIVATE int sqlite3IndexHasDuplicateRootPage(Index*);
SQLITE_PRIVATE int sqlite3Init(sqlite3*, char**);
SQLITE_PRIVATE int sqlite3InitCallback(void*, int, char**, char**);
SQLITE_PRIVATE int sqlite3InitOne(sqlite3*, int, char**, u32);
SQLITE_PRIVATE void sqlite3Pragma(Parse*,Token*,Token*,Token*,int);
#ifndef SQLITE_OMIT_VIRTUALTABLE
SQLITE_PRIVATE Module *sqlite3PragmaVtabRegister(sqlite3*,const char *zName);
#endif
SQLITE_PRIVATE void sqlite3ResetAllSchemasOfConnection(sqlite3*);
SQLITE_PRIVATE void sqlite3ResetOneSchema(sqlite3*,int);
SQLITE_PRIVATE void sqlite3CollapseDatabaseArray(sqlite3*);
SQLITE_PRIVATE void sqlite3CommitInternalChanges(sqlite3*);
SQLITE_PRIVATE void sqlite3ColumnSetExpr(Parse*,Table*,Column*,Expr*);
SQLITE_PRIVATE Expr *sqlite3ColumnExpr(Table*,Column*);
SQLITE_PRIVATE void sqlite3ColumnSetColl(sqlite3*,Column*,const char*zColl);
SQLITE_PRIVATE const char *sqlite3ColumnColl(Column*);
SQLITE_PRIVATE void sqlite3DeleteColumnNames(sqlite3*,Table*);
SQLITE_PRIVATE void sqlite3GenerateColumnNames(Parse *pParse, Select *pSelect);
SQLITE_PRIVATE int sqlite3ColumnsFromExprList(Parse*,ExprList*,i16*,Column**);
SQLITE_PRIVATE void sqlite3SubqueryColumnTypes(Parse*,Table*,Select*,char);
SQLITE_PRIVATE Table *sqlite3ResultSetOfSelect(Parse*,Select*,char);
SQLITE_PRIVATE void sqlite3OpenSchemaTable(Parse *, int);
SQLITE_PRIVATE Index *sqlite3PrimaryKeyIndex(Table*);
SQLITE_PRIVATE i16 sqlite3TableColumnToIndex(Index*, i16);
#ifdef SQLITE_OMIT_GENERATED_COLUMNS
# define sqlite3TableColumnToStorage(T,X) (X)  /* No-op pass-through */
# define sqlite3StorageColumnToTable(T,X) (X)  /* No-op pass-through */
#else
SQLITE_PRIVATE   i16 sqlite3TableColumnToStorage(Table*, i16);
SQLITE_PRIVATE   i16 sqlite3StorageColumnToTable(Table*, i16);
#endif
SQLITE_PRIVATE void sqlite3StartTable(Parse*,Token*,Token*,int,int,int,int);
#if SQLITE_ENABLE_HIDDEN_COLUMNS
SQLITE_PRIVATE   void sqlite3ColumnPropertiesFromName(Table*, Column*);
#else
# define sqlite3ColumnPropertiesFromName(T,C) /* no-op */
#endif
SQLITE_PRIVATE void sqlite3AddColumn(Parse*,Token,Token);
SQLITE_PRIVATE void sqlite3AddNotNull(Parse*, int);
SQLITE_PRIVATE void sqlite3AddPrimaryKey(Parse*, ExprList*, int, int, int);
SQLITE_PRIVATE void sqlite3AddCheckConstraint(Parse*, Expr*, const char*, const char*);
SQLITE_PRIVATE void sqlite3AddDefaultValue(Parse*,Expr*,const char*,const char*);
SQLITE_PRIVATE void sqlite3AddCollateType(Parse*, Token*);
SQLITE_PRIVATE void sqlite3AddGenerated(Parse*,Expr*,Token*);
SQLITE_PRIVATE void sqlite3EndTable(Parse*,Token*,Token*,u32,Select*);
SQLITE_PRIVATE void sqlite3AddReturning(Parse*,ExprList*);
SQLITE_PRIVATE int sqlite3ParseUri(const char*,const char*,unsigned int*,
                    sqlite3_vfs**,char**,char **);
#define sqlite3CodecQueryParameters(A,B,C) 0
SQLITE_PRIVATE Btree *sqlite3DbNameToBtree(sqlite3*,const char*);

#ifdef SQLITE_UNTESTABLE
# define sqlite3FaultSim(X) SQLITE_OK
#else
SQLITE_PRIVATE   int sqlite3FaultSim(int);
#endif

SQLITE_PRIVATE Bitvec *sqlite3BitvecCreate(u32);
SQLITE_PRIVATE int sqlite3BitvecTest(Bitvec*, u32);
SQLITE_PRIVATE int sqlite3BitvecTestNotNull(Bitvec*, u32);
SQLITE_PRIVATE int sqlite3BitvecSet(Bitvec*, u32);
SQLITE_PRIVATE void sqlite3BitvecClear(Bitvec*, u32, void*);
SQLITE_PRIVATE void sqlite3BitvecDestroy(Bitvec*);
SQLITE_PRIVATE u32 sqlite3BitvecSize(Bitvec*);
#ifndef SQLITE_UNTESTABLE
SQLITE_PRIVATE int sqlite3BitvecBuiltinTest(int,int*);
#endif

SQLITE_PRIVATE RowSet *sqlite3RowSetInit(sqlite3*);
SQLITE_PRIVATE void sqlite3RowSetDelete(void*);
SQLITE_PRIVATE void sqlite3RowSetClear(void*);
SQLITE_PRIVATE void sqlite3RowSetInsert(RowSet*, i64);
SQLITE_PRIVATE int sqlite3RowSetTest(RowSet*, int iBatch, i64);
SQLITE_PRIVATE int sqlite3RowSetNext(RowSet*, i64*);

SQLITE_PRIVATE void sqlite3CreateView(Parse*,Token*,Token*,Token*,ExprList*,Select*,int,int);

#if !defined(SQLITE_OMIT_VIEW) || !defined(SQLITE_OMIT_VIRTUALTABLE)
SQLITE_PRIVATE   int sqlite3ViewGetColumnNames(Parse*,Table*);
#else
# define sqlite3ViewGetColumnNames(A,B) 0
#endif

#if SQLITE_MAX_ATTACHED>30
SQLITE_PRIVATE   int sqlite3DbMaskAllZero(yDbMask);
#endif
SQLITE_PRIVATE void sqlite3DropTable(Parse*, SrcList*, int, int);
SQLITE_PRIVATE void sqlite3CodeDropTable(Parse*, Table*, int, int);
SQLITE_PRIVATE void sqlite3DeleteTable(sqlite3*, Table*);
SQLITE_PRIVATE void sqlite3DeleteTableGeneric(sqlite3*, void*);
SQLITE_PRIVATE void sqlite3FreeIndex(sqlite3*, Index*);
#ifndef SQLITE_OMIT_AUTOINCREMENT
SQLITE_PRIVATE   void sqlite3AutoincrementBegin(Parse *pParse);
SQLITE_PRIVATE   void sqlite3AutoincrementEnd(Parse *pParse);
#else
# define sqlite3AutoincrementBegin(X)
# define sqlite3AutoincrementEnd(X)
#endif
SQLITE_PRIVATE void sqlite3Insert(Parse*, SrcList*, Select*, IdList*, int, Upsert*);
#ifndef SQLITE_OMIT_GENERATED_COLUMNS
SQLITE_PRIVATE   void sqlite3ComputeGeneratedColumns(Parse*, int, Table*);
#endif
SQLITE_PRIVATE void *sqlite3ArrayAllocate(sqlite3*,void*,int,int*,int*);
SQLITE_PRIVATE IdList *sqlite3IdListAppend(Parse*, IdList*, Token*);
SQLITE_PRIVATE int sqlite3IdListIndex(IdList*,const char*);
SQLITE_PRIVATE SrcList *sqlite3SrcListEnlarge(Parse*, SrcList*, int, int);
SQLITE_PRIVATE SrcList *sqlite3SrcListAppendList(Parse *pParse, SrcList *p1, SrcList *p2);
SQLITE_PRIVATE SrcList *sqlite3SrcListAppend(Parse*, SrcList*, Token*, Token*);
SQLITE_PRIVATE SrcList *sqlite3SrcListAppendFromTerm(Parse*, SrcList*, Token*, Token*,
                                      Token*, Select*, OnOrUsing*);
SQLITE_PRIVATE void sqlite3SrcListIndexedBy(Parse *, SrcList *, Token *);
SQLITE_PRIVATE void sqlite3SrcListFuncArgs(Parse*, SrcList*, ExprList*);
SQLITE_PRIVATE int sqlite3IndexedByLookup(Parse *, SrcItem *);
SQLITE_PRIVATE void sqlite3SrcListShiftJoinType(Parse*,SrcList*);
SQLITE_PRIVATE void sqlite3SrcListAssignCursors(Parse*, SrcList*);
SQLITE_PRIVATE void sqlite3IdListDelete(sqlite3*, IdList*);
SQLITE_PRIVATE void sqlite3ClearOnOrUsing(sqlite3*, OnOrUsing*);
SQLITE_PRIVATE void sqlite3SrcListDelete(sqlite3*, SrcList*);
SQLITE_PRIVATE Index *sqlite3AllocateIndexObject(sqlite3*,i16,int,char**);
SQLITE_PRIVATE void sqlite3CreateIndex(Parse*,Token*,Token*,SrcList*,ExprList*,int,Token*,
                          Expr*, int, int, u8);
SQLITE_PRIVATE void sqlite3DropIndex(Parse*, SrcList*, int);
SQLITE_PRIVATE int sqlite3Select(Parse*, Select*, SelectDest*);
SQLITE_PRIVATE Select *sqlite3SelectNew(Parse*,ExprList*,SrcList*,Expr*,ExprList*,
                         Expr*,ExprList*,u32,Expr*);
SQLITE_PRIVATE void sqlite3SelectDelete(sqlite3*, Select*);
SQLITE_PRIVATE void sqlite3SelectDeleteGeneric(sqlite3*,void*);
SQLITE_PRIVATE Table *sqlite3SrcListLookup(Parse*, SrcList*);
SQLITE_PRIVATE int sqlite3IsReadOnly(Parse*, Table*, Trigger*);
SQLITE_PRIVATE void sqlite3OpenTable(Parse*, int iCur, int iDb, Table*, int);
#if defined(SQLITE_ENABLE_UPDATE_DELETE_LIMIT) && !defined(SQLITE_OMIT_SUBQUERY)
SQLITE_PRIVATE Expr *sqlite3LimitWhere(Parse*,SrcList*,Expr*,ExprList*,Expr*,char*);
#endif
SQLITE_PRIVATE void sqlite3CodeChangeCount(Vdbe*,int,const char*);
SQLITE_PRIVATE void sqlite3DeleteFrom(Parse*, SrcList*, Expr*, ExprList*, Expr*);
SQLITE_PRIVATE void sqlite3Update(Parse*, SrcList*, ExprList*,Expr*,int,ExprList*,Expr*,
                   Upsert*);
SQLITE_PRIVATE WhereInfo *sqlite3WhereBegin(Parse*,SrcList*,Expr*,ExprList*,
                             ExprList*,Select*,u16,int);
SQLITE_PRIVATE void sqlite3WhereEnd(WhereInfo*);
SQLITE_PRIVATE LogEst sqlite3WhereOutputRowCount(WhereInfo*);
SQLITE_PRIVATE int sqlite3WhereIsDistinct(WhereInfo*);
SQLITE_PRIVATE int sqlite3WhereIsOrdered(WhereInfo*);
SQLITE_PRIVATE int sqlite3WhereOrderByLimitOptLabel(WhereInfo*);
SQLITE_PRIVATE void sqlite3WhereMinMaxOptEarlyOut(Vdbe*,WhereInfo*);
SQLITE_PRIVATE int sqlite3WhereIsSorted(WhereInfo*);
SQLITE_PRIVATE int sqlite3WhereContinueLabel(WhereInfo*);
SQLITE_PRIVATE int sqlite3WhereBreakLabel(WhereInfo*);
SQLITE_PRIVATE int sqlite3WhereOkOnePass(WhereInfo*, int*);
#define ONEPASS_OFF      0        /* Use of ONEPASS not allowed */
#define ONEPASS_SINGLE   1        /* ONEPASS valid for a single row update */
#define ONEPASS_MULTI    2        /* ONEPASS is valid for multiple rows */
SQLITE_PRIVATE int sqlite3WhereUsesDeferredSeek(WhereInfo*);
SQLITE_PRIVATE void sqlite3ExprCodeLoadIndexColumn(Parse*, Index*, int, int, int);
SQLITE_PRIVATE int sqlite3ExprCodeGetColumn(Parse*, Table*, int, int, int, u8);
SQLITE_PRIVATE void sqlite3ExprCodeGetColumnOfTable(Vdbe*, Table*, int, int, int);
SQLITE_PRIVATE void sqlite3ExprCodeMove(Parse*, int, int, int);
SQLITE_PRIVATE void sqlite3ExprCode(Parse*, Expr*, int);
#ifndef SQLITE_OMIT_GENERATED_COLUMNS
SQLITE_PRIVATE void sqlite3ExprCodeGeneratedColumn(Parse*, Table*, Column*, int);
#endif
SQLITE_PRIVATE void sqlite3ExprCodeCopy(Parse*, Expr*, int);
SQLITE_PRIVATE void sqlite3ExprCodeFactorable(Parse*, Expr*, int);
SQLITE_PRIVATE int sqlite3ExprCodeRunJustOnce(Parse*, Expr*, int);
SQLITE_PRIVATE int sqlite3ExprCodeTemp(Parse*, Expr*, int*);
SQLITE_PRIVATE int sqlite3ExprCodeTarget(Parse*, Expr*, int);
SQLITE_PRIVATE int sqlite3ExprCodeExprList(Parse*, ExprList*, int, int, u8);
#define SQLITE_ECEL_DUP      0x01  /* Deep, not shallow copies */
#define SQLITE_ECEL_FACTOR   0x02  /* Factor out constant terms */
#define SQLITE_ECEL_REF      0x04  /* Use ExprList.u.x.iOrderByCol */
#define SQLITE_ECEL_OMITREF  0x08  /* Omit if ExprList.u.x.iOrderByCol */
SQLITE_PRIVATE void sqlite3ExprIfTrue(Parse*, Expr*, int, int);
SQLITE_PRIVATE void sqlite3ExprIfFalse(Parse*, Expr*, int, int);
SQLITE_PRIVATE void sqlite3ExprIfFalseDup(Parse*, Expr*, int, int);
SQLITE_PRIVATE Table *sqlite3FindTable(sqlite3*,const char*, const char*);
#define LOCATE_VIEW    0x01
#define LOCATE_NOERR   0x02
SQLITE_PRIVATE Table *sqlite3LocateTable(Parse*,u32 flags,const char*, const char*);
SQLITE_PRIVATE const char *sqlite3PreferredTableName(const char*);
SQLITE_PRIVATE Table *sqlite3LocateTableItem(Parse*,u32 flags,SrcItem *);
SQLITE_PRIVATE Index *sqlite3FindIndex(sqlite3*,const char*, const char*);
SQLITE_PRIVATE void sqlite3UnlinkAndDeleteTable(sqlite3*,int,const char*);
SQLITE_PRIVATE void sqlite3UnlinkAndDeleteIndex(sqlite3*,int,const char*);
SQLITE_PRIVATE void sqlite3Vacuum(Parse*,Token*,Expr*);
SQLITE_PRIVATE int sqlite3RunVacuum(char**, sqlite3*, int, sqlite3_value*);
SQLITE_PRIVATE char *sqlite3NameFromToken(sqlite3*, const Token*);
SQLITE_PRIVATE int sqlite3ExprCompare(const Parse*,const Expr*,const Expr*, int);
SQLITE_PRIVATE int sqlite3ExprCompareSkip(Expr*,Expr*,int);
SQLITE_PRIVATE int sqlite3ExprListCompare(const ExprList*,const ExprList*, int);
SQLITE_PRIVATE int sqlite3ExprImpliesExpr(const Parse*,const Expr*,const Expr*, int);
SQLITE_PRIVATE int sqlite3ExprImpliesNonNullRow(Expr*,int,int);
SQLITE_PRIVATE void sqlite3AggInfoPersistWalkerInit(Walker*,Parse*);
SQLITE_PRIVATE void sqlite3ExprAnalyzeAggregates(NameContext*, Expr*);
SQLITE_PRIVATE void sqlite3ExprAnalyzeAggList(NameContext*,ExprList*);
SQLITE_PRIVATE int sqlite3ExprCoveredByIndex(Expr*, int iCur, Index *pIdx);
SQLITE_PRIVATE int sqlite3ReferencesSrcList(Parse*, Expr*, SrcList*);
SQLITE_PRIVATE Vdbe *sqlite3GetVdbe(Parse*);
#ifndef SQLITE_UNTESTABLE
SQLITE_PRIVATE void sqlite3PrngSaveState(void);
SQLITE_PRIVATE void sqlite3PrngRestoreState(void);
#endif
SQLITE_PRIVATE void sqlite3RollbackAll(sqlite3*,int);
SQLITE_PRIVATE void sqlite3CodeVerifySchema(Parse*, int);
SQLITE_PRIVATE void sqlite3CodeVerifyNamedSchema(Parse*, const char *zDb);
SQLITE_PRIVATE void sqlite3BeginTransaction(Parse*, int);
SQLITE_PRIVATE void sqlite3EndTransaction(Parse*,int);
SQLITE_PRIVATE void sqlite3Savepoint(Parse*, int, Token*);
SQLITE_PRIVATE void sqlite3CloseSavepoints(sqlite3 *);
SQLITE_PRIVATE void sqlite3LeaveMutexAndCloseZombie(sqlite3*);
SQLITE_PRIVATE u32 sqlite3IsTrueOrFalse(const char*);
SQLITE_PRIVATE int sqlite3ExprIdToTrueFalse(Expr*);
SQLITE_PRIVATE int sqlite3ExprTruthValue(const Expr*);
SQLITE_PRIVATE int sqlite3ExprIsConstant(Parse*,Expr*);
SQLITE_PRIVATE int sqlite3ExprIsConstantOrFunction(Expr*, u8);
SQLITE_PRIVATE int sqlite3ExprIsConstantOrGroupBy(Parse*, Expr*, ExprList*);
SQLITE_PRIVATE int sqlite3ExprIsSingleTableConstraint(Expr*,const SrcList*,int,int);
#ifdef SQLITE_ENABLE_CURSOR_HINTS
SQLITE_PRIVATE int sqlite3ExprContainsSubquery(Expr*);
#endif
SQLITE_PRIVATE int sqlite3ExprIsInteger(const Expr*, int*);
SQLITE_PRIVATE int sqlite3ExprCanBeNull(const Expr*);
SQLITE_PRIVATE int sqlite3ExprNeedsNoAffinityChange(const Expr*, char);
SQLITE_PRIVATE int sqlite3IsRowid(const char*);
SQLITE_PRIVATE const char *sqlite3RowidAlias(Table *pTab);
SQLITE_PRIVATE void sqlite3GenerateRowDelete(
    Parse*,Table*,Trigger*,int,int,int,i16,u8,u8,u8,int);
SQLITE_PRIVATE void sqlite3GenerateRowIndexDelete(Parse*, Table*, int, int, int*, int);
SQLITE_PRIVATE int sqlite3GenerateIndexKey(Parse*, Index*, int, int, int, int*,Index*,int);
SQLITE_PRIVATE void sqlite3ResolvePartIdxLabel(Parse*,int);
SQLITE_PRIVATE int sqlite3ExprReferencesUpdatedColumn(Expr*,int*,int);
SQLITE_PRIVATE void sqlite3GenerateConstraintChecks(Parse*,Table*,int*,int,int,int,int,
                                     u8,u8,int,int*,int*,Upsert*);
#ifdef SQLITE_ENABLE_NULL_TRIM
SQLITE_PRIVATE   void sqlite3SetMakeRecordP5(Vdbe*,Table*);
#else
# define sqlite3SetMakeRecordP5(A,B)
#endif
SQLITE_PRIVATE void sqlite3CompleteInsertion(Parse*,Table*,int,int,int,int*,int,int,int);
SQLITE_PRIVATE int sqlite3OpenTableAndIndices(Parse*, Table*, int, u8, int, u8*, int*, int*);
SQLITE_PRIVATE void sqlite3BeginWriteOperation(Parse*, int, int);
SQLITE_PRIVATE void sqlite3MultiWrite(Parse*);
SQLITE_PRIVATE void sqlite3MayAbort(Parse*);
SQLITE_PRIVATE void sqlite3HaltConstraint(Parse*, int, int, char*, i8, u8);
SQLITE_PRIVATE void sqlite3UniqueConstraint(Parse*, int, Index*);
SQLITE_PRIVATE void sqlite3RowidConstraint(Parse*, int, Table*);
SQLITE_PRIVATE Expr *sqlite3ExprDup(sqlite3*,const Expr*,int);
SQLITE_PRIVATE ExprList *sqlite3ExprListDup(sqlite3*,const ExprList*,int);
SQLITE_PRIVATE SrcList *sqlite3SrcListDup(sqlite3*,const SrcList*,int);
SQLITE_PRIVATE IdList *sqlite3IdListDup(sqlite3*,const IdList*);
SQLITE_PRIVATE Select *sqlite3SelectDup(sqlite3*,const Select*,int);
SQLITE_PRIVATE FuncDef *sqlite3FunctionSearch(int,const char*);
SQLITE_PRIVATE void sqlite3InsertBuiltinFuncs(FuncDef*,int);
SQLITE_PRIVATE FuncDef *sqlite3FindFunction(sqlite3*,const char*,int,u8,u8);
SQLITE_PRIVATE void sqlite3QuoteValue(StrAccum*,sqlite3_value*);
SQLITE_PRIVATE void sqlite3RegisterBuiltinFunctions(void);
SQLITE_PRIVATE void sqlite3RegisterDateTimeFunctions(void);
SQLITE_PRIVATE void sqlite3RegisterJsonFunctions(void);
SQLITE_PRIVATE void sqlite3RegisterPerConnectionBuiltinFunctions(sqlite3*);
#if !defined(SQLITE_OMIT_VIRTUALTABLE) && !defined(SQLITE_OMIT_JSON)
SQLITE_PRIVATE   int sqlite3JsonTableFunctions(sqlite3*);
#endif
SQLITE_PRIVATE int sqlite3SafetyCheckOk(sqlite3*);
SQLITE_PRIVATE int sqlite3SafetyCheckSickOrOk(sqlite3*);
SQLITE_PRIVATE void sqlite3ChangeCookie(Parse*, int);
SQLITE_PRIVATE With *sqlite3WithDup(sqlite3 *db, With *p);

#if !defined(SQLITE_OMIT_VIEW) && !defined(SQLITE_OMIT_TRIGGER)
SQLITE_PRIVATE void sqlite3MaterializeView(Parse*, Table*, Expr*, ExprList*,Expr*,int);
#endif

#ifndef SQLITE_OMIT_TRIGGER
SQLITE_PRIVATE   void sqlite3BeginTrigger(Parse*, Token*,Token*,int,int,IdList*,SrcList*,
                           Expr*,int, int);
SQLITE_PRIVATE   void sqlite3FinishTrigger(Parse*, TriggerStep*, Token*);
SQLITE_PRIVATE   void sqlite3DropTrigger(Parse*, SrcList*, int);
SQLITE_PRIVATE   void sqlite3DropTriggerPtr(Parse*, Trigger*);
SQLITE_PRIVATE   Trigger *sqlite3TriggersExist(Parse *, Table*, int, ExprList*, int *pMask);
SQLITE_PRIVATE   Trigger *sqlite3TriggerList(Parse *, Table *);
SQLITE_PRIVATE   void sqlite3CodeRowTrigger(Parse*, Trigger *, int, ExprList*, int, Table *,
                            int, int, int);
SQLITE_PRIVATE   void sqlite3CodeRowTriggerDirect(Parse *, Trigger *, Table *, int, int, int);
  void sqliteViewTriggers(Parse*, Table*, Expr*, int, ExprList*);
SQLITE_PRIVATE   void sqlite3DeleteTriggerStep(sqlite3*, TriggerStep*);
SQLITE_PRIVATE   TriggerStep *sqlite3TriggerSelectStep(sqlite3*,Select*,
                                        const char*,const char*);
SQLITE_PRIVATE   TriggerStep *sqlite3TriggerInsertStep(Parse*,Token*, IdList*,
                                        Select*,u8,Upsert*,
                                        const char*,const char*);
SQLITE_PRIVATE   TriggerStep *sqlite3TriggerUpdateStep(Parse*,Token*,SrcList*,ExprList*,
                                        Expr*, u8, const char*,const char*);
SQLITE_PRIVATE   TriggerStep *sqlite3TriggerDeleteStep(Parse*,Token*, Expr*,
                                        const char*,const char*);
SQLITE_PRIVATE   void sqlite3DeleteTrigger(sqlite3*, Trigger*);
SQLITE_PRIVATE   void sqlite3UnlinkAndDeleteTrigger(sqlite3*,int,const char*);
SQLITE_PRIVATE   u32 sqlite3TriggerColmask(Parse*,Trigger*,ExprList*,int,int,Table*,int);
SQLITE_PRIVATE   SrcList *sqlite3TriggerStepSrc(Parse*, TriggerStep*);
# define sqlite3ParseToplevel(p) ((p)->pToplevel ? (p)->pToplevel : (p))
# define sqlite3IsToplevel(p) ((p)->pToplevel==0)
#else
# define sqlite3TriggersExist(B,C,D,E,F) 0
# define sqlite3DeleteTrigger(A,B)
# define sqlite3DropTriggerPtr(A,B)
# define sqlite3UnlinkAndDeleteTrigger(A,B,C)
# define sqlite3CodeRowTrigger(A,B,C,D,E,F,G,H,I)
# define sqlite3CodeRowTriggerDirect(A,B,C,D,E,F)
# define sqlite3TriggerList(X, Y) 0
# define sqlite3ParseToplevel(p) p
# define sqlite3IsToplevel(p) 1
# define sqlite3TriggerColmask(A,B,C,D,E,F,G) 0
# define sqlite3TriggerStepSrc(A,B) 0
#endif

SQLITE_PRIVATE int sqlite3JoinType(Parse*, Token*, Token*, Token*);
SQLITE_PRIVATE int sqlite3ColumnIndex(Table *pTab, const char *zCol);
SQLITE_PRIVATE void sqlite3SrcItemColumnUsed(SrcItem*,int);
SQLITE_PRIVATE void sqlite3SetJoinExpr(Expr*,int,u32);
SQLITE_PRIVATE void sqlite3CreateForeignKey(Parse*, ExprList*, Token*, ExprList*, int);
SQLITE_PRIVATE void sqlite3DeferForeignKey(Parse*, int);
#ifndef SQLITE_OMIT_AUTHORIZATION
SQLITE_PRIVATE   void sqlite3AuthRead(Parse*,Expr*,Schema*,SrcList*);
SQLITE_PRIVATE   int sqlite3AuthCheck(Parse*,int, const char*, const char*, const char*);
SQLITE_PRIVATE   void sqlite3AuthContextPush(Parse*, AuthContext*, const char*);
SQLITE_PRIVATE   void sqlite3AuthContextPop(AuthContext*);
SQLITE_PRIVATE   int sqlite3AuthReadCol(Parse*, const char *, const char *, int);
#else
# define sqlite3AuthRead(a,b,c,d)
# define sqlite3AuthCheck(a,b,c,d,e)    SQLITE_OK
# define sqlite3AuthContextPush(a,b,c)
# define sqlite3AuthContextPop(a)  ((void)(a))
#endif
SQLITE_PRIVATE int sqlite3DbIsNamed(sqlite3 *db, int iDb, const char *zName);
SQLITE_PRIVATE void sqlite3Attach(Parse*, Expr*, Expr*, Expr*);
SQLITE_PRIVATE void sqlite3Detach(Parse*, Expr*);
SQLITE_PRIVATE void sqlite3FixInit(DbFixer*, Parse*, int, const char*, const Token*);
SQLITE_PRIVATE int sqlite3FixSrcList(DbFixer*, SrcList*);
SQLITE_PRIVATE int sqlite3FixSelect(DbFixer*, Select*);
SQLITE_PRIVATE int sqlite3FixExpr(DbFixer*, Expr*);
