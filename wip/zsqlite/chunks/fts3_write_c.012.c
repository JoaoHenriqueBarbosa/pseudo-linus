
/*
** This function is called after an incrmental-merge operation has run to
** merge (or partially merge) two or more segments from absolute level
** iAbsLevel.
**
** Each input segment is either removed from the db completely (if all of
** its data was copied to the output segment by the incrmerge operation)
** or modified in place so that it no longer contains those entries that
** have been duplicated in the output segment.
*/
static int fts3IncrmergeChomp(
  Fts3Table *p,                   /* FTS table handle */
  sqlite3_int64 iAbsLevel,        /* Absolute level containing segments */
  Fts3MultiSegReader *pCsr,       /* Chomp all segments opened by this cursor */
  int *pnRem                      /* Number of segments not deleted */
){
  int i;
  int nRem = 0;
  int rc = SQLITE_OK;

  for(i=pCsr->nSegment-1; i>=0 && rc==SQLITE_OK; i--){
    Fts3SegReader *pSeg = 0;
    int j;

    /* Find the Fts3SegReader object with Fts3SegReader.iIdx==i. It is hiding
    ** somewhere in the pCsr->apSegment[] array.  */
    for(j=0; ALWAYS(j<pCsr->nSegment); j++){
      pSeg = pCsr->apSegment[j];
      if( pSeg->iIdx==i ) break;
    }
    assert( j<pCsr->nSegment && pSeg->iIdx==i );

    if( pSeg->aNode==0 ){
      /* Seg-reader is at EOF. Remove the entire input segment. */
      rc = fts3DeleteSegment(p, pSeg);
      if( rc==SQLITE_OK ){
        rc = fts3RemoveSegdirEntry(p, iAbsLevel, pSeg->iIdx);
      }
      *pnRem = 0;
    }else{
      /* The incremental merge did not copy all the data from this
      ** segment to the upper level. The segment is modified in place
      ** so that it contains no keys smaller than zTerm/nTerm. */
      const char *zTerm = pSeg->zTerm;
      int nTerm = pSeg->nTerm;
      rc = fts3TruncateSegment(p, iAbsLevel, pSeg->iIdx, zTerm, nTerm);
      nRem++;
    }
  }

  if( rc==SQLITE_OK && nRem!=pCsr->nSegment ){
    rc = fts3RepackSegdirLevel(p, iAbsLevel);
  }

  *pnRem = nRem;
  return rc;
}

/*
** Store an incr-merge hint in the database.
*/
static int fts3IncrmergeHintStore(Fts3Table *p, Blob *pHint){
  sqlite3_stmt *pReplace = 0;
  int rc;                         /* Return code */

  rc = fts3SqlStmt(p, SQL_REPLACE_STAT, &pReplace, 0);
  if( rc==SQLITE_OK ){
    sqlite3_bind_int(pReplace, 1, FTS_STAT_INCRMERGEHINT);
    sqlite3_bind_blob(pReplace, 2, pHint->a, pHint->n, SQLITE_STATIC);
    sqlite3_step(pReplace);
    rc = sqlite3_reset(pReplace);
    sqlite3_bind_null(pReplace, 2);
  }

  return rc;
}

/*
** Load an incr-merge hint from the database. The incr-merge hint, if one
** exists, is stored in the rowid==1 row of the %_stat table.
**
** If successful, populate blob *pHint with the value read from the %_stat
** table and return SQLITE_OK. Otherwise, if an error occurs, return an
** SQLite error code.
*/
static int fts3IncrmergeHintLoad(Fts3Table *p, Blob *pHint){
  sqlite3_stmt *pSelect = 0;
  int rc;

  pHint->n = 0;
  rc = fts3SqlStmt(p, SQL_SELECT_STAT, &pSelect, 0);
  if( rc==SQLITE_OK ){
    int rc2;
    sqlite3_bind_int(pSelect, 1, FTS_STAT_INCRMERGEHINT);
    if( SQLITE_ROW==sqlite3_step(pSelect) ){
      const char *aHint = sqlite3_column_blob(pSelect, 0);
      int nHint = sqlite3_column_bytes(pSelect, 0);
      if( aHint ){
        blobGrowBuffer(pHint, nHint, &rc);
        if( rc==SQLITE_OK ){
          if( ALWAYS(pHint->a!=0) ) memcpy(pHint->a, aHint, nHint);
          pHint->n = nHint;
        }
      }
    }
    rc2 = sqlite3_reset(pSelect);
    if( rc==SQLITE_OK ) rc = rc2;
  }

  return rc;
}

/*
** If *pRc is not SQLITE_OK when this function is called, it is a no-op.
** Otherwise, append an entry to the hint stored in blob *pHint. Each entry
** consists of two varints, the absolute level number of the input segments
** and the number of input segments.
**
** If successful, leave *pRc set to SQLITE_OK and return. If an error occurs,
** set *pRc to an SQLite error code before returning.
*/
static void fts3IncrmergeHintPush(
  Blob *pHint,                    /* Hint blob to append to */
  i64 iAbsLevel,                  /* First varint to store in hint */
  int nInput,                     /* Second varint to store in hint */
  int *pRc                        /* IN/OUT: Error code */
){
  blobGrowBuffer(pHint, pHint->n + 2*FTS3_VARINT_MAX, pRc);
  if( *pRc==SQLITE_OK ){
    pHint->n += sqlite3Fts3PutVarint(&pHint->a[pHint->n], iAbsLevel);
    pHint->n += sqlite3Fts3PutVarint(&pHint->a[pHint->n], (i64)nInput);
  }
}

/*
** Read the last entry (most recently pushed) from the hint blob *pHint
** and then remove the entry. Write the two values read to *piAbsLevel and
** *pnInput before returning.
**
** If no error occurs, return SQLITE_OK. If the hint blob in *pHint does
** not contain at least two valid varints, return SQLITE_CORRUPT_VTAB.
*/
static int fts3IncrmergeHintPop(Blob *pHint, i64 *piAbsLevel, int *pnInput){
  const int nHint = pHint->n;
  int i;

  i = pHint->n-1;
  if( (pHint->a[i] & 0x80) ) return FTS_CORRUPT_VTAB;
  while( i>0 && (pHint->a[i-1] & 0x80) ) i--;
  if( i==0 ) return FTS_CORRUPT_VTAB;
  i--;
  while( i>0 && (pHint->a[i-1] & 0x80) ) i--;

  pHint->n = i;
  i += sqlite3Fts3GetVarint(&pHint->a[i], piAbsLevel);
  i += fts3GetVarint32(&pHint->a[i], pnInput);
  assert( i<=nHint );
  if( i!=nHint ) return FTS_CORRUPT_VTAB;

  return SQLITE_OK;
}


/*
** Attempt an incremental merge that writes nMerge leaf blocks.
**
** Incremental merges happen nMin segments at a time. The segments
** to be merged are the nMin oldest segments (the ones with the smallest
** values for the _segdir.idx field) in the highest level that contains
** at least nMin segments. Multiple merges might occur in an attempt to
** write the quota of nMerge leaf blocks.
*/
SQLITE_PRIVATE int sqlite3Fts3Incrmerge(Fts3Table *p, int nMerge, int nMin){
  int rc;                         /* Return code */
  int nRem = nMerge;              /* Number of leaf pages yet to  be written */
  Fts3MultiSegReader *pCsr;       /* Cursor used to read input data */
  Fts3SegFilter *pFilter;         /* Filter used with cursor pCsr */
  IncrmergeWriter *pWriter;       /* Writer object */
  int nSeg = 0;                   /* Number of input segments */
  sqlite3_int64 iAbsLevel = 0;    /* Absolute level number to work on */
  Blob hint = {0, 0, 0};          /* Hint read from %_stat table */
  int bDirtyHint = 0;             /* True if blob 'hint' has been modified */

  /* Allocate space for the cursor, filter and writer objects */
  const int nAlloc = sizeof(*pCsr) + sizeof(*pFilter) + sizeof(*pWriter);
  pWriter = (IncrmergeWriter *)sqlite3_malloc64(nAlloc);
  if( !pWriter ) return SQLITE_NOMEM;
  pFilter = (Fts3SegFilter *)&pWriter[1];
  pCsr = (Fts3MultiSegReader *)&pFilter[1];

  rc = fts3IncrmergeHintLoad(p, &hint);
  while( rc==SQLITE_OK && nRem>0 ){
    const i64 nMod = FTS3_SEGDIR_MAXLEVEL * p->nIndex;
    sqlite3_stmt *pFindLevel = 0; /* SQL used to determine iAbsLevel */
    int bUseHint = 0;             /* True if attempting to append */
    int iIdx = 0;                 /* Largest idx in level (iAbsLevel+1) */

    /* Search the %_segdir table for the absolute level with the smallest
    ** relative level number that contains at least nMin segments, if any.
    ** If one is found, set iAbsLevel to the absolute level number and
    ** nSeg to nMin. If no level with at least nMin segments can be found,
    ** set nSeg to -1.
    */
    rc = fts3SqlStmt(p, SQL_FIND_MERGE_LEVEL, &pFindLevel, 0);
    sqlite3_bind_int(pFindLevel, 1, MAX(2, nMin));
    if( sqlite3_step(pFindLevel)==SQLITE_ROW ){
      iAbsLevel = sqlite3_column_int64(pFindLevel, 0);
      nSeg = sqlite3_column_int(pFindLevel, 1);
      assert( nSeg>=2 );
    }else{
      nSeg = -1;
    }
    rc = sqlite3_reset(pFindLevel);

    /* If the hint read from the %_stat table is not empty, check if the
    ** last entry in it specifies a relative level smaller than or equal
    ** to the level identified by the block above (if any). If so, this
    ** iteration of the loop will work on merging at the hinted level.
    */
    if( rc==SQLITE_OK && hint.n ){
      int nHint = hint.n;
      sqlite3_int64 iHintAbsLevel = 0;      /* Hint level */
      int nHintSeg = 0;                     /* Hint number of segments */

      rc = fts3IncrmergeHintPop(&hint, &iHintAbsLevel, &nHintSeg);
      if( nSeg<0 || (iAbsLevel % nMod) >= (iHintAbsLevel % nMod) ){
        /* Based on the scan in the block above, it is known that there
        ** are no levels with a relative level smaller than that of
        ** iAbsLevel with more than nSeg segments, or if nSeg is -1,
        ** no levels with more than nMin segments. Use this to limit the
        ** value of nHintSeg to avoid a large memory allocation in case the
        ** merge-hint is corrupt*/
        iAbsLevel = iHintAbsLevel;
        nSeg = MIN(MAX(nMin,nSeg), nHintSeg);
        bUseHint = 1;
        bDirtyHint = 1;
      }else{
        /* This undoes the effect of the HintPop() above - so that no entry
        ** is removed from the hint blob.  */
        hint.n = nHint;
      }
    }

    /* If nSeg is less that zero, then there is no level with at least
    ** nMin segments and no hint in the %_stat table. No work to do.
    ** Exit early in this case.  */
    if( nSeg<=0 ) break;

    assert( nMod<=0x7FFFFFFF );
    if( iAbsLevel<0 || iAbsLevel>(nMod<<32) ){
      rc = FTS_CORRUPT_VTAB;
      break;
    }

    /* Open a cursor to iterate through the contents of the oldest nSeg
    ** indexes of absolute level iAbsLevel. If this cursor is opened using
    ** the 'hint' parameters, it is possible that there are less than nSeg
    ** segments available in level iAbsLevel. In this case, no work is
    ** done on iAbsLevel - fall through to the next iteration of the loop
    ** to start work on some other level.  */
    memset(pWriter, 0, nAlloc);
    pFilter->flags = FTS3_SEGMENT_REQUIRE_POS;

    if( rc==SQLITE_OK ){
      rc = fts3IncrmergeOutputIdx(p, iAbsLevel, &iIdx);
      assert( bUseHint==1 || bUseHint==0 );
      if( iIdx==0 || (bUseHint && iIdx==1) ){
        int bIgnore = 0;
        rc = fts3SegmentIsMaxLevel(p, iAbsLevel+1, &bIgnore);
        if( bIgnore ){
          pFilter->flags |= FTS3_SEGMENT_IGNORE_EMPTY;
        }
      }
    }

    if( rc==SQLITE_OK ){
      rc = fts3IncrmergeCsr(p, iAbsLevel, nSeg, pCsr);
    }
    if( SQLITE_OK==rc && pCsr->nSegment==nSeg
     && SQLITE_OK==(rc = sqlite3Fts3SegReaderStart(p, pCsr, pFilter))
    ){
      int bEmpty = 0;
      rc = sqlite3Fts3SegReaderStep(p, pCsr);
      if( rc==SQLITE_OK ){
        bEmpty = 1;
      }else if( rc!=SQLITE_ROW ){
        sqlite3Fts3SegReaderFinish(pCsr);
        break;
      }
      if( bUseHint && iIdx>0 ){
        const char *zKey = pCsr->zTerm;
        int nKey = pCsr->nTerm;
        rc = fts3IncrmergeLoad(p, iAbsLevel, iIdx-1, zKey, nKey, pWriter);
      }else{
        rc = fts3IncrmergeWriter(p, iAbsLevel, iIdx, pCsr, pWriter);
      }

      if( rc==SQLITE_OK && pWriter->nLeafEst ){
        fts3LogMerge(nSeg, iAbsLevel);
        if( bEmpty==0 ){
          do {
            rc = fts3IncrmergeAppend(p, pWriter, pCsr);
            if( rc==SQLITE_OK ) rc = sqlite3Fts3SegReaderStep(p, pCsr);
            if( pWriter->nWork>=nRem && rc==SQLITE_ROW ) rc = SQLITE_OK;
          }while( rc==SQLITE_ROW );
        }

        /* Update or delete the input segments */
        if( rc==SQLITE_OK ){
          nRem -= (1 + pWriter->nWork);
          rc = fts3IncrmergeChomp(p, iAbsLevel, pCsr, &nSeg);
          if( nSeg!=0 ){
            bDirtyHint = 1;
            fts3IncrmergeHintPush(&hint, iAbsLevel, nSeg, &rc);
          }
        }
      }

      if( nSeg!=0 ){
        pWriter->nLeafData = pWriter->nLeafData * -1;
      }
      fts3IncrmergeRelease(p, pWriter, &rc);
      if( nSeg==0 && pWriter->bNoLeafData==0 ){
        fts3PromoteSegments(p, iAbsLevel+1, pWriter->nLeafData);
      }
    }

    sqlite3Fts3SegReaderFinish(pCsr);
  }

  /* Write the hint values into the %_stat table for the next incr-merger */
  if( bDirtyHint && rc==SQLITE_OK ){
    rc = fts3IncrmergeHintStore(p, &hint);
  }

  sqlite3_free(pWriter);
  sqlite3_free(hint.a);
  return rc;
}

/*
** Convert the text beginning at *pz into an integer and return
** its value.  Advance *pz to point to the first character past
** the integer.
**
** This function used for parameters to merge= and incrmerge=
** commands.
*/
static int fts3Getint(const char **pz){
  const char *z = *pz;
  int i = 0;
  while( (*z)>='0' && (*z)<='9' && i<214748363 ) i = 10*i + *(z++) - '0';
  *pz = z;
  return i;
}
