#endif /* !SQLITE_OMIT_OR_OPTIMIZATION && !SQLITE_OMIT_SUBQUERY */

/*
** We already know that pExpr is a binary operator where both operands are
** column references.  This routine checks to see if pExpr is an equivalence
** relation:
**   1.  The SQLITE_Transitive optimization must be enabled
**   2.  Must be either an == or an IS operator
**   3.  Not originating in the ON clause of an OUTER JOIN
**   4.  The affinities of A and B must be compatible
**   5a. Both operands use the same collating sequence OR
**   5b. The overall collating sequence is BINARY
** If this routine returns TRUE, that means that the RHS can be substituted
** for the LHS anyplace else in the WHERE clause where the LHS column occurs.
** This is an optimization.  No harm comes from returning 0.  But if 1 is
** returned when it should not be, then incorrect answers might result.
*/
static int termIsEquivalence(Parse *pParse, Expr *pExpr){
  char aff1, aff2;
  CollSeq *pColl;
  if( !OptimizationEnabled(pParse->db, SQLITE_Transitive) ) return 0;
  if( pExpr->op!=TK_EQ && pExpr->op!=TK_IS ) return 0;
  if( ExprHasProperty(pExpr, EP_OuterON) ) return 0;
  aff1 = sqlite3ExprAffinity(pExpr->pLeft);
  aff2 = sqlite3ExprAffinity(pExpr->pRight);
  if( aff1!=aff2
   && (!sqlite3IsNumericAffinity(aff1) || !sqlite3IsNumericAffinity(aff2))
  ){
    return 0;
  }
  pColl = sqlite3ExprCompareCollSeq(pParse, pExpr);
  if( sqlite3IsBinary(pColl) ) return 1;
  return sqlite3ExprCollSeqMatch(pParse, pExpr->pLeft, pExpr->pRight);
}

/*
** Recursively walk the expressions of a SELECT statement and generate
** a bitmask indicating which tables are used in that expression
** tree.
*/
static Bitmask exprSelectUsage(WhereMaskSet *pMaskSet, Select *pS){
  Bitmask mask = 0;
  while( pS ){
    SrcList *pSrc = pS->pSrc;
    mask |= sqlite3WhereExprListUsage(pMaskSet, pS->pEList);
    mask |= sqlite3WhereExprListUsage(pMaskSet, pS->pGroupBy);
    mask |= sqlite3WhereExprListUsage(pMaskSet, pS->pOrderBy);
    mask |= sqlite3WhereExprUsage(pMaskSet, pS->pWhere);
    mask |= sqlite3WhereExprUsage(pMaskSet, pS->pHaving);
    if( ALWAYS(pSrc!=0) ){
      int i;
      for(i=0; i<pSrc->nSrc; i++){
        mask |= exprSelectUsage(pMaskSet, pSrc->a[i].pSelect);
        if( pSrc->a[i].fg.isUsing==0 ){
          mask |= sqlite3WhereExprUsage(pMaskSet, pSrc->a[i].u3.pOn);
        }
        if( pSrc->a[i].fg.isTabFunc ){
          mask |= sqlite3WhereExprListUsage(pMaskSet, pSrc->a[i].u1.pFuncArg);
        }
      }
    }
    pS = pS->pPrior;
  }
  return mask;
}

/*
** Expression pExpr is one operand of a comparison operator that might
** be useful for indexing.  This routine checks to see if pExpr appears
** in any index.  Return TRUE (1) if pExpr is an indexed term and return
** FALSE (0) if not.  If TRUE is returned, also set aiCurCol[0] to the cursor
** number of the table that is indexed and aiCurCol[1] to the column number
** of the column that is indexed, or XN_EXPR (-2) if an expression is being
** indexed.
**
** If pExpr is a TK_COLUMN column reference, then this routine always returns
** true even if that particular column is not indexed, because the column
** might be added to an automatic index later.
*/
static SQLITE_NOINLINE int exprMightBeIndexed2(
  SrcList *pFrom,        /* The FROM clause */
  int *aiCurCol,         /* Write the referenced table cursor and column here */
  Expr *pExpr,           /* An operand of a comparison operator */
  int j                  /* Start looking with the j-th pFrom entry */
){
  Index *pIdx;
  int i;
  int iCur;
  do{
    iCur = pFrom->a[j].iCursor;
    for(pIdx=pFrom->a[j].pTab->pIndex; pIdx; pIdx=pIdx->pNext){
      if( pIdx->aColExpr==0 ) continue;
      for(i=0; i<pIdx->nKeyCol; i++){
        if( pIdx->aiColumn[i]!=XN_EXPR ) continue;
        assert( pIdx->bHasExpr );
        if( sqlite3ExprCompareSkip(pExpr,pIdx->aColExpr->a[i].pExpr,iCur)==0
         && !sqlite3ExprIsConstant(0,pIdx->aColExpr->a[i].pExpr)
        ){
          aiCurCol[0] = iCur;
          aiCurCol[1] = XN_EXPR;
          return 1;
        }
      }
    }
  }while( ++j < pFrom->nSrc );
  return 0;
}
static int exprMightBeIndexed(
  SrcList *pFrom,        /* The FROM clause */
  int *aiCurCol,         /* Write the referenced table cursor & column here */
  Expr *pExpr,           /* An operand of a comparison operator */
  int op                 /* The specific comparison operator */
){
  int i;

  /* If this expression is a vector to the left or right of a
  ** inequality constraint (>, <, >= or <=), perform the processing
  ** on the first element of the vector.  */
  assert( TK_GT+1==TK_LE && TK_GT+2==TK_LT && TK_GT+3==TK_GE );
  assert( TK_IS<TK_GE && TK_ISNULL<TK_GE && TK_IN<TK_GE );
  assert( op<=TK_GE );
  if( pExpr->op==TK_VECTOR && (op>=TK_GT && ALWAYS(op<=TK_GE)) ){
    assert( ExprUseXList(pExpr) );
    pExpr = pExpr->x.pList->a[0].pExpr;
  }

  if( pExpr->op==TK_COLUMN ){
    aiCurCol[0] = pExpr->iTable;
    aiCurCol[1] = pExpr->iColumn;
    return 1;
  }

  for(i=0; i<pFrom->nSrc; i++){
    Index *pIdx;
    for(pIdx=pFrom->a[i].pTab->pIndex; pIdx; pIdx=pIdx->pNext){
      if( pIdx->aColExpr ){
        return exprMightBeIndexed2(pFrom,aiCurCol,pExpr,i);
      }
    }
  }
  return 0;
}


/*
** The input to this routine is an WhereTerm structure with only the
** "pExpr" field filled in.  The job of this routine is to analyze the
** subexpression and populate all the other fields of the WhereTerm
** structure.
**
** If the expression is of the form "<expr> <op> X" it gets commuted
** to the standard form of "X <op> <expr>".
**
** If the expression is of the form "X <op> Y" where both X and Y are
** columns, then the original expression is unchanged and a new virtual
** term of the form "Y <op> X" is added to the WHERE clause and
** analyzed separately.  The original term is marked with TERM_COPIED
** and the new term is marked with TERM_DYNAMIC (because it's pExpr
** needs to be freed with the WhereClause) and TERM_VIRTUAL (because it
** is a commuted copy of a prior term.)  The original term has nChild=1
** and the copy has idxParent set to the index of the original term.
*/
static void exprAnalyze(
  SrcList *pSrc,            /* the FROM clause */
  WhereClause *pWC,         /* the WHERE clause */
  int idxTerm               /* Index of the term to be analyzed */
){
  WhereInfo *pWInfo = pWC->pWInfo; /* WHERE clause processing context */
  WhereTerm *pTerm;                /* The term to be analyzed */
  WhereMaskSet *pMaskSet;          /* Set of table index masks */
  Expr *pExpr;                     /* The expression to be analyzed */
  Bitmask prereqLeft;              /* Prerequisites of the pExpr->pLeft */
  Bitmask prereqAll;               /* Prerequisites of pExpr */
  Bitmask extraRight = 0;          /* Extra dependencies on LEFT JOIN */
  Expr *pStr1 = 0;                 /* RHS of LIKE/GLOB operator */
  int isComplete = 0;              /* RHS of LIKE/GLOB ends with wildcard */
  int noCase = 0;                  /* uppercase equivalent to lowercase */
  int op;                          /* Top-level operator.  pExpr->op */
  Parse *pParse = pWInfo->pParse;  /* Parsing context */
  sqlite3 *db = pParse->db;        /* Database connection */
  unsigned char eOp2 = 0;          /* op2 value for LIKE/REGEXP/GLOB */
  int nLeft;                       /* Number of elements on left side vector */

  if( db->mallocFailed ){
    return;
  }
  assert( pWC->nTerm > idxTerm );
  pTerm = &pWC->a[idxTerm];
  pMaskSet = &pWInfo->sMaskSet;
  pExpr = pTerm->pExpr;
  assert( pExpr!=0 ); /* Because malloc() has not failed */
  assert( pExpr->op!=TK_AS && pExpr->op!=TK_COLLATE );
  pMaskSet->bVarSelect = 0;
  prereqLeft = sqlite3WhereExprUsage(pMaskSet, pExpr->pLeft);
  op = pExpr->op;
  if( op==TK_IN ){
    assert( pExpr->pRight==0 );
    if( sqlite3ExprCheckIN(pParse, pExpr) ) return;
    if( ExprUseXSelect(pExpr) ){
      pTerm->prereqRight = exprSelectUsage(pMaskSet, pExpr->x.pSelect);
    }else{
      pTerm->prereqRight = sqlite3WhereExprListUsage(pMaskSet, pExpr->x.pList);
    }
    prereqAll = prereqLeft | pTerm->prereqRight;
  }else{
    pTerm->prereqRight = sqlite3WhereExprUsage(pMaskSet, pExpr->pRight);
    if( pExpr->pLeft==0
     || ExprHasProperty(pExpr, EP_xIsSelect|EP_IfNullRow)
     || pExpr->x.pList!=0
    ){
      prereqAll = sqlite3WhereExprUsageNN(pMaskSet, pExpr);
    }else{
      prereqAll = prereqLeft | pTerm->prereqRight;
    }
  }
  if( pMaskSet->bVarSelect ) pTerm->wtFlags |= TERM_VARSELECT;

#ifdef SQLITE_DEBUG
  if( prereqAll!=sqlite3WhereExprUsageNN(pMaskSet, pExpr) ){
    printf("\n*** Incorrect prereqAll computed for:\n");
    sqlite3TreeViewExpr(0,pExpr,0);
    assert( 0 );
  }
#endif

  if( ExprHasProperty(pExpr, EP_OuterON|EP_InnerON) ){
    Bitmask x = sqlite3WhereGetMask(pMaskSet, pExpr->w.iJoin);
    if( ExprHasProperty(pExpr, EP_OuterON) ){
      prereqAll |= x;
      extraRight = x-1;  /* ON clause terms may not be used with an index
                         ** on left table of a LEFT JOIN.  Ticket #3015 */
      if( (prereqAll>>1)>=x ){
        sqlite3ErrorMsg(pParse, "ON clause references tables to its right");
        return;
      }
    }else if( (prereqAll>>1)>=x ){
      /* The ON clause of an INNER JOIN references a table to its right.
      ** Most other SQL database engines raise an error.  But SQLite versions
      ** 3.0 through 3.38 just put the ON clause constraint into the WHERE
      ** clause and carried on.   Beginning with 3.39, raise an error only
      ** if there is a RIGHT or FULL JOIN in the query.  This makes SQLite
      ** more like other systems, and also preserves legacy. */
      if( ALWAYS(pSrc->nSrc>0) && (pSrc->a[0].fg.jointype & JT_LTORJ)!=0 ){
        sqlite3ErrorMsg(pParse, "ON clause references tables to its right");
        return;
      }
      ExprClearProperty(pExpr, EP_InnerON);
    }
  }
  pTerm->prereqAll = prereqAll;
  pTerm->leftCursor = -1;
  pTerm->iParent = -1;
  pTerm->eOperator = 0;
  if( allowedOp(op) ){
    int aiCurCol[2];
    Expr *pLeft = sqlite3ExprSkipCollate(pExpr->pLeft);
    Expr *pRight = sqlite3ExprSkipCollate(pExpr->pRight);
    u16 opMask = (pTerm->prereqRight & prereqLeft)==0 ? WO_ALL : WO_EQUIV;

    if( pTerm->u.x.iField>0 ){
      assert( op==TK_IN );
      assert( pLeft->op==TK_VECTOR );
      assert( ExprUseXList(pLeft) );
      pLeft = pLeft->x.pList->a[pTerm->u.x.iField-1].pExpr;
    }

    if( exprMightBeIndexed(pSrc, aiCurCol, pLeft, op) ){
      pTerm->leftCursor = aiCurCol[0];
      assert( (pTerm->eOperator & (WO_OR|WO_AND))==0 );
      pTerm->u.x.leftColumn = aiCurCol[1];
      pTerm->eOperator = operatorMask(op) & opMask;
    }
    if( op==TK_IS ) pTerm->wtFlags |= TERM_IS;
    if( pRight
     && exprMightBeIndexed(pSrc, aiCurCol, pRight, op)
     && !ExprHasProperty(pRight, EP_FixedCol)
    ){
      WhereTerm *pNew;
      Expr *pDup;
      u16 eExtraOp = 0;        /* Extra bits for pNew->eOperator */
      assert( pTerm->u.x.iField==0 );
      if( pTerm->leftCursor>=0 ){
        int idxNew;
        pDup = sqlite3ExprDup(db, pExpr, 0);
        if( db->mallocFailed ){
          sqlite3ExprDelete(db, pDup);
          return;
        }
        idxNew = whereClauseInsert(pWC, pDup, TERM_VIRTUAL|TERM_DYNAMIC);
        if( idxNew==0 ) return;
        pNew = &pWC->a[idxNew];
        markTermAsChild(pWC, idxNew, idxTerm);
        if( op==TK_IS ) pNew->wtFlags |= TERM_IS;
        pTerm = &pWC->a[idxTerm];
        pTerm->wtFlags |= TERM_COPIED;

        if( termIsEquivalence(pParse, pDup) ){
          pTerm->eOperator |= WO_EQUIV;
          eExtraOp = WO_EQUIV;
        }
      }else{
        pDup = pExpr;
        pNew = pTerm;
      }
      pNew->wtFlags |= exprCommute(pParse, pDup);
      pNew->leftCursor = aiCurCol[0];
      assert( (pTerm->eOperator & (WO_OR|WO_AND))==0 );
      pNew->u.x.leftColumn = aiCurCol[1];
      testcase( (prereqLeft | extraRight) != prereqLeft );
      pNew->prereqRight = prereqLeft | extraRight;
      pNew->prereqAll = prereqAll;
      pNew->eOperator = (operatorMask(pDup->op) + eExtraOp) & opMask;
    }else
    if( op==TK_ISNULL
     && !ExprHasProperty(pExpr,EP_OuterON)
     && 0==sqlite3ExprCanBeNull(pLeft)
    ){
      assert( !ExprHasProperty(pExpr, EP_IntValue) );
      pExpr->op = TK_TRUEFALSE;  /* See tag-20230504-1 */
      pExpr->u.zToken = "false";
      ExprSetProperty(pExpr, EP_IsFalse);
      pTerm->prereqAll = 0;
      pTerm->eOperator = 0;
    }
  }

#ifndef SQLITE_OMIT_BETWEEN_OPTIMIZATION
  /* If a term is the BETWEEN operator, create two new virtual terms
  ** that define the range that the BETWEEN implements.  For example:
  **
  **      a BETWEEN b AND c
  **
  ** is converted into:
  **
  **      (a BETWEEN b AND c) AND (a>=b) AND (a<=c)
  **
  ** The two new terms are added onto the end of the WhereClause object.
  ** The new terms are "dynamic" and are children of the original BETWEEN
  ** term.  That means that if the BETWEEN term is coded, the children are
  ** skipped.  Or, if the children are satisfied by an index, the original
  ** BETWEEN term is skipped.
  */
  else if( pExpr->op==TK_BETWEEN && pWC->op==TK_AND ){
    ExprList *pList;
    int i;
    static const u8 ops[] = {TK_GE, TK_LE};
    assert( ExprUseXList(pExpr) );
    pList = pExpr->x.pList;
    assert( pList!=0 );
    assert( pList->nExpr==2 );
    for(i=0; i<2; i++){
      Expr *pNewExpr;
      int idxNew;
      pNewExpr = sqlite3PExpr(pParse, ops[i],
                             sqlite3ExprDup(db, pExpr->pLeft, 0),
                             sqlite3ExprDup(db, pList->a[i].pExpr, 0));
      transferJoinMarkings(pNewExpr, pExpr);
      idxNew = whereClauseInsert(pWC, pNewExpr, TERM_VIRTUAL|TERM_DYNAMIC);
      testcase( idxNew==0 );
      exprAnalyze(pSrc, pWC, idxNew);
      pTerm = &pWC->a[idxTerm];
      markTermAsChild(pWC, idxNew, idxTerm);
    }
  }
#endif /* SQLITE_OMIT_BETWEEN_OPTIMIZATION */

#if !defined(SQLITE_OMIT_OR_OPTIMIZATION) && !defined(SQLITE_OMIT_SUBQUERY)
  /* Analyze a term that is composed of two or more subterms connected by
  ** an OR operator.
  */
  else if( pExpr->op==TK_OR ){
    assert( pWC->op==TK_AND );
    exprAnalyzeOrTerm(pSrc, pWC, idxTerm);
    pTerm = &pWC->a[idxTerm];
  }
#endif /* SQLITE_OMIT_OR_OPTIMIZATION */
  /* The form "x IS NOT NULL" can sometimes be evaluated more efficiently
  ** as "x>NULL" if x is not an INTEGER PRIMARY KEY.  So construct a
  ** virtual term of that form.
  **
  ** The virtual term must be tagged with TERM_VNULL.
  */
  else if( pExpr->op==TK_NOTNULL ){
    if( pExpr->pLeft->op==TK_COLUMN
     && pExpr->pLeft->iColumn>=0
     && !ExprHasProperty(pExpr, EP_OuterON)
    ){
      Expr *pNewExpr;
      Expr *pLeft = pExpr->pLeft;
      int idxNew;
      WhereTerm *pNewTerm;

      pNewExpr = sqlite3PExpr(pParse, TK_GT,
                              sqlite3ExprDup(db, pLeft, 0),
                              sqlite3ExprAlloc(db, TK_NULL, 0, 0));

      idxNew = whereClauseInsert(pWC, pNewExpr,
                                TERM_VIRTUAL|TERM_DYNAMIC|TERM_VNULL);
      if( idxNew ){
        pNewTerm = &pWC->a[idxNew];
        pNewTerm->prereqRight = 0;
        pNewTerm->leftCursor = pLeft->iTable;
        pNewTerm->u.x.leftColumn = pLeft->iColumn;
        pNewTerm->eOperator = WO_GT;
        markTermAsChild(pWC, idxNew, idxTerm);
        pTerm = &pWC->a[idxTerm];
        pTerm->wtFlags |= TERM_COPIED;
        pNewTerm->prereqAll = pTerm->prereqAll;
      }
    }
  }


#ifndef SQLITE_OMIT_LIKE_OPTIMIZATION
  /* Add constraints to reduce the search space on a LIKE or GLOB
  ** operator.
  **
  ** A like pattern of the form "x LIKE 'aBc%'" is changed into constraints
  **
  **          x>='ABC' AND x<'abd' AND x LIKE 'aBc%'
  **
  ** The last character of the prefix "abc" is incremented to form the
  ** termination condition "abd".  If case is not significant (the default
  ** for LIKE) then the lower-bound is made all uppercase and the upper-
  ** bound is made all lowercase so that the bounds also work when comparing
  ** BLOBs.
  */
  else if( pExpr->op==TK_FUNCTION
   && pWC->op==TK_AND
   && isLikeOrGlob(pParse, pExpr, &pStr1, &isComplete, &noCase)
  ){
    Expr *pLeft;       /* LHS of LIKE/GLOB operator */
    Expr *pStr2;       /* Copy of pStr1 - RHS of LIKE/GLOB operator */
    Expr *pNewExpr1;
    Expr *pNewExpr2;
    int idxNew1;
    int idxNew2;
    const char *zCollSeqName;     /* Name of collating sequence */
    const u16 wtFlags = TERM_LIKEOPT | TERM_VIRTUAL | TERM_DYNAMIC;

    assert( ExprUseXList(pExpr) );
    pLeft = pExpr->x.pList->a[1].pExpr;
    pStr2 = sqlite3ExprDup(db, pStr1, 0);
    assert( pStr1==0 || !ExprHasProperty(pStr1, EP_IntValue) );
    assert( pStr2==0 || !ExprHasProperty(pStr2, EP_IntValue) );


    /* Convert the lower bound to upper-case and the upper bound to
    ** lower-case (upper-case is less than lower-case in ASCII) so that
    ** the range constraints also work for BLOBs
    */
    if( noCase && !pParse->db->mallocFailed ){
      int i;
      char c;
      pTerm->wtFlags |= TERM_LIKE;
      for(i=0; (c = pStr1->u.zToken[i])!=0; i++){
        pStr1->u.zToken[i] = sqlite3Toupper(c);
        pStr2->u.zToken[i] = sqlite3Tolower(c);
      }
    }

    if( !db->mallocFailed ){
      u8 c, *pC;       /* Last character before the first wildcard */
      pC = (u8*)&pStr2->u.zToken[sqlite3Strlen30(pStr2->u.zToken)-1];
      c = *pC;
      if( noCase ){
        /* The point is to increment the last character before the first
        ** wildcard.  But if we increment '@', that will push it into the
        ** alphabetic range where case conversions will mess up the
        ** inequality.  To avoid this, make sure to also run the full
        ** LIKE on all candidate expressions by clearing the isComplete flag
        */
        if( c=='A'-1 ) isComplete = 0;
        c = sqlite3UpperToLower[c];
      }
      *pC = c + 1;
    }
    zCollSeqName = noCase ? "NOCASE" : sqlite3StrBINARY;
    pNewExpr1 = sqlite3ExprDup(db, pLeft, 0);
    pNewExpr1 = sqlite3PExpr(pParse, TK_GE,
           sqlite3ExprAddCollateString(pParse,pNewExpr1,zCollSeqName),
           pStr1);
    transferJoinMarkings(pNewExpr1, pExpr);
    idxNew1 = whereClauseInsert(pWC, pNewExpr1, wtFlags);
    testcase( idxNew1==0 );
    pNewExpr2 = sqlite3ExprDup(db, pLeft, 0);
    pNewExpr2 = sqlite3PExpr(pParse, TK_LT,
           sqlite3ExprAddCollateString(pParse,pNewExpr2,zCollSeqName),
           pStr2);
    transferJoinMarkings(pNewExpr2, pExpr);
    idxNew2 = whereClauseInsert(pWC, pNewExpr2, wtFlags);
    testcase( idxNew2==0 );
    exprAnalyze(pSrc, pWC, idxNew1);
    exprAnalyze(pSrc, pWC, idxNew2);
    pTerm = &pWC->a[idxTerm];
    if( isComplete ){
      markTermAsChild(pWC, idxNew1, idxTerm);
      markTermAsChild(pWC, idxNew2, idxTerm);
    }
  }
#endif /* SQLITE_OMIT_LIKE_OPTIMIZATION */

  /* If there is a vector == or IS term - e.g. "(a, b) == (?, ?)" - create
  ** new terms for each component comparison - "a = ?" and "b = ?".  The
  ** new terms completely replace the original vector comparison, which is
  ** no longer used.
  **
  ** This is only required if at least one side of the comparison operation
  ** is not a sub-select.
  **
  ** tag-20220128a
  */
  if( (pExpr->op==TK_EQ || pExpr->op==TK_IS)
   && (nLeft = sqlite3ExprVectorSize(pExpr->pLeft))>1
   && sqlite3ExprVectorSize(pExpr->pRight)==nLeft
   && ( (pExpr->pLeft->flags & EP_xIsSelect)==0
     || (pExpr->pRight->flags & EP_xIsSelect)==0)
   && pWC->op==TK_AND
  ){
    int i;
    for(i=0; i<nLeft; i++){
      int idxNew;
      Expr *pNew;
      Expr *pLeft = sqlite3ExprForVectorField(pParse, pExpr->pLeft, i, nLeft);
      Expr *pRight = sqlite3ExprForVectorField(pParse, pExpr->pRight, i, nLeft);

      pNew = sqlite3PExpr(pParse, pExpr->op, pLeft, pRight);
      transferJoinMarkings(pNew, pExpr);
      idxNew = whereClauseInsert(pWC, pNew, TERM_DYNAMIC|TERM_SLICE);
      exprAnalyze(pSrc, pWC, idxNew);
    }
    pTerm = &pWC->a[idxTerm];
    pTerm->wtFlags |= TERM_CODED|TERM_VIRTUAL;  /* Disable the original */
    pTerm->eOperator = WO_ROWVAL;
  }

  /* If there is a vector IN term - e.g. "(a, b) IN (SELECT ...)" - create
  ** a virtual term for each vector component. The expression object
  ** used by each such virtual term is pExpr (the full vector IN(...)
  ** expression). The WhereTerm.u.x.iField variable identifies the index within
  ** the vector on the LHS that the virtual term represents.
  **
  ** This only works if the RHS is a simple SELECT (not a compound) that does
  ** not use window functions.
  */
  else if( pExpr->op==TK_IN
   && pTerm->u.x.iField==0
   && pExpr->pLeft->op==TK_VECTOR
   && ALWAYS( ExprUseXSelect(pExpr) )
   && (pExpr->x.pSelect->pPrior==0 || (pExpr->x.pSelect->selFlags & SF_Values))
#ifndef SQLITE_OMIT_WINDOWFUNC
   && pExpr->x.pSelect->pWin==0
#endif
   && pWC->op==TK_AND
  ){
    int i;
    for(i=0; i<sqlite3ExprVectorSize(pExpr->pLeft); i++){
      int idxNew;
      idxNew = whereClauseInsert(pWC, pExpr, TERM_VIRTUAL|TERM_SLICE);
      pWC->a[idxNew].u.x.iField = i+1;
      exprAnalyze(pSrc, pWC, idxNew);
      markTermAsChild(pWC, idxNew, idxTerm);
    }
  }

#ifndef SQLITE_OMIT_VIRTUALTABLE
  /* Add a WO_AUX auxiliary term to the constraint set if the
  ** current expression is of the form "column OP expr" where OP
  ** is an operator that gets passed into virtual tables but which is
  ** not normally optimized for ordinary tables.  In other words, OP
  ** is one of MATCH, LIKE, GLOB, REGEXP, !=, IS, IS NOT, or NOT NULL.
  ** This information is used by the xBestIndex methods of
  ** virtual tables.  The native query optimizer does not attempt
  ** to do anything with MATCH functions.
  */
  else if( pWC->op==TK_AND ){
    Expr *pRight = 0, *pLeft = 0;
    int res = isAuxiliaryVtabOperator(db, pExpr, &eOp2, &pLeft, &pRight);
    while( res-- > 0 ){
      int idxNew;
      WhereTerm *pNewTerm;
      Bitmask prereqColumn, prereqExpr;

      prereqExpr = sqlite3WhereExprUsage(pMaskSet, pRight);
      prereqColumn = sqlite3WhereExprUsage(pMaskSet, pLeft);
      if( (prereqExpr & prereqColumn)==0 ){
        Expr *pNewExpr;
        pNewExpr = sqlite3PExpr(pParse, TK_MATCH,
            0, sqlite3ExprDup(db, pRight, 0));
        if( ExprHasProperty(pExpr, EP_OuterON) && pNewExpr ){
          ExprSetProperty(pNewExpr, EP_OuterON);
          pNewExpr->w.iJoin = pExpr->w.iJoin;
        }
        idxNew = whereClauseInsert(pWC, pNewExpr, TERM_VIRTUAL|TERM_DYNAMIC);
        testcase( idxNew==0 );
        pNewTerm = &pWC->a[idxNew];
        pNewTerm->prereqRight = prereqExpr;
        pNewTerm->leftCursor = pLeft->iTable;
        pNewTerm->u.x.leftColumn = pLeft->iColumn;
        pNewTerm->eOperator = WO_AUX;
        pNewTerm->eMatchOp = eOp2;
        markTermAsChild(pWC, idxNew, idxTerm);
        pTerm = &pWC->a[idxTerm];
        pTerm->wtFlags |= TERM_COPIED;
        pNewTerm->prereqAll = pTerm->prereqAll;
      }
      SWAP(Expr*, pLeft, pRight);
    }
  }
#endif /* SQLITE_OMIT_VIRTUALTABLE */

  /* Prevent ON clause terms of a LEFT JOIN from being used to drive
  ** an index for tables to the left of the join.
  */
  testcase( pTerm!=&pWC->a[idxTerm] );
  pTerm = &pWC->a[idxTerm];
  pTerm->prereqRight |= extraRight;
}
