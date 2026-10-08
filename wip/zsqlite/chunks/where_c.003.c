#endif /* SQLITE_OMIT_AUTOMATIC_INDEX */

/*
** Generate bytecode that will initialize a Bloom filter that is appropriate
** for pLevel.
**
** If there are inner loops within pLevel that have the WHERE_BLOOMFILTER
** flag set, initialize a Bloomfilter for them as well.  Except don't do
** this recursive initialization if the SQLITE_BloomPulldown optimization has
** been turned off.
**
** When the Bloom filter is initialized, the WHERE_BLOOMFILTER flag is cleared
** from the loop, but the regFilter value is set to a register that implements
** the Bloom filter.  When regFilter is positive, the
** sqlite3WhereCodeOneLoopStart() will generate code to test the Bloom filter
** and skip the subsequence B-Tree seek if the Bloom filter indicates that
** no matching rows exist.
**
** This routine may only be called if it has previously been determined that
** the loop would benefit from a Bloom filter, and the WHERE_BLOOMFILTER bit
** is set.
*/
static SQLITE_NOINLINE void sqlite3ConstructBloomFilter(
  WhereInfo *pWInfo,    /* The WHERE clause */
  int iLevel,           /* Index in pWInfo->a[] that is pLevel */
  WhereLevel *pLevel,   /* Make a Bloom filter for this FROM term */
  Bitmask notReady      /* Loops that are not ready */
){
  int addrOnce;                        /* Address of opening OP_Once */
  int addrTop;                         /* Address of OP_Rewind */
  int addrCont;                        /* Jump here to skip a row */
  const WhereTerm *pTerm;              /* For looping over WHERE clause terms */
  const WhereTerm *pWCEnd;             /* Last WHERE clause term */
  Parse *pParse = pWInfo->pParse;      /* Parsing context */
  Vdbe *v = pParse->pVdbe;             /* VDBE under construction */
  WhereLoop *pLoop = pLevel->pWLoop;   /* The loop being coded */
  int iCur;                            /* Cursor for table getting the filter */
  IndexedExpr *saved_pIdxEpr;          /* saved copy of Parse.pIdxEpr */
  IndexedExpr *saved_pIdxPartExpr;     /* saved copy of Parse.pIdxPartExpr */

  saved_pIdxEpr = pParse->pIdxEpr;
  saved_pIdxPartExpr = pParse->pIdxPartExpr;
  pParse->pIdxEpr = 0;
  pParse->pIdxPartExpr = 0;

  assert( pLoop!=0 );
  assert( v!=0 );
  assert( pLoop->wsFlags & WHERE_BLOOMFILTER );
  assert( (pLoop->wsFlags & WHERE_IDX_ONLY)==0 );

  addrOnce = sqlite3VdbeAddOp0(v, OP_Once); VdbeCoverage(v);
  do{
    const SrcList *pTabList;
    const SrcItem *pItem;
    const Table *pTab;
    u64 sz;
    int iSrc;
    sqlite3WhereExplainBloomFilter(pParse, pWInfo, pLevel);
    addrCont = sqlite3VdbeMakeLabel(pParse);
    iCur = pLevel->iTabCur;
    pLevel->regFilter = ++pParse->nMem;

    /* The Bloom filter is a Blob held in a register.  Initialize it
    ** to zero-filled blob of at least 80K bits, but maybe more if the
    ** estimated size of the table is larger.  We could actually
    ** measure the size of the table at run-time using OP_Count with
    ** P3==1 and use that value to initialize the blob.  But that makes
    ** testing complicated.  By basing the blob size on the value in the
    ** sqlite_stat1 table, testing is much easier.
    */
    pTabList = pWInfo->pTabList;
    iSrc = pLevel->iFrom;
    pItem = &pTabList->a[iSrc];
    assert( pItem!=0 );
    pTab = pItem->pTab;
    assert( pTab!=0 );
    sz = sqlite3LogEstToInt(pTab->nRowLogEst);
    if( sz<10000 ){
      sz = 10000;
    }else if( sz>10000000 ){
      sz = 10000000;
    }
    sqlite3VdbeAddOp2(v, OP_Blob, (int)sz, pLevel->regFilter);

    addrTop = sqlite3VdbeAddOp1(v, OP_Rewind, iCur); VdbeCoverage(v);
    pWCEnd = &pWInfo->sWC.a[pWInfo->sWC.nTerm];
    for(pTerm=pWInfo->sWC.a; pTerm<pWCEnd; pTerm++){
      Expr *pExpr = pTerm->pExpr;
      if( (pTerm->wtFlags & TERM_VIRTUAL)==0
       && sqlite3ExprIsSingleTableConstraint(pExpr, pTabList, iSrc, 0)
      ){
        sqlite3ExprIfFalse(pParse, pTerm->pExpr, addrCont, SQLITE_JUMPIFNULL);
      }
    }
    if( pLoop->wsFlags & WHERE_IPK ){
      int r1 = sqlite3GetTempReg(pParse);
      sqlite3VdbeAddOp2(v, OP_Rowid, iCur, r1);
      sqlite3VdbeAddOp4Int(v, OP_FilterAdd, pLevel->regFilter, 0, r1, 1);
      sqlite3ReleaseTempReg(pParse, r1);
    }else{
      Index *pIdx = pLoop->u.btree.pIndex;
      int n = pLoop->u.btree.nEq;
      int r1 = sqlite3GetTempRange(pParse, n);
      int jj;
      for(jj=0; jj<n; jj++){
        assert( pIdx->pTable==pItem->pTab );
        sqlite3ExprCodeLoadIndexColumn(pParse, pIdx, iCur, jj, r1+jj);
      }
      sqlite3VdbeAddOp4Int(v, OP_FilterAdd, pLevel->regFilter, 0, r1, n);
      sqlite3ReleaseTempRange(pParse, r1, n);
    }
    sqlite3VdbeResolveLabel(v, addrCont);
    sqlite3VdbeAddOp2(v, OP_Next, pLevel->iTabCur, addrTop+1);
    VdbeCoverage(v);
    sqlite3VdbeJumpHere(v, addrTop);
    pLoop->wsFlags &= ~WHERE_BLOOMFILTER;
    if( OptimizationDisabled(pParse->db, SQLITE_BloomPulldown) ) break;
    while( ++iLevel < pWInfo->nLevel ){
      const SrcItem *pTabItem;
      pLevel = &pWInfo->a[iLevel];
      pTabItem = &pWInfo->pTabList->a[pLevel->iFrom];
      if( pTabItem->fg.jointype & (JT_LEFT|JT_LTORJ) ) continue;
      pLoop = pLevel->pWLoop;
      if( NEVER(pLoop==0) ) continue;
      if( pLoop->prereq & notReady ) continue;
      if( (pLoop->wsFlags & (WHERE_BLOOMFILTER|WHERE_COLUMN_IN))
                 ==WHERE_BLOOMFILTER
      ){
        /* This is a candidate for bloom-filter pull-down (early evaluation).
        ** The test that WHERE_COLUMN_IN is omitted is important, as we are
        ** not able to do early evaluation of bloom filters that make use of
        ** the IN operator */
        break;
      }
    }
  }while( iLevel < pWInfo->nLevel );
  sqlite3VdbeJumpHere(v, addrOnce);
  pParse->pIdxEpr = saved_pIdxEpr;
  pParse->pIdxPartExpr = saved_pIdxPartExpr;
}


#ifndef SQLITE_OMIT_VIRTUALTABLE
/*
** Allocate and populate an sqlite3_index_info structure. It is the
** responsibility of the caller to eventually release the structure
** by passing the pointer returned by this function to freeIndexInfo().
*/
static sqlite3_index_info *allocateIndexInfo(
  WhereInfo *pWInfo,              /* The WHERE clause */
  WhereClause *pWC,               /* The WHERE clause being analyzed */
  Bitmask mUnusable,              /* Ignore terms with these prereqs */
  SrcItem *pSrc,                  /* The FROM clause term that is the vtab */
  u16 *pmNoOmit                   /* Mask of terms not to omit */
){
  int i, j;
  int nTerm;
  Parse *pParse = pWInfo->pParse;
  struct sqlite3_index_constraint *pIdxCons;
  struct sqlite3_index_orderby *pIdxOrderBy;
  struct sqlite3_index_constraint_usage *pUsage;
  struct HiddenIndexInfo *pHidden;
  WhereTerm *pTerm;
  int nOrderBy;
  sqlite3_index_info *pIdxInfo;
  u16 mNoOmit = 0;
  const Table *pTab;
  int eDistinct = 0;
  ExprList *pOrderBy = pWInfo->pOrderBy;

  assert( pSrc!=0 );
  pTab = pSrc->pTab;
  assert( pTab!=0 );
  assert( IsVirtual(pTab) );

  /* Find all WHERE clause constraints referring to this virtual table.
  ** Mark each term with the TERM_OK flag.  Set nTerm to the number of
  ** terms found.
  */
  for(i=nTerm=0, pTerm=pWC->a; i<pWC->nTerm; i++, pTerm++){
    pTerm->wtFlags &= ~TERM_OK;
    if( pTerm->leftCursor != pSrc->iCursor ) continue;
    if( pTerm->prereqRight & mUnusable ) continue;
    assert( IsPowerOfTwo(pTerm->eOperator & ~WO_EQUIV) );
    testcase( pTerm->eOperator & WO_IN );
    testcase( pTerm->eOperator & WO_ISNULL );
    testcase( pTerm->eOperator & WO_IS );
    testcase( pTerm->eOperator & WO_ALL );
    if( (pTerm->eOperator & ~(WO_EQUIV))==0 ) continue;
    if( pTerm->wtFlags & TERM_VNULL ) continue;

    assert( (pTerm->eOperator & (WO_OR|WO_AND))==0 );
    assert( pTerm->u.x.leftColumn>=XN_ROWID );
    assert( pTerm->u.x.leftColumn<pTab->nCol );
    if( (pSrc->fg.jointype & (JT_LEFT|JT_LTORJ|JT_RIGHT))!=0
     && !constraintCompatibleWithOuterJoin(pTerm,pSrc)
    ){
      continue;
    }
    nTerm++;
    pTerm->wtFlags |= TERM_OK;
  }

  /* If the ORDER BY clause contains only columns in the current
  ** virtual table then allocate space for the aOrderBy part of
  ** the sqlite3_index_info structure.
  */
  nOrderBy = 0;
  if( pOrderBy ){
    int n = pOrderBy->nExpr;
    for(i=0; i<n; i++){
      Expr *pExpr = pOrderBy->a[i].pExpr;
      Expr *pE2;

      /* Skip over constant terms in the ORDER BY clause */
      if( sqlite3ExprIsConstant(0, pExpr) ){
        continue;
      }

      /* Virtual tables are unable to deal with NULLS FIRST */
      if( pOrderBy->a[i].fg.sortFlags & KEYINFO_ORDER_BIGNULL ) break;

      /* First case - a direct column references without a COLLATE operator */
      if( pExpr->op==TK_COLUMN && pExpr->iTable==pSrc->iCursor ){
        assert( pExpr->iColumn>=XN_ROWID && pExpr->iColumn<pTab->nCol );
        continue;
      }

      /* 2nd case - a column reference with a COLLATE operator.  Only match
      ** of the COLLATE operator matches the collation of the column. */
      if( pExpr->op==TK_COLLATE
       && (pE2 = pExpr->pLeft)->op==TK_COLUMN
       && pE2->iTable==pSrc->iCursor
      ){
        const char *zColl;  /* The collating sequence name */
        assert( !ExprHasProperty(pExpr, EP_IntValue) );
        assert( pExpr->u.zToken!=0 );
        assert( pE2->iColumn>=XN_ROWID && pE2->iColumn<pTab->nCol );
        pExpr->iColumn = pE2->iColumn;
        if( pE2->iColumn<0 ) continue;  /* Collseq does not matter for rowid */
        zColl = sqlite3ColumnColl(&pTab->aCol[pE2->iColumn]);
        if( zColl==0 ) zColl = sqlite3StrBINARY;
        if( sqlite3_stricmp(pExpr->u.zToken, zColl)==0 ) continue;
      }

      /* No matches cause a break out of the loop */
      break;
    }
    if( i==n ){
      nOrderBy = n;
      if( (pWInfo->wctrlFlags & WHERE_DISTINCTBY) && !pSrc->fg.rowidUsed ){
        eDistinct = 2 + ((pWInfo->wctrlFlags & WHERE_SORTBYGROUP)!=0);
      }else if( pWInfo->wctrlFlags & WHERE_GROUPBY ){
        eDistinct = 1;
      }
    }
  }

  /* Allocate the sqlite3_index_info structure
  */
  pIdxInfo = sqlite3DbMallocZero(pParse->db, sizeof(*pIdxInfo)
                           + (sizeof(*pIdxCons) + sizeof(*pUsage))*nTerm
                           + sizeof(*pIdxOrderBy)*nOrderBy + sizeof(*pHidden)
                           + sizeof(sqlite3_value*)*nTerm );
  if( pIdxInfo==0 ){
    sqlite3ErrorMsg(pParse, "out of memory");
    return 0;
  }
  pHidden = (struct HiddenIndexInfo*)&pIdxInfo[1];
  pIdxCons = (struct sqlite3_index_constraint*)&pHidden->aRhs[nTerm];
  pIdxOrderBy = (struct sqlite3_index_orderby*)&pIdxCons[nTerm];
  pUsage = (struct sqlite3_index_constraint_usage*)&pIdxOrderBy[nOrderBy];
  pIdxInfo->aConstraint = pIdxCons;
  pIdxInfo->aOrderBy = pIdxOrderBy;
  pIdxInfo->aConstraintUsage = pUsage;
  pHidden->pWC = pWC;
  pHidden->pParse = pParse;
  pHidden->eDistinct = eDistinct;
  pHidden->mIn = 0;
  for(i=j=0, pTerm=pWC->a; i<pWC->nTerm; i++, pTerm++){
    u16 op;
    if( (pTerm->wtFlags & TERM_OK)==0 ) continue;
    pIdxCons[j].iColumn = pTerm->u.x.leftColumn;
    pIdxCons[j].iTermOffset = i;
    op = pTerm->eOperator & WO_ALL;
    if( op==WO_IN ){
      if( (pTerm->wtFlags & TERM_SLICE)==0 ){
        pHidden->mIn |= SMASKBIT32(j);
      }
      op = WO_EQ;
    }
    if( op==WO_AUX ){
      pIdxCons[j].op = pTerm->eMatchOp;
    }else if( op & (WO_ISNULL|WO_IS) ){
      if( op==WO_ISNULL ){
        pIdxCons[j].op = SQLITE_INDEX_CONSTRAINT_ISNULL;
      }else{
        pIdxCons[j].op = SQLITE_INDEX_CONSTRAINT_IS;
      }
    }else{
      pIdxCons[j].op = (u8)op;
      /* The direct assignment in the previous line is possible only because
      ** the WO_ and SQLITE_INDEX_CONSTRAINT_ codes are identical.  The
      ** following asserts verify this fact. */
      assert( WO_EQ==SQLITE_INDEX_CONSTRAINT_EQ );
      assert( WO_LT==SQLITE_INDEX_CONSTRAINT_LT );
      assert( WO_LE==SQLITE_INDEX_CONSTRAINT_LE );
      assert( WO_GT==SQLITE_INDEX_CONSTRAINT_GT );
      assert( WO_GE==SQLITE_INDEX_CONSTRAINT_GE );
      assert( pTerm->eOperator&(WO_IN|WO_EQ|WO_LT|WO_LE|WO_GT|WO_GE|WO_AUX) );

      if( op & (WO_LT|WO_LE|WO_GT|WO_GE)
       && sqlite3ExprIsVector(pTerm->pExpr->pRight)
      ){
        testcase( j!=i );
        if( j<16 ) mNoOmit |= (1 << j);
        if( op==WO_LT ) pIdxCons[j].op = WO_LE;
        if( op==WO_GT ) pIdxCons[j].op = WO_GE;
      }
    }

    j++;
  }
  assert( j==nTerm );
  pIdxInfo->nConstraint = j;
  for(i=j=0; i<nOrderBy; i++){
    Expr *pExpr = pOrderBy->a[i].pExpr;
    if( sqlite3ExprIsConstant(0, pExpr) ) continue;
    assert( pExpr->op==TK_COLUMN
         || (pExpr->op==TK_COLLATE && pExpr->pLeft->op==TK_COLUMN
              && pExpr->iColumn==pExpr->pLeft->iColumn) );
    pIdxOrderBy[j].iColumn = pExpr->iColumn;
    pIdxOrderBy[j].desc = pOrderBy->a[i].fg.sortFlags & KEYINFO_ORDER_DESC;
    j++;
  }
  pIdxInfo->nOrderBy = j;

  *pmNoOmit = mNoOmit;
  return pIdxInfo;
}

/*
** Free an sqlite3_index_info structure allocated by allocateIndexInfo()
** and possibly modified by xBestIndex methods.
*/
static void freeIndexInfo(sqlite3 *db, sqlite3_index_info *pIdxInfo){
  HiddenIndexInfo *pHidden;
  int i;
  assert( pIdxInfo!=0 );
  pHidden = (HiddenIndexInfo*)&pIdxInfo[1];
  assert( pHidden->pParse!=0 );
  assert( pHidden->pParse->db==db );
  for(i=0; i<pIdxInfo->nConstraint; i++){
    sqlite3ValueFree(pHidden->aRhs[i]); /* IMP: R-14553-25174 */
    pHidden->aRhs[i] = 0;
  }
  sqlite3DbFree(db, pIdxInfo);
}
