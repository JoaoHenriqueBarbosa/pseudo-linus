
/***************************************************************************
** Routines with file scope above.  Interface to the rest of the where.c
** subsystem follows.
***************************************************************************/

/*
** This routine identifies subexpressions in the WHERE clause where
** each subexpression is separated by the AND operator or some other
** operator specified in the op parameter.  The WhereClause structure
** is filled with pointers to subexpressions.  For example:
**
**    WHERE  a=='hello' AND coalesce(b,11)<10 AND (c+12!=d OR c==22)
**           \________/     \_______________/     \________________/
**            slot[0]            slot[1]               slot[2]
**
** The original WHERE clause in pExpr is unaltered.  All this routine
** does is make slot[] entries point to substructure within pExpr.
**
** In the previous sentence and in the diagram, "slot[]" refers to
** the WhereClause.a[] array.  The slot[] array grows as needed to contain
** all terms of the WHERE clause.
*/
SQLITE_PRIVATE void sqlite3WhereSplit(WhereClause *pWC, Expr *pExpr, u8 op){
  Expr *pE2 = sqlite3ExprSkipCollateAndLikely(pExpr);
  pWC->op = op;
  assert( pE2!=0 || pExpr==0 );
  if( pE2==0 ) return;
  if( pE2->op!=op ){
    whereClauseInsert(pWC, pExpr, 0);
  }else{
    sqlite3WhereSplit(pWC, pE2->pLeft, op);
    sqlite3WhereSplit(pWC, pE2->pRight, op);
  }
}

/*
** Add either a LIMIT (if eMatchOp==SQLITE_INDEX_CONSTRAINT_LIMIT) or
** OFFSET (if eMatchOp==SQLITE_INDEX_CONSTRAINT_OFFSET) term to the
** where-clause passed as the first argument. The value for the term
** is found in register iReg.
**
** In the common case where the value is a simple integer
** (example: "LIMIT 5 OFFSET 10") then the expression codes as a
** TK_INTEGER so that it will be available to sqlite3_vtab_rhs_value().
** If not, then it codes as a TK_REGISTER expression.
*/
static void whereAddLimitExpr(
  WhereClause *pWC,   /* Add the constraint to this WHERE clause */
  int iReg,           /* Register that will hold value of the limit/offset */
  Expr *pExpr,        /* Expression that defines the limit/offset */
  int iCsr,           /* Cursor to which the constraint applies */
  int eMatchOp        /* SQLITE_INDEX_CONSTRAINT_LIMIT or _OFFSET */
){
  Parse *pParse = pWC->pWInfo->pParse;
  sqlite3 *db = pParse->db;
  Expr *pNew;
  int iVal = 0;

  if( sqlite3ExprIsInteger(pExpr, &iVal) && iVal>=0 ){
    Expr *pVal = sqlite3Expr(db, TK_INTEGER, 0);
    if( pVal==0 ) return;
    ExprSetProperty(pVal, EP_IntValue);
    pVal->u.iValue = iVal;
    pNew = sqlite3PExpr(pParse, TK_MATCH, 0, pVal);
  }else{
    Expr *pVal = sqlite3Expr(db, TK_REGISTER, 0);
    if( pVal==0 ) return;
    pVal->iTable = iReg;
    pNew = sqlite3PExpr(pParse, TK_MATCH, 0, pVal);
  }
  if( pNew ){
    WhereTerm *pTerm;
    int idx;
    idx = whereClauseInsert(pWC, pNew, TERM_DYNAMIC|TERM_VIRTUAL);
    pTerm = &pWC->a[idx];
    pTerm->leftCursor = iCsr;
    pTerm->eOperator = WO_AUX;
    pTerm->eMatchOp = eMatchOp;
  }
}

/*
** Possibly add terms corresponding to the LIMIT and OFFSET clauses of the
** SELECT statement passed as the second argument. These terms are only
** added if:
**
**   1. The SELECT statement has a LIMIT clause, and
**   2. The SELECT statement is not an aggregate or DISTINCT query, and
**   3. The SELECT statement has exactly one object in its from clause, and
**      that object is a virtual table, and
**   4. There are no terms in the WHERE clause that will not be passed
**      to the virtual table xBestIndex method.
**   5. The ORDER BY clause, if any, will be made available to the xBestIndex
**      method.
**
** LIMIT and OFFSET terms are ignored by most of the planner code. They
** exist only so that they may be passed to the xBestIndex method of the
** single virtual table in the FROM clause of the SELECT.
*/
SQLITE_PRIVATE void SQLITE_NOINLINE sqlite3WhereAddLimit(WhereClause *pWC, Select *p){
  assert( p!=0 && p->pLimit!=0 );                 /* 1 -- checked by caller */
  if( p->pGroupBy==0
   && (p->selFlags & (SF_Distinct|SF_Aggregate))==0             /* 2 */
   && (p->pSrc->nSrc==1 && IsVirtual(p->pSrc->a[0].pTab))       /* 3 */
  ){
    ExprList *pOrderBy = p->pOrderBy;
    int iCsr = p->pSrc->a[0].iCursor;
    int ii;

    /* Check condition (4). Return early if it is not met. */
    for(ii=0; ii<pWC->nTerm; ii++){
      if( pWC->a[ii].wtFlags & TERM_CODED ){
        /* This term is a vector operation that has been decomposed into
        ** other, subsequent terms.  It can be ignored. See tag-20220128a */
        assert( pWC->a[ii].wtFlags & TERM_VIRTUAL );
        assert( pWC->a[ii].eOperator==WO_ROWVAL );
        continue;
      }
      if( pWC->a[ii].nChild ){
        /* If this term has child terms, then they are also part of the
        ** pWC->a[] array. So this term can be ignored, as a LIMIT clause
        ** will only be added if each of the child terms passes the
        ** (leftCursor==iCsr) test below.  */
        continue;
      }
      if( pWC->a[ii].leftCursor!=iCsr ) return;
      if( pWC->a[ii].prereqRight!=0 ) return;
    }

    /* Check condition (5). Return early if it is not met. */
    if( pOrderBy ){
      for(ii=0; ii<pOrderBy->nExpr; ii++){
        Expr *pExpr = pOrderBy->a[ii].pExpr;
        if( pExpr->op!=TK_COLUMN ) return;
        if( pExpr->iTable!=iCsr ) return;
        if( pOrderBy->a[ii].fg.sortFlags & KEYINFO_ORDER_BIGNULL ) return;
      }
    }

    /* All conditions are met. Add the terms to the where-clause object. */
    assert( p->pLimit->op==TK_LIMIT );
    if( p->iOffset!=0 && (p->selFlags & SF_Compound)==0 ){
      whereAddLimitExpr(pWC, p->iOffset, p->pLimit->pRight,
                        iCsr, SQLITE_INDEX_CONSTRAINT_OFFSET);
    }
    if( p->iOffset==0 || (p->selFlags & SF_Compound)==0 ){
      whereAddLimitExpr(pWC, p->iLimit, p->pLimit->pLeft,
                        iCsr, SQLITE_INDEX_CONSTRAINT_LIMIT);
    }
  }
}

/*
** Initialize a preallocated WhereClause structure.
*/
SQLITE_PRIVATE void sqlite3WhereClauseInit(
  WhereClause *pWC,        /* The WhereClause to be initialized */
  WhereInfo *pWInfo        /* The WHERE processing context */
){
  pWC->pWInfo = pWInfo;
  pWC->hasOr = 0;
  pWC->pOuter = 0;
  pWC->nTerm = 0;
  pWC->nBase = 0;
  pWC->nSlot = ArraySize(pWC->aStatic);
  pWC->a = pWC->aStatic;
}

/*
** Deallocate a WhereClause structure.  The WhereClause structure
** itself is not freed.  This routine is the inverse of
** sqlite3WhereClauseInit().
*/
SQLITE_PRIVATE void sqlite3WhereClauseClear(WhereClause *pWC){
  sqlite3 *db = pWC->pWInfo->pParse->db;
  assert( pWC->nTerm>=pWC->nBase );
  if( pWC->nTerm>0 ){
    WhereTerm *a = pWC->a;
    WhereTerm *aLast = &pWC->a[pWC->nTerm-1];
#ifdef SQLITE_DEBUG
    int i;
    /* Verify that every term past pWC->nBase is virtual */
    for(i=pWC->nBase; i<pWC->nTerm; i++){
      assert( (pWC->a[i].wtFlags & TERM_VIRTUAL)!=0 );
    }
#endif
    while(1){
      assert( a->eMatchOp==0 || a->eOperator==WO_AUX );
      if( a->wtFlags & TERM_DYNAMIC ){
        sqlite3ExprDelete(db, a->pExpr);
      }
      if( a->wtFlags & (TERM_ORINFO|TERM_ANDINFO) ){
        if( a->wtFlags & TERM_ORINFO ){
          assert( (a->wtFlags & TERM_ANDINFO)==0 );
          whereOrInfoDelete(db, a->u.pOrInfo);
        }else{
          assert( (a->wtFlags & TERM_ANDINFO)!=0 );
          whereAndInfoDelete(db, a->u.pAndInfo);
        }
      }
      if( a==aLast ) break;
      a++;
    }
  }
}


/*
** These routines walk (recursively) an expression tree and generate
** a bitmask indicating which tables are used in that expression
** tree.
**
** sqlite3WhereExprUsage(MaskSet, Expr) ->
**
**       Return a Bitmask of all tables referenced by Expr.  Expr can be
**       be NULL, in which case 0 is returned.
**
** sqlite3WhereExprUsageNN(MaskSet, Expr) ->
**
**       Same as sqlite3WhereExprUsage() except that Expr must not be
**       NULL.  The "NN" suffix on the name stands for "Not Null".
**
** sqlite3WhereExprListUsage(MaskSet, ExprList) ->
**
**       Return a Bitmask of all tables referenced by every expression
**       in the expression list ExprList.  ExprList can be NULL, in which
**       case 0 is returned.
**
** sqlite3WhereExprUsageFull(MaskSet, ExprList) ->
**
**       Internal use only.  Called only by sqlite3WhereExprUsageNN() for
**       complex expressions that require pushing register values onto
**       the stack.  Many calls to sqlite3WhereExprUsageNN() do not need
**       the more complex analysis done by this routine.  Hence, the
**       computations done by this routine are broken out into a separate
**       "no-inline" function to avoid the stack push overhead in the
**       common case where it is not needed.
*/
static SQLITE_NOINLINE Bitmask sqlite3WhereExprUsageFull(
  WhereMaskSet *pMaskSet,
  Expr *p
){
  Bitmask mask;
  mask = (p->op==TK_IF_NULL_ROW) ? sqlite3WhereGetMask(pMaskSet, p->iTable) : 0;
  if( p->pLeft ) mask |= sqlite3WhereExprUsageNN(pMaskSet, p->pLeft);
  if( p->pRight ){
    mask |= sqlite3WhereExprUsageNN(pMaskSet, p->pRight);
    assert( p->x.pList==0 );
  }else if( ExprUseXSelect(p) ){
    if( ExprHasProperty(p, EP_VarSelect) ) pMaskSet->bVarSelect = 1;
    mask |= exprSelectUsage(pMaskSet, p->x.pSelect);
  }else if( p->x.pList ){
    mask |= sqlite3WhereExprListUsage(pMaskSet, p->x.pList);
  }
#ifndef SQLITE_OMIT_WINDOWFUNC
  if( (p->op==TK_FUNCTION || p->op==TK_AGG_FUNCTION) && ExprUseYWin(p) ){
    assert( p->y.pWin!=0 );
    mask |= sqlite3WhereExprListUsage(pMaskSet, p->y.pWin->pPartition);
    mask |= sqlite3WhereExprListUsage(pMaskSet, p->y.pWin->pOrderBy);
    mask |= sqlite3WhereExprUsage(pMaskSet, p->y.pWin->pFilter);
  }
#endif
  return mask;
}
SQLITE_PRIVATE Bitmask sqlite3WhereExprUsageNN(WhereMaskSet *pMaskSet, Expr *p){
  if( p->op==TK_COLUMN && !ExprHasProperty(p, EP_FixedCol) ){
    return sqlite3WhereGetMask(pMaskSet, p->iTable);
  }else if( ExprHasProperty(p, EP_TokenOnly|EP_Leaf) ){
    assert( p->op!=TK_IF_NULL_ROW );
    return 0;
  }
  return sqlite3WhereExprUsageFull(pMaskSet, p);
}
SQLITE_PRIVATE Bitmask sqlite3WhereExprUsage(WhereMaskSet *pMaskSet, Expr *p){
  return p ? sqlite3WhereExprUsageNN(pMaskSet,p) : 0;
}
SQLITE_PRIVATE Bitmask sqlite3WhereExprListUsage(WhereMaskSet *pMaskSet, ExprList *pList){
  int i;
  Bitmask mask = 0;
  if( pList ){
    for(i=0; i<pList->nExpr; i++){
      mask |= sqlite3WhereExprUsage(pMaskSet, pList->a[i].pExpr);
    }
  }
  return mask;
}


/*
** Call exprAnalyze on all terms in a WHERE clause.
**
** Note that exprAnalyze() might add new virtual terms onto the
** end of the WHERE clause.  We do not want to analyze these new
** virtual terms, so start analyzing at the end and work forward
** so that the added virtual terms are never processed.
*/
SQLITE_PRIVATE void sqlite3WhereExprAnalyze(
  SrcList *pTabList,       /* the FROM clause */
  WhereClause *pWC         /* the WHERE clause to be analyzed */
){
  int i;
  for(i=pWC->nTerm-1; i>=0; i--){
    exprAnalyze(pTabList, pWC, i);
  }
}

/*
** For table-valued-functions, transform the function arguments into
** new WHERE clause terms.
**
** Each function argument translates into an equality constraint against
** a HIDDEN column in the table.
*/
SQLITE_PRIVATE void sqlite3WhereTabFuncArgs(
  Parse *pParse,                    /* Parsing context */
  SrcItem *pItem,                   /* The FROM clause term to process */
  WhereClause *pWC                  /* Xfer function arguments to here */
){
  Table *pTab;
  int j, k;
  ExprList *pArgs;
  Expr *pColRef;
  Expr *pTerm;
  if( pItem->fg.isTabFunc==0 ) return;
  pTab = pItem->pTab;
  assert( pTab!=0 );
  pArgs = pItem->u1.pFuncArg;
  if( pArgs==0 ) return;
  for(j=k=0; j<pArgs->nExpr; j++){
    Expr *pRhs;
    u32 joinType;
    while( k<pTab->nCol && (pTab->aCol[k].colFlags & COLFLAG_HIDDEN)==0 ){k++;}
    if( k>=pTab->nCol ){
      sqlite3ErrorMsg(pParse, "too many arguments on %s() - max %d",
                      pTab->zName, j);
      return;
    }
    pColRef = sqlite3ExprAlloc(pParse->db, TK_COLUMN, 0, 0);
    if( pColRef==0 ) return;
    pColRef->iTable = pItem->iCursor;
    pColRef->iColumn = k++;
    assert( ExprUseYTab(pColRef) );
    pColRef->y.pTab = pTab;
    pItem->colUsed |= sqlite3ExprColUsed(pColRef);
    pRhs = sqlite3PExpr(pParse, TK_UPLUS,
        sqlite3ExprDup(pParse->db, pArgs->a[j].pExpr, 0), 0);
    pTerm = sqlite3PExpr(pParse, TK_EQ, pColRef, pRhs);
    if( pItem->fg.jointype & (JT_LEFT|JT_RIGHT) ){
      testcase( pItem->fg.jointype & JT_LEFT );  /* testtag-20230227a */
      testcase( pItem->fg.jointype & JT_RIGHT ); /* testtag-20230227b */
      joinType = EP_OuterON;
    }else{
      testcase( pItem->fg.jointype & JT_LTORJ ); /* testtag-20230227c */
      joinType = EP_InnerON;
    }
    sqlite3SetJoinExpr(pTerm, pItem->iCursor, joinType);
    whereClauseInsert(pWC, pTerm, TERM_DYNAMIC);
  }
}
