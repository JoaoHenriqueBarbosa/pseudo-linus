
/*
** Generate the code for the loop that finds all non-matched terms
** for a RIGHT JOIN.
*/
SQLITE_PRIVATE SQLITE_NOINLINE void sqlite3WhereRightJoinLoop(
  WhereInfo *pWInfo,
  int iLevel,
  WhereLevel *pLevel
){
  Parse *pParse = pWInfo->pParse;
  Vdbe *v = pParse->pVdbe;
  WhereRightJoin *pRJ = pLevel->pRJ;
  Expr *pSubWhere = 0;
  WhereClause *pWC = &pWInfo->sWC;
  WhereInfo *pSubWInfo;
  WhereLoop *pLoop = pLevel->pWLoop;
  SrcItem *pTabItem = &pWInfo->pTabList->a[pLevel->iFrom];
  SrcList sFrom;
  Bitmask mAll = 0;
  int k;

  ExplainQueryPlan((pParse, 1, "RIGHT-JOIN %s", pTabItem->pTab->zName));
  sqlite3VdbeNoJumpsOutsideSubrtn(v, pRJ->addrSubrtn, pRJ->endSubrtn,
                                  pRJ->regReturn);
  for(k=0; k<iLevel; k++){
    int iIdxCur;
    SrcItem *pRight;
    assert( pWInfo->a[k].pWLoop->iTab == pWInfo->a[k].iFrom );
    pRight = &pWInfo->pTabList->a[pWInfo->a[k].iFrom];
    mAll |= pWInfo->a[k].pWLoop->maskSelf;
    if( pRight->fg.viaCoroutine ){
      sqlite3VdbeAddOp3(
          v, OP_Null, 0, pRight->regResult,
          pRight->regResult + pRight->pSelect->pEList->nExpr-1
      );
    }
    sqlite3VdbeAddOp1(v, OP_NullRow, pWInfo->a[k].iTabCur);
    iIdxCur = pWInfo->a[k].iIdxCur;
    if( iIdxCur ){
      sqlite3VdbeAddOp1(v, OP_NullRow, iIdxCur);
    }
  }
  if( (pTabItem->fg.jointype & JT_LTORJ)==0 ){
    mAll |= pLoop->maskSelf;
    for(k=0; k<pWC->nTerm; k++){
      WhereTerm *pTerm = &pWC->a[k];
      if( (pTerm->wtFlags & (TERM_VIRTUAL|TERM_SLICE))!=0
       && pTerm->eOperator!=WO_ROWVAL
      ){
        break;
      }
      if( pTerm->prereqAll & ~mAll ) continue;
      if( ExprHasProperty(pTerm->pExpr, EP_OuterON|EP_InnerON) ) continue;
      pSubWhere = sqlite3ExprAnd(pParse, pSubWhere,
                                 sqlite3ExprDup(pParse->db, pTerm->pExpr, 0));
    }
  }
  sFrom.nSrc = 1;
  sFrom.nAlloc = 1;
  memcpy(&sFrom.a[0], pTabItem, sizeof(SrcItem));
  sFrom.a[0].fg.jointype = 0;
  assert( pParse->withinRJSubrtn < 100 );
  pParse->withinRJSubrtn++;
  pSubWInfo = sqlite3WhereBegin(pParse, &sFrom, pSubWhere, 0, 0, 0,
                                WHERE_RIGHT_JOIN, 0);
  if( pSubWInfo ){
    int iCur = pLevel->iTabCur;
    int r = ++pParse->nMem;
    int nPk;
    int jmp;
    int addrCont = sqlite3WhereContinueLabel(pSubWInfo);
    Table *pTab = pTabItem->pTab;
    if( HasRowid(pTab) ){
      sqlite3ExprCodeGetColumnOfTable(v, pTab, iCur, -1, r);
      nPk = 1;
    }else{
      int iPk;
      Index *pPk = sqlite3PrimaryKeyIndex(pTab);
      nPk = pPk->nKeyCol;
      pParse->nMem += nPk - 1;
      for(iPk=0; iPk<nPk; iPk++){
        int iCol = pPk->aiColumn[iPk];
        sqlite3ExprCodeGetColumnOfTable(v, pTab, iCur, iCol,r+iPk);
      }
    }
    jmp = sqlite3VdbeAddOp4Int(v, OP_Filter, pRJ->regBloom, 0, r, nPk);
    VdbeCoverage(v);
    sqlite3VdbeAddOp4Int(v, OP_Found, pRJ->iMatch, addrCont, r, nPk);
    VdbeCoverage(v);
    sqlite3VdbeJumpHere(v, jmp);
    sqlite3VdbeAddOp2(v, OP_Gosub, pRJ->regReturn, pRJ->addrSubrtn);
    sqlite3WhereEnd(pSubWInfo);
  }
  sqlite3ExprDelete(pParse->db, pSubWhere);
  ExplainQueryPlanPop(pParse);
  assert( pParse->withinRJSubrtn>0 );
  pParse->withinRJSubrtn--;
}

