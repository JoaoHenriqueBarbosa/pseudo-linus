
/*
** This is called to implement the special "VALUES('merge', $nMerge)"
** INSERT command.
*/
static int sqlite3Fts5IndexMerge(Fts5Index *p, int nMerge){
  Fts5Structure *pStruct = 0;

  fts5IndexFlush(p);
  pStruct = fts5StructureRead(p);
  if( pStruct ){
    int nMin = p->pConfig->nUsermerge;
    fts5StructureInvalidate(p);
    if( nMerge<0 ){
      Fts5Structure *pNew = fts5IndexOptimizeStruct(p, pStruct);
      fts5StructureRelease(pStruct);
      pStruct = pNew;
      nMin = 1;
      nMerge = nMerge*-1;
    }
    if( pStruct && pStruct->nLevel ){
      if( fts5IndexMerge(p, &pStruct, nMerge, nMin) ){
        fts5StructureWrite(p, pStruct);
      }
    }
    fts5StructureRelease(pStruct);
  }
  return fts5IndexReturn(p);
}

static void fts5AppendRowid(
  Fts5Index *p,
  u64 iDelta,
  Fts5Iter *pUnused,
  Fts5Buffer *pBuf
){
  UNUSED_PARAM(pUnused);
  fts5BufferAppendVarint(&p->rc, pBuf, iDelta);
}

static void fts5AppendPoslist(
  Fts5Index *p,
  u64 iDelta,
  Fts5Iter *pMulti,
  Fts5Buffer *pBuf
){
  int nData = pMulti->base.nData;
  int nByte = nData + 9 + 9 + FTS5_DATA_ZERO_PADDING;
  assert( nData>0 );
  if( p->rc==SQLITE_OK && 0==fts5BufferGrow(&p->rc, pBuf, nByte) ){
    fts5BufferSafeAppendVarint(pBuf, iDelta);
    fts5BufferSafeAppendVarint(pBuf, nData*2);
    fts5BufferSafeAppendBlob(pBuf, pMulti->base.pData, nData);
    memset(&pBuf->p[pBuf->n], 0, FTS5_DATA_ZERO_PADDING);
  }
}


static void fts5DoclistIterNext(Fts5DoclistIter *pIter){
  u8 *p = pIter->aPoslist + pIter->nSize + pIter->nPoslist;

  assert( pIter->aPoslist || (p==0 && pIter->aPoslist==0) );
  if( p>=pIter->aEof ){
    pIter->aPoslist = 0;
  }else{
    i64 iDelta;

    p += fts5GetVarint(p, (u64*)&iDelta);
    pIter->iRowid += iDelta;

    /* Read position list size */
    if( p[0] & 0x80 ){
      int nPos;
      pIter->nSize = fts5GetVarint32(p, nPos);
      pIter->nPoslist = (nPos>>1);
    }else{
      pIter->nPoslist = ((int)(p[0])) >> 1;
      pIter->nSize = 1;
    }

    pIter->aPoslist = p;
    if( &pIter->aPoslist[pIter->nPoslist]>pIter->aEof ){
      pIter->aPoslist = 0;
    }
  }
}

static void fts5DoclistIterInit(
  Fts5Buffer *pBuf,
  Fts5DoclistIter *pIter
){
  memset(pIter, 0, sizeof(*pIter));
  if( pBuf->n>0 ){
    pIter->aPoslist = pBuf->p;
    pIter->aEof = &pBuf->p[pBuf->n];
    fts5DoclistIterNext(pIter);
  }
}

#if 0
/*
** Append a doclist to buffer pBuf.
**
** This function assumes that space within the buffer has already been
** allocated.
*/
static void fts5MergeAppendDocid(
  Fts5Buffer *pBuf,               /* Buffer to write to */
  i64 *piLastRowid,               /* IN/OUT: Previous rowid written (if any) */
  i64 iRowid                      /* Rowid to append */
){
  assert( pBuf->n!=0 || (*piLastRowid)==0 );
  fts5BufferSafeAppendVarint(pBuf, iRowid - *piLastRowid);
  *piLastRowid = iRowid;
}
#endif

#define fts5MergeAppendDocid(pBuf, iLastRowid, iRowid) {                 \
  assert( (pBuf)->n!=0 || (iLastRowid)==0 );                             \
  fts5BufferSafeAppendVarint((pBuf), (u64)(iRowid) - (u64)(iLastRowid)); \
  (iLastRowid) = (iRowid);                                               \
}

/*
** Swap the contents of buffer *p1 with that of *p2.
*/
static void fts5BufferSwap(Fts5Buffer *p1, Fts5Buffer *p2){
  Fts5Buffer tmp = *p1;
  *p1 = *p2;
  *p2 = tmp;
}

static void fts5NextRowid(Fts5Buffer *pBuf, int *piOff, i64 *piRowid){
  int i = *piOff;
  if( i>=pBuf->n ){
    *piOff = -1;
  }else{
    u64 iVal;
    *piOff = i + sqlite3Fts5GetVarint(&pBuf->p[i], &iVal);
    *piRowid += iVal;
  }
}

/*
** This is the equivalent of fts5MergePrefixLists() for detail=none mode.
** In this case the buffers consist of a delta-encoded list of rowids only.
*/
static void fts5MergeRowidLists(
  Fts5Index *p,                   /* FTS5 backend object */
  Fts5Buffer *p1,                 /* First list to merge */
  int nBuf,                       /* Number of entries in apBuf[] */
  Fts5Buffer *aBuf                /* Array of other lists to merge into p1 */
){
  int i1 = 0;
  int i2 = 0;
  i64 iRowid1 = 0;
  i64 iRowid2 = 0;
  i64 iOut = 0;
  Fts5Buffer *p2 = &aBuf[0];
  Fts5Buffer out;

  (void)nBuf;
  memset(&out, 0, sizeof(out));
  assert( nBuf==1 );
  sqlite3Fts5BufferSize(&p->rc, &out, p1->n + p2->n);
  if( p->rc ) return;

  fts5NextRowid(p1, &i1, &iRowid1);
  fts5NextRowid(p2, &i2, &iRowid2);
  while( i1>=0 || i2>=0 ){
    if( i1>=0 && (i2<0 || iRowid1<iRowid2) ){
      assert( iOut==0 || iRowid1>iOut );
      fts5BufferSafeAppendVarint(&out, iRowid1 - iOut);
      iOut = iRowid1;
      fts5NextRowid(p1, &i1, &iRowid1);
    }else{
      assert( iOut==0 || iRowid2>iOut );
      fts5BufferSafeAppendVarint(&out, iRowid2 - iOut);
      iOut = iRowid2;
      if( i1>=0 && iRowid1==iRowid2 ){
        fts5NextRowid(p1, &i1, &iRowid1);
      }
      fts5NextRowid(p2, &i2, &iRowid2);
    }
  }

  fts5BufferSwap(&out, p1);
  fts5BufferFree(&out);
}

typedef struct PrefixMerger PrefixMerger;
struct PrefixMerger {
  Fts5DoclistIter iter;           /* Doclist iterator */
  i64 iPos;                       /* For iterating through a position list */
  int iOff;
  u8 *aPos;
  PrefixMerger *pNext;            /* Next in docid/poslist order */
};

static void fts5PrefixMergerInsertByRowid(
  PrefixMerger **ppHead,
  PrefixMerger *p
){
  if( p->iter.aPoslist ){
    PrefixMerger **pp = ppHead;
    while( *pp && p->iter.iRowid>(*pp)->iter.iRowid ){
      pp = &(*pp)->pNext;
    }
    p->pNext = *pp;
    *pp = p;
  }
}

static void fts5PrefixMergerInsertByPosition(
  PrefixMerger **ppHead,
  PrefixMerger *p
){
  if( p->iPos>=0 ){
    PrefixMerger **pp = ppHead;
    while( *pp && p->iPos>(*pp)->iPos ){
      pp = &(*pp)->pNext;
    }
    p->pNext = *pp;
    *pp = p;
  }
}


/*
** Array aBuf[] contains nBuf doclists. These are all merged in with the
** doclist in buffer p1.
*/
static void fts5MergePrefixLists(
  Fts5Index *p,                   /* FTS5 backend object */
  Fts5Buffer *p1,                 /* First list to merge */
  int nBuf,                       /* Number of buffers in array aBuf[] */
  Fts5Buffer *aBuf                /* Other lists to merge in */
){
#define fts5PrefixMergerNextPosition(p) \
  sqlite3Fts5PoslistNext64((p)->aPos,(p)->iter.nPoslist,&(p)->iOff,&(p)->iPos)
#define FTS5_MERGE_NLIST 16
  PrefixMerger aMerger[FTS5_MERGE_NLIST];
  PrefixMerger *pHead = 0;
  int i;
  int nOut = 0;
  Fts5Buffer out = {0, 0, 0};
  Fts5Buffer tmp = {0, 0, 0};
  i64 iLastRowid = 0;

  /* Initialize a doclist-iterator for each input buffer. Arrange them in
  ** a linked-list starting at pHead in ascending order of rowid. Avoid
  ** linking any iterators already at EOF into the linked list at all. */
  assert( nBuf+1<=(int)(sizeof(aMerger)/sizeof(aMerger[0])) );
  memset(aMerger, 0, sizeof(PrefixMerger)*(nBuf+1));
  pHead = &aMerger[nBuf];
  fts5DoclistIterInit(p1, &pHead->iter);
  for(i=0; i<nBuf; i++){
    fts5DoclistIterInit(&aBuf[i], &aMerger[i].iter);
    fts5PrefixMergerInsertByRowid(&pHead, &aMerger[i]);
    nOut += aBuf[i].n;
  }
  if( nOut==0 ) return;
  nOut += p1->n + 9 + 10*nBuf;

  /* The maximum size of the output is equal to the sum of the
  ** input sizes + 1 varint (9 bytes). The extra varint is because if the
  ** first rowid in one input is a large negative number, and the first in
  ** the other a non-negative number, the delta for the non-negative
  ** number will be larger on disk than the literal integer value
  ** was.
  **
  ** Or, if the input position-lists are corrupt, then the output might
  ** include up to (nBuf+1) extra 10-byte positions created by interpreting -1
  ** (the value PoslistNext64() uses for EOF) as a position and appending
  ** it to the output. This can happen at most once for each input
  ** position-list, hence (nBuf+1) 10 byte paddings.  */
  if( sqlite3Fts5BufferSize(&p->rc, &out, nOut) ) return;

  while( pHead ){
    fts5MergeAppendDocid(&out, iLastRowid, pHead->iter.iRowid);

    if( pHead->pNext && iLastRowid==pHead->pNext->iter.iRowid ){
      /* Merge data from two or more poslists */
      i64 iPrev = 0;
      int nTmp = FTS5_DATA_ZERO_PADDING;
      int nMerge = 0;
      PrefixMerger *pSave = pHead;
      PrefixMerger *pThis = 0;
      int nTail = 0;

      pHead = 0;
      while( pSave && pSave->iter.iRowid==iLastRowid ){
        PrefixMerger *pNext = pSave->pNext;
        pSave->iOff = 0;
        pSave->iPos = 0;
        pSave->aPos = &pSave->iter.aPoslist[pSave->iter.nSize];
        fts5PrefixMergerNextPosition(pSave);
        nTmp += pSave->iter.nPoslist + 10;
        nMerge++;
        fts5PrefixMergerInsertByPosition(&pHead, pSave);
        pSave = pNext;
      }

      if( pHead==0 || pHead->pNext==0 ){
        p->rc = FTS5_CORRUPT;
        break;
      }

      /* See the earlier comment in this function for an explanation of why
      ** corrupt input position lists might cause the output to consume
      ** at most nMerge*10 bytes of unexpected space. */
      if( sqlite3Fts5BufferSize(&p->rc, &tmp, nTmp+nMerge*10) ){
        break;
      }
      fts5BufferZero(&tmp);

      pThis = pHead;
      pHead = pThis->pNext;
      sqlite3Fts5PoslistSafeAppend(&tmp, &iPrev, pThis->iPos);
      fts5PrefixMergerNextPosition(pThis);
      fts5PrefixMergerInsertByPosition(&pHead, pThis);

      while( pHead->pNext ){
        pThis = pHead;
        if( pThis->iPos!=iPrev ){
          sqlite3Fts5PoslistSafeAppend(&tmp, &iPrev, pThis->iPos);
        }
        fts5PrefixMergerNextPosition(pThis);
        pHead = pThis->pNext;
        fts5PrefixMergerInsertByPosition(&pHead, pThis);
      }

      if( pHead->iPos!=iPrev ){
        sqlite3Fts5PoslistSafeAppend(&tmp, &iPrev, pHead->iPos);
      }
      nTail = pHead->iter.nPoslist - pHead->iOff;

      /* WRITEPOSLISTSIZE */
      assert_nc( tmp.n+nTail<=nTmp );
      assert( tmp.n+nTail<=nTmp+nMerge*10 );
      if( tmp.n+nTail>nTmp-FTS5_DATA_ZERO_PADDING ){
        if( p->rc==SQLITE_OK ) p->rc = FTS5_CORRUPT;
        break;
      }
      fts5BufferSafeAppendVarint(&out, (tmp.n+nTail) * 2);
      fts5BufferSafeAppendBlob(&out, tmp.p, tmp.n);
      if( nTail>0 ){
        fts5BufferSafeAppendBlob(&out, &pHead->aPos[pHead->iOff], nTail);
      }

      pHead = pSave;
      for(i=0; i<nBuf+1; i++){
        PrefixMerger *pX = &aMerger[i];
        if( pX->iter.aPoslist && pX->iter.iRowid==iLastRowid ){
          fts5DoclistIterNext(&pX->iter);
          fts5PrefixMergerInsertByRowid(&pHead, pX);
        }
      }

    }else{
      /* Copy poslist from pHead to output */
      PrefixMerger *pThis = pHead;
      Fts5DoclistIter *pI = &pThis->iter;
      fts5BufferSafeAppendBlob(&out, pI->aPoslist, pI->nPoslist+pI->nSize);
      fts5DoclistIterNext(pI);
      pHead = pThis->pNext;
      fts5PrefixMergerInsertByRowid(&pHead, pThis);
    }
  }

  fts5BufferFree(p1);
  fts5BufferFree(&tmp);
  memset(&out.p[out.n], 0, FTS5_DATA_ZERO_PADDING);
  *p1 = out;
}
