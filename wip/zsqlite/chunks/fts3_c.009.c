
/*
** Implementation of xSync() method. Flush the contents of the pending-terms
** hash-table to the database.
*/
static int fts3SyncMethod(sqlite3_vtab *pVtab){

  /* Following an incremental-merge operation, assuming that the input
  ** segments are not completely consumed (the usual case), they are updated
  ** in place to remove the entries that have already been merged. This
  ** involves updating the leaf block that contains the smallest unmerged
  ** entry and each block (if any) between the leaf and the root node. So
  ** if the height of the input segment b-trees is N, and input segments
  ** are merged eight at a time, updating the input segments at the end
  ** of an incremental-merge requires writing (8*(1+N)) blocks. N is usually
  ** small - often between 0 and 2. So the overhead of the incremental
  ** merge is somewhere between 8 and 24 blocks. To avoid this overhead
  ** dwarfing the actual productive work accomplished, the incremental merge
  ** is only attempted if it will write at least 64 leaf blocks. Hence
  ** nMinMerge.
  **
  ** Of course, updating the input segments also involves deleting a bunch
  ** of blocks from the segments table. But this is not considered overhead
  ** as it would also be required by a crisis-merge that used the same input
  ** segments.
  */
  const u32 nMinMerge = 64;       /* Minimum amount of incr-merge work to do */

  Fts3Table *p = (Fts3Table*)pVtab;
  int rc;
  i64 iLastRowid = sqlite3_last_insert_rowid(p->db);

  rc = sqlite3Fts3PendingTermsFlush(p);
  if( rc==SQLITE_OK
   && p->nLeafAdd>(nMinMerge/16)
   && p->nAutoincrmerge && p->nAutoincrmerge!=0xff
  ){
    int mxLevel = 0;              /* Maximum relative level value in db */
    int A;                        /* Incr-merge parameter A */

    rc = sqlite3Fts3MaxLevel(p, &mxLevel);
    assert( rc==SQLITE_OK || mxLevel==0 );
    A = p->nLeafAdd * mxLevel;
    A += (A/2);
    if( A>(int)nMinMerge ) rc = sqlite3Fts3Incrmerge(p, A, p->nAutoincrmerge);
  }
  sqlite3Fts3SegmentsClose(p);
  sqlite3_set_last_insert_rowid(p->db, iLastRowid);
  return rc;
}

/*
** If it is currently unknown whether or not the FTS table has an %_stat
** table (if p->bHasStat==2), attempt to determine this (set p->bHasStat
** to 0 or 1). Return SQLITE_OK if successful, or an SQLite error code
** if an error occurs.
*/
static int fts3SetHasStat(Fts3Table *p){
  int rc = SQLITE_OK;
  if( p->bHasStat==2 ){
    char *zTbl = sqlite3_mprintf("%s_stat", p->zName);
    if( zTbl ){
      int res = sqlite3_table_column_metadata(p->db, p->zDb, zTbl, 0,0,0,0,0,0);
      sqlite3_free(zTbl);
      p->bHasStat = (res==SQLITE_OK);
    }else{
      rc = SQLITE_NOMEM;
    }
  }
  return rc;
}

/*
** Implementation of xBegin() method.
*/
static int fts3BeginMethod(sqlite3_vtab *pVtab){
  Fts3Table *p = (Fts3Table*)pVtab;
  int rc;
  UNUSED_PARAMETER(pVtab);
  assert( p->pSegments==0 );
  assert( p->nPendingData==0 );
  assert( p->inTransaction!=1 );
  p->nLeafAdd = 0;
  rc = fts3SetHasStat(p);
#ifdef SQLITE_DEBUG
  if( rc==SQLITE_OK ){
    p->inTransaction = 1;
    p->mxSavepoint = -1;
  }
#endif
  return rc;
}

/*
** Implementation of xCommit() method. This is a no-op. The contents of
** the pending-terms hash-table have already been flushed into the database
** by fts3SyncMethod().
*/
static int fts3CommitMethod(sqlite3_vtab *pVtab){
  TESTONLY( Fts3Table *p = (Fts3Table*)pVtab );
  UNUSED_PARAMETER(pVtab);
  assert( p->nPendingData==0 );
  assert( p->inTransaction!=0 );
  assert( p->pSegments==0 );
  TESTONLY( p->inTransaction = 0 );
  TESTONLY( p->mxSavepoint = -1; );
  return SQLITE_OK;
}

/*
** Implementation of xRollback(). Discard the contents of the pending-terms
** hash-table. Any changes made to the database are reverted by SQLite.
*/
static int fts3RollbackMethod(sqlite3_vtab *pVtab){
  Fts3Table *p = (Fts3Table*)pVtab;
  sqlite3Fts3PendingTermsClear(p);
  assert( p->inTransaction!=0 );
  TESTONLY( p->inTransaction = 0 );
  TESTONLY( p->mxSavepoint = -1; );
  return SQLITE_OK;
}

/*
** When called, *ppPoslist must point to the byte immediately following the
** end of a position-list. i.e. ( (*ppPoslist)[-1]==POS_END ). This function
** moves *ppPoslist so that it instead points to the first byte of the
** same position list.
*/
static void fts3ReversePoslist(char *pStart, char **ppPoslist){
  char *p = &(*ppPoslist)[-2];
  char c = 0;

  /* Skip backwards passed any trailing 0x00 bytes added by NearTrim() */
  while( p>pStart && (c=*p--)==0 );

  /* Search backwards for a varint with value zero (the end of the previous
  ** poslist). This is an 0x00 byte preceded by some byte that does not
  ** have the 0x80 bit set.  */
  while( p>pStart && (*p & 0x80) | c ){
    c = *p--;
  }
  assert( p==pStart || c==0 );

  /* At this point p points to that preceding byte without the 0x80 bit
  ** set. So to find the start of the poslist, skip forward 2 bytes then
  ** over a varint.
  **
  ** Normally. The other case is that p==pStart and the poslist to return
  ** is the first in the doclist. In this case do not skip forward 2 bytes.
  ** The second part of the if condition (c==0 && *ppPoslist>&p[2])
  ** is required for cases where the first byte of a doclist and the
  ** doclist is empty. For example, if the first docid is 10, a doclist
  ** that begins with:
  **
  **   0x0A 0x00 <next docid delta varint>
  */
  if( p>pStart || (c==0 && *ppPoslist>&p[2]) ){ p = &p[2]; }
  while( *p++&0x80 );
  *ppPoslist = p;
}

/*
** Helper function used by the implementation of the overloaded snippet(),
** offsets() and optimize() SQL functions.
**
** If the value passed as the third argument is a blob of size
** sizeof(Fts3Cursor*), then the blob contents are copied to the
** output variable *ppCsr and SQLITE_OK is returned. Otherwise, an error
** message is written to context pContext and SQLITE_ERROR returned. The
** string passed via zFunc is used as part of the error message.
*/
static int fts3FunctionArg(
  sqlite3_context *pContext,      /* SQL function call context */
  const char *zFunc,              /* Function name */
  sqlite3_value *pVal,            /* argv[0] passed to function */
  Fts3Cursor **ppCsr              /* OUT: Store cursor handle here */
){
  int rc;
  *ppCsr = (Fts3Cursor*)sqlite3_value_pointer(pVal, "fts3cursor");
  if( (*ppCsr)!=0 ){
    rc = SQLITE_OK;
  }else{
    char *zErr = sqlite3_mprintf("illegal first argument to %s", zFunc);
    sqlite3_result_error(pContext, zErr, -1);
    sqlite3_free(zErr);
    rc = SQLITE_ERROR;
  }
  return rc;
}

/*
** Implementation of the snippet() function for FTS3
*/
static void fts3SnippetFunc(
  sqlite3_context *pContext,      /* SQLite function call context */
  int nVal,                       /* Size of apVal[] array */
  sqlite3_value **apVal           /* Array of arguments */
){
  Fts3Cursor *pCsr;               /* Cursor handle passed through apVal[0] */
  const char *zStart = "<b>";
  const char *zEnd = "</b>";
  const char *zEllipsis = "<b>...</b>";
  int iCol = -1;
  int nToken = 15;                /* Default number of tokens in snippet */

  /* There must be at least one argument passed to this function (otherwise
  ** the non-overloaded version would have been called instead of this one).
  */
  assert( nVal>=1 );

  if( nVal>6 ){
    sqlite3_result_error(pContext,
        "wrong number of arguments to function snippet()", -1);
    return;
  }
  if( fts3FunctionArg(pContext, "snippet", apVal[0], &pCsr) ) return;

  switch( nVal ){
    case 6: nToken = sqlite3_value_int(apVal[5]);
            /* no break */ deliberate_fall_through
    case 5: iCol = sqlite3_value_int(apVal[4]);
            /* no break */ deliberate_fall_through
    case 4: zEllipsis = (const char*)sqlite3_value_text(apVal[3]);
            /* no break */ deliberate_fall_through
    case 3: zEnd = (const char*)sqlite3_value_text(apVal[2]);
            /* no break */ deliberate_fall_through
    case 2: zStart = (const char*)sqlite3_value_text(apVal[1]);
  }
  if( !zEllipsis || !zEnd || !zStart ){
    sqlite3_result_error_nomem(pContext);
  }else if( nToken==0 ){
    sqlite3_result_text(pContext, "", -1, SQLITE_STATIC);
  }else if( SQLITE_OK==fts3CursorSeek(pContext, pCsr) ){
    sqlite3Fts3Snippet(pContext, pCsr, zStart, zEnd, zEllipsis, iCol, nToken);
  }
}

/*
** Implementation of the offsets() function for FTS3
*/
static void fts3OffsetsFunc(
  sqlite3_context *pContext,      /* SQLite function call context */
  int nVal,                       /* Size of argument array */
  sqlite3_value **apVal           /* Array of arguments */
){
  Fts3Cursor *pCsr;               /* Cursor handle passed through apVal[0] */

  UNUSED_PARAMETER(nVal);

  assert( nVal==1 );
  if( fts3FunctionArg(pContext, "offsets", apVal[0], &pCsr) ) return;
  assert( pCsr );
  if( SQLITE_OK==fts3CursorSeek(pContext, pCsr) ){
    sqlite3Fts3Offsets(pContext, pCsr);
  }
}

/*
** Implementation of the special optimize() function for FTS3. This
** function merges all segments in the database to a single segment.
** Example usage is:
**
**   SELECT optimize(t) FROM t LIMIT 1;
**
** where 't' is the name of an FTS3 table.
*/
static void fts3OptimizeFunc(
  sqlite3_context *pContext,      /* SQLite function call context */
  int nVal,                       /* Size of argument array */
  sqlite3_value **apVal           /* Array of arguments */
){
  int rc;                         /* Return code */
  Fts3Table *p;                   /* Virtual table handle */
  Fts3Cursor *pCursor;            /* Cursor handle passed through apVal[0] */

  UNUSED_PARAMETER(nVal);

  assert( nVal==1 );
  if( fts3FunctionArg(pContext, "optimize", apVal[0], &pCursor) ) return;
  p = (Fts3Table *)pCursor->base.pVtab;
  assert( p );

  rc = sqlite3Fts3Optimize(p);

  switch( rc ){
    case SQLITE_OK:
      sqlite3_result_text(pContext, "Index optimized", -1, SQLITE_STATIC);
      break;
    case SQLITE_DONE:
      sqlite3_result_text(pContext, "Index already optimal", -1, SQLITE_STATIC);
      break;
    default:
      sqlite3_result_error_code(pContext, rc);
      break;
  }
}

/*
** Implementation of the matchinfo() function for FTS3
*/
static void fts3MatchinfoFunc(
  sqlite3_context *pContext,      /* SQLite function call context */
  int nVal,                       /* Size of argument array */
  sqlite3_value **apVal           /* Array of arguments */
){
  Fts3Cursor *pCsr;               /* Cursor handle passed through apVal[0] */
  assert( nVal==1 || nVal==2 );
  if( SQLITE_OK==fts3FunctionArg(pContext, "matchinfo", apVal[0], &pCsr) ){
    const char *zArg = 0;
    if( nVal>1 ){
      zArg = (const char *)sqlite3_value_text(apVal[1]);
    }
    sqlite3Fts3Matchinfo(pContext, pCsr, zArg);
  }
}

/*
** This routine implements the xFindFunction method for the FTS3
** virtual table.
*/
static int fts3FindFunctionMethod(
  sqlite3_vtab *pVtab,            /* Virtual table handle */
  int nArg,                       /* Number of SQL function arguments */
  const char *zName,              /* Name of SQL function */
  void (**pxFunc)(sqlite3_context*,int,sqlite3_value**), /* OUT: Result */
  void **ppArg                    /* Unused */
){
  struct Overloaded {
    const char *zName;
    void (*xFunc)(sqlite3_context*,int,sqlite3_value**);
  } aOverload[] = {
    { "snippet", fts3SnippetFunc },
    { "offsets", fts3OffsetsFunc },
    { "optimize", fts3OptimizeFunc },
    { "matchinfo", fts3MatchinfoFunc },
  };
  int i;                          /* Iterator variable */

  UNUSED_PARAMETER(pVtab);
  UNUSED_PARAMETER(nArg);
  UNUSED_PARAMETER(ppArg);

  for(i=0; i<SizeofArray(aOverload); i++){
    if( strcmp(zName, aOverload[i].zName)==0 ){
      *pxFunc = aOverload[i].xFunc;
      return 1;
    }
  }

  /* No function of the specified name was found. Return 0. */
  return 0;
}
