
static void fts5MultiIterNext2(
  Fts5Index *p,
  Fts5Iter *pIter,
  int *pbNewTerm                  /* OUT: True if *might* be new term */
){
  assert( pIter->bSkipEmpty );
  if( p->rc==SQLITE_OK ){
    *pbNewTerm = 0;
    do{
      int iFirst = pIter->aFirst[1].iFirst;
      Fts5SegIter *pSeg = &pIter->aSeg[iFirst];
      int bNewTerm = 0;

      assert( p->rc==SQLITE_OK );
      pSeg->xNext(p, pSeg, &bNewTerm);
      if( pSeg->pLeaf==0 || bNewTerm
       || fts5MultiIterAdvanceRowid(pIter, iFirst, &pSeg)
      ){
        fts5MultiIterAdvanced(p, pIter, iFirst, 1);
        fts5MultiIterSetEof(pIter);
        *pbNewTerm = 1;
      }
      fts5AssertMultiIterSetup(p, pIter);

    }while( (fts5MultiIterIsEmpty(p, pIter) || fts5MultiIterIsDeleted(pIter))
         && (p->rc==SQLITE_OK)
    );
  }
}

static void fts5IterSetOutputs_Noop(Fts5Iter *pUnused1, Fts5SegIter *pUnused2){
  UNUSED_PARAM2(pUnused1, pUnused2);
}

static Fts5Iter *fts5MultiIterAlloc(
  Fts5Index *p,                   /* FTS5 backend to iterate within */
  int nSeg
){
  Fts5Iter *pNew;
  i64 nSlot;                      /* Power of two >= nSeg */

  for(nSlot=2; nSlot<nSeg; nSlot=nSlot*2);
  pNew = fts5IdxMalloc(p,
      sizeof(Fts5Iter) +                  /* pNew */
      sizeof(Fts5SegIter) * (nSlot-1) +   /* pNew->aSeg[] */
      sizeof(Fts5CResult) * nSlot         /* pNew->aFirst[] */
  );
  if( pNew ){
    pNew->nSeg = nSlot;
    pNew->aFirst = (Fts5CResult*)&pNew->aSeg[nSlot];
    pNew->pIndex = p;
    pNew->xSetOutputs = fts5IterSetOutputs_Noop;
  }
  return pNew;
}

static void fts5PoslistCallback(
  Fts5Index *pUnused,
  void *pContext,
  const u8 *pChunk, int nChunk
){
  UNUSED_PARAM(pUnused);
  assert_nc( nChunk>=0 );
  if( nChunk>0 ){
    fts5BufferSafeAppendBlob((Fts5Buffer*)pContext, pChunk, nChunk);
  }
}

typedef struct PoslistCallbackCtx PoslistCallbackCtx;
struct PoslistCallbackCtx {
  Fts5Buffer *pBuf;               /* Append to this buffer */
  Fts5Colset *pColset;            /* Restrict matches to this column */
  int eState;                     /* See above */
};

typedef struct PoslistOffsetsCtx PoslistOffsetsCtx;
struct PoslistOffsetsCtx {
  Fts5Buffer *pBuf;               /* Append to this buffer */
  Fts5Colset *pColset;            /* Restrict matches to this column */
  int iRead;
  int iWrite;
};

/*
** TODO: Make this more efficient!
*/
static int fts5IndexColsetTest(Fts5Colset *pColset, int iCol){
  int i;
  for(i=0; i<pColset->nCol; i++){
    if( pColset->aiCol[i]==iCol ) return 1;
  }
  return 0;
}

static void fts5PoslistOffsetsCallback(
  Fts5Index *pUnused,
  void *pContext,
  const u8 *pChunk, int nChunk
){
  PoslistOffsetsCtx *pCtx = (PoslistOffsetsCtx*)pContext;
  UNUSED_PARAM(pUnused);
  assert_nc( nChunk>=0 );
  if( nChunk>0 ){
    int i = 0;
    while( i<nChunk ){
      int iVal;
      i += fts5GetVarint32(&pChunk[i], iVal);
      iVal += pCtx->iRead - 2;
      pCtx->iRead = iVal;
      if( fts5IndexColsetTest(pCtx->pColset, iVal) ){
        fts5BufferSafeAppendVarint(pCtx->pBuf, iVal + 2 - pCtx->iWrite);
        pCtx->iWrite = iVal;
      }
    }
  }
}

static void fts5PoslistFilterCallback(
  Fts5Index *pUnused,
  void *pContext,
  const u8 *pChunk, int nChunk
){
  PoslistCallbackCtx *pCtx = (PoslistCallbackCtx*)pContext;
  UNUSED_PARAM(pUnused);
  assert_nc( nChunk>=0 );
  if( nChunk>0 ){
    /* Search through to find the first varint with value 1. This is the
    ** start of the next columns hits. */
    int i = 0;
    int iStart = 0;

    if( pCtx->eState==2 ){
      int iCol;
      fts5FastGetVarint32(pChunk, i, iCol);
      if( fts5IndexColsetTest(pCtx->pColset, iCol) ){
        pCtx->eState = 1;
        fts5BufferSafeAppendVarint(pCtx->pBuf, 1);
      }else{
        pCtx->eState = 0;
      }
    }

    do {
      while( i<nChunk && pChunk[i]!=0x01 ){
        while( pChunk[i] & 0x80 ) i++;
        i++;
      }
      if( pCtx->eState ){
        fts5BufferSafeAppendBlob(pCtx->pBuf, &pChunk[iStart], i-iStart);
      }
      if( i<nChunk ){
        int iCol;
        iStart = i;
        i++;
        if( i>=nChunk ){
          pCtx->eState = 2;
        }else{
          fts5FastGetVarint32(pChunk, i, iCol);
          pCtx->eState = fts5IndexColsetTest(pCtx->pColset, iCol);
          if( pCtx->eState ){
            fts5BufferSafeAppendBlob(pCtx->pBuf, &pChunk[iStart], i-iStart);
            iStart = i;
          }
        }
      }
    }while( i<nChunk );
  }
}

static void fts5ChunkIterate(
  Fts5Index *p,                   /* Index object */
  Fts5SegIter *pSeg,              /* Poslist of this iterator */
  void *pCtx,                     /* Context pointer for xChunk callback */
  void (*xChunk)(Fts5Index*, void*, const u8*, int)
){
  int nRem = pSeg->nPos;          /* Number of bytes still to come */
  Fts5Data *pData = 0;
  u8 *pChunk = &pSeg->pLeaf->p[pSeg->iLeafOffset];
  int nChunk = MIN(nRem, pSeg->pLeaf->szLeaf - pSeg->iLeafOffset);
  int pgno = pSeg->iLeafPgno;
  int pgnoSave = 0;

  /* This function does not work with detail=none databases. */
  assert( p->pConfig->eDetail!=FTS5_DETAIL_NONE );

  if( (pSeg->flags & FTS5_SEGITER_REVERSE)==0 ){
    pgnoSave = pgno+1;
  }

  while( 1 ){
    xChunk(p, pCtx, pChunk, nChunk);
    nRem -= nChunk;
    fts5DataRelease(pData);
    if( nRem<=0 ){
      break;
    }else if( pSeg->pSeg==0 ){
      p->rc = FTS5_CORRUPT;
      return;
    }else{
      pgno++;
      pData = fts5LeafRead(p, FTS5_SEGMENT_ROWID(pSeg->pSeg->iSegid, pgno));
      if( pData==0 ) break;
      pChunk = &pData->p[4];
      nChunk = MIN(nRem, pData->szLeaf - 4);
      if( pgno==pgnoSave ){
        assert( pSeg->pNextLeaf==0 );
        pSeg->pNextLeaf = pData;
        pData = 0;
      }
    }
  }
}

/*
** Iterator pIter currently points to a valid entry (not EOF). This
** function appends the position list data for the current entry to
** buffer pBuf. It does not make a copy of the position-list size
** field.
*/
static void fts5SegiterPoslist(
  Fts5Index *p,
  Fts5SegIter *pSeg,
  Fts5Colset *pColset,
  Fts5Buffer *pBuf
){
  assert( pBuf!=0 );
  assert( pSeg!=0 );
  if( 0==fts5BufferGrow(&p->rc, pBuf, pSeg->nPos+FTS5_DATA_ZERO_PADDING) ){
    assert( pBuf->p!=0 );
    assert( pBuf->nSpace >= pBuf->n+pSeg->nPos+FTS5_DATA_ZERO_PADDING );
    memset(&pBuf->p[pBuf->n+pSeg->nPos], 0, FTS5_DATA_ZERO_PADDING);
    if( pColset==0 ){
      fts5ChunkIterate(p, pSeg, (void*)pBuf, fts5PoslistCallback);
    }else{
      if( p->pConfig->eDetail==FTS5_DETAIL_FULL ){
        PoslistCallbackCtx sCtx;
        sCtx.pBuf = pBuf;
        sCtx.pColset = pColset;
        sCtx.eState = fts5IndexColsetTest(pColset, 0);
        assert( sCtx.eState==0 || sCtx.eState==1 );
        fts5ChunkIterate(p, pSeg, (void*)&sCtx, fts5PoslistFilterCallback);
      }else{
        PoslistOffsetsCtx sCtx;
        memset(&sCtx, 0, sizeof(sCtx));
        sCtx.pBuf = pBuf;
        sCtx.pColset = pColset;
        fts5ChunkIterate(p, pSeg, (void*)&sCtx, fts5PoslistOffsetsCallback);
      }
    }
  }
}

/*
** Parameter pPos points to a buffer containing a position list, size nPos.
** This function filters it according to pColset (which must be non-NULL)
** and sets pIter->base.pData/nData to point to the new position list.
** If memory is required for the new position list, use buffer pIter->poslist.
** Or, if the new position list is a contiguous subset of the input, set
** pIter->base.pData/nData to point directly to it.
**
** This function is a no-op if *pRc is other than SQLITE_OK when it is
** called. If an OOM error is encountered, *pRc is set to SQLITE_NOMEM
** before returning.
*/
static void fts5IndexExtractColset(
  int *pRc,
  Fts5Colset *pColset,            /* Colset to filter on */
  const u8 *pPos, int nPos,       /* Position list */
  Fts5Iter *pIter
){
  if( *pRc==SQLITE_OK ){
    const u8 *p = pPos;
    const u8 *aCopy = p;
    const u8 *pEnd = &p[nPos];    /* One byte past end of position list */
    int i = 0;
    int iCurrent = 0;

    if( pColset->nCol>1 && sqlite3Fts5BufferSize(pRc, &pIter->poslist, nPos) ){
      return;
    }

    while( 1 ){
      while( pColset->aiCol[i]<iCurrent ){
        i++;
        if( i==pColset->nCol ){
          pIter->base.pData = pIter->poslist.p;
          pIter->base.nData = pIter->poslist.n;
          return;
        }
      }

      /* Advance pointer p until it points to pEnd or an 0x01 byte that is
      ** not part of a varint */
      while( p<pEnd && *p!=0x01 ){
        while( *p++ & 0x80 );
      }

      if( pColset->aiCol[i]==iCurrent ){
        if( pColset->nCol==1 ){
          pIter->base.pData = aCopy;
          pIter->base.nData = p-aCopy;
          return;
        }
        fts5BufferSafeAppendBlob(&pIter->poslist, aCopy, p-aCopy);
      }
      if( p>=pEnd ){
        pIter->base.pData = pIter->poslist.p;
        pIter->base.nData = pIter->poslist.n;
        return;
      }
      aCopy = p++;
      iCurrent = *p++;
      if( iCurrent & 0x80 ){
        p--;
        p += fts5GetVarint32(p, iCurrent);
      }
    }
  }

}

/*
** xSetOutputs callback used by detail=none tables.
*/
static void fts5IterSetOutputs_None(Fts5Iter *pIter, Fts5SegIter *pSeg){
  assert( pIter->pIndex->pConfig->eDetail==FTS5_DETAIL_NONE );
  pIter->base.iRowid = pSeg->iRowid;
  pIter->base.nData = pSeg->nPos;
}

/*
** xSetOutputs callback used by detail=full and detail=col tables when no
** column filters are specified.
*/
static void fts5IterSetOutputs_Nocolset(Fts5Iter *pIter, Fts5SegIter *pSeg){
  pIter->base.iRowid = pSeg->iRowid;
  pIter->base.nData = pSeg->nPos;

  assert( pIter->pIndex->pConfig->eDetail!=FTS5_DETAIL_NONE );
  assert( pIter->pColset==0 );

  if( pSeg->iLeafOffset+pSeg->nPos<=pSeg->pLeaf->szLeaf ){
    /* All data is stored on the current page. Populate the output
    ** variables to point into the body of the page object. */
    pIter->base.pData = &pSeg->pLeaf->p[pSeg->iLeafOffset];
  }else{
    /* The data is distributed over two or more pages. Copy it into the
    ** Fts5Iter.poslist buffer and then set the output pointer to point
    ** to this buffer.  */
    fts5BufferZero(&pIter->poslist);
    fts5SegiterPoslist(pIter->pIndex, pSeg, 0, &pIter->poslist);
    pIter->base.pData = pIter->poslist.p;
  }
}
