
#ifdef SQLITE_DEBUG
/*
** This function is purely an internal test. It does not contribute to
** FTS functionality, or even the integrity-check, in any way.
**
** Instead, it tests that the same set of pgno/rowid combinations are
** visited regardless of whether the doclist-index identified by parameters
** iSegid/iLeaf is iterated in forwards or reverse order.
*/
static void fts5TestDlidxReverse(
  Fts5Index *p,
  int iSegid,                     /* Segment id to load from */
  int iLeaf                       /* Load doclist-index for this leaf */
){
  Fts5DlidxIter *pDlidx = 0;
  u64 cksum1 = 13;
  u64 cksum2 = 13;

  for(pDlidx=fts5DlidxIterInit(p, 0, iSegid, iLeaf);
      fts5DlidxIterEof(p, pDlidx)==0;
      fts5DlidxIterNext(p, pDlidx)
  ){
    i64 iRowid = fts5DlidxIterRowid(pDlidx);
    int pgno = fts5DlidxIterPgno(pDlidx);
    assert( pgno>iLeaf );
    cksum1 += iRowid + ((i64)pgno<<32);
  }
  fts5DlidxIterFree(pDlidx);
  pDlidx = 0;

  for(pDlidx=fts5DlidxIterInit(p, 1, iSegid, iLeaf);
      fts5DlidxIterEof(p, pDlidx)==0;
      fts5DlidxIterPrev(p, pDlidx)
  ){
    i64 iRowid = fts5DlidxIterRowid(pDlidx);
    int pgno = fts5DlidxIterPgno(pDlidx);
    assert( fts5DlidxIterPgno(pDlidx)>iLeaf );
    cksum2 += iRowid + ((i64)pgno<<32);
  }
  fts5DlidxIterFree(pDlidx);
  pDlidx = 0;

  if( p->rc==SQLITE_OK && cksum1!=cksum2 ) p->rc = FTS5_CORRUPT;
}

static int fts5QueryCksum(
  Fts5Index *p,                   /* Fts5 index object */
  int iIdx,
  const char *z,                  /* Index key to query for */
  int n,                          /* Size of index key in bytes */
  int flags,                      /* Flags for Fts5IndexQuery */
  u64 *pCksum                     /* IN/OUT: Checksum value */
){
  int eDetail = p->pConfig->eDetail;
  u64 cksum = *pCksum;
  Fts5IndexIter *pIter = 0;
  int rc = sqlite3Fts5IndexQuery(
      p, z, n, (flags | FTS5INDEX_QUERY_NOTOKENDATA), 0, &pIter
  );

  while( rc==SQLITE_OK && ALWAYS(pIter!=0) && 0==sqlite3Fts5IterEof(pIter) ){
    i64 rowid = pIter->iRowid;

    if( eDetail==FTS5_DETAIL_NONE ){
      cksum ^= sqlite3Fts5IndexEntryCksum(rowid, 0, 0, iIdx, z, n);
    }else{
      Fts5PoslistReader sReader;
      for(sqlite3Fts5PoslistReaderInit(pIter->pData, pIter->nData, &sReader);
          sReader.bEof==0;
          sqlite3Fts5PoslistReaderNext(&sReader)
      ){
        int iCol = FTS5_POS2COLUMN(sReader.iPos);
        int iOff = FTS5_POS2OFFSET(sReader.iPos);
        cksum ^= sqlite3Fts5IndexEntryCksum(rowid, iCol, iOff, iIdx, z, n);
      }
    }
    if( rc==SQLITE_OK ){
      rc = sqlite3Fts5IterNext(pIter);
    }
  }
  sqlite3Fts5IterClose(pIter);

  *pCksum = cksum;
  return rc;
}

/*
** Check if buffer z[], size n bytes, contains as series of valid utf-8
** encoded codepoints. If so, return 0. Otherwise, if the buffer does not
** contain valid utf-8, return non-zero.
*/
static int fts5TestUtf8(const char *z, int n){
  int i = 0;
  assert_nc( n>0 );
  while( i<n ){
    if( (z[i] & 0x80)==0x00 ){
      i++;
    }else
    if( (z[i] & 0xE0)==0xC0 ){
      if( i+1>=n || (z[i+1] & 0xC0)!=0x80 ) return 1;
      i += 2;
    }else
    if( (z[i] & 0xF0)==0xE0 ){
      if( i+2>=n || (z[i+1] & 0xC0)!=0x80 || (z[i+2] & 0xC0)!=0x80 ) return 1;
      i += 3;
    }else
    if( (z[i] & 0xF8)==0xF0 ){
      if( i+3>=n || (z[i+1] & 0xC0)!=0x80 || (z[i+2] & 0xC0)!=0x80 ) return 1;
      if( (z[i+2] & 0xC0)!=0x80 ) return 1;
      i += 3;
    }else{
      return 1;
    }
  }

  return 0;
}

/*
** This function is also purely an internal test. It does not contribute to
** FTS functionality, or even the integrity-check, in any way.
*/
static void fts5TestTerm(
  Fts5Index *p,
  Fts5Buffer *pPrev,              /* Previous term */
  const char *z, int n,           /* Possibly new term to test */
  u64 expected,
  u64 *pCksum
){
  int rc = p->rc;
  if( pPrev->n==0 ){
    fts5BufferSet(&rc, pPrev, n, (const u8*)z);
  }else
  if( rc==SQLITE_OK && (pPrev->n!=n || memcmp(pPrev->p, z, n)) ){
    u64 cksum3 = *pCksum;
    const char *zTerm = (const char*)&pPrev->p[1];  /* term sans prefix-byte */
    int nTerm = pPrev->n-1;            /* Size of zTerm in bytes */
    int iIdx = (pPrev->p[0] - FTS5_MAIN_PREFIX);
    int flags = (iIdx==0 ? 0 : FTS5INDEX_QUERY_PREFIX);
    u64 ck1 = 0;
    u64 ck2 = 0;

    /* Check that the results returned for ASC and DESC queries are
    ** the same. If not, call this corruption.  */
    rc = fts5QueryCksum(p, iIdx, zTerm, nTerm, flags, &ck1);
    if( rc==SQLITE_OK ){
      int f = flags|FTS5INDEX_QUERY_DESC;
      rc = fts5QueryCksum(p, iIdx, zTerm, nTerm, f, &ck2);
    }
    if( rc==SQLITE_OK && ck1!=ck2 ) rc = FTS5_CORRUPT;

    /* If this is a prefix query, check that the results returned if the
    ** the index is disabled are the same. In both ASC and DESC order.
    **
    ** This check may only be performed if the hash table is empty. This
    ** is because the hash table only supports a single scan query at
    ** a time, and the multi-iter loop from which this function is called
    ** is already performing such a scan.
    **
    ** Also only do this if buffer zTerm contains nTerm bytes of valid
    ** utf-8. Otherwise, the last part of the buffer contents might contain
    ** a non-utf-8 sequence that happens to be a prefix of a valid utf-8
    ** character stored in the main fts index, which will cause the
    ** test to fail.  */
    if( p->nPendingData==0 && 0==fts5TestUtf8(zTerm, nTerm) ){
      if( iIdx>0 && rc==SQLITE_OK ){
        int f = flags|FTS5INDEX_QUERY_TEST_NOIDX;
        ck2 = 0;
        rc = fts5QueryCksum(p, iIdx, zTerm, nTerm, f, &ck2);
        if( rc==SQLITE_OK && ck1!=ck2 ) rc = FTS5_CORRUPT;
      }
      if( iIdx>0 && rc==SQLITE_OK ){
        int f = flags|FTS5INDEX_QUERY_TEST_NOIDX|FTS5INDEX_QUERY_DESC;
        ck2 = 0;
        rc = fts5QueryCksum(p, iIdx, zTerm, nTerm, f, &ck2);
        if( rc==SQLITE_OK && ck1!=ck2 ) rc = FTS5_CORRUPT;
      }
    }

    cksum3 ^= ck1;
    fts5BufferSet(&rc, pPrev, n, (const u8*)z);

    if( rc==SQLITE_OK && cksum3!=expected ){
      rc = FTS5_CORRUPT;
    }
    *pCksum = cksum3;
  }
  p->rc = rc;
}

#else
# define fts5TestDlidxReverse(x,y,z)
# define fts5TestTerm(u,v,w,x,y,z)
#endif

/*
** Check that:
**
**   1) All leaves of pSeg between iFirst and iLast (inclusive) exist and
**      contain zero terms.
**   2) All leaves of pSeg between iNoRowid and iLast (inclusive) exist and
**      contain zero rowids.
*/
static void fts5IndexIntegrityCheckEmpty(
  Fts5Index *p,
  Fts5StructureSegment *pSeg,     /* Segment to check internal consistency */
  int iFirst,
  int iNoRowid,
  int iLast
){
  int i;

  /* Now check that the iter.nEmpty leaves following the current leaf
  ** (a) exist and (b) contain no terms. */
  for(i=iFirst; p->rc==SQLITE_OK && i<=iLast; i++){
    Fts5Data *pLeaf = fts5DataRead(p, FTS5_SEGMENT_ROWID(pSeg->iSegid, i));
    if( pLeaf ){
      if( !fts5LeafIsTermless(pLeaf) ) p->rc = FTS5_CORRUPT;
      if( i>=iNoRowid && 0!=fts5LeafFirstRowidOff(pLeaf) ) p->rc = FTS5_CORRUPT;
    }
    fts5DataRelease(pLeaf);
  }
}

static void fts5IntegrityCheckPgidx(Fts5Index *p, Fts5Data *pLeaf){
  i64 iTermOff = 0;
  int ii;

  Fts5Buffer buf1 = {0,0,0};
  Fts5Buffer buf2 = {0,0,0};

  ii = pLeaf->szLeaf;
  while( ii<pLeaf->nn && p->rc==SQLITE_OK ){
    int res;
    i64 iOff;
    int nIncr;

    ii += fts5GetVarint32(&pLeaf->p[ii], nIncr);
    iTermOff += nIncr;
    iOff = iTermOff;

    if( iOff>=pLeaf->szLeaf ){
      p->rc = FTS5_CORRUPT;
    }else if( iTermOff==nIncr ){
      int nByte;
      iOff += fts5GetVarint32(&pLeaf->p[iOff], nByte);
      if( (iOff+nByte)>pLeaf->szLeaf ){
        p->rc = FTS5_CORRUPT;
      }else{
        fts5BufferSet(&p->rc, &buf1, nByte, &pLeaf->p[iOff]);
      }
    }else{
      int nKeep, nByte;
      iOff += fts5GetVarint32(&pLeaf->p[iOff], nKeep);
      iOff += fts5GetVarint32(&pLeaf->p[iOff], nByte);
      if( nKeep>buf1.n || (iOff+nByte)>pLeaf->szLeaf ){
        p->rc = FTS5_CORRUPT;
      }else{
        buf1.n = nKeep;
        fts5BufferAppendBlob(&p->rc, &buf1, nByte, &pLeaf->p[iOff]);
      }

      if( p->rc==SQLITE_OK ){
        res = fts5BufferCompare(&buf1, &buf2);
        if( res<=0 ) p->rc = FTS5_CORRUPT;
      }
    }
    fts5BufferSet(&p->rc, &buf2, buf1.n, buf1.p);
  }

  fts5BufferFree(&buf1);
  fts5BufferFree(&buf2);
}

static void fts5IndexIntegrityCheckSegment(
  Fts5Index *p,                   /* FTS5 backend object */
  Fts5StructureSegment *pSeg      /* Segment to check internal consistency */
){
  Fts5Config *pConfig = p->pConfig;
  int bSecureDelete = (pConfig->iVersion==FTS5_CURRENT_VERSION_SECUREDELETE);
  sqlite3_stmt *pStmt = 0;
  int rc2;
  int iIdxPrevLeaf = pSeg->pgnoFirst-1;
  int iDlidxPrevLeaf = pSeg->pgnoLast;

  if( pSeg->pgnoFirst==0 ) return;

  fts5IndexPrepareStmt(p, &pStmt, sqlite3_mprintf(
      "SELECT segid, term, (pgno>>1), (pgno&1) FROM %Q.'%q_idx' WHERE segid=%d "
      "ORDER BY 1, 2",
      pConfig->zDb, pConfig->zName, pSeg->iSegid
  ));

  /* Iterate through the b-tree hierarchy.  */
  while( p->rc==SQLITE_OK && SQLITE_ROW==sqlite3_step(pStmt) ){
    i64 iRow;                     /* Rowid for this leaf */
    Fts5Data *pLeaf;              /* Data for this leaf */

    const char *zIdxTerm = (const char*)sqlite3_column_blob(pStmt, 1);
    int nIdxTerm = sqlite3_column_bytes(pStmt, 1);
    int iIdxLeaf = sqlite3_column_int(pStmt, 2);
    int bIdxDlidx = sqlite3_column_int(pStmt, 3);

    /* If the leaf in question has already been trimmed from the segment,
    ** ignore this b-tree entry. Otherwise, load it into memory. */
    if( iIdxLeaf<pSeg->pgnoFirst ) continue;
    iRow = FTS5_SEGMENT_ROWID(pSeg->iSegid, iIdxLeaf);
    pLeaf = fts5LeafRead(p, iRow);
    if( pLeaf==0 ) break;

    /* Check that the leaf contains at least one term, and that it is equal
    ** to or larger than the split-key in zIdxTerm.  Also check that if there
    ** is also a rowid pointer within the leaf page header, it points to a
    ** location before the term.  */
    if( pLeaf->nn<=pLeaf->szLeaf ){

      if( nIdxTerm==0
       && pConfig->iVersion==FTS5_CURRENT_VERSION_SECUREDELETE
       && pLeaf->nn==pLeaf->szLeaf
       && pLeaf->nn==4
      ){
        /* special case - the very first page in a segment keeps its %_idx
        ** entry even if all the terms are removed from it by secure-delete
        ** operations. */
      }else{
        p->rc = FTS5_CORRUPT;
      }

    }else{
      int iOff;                   /* Offset of first term on leaf */
      int iRowidOff;              /* Offset of first rowid on leaf */
      int nTerm;                  /* Size of term on leaf in bytes */
      int res;                    /* Comparison of term and split-key */

      iOff = fts5LeafFirstTermOff(pLeaf);
      iRowidOff = fts5LeafFirstRowidOff(pLeaf);
      if( iRowidOff>=iOff || iOff>=pLeaf->szLeaf ){
        p->rc = FTS5_CORRUPT;
      }else{
        iOff += fts5GetVarint32(&pLeaf->p[iOff], nTerm);
        res = fts5Memcmp(&pLeaf->p[iOff], zIdxTerm, MIN(nTerm, nIdxTerm));
        if( res==0 ) res = nTerm - nIdxTerm;
        if( res<0 ) p->rc = FTS5_CORRUPT;
      }

      fts5IntegrityCheckPgidx(p, pLeaf);
    }
    fts5DataRelease(pLeaf);
    if( p->rc ) break;

    /* Now check that the iter.nEmpty leaves following the current leaf
    ** (a) exist and (b) contain no terms. */
    fts5IndexIntegrityCheckEmpty(
        p, pSeg, iIdxPrevLeaf+1, iDlidxPrevLeaf+1, iIdxLeaf-1
    );
    if( p->rc ) break;

    /* If there is a doclist-index, check that it looks right. */
    if( bIdxDlidx ){
      Fts5DlidxIter *pDlidx = 0;  /* For iterating through doclist index */
      int iPrevLeaf = iIdxLeaf;
      int iSegid = pSeg->iSegid;
      int iPg = 0;
      i64 iKey;

      for(pDlidx=fts5DlidxIterInit(p, 0, iSegid, iIdxLeaf);
          fts5DlidxIterEof(p, pDlidx)==0;
          fts5DlidxIterNext(p, pDlidx)
      ){

        /* Check any rowid-less pages that occur before the current leaf. */
        for(iPg=iPrevLeaf+1; iPg<fts5DlidxIterPgno(pDlidx); iPg++){
          iKey = FTS5_SEGMENT_ROWID(iSegid, iPg);
          pLeaf = fts5DataRead(p, iKey);
          if( pLeaf ){
            if( fts5LeafFirstRowidOff(pLeaf)!=0 ) p->rc = FTS5_CORRUPT;
            fts5DataRelease(pLeaf);
          }
        }
        iPrevLeaf = fts5DlidxIterPgno(pDlidx);

        /* Check that the leaf page indicated by the iterator really does
        ** contain the rowid suggested by the same. */
        iKey = FTS5_SEGMENT_ROWID(iSegid, iPrevLeaf);
        pLeaf = fts5DataRead(p, iKey);
        if( pLeaf ){
          i64 iRowid;
          int iRowidOff = fts5LeafFirstRowidOff(pLeaf);
          ASSERT_SZLEAF_OK(pLeaf);
          if( iRowidOff>=pLeaf->szLeaf ){
            p->rc = FTS5_CORRUPT;
          }else if( bSecureDelete==0 || iRowidOff>0 ){
            i64 iDlRowid = fts5DlidxIterRowid(pDlidx);
            fts5GetVarint(&pLeaf->p[iRowidOff], (u64*)&iRowid);
            if( iRowid<iDlRowid || (bSecureDelete==0 && iRowid!=iDlRowid) ){
              p->rc = FTS5_CORRUPT;
            }
          }
          fts5DataRelease(pLeaf);
        }
      }

      iDlidxPrevLeaf = iPg;
      fts5DlidxIterFree(pDlidx);
      fts5TestDlidxReverse(p, iSegid, iIdxLeaf);
    }else{
      iDlidxPrevLeaf = pSeg->pgnoLast;
      /* TODO: Check there is no doclist index */
    }

    iIdxPrevLeaf = iIdxLeaf;
  }

  rc2 = sqlite3_finalize(pStmt);
  if( p->rc==SQLITE_OK ) p->rc = rc2;

  /* Page iter.iLeaf must now be the rightmost leaf-page in the segment */
#if 0
  if( p->rc==SQLITE_OK && iter.iLeaf!=pSeg->pgnoLast ){
    p->rc = FTS5_CORRUPT;
  }
#endif
}
