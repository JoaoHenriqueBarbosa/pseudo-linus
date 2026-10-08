
/*
** This routine is called when an INITIALLY IMMEDIATE or INITIALLY DEFERRED
** clause is seen as part of a foreign key definition.  The isDeferred
** parameter is 1 for INITIALLY DEFERRED and 0 for INITIALLY IMMEDIATE.
** The behavior of the most recently created foreign key is adjusted
** accordingly.
*/
SQLITE_PRIVATE void sqlite3DeferForeignKey(Parse *pParse, int isDeferred){
#ifndef SQLITE_OMIT_FOREIGN_KEY
  Table *pTab;
  FKey *pFKey;
  if( (pTab = pParse->pNewTable)==0 ) return;
  if( NEVER(!IsOrdinaryTable(pTab)) ) return;
  if( (pFKey = pTab->u.tab.pFKey)==0 ) return;
  assert( isDeferred==0 || isDeferred==1 ); /* EV: R-30323-21917 */
  pFKey->isDeferred = (u8)isDeferred;
#endif
}

/*
** Generate code that will erase and refill index *pIdx.  This is
** used to initialize a newly created index or to recompute the
** content of an index in response to a REINDEX command.
**
** if memRootPage is not negative, it means that the index is newly
** created.  The register specified by memRootPage contains the
** root page number of the index.  If memRootPage is negative, then
** the index already exists and must be cleared before being refilled and
** the root page number of the index is taken from pIndex->tnum.
*/
static void sqlite3RefillIndex(Parse *pParse, Index *pIndex, int memRootPage){
  Table *pTab = pIndex->pTable;  /* The table that is indexed */
  int iTab = pParse->nTab++;     /* Btree cursor used for pTab */
  int iIdx = pParse->nTab++;     /* Btree cursor used for pIndex */
  int iSorter;                   /* Cursor opened by OpenSorter (if in use) */
  int addr1;                     /* Address of top of loop */
  int addr2;                     /* Address to jump to for next iteration */
  Pgno tnum;                     /* Root page of index */
  int iPartIdxLabel;             /* Jump to this label to skip a row */
  Vdbe *v;                       /* Generate code into this virtual machine */
  KeyInfo *pKey;                 /* KeyInfo for index */
  int regRecord;                 /* Register holding assembled index record */
  sqlite3 *db = pParse->db;      /* The database connection */
  int iDb = sqlite3SchemaToIndex(db, pIndex->pSchema);

#ifndef SQLITE_OMIT_AUTHORIZATION
  if( sqlite3AuthCheck(pParse, SQLITE_REINDEX, pIndex->zName, 0,
      db->aDb[iDb].zDbSName ) ){
    return;
  }
#endif

  /* Require a write-lock on the table to perform this operation */
  sqlite3TableLock(pParse, iDb, pTab->tnum, 1, pTab->zName);

  v = sqlite3GetVdbe(pParse);
  if( v==0 ) return;
  if( memRootPage>=0 ){
    tnum = (Pgno)memRootPage;
  }else{
    tnum = pIndex->tnum;
  }
  pKey = sqlite3KeyInfoOfIndex(pParse, pIndex);
  assert( pKey!=0 || pParse->nErr );

  /* Open the sorter cursor if we are to use one. */
  iSorter = pParse->nTab++;
  sqlite3VdbeAddOp4(v, OP_SorterOpen, iSorter, 0, pIndex->nKeyCol, (char*)
                    sqlite3KeyInfoRef(pKey), P4_KEYINFO);

  /* Open the table. Loop through all rows of the table, inserting index
  ** records into the sorter. */
  sqlite3OpenTable(pParse, iTab, iDb, pTab, OP_OpenRead);
  addr1 = sqlite3VdbeAddOp2(v, OP_Rewind, iTab, 0); VdbeCoverage(v);
  regRecord = sqlite3GetTempReg(pParse);
  sqlite3MultiWrite(pParse);

  sqlite3GenerateIndexKey(pParse,pIndex,iTab,regRecord,0,&iPartIdxLabel,0,0);
  sqlite3VdbeAddOp2(v, OP_SorterInsert, iSorter, regRecord);
  sqlite3ResolvePartIdxLabel(pParse, iPartIdxLabel);
  sqlite3VdbeAddOp2(v, OP_Next, iTab, addr1+1); VdbeCoverage(v);
  sqlite3VdbeJumpHere(v, addr1);
  if( memRootPage<0 ) sqlite3VdbeAddOp2(v, OP_Clear, tnum, iDb);
  sqlite3VdbeAddOp4(v, OP_OpenWrite, iIdx, (int)tnum, iDb,
                    (char *)pKey, P4_KEYINFO);
  sqlite3VdbeChangeP5(v, OPFLAG_BULKCSR|((memRootPage>=0)?OPFLAG_P2ISREG:0));

  addr1 = sqlite3VdbeAddOp2(v, OP_SorterSort, iSorter, 0); VdbeCoverage(v);
  if( IsUniqueIndex(pIndex) ){
    int j2 = sqlite3VdbeGoto(v, 1);
    addr2 = sqlite3VdbeCurrentAddr(v);
    sqlite3VdbeVerifyAbortable(v, OE_Abort);
    sqlite3VdbeAddOp4Int(v, OP_SorterCompare, iSorter, j2, regRecord,
                         pIndex->nKeyCol); VdbeCoverage(v);
    sqlite3UniqueConstraint(pParse, OE_Abort, pIndex);
    sqlite3VdbeJumpHere(v, j2);
  }else{
    /* Most CREATE INDEX and REINDEX statements that are not UNIQUE can not
    ** abort. The exception is if one of the indexed expressions contains a
    ** user function that throws an exception when it is evaluated. But the
    ** overhead of adding a statement journal to a CREATE INDEX statement is
    ** very small (since most of the pages written do not contain content that
    ** needs to be restored if the statement aborts), so we call
    ** sqlite3MayAbort() for all CREATE INDEX statements.  */
    sqlite3MayAbort(pParse);
    addr2 = sqlite3VdbeCurrentAddr(v);
  }
  sqlite3VdbeAddOp3(v, OP_SorterData, iSorter, regRecord, iIdx);
  if( !pIndex->bAscKeyBug ){
    /* This OP_SeekEnd opcode makes index insert for a REINDEX go much
    ** faster by avoiding unnecessary seeks.  But the optimization does
    ** not work for UNIQUE constraint indexes on WITHOUT ROWID tables
    ** with DESC primary keys, since those indexes have there keys in
    ** a different order from the main table.
    ** See ticket: https://www.sqlite.org/src/info/bba7b69f9849b5bf
    */
    sqlite3VdbeAddOp1(v, OP_SeekEnd, iIdx);
  }
  sqlite3VdbeAddOp2(v, OP_IdxInsert, iIdx, regRecord);
  sqlite3VdbeChangeP5(v, OPFLAG_USESEEKRESULT);
  sqlite3ReleaseTempReg(pParse, regRecord);
  sqlite3VdbeAddOp2(v, OP_SorterNext, iSorter, addr2); VdbeCoverage(v);
  sqlite3VdbeJumpHere(v, addr1);

  sqlite3VdbeAddOp1(v, OP_Close, iTab);
  sqlite3VdbeAddOp1(v, OP_Close, iIdx);
  sqlite3VdbeAddOp1(v, OP_Close, iSorter);
}

/*
** Allocate heap space to hold an Index object with nCol columns.
**
** Increase the allocation size to provide an extra nExtra bytes
** of 8-byte aligned space after the Index object and return a
** pointer to this extra space in *ppExtra.
*/
SQLITE_PRIVATE Index *sqlite3AllocateIndexObject(
  sqlite3 *db,         /* Database connection */
  i16 nCol,            /* Total number of columns in the index */
  int nExtra,          /* Number of bytes of extra space to alloc */
  char **ppExtra       /* Pointer to the "extra" space */
){
  Index *p;            /* Allocated index object */
  int nByte;           /* Bytes of space for Index object + arrays */

  nByte = ROUND8(sizeof(Index)) +              /* Index structure  */
          ROUND8(sizeof(char*)*nCol) +         /* Index.azColl     */
          ROUND8(sizeof(LogEst)*(nCol+1) +     /* Index.aiRowLogEst   */
                 sizeof(i16)*nCol +            /* Index.aiColumn   */
                 sizeof(u8)*nCol);             /* Index.aSortOrder */
  p = sqlite3DbMallocZero(db, nByte + nExtra);
  if( p ){
    char *pExtra = ((char*)p)+ROUND8(sizeof(Index));
    p->azColl = (const char**)pExtra; pExtra += ROUND8(sizeof(char*)*nCol);
    p->aiRowLogEst = (LogEst*)pExtra; pExtra += sizeof(LogEst)*(nCol+1);
    p->aiColumn = (i16*)pExtra;       pExtra += sizeof(i16)*nCol;
    p->aSortOrder = (u8*)pExtra;
    p->nColumn = nCol;
    p->nKeyCol = nCol - 1;
    *ppExtra = ((char*)p) + nByte;
  }
  return p;
}

/*
** If expression list pList contains an expression that was parsed with
** an explicit "NULLS FIRST" or "NULLS LAST" clause, leave an error in
** pParse and return non-zero. Otherwise, return zero.
*/
SQLITE_PRIVATE int sqlite3HasExplicitNulls(Parse *pParse, ExprList *pList){
  if( pList ){
    int i;
    for(i=0; i<pList->nExpr; i++){
      if( pList->a[i].fg.bNulls ){
        u8 sf = pList->a[i].fg.sortFlags;
        sqlite3ErrorMsg(pParse, "unsupported use of NULLS %s",
            (sf==0 || sf==3) ? "FIRST" : "LAST"
        );
        return 1;
      }
    }
  }
  return 0;
}

/*
** Create a new index for an SQL table.  pName1.pName2 is the name of the index
** and pTblList is the name of the table that is to be indexed.  Both will
** be NULL for a primary key or an index that is created to satisfy a
** UNIQUE constraint.  If pTable and pIndex are NULL, use pParse->pNewTable
** as the table to be indexed.  pParse->pNewTable is a table that is
** currently being constructed by a CREATE TABLE statement.
**
** pList is a list of columns to be indexed.  pList will be NULL if this
** is a primary key or unique-constraint on the most recent column added
** to the table currently under construction.
*/
SQLITE_PRIVATE void sqlite3CreateIndex(
  Parse *pParse,     /* All information about this parse */
  Token *pName1,     /* First part of index name. May be NULL */
  Token *pName2,     /* Second part of index name. May be NULL */
  SrcList *pTblName, /* Table to index. Use pParse->pNewTable if 0 */
  ExprList *pList,   /* A list of columns to be indexed */
  int onError,       /* OE_Abort, OE_Ignore, OE_Replace, or OE_None */
  Token *pStart,     /* The CREATE token that begins this statement */
  Expr *pPIWhere,    /* WHERE clause for partial indices */
  int sortOrder,     /* Sort order of primary key when pList==NULL */
  int ifNotExist,    /* Omit error if index already exists */
  u8 idxType         /* The index type */
){
  Table *pTab = 0;     /* Table to be indexed */
  Index *pIndex = 0;   /* The index to be created */
  char *zName = 0;     /* Name of the index */
  int nName;           /* Number of characters in zName */
  int i, j;
  DbFixer sFix;        /* For assigning database names to pTable */
  int sortOrderMask;   /* 1 to honor DESC in index.  0 to ignore. */
  sqlite3 *db = pParse->db;
  Db *pDb;             /* The specific table containing the indexed database */
  int iDb;             /* Index of the database that is being written */
  Token *pName = 0;    /* Unqualified name of the index to create */
  struct ExprList_item *pListItem; /* For looping over pList */
  int nExtra = 0;                  /* Space allocated for zExtra[] */
  int nExtraCol;                   /* Number of extra columns needed */
  char *zExtra = 0;                /* Extra space after the Index object */
  Index *pPk = 0;      /* PRIMARY KEY index for WITHOUT ROWID tables */

  assert( db->pParse==pParse );
  if( pParse->nErr ){
    goto exit_create_index;
  }
  assert( db->mallocFailed==0 );
  if( IN_DECLARE_VTAB && idxType!=SQLITE_IDXTYPE_PRIMARYKEY ){
    goto exit_create_index;
  }
  if( SQLITE_OK!=sqlite3ReadSchema(pParse) ){
    goto exit_create_index;
  }
  if( sqlite3HasExplicitNulls(pParse, pList) ){
    goto exit_create_index;
  }

  /*
  ** Find the table that is to be indexed.  Return early if not found.
  */
  if( pTblName!=0 ){

    /* Use the two-part index name to determine the database
    ** to search for the table. 'Fix' the table name to this db
    ** before looking up the table.
    */
    assert( pName1 && pName2 );
    iDb = sqlite3TwoPartName(pParse, pName1, pName2, &pName);
    if( iDb<0 ) goto exit_create_index;
    assert( pName && pName->z );

#ifndef SQLITE_OMIT_TEMPDB
    /* If the index name was unqualified, check if the table
    ** is a temp table. If so, set the database to 1. Do not do this
    ** if initializing a database schema.
    */
    if( !db->init.busy ){
      pTab = sqlite3SrcListLookup(pParse, pTblName);
      if( pName2->n==0 && pTab && pTab->pSchema==db->aDb[1].pSchema ){
        iDb = 1;
      }
    }
#endif

    sqlite3FixInit(&sFix, pParse, iDb, "index", pName);
    if( sqlite3FixSrcList(&sFix, pTblName) ){
      /* Because the parser constructs pTblName from a single identifier,
      ** sqlite3FixSrcList can never fail. */
      assert(0);
    }
    pTab = sqlite3LocateTableItem(pParse, 0, &pTblName->a[0]);
    assert( db->mallocFailed==0 || pTab==0 );
    if( pTab==0 ) goto exit_create_index;
    if( iDb==1 && db->aDb[iDb].pSchema!=pTab->pSchema ){
      sqlite3ErrorMsg(pParse,
           "cannot create a TEMP index on non-TEMP table \"%s\"",
           pTab->zName);
      goto exit_create_index;
    }
    if( !HasRowid(pTab) ) pPk = sqlite3PrimaryKeyIndex(pTab);
  }else{
    assert( pName==0 );
    assert( pStart==0 );
    pTab = pParse->pNewTable;
    if( !pTab ) goto exit_create_index;
    iDb = sqlite3SchemaToIndex(db, pTab->pSchema);
  }
  pDb = &db->aDb[iDb];

  assert( pTab!=0 );
  if( sqlite3StrNICmp(pTab->zName, "sqlite_", 7)==0
       && db->init.busy==0
       && pTblName!=0
#if SQLITE_USER_AUTHENTICATION
       && sqlite3UserAuthTable(pTab->zName)==0
#endif
  ){
    sqlite3ErrorMsg(pParse, "table %s may not be indexed", pTab->zName);
    goto exit_create_index;
  }
#ifndef SQLITE_OMIT_VIEW
  if( IsView(pTab) ){
    sqlite3ErrorMsg(pParse, "views may not be indexed");
    goto exit_create_index;
  }
#endif
#ifndef SQLITE_OMIT_VIRTUALTABLE
  if( IsVirtual(pTab) ){
    sqlite3ErrorMsg(pParse, "virtual tables may not be indexed");
    goto exit_create_index;
  }
#endif

  /*
  ** Find the name of the index.  Make sure there is not already another
  ** index or table with the same name.
  **
  ** Exception:  If we are reading the names of permanent indices from the
  ** sqlite_schema table (because some other process changed the schema) and
  ** one of the index names collides with the name of a temporary table or
  ** index, then we will continue to process this index.
  **
  ** If pName==0 it means that we are
  ** dealing with a primary key or UNIQUE constraint.  We have to invent our
  ** own name.
  */
  if( pName ){
    zName = sqlite3NameFromToken(db, pName);
    if( zName==0 ) goto exit_create_index;
    assert( pName->z!=0 );
    if( SQLITE_OK!=sqlite3CheckObjectName(pParse, zName,"index",pTab->zName) ){
      goto exit_create_index;
    }
    if( !IN_RENAME_OBJECT ){
      if( !db->init.busy ){
        if( sqlite3FindTable(db, zName, pDb->zDbSName)!=0 ){
          sqlite3ErrorMsg(pParse, "there is already a table named %s", zName);
          goto exit_create_index;
        }
      }
      if( sqlite3FindIndex(db, zName, pDb->zDbSName)!=0 ){
        if( !ifNotExist ){
          sqlite3ErrorMsg(pParse, "index %s already exists", zName);
        }else{
