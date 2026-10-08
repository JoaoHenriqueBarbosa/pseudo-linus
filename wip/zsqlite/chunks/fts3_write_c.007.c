
static int fts3SegReaderStart(
  Fts3Table *p,                   /* Virtual table handle */
  Fts3MultiSegReader *pCsr,       /* Cursor object */
  const char *zTerm,              /* Term searched for (or NULL) */
  int nTerm                       /* Length of zTerm in bytes */
){
  int i;
  int nSeg = pCsr->nSegment;

  /* If the Fts3SegFilter defines a specific term (or term prefix) to search
  ** for, then advance each segment iterator until it points to a term of
  ** equal or greater value than the specified term. This prevents many
  ** unnecessary merge/sort operations for the case where single segment
  ** b-tree leaf nodes contain more than one term.
  */
  for(i=0; pCsr->bRestart==0 && i<pCsr->nSegment; i++){
    int res = 0;
    Fts3SegReader *pSeg = pCsr->apSegment[i];
    do {
      int rc = fts3SegReaderNext(p, pSeg, 0);
      if( rc!=SQLITE_OK ) return rc;
    }while( zTerm && (res = fts3SegReaderTermCmp(pSeg, zTerm, nTerm))<0 );

    if( pSeg->bLookup && res!=0 ){
      fts3SegReaderSetEof(pSeg);
    }
  }
  fts3SegReaderSort(pCsr->apSegment, nSeg, nSeg, fts3SegReaderCmp);

  return SQLITE_OK;
}

SQLITE_PRIVATE int sqlite3Fts3SegReaderStart(
  Fts3Table *p,                   /* Virtual table handle */
  Fts3MultiSegReader *pCsr,       /* Cursor object */
  Fts3SegFilter *pFilter          /* Restrictions on range of iteration */
){
  pCsr->pFilter = pFilter;
  return fts3SegReaderStart(p, pCsr, pFilter->zTerm, pFilter->nTerm);
}

SQLITE_PRIVATE int sqlite3Fts3MsrIncrStart(
  Fts3Table *p,                   /* Virtual table handle */
  Fts3MultiSegReader *pCsr,       /* Cursor object */
  int iCol,                       /* Column to match on. */
  const char *zTerm,              /* Term to iterate through a doclist for */
  int nTerm                       /* Number of bytes in zTerm */
){
  int i;
  int rc;
  int nSegment = pCsr->nSegment;
  int (*xCmp)(Fts3SegReader *, Fts3SegReader *) = (
    p->bDescIdx ? fts3SegReaderDoclistCmpRev : fts3SegReaderDoclistCmp
  );

  assert( pCsr->pFilter==0 );
  assert( zTerm && nTerm>0 );

  /* Advance each segment iterator until it points to the term zTerm/nTerm. */
  rc = fts3SegReaderStart(p, pCsr, zTerm, nTerm);
  if( rc!=SQLITE_OK ) return rc;

  /* Determine how many of the segments actually point to zTerm/nTerm. */
  for(i=0; i<nSegment; i++){
    Fts3SegReader *pSeg = pCsr->apSegment[i];
    if( !pSeg->aNode || fts3SegReaderTermCmp(pSeg, zTerm, nTerm) ){
      break;
    }
  }
  pCsr->nAdvance = i;

  /* Advance each of the segments to point to the first docid. */
  for(i=0; i<pCsr->nAdvance; i++){
    rc = fts3SegReaderFirstDocid(p, pCsr->apSegment[i]);
    if( rc!=SQLITE_OK ) return rc;
  }
  fts3SegReaderSort(pCsr->apSegment, i, i, xCmp);

  assert( iCol<0 || iCol<p->nColumn );
  pCsr->iColFilter = iCol;

  return SQLITE_OK;
}

/*
** This function is called on a MultiSegReader that has been started using
** sqlite3Fts3MsrIncrStart(). One or more calls to MsrIncrNext() may also
** have been made. Calling this function puts the MultiSegReader in such
** a state that if the next two calls are:
**
**   sqlite3Fts3SegReaderStart()
**   sqlite3Fts3SegReaderStep()
**
** then the entire doclist for the term is available in
** MultiSegReader.aDoclist/nDoclist.
*/
SQLITE_PRIVATE int sqlite3Fts3MsrIncrRestart(Fts3MultiSegReader *pCsr){
  int i;                          /* Used to iterate through segment-readers */

  assert( pCsr->zTerm==0 );
  assert( pCsr->nTerm==0 );
  assert( pCsr->aDoclist==0 );
  assert( pCsr->nDoclist==0 );

  pCsr->nAdvance = 0;
  pCsr->bRestart = 1;
  for(i=0; i<pCsr->nSegment; i++){
    pCsr->apSegment[i]->pOffsetList = 0;
    pCsr->apSegment[i]->nOffsetList = 0;
    pCsr->apSegment[i]->iDocid = 0;
  }

  return SQLITE_OK;
}

static int fts3GrowSegReaderBuffer(Fts3MultiSegReader *pCsr, i64 nReq){
  if( nReq>pCsr->nBuffer ){
    char *aNew;
    pCsr->nBuffer = nReq*2;
    aNew = sqlite3_realloc64(pCsr->aBuffer, pCsr->nBuffer);
    if( !aNew ){
      return SQLITE_NOMEM;
    }
    pCsr->aBuffer = aNew;
  }
  return SQLITE_OK;
}


SQLITE_PRIVATE int sqlite3Fts3SegReaderStep(
  Fts3Table *p,                   /* Virtual table handle */
  Fts3MultiSegReader *pCsr        /* Cursor object */
){
  int rc = SQLITE_OK;

  int isIgnoreEmpty =  (pCsr->pFilter->flags & FTS3_SEGMENT_IGNORE_EMPTY);
  int isRequirePos =   (pCsr->pFilter->flags & FTS3_SEGMENT_REQUIRE_POS);
  int isColFilter =    (pCsr->pFilter->flags & FTS3_SEGMENT_COLUMN_FILTER);
  int isPrefix =       (pCsr->pFilter->flags & FTS3_SEGMENT_PREFIX);
  int isScan =         (pCsr->pFilter->flags & FTS3_SEGMENT_SCAN);
  int isFirst =        (pCsr->pFilter->flags & FTS3_SEGMENT_FIRST);

  Fts3SegReader **apSegment = pCsr->apSegment;
  int nSegment = pCsr->nSegment;
  Fts3SegFilter *pFilter = pCsr->pFilter;
  int (*xCmp)(Fts3SegReader *, Fts3SegReader *) = (
    p->bDescIdx ? fts3SegReaderDoclistCmpRev : fts3SegReaderDoclistCmp
  );

  if( pCsr->nSegment==0 ) return SQLITE_OK;

  do {
    int nMerge;
    int i;

    /* Advance the first pCsr->nAdvance entries in the apSegment[] array
    ** forward. Then sort the list in order of current term again.
    */
    for(i=0; i<pCsr->nAdvance; i++){
      Fts3SegReader *pSeg = apSegment[i];
      if( pSeg->bLookup ){
        fts3SegReaderSetEof(pSeg);
      }else{
        rc = fts3SegReaderNext(p, pSeg, 0);
      }
      if( rc!=SQLITE_OK ) return rc;
    }
    fts3SegReaderSort(apSegment, nSegment, pCsr->nAdvance, fts3SegReaderCmp);
    pCsr->nAdvance = 0;

    /* If all the seg-readers are at EOF, we're finished. return SQLITE_OK. */
    assert( rc==SQLITE_OK );
    if( apSegment[0]->aNode==0 ) break;

    pCsr->nTerm = apSegment[0]->nTerm;
    pCsr->zTerm = apSegment[0]->zTerm;

    /* If this is a prefix-search, and if the term that apSegment[0] points
    ** to does not share a suffix with pFilter->zTerm/nTerm, then all
    ** required callbacks have been made. In this case exit early.
    **
    ** Similarly, if this is a search for an exact match, and the first term
    ** of segment apSegment[0] is not a match, exit early.
    */
    if( pFilter->zTerm && !isScan ){
      if( pCsr->nTerm<pFilter->nTerm
       || (!isPrefix && pCsr->nTerm>pFilter->nTerm)
       || memcmp(pCsr->zTerm, pFilter->zTerm, pFilter->nTerm)
      ){
        break;
      }
    }

    nMerge = 1;
    while( nMerge<nSegment
        && apSegment[nMerge]->aNode
        && apSegment[nMerge]->nTerm==pCsr->nTerm
        && 0==memcmp(pCsr->zTerm, apSegment[nMerge]->zTerm, pCsr->nTerm)
    ){
      nMerge++;
    }

    assert( isIgnoreEmpty || (isRequirePos && !isColFilter) );
    if( nMerge==1
     && !isIgnoreEmpty
     && !isFirst
     && (p->bDescIdx==0 || fts3SegReaderIsPending(apSegment[0])==0)
    ){
      pCsr->nDoclist = apSegment[0]->nDoclist;
      if( fts3SegReaderIsPending(apSegment[0]) ){
        rc = fts3MsrBufferData(pCsr, apSegment[0]->aDoclist,
                               (i64)pCsr->nDoclist);
        pCsr->aDoclist = pCsr->aBuffer;
      }else{
        pCsr->aDoclist = apSegment[0]->aDoclist;
      }
      if( rc==SQLITE_OK ) rc = SQLITE_ROW;
    }else{
      int nDoclist = 0;           /* Size of doclist */
      sqlite3_int64 iPrev = 0;    /* Previous docid stored in doclist */

      /* The current term of the first nMerge entries in the array
      ** of Fts3SegReader objects is the same. The doclists must be merged
      ** and a single term returned with the merged doclist.
      */
      for(i=0; i<nMerge; i++){
        fts3SegReaderFirstDocid(p, apSegment[i]);
      }
      fts3SegReaderSort(apSegment, nMerge, nMerge, xCmp);
      while( apSegment[0]->pOffsetList ){
        int j;                    /* Number of segments that share a docid */
        char *pList = 0;
        int nList = 0;
        int nByte;
        sqlite3_int64 iDocid = apSegment[0]->iDocid;
        fts3SegReaderNextDocid(p, apSegment[0], &pList, &nList);
        j = 1;
        while( j<nMerge
            && apSegment[j]->pOffsetList
            && apSegment[j]->iDocid==iDocid
        ){
          fts3SegReaderNextDocid(p, apSegment[j], 0, 0);
          j++;
        }

        if( isColFilter ){
          fts3ColumnFilter(pFilter->iCol, 0, &pList, &nList);
        }

        if( !isIgnoreEmpty || nList>0 ){

          /* Calculate the 'docid' delta value to write into the merged
          ** doclist. */
          sqlite3_int64 iDelta;
          if( p->bDescIdx && nDoclist>0 ){
            if( iPrev<=iDocid ) return FTS_CORRUPT_VTAB;
            iDelta = (i64)((u64)iPrev - (u64)iDocid);
          }else{
            if( nDoclist>0 && iPrev>=iDocid ) return FTS_CORRUPT_VTAB;
            iDelta = (i64)((u64)iDocid - (u64)iPrev);
          }

          nByte = sqlite3Fts3VarintLen(iDelta) + (isRequirePos?nList+1:0);

          rc = fts3GrowSegReaderBuffer(pCsr,
                                   (i64)nByte+nDoclist+FTS3_NODE_PADDING);
          if( rc ) return rc;

          if( isFirst ){
            char *a = &pCsr->aBuffer[nDoclist];
            int nWrite;

            nWrite = sqlite3Fts3FirstFilter(iDelta, pList, nList, a);
            if( nWrite ){
              iPrev = iDocid;
              nDoclist += nWrite;
            }
          }else{
            nDoclist += sqlite3Fts3PutVarint(&pCsr->aBuffer[nDoclist], iDelta);
            iPrev = iDocid;
            if( isRequirePos ){
              memcpy(&pCsr->aBuffer[nDoclist], pList, nList);
              nDoclist += nList;
              pCsr->aBuffer[nDoclist++] = '\0';
            }
          }
        }

        fts3SegReaderSort(apSegment, nMerge, j, xCmp);
      }
      if( nDoclist>0 ){
        rc = fts3GrowSegReaderBuffer(pCsr, (i64)nDoclist+FTS3_NODE_PADDING);
        if( rc ) return rc;
        memset(&pCsr->aBuffer[nDoclist], 0, FTS3_NODE_PADDING);
        pCsr->aDoclist = pCsr->aBuffer;
        pCsr->nDoclist = nDoclist;
        rc = SQLITE_ROW;
      }
    }
    pCsr->nAdvance = nMerge;
  }while( rc==SQLITE_OK );

  return rc;
}


SQLITE_PRIVATE void sqlite3Fts3SegReaderFinish(
  Fts3MultiSegReader *pCsr       /* Cursor object */
){
  if( pCsr ){
    int i;
    for(i=0; i<pCsr->nSegment; i++){
      sqlite3Fts3SegReaderFree(pCsr->apSegment[i]);
    }
    sqlite3_free(pCsr->apSegment);
    sqlite3_free(pCsr->aBuffer);

    pCsr->nSegment = 0;
    pCsr->apSegment = 0;
    pCsr->aBuffer = 0;
  }
}

/*
** Decode the "end_block" field, selected by column iCol of the SELECT
** statement passed as the first argument.
**
** The "end_block" field may contain either an integer, or a text field
** containing the text representation of two non-negative integers separated
** by one or more space (0x20) characters. In the first case, set *piEndBlock
** to the integer value and *pnByte to zero before returning. In the second,
** set *piEndBlock to the first value and *pnByte to the second.
*/
static void fts3ReadEndBlockField(
  sqlite3_stmt *pStmt,
  int iCol,
  i64 *piEndBlock,
  i64 *pnByte
){
  const unsigned char *zText = sqlite3_column_text(pStmt, iCol);
  if( zText ){
    int i;
    int iMul = 1;
    u64 iVal = 0;
    for(i=0; zText[i]>='0' && zText[i]<='9'; i++){
      iVal = iVal*10 + (zText[i] - '0');
    }
    *piEndBlock = (i64)iVal;
    while( zText[i]==' ' ) i++;
    iVal = 0;
    if( zText[i]=='-' ){
      i++;
      iMul = -1;
    }
    for(/* no-op */; zText[i]>='0' && zText[i]<='9'; i++){
      iVal = iVal*10 + (zText[i] - '0');
    }
    *pnByte = ((i64)iVal * (i64)iMul);
  }
}
