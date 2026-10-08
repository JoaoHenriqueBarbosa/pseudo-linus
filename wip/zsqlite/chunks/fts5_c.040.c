
/*
** pToken points to a buffer of size nToken bytes containing a search
** term, including the index number at the start, used on a tokendata=1
** table. This function returns true if the term in buffer pBuf matches
** token pToken/nToken.
*/
static int fts5IsTokendataPrefix(
  Fts5Buffer *pBuf,
  const u8 *pToken,
  int nToken
){
  return (
      pBuf->n>=nToken
   && 0==memcmp(pBuf->p, pToken, nToken)
   && (pBuf->n==nToken || pBuf->p[nToken]==0x00)
  );
}

/*
** Ensure the segment-iterator passed as the only argument points to EOF.
*/
static void fts5SegIterSetEOF(Fts5SegIter *pSeg){
  fts5DataRelease(pSeg->pLeaf);
  pSeg->pLeaf = 0;
}

/*
** Usually, a tokendata=1 iterator (struct Fts5TokenDataIter) accumulates an
** array of these for each row it visits. Or, for an iterator used by an
** "ORDER BY rank" query, it accumulates an array of these for the entire
** query.
**
** Each instance in the array indicates the iterator (and therefore term)
** associated with position iPos of rowid iRowid. This is used by the
** xInstToken() API.
*/
struct Fts5TokenDataMap {
  i64 iRowid;                     /* Row this token is located in */
  i64 iPos;                       /* Position of token */
  int iIter;                      /* Iterator token was read from */
};

/*
** An object used to supplement Fts5Iter for tokendata=1 iterators.
*/
struct Fts5TokenDataIter {
  int nIter;
  int nIterAlloc;

  int nMap;
  int nMapAlloc;
  Fts5TokenDataMap *aMap;

  Fts5PoslistReader *aPoslistReader;
  int *aPoslistToIter;
  Fts5Iter *apIter[1];
};

/*
** This function appends iterator pAppend to Fts5TokenDataIter pIn and
** returns the result.
*/
static Fts5TokenDataIter *fts5AppendTokendataIter(
  Fts5Index *p,                   /* Index object (for error code) */
  Fts5TokenDataIter *pIn,         /* Current Fts5TokenDataIter struct */
  Fts5Iter *pAppend               /* Append this iterator */
){
  Fts5TokenDataIter *pRet = pIn;

  if( p->rc==SQLITE_OK ){
    if( pIn==0 || pIn->nIter==pIn->nIterAlloc ){
      int nAlloc = pIn ? pIn->nIterAlloc*2 : 16;
      int nByte = nAlloc * sizeof(Fts5Iter*) + sizeof(Fts5TokenDataIter);
      Fts5TokenDataIter *pNew = (Fts5TokenDataIter*)sqlite3_realloc(pIn, nByte);

      if( pNew==0 ){
        p->rc = SQLITE_NOMEM;
      }else{
        if( pIn==0 ) memset(pNew, 0, nByte);
        pRet = pNew;
        pNew->nIterAlloc = nAlloc;
      }
    }
  }
  if( p->rc ){
    sqlite3Fts5IterClose((Fts5IndexIter*)pAppend);
  }else{
    pRet->apIter[pRet->nIter++] = pAppend;
  }
  assert( pRet==0 || pRet->nIter<=pRet->nIterAlloc );

  return pRet;
}

/*
** Delete an Fts5TokenDataIter structure and its contents.
*/
static void fts5TokendataIterDelete(Fts5TokenDataIter *pSet){
  if( pSet ){
    int ii;
    for(ii=0; ii<pSet->nIter; ii++){
      fts5MultiIterFree(pSet->apIter[ii]);
    }
    sqlite3_free(pSet->aPoslistReader);
    sqlite3_free(pSet->aMap);
    sqlite3_free(pSet);
  }
}

/*
** Append a mapping to the token-map belonging to object pT.
*/
static void fts5TokendataIterAppendMap(
  Fts5Index *p,
  Fts5TokenDataIter *pT,
  int iIter,
  i64 iRowid,
  i64 iPos
){
  if( p->rc==SQLITE_OK ){
    if( pT->nMap==pT->nMapAlloc ){
      int nNew = pT->nMapAlloc ? pT->nMapAlloc*2 : 64;
      int nByte = nNew * sizeof(Fts5TokenDataMap);
      Fts5TokenDataMap *aNew;

      aNew = (Fts5TokenDataMap*)sqlite3_realloc(pT->aMap, nByte);
      if( aNew==0 ){
        p->rc = SQLITE_NOMEM;
        return;
      }

      pT->aMap = aNew;
      pT->nMapAlloc = nNew;
    }

    pT->aMap[pT->nMap].iRowid = iRowid;
    pT->aMap[pT->nMap].iPos = iPos;
    pT->aMap[pT->nMap].iIter = iIter;
    pT->nMap++;
  }
}

/*
** The iterator passed as the only argument must be a tokendata=1 iterator
** (pIter->pTokenDataIter!=0). This function sets the iterator output
** variables (pIter->base.*) according to the contents of the current
** row.
*/
static void fts5IterSetOutputsTokendata(Fts5Iter *pIter){
  int ii;
  int nHit = 0;
  i64 iRowid = SMALLEST_INT64;
  int iMin = 0;

  Fts5TokenDataIter *pT = pIter->pTokenDataIter;

  pIter->base.nData = 0;
  pIter->base.pData = 0;

  for(ii=0; ii<pT->nIter; ii++){
    Fts5Iter *p = pT->apIter[ii];
    if( p->base.bEof==0 ){
      if( nHit==0 || p->base.iRowid<iRowid ){
        iRowid = p->base.iRowid;
        nHit = 1;
        pIter->base.pData = p->base.pData;
        pIter->base.nData = p->base.nData;
        iMin = ii;
      }else if( p->base.iRowid==iRowid ){
        nHit++;
      }
    }
  }

  if( nHit==0 ){
    pIter->base.bEof = 1;
  }else{
    int eDetail = pIter->pIndex->pConfig->eDetail;
    pIter->base.bEof = 0;
    pIter->base.iRowid = iRowid;

    if( nHit==1 && eDetail==FTS5_DETAIL_FULL ){
      fts5TokendataIterAppendMap(pIter->pIndex, pT, iMin, iRowid, -1);
    }else
    if( nHit>1 && eDetail!=FTS5_DETAIL_NONE ){
      int nReader = 0;
      int nByte = 0;
      i64 iPrev = 0;

      /* Allocate array of iterators if they are not already allocated. */
      if( pT->aPoslistReader==0 ){
        pT->aPoslistReader = (Fts5PoslistReader*)sqlite3Fts5MallocZero(
            &pIter->pIndex->rc,
            pT->nIter * (sizeof(Fts5PoslistReader) + sizeof(int))
        );
        if( pT->aPoslistReader==0 ) return;
        pT->aPoslistToIter = (int*)&pT->aPoslistReader[pT->nIter];
      }

      /* Populate an iterator for each poslist that will be merged */
      for(ii=0; ii<pT->nIter; ii++){
        Fts5Iter *p = pT->apIter[ii];
        if( iRowid==p->base.iRowid ){
          pT->aPoslistToIter[nReader] = ii;
          sqlite3Fts5PoslistReaderInit(
              p->base.pData, p->base.nData, &pT->aPoslistReader[nReader++]
          );
          nByte += p->base.nData;
        }
      }

      /* Ensure the output buffer is large enough */
      if( fts5BufferGrow(&pIter->pIndex->rc, &pIter->poslist, nByte+nHit*10) ){
        return;
      }

      /* Ensure the token-mapping is large enough */
      if( eDetail==FTS5_DETAIL_FULL && pT->nMapAlloc<(pT->nMap + nByte) ){
        int nNew = (pT->nMapAlloc + nByte) * 2;
        Fts5TokenDataMap *aNew = (Fts5TokenDataMap*)sqlite3_realloc(
            pT->aMap, nNew*sizeof(Fts5TokenDataMap)
        );
        if( aNew==0 ){
          pIter->pIndex->rc = SQLITE_NOMEM;
          return;
        }
        pT->aMap = aNew;
        pT->nMapAlloc = nNew;
      }

      pIter->poslist.n = 0;

      while( 1 ){
        i64 iMinPos = LARGEST_INT64;

        /* Find smallest position */
        iMin = 0;
        for(ii=0; ii<nReader; ii++){
          Fts5PoslistReader *pReader = &pT->aPoslistReader[ii];
          if( pReader->bEof==0 ){
            if( pReader->iPos<iMinPos ){
              iMinPos = pReader->iPos;
              iMin = ii;
            }
          }
        }

        /* If all readers were at EOF, break out of the loop. */
        if( iMinPos==LARGEST_INT64 ) break;

        sqlite3Fts5PoslistSafeAppend(&pIter->poslist, &iPrev, iMinPos);
        sqlite3Fts5PoslistReaderNext(&pT->aPoslistReader[iMin]);

        if( eDetail==FTS5_DETAIL_FULL ){
          pT->aMap[pT->nMap].iPos = iMinPos;
          pT->aMap[pT->nMap].iIter = pT->aPoslistToIter[iMin];
          pT->aMap[pT->nMap].iRowid = iRowid;
          pT->nMap++;
        }
      }

      pIter->base.pData = pIter->poslist.p;
      pIter->base.nData = pIter->poslist.n;
    }
  }
}

/*
** The iterator passed as the only argument must be a tokendata=1 iterator
** (pIter->pTokenDataIter!=0). This function advances the iterator. If
** argument bFrom is false, then the iterator is advanced to the next
** entry. Or, if bFrom is true, it is advanced to the first entry with
** a rowid of iFrom or greater.
*/
static void fts5TokendataIterNext(Fts5Iter *pIter, int bFrom, i64 iFrom){
  int ii;
  Fts5TokenDataIter *pT = pIter->pTokenDataIter;
  Fts5Index *pIndex = pIter->pIndex;

  for(ii=0; ii<pT->nIter; ii++){
    Fts5Iter *p = pT->apIter[ii];
    if( p->base.bEof==0
     && (p->base.iRowid==pIter->base.iRowid || (bFrom && p->base.iRowid<iFrom))
    ){
      fts5MultiIterNext(pIndex, p, bFrom, iFrom);
      while( bFrom && p->base.bEof==0
          && p->base.iRowid<iFrom
          && pIndex->rc==SQLITE_OK
      ){
        fts5MultiIterNext(pIndex, p, 0, 0);
      }
    }
  }

  if( pIndex->rc==SQLITE_OK ){
    fts5IterSetOutputsTokendata(pIter);
  }
}

/*
** If the segment-iterator passed as the first argument is at EOF, then
** set pIter->term to a copy of buffer pTerm.
*/
static void fts5TokendataSetTermIfEof(Fts5Iter *pIter, Fts5Buffer *pTerm){
  if( pIter && pIter->aSeg[0].pLeaf==0 ){
    fts5BufferSet(&pIter->pIndex->rc, &pIter->aSeg[0].term, pTerm->n, pTerm->p);
  }
}

/*
** This function sets up an iterator to use for a non-prefix query on a
** tokendata=1 table.
*/
static Fts5Iter *fts5SetupTokendataIter(
  Fts5Index *p,                   /* FTS index to query */
  const u8 *pToken,               /* Buffer containing query term */
  int nToken,                     /* Size of buffer pToken in bytes */
  Fts5Colset *pColset             /* Colset to filter on */
){
  Fts5Iter *pRet = 0;
  Fts5TokenDataIter *pSet = 0;
  Fts5Structure *pStruct = 0;
  const int flags = FTS5INDEX_QUERY_SCANONETERM | FTS5INDEX_QUERY_SCAN;

  Fts5Buffer bSeek = {0, 0, 0};
  Fts5Buffer *pSmall = 0;

  fts5IndexFlush(p);
  pStruct = fts5StructureRead(p);

  while( p->rc==SQLITE_OK ){
    Fts5Iter *pPrev = pSet ? pSet->apIter[pSet->nIter-1] : 0;
    Fts5Iter *pNew = 0;
    Fts5SegIter *pNewIter = 0;
    Fts5SegIter *pPrevIter = 0;

    int iLvl, iSeg, ii;

    pNew = fts5MultiIterAlloc(p, pStruct->nSegment);
    if( pSmall ){
      fts5BufferSet(&p->rc, &bSeek, pSmall->n, pSmall->p);
      fts5BufferAppendBlob(&p->rc, &bSeek, 1, (const u8*)"\0");
    }else{
      fts5BufferSet(&p->rc, &bSeek, nToken, pToken);
    }
    if( p->rc ){
      sqlite3Fts5IterClose((Fts5IndexIter*)pNew);
      break;
    }

    pNewIter = &pNew->aSeg[0];
    pPrevIter = (pPrev ? &pPrev->aSeg[0] : 0);
    for(iLvl=0; iLvl<pStruct->nLevel; iLvl++){
      for(iSeg=pStruct->aLevel[iLvl].nSeg-1; iSeg>=0; iSeg--){
        Fts5StructureSegment *pSeg = &pStruct->aLevel[iLvl].aSeg[iSeg];
        int bDone = 0;

        if( pPrevIter ){
          if( fts5BufferCompare(pSmall, &pPrevIter->term) ){
            memcpy(pNewIter, pPrevIter, sizeof(Fts5SegIter));
            memset(pPrevIter, 0, sizeof(Fts5SegIter));
            bDone = 1;
          }else if( pPrevIter->iEndofDoclist>pPrevIter->pLeaf->szLeaf ){
            fts5SegIterNextInit(p,(const char*)bSeek.p,bSeek.n-1,pSeg,pNewIter);
            bDone = 1;
          }
        }

        if( bDone==0 ){
          fts5SegIterSeekInit(p, bSeek.p, bSeek.n, flags, pSeg, pNewIter);
        }

        if( pPrevIter ){
          if( pPrevIter->pTombArray ){
            pNewIter->pTombArray = pPrevIter->pTombArray;
            pNewIter->pTombArray->nRef++;
          }
        }else{
          fts5SegIterAllocTombstone(p, pNewIter);
        }

        pNewIter++;
        if( pPrevIter ) pPrevIter++;
        if( p->rc ) break;
      }
    }
    fts5TokendataSetTermIfEof(pPrev, pSmall);

    pNew->bSkipEmpty = 1;
    pNew->pColset = pColset;
    fts5IterSetOutputCb(&p->rc, pNew);

    /* Loop through all segments in the new iterator. Find the smallest
    ** term that any segment-iterator points to. Iterator pNew will be
    ** used for this term. Also, set any iterator that points to a term that
    ** does not match pToken/nToken to point to EOF */
    pSmall = 0;
    for(ii=0; ii<pNew->nSeg; ii++){
      Fts5SegIter *pII = &pNew->aSeg[ii];
      if( 0==fts5IsTokendataPrefix(&pII->term, pToken, nToken) ){
        fts5SegIterSetEOF(pII);
      }
      if( pII->pLeaf && (!pSmall || fts5BufferCompare(pSmall, &pII->term)>0) ){
        pSmall = &pII->term;
      }
    }

    /* If pSmall is still NULL at this point, then the new iterator does
    ** not point to any terms that match the query. So delete it and break
    ** out of the loop - all required iterators have been collected.  */
    if( pSmall==0 ){
      sqlite3Fts5IterClose((Fts5IndexIter*)pNew);
      break;
    }

    /* Append this iterator to the set and continue. */
    pSet = fts5AppendTokendataIter(p, pSet, pNew);
  }

  if( p->rc==SQLITE_OK && pSet ){
    int ii;
    for(ii=0; ii<pSet->nIter; ii++){
      Fts5Iter *pIter = pSet->apIter[ii];
      int iSeg;
      for(iSeg=0; iSeg<pIter->nSeg; iSeg++){
        pIter->aSeg[iSeg].flags |= FTS5_SEGITER_ONETERM;
      }
      fts5MultiIterFinishSetup(p, pIter);
    }
  }

  if( p->rc==SQLITE_OK ){
    pRet = fts5MultiIterAlloc(p, 0);
  }
  if( pRet ){
    pRet->pTokenDataIter = pSet;
    if( pSet ){
      fts5IterSetOutputsTokendata(pRet);
    }else{
      pRet->base.bEof = 1;
    }
  }else{
    fts5TokendataIterDelete(pSet);
  }

  fts5StructureRelease(pStruct);
  fts5BufferFree(&bSeek);
  return pRet;
}
