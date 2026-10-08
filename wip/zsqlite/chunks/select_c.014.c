
/*
** Invoke the OP_AggFinalize opcode for every aggregate function
** in the AggInfo structure.
*/
static void finalizeAggFunctions(Parse *pParse, AggInfo *pAggInfo){
  Vdbe *v = pParse->pVdbe;
  int i;
  struct AggInfo_func *pF;
  for(i=0, pF=pAggInfo->aFunc; i<pAggInfo->nFunc; i++, pF++){
    ExprList *pList;
    assert( ExprUseXList(pF->pFExpr) );
    pList = pF->pFExpr->x.pList;
    if( pF->iOBTab>=0 ){
      /* For an ORDER BY aggregate, calls to OP_AggStep were deferred.  Inputs
      ** were stored in emphermal table pF->iOBTab.  Here, we extract those
      ** inputs (in ORDER BY order) and make all calls to OP_AggStep
      ** before doing the OP_AggFinal call. */
      int iTop;        /* Start of loop for extracting columns */
      int nArg;        /* Number of columns to extract */
      int nKey;        /* Key columns to be skipped */
      int regAgg;      /* Extract into this array */
      int j;           /* Loop counter */

      assert( pF->pFunc!=0 );
      nArg = pList->nExpr;
      regAgg = sqlite3GetTempRange(pParse, nArg);

      if( pF->bOBPayload==0 ){
        nKey = 0;
      }else{
        assert( pF->pFExpr->pLeft!=0 );
        assert( ExprUseXList(pF->pFExpr->pLeft) );
        assert( pF->pFExpr->pLeft->x.pList!=0 );
        nKey = pF->pFExpr->pLeft->x.pList->nExpr;
        if( ALWAYS(!pF->bOBUnique) ) nKey++;
      }
      iTop = sqlite3VdbeAddOp1(v, OP_Rewind, pF->iOBTab); VdbeCoverage(v);
      for(j=nArg-1; j>=0; j--){
        sqlite3VdbeAddOp3(v, OP_Column, pF->iOBTab, nKey+j, regAgg+j);
      }
      if( pF->bUseSubtype ){
        int regSubtype = sqlite3GetTempReg(pParse);
        int iBaseCol = nKey + nArg + (pF->bOBPayload==0 && pF->bOBUnique==0);
        for(j=nArg-1; j>=0; j--){
          sqlite3VdbeAddOp3(v, OP_Column, pF->iOBTab, iBaseCol+j, regSubtype);
          sqlite3VdbeAddOp2(v, OP_SetSubtype, regSubtype, regAgg+j);
        }
        sqlite3ReleaseTempReg(pParse, regSubtype);
      }
      sqlite3VdbeAddOp3(v, OP_AggStep, 0, regAgg, AggInfoFuncReg(pAggInfo,i));
      sqlite3VdbeAppendP4(v, pF->pFunc, P4_FUNCDEF);
      sqlite3VdbeChangeP5(v, (u8)nArg);
      sqlite3VdbeAddOp2(v, OP_Next, pF->iOBTab, iTop+1); VdbeCoverage(v);
      sqlite3VdbeJumpHere(v, iTop);
      sqlite3ReleaseTempRange(pParse, regAgg, nArg);
    }
    sqlite3VdbeAddOp2(v, OP_AggFinal, AggInfoFuncReg(pAggInfo,i),
                      pList ? pList->nExpr : 0);
    sqlite3VdbeAppendP4(v, pF->pFunc, P4_FUNCDEF);
  }
}

/*
** Generate code that will update the accumulator memory cells for an
** aggregate based on the current cursor position.
**
** If regAcc is non-zero and there are no min() or max() aggregates
** in pAggInfo, then only populate the pAggInfo->nAccumulator accumulator
** registers if register regAcc contains 0. The caller will take care
** of setting and clearing regAcc.
**
** For an ORDER BY aggregate, the actual accumulator memory cell update
** is deferred until after all input rows have been received, so that they
** can be run in the requested order.  In that case, instead of invoking
** OP_AggStep to update the accumulator, just add the arguments that would
** have been passed into OP_AggStep into the sorting ephemeral table
** (along with the appropriate sort key).
*/
static void updateAccumulator(
  Parse *pParse,
  int regAcc,
  AggInfo *pAggInfo,
  int eDistinctType
){
  Vdbe *v = pParse->pVdbe;
  int i;
  int regHit = 0;
  int addrHitTest = 0;
  struct AggInfo_func *pF;
  struct AggInfo_col *pC;

  assert( pAggInfo->iFirstReg>0 );
  if( pParse->nErr ) return;
  pAggInfo->directMode = 1;
  for(i=0, pF=pAggInfo->aFunc; i<pAggInfo->nFunc; i++, pF++){
    int nArg;
    int addrNext = 0;
    int regAgg;
    int regAggSz = 0;
    int regDistinct = 0;
    ExprList *pList;
    assert( ExprUseXList(pF->pFExpr) );
    assert( !IsWindowFunc(pF->pFExpr) );
    assert( pF->pFunc!=0 );
    pList = pF->pFExpr->x.pList;
    if( ExprHasProperty(pF->pFExpr, EP_WinFunc) ){
      Expr *pFilter = pF->pFExpr->y.pWin->pFilter;
      if( pAggInfo->nAccumulator
       && (pF->pFunc->funcFlags & SQLITE_FUNC_NEEDCOLL)
       && regAcc
      ){
        /* If regAcc==0, there there exists some min() or max() function
        ** without a FILTER clause that will ensure the magnet registers
        ** are populated. */
        if( regHit==0 ) regHit = ++pParse->nMem;
        /* If this is the first row of the group (regAcc contains 0), clear the
        ** "magnet" register regHit so that the accumulator registers
        ** are populated if the FILTER clause jumps over the the
        ** invocation of min() or max() altogether. Or, if this is not
        ** the first row (regAcc contains 1), set the magnet register so that
        ** the accumulators are not populated unless the min()/max() is invoked
        ** and indicates that they should be.  */
        sqlite3VdbeAddOp2(v, OP_Copy, regAcc, regHit);
      }
      addrNext = sqlite3VdbeMakeLabel(pParse);
      sqlite3ExprIfFalse(pParse, pFilter, addrNext, SQLITE_JUMPIFNULL);
    }
    if( pF->iOBTab>=0 ){
      /* Instead of invoking AggStep, we must push the arguments that would
      ** have been passed to AggStep onto the sorting table. */
      int jj;                /* Registered used so far in building the record */
      ExprList *pOBList;     /* The ORDER BY clause */
      assert( pList!=0 );
      nArg = pList->nExpr;
      assert( nArg>0 );
      assert( pF->pFExpr->pLeft!=0 );
      assert( pF->pFExpr->pLeft->op==TK_ORDER );
      assert( ExprUseXList(pF->pFExpr->pLeft) );
      pOBList = pF->pFExpr->pLeft->x.pList;
      assert( pOBList!=0 );
      assert( pOBList->nExpr>0 );
      regAggSz = pOBList->nExpr;
      if( !pF->bOBUnique ){
        regAggSz++;   /* One register for OP_Sequence */
      }
      if( pF->bOBPayload ){
        regAggSz += nArg;
      }
      if( pF->bUseSubtype ){
        regAggSz += nArg;
      }
      regAggSz++;  /* One extra register to hold result of MakeRecord */
      regAgg = sqlite3GetTempRange(pParse, regAggSz);
      regDistinct = regAgg;
      sqlite3ExprCodeExprList(pParse, pOBList, regAgg, 0, SQLITE_ECEL_DUP);
      jj = pOBList->nExpr;
      if( !pF->bOBUnique ){
        sqlite3VdbeAddOp2(v, OP_Sequence, pF->iOBTab, regAgg+jj);
        jj++;
      }
      if( pF->bOBPayload ){
        regDistinct = regAgg+jj;
        sqlite3ExprCodeExprList(pParse, pList, regDistinct, 0, SQLITE_ECEL_DUP);
        jj += nArg;
      }
      if( pF->bUseSubtype ){
        int kk;
        int regBase = pF->bOBPayload ? regDistinct : regAgg;
        for(kk=0; kk<nArg; kk++, jj++){
          sqlite3VdbeAddOp2(v, OP_GetSubtype, regBase+kk, regAgg+jj);
        }
      }
    }else if( pList ){
      nArg = pList->nExpr;
      regAgg = sqlite3GetTempRange(pParse, nArg);
      regDistinct = regAgg;
      sqlite3ExprCodeExprList(pParse, pList, regAgg, 0, SQLITE_ECEL_DUP);
    }else{
      nArg = 0;
      regAgg = 0;
    }
    if( pF->iDistinct>=0 && pList ){
      if( addrNext==0 ){
        addrNext = sqlite3VdbeMakeLabel(pParse);
      }
      pF->iDistinct = codeDistinct(pParse, eDistinctType,
          pF->iDistinct, addrNext, pList, regDistinct);
    }
    if( pF->iOBTab>=0 ){
      /* Insert a new record into the ORDER BY table */
      sqlite3VdbeAddOp3(v, OP_MakeRecord, regAgg, regAggSz-1,
                        regAgg+regAggSz-1);
      sqlite3VdbeAddOp4Int(v, OP_IdxInsert, pF->iOBTab, regAgg+regAggSz-1,
                           regAgg, regAggSz-1);
      sqlite3ReleaseTempRange(pParse, regAgg, regAggSz);
    }else{
      /* Invoke the AggStep function */
      if( pF->pFunc->funcFlags & SQLITE_FUNC_NEEDCOLL ){
        CollSeq *pColl = 0;
        struct ExprList_item *pItem;
        int j;
        assert( pList!=0 );  /* pList!=0 if pF->pFunc has NEEDCOLL */
        for(j=0, pItem=pList->a; !pColl && j<nArg; j++, pItem++){
          pColl = sqlite3ExprCollSeq(pParse, pItem->pExpr);
        }
        if( !pColl ){
          pColl = pParse->db->pDfltColl;
        }
        if( regHit==0 && pAggInfo->nAccumulator ) regHit = ++pParse->nMem;
        sqlite3VdbeAddOp4(v, OP_CollSeq, regHit, 0, 0,
                         (char *)pColl, P4_COLLSEQ);
      }
      sqlite3VdbeAddOp3(v, OP_AggStep, 0, regAgg, AggInfoFuncReg(pAggInfo,i));
      sqlite3VdbeAppendP4(v, pF->pFunc, P4_FUNCDEF);
      sqlite3VdbeChangeP5(v, (u8)nArg);
      sqlite3ReleaseTempRange(pParse, regAgg, nArg);
    }
    if( addrNext ){
      sqlite3VdbeResolveLabel(v, addrNext);
    }
  }
  if( regHit==0 && pAggInfo->nAccumulator ){
    regHit = regAcc;
  }
  if( regHit ){
    addrHitTest = sqlite3VdbeAddOp1(v, OP_If, regHit); VdbeCoverage(v);
  }
  for(i=0, pC=pAggInfo->aCol; i<pAggInfo->nAccumulator; i++, pC++){
    sqlite3ExprCode(pParse, pC->pCExpr, AggInfoColumnReg(pAggInfo,i));
  }

  pAggInfo->directMode = 0;
  if( addrHitTest ){
    sqlite3VdbeJumpHereOrPopInst(v, addrHitTest);
  }
}

/*
** Add a single OP_Explain instruction to the VDBE to explain a simple
** count(*) query ("SELECT count(*) FROM pTab").
*/
#ifndef SQLITE_OMIT_EXPLAIN
static void explainSimpleCount(
  Parse *pParse,                  /* Parse context */
  Table *pTab,                    /* Table being queried */
  Index *pIdx                     /* Index used to optimize scan, or NULL */
){
  if( pParse->explain==2 ){
    int bCover = (pIdx!=0 && (HasRowid(pTab) || !IsPrimaryKeyIndex(pIdx)));
    sqlite3VdbeExplain(pParse, 0, "SCAN %s%s%s",
        pTab->zName,
        bCover ? " USING COVERING INDEX " : "",
        bCover ? pIdx->zName : ""
    );
  }
}
#else
# define explainSimpleCount(a,b,c)
#endif

/*
** sqlite3WalkExpr() callback used by havingToWhere().
**
** If the node passed to the callback is a TK_AND node, return
** WRC_Continue to tell sqlite3WalkExpr() to iterate through child nodes.
**
** Otherwise, return WRC_Prune. In this case, also check if the
** sub-expression matches the criteria for being moved to the WHERE
** clause. If so, add it to the WHERE clause and replace the sub-expression
** within the HAVING expression with a constant "1".
*/
static int havingToWhereExprCb(Walker *pWalker, Expr *pExpr){
  if( pExpr->op!=TK_AND ){
    Select *pS = pWalker->u.pSelect;
    /* This routine is called before the HAVING clause of the current
    ** SELECT is analyzed for aggregates. So if pExpr->pAggInfo is set
    ** here, it indicates that the expression is a correlated reference to a
    ** column from an outer aggregate query, or an aggregate function that
    ** belongs to an outer query. Do not move the expression to the WHERE
    ** clause in this obscure case, as doing so may corrupt the outer Select
    ** statements AggInfo structure.  */
    if( sqlite3ExprIsConstantOrGroupBy(pWalker->pParse, pExpr, pS->pGroupBy)
     && ExprAlwaysFalse(pExpr)==0
     && pExpr->pAggInfo==0
    ){
      sqlite3 *db = pWalker->pParse->db;
      Expr *pNew = sqlite3Expr(db, TK_INTEGER, "1");
      if( pNew ){
        Expr *pWhere = pS->pWhere;
        SWAP(Expr, *pNew, *pExpr);
        pNew = sqlite3ExprAnd(pWalker->pParse, pWhere, pNew);
        pS->pWhere = pNew;
        pWalker->eCode = 1;
      }
    }
    return WRC_Prune;
  }
  return WRC_Continue;
}

/*
** Transfer eligible terms from the HAVING clause of a query, which is
** processed after grouping, to the WHERE clause, which is processed before
** grouping. For example, the query:
**
**   SELECT * FROM <tables> WHERE a=? GROUP BY b HAVING b=? AND c=?
**
** can be rewritten as:
**
**   SELECT * FROM <tables> WHERE a=? AND b=? GROUP BY b HAVING c=?
**
** A term of the HAVING expression is eligible for transfer if it consists
** entirely of constants and expressions that are also GROUP BY terms that
** use the "BINARY" collation sequence.
*/
static void havingToWhere(Parse *pParse, Select *p){
  Walker sWalker;
  memset(&sWalker, 0, sizeof(sWalker));
  sWalker.pParse = pParse;
  sWalker.xExprCallback = havingToWhereExprCb;
  sWalker.u.pSelect = p;
  sqlite3WalkExpr(&sWalker, p->pHaving);
#if TREETRACE_ENABLED
  if( sWalker.eCode && (sqlite3TreeTrace & 0x100)!=0 ){
    TREETRACE(0x100,pParse,p,("Move HAVING terms into WHERE:\n"));
    sqlite3TreeViewSelect(0, p, 0);
  }
#endif
}

/*
** Check to see if the pThis entry of pTabList is a self-join of another view.
** Search FROM-clause entries in the range of iFirst..iEnd, including iFirst
** but stopping before iEnd.
**
** If pThis is a self-join, then return the SrcItem for the first other
** instance of that view found.  If pThis is not a self-join then return 0.
*/
static SrcItem *isSelfJoinView(
  SrcList *pTabList,           /* Search for self-joins in this FROM clause */
  SrcItem *pThis,              /* Search for prior reference to this subquery */
  int iFirst, int iEnd        /* Range of FROM-clause entries to search. */
){
  SrcItem *pItem;
  assert( pThis->pSelect!=0 );
  if( pThis->pSelect->selFlags & SF_PushDown ) return 0;
  while( iFirst<iEnd ){
    Select *pS1;
    pItem = &pTabList->a[iFirst++];
    if( pItem->pSelect==0 ) continue;
    if( pItem->fg.viaCoroutine ) continue;
    if( pItem->zName==0 ) continue;
    assert( pItem->pTab!=0 );
    assert( pThis->pTab!=0 );
    if( pItem->pTab->pSchema!=pThis->pTab->pSchema ) continue;
    if( sqlite3_stricmp(pItem->zName, pThis->zName)!=0 ) continue;
    pS1 = pItem->pSelect;
    if( pItem->pTab->pSchema==0 && pThis->pSelect->selId!=pS1->selId ){
      /* The query flattener left two different CTE tables with identical
      ** names in the same FROM clause. */
      continue;
    }
    if( pItem->pSelect->selFlags & SF_PushDown ){
      /* The view was modified by some other optimization such as
      ** pushDownWhereTerms() */
      continue;
    }
    return pItem;
  }
  return 0;
}
