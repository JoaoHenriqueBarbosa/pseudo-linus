

/*
** A segment of size nByte bytes has just been written to absolute level
** iAbsLevel. Promote any segments that should be promoted as a result.
*/
static int fts3PromoteSegments(
  Fts3Table *p,                   /* FTS table handle */
  sqlite3_int64 iAbsLevel,        /* Absolute level just updated */
  sqlite3_int64 nByte             /* Size of new segment at iAbsLevel */
){
  int rc = SQLITE_OK;
  sqlite3_stmt *pRange;

  rc = fts3SqlStmt(p, SQL_SELECT_LEVEL_RANGE2, &pRange, 0);

  if( rc==SQLITE_OK ){
    int bOk = 0;
    i64 iLast = (iAbsLevel/FTS3_SEGDIR_MAXLEVEL + 1) * FTS3_SEGDIR_MAXLEVEL - 1;
    i64 nLimit = (nByte*3)/2;

    /* Loop through all entries in the %_segdir table corresponding to
    ** segments in this index on levels greater than iAbsLevel. If there is
    ** at least one such segment, and it is possible to determine that all
    ** such segments are smaller than nLimit bytes in size, they will be
    ** promoted to level iAbsLevel.  */
    sqlite3_bind_int64(pRange, 1, iAbsLevel+1);
    sqlite3_bind_int64(pRange, 2, iLast);
    while( SQLITE_ROW==sqlite3_step(pRange) ){
      i64 nSize = 0, dummy;
      fts3ReadEndBlockField(pRange, 2, &dummy, &nSize);
      if( nSize<=0 || nSize>nLimit ){
        /* If nSize==0, then the %_segdir.end_block field does not not
        ** contain a size value. This happens if it was written by an
        ** old version of FTS. In this case it is not possible to determine
        ** the size of the segment, and so segment promotion does not
        ** take place.  */
        bOk = 0;
        break;
      }
      bOk = 1;
    }
    rc = sqlite3_reset(pRange);

    if( bOk ){
      int iIdx = 0;
      sqlite3_stmt *pUpdate1 = 0;
      sqlite3_stmt *pUpdate2 = 0;

      if( rc==SQLITE_OK ){
        rc = fts3SqlStmt(p, SQL_UPDATE_LEVEL_IDX, &pUpdate1, 0);
      }
      if( rc==SQLITE_OK ){
        rc = fts3SqlStmt(p, SQL_UPDATE_LEVEL, &pUpdate2, 0);
      }

      if( rc==SQLITE_OK ){

        /* Loop through all %_segdir entries for segments in this index with
        ** levels equal to or greater than iAbsLevel. As each entry is visited,
        ** updated it to set (level = -1) and (idx = N), where N is 0 for the
        ** oldest segment in the range, 1 for the next oldest, and so on.
        **
        ** In other words, move all segments being promoted to level -1,
        ** setting the "idx" fields as appropriate to keep them in the same
        ** order. The contents of level -1 (which is never used, except
        ** transiently here), will be moved back to level iAbsLevel below.  */
        sqlite3_bind_int64(pRange, 1, iAbsLevel);
        while( SQLITE_ROW==sqlite3_step(pRange) ){
          sqlite3_bind_int(pUpdate1, 1, iIdx++);
          sqlite3_bind_int(pUpdate1, 2, sqlite3_column_int(pRange, 0));
          sqlite3_bind_int(pUpdate1, 3, sqlite3_column_int(pRange, 1));
          sqlite3_step(pUpdate1);
          rc = sqlite3_reset(pUpdate1);
          if( rc!=SQLITE_OK ){
            sqlite3_reset(pRange);
            break;
          }
        }
      }
      if( rc==SQLITE_OK ){
        rc = sqlite3_reset(pRange);
      }

      /* Move level -1 to level iAbsLevel */
      if( rc==SQLITE_OK ){
        sqlite3_bind_int64(pUpdate2, 1, iAbsLevel);
        sqlite3_step(pUpdate2);
        rc = sqlite3_reset(pUpdate2);
      }
    }
  }


  return rc;
}

/*
** Merge all level iLevel segments in the database into a single
** iLevel+1 segment. Or, if iLevel<0, merge all segments into a
** single segment with a level equal to the numerically largest level
** currently present in the database.
**
** If this function is called with iLevel<0, but there is only one
** segment in the database, SQLITE_DONE is returned immediately.
** Otherwise, if successful, SQLITE_OK is returned. If an error occurs,
** an SQLite error code is returned.
*/
static int fts3SegmentMerge(
  Fts3Table *p,
  int iLangid,                    /* Language id to merge */
  int iIndex,                     /* Index in p->aIndex[] to merge */
  int iLevel                      /* Level to merge */
){
  int rc;                         /* Return code */
  int iIdx = 0;                   /* Index of new segment */
  sqlite3_int64 iNewLevel = 0;    /* Level/index to create new segment at */
  SegmentWriter *pWriter = 0;     /* Used to write the new, merged, segment */
  Fts3SegFilter filter;           /* Segment term filter condition */
  Fts3MultiSegReader csr;         /* Cursor to iterate through level(s) */
  int bIgnoreEmpty = 0;           /* True to ignore empty segments */
  i64 iMaxLevel = 0;              /* Max level number for this index/langid */

  assert( iLevel==FTS3_SEGCURSOR_ALL
       || iLevel==FTS3_SEGCURSOR_PENDING
       || iLevel>=0
  );
  assert( iLevel<FTS3_SEGDIR_MAXLEVEL );
  assert( iIndex>=0 && iIndex<p->nIndex );

  rc = sqlite3Fts3SegReaderCursor(p, iLangid, iIndex, iLevel, 0, 0, 1, 0, &csr);
  if( rc!=SQLITE_OK || csr.nSegment==0 ) goto finished;

  if( iLevel!=FTS3_SEGCURSOR_PENDING ){
    rc = fts3SegmentMaxLevel(p, iLangid, iIndex, &iMaxLevel);
    if( rc!=SQLITE_OK ) goto finished;
  }

  if( iLevel==FTS3_SEGCURSOR_ALL ){
    /* This call is to merge all segments in the database to a single
    ** segment. The level of the new segment is equal to the numerically
    ** greatest segment level currently present in the database for this
    ** index. The idx of the new segment is always 0.  */
    if( csr.nSegment==1 && 0==fts3SegReaderIsPending(csr.apSegment[0]) ){
      rc = SQLITE_DONE;
      goto finished;
    }
    iNewLevel = iMaxLevel;
    bIgnoreEmpty = 1;

  }else{
    /* This call is to merge all segments at level iLevel. find the next
    ** available segment index at level iLevel+1. The call to
    ** fts3AllocateSegdirIdx() will merge the segments at level iLevel+1 to
    ** a single iLevel+2 segment if necessary.  */
    assert( FTS3_SEGCURSOR_PENDING==-1 );
    iNewLevel = getAbsoluteLevel(p, iLangid, iIndex, iLevel+1);
    rc = fts3AllocateSegdirIdx(p, iLangid, iIndex, iLevel+1, &iIdx);
    bIgnoreEmpty = (iLevel!=FTS3_SEGCURSOR_PENDING) && (iNewLevel>iMaxLevel);
  }
  if( rc!=SQLITE_OK ) goto finished;

  assert( csr.nSegment>0 );
  assert_fts3_nc( iNewLevel>=getAbsoluteLevel(p, iLangid, iIndex, 0) );
  assert_fts3_nc(
    iNewLevel<getAbsoluteLevel(p, iLangid, iIndex,FTS3_SEGDIR_MAXLEVEL)
  );

  memset(&filter, 0, sizeof(Fts3SegFilter));
  filter.flags = FTS3_SEGMENT_REQUIRE_POS;
  filter.flags |= (bIgnoreEmpty ? FTS3_SEGMENT_IGNORE_EMPTY : 0);

  rc = sqlite3Fts3SegReaderStart(p, &csr, &filter);
  while( SQLITE_OK==rc ){
    rc = sqlite3Fts3SegReaderStep(p, &csr);
    if( rc!=SQLITE_ROW ) break;
    rc = fts3SegWriterAdd(p, &pWriter, 1,
        csr.zTerm, csr.nTerm, csr.aDoclist, csr.nDoclist);
  }
  if( rc!=SQLITE_OK ) goto finished;
  assert_fts3_nc( pWriter || bIgnoreEmpty );

  if( iLevel!=FTS3_SEGCURSOR_PENDING ){
    rc = fts3DeleteSegdir(
        p, iLangid, iIndex, iLevel, csr.apSegment, csr.nSegment
    );
    if( rc!=SQLITE_OK ) goto finished;
  }
  if( pWriter ){
    rc = fts3SegWriterFlush(p, pWriter, iNewLevel, iIdx);
    if( rc==SQLITE_OK ){
      if( iLevel==FTS3_SEGCURSOR_PENDING || iNewLevel<iMaxLevel ){
        rc = fts3PromoteSegments(p, iNewLevel, pWriter->nLeafData);
      }
    }
  }

 finished:
  fts3SegWriterFree(pWriter);
  sqlite3Fts3SegReaderFinish(&csr);
  return rc;
}


/*
** Flush the contents of pendingTerms to level 0 segments.
*/
SQLITE_PRIVATE int sqlite3Fts3PendingTermsFlush(Fts3Table *p){
  int rc = SQLITE_OK;
  int i;

  for(i=0; rc==SQLITE_OK && i<p->nIndex; i++){
    rc = fts3SegmentMerge(p, p->iPrevLangid, i, FTS3_SEGCURSOR_PENDING);
    if( rc==SQLITE_DONE ) rc = SQLITE_OK;
  }

  /* Determine the auto-incr-merge setting if unknown.  If enabled,
  ** estimate the number of leaf blocks of content to be written
  */
  if( rc==SQLITE_OK && p->bHasStat
   && p->nAutoincrmerge==0xff && p->nLeafAdd>0
  ){
    sqlite3_stmt *pStmt = 0;
    rc = fts3SqlStmt(p, SQL_SELECT_STAT, &pStmt, 0);
    if( rc==SQLITE_OK ){
      sqlite3_bind_int(pStmt, 1, FTS_STAT_AUTOINCRMERGE);
      rc = sqlite3_step(pStmt);
      if( rc==SQLITE_ROW ){
        p->nAutoincrmerge = sqlite3_column_int(pStmt, 0);
        if( p->nAutoincrmerge==1 ) p->nAutoincrmerge = 8;
      }else if( rc==SQLITE_DONE ){
        p->nAutoincrmerge = 0;
      }
      rc = sqlite3_reset(pStmt);
    }
  }

  if( rc==SQLITE_OK ){
    sqlite3Fts3PendingTermsClear(p);
  }
  return rc;
}

/*
** Encode N integers as varints into a blob.
*/
static void fts3EncodeIntArray(
  int N,             /* The number of integers to encode */
  u32 *a,            /* The integer values */
  char *zBuf,        /* Write the BLOB here */
  int *pNBuf         /* Write number of bytes if zBuf[] used here */
){
  int i, j;
  for(i=j=0; i<N; i++){
    j += sqlite3Fts3PutVarint(&zBuf[j], (sqlite3_int64)a[i]);
  }
  *pNBuf = j;
}

/*
** Decode a blob of varints into N integers
*/
static void fts3DecodeIntArray(
  int N,             /* The number of integers to decode */
  u32 *a,            /* Write the integer values */
  const char *zBuf,  /* The BLOB containing the varints */
  int nBuf           /* size of the BLOB */
){
  int i = 0;
  if( nBuf && (zBuf[nBuf-1]&0x80)==0 ){
    int j;
    for(i=j=0; i<N && j<nBuf; i++){
      sqlite3_int64 x;
      j += sqlite3Fts3GetVarint(&zBuf[j], &x);
      a[i] = (u32)(x & 0xffffffff);
    }
  }
  while( i<N ) a[i++] = 0;
}

/*
** Insert the sizes (in tokens) for each column of the document
** with docid equal to p->iPrevDocid.  The sizes are encoded as
** a blob of varints.
*/
static void fts3InsertDocsize(
  int *pRC,                       /* Result code */
  Fts3Table *p,                   /* Table into which to insert */
  u32 *aSz                        /* Sizes of each column, in tokens */
){
  char *pBlob;             /* The BLOB encoding of the document size */
  int nBlob;               /* Number of bytes in the BLOB */
  sqlite3_stmt *pStmt;     /* Statement used to insert the encoding */
  int rc;                  /* Result code from subfunctions */

  if( *pRC ) return;
  pBlob = sqlite3_malloc64( 10*(sqlite3_int64)p->nColumn );
  if( pBlob==0 ){
    *pRC = SQLITE_NOMEM;
    return;
  }
  fts3EncodeIntArray(p->nColumn, aSz, pBlob, &nBlob);
  rc = fts3SqlStmt(p, SQL_REPLACE_DOCSIZE, &pStmt, 0);
  if( rc ){
    sqlite3_free(pBlob);
    *pRC = rc;
    return;
  }
  sqlite3_bind_int64(pStmt, 1, p->iPrevDocid);
  sqlite3_bind_blob(pStmt, 2, pBlob, nBlob, sqlite3_free);
  sqlite3_step(pStmt);
  *pRC = sqlite3_reset(pStmt);
}

/*
** Record 0 of the %_stat table contains a blob consisting of N varints,
** where N is the number of user defined columns in the fts3 table plus
** two. If nCol is the number of user defined columns, then values of the
** varints are set as follows:
**
**   Varint 0:       Total number of rows in the table.
**
**   Varint 1..nCol: For each column, the total number of tokens stored in
**                   the column for all rows of the table.
**
**   Varint 1+nCol:  The total size, in bytes, of all text values in all
**                   columns of all rows of the table.
**
*/
static void fts3UpdateDocTotals(
  int *pRC,                       /* The result code */
  Fts3Table *p,                   /* Table being updated */
  u32 *aSzIns,                    /* Size increases */
  u32 *aSzDel,                    /* Size decreases */
  int nChng                       /* Change in the number of documents */
){
  char *pBlob;             /* Storage for BLOB written into %_stat */
  int nBlob;               /* Size of BLOB written into %_stat */
  u32 *a;                  /* Array of integers that becomes the BLOB */
  sqlite3_stmt *pStmt;     /* Statement for reading and writing */
  int i;                   /* Loop counter */
  int rc;                  /* Result code from subfunctions */

  const int nStat = p->nColumn+2;

  if( *pRC ) return;
  a = sqlite3_malloc64( (sizeof(u32)+10)*(sqlite3_int64)nStat );
  if( a==0 ){
    *pRC = SQLITE_NOMEM;
    return;
  }
  pBlob = (char*)&a[nStat];
  rc = fts3SqlStmt(p, SQL_SELECT_STAT, &pStmt, 0);
  if( rc ){
    sqlite3_free(a);
    *pRC = rc;
    return;
  }
  sqlite3_bind_int(pStmt, 1, FTS_STAT_DOCTOTAL);
  if( sqlite3_step(pStmt)==SQLITE_ROW ){
    fts3DecodeIntArray(nStat, a,
         sqlite3_column_blob(pStmt, 0),
         sqlite3_column_bytes(pStmt, 0));
  }else{
    memset(a, 0, sizeof(u32)*(nStat) );
  }
  rc = sqlite3_reset(pStmt);
  if( rc!=SQLITE_OK ){
    sqlite3_free(a);
    *pRC = rc;
    return;
  }
  if( nChng<0 && a[0]<(u32)(-nChng) ){
    a[0] = 0;
  }else{
    a[0] += nChng;
  }
  for(i=0; i<p->nColumn+1; i++){
    u32 x = a[i+1];
    if( x+aSzIns[i] < aSzDel[i] ){
      x = 0;
    }else{
      x = x + aSzIns[i] - aSzDel[i];
    }
    a[i+1] = x;
  }
  fts3EncodeIntArray(nStat, a, pBlob, &nBlob);
  rc = fts3SqlStmt(p, SQL_REPLACE_STAT, &pStmt, 0);
  if( rc ){
    sqlite3_free(a);
    *pRC = rc;
    return;
  }
  sqlite3_bind_int(pStmt, 1, FTS_STAT_DOCTOTAL);
  sqlite3_bind_blob(pStmt, 2, pBlob, nBlob, SQLITE_STATIC);
  sqlite3_step(pStmt);
  *pRC = sqlite3_reset(pStmt);
  sqlite3_bind_null(pStmt, 2);
  sqlite3_free(a);
}
