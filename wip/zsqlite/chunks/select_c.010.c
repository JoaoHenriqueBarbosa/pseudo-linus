#endif /* !defined(SQLITE_OMIT_SUBQUERY) || !defined(SQLITE_OMIT_VIEW) */

/*
** A structure to keep track of all of the column values that are fixed to
** a known value due to WHERE clause constraints of the form COLUMN=VALUE.
*/
typedef struct WhereConst WhereConst;
struct WhereConst {
  Parse *pParse;   /* Parsing context */
  u8 *pOomFault;   /* Pointer to pParse->db->mallocFailed */
  int nConst;      /* Number for COLUMN=CONSTANT terms */
  int nChng;       /* Number of times a constant is propagated */
  int bHasAffBlob; /* At least one column in apExpr[] as affinity BLOB */
  u32 mExcludeOn;  /* Which ON expressions to exclude from considertion.
                   ** Either EP_OuterON or EP_InnerON|EP_OuterON */
  Expr **apExpr;   /* [i*2] is COLUMN and [i*2+1] is VALUE */
};

/*
** Add a new entry to the pConst object.  Except, do not add duplicate
** pColumn entries.  Also, do not add if doing so would not be appropriate.
**
** The caller guarantees the pColumn is a column and pValue is a constant.
** This routine has to do some additional checks before completing the
** insert.
*/
static void constInsert(
  WhereConst *pConst,  /* The WhereConst into which we are inserting */
  Expr *pColumn,       /* The COLUMN part of the constraint */
  Expr *pValue,        /* The VALUE part of the constraint */
  Expr *pExpr          /* Overall expression: COLUMN=VALUE or VALUE=COLUMN */
){
  int i;
  assert( pColumn->op==TK_COLUMN );
  assert( sqlite3ExprIsConstant(pConst->pParse, pValue) );

  if( ExprHasProperty(pColumn, EP_FixedCol) ) return;
  if( sqlite3ExprAffinity(pValue)!=0 ) return;
  if( !sqlite3IsBinary(sqlite3ExprCompareCollSeq(pConst->pParse,pExpr)) ){
    return;
  }

  /* 2018-10-25 ticket [cf5ed20f]
  ** Make sure the same pColumn is not inserted more than once */
  for(i=0; i<pConst->nConst; i++){
    const Expr *pE2 = pConst->apExpr[i*2];
    assert( pE2->op==TK_COLUMN );
    if( pE2->iTable==pColumn->iTable
     && pE2->iColumn==pColumn->iColumn
    ){
      return;  /* Already present.  Return without doing anything. */
    }
  }
  if( sqlite3ExprAffinity(pColumn)==SQLITE_AFF_BLOB ){
    pConst->bHasAffBlob = 1;
  }

  pConst->nConst++;
  pConst->apExpr = sqlite3DbReallocOrFree(pConst->pParse->db, pConst->apExpr,
                         pConst->nConst*2*sizeof(Expr*));
  if( pConst->apExpr==0 ){
    pConst->nConst = 0;
  }else{
    pConst->apExpr[pConst->nConst*2-2] = pColumn;
    pConst->apExpr[pConst->nConst*2-1] = pValue;
  }
}

/*
** Find all terms of COLUMN=VALUE or VALUE=COLUMN in pExpr where VALUE
** is a constant expression and where the term must be true because it
** is part of the AND-connected terms of the expression.  For each term
** found, add it to the pConst structure.
*/
static void findConstInWhere(WhereConst *pConst, Expr *pExpr){
  Expr *pRight, *pLeft;
  if( NEVER(pExpr==0) ) return;
  if( ExprHasProperty(pExpr, pConst->mExcludeOn) ){
    testcase( ExprHasProperty(pExpr, EP_OuterON) );
    testcase( ExprHasProperty(pExpr, EP_InnerON) );
    return;
  }
  if( pExpr->op==TK_AND ){
    findConstInWhere(pConst, pExpr->pRight);
    findConstInWhere(pConst, pExpr->pLeft);
    return;
  }
  if( pExpr->op!=TK_EQ ) return;
  pRight = pExpr->pRight;
  pLeft = pExpr->pLeft;
  assert( pRight!=0 );
  assert( pLeft!=0 );
  if( pRight->op==TK_COLUMN && sqlite3ExprIsConstant(pConst->pParse, pLeft) ){
    constInsert(pConst,pRight,pLeft,pExpr);
  }
  if( pLeft->op==TK_COLUMN && sqlite3ExprIsConstant(pConst->pParse, pRight) ){
    constInsert(pConst,pLeft,pRight,pExpr);
  }
}

/*
** This is a helper function for Walker callback propagateConstantExprRewrite().
**
** Argument pExpr is a candidate expression to be replaced by a value. If
** pExpr is equivalent to one of the columns named in pWalker->u.pConst,
** then overwrite it with the corresponding value. Except, do not do so
** if argument bIgnoreAffBlob is non-zero and the affinity of pExpr
** is SQLITE_AFF_BLOB.
*/
static int propagateConstantExprRewriteOne(
  WhereConst *pConst,
  Expr *pExpr,
  int bIgnoreAffBlob
){
  int i;
  if( pConst->pOomFault[0] ) return WRC_Prune;
  if( pExpr->op!=TK_COLUMN ) return WRC_Continue;
  if( ExprHasProperty(pExpr, EP_FixedCol|pConst->mExcludeOn) ){
    testcase( ExprHasProperty(pExpr, EP_FixedCol) );
    testcase( ExprHasProperty(pExpr, EP_OuterON) );
    testcase( ExprHasProperty(pExpr, EP_InnerON) );
    return WRC_Continue;
  }
  for(i=0; i<pConst->nConst; i++){
    Expr *pColumn = pConst->apExpr[i*2];
    if( pColumn==pExpr ) continue;
    if( pColumn->iTable!=pExpr->iTable ) continue;
    if( pColumn->iColumn!=pExpr->iColumn ) continue;
    if( bIgnoreAffBlob && sqlite3ExprAffinity(pColumn)==SQLITE_AFF_BLOB ){
      break;
    }
    /* A match is found.  Add the EP_FixedCol property */
    pConst->nChng++;
    ExprClearProperty(pExpr, EP_Leaf);
    ExprSetProperty(pExpr, EP_FixedCol);
    assert( pExpr->pLeft==0 );
    pExpr->pLeft = sqlite3ExprDup(pConst->pParse->db, pConst->apExpr[i*2+1], 0);
    if( pConst->pParse->db->mallocFailed ) return WRC_Prune;
    break;
  }
  return WRC_Prune;
}

/*
** This is a Walker expression callback. pExpr is a node from the WHERE
** clause of a SELECT statement. This function examines pExpr to see if
** any substitutions based on the contents of pWalker->u.pConst should
** be made to pExpr or its immediate children.
**
** A substitution is made if:
**
**   + pExpr is a column with an affinity other than BLOB that matches
**     one of the columns in pWalker->u.pConst, or
**
**   + pExpr is a binary comparison operator (=, <=, >=, <, >) that
**     uses an affinity other than TEXT and one of its immediate
**     children is a column that matches one of the columns in
**     pWalker->u.pConst.
*/
static int propagateConstantExprRewrite(Walker *pWalker, Expr *pExpr){
  WhereConst *pConst = pWalker->u.pConst;
  assert( TK_GT==TK_EQ+1 );
  assert( TK_LE==TK_EQ+2 );
  assert( TK_LT==TK_EQ+3 );
  assert( TK_GE==TK_EQ+4 );
  if( pConst->bHasAffBlob ){
    if( (pExpr->op>=TK_EQ && pExpr->op<=TK_GE)
     || pExpr->op==TK_IS
    ){
      propagateConstantExprRewriteOne(pConst, pExpr->pLeft, 0);
      if( pConst->pOomFault[0] ) return WRC_Prune;
      if( sqlite3ExprAffinity(pExpr->pLeft)!=SQLITE_AFF_TEXT ){
        propagateConstantExprRewriteOne(pConst, pExpr->pRight, 0);
      }
    }
  }
  return propagateConstantExprRewriteOne(pConst, pExpr, pConst->bHasAffBlob);
}

/*
** The WHERE-clause constant propagation optimization.
**
** If the WHERE clause contains terms of the form COLUMN=CONSTANT or
** CONSTANT=COLUMN that are top-level AND-connected terms that are not
** part of a ON clause from a LEFT JOIN, then throughout the query
** replace all other occurrences of COLUMN with CONSTANT.
**
** For example, the query:
**
**      SELECT * FROM t1, t2, t3 WHERE t1.a=39 AND t2.b=t1.a AND t3.c=t2.b
**
** Is transformed into
**
**      SELECT * FROM t1, t2, t3 WHERE t1.a=39 AND t2.b=39 AND t3.c=39
**
** Return true if any transformations where made and false if not.
**
** Implementation note:  Constant propagation is tricky due to affinity
** and collating sequence interactions.  Consider this example:
**
**    CREATE TABLE t1(a INT,b TEXT);
**    INSERT INTO t1 VALUES(123,'0123');
**    SELECT * FROM t1 WHERE a=123 AND b=a;
**    SELECT * FROM t1 WHERE a=123 AND b=123;
**
** The two SELECT statements above should return different answers.  b=a
** is always true because the comparison uses numeric affinity, but b=123
** is false because it uses text affinity and '0123' is not the same as '123'.
** To work around this, the expression tree is not actually changed from
** "b=a" to "b=123" but rather the "a" in "b=a" is tagged with EP_FixedCol
** and the "123" value is hung off of the pLeft pointer.  Code generator
** routines know to generate the constant "123" instead of looking up the
** column value.  Also, to avoid collation problems, this optimization is
** only attempted if the "a=123" term uses the default BINARY collation.
**
** 2021-05-25 forum post 6a06202608: Another troublesome case is...
**
**    CREATE TABLE t1(x);
**    INSERT INTO t1 VALUES(10.0);
**    SELECT 1 FROM t1 WHERE x=10 AND x LIKE 10;
**
** The query should return no rows, because the t1.x value is '10.0' not '10'
** and '10.0' is not LIKE '10'.  But if we are not careful, the first WHERE
** term "x=10" will cause the second WHERE term to become "10 LIKE 10",
** resulting in a false positive.  To avoid this, constant propagation for
** columns with BLOB affinity is only allowed if the constant is used with
** operators ==, <=, <, >=, >, or IS in a way that will cause the correct
** type conversions to occur.  See logic associated with the bHasAffBlob flag
** for details.
*/
static int propagateConstants(
  Parse *pParse,   /* The parsing context */
  Select *p        /* The query in which to propagate constants */
){
  WhereConst x;
  Walker w;
  int nChng = 0;
  x.pParse = pParse;
  x.pOomFault = &pParse->db->mallocFailed;
  do{
    x.nConst = 0;
    x.nChng = 0;
    x.apExpr = 0;
    x.bHasAffBlob = 0;
    if( ALWAYS(p->pSrc!=0)
     && p->pSrc->nSrc>0
     && (p->pSrc->a[0].fg.jointype & JT_LTORJ)!=0
    ){
      /* Do not propagate constants on any ON clause if there is a
      ** RIGHT JOIN anywhere in the query */
      x.mExcludeOn = EP_InnerON | EP_OuterON;
    }else{
      /* Do not propagate constants through the ON clause of a LEFT JOIN */
      x.mExcludeOn = EP_OuterON;
    }
    findConstInWhere(&x, p->pWhere);
    if( x.nConst ){
      memset(&w, 0, sizeof(w));
      w.pParse = pParse;
      w.xExprCallback = propagateConstantExprRewrite;
      w.xSelectCallback = sqlite3SelectWalkNoop;
      w.xSelectCallback2 = 0;
      w.walkerDepth = 0;
      w.u.pConst = &x;
      sqlite3WalkExpr(&w, p->pWhere);
      sqlite3DbFree(x.pParse->db, x.apExpr);
      nChng += x.nChng;
    }
  }while( x.nChng );
  return nChng;
}

#if !defined(SQLITE_OMIT_SUBQUERY) || !defined(SQLITE_OMIT_VIEW)
# if !defined(SQLITE_OMIT_WINDOWFUNC)
/*
** This function is called to determine whether or not it is safe to
** push WHERE clause expression pExpr down to FROM clause sub-query
** pSubq, which contains at least one window function. Return 1
** if it is safe and the expression should be pushed down, or 0
** otherwise.
**
** It is only safe to push the expression down if it consists only
** of constants and copies of expressions that appear in the PARTITION
** BY clause of all window function used by the sub-query. It is safe
** to filter out entire partitions, but not rows within partitions, as
** this may change the results of the window functions.
**
** At the time this function is called it is guaranteed that
**
**   * the sub-query uses only one distinct window frame, and
**   * that the window frame has a PARTITION BY clause.
*/
static int pushDownWindowCheck(Parse *pParse, Select *pSubq, Expr *pExpr){
  assert( pSubq->pWin->pPartition );
  assert( (pSubq->selFlags & SF_MultiPart)==0 );
  assert( pSubq->pPrior==0 );
  return sqlite3ExprIsConstantOrGroupBy(pParse, pExpr, pSubq->pWin->pPartition);
}
# endif /* SQLITE_OMIT_WINDOWFUNC */
#endif /* !defined(SQLITE_OMIT_SUBQUERY) || !defined(SQLITE_OMIT_VIEW) */

#if !defined(SQLITE_OMIT_SUBQUERY) || !defined(SQLITE_OMIT_VIEW)
/*
** Make copies of relevant WHERE clause terms of the outer query into
** the WHERE clause of subquery.  Example:
**
**    SELECT * FROM (SELECT a AS x, c-d AS y FROM t1) WHERE x=5 AND y=10;
**
** Transformed into:
**
**    SELECT * FROM (SELECT a AS x, c-d AS y FROM t1 WHERE a=5 AND c-d=10)
**     WHERE x=5 AND y=10;
**
** The hope is that the terms added to the inner query will make it more
** efficient.
**
** NAME AMBIGUITY
**
** This optimization is called the "WHERE-clause push-down optimization".
**
** Do not confuse this optimization with another unrelated optimization
** with a similar name:  The "MySQL push-down optimization" causes WHERE
** clause terms that can be evaluated using only the index and without
** reference to the table are run first, so that if they are false,
** unnecessary table seeks are avoided.
**
** RULES
**
** Do not attempt this optimization if:
**
**   (1) (** This restriction was removed on 2017-09-29.  We used to
**           disallow this optimization for aggregate subqueries, but now
**           it is allowed by putting the extra terms on the HAVING clause.
**           The added HAVING clause is pointless if the subquery lacks
**           a GROUP BY clause.  But such a HAVING clause is also harmless
**           so there does not appear to be any reason to add extra logic
**           to suppress it. **)
**
**   (2) The inner query is the recursive part of a common table expression.
**
**   (3) The inner query has a LIMIT clause (since the changes to the WHERE
**       clause would change the meaning of the LIMIT).
**
**   (4) The inner query is the right operand of a LEFT JOIN and the
**       expression to be pushed down does not come from the ON clause
**       on that LEFT JOIN.
**
**   (5) The WHERE clause expression originates in the ON or USING clause
**       of a LEFT JOIN where iCursor is not the right-hand table of that
**       left join.  An example:
**
**           SELECT *
**           FROM (SELECT 1 AS a1 UNION ALL SELECT 2) AS aa
**           JOIN (SELECT 1 AS b2 UNION ALL SELECT 2) AS bb ON (a1=b2)
**           LEFT JOIN (SELECT 8 AS c3 UNION ALL SELECT 9) AS cc ON (b2=2);
**
**       The correct answer is three rows:  (1,1,NULL),(2,2,8),(2,2,9).
**       But if the (b2=2) term were to be pushed down into the bb subquery,
**       then the (1,1,NULL) row would be suppressed.
**
**   (6) Window functions make things tricky as changes to the WHERE clause
**       of the inner query could change the window over which window
**       functions are calculated. Therefore, do not attempt the optimization
**       if:
**
**     (6a) The inner query uses multiple incompatible window partitions.
**
**     (6b) The inner query is a compound and uses window-functions.
**
**     (6c) The WHERE clause does not consist entirely of constants and
**          copies of expressions found in the PARTITION BY clause of
**          all window-functions used by the sub-query. It is safe to
**          filter out entire partitions, as this does not change the
**          window over which any window-function is calculated.
**
**   (7) The inner query is a Common Table Expression (CTE) that should
**       be materialized.  (This restriction is implemented in the calling
**       routine.)
**
**   (8) If the subquery is a compound that uses UNION, INTERSECT,
**       or EXCEPT, then all of the result set columns for all arms of
**       the compound must use the BINARY collating sequence.
**
**   (9) All three of the following are true:
**
**       (9a) The WHERE clause expression originates in the ON or USING clause
**            of a join (either an INNER or an OUTER join), and
**
**       (9b) The subquery is to the right of the ON/USING clause
**
**       (9c) There is a RIGHT JOIN (or FULL JOIN) in between the ON/USING
**            clause and the subquery.
**
**       Without this restriction, the WHERE-clause push-down optimization
**       might move the ON/USING filter expression from the left side of a
**       RIGHT JOIN over to the right side, which leads to incorrect answers.
**       See also restriction (6) in sqlite3ExprIsSingleTableConstraint().
**
**  (10) The inner query is not the right-hand table of a RIGHT JOIN.
**
**  (11) The subquery is not a VALUES clause
**
**  (12) The WHERE clause is not "rowid ISNULL" or the equivalent.  This
**       case only comes up if SQLite is compiled using
**       SQLITE_ALLOW_ROWID_IN_VIEW.
**
** Return 0 if no changes are made and non-zero if one or more WHERE clause
** terms are duplicated into the subquery.
*/
static int pushDownWhereTerms(
  Parse *pParse,        /* Parse context (for malloc() and error reporting) */
  Select *pSubq,        /* The subquery whose WHERE clause is to be augmented */
  Expr *pWhere,         /* The WHERE clause of the outer query */
  SrcList *pSrcList,    /* The complete from clause of the outer query */
  int iSrc              /* Which FROM clause term to try to push into  */
){
  Expr *pNew;
  SrcItem *pSrc;        /* The subquery FROM term into which WHERE is pushed */
  int nChng = 0;
  pSrc = &pSrcList->a[iSrc];
  if( pWhere==0 ) return 0;
  if( pSubq->selFlags & (SF_Recursive|SF_MultiPart) ){
    return 0;           /* restrictions (2) and (11) */
  }
  if( pSrc->fg.jointype & (JT_LTORJ|JT_RIGHT) ){
    return 0;           /* restrictions (10) */
  }

  if( pSubq->pPrior ){
    Select *pSel;
    int notUnionAll = 0;
    for(pSel=pSubq; pSel; pSel=pSel->pPrior){
      u8 op = pSel->op;
      assert( op==TK_ALL || op==TK_SELECT
           || op==TK_UNION || op==TK_INTERSECT || op==TK_EXCEPT );
      if( op!=TK_ALL && op!=TK_SELECT ){
        notUnionAll = 1;
      }
#ifndef SQLITE_OMIT_WINDOWFUNC
      if( pSel->pWin ) return 0;    /* restriction (6b) */
#endif
    }
    if( notUnionAll ){
      /* If any of the compound arms are connected using UNION, INTERSECT,
      ** or EXCEPT, then we must ensure that none of the columns use a
      ** non-BINARY collating sequence. */
      for(pSel=pSubq; pSel; pSel=pSel->pPrior){
        int ii;
        const ExprList *pList = pSel->pEList;
        assert( pList!=0 );
        for(ii=0; ii<pList->nExpr; ii++){
          CollSeq *pColl = sqlite3ExprCollSeq(pParse, pList->a[ii].pExpr);
          if( !sqlite3IsBinary(pColl) ){
            return 0;  /* Restriction (8) */
          }
        }
      }
    }
  }else{
#ifndef SQLITE_OMIT_WINDOWFUNC
    if( pSubq->pWin && pSubq->pWin->pPartition==0 ) return 0;
#endif
  }

#ifdef SQLITE_DEBUG
  /* Only the first term of a compound can have a WITH clause.  But make
  ** sure no other terms are marked SF_Recursive in case something changes
  ** in the future.
  */
  {
    Select *pX;
    for(pX=pSubq; pX; pX=pX->pPrior){
      assert( (pX->selFlags & (SF_Recursive))==0 );
    }
  }
#endif

  if( pSubq->pLimit!=0 ){
    return 0; /* restriction (3) */
  }
  while( pWhere->op==TK_AND ){
    nChng += pushDownWhereTerms(pParse, pSubq, pWhere->pRight, pSrcList, iSrc);
    pWhere = pWhere->pLeft;
  }

#if 0 /* These checks now done by sqlite3ExprIsSingleTableConstraint() */
  if( ExprHasProperty(pWhere, EP_OuterON|EP_InnerON) /* (9a) */
   && (pSrcList->a[0].fg.jointype & JT_LTORJ)!=0     /* Fast pre-test of (9c) */
  ){
    int jj;
    for(jj=0; jj<iSrc; jj++){
      if( pWhere->w.iJoin==pSrcList->a[jj].iCursor ){
        /* If we reach this point, both (9a) and (9b) are satisfied.
        ** The following loop checks (9c):
        */
        for(jj++; jj<iSrc; jj++){
          if( (pSrcList->a[jj].fg.jointype & JT_RIGHT)!=0 ){
            return 0;  /* restriction (9) */
          }
        }
      }
    }
  }
  if( isLeftJoin
   && (ExprHasProperty(pWhere,EP_OuterON)==0
         || pWhere->w.iJoin!=iCursor)
  ){
    return 0; /* restriction (4) */
  }
  if( ExprHasProperty(pWhere,EP_OuterON)
   && pWhere->w.iJoin!=iCursor
  ){
    return 0; /* restriction (5) */
  }
#endif

#ifdef SQLITE_ALLOW_ROWID_IN_VIEW
  if( ViewCanHaveRowid && (pWhere->op==TK_ISNULL || pWhere->op==TK_NOTNULL) ){
    Expr *pLeft = pWhere->pLeft;
    if( ALWAYS(pLeft)
     && pLeft->op==TK_COLUMN
     && pLeft->iColumn < 0
    ){
      return 0;  /* Restriction (12) */
    }
  }
#endif

  if( sqlite3ExprIsSingleTableConstraint(pWhere, pSrcList, iSrc, 1) ){
    nChng++;
    pSubq->selFlags |= SF_PushDown;
    while( pSubq ){
      SubstContext x;
      pNew = sqlite3ExprDup(pParse->db, pWhere, 0);
      unsetJoinExpr(pNew, -1, 1);
      x.pParse = pParse;
      x.iTable = pSrc->iCursor;
      x.iNewTable = pSrc->iCursor;
      x.isOuterJoin = 0;
      x.pEList = pSubq->pEList;
      x.pCList = findLeftmostExprlist(pSubq);
      pNew = substExpr(&x, pNew);
#ifndef SQLITE_OMIT_WINDOWFUNC
      if( pSubq->pWin && 0==pushDownWindowCheck(pParse, pSubq, pNew) ){
        /* Restriction 6c has prevented push-down in this case */
        sqlite3ExprDelete(pParse->db, pNew);
        nChng--;
        break;
      }
#endif
      if( pSubq->selFlags & SF_Aggregate ){
        pSubq->pHaving = sqlite3ExprAnd(pParse, pSubq->pHaving, pNew);
      }else{
        pSubq->pWhere = sqlite3ExprAnd(pParse, pSubq->pWhere, pNew);
      }
      pSubq = pSubq->pPrior;
    }
  }
  return nChng;
}
