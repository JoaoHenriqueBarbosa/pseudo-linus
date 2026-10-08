
/*
** Allocate and return a pointer to an expression to load the column iCol
** from datasource iSrc in SrcList pSrc.
*/
SQLITE_PRIVATE Expr *sqlite3CreateColumnExpr(sqlite3 *db, SrcList *pSrc, int iSrc, int iCol){
  Expr *p = sqlite3ExprAlloc(db, TK_COLUMN, 0, 0);
  if( p ){
    SrcItem *pItem = &pSrc->a[iSrc];
    Table *pTab;
    assert( ExprUseYTab(p) );
    pTab = p->y.pTab = pItem->pTab;
    p->iTable = pItem->iCursor;
    if( p->y.pTab->iPKey==iCol ){
      p->iColumn = -1;
    }else{
      p->iColumn = (ynVar)iCol;
      if( (pTab->tabFlags & TF_HasGenerated)!=0
       && (pTab->aCol[iCol].colFlags & COLFLAG_GENERATED)!=0
      ){
        testcase( pTab->nCol==63 );
        testcase( pTab->nCol==64 );
        pItem->colUsed = pTab->nCol>=64 ? ALLBITS : MASKBIT(pTab->nCol)-1;
      }else{
        testcase( iCol==BMS );
        testcase( iCol==BMS-1 );
        pItem->colUsed |= ((Bitmask)1)<<(iCol>=BMS ? BMS-1 : iCol);
      }
    }
  }
  return p;
}

/*
** Report an error that an expression is not valid for some set of
** pNC->ncFlags values determined by validMask.
**
** static void notValid(
**   Parse *pParse,       // Leave error message here
**   NameContext *pNC,    // The name context
**   const char *zMsg,    // Type of error
**   int validMask,       // Set of contexts for which prohibited
**   Expr *pExpr          // Invalidate this expression on error
** ){...}
**
** As an optimization, since the conditional is almost always false
** (because errors are rare), the conditional is moved outside of the
** function call using a macro.
*/
static void notValidImpl(
   Parse *pParse,       /* Leave error message here */
   NameContext *pNC,    /* The name context */
   const char *zMsg,    /* Type of error */
   Expr *pExpr,         /* Invalidate this expression on error */
   Expr *pError         /* Associate error with this expression */
){
  const char *zIn = "partial index WHERE clauses";
  if( pNC->ncFlags & NC_IdxExpr )      zIn = "index expressions";
#ifndef SQLITE_OMIT_CHECK
  else if( pNC->ncFlags & NC_IsCheck ) zIn = "CHECK constraints";
#endif
#ifndef SQLITE_OMIT_GENERATED_COLUMNS
  else if( pNC->ncFlags & NC_GenCol ) zIn = "generated columns";
#endif
  sqlite3ErrorMsg(pParse, "%s prohibited in %s", zMsg, zIn);
  if( pExpr ) pExpr->op = TK_NULL;
  sqlite3RecordErrorOffsetOfExpr(pParse->db, pError);
}
#define sqlite3ResolveNotValid(P,N,M,X,E,R) \
  assert( ((X)&~(NC_IsCheck|NC_PartIdx|NC_IdxExpr|NC_GenCol))==0 ); \
  if( ((N)->ncFlags & (X))!=0 ) notValidImpl(P,N,M,E,R);

/*
** Expression p should encode a floating point value between 1.0 and 0.0.
** Return 1024 times this value.  Or return -1 if p is not a floating point
** value between 1.0 and 0.0.
*/
static int exprProbability(Expr *p){
  double r = -1.0;
  if( p->op!=TK_FLOAT ) return -1;
  assert( !ExprHasProperty(p, EP_IntValue) );
  sqlite3AtoF(p->u.zToken, &r, sqlite3Strlen30(p->u.zToken), SQLITE_UTF8);
  assert( r>=0.0 );
  if( r>1.0 ) return -1;
  return (int)(r*134217728.0);
}

/*
** This routine is callback for sqlite3WalkExpr().
**
** Resolve symbolic names into TK_COLUMN operators for the current
** node in the expression tree.  Return 0 to continue the search down
** the tree or 2 to abort the tree walk.
**
** This routine also does error checking and name resolution for
** function names.  The operator for aggregate functions is changed
** to TK_AGG_FUNCTION.
*/
static int resolveExprStep(Walker *pWalker, Expr *pExpr){
  NameContext *pNC;
  Parse *pParse;

  pNC = pWalker->u.pNC;
  assert( pNC!=0 );
  pParse = pNC->pParse;
  assert( pParse==pWalker->pParse );

#ifndef NDEBUG
  if( pNC->pSrcList && pNC->pSrcList->nAlloc>0 ){
    SrcList *pSrcList = pNC->pSrcList;
    int i;
    for(i=0; i<pNC->pSrcList->nSrc; i++){
      assert( pSrcList->a[i].iCursor>=0 && pSrcList->a[i].iCursor<pParse->nTab);
    }
  }
#endif
  switch( pExpr->op ){

    /* The special operator TK_ROW means use the rowid for the first
    ** column in the FROM clause.  This is used by the LIMIT and ORDER BY
    ** clause processing on UPDATE and DELETE statements, and by
    ** UPDATE ... FROM statement processing.
    */
    case TK_ROW: {
      SrcList *pSrcList = pNC->pSrcList;
      SrcItem *pItem;
      assert( pSrcList && pSrcList->nSrc>=1 );
      pItem = pSrcList->a;
      pExpr->op = TK_COLUMN;
      assert( ExprUseYTab(pExpr) );
      pExpr->y.pTab = pItem->pTab;
      pExpr->iTable = pItem->iCursor;
      pExpr->iColumn--;
      pExpr->affExpr = SQLITE_AFF_INTEGER;
      break;
    }

    /* An optimization:  Attempt to convert
    **
    **      "expr IS NOT NULL"  -->  "TRUE"
    **      "expr IS NULL"      -->  "FALSE"
    **
    ** if we can prove that "expr" is never NULL.  Call this the
    ** "NOT NULL strength reduction optimization".
    **
    ** If this optimization occurs, also restore the NameContext ref-counts
    ** to the state they where in before the "column" LHS expression was
    ** resolved.  This prevents "column" from being counted as having been
    ** referenced, which might prevent a SELECT from being erroneously
    ** marked as correlated.
    **
    ** 2024-03-28: Beware of aggregates.  A bare column of aggregated table
    ** can still evaluate to NULL even though it is marked as NOT NULL.
    ** Example:
    **
    **       CREATE TABLE t1(a INT NOT NULL);
    **       SELECT a, a IS NULL, a IS NOT NULL, count(*) FROM t1;
    **
    ** The "a IS NULL" and "a IS NOT NULL" expressions cannot be optimized
    ** here because at the time this case is hit, we do not yet know whether
    ** or not t1 is being aggregated.  We have to assume the worst and omit
    ** the optimization.  The only time it is safe to apply this optimization
    ** is within the WHERE clause.
    */
    case TK_NOTNULL:
    case TK_ISNULL: {
      int anRef[8];
      NameContext *p;
      int i;
      for(i=0, p=pNC; p && i<ArraySize(anRef); p=p->pNext, i++){
        anRef[i] = p->nRef;
      }
      sqlite3WalkExpr(pWalker, pExpr->pLeft);
      if( IN_RENAME_OBJECT ) return WRC_Prune;
      if( sqlite3ExprCanBeNull(pExpr->pLeft) ){
        /* The expression can be NULL.  So the optimization does not apply */
        return WRC_Prune;
      }

      for(i=0, p=pNC; p; p=p->pNext, i++){
        if( (p->ncFlags & NC_Where)==0 ){
          return WRC_Prune;  /* Not in a WHERE clause.  Unsafe to optimize. */
        }
      }
      testcase( ExprHasProperty(pExpr, EP_OuterON) );
      assert( !ExprHasProperty(pExpr, EP_IntValue) );
#if TREETRACE_ENABLED
      if( sqlite3TreeTrace & 0x80000 ){
        sqlite3DebugPrintf(
           "NOT NULL strength reduction converts the following to %d:\n",
           pExpr->op==TK_NOTNULL
        );
        sqlite3ShowExpr(pExpr);
      }
#endif /* TREETRACE_ENABLED */
      pExpr->u.iValue = (pExpr->op==TK_NOTNULL);
      pExpr->flags |= EP_IntValue;
      pExpr->op = TK_INTEGER;
      for(i=0, p=pNC; p && i<ArraySize(anRef); p=p->pNext, i++){
        p->nRef = anRef[i];
      }
      sqlite3ExprDelete(pParse->db, pExpr->pLeft);
      pExpr->pLeft = 0;
      return WRC_Prune;
    }

    /* A column name:                    ID
    ** Or table name and column name:    ID.ID
    ** Or a database, table and column:  ID.ID.ID
    **
    ** The TK_ID and TK_OUT cases are combined so that there will only
    ** be one call to lookupName().  Then the compiler will in-line
    ** lookupName() for a size reduction and performance increase.
    */
    case TK_ID:
    case TK_DOT: {
      const char *zTable;
      const char *zDb;
      Expr *pRight;

      if( pExpr->op==TK_ID ){
        zDb = 0;
        zTable = 0;
        assert( !ExprHasProperty(pExpr, EP_IntValue) );
        pRight = pExpr;
      }else{
        Expr *pLeft = pExpr->pLeft;
        testcase( pNC->ncFlags & NC_IdxExpr );
        testcase( pNC->ncFlags & NC_GenCol );
        sqlite3ResolveNotValid(pParse, pNC, "the \".\" operator",
                               NC_IdxExpr|NC_GenCol, 0, pExpr);
        pRight = pExpr->pRight;
        if( pRight->op==TK_ID ){
          zDb = 0;
        }else{
          assert( pRight->op==TK_DOT );
          assert( !ExprHasProperty(pRight, EP_IntValue) );
          zDb = pLeft->u.zToken;
          pLeft = pRight->pLeft;
          pRight = pRight->pRight;
        }
        assert( ExprUseUToken(pLeft) && ExprUseUToken(pRight) );
        zTable = pLeft->u.zToken;
        assert( ExprUseYTab(pExpr) );
        if( IN_RENAME_OBJECT ){
          sqlite3RenameTokenRemap(pParse, (void*)pExpr, (void*)pRight);
          sqlite3RenameTokenRemap(pParse, (void*)&pExpr->y.pTab, (void*)pLeft);
        }
      }
      return lookupName(pParse, zDb, zTable, pRight, pNC, pExpr);
    }

    /* Resolve function names
    */
    case TK_FUNCTION: {
      ExprList *pList = pExpr->x.pList;    /* The argument list */
      int n = pList ? pList->nExpr : 0;    /* Number of arguments */
      int no_such_func = 0;       /* True if no such function exists */
      int wrong_num_args = 0;     /* True if wrong number of arguments */
      int is_agg = 0;             /* True if is an aggregate function */
      const char *zId;            /* The function name. */
      FuncDef *pDef;              /* Information about the function */
      u8 enc = ENC(pParse->db);   /* The database encoding */
      int savedAllowFlags = (pNC->ncFlags & (NC_AllowAgg | NC_AllowWin));
#ifndef SQLITE_OMIT_WINDOWFUNC
      Window *pWin = (IsWindowFunc(pExpr) ? pExpr->y.pWin : 0);
#endif
      assert( !ExprHasProperty(pExpr, EP_xIsSelect|EP_IntValue) );
      assert( pExpr->pLeft==0 || pExpr->pLeft->op==TK_ORDER );
      zId = pExpr->u.zToken;
      pDef = sqlite3FindFunction(pParse->db, zId, n, enc, 0);
      if( pDef==0 ){
        pDef = sqlite3FindFunction(pParse->db, zId, -2, enc, 0);
        if( pDef==0 ){
          no_such_func = 1;
        }else{
          wrong_num_args = 1;
        }
      }else{
        is_agg = pDef->xFinalize!=0;
        if( pDef->funcFlags & SQLITE_FUNC_UNLIKELY ){
          ExprSetProperty(pExpr, EP_Unlikely);
          if( n==2 ){
            pExpr->iTable = exprProbability(pList->a[1].pExpr);
            if( pExpr->iTable<0 ){
              sqlite3ErrorMsg(pParse,
                "second argument to %#T() must be a "
                "constant between 0.0 and 1.0", pExpr);
              pNC->nNcErr++;
            }
          }else{
            /* EVIDENCE-OF: R-61304-29449 The unlikely(X) function is
            ** equivalent to likelihood(X, 0.0625).
            ** EVIDENCE-OF: R-01283-11636 The unlikely(X) function is
            ** short-hand for likelihood(X,0.0625).
            ** EVIDENCE-OF: R-36850-34127 The likely(X) function is short-hand
            ** for likelihood(X,0.9375).
            ** EVIDENCE-OF: R-53436-40973 The likely(X) function is equivalent
            ** to likelihood(X,0.9375). */
            /* TUNING: unlikely() probability is 0.0625.  likely() is 0.9375 */
            pExpr->iTable = pDef->zName[0]=='u' ? 8388608 : 125829120;
          }
        }
#ifndef SQLITE_OMIT_AUTHORIZATION
        {
          int auth = sqlite3AuthCheck(pParse, SQLITE_FUNCTION, 0,pDef->zName,0);
          if( auth!=SQLITE_OK ){
            if( auth==SQLITE_DENY ){
              sqlite3ErrorMsg(pParse, "not authorized to use function: %#T",
                                      pExpr);
              pNC->nNcErr++;
            }
            pExpr->op = TK_NULL;
            return WRC_Prune;
          }
        }
#endif
        if( pDef->funcFlags & (SQLITE_FUNC_CONSTANT|SQLITE_FUNC_SLOCHNG) ){
          /* For the purposes of the EP_ConstFunc flag, date and time
          ** functions and other functions that change slowly are considered
          ** constant because they are constant for the duration of one query.
          ** This allows them to be factored out of inner loops. */
          ExprSetProperty(pExpr,EP_ConstFunc);
        }
        if( (pDef->funcFlags & SQLITE_FUNC_CONSTANT)==0 ){
          /* Clearly non-deterministic functions like random(), but also
          ** date/time functions that use 'now', and other functions like
          ** sqlite_version() that might change over time cannot be used
          ** in an index or generated column.  Curiously, they can be used
          ** in a CHECK constraint.  SQLServer, MySQL, and PostgreSQL all
          ** all this. */
          sqlite3ResolveNotValid(pParse, pNC, "non-deterministic functions",
                                 NC_IdxExpr|NC_PartIdx|NC_GenCol, 0, pExpr);
        }else{
          assert( (NC_SelfRef & 0xff)==NC_SelfRef ); /* Must fit in 8 bits */
          pExpr->op2 = pNC->ncFlags & NC_SelfRef;
          if( pNC->ncFlags & NC_FromDDL ) ExprSetProperty(pExpr, EP_FromDDL);
        }
        if( (pDef->funcFlags & SQLITE_FUNC_INTERNAL)!=0
         && pParse->nested==0
         && (pParse->db->mDbFlags & DBFLAG_InternalFunc)==0
        ){
          /* Internal-use-only functions are disallowed unless the
          ** SQL is being compiled using sqlite3NestedParse() or
          ** the SQLITE_TESTCTRL_INTERNAL_FUNCTIONS test-control has be
          ** used to activate internal functions for testing purposes */
          no_such_func = 1;
          pDef = 0;
        }else
        if( (pDef->funcFlags & (SQLITE_FUNC_DIRECT|SQLITE_FUNC_UNSAFE))!=0
         && !IN_RENAME_OBJECT
        ){
          sqlite3ExprFunctionUsable(pParse, pExpr, pDef);
        }
      }

      if( 0==IN_RENAME_OBJECT ){
#ifndef SQLITE_OMIT_WINDOWFUNC
        assert( is_agg==0 || (pDef->funcFlags & SQLITE_FUNC_MINMAX)
          || (pDef->xValue==0 && pDef->xInverse==0)
          || (pDef->xValue && pDef->xInverse && pDef->xSFunc && pDef->xFinalize)
        );
        if( pDef && pDef->xValue==0 && pWin ){
          sqlite3ErrorMsg(pParse,
              "%#T() may not be used as a window function", pExpr
          );
          pNC->nNcErr++;
        }else if(
              (is_agg && (pNC->ncFlags & NC_AllowAgg)==0)
           || (is_agg && (pDef->funcFlags&SQLITE_FUNC_WINDOW) && !pWin)
           || (is_agg && pWin && (pNC->ncFlags & NC_AllowWin)==0)
        ){
          const char *zType;
          if( (pDef->funcFlags & SQLITE_FUNC_WINDOW) || pWin ){
            zType = "window";
          }else{
            zType = "aggregate";
          }
          sqlite3ErrorMsg(pParse, "misuse of %s function %#T()",zType,pExpr);
          pNC->nNcErr++;
          is_agg = 0;
        }
#else
        if( (is_agg && (pNC->ncFlags & NC_AllowAgg)==0) ){
          sqlite3ErrorMsg(pParse,"misuse of aggregate function %#T()",pExpr);
          pNC->nNcErr++;
          is_agg = 0;
        }
#endif
        else if( no_such_func && pParse->db->init.busy==0
#ifdef SQLITE_ENABLE_UNKNOWN_SQL_FUNCTION
                  && pParse->explain==0
#endif
        ){
          sqlite3ErrorMsg(pParse, "no such function: %#T", pExpr);
          pNC->nNcErr++;
        }else if( wrong_num_args ){
          sqlite3ErrorMsg(pParse,"wrong number of arguments to function %#T()",
               pExpr);
          pNC->nNcErr++;
        }
#ifndef SQLITE_OMIT_WINDOWFUNC
        else if( is_agg==0 && ExprHasProperty(pExpr, EP_WinFunc) ){
          sqlite3ErrorMsg(pParse,
              "FILTER may not be used with non-aggregate %#T()",
              pExpr
          );
          pNC->nNcErr++;
        }
#endif
        else if( is_agg==0 && pExpr->pLeft ){
          sqlite3ExprOrderByAggregateError(pParse, pExpr);
          pNC->nNcErr++;
        }
        if( is_agg ){
          /* Window functions may not be arguments of aggregate functions.
          ** Or arguments of other window functions. But aggregate functions
          ** may be arguments for window functions.  */
#ifndef SQLITE_OMIT_WINDOWFUNC
          pNC->ncFlags &= ~(NC_AllowWin | (!pWin ? NC_AllowAgg : 0));
#else
          pNC->ncFlags &= ~NC_AllowAgg;
#endif
        }
      }
      else if( ExprHasProperty(pExpr, EP_WinFunc) || pExpr->pLeft ){
        is_agg = 1;
      }
      sqlite3WalkExprList(pWalker, pList);
      if( is_agg ){
        if( pExpr->pLeft ){
          assert( pExpr->pLeft->op==TK_ORDER );
          assert( ExprUseXList(pExpr->pLeft) );
          sqlite3WalkExprList(pWalker, pExpr->pLeft->x.pList);
        }
#ifndef SQLITE_OMIT_WINDOWFUNC
        if( pWin ){
          Select *pSel = pNC->pWinSelect;
          assert( pWin==0 || (ExprUseYWin(pExpr) && pWin==pExpr->y.pWin) );
          if( IN_RENAME_OBJECT==0 ){
            sqlite3WindowUpdate(pParse, pSel ? pSel->pWinDefn : 0, pWin, pDef);
            if( pParse->db->mallocFailed ) break;
          }
          sqlite3WalkExprList(pWalker, pWin->pPartition);
          sqlite3WalkExprList(pWalker, pWin->pOrderBy);
          sqlite3WalkExpr(pWalker, pWin->pFilter);
          sqlite3WindowLink(pSel, pWin);
          pNC->ncFlags |= NC_HasWin;
        }else
#endif /* SQLITE_OMIT_WINDOWFUNC */
        {
          NameContext *pNC2;          /* For looping up thru outer contexts */
          pExpr->op = TK_AGG_FUNCTION;
          pExpr->op2 = 0;
#ifndef SQLITE_OMIT_WINDOWFUNC
          if( ExprHasProperty(pExpr, EP_WinFunc) ){
            sqlite3WalkExpr(pWalker, pExpr->y.pWin->pFilter);
          }
#endif
          pNC2 = pNC;
          while( pNC2
              && sqlite3ReferencesSrcList(pParse, pExpr, pNC2->pSrcList)==0
          ){
            pExpr->op2 += (1 + pNC2->nNestedSelect);
            pNC2 = pNC2->pNext;
          }
          assert( pDef!=0 || IN_RENAME_OBJECT );
          if( pNC2 && pDef ){
            pExpr->op2 += pNC2->nNestedSelect;
            assert( SQLITE_FUNC_MINMAX==NC_MinMaxAgg );
            assert( SQLITE_FUNC_ANYORDER==NC_OrderAgg );
            testcase( (pDef->funcFlags & SQLITE_FUNC_MINMAX)!=0 );
            testcase( (pDef->funcFlags & SQLITE_FUNC_ANYORDER)!=0 );
            pNC2->ncFlags |= NC_HasAgg
              | ((pDef->funcFlags^SQLITE_FUNC_ANYORDER)
                  & (SQLITE_FUNC_MINMAX|SQLITE_FUNC_ANYORDER));
          }
        }
        pNC->ncFlags |= savedAllowFlags;
      }
      /* FIX ME:  Compute pExpr->affinity based on the expected return
      ** type of the function
      */
      return WRC_Prune;
    }
#ifndef SQLITE_OMIT_SUBQUERY
    case TK_SELECT:
    case TK_EXISTS:  testcase( pExpr->op==TK_EXISTS );
#endif
    case TK_IN: {
      testcase( pExpr->op==TK_IN );
      if( ExprUseXSelect(pExpr) ){
        int nRef = pNC->nRef;
        testcase( pNC->ncFlags & NC_IsCheck );
        testcase( pNC->ncFlags & NC_PartIdx );
        testcase( pNC->ncFlags & NC_IdxExpr );
        testcase( pNC->ncFlags & NC_GenCol );
        assert( pExpr->x.pSelect );
        if( pNC->ncFlags & NC_SelfRef ){
          notValidImpl(pParse, pNC, "subqueries", pExpr, pExpr);
        }else{
          sqlite3WalkSelect(pWalker, pExpr->x.pSelect);
        }
        assert( pNC->nRef>=nRef );
        if( nRef!=pNC->nRef ){
          ExprSetProperty(pExpr, EP_VarSelect);
          pExpr->x.pSelect->selFlags |= SF_Correlated;
        }
        pNC->ncFlags |= NC_Subquery;
      }
      break;
    }
    case TK_VARIABLE: {
      testcase( pNC->ncFlags & NC_IsCheck );
      testcase( pNC->ncFlags & NC_PartIdx );
      testcase( pNC->ncFlags & NC_IdxExpr );
      testcase( pNC->ncFlags & NC_GenCol );
      sqlite3ResolveNotValid(pParse, pNC, "parameters",
               NC_IsCheck|NC_PartIdx|NC_IdxExpr|NC_GenCol, pExpr, pExpr);
      break;
    }
    case TK_IS:
    case TK_ISNOT: {
      Expr *pRight = sqlite3ExprSkipCollateAndLikely(pExpr->pRight);
      assert( !ExprHasProperty(pExpr, EP_Reduced) );
      /* Handle special cases of "x IS TRUE", "x IS FALSE", "x IS NOT TRUE",
      ** and "x IS NOT FALSE". */
      if( ALWAYS(pRight) && (pRight->op==TK_ID || pRight->op==TK_TRUEFALSE) ){
        int rc = resolveExprStep(pWalker, pRight);
        if( rc==WRC_Abort ) return WRC_Abort;
        if( pRight->op==TK_TRUEFALSE ){
          pExpr->op2 = pExpr->op;
          pExpr->op = TK_TRUTH;
          return WRC_Continue;
        }
      }
      /* no break */ deliberate_fall_through
    }
    case TK_BETWEEN:
    case TK_EQ:
    case TK_NE:
    case TK_LT:
    case TK_LE:
    case TK_GT:
    case TK_GE: {
      int nLeft, nRight;
      if( pParse->db->mallocFailed ) break;
      assert( pExpr->pLeft!=0 );
      nLeft = sqlite3ExprVectorSize(pExpr->pLeft);
      if( pExpr->op==TK_BETWEEN ){
        assert( ExprUseXList(pExpr) );
        nRight = sqlite3ExprVectorSize(pExpr->x.pList->a[0].pExpr);
        if( nRight==nLeft ){
          nRight = sqlite3ExprVectorSize(pExpr->x.pList->a[1].pExpr);
        }
      }else{
        assert( pExpr->pRight!=0 );
        nRight = sqlite3ExprVectorSize(pExpr->pRight);
      }
      if( nLeft!=nRight ){
        testcase( pExpr->op==TK_EQ );
        testcase( pExpr->op==TK_NE );
        testcase( pExpr->op==TK_LT );
        testcase( pExpr->op==TK_LE );
        testcase( pExpr->op==TK_GT );
        testcase( pExpr->op==TK_GE );
        testcase( pExpr->op==TK_IS );
        testcase( pExpr->op==TK_ISNOT );
        testcase( pExpr->op==TK_BETWEEN );
        sqlite3ErrorMsg(pParse, "row value misused");
        sqlite3RecordErrorOffsetOfExpr(pParse->db, pExpr);
      }
      break;
    }
  }
  assert( pParse->db->mallocFailed==0 || pParse->nErr!=0 );
  return pParse->nErr ? WRC_Abort : WRC_Continue;
}
