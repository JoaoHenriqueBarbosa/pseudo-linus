
/*
** This is called as part of flushing a delete to disk in 'secure-delete'
** mode. It edits the segments within the database described by argument
** pStruct to remove the entries for term zTerm, rowid iRowid.
*/
static void fts5FlushSecureDelete(
  Fts5Index *p,
  Fts5Structure *pStruct,
  const char *zTerm,
  int nTerm,
  i64 iRowid
){
  const int f = FTS5INDEX_QUERY_SKIPHASH;
  Fts5Iter *pIter = 0;            /* Used to find term instance */

  fts5MultiIterNew(p, pStruct, f, 0, (const u8*)zTerm, nTerm, -1, 0, &pIter);
  if( fts5MultiIterEof(p, pIter)==0 ){
    i64 iThis = fts5MultiIterRowid(pIter);
    if( iThis<iRowid ){
      fts5MultiIterNextFrom(p, pIter, iRowid);
    }

    if( p->rc==SQLITE_OK
     && fts5MultiIterEof(p, pIter)==0
     && iRowid==fts5MultiIterRowid(pIter)
    ){
      Fts5SegIter *pSeg = &pIter->aSeg[pIter->aFirst[1].iFirst];
      fts5DoSecureDelete(p, pSeg);
    }
  }

  fts5MultiIterFree(pIter);
}


/*
** Flush the contents of in-memory hash table iHash to a new level-0
** segment on disk. Also update the corresponding structure record.
**
** If an error occurs, set the Fts5Index.rc error code. If an error has
** already occurred, this function is a no-op.
*/
static void fts5FlushOneHash(Fts5Index *p){
  Fts5Hash *pHash = p->pHash;
  Fts5Structure *pStruct;
  int iSegid;
  int pgnoLast = 0;                 /* Last leaf page number in segment */

  /* Obtain a reference to the index structure and allocate a new segment-id
  ** for the new level-0 segment.  */
  pStruct = fts5StructureRead(p);
  fts5StructureInvalidate(p);

  if( sqlite3Fts5HashIsEmpty(pHash)==0 ){
    iSegid = fts5AllocateSegid(p, pStruct);
    if( iSegid ){
      const int pgsz = p->pConfig->pgsz;
      int eDetail = p->pConfig->eDetail;
      int bSecureDelete = p->pConfig->bSecureDelete;
      Fts5StructureSegment *pSeg; /* New segment within pStruct */
      Fts5Buffer *pBuf;           /* Buffer in which to assemble leaf page */
      Fts5Buffer *pPgidx;         /* Buffer in which to assemble pgidx */

      Fts5SegWriter writer;
      fts5WriteInit(p, &writer, iSegid);

      pBuf = &writer.writer.buf;
      pPgidx = &writer.writer.pgidx;

      /* fts5WriteInit() should have initialized the buffers to (most likely)
      ** the maximum space required. */
      assert( p->rc || pBuf->nSpace>=(pgsz + FTS5_DATA_PADDING) );
      assert( p->rc || pPgidx->nSpace>=(pgsz + FTS5_DATA_PADDING) );

      /* Begin scanning through hash table entries. This loop runs once for each
      ** term/doclist currently stored within the hash table. */
      if( p->rc==SQLITE_OK ){
        p->rc = sqlite3Fts5HashScanInit(pHash, 0, 0);
      }
      while( p->rc==SQLITE_OK && 0==sqlite3Fts5HashScanEof(pHash) ){
        const char *zTerm;        /* Buffer containing term */
        int nTerm;                /* Size of zTerm in bytes */
        const u8 *pDoclist;       /* Pointer to doclist for this term */
        int nDoclist;             /* Size of doclist in bytes */

        /* Get the term and doclist for this entry. */
        sqlite3Fts5HashScanEntry(pHash, &zTerm, &nTerm, &pDoclist, &nDoclist);
        if( bSecureDelete==0 ){
          fts5WriteAppendTerm(p, &writer, nTerm, (const u8*)zTerm);
          if( p->rc!=SQLITE_OK ) break;
          assert( writer.bFirstRowidInPage==0 );
        }

        if( !bSecureDelete && pgsz>=(pBuf->n + pPgidx->n + nDoclist + 1) ){
          /* The entire doclist will fit on the current leaf. */
          fts5BufferSafeAppendBlob(pBuf, pDoclist, nDoclist);
        }else{
          int bTermWritten = !bSecureDelete;
          i64 iRowid = 0;
          i64 iPrev = 0;
          int iOff = 0;

          /* The entire doclist will not fit on this leaf. The following
          ** loop iterates through the poslists that make up the current
          ** doclist.  */
          while( p->rc==SQLITE_OK && iOff<nDoclist ){
            u64 iDelta = 0;
            iOff += fts5GetVarint(&pDoclist[iOff], &iDelta);
            iRowid += iDelta;

            /* If in secure delete mode, and if this entry in the poslist is
            ** in fact a delete, then edit the existing segments directly
            ** using fts5FlushSecureDelete().  */
            if( bSecureDelete ){
              if( eDetail==FTS5_DETAIL_NONE ){
                if( iOff<nDoclist && pDoclist[iOff]==0x00 ){
                  fts5FlushSecureDelete(p, pStruct, zTerm, nTerm, iRowid);
                  iOff++;
                  if( iOff<nDoclist && pDoclist[iOff]==0x00 ){
                    iOff++;
                    nDoclist = 0;
                  }else{
                    continue;
                  }
                }
              }else if( (pDoclist[iOff] & 0x01) ){
                fts5FlushSecureDelete(p, pStruct, zTerm, nTerm, iRowid);
                if( p->rc!=SQLITE_OK || pDoclist[iOff]==0x01 ){
                  iOff++;
                  continue;
                }
              }
            }

            if( p->rc==SQLITE_OK && bTermWritten==0 ){
              fts5WriteAppendTerm(p, &writer, nTerm, (const u8*)zTerm);
              bTermWritten = 1;
              assert( p->rc!=SQLITE_OK || writer.bFirstRowidInPage==0 );
            }

            if( writer.bFirstRowidInPage ){
              fts5PutU16(&pBuf->p[0], (u16)pBuf->n);   /* first rowid on page */
              pBuf->n += sqlite3Fts5PutVarint(&pBuf->p[pBuf->n], iRowid);
              writer.bFirstRowidInPage = 0;
              fts5WriteDlidxAppend(p, &writer, iRowid);
            }else{
              u64 iRowidDelta = (u64)iRowid - (u64)iPrev;
              pBuf->n += sqlite3Fts5PutVarint(&pBuf->p[pBuf->n], iRowidDelta);
            }
            if( p->rc!=SQLITE_OK ) break;
            assert( pBuf->n<=pBuf->nSpace );
            iPrev = iRowid;

            if( eDetail==FTS5_DETAIL_NONE ){
              if( iOff<nDoclist && pDoclist[iOff]==0 ){
                pBuf->p[pBuf->n++] = 0;
                iOff++;
                if( iOff<nDoclist && pDoclist[iOff]==0 ){
                  pBuf->p[pBuf->n++] = 0;
                  iOff++;
                }
              }
              if( (pBuf->n + pPgidx->n)>=pgsz ){
                fts5WriteFlushLeaf(p, &writer);
              }
            }else{
              int bDel = 0;
              int nPos = 0;
              int nCopy = fts5GetPoslistSize(&pDoclist[iOff], &nPos, &bDel);
              if( bDel && bSecureDelete ){
                fts5BufferAppendVarint(&p->rc, pBuf, nPos*2);
                iOff += nCopy;
                nCopy = nPos;
              }else{
                nCopy += nPos;
              }
              if( (pBuf->n + pPgidx->n + nCopy) <= pgsz ){
                /* The entire poslist will fit on the current leaf. So copy
                ** it in one go. */
                fts5BufferSafeAppendBlob(pBuf, &pDoclist[iOff], nCopy);
              }else{
                /* The entire poslist will not fit on this leaf. So it needs
                ** to be broken into sections. The only qualification being
                ** that each varint must be stored contiguously.  */
                const u8 *pPoslist = &pDoclist[iOff];
                int iPos = 0;
                while( p->rc==SQLITE_OK ){
                  int nSpace = pgsz - pBuf->n - pPgidx->n;
                  int n = 0;
                  if( (nCopy - iPos)<=nSpace ){
                    n = nCopy - iPos;
                  }else{
                    n = fts5PoslistPrefix(&pPoslist[iPos], nSpace);
                  }
                  assert( n>0 );
                  fts5BufferSafeAppendBlob(pBuf, &pPoslist[iPos], n);
                  iPos += n;
                  if( (pBuf->n + pPgidx->n)>=pgsz ){
                    fts5WriteFlushLeaf(p, &writer);
                  }
                  if( iPos>=nCopy ) break;
                }
              }
              iOff += nCopy;
            }
          }
        }

        /* TODO2: Doclist terminator written here. */
        /* pBuf->p[pBuf->n++] = '\0'; */
        assert( pBuf->n<=pBuf->nSpace );
        if( p->rc==SQLITE_OK ) sqlite3Fts5HashScanNext(pHash);
      }
      fts5WriteFinish(p, &writer, &pgnoLast);

      assert( p->rc!=SQLITE_OK || bSecureDelete || pgnoLast>0 );
      if( pgnoLast>0 ){
        /* Update the Fts5Structure. It is written back to the database by the
        ** fts5StructureRelease() call below.  */
        if( pStruct->nLevel==0 ){
          fts5StructureAddLevel(&p->rc, &pStruct);
        }
        fts5StructureExtendLevel(&p->rc, pStruct, 0, 1, 0);
        if( p->rc==SQLITE_OK ){
          pSeg = &pStruct->aLevel[0].aSeg[ pStruct->aLevel[0].nSeg++ ];
          pSeg->iSegid = iSegid;
          pSeg->pgnoFirst = 1;
          pSeg->pgnoLast = pgnoLast;
          if( pStruct->nOriginCntr>0 ){
            pSeg->iOrigin1 = pStruct->nOriginCntr;
            pSeg->iOrigin2 = pStruct->nOriginCntr;
            pSeg->nEntry = p->nPendingRow;
            pStruct->nOriginCntr++;
          }
          pStruct->nSegment++;
        }
        fts5StructurePromote(p, 0, pStruct);
      }
    }
  }

  fts5IndexAutomerge(p, &pStruct, pgnoLast + p->nContentlessDelete);
  fts5IndexCrisismerge(p, &pStruct);
  fts5StructureWrite(p, pStruct);
  fts5StructureRelease(pStruct);
}

/*
** Flush any data stored in the in-memory hash tables to the database.
*/
static void fts5IndexFlush(Fts5Index *p){
  /* Unless it is empty, flush the hash table to disk */
  if( p->flushRc ){
    p->rc = p->flushRc;
    return;
  }
  if( p->nPendingData || p->nContentlessDelete ){
    assert( p->pHash );
    fts5FlushOneHash(p);
    if( p->rc==SQLITE_OK ){
      sqlite3Fts5HashClear(p->pHash);
      p->nPendingData = 0;
      p->nPendingRow = 0;
      p->nContentlessDelete = 0;
    }else if( p->nPendingData || p->nContentlessDelete ){
      p->flushRc = p->rc;
    }
  }
}

static Fts5Structure *fts5IndexOptimizeStruct(
  Fts5Index *p,
  Fts5Structure *pStruct
){
  Fts5Structure *pNew = 0;
  sqlite3_int64 nByte = sizeof(Fts5Structure);
  int nSeg = pStruct->nSegment;
  int i;

  /* Figure out if this structure requires optimization. A structure does
  ** not require optimization if either:
  **
  **  1. it consists of fewer than two segments, or
  **  2. all segments are on the same level, or
  **  3. all segments except one are currently inputs to a merge operation.
  **
  ** In the first case, if there are no tombstone hash pages, return NULL. In
  ** the second, increment the ref-count on *pStruct and return a copy of the
  ** pointer to it.
  */
  if( nSeg==0 ) return 0;
  for(i=0; i<pStruct->nLevel; i++){
    int nThis = pStruct->aLevel[i].nSeg;
    int nMerge = pStruct->aLevel[i].nMerge;
    if( nThis>0 && (nThis==nSeg || (nThis==nSeg-1 && nMerge==nThis)) ){
      if( nSeg==1 && nThis==1 && pStruct->aLevel[i].aSeg[0].nPgTombstone==0 ){
        return 0;
      }
      fts5StructureRef(pStruct);
      return pStruct;
    }
    assert( pStruct->aLevel[i].nMerge<=nThis );
  }

  nByte += (pStruct->nLevel+1) * sizeof(Fts5StructureLevel);
  pNew = (Fts5Structure*)sqlite3Fts5MallocZero(&p->rc, nByte);

  if( pNew ){
    Fts5StructureLevel *pLvl;
    nByte = nSeg * sizeof(Fts5StructureSegment);
    pNew->nLevel = MIN(pStruct->nLevel+1, FTS5_MAX_LEVEL);
    pNew->nRef = 1;
    pNew->nWriteCounter = pStruct->nWriteCounter;
    pNew->nOriginCntr = pStruct->nOriginCntr;
    pLvl = &pNew->aLevel[pNew->nLevel-1];
    pLvl->aSeg = (Fts5StructureSegment*)sqlite3Fts5MallocZero(&p->rc, nByte);
    if( pLvl->aSeg ){
      int iLvl, iSeg;
      int iSegOut = 0;
      /* Iterate through all segments, from oldest to newest. Add them to
      ** the new Fts5Level object so that pLvl->aSeg[0] is the oldest
      ** segment in the data structure.  */
      for(iLvl=pStruct->nLevel-1; iLvl>=0; iLvl--){
        for(iSeg=0; iSeg<pStruct->aLevel[iLvl].nSeg; iSeg++){
          pLvl->aSeg[iSegOut] = pStruct->aLevel[iLvl].aSeg[iSeg];
          iSegOut++;
        }
      }
      pNew->nSegment = pLvl->nSeg = nSeg;
    }else{
      sqlite3_free(pNew);
      pNew = 0;
    }
  }

  return pNew;
}

static int sqlite3Fts5IndexOptimize(Fts5Index *p){
  Fts5Structure *pStruct;
  Fts5Structure *pNew = 0;

  assert( p->rc==SQLITE_OK );
  fts5IndexFlush(p);
  assert( p->rc!=SQLITE_OK || p->nContentlessDelete==0 );
  pStruct = fts5StructureRead(p);
  assert( p->rc!=SQLITE_OK || pStruct!=0 );
  fts5StructureInvalidate(p);

  if( pStruct ){
    pNew = fts5IndexOptimizeStruct(p, pStruct);
  }
  fts5StructureRelease(pStruct);

  assert( pNew==0 || pNew->nSegment>0 );
  if( pNew ){
    int iLvl;
    for(iLvl=0; pNew->aLevel[iLvl].nSeg==0; iLvl++){}
    while( p->rc==SQLITE_OK && pNew->aLevel[iLvl].nSeg>0 ){
      int nRem = FTS5_OPT_WORK_UNIT;
      fts5IndexMergeLevel(p, &pNew, iLvl, &nRem);
    }

    fts5StructureWrite(p, pNew);
    fts5StructureRelease(pNew);
  }

  return fts5IndexReturn(p);
}
