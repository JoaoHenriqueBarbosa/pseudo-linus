#endif

#if !defined(SQLITE_OMIT_SUBQUERY) || !defined(SQLITE_OMIT_VIEW)

/* An instance of the SubstContext object describes an substitution edit
** to be performed on a parse tree.
**
** All references to columns in table iTable are to be replaced by corresponding
** expressions in pEList.
**
** ## About "isOuterJoin":
**
** The isOuterJoin column indicates that the replacement will occur into a
** position in the parent that NULL-able due to an OUTER JOIN.  Either the
** target slot in the parent is the right operand of a LEFT JOIN, or one of
** the left operands of a RIGHT JOIN.  In either case, we need to potentially
** bypass the substituted expression with OP_IfNullRow.
**
** Suppose the original expression is an integer constant. Even though the table
** has the nullRow flag set, because the expression is an integer constant,
** it will not be NULLed out.  So instead, we insert an OP_IfNullRow opcode
** that checks to see if the nullRow flag is set on the table.  If the nullRow
** flag is set, then the value in the register is set to NULL and the original
** expression is bypassed.  If the nullRow flag is not set, then the original
** expression runs to populate the register.
**
** Example where this is needed:
**
**      CREATE TABLE t1(a INTEGER PRIMARY KEY, b INT);
**      CREATE TABLE t2(x INT UNIQUE);
**
**      SELECT a,b,m,x FROM t1 LEFT JOIN (SELECT 59 AS m,x FROM t2) ON b=x;
**
** When the subquery on the right side of the LEFT JOIN is flattened, we
** have to add OP_IfNullRow in front of the OP_Integer that implements the
** "m" value of the subquery so that a NULL will be loaded instead of 59
** when processing a non-matched row of the left.
*/
typedef struct SubstContext {
  Parse *pParse;            /* The parsing context */
  int iTable;               /* Replace references to this table */
  int iNewTable;            /* New table number */
  int isOuterJoin;          /* Add TK_IF_NULL_ROW opcodes on each replacement */
  ExprList *pEList;         /* Replacement expressions */
  ExprList *pCList;         /* Collation sequences for replacement expr */
} SubstContext;

/* Forward Declarations */
static void substExprList(SubstContext*, ExprList*);
static void substSelect(SubstContext*, Select*, int);

/*
** Scan through the expression pExpr.  Replace every reference to
** a column in table number iTable with a copy of the iColumn-th
** entry in pEList.  (But leave references to the ROWID column
** unchanged.)
**
** This routine is part of the flattening procedure.  A subquery
** whose result set is defined by pEList appears as entry in the
** FROM clause of a SELECT such that the VDBE cursor assigned to that
** FORM clause entry is iTable.  This routine makes the necessary
** changes to pExpr so that it refers directly to the source table
** of the subquery rather the result set of the subquery.
*/
static Expr *substExpr(
  SubstContext *pSubst,  /* Description of the substitution */
  Expr *pExpr            /* Expr in which substitution occurs */
){
  if( pExpr==0 ) return 0;
  if( ExprHasProperty(pExpr, EP_OuterON|EP_InnerON)
   && pExpr->w.iJoin==pSubst->iTable
  ){
    testcase( ExprHasProperty(pExpr, EP_InnerON) );
    pExpr->w.iJoin = pSubst->iNewTable;
  }
  if( pExpr->op==TK_COLUMN
   && pExpr->iTable==pSubst->iTable
   && !ExprHasProperty(pExpr, EP_FixedCol)
  ){
#ifdef SQLITE_ALLOW_ROWID_IN_VIEW
    if( pExpr->iColumn<0 ){
      pExpr->op = TK_NULL;
    }else
#endif
    {
      Expr *pNew;
      int iColumn;
      Expr *pCopy;
      Expr ifNullRow;
      iColumn = pExpr->iColumn;
      assert( iColumn>=0 );
      assert( pSubst->pEList!=0 && iColumn<pSubst->pEList->nExpr );
      assert( pExpr->pRight==0 );
      pCopy = pSubst->pEList->a[iColumn].pExpr;
      if( sqlite3ExprIsVector(pCopy) ){
        sqlite3VectorErrorMsg(pSubst->pParse, pCopy);
      }else{
        sqlite3 *db = pSubst->pParse->db;
        if( pSubst->isOuterJoin
         && (pCopy->op!=TK_COLUMN || pCopy->iTable!=pSubst->iNewTable)
        ){
          memset(&ifNullRow, 0, sizeof(ifNullRow));
          ifNullRow.op = TK_IF_NULL_ROW;
          ifNullRow.pLeft = pCopy;
          ifNullRow.iTable = pSubst->iNewTable;
          ifNullRow.iColumn = -99;
          ifNullRow.flags = EP_IfNullRow;
          pCopy = &ifNullRow;
        }
        testcase( ExprHasProperty(pCopy, EP_Subquery) );
        pNew = sqlite3ExprDup(db, pCopy, 0);
        if( db->mallocFailed ){
          sqlite3ExprDelete(db, pNew);
          return pExpr;
        }
        if( pSubst->isOuterJoin ){
          ExprSetProperty(pNew, EP_CanBeNull);
        }
        if( ExprHasProperty(pExpr,EP_OuterON|EP_InnerON) ){
          sqlite3SetJoinExpr(pNew, pExpr->w.iJoin,
                             pExpr->flags & (EP_OuterON|EP_InnerON));
        }
        sqlite3ExprDelete(db, pExpr);
        pExpr = pNew;
        if( pExpr->op==TK_TRUEFALSE ){
          pExpr->u.iValue = sqlite3ExprTruthValue(pExpr);
          pExpr->op = TK_INTEGER;
          ExprSetProperty(pExpr, EP_IntValue);
        }

        /* Ensure that the expression now has an implicit collation sequence,
        ** just as it did when it was a column of a view or sub-query. */
        {
          CollSeq *pNat = sqlite3ExprCollSeq(pSubst->pParse, pExpr);
          CollSeq *pColl = sqlite3ExprCollSeq(pSubst->pParse,
                pSubst->pCList->a[iColumn].pExpr
          );
          if( pNat!=pColl || (pExpr->op!=TK_COLUMN && pExpr->op!=TK_COLLATE) ){
            pExpr = sqlite3ExprAddCollateString(pSubst->pParse, pExpr,
                (pColl ? pColl->zName : "BINARY")
            );
          }
        }
        ExprClearProperty(pExpr, EP_Collate);
      }
    }
  }else{
    if( pExpr->op==TK_IF_NULL_ROW && pExpr->iTable==pSubst->iTable ){
      pExpr->iTable = pSubst->iNewTable;
    }
    pExpr->pLeft = substExpr(pSubst, pExpr->pLeft);
    pExpr->pRight = substExpr(pSubst, pExpr->pRight);
    if( ExprUseXSelect(pExpr) ){
      substSelect(pSubst, pExpr->x.pSelect, 1);
    }else{
      substExprList(pSubst, pExpr->x.pList);
    }
#ifndef SQLITE_OMIT_WINDOWFUNC
    if( ExprHasProperty(pExpr, EP_WinFunc) ){
      Window *pWin = pExpr->y.pWin;
      pWin->pFilter = substExpr(pSubst, pWin->pFilter);
      substExprList(pSubst, pWin->pPartition);
      substExprList(pSubst, pWin->pOrderBy);
    }
#endif
  }
  return pExpr;
}
static void substExprList(
  SubstContext *pSubst, /* Description of the substitution */
  ExprList *pList       /* List to scan and in which to make substitutes */
){
  int i;
  if( pList==0 ) return;
  for(i=0; i<pList->nExpr; i++){
    pList->a[i].pExpr = substExpr(pSubst, pList->a[i].pExpr);
  }
}
static void substSelect(
  SubstContext *pSubst, /* Description of the substitution */
  Select *p,            /* SELECT statement in which to make substitutions */
  int doPrior           /* Do substitutes on p->pPrior too */
){
  SrcList *pSrc;
  SrcItem *pItem;
  int i;
  if( !p ) return;
  do{
    substExprList(pSubst, p->pEList);
    substExprList(pSubst, p->pGroupBy);
    substExprList(pSubst, p->pOrderBy);
    p->pHaving = substExpr(pSubst, p->pHaving);
    p->pWhere = substExpr(pSubst, p->pWhere);
    pSrc = p->pSrc;
    assert( pSrc!=0 );
    for(i=pSrc->nSrc, pItem=pSrc->a; i>0; i--, pItem++){
      substSelect(pSubst, pItem->pSelect, 1);
      if( pItem->fg.isTabFunc ){
        substExprList(pSubst, pItem->u1.pFuncArg);
      }
    }
  }while( doPrior && (p = p->pPrior)!=0 );
}
#endif /* !defined(SQLITE_OMIT_SUBQUERY) || !defined(SQLITE_OMIT_VIEW) */

#if !defined(SQLITE_OMIT_SUBQUERY) || !defined(SQLITE_OMIT_VIEW)
/*
** pSelect is a SELECT statement and pSrcItem is one item in the FROM
** clause of that SELECT.
**
** This routine scans the entire SELECT statement and recomputes the
** pSrcItem->colUsed mask.
*/
static int recomputeColumnsUsedExpr(Walker *pWalker, Expr *pExpr){
  SrcItem *pItem;
  if( pExpr->op!=TK_COLUMN ) return WRC_Continue;
  pItem = pWalker->u.pSrcItem;
  if( pItem->iCursor!=pExpr->iTable ) return WRC_Continue;
  if( pExpr->iColumn<0 ) return WRC_Continue;
  pItem->colUsed |= sqlite3ExprColUsed(pExpr);
  return WRC_Continue;
}
static void recomputeColumnsUsed(
  Select *pSelect,                 /* The complete SELECT statement */
  SrcItem *pSrcItem                /* Which FROM clause item to recompute */
){
  Walker w;
  if( NEVER(pSrcItem->pTab==0) ) return;
  memset(&w, 0, sizeof(w));
  w.xExprCallback = recomputeColumnsUsedExpr;
  w.xSelectCallback = sqlite3SelectWalkNoop;
  w.u.pSrcItem = pSrcItem;
  pSrcItem->colUsed = 0;
  sqlite3WalkSelect(&w, pSelect);
}
#endif /* !defined(SQLITE_OMIT_SUBQUERY) || !defined(SQLITE_OMIT_VIEW) */

#if !defined(SQLITE_OMIT_SUBQUERY) || !defined(SQLITE_OMIT_VIEW)
/*
** Assign new cursor numbers to each of the items in pSrc. For each
** new cursor number assigned, set an entry in the aCsrMap[] array
** to map the old cursor number to the new:
**
**     aCsrMap[iOld+1] = iNew;
**
** The array is guaranteed by the caller to be large enough for all
** existing cursor numbers in pSrc.  aCsrMap[0] is the array size.
**
** If pSrc contains any sub-selects, call this routine recursively
** on the FROM clause of each such sub-select, with iExcept set to -1.
*/
static void srclistRenumberCursors(
  Parse *pParse,                  /* Parse context */
  int *aCsrMap,                   /* Array to store cursor mappings in */
  SrcList *pSrc,                  /* FROM clause to renumber */
  int iExcept                     /* FROM clause item to skip */
){
  int i;
  SrcItem *pItem;
  for(i=0, pItem=pSrc->a; i<pSrc->nSrc; i++, pItem++){
    if( i!=iExcept ){
      Select *p;
      assert( pItem->iCursor < aCsrMap[0] );
      if( !pItem->fg.isRecursive || aCsrMap[pItem->iCursor+1]==0 ){
        aCsrMap[pItem->iCursor+1] = pParse->nTab++;
      }
      pItem->iCursor = aCsrMap[pItem->iCursor+1];
      for(p=pItem->pSelect; p; p=p->pPrior){
        srclistRenumberCursors(pParse, aCsrMap, p->pSrc, -1);
      }
    }
  }
}

/*
** *piCursor is a cursor number.  Change it if it needs to be mapped.
*/
static void renumberCursorDoMapping(Walker *pWalker, int *piCursor){
  int *aCsrMap = pWalker->u.aiCol;
  int iCsr = *piCursor;
  if( iCsr < aCsrMap[0] && aCsrMap[iCsr+1]>0 ){
    *piCursor = aCsrMap[iCsr+1];
  }
}

/*
** Expression walker callback used by renumberCursors() to update
** Expr objects to match newly assigned cursor numbers.
*/
static int renumberCursorsCb(Walker *pWalker, Expr *pExpr){
  int op = pExpr->op;
  if( op==TK_COLUMN || op==TK_IF_NULL_ROW ){
    renumberCursorDoMapping(pWalker, &pExpr->iTable);
  }
  if( ExprHasProperty(pExpr, EP_OuterON) ){
    renumberCursorDoMapping(pWalker, &pExpr->w.iJoin);
  }
  return WRC_Continue;
}

/*
** Assign a new cursor number to each cursor in the FROM clause (Select.pSrc)
** of the SELECT statement passed as the second argument, and to each
** cursor in the FROM clause of any FROM clause sub-selects, recursively.
** Except, do not assign a new cursor number to the iExcept'th element in
** the FROM clause of (*p). Update all expressions and other references
** to refer to the new cursor numbers.
**
** Argument aCsrMap is an array that may be used for temporary working
** space. Two guarantees are made by the caller:
**
**   * the array is larger than the largest cursor number used within the
**     select statement passed as an argument, and
**
**   * the array entries for all cursor numbers that do *not* appear in
**     FROM clauses of the select statement as described above are
**     initialized to zero.
*/
static void renumberCursors(
  Parse *pParse,                  /* Parse context */
  Select *p,                      /* Select to renumber cursors within */
  int iExcept,                    /* FROM clause item to skip */
  int *aCsrMap                    /* Working space */
){
  Walker w;
  srclistRenumberCursors(pParse, aCsrMap, p->pSrc, iExcept);
  memset(&w, 0, sizeof(w));
  w.u.aiCol = aCsrMap;
  w.xExprCallback = renumberCursorsCb;
  w.xSelectCallback = sqlite3SelectWalkNoop;
  sqlite3WalkSelect(&w, p);
}
#endif /* !defined(SQLITE_OMIT_SUBQUERY) || !defined(SQLITE_OMIT_VIEW) */

/*
** If pSel is not part of a compound SELECT, return a pointer to its
** expression list. Otherwise, return a pointer to the expression list
** of the leftmost SELECT in the compound.
*/
static ExprList *findLeftmostExprlist(Select *pSel){
  while( pSel->pPrior ){
    pSel = pSel->pPrior;
  }
  return pSel->pEList;
}

/*
** Return true if any of the result-set columns in the compound query
** have incompatible affinities on one or more arms of the compound.
*/
static int compoundHasDifferentAffinities(Select *p){
  int ii;
  ExprList *pList;
  assert( p!=0 );
  assert( p->pEList!=0 );
  assert( p->pPrior!=0 );
  pList = p->pEList;
  for(ii=0; ii<pList->nExpr; ii++){
    char aff;
    Select *pSub1;
    assert( pList->a[ii].pExpr!=0 );
    aff = sqlite3ExprAffinity(pList->a[ii].pExpr);
    for(pSub1=p->pPrior; pSub1; pSub1=pSub1->pPrior){
      assert( pSub1->pEList!=0 );
      assert( pSub1->pEList->nExpr>ii );
      assert( pSub1->pEList->a[ii].pExpr!=0 );
      if( sqlite3ExprAffinity(pSub1->pEList->a[ii].pExpr)!=aff ){
        return 1;
      }
    }
  }
  return 0;
}
