
/*
** Unlink the Window object from the Select to which it is attached,
** if it is attached.
*/
SQLITE_PRIVATE void sqlite3WindowUnlinkFromSelect(Window *p){
  if( p->ppThis ){
    *p->ppThis = p->pNextWin;
    if( p->pNextWin ) p->pNextWin->ppThis = p->ppThis;
    p->ppThis = 0;
  }
}

/*
** Free the Window object passed as the second argument.
*/
SQLITE_PRIVATE void sqlite3WindowDelete(sqlite3 *db, Window *p){
  if( p ){
    sqlite3WindowUnlinkFromSelect(p);
    sqlite3ExprDelete(db, p->pFilter);
    sqlite3ExprListDelete(db, p->pPartition);
    sqlite3ExprListDelete(db, p->pOrderBy);
    sqlite3ExprDelete(db, p->pEnd);
    sqlite3ExprDelete(db, p->pStart);
    sqlite3DbFree(db, p->zName);
    sqlite3DbFree(db, p->zBase);
    sqlite3DbFree(db, p);
  }
}

/*
** Free the linked list of Window objects starting at the second argument.
*/
SQLITE_PRIVATE void sqlite3WindowListDelete(sqlite3 *db, Window *p){
  while( p ){
    Window *pNext = p->pNextWin;
    sqlite3WindowDelete(db, p);
    p = pNext;
  }
}

/*
** The argument expression is an PRECEDING or FOLLOWING offset.  The
** value should be a non-negative integer.  If the value is not a
** constant, change it to NULL.  The fact that it is then a non-negative
** integer will be caught later.  But it is important not to leave
** variable values in the expression tree.
*/
static Expr *sqlite3WindowOffsetExpr(Parse *pParse, Expr *pExpr){
  if( 0==sqlite3ExprIsConstant(0,pExpr) ){
    if( IN_RENAME_OBJECT ) sqlite3RenameExprUnmap(pParse, pExpr);
    sqlite3ExprDelete(pParse->db, pExpr);
    pExpr = sqlite3ExprAlloc(pParse->db, TK_NULL, 0, 0);
  }
  return pExpr;
}

/*
** Allocate and return a new Window object describing a Window Definition.
*/
SQLITE_PRIVATE Window *sqlite3WindowAlloc(
  Parse *pParse,    /* Parsing context */
  int eType,        /* Frame type. TK_RANGE, TK_ROWS, TK_GROUPS, or 0 */
  int eStart,       /* Start type: CURRENT, PRECEDING, FOLLOWING, UNBOUNDED */
  Expr *pStart,     /* Start window size if TK_PRECEDING or FOLLOWING */
  int eEnd,         /* End type: CURRENT, FOLLOWING, TK_UNBOUNDED, PRECEDING */
  Expr *pEnd,       /* End window size if TK_FOLLOWING or PRECEDING */
  u8 eExclude       /* EXCLUDE clause */
){
  Window *pWin = 0;
  int bImplicitFrame = 0;

  /* Parser assures the following: */
  assert( eType==0 || eType==TK_RANGE || eType==TK_ROWS || eType==TK_GROUPS );
  assert( eStart==TK_CURRENT || eStart==TK_PRECEDING
           || eStart==TK_UNBOUNDED || eStart==TK_FOLLOWING );
  assert( eEnd==TK_CURRENT || eEnd==TK_FOLLOWING
           || eEnd==TK_UNBOUNDED || eEnd==TK_PRECEDING );
  assert( (eStart==TK_PRECEDING || eStart==TK_FOLLOWING)==(pStart!=0) );
  assert( (eEnd==TK_FOLLOWING || eEnd==TK_PRECEDING)==(pEnd!=0) );

  if( eType==0 ){
    bImplicitFrame = 1;
    eType = TK_RANGE;
  }

  /* Additionally, the
  ** starting boundary type may not occur earlier in the following list than
  ** the ending boundary type:
  **
  **   UNBOUNDED PRECEDING
  **   <expr> PRECEDING
  **   CURRENT ROW
  **   <expr> FOLLOWING
  **   UNBOUNDED FOLLOWING
  **
  ** The parser ensures that "UNBOUNDED PRECEDING" cannot be used as an ending
  ** boundary, and than "UNBOUNDED FOLLOWING" cannot be used as a starting
  ** frame boundary.
  */
  if( (eStart==TK_CURRENT && eEnd==TK_PRECEDING)
   || (eStart==TK_FOLLOWING && (eEnd==TK_PRECEDING || eEnd==TK_CURRENT))
  ){
    sqlite3ErrorMsg(pParse, "unsupported frame specification");
    goto windowAllocErr;
  }

  pWin = (Window*)sqlite3DbMallocZero(pParse->db, sizeof(Window));
  if( pWin==0 ) goto windowAllocErr;
  pWin->eFrmType = eType;
  pWin->eStart = eStart;
  pWin->eEnd = eEnd;
  if( eExclude==0 && OptimizationDisabled(pParse->db, SQLITE_WindowFunc) ){
    eExclude = TK_NO;
  }
  pWin->eExclude = eExclude;
  pWin->bImplicitFrame = bImplicitFrame;
  pWin->pEnd = sqlite3WindowOffsetExpr(pParse, pEnd);
  pWin->pStart = sqlite3WindowOffsetExpr(pParse, pStart);
  return pWin;

windowAllocErr:
  sqlite3ExprDelete(pParse->db, pEnd);
  sqlite3ExprDelete(pParse->db, pStart);
  return 0;
}

/*
** Attach PARTITION and ORDER BY clauses pPartition and pOrderBy to window
** pWin. Also, if parameter pBase is not NULL, set pWin->zBase to the
** equivalent nul-terminated string.
*/
SQLITE_PRIVATE Window *sqlite3WindowAssemble(
  Parse *pParse,
  Window *pWin,
  ExprList *pPartition,
  ExprList *pOrderBy,
  Token *pBase
){
  if( pWin ){
    pWin->pPartition = pPartition;
    pWin->pOrderBy = pOrderBy;
    if( pBase ){
      pWin->zBase = sqlite3DbStrNDup(pParse->db, pBase->z, pBase->n);
    }
  }else{
    sqlite3ExprListDelete(pParse->db, pPartition);
    sqlite3ExprListDelete(pParse->db, pOrderBy);
  }
  return pWin;
}

/*
** Window *pWin has just been created from a WINDOW clause. Token pBase
** is the base window. Earlier windows from the same WINDOW clause are
** stored in the linked list starting at pWin->pNextWin. This function
** either updates *pWin according to the base specification, or else
** leaves an error in pParse.
*/
SQLITE_PRIVATE void sqlite3WindowChain(Parse *pParse, Window *pWin, Window *pList){
  if( pWin->zBase ){
    sqlite3 *db = pParse->db;
    Window *pExist = windowFind(pParse, pList, pWin->zBase);
    if( pExist ){
      const char *zErr = 0;
      /* Check for errors */
      if( pWin->pPartition ){
        zErr = "PARTITION clause";
      }else if( pExist->pOrderBy && pWin->pOrderBy ){
        zErr = "ORDER BY clause";
      }else if( pExist->bImplicitFrame==0 ){
        zErr = "frame specification";
      }
      if( zErr ){
        sqlite3ErrorMsg(pParse,
            "cannot override %s of window: %s", zErr, pWin->zBase
        );
      }else{
        pWin->pPartition = sqlite3ExprListDup(db, pExist->pPartition, 0);
        if( pExist->pOrderBy ){
          assert( pWin->pOrderBy==0 );
          pWin->pOrderBy = sqlite3ExprListDup(db, pExist->pOrderBy, 0);
        }
        sqlite3DbFree(db, pWin->zBase);
        pWin->zBase = 0;
      }
    }
  }
}

/*
** Attach window object pWin to expression p.
*/
SQLITE_PRIVATE void sqlite3WindowAttach(Parse *pParse, Expr *p, Window *pWin){
  if( p ){
    assert( p->op==TK_FUNCTION );
    assert( pWin );
    assert( ExprIsFullSize(p) );
    p->y.pWin = pWin;
    ExprSetProperty(p, EP_WinFunc|EP_FullSize);
    pWin->pOwner = p;
    if( (p->flags & EP_Distinct) && pWin->eFrmType!=TK_FILTER ){
      sqlite3ErrorMsg(pParse,
          "DISTINCT is not supported for window functions"
      );
    }
  }else{
    sqlite3WindowDelete(pParse->db, pWin);
  }
}

/*
** Possibly link window pWin into the list at pSel->pWin (window functions
** to be processed as part of SELECT statement pSel). The window is linked
** in if either (a) there are no other windows already linked to this
** SELECT, or (b) the windows already linked use a compatible window frame.
*/
SQLITE_PRIVATE void sqlite3WindowLink(Select *pSel, Window *pWin){
  if( pSel ){
    if( 0==pSel->pWin || 0==sqlite3WindowCompare(0, pSel->pWin, pWin, 0) ){
      pWin->pNextWin = pSel->pWin;
      if( pSel->pWin ){
        pSel->pWin->ppThis = &pWin->pNextWin;
      }
      pSel->pWin = pWin;
      pWin->ppThis = &pSel->pWin;
    }else{
      if( sqlite3ExprListCompare(pWin->pPartition, pSel->pWin->pPartition,-1) ){
        pSel->selFlags |= SF_MultiPart;
      }
    }
  }
}

/*
** Return 0 if the two window objects are identical, 1 if they are
** different, or 2 if it cannot be determined if the objects are identical
** or not. Identical window objects can be processed in a single scan.
*/
SQLITE_PRIVATE int sqlite3WindowCompare(
  const Parse *pParse,
  const Window *p1,
  const Window *p2,
  int bFilter
){
  int res;
  if( NEVER(p1==0) || NEVER(p2==0) ) return 1;
  if( p1->eFrmType!=p2->eFrmType ) return 1;
  if( p1->eStart!=p2->eStart ) return 1;
  if( p1->eEnd!=p2->eEnd ) return 1;
  if( p1->eExclude!=p2->eExclude ) return 1;
  if( sqlite3ExprCompare(pParse, p1->pStart, p2->pStart, -1) ) return 1;
  if( sqlite3ExprCompare(pParse, p1->pEnd, p2->pEnd, -1) ) return 1;
  if( (res = sqlite3ExprListCompare(p1->pPartition, p2->pPartition, -1)) ){
    return res;
  }
  if( (res = sqlite3ExprListCompare(p1->pOrderBy, p2->pOrderBy, -1)) ){
    return res;
  }
  if( bFilter ){
    if( (res = sqlite3ExprCompare(pParse, p1->pFilter, p2->pFilter, -1)) ){
      return res;
    }
  }
  return 0;
}


/*
** This is called by code in select.c before it calls sqlite3WhereBegin()
** to begin iterating through the sub-query results. It is used to allocate
** and initialize registers and cursors used by sqlite3WindowCodeStep().
*/
SQLITE_PRIVATE void sqlite3WindowCodeInit(Parse *pParse, Select *pSelect){
  int nEphExpr = pSelect->pSrc->a[0].pSelect->pEList->nExpr;
  Window *pMWin = pSelect->pWin;
  Window *pWin;
  Vdbe *v = sqlite3GetVdbe(pParse);

  sqlite3VdbeAddOp2(v, OP_OpenEphemeral, pMWin->iEphCsr, nEphExpr);
  sqlite3VdbeAddOp2(v, OP_OpenDup, pMWin->iEphCsr+1, pMWin->iEphCsr);
  sqlite3VdbeAddOp2(v, OP_OpenDup, pMWin->iEphCsr+2, pMWin->iEphCsr);
  sqlite3VdbeAddOp2(v, OP_OpenDup, pMWin->iEphCsr+3, pMWin->iEphCsr);

  /* Allocate registers to use for PARTITION BY values, if any. Initialize
  ** said registers to NULL.  */
  if( pMWin->pPartition ){
    int nExpr = pMWin->pPartition->nExpr;
    pMWin->regPart = pParse->nMem+1;
    pParse->nMem += nExpr;
    sqlite3VdbeAddOp3(v, OP_Null, 0, pMWin->regPart, pMWin->regPart+nExpr-1);
  }

  pMWin->regOne = ++pParse->nMem;
  sqlite3VdbeAddOp2(v, OP_Integer, 1, pMWin->regOne);

  if( pMWin->eExclude ){
    pMWin->regStartRowid = ++pParse->nMem;
    pMWin->regEndRowid = ++pParse->nMem;
    pMWin->csrApp = pParse->nTab++;
    sqlite3VdbeAddOp2(v, OP_Integer, 1, pMWin->regStartRowid);
    sqlite3VdbeAddOp2(v, OP_Integer, 0, pMWin->regEndRowid);
    sqlite3VdbeAddOp2(v, OP_OpenDup, pMWin->csrApp, pMWin->iEphCsr);
    return;
  }

  for(pWin=pMWin; pWin; pWin=pWin->pNextWin){
    FuncDef *p = pWin->pWFunc;
    if( (p->funcFlags & SQLITE_FUNC_MINMAX) && pWin->eStart!=TK_UNBOUNDED ){
      /* The inline versions of min() and max() require a single ephemeral
      ** table and 3 registers. The registers are used as follows:
      **
      **   regApp+0: slot to copy min()/max() argument to for MakeRecord
      **   regApp+1: integer value used to ensure keys are unique
      **   regApp+2: output of MakeRecord
      */
      ExprList *pList;
      KeyInfo *pKeyInfo;
      assert( ExprUseXList(pWin->pOwner) );
      pList = pWin->pOwner->x.pList;
      pKeyInfo = sqlite3KeyInfoFromExprList(pParse, pList, 0, 0);
      pWin->csrApp = pParse->nTab++;
      pWin->regApp = pParse->nMem+1;
      pParse->nMem += 3;
      if( pKeyInfo && pWin->pWFunc->zName[1]=='i' ){
        assert( pKeyInfo->aSortFlags[0]==0 );
        pKeyInfo->aSortFlags[0] = KEYINFO_ORDER_DESC;
      }
      sqlite3VdbeAddOp2(v, OP_OpenEphemeral, pWin->csrApp, 2);
      sqlite3VdbeAppendP4(v, pKeyInfo, P4_KEYINFO);
      sqlite3VdbeAddOp2(v, OP_Integer, 0, pWin->regApp+1);
    }
    else if( p->zName==nth_valueName || p->zName==first_valueName ){
      /* Allocate two registers at pWin->regApp. These will be used to
      ** store the start and end index of the current frame.  */
      pWin->regApp = pParse->nMem+1;
      pWin->csrApp = pParse->nTab++;
      pParse->nMem += 2;
      sqlite3VdbeAddOp2(v, OP_OpenDup, pWin->csrApp, pMWin->iEphCsr);
    }
    else if( p->zName==leadName || p->zName==lagName ){
      pWin->csrApp = pParse->nTab++;
      sqlite3VdbeAddOp2(v, OP_OpenDup, pWin->csrApp, pMWin->iEphCsr);
    }
  }
}

#define WINDOW_STARTING_INT  0
#define WINDOW_ENDING_INT    1
#define WINDOW_NTH_VALUE_INT 2
#define WINDOW_STARTING_NUM  3
#define WINDOW_ENDING_NUM    4

/*
** A "PRECEDING <expr>" (eCond==0) or "FOLLOWING <expr>" (eCond==1) or the
** value of the second argument to nth_value() (eCond==2) has just been
** evaluated and the result left in register reg. This function generates VM
** code to check that the value is a non-negative integer and throws an
** exception if it is not.
*/
static void windowCheckValue(Parse *pParse, int reg, int eCond){
  static const char *azErr[] = {
    "frame starting offset must be a non-negative integer",
    "frame ending offset must be a non-negative integer",
    "second argument to nth_value must be a positive integer",
    "frame starting offset must be a non-negative number",
    "frame ending offset must be a non-negative number",
  };
  static int aOp[] = { OP_Ge, OP_Ge, OP_Gt, OP_Ge, OP_Ge };
  Vdbe *v = sqlite3GetVdbe(pParse);
  int regZero = sqlite3GetTempReg(pParse);
  assert( eCond>=0 && eCond<ArraySize(azErr) );
  sqlite3VdbeAddOp2(v, OP_Integer, 0, regZero);
  if( eCond>=WINDOW_STARTING_NUM ){
    int regString = sqlite3GetTempReg(pParse);
    sqlite3VdbeAddOp4(v, OP_String8, 0, regString, 0, "", P4_STATIC);
    sqlite3VdbeAddOp3(v, OP_Ge, regString, sqlite3VdbeCurrentAddr(v)+2, reg);
    sqlite3VdbeChangeP5(v, SQLITE_AFF_NUMERIC|SQLITE_JUMPIFNULL);
    VdbeCoverage(v);
    assert( eCond==3 || eCond==4 );
    VdbeCoverageIf(v, eCond==3);
    VdbeCoverageIf(v, eCond==4);
  }else{
    sqlite3VdbeAddOp2(v, OP_MustBeInt, reg, sqlite3VdbeCurrentAddr(v)+2);
    VdbeCoverage(v);
    assert( eCond==0 || eCond==1 || eCond==2 );
    VdbeCoverageIf(v, eCond==0);
    VdbeCoverageIf(v, eCond==1);
    VdbeCoverageIf(v, eCond==2);
  }
  sqlite3VdbeAddOp3(v, aOp[eCond], regZero, sqlite3VdbeCurrentAddr(v)+2, reg);
  sqlite3VdbeChangeP5(v, SQLITE_AFF_NUMERIC);
  VdbeCoverageNeverNullIf(v, eCond==0); /* NULL case captured by */
  VdbeCoverageNeverNullIf(v, eCond==1); /*   the OP_MustBeInt */
  VdbeCoverageNeverNullIf(v, eCond==2);
  VdbeCoverageNeverNullIf(v, eCond==3); /* NULL case caught by */
  VdbeCoverageNeverNullIf(v, eCond==4); /*   the OP_Ge */
  sqlite3MayAbort(pParse);
  sqlite3VdbeAddOp2(v, OP_Halt, SQLITE_ERROR, OE_Abort);
  sqlite3VdbeAppendP4(v, (void*)azErr[eCond], P4_STATIC);
  sqlite3ReleaseTempReg(pParse, regZero);
}
