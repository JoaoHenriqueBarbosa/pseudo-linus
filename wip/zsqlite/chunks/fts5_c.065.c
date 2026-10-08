
static void fts5VocabResetCursor(Fts5VocabCursor *pCsr){
  pCsr->rowid = 0;
  sqlite3Fts5IterClose(pCsr->pIter);
  sqlite3Fts5StructureRelease(pCsr->pStruct);
  pCsr->pStruct = 0;
  pCsr->pIter = 0;
  sqlite3_free(pCsr->zLeTerm);
  pCsr->nLeTerm = -1;
  pCsr->zLeTerm = 0;
  pCsr->bEof = 0;
}

/*
** Close the cursor.  For additional information see the documentation
** on the xClose method of the virtual table interface.
*/
static int fts5VocabCloseMethod(sqlite3_vtab_cursor *pCursor){
  Fts5VocabCursor *pCsr = (Fts5VocabCursor*)pCursor;
  fts5VocabResetCursor(pCsr);
  sqlite3Fts5BufferFree(&pCsr->term);
  sqlite3_finalize(pCsr->pStmt);
  sqlite3_free(pCsr);
  return SQLITE_OK;
}

static int fts5VocabInstanceNewTerm(Fts5VocabCursor *pCsr){
  int rc = SQLITE_OK;

  if( sqlite3Fts5IterEof(pCsr->pIter) ){
    pCsr->bEof = 1;
  }else{
    const char *zTerm;
    int nTerm;
    zTerm = sqlite3Fts5IterTerm(pCsr->pIter, &nTerm);
    if( pCsr->nLeTerm>=0 ){
      int nCmp = MIN(nTerm, pCsr->nLeTerm);
      int bCmp = memcmp(pCsr->zLeTerm, zTerm, nCmp);
      if( bCmp<0 || (bCmp==0 && pCsr->nLeTerm<nTerm) ){
        pCsr->bEof = 1;
      }
    }

    sqlite3Fts5BufferSet(&rc, &pCsr->term, nTerm, (const u8*)zTerm);
  }
  return rc;
}

static int fts5VocabInstanceNext(Fts5VocabCursor *pCsr){
  int eDetail = pCsr->pFts5->pConfig->eDetail;
  int rc = SQLITE_OK;
  Fts5IndexIter *pIter = pCsr->pIter;
  i64 *pp = &pCsr->iInstPos;
  int *po = &pCsr->iInstOff;

  assert( sqlite3Fts5IterEof(pIter)==0 );
  assert( pCsr->bEof==0 );
  while( eDetail==FTS5_DETAIL_NONE
      || sqlite3Fts5PoslistNext64(pIter->pData, pIter->nData, po, pp)
  ){
    pCsr->iInstPos = 0;
    pCsr->iInstOff = 0;

    rc = sqlite3Fts5IterNextScan(pCsr->pIter);
    if( rc==SQLITE_OK ){
      rc = fts5VocabInstanceNewTerm(pCsr);
      if( pCsr->bEof || eDetail==FTS5_DETAIL_NONE ) break;
    }
    if( rc ){
      pCsr->bEof = 1;
      break;
    }
  }

  return rc;
}

/*
** Advance the cursor to the next row in the table.
*/
static int fts5VocabNextMethod(sqlite3_vtab_cursor *pCursor){
  Fts5VocabCursor *pCsr = (Fts5VocabCursor*)pCursor;
  Fts5VocabTable *pTab = (Fts5VocabTable*)pCursor->pVtab;
  int nCol = pCsr->pFts5->pConfig->nCol;
  int rc;

  rc = sqlite3Fts5StructureTest(pCsr->pFts5->pIndex, pCsr->pStruct);
  if( rc!=SQLITE_OK ) return rc;
  pCsr->rowid++;

  if( pTab->eType==FTS5_VOCAB_INSTANCE ){
    return fts5VocabInstanceNext(pCsr);
  }

  if( pTab->eType==FTS5_VOCAB_COL ){
    for(pCsr->iCol++; pCsr->iCol<nCol; pCsr->iCol++){
      if( pCsr->aDoc[pCsr->iCol] ) break;
    }
  }

  if( pTab->eType!=FTS5_VOCAB_COL || pCsr->iCol>=nCol ){
    if( sqlite3Fts5IterEof(pCsr->pIter) ){
      pCsr->bEof = 1;
    }else{
      const char *zTerm;
      int nTerm;

      zTerm = sqlite3Fts5IterTerm(pCsr->pIter, &nTerm);
      assert( nTerm>=0 );
      if( pCsr->nLeTerm>=0 ){
        int nCmp = MIN(nTerm, pCsr->nLeTerm);
        int bCmp = memcmp(pCsr->zLeTerm, zTerm, nCmp);
        if( bCmp<0 || (bCmp==0 && pCsr->nLeTerm<nTerm) ){
          pCsr->bEof = 1;
          return SQLITE_OK;
        }
      }

      sqlite3Fts5BufferSet(&rc, &pCsr->term, nTerm, (const u8*)zTerm);
      memset(pCsr->aCnt, 0, nCol * sizeof(i64));
      memset(pCsr->aDoc, 0, nCol * sizeof(i64));
      pCsr->iCol = 0;

      assert( pTab->eType==FTS5_VOCAB_COL || pTab->eType==FTS5_VOCAB_ROW );
      while( rc==SQLITE_OK ){
        int eDetail = pCsr->pFts5->pConfig->eDetail;
        const u8 *pPos; int nPos;   /* Position list */
        i64 iPos = 0;               /* 64-bit position read from poslist */
        int iOff = 0;               /* Current offset within position list */

        pPos = pCsr->pIter->pData;
        nPos = pCsr->pIter->nData;

        switch( pTab->eType ){
          case FTS5_VOCAB_ROW:
            if( eDetail==FTS5_DETAIL_FULL ){
              while( 0==sqlite3Fts5PoslistNext64(pPos, nPos, &iOff, &iPos) ){
                pCsr->aCnt[0]++;
              }
            }
            pCsr->aDoc[0]++;
            break;

          case FTS5_VOCAB_COL:
            if( eDetail==FTS5_DETAIL_FULL ){
              int iCol = -1;
              while( 0==sqlite3Fts5PoslistNext64(pPos, nPos, &iOff, &iPos) ){
                int ii = FTS5_POS2COLUMN(iPos);
                if( iCol!=ii ){
                  if( ii>=nCol ){
                    rc = FTS5_CORRUPT;
                    break;
                  }
                  pCsr->aDoc[ii]++;
                  iCol = ii;
                }
                pCsr->aCnt[ii]++;
              }
            }else if( eDetail==FTS5_DETAIL_COLUMNS ){
              while( 0==sqlite3Fts5PoslistNext64(pPos, nPos, &iOff,&iPos) ){
                assert_nc( iPos>=0 && iPos<nCol );
                if( iPos>=nCol ){
                  rc = FTS5_CORRUPT;
                  break;
                }
                pCsr->aDoc[iPos]++;
              }
            }else{
              assert( eDetail==FTS5_DETAIL_NONE );
              pCsr->aDoc[0]++;
            }
            break;

          default:
            assert( pTab->eType==FTS5_VOCAB_INSTANCE );
            break;
        }

        if( rc==SQLITE_OK ){
          rc = sqlite3Fts5IterNextScan(pCsr->pIter);
        }
        if( pTab->eType==FTS5_VOCAB_INSTANCE ) break;

        if( rc==SQLITE_OK ){
          zTerm = sqlite3Fts5IterTerm(pCsr->pIter, &nTerm);
          if( nTerm!=pCsr->term.n
          || (nTerm>0 && memcmp(zTerm, pCsr->term.p, nTerm))
          ){
            break;
          }
          if( sqlite3Fts5IterEof(pCsr->pIter) ) break;
        }
      }
    }
  }

  if( rc==SQLITE_OK && pCsr->bEof==0 && pTab->eType==FTS5_VOCAB_COL ){
    for(/* noop */; pCsr->iCol<nCol && pCsr->aDoc[pCsr->iCol]==0; pCsr->iCol++);
    if( pCsr->iCol==nCol ){
      rc = FTS5_CORRUPT;
    }
  }
  return rc;
}

/*
** This is the xFilter implementation for the virtual table.
*/
static int fts5VocabFilterMethod(
  sqlite3_vtab_cursor *pCursor,   /* The cursor used for this query */
  int idxNum,                     /* Strategy index */
  const char *zUnused,            /* Unused */
  int nUnused,                    /* Number of elements in apVal */
  sqlite3_value **apVal           /* Arguments for the indexing scheme */
){
  Fts5VocabTable *pTab = (Fts5VocabTable*)pCursor->pVtab;
  Fts5VocabCursor *pCsr = (Fts5VocabCursor*)pCursor;
  int eType = pTab->eType;
  int rc = SQLITE_OK;

  int iVal = 0;
  int f = FTS5INDEX_QUERY_SCAN;
  const char *zTerm = 0;
  int nTerm = 0;

  sqlite3_value *pEq = 0;
  sqlite3_value *pGe = 0;
  sqlite3_value *pLe = 0;

  UNUSED_PARAM2(zUnused, nUnused);

  fts5VocabResetCursor(pCsr);
  if( idxNum & FTS5_VOCAB_TERM_EQ ) pEq = apVal[iVal++];
  if( idxNum & FTS5_VOCAB_TERM_GE ) pGe = apVal[iVal++];
  if( idxNum & FTS5_VOCAB_TERM_LE ) pLe = apVal[iVal++];

  if( pEq ){
    zTerm = (const char *)sqlite3_value_text(pEq);
    nTerm = sqlite3_value_bytes(pEq);
    f = FTS5INDEX_QUERY_NOTOKENDATA;
  }else{
    if( pGe ){
      zTerm = (const char *)sqlite3_value_text(pGe);
      nTerm = sqlite3_value_bytes(pGe);
    }
    if( pLe ){
      const char *zCopy = (const char *)sqlite3_value_text(pLe);
      if( zCopy==0 ) zCopy = "";
      pCsr->nLeTerm = sqlite3_value_bytes(pLe);
      pCsr->zLeTerm = sqlite3_malloc(pCsr->nLeTerm+1);
      if( pCsr->zLeTerm==0 ){
        rc = SQLITE_NOMEM;
      }else{
        memcpy(pCsr->zLeTerm, zCopy, pCsr->nLeTerm+1);
      }
    }
  }

  if( rc==SQLITE_OK ){
    Fts5Index *pIndex = pCsr->pFts5->pIndex;
    rc = sqlite3Fts5IndexQuery(pIndex, zTerm, nTerm, f, 0, &pCsr->pIter);
    if( rc==SQLITE_OK ){
      pCsr->pStruct = sqlite3Fts5StructureRef(pIndex);
    }
  }
  if( rc==SQLITE_OK && eType==FTS5_VOCAB_INSTANCE ){
    rc = fts5VocabInstanceNewTerm(pCsr);
  }
  if( rc==SQLITE_OK && !pCsr->bEof
   && (eType!=FTS5_VOCAB_INSTANCE
    || pCsr->pFts5->pConfig->eDetail!=FTS5_DETAIL_NONE)
  ){
    rc = fts5VocabNextMethod(pCursor);
  }

  return rc;
}

/*
** This is the xEof method of the virtual table. SQLite calls this
** routine to find out if it has reached the end of a result set.
*/
static int fts5VocabEofMethod(sqlite3_vtab_cursor *pCursor){
  Fts5VocabCursor *pCsr = (Fts5VocabCursor*)pCursor;
  return pCsr->bEof;
}

static int fts5VocabColumnMethod(
  sqlite3_vtab_cursor *pCursor,   /* Cursor to retrieve value from */
  sqlite3_context *pCtx,          /* Context for sqlite3_result_xxx() calls */
  int iCol                        /* Index of column to read value from */
){
  Fts5VocabCursor *pCsr = (Fts5VocabCursor*)pCursor;
  int eDetail = pCsr->pFts5->pConfig->eDetail;
  int eType = ((Fts5VocabTable*)(pCursor->pVtab))->eType;
  i64 iVal = 0;

  if( iCol==0 ){
    sqlite3_result_text(
        pCtx, (const char*)pCsr->term.p, pCsr->term.n, SQLITE_TRANSIENT
    );
  }else if( eType==FTS5_VOCAB_COL ){
    assert( iCol==1 || iCol==2 || iCol==3 );
    if( iCol==1 ){
      if( eDetail!=FTS5_DETAIL_NONE ){
        const char *z = pCsr->pFts5->pConfig->azCol[pCsr->iCol];
        sqlite3_result_text(pCtx, z, -1, SQLITE_STATIC);
      }
    }else if( iCol==2 ){
      iVal = pCsr->aDoc[pCsr->iCol];
    }else{
      iVal = pCsr->aCnt[pCsr->iCol];
    }
  }else if( eType==FTS5_VOCAB_ROW ){
    assert( iCol==1 || iCol==2 );
    if( iCol==1 ){
      iVal = pCsr->aDoc[0];
    }else{
      iVal = pCsr->aCnt[0];
    }
  }else{
    assert( eType==FTS5_VOCAB_INSTANCE );
    switch( iCol ){
      case 1:
        sqlite3_result_int64(pCtx, pCsr->pIter->iRowid);
        break;
      case 2: {
        int ii = -1;
        if( eDetail==FTS5_DETAIL_FULL ){
          ii = FTS5_POS2COLUMN(pCsr->iInstPos);
        }else if( eDetail==FTS5_DETAIL_COLUMNS ){
          ii = (int)pCsr->iInstPos;
        }
        if( ii>=0 && ii<pCsr->pFts5->pConfig->nCol ){
          const char *z = pCsr->pFts5->pConfig->azCol[ii];
          sqlite3_result_text(pCtx, z, -1, SQLITE_STATIC);
        }
        break;
      }
      default: {
        assert( iCol==3 );
        if( eDetail==FTS5_DETAIL_FULL ){
          int ii = FTS5_POS2OFFSET(pCsr->iInstPos);
          sqlite3_result_int(pCtx, ii);
        }
        break;
      }
    }
  }

  if( iVal>0 ) sqlite3_result_int64(pCtx, iVal);
  return SQLITE_OK;
}
