
/*
** This function retrieves the doclist for the specified term (or term
** prefix) from the database.
*/
static int fts3TermSelect(
  Fts3Table *p,                   /* Virtual table handle */
  Fts3PhraseToken *pTok,          /* Token to query for */
  int iColumn,                    /* Column to query (or -ve for all columns) */
  int *pnOut,                     /* OUT: Size of buffer at *ppOut */
  char **ppOut                    /* OUT: Malloced result buffer */
){
  int rc;                         /* Return code */
  Fts3MultiSegReader *pSegcsr;    /* Seg-reader cursor for this term */
  TermSelect tsc;                 /* Object for pair-wise doclist merging */
  Fts3SegFilter filter;           /* Segment term filter configuration */

  pSegcsr = pTok->pSegcsr;
  memset(&tsc, 0, sizeof(TermSelect));

  filter.flags = FTS3_SEGMENT_IGNORE_EMPTY | FTS3_SEGMENT_REQUIRE_POS
        | (pTok->isPrefix ? FTS3_SEGMENT_PREFIX : 0)
        | (pTok->bFirst ? FTS3_SEGMENT_FIRST : 0)
        | (iColumn<p->nColumn ? FTS3_SEGMENT_COLUMN_FILTER : 0);
  filter.iCol = iColumn;
  filter.zTerm = pTok->z;
  filter.nTerm = pTok->n;

  rc = sqlite3Fts3SegReaderStart(p, pSegcsr, &filter);
  while( SQLITE_OK==rc
      && SQLITE_ROW==(rc = sqlite3Fts3SegReaderStep(p, pSegcsr))
  ){
    rc = fts3TermSelectMerge(p, &tsc, pSegcsr->aDoclist, pSegcsr->nDoclist);
  }

  if( rc==SQLITE_OK ){
    rc = fts3TermSelectFinishMerge(p, &tsc);
  }
  if( rc==SQLITE_OK ){
    *ppOut = tsc.aaOutput[0];
    *pnOut = tsc.anOutput[0];
  }else{
    int i;
    for(i=0; i<SizeofArray(tsc.aaOutput); i++){
      sqlite3_free(tsc.aaOutput[i]);
    }
  }

  fts3SegReaderCursorFree(pSegcsr);
  pTok->pSegcsr = 0;
  return rc;
}

/*
** This function counts the total number of docids in the doclist stored
** in buffer aList[], size nList bytes.
**
** If the isPoslist argument is true, then it is assumed that the doclist
** contains a position-list following each docid. Otherwise, it is assumed
** that the doclist is simply a list of docids stored as delta encoded
** varints.
*/
static int fts3DoclistCountDocids(char *aList, int nList){
  int nDoc = 0;                   /* Return value */
  if( aList ){
    char *aEnd = &aList[nList];   /* Pointer to one byte after EOF */
    char *p = aList;              /* Cursor */
    while( p<aEnd ){
      nDoc++;
      while( (*p++)&0x80 );     /* Skip docid varint */
      fts3PoslistCopy(0, &p);   /* Skip over position list */
    }
  }

  return nDoc;
}

/*
** Advance the cursor to the next row in the %_content table that
** matches the search criteria.  For a MATCH search, this will be
** the next row that matches. For a full-table scan, this will be
** simply the next row in the %_content table.  For a docid lookup,
** this routine simply sets the EOF flag.
**
** Return SQLITE_OK if nothing goes wrong.  SQLITE_OK is returned
** even if we reach end-of-file.  The fts3EofMethod() will be called
** subsequently to determine whether or not an EOF was hit.
*/
static int fts3NextMethod(sqlite3_vtab_cursor *pCursor){
  int rc;
  Fts3Cursor *pCsr = (Fts3Cursor *)pCursor;
  if( pCsr->eSearch==FTS3_DOCID_SEARCH || pCsr->eSearch==FTS3_FULLSCAN_SEARCH ){
    Fts3Table *pTab = (Fts3Table*)pCursor->pVtab;
    pTab->bLock++;
    if( SQLITE_ROW!=sqlite3_step(pCsr->pStmt) ){
      pCsr->isEof = 1;
      rc = sqlite3_reset(pCsr->pStmt);
    }else{
      pCsr->iPrevId = sqlite3_column_int64(pCsr->pStmt, 0);
      rc = SQLITE_OK;
    }
    pTab->bLock--;
  }else{
    rc = fts3EvalNext((Fts3Cursor *)pCursor);
  }
  assert( ((Fts3Table *)pCsr->base.pVtab)->pSegments==0 );
  return rc;
}

/*
** If the numeric type of argument pVal is "integer", then return it
** converted to a 64-bit signed integer. Otherwise, return a copy of
** the second parameter, iDefault.
*/
static sqlite3_int64 fts3DocidRange(sqlite3_value *pVal, i64 iDefault){
  if( pVal ){
    int eType = sqlite3_value_numeric_type(pVal);
    if( eType==SQLITE_INTEGER ){
      return sqlite3_value_int64(pVal);
    }
  }
  return iDefault;
}

/*
** This is the xFilter interface for the virtual table.  See
** the virtual table xFilter method documentation for additional
** information.
**
** If idxNum==FTS3_FULLSCAN_SEARCH then do a full table scan against
** the %_content table.
**
** If idxNum==FTS3_DOCID_SEARCH then do a docid lookup for a single entry
** in the %_content table.
**
** If idxNum>=FTS3_FULLTEXT_SEARCH then use the full text index.  The
** column on the left-hand side of the MATCH operator is column
** number idxNum-FTS3_FULLTEXT_SEARCH, 0 indexed.  argv[0] is the right-hand
** side of the MATCH operator.
*/
static int fts3FilterMethod(
  sqlite3_vtab_cursor *pCursor,   /* The cursor used for this query */
  int idxNum,                     /* Strategy index */
  const char *idxStr,             /* Unused */
  int nVal,                       /* Number of elements in apVal */
  sqlite3_value **apVal           /* Arguments for the indexing scheme */
){
  int rc = SQLITE_OK;
  char *zSql;                     /* SQL statement used to access %_content */
  int eSearch;
  Fts3Table *p = (Fts3Table *)pCursor->pVtab;
  Fts3Cursor *pCsr = (Fts3Cursor *)pCursor;

  sqlite3_value *pCons = 0;       /* The MATCH or rowid constraint, if any */
  sqlite3_value *pLangid = 0;     /* The "langid = ?" constraint, if any */
  sqlite3_value *pDocidGe = 0;    /* The "docid >= ?" constraint, if any */
  sqlite3_value *pDocidLe = 0;    /* The "docid <= ?" constraint, if any */
  int iIdx;

  UNUSED_PARAMETER(idxStr);
  UNUSED_PARAMETER(nVal);

  if( p->bLock ){
    return SQLITE_ERROR;
  }

  eSearch = (idxNum & 0x0000FFFF);
  assert( eSearch>=0 && eSearch<=(FTS3_FULLTEXT_SEARCH+p->nColumn) );
  assert( p->pSegments==0 );

  /* Collect arguments into local variables */
  iIdx = 0;
  if( eSearch!=FTS3_FULLSCAN_SEARCH ) pCons = apVal[iIdx++];
  if( idxNum & FTS3_HAVE_LANGID ) pLangid = apVal[iIdx++];
  if( idxNum & FTS3_HAVE_DOCID_GE ) pDocidGe = apVal[iIdx++];
  if( idxNum & FTS3_HAVE_DOCID_LE ) pDocidLe = apVal[iIdx++];
  assert( iIdx==nVal );

  /* In case the cursor has been used before, clear it now. */
  fts3ClearCursor(pCsr);

  /* Set the lower and upper bounds on docids to return */
  pCsr->iMinDocid = fts3DocidRange(pDocidGe, SMALLEST_INT64);
  pCsr->iMaxDocid = fts3DocidRange(pDocidLe, LARGEST_INT64);

  if( idxStr ){
    pCsr->bDesc = (idxStr[0]=='D');
  }else{
    pCsr->bDesc = p->bDescIdx;
  }
  pCsr->eSearch = (i16)eSearch;

  if( eSearch!=FTS3_DOCID_SEARCH && eSearch!=FTS3_FULLSCAN_SEARCH ){
    int iCol = eSearch-FTS3_FULLTEXT_SEARCH;
    const char *zQuery = (const char *)sqlite3_value_text(pCons);

    if( zQuery==0 && sqlite3_value_type(pCons)!=SQLITE_NULL ){
      return SQLITE_NOMEM;
    }

    pCsr->iLangid = 0;
    if( pLangid ) pCsr->iLangid = sqlite3_value_int(pLangid);

    assert( p->base.zErrMsg==0 );
    rc = sqlite3Fts3ExprParse(p->pTokenizer, pCsr->iLangid,
        p->azColumn, p->bFts4, p->nColumn, iCol, zQuery, -1, &pCsr->pExpr,
        &p->base.zErrMsg
    );
    if( rc!=SQLITE_OK ){
      return rc;
    }

    rc = fts3EvalStart(pCsr);
    sqlite3Fts3SegmentsClose(p);
    if( rc!=SQLITE_OK ) return rc;
    pCsr->pNextId = pCsr->aDoclist;
    pCsr->iPrevId = 0;
  }

  /* Compile a SELECT statement for this cursor. For a full-table-scan, the
  ** statement loops through all rows of the %_content table. For a
  ** full-text query or docid lookup, the statement retrieves a single
  ** row by docid.
  */
  if( eSearch==FTS3_FULLSCAN_SEARCH ){
    if( pDocidGe || pDocidLe ){
      zSql = sqlite3_mprintf(
          "SELECT %s WHERE rowid BETWEEN %lld AND %lld ORDER BY rowid %s",
          p->zReadExprlist, pCsr->iMinDocid, pCsr->iMaxDocid,
          (pCsr->bDesc ? "DESC" : "ASC")
      );
    }else{
      zSql = sqlite3_mprintf("SELECT %s ORDER BY rowid %s",
          p->zReadExprlist, (pCsr->bDesc ? "DESC" : "ASC")
      );
    }
    if( zSql ){
      p->bLock++;
      rc = sqlite3_prepare_v3(
          p->db,zSql,-1,SQLITE_PREPARE_PERSISTENT,&pCsr->pStmt,0
      );
      p->bLock--;
      sqlite3_free(zSql);
    }else{
      rc = SQLITE_NOMEM;
    }
  }else if( eSearch==FTS3_DOCID_SEARCH ){
    rc = fts3CursorSeekStmt(pCsr);
    if( rc==SQLITE_OK ){
      rc = sqlite3_bind_value(pCsr->pStmt, 1, pCons);
    }
  }
  if( rc!=SQLITE_OK ) return rc;

  return fts3NextMethod(pCursor);
}

/*
** This is the xEof method of the virtual table. SQLite calls this
** routine to find out if it has reached the end of a result set.
*/
static int fts3EofMethod(sqlite3_vtab_cursor *pCursor){
  Fts3Cursor *pCsr = (Fts3Cursor*)pCursor;
  if( pCsr->isEof ){
    fts3ClearCursor(pCsr);
    pCsr->isEof = 1;
  }
  return pCsr->isEof;
}

/*
** This is the xRowid method. The SQLite core calls this routine to
** retrieve the rowid for the current row of the result set. fts3
** exposes %_content.docid as the rowid for the virtual table. The
** rowid should be written to *pRowid.
*/
static int fts3RowidMethod(sqlite3_vtab_cursor *pCursor, sqlite_int64 *pRowid){
  Fts3Cursor *pCsr = (Fts3Cursor *) pCursor;
  *pRowid = pCsr->iPrevId;
  return SQLITE_OK;
}

/*
** This is the xColumn method, called by SQLite to request a value from
** the row that the supplied cursor currently points to.
**
** If:
**
**   (iCol <  p->nColumn)   -> The value of the iCol'th user column.
**   (iCol == p->nColumn)   -> Magic column with the same name as the table.
**   (iCol == p->nColumn+1) -> Docid column
**   (iCol == p->nColumn+2) -> Langid column
*/
static int fts3ColumnMethod(
  sqlite3_vtab_cursor *pCursor,   /* Cursor to retrieve value from */
  sqlite3_context *pCtx,          /* Context for sqlite3_result_xxx() calls */
  int iCol                        /* Index of column to read value from */
){
  int rc = SQLITE_OK;             /* Return Code */
  Fts3Cursor *pCsr = (Fts3Cursor *) pCursor;
  Fts3Table *p = (Fts3Table *)pCursor->pVtab;

  /* The column value supplied by SQLite must be in range. */
  assert( iCol>=0 && iCol<=p->nColumn+2 );

  switch( iCol-p->nColumn ){
    case 0:
      /* The special 'table-name' column */
      sqlite3_result_pointer(pCtx, pCsr, "fts3cursor", 0);
      break;

    case 1:
      /* The docid column */
      sqlite3_result_int64(pCtx, pCsr->iPrevId);
      break;

    case 2:
      if( pCsr->pExpr ){
        sqlite3_result_int64(pCtx, pCsr->iLangid);
        break;
      }else if( p->zLanguageid==0 ){
        sqlite3_result_int(pCtx, 0);
        break;
      }else{
        iCol = p->nColumn;
        /* no break */ deliberate_fall_through
      }

    default:
      /* A user column. Or, if this is a full-table scan, possibly the
      ** language-id column. Seek the cursor. */
      rc = fts3CursorSeek(0, pCsr);
      if( rc==SQLITE_OK && sqlite3_data_count(pCsr->pStmt)-1>iCol ){
        sqlite3_result_value(pCtx, sqlite3_column_value(pCsr->pStmt, iCol+1));
      }
      break;
  }

  assert( ((Fts3Table *)pCsr->base.pVtab)->pSegments==0 );
  return rc;
}

/*
** This function is the implementation of the xUpdate callback used by
** FTS3 virtual tables. It is invoked by SQLite each time a row is to be
** inserted, updated or deleted.
*/
static int fts3UpdateMethod(
  sqlite3_vtab *pVtab,            /* Virtual table handle */
  int nArg,                       /* Size of argument array */
  sqlite3_value **apVal,          /* Array of arguments */
  sqlite_int64 *pRowid            /* OUT: The affected (or effected) rowid */
){
  return sqlite3Fts3UpdateMethod(pVtab, nArg, apVal, pRowid);
}
