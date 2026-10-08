
#if SQLITE_DEBUG
/*
** Always assert.  This xSelectCallback2 implementation proves that the
** xSelectCallback2 is never invoked.
*/
SQLITE_PRIVATE void sqlite3SelectWalkAssert2(Walker *NotUsed, Select *NotUsed2){
  UNUSED_PARAMETER2(NotUsed, NotUsed2);
  assert( 0 );
}
#endif
/*
** This routine "expands" a SELECT statement and all of its subqueries.
** For additional information on what it means to "expand" a SELECT
** statement, see the comment on the selectExpand worker callback above.
**
** Expanding a SELECT statement is the first step in processing a
** SELECT statement.  The SELECT statement must be expanded before
** name resolution is performed.
**
** If anything goes wrong, an error message is written into pParse.
** The calling function can detect the problem by looking at pParse->nErr
** and/or pParse->db->mallocFailed.
*/
static void sqlite3SelectExpand(Parse *pParse, Select *pSelect){
  Walker w;
  w.xExprCallback = sqlite3ExprWalkNoop;
  w.pParse = pParse;
  if( OK_IF_ALWAYS_TRUE(pParse->hasCompound) ){
    w.xSelectCallback = convertCompoundSelectToSubquery;
    w.xSelectCallback2 = 0;
    sqlite3WalkSelect(&w, pSelect);
  }
  w.xSelectCallback = selectExpander;
  w.xSelectCallback2 = sqlite3SelectPopWith;
  w.eCode = 0;
  sqlite3WalkSelect(&w, pSelect);
}


#ifndef SQLITE_OMIT_SUBQUERY
/*
** This is a Walker.xSelectCallback callback for the sqlite3SelectTypeInfo()
** interface.
**
** For each FROM-clause subquery, add Column.zType, Column.zColl, and
** Column.affinity information to the Table structure that represents
** the result set of that subquery.
**
** The Table structure that represents the result set was constructed
** by selectExpander() but the type and collation and affinity information
** was omitted at that point because identifiers had not yet been resolved.
** This routine is called after identifier resolution.
*/
static void selectAddSubqueryTypeInfo(Walker *pWalker, Select *p){
  Parse *pParse;
  int i;
  SrcList *pTabList;
  SrcItem *pFrom;

  if( p->selFlags & SF_HasTypeInfo ) return;
  p->selFlags |= SF_HasTypeInfo;
  pParse = pWalker->pParse;
  assert( (p->selFlags & SF_Resolved) );
  pTabList = p->pSrc;
  for(i=0, pFrom=pTabList->a; i<pTabList->nSrc; i++, pFrom++){
    Table *pTab = pFrom->pTab;
    assert( pTab!=0 );
    if( (pTab->tabFlags & TF_Ephemeral)!=0 ){
      /* A sub-query in the FROM clause of a SELECT */
      Select *pSel = pFrom->pSelect;
      if( pSel ){
        sqlite3SubqueryColumnTypes(pParse, pTab, pSel, SQLITE_AFF_NONE);
      }
    }
  }
}
#endif


/*
** This routine adds datatype and collating sequence information to
** the Table structures of all FROM-clause subqueries in a
** SELECT statement.
**
** Use this routine after name resolution.
*/
static void sqlite3SelectAddTypeInfo(Parse *pParse, Select *pSelect){
#ifndef SQLITE_OMIT_SUBQUERY
  Walker w;
  w.xSelectCallback = sqlite3SelectWalkNoop;
  w.xSelectCallback2 = selectAddSubqueryTypeInfo;
  w.xExprCallback = sqlite3ExprWalkNoop;
  w.pParse = pParse;
  sqlite3WalkSelect(&w, pSelect);
#endif
}


/*
** This routine sets up a SELECT statement for processing.  The
** following is accomplished:
**
**     *  VDBE Cursor numbers are assigned to all FROM-clause terms.
**     *  Ephemeral Table objects are created for all FROM-clause subqueries.
**     *  ON and USING clauses are shifted into WHERE statements
**     *  Wildcards "*" and "TABLE.*" in result sets are expanded.
**     *  Identifiers in expression are matched to tables.
**
** This routine acts recursively on all subqueries within the SELECT.
*/
SQLITE_PRIVATE void sqlite3SelectPrep(
  Parse *pParse,         /* The parser context */
  Select *p,             /* The SELECT statement being coded. */
  NameContext *pOuterNC  /* Name context for container */
){
  assert( p!=0 || pParse->db->mallocFailed );
  assert( pParse->db->pParse==pParse );
  if( pParse->db->mallocFailed ) return;
  if( p->selFlags & SF_HasTypeInfo ) return;
  sqlite3SelectExpand(pParse, p);
  if( pParse->nErr ) return;
  sqlite3ResolveSelectNames(pParse, p, pOuterNC);
  if( pParse->nErr ) return;
  sqlite3SelectAddTypeInfo(pParse, p);
}

#if TREETRACE_ENABLED
/*
** Display all information about an AggInfo object
*/
static void printAggInfo(AggInfo *pAggInfo){
  int ii;
  sqlite3DebugPrintf("AggInfo %d/%p:\n",
     pAggInfo->selId, pAggInfo);
  for(ii=0; ii<pAggInfo->nColumn; ii++){
    struct AggInfo_col *pCol = &pAggInfo->aCol[ii];
    sqlite3DebugPrintf(
       "agg-column[%d] pTab=%s iTable=%d iColumn=%d iMem=%d"
       " iSorterColumn=%d %s\n",
       ii, pCol->pTab ? pCol->pTab->zName : "NULL",
       pCol->iTable, pCol->iColumn, pAggInfo->iFirstReg+ii,
       pCol->iSorterColumn,
       ii>=pAggInfo->nAccumulator ? "" : " Accumulator");
    sqlite3TreeViewExpr(0, pAggInfo->aCol[ii].pCExpr, 0);
  }
  for(ii=0; ii<pAggInfo->nFunc; ii++){
    sqlite3DebugPrintf("agg-func[%d]: iMem=%d\n",
        ii, pAggInfo->iFirstReg+pAggInfo->nColumn+ii);
    sqlite3TreeViewExpr(0, pAggInfo->aFunc[ii].pFExpr, 0);
  }
}
#endif /* TREETRACE_ENABLED */

/*
** Analyze the arguments to aggregate functions.  Create new pAggInfo->aCol[]
** entries for columns that are arguments to aggregate functions but which
** are not otherwise used.
**
** The aCol[] entries in AggInfo prior to nAccumulator are columns that
** are referenced outside of aggregate functions.  These might be columns
** that are part of the GROUP by clause, for example.  Other database engines
** would throw an error if there is a column reference that is not in the
** GROUP BY clause and that is not part of an aggregate function argument.
** But SQLite allows this.
**
** The aCol[] entries beginning with the aCol[nAccumulator] and following
** are column references that are used exclusively as arguments to
** aggregate functions.  This routine is responsible for computing
** (or recomputing) those aCol[] entries.
*/
static void analyzeAggFuncArgs(
  AggInfo *pAggInfo,
  NameContext *pNC
){
  int i;
  assert( pAggInfo!=0 );
  assert( pAggInfo->iFirstReg==0 );
  pNC->ncFlags |= NC_InAggFunc;
  for(i=0; i<pAggInfo->nFunc; i++){
    Expr *pExpr = pAggInfo->aFunc[i].pFExpr;
    assert( pExpr->op==TK_FUNCTION || pExpr->op==TK_AGG_FUNCTION );
    assert( ExprUseXList(pExpr) );
    sqlite3ExprAnalyzeAggList(pNC, pExpr->x.pList);
    if( pExpr->pLeft ){
      assert( pExpr->pLeft->op==TK_ORDER );
      assert( ExprUseXList(pExpr->pLeft) );
      sqlite3ExprAnalyzeAggList(pNC, pExpr->pLeft->x.pList);
    }
#ifndef SQLITE_OMIT_WINDOWFUNC
    assert( !IsWindowFunc(pExpr) );
    if( ExprHasProperty(pExpr, EP_WinFunc) ){
      sqlite3ExprAnalyzeAggregates(pNC, pExpr->y.pWin->pFilter);
    }
#endif
  }
  pNC->ncFlags &= ~NC_InAggFunc;
}

/*
** An index on expressions is being used in the inner loop of an
** aggregate query with a GROUP BY clause.  This routine attempts
** to adjust the AggInfo object to take advantage of index and to
** perhaps use the index as a covering index.
**
*/
static void optimizeAggregateUseOfIndexedExpr(
  Parse *pParse,          /* Parsing context */
  Select *pSelect,        /* The SELECT statement being processed */
  AggInfo *pAggInfo,      /* The aggregate info */
  NameContext *pNC        /* Name context used to resolve agg-func args */
){
  assert( pAggInfo->iFirstReg==0 );
  assert( pSelect!=0 );
  assert( pSelect->pGroupBy!=0 );
  pAggInfo->nColumn = pAggInfo->nAccumulator;
  if( ALWAYS(pAggInfo->nSortingColumn>0) ){
    int mx = pSelect->pGroupBy->nExpr - 1;
    int j, k;
    for(j=0; j<pAggInfo->nColumn; j++){
      k = pAggInfo->aCol[j].iSorterColumn;
      if( k>mx ) mx = k;
    }
    pAggInfo->nSortingColumn = mx+1;
  }
  analyzeAggFuncArgs(pAggInfo, pNC);
#if TREETRACE_ENABLED
  if( sqlite3TreeTrace & 0x20 ){
    IndexedExpr *pIEpr;
    TREETRACE(0x20, pParse, pSelect,
        ("AggInfo (possibly) adjusted for Indexed Exprs\n"));
    sqlite3TreeViewSelect(0, pSelect, 0);
    for(pIEpr=pParse->pIdxEpr; pIEpr; pIEpr=pIEpr->pIENext){
      printf("data-cursor=%d index={%d,%d}\n",
          pIEpr->iDataCur, pIEpr->iIdxCur, pIEpr->iIdxCol);
      sqlite3TreeViewExpr(0, pIEpr->pExpr, 0);
    }
    printAggInfo(pAggInfo);
  }
#else
  UNUSED_PARAMETER(pSelect);
  UNUSED_PARAMETER(pParse);
#endif
}

/*
** Walker callback for aggregateConvertIndexedExprRefToColumn().
*/
static int aggregateIdxEprRefToColCallback(Walker *pWalker, Expr *pExpr){
  AggInfo *pAggInfo;
  struct AggInfo_col *pCol;
  UNUSED_PARAMETER(pWalker);
  if( pExpr->pAggInfo==0 ) return WRC_Continue;
  if( pExpr->op==TK_AGG_COLUMN ) return WRC_Continue;
  if( pExpr->op==TK_AGG_FUNCTION ) return WRC_Continue;
  if( pExpr->op==TK_IF_NULL_ROW ) return WRC_Continue;
  pAggInfo = pExpr->pAggInfo;
  if( NEVER(pExpr->iAgg>=pAggInfo->nColumn) ) return WRC_Continue;
  assert( pExpr->iAgg>=0 );
  pCol = &pAggInfo->aCol[pExpr->iAgg];
  pExpr->op = TK_AGG_COLUMN;
  pExpr->iTable = pCol->iTable;
  pExpr->iColumn = pCol->iColumn;
  ExprClearProperty(pExpr, EP_Skip|EP_Collate|EP_Unlikely);
  return WRC_Prune;
}

/*
** Convert every pAggInfo->aFunc[].pExpr such that any node within
** those expressions that has pAppInfo set is changed into a TK_AGG_COLUMN
** opcode.
*/
static void aggregateConvertIndexedExprRefToColumn(AggInfo *pAggInfo){
  int i;
  Walker w;
  memset(&w, 0, sizeof(w));
  w.xExprCallback = aggregateIdxEprRefToColCallback;
  for(i=0; i<pAggInfo->nFunc; i++){
    sqlite3WalkExpr(&w, pAggInfo->aFunc[i].pFExpr);
  }
}


/*
** Allocate a block of registers so that there is one register for each
** pAggInfo->aCol[] and pAggInfo->aFunc[] entry in pAggInfo.  The first
** register in this block is stored in pAggInfo->iFirstReg.
**
** This routine may only be called once for each AggInfo object.  Prior
** to calling this routine:
**
**     *  The aCol[] and aFunc[] arrays may be modified
**     *  The AggInfoColumnReg() and AggInfoFuncReg() macros may not be used
**
** After calling this routine:
**
**     *  The aCol[] and aFunc[] arrays are fixed
**     *  The AggInfoColumnReg() and AggInfoFuncReg() macros may be used
**
*/
static void assignAggregateRegisters(Parse *pParse, AggInfo *pAggInfo){
  assert( pAggInfo!=0 );
  assert( pAggInfo->iFirstReg==0 );
  pAggInfo->iFirstReg = pParse->nMem + 1;
  pParse->nMem += pAggInfo->nColumn + pAggInfo->nFunc;
}

/*
** Reset the aggregate accumulator.
**
** The aggregate accumulator is a set of memory cells that hold
** intermediate results while calculating an aggregate.  This
** routine generates code that stores NULLs in all of those memory
** cells.
*/
static void resetAccumulator(Parse *pParse, AggInfo *pAggInfo){
  Vdbe *v = pParse->pVdbe;
  int i;
  struct AggInfo_func *pFunc;
  int nReg = pAggInfo->nFunc + pAggInfo->nColumn;
  assert( pAggInfo->iFirstReg>0 );
  assert( pParse->db->pParse==pParse );
  assert( pParse->db->mallocFailed==0 || pParse->nErr!=0 );
  if( nReg==0 ) return;
  if( pParse->nErr ) return;
  sqlite3VdbeAddOp3(v, OP_Null, 0, pAggInfo->iFirstReg,
                    pAggInfo->iFirstReg+nReg-1);
  for(pFunc=pAggInfo->aFunc, i=0; i<pAggInfo->nFunc; i++, pFunc++){
    if( pFunc->iDistinct>=0 ){
      Expr *pE = pFunc->pFExpr;
      assert( ExprUseXList(pE) );
      if( pE->x.pList==0 || pE->x.pList->nExpr!=1 ){
        sqlite3ErrorMsg(pParse, "DISTINCT aggregates must have exactly one "
           "argument");
        pFunc->iDistinct = -1;
      }else{
        KeyInfo *pKeyInfo = sqlite3KeyInfoFromExprList(pParse, pE->x.pList,0,0);
        pFunc->iDistAddr = sqlite3VdbeAddOp4(v, OP_OpenEphemeral,
            pFunc->iDistinct, 0, 0, (char*)pKeyInfo, P4_KEYINFO);
        ExplainQueryPlan((pParse, 0, "USE TEMP B-TREE FOR %s(DISTINCT)",
                          pFunc->pFunc->zName));
      }
    }
    if( pFunc->iOBTab>=0 ){
      ExprList *pOBList;
      KeyInfo *pKeyInfo;
      int nExtra = 0;
      assert( pFunc->pFExpr->pLeft!=0 );
      assert( pFunc->pFExpr->pLeft->op==TK_ORDER );
      assert( ExprUseXList(pFunc->pFExpr->pLeft) );
      assert( pFunc->pFunc!=0 );
      pOBList = pFunc->pFExpr->pLeft->x.pList;
      if( !pFunc->bOBUnique ){
        nExtra++;  /* One extra column for the OP_Sequence */
      }
      if( pFunc->bOBPayload ){
        /* extra columns for the function arguments */
        assert( ExprUseXList(pFunc->pFExpr) );
        nExtra += pFunc->pFExpr->x.pList->nExpr;
      }
      if( pFunc->bUseSubtype ){
        nExtra += pFunc->pFExpr->x.pList->nExpr;
      }
      pKeyInfo = sqlite3KeyInfoFromExprList(pParse, pOBList, 0, nExtra);
      if( !pFunc->bOBUnique && pParse->nErr==0 ){
        pKeyInfo->nKeyField++;
      }
      sqlite3VdbeAddOp4(v, OP_OpenEphemeral,
            pFunc->iOBTab, pOBList->nExpr+nExtra, 0,
            (char*)pKeyInfo, P4_KEYINFO);
      ExplainQueryPlan((pParse, 0, "USE TEMP B-TREE FOR %s(ORDER BY)",
                          pFunc->pFunc->zName));
    }
  }
}
