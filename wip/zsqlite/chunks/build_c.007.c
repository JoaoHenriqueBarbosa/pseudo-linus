
#ifndef SQLITE_OMIT_VIEW
/*
** The parser calls this routine in order to create a new VIEW
*/
SQLITE_PRIVATE void sqlite3CreateView(
  Parse *pParse,     /* The parsing context */
  Token *pBegin,     /* The CREATE token that begins the statement */
  Token *pName1,     /* The token that holds the name of the view */
  Token *pName2,     /* The token that holds the name of the view */
  ExprList *pCNames, /* Optional list of view column names */
  Select *pSelect,   /* A SELECT statement that will become the new view */
  int isTemp,        /* TRUE for a TEMPORARY view */
  int noErr          /* Suppress error messages if VIEW already exists */
){
  Table *p;
  int n;
  const char *z;
  Token sEnd;
  DbFixer sFix;
  Token *pName = 0;
  int iDb;
  sqlite3 *db = pParse->db;

  if( pParse->nVar>0 ){
    sqlite3ErrorMsg(pParse, "parameters are not allowed in views");
    goto create_view_fail;
  }
  sqlite3StartTable(pParse, pName1, pName2, isTemp, 1, 0, noErr);
  p = pParse->pNewTable;
  if( p==0 || pParse->nErr ) goto create_view_fail;

  /* Legacy versions of SQLite allowed the use of the magic "rowid" column
  ** on a view, even though views do not have rowids.  The following flag
  ** setting fixes this problem.  But the fix can be disabled by compiling
  ** with -DSQLITE_ALLOW_ROWID_IN_VIEW in case there are legacy apps that
  ** depend upon the old buggy behavior.  The ability can also be toggled
  ** using sqlite3_config(SQLITE_CONFIG_ROWID_IN_VIEW,...) */
#ifdef SQLITE_ALLOW_ROWID_IN_VIEW
  p->tabFlags |= sqlite3Config.mNoVisibleRowid; /* Optional. Allow by default */
#else
  p->tabFlags |= TF_NoVisibleRowid;             /* Never allow rowid in view */
#endif

  sqlite3TwoPartName(pParse, pName1, pName2, &pName);
  iDb = sqlite3SchemaToIndex(db, p->pSchema);
  sqlite3FixInit(&sFix, pParse, iDb, "view", pName);
  if( sqlite3FixSelect(&sFix, pSelect) ) goto create_view_fail;

  /* Make a copy of the entire SELECT statement that defines the view.
  ** This will force all the Expr.token.z values to be dynamically
  ** allocated rather than point to the input string - which means that
  ** they will persist after the current sqlite3_exec() call returns.
  */
  pSelect->selFlags |= SF_View;
  if( IN_RENAME_OBJECT ){
    p->u.view.pSelect = pSelect;
    pSelect = 0;
  }else{
    p->u.view.pSelect = sqlite3SelectDup(db, pSelect, EXPRDUP_REDUCE);
  }
  p->pCheck = sqlite3ExprListDup(db, pCNames, EXPRDUP_REDUCE);
  p->eTabType = TABTYP_VIEW;
  if( db->mallocFailed ) goto create_view_fail;

  /* Locate the end of the CREATE VIEW statement.  Make sEnd point to
  ** the end.
  */
  sEnd = pParse->sLastToken;
  assert( sEnd.z[0]!=0 || sEnd.n==0 );
  if( sEnd.z[0]!=';' ){
    sEnd.z += sEnd.n;
  }
  sEnd.n = 0;
  n = (int)(sEnd.z - pBegin->z);
  assert( n>0 );
  z = pBegin->z;
  while( sqlite3Isspace(z[n-1]) ){ n--; }
  sEnd.z = &z[n-1];
  sEnd.n = 1;

  /* Use sqlite3EndTable() to add the view to the schema table */
  sqlite3EndTable(pParse, 0, &sEnd, 0, 0);

create_view_fail:
  sqlite3SelectDelete(db, pSelect);
  if( IN_RENAME_OBJECT ){
    sqlite3RenameExprlistUnmap(pParse, pCNames);
  }
  sqlite3ExprListDelete(db, pCNames);
  return;
}
#endif /* SQLITE_OMIT_VIEW */

#if !defined(SQLITE_OMIT_VIEW) || !defined(SQLITE_OMIT_VIRTUALTABLE)
/*
** The Table structure pTable is really a VIEW.  Fill in the names of
** the columns of the view in the pTable structure.  Return non-zero if
** there are errors.  If an error is seen an error message is left
** in pParse->zErrMsg.
*/
static SQLITE_NOINLINE int viewGetColumnNames(Parse *pParse, Table *pTable){
  Table *pSelTab;   /* A fake table from which we get the result set */
  Select *pSel;     /* Copy of the SELECT that implements the view */
  int nErr = 0;     /* Number of errors encountered */
  sqlite3 *db = pParse->db;  /* Database connection for malloc errors */
#ifndef SQLITE_OMIT_VIRTUALTABLE
  int rc;
#endif
#ifndef SQLITE_OMIT_AUTHORIZATION
  sqlite3_xauth xAuth;       /* Saved xAuth pointer */
#endif

  assert( pTable );

#ifndef SQLITE_OMIT_VIRTUALTABLE
  if( IsVirtual(pTable) ){
    db->nSchemaLock++;
    rc = sqlite3VtabCallConnect(pParse, pTable);
    db->nSchemaLock--;
    return rc;
  }
#endif

#ifndef SQLITE_OMIT_VIEW
  /* A positive nCol means the columns names for this view are
  ** already known.  This routine is not called unless either the
  ** table is virtual or nCol is zero.
  */
  assert( pTable->nCol<=0 );

  /* A negative nCol is a special marker meaning that we are currently
  ** trying to compute the column names.  If we enter this routine with
  ** a negative nCol, it means two or more views form a loop, like this:
  **
  **     CREATE VIEW one AS SELECT * FROM two;
  **     CREATE VIEW two AS SELECT * FROM one;
  **
  ** Actually, the error above is now caught prior to reaching this point.
  ** But the following test is still important as it does come up
  ** in the following:
  **
  **     CREATE TABLE main.ex1(a);
  **     CREATE TEMP VIEW ex1 AS SELECT a FROM ex1;
  **     SELECT * FROM temp.ex1;
  */
  if( pTable->nCol<0 ){
    sqlite3ErrorMsg(pParse, "view %s is circularly defined", pTable->zName);
    return 1;
  }
  assert( pTable->nCol>=0 );

  /* If we get this far, it means we need to compute the table names.
  ** Note that the call to sqlite3ResultSetOfSelect() will expand any
  ** "*" elements in the results set of the view and will assign cursors
  ** to the elements of the FROM clause.  But we do not want these changes
  ** to be permanent.  So the computation is done on a copy of the SELECT
  ** statement that defines the view.
  */
  assert( IsView(pTable) );
  pSel = sqlite3SelectDup(db, pTable->u.view.pSelect, 0);
  if( pSel ){
    u8 eParseMode = pParse->eParseMode;
    int nTab = pParse->nTab;
    int nSelect = pParse->nSelect;
    pParse->eParseMode = PARSE_MODE_NORMAL;
    sqlite3SrcListAssignCursors(pParse, pSel->pSrc);
    pTable->nCol = -1;
    DisableLookaside;
#ifndef SQLITE_OMIT_AUTHORIZATION
    xAuth = db->xAuth;
    db->xAuth = 0;
    pSelTab = sqlite3ResultSetOfSelect(pParse, pSel, SQLITE_AFF_NONE);
    db->xAuth = xAuth;
#else
    pSelTab = sqlite3ResultSetOfSelect(pParse, pSel, SQLITE_AFF_NONE);
#endif
    pParse->nTab = nTab;
    pParse->nSelect = nSelect;
    if( pSelTab==0 ){
      pTable->nCol = 0;
      nErr++;
    }else if( pTable->pCheck ){
      /* CREATE VIEW name(arglist) AS ...
      ** The names of the columns in the table are taken from
      ** arglist which is stored in pTable->pCheck.  The pCheck field
      ** normally holds CHECK constraints on an ordinary table, but for
      ** a VIEW it holds the list of column names.
      */
      sqlite3ColumnsFromExprList(pParse, pTable->pCheck,
                                 &pTable->nCol, &pTable->aCol);
      if( pParse->nErr==0
       && pTable->nCol==pSel->pEList->nExpr
      ){
        assert( db->mallocFailed==0 );
        sqlite3SubqueryColumnTypes(pParse, pTable, pSel, SQLITE_AFF_NONE);
      }
    }else{
      /* CREATE VIEW name AS...  without an argument list.  Construct
      ** the column names from the SELECT statement that defines the view.
      */
      assert( pTable->aCol==0 );
      pTable->nCol = pSelTab->nCol;
      pTable->aCol = pSelTab->aCol;
      pTable->tabFlags |= (pSelTab->tabFlags & COLFLAG_NOINSERT);
      pSelTab->nCol = 0;
      pSelTab->aCol = 0;
      assert( sqlite3SchemaMutexHeld(db, 0, pTable->pSchema) );
    }
    pTable->nNVCol = pTable->nCol;
    sqlite3DeleteTable(db, pSelTab);
    sqlite3SelectDelete(db, pSel);
    EnableLookaside;
    pParse->eParseMode = eParseMode;
  } else {
    nErr++;
  }
  pTable->pSchema->schemaFlags |= DB_UnresetViews;
  if( db->mallocFailed ){
    sqlite3DeleteColumnNames(db, pTable);
  }
#endif /* SQLITE_OMIT_VIEW */
  return nErr + pParse->nErr;
}
SQLITE_PRIVATE int sqlite3ViewGetColumnNames(Parse *pParse, Table *pTable){
  assert( pTable!=0 );
  if( !IsVirtual(pTable) && pTable->nCol>0 ) return 0;
  return viewGetColumnNames(pParse, pTable);
}
#endif /* !defined(SQLITE_OMIT_VIEW) || !defined(SQLITE_OMIT_VIRTUALTABLE) */

#ifndef SQLITE_OMIT_VIEW
/*
** Clear the column names from every VIEW in database idx.
*/
static void sqliteViewResetAll(sqlite3 *db, int idx){
  HashElem *i;
  assert( sqlite3SchemaMutexHeld(db, idx, 0) );
  if( !DbHasProperty(db, idx, DB_UnresetViews) ) return;
  for(i=sqliteHashFirst(&db->aDb[idx].pSchema->tblHash); i;i=sqliteHashNext(i)){
    Table *pTab = sqliteHashData(i);
    if( IsView(pTab) ){
      sqlite3DeleteColumnNames(db, pTab);
    }
  }
  DbClearProperty(db, idx, DB_UnresetViews);
}
#else
# define sqliteViewResetAll(A,B)
#endif /* SQLITE_OMIT_VIEW */

/*
** This function is called by the VDBE to adjust the internal schema
** used by SQLite when the btree layer moves a table root page. The
** root-page of a table or index in database iDb has changed from iFrom
** to iTo.
**
** Ticket #1728:  The symbol table might still contain information
** on tables and/or indices that are the process of being deleted.
** If you are unlucky, one of those deleted indices or tables might
** have the same rootpage number as the real table or index that is
** being moved.  So we cannot stop searching after the first match
** because the first match might be for one of the deleted indices
** or tables and not the table/index that is actually being moved.
** We must continue looping until all tables and indices with
** rootpage==iFrom have been converted to have a rootpage of iTo
** in order to be certain that we got the right one.
*/
#ifndef SQLITE_OMIT_AUTOVACUUM
SQLITE_PRIVATE void sqlite3RootPageMoved(sqlite3 *db, int iDb, Pgno iFrom, Pgno iTo){
  HashElem *pElem;
  Hash *pHash;
  Db *pDb;

  assert( sqlite3SchemaMutexHeld(db, iDb, 0) );
  pDb = &db->aDb[iDb];
  pHash = &pDb->pSchema->tblHash;
  for(pElem=sqliteHashFirst(pHash); pElem; pElem=sqliteHashNext(pElem)){
    Table *pTab = sqliteHashData(pElem);
    if( pTab->tnum==iFrom ){
      pTab->tnum = iTo;
    }
  }
  pHash = &pDb->pSchema->idxHash;
  for(pElem=sqliteHashFirst(pHash); pElem; pElem=sqliteHashNext(pElem)){
    Index *pIdx = sqliteHashData(pElem);
    if( pIdx->tnum==iFrom ){
      pIdx->tnum = iTo;
    }
  }
}
#endif

/*
** Write code to erase the table with root-page iTable from database iDb.
** Also write code to modify the sqlite_schema table and internal schema
** if a root-page of another table is moved by the btree-layer whilst
** erasing iTable (this can happen with an auto-vacuum database).
*/
static void destroyRootPage(Parse *pParse, int iTable, int iDb){
  Vdbe *v = sqlite3GetVdbe(pParse);
  int r1 = sqlite3GetTempReg(pParse);
  if( iTable<2 ) sqlite3ErrorMsg(pParse, "corrupt schema");
  sqlite3VdbeAddOp3(v, OP_Destroy, iTable, r1, iDb);
  sqlite3MayAbort(pParse);
#ifndef SQLITE_OMIT_AUTOVACUUM
  /* OP_Destroy stores an in integer r1. If this integer
  ** is non-zero, then it is the root page number of a table moved to
  ** location iTable. The following code modifies the sqlite_schema table to
  ** reflect this.
  **
  ** The "#NNN" in the SQL is a special constant that means whatever value
  ** is in register NNN.  See grammar rules associated with the TK_REGISTER
  ** token for additional information.
  */
  sqlite3NestedParse(pParse,
     "UPDATE %Q." LEGACY_SCHEMA_TABLE
     " SET rootpage=%d WHERE #%d AND rootpage=#%d",
     pParse->db->aDb[iDb].zDbSName, iTable, r1, r1);
#endif
  sqlite3ReleaseTempReg(pParse, r1);
}

/*
** Write VDBE code to erase table pTab and all associated indices on disk.
** Code to update the sqlite_schema tables and internal schema definitions
** in case a root-page belonging to another table is moved by the btree layer
** is also added (this can happen with an auto-vacuum database).
*/
static void destroyTable(Parse *pParse, Table *pTab){
  /* If the database may be auto-vacuum capable (if SQLITE_OMIT_AUTOVACUUM
  ** is not defined), then it is important to call OP_Destroy on the
  ** table and index root-pages in order, starting with the numerically
  ** largest root-page number. This guarantees that none of the root-pages
  ** to be destroyed is relocated by an earlier OP_Destroy. i.e. if the
  ** following were coded:
  **
  ** OP_Destroy 4 0
  ** ...
  ** OP_Destroy 5 0
  **
  ** and root page 5 happened to be the largest root-page number in the
  ** database, then root page 5 would be moved to page 4 by the
  ** "OP_Destroy 4 0" opcode. The subsequent "OP_Destroy 5 0" would hit
  ** a free-list page.
  */
  Pgno iTab = pTab->tnum;
  Pgno iDestroyed = 0;

  while( 1 ){
    Index *pIdx;
    Pgno iLargest = 0;

    if( iDestroyed==0 || iTab<iDestroyed ){
      iLargest = iTab;
    }
    for(pIdx=pTab->pIndex; pIdx; pIdx=pIdx->pNext){
      Pgno iIdx = pIdx->tnum;
      assert( pIdx->pSchema==pTab->pSchema );
      if( (iDestroyed==0 || (iIdx<iDestroyed)) && iIdx>iLargest ){
        iLargest = iIdx;
      }
    }
    if( iLargest==0 ){
      return;
    }else{
      int iDb = sqlite3SchemaToIndex(pParse->db, pTab->pSchema);
      assert( iDb>=0 && iDb<pParse->db->nDb );
      destroyRootPage(pParse, iLargest, iDb);
      iDestroyed = iLargest;
    }
  }
}
