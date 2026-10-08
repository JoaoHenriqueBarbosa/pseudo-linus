/*
** 2015-06-08
**
** The author disclaims copyright to this source code.  In place of
** a legal notice, here is a blessing:
**
**    May you do good and not evil.
**    May you find forgiveness for yourself and forgive others.
**    May you share freely, never taking more than you give.
**
*************************************************************************
** This module contains C code that generates VDBE code used to process
** the WHERE clause of SQL statements.
**
** This file was originally part of where.c but was split out to improve
** readability and editability.  This file contains utility routines for
** analyzing Expr objects in the WHERE clause.
*/
/* #include "sqliteInt.h" */
/* #include "whereInt.h" */

/* Forward declarations */
static void exprAnalyze(SrcList*, WhereClause*, int);

/*
** Deallocate all memory associated with a WhereOrInfo object.
*/
static void whereOrInfoDelete(sqlite3 *db, WhereOrInfo *p){
  sqlite3WhereClauseClear(&p->wc);
  sqlite3DbFree(db, p);
}

/*
** Deallocate all memory associated with a WhereAndInfo object.
*/
static void whereAndInfoDelete(sqlite3 *db, WhereAndInfo *p){
  sqlite3WhereClauseClear(&p->wc);
  sqlite3DbFree(db, p);
}

/*
** Add a single new WhereTerm entry to the WhereClause object pWC.
** The new WhereTerm object is constructed from Expr p and with wtFlags.
** The index in pWC->a[] of the new WhereTerm is returned on success.
** 0 is returned if the new WhereTerm could not be added due to a memory
** allocation error.  The memory allocation failure will be recorded in
** the db->mallocFailed flag so that higher-level functions can detect it.
**
** This routine will increase the size of the pWC->a[] array as necessary.
**
** If the wtFlags argument includes TERM_DYNAMIC, then responsibility
** for freeing the expression p is assumed by the WhereClause object pWC.
** This is true even if this routine fails to allocate a new WhereTerm.
**
** WARNING:  This routine might reallocate the space used to store
** WhereTerms.  All pointers to WhereTerms should be invalidated after
** calling this routine.  Such pointers may be reinitialized by referencing
** the pWC->a[] array.
*/
static int whereClauseInsert(WhereClause *pWC, Expr *p, u16 wtFlags){
  WhereTerm *pTerm;
  int idx;
  testcase( wtFlags & TERM_VIRTUAL );
  if( pWC->nTerm>=pWC->nSlot ){
    WhereTerm *pOld = pWC->a;
    sqlite3 *db = pWC->pWInfo->pParse->db;
    pWC->a = sqlite3WhereMalloc(pWC->pWInfo, sizeof(pWC->a[0])*pWC->nSlot*2 );
    if( pWC->a==0 ){
      if( wtFlags & TERM_DYNAMIC ){
        sqlite3ExprDelete(db, p);
      }
      pWC->a = pOld;
      return 0;
    }
    memcpy(pWC->a, pOld, sizeof(pWC->a[0])*pWC->nTerm);
    pWC->nSlot = pWC->nSlot*2;
  }
  pTerm = &pWC->a[idx = pWC->nTerm++];
  if( (wtFlags & TERM_VIRTUAL)==0 ) pWC->nBase = pWC->nTerm;
  if( p && ExprHasProperty(p, EP_Unlikely) ){
    pTerm->truthProb = sqlite3LogEst(p->iTable) - 270;
  }else{
    pTerm->truthProb = 1;
  }
  pTerm->pExpr = sqlite3ExprSkipCollateAndLikely(p);
  pTerm->wtFlags = wtFlags;
  pTerm->pWC = pWC;
  pTerm->iParent = -1;
  memset(&pTerm->eOperator, 0,
         sizeof(WhereTerm) - offsetof(WhereTerm,eOperator));
  return idx;
}

/*
** Return TRUE if the given operator is one of the operators that is
** allowed for an indexable WHERE clause term.  The allowed operators are
** "=", "<", ">", "<=", ">=", "IN", "IS", and "IS NULL"
*/
static int allowedOp(int op){
  assert( TK_GT>TK_EQ && TK_GT<TK_GE );
  assert( TK_LT>TK_EQ && TK_LT<TK_GE );
  assert( TK_LE>TK_EQ && TK_LE<TK_GE );
  assert( TK_GE==TK_EQ+4 );
  return op==TK_IN || (op>=TK_EQ && op<=TK_GE) || op==TK_ISNULL || op==TK_IS;
}

/*
** Commute a comparison operator.  Expressions of the form "X op Y"
** are converted into "Y op X".
*/
static u16 exprCommute(Parse *pParse, Expr *pExpr){
  if( pExpr->pLeft->op==TK_VECTOR
   || pExpr->pRight->op==TK_VECTOR
   || sqlite3BinaryCompareCollSeq(pParse, pExpr->pLeft, pExpr->pRight) !=
      sqlite3BinaryCompareCollSeq(pParse, pExpr->pRight, pExpr->pLeft)
  ){
    pExpr->flags ^= EP_Commuted;
  }
  SWAP(Expr*,pExpr->pRight,pExpr->pLeft);
  if( pExpr->op>=TK_GT ){
    assert( TK_LT==TK_GT+2 );
    assert( TK_GE==TK_LE+2 );
    assert( TK_GT>TK_EQ );
    assert( TK_GT<TK_LE );
    assert( pExpr->op>=TK_GT && pExpr->op<=TK_GE );
    pExpr->op = ((pExpr->op-TK_GT)^2)+TK_GT;
  }
  return 0;
}

/*
** Translate from TK_xx operator to WO_xx bitmask.
*/
static u16 operatorMask(int op){
  u16 c;
  assert( allowedOp(op) );
  if( op==TK_IN ){
    c = WO_IN;
  }else if( op==TK_ISNULL ){
    c = WO_ISNULL;
  }else if( op==TK_IS ){
    c = WO_IS;
  }else{
    assert( (WO_EQ<<(op-TK_EQ)) < 0x7fff );
    c = (u16)(WO_EQ<<(op-TK_EQ));
  }
  assert( op!=TK_ISNULL || c==WO_ISNULL );
  assert( op!=TK_IN || c==WO_IN );
  assert( op!=TK_EQ || c==WO_EQ );
  assert( op!=TK_LT || c==WO_LT );
  assert( op!=TK_LE || c==WO_LE );
  assert( op!=TK_GT || c==WO_GT );
  assert( op!=TK_GE || c==WO_GE );
  assert( op!=TK_IS || c==WO_IS );
  return c;
}


#ifndef SQLITE_OMIT_LIKE_OPTIMIZATION
/*
** Check to see if the given expression is a LIKE or GLOB operator that
** can be optimized using inequality constraints.  Return TRUE if it is
** so and false if not.
**
** In order for the operator to be optimizible, the RHS must be a string
** literal that does not begin with a wildcard.  The LHS must be a column
** that may only be NULL, a string, or a BLOB, never a number. (This means
** that virtual tables cannot participate in the LIKE optimization.)  The
** collating sequence for the column on the LHS must be appropriate for
** the operator.
*/
static int isLikeOrGlob(
  Parse *pParse,    /* Parsing and code generating context */
  Expr *pExpr,      /* Test this expression */
  Expr **ppPrefix,  /* Pointer to TK_STRING expression with pattern prefix */
  int *pisComplete, /* True if the only wildcard is % in the last character */
  int *pnoCase      /* True if uppercase is equivalent to lowercase */
){
  const u8 *z = 0;           /* String on RHS of LIKE operator */
  Expr *pRight, *pLeft;      /* Right and left size of LIKE operator */
  ExprList *pList;           /* List of operands to the LIKE operator */
  u8 c;                      /* One character in z[] */
  int cnt;                   /* Number of non-wildcard prefix characters */
  u8 wc[4];                  /* Wildcard characters */
  sqlite3 *db = pParse->db;  /* Database connection */
  sqlite3_value *pVal = 0;
  int op;                    /* Opcode of pRight */
  int rc;                    /* Result code to return */

  if( !sqlite3IsLikeFunction(db, pExpr, pnoCase, (char*)wc) ){
    return 0;
  }
#ifdef SQLITE_EBCDIC
  if( *pnoCase ) return 0;
#endif
  assert( ExprUseXList(pExpr) );
  pList = pExpr->x.pList;
  pLeft = pList->a[1].pExpr;

  pRight = sqlite3ExprSkipCollate(pList->a[0].pExpr);
  op = pRight->op;
  if( op==TK_VARIABLE && (db->flags & SQLITE_EnableQPSG)==0 ){
    Vdbe *pReprepare = pParse->pReprepare;
    int iCol = pRight->iColumn;
    pVal = sqlite3VdbeGetBoundValue(pReprepare, iCol, SQLITE_AFF_BLOB);
    if( pVal && sqlite3_value_type(pVal)==SQLITE_TEXT ){
      z = sqlite3_value_text(pVal);
    }
    sqlite3VdbeSetVarmask(pParse->pVdbe, iCol);
    assert( pRight->op==TK_VARIABLE || pRight->op==TK_REGISTER );
  }else if( op==TK_STRING ){
    assert( !ExprHasProperty(pRight, EP_IntValue) );
     z = (u8*)pRight->u.zToken;
  }
  if( z ){

    /* Count the number of prefix characters prior to the first wildcard */
    cnt = 0;
    while( (c=z[cnt])!=0 && c!=wc[0] && c!=wc[1] && c!=wc[2] ){
      cnt++;
      if( c==wc[3] && z[cnt]!=0 ) cnt++;
    }

    /* The optimization is possible only if (1) the pattern does not begin
    ** with a wildcard and if (2) the non-wildcard prefix does not end with
    ** an (illegal 0xff) character, or (3) the pattern does not consist of
    ** a single escape character. The second condition is necessary so
    ** that we can increment the prefix key to find an upper bound for the
    ** range search. The third is because the caller assumes that the pattern
    ** consists of at least one character after all escapes have been
    ** removed.  */
    if( (cnt>1 || (cnt>0 && z[0]!=wc[3])) && 255!=(u8)z[cnt-1] ){
      Expr *pPrefix;

      /* A "complete" match if the pattern ends with "*" or "%" */
      *pisComplete = c==wc[0] && z[cnt+1]==0;

      /* Get the pattern prefix.  Remove all escapes from the prefix. */
      pPrefix = sqlite3Expr(db, TK_STRING, (char*)z);
      if( pPrefix ){
        int iFrom, iTo;
        char *zNew;
        assert( !ExprHasProperty(pPrefix, EP_IntValue) );
        zNew = pPrefix->u.zToken;
        zNew[cnt] = 0;
        for(iFrom=iTo=0; iFrom<cnt; iFrom++){
          if( zNew[iFrom]==wc[3] ) iFrom++;
          zNew[iTo++] = zNew[iFrom];
        }
        zNew[iTo] = 0;
        assert( iTo>0 );

        /* If the LHS is not an ordinary column with TEXT affinity, then the
        ** pattern prefix boundaries (both the start and end boundaries) must
        ** not look like a number.  Otherwise the pattern might be treated as
        ** a number, which will invalidate the LIKE optimization.
        **
        ** Getting this right has been a persistent source of bugs in the
        ** LIKE optimization.  See, for example:
        **    2018-09-10 https://sqlite.org/src/info/c94369cae9b561b1
        **    2019-05-02 https://sqlite.org/src/info/b043a54c3de54b28
        **    2019-06-10 https://sqlite.org/src/info/fd76310a5e843e07
        **    2019-06-14 https://sqlite.org/src/info/ce8717f0885af975
        **    2019-09-03 https://sqlite.org/src/info/0f0428096f17252a
        */
        if( pLeft->op!=TK_COLUMN
         || sqlite3ExprAffinity(pLeft)!=SQLITE_AFF_TEXT
         || (ALWAYS( ExprUseYTab(pLeft) )
             && ALWAYS(pLeft->y.pTab)
             && IsVirtual(pLeft->y.pTab))  /* Might be numeric */
        ){
          int isNum;
          double rDummy;
          isNum = sqlite3AtoF(zNew, &rDummy, iTo, SQLITE_UTF8);
          if( isNum<=0 ){
            if( iTo==1 && zNew[0]=='-' ){
              isNum = +1;
            }else{
              zNew[iTo-1]++;
              isNum = sqlite3AtoF(zNew, &rDummy, iTo, SQLITE_UTF8);
              zNew[iTo-1]--;
            }
          }
          if( isNum>0 ){
            sqlite3ExprDelete(db, pPrefix);
            sqlite3ValueFree(pVal);
            return 0;
          }
        }
      }
      *ppPrefix = pPrefix;

      /* If the RHS pattern is a bound parameter, make arrangements to
      ** reprepare the statement when that parameter is rebound */
      if( op==TK_VARIABLE ){
        Vdbe *v = pParse->pVdbe;
        sqlite3VdbeSetVarmask(v, pRight->iColumn);
        assert( !ExprHasProperty(pRight, EP_IntValue) );
        if( *pisComplete && pRight->u.zToken[1] ){
          /* If the rhs of the LIKE expression is a variable, and the current
          ** value of the variable means there is no need to invoke the LIKE
          ** function, then no OP_Variable will be added to the program.
          ** This causes problems for the sqlite3_bind_parameter_name()
          ** API. To work around them, add a dummy OP_Variable here.
          */
          int r1 = sqlite3GetTempReg(pParse);
          sqlite3ExprCodeTarget(pParse, pRight, r1);
          sqlite3VdbeChangeP3(v, sqlite3VdbeCurrentAddr(v)-1, 0);
          sqlite3ReleaseTempReg(pParse, r1);
        }
      }
    }else{
      z = 0;
    }
  }

  rc = (z!=0);
  sqlite3ValueFree(pVal);
  return rc;
}
#endif /* SQLITE_OMIT_LIKE_OPTIMIZATION */


#ifndef SQLITE_OMIT_VIRTUALTABLE
/*
** Check to see if the pExpr expression is a form that needs to be passed
** to the xBestIndex method of virtual tables.  Forms of interest include:
**
**          Expression                   Virtual Table Operator
**          -----------------------      ---------------------------------
**      1.  column MATCH expr            SQLITE_INDEX_CONSTRAINT_MATCH
**      2.  column GLOB expr             SQLITE_INDEX_CONSTRAINT_GLOB
**      3.  column LIKE expr             SQLITE_INDEX_CONSTRAINT_LIKE
**      4.  column REGEXP expr           SQLITE_INDEX_CONSTRAINT_REGEXP
**      5.  column != expr               SQLITE_INDEX_CONSTRAINT_NE
**      6.  expr != column               SQLITE_INDEX_CONSTRAINT_NE
**      7.  column IS NOT expr           SQLITE_INDEX_CONSTRAINT_ISNOT
**      8.  expr IS NOT column           SQLITE_INDEX_CONSTRAINT_ISNOT
**      9.  column IS NOT NULL           SQLITE_INDEX_CONSTRAINT_ISNOTNULL
**
** In every case, "column" must be a column of a virtual table.  If there
** is a match, set *ppLeft to the "column" expression, set *ppRight to the
** "expr" expression (even though in forms (6) and (8) the column is on the
** right and the expression is on the left).  Also set *peOp2 to the
** appropriate virtual table operator.  The return value is 1 or 2 if there
** is a match.  The usual return is 1, but if the RHS is also a column
** of virtual table in forms (5) or (7) then return 2.
**
** If the expression matches none of the patterns above, return 0.
*/
static int isAuxiliaryVtabOperator(
  sqlite3 *db,                    /* Parsing context */
  Expr *pExpr,                    /* Test this expression */
  unsigned char *peOp2,           /* OUT: 0 for MATCH, or else an op2 value */
  Expr **ppLeft,                  /* Column expression to left of MATCH/op2 */
  Expr **ppRight                  /* Expression to left of MATCH/op2 */
){
  if( pExpr->op==TK_FUNCTION ){
    static const struct Op2 {
      const char *zOp;
      unsigned char eOp2;
    } aOp[] = {
      { "match",  SQLITE_INDEX_CONSTRAINT_MATCH },
      { "glob",   SQLITE_INDEX_CONSTRAINT_GLOB },
      { "like",   SQLITE_INDEX_CONSTRAINT_LIKE },
      { "regexp", SQLITE_INDEX_CONSTRAINT_REGEXP }
    };
    ExprList *pList;
    Expr *pCol;                     /* Column reference */
    int i;

    assert( ExprUseXList(pExpr) );
    pList = pExpr->x.pList;
    if( pList==0 || pList->nExpr!=2 ){
      return 0;
    }

    /* Built-in operators MATCH, GLOB, LIKE, and REGEXP attach to a
    ** virtual table on their second argument, which is the same as
    ** the left-hand side operand in their in-fix form.
    **
    **       vtab_column MATCH expression
    **       MATCH(expression,vtab_column)
    */
    pCol = pList->a[1].pExpr;
    assert( pCol->op!=TK_COLUMN || (ExprUseYTab(pCol) && pCol->y.pTab!=0) );
    if( ExprIsVtab(pCol) ){
      for(i=0; i<ArraySize(aOp); i++){
        assert( !ExprHasProperty(pExpr, EP_IntValue) );
        if( sqlite3StrICmp(pExpr->u.zToken, aOp[i].zOp)==0 ){
          *peOp2 = aOp[i].eOp2;
          *ppRight = pList->a[0].pExpr;
          *ppLeft = pCol;
          return 1;
        }
      }
    }

    /* We can also match against the first column of overloaded
    ** functions where xFindFunction returns a value of at least
    ** SQLITE_INDEX_CONSTRAINT_FUNCTION.
    **
    **      OVERLOADED(vtab_column,expression)
    **
    ** Historically, xFindFunction expected to see lower-case function
    ** names.  But for this use case, xFindFunction is expected to deal
    ** with function names in an arbitrary case.
    */
    pCol = pList->a[0].pExpr;
    assert( pCol->op!=TK_COLUMN || ExprUseYTab(pCol) );
    assert( pCol->op!=TK_COLUMN || (ExprUseYTab(pCol) && pCol->y.pTab!=0) );
    if( ExprIsVtab(pCol) ){
      sqlite3_vtab *pVtab;
      sqlite3_module *pMod;
      void (*xNotUsed)(sqlite3_context*,int,sqlite3_value**);
      void *pNotUsed;
      pVtab = sqlite3GetVTable(db, pCol->y.pTab)->pVtab;
      assert( pVtab!=0 );
      assert( pVtab->pModule!=0 );
      assert( !ExprHasProperty(pExpr, EP_IntValue) );
      pMod = (sqlite3_module *)pVtab->pModule;
      if( pMod->xFindFunction!=0 ){
        i = pMod->xFindFunction(pVtab,2, pExpr->u.zToken, &xNotUsed, &pNotUsed);
        if( i>=SQLITE_INDEX_CONSTRAINT_FUNCTION ){
          *peOp2 = i;
          *ppRight = pList->a[1].pExpr;
          *ppLeft = pCol;
          return 1;
        }
      }
    }
  }else if( pExpr->op==TK_NE || pExpr->op==TK_ISNOT || pExpr->op==TK_NOTNULL ){
    int res = 0;
    Expr *pLeft = pExpr->pLeft;
    Expr *pRight = pExpr->pRight;
    assert( pLeft->op!=TK_COLUMN || (ExprUseYTab(pLeft) && pLeft->y.pTab!=0) );
    if( ExprIsVtab(pLeft) ){
      res++;
    }
    assert( pRight==0 || pRight->op!=TK_COLUMN
            || (ExprUseYTab(pRight) && pRight->y.pTab!=0) );
    if( pRight && ExprIsVtab(pRight) ){
      res++;
      SWAP(Expr*, pLeft, pRight);
    }
    *ppLeft = pLeft;
    *ppRight = pRight;
    if( pExpr->op==TK_NE ) *peOp2 = SQLITE_INDEX_CONSTRAINT_NE;
    if( pExpr->op==TK_ISNOT ) *peOp2 = SQLITE_INDEX_CONSTRAINT_ISNOT;
    if( pExpr->op==TK_NOTNULL ) *peOp2 = SQLITE_INDEX_CONSTRAINT_ISNOTNULL;
    return res;
  }
  return 0;
}
