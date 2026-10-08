
/*
** Fill the Index.aiRowEst[] array with default information - information
** to be used when we have not run the ANALYZE command.
**
** aiRowEst[0] is supposed to contain the number of elements in the index.
** Since we do not know, guess 1 million.  aiRowEst[1] is an estimate of the
** number of rows in the table that match any particular value of the
** first column of the index.  aiRowEst[2] is an estimate of the number
** of rows that match any particular combination of the first 2 columns
** of the index.  And so forth.  It must always be the case that
*
**           aiRowEst[N]<=aiRowEst[N-1]
**           aiRowEst[N]>=1
**
** Apart from that, we have little to go on besides intuition as to
** how aiRowEst[] should be initialized.  The numbers generated here
** are based on typical values found in actual indices.
*/
SQLITE_PRIVATE void sqlite3DefaultRowEst(Index *pIdx){
               /*                10,  9,  8,  7,  6 */
  static const LogEst aVal[] = { 33, 32, 30, 28, 26 };
  LogEst *a = pIdx->aiRowLogEst;
  LogEst x;
  int nCopy = MIN(ArraySize(aVal), pIdx->nKeyCol);
  int i;

  /* Indexes with default row estimates should not have stat1 data */
  assert( !pIdx->hasStat1 );

  /* Set the first entry (number of rows in the index) to the estimated
  ** number of rows in the table, or half the number of rows in the table
  ** for a partial index.
  **
  ** 2020-05-27:  If some of the stat data is coming from the sqlite_stat1
  ** table but other parts we are having to guess at, then do not let the
  ** estimated number of rows in the table be less than 1000 (LogEst 99).
  ** Failure to do this can cause the indexes for which we do not have
  ** stat1 data to be ignored by the query planner.
  */
  x = pIdx->pTable->nRowLogEst;
  assert( 99==sqlite3LogEst(1000) );
  if( x<99 ){
    pIdx->pTable->nRowLogEst = x = 99;
  }
  if( pIdx->pPartIdxWhere!=0 ){ x -= 10;  assert( 10==sqlite3LogEst(2) ); }
  a[0] = x;

  /* Estimate that a[1] is 10, a[2] is 9, a[3] is 8, a[4] is 7, a[5] is
  ** 6 and each subsequent value (if any) is 5.  */
  memcpy(&a[1], aVal, nCopy*sizeof(LogEst));
  for(i=nCopy+1; i<=pIdx->nKeyCol; i++){
    a[i] = 23;                    assert( 23==sqlite3LogEst(5) );
  }

  assert( 0==sqlite3LogEst(1) );
  if( IsUniqueIndex(pIdx) ) a[pIdx->nKeyCol] = 0;
}

/*
** This routine will drop an existing named index.  This routine
** implements the DROP INDEX statement.
*/
SQLITE_PRIVATE void sqlite3DropIndex(Parse *pParse, SrcList *pName, int ifExists){
  Index *pIndex;
  Vdbe *v;
  sqlite3 *db = pParse->db;
  int iDb;

  if( db->mallocFailed ){
    goto exit_drop_index;
  }
  assert( pParse->nErr==0 );   /* Never called with prior non-OOM errors */
  assert( pName->nSrc==1 );
  if( SQLITE_OK!=sqlite3ReadSchema(pParse) ){
    goto exit_drop_index;
  }
  pIndex = sqlite3FindIndex(db, pName->a[0].zName, pName->a[0].zDatabase);
  if( pIndex==0 ){
    if( !ifExists ){
      sqlite3ErrorMsg(pParse, "no such index: %S", pName->a);
    }else{
      sqlite3CodeVerifyNamedSchema(pParse, pName->a[0].zDatabase);
      sqlite3ForceNotReadOnly(pParse);
    }
    pParse->checkSchema = 1;
    goto exit_drop_index;
  }
  if( pIndex->idxType!=SQLITE_IDXTYPE_APPDEF ){
    sqlite3ErrorMsg(pParse, "index associated with UNIQUE "
      "or PRIMARY KEY constraint cannot be dropped", 0);
    goto exit_drop_index;
  }
  iDb = sqlite3SchemaToIndex(db, pIndex->pSchema);
#ifndef SQLITE_OMIT_AUTHORIZATION
  {
    int code = SQLITE_DROP_INDEX;
    Table *pTab = pIndex->pTable;
    const char *zDb = db->aDb[iDb].zDbSName;
    const char *zTab = SCHEMA_TABLE(iDb);
    if( sqlite3AuthCheck(pParse, SQLITE_DELETE, zTab, 0, zDb) ){
      goto exit_drop_index;
    }
    if( !OMIT_TEMPDB && iDb==1 ) code = SQLITE_DROP_TEMP_INDEX;
    if( sqlite3AuthCheck(pParse, code, pIndex->zName, pTab->zName, zDb) ){
      goto exit_drop_index;
    }
  }
#endif

  /* Generate code to remove the index and from the schema table */
  v = sqlite3GetVdbe(pParse);
  if( v ){
    sqlite3BeginWriteOperation(pParse, 1, iDb);
    sqlite3NestedParse(pParse,
       "DELETE FROM %Q." LEGACY_SCHEMA_TABLE " WHERE name=%Q AND type='index'",
       db->aDb[iDb].zDbSName, pIndex->zName
    );
    sqlite3ClearStatTables(pParse, iDb, "idx", pIndex->zName);
    sqlite3ChangeCookie(pParse, iDb);
    destroyRootPage(pParse, pIndex->tnum, iDb);
    sqlite3VdbeAddOp4(v, OP_DropIndex, iDb, 0, 0, pIndex->zName, 0);
  }

exit_drop_index:
  sqlite3SrcListDelete(db, pName);
}

/*
** pArray is a pointer to an array of objects. Each object in the
** array is szEntry bytes in size. This routine uses sqlite3DbRealloc()
** to extend the array so that there is space for a new object at the end.
**
** When this function is called, *pnEntry contains the current size of
** the array (in entries - so the allocation is ((*pnEntry) * szEntry) bytes
** in total).
**
** If the realloc() is successful (i.e. if no OOM condition occurs), the
** space allocated for the new object is zeroed, *pnEntry updated to
** reflect the new size of the array and a pointer to the new allocation
** returned. *pIdx is set to the index of the new array entry in this case.
**
** Otherwise, if the realloc() fails, *pIdx is set to -1, *pnEntry remains
** unchanged and a copy of pArray returned.
*/
SQLITE_PRIVATE void *sqlite3ArrayAllocate(
  sqlite3 *db,      /* Connection to notify of malloc failures */
  void *pArray,     /* Array of objects.  Might be reallocated */
  int szEntry,      /* Size of each object in the array */
  int *pnEntry,     /* Number of objects currently in use */
  int *pIdx         /* Write the index of a new slot here */
){
  char *z;
  sqlite3_int64 n = *pIdx = *pnEntry;
  if( (n & (n-1))==0 ){
    sqlite3_int64 sz = (n==0) ? 1 : 2*n;
    void *pNew = sqlite3DbRealloc(db, pArray, sz*szEntry);
    if( pNew==0 ){
      *pIdx = -1;
      return pArray;
    }
    pArray = pNew;
  }
  z = (char*)pArray;
  memset(&z[n * szEntry], 0, szEntry);
  ++*pnEntry;
  return pArray;
}

/*
** Append a new element to the given IdList.  Create a new IdList if
** need be.
**
** A new IdList is returned, or NULL if malloc() fails.
*/
SQLITE_PRIVATE IdList *sqlite3IdListAppend(Parse *pParse, IdList *pList, Token *pToken){
  sqlite3 *db = pParse->db;
  int i;
  if( pList==0 ){
    pList = sqlite3DbMallocZero(db, sizeof(IdList) );
    if( pList==0 ) return 0;
  }else{
    IdList *pNew;
    pNew = sqlite3DbRealloc(db, pList,
                 sizeof(IdList) + pList->nId*sizeof(pList->a));
    if( pNew==0 ){
      sqlite3IdListDelete(db, pList);
      return 0;
    }
    pList = pNew;
  }
  i = pList->nId++;
  pList->a[i].zName = sqlite3NameFromToken(db, pToken);
  if( IN_RENAME_OBJECT && pList->a[i].zName ){
    sqlite3RenameTokenMap(pParse, (void*)pList->a[i].zName, pToken);
  }
  return pList;
}

/*
** Delete an IdList.
*/
SQLITE_PRIVATE void sqlite3IdListDelete(sqlite3 *db, IdList *pList){
  int i;
  assert( db!=0 );
  if( pList==0 ) return;
  assert( pList->eU4!=EU4_EXPR ); /* EU4_EXPR mode is not currently used */
  for(i=0; i<pList->nId; i++){
    sqlite3DbFree(db, pList->a[i].zName);
  }
  sqlite3DbNNFreeNN(db, pList);
}

/*
** Return the index in pList of the identifier named zId.  Return -1
** if not found.
*/
SQLITE_PRIVATE int sqlite3IdListIndex(IdList *pList, const char *zName){
  int i;
  assert( pList!=0 );
  for(i=0; i<pList->nId; i++){
    if( sqlite3StrICmp(pList->a[i].zName, zName)==0 ) return i;
  }
  return -1;
}

/*
** Maximum size of a SrcList object.
** The SrcList object is used to represent the FROM clause of a
** SELECT statement, and the query planner cannot deal with more
** than 64 tables in a join.  So any value larger than 64 here
** is sufficient for most uses.  Smaller values, like say 10, are
** appropriate for small and memory-limited applications.
*/
#ifndef SQLITE_MAX_SRCLIST
# define SQLITE_MAX_SRCLIST 200
#endif

/*
** Expand the space allocated for the given SrcList object by
** creating nExtra new slots beginning at iStart.  iStart is zero based.
** New slots are zeroed.
**
** For example, suppose a SrcList initially contains two entries: A,B.
** To append 3 new entries onto the end, do this:
**
**    sqlite3SrcListEnlarge(db, pSrclist, 3, 2);
**
** After the call above it would contain:  A, B, nil, nil, nil.
** If the iStart argument had been 1 instead of 2, then the result
** would have been:  A, nil, nil, nil, B.  To prepend the new slots,
** the iStart value would be 0.  The result then would
** be: nil, nil, nil, A, B.
**
** If a memory allocation fails or the SrcList becomes too large, leave
** the original SrcList unchanged, return NULL, and leave an error message
** in pParse.
*/
SQLITE_PRIVATE SrcList *sqlite3SrcListEnlarge(
  Parse *pParse,     /* Parsing context into which errors are reported */
  SrcList *pSrc,     /* The SrcList to be enlarged */
  int nExtra,        /* Number of new slots to add to pSrc->a[] */
  int iStart         /* Index in pSrc->a[] of first new slot */
){
  int i;

  /* Sanity checking on calling parameters */
  assert( iStart>=0 );
  assert( nExtra>=1 );
  assert( pSrc!=0 );
  assert( iStart<=pSrc->nSrc );

  /* Allocate additional space if needed */
  if( (u32)pSrc->nSrc+nExtra>pSrc->nAlloc ){
    SrcList *pNew;
    sqlite3_int64 nAlloc = 2*(sqlite3_int64)pSrc->nSrc+nExtra;
    sqlite3 *db = pParse->db;

    if( pSrc->nSrc+nExtra>=SQLITE_MAX_SRCLIST ){
      sqlite3ErrorMsg(pParse, "too many FROM clause terms, max: %d",
                      SQLITE_MAX_SRCLIST);
      return 0;
    }
    if( nAlloc>SQLITE_MAX_SRCLIST ) nAlloc = SQLITE_MAX_SRCLIST;
    pNew = sqlite3DbRealloc(db, pSrc,
               sizeof(*pSrc) + (nAlloc-1)*sizeof(pSrc->a[0]) );
    if( pNew==0 ){
      assert( db->mallocFailed );
      return 0;
    }
    pSrc = pNew;
    pSrc->nAlloc = nAlloc;
  }

  /* Move existing slots that come after the newly inserted slots
  ** out of the way */
  for(i=pSrc->nSrc-1; i>=iStart; i--){
    pSrc->a[i+nExtra] = pSrc->a[i];
  }
  pSrc->nSrc += nExtra;

  /* Zero the newly allocated slots */
  memset(&pSrc->a[iStart], 0, sizeof(pSrc->a[0])*nExtra);
  for(i=iStart; i<iStart+nExtra; i++){
    pSrc->a[i].iCursor = -1;
  }

  /* Return a pointer to the enlarged SrcList */
  return pSrc;
}


/*
** Append a new table name to the given SrcList.  Create a new SrcList if
** need be.  A new entry is created in the SrcList even if pTable is NULL.
**
** A SrcList is returned, or NULL if there is an OOM error or if the
** SrcList grows to large.  The returned
** SrcList might be the same as the SrcList that was input or it might be
** a new one.  If an OOM error does occurs, then the prior value of pList
** that is input to this routine is automatically freed.
**
** If pDatabase is not null, it means that the table has an optional
** database name prefix.  Like this:  "database.table".  The pDatabase
** points to the table name and the pTable points to the database name.
** The SrcList.a[].zName field is filled with the table name which might
** come from pTable (if pDatabase is NULL) or from pDatabase.
** SrcList.a[].zDatabase is filled with the database name from pTable,
** or with NULL if no database is specified.
**
** In other words, if call like this:
**
**         sqlite3SrcListAppend(D,A,B,0);
**
** Then B is a table name and the database name is unspecified.  If called
** like this:
**
**         sqlite3SrcListAppend(D,A,B,C);
**
** Then C is the table name and B is the database name.  If C is defined
** then so is B.  In other words, we never have a case where:
**
**         sqlite3SrcListAppend(D,A,0,C);
**
** Both pTable and pDatabase are assumed to be quoted.  They are dequoted
** before being added to the SrcList.
*/
SQLITE_PRIVATE SrcList *sqlite3SrcListAppend(
  Parse *pParse,      /* Parsing context, in which errors are reported */
  SrcList *pList,     /* Append to this SrcList. NULL creates a new SrcList */
  Token *pTable,      /* Table to append */
  Token *pDatabase    /* Database of the table */
){
  SrcItem *pItem;
  sqlite3 *db;
  assert( pDatabase==0 || pTable!=0 );  /* Cannot have C without B */
  assert( pParse!=0 );
  assert( pParse->db!=0 );
  db = pParse->db;
  if( pList==0 ){
    pList = sqlite3DbMallocRawNN(pParse->db, sizeof(SrcList) );
    if( pList==0 ) return 0;
    pList->nAlloc = 1;
    pList->nSrc = 1;
    memset(&pList->a[0], 0, sizeof(pList->a[0]));
    pList->a[0].iCursor = -1;
  }else{
    SrcList *pNew = sqlite3SrcListEnlarge(pParse, pList, 1, pList->nSrc);
    if( pNew==0 ){
      sqlite3SrcListDelete(db, pList);
      return 0;
    }else{
      pList = pNew;
    }
  }
  pItem = &pList->a[pList->nSrc-1];
  if( pDatabase && pDatabase->z==0 ){
    pDatabase = 0;
  }
  if( pDatabase ){
    pItem->zName = sqlite3NameFromToken(db, pDatabase);
    pItem->zDatabase = sqlite3NameFromToken(db, pTable);
  }else{
    pItem->zName = sqlite3NameFromToken(db, pTable);
    pItem->zDatabase = 0;
  }
  return pList;
}
