
/*
** Flush all data associated with the SegmentWriter object pWriter to the
** database. This function must be called after all terms have been added
** to the segment using fts3SegWriterAdd(). If successful, SQLITE_OK is
** returned. Otherwise, an SQLite error code.
*/
static int fts3SegWriterFlush(
  Fts3Table *p,                   /* Virtual table handle */
  SegmentWriter *pWriter,         /* SegmentWriter to flush to the db */
  sqlite3_int64 iLevel,           /* Value for 'level' column of %_segdir */
  int iIdx                        /* Value for 'idx' column of %_segdir */
){
  int rc;                         /* Return code */
  if( pWriter->pTree ){
    sqlite3_int64 iLast = 0;      /* Largest block id written to database */
    sqlite3_int64 iLastLeaf;      /* Largest leaf block id written to db */
    char *zRoot = NULL;           /* Pointer to buffer containing root node */
    int nRoot = 0;                /* Size of buffer zRoot */

    iLastLeaf = pWriter->iFree;
    rc = fts3WriteSegment(p, pWriter->iFree++, pWriter->aData, pWriter->nData);
    if( rc==SQLITE_OK ){
      rc = fts3NodeWrite(p, pWriter->pTree, 1,
          pWriter->iFirst, pWriter->iFree, &iLast, &zRoot, &nRoot);
    }
    if( rc==SQLITE_OK ){
      rc = fts3WriteSegdir(p, iLevel, iIdx,
          pWriter->iFirst, iLastLeaf, iLast, pWriter->nLeafData, zRoot, nRoot);
    }
  }else{
    /* The entire tree fits on the root node. Write it to the segdir table. */
    rc = fts3WriteSegdir(p, iLevel, iIdx,
        0, 0, 0, pWriter->nLeafData, pWriter->aData, pWriter->nData);
  }
  p->nLeafAdd++;
  return rc;
}

/*
** Release all memory held by the SegmentWriter object passed as the
** first argument.
*/
static void fts3SegWriterFree(SegmentWriter *pWriter){
  if( pWriter ){
    sqlite3_free(pWriter->aData);
    sqlite3_free(pWriter->zMalloc);
    fts3NodeFree(pWriter->pTree);
    sqlite3_free(pWriter);
  }
}

/*
** The first value in the apVal[] array is assumed to contain an integer.
** This function tests if there exist any documents with docid values that
** are different from that integer. i.e. if deleting the document with docid
** pRowid would mean the FTS3 table were empty.
**
** If successful, *pisEmpty is set to true if the table is empty except for
** document pRowid, or false otherwise, and SQLITE_OK is returned. If an
** error occurs, an SQLite error code is returned.
*/
static int fts3IsEmpty(Fts3Table *p, sqlite3_value *pRowid, int *pisEmpty){
  sqlite3_stmt *pStmt;
  int rc;
  if( p->zContentTbl ){
    /* If using the content=xxx option, assume the table is never empty */
    *pisEmpty = 0;
    rc = SQLITE_OK;
  }else{
    rc = fts3SqlStmt(p, SQL_IS_EMPTY, &pStmt, &pRowid);
    if( rc==SQLITE_OK ){
      if( SQLITE_ROW==sqlite3_step(pStmt) ){
        *pisEmpty = sqlite3_column_int(pStmt, 0);
      }
      rc = sqlite3_reset(pStmt);
    }
  }
  return rc;
}

/*
** Set *pnMax to the largest segment level in the database for the index
** iIndex.
**
** Segment levels are stored in the 'level' column of the %_segdir table.
**
** Return SQLITE_OK if successful, or an SQLite error code if not.
*/
static int fts3SegmentMaxLevel(
  Fts3Table *p,
  int iLangid,
  int iIndex,
  sqlite3_int64 *pnMax
){
  sqlite3_stmt *pStmt;
  int rc;
  assert( iIndex>=0 && iIndex<p->nIndex );

  /* Set pStmt to the compiled version of:
  **
  **   SELECT max(level) FROM %Q.'%q_segdir' WHERE level BETWEEN ? AND ?
  **
  ** (1024 is actually the value of macro FTS3_SEGDIR_PREFIXLEVEL_STR).
  */
  rc = fts3SqlStmt(p, SQL_SELECT_SEGDIR_MAX_LEVEL, &pStmt, 0);
  if( rc!=SQLITE_OK ) return rc;
  sqlite3_bind_int64(pStmt, 1, getAbsoluteLevel(p, iLangid, iIndex, 0));
  sqlite3_bind_int64(pStmt, 2,
      getAbsoluteLevel(p, iLangid, iIndex, FTS3_SEGDIR_MAXLEVEL-1)
  );
  if( SQLITE_ROW==sqlite3_step(pStmt) ){
    *pnMax = sqlite3_column_int64(pStmt, 0);
  }
  return sqlite3_reset(pStmt);
}

/*
** iAbsLevel is an absolute level that may be assumed to exist within
** the database. This function checks if it is the largest level number
** within its index. Assuming no error occurs, *pbMax is set to 1 if
** iAbsLevel is indeed the largest level, or 0 otherwise, and SQLITE_OK
** is returned. If an error occurs, an error code is returned and the
** final value of *pbMax is undefined.
*/
static int fts3SegmentIsMaxLevel(Fts3Table *p, i64 iAbsLevel, int *pbMax){

  /* Set pStmt to the compiled version of:
  **
  **   SELECT max(level) FROM %Q.'%q_segdir' WHERE level BETWEEN ? AND ?
  **
  ** (1024 is actually the value of macro FTS3_SEGDIR_PREFIXLEVEL_STR).
  */
  sqlite3_stmt *pStmt;
  int rc = fts3SqlStmt(p, SQL_SELECT_SEGDIR_MAX_LEVEL, &pStmt, 0);
  if( rc!=SQLITE_OK ) return rc;
  sqlite3_bind_int64(pStmt, 1, iAbsLevel+1);
  sqlite3_bind_int64(pStmt, 2,
      (((u64)iAbsLevel/FTS3_SEGDIR_MAXLEVEL)+1) * FTS3_SEGDIR_MAXLEVEL
  );

  *pbMax = 0;
  if( SQLITE_ROW==sqlite3_step(pStmt) ){
    *pbMax = sqlite3_column_type(pStmt, 0)==SQLITE_NULL;
  }
  return sqlite3_reset(pStmt);
}

/*
** Delete all entries in the %_segments table associated with the segment
** opened with seg-reader pSeg. This function does not affect the contents
** of the %_segdir table.
*/
static int fts3DeleteSegment(
  Fts3Table *p,                   /* FTS table handle */
  Fts3SegReader *pSeg             /* Segment to delete */
){
  int rc = SQLITE_OK;             /* Return code */
  if( pSeg->iStartBlock ){
    sqlite3_stmt *pDelete;        /* SQL statement to delete rows */
    rc = fts3SqlStmt(p, SQL_DELETE_SEGMENTS_RANGE, &pDelete, 0);
    if( rc==SQLITE_OK ){
      sqlite3_bind_int64(pDelete, 1, pSeg->iStartBlock);
      sqlite3_bind_int64(pDelete, 2, pSeg->iEndBlock);
      sqlite3_step(pDelete);
      rc = sqlite3_reset(pDelete);
    }
  }
  return rc;
}

/*
** This function is used after merging multiple segments into a single large
** segment to delete the old, now redundant, segment b-trees. Specifically,
** it:
**
**   1) Deletes all %_segments entries for the segments associated with
**      each of the SegReader objects in the array passed as the third
**      argument, and
**
**   2) deletes all %_segdir entries with level iLevel, or all %_segdir
**      entries regardless of level if (iLevel<0).
**
** SQLITE_OK is returned if successful, otherwise an SQLite error code.
*/
static int fts3DeleteSegdir(
  Fts3Table *p,                   /* Virtual table handle */
  int iLangid,                    /* Language id */
  int iIndex,                     /* Index for p->aIndex */
  int iLevel,                     /* Level of %_segdir entries to delete */
  Fts3SegReader **apSegment,      /* Array of SegReader objects */
  int nReader                     /* Size of array apSegment */
){
  int rc = SQLITE_OK;             /* Return Code */
  int i;                          /* Iterator variable */
  sqlite3_stmt *pDelete = 0;      /* SQL statement to delete rows */

  for(i=0; rc==SQLITE_OK && i<nReader; i++){
    rc = fts3DeleteSegment(p, apSegment[i]);
  }
  if( rc!=SQLITE_OK ){
    return rc;
  }

  assert( iLevel>=0 || iLevel==FTS3_SEGCURSOR_ALL );
  if( iLevel==FTS3_SEGCURSOR_ALL ){
    rc = fts3SqlStmt(p, SQL_DELETE_SEGDIR_RANGE, &pDelete, 0);
    if( rc==SQLITE_OK ){
      sqlite3_bind_int64(pDelete, 1, getAbsoluteLevel(p, iLangid, iIndex, 0));
      sqlite3_bind_int64(pDelete, 2,
          getAbsoluteLevel(p, iLangid, iIndex, FTS3_SEGDIR_MAXLEVEL-1)
      );
    }
  }else{
    rc = fts3SqlStmt(p, SQL_DELETE_SEGDIR_LEVEL, &pDelete, 0);
    if( rc==SQLITE_OK ){
      sqlite3_bind_int64(
          pDelete, 1, getAbsoluteLevel(p, iLangid, iIndex, iLevel)
      );
    }
  }

  if( rc==SQLITE_OK ){
    sqlite3_step(pDelete);
    rc = sqlite3_reset(pDelete);
  }

  return rc;
}

/*
** When this function is called, buffer *ppList (size *pnList bytes) contains
** a position list that may (or may not) feature multiple columns. This
** function adjusts the pointer *ppList and the length *pnList so that they
** identify the subset of the position list that corresponds to column iCol.
**
** If there are no entries in the input position list for column iCol, then
** *pnList is set to zero before returning.
**
** If parameter bZero is non-zero, then any part of the input list following
** the end of the output list is zeroed before returning.
*/
static void fts3ColumnFilter(
  int iCol,                       /* Column to filter on */
  int bZero,                      /* Zero out anything following *ppList */
  char **ppList,                  /* IN/OUT: Pointer to position list */
  int *pnList                     /* IN/OUT: Size of buffer *ppList in bytes */
){
  char *pList = *ppList;
  int nList = *pnList;
  char *pEnd = &pList[nList];
  int iCurrent = 0;
  char *p = pList;

  assert( iCol>=0 );
  while( 1 ){
    char c = 0;
    while( p<pEnd && (c | *p)&0xFE ) c = *p++ & 0x80;

    if( iCol==iCurrent ){
      nList = (int)(p - pList);
      break;
    }

    nList -= (int)(p - pList);
    pList = p;
    if( nList<=0 ){
      break;
    }
    p = &pList[1];
    p += fts3GetVarint32(p, &iCurrent);
  }

  if( bZero && (pEnd - &pList[nList])>0){
    memset(&pList[nList], 0, pEnd - &pList[nList]);
  }
  *ppList = pList;
  *pnList = nList;
}

/*
** Cache data in the Fts3MultiSegReader.aBuffer[] buffer (overwriting any
** existing data). Grow the buffer if required.
**
** If successful, return SQLITE_OK. Otherwise, if an OOM error is encountered
** trying to resize the buffer, return SQLITE_NOMEM.
*/
static int fts3MsrBufferData(
  Fts3MultiSegReader *pMsr,       /* Multi-segment-reader handle */
  char *pList,
  i64 nList
){
  if( (nList+FTS3_NODE_PADDING)>pMsr->nBuffer ){
    char *pNew;
    int nNew = nList*2 + FTS3_NODE_PADDING;
    pNew = (char *)sqlite3_realloc64(pMsr->aBuffer, nNew);
    if( !pNew ) return SQLITE_NOMEM;
    pMsr->aBuffer = pNew;
    pMsr->nBuffer = nNew;
  }

  assert( nList>0 );
  memcpy(pMsr->aBuffer, pList, nList);
  memset(&pMsr->aBuffer[nList], 0, FTS3_NODE_PADDING);
  return SQLITE_OK;
}

SQLITE_PRIVATE int sqlite3Fts3MsrIncrNext(
  Fts3Table *p,                   /* Virtual table handle */
  Fts3MultiSegReader *pMsr,       /* Multi-segment-reader handle */
  sqlite3_int64 *piDocid,         /* OUT: Docid value */
  char **paPoslist,               /* OUT: Pointer to position list */
  int *pnPoslist                  /* OUT: Size of position list in bytes */
){
  int nMerge = pMsr->nAdvance;
  Fts3SegReader **apSegment = pMsr->apSegment;
  int (*xCmp)(Fts3SegReader *, Fts3SegReader *) = (
    p->bDescIdx ? fts3SegReaderDoclistCmpRev : fts3SegReaderDoclistCmp
  );

  if( nMerge==0 ){
    *paPoslist = 0;
    return SQLITE_OK;
  }

  while( 1 ){
    Fts3SegReader *pSeg;
    pSeg = pMsr->apSegment[0];

    if( pSeg->pOffsetList==0 ){
      *paPoslist = 0;
      break;
    }else{
      int rc;
      char *pList;
      int nList;
      int j;
      sqlite3_int64 iDocid = apSegment[0]->iDocid;

      rc = fts3SegReaderNextDocid(p, apSegment[0], &pList, &nList);
      j = 1;
      while( rc==SQLITE_OK
        && j<nMerge
        && apSegment[j]->pOffsetList
        && apSegment[j]->iDocid==iDocid
      ){
        rc = fts3SegReaderNextDocid(p, apSegment[j], 0, 0);
        j++;
      }
      if( rc!=SQLITE_OK ) return rc;
      fts3SegReaderSort(pMsr->apSegment, nMerge, j, xCmp);

      if( nList>0 && fts3SegReaderIsPending(apSegment[0]) ){
        rc = fts3MsrBufferData(pMsr, pList, (i64)nList+1);
        if( rc!=SQLITE_OK ) return rc;
        assert( (pMsr->aBuffer[nList] & 0xFE)==0x00 );
        pList = pMsr->aBuffer;
      }

      if( pMsr->iColFilter>=0 ){
        fts3ColumnFilter(pMsr->iColFilter, 1, &pList, &nList);
      }

      if( nList>0 ){
        *paPoslist = pList;
        *piDocid = iDocid;
        *pnPoslist = nList;
        break;
      }
    }
  }

  return SQLITE_OK;
}
