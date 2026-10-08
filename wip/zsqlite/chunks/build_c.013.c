SQLITE_PRIVATE void sqlite3CodeVerifySchema(Parse *pParse, int iDb){
  sqlite3CodeVerifySchemaAtToplevel(sqlite3ParseToplevel(pParse), iDb);
}


/*
** If argument zDb is NULL, then call sqlite3CodeVerifySchema() for each
** attached database. Otherwise, invoke it for the database named zDb only.
*/
SQLITE_PRIVATE void sqlite3CodeVerifyNamedSchema(Parse *pParse, const char *zDb){
  sqlite3 *db = pParse->db;
  int i;
  for(i=0; i<db->nDb; i++){
    Db *pDb = &db->aDb[i];
    if( pDb->pBt && (!zDb || 0==sqlite3StrICmp(zDb, pDb->zDbSName)) ){
      sqlite3CodeVerifySchema(pParse, i);
    }
  }
}

/*
** Generate VDBE code that prepares for doing an operation that
** might change the database.
**
** This routine starts a new transaction if we are not already within
** a transaction.  If we are already within a transaction, then a checkpoint
** is set if the setStatement parameter is true.  A checkpoint should
** be set for operations that might fail (due to a constraint) part of
** the way through and which will need to undo some writes without having to
** rollback the whole transaction.  For operations where all constraints
** can be checked before any changes are made to the database, it is never
** necessary to undo a write and the checkpoint should not be set.
*/
SQLITE_PRIVATE void sqlite3BeginWriteOperation(Parse *pParse, int setStatement, int iDb){
  Parse *pToplevel = sqlite3ParseToplevel(pParse);
  sqlite3CodeVerifySchemaAtToplevel(pToplevel, iDb);
  DbMaskSet(pToplevel->writeMask, iDb);
  pToplevel->isMultiWrite |= setStatement;
}

/*
** Indicate that the statement currently under construction might write
** more than one entry (example: deleting one row then inserting another,
** inserting multiple rows in a table, or inserting a row and index entries.)
** If an abort occurs after some of these writes have completed, then it will
** be necessary to undo the completed writes.
*/
SQLITE_PRIVATE void sqlite3MultiWrite(Parse *pParse){
  Parse *pToplevel = sqlite3ParseToplevel(pParse);
  pToplevel->isMultiWrite = 1;
}

/*
** The code generator calls this routine if is discovers that it is
** possible to abort a statement prior to completion.  In order to
** perform this abort without corrupting the database, we need to make
** sure that the statement is protected by a statement transaction.
**
** Technically, we only need to set the mayAbort flag if the
** isMultiWrite flag was previously set.  There is a time dependency
** such that the abort must occur after the multiwrite.  This makes
** some statements involving the REPLACE conflict resolution algorithm
** go a little faster.  But taking advantage of this time dependency
** makes it more difficult to prove that the code is correct (in
** particular, it prevents us from writing an effective
** implementation of sqlite3AssertMayAbort()) and so we have chosen
** to take the safe route and skip the optimization.
*/
SQLITE_PRIVATE void sqlite3MayAbort(Parse *pParse){
  Parse *pToplevel = sqlite3ParseToplevel(pParse);
  pToplevel->mayAbort = 1;
}

/*
** Code an OP_Halt that causes the vdbe to return an SQLITE_CONSTRAINT
** error. The onError parameter determines which (if any) of the statement
** and/or current transaction is rolled back.
*/
SQLITE_PRIVATE void sqlite3HaltConstraint(
  Parse *pParse,    /* Parsing context */
  int errCode,      /* extended error code */
  int onError,      /* Constraint type */
  char *p4,         /* Error message */
  i8 p4type,        /* P4_STATIC or P4_TRANSIENT */
  u8 p5Errmsg       /* P5_ErrMsg type */
){
  Vdbe *v;
  assert( pParse->pVdbe!=0 );
  v = sqlite3GetVdbe(pParse);
  assert( (errCode&0xff)==SQLITE_CONSTRAINT || pParse->nested );
  if( onError==OE_Abort ){
    sqlite3MayAbort(pParse);
  }
  sqlite3VdbeAddOp4(v, OP_Halt, errCode, onError, 0, p4, p4type);
  sqlite3VdbeChangeP5(v, p5Errmsg);
}

/*
** Code an OP_Halt due to UNIQUE or PRIMARY KEY constraint violation.
*/
SQLITE_PRIVATE void sqlite3UniqueConstraint(
  Parse *pParse,    /* Parsing context */
  int onError,      /* Constraint type */
  Index *pIdx       /* The index that triggers the constraint */
){
  char *zErr;
  int j;
  StrAccum errMsg;
  Table *pTab = pIdx->pTable;

  sqlite3StrAccumInit(&errMsg, pParse->db, 0, 0,
                      pParse->db->aLimit[SQLITE_LIMIT_LENGTH]);
  if( pIdx->aColExpr ){
    sqlite3_str_appendf(&errMsg, "index '%q'", pIdx->zName);
  }else{
    for(j=0; j<pIdx->nKeyCol; j++){
      char *zCol;
      assert( pIdx->aiColumn[j]>=0 );
      zCol = pTab->aCol[pIdx->aiColumn[j]].zCnName;
      if( j ) sqlite3_str_append(&errMsg, ", ", 2);
      sqlite3_str_appendall(&errMsg, pTab->zName);
      sqlite3_str_append(&errMsg, ".", 1);
      sqlite3_str_appendall(&errMsg, zCol);
    }
  }
  zErr = sqlite3StrAccumFinish(&errMsg);
  sqlite3HaltConstraint(pParse,
    IsPrimaryKeyIndex(pIdx) ? SQLITE_CONSTRAINT_PRIMARYKEY
                            : SQLITE_CONSTRAINT_UNIQUE,
    onError, zErr, P4_DYNAMIC, P5_ConstraintUnique);
}


/*
** Code an OP_Halt due to non-unique rowid.
*/
SQLITE_PRIVATE void sqlite3RowidConstraint(
  Parse *pParse,    /* Parsing context */
  int onError,      /* Conflict resolution algorithm */
  Table *pTab       /* The table with the non-unique rowid */
){
  char *zMsg;
  int rc;
  if( pTab->iPKey>=0 ){
    zMsg = sqlite3MPrintf(pParse->db, "%s.%s", pTab->zName,
                          pTab->aCol[pTab->iPKey].zCnName);
    rc = SQLITE_CONSTRAINT_PRIMARYKEY;
  }else{
    zMsg = sqlite3MPrintf(pParse->db, "%s.rowid", pTab->zName);
    rc = SQLITE_CONSTRAINT_ROWID;
  }
  sqlite3HaltConstraint(pParse, rc, onError, zMsg, P4_DYNAMIC,
                        P5_ConstraintUnique);
}

/*
** Check to see if pIndex uses the collating sequence pColl.  Return
** true if it does and false if it does not.
*/
#ifndef SQLITE_OMIT_REINDEX
static int collationMatch(const char *zColl, Index *pIndex){
  int i;
  assert( zColl!=0 );
  for(i=0; i<pIndex->nColumn; i++){
    const char *z = pIndex->azColl[i];
    assert( z!=0 || pIndex->aiColumn[i]<0 );
    if( pIndex->aiColumn[i]>=0 && 0==sqlite3StrICmp(z, zColl) ){
      return 1;
    }
  }
  return 0;
}
#endif

/*
** Recompute all indices of pTab that use the collating sequence pColl.
** If pColl==0 then recompute all indices of pTab.
*/
#ifndef SQLITE_OMIT_REINDEX
static void reindexTable(Parse *pParse, Table *pTab, char const *zColl){
  if( !IsVirtual(pTab) ){
    Index *pIndex;              /* An index associated with pTab */

    for(pIndex=pTab->pIndex; pIndex; pIndex=pIndex->pNext){
      if( zColl==0 || collationMatch(zColl, pIndex) ){
        int iDb = sqlite3SchemaToIndex(pParse->db, pTab->pSchema);
        sqlite3BeginWriteOperation(pParse, 0, iDb);
        sqlite3RefillIndex(pParse, pIndex, -1);
      }
    }
  }
}
#endif

/*
** Recompute all indices of all tables in all databases where the
** indices use the collating sequence pColl.  If pColl==0 then recompute
** all indices everywhere.
*/
#ifndef SQLITE_OMIT_REINDEX
static void reindexDatabases(Parse *pParse, char const *zColl){
  Db *pDb;                    /* A single database */
  int iDb;                    /* The database index number */
  sqlite3 *db = pParse->db;   /* The database connection */
  HashElem *k;                /* For looping over tables in pDb */
  Table *pTab;                /* A table in the database */

  assert( sqlite3BtreeHoldsAllMutexes(db) );  /* Needed for schema access */
  for(iDb=0, pDb=db->aDb; iDb<db->nDb; iDb++, pDb++){
    assert( pDb!=0 );
    for(k=sqliteHashFirst(&pDb->pSchema->tblHash);  k; k=sqliteHashNext(k)){
      pTab = (Table*)sqliteHashData(k);
      reindexTable(pParse, pTab, zColl);
    }
  }
}
#endif

/*
** Generate code for the REINDEX command.
**
**        REINDEX                            -- 1
**        REINDEX  <collation>               -- 2
**        REINDEX  ?<database>.?<tablename>  -- 3
**        REINDEX  ?<database>.?<indexname>  -- 4
**
** Form 1 causes all indices in all attached databases to be rebuilt.
** Form 2 rebuilds all indices in all databases that use the named
** collating function.  Forms 3 and 4 rebuild the named index or all
** indices associated with the named table.
*/
#ifndef SQLITE_OMIT_REINDEX
SQLITE_PRIVATE void sqlite3Reindex(Parse *pParse, Token *pName1, Token *pName2){
  CollSeq *pColl;             /* Collating sequence to be reindexed, or NULL */
  char *z;                    /* Name of a table or index */
  const char *zDb;            /* Name of the database */
  Table *pTab;                /* A table in the database */
  Index *pIndex;              /* An index associated with pTab */
  int iDb;                    /* The database index number */
  sqlite3 *db = pParse->db;   /* The database connection */
  Token *pObjName;            /* Name of the table or index to be reindexed */

  /* Read the database schema. If an error occurs, leave an error message
  ** and code in pParse and return NULL. */
  if( SQLITE_OK!=sqlite3ReadSchema(pParse) ){
    return;
  }

  if( pName1==0 ){
    reindexDatabases(pParse, 0);
    return;
  }else if( NEVER(pName2==0) || pName2->z==0 ){
    char *zColl;
    assert( pName1->z );
    zColl = sqlite3NameFromToken(pParse->db, pName1);
    if( !zColl ) return;
    pColl = sqlite3FindCollSeq(db, ENC(db), zColl, 0);
    if( pColl ){
      reindexDatabases(pParse, zColl);
      sqlite3DbFree(db, zColl);
      return;
    }
    sqlite3DbFree(db, zColl);
  }
  iDb = sqlite3TwoPartName(pParse, pName1, pName2, &pObjName);
  if( iDb<0 ) return;
  z = sqlite3NameFromToken(db, pObjName);
  if( z==0 ) return;
  zDb = pName2->n ? db->aDb[iDb].zDbSName : 0;
  pTab = sqlite3FindTable(db, z, zDb);
  if( pTab ){
    reindexTable(pParse, pTab, 0);
    sqlite3DbFree(db, z);
    return;
  }
  pIndex = sqlite3FindIndex(db, z, zDb);
  sqlite3DbFree(db, z);
  if( pIndex ){
    iDb = sqlite3SchemaToIndex(db, pIndex->pTable->pSchema);
    sqlite3BeginWriteOperation(pParse, 0, iDb);
    sqlite3RefillIndex(pParse, pIndex, -1);
    return;
  }
  sqlite3ErrorMsg(pParse, "unable to identify the object to be reindexed");
}
#endif

/*
** Return a KeyInfo structure that is appropriate for the given Index.
**
** The caller should invoke sqlite3KeyInfoUnref() on the returned object
** when it has finished using it.
*/
SQLITE_PRIVATE KeyInfo *sqlite3KeyInfoOfIndex(Parse *pParse, Index *pIdx){
  int i;
  int nCol = pIdx->nColumn;
  int nKey = pIdx->nKeyCol;
  KeyInfo *pKey;
  if( pParse->nErr ) return 0;
  if( pIdx->uniqNotNull ){
    pKey = sqlite3KeyInfoAlloc(pParse->db, nKey, nCol-nKey);
  }else{
    pKey = sqlite3KeyInfoAlloc(pParse->db, nCol, 0);
  }
  if( pKey ){
    assert( sqlite3KeyInfoIsWriteable(pKey) );
    for(i=0; i<nCol; i++){
      const char *zColl = pIdx->azColl[i];
      pKey->aColl[i] = zColl==sqlite3StrBINARY ? 0 :
                        sqlite3LocateCollSeq(pParse, zColl);
      pKey->aSortFlags[i] = pIdx->aSortOrder[i];
      assert( 0==(pKey->aSortFlags[i] & KEYINFO_ORDER_BIGNULL) );
    }
    if( pParse->nErr ){
      assert( pParse->rc==SQLITE_ERROR_MISSING_COLLSEQ );
      if( pIdx->bNoQuery==0 ){
        /* Deactivate the index because it contains an unknown collating
        ** sequence.  The only way to reactive the index is to reload the
        ** schema.  Adding the missing collating sequence later does not
        ** reactive the index.  The application had the chance to register
        ** the missing index using the collation-needed callback.  For
        ** simplicity, SQLite will not give the application a second chance.
        */
        pIdx->bNoQuery = 1;
        pParse->rc = SQLITE_ERROR_RETRY;
      }
      sqlite3KeyInfoUnref(pKey);
      pKey = 0;
    }
  }
  return pKey;
}

#ifndef SQLITE_OMIT_CTE
/*
** Create a new CTE object
*/
SQLITE_PRIVATE Cte *sqlite3CteNew(
  Parse *pParse,          /* Parsing context */
  Token *pName,           /* Name of the common-table */
  ExprList *pArglist,     /* Optional column name list for the table */
  Select *pQuery,         /* Query used to initialize the table */
  u8 eM10d                /* The MATERIALIZED flag */
){
  Cte *pNew;
  sqlite3 *db = pParse->db;

  pNew = sqlite3DbMallocZero(db, sizeof(*pNew));
  assert( pNew!=0 || db->mallocFailed );

  if( db->mallocFailed ){
    sqlite3ExprListDelete(db, pArglist);
    sqlite3SelectDelete(db, pQuery);
  }else{
    pNew->pSelect = pQuery;
    pNew->pCols = pArglist;
    pNew->zName = sqlite3NameFromToken(pParse->db, pName);
    pNew->eM10d = eM10d;
  }
  return pNew;
}
