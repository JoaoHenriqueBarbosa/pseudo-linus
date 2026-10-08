
/*
** Set the EP_OuterON property on all terms of the given expression.
** And set the Expr.w.iJoin to iTable for every term in the
** expression.
**
** The EP_OuterON property is used on terms of an expression to tell
** the OUTER JOIN processing logic that this term is part of the
** join restriction specified in the ON or USING clause and not a part
** of the more general WHERE clause.  These terms are moved over to the
** WHERE clause during join processing but we need to remember that they
** originated in the ON or USING clause.
**
** The Expr.w.iJoin tells the WHERE clause processing that the
** expression depends on table w.iJoin even if that table is not
** explicitly mentioned in the expression.  That information is needed
** for cases like this:
**
**    SELECT * FROM t1 LEFT JOIN t2 ON t1.a=t2.b AND t1.x=5
**
** The where clause needs to defer the handling of the t1.x=5
** term until after the t2 loop of the join.  In that way, a
** NULL t2 row will be inserted whenever t1.x!=5.  If we do not
** defer the handling of t1.x=5, it will be processed immediately
** after the t1 loop and rows with t1.x!=5 will never appear in
** the output, which is incorrect.
*/
SQLITE_PRIVATE void sqlite3SetJoinExpr(Expr *p, int iTable, u32 joinFlag){
  assert( joinFlag==EP_OuterON || joinFlag==EP_InnerON );
  while( p ){
    ExprSetProperty(p, joinFlag);
    assert( !ExprHasProperty(p, EP_TokenOnly|EP_Reduced) );
    ExprSetVVAProperty(p, EP_NoReduce);
    p->w.iJoin = iTable;
    if( p->op==TK_FUNCTION ){
      assert( ExprUseXList(p) );
      if( p->x.pList ){
        int i;
        for(i=0; i<p->x.pList->nExpr; i++){
          sqlite3SetJoinExpr(p->x.pList->a[i].pExpr, iTable, joinFlag);
        }
      }
    }
    sqlite3SetJoinExpr(p->pLeft, iTable, joinFlag);
    p = p->pRight;
  }
}

/* Undo the work of sqlite3SetJoinExpr().  This is used when a LEFT JOIN
** is simplified into an ordinary JOIN, and when an ON expression is
** "pushed down" into the WHERE clause of a subquery.
**
** Convert every term that is marked with EP_OuterON and w.iJoin==iTable into
** an ordinary term that omits the EP_OuterON mark.  Or if iTable<0, then
** just clear every EP_OuterON and EP_InnerON mark from the expression tree.
**
** If nullable is true, that means that Expr p might evaluate to NULL even
** if it is a reference to a NOT NULL column.  This can happen, for example,
** if the table that p references is on the left side of a RIGHT JOIN.
** If nullable is true, then take care to not remove the EP_CanBeNull bit.
** See forum thread https://sqlite.org/forum/forumpost/b40696f50145d21c
*/
static void unsetJoinExpr(Expr *p, int iTable, int nullable){
  while( p ){
    if( iTable<0 || (ExprHasProperty(p, EP_OuterON) && p->w.iJoin==iTable) ){
      ExprClearProperty(p, EP_OuterON|EP_InnerON);
      if( iTable>=0 ) ExprSetProperty(p, EP_InnerON);
    }
    if( p->op==TK_COLUMN && p->iTable==iTable && !nullable ){
      ExprClearProperty(p, EP_CanBeNull);
    }
    if( p->op==TK_FUNCTION ){
      assert( ExprUseXList(p) );
      assert( p->pLeft==0 );
      if( p->x.pList ){
        int i;
        for(i=0; i<p->x.pList->nExpr; i++){
          unsetJoinExpr(p->x.pList->a[i].pExpr, iTable, nullable);
        }
      }
    }
    unsetJoinExpr(p->pLeft, iTable, nullable);
    p = p->pRight;
  }
}

/*
** This routine processes the join information for a SELECT statement.
**
**   *  A NATURAL join is converted into a USING join.  After that, we
**      do not need to be concerned with NATURAL joins and we only have
**      think about USING joins.
**
**   *  ON and USING clauses result in extra terms being added to the
**      WHERE clause to enforce the specified constraints.  The extra
**      WHERE clause terms will be tagged with EP_OuterON or
**      EP_InnerON so that we know that they originated in ON/USING.
**
** The terms of a FROM clause are contained in the Select.pSrc structure.
** The left most table is the first entry in Select.pSrc.  The right-most
** table is the last entry.  The join operator is held in the entry to
** the right.  Thus entry 1 contains the join operator for the join between
** entries 0 and 1.  Any ON or USING clauses associated with the join are
** also attached to the right entry.
**
** This routine returns the number of errors encountered.
*/
static int sqlite3ProcessJoin(Parse *pParse, Select *p){
  SrcList *pSrc;                  /* All tables in the FROM clause */
  int i, j;                       /* Loop counters */
  SrcItem *pLeft;                 /* Left table being joined */
  SrcItem *pRight;                /* Right table being joined */

  pSrc = p->pSrc;
  pLeft = &pSrc->a[0];
  pRight = &pLeft[1];
  for(i=0; i<pSrc->nSrc-1; i++, pRight++, pLeft++){
    Table *pRightTab = pRight->pTab;
    u32 joinType;

    if( NEVER(pLeft->pTab==0 || pRightTab==0) ) continue;
    joinType = (pRight->fg.jointype & JT_OUTER)!=0 ? EP_OuterON : EP_InnerON;

    /* If this is a NATURAL join, synthesize an appropriate USING clause
    ** to specify which columns should be joined.
    */
    if( pRight->fg.jointype & JT_NATURAL ){
      IdList *pUsing = 0;
      if( pRight->fg.isUsing || pRight->u3.pOn ){
        sqlite3ErrorMsg(pParse, "a NATURAL join may not have "
           "an ON or USING clause", 0);
        return 1;
      }
      for(j=0; j<pRightTab->nCol; j++){
        char *zName;   /* Name of column in the right table */

        if( IsHiddenColumn(&pRightTab->aCol[j]) ) continue;
        zName = pRightTab->aCol[j].zCnName;
        if( tableAndColumnIndex(pSrc, 0, i, zName, 0, 0, 1) ){
          pUsing = sqlite3IdListAppend(pParse, pUsing, 0);
          if( pUsing ){
            assert( pUsing->nId>0 );
            assert( pUsing->a[pUsing->nId-1].zName==0 );
            pUsing->a[pUsing->nId-1].zName = sqlite3DbStrDup(pParse->db, zName);
          }
        }
      }
      if( pUsing ){
        pRight->fg.isUsing = 1;
        pRight->fg.isSynthUsing = 1;
        pRight->u3.pUsing = pUsing;
      }
      if( pParse->nErr ) return 1;
    }

    /* Create extra terms on the WHERE clause for each column named
    ** in the USING clause.  Example: If the two tables to be joined are
    ** A and B and the USING clause names X, Y, and Z, then add this
    ** to the WHERE clause:    A.X=B.X AND A.Y=B.Y AND A.Z=B.Z
    ** Report an error if any column mentioned in the USING clause is
    ** not contained in both tables to be joined.
    */
    if( pRight->fg.isUsing ){
      IdList *pList = pRight->u3.pUsing;
      sqlite3 *db = pParse->db;
      assert( pList!=0 );
      for(j=0; j<pList->nId; j++){
        char *zName;     /* Name of the term in the USING clause */
        int iLeft;       /* Table on the left with matching column name */
        int iLeftCol;    /* Column number of matching column on the left */
        int iRightCol;   /* Column number of matching column on the right */
        Expr *pE1;       /* Reference to the column on the LEFT of the join */
        Expr *pE2;       /* Reference to the column on the RIGHT of the join */
        Expr *pEq;       /* Equality constraint.  pE1 == pE2 */

        zName = pList->a[j].zName;
        iRightCol = sqlite3ColumnIndex(pRightTab, zName);
        if( iRightCol<0
         || tableAndColumnIndex(pSrc, 0, i, zName, &iLeft, &iLeftCol,
                                pRight->fg.isSynthUsing)==0
        ){
          sqlite3ErrorMsg(pParse, "cannot join using column %s - column "
            "not present in both tables", zName);
          return 1;
        }
        pE1 = sqlite3CreateColumnExpr(db, pSrc, iLeft, iLeftCol);
        sqlite3SrcItemColumnUsed(&pSrc->a[iLeft], iLeftCol);
        if( (pSrc->a[0].fg.jointype & JT_LTORJ)!=0 ){
          /* This branch runs if the query contains one or more RIGHT or FULL
          ** JOINs.  If only a single table on the left side of this join
          ** contains the zName column, then this branch is a no-op.
          ** But if there are two or more tables on the left side
          ** of the join, construct a coalesce() function that gathers all
          ** such tables.  Raise an error if more than one of those references
          ** to zName is not also within a prior USING clause.
          **
          ** We really ought to raise an error if there are two or more
          ** non-USING references to zName on the left of an INNER or LEFT
          ** JOIN.  But older versions of SQLite do not do that, so we avoid
          ** adding a new error so as to not break legacy applications.
          */
          ExprList *pFuncArgs = 0;   /* Arguments to the coalesce() */
          static const Token tkCoalesce = { "coalesce", 8 };
          while( tableAndColumnIndex(pSrc, iLeft+1, i, zName, &iLeft, &iLeftCol,
                                     pRight->fg.isSynthUsing)!=0 ){
            if( pSrc->a[iLeft].fg.isUsing==0
             || sqlite3IdListIndex(pSrc->a[iLeft].u3.pUsing, zName)<0
            ){
              sqlite3ErrorMsg(pParse, "ambiguous reference to %s in USING()",
                              zName);
              break;
            }
            pFuncArgs = sqlite3ExprListAppend(pParse, pFuncArgs, pE1);
            pE1 = sqlite3CreateColumnExpr(db, pSrc, iLeft, iLeftCol);
            sqlite3SrcItemColumnUsed(&pSrc->a[iLeft], iLeftCol);
          }
          if( pFuncArgs ){
            pFuncArgs = sqlite3ExprListAppend(pParse, pFuncArgs, pE1);
            pE1 = sqlite3ExprFunction(pParse, pFuncArgs, &tkCoalesce, 0);
          }
        }
        pE2 = sqlite3CreateColumnExpr(db, pSrc, i+1, iRightCol);
        sqlite3SrcItemColumnUsed(pRight, iRightCol);
        pEq = sqlite3PExpr(pParse, TK_EQ, pE1, pE2);
        assert( pE2!=0 || pEq==0 );
        if( pEq ){
          ExprSetProperty(pEq, joinType);
          assert( !ExprHasProperty(pEq, EP_TokenOnly|EP_Reduced) );
          ExprSetVVAProperty(pEq, EP_NoReduce);
          pEq->w.iJoin = pE2->iTable;
        }
        p->pWhere = sqlite3ExprAnd(pParse, p->pWhere, pEq);
      }
    }

    /* Add the ON clause to the end of the WHERE clause, connected by
    ** an AND operator.
    */
    else if( pRight->u3.pOn ){
      sqlite3SetJoinExpr(pRight->u3.pOn, pRight->iCursor, joinType);
      p->pWhere = sqlite3ExprAnd(pParse, p->pWhere, pRight->u3.pOn);
      pRight->u3.pOn = 0;
      pRight->fg.isOn = 1;
    }
  }
  return 0;
}

/*
** An instance of this object holds information (beyond pParse and pSelect)
** needed to load the next result row that is to be added to the sorter.
*/
typedef struct RowLoadInfo RowLoadInfo;
struct RowLoadInfo {
  int regResult;               /* Store results in array of registers here */
  u8 ecelFlags;                /* Flag argument to ExprCodeExprList() */
#ifdef SQLITE_ENABLE_SORTER_REFERENCES
  ExprList *pExtra;            /* Extra columns needed by sorter refs */
  int regExtraResult;          /* Where to load the extra columns */
#endif
};

/*
** This routine does the work of loading query data into an array of
** registers so that it can be added to the sorter.
*/
static void innerLoopLoadRow(
  Parse *pParse,             /* Statement under construction */
  Select *pSelect,           /* The query being coded */
  RowLoadInfo *pInfo         /* Info needed to complete the row load */
){
  sqlite3ExprCodeExprList(pParse, pSelect->pEList, pInfo->regResult,
                          0, pInfo->ecelFlags);
#ifdef SQLITE_ENABLE_SORTER_REFERENCES
  if( pInfo->pExtra ){
    sqlite3ExprCodeExprList(pParse, pInfo->pExtra, pInfo->regExtraResult, 0, 0);
    sqlite3ExprListDelete(pParse->db, pInfo->pExtra);
  }
#endif
}

/*
** Code the OP_MakeRecord instruction that generates the entry to be
** added into the sorter.
**
** Return the register in which the result is stored.
*/
static int makeSorterRecord(
  Parse *pParse,
  SortCtx *pSort,
  Select *pSelect,
  int regBase,
  int nBase
){
  int nOBSat = pSort->nOBSat;
  Vdbe *v = pParse->pVdbe;
  int regOut = ++pParse->nMem;
  if( pSort->pDeferredRowLoad ){
    innerLoopLoadRow(pParse, pSelect, pSort->pDeferredRowLoad);
  }
  sqlite3VdbeAddOp3(v, OP_MakeRecord, regBase+nOBSat, nBase-nOBSat, regOut);
  return regOut;
}

/*
** Generate code that will push the record in registers regData
** through regData+nData-1 onto the sorter.
*/
static void pushOntoSorter(
  Parse *pParse,         /* Parser context */
  SortCtx *pSort,        /* Information about the ORDER BY clause */
  Select *pSelect,       /* The whole SELECT statement */
  int regData,           /* First register holding data to be sorted */
  int regOrigData,       /* First register holding data before packing */
  int nData,             /* Number of elements in the regData data array */
  int nPrefixReg         /* No. of reg prior to regData available for use */
){
  Vdbe *v = pParse->pVdbe;                         /* Stmt under construction */
  int bSeq = ((pSort->sortFlags & SORTFLAG_UseSorter)==0);
  int nExpr = pSort->pOrderBy->nExpr;              /* No. of ORDER BY terms */
  int nBase = nExpr + bSeq + nData;                /* Fields in sorter record */
  int regBase;                                     /* Regs for sorter record */
  int regRecord = 0;                               /* Assembled sorter record */
  int nOBSat = pSort->nOBSat;                      /* ORDER BY terms to skip */
  int op;                            /* Opcode to add sorter record to sorter */
  int iLimit;                        /* LIMIT counter */
  int iSkip = 0;                     /* End of the sorter insert loop */

  assert( bSeq==0 || bSeq==1 );

  /* Three cases:
  **   (1) The data to be sorted has already been packed into a Record
  **       by a prior OP_MakeRecord.  In this case nData==1 and regData
  **       will be completely unrelated to regOrigData.
  **   (2) All output columns are included in the sort record.  In that
  **       case regData==regOrigData.
  **   (3) Some output columns are omitted from the sort record due to
  **       the SQLITE_ENABLE_SORTER_REFERENCES optimization, or due to the
  **       SQLITE_ECEL_OMITREF optimization, or due to the
  **       SortCtx.pDeferredRowLoad optimization.  In any of these cases
  **       regOrigData is 0 to prevent this routine from trying to copy
  **       values that might not yet exist.
  */
  assert( nData==1 || regData==regOrigData || regOrigData==0 );

#ifdef SQLITE_ENABLE_STMT_SCANSTATUS
  pSort->addrPush = sqlite3VdbeCurrentAddr(v);
#endif

  if( nPrefixReg ){
    assert( nPrefixReg==nExpr+bSeq );
    regBase = regData - nPrefixReg;
  }else{
    regBase = pParse->nMem + 1;
    pParse->nMem += nBase;
  }
  assert( pSelect->iOffset==0 || pSelect->iLimit!=0 );
  iLimit = pSelect->iOffset ? pSelect->iOffset+1 : pSelect->iLimit;
  pSort->labelDone = sqlite3VdbeMakeLabel(pParse);
  sqlite3ExprCodeExprList(pParse, pSort->pOrderBy, regBase, regOrigData,
                          SQLITE_ECEL_DUP | (regOrigData? SQLITE_ECEL_REF : 0));
  if( bSeq ){
    sqlite3VdbeAddOp2(v, OP_Sequence, pSort->iECursor, regBase+nExpr);
  }
  if( nPrefixReg==0 && nData>0 ){
    sqlite3ExprCodeMove(pParse, regData, regBase+nExpr+bSeq, nData);
  }
  if( nOBSat>0 ){
    int regPrevKey;   /* The first nOBSat columns of the previous row */
    int addrFirst;    /* Address of the OP_IfNot opcode */
    int addrJmp;      /* Address of the OP_Jump opcode */
    VdbeOp *pOp;      /* Opcode that opens the sorter */
    int nKey;         /* Number of sorting key columns, including OP_Sequence */
    KeyInfo *pKI;     /* Original KeyInfo on the sorter table */

    regRecord = makeSorterRecord(pParse, pSort, pSelect, regBase, nBase);
    regPrevKey = pParse->nMem+1;
    pParse->nMem += pSort->nOBSat;
    nKey = nExpr - pSort->nOBSat + bSeq;
    if( bSeq ){
      addrFirst = sqlite3VdbeAddOp1(v, OP_IfNot, regBase+nExpr);
    }else{
      addrFirst = sqlite3VdbeAddOp1(v, OP_SequenceTest, pSort->iECursor);
    }
    VdbeCoverage(v);
    sqlite3VdbeAddOp3(v, OP_Compare, regPrevKey, regBase, pSort->nOBSat);
    pOp = sqlite3VdbeGetOp(v, pSort->addrSortIndex);
    if( pParse->db->mallocFailed ) return;
    pOp->p2 = nKey + nData;
    pKI = pOp->p4.pKeyInfo;
    memset(pKI->aSortFlags, 0, pKI->nKeyField); /* Makes OP_Jump testable */
    sqlite3VdbeChangeP4(v, -1, (char*)pKI, P4_KEYINFO);
    testcase( pKI->nAllField > pKI->nKeyField+2 );
    pOp->p4.pKeyInfo = sqlite3KeyInfoFromExprList(pParse,pSort->pOrderBy,nOBSat,
                                           pKI->nAllField-pKI->nKeyField-1);
    pOp = 0; /* Ensure pOp not used after sqlite3VdbeAddOp3() */
    addrJmp = sqlite3VdbeCurrentAddr(v);
    sqlite3VdbeAddOp3(v, OP_Jump, addrJmp+1, 0, addrJmp+1); VdbeCoverage(v);
    pSort->labelBkOut = sqlite3VdbeMakeLabel(pParse);
    pSort->regReturn = ++pParse->nMem;
    sqlite3VdbeAddOp2(v, OP_Gosub, pSort->regReturn, pSort->labelBkOut);
    sqlite3VdbeAddOp1(v, OP_ResetSorter, pSort->iECursor);
    if( iLimit ){
      sqlite3VdbeAddOp2(v, OP_IfNot, iLimit, pSort->labelDone);
      VdbeCoverage(v);
    }
    sqlite3VdbeJumpHere(v, addrFirst);
    sqlite3ExprCodeMove(pParse, regBase, regPrevKey, pSort->nOBSat);
    sqlite3VdbeJumpHere(v, addrJmp);
  }
  if( iLimit ){
    /* At this point the values for the new sorter entry are stored
    ** in an array of registers. They need to be composed into a record
    ** and inserted into the sorter if either (a) there are currently
    ** less than LIMIT+OFFSET items or (b) the new record is smaller than
    ** the largest record currently in the sorter. If (b) is true and there
    ** are already LIMIT+OFFSET items in the sorter, delete the largest
    ** entry before inserting the new one. This way there are never more
    ** than LIMIT+OFFSET items in the sorter.
    **
    ** If the new record does not need to be inserted into the sorter,
    ** jump to the next iteration of the loop. If the pSort->labelOBLopt
    ** value is not zero, then it is a label of where to jump.  Otherwise,
    ** just bypass the row insert logic.  See the header comment on the
    ** sqlite3WhereOrderByLimitOptLabel() function for additional info.
    */
    int iCsr = pSort->iECursor;
    sqlite3VdbeAddOp2(v, OP_IfNotZero, iLimit, sqlite3VdbeCurrentAddr(v)+4);
    VdbeCoverage(v);
    sqlite3VdbeAddOp2(v, OP_Last, iCsr, 0);
    iSkip = sqlite3VdbeAddOp4Int(v, OP_IdxLE,
                                 iCsr, 0, regBase+nOBSat, nExpr-nOBSat);
    VdbeCoverage(v);
    sqlite3VdbeAddOp1(v, OP_Delete, iCsr);
  }
  if( regRecord==0 ){
    regRecord = makeSorterRecord(pParse, pSort, pSelect, regBase, nBase);
  }
  if( pSort->sortFlags & SORTFLAG_UseSorter ){
    op = OP_SorterInsert;
  }else{
    op = OP_IdxInsert;
  }
  sqlite3VdbeAddOp4Int(v, op, pSort->iECursor, regRecord,
                       regBase+nOBSat, nBase-nOBSat);
  if( iSkip ){
    sqlite3VdbeChangeP2(v, iSkip,
         pSort->labelOBLopt ? pSort->labelOBLopt : sqlite3VdbeCurrentAddr(v));
  }
#ifdef SQLITE_ENABLE_STMT_SCANSTATUS
  pSort->addrPushEnd = sqlite3VdbeCurrentAddr(v)-1;
#endif
}
