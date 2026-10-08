

/*
** Open a new iterator to iterate though all rowid that match the
** specified token or token prefix.
*/
static int sqlite3Fts5IndexQuery(
  Fts5Index *p,                   /* FTS index to query */
  const char *pToken, int nToken, /* Token (or prefix) to query for */
  int flags,                      /* Mask of FTS5INDEX_QUERY_X flags */
  Fts5Colset *pColset,            /* Match these columns only */
  Fts5IndexIter **ppIter          /* OUT: New iterator object */
){
  Fts5Config *pConfig = p->pConfig;
  Fts5Iter *pRet = 0;
  Fts5Buffer buf = {0, 0, 0};

  /* If the QUERY_SCAN flag is set, all other flags must be clear. */
  assert( (flags & FTS5INDEX_QUERY_SCAN)==0 || flags==FTS5INDEX_QUERY_SCAN );

  if( sqlite3Fts5BufferSize(&p->rc, &buf, nToken+1)==0 ){
    int iIdx = 0;                 /* Index to search */
    int iPrefixIdx = 0;           /* +1 prefix index */
    int bTokendata = pConfig->bTokendata;
    if( nToken>0 ) memcpy(&buf.p[1], pToken, nToken);

    if( flags & (FTS5INDEX_QUERY_NOTOKENDATA|FTS5INDEX_QUERY_SCAN) ){
      bTokendata = 0;
    }

    /* Figure out which index to search and set iIdx accordingly. If this
    ** is a prefix query for which there is no prefix index, set iIdx to
    ** greater than pConfig->nPrefix to indicate that the query will be
    ** satisfied by scanning multiple terms in the main index.
    **
    ** If the QUERY_TEST_NOIDX flag was specified, then this must be a
    ** prefix-query. Instead of using a prefix-index (if one exists),
    ** evaluate the prefix query using the main FTS index. This is used
    ** for internal sanity checking by the integrity-check in debug
    ** mode only.  */
#ifdef SQLITE_DEBUG
    if( pConfig->bPrefixIndex==0 || (flags & FTS5INDEX_QUERY_TEST_NOIDX) ){
      assert( flags & FTS5INDEX_QUERY_PREFIX );
      iIdx = 1+pConfig->nPrefix;
    }else
#endif
    if( flags & FTS5INDEX_QUERY_PREFIX ){
      int nChar = fts5IndexCharlen(pToken, nToken);
      for(iIdx=1; iIdx<=pConfig->nPrefix; iIdx++){
        int nIdxChar = pConfig->aPrefix[iIdx-1];
        if( nIdxChar==nChar ) break;
        if( nIdxChar==nChar+1 ) iPrefixIdx = iIdx;
      }
    }

    if( bTokendata && iIdx==0 ){
      buf.p[0] = '0';
      pRet = fts5SetupTokendataIter(p, buf.p, nToken+1, pColset);
    }else if( iIdx<=pConfig->nPrefix ){
      /* Straight index lookup */
      Fts5Structure *pStruct = fts5StructureRead(p);
      buf.p[0] = (u8)(FTS5_MAIN_PREFIX + iIdx);
      if( pStruct ){
        fts5MultiIterNew(p, pStruct, flags | FTS5INDEX_QUERY_SKIPEMPTY,
            pColset, buf.p, nToken+1, -1, 0, &pRet
        );
        fts5StructureRelease(pStruct);
      }
    }else{
      /* Scan multiple terms in the main index */
      int bDesc = (flags & FTS5INDEX_QUERY_DESC)!=0;
      fts5SetupPrefixIter(p, bDesc, iPrefixIdx, buf.p, nToken+1, pColset,&pRet);
      if( pRet==0 ){
        assert( p->rc!=SQLITE_OK );
      }else{
        assert( pRet->pColset==0 );
        fts5IterSetOutputCb(&p->rc, pRet);
        if( p->rc==SQLITE_OK ){
          Fts5SegIter *pSeg = &pRet->aSeg[pRet->aFirst[1].iFirst];
          if( pSeg->pLeaf ) pRet->xSetOutputs(pRet, pSeg);
        }
      }
    }

    if( p->rc ){
      sqlite3Fts5IterClose((Fts5IndexIter*)pRet);
      pRet = 0;
      sqlite3Fts5IndexCloseReader(p);
    }

    *ppIter = (Fts5IndexIter*)pRet;
    sqlite3Fts5BufferFree(&buf);
  }
  return fts5IndexReturn(p);
}

/*
** Return true if the iterator passed as the only argument is at EOF.
*/
/*
** Move to the next matching rowid.
*/
static int sqlite3Fts5IterNext(Fts5IndexIter *pIndexIter){
  Fts5Iter *pIter = (Fts5Iter*)pIndexIter;
  assert( pIter->pIndex->rc==SQLITE_OK );
  if( pIter->pTokenDataIter ){
    fts5TokendataIterNext(pIter, 0, 0);
  }else{
    fts5MultiIterNext(pIter->pIndex, pIter, 0, 0);
  }
  return fts5IndexReturn(pIter->pIndex);
}

/*
** Move to the next matching term/rowid. Used by the fts5vocab module.
*/
static int sqlite3Fts5IterNextScan(Fts5IndexIter *pIndexIter){
  Fts5Iter *pIter = (Fts5Iter*)pIndexIter;
  Fts5Index *p = pIter->pIndex;

  assert( pIter->pIndex->rc==SQLITE_OK );

  fts5MultiIterNext(p, pIter, 0, 0);
  if( p->rc==SQLITE_OK ){
    Fts5SegIter *pSeg = &pIter->aSeg[ pIter->aFirst[1].iFirst ];
    if( pSeg->pLeaf && pSeg->term.p[0]!=FTS5_MAIN_PREFIX ){
      fts5DataRelease(pSeg->pLeaf);
      pSeg->pLeaf = 0;
      pIter->base.bEof = 1;
    }
  }

  return fts5IndexReturn(pIter->pIndex);
}

/*
** Move to the next matching rowid that occurs at or after iMatch. The
** definition of "at or after" depends on whether this iterator iterates
** in ascending or descending rowid order.
*/
static int sqlite3Fts5IterNextFrom(Fts5IndexIter *pIndexIter, i64 iMatch){
  Fts5Iter *pIter = (Fts5Iter*)pIndexIter;
  if( pIter->pTokenDataIter ){
    fts5TokendataIterNext(pIter, 1, iMatch);
  }else{
    fts5MultiIterNextFrom(pIter->pIndex, pIter, iMatch);
  }
  return fts5IndexReturn(pIter->pIndex);
}

/*
** Return the current term.
*/
static const char *sqlite3Fts5IterTerm(Fts5IndexIter *pIndexIter, int *pn){
  int n;
  const char *z = (const char*)fts5MultiIterTerm((Fts5Iter*)pIndexIter, &n);
  assert_nc( z || n<=1 );
  *pn = n-1;
  return (z ? &z[1] : 0);
}

/*
** This is used by xInstToken() to access the token at offset iOff, column
** iCol of row iRowid. The token is returned via output variables *ppOut
** and *pnOut. The iterator passed as the first argument must be a tokendata=1
** iterator (pIter->pTokenDataIter!=0).
*/
static int sqlite3Fts5IterToken(
  Fts5IndexIter *pIndexIter,
  i64 iRowid,
  int iCol,
  int iOff,
  const char **ppOut, int *pnOut
){
  Fts5Iter *pIter = (Fts5Iter*)pIndexIter;
  Fts5TokenDataIter *pT = pIter->pTokenDataIter;
  Fts5TokenDataMap *aMap = pT->aMap;
  i64 iPos = (((i64)iCol)<<32) + iOff;

  int i1 = 0;
  int i2 = pT->nMap;
  int iTest = 0;

  while( i2>i1 ){
    iTest = (i1 + i2) / 2;

    if( aMap[iTest].iRowid<iRowid ){
      i1 = iTest+1;
    }else if( aMap[iTest].iRowid>iRowid ){
      i2 = iTest;
    }else{
      if( aMap[iTest].iPos<iPos ){
        if( aMap[iTest].iPos<0 ){
          break;
        }
        i1 = iTest+1;
      }else if( aMap[iTest].iPos>iPos ){
        i2 = iTest;
      }else{
        break;
      }
    }
  }

  if( i2>i1 ){
    Fts5Iter *pMap = pT->apIter[aMap[iTest].iIter];
    *ppOut = (const char*)pMap->aSeg[0].term.p+1;
    *pnOut = pMap->aSeg[0].term.n-1;
  }

  return SQLITE_OK;
}

/*
** Clear any existing entries from the token-map associated with the
** iterator passed as the only argument.
*/
static void sqlite3Fts5IndexIterClearTokendata(Fts5IndexIter *pIndexIter){
  Fts5Iter *pIter = (Fts5Iter*)pIndexIter;
  if( pIter && pIter->pTokenDataIter ){
    pIter->pTokenDataIter->nMap = 0;
  }
}

/*
** Set a token-mapping for the iterator passed as the first argument. This
** is used in detail=column or detail=none mode when a token is requested
** using the xInstToken() API. In this case the caller tokenizers the
** current row and configures the token-mapping via multiple calls to this
** function.
*/
static int sqlite3Fts5IndexIterWriteTokendata(
  Fts5IndexIter *pIndexIter,
  const char *pToken, int nToken,
  i64 iRowid, int iCol, int iOff
){
  Fts5Iter *pIter = (Fts5Iter*)pIndexIter;
  Fts5TokenDataIter *pT = pIter->pTokenDataIter;
  Fts5Index *p = pIter->pIndex;
  int ii;

  assert( p->pConfig->eDetail!=FTS5_DETAIL_FULL );
  assert( pIter->pTokenDataIter );

  for(ii=0; ii<pT->nIter; ii++){
    Fts5Buffer *pTerm = &pT->apIter[ii]->aSeg[0].term;
    if( nToken==pTerm->n-1 && memcmp(pToken, pTerm->p+1, nToken)==0 ) break;
  }
  if( ii<pT->nIter ){
    fts5TokendataIterAppendMap(p, pT, ii, iRowid, (((i64)iCol)<<32) + iOff);
  }
  return fts5IndexReturn(p);
}

/*
** Close an iterator opened by an earlier call to sqlite3Fts5IndexQuery().
*/
static void sqlite3Fts5IterClose(Fts5IndexIter *pIndexIter){
  if( pIndexIter ){
    Fts5Iter *pIter = (Fts5Iter*)pIndexIter;
    Fts5Index *pIndex = pIter->pIndex;
    fts5TokendataIterDelete(pIter->pTokenDataIter);
    fts5MultiIterFree(pIter);
    sqlite3Fts5IndexCloseReader(pIndex);
  }
}

/*
** Read and decode the "averages" record from the database.
**
** Parameter anSize must point to an array of size nCol, where nCol is
** the number of user defined columns in the FTS table.
*/
static int sqlite3Fts5IndexGetAverages(Fts5Index *p, i64 *pnRow, i64 *anSize){
  int nCol = p->pConfig->nCol;
  Fts5Data *pData;

  *pnRow = 0;
  memset(anSize, 0, sizeof(i64) * nCol);
  pData = fts5DataRead(p, FTS5_AVERAGES_ROWID);
  if( p->rc==SQLITE_OK && pData->nn ){
    int i = 0;
    int iCol;
    i += fts5GetVarint(&pData->p[i], (u64*)pnRow);
    for(iCol=0; i<pData->nn && iCol<nCol; iCol++){
      i += fts5GetVarint(&pData->p[i], (u64*)&anSize[iCol]);
    }
  }

  fts5DataRelease(pData);
  return fts5IndexReturn(p);
}

/*
** Replace the current "averages" record with the contents of the buffer
** supplied as the second argument.
*/
static int sqlite3Fts5IndexSetAverages(Fts5Index *p, const u8 *pData, int nData){
  assert( p->rc==SQLITE_OK );
  fts5DataWrite(p, FTS5_AVERAGES_ROWID, pData, nData);
  return fts5IndexReturn(p);
}

/*
** Return the total number of blocks this module has read from the %_data
** table since it was created.
*/
static int sqlite3Fts5IndexReads(Fts5Index *p){
  return p->nRead;
}

/*
** Set the 32-bit cookie value stored at the start of all structure
** records to the value passed as the second argument.
**
** Return SQLITE_OK if successful, or an SQLite error code if an error
** occurs.
*/
static int sqlite3Fts5IndexSetCookie(Fts5Index *p, int iNew){
  int rc;                              /* Return code */
  Fts5Config *pConfig = p->pConfig;    /* Configuration object */
  u8 aCookie[4];                       /* Binary representation of iNew */
  sqlite3_blob *pBlob = 0;

  assert( p->rc==SQLITE_OK );
  sqlite3Fts5Put32(aCookie, iNew);

  rc = sqlite3_blob_open(pConfig->db, pConfig->zDb, p->zDataTbl,
      "block", FTS5_STRUCTURE_ROWID, 1, &pBlob
  );
  if( rc==SQLITE_OK ){
    sqlite3_blob_write(pBlob, aCookie, 4, 0);
    rc = sqlite3_blob_close(pBlob);
  }

  return rc;
}

static int sqlite3Fts5IndexLoadConfig(Fts5Index *p){
  Fts5Structure *pStruct;
  pStruct = fts5StructureRead(p);
  fts5StructureRelease(pStruct);
  return fts5IndexReturn(p);
}

/*
** Retrieve the origin value that will be used for the segment currently
** being accumulated in the in-memory hash table when it is flushed to
** disk. If successful, SQLITE_OK is returned and (*piOrigin) set to
** the queried value. Or, if an error occurs, an error code is returned
** and the final value of (*piOrigin) is undefined.
*/
static int sqlite3Fts5IndexGetOrigin(Fts5Index *p, i64 *piOrigin){
  Fts5Structure *pStruct;
  pStruct = fts5StructureRead(p);
  if( pStruct ){
    *piOrigin = pStruct->nOriginCntr;
    fts5StructureRelease(pStruct);
  }
  return fts5IndexReturn(p);
}
