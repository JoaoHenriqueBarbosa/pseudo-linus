
/*
** Context object passed through sqlite3WalkExprList() to
** selectWindowRewriteExprCb() by selectWindowRewriteEList().
*/
typedef struct WindowRewrite WindowRewrite;
struct WindowRewrite {
  Window *pWin;
  SrcList *pSrc;
  ExprList *pSub;
  Table *pTab;
  Select *pSubSelect;             /* Current sub-select, if any */
};

/*
** Callback function used by selectWindowRewriteEList(). If necessary,
** this function appends to the output expression-list and updates
** expression (*ppExpr) in place.
*/
static int selectWindowRewriteExprCb(Walker *pWalker, Expr *pExpr){
  struct WindowRewrite *p = pWalker->u.pRewrite;
  Parse *pParse = pWalker->pParse;
  assert( p!=0 );
  assert( p->pWin!=0 );

  /* If this function is being called from within a scalar sub-select
  ** that used by the SELECT statement being processed, only process
  ** TK_COLUMN expressions that refer to it (the outer SELECT). Do
  ** not process aggregates or window functions at all, as they belong
  ** to the scalar sub-select.  */
  if( p->pSubSelect ){
    if( pExpr->op!=TK_COLUMN ){
      return WRC_Continue;
    }else{
      int nSrc = p->pSrc->nSrc;
      int i;
      for(i=0; i<nSrc; i++){
        if( pExpr->iTable==p->pSrc->a[i].iCursor ) break;
      }
      if( i==nSrc ) return WRC_Continue;
    }
  }

  switch( pExpr->op ){

    case TK_FUNCTION:
      if( !ExprHasProperty(pExpr, EP_WinFunc) ){
        break;
      }else{
        Window *pWin;
        for(pWin=p->pWin; pWin; pWin=pWin->pNextWin){
          if( pExpr->y.pWin==pWin ){
            assert( pWin->pOwner==pExpr );
            return WRC_Prune;
          }
        }
      }
      /* no break */ deliberate_fall_through

    case TK_IF_NULL_ROW:
    case TK_AGG_FUNCTION:
    case TK_COLUMN: {
      int iCol = -1;
      if( pParse->db->mallocFailed ) return WRC_Abort;
      if( p->pSub ){
        int i;
        for(i=0; i<p->pSub->nExpr; i++){
          if( 0==sqlite3ExprCompare(0, p->pSub->a[i].pExpr, pExpr, -1) ){
            iCol = i;
            break;
          }
        }
      }
      if( iCol<0 ){
        Expr *pDup = sqlite3ExprDup(pParse->db, pExpr, 0);
        if( pDup && pDup->op==TK_AGG_FUNCTION ) pDup->op = TK_FUNCTION;
        p->pSub = sqlite3ExprListAppend(pParse, p->pSub, pDup);
      }
      if( p->pSub ){
        int f = pExpr->flags & EP_Collate;
        assert( ExprHasProperty(pExpr, EP_Static)==0 );
        ExprSetProperty(pExpr, EP_Static);
        sqlite3ExprDelete(pParse->db, pExpr);
        ExprClearProperty(pExpr, EP_Static);
        memset(pExpr, 0, sizeof(Expr));

        pExpr->op = TK_COLUMN;
        pExpr->iColumn = (iCol<0 ? p->pSub->nExpr-1: iCol);
        pExpr->iTable = p->pWin->iEphCsr;
        pExpr->y.pTab = p->pTab;
        pExpr->flags = f;
      }
      if( pParse->db->mallocFailed ) return WRC_Abort;
      break;
    }

    default: /* no-op */
      break;
  }

  return WRC_Continue;
}
static int selectWindowRewriteSelectCb(Walker *pWalker, Select *pSelect){
  struct WindowRewrite *p = pWalker->u.pRewrite;
  Select *pSave = p->pSubSelect;
  if( pSave==pSelect ){
    return WRC_Continue;
  }else{
    p->pSubSelect = pSelect;
    sqlite3WalkSelect(pWalker, pSelect);
    p->pSubSelect = pSave;
  }
  return WRC_Prune;
}


/*
** Iterate through each expression in expression-list pEList. For each:
**
**   * TK_COLUMN,
**   * aggregate function, or
**   * window function with a Window object that is not a member of the
**     Window list passed as the second argument (pWin).
**
** Append the node to output expression-list (*ppSub). And replace it
** with a TK_COLUMN that reads the (N-1)th element of table
** pWin->iEphCsr, where N is the number of elements in (*ppSub) after
** appending the new one.
*/
static void selectWindowRewriteEList(
  Parse *pParse,
  Window *pWin,
  SrcList *pSrc,
  ExprList *pEList,               /* Rewrite expressions in this list */
  Table *pTab,
  ExprList **ppSub                /* IN/OUT: Sub-select expression-list */
){
  Walker sWalker;
  WindowRewrite sRewrite;

  assert( pWin!=0 );
  memset(&sWalker, 0, sizeof(Walker));
  memset(&sRewrite, 0, sizeof(WindowRewrite));

  sRewrite.pSub = *ppSub;
  sRewrite.pWin = pWin;
  sRewrite.pSrc = pSrc;
  sRewrite.pTab = pTab;

  sWalker.pParse = pParse;
  sWalker.xExprCallback = selectWindowRewriteExprCb;
  sWalker.xSelectCallback = selectWindowRewriteSelectCb;
  sWalker.u.pRewrite = &sRewrite;

  (void)sqlite3WalkExprList(&sWalker, pEList);

  *ppSub = sRewrite.pSub;
}

/*
** Append a copy of each expression in expression-list pAppend to
** expression list pList. Return a pointer to the result list.
*/
static ExprList *exprListAppendList(
  Parse *pParse,          /* Parsing context */
  ExprList *pList,        /* List to which to append. Might be NULL */
  ExprList *pAppend,      /* List of values to append. Might be NULL */
  int bIntToNull
){
  if( pAppend ){
    int i;
    int nInit = pList ? pList->nExpr : 0;
    for(i=0; i<pAppend->nExpr; i++){
      sqlite3 *db = pParse->db;
      Expr *pDup = sqlite3ExprDup(db, pAppend->a[i].pExpr, 0);
      if( db->mallocFailed ){
        sqlite3ExprDelete(db, pDup);
        break;
      }
      if( bIntToNull ){
        int iDummy;
        Expr *pSub;
        pSub = sqlite3ExprSkipCollateAndLikely(pDup);
        if( sqlite3ExprIsInteger(pSub, &iDummy) ){
          pSub->op = TK_NULL;
          pSub->flags &= ~(EP_IntValue|EP_IsTrue|EP_IsFalse);
          pSub->u.zToken = 0;
        }
      }
      pList = sqlite3ExprListAppend(pParse, pList, pDup);
      if( pList ) pList->a[nInit+i].fg.sortFlags = pAppend->a[i].fg.sortFlags;
    }
  }
  return pList;
}

/*
** When rewriting a query, if the new subquery in the FROM clause
** contains TK_AGG_FUNCTION nodes that refer to an outer query,
** then we have to increase the Expr->op2 values of those nodes
** due to the extra subquery layer that was added.
**
** See also the incrAggDepth() routine in resolve.c
*/
static int sqlite3WindowExtraAggFuncDepth(Walker *pWalker, Expr *pExpr){
  if( pExpr->op==TK_AGG_FUNCTION
   && pExpr->op2>=pWalker->walkerDepth
  ){
    pExpr->op2++;
  }
  return WRC_Continue;
}

static int disallowAggregatesInOrderByCb(Walker *pWalker, Expr *pExpr){
  if( pExpr->op==TK_AGG_FUNCTION && pExpr->pAggInfo==0 ){
    assert( !ExprHasProperty(pExpr, EP_IntValue) );
     sqlite3ErrorMsg(pWalker->pParse,
         "misuse of aggregate: %s()", pExpr->u.zToken);
  }
  return WRC_Continue;
}

/*
** If the SELECT statement passed as the second argument does not invoke
** any SQL window functions, this function is a no-op. Otherwise, it
** rewrites the SELECT statement so that window function xStep functions
** are invoked in the correct order as described under "SELECT REWRITING"
** at the top of this file.
*/
SQLITE_PRIVATE int sqlite3WindowRewrite(Parse *pParse, Select *p){
  int rc = SQLITE_OK;
  if( p->pWin
   && p->pPrior==0
   && ALWAYS((p->selFlags & SF_WinRewrite)==0)
   && ALWAYS(!IN_RENAME_OBJECT)
  ){
    Vdbe *v = sqlite3GetVdbe(pParse);
    sqlite3 *db = pParse->db;
    Select *pSub = 0;             /* The subquery */
    SrcList *pSrc = p->pSrc;
    Expr *pWhere = p->pWhere;
    ExprList *pGroupBy = p->pGroupBy;
    Expr *pHaving = p->pHaving;
    ExprList *pSort = 0;

    ExprList *pSublist = 0;       /* Expression list for sub-query */
    Window *pMWin = p->pWin;      /* Main window object */
    Window *pWin;                 /* Window object iterator */
    Table *pTab;
    Walker w;

    u32 selFlags = p->selFlags;

    pTab = sqlite3DbMallocZero(db, sizeof(Table));
    if( pTab==0 ){
      return sqlite3ErrorToParser(db, SQLITE_NOMEM);
    }
    sqlite3AggInfoPersistWalkerInit(&w, pParse);
    sqlite3WalkSelect(&w, p);
    if( (p->selFlags & SF_Aggregate)==0 ){
      w.xExprCallback = disallowAggregatesInOrderByCb;
      w.xSelectCallback = 0;
      sqlite3WalkExprList(&w, p->pOrderBy);
    }

    p->pSrc = 0;
    p->pWhere = 0;
    p->pGroupBy = 0;
    p->pHaving = 0;
    p->selFlags &= ~SF_Aggregate;
    p->selFlags |= SF_WinRewrite;

    /* Create the ORDER BY clause for the sub-select. This is the concatenation
    ** of the window PARTITION and ORDER BY clauses. Then, if this makes it
    ** redundant, remove the ORDER BY from the parent SELECT.  */
    pSort = exprListAppendList(pParse, 0, pMWin->pPartition, 1);
    pSort = exprListAppendList(pParse, pSort, pMWin->pOrderBy, 1);
    if( pSort && p->pOrderBy && p->pOrderBy->nExpr<=pSort->nExpr ){
      int nSave = pSort->nExpr;
      pSort->nExpr = p->pOrderBy->nExpr;
      if( sqlite3ExprListCompare(pSort, p->pOrderBy, -1)==0 ){
        sqlite3ExprListDelete(db, p->pOrderBy);
        p->pOrderBy = 0;
      }
      pSort->nExpr = nSave;
    }

    /* Assign a cursor number for the ephemeral table used to buffer rows.
    ** The OpenEphemeral instruction is coded later, after it is known how
    ** many columns the table will have.  */
    pMWin->iEphCsr = pParse->nTab++;
    pParse->nTab += 3;

    selectWindowRewriteEList(pParse, pMWin, pSrc, p->pEList, pTab, &pSublist);
    selectWindowRewriteEList(pParse, pMWin, pSrc, p->pOrderBy, pTab, &pSublist);
    pMWin->nBufferCol = (pSublist ? pSublist->nExpr : 0);

    /* Append the PARTITION BY and ORDER BY expressions to the to the
    ** sub-select expression list. They are required to figure out where
    ** boundaries for partitions and sets of peer rows lie.  */
    pSublist = exprListAppendList(pParse, pSublist, pMWin->pPartition, 0);
    pSublist = exprListAppendList(pParse, pSublist, pMWin->pOrderBy, 0);

    /* Append the arguments passed to each window function to the
    ** sub-select expression list. Also allocate two registers for each
    ** window function - one for the accumulator, another for interim
    ** results.  */
    for(pWin=pMWin; pWin; pWin=pWin->pNextWin){
      ExprList *pArgs;
      assert( ExprUseXList(pWin->pOwner) );
      assert( pWin->pWFunc!=0 );
      pArgs = pWin->pOwner->x.pList;
      if( pWin->pWFunc->funcFlags & SQLITE_SUBTYPE ){
        selectWindowRewriteEList(pParse, pMWin, pSrc, pArgs, pTab, &pSublist);
        pWin->iArgCol = (pSublist ? pSublist->nExpr : 0);
        pWin->bExprArgs = 1;
      }else{
        pWin->iArgCol = (pSublist ? pSublist->nExpr : 0);
        pSublist = exprListAppendList(pParse, pSublist, pArgs, 0);
      }
      if( pWin->pFilter ){
        Expr *pFilter = sqlite3ExprDup(db, pWin->pFilter, 0);
        pSublist = sqlite3ExprListAppend(pParse, pSublist, pFilter);
      }
      pWin->regAccum = ++pParse->nMem;
      pWin->regResult = ++pParse->nMem;
      sqlite3VdbeAddOp2(v, OP_Null, 0, pWin->regAccum);
    }

    /* If there is no ORDER BY or PARTITION BY clause, and the window
    ** function accepts zero arguments, and there are no other columns
    ** selected (e.g. "SELECT row_number() OVER () FROM t1"), it is possible
    ** that pSublist is still NULL here. Add a constant expression here to
    ** keep everything legal in this case.
    */
    if( pSublist==0 ){
      pSublist = sqlite3ExprListAppend(pParse, 0,
        sqlite3Expr(db, TK_INTEGER, "0")
      );
    }

    pSub = sqlite3SelectNew(
        pParse, pSublist, pSrc, pWhere, pGroupBy, pHaving, pSort, 0, 0
    );
    TREETRACE(0x40,pParse,pSub,
       ("New window-function subquery in FROM clause of (%u/%p)\n",
       p->selId, p));
    p->pSrc = sqlite3SrcListAppend(pParse, 0, 0, 0);
    assert( pSub!=0 || p->pSrc==0 ); /* Due to db->mallocFailed test inside
                                     ** of sqlite3DbMallocRawNN() called from
                                     ** sqlite3SrcListAppend() */
    if( p->pSrc ){
      Table *pTab2;
      p->pSrc->a[0].pSelect = pSub;
      p->pSrc->a[0].fg.isCorrelated = 1;
      sqlite3SrcListAssignCursors(pParse, p->pSrc);
      pSub->selFlags |= SF_Expanded|SF_OrderByReqd;
      pTab2 = sqlite3ResultSetOfSelect(pParse, pSub, SQLITE_AFF_NONE);
      pSub->selFlags |= (selFlags & SF_Aggregate);
      if( pTab2==0 ){
        /* Might actually be some other kind of error, but in that case
        ** pParse->nErr will be set, so if SQLITE_NOMEM is set, we will get
        ** the correct error message regardless. */
        rc = SQLITE_NOMEM;
      }else{
        memcpy(pTab, pTab2, sizeof(Table));
        pTab->tabFlags |= TF_Ephemeral;
        p->pSrc->a[0].pTab = pTab;
        pTab = pTab2;
        memset(&w, 0, sizeof(w));
        w.xExprCallback = sqlite3WindowExtraAggFuncDepth;
        w.xSelectCallback = sqlite3WalkerDepthIncrease;
        w.xSelectCallback2 = sqlite3WalkerDepthDecrease;
        sqlite3WalkSelect(&w, pSub);
      }
    }else{
      sqlite3SelectDelete(db, pSub);
    }
    if( db->mallocFailed ) rc = SQLITE_NOMEM;

    /* Defer deleting the temporary table pTab because if an error occurred,
    ** there could still be references to that table embedded in the
    ** result-set or ORDER BY clause of the SELECT statement p.  */
    sqlite3ParserAddCleanup(pParse, sqlite3DbFree, pTab);
  }

  assert( rc==SQLITE_OK || pParse->nErr!=0 );
  return rc;
}
