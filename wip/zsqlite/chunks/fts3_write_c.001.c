

static int fts3SelectDocsize(
  Fts3Table *pTab,                /* FTS3 table handle */
  sqlite3_int64 iDocid,           /* Docid to bind for SQL_SELECT_DOCSIZE */
  sqlite3_stmt **ppStmt           /* OUT: Statement handle */
){
  sqlite3_stmt *pStmt = 0;        /* Statement requested from fts3SqlStmt() */
  int rc;                         /* Return code */

  rc = fts3SqlStmt(pTab, SQL_SELECT_DOCSIZE, &pStmt, 0);
  if( rc==SQLITE_OK ){
    sqlite3_bind_int64(pStmt, 1, iDocid);
    rc = sqlite3_step(pStmt);
    if( rc!=SQLITE_ROW || sqlite3_column_type(pStmt, 0)!=SQLITE_BLOB ){
      rc = sqlite3_reset(pStmt);
      if( rc==SQLITE_OK ) rc = FTS_CORRUPT_VTAB;
      pStmt = 0;
    }else{
      rc = SQLITE_OK;
    }
  }

  *ppStmt = pStmt;
  return rc;
}

SQLITE_PRIVATE int sqlite3Fts3SelectDoctotal(
  Fts3Table *pTab,                /* Fts3 table handle */
  sqlite3_stmt **ppStmt           /* OUT: Statement handle */
){
  sqlite3_stmt *pStmt = 0;
  int rc;
  rc = fts3SqlStmt(pTab, SQL_SELECT_STAT, &pStmt, 0);
  if( rc==SQLITE_OK ){
    sqlite3_bind_int(pStmt, 1, FTS_STAT_DOCTOTAL);
    if( sqlite3_step(pStmt)!=SQLITE_ROW
     || sqlite3_column_type(pStmt, 0)!=SQLITE_BLOB
    ){
      rc = sqlite3_reset(pStmt);
      if( rc==SQLITE_OK ) rc = FTS_CORRUPT_VTAB;
      pStmt = 0;
    }
  }
  *ppStmt = pStmt;
  return rc;
}

SQLITE_PRIVATE int sqlite3Fts3SelectDocsize(
  Fts3Table *pTab,                /* Fts3 table handle */
  sqlite3_int64 iDocid,           /* Docid to read size data for */
  sqlite3_stmt **ppStmt           /* OUT: Statement handle */
){
  return fts3SelectDocsize(pTab, iDocid, ppStmt);
}

/*
** Similar to fts3SqlStmt(). Except, after binding the parameters in
** array apVal[] to the SQL statement identified by eStmt, the statement
** is executed.
**
** Returns SQLITE_OK if the statement is successfully executed, or an
** SQLite error code otherwise.
*/
static void fts3SqlExec(
  int *pRC,                /* Result code */
  Fts3Table *p,            /* The FTS3 table */
  int eStmt,               /* Index of statement to evaluate */
  sqlite3_value **apVal    /* Parameters to bind */
){
  sqlite3_stmt *pStmt;
  int rc;
  if( *pRC ) return;
  rc = fts3SqlStmt(p, eStmt, &pStmt, apVal);
  if( rc==SQLITE_OK ){
    sqlite3_step(pStmt);
    rc = sqlite3_reset(pStmt);
  }
  *pRC = rc;
}


/*
** This function ensures that the caller has obtained an exclusive
** shared-cache table-lock on the %_segdir table. This is required before
** writing data to the fts3 table. If this lock is not acquired first, then
** the caller may end up attempting to take this lock as part of committing
** a transaction, causing SQLite to return SQLITE_LOCKED or
** LOCKED_SHAREDCACHEto a COMMIT command.
**
** It is best to avoid this because if FTS3 returns any error when
** committing a transaction, the whole transaction will be rolled back.
** And this is not what users expect when they get SQLITE_LOCKED_SHAREDCACHE.
** It can still happen if the user locks the underlying tables directly
** instead of accessing them via FTS.
*/
static int fts3Writelock(Fts3Table *p){
  int rc = SQLITE_OK;

  if( p->nPendingData==0 ){
    sqlite3_stmt *pStmt;
    rc = fts3SqlStmt(p, SQL_DELETE_SEGDIR_LEVEL, &pStmt, 0);
    if( rc==SQLITE_OK ){
      sqlite3_bind_null(pStmt, 1);
      sqlite3_step(pStmt);
      rc = sqlite3_reset(pStmt);
    }
  }

  return rc;
}

/*
** FTS maintains a separate indexes for each language-id (a 32-bit integer).
** Within each language id, a separate index is maintained to store the
** document terms, and each configured prefix size (configured the FTS
** "prefix=" option). And each index consists of multiple levels ("relative
** levels").
**
** All three of these values (the language id, the specific index and the
** level within the index) are encoded in 64-bit integer values stored
** in the %_segdir table on disk. This function is used to convert three
** separate component values into the single 64-bit integer value that
** can be used to query the %_segdir table.
**
** Specifically, each language-id/index combination is allocated 1024
** 64-bit integer level values ("absolute levels"). The main terms index
** for language-id 0 is allocate values 0-1023. The first prefix index
** (if any) for language-id 0 is allocated values 1024-2047. And so on.
** Language 1 indexes are allocated immediately following language 0.
**
** So, for a system with nPrefix prefix indexes configured, the block of
** absolute levels that corresponds to language-id iLangid and index
** iIndex starts at absolute level ((iLangid * (nPrefix+1) + iIndex) * 1024).
*/
static sqlite3_int64 getAbsoluteLevel(
  Fts3Table *p,                   /* FTS3 table handle */
  int iLangid,                    /* Language id */
  int iIndex,                     /* Index in p->aIndex[] */
  int iLevel                      /* Level of segments */
){
  sqlite3_int64 iBase;            /* First absolute level for iLangid/iIndex */
  assert_fts3_nc( iLangid>=0 );
  assert( p->nIndex>0 );
  assert( iIndex>=0 && iIndex<p->nIndex );

  iBase = ((sqlite3_int64)iLangid * p->nIndex + iIndex) * FTS3_SEGDIR_MAXLEVEL;
  return iBase + iLevel;
}

/*
** Set *ppStmt to a statement handle that may be used to iterate through
** all rows in the %_segdir table, from oldest to newest. If successful,
** return SQLITE_OK. If an error occurs while preparing the statement,
** return an SQLite error code.
**
** There is only ever one instance of this SQL statement compiled for
** each FTS3 table.
**
** The statement returns the following columns from the %_segdir table:
**
**   0: idx
**   1: start_block
**   2: leaves_end_block
**   3: end_block
**   4: root
*/
SQLITE_PRIVATE int sqlite3Fts3AllSegdirs(
  Fts3Table *p,                   /* FTS3 table */
  int iLangid,                    /* Language being queried */
  int iIndex,                     /* Index for p->aIndex[] */
  int iLevel,                     /* Level to select (relative level) */
  sqlite3_stmt **ppStmt           /* OUT: Compiled statement */
){
  int rc;
  sqlite3_stmt *pStmt = 0;

  assert( iLevel==FTS3_SEGCURSOR_ALL || iLevel>=0 );
  assert( iLevel<FTS3_SEGDIR_MAXLEVEL );
  assert( iIndex>=0 && iIndex<p->nIndex );

  if( iLevel<0 ){
    /* "SELECT * FROM %_segdir WHERE level BETWEEN ? AND ? ORDER BY ..." */
    rc = fts3SqlStmt(p, SQL_SELECT_LEVEL_RANGE, &pStmt, 0);
    if( rc==SQLITE_OK ){
      sqlite3_bind_int64(pStmt, 1, getAbsoluteLevel(p, iLangid, iIndex, 0));
      sqlite3_bind_int64(pStmt, 2,
          getAbsoluteLevel(p, iLangid, iIndex, FTS3_SEGDIR_MAXLEVEL-1)
      );
    }
  }else{
    /* "SELECT * FROM %_segdir WHERE level = ? ORDER BY ..." */
    rc = fts3SqlStmt(p, SQL_SELECT_LEVEL, &pStmt, 0);
    if( rc==SQLITE_OK ){
      sqlite3_bind_int64(pStmt, 1, getAbsoluteLevel(p, iLangid, iIndex,iLevel));
    }
  }
  *ppStmt = pStmt;
  return rc;
}


/*
** Append a single varint to a PendingList buffer. SQLITE_OK is returned
** if successful, or an SQLite error code otherwise.
**
** This function also serves to allocate the PendingList structure itself.
** For example, to create a new PendingList structure containing two
** varints:
**
**   PendingList *p = 0;
**   fts3PendingListAppendVarint(&p, 1);
**   fts3PendingListAppendVarint(&p, 2);
*/
static int fts3PendingListAppendVarint(
  PendingList **pp,               /* IN/OUT: Pointer to PendingList struct */
  sqlite3_int64 i                 /* Value to append to data */
){
  PendingList *p = *pp;

  /* Allocate or grow the PendingList as required. */
  if( !p ){
    p = sqlite3_malloc64(sizeof(*p) + 100);
    if( !p ){
      return SQLITE_NOMEM;
    }
    p->nSpace = 100;
    p->aData = (char *)&p[1];
    p->nData = 0;
  }
  else if( p->nData+FTS3_VARINT_MAX+1>p->nSpace ){
    i64 nNew = p->nSpace * 2;
    p = sqlite3_realloc64(p, sizeof(*p) + nNew);
    if( !p ){
      sqlite3_free(*pp);
      *pp = 0;
      return SQLITE_NOMEM;
    }
    p->nSpace = (int)nNew;
    p->aData = (char *)&p[1];
  }

  /* Append the new serialized varint to the end of the list. */
  p->nData += sqlite3Fts3PutVarint(&p->aData[p->nData], i);
  p->aData[p->nData] = '\0';
  *pp = p;
  return SQLITE_OK;
}

/*
** Add a docid/column/position entry to a PendingList structure. Non-zero
** is returned if the structure is sqlite3_realloced as part of adding
** the entry. Otherwise, zero.
**
** If an OOM error occurs, *pRc is set to SQLITE_NOMEM before returning.
** Zero is always returned in this case. Otherwise, if no OOM error occurs,
** it is set to SQLITE_OK.
*/
static int fts3PendingListAppend(
  PendingList **pp,               /* IN/OUT: PendingList structure */
  sqlite3_int64 iDocid,           /* Docid for entry to add */
  sqlite3_int64 iCol,             /* Column for entry to add */
  sqlite3_int64 iPos,             /* Position of term for entry to add */
  int *pRc                        /* OUT: Return code */
){
  PendingList *p = *pp;
  int rc = SQLITE_OK;

  assert( !p || p->iLastDocid<=iDocid );

  if( !p || p->iLastDocid!=iDocid ){
    u64 iDelta = (u64)iDocid - (u64)(p ? p->iLastDocid : 0);
    if( p ){
      assert( p->nData<p->nSpace );
      assert( p->aData[p->nData]==0 );
      p->nData++;
    }
    if( SQLITE_OK!=(rc = fts3PendingListAppendVarint(&p, iDelta)) ){
      goto pendinglistappend_out;
    }
    p->iLastCol = -1;
    p->iLastPos = 0;
    p->iLastDocid = iDocid;
  }
  if( iCol>0 && p->iLastCol!=iCol ){
    if( SQLITE_OK!=(rc = fts3PendingListAppendVarint(&p, 1))
     || SQLITE_OK!=(rc = fts3PendingListAppendVarint(&p, iCol))
    ){
      goto pendinglistappend_out;
    }
    p->iLastCol = iCol;
    p->iLastPos = 0;
  }
  if( iCol>=0 ){
    assert( iPos>p->iLastPos || (iPos==0 && p->iLastPos==0) );
    rc = fts3PendingListAppendVarint(&p, 2+iPos-p->iLastPos);
    if( rc==SQLITE_OK ){
      p->iLastPos = iPos;
    }
  }

 pendinglistappend_out:
  *pRc = rc;
  if( p!=*pp ){
    *pp = p;
    return 1;
  }
  return 0;
}

/*
** Free a PendingList object allocated by fts3PendingListAppend().
*/
static void fts3PendingListDelete(PendingList *pList){
  sqlite3_free(pList);
}

/*
** Add an entry to one of the pending-terms hash tables.
*/
static int fts3PendingTermsAddOne(
  Fts3Table *p,
  int iCol,
  int iPos,
  Fts3Hash *pHash,                /* Pending terms hash table to add entry to */
  const char *zToken,
  int nToken
){
  PendingList *pList;
  int rc = SQLITE_OK;

  pList = (PendingList *)fts3HashFind(pHash, zToken, nToken);
  if( pList ){
    p->nPendingData -= (pList->nData + nToken + sizeof(Fts3HashElem));
  }
  if( fts3PendingListAppend(&pList, p->iPrevDocid, iCol, iPos, &rc) ){
    if( pList==fts3HashInsert(pHash, zToken, nToken, pList) ){
      /* Malloc failed while inserting the new entry. This can only
      ** happen if there was no previous entry for this token.
      */
      assert( 0==fts3HashFind(pHash, zToken, nToken) );
      sqlite3_free(pList);
      rc = SQLITE_NOMEM;
    }
  }
  if( rc==SQLITE_OK ){
    p->nPendingData += (pList->nData + nToken + sizeof(Fts3HashElem));
  }
  return rc;
}

/*
** Tokenize the nul-terminated string zText and add all tokens to the
** pending-terms hash-table. The docid used is that currently stored in
** p->iPrevDocid, and the column is specified by argument iCol.
**
** If successful, SQLITE_OK is returned. Otherwise, an SQLite error code.
*/
static int fts3PendingTermsAdd(
  Fts3Table *p,                   /* Table into which text will be inserted */
  int iLangid,                    /* Language id to use */
  const char *zText,              /* Text of document to be inserted */
  int iCol,                       /* Column into which text is being inserted */
  u32 *pnWord                     /* IN/OUT: Incr. by number tokens inserted */
){
  int rc;
  int iStart = 0;
  int iEnd = 0;
  int iPos = 0;
  int nWord = 0;

  char const *zToken;
  int nToken = 0;

  sqlite3_tokenizer *pTokenizer = p->pTokenizer;
  sqlite3_tokenizer_module const *pModule = pTokenizer->pModule;
  sqlite3_tokenizer_cursor *pCsr;
  int (*xNext)(sqlite3_tokenizer_cursor *pCursor,
      const char**,int*,int*,int*,int*);

  assert( pTokenizer && pModule );

  /* If the user has inserted a NULL value, this function may be called with
  ** zText==0. In this case, add zero token entries to the hash table and
  ** return early. */
  if( zText==0 ){
    *pnWord = 0;
    return SQLITE_OK;
  }

  rc = sqlite3Fts3OpenTokenizer(pTokenizer, iLangid, zText, -1, &pCsr);
  if( rc!=SQLITE_OK ){
    return rc;
  }

  xNext = pModule->xNext;
  while( SQLITE_OK==rc
      && SQLITE_OK==(rc = xNext(pCsr, &zToken, &nToken, &iStart, &iEnd, &iPos))
  ){
    int i;
    if( iPos>=nWord ) nWord = iPos+1;

    /* Positions cannot be negative; we use -1 as a terminator internally.
    ** Tokens must have a non-zero length.
    */
    if( iPos<0 || !zToken || nToken<=0 ){
      rc = SQLITE_ERROR;
      break;
    }

    /* Add the term to the terms index */
    rc = fts3PendingTermsAddOne(
        p, iCol, iPos, &p->aIndex[0].hPending, zToken, nToken
    );

    /* Add the term to each of the prefix indexes that it is not too
    ** short for. */
    for(i=1; rc==SQLITE_OK && i<p->nIndex; i++){
      struct Fts3Index *pIndex = &p->aIndex[i];
      if( nToken<pIndex->nPrefix ) continue;
      rc = fts3PendingTermsAddOne(
          p, iCol, iPos, &pIndex->hPending, zToken, pIndex->nPrefix
      );
    }
  }

  pModule->xClose(pCsr);
  *pnWord += nWord;
  return (rc==SQLITE_DONE ? SQLITE_OK : rc);
}
