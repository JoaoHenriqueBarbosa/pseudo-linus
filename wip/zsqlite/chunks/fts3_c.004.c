
/*
** The xConnect() and xCreate() methods for the virtual table. All the
** work is done in function fts3InitVtab().
*/
static int fts3ConnectMethod(
  sqlite3 *db,                    /* Database connection */
  void *pAux,                     /* Pointer to tokenizer hash table */
  int argc,                       /* Number of elements in argv array */
  const char * const *argv,       /* xCreate/xConnect argument array */
  sqlite3_vtab **ppVtab,          /* OUT: New sqlite3_vtab object */
  char **pzErr                    /* OUT: sqlite3_malloc'd error message */
){
  return fts3InitVtab(0, db, pAux, argc, argv, ppVtab, pzErr);
}
static int fts3CreateMethod(
  sqlite3 *db,                    /* Database connection */
  void *pAux,                     /* Pointer to tokenizer hash table */
  int argc,                       /* Number of elements in argv array */
  const char * const *argv,       /* xCreate/xConnect argument array */
  sqlite3_vtab **ppVtab,          /* OUT: New sqlite3_vtab object */
  char **pzErr                    /* OUT: sqlite3_malloc'd error message */
){
  return fts3InitVtab(1, db, pAux, argc, argv, ppVtab, pzErr);
}

/*
** Set the pIdxInfo->estimatedRows variable to nRow. Unless this
** extension is currently being used by a version of SQLite too old to
** support estimatedRows. In that case this function is a no-op.
*/
static void fts3SetEstimatedRows(sqlite3_index_info *pIdxInfo, i64 nRow){
#if SQLITE_VERSION_NUMBER>=3008002
  if( sqlite3_libversion_number()>=3008002 ){
    pIdxInfo->estimatedRows = nRow;
  }
#endif
}

/*
** Set the SQLITE_INDEX_SCAN_UNIQUE flag in pIdxInfo->flags. Unless this
** extension is currently being used by a version of SQLite too old to
** support index-info flags. In that case this function is a no-op.
*/
static void fts3SetUniqueFlag(sqlite3_index_info *pIdxInfo){
#if SQLITE_VERSION_NUMBER>=3008012
  if( sqlite3_libversion_number()>=3008012 ){
    pIdxInfo->idxFlags |= SQLITE_INDEX_SCAN_UNIQUE;
  }
#endif
}

/*
** Implementation of the xBestIndex method for FTS3 tables. There
** are three possible strategies, in order of preference:
**
**   1. Direct lookup by rowid or docid.
**   2. Full-text search using a MATCH operator on a non-docid column.
**   3. Linear scan of %_content table.
*/
static int fts3BestIndexMethod(sqlite3_vtab *pVTab, sqlite3_index_info *pInfo){
  Fts3Table *p = (Fts3Table *)pVTab;
  int i;                          /* Iterator variable */
  int iCons = -1;                 /* Index of constraint to use */

  int iLangidCons = -1;           /* Index of langid=x constraint, if present */
  int iDocidGe = -1;              /* Index of docid>=x constraint, if present */
  int iDocidLe = -1;              /* Index of docid<=x constraint, if present */
  int iIdx;

  if( p->bLock ){
    return SQLITE_ERROR;
  }

  /* By default use a full table scan. This is an expensive option,
  ** so search through the constraints to see if a more efficient
  ** strategy is possible.
  */
  pInfo->idxNum = FTS3_FULLSCAN_SEARCH;
  pInfo->estimatedCost = 5000000;
  for(i=0; i<pInfo->nConstraint; i++){
    int bDocid;                 /* True if this constraint is on docid */
    struct sqlite3_index_constraint *pCons = &pInfo->aConstraint[i];
    if( pCons->usable==0 ){
      if( pCons->op==SQLITE_INDEX_CONSTRAINT_MATCH ){
        /* There exists an unusable MATCH constraint. This means that if
        ** the planner does elect to use the results of this call as part
        ** of the overall query plan the user will see an "unable to use
        ** function MATCH in the requested context" error. To discourage
        ** this, return a very high cost here.  */
        pInfo->idxNum = FTS3_FULLSCAN_SEARCH;
        pInfo->estimatedCost = 1e50;
        fts3SetEstimatedRows(pInfo, ((sqlite3_int64)1) << 50);
        return SQLITE_OK;
      }
      continue;
    }

    bDocid = (pCons->iColumn<0 || pCons->iColumn==p->nColumn+1);

    /* A direct lookup on the rowid or docid column. Assign a cost of 1.0. */
    if( iCons<0 && pCons->op==SQLITE_INDEX_CONSTRAINT_EQ && bDocid ){
      pInfo->idxNum = FTS3_DOCID_SEARCH;
      pInfo->estimatedCost = 1.0;
      iCons = i;
    }

    /* A MATCH constraint. Use a full-text search.
    **
    ** If there is more than one MATCH constraint available, use the first
    ** one encountered. If there is both a MATCH constraint and a direct
    ** rowid/docid lookup, prefer the MATCH strategy. This is done even
    ** though the rowid/docid lookup is faster than a MATCH query, selecting
    ** it would lead to an "unable to use function MATCH in the requested
    ** context" error.
    */
    if( pCons->op==SQLITE_INDEX_CONSTRAINT_MATCH
     && pCons->iColumn>=0 && pCons->iColumn<=p->nColumn
    ){
      pInfo->idxNum = FTS3_FULLTEXT_SEARCH + pCons->iColumn;
      pInfo->estimatedCost = 2.0;
      iCons = i;
    }

    /* Equality constraint on the langid column */
    if( pCons->op==SQLITE_INDEX_CONSTRAINT_EQ
     && pCons->iColumn==p->nColumn + 2
    ){
      iLangidCons = i;
    }

    if( bDocid ){
      switch( pCons->op ){
        case SQLITE_INDEX_CONSTRAINT_GE:
        case SQLITE_INDEX_CONSTRAINT_GT:
          iDocidGe = i;
          break;

        case SQLITE_INDEX_CONSTRAINT_LE:
        case SQLITE_INDEX_CONSTRAINT_LT:
          iDocidLe = i;
          break;
      }
    }
  }

  /* If using a docid=? or rowid=? strategy, set the UNIQUE flag. */
  if( pInfo->idxNum==FTS3_DOCID_SEARCH ) fts3SetUniqueFlag(pInfo);

  iIdx = 1;
  if( iCons>=0 ){
    pInfo->aConstraintUsage[iCons].argvIndex = iIdx++;
    pInfo->aConstraintUsage[iCons].omit = 1;
  }
  if( iLangidCons>=0 ){
    pInfo->idxNum |= FTS3_HAVE_LANGID;
    pInfo->aConstraintUsage[iLangidCons].argvIndex = iIdx++;
  }
  if( iDocidGe>=0 ){
    pInfo->idxNum |= FTS3_HAVE_DOCID_GE;
    pInfo->aConstraintUsage[iDocidGe].argvIndex = iIdx++;
  }
  if( iDocidLe>=0 ){
    pInfo->idxNum |= FTS3_HAVE_DOCID_LE;
    pInfo->aConstraintUsage[iDocidLe].argvIndex = iIdx++;
  }

  /* Regardless of the strategy selected, FTS can deliver rows in rowid (or
  ** docid) order. Both ascending and descending are possible.
  */
  if( pInfo->nOrderBy==1 ){
    struct sqlite3_index_orderby *pOrder = &pInfo->aOrderBy[0];
    if( pOrder->iColumn<0 || pOrder->iColumn==p->nColumn+1 ){
      if( pOrder->desc ){
        pInfo->idxStr = "DESC";
      }else{
        pInfo->idxStr = "ASC";
      }
      pInfo->orderByConsumed = 1;
    }
  }

  assert( p->pSegments==0 );
  return SQLITE_OK;
}

/*
** Implementation of xOpen method.
*/
static int fts3OpenMethod(sqlite3_vtab *pVTab, sqlite3_vtab_cursor **ppCsr){
  sqlite3_vtab_cursor *pCsr;               /* Allocated cursor */

  UNUSED_PARAMETER(pVTab);

  /* Allocate a buffer large enough for an Fts3Cursor structure. If the
  ** allocation succeeds, zero it and return SQLITE_OK. Otherwise,
  ** if the allocation fails, return SQLITE_NOMEM.
  */
  *ppCsr = pCsr = (sqlite3_vtab_cursor *)sqlite3_malloc(sizeof(Fts3Cursor));
  if( !pCsr ){
    return SQLITE_NOMEM;
  }
  memset(pCsr, 0, sizeof(Fts3Cursor));
  return SQLITE_OK;
}

/*
** Finalize the statement handle at pCsr->pStmt.
**
** Or, if that statement handle is one created by fts3CursorSeekStmt(),
** and the Fts3Table.pSeekStmt slot is currently NULL, save the statement
** pointer there instead of finalizing it.
*/
static void fts3CursorFinalizeStmt(Fts3Cursor *pCsr){
  if( pCsr->bSeekStmt ){
    Fts3Table *p = (Fts3Table *)pCsr->base.pVtab;
    if( p->pSeekStmt==0 ){
      p->pSeekStmt = pCsr->pStmt;
      sqlite3_reset(pCsr->pStmt);
      pCsr->pStmt = 0;
    }
    pCsr->bSeekStmt = 0;
  }
  sqlite3_finalize(pCsr->pStmt);
}

/*
** Free all resources currently held by the cursor passed as the only
** argument.
*/
static void fts3ClearCursor(Fts3Cursor *pCsr){
  fts3CursorFinalizeStmt(pCsr);
  sqlite3Fts3FreeDeferredTokens(pCsr);
  sqlite3_free(pCsr->aDoclist);
  sqlite3Fts3MIBufferFree(pCsr->pMIBuffer);
  sqlite3Fts3ExprFree(pCsr->pExpr);
  memset(&(&pCsr->base)[1], 0, sizeof(Fts3Cursor)-sizeof(sqlite3_vtab_cursor));
}

/*
** Close the cursor.  For additional information see the documentation
** on the xClose method of the virtual table interface.
*/
static int fts3CloseMethod(sqlite3_vtab_cursor *pCursor){
  Fts3Cursor *pCsr = (Fts3Cursor *)pCursor;
  assert( ((Fts3Table *)pCsr->base.pVtab)->pSegments==0 );
  fts3ClearCursor(pCsr);
  assert( ((Fts3Table *)pCsr->base.pVtab)->pSegments==0 );
  sqlite3_free(pCsr);
  return SQLITE_OK;
}

/*
** If pCsr->pStmt has not been prepared (i.e. if pCsr->pStmt==0), then
** compose and prepare an SQL statement of the form:
**
**    "SELECT <columns> FROM %_content WHERE rowid = ?"
**
** (or the equivalent for a content=xxx table) and set pCsr->pStmt to
** it. If an error occurs, return an SQLite error code.
*/
static int fts3CursorSeekStmt(Fts3Cursor *pCsr){
  int rc = SQLITE_OK;
  if( pCsr->pStmt==0 ){
    Fts3Table *p = (Fts3Table *)pCsr->base.pVtab;
    char *zSql;
    if( p->pSeekStmt ){
      pCsr->pStmt = p->pSeekStmt;
      p->pSeekStmt = 0;
    }else{
      zSql = sqlite3_mprintf("SELECT %s WHERE rowid = ?", p->zReadExprlist);
      if( !zSql ) return SQLITE_NOMEM;
      p->bLock++;
      rc = sqlite3_prepare_v3(
          p->db, zSql,-1,SQLITE_PREPARE_PERSISTENT,&pCsr->pStmt,0
      );
      p->bLock--;
      sqlite3_free(zSql);
    }
    if( rc==SQLITE_OK ) pCsr->bSeekStmt = 1;
  }
  return rc;
}

/*
** Position the pCsr->pStmt statement so that it is on the row
** of the %_content table that contains the last match.  Return
** SQLITE_OK on success.
*/
static int fts3CursorSeek(sqlite3_context *pContext, Fts3Cursor *pCsr){
  int rc = SQLITE_OK;
  if( pCsr->isRequireSeek ){
    rc = fts3CursorSeekStmt(pCsr);
    if( rc==SQLITE_OK ){
      Fts3Table *pTab = (Fts3Table*)pCsr->base.pVtab;
      pTab->bLock++;
      sqlite3_bind_int64(pCsr->pStmt, 1, pCsr->iPrevId);
      pCsr->isRequireSeek = 0;
      if( SQLITE_ROW==sqlite3_step(pCsr->pStmt) ){
        pTab->bLock--;
        return SQLITE_OK;
      }else{
        pTab->bLock--;
        rc = sqlite3_reset(pCsr->pStmt);
        if( rc==SQLITE_OK && ((Fts3Table *)pCsr->base.pVtab)->zContentTbl==0 ){
          /* If no row was found and no error has occurred, then the %_content
          ** table is missing a row that is present in the full-text index.
          ** The data structures are corrupt.  */
          rc = FTS_CORRUPT_VTAB;
          pCsr->isEof = 1;
        }
      }
    }
  }

  if( rc!=SQLITE_OK && pContext ){
    sqlite3_result_error_code(pContext, rc);
  }
  return rc;
}

/*
** This function is used to process a single interior node when searching
** a b-tree for a term or term prefix. The node data is passed to this
** function via the zNode/nNode parameters. The term to search for is
** passed in zTerm/nTerm.
**
** If piFirst is not NULL, then this function sets *piFirst to the blockid
** of the child node that heads the sub-tree that may contain the term.
**
** If piLast is not NULL, then *piLast is set to the right-most child node
** that heads a sub-tree that may contain a term for which zTerm/nTerm is
** a prefix.
**
** If an OOM error occurs, SQLITE_NOMEM is returned. Otherwise, SQLITE_OK.
*/
static int fts3ScanInteriorNode(
  const char *zTerm,              /* Term to select leaves for */
  int nTerm,                      /* Size of term zTerm in bytes */
  const char *zNode,              /* Buffer containing segment interior node */
  int nNode,                      /* Size of buffer at zNode */
  sqlite3_int64 *piFirst,         /* OUT: Selected child node */
  sqlite3_int64 *piLast           /* OUT: Selected child node */
){
  int rc = SQLITE_OK;             /* Return code */
  const char *zCsr = zNode;       /* Cursor to iterate through node */
  const char *zEnd = &zCsr[nNode];/* End of interior node buffer */
  char *zBuffer = 0;              /* Buffer to load terms into */
  i64 nAlloc = 0;                 /* Size of allocated buffer */
  int isFirstTerm = 1;            /* True when processing first term on page */
  u64 iChild;                     /* Block id of child node to descend to */
  int nBuffer = 0;                /* Total term size */

  /* Skip over the 'height' varint that occurs at the start of every
  ** interior node. Then load the blockid of the left-child of the b-tree
  ** node into variable iChild.
  **
  ** Even if the data structure on disk is corrupted, this (reading two
  ** varints from the buffer) does not risk an overread. If zNode is a
  ** root node, then the buffer comes from a SELECT statement. SQLite does
  ** not make this guarantee explicitly, but in practice there are always
  ** either more than 20 bytes of allocated space following the nNode bytes of
  ** contents, or two zero bytes. Or, if the node is read from the %_segments
  ** table, then there are always 20 bytes of zeroed padding following the
  ** nNode bytes of content (see sqlite3Fts3ReadBlock() for details).
  */
  zCsr += sqlite3Fts3GetVarintU(zCsr, &iChild);
  zCsr += sqlite3Fts3GetVarintU(zCsr, &iChild);
  if( zCsr>zEnd ){
    return FTS_CORRUPT_VTAB;
  }

  while( zCsr<zEnd && (piFirst || piLast) ){
    int cmp;                      /* memcmp() result */
    int nSuffix;                  /* Size of term suffix */
    int nPrefix = 0;              /* Size of term prefix */

    /* Load the next term on the node into zBuffer. Use realloc() to expand
    ** the size of zBuffer if required.  */
    if( !isFirstTerm ){
      zCsr += fts3GetVarint32(zCsr, &nPrefix);
      if( nPrefix>nBuffer ){
        rc = FTS_CORRUPT_VTAB;
        goto finish_scan;
      }
    }
    isFirstTerm = 0;
    zCsr += fts3GetVarint32(zCsr, &nSuffix);

    assert( nPrefix>=0 && nSuffix>=0 );
    if( nPrefix>zCsr-zNode || nSuffix>zEnd-zCsr || nSuffix==0 ){
      rc = FTS_CORRUPT_VTAB;
      goto finish_scan;
    }
    if( (i64)nPrefix+nSuffix>nAlloc ){
      char *zNew;
      nAlloc = ((i64)nPrefix+nSuffix) * 2;
      zNew = (char *)sqlite3_realloc64(zBuffer, nAlloc);
      if( !zNew ){
        rc = SQLITE_NOMEM;
        goto finish_scan;
      }
      zBuffer = zNew;
    }
    assert( zBuffer );
    memcpy(&zBuffer[nPrefix], zCsr, nSuffix);
    nBuffer = nPrefix + nSuffix;
    zCsr += nSuffix;

    /* Compare the term we are searching for with the term just loaded from
    ** the interior node. If the specified term is greater than or equal
    ** to the term from the interior node, then all terms on the sub-tree
    ** headed by node iChild are smaller than zTerm. No need to search
    ** iChild.
    **
    ** If the interior node term is larger than the specified term, then
    ** the tree headed by iChild may contain the specified term.
    */
    cmp = memcmp(zTerm, zBuffer, (nBuffer>nTerm ? nTerm : nBuffer));
    if( piFirst && (cmp<0 || (cmp==0 && nBuffer>nTerm)) ){
      *piFirst = (i64)iChild;
      piFirst = 0;
    }

    if( piLast && cmp<0 ){
      *piLast = (i64)iChild;
      piLast = 0;
    }

    iChild++;
  };

  if( piFirst ) *piFirst = (i64)iChild;
  if( piLast ) *piLast = (i64)iChild;

 finish_scan:
  sqlite3_free(zBuffer);
  return rc;
}
