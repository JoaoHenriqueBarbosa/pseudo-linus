
/*
** Calling this function indicates that subsequent calls to
** fts3PendingTermsAdd() are to add term/position-list pairs for the
** contents of the document with docid iDocid.
*/
static int fts3PendingTermsDocid(
  Fts3Table *p,                   /* Full-text table handle */
  int bDelete,                    /* True if this op is a delete */
  int iLangid,                    /* Language id of row being written */
  sqlite_int64 iDocid             /* Docid of row being written */
){
  assert( iLangid>=0 );
  assert( bDelete==1 || bDelete==0 );

  /* TODO(shess) Explore whether partially flushing the buffer on
  ** forced-flush would provide better performance.  I suspect that if
  ** we ordered the doclists by size and flushed the largest until the
  ** buffer was half empty, that would let the less frequent terms
  ** generate longer doclists.
  */
  if( iDocid<p->iPrevDocid
   || (iDocid==p->iPrevDocid && p->bPrevDelete==0)
   || p->iPrevLangid!=iLangid
   || p->nPendingData>p->nMaxPendingData
  ){
    int rc = sqlite3Fts3PendingTermsFlush(p);
    if( rc!=SQLITE_OK ) return rc;
  }
  p->iPrevDocid = iDocid;
  p->iPrevLangid = iLangid;
  p->bPrevDelete = bDelete;
  return SQLITE_OK;
}

/*
** Discard the contents of the pending-terms hash tables.
*/
SQLITE_PRIVATE void sqlite3Fts3PendingTermsClear(Fts3Table *p){
  int i;
  for(i=0; i<p->nIndex; i++){
    Fts3HashElem *pElem;
    Fts3Hash *pHash = &p->aIndex[i].hPending;
    for(pElem=fts3HashFirst(pHash); pElem; pElem=fts3HashNext(pElem)){
      PendingList *pList = (PendingList *)fts3HashData(pElem);
      fts3PendingListDelete(pList);
    }
    fts3HashClear(pHash);
  }
  p->nPendingData = 0;
}

/*
** This function is called by the xUpdate() method as part of an INSERT
** operation. It adds entries for each term in the new record to the
** pendingTerms hash table.
**
** Argument apVal is the same as the similarly named argument passed to
** fts3InsertData(). Parameter iDocid is the docid of the new row.
*/
static int fts3InsertTerms(
  Fts3Table *p,
  int iLangid,
  sqlite3_value **apVal,
  u32 *aSz
){
  int i;                          /* Iterator variable */
  for(i=2; i<p->nColumn+2; i++){
    int iCol = i-2;
    if( p->abNotindexed[iCol]==0 ){
      const char *zText = (const char *)sqlite3_value_text(apVal[i]);
      int rc = fts3PendingTermsAdd(p, iLangid, zText, iCol, &aSz[iCol]);
      if( rc!=SQLITE_OK ){
        return rc;
      }
      aSz[p->nColumn] += sqlite3_value_bytes(apVal[i]);
    }
  }
  return SQLITE_OK;
}

/*
** This function is called by the xUpdate() method for an INSERT operation.
** The apVal parameter is passed a copy of the apVal argument passed by
** SQLite to the xUpdate() method. i.e:
**
**   apVal[0]                Not used for INSERT.
**   apVal[1]                rowid
**   apVal[2]                Left-most user-defined column
**   ...
**   apVal[p->nColumn+1]     Right-most user-defined column
**   apVal[p->nColumn+2]     Hidden column with same name as table
**   apVal[p->nColumn+3]     Hidden "docid" column (alias for rowid)
**   apVal[p->nColumn+4]     Hidden languageid column
*/
static int fts3InsertData(
  Fts3Table *p,                   /* Full-text table */
  sqlite3_value **apVal,          /* Array of values to insert */
  sqlite3_int64 *piDocid          /* OUT: Docid for row just inserted */
){
  int rc;                         /* Return code */
  sqlite3_stmt *pContentInsert;   /* INSERT INTO %_content VALUES(...) */

  if( p->zContentTbl ){
    sqlite3_value *pRowid = apVal[p->nColumn+3];
    if( sqlite3_value_type(pRowid)==SQLITE_NULL ){
      pRowid = apVal[1];
    }
    if( sqlite3_value_type(pRowid)!=SQLITE_INTEGER ){
      return SQLITE_CONSTRAINT;
    }
    *piDocid = sqlite3_value_int64(pRowid);
    return SQLITE_OK;
  }

  /* Locate the statement handle used to insert data into the %_content
  ** table. The SQL for this statement is:
  **
  **   INSERT INTO %_content VALUES(?, ?, ?, ...)
  **
  ** The statement features N '?' variables, where N is the number of user
  ** defined columns in the FTS3 table, plus one for the docid field.
  */
  rc = fts3SqlStmt(p, SQL_CONTENT_INSERT, &pContentInsert, &apVal[1]);
  if( rc==SQLITE_OK && p->zLanguageid ){
    rc = sqlite3_bind_int(
        pContentInsert, p->nColumn+2,
        sqlite3_value_int(apVal[p->nColumn+4])
    );
  }
  if( rc!=SQLITE_OK ) return rc;

  /* There is a quirk here. The users INSERT statement may have specified
  ** a value for the "rowid" field, for the "docid" field, or for both.
  ** Which is a problem, since "rowid" and "docid" are aliases for the
  ** same value. For example:
  **
  **   INSERT INTO fts3tbl(rowid, docid) VALUES(1, 2);
  **
  ** In FTS3, this is an error. It is an error to specify non-NULL values
  ** for both docid and some other rowid alias.
  */
  if( SQLITE_NULL!=sqlite3_value_type(apVal[3+p->nColumn]) ){
    if( SQLITE_NULL==sqlite3_value_type(apVal[0])
     && SQLITE_NULL!=sqlite3_value_type(apVal[1])
    ){
      /* A rowid/docid conflict. */
      return SQLITE_ERROR;
    }
    rc = sqlite3_bind_value(pContentInsert, 1, apVal[3+p->nColumn]);
    if( rc!=SQLITE_OK ) return rc;
  }

  /* Execute the statement to insert the record. Set *piDocid to the
  ** new docid value.
  */
  sqlite3_step(pContentInsert);
  rc = sqlite3_reset(pContentInsert);

  *piDocid = sqlite3_last_insert_rowid(p->db);
  return rc;
}



/*
** Remove all data from the FTS3 table. Clear the hash table containing
** pending terms.
*/
static int fts3DeleteAll(Fts3Table *p, int bContent){
  int rc = SQLITE_OK;             /* Return code */

  /* Discard the contents of the pending-terms hash table. */
  sqlite3Fts3PendingTermsClear(p);

  /* Delete everything from the shadow tables. Except, leave %_content as
  ** is if bContent is false.  */
  assert( p->zContentTbl==0 || bContent==0 );
  if( bContent ) fts3SqlExec(&rc, p, SQL_DELETE_ALL_CONTENT, 0);
  fts3SqlExec(&rc, p, SQL_DELETE_ALL_SEGMENTS, 0);
  fts3SqlExec(&rc, p, SQL_DELETE_ALL_SEGDIR, 0);
  if( p->bHasDocsize ){
    fts3SqlExec(&rc, p, SQL_DELETE_ALL_DOCSIZE, 0);
  }
  if( p->bHasStat ){
    fts3SqlExec(&rc, p, SQL_DELETE_ALL_STAT, 0);
  }
  return rc;
}

/*
**
*/
static int langidFromSelect(Fts3Table *p, sqlite3_stmt *pSelect){
  int iLangid = 0;
  if( p->zLanguageid ) iLangid = sqlite3_column_int(pSelect, p->nColumn+1);
  return iLangid;
}

/*
** The first element in the apVal[] array is assumed to contain the docid
** (an integer) of a row about to be deleted. Remove all terms from the
** full-text index.
*/
static void fts3DeleteTerms(
  int *pRC,               /* Result code */
  Fts3Table *p,           /* The FTS table to delete from */
  sqlite3_value *pRowid,  /* The docid to be deleted */
  u32 *aSz,               /* Sizes of deleted document written here */
  int *pbFound            /* OUT: Set to true if row really does exist */
){
  int rc;
  sqlite3_stmt *pSelect;

  assert( *pbFound==0 );
  if( *pRC ) return;
  rc = fts3SqlStmt(p, SQL_SELECT_CONTENT_BY_ROWID, &pSelect, &pRowid);
  if( rc==SQLITE_OK ){
    if( SQLITE_ROW==sqlite3_step(pSelect) ){
      int i;
      int iLangid = langidFromSelect(p, pSelect);
      i64 iDocid = sqlite3_column_int64(pSelect, 0);
      rc = fts3PendingTermsDocid(p, 1, iLangid, iDocid);
      for(i=1; rc==SQLITE_OK && i<=p->nColumn; i++){
        int iCol = i-1;
        if( p->abNotindexed[iCol]==0 ){
          const char *zText = (const char *)sqlite3_column_text(pSelect, i);
          rc = fts3PendingTermsAdd(p, iLangid, zText, -1, &aSz[iCol]);
          aSz[p->nColumn] += sqlite3_column_bytes(pSelect, i);
        }
      }
      if( rc!=SQLITE_OK ){
        sqlite3_reset(pSelect);
        *pRC = rc;
        return;
      }
      *pbFound = 1;
    }
    rc = sqlite3_reset(pSelect);
  }else{
    sqlite3_reset(pSelect);
  }
  *pRC = rc;
}

/*
** Forward declaration to account for the circular dependency between
** functions fts3SegmentMerge() and fts3AllocateSegdirIdx().
*/
static int fts3SegmentMerge(Fts3Table *, int, int, int);

/*
** This function allocates a new level iLevel index in the segdir table.
** Usually, indexes are allocated within a level sequentially starting
** with 0, so the allocated index is one greater than the value returned
** by:
**
**   SELECT max(idx) FROM %_segdir WHERE level = :iLevel
**
** However, if there are already FTS3_MERGE_COUNT indexes at the requested
** level, they are merged into a single level (iLevel+1) segment and the
** allocated index is 0.
**
** If successful, *piIdx is set to the allocated index slot and SQLITE_OK
** returned. Otherwise, an SQLite error code is returned.
*/
static int fts3AllocateSegdirIdx(
  Fts3Table *p,
  int iLangid,                    /* Language id */
  int iIndex,                     /* Index for p->aIndex */
  int iLevel,
  int *piIdx
){
  int rc;                         /* Return Code */
  sqlite3_stmt *pNextIdx;         /* Query for next idx at level iLevel */
  int iNext = 0;                  /* Result of query pNextIdx */

  assert( iLangid>=0 );
  assert( p->nIndex>=1 );

  /* Set variable iNext to the next available segdir index at level iLevel. */
  rc = fts3SqlStmt(p, SQL_NEXT_SEGMENT_INDEX, &pNextIdx, 0);
  if( rc==SQLITE_OK ){
    sqlite3_bind_int64(
        pNextIdx, 1, getAbsoluteLevel(p, iLangid, iIndex, iLevel)
    );
    if( SQLITE_ROW==sqlite3_step(pNextIdx) ){
      iNext = sqlite3_column_int(pNextIdx, 0);
    }
    rc = sqlite3_reset(pNextIdx);
  }

  if( rc==SQLITE_OK ){
    /* If iNext is FTS3_MERGE_COUNT, indicating that level iLevel is already
    ** full, merge all segments in level iLevel into a single iLevel+1
    ** segment and allocate (newly freed) index 0 at level iLevel. Otherwise,
    ** if iNext is less than FTS3_MERGE_COUNT, allocate index iNext.
    */
    if( iNext>=MergeCount(p) ){
      fts3LogMerge(16, getAbsoluteLevel(p, iLangid, iIndex, iLevel));
      rc = fts3SegmentMerge(p, iLangid, iIndex, iLevel);
      *piIdx = 0;
    }else{
      *piIdx = iNext;
    }
  }

  return rc;
}

/*
** The %_segments table is declared as follows:
**
**   CREATE TABLE %_segments(blockid INTEGER PRIMARY KEY, block BLOB)
**
** This function reads data from a single row of the %_segments table. The
** specific row is identified by the iBlockid parameter. If paBlob is not
** NULL, then a buffer is allocated using sqlite3_malloc() and populated
** with the contents of the blob stored in the "block" column of the
** identified table row is. Whether or not paBlob is NULL, *pnBlob is set
** to the size of the blob in bytes before returning.
**
** If an error occurs, or the table does not contain the specified row,
** an SQLite error code is returned. Otherwise, SQLITE_OK is returned. If
** paBlob is non-NULL, then it is the responsibility of the caller to
** eventually free the returned buffer.
**
** This function may leave an open sqlite3_blob* handle in the
** Fts3Table.pSegments variable. This handle is reused by subsequent calls
** to this function. The handle may be closed by calling the
** sqlite3Fts3SegmentsClose() function. Reusing a blob handle is a handy
** performance improvement, but the blob handle should always be closed
** before control is returned to the user (to prevent a lock being held
** on the database file for longer than necessary). Thus, any virtual table
** method (xFilter etc.) that may directly or indirectly call this function
** must call sqlite3Fts3SegmentsClose() before returning.
*/
SQLITE_PRIVATE int sqlite3Fts3ReadBlock(
  Fts3Table *p,                   /* FTS3 table handle */
  sqlite3_int64 iBlockid,         /* Access the row with blockid=$iBlockid */
  char **paBlob,                  /* OUT: Blob data in malloc'd buffer */
  int *pnBlob,                    /* OUT: Size of blob data */
  int *pnLoad                     /* OUT: Bytes actually loaded */
){
  int rc;                         /* Return code */

  /* pnBlob must be non-NULL. paBlob may be NULL or non-NULL. */
  assert( pnBlob );

  if( p->pSegments ){
    rc = sqlite3_blob_reopen(p->pSegments, iBlockid);
  }else{
    if( 0==p->zSegmentsTbl ){
      p->zSegmentsTbl = sqlite3_mprintf("%s_segments", p->zName);
      if( 0==p->zSegmentsTbl ) return SQLITE_NOMEM;
    }
    rc = sqlite3_blob_open(
       p->db, p->zDb, p->zSegmentsTbl, "block", iBlockid, 0, &p->pSegments
    );
  }

  if( rc==SQLITE_OK ){
    int nByte = sqlite3_blob_bytes(p->pSegments);
    *pnBlob = nByte;
    if( paBlob ){
      char *aByte = sqlite3_malloc64((i64)nByte + FTS3_NODE_PADDING);
      if( !aByte ){
        rc = SQLITE_NOMEM;
      }else{
        if( pnLoad && nByte>(FTS3_NODE_CHUNK_THRESHOLD) ){
          nByte = FTS3_NODE_CHUNKSIZE;
          *pnLoad = nByte;
        }
        rc = sqlite3_blob_read(p->pSegments, aByte, nByte, 0);
        memset(&aByte[nByte], 0, FTS3_NODE_PADDING);
        if( rc!=SQLITE_OK ){
          sqlite3_free(aByte);
          aByte = 0;
        }
      }
      *paBlob = aByte;
    }
  }else if( rc==SQLITE_ERROR ){
    rc = FTS_CORRUPT_VTAB;
  }

  return rc;
}
