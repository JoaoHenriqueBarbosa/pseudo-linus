
/*
** This routine implements a heuristic designed to improve query planning.
** This routine is called in between the first and second call to
** wherePathSolver().  Hence the name "Interstage" "Heuristic".
**
** The first call to wherePathSolver() (hereafter just "solver()") computes
** the best path without regard to the order of the outputs.  The second call
** to the solver() builds upon the first call to try to find an alternative
** path that satisfies the ORDER BY clause.
**
** This routine looks at the results of the first solver() run, and for
** every FROM clause term in the resulting query plan that uses an equality
** constraint against an index, disable other WhereLoops for that same
** FROM clause term that would try to do a full-table scan.  This prevents
** an index search from being converted into a full-table scan in order to
** satisfy an ORDER BY clause, since even though we might get slightly better
** performance using the full-scan without sorting if the output size
** estimates are very precise, we might also get severe performance
** degradation using the full-scan if the output size estimate is too large.
** It is better to err on the side of caution.
**
** Except, if the first solver() call generated a full-table scan in an outer
** loop then stop this analysis at the first full-scan, since the second
** solver() run might try to swap that full-scan for another in order to
** get the output into the correct order.  In other words, we allow a
** rewrite like this:
**
**     First Solver()                      Second Solver()
**       |-- SCAN t1                         |-- SCAN t2
**       |-- SEARCH t2                       `-- SEARCH t1
**       `-- SORT USING B-TREE
**
** The purpose of this routine is to disallow rewrites such as:
**
**     First Solver()                      Second Solver()
**       |-- SEARCH t1                       |-- SCAN t2     <--- bad!
**       |-- SEARCH t2                       `-- SEARCH t1
**       `-- SORT USING B-TREE
**
** See test cases in test/whereN.test for the real-world query that
** originally provoked this heuristic.
*/
static SQLITE_NOINLINE void whereInterstageHeuristic(WhereInfo *pWInfo){
  int i;
#ifdef WHERETRACE_ENABLED
  int once = 0;
#endif
  for(i=0; i<pWInfo->nLevel; i++){
    WhereLoop *p = pWInfo->a[i].pWLoop;
    if( p==0 ) break;
    if( (p->wsFlags & WHERE_VIRTUALTABLE)!=0 ) continue;
    if( (p->wsFlags & (WHERE_COLUMN_EQ|WHERE_COLUMN_NULL|WHERE_COLUMN_IN))!=0 ){
      u8 iTab = p->iTab;
      WhereLoop *pLoop;
      for(pLoop=pWInfo->pLoops; pLoop; pLoop=pLoop->pNextLoop){
        if( pLoop->iTab!=iTab ) continue;
        if( (pLoop->wsFlags & (WHERE_CONSTRAINT|WHERE_AUTO_INDEX))!=0 ){
          /* Auto-index and index-constrained loops allowed to remain */
          continue;
        }
#ifdef WHERETRACE_ENABLED
        if( sqlite3WhereTrace & 0x80 ){
          if( once==0 ){
            sqlite3DebugPrintf("Loops disabled by interstage heuristic:\n");
            once = 1;
          }
          sqlite3WhereLoopPrint(pLoop, &pWInfo->sWC);
        }
#endif /* WHERETRACE_ENABLED */
        pLoop->prereq = ALLBITS;  /* Prevent 2nd solver() from using this one */
      }
    }else{
      break;
    }
  }
}

/*
** Most queries use only a single table (they are not joins) and have
** simple == constraints against indexed fields.  This routine attempts
** to plan those simple cases using much less ceremony than the
** general-purpose query planner, and thereby yield faster sqlite3_prepare()
** times for the common case.
**
** Return non-zero on success, if this query can be handled by this
** no-frills query planner.  Return zero if this query needs the
** general-purpose query planner.
*/
static int whereShortCut(WhereLoopBuilder *pBuilder){
  WhereInfo *pWInfo;
  SrcItem *pItem;
  WhereClause *pWC;
  WhereTerm *pTerm;
  WhereLoop *pLoop;
  int iCur;
  int j;
  Table *pTab;
  Index *pIdx;
  WhereScan scan;

  pWInfo = pBuilder->pWInfo;
  if( pWInfo->wctrlFlags & WHERE_OR_SUBCLAUSE ) return 0;
  assert( pWInfo->pTabList->nSrc>=1 );
  pItem = pWInfo->pTabList->a;
  pTab = pItem->pTab;
  if( IsVirtual(pTab) ) return 0;
  if( pItem->fg.isIndexedBy || pItem->fg.notIndexed ){
    testcase( pItem->fg.isIndexedBy );
    testcase( pItem->fg.notIndexed );
    return 0;
  }
  iCur = pItem->iCursor;
  pWC = &pWInfo->sWC;
  pLoop = pBuilder->pNew;
  pLoop->wsFlags = 0;
  pLoop->nSkip = 0;
  pTerm = whereScanInit(&scan, pWC, iCur, -1, WO_EQ|WO_IS, 0);
  while( pTerm && pTerm->prereqRight ) pTerm = whereScanNext(&scan);
  if( pTerm ){
    testcase( pTerm->eOperator & WO_IS );
    pLoop->wsFlags = WHERE_COLUMN_EQ|WHERE_IPK|WHERE_ONEROW;
    pLoop->aLTerm[0] = pTerm;
    pLoop->nLTerm = 1;
    pLoop->u.btree.nEq = 1;
    /* TUNING: Cost of a rowid lookup is 10 */
    pLoop->rRun = 33;  /* 33==sqlite3LogEst(10) */
  }else{
    for(pIdx=pTab->pIndex; pIdx; pIdx=pIdx->pNext){
      int opMask;
      assert( pLoop->aLTermSpace==pLoop->aLTerm );
      if( !IsUniqueIndex(pIdx)
       || pIdx->pPartIdxWhere!=0
       || pIdx->nKeyCol>ArraySize(pLoop->aLTermSpace)
      ) continue;
      opMask = pIdx->uniqNotNull ? (WO_EQ|WO_IS) : WO_EQ;
      for(j=0; j<pIdx->nKeyCol; j++){
        pTerm = whereScanInit(&scan, pWC, iCur, j, opMask, pIdx);
        while( pTerm && pTerm->prereqRight ) pTerm = whereScanNext(&scan);
        if( pTerm==0 ) break;
        testcase( pTerm->eOperator & WO_IS );
        pLoop->aLTerm[j] = pTerm;
      }
      if( j!=pIdx->nKeyCol ) continue;
      pLoop->wsFlags = WHERE_COLUMN_EQ|WHERE_ONEROW|WHERE_INDEXED;
      if( pIdx->isCovering || (pItem->colUsed & pIdx->colNotIdxed)==0 ){
        pLoop->wsFlags |= WHERE_IDX_ONLY;
      }
      pLoop->nLTerm = j;
      pLoop->u.btree.nEq = j;
      pLoop->u.btree.pIndex = pIdx;
      /* TUNING: Cost of a unique index lookup is 15 */
      pLoop->rRun = 39;  /* 39==sqlite3LogEst(15) */
      break;
    }
  }
  if( pLoop->wsFlags ){
    pLoop->nOut = (LogEst)1;
    pWInfo->a[0].pWLoop = pLoop;
    assert( pWInfo->sMaskSet.n==1 && iCur==pWInfo->sMaskSet.ix[0] );
    pLoop->maskSelf = 1; /* sqlite3WhereGetMask(&pWInfo->sMaskSet, iCur); */
    pWInfo->a[0].iTabCur = iCur;
    pWInfo->nRowOut = 1;
    if( pWInfo->pOrderBy ) pWInfo->nOBSat =  pWInfo->pOrderBy->nExpr;
    if( pWInfo->wctrlFlags & WHERE_WANT_DISTINCT ){
      pWInfo->eDistinct = WHERE_DISTINCT_UNIQUE;
    }
    if( scan.iEquiv>1 ) pLoop->wsFlags |= WHERE_TRANSCONS;
#ifdef SQLITE_DEBUG
    pLoop->cId = '0';
#endif
#ifdef WHERETRACE_ENABLED
    if( sqlite3WhereTrace & 0x02 ){
      sqlite3DebugPrintf("whereShortCut() used to compute solution\n");
    }
#endif
    return 1;
  }
  return 0;
}

/*
** Helper function for exprIsDeterministic().
*/
static int exprNodeIsDeterministic(Walker *pWalker, Expr *pExpr){
  if( pExpr->op==TK_FUNCTION && ExprHasProperty(pExpr, EP_ConstFunc)==0 ){
    pWalker->eCode = 0;
    return WRC_Abort;
  }
  return WRC_Continue;
}

/*
** Return true if the expression contains no non-deterministic SQL
** functions. Do not consider non-deterministic SQL functions that are
** part of sub-select statements.
*/
static int exprIsDeterministic(Expr *p){
  Walker w;
  memset(&w, 0, sizeof(w));
  w.eCode = 1;
  w.xExprCallback = exprNodeIsDeterministic;
  w.xSelectCallback = sqlite3SelectWalkFail;
  sqlite3WalkExpr(&w, p);
  return w.eCode;
}


#ifdef WHERETRACE_ENABLED
/*
** Display all WhereLoops in pWInfo
*/
static void showAllWhereLoops(WhereInfo *pWInfo, WhereClause *pWC){
  if( sqlite3WhereTrace ){    /* Display all of the WhereLoop objects */
    WhereLoop *p;
    int i;
    static const char zLabel[] = "0123456789abcdefghijklmnopqrstuvwyxz"
                                           "ABCDEFGHIJKLMNOPQRSTUVWYXZ";
    for(p=pWInfo->pLoops, i=0; p; p=p->pNextLoop, i++){
      p->cId = zLabel[i%(sizeof(zLabel)-1)];
      sqlite3WhereLoopPrint(p, pWC);
    }
  }
}
# define WHERETRACE_ALL_LOOPS(W,C) showAllWhereLoops(W,C)
#else
# define WHERETRACE_ALL_LOOPS(W,C)
#endif

/* Attempt to omit tables from a join that do not affect the result.
** For a table to not affect the result, the following must be true:
**
**   1) The query must not be an aggregate.
**   2) The table must be the RHS of a LEFT JOIN.
**   3) Either the query must be DISTINCT, or else the ON or USING clause
**      must contain a constraint that limits the scan of the table to
**      at most a single row.
**   4) The table must not be referenced by any part of the query apart
**      from its own USING or ON clause.
**   5) The table must not have an inner-join ON or USING clause if there is
**      a RIGHT JOIN anywhere in the query.  Otherwise the ON/USING clause
**      might move from the right side to the left side of the RIGHT JOIN.
**      Note: Due to (2), this condition can only arise if the table is
**      the right-most table of a subquery that was flattened into the
**      main query and that subquery was the right-hand operand of an
**      inner join that held an ON or USING clause.
**   6) The ORDER BY clause has 63 or fewer terms
**   7) The omit-noop-join optimization is enabled.
**
** Items (1), (6), and (7) are checked by the caller.
**
** For example, given:
**
**     CREATE TABLE t1(ipk INTEGER PRIMARY KEY, v1);
**     CREATE TABLE t2(ipk INTEGER PRIMARY KEY, v2);
**     CREATE TABLE t3(ipk INTEGER PRIMARY KEY, v3);
**
** then table t2 can be omitted from the following:
**
**     SELECT v1, v3 FROM t1
**       LEFT JOIN t2 ON (t1.ipk=t2.ipk)
**       LEFT JOIN t3 ON (t1.ipk=t3.ipk)
**
** or from:
**
**     SELECT DISTINCT v1, v3 FROM t1
**       LEFT JOIN t2
**       LEFT JOIN t3 ON (t1.ipk=t3.ipk)
*/
static SQLITE_NOINLINE Bitmask whereOmitNoopJoin(
  WhereInfo *pWInfo,
  Bitmask notReady
){
  int i;
  Bitmask tabUsed;
  int hasRightJoin;

  /* Preconditions checked by the caller */
  assert( pWInfo->nLevel>=2 );
  assert( OptimizationEnabled(pWInfo->pParse->db, SQLITE_OmitNoopJoin) );

  /* These two preconditions checked by the caller combine to guarantee
  ** condition (1) of the header comment */
  assert( pWInfo->pResultSet!=0 );
  assert( 0==(pWInfo->wctrlFlags & WHERE_AGG_DISTINCT) );

  tabUsed = sqlite3WhereExprListUsage(&pWInfo->sMaskSet, pWInfo->pResultSet);
  if( pWInfo->pOrderBy ){
    tabUsed |= sqlite3WhereExprListUsage(&pWInfo->sMaskSet, pWInfo->pOrderBy);
  }
  hasRightJoin = (pWInfo->pTabList->a[0].fg.jointype & JT_LTORJ)!=0;
  for(i=pWInfo->nLevel-1; i>=1; i--){
    WhereTerm *pTerm, *pEnd;
    SrcItem *pItem;
    WhereLoop *pLoop;
    pLoop = pWInfo->a[i].pWLoop;
    pItem = &pWInfo->pTabList->a[pLoop->iTab];
    if( (pItem->fg.jointype & (JT_LEFT|JT_RIGHT))!=JT_LEFT ) continue;
    if( (pWInfo->wctrlFlags & WHERE_WANT_DISTINCT)==0
     && (pLoop->wsFlags & WHERE_ONEROW)==0
    ){
      continue;
    }
    if( (tabUsed & pLoop->maskSelf)!=0 ) continue;
    pEnd = pWInfo->sWC.a + pWInfo->sWC.nTerm;
    for(pTerm=pWInfo->sWC.a; pTerm<pEnd; pTerm++){
      if( (pTerm->prereqAll & pLoop->maskSelf)!=0 ){
        if( !ExprHasProperty(pTerm->pExpr, EP_OuterON)
         || pTerm->pExpr->w.iJoin!=pItem->iCursor
        ){
          break;
        }
      }
      if( hasRightJoin
       && ExprHasProperty(pTerm->pExpr, EP_InnerON)
       && pTerm->pExpr->w.iJoin==pItem->iCursor
      ){
        break;  /* restriction (5) */
      }
    }
    if( pTerm<pEnd ) continue;
    WHERETRACE(0xffffffff, ("-> drop loop %c not used\n", pLoop->cId));
    notReady &= ~pLoop->maskSelf;
    for(pTerm=pWInfo->sWC.a; pTerm<pEnd; pTerm++){
      if( (pTerm->prereqAll & pLoop->maskSelf)!=0 ){
        pTerm->wtFlags |= TERM_CODED;
      }
    }
    if( i!=pWInfo->nLevel-1 ){
      int nByte = (pWInfo->nLevel-1-i) * sizeof(WhereLevel);
      memmove(&pWInfo->a[i], &pWInfo->a[i+1], nByte);
    }
    pWInfo->nLevel--;
    assert( pWInfo->nLevel>0 );
  }
  return notReady;
}

/*
** Check to see if there are any SEARCH loops that might benefit from
** using a Bloom filter.  Consider a Bloom filter if:
**
**   (1)  The SEARCH happens more than N times where N is the number
**        of rows in the table that is being considered for the Bloom
**        filter.
**   (2)  Some searches are expected to find zero rows.  (This is determined
**        by the WHERE_SELFCULL flag on the term.)
**   (3)  Bloom-filter processing is not disabled.  (Checked by the
**        caller.)
**   (4)  The size of the table being searched is known by ANALYZE.
**
** This block of code merely checks to see if a Bloom filter would be
** appropriate, and if so sets the WHERE_BLOOMFILTER flag on the
** WhereLoop.  The implementation of the Bloom filter comes further
** down where the code for each WhereLoop is generated.
*/
static SQLITE_NOINLINE void whereCheckIfBloomFilterIsUseful(
  const WhereInfo *pWInfo
){
  int i;
  LogEst nSearch = 0;

  assert( pWInfo->nLevel>=2 );
  assert( OptimizationEnabled(pWInfo->pParse->db, SQLITE_BloomFilter) );
  for(i=0; i<pWInfo->nLevel; i++){
    WhereLoop *pLoop = pWInfo->a[i].pWLoop;
    const unsigned int reqFlags = (WHERE_SELFCULL|WHERE_COLUMN_EQ);
    SrcItem *pItem = &pWInfo->pTabList->a[pLoop->iTab];
    Table *pTab = pItem->pTab;
    if( (pTab->tabFlags & TF_HasStat1)==0 ) break;
    pTab->tabFlags |= TF_MaybeReanalyze;
    if( i>=1
     && (pLoop->wsFlags & reqFlags)==reqFlags
     /* vvvvvv--- Always the case if WHERE_COLUMN_EQ is defined */
     && ALWAYS((pLoop->wsFlags & (WHERE_IPK|WHERE_INDEXED))!=0)
    ){
      if( nSearch > pTab->nRowLogEst ){
        testcase( pItem->fg.jointype & JT_LEFT );
        pLoop->wsFlags |= WHERE_BLOOMFILTER;
        pLoop->wsFlags &= ~WHERE_IDX_ONLY;
        WHERETRACE(0xffffffff, (
           "-> use Bloom-filter on loop %c because there are ~%.1e "
           "lookups into %s which has only ~%.1e rows\n",
           pLoop->cId, (double)sqlite3LogEstToInt(nSearch), pTab->zName,
           (double)sqlite3LogEstToInt(pTab->nRowLogEst)));
      }
    }
    nSearch += pLoop->nOut;
  }
}
