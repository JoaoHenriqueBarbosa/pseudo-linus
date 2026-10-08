/*
** 2008 August 18
**
** The author disclaims copyright to this source code.  In place of
** a legal notice, here is a blessing:
**
**    May you do good and not evil.
**    May you find forgiveness for yourself and forgive others.
**    May you share freely, never taking more than you give.
**
*************************************************************************
**
** This file contains routines used for walking the parser tree and
** resolve all identifiers by associating them with a particular
** table and column.
*/
/* #include "sqliteInt.h" */

/*
** Magic table number to mean the EXCLUDED table in an UPSERT statement.
*/
#define EXCLUDED_TABLE_NUMBER  2

/*
** Walk the expression tree pExpr and increase the aggregate function
** depth (the Expr.op2 field) by N on every TK_AGG_FUNCTION node.
** This needs to occur when copying a TK_AGG_FUNCTION node from an
** outer query into an inner subquery.
**
** incrAggFunctionDepth(pExpr,n) is the main routine.  incrAggDepth(..)
** is a helper function - a callback for the tree walker.
**
** See also the sqlite3WindowExtraAggFuncDepth() routine in window.c
*/
static int incrAggDepth(Walker *pWalker, Expr *pExpr){
  if( pExpr->op==TK_AGG_FUNCTION ) pExpr->op2 += pWalker->u.n;
  return WRC_Continue;
}
static void incrAggFunctionDepth(Expr *pExpr, int N){
  if( N>0 ){
    Walker w;
    memset(&w, 0, sizeof(w));
    w.xExprCallback = incrAggDepth;
    w.u.n = N;
    sqlite3WalkExpr(&w, pExpr);
  }
}

/*
** Turn the pExpr expression into an alias for the iCol-th column of the
** result set in pEList.
**
** If the reference is followed by a COLLATE operator, then make sure
** the COLLATE operator is preserved.  For example:
**
**     SELECT a+b, c+d FROM t1 ORDER BY 1 COLLATE nocase;
**
** Should be transformed into:
**
**     SELECT a+b, c+d FROM t1 ORDER BY (a+b) COLLATE nocase;
**
** The nSubquery parameter specifies how many levels of subquery the
** alias is removed from the original expression.  The usual value is
** zero but it might be more if the alias is contained within a subquery
** of the original expression.  The Expr.op2 field of TK_AGG_FUNCTION
** structures must be increased by the nSubquery amount.
*/
static void resolveAlias(
  Parse *pParse,         /* Parsing context */
  ExprList *pEList,      /* A result set */
  int iCol,              /* A column in the result set.  0..pEList->nExpr-1 */
  Expr *pExpr,           /* Transform this into an alias to the result set */
  int nSubquery          /* Number of subqueries that the label is moving */
){
  Expr *pOrig;           /* The iCol-th column of the result set */
  Expr *pDup;            /* Copy of pOrig */
  sqlite3 *db;           /* The database connection */

  assert( iCol>=0 && iCol<pEList->nExpr );
  pOrig = pEList->a[iCol].pExpr;
  assert( pOrig!=0 );
  assert( !ExprHasProperty(pExpr, EP_Reduced|EP_TokenOnly) );
  if( pExpr->pAggInfo ) return;
  db = pParse->db;
  pDup = sqlite3ExprDup(db, pOrig, 0);
  if( db->mallocFailed ){
    sqlite3ExprDelete(db, pDup);
    pDup = 0;
  }else{
    Expr temp;
    incrAggFunctionDepth(pDup, nSubquery);
    if( pExpr->op==TK_COLLATE ){
      assert( !ExprHasProperty(pExpr, EP_IntValue) );
      pDup = sqlite3ExprAddCollateString(pParse, pDup, pExpr->u.zToken);
    }
    memcpy(&temp, pDup, sizeof(Expr));
    memcpy(pDup, pExpr, sizeof(Expr));
    memcpy(pExpr, &temp, sizeof(Expr));
    if( ExprHasProperty(pExpr, EP_WinFunc) ){
      if( ALWAYS(pExpr->y.pWin!=0) ){
        pExpr->y.pWin->pOwner = pExpr;
      }
    }
    sqlite3ExprDeferredDelete(pParse, pDup);
  }
}

/*
** Subqueries store the original database, table and column names for their
** result sets in ExprList.a[].zSpan, in the form "DATABASE.TABLE.COLUMN",
** and mark the expression-list item by setting ExprList.a[].fg.eEName
** to ENAME_TAB.
**
** Check to see if the zSpan/eEName of the expression-list item passed to this
** routine matches the zDb, zTab, and zCol.  If any of zDb, zTab, and zCol are
** NULL then those fields will match anything. Return true if there is a match,
** or false otherwise.
**
** SF_NestedFrom subqueries also store an entry for the implicit rowid (or
** _rowid_, or oid) column by setting ExprList.a[].fg.eEName to ENAME_ROWID,
** and setting zSpan to "DATABASE.TABLE.<rowid-alias>". This type of pItem
** argument matches if zCol is a rowid alias. If it is not NULL, (*pbRowid)
** is set to 1 if there is this kind of match.
*/
SQLITE_PRIVATE int sqlite3MatchEName(
  const struct ExprList_item *pItem,
  const char *zCol,
  const char *zTab,
  const char *zDb,
  int *pbRowid
){
  int n;
  const char *zSpan;
  int eEName = pItem->fg.eEName;
  if( eEName!=ENAME_TAB && (eEName!=ENAME_ROWID || NEVER(pbRowid==0)) ){
    return 0;
  }
  assert( pbRowid==0 || *pbRowid==0 );
  zSpan = pItem->zEName;
  for(n=0; ALWAYS(zSpan[n]) && zSpan[n]!='.'; n++){}
  if( zDb && (sqlite3StrNICmp(zSpan, zDb, n)!=0 || zDb[n]!=0) ){
    return 0;
  }
  zSpan += n+1;
  for(n=0; ALWAYS(zSpan[n]) && zSpan[n]!='.'; n++){}
  if( zTab && (sqlite3StrNICmp(zSpan, zTab, n)!=0 || zTab[n]!=0) ){
    return 0;
  }
  zSpan += n+1;
  if( zCol ){
    if( eEName==ENAME_TAB && sqlite3StrICmp(zSpan, zCol)!=0 ) return 0;
    if( eEName==ENAME_ROWID && sqlite3IsRowid(zCol)==0 ) return 0;
  }
  if( eEName==ENAME_ROWID ) *pbRowid = 1;
  return 1;
}

/*
** Return TRUE if the double-quoted string  mis-feature should be supported.
*/
static int areDoubleQuotedStringsEnabled(sqlite3 *db, NameContext *pTopNC){
  if( db->init.busy ) return 1;  /* Always support for legacy schemas */
  if( pTopNC->ncFlags & NC_IsDDL ){
    /* Currently parsing a DDL statement */
    if( sqlite3WritableSchema(db) && (db->flags & SQLITE_DqsDML)!=0 ){
      return 1;
    }
    return (db->flags & SQLITE_DqsDDL)!=0;
  }else{
    /* Currently parsing a DML statement */
    return (db->flags & SQLITE_DqsDML)!=0;
  }
}

/*
** The argument is guaranteed to be a non-NULL Expr node of type TK_COLUMN.
** return the appropriate colUsed mask.
*/
SQLITE_PRIVATE Bitmask sqlite3ExprColUsed(Expr *pExpr){
  int n;
  Table *pExTab;

  n = pExpr->iColumn;
  assert( ExprUseYTab(pExpr) );
  pExTab = pExpr->y.pTab;
  assert( pExTab!=0 );
  assert( n < pExTab->nCol );
  if( (pExTab->tabFlags & TF_HasGenerated)!=0
   && (pExTab->aCol[n].colFlags & COLFLAG_GENERATED)!=0
  ){
    testcase( pExTab->nCol==BMS-1 );
    testcase( pExTab->nCol==BMS );
    return pExTab->nCol>=BMS ? ALLBITS : MASKBIT(pExTab->nCol)-1;
  }else{
    testcase( n==BMS-1 );
    testcase( n==BMS );
    if( n>=BMS ) n = BMS-1;
    return ((Bitmask)1)<<n;
  }
}

/*
** Create a new expression term for the column specified by pMatch and
** iColumn.  Append this new expression term to the FULL JOIN Match set
** in *ppList.  Create a new *ppList if this is the first term in the
** set.
*/
static void extendFJMatch(
  Parse *pParse,          /* Parsing context */
  ExprList **ppList,      /* ExprList to extend */
  SrcItem *pMatch,        /* Source table containing the column */
  i16 iColumn             /* The column number */
){
  Expr *pNew = sqlite3ExprAlloc(pParse->db, TK_COLUMN, 0, 0);
  if( pNew ){
    pNew->iTable = pMatch->iCursor;
    pNew->iColumn = iColumn;
    pNew->y.pTab = pMatch->pTab;
    assert( (pMatch->fg.jointype & (JT_LEFT|JT_LTORJ))!=0 );
    ExprSetProperty(pNew, EP_CanBeNull);
    *ppList = sqlite3ExprListAppend(pParse, *ppList, pNew);
  }
}

/*
** Return TRUE (non-zero) if zTab is a valid name for the schema table pTab.
*/
static SQLITE_NOINLINE int isValidSchemaTableName(
  const char *zTab,         /* Name as it appears in the SQL */
  Table *pTab,              /* The schema table we are trying to match */
  const char *zDb           /* non-NULL if a database qualifier is present */
){
  const char *zLegacy;
  assert( pTab!=0 );
  assert( pTab->tnum==1 );
  if( sqlite3StrNICmp(zTab, "sqlite_", 7)!=0 ) return 0;
  zLegacy = pTab->zName;
  if( strcmp(zLegacy+7, &LEGACY_TEMP_SCHEMA_TABLE[7])==0 ){
    if( sqlite3StrICmp(zTab+7, &PREFERRED_TEMP_SCHEMA_TABLE[7])==0 ){
      return 1;
    }
    if( zDb==0 ) return 0;
    if( sqlite3StrICmp(zTab+7, &LEGACY_SCHEMA_TABLE[7])==0 ) return 1;
    if( sqlite3StrICmp(zTab+7, &PREFERRED_SCHEMA_TABLE[7])==0 ) return 1;
  }else{
    if( sqlite3StrICmp(zTab+7, &PREFERRED_SCHEMA_TABLE[7])==0 ) return 1;
  }
  return 0;
}

/*
** Given the name of a column of the form X.Y.Z or Y.Z or just Z, look up
** that name in the set of source tables in pSrcList and make the pExpr
** expression node refer back to that source column.  The following changes
** are made to pExpr:
**
**    pExpr->iDb           Set the index in db->aDb[] of the database X
**                         (even if X is implied).
**    pExpr->iTable        Set to the cursor number for the table obtained
**                         from pSrcList.
**    pExpr->y.pTab        Points to the Table structure of X.Y (even if
**                         X and/or Y are implied.)
**    pExpr->iColumn       Set to the column number within the table.
**    pExpr->op            Set to TK_COLUMN.
**    pExpr->pLeft         Any expression this points to is deleted
**    pExpr->pRight        Any expression this points to is deleted.
**
** The zDb variable is the name of the database (the "X").  This value may be
** NULL meaning that name is of the form Y.Z or Z.  Any available database
** can be used.  The zTable variable is the name of the table (the "Y").  This
** value can be NULL if zDb is also NULL.  If zTable is NULL it
** means that the form of the name is Z and that columns from any table
** can be used.
**
** If the name cannot be resolved unambiguously, leave an error message
** in pParse and return WRC_Abort.  Return WRC_Prune on success.
*/
static int lookupName(
  Parse *pParse,       /* The parsing context */
  const char *zDb,     /* Name of the database containing table, or NULL */
  const char *zTab,    /* Name of table containing column, or NULL */
  const Expr *pRight,  /* Name of the column. */
  NameContext *pNC,    /* The name context used to resolve the name */
  Expr *pExpr          /* Make this EXPR node point to the selected column */
){
  int i, j;                         /* Loop counters */
  int cnt = 0;                      /* Number of matching column names */
  int cntTab = 0;                   /* Number of potential "rowid" matches */
  int nSubquery = 0;                /* How many levels of subquery */
  sqlite3 *db = pParse->db;         /* The database connection */
  SrcItem *pItem;                   /* Use for looping over pSrcList items */
  SrcItem *pMatch = 0;              /* The matching pSrcList item */
  NameContext *pTopNC = pNC;        /* First namecontext in the list */
  Schema *pSchema = 0;              /* Schema of the expression */
  int eNewExprOp = TK_COLUMN;       /* New value for pExpr->op on success */
  Table *pTab = 0;                  /* Table holding the row */
  Column *pCol;                     /* A column of pTab */
  ExprList *pFJMatch = 0;           /* Matches for FULL JOIN .. USING */
  const char *zCol = pRight->u.zToken;

  assert( pNC );     /* the name context cannot be NULL. */
  assert( zCol );    /* The Z in X.Y.Z cannot be NULL */
  assert( zDb==0 || zTab!=0 );
  assert( !ExprHasProperty(pExpr, EP_TokenOnly|EP_Reduced) );

  /* Initialize the node to no-match */
  pExpr->iTable = -1;
  ExprSetVVAProperty(pExpr, EP_NoReduce);

  /* Translate the schema name in zDb into a pointer to the corresponding
  ** schema.  If not found, pSchema will remain NULL and nothing will match
  ** resulting in an appropriate error message toward the end of this routine
  */
  if( zDb ){
    testcase( pNC->ncFlags & NC_PartIdx );
    testcase( pNC->ncFlags & NC_IsCheck );
    if( (pNC->ncFlags & (NC_PartIdx|NC_IsCheck))!=0 ){
      /* Silently ignore database qualifiers inside CHECK constraints and
      ** partial indices.  Do not raise errors because that might break
      ** legacy and because it does not hurt anything to just ignore the
      ** database name. */
      zDb = 0;
    }else{
      for(i=0; i<db->nDb; i++){
        assert( db->aDb[i].zDbSName );
        if( sqlite3StrICmp(db->aDb[i].zDbSName,zDb)==0 ){
          pSchema = db->aDb[i].pSchema;
          break;
        }
      }
      if( i==db->nDb && sqlite3StrICmp("main", zDb)==0 ){
        /* This branch is taken when the main database has been renamed
        ** using SQLITE_DBCONFIG_MAINDBNAME. */
        pSchema = db->aDb[0].pSchema;
        zDb = db->aDb[0].zDbSName;
      }
    }
  }

  /* Start at the inner-most context and move outward until a match is found */
  assert( pNC && cnt==0 );
  do{
    ExprList *pEList;
    SrcList *pSrcList = pNC->pSrcList;

    if( pSrcList ){
      for(i=0, pItem=pSrcList->a; i<pSrcList->nSrc; i++, pItem++){
        u8 hCol;
        pTab = pItem->pTab;
        assert( pTab!=0 && pTab->zName!=0 );
