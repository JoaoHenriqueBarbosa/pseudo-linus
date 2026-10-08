
static void fts5SetupPrefixIter(
  Fts5Index *p,                   /* Index to read from */
  int bDesc,                      /* True for "ORDER BY rowid DESC" */
  int iIdx,                       /* Index to scan for data */
  u8 *pToken,                     /* Buffer containing prefix to match */
  int nToken,                     /* Size of buffer pToken in bytes */
  Fts5Colset *pColset,            /* Restrict matches to these columns */
  Fts5Iter **ppIter               /* OUT: New iterator */
){
  Fts5Structure *pStruct;
  Fts5Buffer *aBuf;
  int nBuf = 32;
  int nMerge = 1;

  void (*xMerge)(Fts5Index*, Fts5Buffer*, int, Fts5Buffer*);
  void (*xAppend)(Fts5Index*, u64, Fts5Iter*, Fts5Buffer*);
  if( p->pConfig->eDetail==FTS5_DETAIL_NONE ){
    xMerge = fts5MergeRowidLists;
    xAppend = fts5AppendRowid;
  }else{
    nMerge = FTS5_MERGE_NLIST-1;
    nBuf = nMerge*8;   /* Sufficient to merge (16^8)==(2^32) lists */
    xMerge = fts5MergePrefixLists;
    xAppend = fts5AppendPoslist;
  }

  aBuf = (Fts5Buffer*)fts5IdxMalloc(p, sizeof(Fts5Buffer)*nBuf);
  pStruct = fts5StructureRead(p);
  assert( p->rc!=SQLITE_OK || (aBuf && pStruct) );

  if( p->rc==SQLITE_OK ){
    const int flags = FTS5INDEX_QUERY_SCAN
                    | FTS5INDEX_QUERY_SKIPEMPTY
                    | FTS5INDEX_QUERY_NOOUTPUT;
    int i;
    i64 iLastRowid = 0;
    Fts5Iter *p1 = 0;     /* Iterator used to gather data from index */
    Fts5Data *pData;
    Fts5Buffer doclist;
    int bNewTerm = 1;

    memset(&doclist, 0, sizeof(doclist));

    /* If iIdx is non-zero, then it is the number of a prefix-index for
    ** prefixes 1 character longer than the prefix being queried for. That
    ** index contains all the doclists required, except for the one
    ** corresponding to the prefix itself. That one is extracted from the
    ** main term index here.  */
    if( iIdx!=0 ){
      int dummy = 0;
      const int f2 = FTS5INDEX_QUERY_SKIPEMPTY|FTS5INDEX_QUERY_NOOUTPUT;
      pToken[0] = FTS5_MAIN_PREFIX;
      fts5MultiIterNew(p, pStruct, f2, pColset, pToken, nToken, -1, 0, &p1);
      fts5IterSetOutputCb(&p->rc, p1);
      for(;
        fts5MultiIterEof(p, p1)==0;
        fts5MultiIterNext2(p, p1, &dummy)
      ){
        Fts5SegIter *pSeg = &p1->aSeg[ p1->aFirst[1].iFirst ];
        p1->xSetOutputs(p1, pSeg);
        if( p1->base.nData ){
          xAppend(p, (u64)p1->base.iRowid-(u64)iLastRowid, p1, &doclist);
          iLastRowid = p1->base.iRowid;
        }
      }
      fts5MultiIterFree(p1);
    }

    pToken[0] = FTS5_MAIN_PREFIX + iIdx;
    fts5MultiIterNew(p, pStruct, flags, pColset, pToken, nToken, -1, 0, &p1);
    fts5IterSetOutputCb(&p->rc, p1);

    for( /* no-op */ ;
        fts5MultiIterEof(p, p1)==0;
        fts5MultiIterNext2(p, p1, &bNewTerm)
    ){
      Fts5SegIter *pSeg = &p1->aSeg[ p1->aFirst[1].iFirst ];
      int nTerm = pSeg->term.n;
      const u8 *pTerm = pSeg->term.p;
      p1->xSetOutputs(p1, pSeg);

      assert_nc( memcmp(pToken, pTerm, MIN(nToken, nTerm))<=0 );
      if( bNewTerm ){
        if( nTerm<nToken || memcmp(pToken, pTerm, nToken) ) break;
      }

      if( p1->base.nData==0 ) continue;
      if( p1->base.iRowid<=iLastRowid && doclist.n>0 ){
        for(i=0; p->rc==SQLITE_OK && doclist.n; i++){
          int i1 = i*nMerge;
          int iStore;
          assert( i1+nMerge<=nBuf );
          for(iStore=i1; iStore<i1+nMerge; iStore++){
            if( aBuf[iStore].n==0 ){
              fts5BufferSwap(&doclist, &aBuf[iStore]);
              fts5BufferZero(&doclist);
              break;
            }
          }
          if( iStore==i1+nMerge ){
            xMerge(p, &doclist, nMerge, &aBuf[i1]);
            for(iStore=i1; iStore<i1+nMerge; iStore++){
              fts5BufferZero(&aBuf[iStore]);
            }
          }
        }
        iLastRowid = 0;
      }

      xAppend(p, (u64)p1->base.iRowid-(u64)iLastRowid, p1, &doclist);
      iLastRowid = p1->base.iRowid;
    }

    assert( (nBuf%nMerge)==0 );
    for(i=0; i<nBuf; i+=nMerge){
      int iFree;
      if( p->rc==SQLITE_OK ){
        xMerge(p, &doclist, nMerge, &aBuf[i]);
      }
      for(iFree=i; iFree<i+nMerge; iFree++){
        fts5BufferFree(&aBuf[iFree]);
      }
    }
    fts5MultiIterFree(p1);

    pData = fts5IdxMalloc(p, sizeof(*pData)+doclist.n+FTS5_DATA_ZERO_PADDING);
    if( pData ){
      pData->p = (u8*)&pData[1];
      pData->nn = pData->szLeaf = doclist.n;
      if( doclist.n ) memcpy(pData->p, doclist.p, doclist.n);
      fts5MultiIterNew2(p, pData, bDesc, ppIter);
    }
    fts5BufferFree(&doclist);
  }

  fts5StructureRelease(pStruct);
  sqlite3_free(aBuf);
}


/*
** Indicate that all subsequent calls to sqlite3Fts5IndexWrite() pertain
** to the document with rowid iRowid.
*/
static int sqlite3Fts5IndexBeginWrite(Fts5Index *p, int bDelete, i64 iRowid){
  assert( p->rc==SQLITE_OK );

  /* Allocate the hash table if it has not already been allocated */
  if( p->pHash==0 ){
    p->rc = sqlite3Fts5HashNew(p->pConfig, &p->pHash, &p->nPendingData);
  }

  /* Flush the hash table to disk if required */
  if( iRowid<p->iWriteRowid
   || (iRowid==p->iWriteRowid && p->bDelete==0)
   || (p->nPendingData > p->pConfig->nHashSize)
  ){
    fts5IndexFlush(p);
  }

  p->iWriteRowid = iRowid;
  p->bDelete = bDelete;
  if( bDelete==0 ){
    p->nPendingRow++;
  }
  return fts5IndexReturn(p);
}

/*
** Commit data to disk.
*/
static int sqlite3Fts5IndexSync(Fts5Index *p){
  assert( p->rc==SQLITE_OK );
  fts5IndexFlush(p);
  sqlite3Fts5IndexCloseReader(p);
  return fts5IndexReturn(p);
}

/*
** Discard any data stored in the in-memory hash tables. Do not write it
** to the database. Additionally, assume that the contents of the %_data
** table may have changed on disk. So any in-memory caches of %_data
** records must be invalidated.
*/
static int sqlite3Fts5IndexRollback(Fts5Index *p){
  sqlite3Fts5IndexCloseReader(p);
  fts5IndexDiscardData(p);
  fts5StructureInvalidate(p);
  /* assert( p->rc==SQLITE_OK ); */
  return SQLITE_OK;
}

/*
** The %_data table is completely empty when this function is called. This
** function populates it with the initial structure objects for each index,
** and the initial version of the "averages" record (a zero-byte blob).
*/
static int sqlite3Fts5IndexReinit(Fts5Index *p){
  Fts5Structure s;
  fts5StructureInvalidate(p);
  fts5IndexDiscardData(p);
  memset(&s, 0, sizeof(Fts5Structure));
  if( p->pConfig->bContentlessDelete ){
    s.nOriginCntr = 1;
  }
  fts5DataWrite(p, FTS5_AVERAGES_ROWID, (const u8*)"", 0);
  fts5StructureWrite(p, &s);
  return fts5IndexReturn(p);
}

/*
** Open a new Fts5Index handle. If the bCreate argument is true, create
** and initialize the underlying %_data table.
**
** If successful, set *pp to point to the new object and return SQLITE_OK.
** Otherwise, set *pp to NULL and return an SQLite error code.
*/
static int sqlite3Fts5IndexOpen(
  Fts5Config *pConfig,
  int bCreate,
  Fts5Index **pp,
  char **pzErr
){
  int rc = SQLITE_OK;
  Fts5Index *p;                   /* New object */

  *pp = p = (Fts5Index*)sqlite3Fts5MallocZero(&rc, sizeof(Fts5Index));
  if( rc==SQLITE_OK ){
    p->pConfig = pConfig;
    p->nWorkUnit = FTS5_WORK_UNIT;
    p->zDataTbl = sqlite3Fts5Mprintf(&rc, "%s_data", pConfig->zName);
    if( p->zDataTbl && bCreate ){
      rc = sqlite3Fts5CreateTable(
          pConfig, "data", "id INTEGER PRIMARY KEY, block BLOB", 0, pzErr
      );
      if( rc==SQLITE_OK ){
        rc = sqlite3Fts5CreateTable(pConfig, "idx",
            "segid, term, pgno, PRIMARY KEY(segid, term)",
            1, pzErr
        );
      }
      if( rc==SQLITE_OK ){
        rc = sqlite3Fts5IndexReinit(p);
      }
    }
  }

  assert( rc!=SQLITE_OK || p->rc==SQLITE_OK );
  if( rc ){
    sqlite3Fts5IndexClose(p);
    *pp = 0;
  }
  return rc;
}

/*
** Close a handle opened by an earlier call to sqlite3Fts5IndexOpen().
*/
static int sqlite3Fts5IndexClose(Fts5Index *p){
  int rc = SQLITE_OK;
  if( p ){
    assert( p->pReader==0 );
    fts5StructureInvalidate(p);
    sqlite3_finalize(p->pWriter);
    sqlite3_finalize(p->pDeleter);
    sqlite3_finalize(p->pIdxWriter);
    sqlite3_finalize(p->pIdxDeleter);
    sqlite3_finalize(p->pIdxSelect);
    sqlite3_finalize(p->pIdxNextSelect);
    sqlite3_finalize(p->pDataVersion);
    sqlite3_finalize(p->pDeleteFromIdx);
    sqlite3Fts5HashFree(p->pHash);
    sqlite3_free(p->zDataTbl);
    sqlite3_free(p);
  }
  return rc;
}

/*
** Argument p points to a buffer containing utf-8 text that is n bytes in
** size. Return the number of bytes in the nChar character prefix of the
** buffer, or 0 if there are less than nChar characters in total.
*/
static int sqlite3Fts5IndexCharlenToBytelen(
  const char *p,
  int nByte,
  int nChar
){
  int n = 0;
  int i;
  for(i=0; i<nChar; i++){
    if( n>=nByte ) return 0;      /* Input contains fewer than nChar chars */
    if( (unsigned char)p[n++]>=0xc0 ){
      if( n>=nByte ) return 0;
      while( (p[n] & 0xc0)==0x80 ){
        n++;
        if( n>=nByte ){
          if( i+1==nChar ) break;
          return 0;
        }
      }
    }
  }
  return n;
}

/*
** pIn is a UTF-8 encoded string, nIn bytes in size. Return the number of
** unicode characters in the string.
*/
static int fts5IndexCharlen(const char *pIn, int nIn){
  int nChar = 0;
  int i = 0;
  while( i<nIn ){
    if( (unsigned char)pIn[i++]>=0xc0 ){
      while( i<nIn && (pIn[i] & 0xc0)==0x80 ) i++;
    }
    nChar++;
  }
  return nChar;
}

/*
** Insert or remove data to or from the index. Each time a document is
** added to or removed from the index, this function is called one or more
** times.
**
** For an insert, it must be called once for each token in the new document.
** If the operation is a delete, it must be called (at least) once for each
** unique token in the document with an iCol value less than zero. The iPos
** argument is ignored for a delete.
*/
static int sqlite3Fts5IndexWrite(
  Fts5Index *p,                   /* Index to write to */
  int iCol,                       /* Column token appears in (-ve -> delete) */
  int iPos,                       /* Position of token within column */
  const char *pToken, int nToken  /* Token to add or remove to or from index */
){
  int i;                          /* Used to iterate through indexes */
  int rc = SQLITE_OK;             /* Return code */
  Fts5Config *pConfig = p->pConfig;

  assert( p->rc==SQLITE_OK );
  assert( (iCol<0)==p->bDelete );

  /* Add the entry to the main terms index. */
  rc = sqlite3Fts5HashWrite(
      p->pHash, p->iWriteRowid, iCol, iPos, FTS5_MAIN_PREFIX, pToken, nToken
  );

  for(i=0; i<pConfig->nPrefix && rc==SQLITE_OK; i++){
    const int nChar = pConfig->aPrefix[i];
    int nByte = sqlite3Fts5IndexCharlenToBytelen(pToken, nToken, nChar);
    if( nByte ){
      rc = sqlite3Fts5HashWrite(p->pHash,
          p->iWriteRowid, iCol, iPos, (char)(FTS5_MAIN_PREFIX+i+1), pToken,
          nByte
      );
    }
  }

  return rc;
}
