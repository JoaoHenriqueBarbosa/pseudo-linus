
/*
** Deallocate a single AggInfo object
*/
static void agginfoFree(sqlite3 *db, void *pArg){
  AggInfo *p = (AggInfo*)pArg;
  sqlite3DbFree(db, p->aCol);
  sqlite3DbFree(db, p->aFunc);
  sqlite3DbFreeNN(db, p);
}

/*
** Attempt to transform a query of the form
**
**    SELECT count(*) FROM (SELECT x FROM t1 UNION ALL SELECT y FROM t2)
**
** Into this:
**
**    SELECT (SELECT count(*) FROM t1)+(SELECT count(*) FROM t2)
**
** The transformation only works if all of the following are true:
**
**   *  The subquery is a UNION ALL of two or more terms
**   *  The subquery does not have a LIMIT clause
**   *  There is no WHERE or GROUP BY or HAVING clauses on the subqueries
**   *  The outer query is a simple count(*) with no WHERE clause or other
**      extraneous syntax.
**
** Return TRUE if the optimization is undertaken.
*/
static int countOfViewOptimization(Parse *pParse, Select *p){
  Select *pSub, *pPrior;
  Expr *pExpr;
  Expr *pCount;
  sqlite3 *db;
  if( (p->selFlags & SF_Aggregate)==0 ) return 0;   /* This is an aggregate */
  if( p->pEList->nExpr!=1 ) return 0;               /* Single result column */
  if( p->pWhere ) return 0;
  if( p->pHaving ) return 0;
  if( p->pGroupBy ) return 0;
  if( p->pOrderBy ) return 0;
  pExpr = p->pEList->a[0].pExpr;
  if( pExpr->op!=TK_AGG_FUNCTION ) return 0;        /* Result is an aggregate */
  assert( ExprUseUToken(pExpr) );
  if( sqlite3_stricmp(pExpr->u.zToken,"count") ) return 0;  /* Is count() */
  assert( ExprUseXList(pExpr) );
  if( pExpr->x.pList!=0 ) return 0;                 /* Must be count(*) */
  if( p->pSrc->nSrc!=1 ) return 0;                  /* One table in FROM  */
  if( ExprHasProperty(pExpr, EP_WinFunc) ) return 0;/* Not a window function */
  pSub = p->pSrc->a[0].pSelect;
  if( pSub==0 ) return 0;                           /* The FROM is a subquery */
  if( pSub->pPrior==0 ) return 0;                   /* Must be a compound */
  if( pSub->selFlags & SF_CopyCte ) return 0;       /* Not a CTE */
  do{
    if( pSub->op!=TK_ALL && pSub->pPrior ) return 0;  /* Must be UNION ALL */
    if( pSub->pWhere ) return 0;                      /* No WHERE clause */
    if( pSub->pLimit ) return 0;                      /* No LIMIT clause */
    if( pSub->selFlags & SF_Aggregate ) return 0;     /* Not an aggregate */
    assert( pSub->pHaving==0 );  /* Due to the previous */
   pSub = pSub->pPrior;                              /* Repeat over compound */
  }while( pSub );

  /* If we reach this point then it is OK to perform the transformation */

  db = pParse->db;
  pCount = pExpr;
  pExpr = 0;
  pSub = p->pSrc->a[0].pSelect;
  p->pSrc->a[0].pSelect = 0;
  sqlite3SrcListDelete(db, p->pSrc);
  p->pSrc = sqlite3DbMallocZero(pParse->db, sizeof(*p->pSrc));
  while( pSub ){
    Expr *pTerm;
    pPrior = pSub->pPrior;
    pSub->pPrior = 0;
    pSub->pNext = 0;
    pSub->selFlags |= SF_Aggregate;
    pSub->selFlags &= ~SF_Compound;
    pSub->nSelectRow = 0;
    sqlite3ParserAddCleanup(pParse, sqlite3ExprListDeleteGeneric, pSub->pEList);
    pTerm = pPrior ? sqlite3ExprDup(db, pCount, 0) : pCount;
    pSub->pEList = sqlite3ExprListAppend(pParse, 0, pTerm);
    pTerm = sqlite3PExpr(pParse, TK_SELECT, 0, 0);
    sqlite3PExprAddSelect(pParse, pTerm, pSub);
    if( pExpr==0 ){
      pExpr = pTerm;
    }else{
      pExpr = sqlite3PExpr(pParse, TK_PLUS, pTerm, pExpr);
    }
    pSub = pPrior;
  }
  p->pEList->a[0].pExpr = pExpr;
  p->selFlags &= ~SF_Aggregate;

#if TREETRACE_ENABLED
  if( sqlite3TreeTrace & 0x200 ){
    TREETRACE(0x200,pParse,p,("After count-of-view optimization:\n"));
    sqlite3TreeViewSelect(0, p, 0);
  }
#endif
  return 1;
}

/*
** If any term of pSrc, or any SF_NestedFrom sub-query, is not the same
** as pSrcItem but has the same alias as p0, then return true.
** Otherwise return false.
*/
static int sameSrcAlias(SrcItem *p0, SrcList *pSrc){
  int i;
  for(i=0; i<pSrc->nSrc; i++){
    SrcItem *p1 = &pSrc->a[i];
    if( p1==p0 ) continue;
    if( p0->pTab==p1->pTab && 0==sqlite3_stricmp(p0->zAlias, p1->zAlias) ){
      return 1;
    }
    if( p1->pSelect
     && (p1->pSelect->selFlags & SF_NestedFrom)!=0
     && sameSrcAlias(p0, p1->pSelect->pSrc)
    ){
      return 1;
    }
  }
  return 0;
}

/*
** Return TRUE (non-zero) if the i-th entry in the pTabList SrcList can
** be implemented as a co-routine.  The i-th entry is guaranteed to be
** a subquery.
**
** The subquery is implemented as a co-routine if all of the following are
** true:
**
**    (1)  The subquery will likely be implemented in the outer loop of
**         the query.  This will be the case if any one of the following
**         conditions hold:
**         (a)  The subquery is the only term in the FROM clause
**         (b)  The subquery is the left-most term and a CROSS JOIN or similar
**              requires it to be the outer loop
**         (c)  All of the following are true:
**                (i) The subquery is the left-most subquery in the FROM clause
**               (ii) There is nothing that would prevent the subquery from
**                    being used as the outer loop if the sqlite3WhereBegin()
**                    routine nominates it to that position.
**              (iii) The query is not a UPDATE ... FROM
**    (2)  The subquery is not a CTE that should be materialized because
**         (a) the AS MATERIALIZED keyword is used, or
**         (b) the CTE is used multiple times and does not have the
**             NOT MATERIALIZED keyword
**    (3)  The subquery is not part of a left operand for a RIGHT JOIN
**    (4)  The SQLITE_Coroutine optimization disable flag is not set
**    (5)  The subquery is not self-joined
*/
static int fromClauseTermCanBeCoroutine(
  Parse *pParse,          /* Parsing context */
  SrcList *pTabList,      /* FROM clause */
  int i,                  /* Which term of the FROM clause holds the subquery */
  int selFlags            /* Flags on the SELECT statement */
){
  SrcItem *pItem = &pTabList->a[i];
  if( pItem->fg.isCte ){
    const CteUse *pCteUse = pItem->u2.pCteUse;
    if( pCteUse->eM10d==M10d_Yes ) return 0;                          /* (2a) */
    if( pCteUse->nUse>=2 && pCteUse->eM10d!=M10d_No ) return 0;       /* (2b) */
  }
  if( pTabList->a[0].fg.jointype & JT_LTORJ ) return 0;               /* (3)  */
  if( OptimizationDisabled(pParse->db, SQLITE_Coroutines) ) return 0; /* (4)  */
  if( isSelfJoinView(pTabList, pItem, i+1, pTabList->nSrc)!=0 ){
    return 0;                                                          /* (5) */
  }
  if( i==0 ){
    if( pTabList->nSrc==1 ) return 1;                             /* (1a) */
    if( pTabList->a[1].fg.jointype & JT_CROSS ) return 1;         /* (1b) */
    if( selFlags & SF_UpdateFrom )              return 0;         /* (1c-iii) */
    return 1;
  }
  if( selFlags & SF_UpdateFrom ) return 0;                        /* (1c-iii) */
  while( 1 /*exit-by-break*/ ){
    if( pItem->fg.jointype & (JT_OUTER|JT_CROSS)  ) return 0;     /* (1c-ii) */
    if( i==0 ) break;
    i--;
    pItem--;
    if( pItem->pSelect!=0 ) return 0;                             /* (1c-i) */
  }
  return 1;
}

/*
** Generate code for the SELECT statement given in the p argument.
**
** The results are returned according to the SelectDest structure.
** See comments in sqliteInt.h for further information.
**
** This routine returns the number of errors.  If any errors are
** encountered, then an appropriate error message is left in
** pParse->zErrMsg.
**
** This routine does NOT free the Select structure passed in.  The
** calling function needs to do that.
*/
SQLITE_PRIVATE int sqlite3Select(
  Parse *pParse,         /* The parser context */
  Select *p,             /* The SELECT statement being coded. */
  SelectDest *pDest      /* What to do with the query results */
){
  int i, j;              /* Loop counters */
  WhereInfo *pWInfo;     /* Return from sqlite3WhereBegin() */
  Vdbe *v;               /* The virtual machine under construction */
  int isAgg;             /* True for select lists like "count(*)" */
  ExprList *pEList = 0;  /* List of columns to extract. */
  SrcList *pTabList;     /* List of tables to select from */
  Expr *pWhere;          /* The WHERE clause.  May be NULL */
  ExprList *pGroupBy;    /* The GROUP BY clause.  May be NULL */
  Expr *pHaving;         /* The HAVING clause.  May be NULL */
  AggInfo *pAggInfo = 0; /* Aggregate information */
  int rc = 1;            /* Value to return from this function */
  DistinctCtx sDistinct; /* Info on how to code the DISTINCT keyword */
  SortCtx sSort;         /* Info on how to code the ORDER BY clause */
  int iEnd;              /* Address of the end of the query */
  sqlite3 *db;           /* The database connection */
  ExprList *pMinMaxOrderBy = 0;  /* Added ORDER BY for min/max queries */
  u8 minMaxFlag;                 /* Flag for min/max queries */

  db = pParse->db;
  assert( pParse==db->pParse );
  v = sqlite3GetVdbe(pParse);
  if( p==0 || pParse->nErr ){
    return 1;
  }
  assert( db->mallocFailed==0 );
  if( sqlite3AuthCheck(pParse, SQLITE_SELECT, 0, 0, 0) ) return 1;
#if TREETRACE_ENABLED
  TREETRACE(0x1,pParse,p, ("begin processing:\n", pParse->addrExplain));
  if( sqlite3TreeTrace & 0x10000 ){
    if( (sqlite3TreeTrace & 0x10001)==0x10000 ){
      sqlite3TreeViewLine(0, "In sqlite3Select() at %s:%d",
                           __FILE__, __LINE__);
    }
    sqlite3ShowSelect(p);
  }
#endif

  assert( p->pOrderBy==0 || pDest->eDest!=SRT_DistFifo );
  assert( p->pOrderBy==0 || pDest->eDest!=SRT_Fifo );
  assert( p->pOrderBy==0 || pDest->eDest!=SRT_DistQueue );
  assert( p->pOrderBy==0 || pDest->eDest!=SRT_Queue );
  if( IgnorableDistinct(pDest) ){
    assert(pDest->eDest==SRT_Exists     || pDest->eDest==SRT_Union ||
           pDest->eDest==SRT_Except     || pDest->eDest==SRT_Discard ||
           pDest->eDest==SRT_DistQueue  || pDest->eDest==SRT_DistFifo );
    /* All of these destinations are also able to ignore the ORDER BY clause */
    if( p->pOrderBy ){
#if TREETRACE_ENABLED
      TREETRACE(0x800,pParse,p, ("dropping superfluous ORDER BY:\n"));
      if( sqlite3TreeTrace & 0x800 ){
        sqlite3TreeViewExprList(0, p->pOrderBy, 0, "ORDERBY");
      }
#endif
      sqlite3ParserAddCleanup(pParse, sqlite3ExprListDeleteGeneric,
                              p->pOrderBy);
      testcase( pParse->earlyCleanup );
      p->pOrderBy = 0;
    }
    p->selFlags &= ~SF_Distinct;
    p->selFlags |= SF_NoopOrderBy;
  }
  sqlite3SelectPrep(pParse, p, 0);
  if( pParse->nErr ){
    goto select_end;
  }
  assert( db->mallocFailed==0 );
  assert( p->pEList!=0 );
#if TREETRACE_ENABLED
  if( sqlite3TreeTrace & 0x10 ){
    TREETRACE(0x10,pParse,p, ("after name resolution:\n"));
    sqlite3TreeViewSelect(0, p, 0);
  }
#endif

  /* If the SF_UFSrcCheck flag is set, then this function is being called
  ** as part of populating the temp table for an UPDATE...FROM statement.
  ** In this case, it is an error if the target object (pSrc->a[0]) name
  ** or alias is duplicated within FROM clause (pSrc->a[1..n]).
  **
  ** Postgres disallows this case too. The reason is that some other
  ** systems handle this case differently, and not all the same way,
  ** which is just confusing. To avoid this, we follow PG's lead and
  ** disallow it altogether.  */
  if( p->selFlags & SF_UFSrcCheck ){
    SrcItem *p0 = &p->pSrc->a[0];
    if( sameSrcAlias(p0, p->pSrc) ){
      sqlite3ErrorMsg(pParse,
          "target object/alias may not appear in FROM clause: %s",
          p0->zAlias ? p0->zAlias : p0->pTab->zName
      );
      goto select_end;
    }

    /* Clear the SF_UFSrcCheck flag. The check has already been performed,
    ** and leaving this flag set can cause errors if a compound sub-query
    ** in p->pSrc is flattened into this query and this function called
    ** again as part of compound SELECT processing.  */
    p->selFlags &= ~SF_UFSrcCheck;
  }

  if( pDest->eDest==SRT_Output ){
    sqlite3GenerateColumnNames(pParse, p);
  }

#ifndef SQLITE_OMIT_WINDOWFUNC
  if( sqlite3WindowRewrite(pParse, p) ){
    assert( pParse->nErr );
    goto select_end;
  }
#if TREETRACE_ENABLED
  if( p->pWin && (sqlite3TreeTrace & 0x40)!=0 ){
    TREETRACE(0x40,pParse,p, ("after window rewrite:\n"));
    sqlite3TreeViewSelect(0, p, 0);
  }
#endif
#endif /* SQLITE_OMIT_WINDOWFUNC */
  pTabList = p->pSrc;
  isAgg = (p->selFlags & SF_Aggregate)!=0;
  memset(&sSort, 0, sizeof(sSort));
  sSort.pOrderBy = p->pOrderBy;

  /* Try to do various optimizations (flattening subqueries, and strength
  ** reduction of join operators) in the FROM clause up into the main query
  */
#if !defined(SQLITE_OMIT_SUBQUERY) || !defined(SQLITE_OMIT_VIEW)
  for(i=0; !p->pPrior && i<pTabList->nSrc; i++){
    SrcItem *pItem = &pTabList->a[i];
    Select *pSub = pItem->pSelect;
    Table *pTab = pItem->pTab;

    /* The expander should have already created transient Table objects
    ** even for FROM clause elements such as subqueries that do not correspond
    ** to a real table */
    assert( pTab!=0 );

    /* Try to simplify joins:
    **
    **      LEFT JOIN  ->  JOIN
    **     RIGHT JOIN  ->  JOIN
    **      FULL JOIN  ->  RIGHT JOIN
    **
    ** If terms of the i-th table are used in the WHERE clause in such a
    ** way that the i-th table cannot be the NULL row of a join, then
    ** perform the appropriate simplification. This is called
