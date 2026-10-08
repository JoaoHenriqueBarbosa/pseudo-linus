
/*
** Allocate a new SegReader object.
*/
SQLITE_PRIVATE int sqlite3Fts3SegReaderNew(
  int iAge,                       /* Segment "age". */
  int bLookup,                    /* True for a lookup only */
  sqlite3_int64 iStartLeaf,       /* First leaf to traverse */
  sqlite3_int64 iEndLeaf,         /* Final leaf to traverse */
  sqlite3_int64 iEndBlock,        /* Final block of segment */
  const char *zRoot,              /* Buffer containing root node */
  int nRoot,                      /* Size of buffer containing root node */
  Fts3SegReader **ppReader        /* OUT: Allocated Fts3SegReader */
){
  Fts3SegReader *pReader;         /* Newly allocated SegReader object */
  int nExtra = 0;                 /* Bytes to allocate segment root node */

  assert( zRoot!=0 || nRoot==0 );
#ifdef CORRUPT_DB
  assert( zRoot!=0 || CORRUPT_DB );
#endif

  if( iStartLeaf==0 ){
    if( iEndLeaf!=0 ) return FTS_CORRUPT_VTAB;
    nExtra = nRoot + FTS3_NODE_PADDING;
  }

  pReader = (Fts3SegReader *)sqlite3_malloc64(sizeof(Fts3SegReader) + nExtra);
  if( !pReader ){
    return SQLITE_NOMEM;
  }
  memset(pReader, 0, sizeof(Fts3SegReader));
  pReader->iIdx = iAge;
  pReader->bLookup = bLookup!=0;
  pReader->iStartBlock = iStartLeaf;
  pReader->iLeafEndBlock = iEndLeaf;
  pReader->iEndBlock = iEndBlock;

  if( nExtra ){
    /* The entire segment is stored in the root node. */
    pReader->aNode = (char *)&pReader[1];
    pReader->rootOnly = 1;
    pReader->nNode = nRoot;
    if( nRoot ) memcpy(pReader->aNode, zRoot, nRoot);
    memset(&pReader->aNode[nRoot], 0, FTS3_NODE_PADDING);
  }else{
    pReader->iCurrentBlock = iStartLeaf-1;
  }
  *ppReader = pReader;
  return SQLITE_OK;
}

/*
** This is a comparison function used as a qsort() callback when sorting
** an array of pending terms by term. This occurs as part of flushing
** the contents of the pending-terms hash table to the database.
*/
static int SQLITE_CDECL fts3CompareElemByTerm(
  const void *lhs,
  const void *rhs
){
  char *z1 = fts3HashKey(*(Fts3HashElem **)lhs);
  char *z2 = fts3HashKey(*(Fts3HashElem **)rhs);
  int n1 = fts3HashKeysize(*(Fts3HashElem **)lhs);
  int n2 = fts3HashKeysize(*(Fts3HashElem **)rhs);

  int n = (n1<n2 ? n1 : n2);
  int c = memcmp(z1, z2, n);
  if( c==0 ){
    c = n1 - n2;
  }
  return c;
}

/*
** This function is used to allocate an Fts3SegReader that iterates through
** a subset of the terms stored in the Fts3Table.pendingTerms array.
**
** If the isPrefixIter parameter is zero, then the returned SegReader iterates
** through each term in the pending-terms table. Or, if isPrefixIter is
** non-zero, it iterates through each term and its prefixes. For example, if
** the pending terms hash table contains the terms "sqlite", "mysql" and
** "firebird", then the iterator visits the following 'terms' (in the order
** shown):
**
**   f fi fir fire fireb firebi firebir firebird
**   m my mys mysq mysql
**   s sq sql sqli sqlit sqlite
**
** Whereas if isPrefixIter is zero, the terms visited are:
**
**   firebird mysql sqlite
*/
SQLITE_PRIVATE int sqlite3Fts3SegReaderPending(
  Fts3Table *p,                   /* Virtual table handle */
  int iIndex,                     /* Index for p->aIndex */
  const char *zTerm,              /* Term to search for */
  int nTerm,                      /* Size of buffer zTerm */
  int bPrefix,                    /* True for a prefix iterator */
  Fts3SegReader **ppReader        /* OUT: SegReader for pending-terms */
){
  Fts3SegReader *pReader = 0;     /* Fts3SegReader object to return */
  Fts3HashElem *pE;               /* Iterator variable */
  Fts3HashElem **aElem = 0;       /* Array of term hash entries to scan */
  int nElem = 0;                  /* Size of array at aElem */
  int rc = SQLITE_OK;             /* Return Code */
  Fts3Hash *pHash;

  pHash = &p->aIndex[iIndex].hPending;
  if( bPrefix ){
    int nAlloc = 0;               /* Size of allocated array at aElem */

    for(pE=fts3HashFirst(pHash); pE; pE=fts3HashNext(pE)){
      char *zKey = (char *)fts3HashKey(pE);
      int nKey = fts3HashKeysize(pE);
      if( nTerm==0 || (nKey>=nTerm && 0==memcmp(zKey, zTerm, nTerm)) ){
        if( nElem==nAlloc ){
          Fts3HashElem **aElem2;
          nAlloc += 16;
          aElem2 = (Fts3HashElem **)sqlite3_realloc64(
              aElem, nAlloc*sizeof(Fts3HashElem *)
          );
          if( !aElem2 ){
            rc = SQLITE_NOMEM;
            nElem = 0;
            break;
          }
          aElem = aElem2;
        }

        aElem[nElem++] = pE;
      }
    }

    /* If more than one term matches the prefix, sort the Fts3HashElem
    ** objects in term order using qsort(). This uses the same comparison
    ** callback as is used when flushing terms to disk.
    */
    if( nElem>1 ){
      qsort(aElem, nElem, sizeof(Fts3HashElem *), fts3CompareElemByTerm);
    }

  }else{
    /* The query is a simple term lookup that matches at most one term in
    ** the index. All that is required is a straight hash-lookup.
    **
    ** Because the stack address of pE may be accessed via the aElem pointer
    ** below, the "Fts3HashElem *pE" must be declared so that it is valid
    ** within this entire function, not just this "else{...}" block.
    */
    pE = fts3HashFindElem(pHash, zTerm, nTerm);
    if( pE ){
      aElem = &pE;
      nElem = 1;
    }
  }

  if( nElem>0 ){
    sqlite3_int64 nByte;
    nByte = sizeof(Fts3SegReader) + (nElem+1)*sizeof(Fts3HashElem *);
    pReader = (Fts3SegReader *)sqlite3_malloc64(nByte);
    if( !pReader ){
      rc = SQLITE_NOMEM;
    }else{
      memset(pReader, 0, nByte);
      pReader->iIdx = 0x7FFFFFFF;
      pReader->ppNextElem = (Fts3HashElem **)&pReader[1];
      memcpy(pReader->ppNextElem, aElem, nElem*sizeof(Fts3HashElem *));
    }
  }

  if( bPrefix ){
    sqlite3_free(aElem);
  }
  *ppReader = pReader;
  return rc;
}

/*
** Compare the entries pointed to by two Fts3SegReader structures.
** Comparison is as follows:
**
**   1) EOF is greater than not EOF.
**
**   2) The current terms (if any) are compared using memcmp(). If one
**      term is a prefix of another, the longer term is considered the
**      larger.
**
**   3) By segment age. An older segment is considered larger.
*/
static int fts3SegReaderCmp(Fts3SegReader *pLhs, Fts3SegReader *pRhs){
  int rc;
  if( pLhs->aNode && pRhs->aNode ){
    int rc2 = pLhs->nTerm - pRhs->nTerm;
    if( rc2<0 ){
      rc = memcmp(pLhs->zTerm, pRhs->zTerm, pLhs->nTerm);
    }else{
      rc = memcmp(pLhs->zTerm, pRhs->zTerm, pRhs->nTerm);
    }
    if( rc==0 ){
      rc = rc2;
    }
  }else{
    rc = (pLhs->aNode==0) - (pRhs->aNode==0);
  }
  if( rc==0 ){
    rc = pRhs->iIdx - pLhs->iIdx;
  }
  assert_fts3_nc( rc!=0 );
  return rc;
}

/*
** A different comparison function for SegReader structures. In this
** version, it is assumed that each SegReader points to an entry in
** a doclist for identical terms. Comparison is made as follows:
**
**   1) EOF (end of doclist in this case) is greater than not EOF.
**
**   2) By current docid.
**
**   3) By segment age. An older segment is considered larger.
*/
static int fts3SegReaderDoclistCmp(Fts3SegReader *pLhs, Fts3SegReader *pRhs){
  int rc = (pLhs->pOffsetList==0)-(pRhs->pOffsetList==0);
  if( rc==0 ){
    if( pLhs->iDocid==pRhs->iDocid ){
      rc = pRhs->iIdx - pLhs->iIdx;
    }else{
      rc = (pLhs->iDocid > pRhs->iDocid) ? 1 : -1;
    }
  }
  assert( pLhs->aNode && pRhs->aNode );
  return rc;
}
static int fts3SegReaderDoclistCmpRev(Fts3SegReader *pLhs, Fts3SegReader *pRhs){
  int rc = (pLhs->pOffsetList==0)-(pRhs->pOffsetList==0);
  if( rc==0 ){
    if( pLhs->iDocid==pRhs->iDocid ){
      rc = pRhs->iIdx - pLhs->iIdx;
    }else{
      rc = (pLhs->iDocid < pRhs->iDocid) ? 1 : -1;
    }
  }
  assert( pLhs->aNode && pRhs->aNode );
  return rc;
}

/*
** Compare the term that the Fts3SegReader object passed as the first argument
** points to with the term specified by arguments zTerm and nTerm.
**
** If the pSeg iterator is already at EOF, return 0. Otherwise, return
** -ve if the pSeg term is less than zTerm/nTerm, 0 if the two terms are
** equal, or +ve if the pSeg term is greater than zTerm/nTerm.
*/
static int fts3SegReaderTermCmp(
  Fts3SegReader *pSeg,            /* Segment reader object */
  const char *zTerm,              /* Term to compare to */
  int nTerm                       /* Size of term zTerm in bytes */
){
  int res = 0;
  if( pSeg->aNode ){
    if( pSeg->nTerm>nTerm ){
      res = memcmp(pSeg->zTerm, zTerm, nTerm);
    }else{
      res = memcmp(pSeg->zTerm, zTerm, pSeg->nTerm);
    }
    if( res==0 ){
      res = pSeg->nTerm-nTerm;
    }
  }
  return res;
}

/*
** Argument apSegment is an array of nSegment elements. It is known that
** the final (nSegment-nSuspect) members are already in sorted order
** (according to the comparison function provided). This function shuffles
** the array around until all entries are in sorted order.
*/
static void fts3SegReaderSort(
  Fts3SegReader **apSegment,                     /* Array to sort entries of */
  int nSegment,                                  /* Size of apSegment array */
  int nSuspect,                                  /* Unsorted entry count */
  int (*xCmp)(Fts3SegReader *, Fts3SegReader *)  /* Comparison function */
){
  int i;                          /* Iterator variable */

  assert( nSuspect<=nSegment );

  if( nSuspect==nSegment ) nSuspect--;
  for(i=nSuspect-1; i>=0; i--){
    int j;
    for(j=i; j<(nSegment-1); j++){
      Fts3SegReader *pTmp;
      if( xCmp(apSegment[j], apSegment[j+1])<0 ) break;
      pTmp = apSegment[j+1];
      apSegment[j+1] = apSegment[j];
      apSegment[j] = pTmp;
    }
  }

#ifndef NDEBUG
  /* Check that the list really is sorted now. */
  for(i=0; i<(nSuspect-1); i++){
    assert( xCmp(apSegment[i], apSegment[i+1])<0 );
  }
#endif
}

/*
** Insert a record into the %_segments table.
*/
static int fts3WriteSegment(
  Fts3Table *p,                   /* Virtual table handle */
  sqlite3_int64 iBlock,           /* Block id for new block */
  char *z,                        /* Pointer to buffer containing block data */
  int n                           /* Size of buffer z in bytes */
){
  sqlite3_stmt *pStmt;
  int rc = fts3SqlStmt(p, SQL_INSERT_SEGMENTS, &pStmt, 0);
  if( rc==SQLITE_OK ){
    sqlite3_bind_int64(pStmt, 1, iBlock);
    sqlite3_bind_blob(pStmt, 2, z, n, SQLITE_STATIC);
    sqlite3_step(pStmt);
    rc = sqlite3_reset(pStmt);
    sqlite3_bind_null(pStmt, 2);
  }
  return rc;
}

/*
** Find the largest relative level number in the table. If successful, set
** *pnMax to this value and return SQLITE_OK. Otherwise, if an error occurs,
** set *pnMax to zero and return an SQLite error code.
*/
SQLITE_PRIVATE int sqlite3Fts3MaxLevel(Fts3Table *p, int *pnMax){
  int rc;
  int mxLevel = 0;
  sqlite3_stmt *pStmt = 0;

  rc = fts3SqlStmt(p, SQL_SELECT_MXLEVEL, &pStmt, 0);
  if( rc==SQLITE_OK ){
    if( SQLITE_ROW==sqlite3_step(pStmt) ){
      mxLevel = sqlite3_column_int(pStmt, 0);
    }
    rc = sqlite3_reset(pStmt);
  }
  *pnMax = mxLevel;
  return rc;
}
