    /* Run a separate WHERE clause for each term of the OR clause.  After
    ** eliminating duplicates from other WHERE clauses, the action for each
    ** sub-WHERE clause is to to invoke the main loop body as a subroutine.
    */
    ExplainQueryPlan((pParse, 1, "MULTI-INDEX OR"));
    for(ii=0; ii<pOrWc->nTerm; ii++){
      WhereTerm *pOrTerm = &pOrWc->a[ii];
      if( pOrTerm->leftCursor==iCur || (pOrTerm->eOperator & WO_AND)!=0 ){
        WhereInfo *pSubWInfo;           /* Info for single OR-term scan */
        Expr *pOrExpr = pOrTerm->pExpr; /* Current OR clause term */
        Expr *pDelete;                  /* Local copy of OR clause term */
        int jmp1 = 0;                   /* Address of jump operation */
        testcase( (pTabItem[0].fg.jointype & JT_LEFT)!=0
               && !ExprHasProperty(pOrExpr, EP_OuterON)
        ); /* See TH3 vtab25.400 and ticket 614b25314c766238 */
        pDelete = pOrExpr = sqlite3ExprDup(db, pOrExpr, 0);
        if( db->mallocFailed ){
          sqlite3ExprDelete(db, pDelete);
          continue;
        }
        if( pAndExpr ){
          pAndExpr->pLeft = pOrExpr;
          pOrExpr = pAndExpr;
        }
        /* Loop through table entries that match term pOrTerm. */
        ExplainQueryPlan((pParse, 1, "INDEX %d", ii+1));
        WHERETRACE(0xffffffff, ("Subplan for OR-clause:\n"));
        pSubWInfo = sqlite3WhereBegin(pParse, pOrTab, pOrExpr, 0, 0, 0,
                                      WHERE_OR_SUBCLAUSE, iCovCur);
        assert( pSubWInfo || pParse->nErr );
        if( pSubWInfo ){
          WhereLoop *pSubLoop;
          int addrExplain = sqlite3WhereExplainOneScan(
              pParse, pOrTab, &pSubWInfo->a[0], 0
          );
          sqlite3WhereAddScanStatus(v, pOrTab, &pSubWInfo->a[0], addrExplain);

          /* This is the sub-WHERE clause body.  First skip over
          ** duplicate rows from prior sub-WHERE clauses, and record the
          ** rowid (or PRIMARY KEY) for the current row so that the same
          ** row will be skipped in subsequent sub-WHERE clauses.
          */
          if( (pWInfo->wctrlFlags & WHERE_DUPLICATES_OK)==0 ){
            int iSet = ((ii==pOrWc->nTerm-1)?-1:ii);
            if( HasRowid(pTab) ){
              sqlite3ExprCodeGetColumnOfTable(v, pTab, iCur, -1, regRowid);
              jmp1 = sqlite3VdbeAddOp4Int(v, OP_RowSetTest, regRowset, 0,
                                          regRowid, iSet);
              VdbeCoverage(v);
            }else{
              Index *pPk = sqlite3PrimaryKeyIndex(pTab);
              int nPk = pPk->nKeyCol;
              int iPk;
              int r;

              /* Read the PK into an array of temp registers. */
              r = sqlite3GetTempRange(pParse, nPk);
              for(iPk=0; iPk<nPk; iPk++){
                int iCol = pPk->aiColumn[iPk];
                sqlite3ExprCodeGetColumnOfTable(v, pTab, iCur, iCol,r+iPk);
              }

              /* Check if the temp table already contains this key. If so,
              ** the row has already been included in the result set and
              ** can be ignored (by jumping past the Gosub below). Otherwise,
              ** insert the key into the temp table and proceed with processing
              ** the row.
              **
              ** Use some of the same optimizations as OP_RowSetTest: If iSet
              ** is zero, assume that the key cannot already be present in
              ** the temp table. And if iSet is -1, assume that there is no
              ** need to insert the key into the temp table, as it will never
              ** be tested for.  */
              if( iSet ){
                jmp1 = sqlite3VdbeAddOp4Int(v, OP_Found, regRowset, 0, r, nPk);
                VdbeCoverage(v);
              }
              if( iSet>=0 ){
                sqlite3VdbeAddOp3(v, OP_MakeRecord, r, nPk, regRowid);
                sqlite3VdbeAddOp4Int(v, OP_IdxInsert, regRowset, regRowid,
                                     r, nPk);
                if( iSet ) sqlite3VdbeChangeP5(v, OPFLAG_USESEEKRESULT);
              }

              /* Release the array of temp registers */
              sqlite3ReleaseTempRange(pParse, r, nPk);
            }
          }

          /* Invoke the main loop body as a subroutine */
          sqlite3VdbeAddOp2(v, OP_Gosub, regReturn, iLoopBody);

          /* Jump here (skipping the main loop body subroutine) if the
          ** current sub-WHERE row is a duplicate from prior sub-WHEREs. */
          if( jmp1 ) sqlite3VdbeJumpHere(v, jmp1);

          /* The pSubWInfo->untestedTerms flag means that this OR term
          ** contained one or more AND term from a notReady table.  The
          ** terms from the notReady table could not be tested and will
          ** need to be tested later.
          */
          if( pSubWInfo->untestedTerms ) untestedTerms = 1;

          /* If all of the OR-connected terms are optimized using the same
          ** index, and the index is opened using the same cursor number
          ** by each call to sqlite3WhereBegin() made by this loop, it may
          ** be possible to use that index as a covering index.
          **
          ** If the call to sqlite3WhereBegin() above resulted in a scan that
          ** uses an index, and this is either the first OR-connected term
          ** processed or the index is the same as that used by all previous
          ** terms, set pCov to the candidate covering index. Otherwise, set
          ** pCov to NULL to indicate that no candidate covering index will
          ** be available.
          */
          pSubLoop = pSubWInfo->a[0].pWLoop;
          assert( (pSubLoop->wsFlags & WHERE_AUTO_INDEX)==0 );
          if( (pSubLoop->wsFlags & WHERE_INDEXED)!=0
           && (ii==0 || pSubLoop->u.btree.pIndex==pCov)
           && (HasRowid(pTab) || !IsPrimaryKeyIndex(pSubLoop->u.btree.pIndex))
          ){
            assert( pSubWInfo->a[0].iIdxCur==iCovCur );
            pCov = pSubLoop->u.btree.pIndex;
          }else{
            pCov = 0;
          }
          if( sqlite3WhereUsesDeferredSeek(pSubWInfo) ){
            pWInfo->bDeferredSeek = 1;
          }

          /* Finish the loop through table entries that match term pOrTerm. */
          sqlite3WhereEnd(pSubWInfo);
          ExplainQueryPlanPop(pParse);
        }
        sqlite3ExprDelete(db, pDelete);
      }
    }
    ExplainQueryPlanPop(pParse);
    assert( pLevel->pWLoop==pLoop );
    assert( (pLoop->wsFlags & WHERE_MULTI_OR)!=0 );
    assert( (pLoop->wsFlags & WHERE_IN_ABLE)==0 );
    pLevel->u.pCoveringIdx = pCov;
    if( pCov ) pLevel->iIdxCur = iCovCur;
    if( pAndExpr ){
      pAndExpr->pLeft = 0;
      sqlite3ExprDelete(db, pAndExpr);
    }
    sqlite3VdbeChangeP1(v, iRetInit, sqlite3VdbeCurrentAddr(v));
    sqlite3VdbeGoto(v, pLevel->addrBrk);
    sqlite3VdbeResolveLabel(v, iLoopBody);

    /* Set the P2 operand of the OP_Return opcode that will end the current
    ** loop to point to this spot, which is the top of the next containing
    ** loop.  The byte-code formatter will use that P2 value as a hint to
    ** indent everything in between the this point and the final OP_Return.
    ** See tag-20220407a in vdbe.c and shell.c */
    assert( pLevel->op==OP_Return );
    pLevel->p2 = sqlite3VdbeCurrentAddr(v);

    if( pWInfo->nLevel>1 ){ sqlite3DbFreeNN(db, pOrTab); }
    if( !untestedTerms ) disableTerm(pLevel, pTerm);
  }else
#endif /* SQLITE_OMIT_OR_OPTIMIZATION */

  {
    /* Case 6:  There is no usable index.  We must do a complete
    **          scan of the entire table.
    */
    static const u8 aStep[] = { OP_Next, OP_Prev };
    static const u8 aStart[] = { OP_Rewind, OP_Last };
    assert( bRev==0 || bRev==1 );
    if( pTabItem->fg.isRecursive ){
      /* Tables marked isRecursive have only a single row that is stored in
      ** a pseudo-cursor.  No need to Rewind or Next such cursors. */
      pLevel->op = OP_Noop;
    }else{
      codeCursorHint(pTabItem, pWInfo, pLevel, 0);
      pLevel->op = aStep[bRev];
      pLevel->p1 = iCur;
      pLevel->p2 = 1 + sqlite3VdbeAddOp2(v, aStart[bRev], iCur, addrHalt);
      VdbeCoverageIf(v, bRev==0);
      VdbeCoverageIf(v, bRev!=0);
      pLevel->p5 = SQLITE_STMTSTATUS_FULLSCAN_STEP;
    }
  }

#ifdef SQLITE_ENABLE_STMT_SCANSTATUS
  pLevel->addrVisit = sqlite3VdbeCurrentAddr(v);
#endif

  /* Insert code to test every subexpression that can be completely
  ** computed using the current set of tables.
  **
  ** This loop may run between one and three times, depending on the
  ** constraints to be generated. The value of stack variable iLoop
  ** determines the constraints coded by each iteration, as follows:
  **
  ** iLoop==1: Code only expressions that are entirely covered by pIdx.
  ** iLoop==2: Code remaining expressions that do not contain correlated
  **           sub-queries.
  ** iLoop==3: Code all remaining expressions.
  **
  ** An effort is made to skip unnecessary iterations of the loop.
  **
  ** This optimization of causing simple query restrictions to occur before
  ** more complex one is call the "push-down" optimization in MySQL.  Here
  ** in SQLite, the name is "MySQL push-down", since there is also another
  ** totally unrelated optimization called "WHERE-clause push-down".
  ** Sometimes the qualifier is omitted, resulting in an ambiguity, so beware.
  */
  iLoop = (pIdx ? 1 : 2);
  do{
    int iNext = 0;                /* Next value for iLoop */
    for(pTerm=pWC->a, j=pWC->nTerm; j>0; j--, pTerm++){
      Expr *pE;
      int skipLikeAddr = 0;
      testcase( pTerm->wtFlags & TERM_VIRTUAL );
      testcase( pTerm->wtFlags & TERM_CODED );
      if( pTerm->wtFlags & (TERM_VIRTUAL|TERM_CODED) ) continue;
      if( (pTerm->prereqAll & pLevel->notReady)!=0 ){
        testcase( pWInfo->untestedTerms==0
            && (pWInfo->wctrlFlags & WHERE_OR_SUBCLAUSE)!=0 );
        pWInfo->untestedTerms = 1;
        continue;
      }
      pE = pTerm->pExpr;
      assert( pE!=0 );
      if( pTabItem->fg.jointype & (JT_LEFT|JT_LTORJ|JT_RIGHT) ){
        if( !ExprHasProperty(pE,EP_OuterON|EP_InnerON) ){
          /* Defer processing WHERE clause constraints until after outer
          ** join processing.  tag-20220513a */
          continue;
        }else if( (pTabItem->fg.jointype & JT_LEFT)==JT_LEFT
               && !ExprHasProperty(pE,EP_OuterON) ){
          continue;
        }else{
          Bitmask m = sqlite3WhereGetMask(&pWInfo->sMaskSet, pE->w.iJoin);
          if( m & pLevel->notReady ){
            /* An ON clause that is not ripe */
            continue;
          }
        }
      }
      if( iLoop==1 && !sqlite3ExprCoveredByIndex(pE, pLevel->iTabCur, pIdx) ){
        iNext = 2;
        continue;
      }
      if( iLoop<3 && (pTerm->wtFlags & TERM_VARSELECT) ){
        if( iNext==0 ) iNext = 3;
        continue;
      }

      if( (pTerm->wtFlags & TERM_LIKECOND)!=0 ){
        /* If the TERM_LIKECOND flag is set, that means that the range search
        ** is sufficient to guarantee that the LIKE operator is true, so we
        ** can skip the call to the like(A,B) function.  But this only works
        ** for strings.  So do not skip the call to the function on the pass
        ** that compares BLOBs. */
#ifdef SQLITE_LIKE_DOESNT_MATCH_BLOBS
        continue;
#else
        u32 x = pLevel->iLikeRepCntr;
        if( x>0 ){
          skipLikeAddr = sqlite3VdbeAddOp1(v, (x&1)?OP_IfNot:OP_If,(int)(x>>1));
          VdbeCoverageIf(v, (x&1)==1);
          VdbeCoverageIf(v, (x&1)==0);
        }
#endif
      }
#ifdef WHERETRACE_ENABLED /* 0xffffffff */
      if( sqlite3WhereTrace ){
        VdbeNoopComment((v, "WhereTerm[%d] (%p) priority=%d",
                         pWC->nTerm-j, pTerm, iLoop));
      }
      if( sqlite3WhereTrace & 0x4000 ){
        sqlite3DebugPrintf("Coding auxiliary constraint:\n");
        sqlite3WhereTermPrint(pTerm, pWC->nTerm-j);
      }
#endif
      sqlite3ExprIfFalse(pParse, pE, addrCont, SQLITE_JUMPIFNULL);
      if( skipLikeAddr ) sqlite3VdbeJumpHere(v, skipLikeAddr);
      pTerm->wtFlags |= TERM_CODED;
    }
    iLoop = iNext;
  }while( iLoop>0 );

  /* Insert code to test for implied constraints based on transitivity
  ** of the "==" operator.
  **
  ** Example: If the WHERE clause contains "t1.a=t2.b" and "t2.b=123"
  ** and we are coding the t1 loop and the t2 loop has not yet coded,
  ** then we cannot use the "t1.a=t2.b" constraint, but we can code
  ** the implied "t1.a=123" constraint.
  */
  for(pTerm=pWC->a, j=pWC->nBase; j>0; j--, pTerm++){
    Expr *pE, sEAlt;
    WhereTerm *pAlt;
    if( pTerm->wtFlags & (TERM_VIRTUAL|TERM_CODED) ) continue;
    if( (pTerm->eOperator & (WO_EQ|WO_IS))==0 ) continue;
    if( (pTerm->eOperator & WO_EQUIV)==0 ) continue;
    if( pTerm->leftCursor!=iCur ) continue;
    if( pTabItem->fg.jointype & (JT_LEFT|JT_LTORJ|JT_RIGHT) ) continue;
    pE = pTerm->pExpr;
#ifdef WHERETRACE_ENABLED /* 0x4001 */
    if( (sqlite3WhereTrace & 0x4001)==0x4001 ){
      sqlite3DebugPrintf("Coding transitive constraint:\n");
      sqlite3WhereTermPrint(pTerm, pWC->nTerm-j);
    }
#endif
    assert( !ExprHasProperty(pE, EP_OuterON) );
    assert( (pTerm->prereqRight & pLevel->notReady)!=0 );
    assert( (pTerm->eOperator & (WO_OR|WO_AND))==0 );
    pAlt = sqlite3WhereFindTerm(pWC, iCur, pTerm->u.x.leftColumn, notReady,
                    WO_EQ|WO_IN|WO_IS, 0);
    if( pAlt==0 ) continue;
    if( pAlt->wtFlags & (TERM_CODED) ) continue;
    if( (pAlt->eOperator & WO_IN)
     && ExprUseXSelect(pAlt->pExpr)
     && (pAlt->pExpr->x.pSelect->pEList->nExpr>1)
    ){
      continue;
    }
    testcase( pAlt->eOperator & WO_EQ );
    testcase( pAlt->eOperator & WO_IS );
    testcase( pAlt->eOperator & WO_IN );
    VdbeModuleComment((v, "begin transitive constraint"));
    sEAlt = *pAlt->pExpr;
    sEAlt.pLeft = pE->pLeft;
    sqlite3ExprIfFalse(pParse, &sEAlt, addrCont, SQLITE_JUMPIFNULL);
    pAlt->wtFlags |= TERM_CODED;
  }

  /* For a RIGHT OUTER JOIN, record the fact that the current row has
  ** been matched at least once.
  */
  if( pLevel->pRJ ){
    Table *pTab;
    int nPk;
    int r;
    int jmp1 = 0;
    WhereRightJoin *pRJ = pLevel->pRJ;

    /* pTab is the right-hand table of the RIGHT JOIN.  Generate code that
    ** will record that the current row of that table has been matched at
    ** least once.  This is accomplished by storing the PK for the row in
    ** both the iMatch index and the regBloom Bloom filter.
    */
    pTab = pWInfo->pTabList->a[pLevel->iFrom].pTab;
    if( HasRowid(pTab) ){
      r = sqlite3GetTempRange(pParse, 2);
      sqlite3ExprCodeGetColumnOfTable(v, pTab, pLevel->iTabCur, -1, r+1);
      nPk = 1;
    }else{
      int iPk;
      Index *pPk = sqlite3PrimaryKeyIndex(pTab);
      nPk = pPk->nKeyCol;
      r = sqlite3GetTempRange(pParse, nPk+1);
      for(iPk=0; iPk<nPk; iPk++){
        int iCol = pPk->aiColumn[iPk];
        sqlite3ExprCodeGetColumnOfTable(v, pTab, iCur, iCol,r+1+iPk);
      }
    }
    jmp1 = sqlite3VdbeAddOp4Int(v, OP_Found, pRJ->iMatch, 0, r+1, nPk);
    VdbeCoverage(v);
    VdbeComment((v, "match against %s", pTab->zName));
    sqlite3VdbeAddOp3(v, OP_MakeRecord, r+1, nPk, r);
    sqlite3VdbeAddOp4Int(v, OP_IdxInsert, pRJ->iMatch, r, r+1, nPk);
    sqlite3VdbeAddOp4Int(v, OP_FilterAdd, pRJ->regBloom, 0, r+1, nPk);
    sqlite3VdbeChangeP5(v, OPFLAG_USESEEKRESULT);
    sqlite3VdbeJumpHere(v, jmp1);
    sqlite3ReleaseTempRange(pParse, r, nPk+1);
  }

  /* For a LEFT OUTER JOIN, generate code that will record the fact that
  ** at least one row of the right table has matched the left table.
  */
  if( pLevel->iLeftJoin ){
    pLevel->addrFirst = sqlite3VdbeCurrentAddr(v);
    sqlite3VdbeAddOp2(v, OP_Integer, 1, pLevel->iLeftJoin);
    VdbeComment((v, "record LEFT JOIN hit"));
    if( pLevel->pRJ==0 ){
      goto code_outer_join_constraints; /* WHERE clause constraints */
    }
  }

  if( pLevel->pRJ ){
    /* Create a subroutine used to process all interior loops and code
    ** of the RIGHT JOIN.  During normal operation, the subroutine will
    ** be in-line with the rest of the code.  But at the end, a separate
    ** loop will run that invokes this subroutine for unmatched rows
    ** of pTab, with all tables to left begin set to NULL.
    */
    WhereRightJoin *pRJ = pLevel->pRJ;
    sqlite3VdbeAddOp2(v, OP_BeginSubrtn, 0, pRJ->regReturn);
    pRJ->addrSubrtn = sqlite3VdbeCurrentAddr(v);
    assert( pParse->withinRJSubrtn < 255 );
    pParse->withinRJSubrtn++;

    /* WHERE clause constraints must be deferred until after outer join
    ** row elimination has completed, since WHERE clause constraints apply
    ** to the results of the OUTER JOIN.  The following loop generates the
    ** appropriate WHERE clause constraint checks.  tag-20220513a.
    */
  code_outer_join_constraints:
    for(pTerm=pWC->a, j=0; j<pWC->nBase; j++, pTerm++){
      testcase( pTerm->wtFlags & TERM_VIRTUAL );
      testcase( pTerm->wtFlags & TERM_CODED );
      if( pTerm->wtFlags & (TERM_VIRTUAL|TERM_CODED) ) continue;
      if( (pTerm->prereqAll & pLevel->notReady)!=0 ){
        assert( pWInfo->untestedTerms );
        continue;
      }
      if( pTabItem->fg.jointype & JT_LTORJ ) continue;
      assert( pTerm->pExpr );
      sqlite3ExprIfFalse(pParse, pTerm->pExpr, addrCont, SQLITE_JUMPIFNULL);
      pTerm->wtFlags |= TERM_CODED;
    }
  }

#if WHERETRACE_ENABLED /* 0x4001 */
  if( sqlite3WhereTrace & 0x4000 ){
    sqlite3DebugPrintf("All WHERE-clause terms after coding level %d:\n",
                       iLevel);
    sqlite3WhereClausePrint(pWC);
  }
  if( sqlite3WhereTrace & 0x1 ){
    sqlite3DebugPrintf("End Coding level %d:  notReady=%llx\n",
       iLevel, (u64)pLevel->notReady);
  }
#endif
  return pLevel->notReady;
}
