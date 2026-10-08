
/*
** Discard all data currently cached in the hash-tables.
*/
static void fts5IndexDiscardData(Fts5Index *p){
  assert( p->pHash || p->nPendingData==0 );
  if( p->pHash ){
    sqlite3Fts5HashClear(p->pHash);
    p->nPendingData = 0;
    p->nPendingRow = 0;
    p->flushRc = SQLITE_OK;
  }
  p->nContentlessDelete = 0;
}

/*
** Return the size of the prefix, in bytes, that buffer
** (pNew/<length-unknown>) shares with buffer (pOld/nOld).
**
** Buffer (pNew/<length-unknown>) is guaranteed to be greater
** than buffer (pOld/nOld).
*/
static int fts5PrefixCompress(int nOld, const u8 *pOld, const u8 *pNew){
  int i;
  for(i=0; i<nOld; i++){
    if( pOld[i]!=pNew[i] ) break;
  }
  return i;
}

static void fts5WriteDlidxClear(
  Fts5Index *p,
  Fts5SegWriter *pWriter,
  int bFlush                      /* If true, write dlidx to disk */
){
  int i;
  assert( bFlush==0 || (pWriter->nDlidx>0 && pWriter->aDlidx[0].buf.n>0) );
  for(i=0; i<pWriter->nDlidx; i++){
    Fts5DlidxWriter *pDlidx = &pWriter->aDlidx[i];
    if( pDlidx->buf.n==0 ) break;
    if( bFlush ){
      assert( pDlidx->pgno!=0 );
      fts5DataWrite(p,
          FTS5_DLIDX_ROWID(pWriter->iSegid, i, pDlidx->pgno),
          pDlidx->buf.p, pDlidx->buf.n
      );
    }
    sqlite3Fts5BufferZero(&pDlidx->buf);
    pDlidx->bPrevValid = 0;
  }
}

/*
** Grow the pWriter->aDlidx[] array to at least nLvl elements in size.
** Any new array elements are zeroed before returning.
*/
static int fts5WriteDlidxGrow(
  Fts5Index *p,
  Fts5SegWriter *pWriter,
  int nLvl
){
  if( p->rc==SQLITE_OK && nLvl>=pWriter->nDlidx ){
    Fts5DlidxWriter *aDlidx = (Fts5DlidxWriter*)sqlite3_realloc64(
        pWriter->aDlidx, sizeof(Fts5DlidxWriter) * nLvl
    );
    if( aDlidx==0 ){
      p->rc = SQLITE_NOMEM;
    }else{
      size_t nByte = sizeof(Fts5DlidxWriter) * (nLvl - pWriter->nDlidx);
      memset(&aDlidx[pWriter->nDlidx], 0, nByte);
      pWriter->aDlidx = aDlidx;
      pWriter->nDlidx = nLvl;
    }
  }
  return p->rc;
}

/*
** If the current doclist-index accumulating in pWriter->aDlidx[] is large
** enough, flush it to disk and return 1. Otherwise discard it and return
** zero.
*/
static int fts5WriteFlushDlidx(Fts5Index *p, Fts5SegWriter *pWriter){
  int bFlag = 0;

  /* If there were FTS5_MIN_DLIDX_SIZE or more empty leaf pages written
  ** to the database, also write the doclist-index to disk.  */
  if( pWriter->aDlidx[0].buf.n>0 && pWriter->nEmpty>=FTS5_MIN_DLIDX_SIZE ){
    bFlag = 1;
  }
  fts5WriteDlidxClear(p, pWriter, bFlag);
  pWriter->nEmpty = 0;
  return bFlag;
}

/*
** This function is called whenever processing of the doclist for the
** last term on leaf page (pWriter->iBtPage) is completed.
**
** The doclist-index for that term is currently stored in-memory within the
** Fts5SegWriter.aDlidx[] array. If it is large enough, this function
** writes it out to disk. Or, if it is too small to bother with, discards
** it.
**
** Fts5SegWriter.btterm currently contains the first term on page iBtPage.
*/
static void fts5WriteFlushBtree(Fts5Index *p, Fts5SegWriter *pWriter){
  int bFlag;

  assert( pWriter->iBtPage || pWriter->nEmpty==0 );
  if( pWriter->iBtPage==0 ) return;
  bFlag = fts5WriteFlushDlidx(p, pWriter);

  if( p->rc==SQLITE_OK ){
    const char *z = (pWriter->btterm.n>0?(const char*)pWriter->btterm.p:"");
    /* The following was already done in fts5WriteInit(): */
    /* sqlite3_bind_int(p->pIdxWriter, 1, pWriter->iSegid); */
    sqlite3_bind_blob(p->pIdxWriter, 2, z, pWriter->btterm.n, SQLITE_STATIC);
    sqlite3_bind_int64(p->pIdxWriter, 3, bFlag + ((i64)pWriter->iBtPage<<1));
    sqlite3_step(p->pIdxWriter);
    p->rc = sqlite3_reset(p->pIdxWriter);
    sqlite3_bind_null(p->pIdxWriter, 2);
  }
  pWriter->iBtPage = 0;
}

/*
** This is called once for each leaf page except the first that contains
** at least one term. Argument (nTerm/pTerm) is the split-key - a term that
** is larger than all terms written to earlier leaves, and equal to or
** smaller than the first term on the new leaf.
**
** If an error occurs, an error code is left in Fts5Index.rc. If an error
** has already occurred when this function is called, it is a no-op.
*/
static void fts5WriteBtreeTerm(
  Fts5Index *p,                   /* FTS5 backend object */
  Fts5SegWriter *pWriter,         /* Writer object */
  int nTerm, const u8 *pTerm      /* First term on new page */
){
  fts5WriteFlushBtree(p, pWriter);
  if( p->rc==SQLITE_OK ){
    fts5BufferSet(&p->rc, &pWriter->btterm, nTerm, pTerm);
    pWriter->iBtPage = pWriter->writer.pgno;
  }
}

/*
** This function is called when flushing a leaf page that contains no
** terms at all to disk.
*/
static void fts5WriteBtreeNoTerm(
  Fts5Index *p,                   /* FTS5 backend object */
  Fts5SegWriter *pWriter          /* Writer object */
){
  /* If there were no rowids on the leaf page either and the doclist-index
  ** has already been started, append an 0x00 byte to it.  */
  if( pWriter->bFirstRowidInPage && pWriter->aDlidx[0].buf.n>0 ){
    Fts5DlidxWriter *pDlidx = &pWriter->aDlidx[0];
    assert( pDlidx->bPrevValid );
    sqlite3Fts5BufferAppendVarint(&p->rc, &pDlidx->buf, 0);
  }

  /* Increment the "number of sequential leaves without a term" counter. */
  pWriter->nEmpty++;
}

static i64 fts5DlidxExtractFirstRowid(Fts5Buffer *pBuf){
  i64 iRowid;
  int iOff;

  iOff = 1 + fts5GetVarint(&pBuf->p[1], (u64*)&iRowid);
  fts5GetVarint(&pBuf->p[iOff], (u64*)&iRowid);
  return iRowid;
}

/*
** Rowid iRowid has just been appended to the current leaf page. It is the
** first on the page. This function appends an appropriate entry to the current
** doclist-index.
*/
static void fts5WriteDlidxAppend(
  Fts5Index *p,
  Fts5SegWriter *pWriter,
  i64 iRowid
){
  int i;
  int bDone = 0;

  for(i=0; p->rc==SQLITE_OK && bDone==0; i++){
    i64 iVal;
    Fts5DlidxWriter *pDlidx = &pWriter->aDlidx[i];

    if( pDlidx->buf.n>=p->pConfig->pgsz ){
      /* The current doclist-index page is full. Write it to disk and push
      ** a copy of iRowid (which will become the first rowid on the next
      ** doclist-index leaf page) up into the next level of the b-tree
      ** hierarchy. If the node being flushed is currently the root node,
      ** also push its first rowid upwards. */
      pDlidx->buf.p[0] = 0x01;    /* Not the root node */
      fts5DataWrite(p,
          FTS5_DLIDX_ROWID(pWriter->iSegid, i, pDlidx->pgno),
          pDlidx->buf.p, pDlidx->buf.n
      );
      fts5WriteDlidxGrow(p, pWriter, i+2);
      pDlidx = &pWriter->aDlidx[i];
      if( p->rc==SQLITE_OK && pDlidx[1].buf.n==0 ){
        i64 iFirst = fts5DlidxExtractFirstRowid(&pDlidx->buf);

        /* This was the root node. Push its first rowid up to the new root. */
        pDlidx[1].pgno = pDlidx->pgno;
        sqlite3Fts5BufferAppendVarint(&p->rc, &pDlidx[1].buf, 0);
        sqlite3Fts5BufferAppendVarint(&p->rc, &pDlidx[1].buf, pDlidx->pgno);
        sqlite3Fts5BufferAppendVarint(&p->rc, &pDlidx[1].buf, iFirst);
        pDlidx[1].bPrevValid = 1;
        pDlidx[1].iPrev = iFirst;
      }

      sqlite3Fts5BufferZero(&pDlidx->buf);
      pDlidx->bPrevValid = 0;
      pDlidx->pgno++;
    }else{
      bDone = 1;
    }

    if( pDlidx->bPrevValid ){
      iVal = (u64)iRowid - (u64)pDlidx->iPrev;
    }else{
      i64 iPgno = (i==0 ? pWriter->writer.pgno : pDlidx[-1].pgno);
      assert( pDlidx->buf.n==0 );
      sqlite3Fts5BufferAppendVarint(&p->rc, &pDlidx->buf, !bDone);
      sqlite3Fts5BufferAppendVarint(&p->rc, &pDlidx->buf, iPgno);
      iVal = iRowid;
    }

    sqlite3Fts5BufferAppendVarint(&p->rc, &pDlidx->buf, iVal);
    pDlidx->bPrevValid = 1;
    pDlidx->iPrev = iRowid;
  }
}

static void fts5WriteFlushLeaf(Fts5Index *p, Fts5SegWriter *pWriter){
  static const u8 zero[] = { 0x00, 0x00, 0x00, 0x00 };
  Fts5PageWriter *pPage = &pWriter->writer;
  i64 iRowid;

  assert( (pPage->pgidx.n==0)==(pWriter->bFirstTermInPage) );

  /* Set the szLeaf header field. */
  assert( 0==fts5GetU16(&pPage->buf.p[2]) );
  fts5PutU16(&pPage->buf.p[2], (u16)pPage->buf.n);

  if( pWriter->bFirstTermInPage ){
    /* No term was written to this page. */
    assert( pPage->pgidx.n==0 );
    fts5WriteBtreeNoTerm(p, pWriter);
  }else{
    /* Append the pgidx to the page buffer. Set the szLeaf header field. */
    fts5BufferAppendBlob(&p->rc, &pPage->buf, pPage->pgidx.n, pPage->pgidx.p);
  }

  /* Write the page out to disk */
  iRowid = FTS5_SEGMENT_ROWID(pWriter->iSegid, pPage->pgno);
  fts5DataWrite(p, iRowid, pPage->buf.p, pPage->buf.n);

  /* Initialize the next page. */
  fts5BufferZero(&pPage->buf);
  fts5BufferZero(&pPage->pgidx);
  fts5BufferAppendBlob(&p->rc, &pPage->buf, 4, zero);
  pPage->iPrevPgidx = 0;
  pPage->pgno++;

  /* Increase the leaves written counter */
  pWriter->nLeafWritten++;

  /* The new leaf holds no terms or rowids */
  pWriter->bFirstTermInPage = 1;
  pWriter->bFirstRowidInPage = 1;
}

/*
** Append term pTerm/nTerm to the segment being written by the writer passed
** as the second argument.
**
** If an error occurs, set the Fts5Index.rc error code. If an error has
** already occurred, this function is a no-op.
*/
static void fts5WriteAppendTerm(
  Fts5Index *p,
  Fts5SegWriter *pWriter,
  int nTerm, const u8 *pTerm
){
  int nPrefix;                    /* Bytes of prefix compression for term */
  Fts5PageWriter *pPage = &pWriter->writer;
  Fts5Buffer *pPgidx = &pWriter->writer.pgidx;
  int nMin = MIN(pPage->term.n, nTerm);

  assert( p->rc==SQLITE_OK );
  assert( pPage->buf.n>=4 );
  assert( pPage->buf.n>4 || pWriter->bFirstTermInPage );

  /* If the current leaf page is full, flush it to disk. */
  if( (pPage->buf.n + pPgidx->n + nTerm + 2)>=p->pConfig->pgsz ){
    if( pPage->buf.n>4 ){
      fts5WriteFlushLeaf(p, pWriter);
      if( p->rc!=SQLITE_OK ) return;
    }
    fts5BufferGrow(&p->rc, &pPage->buf, nTerm+FTS5_DATA_PADDING);
  }

  /* TODO1: Updating pgidx here. */
  pPgidx->n += sqlite3Fts5PutVarint(
      &pPgidx->p[pPgidx->n], pPage->buf.n - pPage->iPrevPgidx
  );
  pPage->iPrevPgidx = pPage->buf.n;
#if 0
  fts5PutU16(&pPgidx->p[pPgidx->n], pPage->buf.n);
  pPgidx->n += 2;
#endif

  if( pWriter->bFirstTermInPage ){
    nPrefix = 0;
    if( pPage->pgno!=1 ){
      /* This is the first term on a leaf that is not the leftmost leaf in
      ** the segment b-tree. In this case it is necessary to add a term to
      ** the b-tree hierarchy that is (a) larger than the largest term
      ** already written to the segment and (b) smaller than or equal to
      ** this term. In other words, a prefix of (pTerm/nTerm) that is one
      ** byte longer than the longest prefix (pTerm/nTerm) shares with the
      ** previous term.
      **
      ** Usually, the previous term is available in pPage->term. The exception
      ** is if this is the first term written in an incremental-merge step.
      ** In this case the previous term is not available, so just write a
      ** copy of (pTerm/nTerm) into the parent node. This is slightly
      ** inefficient, but still correct.  */
      int n = nTerm;
      if( pPage->term.n ){
        n = 1 + fts5PrefixCompress(nMin, pPage->term.p, pTerm);
      }
      fts5WriteBtreeTerm(p, pWriter, n, pTerm);
      if( p->rc!=SQLITE_OK ) return;
      pPage = &pWriter->writer;
    }
  }else{
    nPrefix = fts5PrefixCompress(nMin, pPage->term.p, pTerm);
    fts5BufferAppendVarint(&p->rc, &pPage->buf, nPrefix);
  }

  /* Append the number of bytes of new data, then the term data itself
  ** to the page. */
  fts5BufferAppendVarint(&p->rc, &pPage->buf, nTerm - nPrefix);
  fts5BufferAppendBlob(&p->rc, &pPage->buf, nTerm - nPrefix, &pTerm[nPrefix]);

  /* Update the Fts5PageWriter.term field. */
  fts5BufferSet(&p->rc, &pPage->term, nTerm, pTerm);
  pWriter->bFirstTermInPage = 0;

  pWriter->bFirstRowidInPage = 0;
  pWriter->bFirstRowidInDoclist = 1;

  assert( p->rc || (pWriter->nDlidx>0 && pWriter->aDlidx[0].buf.n==0) );
  pWriter->aDlidx[0].pgno = pPage->pgno;
}
