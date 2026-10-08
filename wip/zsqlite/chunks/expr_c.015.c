
/*
** Like sqlite3ExprCompare() except COLLATE operators at the top-level
** are ignored.
*/
SQLITE_PRIVATE int sqlite3ExprCompareSkip(Expr *pA,Expr *pB, int iTab){
  return sqlite3ExprCompare(0,
             sqlite3ExprSkipCollate(pA),
             sqlite3ExprSkipCollate(pB),
             iTab);
}

/*
** Return non-zero if Expr p can only be true if pNN is not NULL.
**
** Or if seenNot is true, return non-zero if Expr p can only be
** non-NULL if pNN is not NULL
*/
static int exprImpliesNotNull(
  const Parse *pParse,/* Parsing context */
  const Expr *p,      /* The expression to be checked */
  const Expr *pNN,    /* The expression that is NOT NULL */
  int iTab,           /* Table being evaluated */
  int seenNot         /* Return true only if p can be any non-NULL value */
){
  assert( p );
  assert( pNN );
  if( sqlite3ExprCompare(pParse, p, pNN, iTab)==0 ){
    return pNN->op!=TK_NULL;
  }
  switch( p->op ){
    case TK_IN: {
      if( seenNot && ExprHasProperty(p, EP_xIsSelect) ) return 0;
      assert( ExprUseXSelect(p) || (p->x.pList!=0 && p->x.pList->nExpr>0) );
      return exprImpliesNotNull(pParse, p->pLeft, pNN, iTab, 1);
    }
    case TK_BETWEEN: {
      ExprList *pList;
      assert( ExprUseXList(p) );
      pList = p->x.pList;
      assert( pList!=0 );
      assert( pList->nExpr==2 );
      if( seenNot ) return 0;
      if( exprImpliesNotNull(pParse, pList->a[0].pExpr, pNN, iTab, 1)
       || exprImpliesNotNull(pParse, pList->a[1].pExpr, pNN, iTab, 1)
      ){
        return 1;
      }
      return exprImpliesNotNull(pParse, p->pLeft, pNN, iTab, 1);
    }
    case TK_EQ:
    case TK_NE:
    case TK_LT:
    case TK_LE:
    case TK_GT:
    case TK_GE:
    case TK_PLUS:
    case TK_MINUS:
    case TK_BITOR:
    case TK_LSHIFT:
    case TK_RSHIFT:
    case TK_CONCAT:
      seenNot = 1;
      /* no break */ deliberate_fall_through
    case TK_STAR:
    case TK_REM:
    case TK_BITAND:
    case TK_SLASH: {
      if( exprImpliesNotNull(pParse, p->pRight, pNN, iTab, seenNot) ) return 1;
      /* no break */ deliberate_fall_through
    }
    case TK_SPAN:
    case TK_COLLATE:
    case TK_UPLUS:
    case TK_UMINUS: {
      return exprImpliesNotNull(pParse, p->pLeft, pNN, iTab, seenNot);
    }
    case TK_TRUTH: {
      if( seenNot ) return 0;
      if( p->op2!=TK_IS ) return 0;
      return exprImpliesNotNull(pParse, p->pLeft, pNN, iTab, 1);
    }
    case TK_BITNOT:
    case TK_NOT: {
      return exprImpliesNotNull(pParse, p->pLeft, pNN, iTab, 1);
    }
  }
  return 0;
}

/*
** Return true if we can prove the pE2 will always be true if pE1 is
** true.  Return false if we cannot complete the proof or if pE2 might
** be false.  Examples:
**
**     pE1: x==5       pE2: x==5             Result: true
**     pE1: x>0        pE2: x==5             Result: false
**     pE1: x=21       pE2: x=21 OR y=43     Result: true
**     pE1: x!=123     pE2: x IS NOT NULL    Result: true
**     pE1: x!=?1      pE2: x IS NOT NULL    Result: true
**     pE1: x IS NULL  pE2: x IS NOT NULL    Result: false
**     pE1: x IS ?2    pE2: x IS NOT NULL    Result: false
**
** When comparing TK_COLUMN nodes between pE1 and pE2, if pE2 has
** Expr.iTable<0 then assume a table number given by iTab.
**
** If pParse is not NULL, then the values of bound variables in pE1 are
** compared against literal values in pE2 and pParse->pVdbe->expmask is
** modified to record which bound variables are referenced.  If pParse
** is NULL, then false will be returned if pE1 contains any bound variables.
**
** When in doubt, return false.  Returning true might give a performance
** improvement.  Returning false might cause a performance reduction, but
** it will always give the correct answer and is hence always safe.
*/
SQLITE_PRIVATE int sqlite3ExprImpliesExpr(
  const Parse *pParse,
  const Expr *pE1,
  const Expr *pE2,
  int iTab
){
  if( sqlite3ExprCompare(pParse, pE1, pE2, iTab)==0 ){
    return 1;
  }
  if( pE2->op==TK_OR
   && (sqlite3ExprImpliesExpr(pParse, pE1, pE2->pLeft, iTab)
             || sqlite3ExprImpliesExpr(pParse, pE1, pE2->pRight, iTab) )
  ){
    return 1;
  }
  if( pE2->op==TK_NOTNULL
   && exprImpliesNotNull(pParse, pE1, pE2->pLeft, iTab, 0)
  ){
    return 1;
  }
  return 0;
}

/* This is a helper function to impliesNotNullRow().  In this routine,
** set pWalker->eCode to one only if *both* of the input expressions
** separately have the implies-not-null-row property.
*/
static void bothImplyNotNullRow(Walker *pWalker, Expr *pE1, Expr *pE2){
  if( pWalker->eCode==0 ){
    sqlite3WalkExpr(pWalker, pE1);
    if( pWalker->eCode ){
      pWalker->eCode = 0;
      sqlite3WalkExpr(pWalker, pE2);
    }
  }
}

/*
** This is the Expr node callback for sqlite3ExprImpliesNonNullRow().
** If the expression node requires that the table at pWalker->iCur
** have one or more non-NULL column, then set pWalker->eCode to 1 and abort.
**
** pWalker->mWFlags is non-zero if this inquiry is being undertaking on
** behalf of a RIGHT JOIN (or FULL JOIN).  That makes a difference when
** evaluating terms in the ON clause of an inner join.
**
** This routine controls an optimization.  False positives (setting
** pWalker->eCode to 1 when it should not be) are deadly, but false-negatives
** (never setting pWalker->eCode) is a harmless missed optimization.
*/
static int impliesNotNullRow(Walker *pWalker, Expr *pExpr){
  testcase( pExpr->op==TK_AGG_COLUMN );
  testcase( pExpr->op==TK_AGG_FUNCTION );
  if( ExprHasProperty(pExpr, EP_OuterON) ) return WRC_Prune;
  if( ExprHasProperty(pExpr, EP_InnerON) && pWalker->mWFlags ){
    /* If iCur is used in an inner-join ON clause to the left of a
    ** RIGHT JOIN, that does *not* mean that the table must be non-null.
    ** But it is difficult to check for that condition precisely.
    ** To keep things simple, any use of iCur from any inner-join is
    ** ignored while attempting to simplify a RIGHT JOIN. */
    return WRC_Prune;
  }
  switch( pExpr->op ){
    case TK_ISNOT:
    case TK_ISNULL:
    case TK_NOTNULL:
    case TK_IS:
    case TK_VECTOR:
    case TK_FUNCTION:
    case TK_TRUTH:
    case TK_CASE:
      testcase( pExpr->op==TK_ISNOT );
      testcase( pExpr->op==TK_ISNULL );
      testcase( pExpr->op==TK_NOTNULL );
      testcase( pExpr->op==TK_IS );
      testcase( pExpr->op==TK_VECTOR );
      testcase( pExpr->op==TK_FUNCTION );
      testcase( pExpr->op==TK_TRUTH );
      testcase( pExpr->op==TK_CASE );
      return WRC_Prune;

    case TK_COLUMN:
      if( pWalker->u.iCur==pExpr->iTable ){
        pWalker->eCode = 1;
        return WRC_Abort;
      }
      return WRC_Prune;

    case TK_OR:
    case TK_AND:
      /* Both sides of an AND or OR must separately imply non-null-row.
      ** Consider these cases:
      **    1.  NOT (x AND y)
      **    2.  x OR y
      ** If only one of x or y is non-null-row, then the overall expression
      ** can be true if the other arm is false (case 1) or true (case 2).
      */
      testcase( pExpr->op==TK_OR );
      testcase( pExpr->op==TK_AND );
      bothImplyNotNullRow(pWalker, pExpr->pLeft, pExpr->pRight);
      return WRC_Prune;

    case TK_IN:
      /* Beware of "x NOT IN ()" and "x NOT IN (SELECT 1 WHERE false)",
      ** both of which can be true.  But apart from these cases, if
      ** the left-hand side of the IN is NULL then the IN itself will be
      ** NULL. */
      if( ExprUseXList(pExpr) && ALWAYS(pExpr->x.pList->nExpr>0) ){
        sqlite3WalkExpr(pWalker, pExpr->pLeft);
      }
      return WRC_Prune;

    case TK_BETWEEN:
      /* In "x NOT BETWEEN y AND z" either x must be non-null-row or else
      ** both y and z must be non-null row */
      assert( ExprUseXList(pExpr) );
      assert( pExpr->x.pList->nExpr==2 );
      sqlite3WalkExpr(pWalker, pExpr->pLeft);
      bothImplyNotNullRow(pWalker, pExpr->x.pList->a[0].pExpr,
                                   pExpr->x.pList->a[1].pExpr);
      return WRC_Prune;

    /* Virtual tables are allowed to use constraints like x=NULL.  So
    ** a term of the form x=y does not prove that y is not null if x
    ** is the column of a virtual table */
    case TK_EQ:
    case TK_NE:
    case TK_LT:
    case TK_LE:
    case TK_GT:
    case TK_GE: {
      Expr *pLeft = pExpr->pLeft;
      Expr *pRight = pExpr->pRight;
      testcase( pExpr->op==TK_EQ );
      testcase( pExpr->op==TK_NE );
      testcase( pExpr->op==TK_LT );
      testcase( pExpr->op==TK_LE );
      testcase( pExpr->op==TK_GT );
      testcase( pExpr->op==TK_GE );
      /* The y.pTab=0 assignment in wherecode.c always happens after the
      ** impliesNotNullRow() test */
      assert( pLeft->op!=TK_COLUMN || ExprUseYTab(pLeft) );
      assert( pRight->op!=TK_COLUMN || ExprUseYTab(pRight) );
      if( (pLeft->op==TK_COLUMN
           && ALWAYS(pLeft->y.pTab!=0)
           && IsVirtual(pLeft->y.pTab))
       || (pRight->op==TK_COLUMN
           && ALWAYS(pRight->y.pTab!=0)
           && IsVirtual(pRight->y.pTab))
      ){
        return WRC_Prune;
      }
      /* no break */ deliberate_fall_through
    }
    default:
      return WRC_Continue;
  }
}

/*
** Return true (non-zero) if expression p can only be true if at least
** one column of table iTab is non-null.  In other words, return true
** if expression p will always be NULL or false if every column of iTab
** is NULL.
**
** False negatives are acceptable.  In other words, it is ok to return
** zero even if expression p will never be true of every column of iTab
** is NULL.  A false negative is merely a missed optimization opportunity.
**
** False positives are not allowed, however.  A false positive may result
** in an incorrect answer.
**
** Terms of p that are marked with EP_OuterON (and hence that come from
** the ON or USING clauses of OUTER JOINS) are excluded from the analysis.
**
** This routine is used to check if a LEFT JOIN can be converted into
** an ordinary JOIN.  The p argument is the WHERE clause.  If the WHERE
** clause requires that some column of the right table of the LEFT JOIN
** be non-NULL, then the LEFT JOIN can be safely converted into an
** ordinary join.
*/
SQLITE_PRIVATE int sqlite3ExprImpliesNonNullRow(Expr *p, int iTab, int isRJ){
  Walker w;
  p = sqlite3ExprSkipCollateAndLikely(p);
  if( p==0 ) return 0;
  if( p->op==TK_NOTNULL ){
    p = p->pLeft;
  }else{
    while( p->op==TK_AND ){
      if( sqlite3ExprImpliesNonNullRow(p->pLeft, iTab, isRJ) ) return 1;
      p = p->pRight;
    }
  }
  w.xExprCallback = impliesNotNullRow;
  w.xSelectCallback = 0;
  w.xSelectCallback2 = 0;
  w.eCode = 0;
  w.mWFlags = isRJ!=0;
  w.u.iCur = iTab;
  sqlite3WalkExpr(&w, p);
  return w.eCode;
}

/*
** An instance of the following structure is used by the tree walker
** to determine if an expression can be evaluated by reference to the
** index only, without having to do a search for the corresponding
** table entry.  The IdxCover.pIdx field is the index.  IdxCover.iCur
** is the cursor for the table.
*/
struct IdxCover {
  Index *pIdx;     /* The index to be tested for coverage */
  int iCur;        /* Cursor number for the table corresponding to the index */
};

/*
** Check to see if there are references to columns in table
** pWalker->u.pIdxCover->iCur can be satisfied using the index
** pWalker->u.pIdxCover->pIdx.
*/
static int exprIdxCover(Walker *pWalker, Expr *pExpr){
  if( pExpr->op==TK_COLUMN
   && pExpr->iTable==pWalker->u.pIdxCover->iCur
   && sqlite3TableColumnToIndex(pWalker->u.pIdxCover->pIdx, pExpr->iColumn)<0
  ){
    pWalker->eCode = 1;
    return WRC_Abort;
  }
  return WRC_Continue;
}

/*
** Determine if an index pIdx on table with cursor iCur contains will
** the expression pExpr.  Return true if the index does cover the
** expression and false if the pExpr expression references table columns
** that are not found in the index pIdx.
**
** An index covering an expression means that the expression can be
** evaluated using only the index and without having to lookup the
** corresponding table entry.
*/
SQLITE_PRIVATE int sqlite3ExprCoveredByIndex(
  Expr *pExpr,        /* The index to be tested */
  int iCur,           /* The cursor number for the corresponding table */
  Index *pIdx         /* The index that might be used for coverage */
){
  Walker w;
  struct IdxCover xcov;
  memset(&w, 0, sizeof(w));
  xcov.iCur = iCur;
  xcov.pIdx = pIdx;
  w.xExprCallback = exprIdxCover;
  w.u.pIdxCover = &xcov;
  sqlite3WalkExpr(&w, pExpr);
  return !w.eCode;
}
