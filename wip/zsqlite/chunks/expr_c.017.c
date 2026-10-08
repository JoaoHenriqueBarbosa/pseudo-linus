
/*
** Analyze the pExpr expression looking for aggregate functions and
** for variables that need to be added to AggInfo object that pNC->pAggInfo
** points to.  Additional entries are made on the AggInfo object as
** necessary.
**
** This routine should only be called after the expression has been
** analyzed by sqlite3ResolveExprNames().
*/
SQLITE_PRIVATE void sqlite3ExprAnalyzeAggregates(NameContext *pNC, Expr *pExpr){
  Walker w;
  w.xExprCallback = analyzeAggregate;
  w.xSelectCallback = sqlite3WalkerDepthIncrease;
  w.xSelectCallback2 = sqlite3WalkerDepthDecrease;
  w.walkerDepth = 0;
  w.u.pNC = pNC;
  w.pParse = 0;
  assert( pNC->pSrcList!=0 );
  sqlite3WalkExpr(&w, pExpr);
}

/*
** Call sqlite3ExprAnalyzeAggregates() for every expression in an
** expression list.  Return the number of errors.
**
** If an error is found, the analysis is cut short.
*/
SQLITE_PRIVATE void sqlite3ExprAnalyzeAggList(NameContext *pNC, ExprList *pList){
  struct ExprList_item *pItem;
  int i;
  if( pList ){
    for(pItem=pList->a, i=0; i<pList->nExpr; i++, pItem++){
      sqlite3ExprAnalyzeAggregates(pNC, pItem->pExpr);
    }
  }
}

/*
** Allocate a single new register for use to hold some intermediate result.
*/
SQLITE_PRIVATE int sqlite3GetTempReg(Parse *pParse){
  if( pParse->nTempReg==0 ){
    return ++pParse->nMem;
  }
  return pParse->aTempReg[--pParse->nTempReg];
}

/*
** Deallocate a register, making available for reuse for some other
** purpose.
*/
SQLITE_PRIVATE void sqlite3ReleaseTempReg(Parse *pParse, int iReg){
  if( iReg ){
    sqlite3VdbeReleaseRegisters(pParse, iReg, 1, 0, 0);
    if( pParse->nTempReg<ArraySize(pParse->aTempReg) ){
      pParse->aTempReg[pParse->nTempReg++] = iReg;
    }
  }
}

/*
** Allocate or deallocate a block of nReg consecutive registers.
*/
SQLITE_PRIVATE int sqlite3GetTempRange(Parse *pParse, int nReg){
  int i, n;
  if( nReg==1 ) return sqlite3GetTempReg(pParse);
  i = pParse->iRangeReg;
  n = pParse->nRangeReg;
  if( nReg<=n ){
    pParse->iRangeReg += nReg;
    pParse->nRangeReg -= nReg;
  }else{
    i = pParse->nMem+1;
    pParse->nMem += nReg;
  }
  return i;
}
SQLITE_PRIVATE void sqlite3ReleaseTempRange(Parse *pParse, int iReg, int nReg){
  if( nReg==1 ){
    sqlite3ReleaseTempReg(pParse, iReg);
    return;
  }
  sqlite3VdbeReleaseRegisters(pParse, iReg, nReg, 0, 0);
  if( nReg>pParse->nRangeReg ){
    pParse->nRangeReg = nReg;
    pParse->iRangeReg = iReg;
  }
}

/*
** Mark all temporary registers as being unavailable for reuse.
**
** Always invoke this procedure after coding a subroutine or co-routine
** that might be invoked from other parts of the code, to ensure that
** the sub/co-routine does not use registers in common with the code that
** invokes the sub/co-routine.
*/
SQLITE_PRIVATE void sqlite3ClearTempRegCache(Parse *pParse){
  pParse->nTempReg = 0;
  pParse->nRangeReg = 0;
}

/*
** Make sure sufficient registers have been allocated so that
** iReg is a valid register number.
*/
SQLITE_PRIVATE void sqlite3TouchRegister(Parse *pParse, int iReg){
  if( pParse->nMem<iReg ) pParse->nMem = iReg;
}

#if defined(SQLITE_ENABLE_STAT4) || defined(SQLITE_DEBUG)
/*
** Return the latest reusable register in the set of all registers.
** The value returned is no less than iMin.  If any register iMin or
** greater is in permanent use, then return one more than that last
** permanent register.
*/
SQLITE_PRIVATE int sqlite3FirstAvailableRegister(Parse *pParse, int iMin){
  const ExprList *pList = pParse->pConstExpr;
  if( pList ){
    int i;
    for(i=0; i<pList->nExpr; i++){
      if( pList->a[i].u.iConstExprReg>=iMin ){
        iMin = pList->a[i].u.iConstExprReg + 1;
      }
    }
  }
  pParse->nTempReg = 0;
  pParse->nRangeReg = 0;
  return iMin;
}
#endif /* SQLITE_ENABLE_STAT4 || SQLITE_DEBUG */

/*
** Validate that no temporary register falls within the range of
** iFirst..iLast, inclusive.  This routine is only call from within assert()
** statements.
*/
#ifdef SQLITE_DEBUG
SQLITE_PRIVATE int sqlite3NoTempsInRange(Parse *pParse, int iFirst, int iLast){
  int i;
  if( pParse->nRangeReg>0
   && pParse->iRangeReg+pParse->nRangeReg > iFirst
   && pParse->iRangeReg <= iLast
  ){
     return 0;
  }
  for(i=0; i<pParse->nTempReg; i++){
    if( pParse->aTempReg[i]>=iFirst && pParse->aTempReg[i]<=iLast ){
      return 0;
    }
  }
  if( pParse->pConstExpr ){
    ExprList *pList = pParse->pConstExpr;
    for(i=0; i<pList->nExpr; i++){
      int iReg = pList->a[i].u.iConstExprReg;
      if( iReg==0 ) continue;
      if( iReg>=iFirst && iReg<=iLast ) return 0;
    }
  }
  return 1;
}
#endif /* SQLITE_DEBUG */

