
/*
** The pTruth expression is always true because it is the WHERE clause
** a partial index that is driving a query loop.  Look through all of the
** WHERE clause terms on the query, and if any of those terms must be
** true because pTruth is true, then mark those WHERE clause terms as
** coded.
*/
static void whereApplyPartialIndexConstraints(
  Expr *pTruth,
  int iTabCur,
  WhereClause *pWC
){
  int i;
  WhereTerm *pTerm;
  while( pTruth->op==TK_AND ){
    whereApplyPartialIndexConstraints(pTruth->pLeft, iTabCur, pWC);
    pTruth = pTruth->pRight;
  }
  for(i=0, pTerm=pWC->a; i<pWC->nTerm; i++, pTerm++){
    Expr *pExpr;
    if( pTerm->wtFlags & TERM_CODED ) continue;
    pExpr = pTerm->pExpr;
    if( sqlite3ExprCompare(0, pExpr, pTruth, iTabCur)==0 ){
      pTerm->wtFlags |= TERM_CODED;
    }
  }
}

/*
** This routine is called right after An OP_Filter has been generated and
** before the corresponding index search has been performed.  This routine
** checks to see if there are additional Bloom filters in inner loops that
** can be checked prior to doing the index lookup.  If there are available
** inner-loop Bloom filters, then evaluate those filters now, before the
** index lookup.  The idea is that a Bloom filter check is way faster than
** an index lookup, and the Bloom filter might return false, meaning that
** the index lookup can be skipped.
**
** We know that an inner loop uses a Bloom filter because it has the
** WhereLevel.regFilter set.  If an inner-loop Bloom filter is checked,
** then clear the WhereLevel.regFilter value to prevent the Bloom filter
** from being checked a second time when the inner loop is evaluated.
*/
static SQLITE_NOINLINE void filterPullDown(
  Parse *pParse,       /* Parsing context */
  WhereInfo *pWInfo,   /* Complete information about the WHERE clause */
  int iLevel,          /* Which level of pWInfo->a[] should be coded */
  int addrNxt,         /* Jump here to bypass inner loops */
  Bitmask notReady     /* Loops that are not ready */
){
  while( ++iLevel < pWInfo->nLevel ){
    WhereLevel *pLevel = &pWInfo->a[iLevel];
    WhereLoop *pLoop = pLevel->pWLoop;
    if( pLevel->regFilter==0 ) continue;
    if( pLevel->pWLoop->nSkip ) continue;
    /*         ,--- Because sqlite3ConstructBloomFilter() has will not have set
    **  vvvvv--'    pLevel->regFilter if this were true. */
    if( NEVER(pLoop->prereq & notReady) ) continue;
    assert( pLevel->addrBrk==0 );
    pLevel->addrBrk = addrNxt;
    if( pLoop->wsFlags & WHERE_IPK ){
      WhereTerm *pTerm = pLoop->aLTerm[0];
      int regRowid;
      assert( pTerm!=0 );
      assert( pTerm->pExpr!=0 );
      testcase( pTerm->wtFlags & TERM_VIRTUAL );
      regRowid = sqlite3GetTempReg(pParse);
      regRowid = codeEqualityTerm(pParse, pTerm, pLevel, 0, 0, regRowid);
      sqlite3VdbeAddOp2(pParse->pVdbe, OP_MustBeInt, regRowid, addrNxt);
      VdbeCoverage(pParse->pVdbe);
      sqlite3VdbeAddOp4Int(pParse->pVdbe, OP_Filter, pLevel->regFilter,
                           addrNxt, regRowid, 1);
      VdbeCoverage(pParse->pVdbe);
    }else{
      u16 nEq = pLoop->u.btree.nEq;
      int r1;
      char *zStartAff;

      assert( pLoop->wsFlags & WHERE_INDEXED );
      assert( (pLoop->wsFlags & WHERE_COLUMN_IN)==0 );
      r1 = codeAllEqualityTerms(pParse,pLevel,0,0,&zStartAff);
      codeApplyAffinity(pParse, r1, nEq, zStartAff);
      sqlite3DbFree(pParse->db, zStartAff);
      sqlite3VdbeAddOp4Int(pParse->pVdbe, OP_Filter, pLevel->regFilter,
                           addrNxt, r1, nEq);
      VdbeCoverage(pParse->pVdbe);
    }
    pLevel->regFilter = 0;
    pLevel->addrBrk = 0;
  }
}

/*
** Loop pLoop is a WHERE_INDEXED level that uses at least one IN(...)
** operator. Return true if level pLoop is guaranteed to visit only one
** row for each key generated for the index.
*/
static int whereLoopIsOneRow(WhereLoop *pLoop){
  if( pLoop->u.btree.pIndex->onError
   && pLoop->nSkip==0
   && pLoop->u.btree.nEq==pLoop->u.btree.pIndex->nKeyCol
  ){
    int ii;
    for(ii=0; ii<pLoop->u.btree.nEq; ii++){
      if( pLoop->aLTerm[ii]->eOperator & (WO_IS|WO_ISNULL) ){
        return 0;
      }
    }
    return 1;
  }
  return 0;
}

/*
** Generate code for the start of the iLevel-th loop in the WHERE clause
** implementation described by pWInfo.
*/
SQLITE_PRIVATE Bitmask sqlite3WhereCodeOneLoopStart(
  Parse *pParse,       /* Parsing context */
  Vdbe *v,             /* Prepared statement under construction */
  WhereInfo *pWInfo,   /* Complete information about the WHERE clause */
  int iLevel,          /* Which level of pWInfo->a[] should be coded */
  WhereLevel *pLevel,  /* The current level pointer */
  Bitmask notReady     /* Which tables are currently available */
){
  int j, k;            /* Loop counters */
  int iCur;            /* The VDBE cursor for the table */
  int addrNxt;         /* Where to jump to continue with the next IN case */
  int bRev;            /* True if we need to scan in reverse order */
  WhereLoop *pLoop;    /* The WhereLoop object being coded */
  WhereClause *pWC;    /* Decomposition of the entire WHERE clause */
  WhereTerm *pTerm;               /* A WHERE clause term */
  sqlite3 *db;                    /* Database connection */
  SrcItem *pTabItem;              /* FROM clause term being coded */
  int addrBrk;                    /* Jump here to break out of the loop */
  int addrHalt;                   /* addrBrk for the outermost loop */
  int addrCont;                   /* Jump here to continue with next cycle */
  int iRowidReg = 0;        /* Rowid is stored in this register, if not zero */
  int iReleaseReg = 0;      /* Temp register to free before returning */
  Index *pIdx = 0;          /* Index used by loop (if any) */
  int iLoop;                /* Iteration of constraint generator loop */

  pWC = &pWInfo->sWC;
  db = pParse->db;
  pLoop = pLevel->pWLoop;
  pTabItem = &pWInfo->pTabList->a[pLevel->iFrom];
  iCur = pTabItem->iCursor;
  pLevel->notReady = notReady & ~sqlite3WhereGetMask(&pWInfo->sMaskSet, iCur);
  bRev = (pWInfo->revMask>>iLevel)&1;
  VdbeModuleComment((v, "Begin WHERE-loop%d: %s",iLevel,pTabItem->pTab->zName));
#if WHERETRACE_ENABLED /* 0x4001 */
  if( sqlite3WhereTrace & 0x1 ){
    sqlite3DebugPrintf("Coding level %d of %d:  notReady=%llx  iFrom=%d\n",
       iLevel, pWInfo->nLevel, (u64)notReady, pLevel->iFrom);
    if( sqlite3WhereTrace & 0x1000 ){
      sqlite3WhereLoopPrint(pLoop, pWC);
    }
  }
  if( (sqlite3WhereTrace & 0x4001)==0x4001 ){
    if( iLevel==0 ){
      sqlite3DebugPrintf("WHERE clause being coded:\n");
      sqlite3TreeViewExpr(0, pWInfo->pWhere, 0);
    }
    sqlite3DebugPrintf("All WHERE-clause terms before coding:\n");
    sqlite3WhereClausePrint(pWC);
  }
#endif

  /* Create labels for the "break" and "continue" instructions
  ** for the current loop.  Jump to addrBrk to break out of a loop.
  ** Jump to cont to go immediately to the next iteration of the
  ** loop.
  **
  ** When there is an IN operator, we also have a "addrNxt" label that
  ** means to continue with the next IN value combination.  When
  ** there are no IN operators in the constraints, the "addrNxt" label
  ** is the same as "addrBrk".
  */
  addrBrk = pLevel->addrBrk = pLevel->addrNxt = sqlite3VdbeMakeLabel(pParse);
  addrCont = pLevel->addrCont = sqlite3VdbeMakeLabel(pParse);

  /* If this is the right table of a LEFT OUTER JOIN, allocate and
  ** initialize a memory cell that records if this table matches any
  ** row of the left table of the join.
  */
  assert( (pWInfo->wctrlFlags & (WHERE_OR_SUBCLAUSE|WHERE_RIGHT_JOIN))
       || pLevel->iFrom>0 || (pTabItem[0].fg.jointype & JT_LEFT)==0
  );
  if( pLevel->iFrom>0 && (pTabItem[0].fg.jointype & JT_LEFT)!=0 ){
    pLevel->iLeftJoin = ++pParse->nMem;
    sqlite3VdbeAddOp2(v, OP_Integer, 0, pLevel->iLeftJoin);
    VdbeComment((v, "init LEFT JOIN match flag"));
  }

  /* Compute a safe address to jump to if we discover that the table for
  ** this loop is empty and can never contribute content. */
  for(j=iLevel; j>0; j--){
    if( pWInfo->a[j].iLeftJoin ) break;
    if( pWInfo->a[j].pRJ ) break;
  }
  addrHalt = pWInfo->a[j].addrBrk;

  /* Special case of a FROM clause subquery implemented as a co-routine */
  if( pTabItem->fg.viaCoroutine ){
    int regYield = pTabItem->regReturn;
    sqlite3VdbeAddOp3(v, OP_InitCoroutine, regYield, 0, pTabItem->addrFillSub);
    pLevel->p2 =  sqlite3VdbeAddOp2(v, OP_Yield, regYield, addrBrk);
    VdbeCoverage(v);
    VdbeComment((v, "next row of %s", pTabItem->pTab->zName));
    pLevel->op = OP_Goto;
  }else

#ifndef SQLITE_OMIT_VIRTUALTABLE
  if(  (pLoop->wsFlags & WHERE_VIRTUALTABLE)!=0 ){
    /* Case 1:  The table is a virtual-table.  Use the VFilter and VNext
    **          to access the data.
    */
    int iReg;   /* P3 Value for OP_VFilter */
    int addrNotFound;
    int nConstraint = pLoop->nLTerm;

    iReg = sqlite3GetTempRange(pParse, nConstraint+2);
    addrNotFound = pLevel->addrBrk;
    for(j=0; j<nConstraint; j++){
      int iTarget = iReg+j+2;
      pTerm = pLoop->aLTerm[j];
      if( NEVER(pTerm==0) ) continue;
      if( pTerm->eOperator & WO_IN ){
        if( SMASKBIT32(j) & pLoop->u.vtab.mHandleIn ){
          int iTab = pParse->nTab++;
          int iCache = ++pParse->nMem;
          sqlite3CodeRhsOfIN(pParse, pTerm->pExpr, iTab);
          sqlite3VdbeAddOp3(v, OP_VInitIn, iTab, iTarget, iCache);
        }else{
          codeEqualityTerm(pParse, pTerm, pLevel, j, bRev, iTarget);
          addrNotFound = pLevel->addrNxt;
        }
      }else{
        Expr *pRight = pTerm->pExpr->pRight;
        codeExprOrVector(pParse, pRight, iTarget, 1);
        if( pTerm->eMatchOp==SQLITE_INDEX_CONSTRAINT_OFFSET
         && pLoop->u.vtab.bOmitOffset
        ){
          assert( pTerm->eOperator==WO_AUX );
          assert( pWInfo->pSelect!=0 );
          assert( pWInfo->pSelect->iOffset>0 );
          sqlite3VdbeAddOp2(v, OP_Integer, 0, pWInfo->pSelect->iOffset);
          VdbeComment((v,"Zero OFFSET counter"));
        }
      }
    }
    sqlite3VdbeAddOp2(v, OP_Integer, pLoop->u.vtab.idxNum, iReg);
    sqlite3VdbeAddOp2(v, OP_Integer, nConstraint, iReg+1);
    sqlite3VdbeAddOp4(v, OP_VFilter, iCur, addrNotFound, iReg,
                      pLoop->u.vtab.idxStr,
                      pLoop->u.vtab.needFree ? P4_DYNAMIC : P4_STATIC);
    VdbeCoverage(v);
    pLoop->u.vtab.needFree = 0;
    /* An OOM inside of AddOp4(OP_VFilter) instruction above might have freed
    ** the u.vtab.idxStr.  NULL it out to prevent a use-after-free */
    if( db->mallocFailed ) pLoop->u.vtab.idxStr = 0;
    pLevel->p1 = iCur;
    pLevel->op = pWInfo->eOnePass ? OP_Noop : OP_VNext;
    pLevel->p2 = sqlite3VdbeCurrentAddr(v);
    assert( (pLoop->wsFlags & WHERE_MULTI_OR)==0 );

    for(j=0; j<nConstraint; j++){
      pTerm = pLoop->aLTerm[j];
      if( j<16 && (pLoop->u.vtab.omitMask>>j)&1 ){
        disableTerm(pLevel, pTerm);
        continue;
      }
      if( (pTerm->eOperator & WO_IN)!=0
       && (SMASKBIT32(j) & pLoop->u.vtab.mHandleIn)==0
       && !db->mallocFailed
      ){
        Expr *pCompare;  /* The comparison operator */
        Expr *pRight;    /* RHS of the comparison */
        VdbeOp *pOp;     /* Opcode to access the value of the IN constraint */
        int iIn;         /* IN loop corresponding to the j-th constraint */

        /* Reload the constraint value into reg[iReg+j+2].  The same value
        ** was loaded into the same register prior to the OP_VFilter, but
        ** the xFilter implementation might have changed the datatype or
        ** encoding of the value in the register, so it *must* be reloaded.
        */
        for(iIn=0; ALWAYS(iIn<pLevel->u.in.nIn); iIn++){
          pOp = sqlite3VdbeGetOp(v, pLevel->u.in.aInLoop[iIn].addrInTop);
          if( (pOp->opcode==OP_Column && pOp->p3==iReg+j+2)
           || (pOp->opcode==OP_Rowid && pOp->p2==iReg+j+2)
          ){
            testcase( pOp->opcode==OP_Rowid );
            sqlite3VdbeAddOp3(v, pOp->opcode, pOp->p1, pOp->p2, pOp->p3);
            break;
          }
        }

        /* Generate code that will continue to the next row if
        ** the IN constraint is not satisfied
        */
        pCompare = sqlite3PExpr(pParse, TK_EQ, 0, 0);
        if( !db->mallocFailed ){
          int iFld = pTerm->u.x.iField;
          Expr *pLeft = pTerm->pExpr->pLeft;
          assert( pLeft!=0 );
          if( iFld>0 ){
            assert( pLeft->op==TK_VECTOR );
            assert( ExprUseXList(pLeft) );
            assert( iFld<=pLeft->x.pList->nExpr );
            pCompare->pLeft = pLeft->x.pList->a[iFld-1].pExpr;
          }else{
            pCompare->pLeft = pLeft;
          }
          pCompare->pRight = pRight = sqlite3Expr(db, TK_REGISTER, 0);
          if( pRight ){
            pRight->iTable = iReg+j+2;
            sqlite3ExprIfFalse(
                pParse, pCompare, pLevel->addrCont, SQLITE_JUMPIFNULL
            );
          }
          pCompare->pLeft = 0;
        }
        sqlite3ExprDelete(db, pCompare);
      }
    }

    /* These registers need to be preserved in case there is an IN operator
    ** loop.  So we could deallocate the registers here (and potentially
    ** reuse them later) if (pLoop->wsFlags & WHERE_IN_ABLE)==0.  But it seems
    ** simpler and safer to simply not reuse the registers.
    **
    **    sqlite3ReleaseTempRange(pParse, iReg, nConstraint+2);
    */
  }else
#endif /* SQLITE_OMIT_VIRTUALTABLE */

  if( (pLoop->wsFlags & WHERE_IPK)!=0
   && (pLoop->wsFlags & (WHERE_COLUMN_IN|WHERE_COLUMN_EQ))!=0
  ){
    /* Case 2:  We can directly reference a single row using an
    **          equality comparison against the ROWID field.  Or
    **          we reference multiple rows using a "rowid IN (...)"
    **          construct.
    */
    assert( pLoop->u.btree.nEq==1 );
    pTerm = pLoop->aLTerm[0];
    assert( pTerm!=0 );
    assert( pTerm->pExpr!=0 );
    testcase( pTerm->wtFlags & TERM_VIRTUAL );
