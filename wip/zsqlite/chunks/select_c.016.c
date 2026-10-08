    ** "OUTER JOIN strength reduction" in the SQLite documentation.
    */
    if( (pItem->fg.jointype & (JT_LEFT|JT_LTORJ))!=0
     && sqlite3ExprImpliesNonNullRow(p->pWhere, pItem->iCursor,
                                     pItem->fg.jointype & JT_LTORJ)
     && OptimizationEnabled(db, SQLITE_SimplifyJoin)
    ){
      if( pItem->fg.jointype & JT_LEFT ){
        if( pItem->fg.jointype & JT_RIGHT ){
          TREETRACE(0x1000,pParse,p,
                    ("FULL-JOIN simplifies to RIGHT-JOIN on term %d\n",i));
          pItem->fg.jointype &= ~JT_LEFT;
        }else{
          TREETRACE(0x1000,pParse,p,
                    ("LEFT-JOIN simplifies to JOIN on term %d\n",i));
          pItem->fg.jointype &= ~(JT_LEFT|JT_OUTER);
          unsetJoinExpr(p->pWhere, pItem->iCursor, 0);
        }
      }
      if( pItem->fg.jointype & JT_LTORJ ){
        for(j=i+1; j<pTabList->nSrc; j++){
          SrcItem *pI2 = &pTabList->a[j];
          if( pI2->fg.jointype & JT_RIGHT ){
            if( pI2->fg.jointype & JT_LEFT ){
              TREETRACE(0x1000,pParse,p,
                        ("FULL-JOIN simplifies to LEFT-JOIN on term %d\n",j));
              pI2->fg.jointype &= ~JT_RIGHT;
            }else{
              TREETRACE(0x1000,pParse,p,
                        ("RIGHT-JOIN simplifies to JOIN on term %d\n",j));
              pI2->fg.jointype &= ~(JT_RIGHT|JT_OUTER);
              unsetJoinExpr(p->pWhere, pI2->iCursor, 1);
            }
          }
        }
        for(j=pTabList->nSrc-1; j>=0; j--){
          pTabList->a[j].fg.jointype &= ~JT_LTORJ;
          if( pTabList->a[j].fg.jointype & JT_RIGHT ) break;
        }
      }
    }

    /* No further action if this term of the FROM clause is not a subquery */
    if( pSub==0 ) continue;

    /* Catch mismatch in the declared columns of a view and the number of
    ** columns in the SELECT on the RHS */
    if( pTab->nCol!=pSub->pEList->nExpr ){
      sqlite3ErrorMsg(pParse, "expected %d columns for '%s' but got %d",
                      pTab->nCol, pTab->zName, pSub->pEList->nExpr);
      goto select_end;
    }

    /* Do not attempt the usual optimizations (flattening and ORDER BY
    ** elimination) on a MATERIALIZED common table expression because
    ** a MATERIALIZED common table expression is an optimization fence.
    */
    if( pItem->fg.isCte && pItem->u2.pCteUse->eM10d==M10d_Yes ){
      continue;
    }

    /* Do not try to flatten an aggregate subquery.
    **
    ** Flattening an aggregate subquery is only possible if the outer query
    ** is not a join.  But if the outer query is not a join, then the subquery
    ** will be implemented as a co-routine and there is no advantage to
    ** flattening in that case.
    */
    if( (pSub->selFlags & SF_Aggregate)!=0 ) continue;
    assert( pSub->pGroupBy==0 );

    /* If a FROM-clause subquery has an ORDER BY clause that is not
    ** really doing anything, then delete it now so that it does not
    ** interfere with query flattening.  See the discussion at
    ** https://sqlite.org/forum/forumpost/2d76f2bcf65d256a
    **
    ** Beware of these cases where the ORDER BY clause may not be safely
    ** omitted:
    **
    **    (1)   There is also a LIMIT clause
    **    (2)   The subquery was added to help with window-function
    **          processing
    **    (3)   The subquery is in the FROM clause of an UPDATE
    **    (4)   The outer query uses an aggregate function other than
    **          the built-in count(), min(), or max().
    **    (5)   The ORDER BY isn't going to accomplish anything because
    **          one of:
    **            (a)  The outer query has a different ORDER BY clause
    **            (b)  The subquery is part of a join
    **          See forum post 062d576715d277c8
    **
    ** Also retain the ORDER BY if the OmitOrderBy optimization is disabled.
    */
    if( pSub->pOrderBy!=0
     && (p->pOrderBy!=0 || pTabList->nSrc>1)      /* Condition (5) */
     && pSub->pLimit==0                           /* Condition (1) */
     && (pSub->selFlags & SF_OrderByReqd)==0      /* Condition (2) */
     && (p->selFlags & SF_OrderByReqd)==0         /* Condition (3) and (4) */
     && OptimizationEnabled(db, SQLITE_OmitOrderBy)
    ){
      TREETRACE(0x800,pParse,p,
                ("omit superfluous ORDER BY on %r FROM-clause subquery\n",i+1));
      sqlite3ParserAddCleanup(pParse, sqlite3ExprListDeleteGeneric,
                              pSub->pOrderBy);
      pSub->pOrderBy = 0;
    }

    /* If the outer query contains a "complex" result set (that is,
    ** if the result set of the outer query uses functions or subqueries)
    ** and if the subquery contains an ORDER BY clause and if
    ** it will be implemented as a co-routine, then do not flatten.  This
    ** restriction allows SQL constructs like this:
    **
    **  SELECT expensive_function(x)
    **    FROM (SELECT x FROM tab ORDER BY y LIMIT 10);
    **
    ** The expensive_function() is only computed on the 10 rows that
    ** are output, rather than every row of the table.
    **
    ** The requirement that the outer query have a complex result set
    ** means that flattening does occur on simpler SQL constraints without
    ** the expensive_function() like:
    **
    **  SELECT x FROM (SELECT x FROM tab ORDER BY y LIMIT 10);
    */
    if( pSub->pOrderBy!=0
     && i==0
     && (p->selFlags & SF_ComplexResult)!=0
     && (pTabList->nSrc==1
         || (pTabList->a[1].fg.jointype&(JT_OUTER|JT_CROSS))!=0)
    ){
      continue;
    }

    if( flattenSubquery(pParse, p, i, isAgg) ){
      if( pParse->nErr ) goto select_end;
      /* This subquery can be absorbed into its parent. */
      i = -1;
    }
    pTabList = p->pSrc;
    if( db->mallocFailed ) goto select_end;
    if( !IgnorableOrderby(pDest) ){
      sSort.pOrderBy = p->pOrderBy;
    }
  }
#endif

#ifndef SQLITE_OMIT_COMPOUND_SELECT
  /* Handle compound SELECT statements using the separate multiSelect()
  ** procedure.
  */
  if( p->pPrior ){
    rc = multiSelect(pParse, p, pDest);
#if TREETRACE_ENABLED
    TREETRACE(0x400,pParse,p,("end compound-select processing\n"));
    if( (sqlite3TreeTrace & 0x400)!=0 && ExplainQueryPlanParent(pParse)==0 ){
      sqlite3TreeViewSelect(0, p, 0);
    }
#endif
    if( p->pNext==0 ) ExplainQueryPlanPop(pParse);
    return rc;
  }
#endif

  /* Do the WHERE-clause constant propagation optimization if this is
  ** a join.  No need to speed time on this operation for non-join queries
  ** as the equivalent optimization will be handled by query planner in
  ** sqlite3WhereBegin().
  */
  if( p->pWhere!=0
   && p->pWhere->op==TK_AND
   && OptimizationEnabled(db, SQLITE_PropagateConst)
   && propagateConstants(pParse, p)
  ){
#if TREETRACE_ENABLED
    if( sqlite3TreeTrace & 0x2000 ){
      TREETRACE(0x2000,pParse,p,("After constant propagation:\n"));
      sqlite3TreeViewSelect(0, p, 0);
    }
#endif
  }else{
    TREETRACE(0x2000,pParse,p,("Constant propagation not helpful\n"));
  }

  if( OptimizationEnabled(db, SQLITE_QueryFlattener|SQLITE_CountOfView)
   && countOfViewOptimization(pParse, p)
  ){
    if( db->mallocFailed ) goto select_end;
    pTabList = p->pSrc;
  }

  /* For each term in the FROM clause, do two things:
  ** (1) Authorized unreferenced tables
  ** (2) Generate code for all sub-queries
  */
  for(i=0; i<pTabList->nSrc; i++){
    SrcItem *pItem = &pTabList->a[i];
    SrcItem *pPrior;
    SelectDest dest;
    Select *pSub;
#if !defined(SQLITE_OMIT_SUBQUERY) || !defined(SQLITE_OMIT_VIEW)
    const char *zSavedAuthContext;
#endif

    /* Issue SQLITE_READ authorizations with a fake column name for any
    ** tables that are referenced but from which no values are extracted.
    ** Examples of where these kinds of null SQLITE_READ authorizations
    ** would occur:
    **
    **     SELECT count(*) FROM t1;   -- SQLITE_READ t1.""
    **     SELECT t1.* FROM t1, t2;   -- SQLITE_READ t2.""
    **
    ** The fake column name is an empty string.  It is possible for a table to
    ** have a column named by the empty string, in which case there is no way to
    ** distinguish between an unreferenced table and an actual reference to the
    ** "" column. The original design was for the fake column name to be a NULL,
    ** which would be unambiguous.  But legacy authorization callbacks might
    ** assume the column name is non-NULL and segfault.  The use of an empty
    ** string for the fake column name seems safer.
    */
    if( pItem->colUsed==0 && pItem->zName!=0 ){
      sqlite3AuthCheck(pParse, SQLITE_READ, pItem->zName, "", pItem->zDatabase);
    }

#if !defined(SQLITE_OMIT_SUBQUERY) || !defined(SQLITE_OMIT_VIEW)
    /* Generate code for all sub-queries in the FROM clause
    */
    pSub = pItem->pSelect;
    if( pSub==0 || pItem->addrFillSub!=0 ) continue;

    /* The code for a subquery should only be generated once. */
    assert( pItem->addrFillSub==0 );

    /* Increment Parse.nHeight by the height of the largest expression
    ** tree referred to by this, the parent select. The child select
    ** may contain expression trees of at most
    ** (SQLITE_MAX_EXPR_DEPTH-Parse.nHeight) height. This is a bit
    ** more conservative than necessary, but much easier than enforcing
    ** an exact limit.
    */
    pParse->nHeight += sqlite3SelectExprHeight(p);

    /* Make copies of constant WHERE-clause terms in the outer query down
    ** inside the subquery.  This can help the subquery to run more efficiently.
    */
    if( OptimizationEnabled(db, SQLITE_PushDown)
     && (pItem->fg.isCte==0
         || (pItem->u2.pCteUse->eM10d!=M10d_Yes && pItem->u2.pCteUse->nUse<2))
     && pushDownWhereTerms(pParse, pSub, p->pWhere, pTabList, i)
    ){
#if TREETRACE_ENABLED
      if( sqlite3TreeTrace & 0x4000 ){
        TREETRACE(0x4000,pParse,p,
            ("After WHERE-clause push-down into subquery %d:\n", pSub->selId));
        sqlite3TreeViewSelect(0, p, 0);
      }
#endif
      assert( pItem->pSelect && (pItem->pSelect->selFlags & SF_PushDown)!=0 );
    }else{
      TREETRACE(0x4000,pParse,p,("WHERE-lcause push-down not possible\n"));
    }

    /* Convert unused result columns of the subquery into simple NULL
    ** expressions, to avoid unneeded searching and computation.
    */
    if( OptimizationEnabled(db, SQLITE_NullUnusedCols)
     && disableUnusedSubqueryResultColumns(pItem)
    ){
#if TREETRACE_ENABLED
      if( sqlite3TreeTrace & 0x4000 ){
        TREETRACE(0x4000,pParse,p,
            ("Change unused result columns to NULL for subquery %d:\n",
             pSub->selId));
        sqlite3TreeViewSelect(0, p, 0);
      }
#endif
    }

    zSavedAuthContext = pParse->zAuthContext;
    pParse->zAuthContext = pItem->zName;

    /* Generate code to implement the subquery
    */
    if( fromClauseTermCanBeCoroutine(pParse, pTabList, i, p->selFlags) ){
      /* Implement a co-routine that will return a single row of the result
      ** set on each invocation.
      */
      int addrTop = sqlite3VdbeCurrentAddr(v)+1;

      pItem->regReturn = ++pParse->nMem;
      sqlite3VdbeAddOp3(v, OP_InitCoroutine, pItem->regReturn, 0, addrTop);
      VdbeComment((v, "%!S", pItem));
      pItem->addrFillSub = addrTop;
      sqlite3SelectDestInit(&dest, SRT_Coroutine, pItem->regReturn);
      ExplainQueryPlan((pParse, 1, "CO-ROUTINE %!S", pItem));
      sqlite3Select(pParse, pSub, &dest);
      pItem->pTab->nRowLogEst = pSub->nSelectRow;
      pItem->fg.viaCoroutine = 1;
      pItem->regResult = dest.iSdst;
      sqlite3VdbeEndCoroutine(v, pItem->regReturn);
      sqlite3VdbeJumpHere(v, addrTop-1);
      sqlite3ClearTempRegCache(pParse);
    }else if( pItem->fg.isCte && pItem->u2.pCteUse->addrM9e>0 ){
      /* This is a CTE for which materialization code has already been
      ** generated.  Invoke the subroutine to compute the materialization,
      ** the make the pItem->iCursor be a copy of the ephemeral table that
      ** holds the result of the materialization. */
      CteUse *pCteUse = pItem->u2.pCteUse;
      sqlite3VdbeAddOp2(v, OP_Gosub, pCteUse->regRtn, pCteUse->addrM9e);
      if( pItem->iCursor!=pCteUse->iCur ){
        sqlite3VdbeAddOp2(v, OP_OpenDup, pItem->iCursor, pCteUse->iCur);
        VdbeComment((v, "%!S", pItem));
      }
      pSub->nSelectRow = pCteUse->nRowEst;
    }else if( (pPrior = isSelfJoinView(pTabList, pItem, 0, i))!=0 ){
      /* This view has already been materialized by a prior entry in
      ** this same FROM clause.  Reuse it. */
      if( pPrior->addrFillSub ){
        sqlite3VdbeAddOp2(v, OP_Gosub, pPrior->regReturn, pPrior->addrFillSub);
      }
      sqlite3VdbeAddOp2(v, OP_OpenDup, pItem->iCursor, pPrior->iCursor);
      pSub->nSelectRow = pPrior->pSelect->nSelectRow;
    }else{
      /* Materialize the view.  If the view is not correlated, generate a
      ** subroutine to do the materialization so that subsequent uses of
      ** the same view can reuse the materialization. */
      int topAddr;
      int onceAddr = 0;
#ifdef SQLITE_ENABLE_STMT_SCANSTATUS
      int addrExplain;
#endif

      pItem->regReturn = ++pParse->nMem;
      topAddr = sqlite3VdbeAddOp0(v, OP_Goto);
      pItem->addrFillSub = topAddr+1;
      pItem->fg.isMaterialized = 1;
      if( pItem->fg.isCorrelated==0 ){
        /* If the subquery is not correlated and if we are not inside of
        ** a trigger, then we only need to compute the value of the subquery
        ** once. */
        onceAddr = sqlite3VdbeAddOp0(v, OP_Once); VdbeCoverage(v);
        VdbeComment((v, "materialize %!S", pItem));
      }else{
        VdbeNoopComment((v, "materialize %!S", pItem));
      }
      sqlite3SelectDestInit(&dest, SRT_EphemTab, pItem->iCursor);

      ExplainQueryPlan2(addrExplain, (pParse, 1, "MATERIALIZE %!S", pItem));
      sqlite3Select(pParse, pSub, &dest);
      pItem->pTab->nRowLogEst = pSub->nSelectRow;
