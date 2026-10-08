#endif /* !defined(SQLITE_OMIT_SUBQUERY) || !defined(SQLITE_OMIT_VIEW) */

/*
** Check to see if a subquery contains result-set columns that are
** never used.  If it does, change the value of those result-set columns
** to NULL so that they do not cause unnecessary work to compute.
**
** Return the number of column that were changed to NULL.
*/
static int disableUnusedSubqueryResultColumns(SrcItem *pItem){
  int nCol;
  Select *pSub;      /* The subquery to be simplified */
  Select *pX;        /* For looping over compound elements of pSub */
  Table *pTab;       /* The table that describes the subquery */
  int j;             /* Column number */
  int nChng = 0;     /* Number of columns converted to NULL */
  Bitmask colUsed;   /* Columns that may not be NULLed out */

  assert( pItem!=0 );
  if( pItem->fg.isCorrelated || pItem->fg.isCte ){
    return 0;
  }
  assert( pItem->pTab!=0 );
  pTab = pItem->pTab;
  assert( pItem->pSelect!=0 );
  pSub = pItem->pSelect;
  assert( pSub->pEList->nExpr==pTab->nCol );
  for(pX=pSub; pX; pX=pX->pPrior){
    if( (pX->selFlags & (SF_Distinct|SF_Aggregate))!=0 ){
      testcase( pX->selFlags & SF_Distinct );
      testcase( pX->selFlags & SF_Aggregate );
      return 0;
    }
    if( pX->pPrior && pX->op!=TK_ALL ){
      /* This optimization does not work for compound subqueries that
      ** use UNION, INTERSECT, or EXCEPT.  Only UNION ALL is allowed. */
      return 0;
    }
#ifndef SQLITE_OMIT_WINDOWFUNC
    if( pX->pWin ){
      /* This optimization does not work for subqueries that use window
      ** functions. */
      return 0;
    }
#endif
  }
  colUsed = pItem->colUsed;
  if( pSub->pOrderBy ){
    ExprList *pList = pSub->pOrderBy;
    for(j=0; j<pList->nExpr; j++){
      u16 iCol = pList->a[j].u.x.iOrderByCol;
      if( iCol>0 ){
        iCol--;
        colUsed |= ((Bitmask)1)<<(iCol>=BMS ? BMS-1 : iCol);
      }
    }
  }
  nCol = pTab->nCol;
  for(j=0; j<nCol; j++){
    Bitmask m = j<BMS-1 ? MASKBIT(j) : TOPBIT;
    if( (m & colUsed)!=0 ) continue;
    for(pX=pSub; pX; pX=pX->pPrior) {
      Expr *pY = pX->pEList->a[j].pExpr;
      if( pY->op==TK_NULL ) continue;
      pY->op = TK_NULL;
      ExprClearProperty(pY, EP_Skip|EP_Unlikely);
      pX->selFlags |= SF_PushDown;
      nChng++;
    }
  }
  return nChng;
}


/*
** The pFunc is the only aggregate function in the query.  Check to see
** if the query is a candidate for the min/max optimization.
**
** If the query is a candidate for the min/max optimization, then set
** *ppMinMax to be an ORDER BY clause to be used for the optimization
** and return either WHERE_ORDERBY_MIN or WHERE_ORDERBY_MAX depending on
** whether pFunc is a min() or max() function.
**
** If the query is not a candidate for the min/max optimization, return
** WHERE_ORDERBY_NORMAL (which must be zero).
**
** This routine must be called after aggregate functions have been
** located but before their arguments have been subjected to aggregate
** analysis.
*/
static u8 minMaxQuery(sqlite3 *db, Expr *pFunc, ExprList **ppMinMax){
  int eRet = WHERE_ORDERBY_NORMAL;      /* Return value */
  ExprList *pEList;                     /* Arguments to agg function */
  const char *zFunc;                    /* Name of aggregate function pFunc */
  ExprList *pOrderBy;
  u8 sortFlags = 0;

  assert( *ppMinMax==0 );
  assert( pFunc->op==TK_AGG_FUNCTION );
  assert( !IsWindowFunc(pFunc) );
  assert( ExprUseXList(pFunc) );
  pEList = pFunc->x.pList;
  if( pEList==0
   || pEList->nExpr!=1
   || ExprHasProperty(pFunc, EP_WinFunc)
   || OptimizationDisabled(db, SQLITE_MinMaxOpt)
  ){
    return eRet;
  }
  assert( !ExprHasProperty(pFunc, EP_IntValue) );
  zFunc = pFunc->u.zToken;
  if( sqlite3StrICmp(zFunc, "min")==0 ){
    eRet = WHERE_ORDERBY_MIN;
    if( sqlite3ExprCanBeNull(pEList->a[0].pExpr) ){
      sortFlags = KEYINFO_ORDER_BIGNULL;
    }
  }else if( sqlite3StrICmp(zFunc, "max")==0 ){
    eRet = WHERE_ORDERBY_MAX;
    sortFlags = KEYINFO_ORDER_DESC;
  }else{
    return eRet;
  }
  *ppMinMax = pOrderBy = sqlite3ExprListDup(db, pEList, 0);
  assert( pOrderBy!=0 || db->mallocFailed );
  if( pOrderBy ) pOrderBy->a[0].fg.sortFlags = sortFlags;
  return eRet;
}

/*
** The select statement passed as the first argument is an aggregate query.
** The second argument is the associated aggregate-info object. This
** function tests if the SELECT is of the form:
**
**   SELECT count(*) FROM <tbl>
**
** where table is a database table, not a sub-select or view. If the query
** does match this pattern, then a pointer to the Table object representing
** <tbl> is returned. Otherwise, NULL is returned.
**
** This routine checks to see if it is safe to use the count optimization.
** A correct answer is still obtained (though perhaps more slowly) if
** this routine returns NULL when it could have returned a table pointer.
** But returning the pointer when NULL should have been returned can
** result in incorrect answers and/or crashes.  So, when in doubt, return NULL.
*/
static Table *isSimpleCount(Select *p, AggInfo *pAggInfo){
  Table *pTab;
  Expr *pExpr;

  assert( !p->pGroupBy );

  if( p->pWhere
   || p->pEList->nExpr!=1
   || p->pSrc->nSrc!=1
   || p->pSrc->a[0].pSelect
   || pAggInfo->nFunc!=1
   || p->pHaving
  ){
    return 0;
  }
  pTab = p->pSrc->a[0].pTab;
  assert( pTab!=0 );
  assert( !IsView(pTab) );
  if( !IsOrdinaryTable(pTab) ) return 0;
  pExpr = p->pEList->a[0].pExpr;
  assert( pExpr!=0 );
  if( pExpr->op!=TK_AGG_FUNCTION ) return 0;
  if( pExpr->pAggInfo!=pAggInfo ) return 0;
  if( (pAggInfo->aFunc[0].pFunc->funcFlags&SQLITE_FUNC_COUNT)==0 ) return 0;
  assert( pAggInfo->aFunc[0].pFExpr==pExpr );
  testcase( ExprHasProperty(pExpr, EP_Distinct) );
  testcase( ExprHasProperty(pExpr, EP_WinFunc) );
  if( ExprHasProperty(pExpr, EP_Distinct|EP_WinFunc) ) return 0;

  return pTab;
}

/*
** If the source-list item passed as an argument was augmented with an
** INDEXED BY clause, then try to locate the specified index. If there
** was such a clause and the named index cannot be found, return
** SQLITE_ERROR and leave an error in pParse. Otherwise, populate
** pFrom->pIndex and return SQLITE_OK.
*/
SQLITE_PRIVATE int sqlite3IndexedByLookup(Parse *pParse, SrcItem *pFrom){
  Table *pTab = pFrom->pTab;
  char *zIndexedBy = pFrom->u1.zIndexedBy;
  Index *pIdx;
  assert( pTab!=0 );
  assert( pFrom->fg.isIndexedBy!=0 );

  for(pIdx=pTab->pIndex;
      pIdx && sqlite3StrICmp(pIdx->zName, zIndexedBy);
      pIdx=pIdx->pNext
  );
  if( !pIdx ){
    sqlite3ErrorMsg(pParse, "no such index: %s", zIndexedBy, 0);
    pParse->checkSchema = 1;
    return SQLITE_ERROR;
  }
  assert( pFrom->fg.isCte==0 );
  pFrom->u2.pIBIndex = pIdx;
  return SQLITE_OK;
}

/*
** Detect compound SELECT statements that use an ORDER BY clause with
** an alternative collating sequence.
**
**    SELECT ... FROM t1 EXCEPT SELECT ... FROM t2 ORDER BY .. COLLATE ...
**
** These are rewritten as a subquery:
**
**    SELECT * FROM (SELECT ... FROM t1 EXCEPT SELECT ... FROM t2)
**     ORDER BY ... COLLATE ...
**
** This transformation is necessary because the multiSelectOrderBy() routine
** above that generates the code for a compound SELECT with an ORDER BY clause
** uses a merge algorithm that requires the same collating sequence on the
** result columns as on the ORDER BY clause.  See ticket
** http://www.sqlite.org/src/info/6709574d2a
**
** This transformation is only needed for EXCEPT, INTERSECT, and UNION.
** The UNION ALL operator works fine with multiSelectOrderBy() even when
** there are COLLATE terms in the ORDER BY.
*/
static int convertCompoundSelectToSubquery(Walker *pWalker, Select *p){
  int i;
  Select *pNew;
  Select *pX;
  sqlite3 *db;
  struct ExprList_item *a;
  SrcList *pNewSrc;
  Parse *pParse;
  Token dummy;

  if( p->pPrior==0 ) return WRC_Continue;
  if( p->pOrderBy==0 ) return WRC_Continue;
  for(pX=p; pX && (pX->op==TK_ALL || pX->op==TK_SELECT); pX=pX->pPrior){}
  if( pX==0 ) return WRC_Continue;
  a = p->pOrderBy->a;
#ifndef SQLITE_OMIT_WINDOWFUNC
  /* If iOrderByCol is already non-zero, then it has already been matched
  ** to a result column of the SELECT statement. This occurs when the
  ** SELECT is rewritten for window-functions processing and then passed
  ** to sqlite3SelectPrep() and similar a second time. The rewriting done
  ** by this function is not required in this case. */
  if( a[0].u.x.iOrderByCol ) return WRC_Continue;
#endif
  for(i=p->pOrderBy->nExpr-1; i>=0; i--){
    if( a[i].pExpr->flags & EP_Collate ) break;
  }
  if( i<0 ) return WRC_Continue;

  /* If we reach this point, that means the transformation is required. */

  pParse = pWalker->pParse;
  db = pParse->db;
  pNew = sqlite3DbMallocZero(db, sizeof(*pNew) );
  if( pNew==0 ) return WRC_Abort;
  memset(&dummy, 0, sizeof(dummy));
  pNewSrc = sqlite3SrcListAppendFromTerm(pParse,0,0,0,&dummy,pNew,0);
  if( pNewSrc==0 ) return WRC_Abort;
  *pNew = *p;
  p->pSrc = pNewSrc;
  p->pEList = sqlite3ExprListAppend(pParse, 0, sqlite3Expr(db, TK_ASTERISK, 0));
  p->op = TK_SELECT;
  p->pWhere = 0;
  pNew->pGroupBy = 0;
  pNew->pHaving = 0;
  pNew->pOrderBy = 0;
  p->pPrior = 0;
  p->pNext = 0;
  p->pWith = 0;
#ifndef SQLITE_OMIT_WINDOWFUNC
  p->pWinDefn = 0;
#endif
  p->selFlags &= ~SF_Compound;
  assert( (p->selFlags & SF_Converted)==0 );
  p->selFlags |= SF_Converted;
  assert( pNew->pPrior!=0 );
  pNew->pPrior->pNext = pNew;
  pNew->pLimit = 0;
  return WRC_Continue;
}

/*
** Check to see if the FROM clause term pFrom has table-valued function
** arguments.  If it does, leave an error message in pParse and return
** non-zero, since pFrom is not allowed to be a table-valued function.
*/
static int cannotBeFunction(Parse *pParse, SrcItem *pFrom){
  if( pFrom->fg.isTabFunc ){
    sqlite3ErrorMsg(pParse, "'%s' is not a function", pFrom->zName);
    return 1;
  }
  return 0;
}

#ifndef SQLITE_OMIT_CTE
/*
** Argument pWith (which may be NULL) points to a linked list of nested
** WITH contexts, from inner to outermost. If the table identified by
** FROM clause element pItem is really a common-table-expression (CTE)
** then return a pointer to the CTE definition for that table. Otherwise
** return NULL.
**
** If a non-NULL value is returned, set *ppContext to point to the With
** object that the returned CTE belongs to.
*/
static struct Cte *searchWith(
  With *pWith,                    /* Current innermost WITH clause */
  SrcItem *pItem,                 /* FROM clause element to resolve */
  With **ppContext                /* OUT: WITH clause return value belongs to */
){
  const char *zName = pItem->zName;
  With *p;
  assert( pItem->zDatabase==0 );
  assert( zName!=0 );
  for(p=pWith; p; p=p->pOuter){
    int i;
    for(i=0; i<p->nCte; i++){
      if( sqlite3StrICmp(zName, p->a[i].zName)==0 ){
        *ppContext = p;
        return &p->a[i];
      }
    }
    if( p->bView ) break;
  }
  return 0;
}

/* The code generator maintains a stack of active WITH clauses
** with the inner-most WITH clause being at the top of the stack.
**
** This routine pushes the WITH clause passed as the second argument
** onto the top of the stack. If argument bFree is true, then this
** WITH clause will never be popped from the stack but should instead
** be freed along with the Parse object. In other cases, when
** bFree==0, the With object will be freed along with the SELECT
** statement with which it is associated.
**
** This routine returns a copy of pWith.  Or, if bFree is true and
** the pWith object is destroyed immediately due to an OOM condition,
** then this routine return NULL.
**
** If bFree is true, do not continue to use the pWith pointer after
** calling this routine,  Instead, use only the return value.
*/
SQLITE_PRIVATE With *sqlite3WithPush(Parse *pParse, With *pWith, u8 bFree){
  if( pWith ){
    if( bFree ){
      pWith = (With*)sqlite3ParserAddCleanup(pParse, sqlite3WithDeleteGeneric,
                      pWith);
      if( pWith==0 ) return 0;
    }
    if( pParse->nErr==0 ){
      assert( pParse->pWith!=pWith );
      pWith->pOuter = pParse->pWith;
      pParse->pWith = pWith;
    }
  }
  return pWith;
}
