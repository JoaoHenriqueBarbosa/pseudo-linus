
/*
** Close the blob handle at p->pSegments, if it is open. See comments above
** the sqlite3Fts3ReadBlock() function for details.
*/
SQLITE_PRIVATE void sqlite3Fts3SegmentsClose(Fts3Table *p){
  sqlite3_blob_close(p->pSegments);
  p->pSegments = 0;
}

static int fts3SegReaderIncrRead(Fts3SegReader *pReader){
  int nRead;                      /* Number of bytes to read */
  int rc;                         /* Return code */

  nRead = MIN(pReader->nNode - pReader->nPopulate, FTS3_NODE_CHUNKSIZE);
  rc = sqlite3_blob_read(
      pReader->pBlob,
      &pReader->aNode[pReader->nPopulate],
      nRead,
      pReader->nPopulate
  );

  if( rc==SQLITE_OK ){
    pReader->nPopulate += nRead;
    memset(&pReader->aNode[pReader->nPopulate], 0, FTS3_NODE_PADDING);
    if( pReader->nPopulate==pReader->nNode ){
      sqlite3_blob_close(pReader->pBlob);
      pReader->pBlob = 0;
      pReader->nPopulate = 0;
    }
  }
  return rc;
}

static int fts3SegReaderRequire(Fts3SegReader *pReader, char *pFrom, int nByte){
  int rc = SQLITE_OK;
  assert( !pReader->pBlob
       || (pFrom>=pReader->aNode && pFrom<&pReader->aNode[pReader->nNode])
  );
  while( pReader->pBlob && rc==SQLITE_OK
     &&  (pFrom - pReader->aNode + nByte)>pReader->nPopulate
  ){
    rc = fts3SegReaderIncrRead(pReader);
  }
  return rc;
}

/*
** Set an Fts3SegReader cursor to point at EOF.
*/
static void fts3SegReaderSetEof(Fts3SegReader *pSeg){
  if( !fts3SegReaderIsRootOnly(pSeg) ){
    sqlite3_free(pSeg->aNode);
    sqlite3_blob_close(pSeg->pBlob);
    pSeg->pBlob = 0;
  }
  pSeg->aNode = 0;
}

/*
** Move the iterator passed as the first argument to the next term in the
** segment. If successful, SQLITE_OK is returned. If there is no next term,
** SQLITE_DONE. Otherwise, an SQLite error code.
*/
static int fts3SegReaderNext(
  Fts3Table *p,
  Fts3SegReader *pReader,
  int bIncr
){
  int rc;                         /* Return code of various sub-routines */
  char *pNext;                    /* Cursor variable */
  int nPrefix;                    /* Number of bytes in term prefix */
  int nSuffix;                    /* Number of bytes in term suffix */

  if( !pReader->aDoclist ){
    pNext = pReader->aNode;
  }else{
    pNext = &pReader->aDoclist[pReader->nDoclist];
  }

  if( !pNext || pNext>=&pReader->aNode[pReader->nNode] ){

    if( fts3SegReaderIsPending(pReader) ){
      Fts3HashElem *pElem = *(pReader->ppNextElem);
      sqlite3_free(pReader->aNode);
      pReader->aNode = 0;
      if( pElem ){
        char *aCopy;
        PendingList *pList = (PendingList *)fts3HashData(pElem);
        int nCopy = pList->nData+1;

        int nTerm = fts3HashKeysize(pElem);
        if( (nTerm+1)>pReader->nTermAlloc ){
          sqlite3_free(pReader->zTerm);
          pReader->zTerm = (char*)sqlite3_malloc64(((i64)nTerm+1)*2);
          if( !pReader->zTerm ) return SQLITE_NOMEM;
          pReader->nTermAlloc = (nTerm+1)*2;
        }
        memcpy(pReader->zTerm, fts3HashKey(pElem), nTerm);
        pReader->zTerm[nTerm] = '\0';
        pReader->nTerm = nTerm;

        aCopy = (char*)sqlite3_malloc64(nCopy);
        if( !aCopy ) return SQLITE_NOMEM;
        memcpy(aCopy, pList->aData, nCopy);
        pReader->nNode = pReader->nDoclist = nCopy;
        pReader->aNode = pReader->aDoclist = aCopy;
        pReader->ppNextElem++;
        assert( pReader->aNode );
      }
      return SQLITE_OK;
    }

    fts3SegReaderSetEof(pReader);

    /* If iCurrentBlock>=iLeafEndBlock, this is an EOF condition. All leaf
    ** blocks have already been traversed.  */
#ifdef CORRUPT_DB
    assert( pReader->iCurrentBlock<=pReader->iLeafEndBlock || CORRUPT_DB );
#endif
    if( pReader->iCurrentBlock>=pReader->iLeafEndBlock ){
      return SQLITE_OK;
    }

    rc = sqlite3Fts3ReadBlock(
        p, ++pReader->iCurrentBlock, &pReader->aNode, &pReader->nNode,
        (bIncr ? &pReader->nPopulate : 0)
    );
    if( rc!=SQLITE_OK ) return rc;
    assert( pReader->pBlob==0 );
    if( bIncr && pReader->nPopulate<pReader->nNode ){
      pReader->pBlob = p->pSegments;
      p->pSegments = 0;
    }
    pNext = pReader->aNode;
  }

  assert( !fts3SegReaderIsPending(pReader) );

  rc = fts3SegReaderRequire(pReader, pNext, FTS3_VARINT_MAX*2);
  if( rc!=SQLITE_OK ) return rc;

  /* Because of the FTS3_NODE_PADDING bytes of padding, the following is
  ** safe (no risk of overread) even if the node data is corrupted. */
  pNext += fts3GetVarint32(pNext, &nPrefix);
  pNext += fts3GetVarint32(pNext, &nSuffix);
  if( nSuffix<=0
   || (&pReader->aNode[pReader->nNode] - pNext)<nSuffix
   || nPrefix>pReader->nTerm
  ){
    return FTS_CORRUPT_VTAB;
  }

  /* Both nPrefix and nSuffix were read by fts3GetVarint32() and so are
  ** between 0 and 0x7FFFFFFF. But the sum of the two may cause integer
  ** overflow - hence the (i64) casts.  */
  if( (i64)nPrefix+nSuffix>(i64)pReader->nTermAlloc ){
    i64 nNew = ((i64)nPrefix+nSuffix)*2;
    char *zNew = sqlite3_realloc64(pReader->zTerm, nNew);
    if( !zNew ){
      return SQLITE_NOMEM;
    }
    pReader->zTerm = zNew;
    pReader->nTermAlloc = nNew;
  }

  rc = fts3SegReaderRequire(pReader, pNext, nSuffix+FTS3_VARINT_MAX);
  if( rc!=SQLITE_OK ) return rc;

  memcpy(&pReader->zTerm[nPrefix], pNext, nSuffix);
  pReader->nTerm = nPrefix+nSuffix;
  pNext += nSuffix;
  pNext += fts3GetVarint32(pNext, &pReader->nDoclist);
  pReader->aDoclist = pNext;
  pReader->pOffsetList = 0;

  /* Check that the doclist does not appear to extend past the end of the
  ** b-tree node. And that the final byte of the doclist is 0x00. If either
  ** of these statements is untrue, then the data structure is corrupt.
  */
  if( pReader->nDoclist > pReader->nNode-(pReader->aDoclist-pReader->aNode)
   || (pReader->nPopulate==0 && pReader->aDoclist[pReader->nDoclist-1])
   || pReader->nDoclist==0
  ){
    return FTS_CORRUPT_VTAB;
  }
  return SQLITE_OK;
}

/*
** Set the SegReader to point to the first docid in the doclist associated
** with the current term.
*/
static int fts3SegReaderFirstDocid(Fts3Table *pTab, Fts3SegReader *pReader){
  int rc = SQLITE_OK;
  assert( pReader->aDoclist );
  assert( !pReader->pOffsetList );
  if( pTab->bDescIdx && fts3SegReaderIsPending(pReader) ){
    u8 bEof = 0;
    pReader->iDocid = 0;
    pReader->nOffsetList = 0;
    sqlite3Fts3DoclistPrev(0,
        pReader->aDoclist, pReader->nDoclist, &pReader->pOffsetList,
        &pReader->iDocid, &pReader->nOffsetList, &bEof
    );
  }else{
    rc = fts3SegReaderRequire(pReader, pReader->aDoclist, FTS3_VARINT_MAX);
    if( rc==SQLITE_OK ){
      int n = sqlite3Fts3GetVarint(pReader->aDoclist, &pReader->iDocid);
      pReader->pOffsetList = &pReader->aDoclist[n];
    }
  }
  return rc;
}

/*
** Advance the SegReader to point to the next docid in the doclist
** associated with the current term.
**
** If arguments ppOffsetList and pnOffsetList are not NULL, then
** *ppOffsetList is set to point to the first column-offset list
** in the doclist entry (i.e. immediately past the docid varint).
** *pnOffsetList is set to the length of the set of column-offset
** lists, not including the nul-terminator byte. For example:
*/
static int fts3SegReaderNextDocid(
  Fts3Table *pTab,
  Fts3SegReader *pReader,         /* Reader to advance to next docid */
  char **ppOffsetList,            /* OUT: Pointer to current position-list */
  int *pnOffsetList               /* OUT: Length of *ppOffsetList in bytes */
){
  int rc = SQLITE_OK;
  char *p = pReader->pOffsetList;
  char c = 0;

  assert( p );

  if( pTab->bDescIdx && fts3SegReaderIsPending(pReader) ){
    /* A pending-terms seg-reader for an FTS4 table that uses order=desc.
    ** Pending-terms doclists are always built up in ascending order, so
    ** we have to iterate through them backwards here. */
    u8 bEof = 0;
    if( ppOffsetList ){
      *ppOffsetList = pReader->pOffsetList;
      *pnOffsetList = pReader->nOffsetList - 1;
    }
    sqlite3Fts3DoclistPrev(0,
        pReader->aDoclist, pReader->nDoclist, &p, &pReader->iDocid,
        &pReader->nOffsetList, &bEof
    );
    if( bEof ){
      pReader->pOffsetList = 0;
    }else{
      pReader->pOffsetList = p;
    }
  }else{
    char *pEnd = &pReader->aDoclist[pReader->nDoclist];

    /* Pointer p currently points at the first byte of an offset list. The
    ** following block advances it to point one byte past the end of
    ** the same offset list. */
    while( 1 ){

      /* The following line of code (and the "p++" below the while() loop) is
      ** normally all that is required to move pointer p to the desired
      ** position. The exception is if this node is being loaded from disk
      ** incrementally and pointer "p" now points to the first byte past
      ** the populated part of pReader->aNode[].
      */
      while( *p | c ) c = *p++ & 0x80;
      assert( *p==0 );

      if( pReader->pBlob==0 || p<&pReader->aNode[pReader->nPopulate] ) break;
      rc = fts3SegReaderIncrRead(pReader);
      if( rc!=SQLITE_OK ) return rc;
    }
    p++;

    /* If required, populate the output variables with a pointer to and the
    ** size of the previous offset-list.
    */
    if( ppOffsetList ){
      *ppOffsetList = pReader->pOffsetList;
      *pnOffsetList = (int)(p - pReader->pOffsetList - 1);
    }

    /* List may have been edited in place by fts3EvalNearTrim() */
    while( p<pEnd && *p==0 ) p++;

    /* If there are no more entries in the doclist, set pOffsetList to
    ** NULL. Otherwise, set Fts3SegReader.iDocid to the next docid and
    ** Fts3SegReader.pOffsetList to point to the next offset list before
    ** returning.
    */
    if( p>=pEnd ){
      pReader->pOffsetList = 0;
    }else{
      rc = fts3SegReaderRequire(pReader, p, FTS3_VARINT_MAX);
      if( rc==SQLITE_OK ){
        u64 iDelta;
        pReader->pOffsetList = p + sqlite3Fts3GetVarintU(p, &iDelta);
        if( pTab->bDescIdx ){
          pReader->iDocid = (i64)((u64)pReader->iDocid - iDelta);
        }else{
          pReader->iDocid = (i64)((u64)pReader->iDocid + iDelta);
        }
      }
    }
  }

  return rc;
}


SQLITE_PRIVATE int sqlite3Fts3MsrOvfl(
  Fts3Cursor *pCsr,
  Fts3MultiSegReader *pMsr,
  int *pnOvfl
){
  Fts3Table *p = (Fts3Table*)pCsr->base.pVtab;
  int nOvfl = 0;
  int ii;
  int rc = SQLITE_OK;
  int pgsz = p->nPgsz;

  assert( p->bFts4 );
  assert( pgsz>0 );

  for(ii=0; rc==SQLITE_OK && ii<pMsr->nSegment; ii++){
    Fts3SegReader *pReader = pMsr->apSegment[ii];
    if( !fts3SegReaderIsPending(pReader)
     && !fts3SegReaderIsRootOnly(pReader)
    ){
      sqlite3_int64 jj;
      for(jj=pReader->iStartBlock; jj<=pReader->iLeafEndBlock; jj++){
        int nBlob;
        rc = sqlite3Fts3ReadBlock(p, jj, 0, &nBlob, 0);
        if( rc!=SQLITE_OK ) break;
        if( (nBlob+35)>pgsz ){
          nOvfl += (nBlob + 34)/pgsz;
        }
      }
    }
  }
  *pnOvfl = nOvfl;
  return rc;
}

/*
** Free all allocations associated with the iterator passed as the
** second argument.
*/
SQLITE_PRIVATE void sqlite3Fts3SegReaderFree(Fts3SegReader *pReader){
  if( pReader ){
    sqlite3_free(pReader->zTerm);
    if( !fts3SegReaderIsRootOnly(pReader) ){
      sqlite3_free(pReader->aNode);
    }
    sqlite3_blob_close(pReader->pBlob);
  }
  sqlite3_free(pReader);
}
