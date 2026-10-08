

/*
** Run internal checks to ensure that the FTS index (a) is internally
** consistent and (b) contains entries for which the XOR of the checksums
** as calculated by sqlite3Fts5IndexEntryCksum() is cksum.
**
** Return SQLITE_CORRUPT if any of the internal checks fail, or if the
** checksum does not match. Return SQLITE_OK if all checks pass without
** error, or some other SQLite error code if another error (e.g. OOM)
** occurs.
*/
static int sqlite3Fts5IndexIntegrityCheck(Fts5Index *p, u64 cksum, int bUseCksum){
  int eDetail = p->pConfig->eDetail;
  u64 cksum2 = 0;                 /* Checksum based on contents of indexes */
  Fts5Buffer poslist = {0,0,0};   /* Buffer used to hold a poslist */
  Fts5Iter *pIter;                /* Used to iterate through entire index */
  Fts5Structure *pStruct;         /* Index structure */
  int iLvl, iSeg;

#ifdef SQLITE_DEBUG
  /* Used by extra internal tests only run if NDEBUG is not defined */
  u64 cksum3 = 0;                 /* Checksum based on contents of indexes */
  Fts5Buffer term = {0,0,0};      /* Buffer used to hold most recent term */
#endif
  const int flags = FTS5INDEX_QUERY_NOOUTPUT;

  /* Load the FTS index structure */
  pStruct = fts5StructureRead(p);
  if( pStruct==0 ){
    assert( p->rc!=SQLITE_OK );
    return fts5IndexReturn(p);
  }

  /* Check that the internal nodes of each segment match the leaves */
  for(iLvl=0; iLvl<pStruct->nLevel; iLvl++){
    for(iSeg=0; iSeg<pStruct->aLevel[iLvl].nSeg; iSeg++){
      Fts5StructureSegment *pSeg = &pStruct->aLevel[iLvl].aSeg[iSeg];
      fts5IndexIntegrityCheckSegment(p, pSeg);
    }
  }

  /* The cksum argument passed to this function is a checksum calculated
  ** based on all expected entries in the FTS index (including prefix index
  ** entries). This block checks that a checksum calculated based on the
  ** actual contents of FTS index is identical.
  **
  ** Two versions of the same checksum are calculated. The first (stack
  ** variable cksum2) based on entries extracted from the full-text index
  ** while doing a linear scan of each individual index in turn.
  **
  ** As each term visited by the linear scans, a separate query for the
  ** same term is performed. cksum3 is calculated based on the entries
  ** extracted by these queries.
  */
  for(fts5MultiIterNew(p, pStruct, flags, 0, 0, 0, -1, 0, &pIter);
      fts5MultiIterEof(p, pIter)==0;
      fts5MultiIterNext(p, pIter, 0, 0)
  ){
    int n;                      /* Size of term in bytes */
    i64 iPos = 0;               /* Position read from poslist */
    int iOff = 0;               /* Offset within poslist */
    i64 iRowid = fts5MultiIterRowid(pIter);
    char *z = (char*)fts5MultiIterTerm(pIter, &n);

    /* If this is a new term, query for it. Update cksum3 with the results. */
    fts5TestTerm(p, &term, z, n, cksum2, &cksum3);
    if( p->rc ) break;

    if( eDetail==FTS5_DETAIL_NONE ){
      if( 0==fts5MultiIterIsEmpty(p, pIter) ){
        cksum2 ^= sqlite3Fts5IndexEntryCksum(iRowid, 0, 0, -1, z, n);
      }
    }else{
      poslist.n = 0;
      fts5SegiterPoslist(p, &pIter->aSeg[pIter->aFirst[1].iFirst], 0, &poslist);
      fts5BufferAppendBlob(&p->rc, &poslist, 4, (const u8*)"\0\0\0\0");
      while( 0==sqlite3Fts5PoslistNext64(poslist.p, poslist.n, &iOff, &iPos) ){
        int iCol = FTS5_POS2COLUMN(iPos);
        int iTokOff = FTS5_POS2OFFSET(iPos);
        cksum2 ^= sqlite3Fts5IndexEntryCksum(iRowid, iCol, iTokOff, -1, z, n);
      }
    }
  }
  fts5TestTerm(p, &term, 0, 0, cksum2, &cksum3);

  fts5MultiIterFree(pIter);
  if( p->rc==SQLITE_OK && bUseCksum && cksum!=cksum2 ) p->rc = FTS5_CORRUPT;

  fts5StructureRelease(pStruct);
#ifdef SQLITE_DEBUG
  fts5BufferFree(&term);
#endif
  fts5BufferFree(&poslist);
  return fts5IndexReturn(p);
}

/*************************************************************************
**************************************************************************
** Below this point is the implementation of the fts5_decode() scalar
** function only.
*/

#if defined(SQLITE_TEST) || defined(SQLITE_FTS5_DEBUG)
/*
** Decode a segment-data rowid from the %_data table. This function is
** the opposite of macro FTS5_SEGMENT_ROWID().
*/
static void fts5DecodeRowid(
  i64 iRowid,                     /* Rowid from %_data table */
  int *pbTombstone,               /* OUT: Tombstone hash flag */
  int *piSegid,                   /* OUT: Segment id */
  int *pbDlidx,                   /* OUT: Dlidx flag */
  int *piHeight,                  /* OUT: Height */
  int *piPgno                     /* OUT: Page number */
){
  *piPgno = (int)(iRowid & (((i64)1 << FTS5_DATA_PAGE_B) - 1));
  iRowid >>= FTS5_DATA_PAGE_B;

  *piHeight = (int)(iRowid & (((i64)1 << FTS5_DATA_HEIGHT_B) - 1));
  iRowid >>= FTS5_DATA_HEIGHT_B;

  *pbDlidx = (int)(iRowid & 0x0001);
  iRowid >>= FTS5_DATA_DLI_B;

  *piSegid = (int)(iRowid & (((i64)1 << FTS5_DATA_ID_B) - 1));
  iRowid >>= FTS5_DATA_ID_B;

  *pbTombstone = (int)(iRowid & 0x0001);
}
#endif /* SQLITE_TEST || SQLITE_FTS5_DEBUG */

#if defined(SQLITE_TEST) || defined(SQLITE_FTS5_DEBUG)
static void fts5DebugRowid(int *pRc, Fts5Buffer *pBuf, i64 iKey){
  int iSegid, iHeight, iPgno, bDlidx, bTomb;     /* Rowid compenents */
  fts5DecodeRowid(iKey, &bTomb, &iSegid, &bDlidx, &iHeight, &iPgno);

  if( iSegid==0 ){
    if( iKey==FTS5_AVERAGES_ROWID ){
      sqlite3Fts5BufferAppendPrintf(pRc, pBuf, "{averages} ");
    }else{
      sqlite3Fts5BufferAppendPrintf(pRc, pBuf, "{structure}");
    }
  }
  else{
    sqlite3Fts5BufferAppendPrintf(pRc, pBuf, "{%s%ssegid=%d h=%d pgno=%d}",
        bDlidx ? "dlidx " : "",
        bTomb ? "tombstone " : "",
        iSegid, iHeight, iPgno
    );
  }
}
#endif /* SQLITE_TEST || SQLITE_FTS5_DEBUG */

#if defined(SQLITE_TEST) || defined(SQLITE_FTS5_DEBUG)
static void fts5DebugStructure(
  int *pRc,                       /* IN/OUT: error code */
  Fts5Buffer *pBuf,
  Fts5Structure *p
){
  int iLvl, iSeg;                 /* Iterate through levels, segments */

  for(iLvl=0; iLvl<p->nLevel; iLvl++){
    Fts5StructureLevel *pLvl = &p->aLevel[iLvl];
    sqlite3Fts5BufferAppendPrintf(pRc, pBuf,
        " {lvl=%d nMerge=%d nSeg=%d", iLvl, pLvl->nMerge, pLvl->nSeg
    );
    for(iSeg=0; iSeg<pLvl->nSeg; iSeg++){
      Fts5StructureSegment *pSeg = &pLvl->aSeg[iSeg];
      sqlite3Fts5BufferAppendPrintf(pRc, pBuf, " {id=%d leaves=%d..%d",
          pSeg->iSegid, pSeg->pgnoFirst, pSeg->pgnoLast
      );
      if( pSeg->iOrigin1>0 ){
        sqlite3Fts5BufferAppendPrintf(pRc, pBuf, " origin=%lld..%lld",
            pSeg->iOrigin1, pSeg->iOrigin2
        );
      }
      sqlite3Fts5BufferAppendPrintf(pRc, pBuf, "}");
    }
    sqlite3Fts5BufferAppendPrintf(pRc, pBuf, "}");
  }
}
#endif /* SQLITE_TEST || SQLITE_FTS5_DEBUG */

#if defined(SQLITE_TEST) || defined(SQLITE_FTS5_DEBUG)
/*
** This is part of the fts5_decode() debugging aid.
**
** Arguments pBlob/nBlob contain a serialized Fts5Structure object. This
** function appends a human-readable representation of the same object
** to the buffer passed as the second argument.
*/
static void fts5DecodeStructure(
  int *pRc,                       /* IN/OUT: error code */
  Fts5Buffer *pBuf,
  const u8 *pBlob, int nBlob
){
  int rc;                         /* Return code */
  Fts5Structure *p = 0;           /* Decoded structure object */

  rc = fts5StructureDecode(pBlob, nBlob, 0, &p);
  if( rc!=SQLITE_OK ){
    *pRc = rc;
    return;
  }

  fts5DebugStructure(pRc, pBuf, p);
  fts5StructureRelease(p);
}
#endif /* SQLITE_TEST || SQLITE_FTS5_DEBUG */

#if defined(SQLITE_TEST) || defined(SQLITE_FTS5_DEBUG)
/*
** This is part of the fts5_decode() debugging aid.
**
** Arguments pBlob/nBlob contain an "averages" record. This function
** appends a human-readable representation of record to the buffer passed
** as the second argument.
*/
static void fts5DecodeAverages(
  int *pRc,                       /* IN/OUT: error code */
  Fts5Buffer *pBuf,
  const u8 *pBlob, int nBlob
){
  int i = 0;
  const char *zSpace = "";

  while( i<nBlob ){
    u64 iVal;
    i += sqlite3Fts5GetVarint(&pBlob[i], &iVal);
    sqlite3Fts5BufferAppendPrintf(pRc, pBuf, "%s%d", zSpace, (int)iVal);
    zSpace = " ";
  }
}
#endif /* SQLITE_TEST || SQLITE_FTS5_DEBUG */

#if defined(SQLITE_TEST) || defined(SQLITE_FTS5_DEBUG)
/*
** Buffer (a/n) is assumed to contain a list of serialized varints. Read
** each varint and append its string representation to buffer pBuf. Return
** after either the input buffer is exhausted or a 0 value is read.
**
** The return value is the number of bytes read from the input buffer.
*/
static int fts5DecodePoslist(int *pRc, Fts5Buffer *pBuf, const u8 *a, int n){
  int iOff = 0;
  while( iOff<n ){
    int iVal;
    iOff += fts5GetVarint32(&a[iOff], iVal);
    sqlite3Fts5BufferAppendPrintf(pRc, pBuf, " %d", iVal);
  }
  return iOff;
}
#endif /* SQLITE_TEST || SQLITE_FTS5_DEBUG */

#if defined(SQLITE_TEST) || defined(SQLITE_FTS5_DEBUG)
/*
** The start of buffer (a/n) contains the start of a doclist. The doclist
** may or may not finish within the buffer. This function appends a text
** representation of the part of the doclist that is present to buffer
** pBuf.
**
** The return value is the number of bytes read from the input buffer.
*/
static int fts5DecodeDoclist(int *pRc, Fts5Buffer *pBuf, const u8 *a, int n){
  i64 iDocid = 0;
  int iOff = 0;

  if( n>0 ){
    iOff = sqlite3Fts5GetVarint(a, (u64*)&iDocid);
    sqlite3Fts5BufferAppendPrintf(pRc, pBuf, " id=%lld", iDocid);
  }
  while( iOff<n ){
    int nPos;
    int bDel;
    iOff += fts5GetPoslistSize(&a[iOff], &nPos, &bDel);
    sqlite3Fts5BufferAppendPrintf(pRc, pBuf, " nPos=%d%s", nPos, bDel?"*":"");
    iOff += fts5DecodePoslist(pRc, pBuf, &a[iOff], MIN(n-iOff, nPos));
    if( iOff<n ){
      i64 iDelta;
      iOff += sqlite3Fts5GetVarint(&a[iOff], (u64*)&iDelta);
      iDocid += iDelta;
      sqlite3Fts5BufferAppendPrintf(pRc, pBuf, " id=%lld", iDocid);
    }
  }

  return iOff;
}
#endif /* SQLITE_TEST || SQLITE_FTS5_DEBUG */

#if defined(SQLITE_TEST) || defined(SQLITE_FTS5_DEBUG)
/*
** This function is part of the fts5_decode() debugging function. It is
** only ever used with detail=none tables.
**
** Buffer (pData/nData) contains a doclist in the format used by detail=none
** tables. This function appends a human-readable version of that list to
** buffer pBuf.
**
** If *pRc is other than SQLITE_OK when this function is called, it is a
** no-op. If an OOM or other error occurs within this function, *pRc is
** set to an SQLite error code before returning. The final state of buffer
** pBuf is undefined in this case.
*/
static void fts5DecodeRowidList(
  int *pRc,                       /* IN/OUT: Error code */
  Fts5Buffer *pBuf,               /* Buffer to append text to */
  const u8 *pData, int nData      /* Data to decode list-of-rowids from */
){
  int i = 0;
  i64 iRowid = 0;

  while( i<nData ){
    const char *zApp = "";
    u64 iVal;
    i += sqlite3Fts5GetVarint(&pData[i], &iVal);
    iRowid += iVal;

    if( i<nData && pData[i]==0x00 ){
      i++;
      if( i<nData && pData[i]==0x00 ){
        i++;
        zApp = "+";
      }else{
        zApp = "*";
      }
    }

    sqlite3Fts5BufferAppendPrintf(pRc, pBuf, " %lld%s", iRowid, zApp);
  }
}
#endif /* SQLITE_TEST || SQLITE_FTS5_DEBUG */

#if defined(SQLITE_TEST) || defined(SQLITE_FTS5_DEBUG)
static void fts5BufferAppendTerm(int *pRc, Fts5Buffer *pBuf, Fts5Buffer *pTerm){
  int ii;
  fts5BufferGrow(pRc, pBuf, pTerm->n*2 + 1);
  if( *pRc==SQLITE_OK ){
    for(ii=0; ii<pTerm->n; ii++){
      if( pTerm->p[ii]==0x00 ){
        pBuf->p[pBuf->n++] = '\\';
        pBuf->p[pBuf->n++] = '0';
      }else{
        pBuf->p[pBuf->n++] = pTerm->p[ii];
      }
    }
    pBuf->p[pBuf->n] = 0x00;
  }
}
#endif /* SQLITE_TEST || SQLITE_FTS5_DEBUG */

#if defined(SQLITE_TEST) || defined(SQLITE_FTS5_DEBUG)
/*
** The implementation of user-defined scalar function fts5_decode().
*/
static void fts5DecodeFunction(
  sqlite3_context *pCtx,          /* Function call context */
  int nArg,                       /* Number of args (always 2) */
  sqlite3_value **apVal           /* Function arguments */
){
  i64 iRowid;                     /* Rowid for record being decoded */
  int iSegid,iHeight,iPgno,bDlidx;/* Rowid components */
  int bTomb;
  const u8 *aBlob; int n;         /* Record to decode */
  u8 *a = 0;
  Fts5Buffer s;                   /* Build up text to return here */
  int rc = SQLITE_OK;             /* Return code */
  sqlite3_int64 nSpace = 0;
  int eDetailNone = (sqlite3_user_data(pCtx)!=0);

  assert( nArg==2 );
  UNUSED_PARAM(nArg);
  memset(&s, 0, sizeof(Fts5Buffer));
  iRowid = sqlite3_value_int64(apVal[0]);

  /* Make a copy of the second argument (a blob) in aBlob[]. The aBlob[]
  ** copy is followed by FTS5_DATA_ZERO_PADDING 0x00 bytes, which prevents
  ** buffer overreads even if the record is corrupt.  */
  n = sqlite3_value_bytes(apVal[1]);
  aBlob = sqlite3_value_blob(apVal[1]);
  nSpace = n + FTS5_DATA_ZERO_PADDING;
  a = (u8*)sqlite3Fts5MallocZero(&rc, nSpace);
  if( a==0 ) goto decode_out;
  if( n>0 ) memcpy(a, aBlob, n);

  fts5DecodeRowid(iRowid, &bTomb, &iSegid, &bDlidx, &iHeight, &iPgno);

  fts5DebugRowid(&rc, &s, iRowid);
  if( bDlidx ){
    Fts5Data dlidx;
    Fts5DlidxLvl lvl;

    dlidx.p = a;
    dlidx.nn = n;

    memset(&lvl, 0, sizeof(Fts5DlidxLvl));
    lvl.pData = &dlidx;
    lvl.iLeafPgno = iPgno;

    for(fts5DlidxLvlNext(&lvl); lvl.bEof==0; fts5DlidxLvlNext(&lvl)){
      sqlite3Fts5BufferAppendPrintf(&rc, &s,
          " %d(%lld)", lvl.iLeafPgno, lvl.iRowid
      );
    }
  }else if( bTomb ){
    u32 nElem  = fts5GetU32(&a[4]);
    int szKey = (aBlob[0]==4 || aBlob[0]==8) ? aBlob[0] : 8;
    int nSlot = (n - 8) / szKey;
    int ii;
    sqlite3Fts5BufferAppendPrintf(&rc, &s, " nElem=%d", (int)nElem);
    if( aBlob[1] ){
      sqlite3Fts5BufferAppendPrintf(&rc, &s, " 0");
    }
    for(ii=0; ii<nSlot; ii++){
      u64 iVal = 0;
      if( szKey==4 ){
        u32 *aSlot = (u32*)&aBlob[8];
        if( aSlot[ii] ) iVal = fts5GetU32((u8*)&aSlot[ii]);
      }else{
        u64 *aSlot = (u64*)&aBlob[8];
        if( aSlot[ii] ) iVal = fts5GetU64((u8*)&aSlot[ii]);
      }
      if( iVal!=0 ){
        sqlite3Fts5BufferAppendPrintf(&rc, &s, " %lld", (i64)iVal);
      }
    }
  }else if( iSegid==0 ){
    if( iRowid==FTS5_AVERAGES_ROWID ){
      fts5DecodeAverages(&rc, &s, a, n);
    }else{
      fts5DecodeStructure(&rc, &s, a, n);
    }
  }else if( eDetailNone ){
    Fts5Buffer term;              /* Current term read from page */
    int szLeaf;
    int iPgidxOff = szLeaf = fts5GetU16(&a[2]);
    int iTermOff;
    int nKeep = 0;
    int iOff;

    memset(&term, 0, sizeof(Fts5Buffer));

    /* Decode any entries that occur before the first term. */
    if( szLeaf<n ){
      iPgidxOff += fts5GetVarint32(&a[iPgidxOff], iTermOff);
    }else{
      iTermOff = szLeaf;
    }
    fts5DecodeRowidList(&rc, &s, &a[4], iTermOff-4);

    iOff = iTermOff;
    while( iOff<szLeaf && rc==SQLITE_OK ){
      int nAppend;

      /* Read the term data for the next term*/
      iOff += fts5GetVarint32(&a[iOff], nAppend);
      term.n = nKeep;
      fts5BufferAppendBlob(&rc, &term, nAppend, &a[iOff]);
      sqlite3Fts5BufferAppendPrintf(&rc, &s, " term=");
      fts5BufferAppendTerm(&rc, &s, &term);
      iOff += nAppend;

      /* Figure out where the doclist for this term ends */
      if( iPgidxOff<n ){
        int nIncr;
        iPgidxOff += fts5GetVarint32(&a[iPgidxOff], nIncr);
        iTermOff += nIncr;
      }else{
        iTermOff = szLeaf;
      }
      if( iTermOff>szLeaf ){
        rc = FTS5_CORRUPT;
      }else{
        fts5DecodeRowidList(&rc, &s, &a[iOff], iTermOff-iOff);
      }
      iOff = iTermOff;
      if( iOff<szLeaf ){
        iOff += fts5GetVarint32(&a[iOff], nKeep);
      }
    }

    fts5BufferFree(&term);
  }else{
    Fts5Buffer term;              /* Current term read from page */
    int szLeaf;                   /* Offset of pgidx in a[] */
    int iPgidxOff;
    int iPgidxPrev = 0;           /* Previous value read from pgidx */
    int iTermOff = 0;
    int iRowidOff = 0;
    int iOff;
    int nDoclist;

    memset(&term, 0, sizeof(Fts5Buffer));

    if( n<4 ){
      sqlite3Fts5BufferSet(&rc, &s, 7, (const u8*)"corrupt");
      goto decode_out;
    }else{
      iRowidOff = fts5GetU16(&a[0]);
      iPgidxOff = szLeaf = fts5GetU16(&a[2]);
      if( iPgidxOff<n ){
        fts5GetVarint32(&a[iPgidxOff], iTermOff);
      }else if( iPgidxOff>n ){
        rc = FTS5_CORRUPT;
        goto decode_out;
      }
    }

    /* Decode the position list tail at the start of the page */
    if( iRowidOff!=0 ){
      iOff = iRowidOff;
    }else if( iTermOff!=0 ){
      iOff = iTermOff;
    }else{
      iOff = szLeaf;
    }
    if( iOff>n ){
      rc = FTS5_CORRUPT;
      goto decode_out;
    }
    fts5DecodePoslist(&rc, &s, &a[4], iOff-4);

    /* Decode any more doclist data that appears on the page before the
    ** first term. */
    nDoclist = (iTermOff ? iTermOff : szLeaf) - iOff;
    if( nDoclist+iOff>n ){
      rc = FTS5_CORRUPT;
      goto decode_out;
    }
    fts5DecodeDoclist(&rc, &s, &a[iOff], nDoclist);

    while( iPgidxOff<n && rc==SQLITE_OK ){
      int bFirst = (iPgidxOff==szLeaf);     /* True for first term on page */
      int nByte;                            /* Bytes of data */
      int iEnd;

      iPgidxOff += fts5GetVarint32(&a[iPgidxOff], nByte);
      iPgidxPrev += nByte;
      iOff = iPgidxPrev;

      if( iPgidxOff<n ){
        fts5GetVarint32(&a[iPgidxOff], nByte);
        iEnd = iPgidxPrev + nByte;
      }else{
        iEnd = szLeaf;
      }
      if( iEnd>szLeaf ){
        rc = FTS5_CORRUPT;
        break;
      }

      if( bFirst==0 ){
        iOff += fts5GetVarint32(&a[iOff], nByte);
        if( nByte>term.n ){
          rc = FTS5_CORRUPT;
          break;
        }
        term.n = nByte;
      }
      iOff += fts5GetVarint32(&a[iOff], nByte);
      if( iOff+nByte>n ){
        rc = FTS5_CORRUPT;
        break;
      }
      fts5BufferAppendBlob(&rc, &term, nByte, &a[iOff]);
      iOff += nByte;

      sqlite3Fts5BufferAppendPrintf(&rc, &s, " term=");
      fts5BufferAppendTerm(&rc, &s, &term);
      iOff += fts5DecodeDoclist(&rc, &s, &a[iOff], iEnd-iOff);
    }

    fts5BufferFree(&term);
  }

 decode_out:
  sqlite3_free(a);
  if( rc==SQLITE_OK ){
    sqlite3_result_text(pCtx, (const char*)s.p, s.n, SQLITE_TRANSIENT);
  }else{
    sqlite3_result_error_code(pCtx, rc);
  }
  fts5BufferFree(&s);
}
