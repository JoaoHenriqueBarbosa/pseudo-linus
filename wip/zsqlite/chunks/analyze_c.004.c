
/*
** Look up an index by name.  Or, if the name of a WITHOUT ROWID table
** is supplied instead, find the PRIMARY KEY index for that table.
*/
static Index *findIndexOrPrimaryKey(
  sqlite3 *db,
  const char *zName,
  const char *zDb
){
  Index *pIdx = sqlite3FindIndex(db, zName, zDb);
  if( pIdx==0 ){
    Table *pTab = sqlite3FindTable(db, zName, zDb);
    if( pTab && !HasRowid(pTab) ) pIdx = sqlite3PrimaryKeyIndex(pTab);
  }
  return pIdx;
}

/*
** Load the content from either the sqlite_stat4
** into the relevant Index.aSample[] arrays.
**
** Arguments zSql1 and zSql2 must point to SQL statements that return
** data equivalent to the following:
**
**    zSql1: SELECT idx,count(*) FROM %Q.sqlite_stat4 GROUP BY idx
**    zSql2: SELECT idx,neq,nlt,ndlt,sample FROM %Q.sqlite_stat4
**
** where %Q is replaced with the database name before the SQL is executed.
*/
static int loadStatTbl(
  sqlite3 *db,                  /* Database handle */
  const char *zSql1,            /* SQL statement 1 (see above) */
  const char *zSql2,            /* SQL statement 2 (see above) */
  const char *zDb               /* Database name (e.g. "main") */
){
  int rc;                       /* Result codes from subroutines */
  sqlite3_stmt *pStmt = 0;      /* An SQL statement being run */
  char *zSql;                   /* Text of the SQL statement */
  Index *pPrevIdx = 0;          /* Previous index in the loop */
  IndexSample *pSample;         /* A slot in pIdx->aSample[] */

  assert( db->lookaside.bDisable );
  zSql = sqlite3MPrintf(db, zSql1, zDb);
  if( !zSql ){
    return SQLITE_NOMEM_BKPT;
  }
  rc = sqlite3_prepare(db, zSql, -1, &pStmt, 0);
  sqlite3DbFree(db, zSql);
  if( rc ) return rc;

  while( sqlite3_step(pStmt)==SQLITE_ROW ){
    int nIdxCol = 1;              /* Number of columns in stat4 records */

    char *zIndex;   /* Index name */
    Index *pIdx;    /* Pointer to the index object */
    int nSample;    /* Number of samples */
    int nByte;      /* Bytes of space required */
    int i;          /* Bytes of space required */
    tRowcnt *pSpace;

    zIndex = (char *)sqlite3_column_text(pStmt, 0);
    if( zIndex==0 ) continue;
    nSample = sqlite3_column_int(pStmt, 1);
    pIdx = findIndexOrPrimaryKey(db, zIndex, zDb);
    assert( pIdx==0 || pIdx->nSample==0 );
    if( pIdx==0 ) continue;
    if( pIdx->aSample!=0 ){
      /* The same index appears in sqlite_stat4 under multiple names */
      continue;
    }
    assert( !HasRowid(pIdx->pTable) || pIdx->nColumn==pIdx->nKeyCol+1 );
    if( !HasRowid(pIdx->pTable) && IsPrimaryKeyIndex(pIdx) ){
      nIdxCol = pIdx->nKeyCol;
    }else{
      nIdxCol = pIdx->nColumn;
    }
    pIdx->nSampleCol = nIdxCol;
    pIdx->mxSample = nSample;
    nByte = sizeof(IndexSample) * nSample;
    nByte += sizeof(tRowcnt) * nIdxCol * 3 * nSample;
    nByte += nIdxCol * sizeof(tRowcnt);     /* Space for Index.aAvgEq[] */

    pIdx->aSample = sqlite3DbMallocZero(db, nByte);
    if( pIdx->aSample==0 ){
      sqlite3_finalize(pStmt);
      return SQLITE_NOMEM_BKPT;
    }
    pSpace = (tRowcnt*)&pIdx->aSample[nSample];
    pIdx->aAvgEq = pSpace; pSpace += nIdxCol;
    pIdx->pTable->tabFlags |= TF_HasStat4;
    for(i=0; i<nSample; i++){
      pIdx->aSample[i].anEq = pSpace; pSpace += nIdxCol;
      pIdx->aSample[i].anLt = pSpace; pSpace += nIdxCol;
      pIdx->aSample[i].anDLt = pSpace; pSpace += nIdxCol;
    }
    assert( ((u8*)pSpace)-nByte==(u8*)(pIdx->aSample) );
  }
  rc = sqlite3_finalize(pStmt);
  if( rc ) return rc;

  zSql = sqlite3MPrintf(db, zSql2, zDb);
  if( !zSql ){
    return SQLITE_NOMEM_BKPT;
  }
  rc = sqlite3_prepare(db, zSql, -1, &pStmt, 0);
  sqlite3DbFree(db, zSql);
  if( rc ) return rc;

  while( sqlite3_step(pStmt)==SQLITE_ROW ){
    char *zIndex;                 /* Index name */
    Index *pIdx;                  /* Pointer to the index object */
    int nCol = 1;                 /* Number of columns in index */

    zIndex = (char *)sqlite3_column_text(pStmt, 0);
    if( zIndex==0 ) continue;
    pIdx = findIndexOrPrimaryKey(db, zIndex, zDb);
    if( pIdx==0 ) continue;
    if( pIdx->nSample>=pIdx->mxSample ){
      /* Too many slots used because the same index appears in
      ** sqlite_stat4 using multiple names */
      continue;
    }
    /* This next condition is true if data has already been loaded from
    ** the sqlite_stat4 table. */
    nCol = pIdx->nSampleCol;
    if( pIdx!=pPrevIdx ){
      initAvgEq(pPrevIdx);
      pPrevIdx = pIdx;
    }
    pSample = &pIdx->aSample[pIdx->nSample];
    decodeIntArray((char*)sqlite3_column_text(pStmt,1),nCol,pSample->anEq,0,0);
    decodeIntArray((char*)sqlite3_column_text(pStmt,2),nCol,pSample->anLt,0,0);
    decodeIntArray((char*)sqlite3_column_text(pStmt,3),nCol,pSample->anDLt,0,0);

    /* Take a copy of the sample. Add 8 extra 0x00 bytes the end of the buffer.
    ** This is in case the sample record is corrupted. In that case, the
    ** sqlite3VdbeRecordCompare() may read up to two varints past the
    ** end of the allocated buffer before it realizes it is dealing with
    ** a corrupt record.  Or it might try to read a large integer from the
    ** buffer.  In any case, eight 0x00 bytes prevents this from causing
    ** a buffer overread.  */
    pSample->n = sqlite3_column_bytes(pStmt, 4);
    pSample->p = sqlite3DbMallocZero(db, pSample->n + 8);
    if( pSample->p==0 ){
      sqlite3_finalize(pStmt);
      return SQLITE_NOMEM_BKPT;
    }
    if( pSample->n ){
      memcpy(pSample->p, sqlite3_column_blob(pStmt, 4), pSample->n);
    }
    pIdx->nSample++;
  }
  rc = sqlite3_finalize(pStmt);
  if( rc==SQLITE_OK ) initAvgEq(pPrevIdx);
  return rc;
}

/*
** Load content from the sqlite_stat4 table into
** the Index.aSample[] arrays of all indices.
*/
static int loadStat4(sqlite3 *db, const char *zDb){
  int rc = SQLITE_OK;             /* Result codes from subroutines */
  const Table *pStat4;

  assert( db->lookaside.bDisable );
  if( OptimizationEnabled(db, SQLITE_Stat4)
   && (pStat4 = sqlite3FindTable(db, "sqlite_stat4", zDb))!=0
   && IsOrdinaryTable(pStat4)
  ){
    rc = loadStatTbl(db,
      "SELECT idx,count(*) FROM %Q.sqlite_stat4 GROUP BY idx COLLATE nocase",
      "SELECT idx,neq,nlt,ndlt,sample FROM %Q.sqlite_stat4",
      zDb
    );
  }
  return rc;
}
#endif /* SQLITE_ENABLE_STAT4 */

/*
** Load the content of the sqlite_stat1 and sqlite_stat4 tables. The
** contents of sqlite_stat1 are used to populate the Index.aiRowEst[]
** arrays. The contents of sqlite_stat4 are used to populate the
** Index.aSample[] arrays.
**
** If the sqlite_stat1 table is not present in the database, SQLITE_ERROR
** is returned. In this case, even if SQLITE_ENABLE_STAT4 was defined
** during compilation and the sqlite_stat4 table is present, no data is
** read from it.
**
** If SQLITE_ENABLE_STAT4 was defined during compilation and the
** sqlite_stat4 table is not present in the database, SQLITE_ERROR is
** returned. However, in this case, data is read from the sqlite_stat1
** table (if it is present) before returning.
**
** If an OOM error occurs, this function always sets db->mallocFailed.
** This means if the caller does not care about other errors, the return
** code may be ignored.
*/
SQLITE_PRIVATE int sqlite3AnalysisLoad(sqlite3 *db, int iDb){
  analysisInfo sInfo;
  HashElem *i;
  char *zSql;
  int rc = SQLITE_OK;
  Schema *pSchema = db->aDb[iDb].pSchema;
  const Table *pStat1;

  assert( iDb>=0 && iDb<db->nDb );
  assert( db->aDb[iDb].pBt!=0 );

  /* Clear any prior statistics */
  assert( sqlite3SchemaMutexHeld(db, iDb, 0) );
  for(i=sqliteHashFirst(&pSchema->tblHash); i; i=sqliteHashNext(i)){
    Table *pTab = sqliteHashData(i);
    pTab->tabFlags &= ~TF_HasStat1;
  }
  for(i=sqliteHashFirst(&pSchema->idxHash); i; i=sqliteHashNext(i)){
    Index *pIdx = sqliteHashData(i);
    pIdx->hasStat1 = 0;
#ifdef SQLITE_ENABLE_STAT4
    sqlite3DeleteIndexSamples(db, pIdx);
    pIdx->aSample = 0;
#endif
  }

  /* Load new statistics out of the sqlite_stat1 table */
  sInfo.db = db;
  sInfo.zDatabase = db->aDb[iDb].zDbSName;
  if( (pStat1 = sqlite3FindTable(db, "sqlite_stat1", sInfo.zDatabase))
   && IsOrdinaryTable(pStat1)
  ){
    zSql = sqlite3MPrintf(db,
        "SELECT tbl,idx,stat FROM %Q.sqlite_stat1", sInfo.zDatabase);
    if( zSql==0 ){
      rc = SQLITE_NOMEM_BKPT;
    }else{
      rc = sqlite3_exec(db, zSql, analysisLoader, &sInfo, 0);
      sqlite3DbFree(db, zSql);
    }
  }

  /* Set appropriate defaults on all indexes not in the sqlite_stat1 table */
  assert( sqlite3SchemaMutexHeld(db, iDb, 0) );
  for(i=sqliteHashFirst(&pSchema->idxHash); i; i=sqliteHashNext(i)){
    Index *pIdx = sqliteHashData(i);
    if( !pIdx->hasStat1 ) sqlite3DefaultRowEst(pIdx);
  }

  /* Load the statistics from the sqlite_stat4 table. */
#ifdef SQLITE_ENABLE_STAT4
  if( rc==SQLITE_OK ){
    DisableLookaside;
    rc = loadStat4(db, sInfo.zDatabase);
    EnableLookaside;
  }
  for(i=sqliteHashFirst(&pSchema->idxHash); i; i=sqliteHashNext(i)){
    Index *pIdx = sqliteHashData(i);
    sqlite3_free(pIdx->aiRowEst);
    pIdx->aiRowEst = 0;
  }
#endif

  if( rc==SQLITE_NOMEM ){
    sqlite3OomFault(db);
  }
  return rc;
}


#endif /* SQLITE_OMIT_ANALYZE */

