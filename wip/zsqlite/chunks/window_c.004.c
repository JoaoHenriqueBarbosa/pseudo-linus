
/*
** Return the number of arguments passed to the window-function associated
** with the object passed as the only argument to this function.
*/
static int windowArgCount(Window *pWin){
  const ExprList *pList;
  assert( ExprUseXList(pWin->pOwner) );
  pList = pWin->pOwner->x.pList;
  return (pList ? pList->nExpr : 0);
}

typedef struct WindowCodeArg WindowCodeArg;
typedef struct WindowCsrAndReg WindowCsrAndReg;

/*
** See comments above struct WindowCodeArg.
*/
struct WindowCsrAndReg {
  int csr;                        /* Cursor number */
  int reg;                        /* First in array of peer values */
};

/*
** A single instance of this structure is allocated on the stack by
** sqlite3WindowCodeStep() and a pointer to it passed to the various helper
** routines. This is to reduce the number of arguments required by each
** helper function.
**
** regArg:
**   Each window function requires an accumulator register (just as an
**   ordinary aggregate function does). This variable is set to the first
**   in an array of accumulator registers - one for each window function
**   in the WindowCodeArg.pMWin list.
**
** eDelete:
**   The window functions implementation sometimes caches the input rows
**   that it processes in a temporary table. If it is not zero, this
**   variable indicates when rows may be removed from the temp table (in
**   order to reduce memory requirements - it would always be safe just
**   to leave them there). Possible values for eDelete are:
**
**      WINDOW_RETURN_ROW:
**        An input row can be discarded after it is returned to the caller.
**
**      WINDOW_AGGINVERSE:
**        An input row can be discarded after the window functions xInverse()
**        callbacks have been invoked in it.
**
**      WINDOW_AGGSTEP:
**        An input row can be discarded after the window functions xStep()
**        callbacks have been invoked in it.
**
** start,current,end
**   Consider a window-frame similar to the following:
**
**     (ORDER BY a, b GROUPS BETWEEN 2 PRECEDING AND 2 FOLLOWING)
**
**   The windows functions implementation caches the input rows in a temp
**   table, sorted by "a, b" (it actually populates the cache lazily, and
**   aggressively removes rows once they are no longer required, but that's
**   a mere detail). It keeps three cursors open on the temp table. One
**   (current) that points to the next row to return to the query engine
**   once its window function values have been calculated. Another (end)
**   points to the next row to call the xStep() method of each window function
**   on (so that it is 2 groups ahead of current). And a third (start) that
**   points to the next row to call the xInverse() method of each window
**   function on.
**
**   Each cursor (start, current and end) consists of a VDBE cursor
**   (WindowCsrAndReg.csr) and an array of registers (starting at
**   WindowCodeArg.reg) that always contains a copy of the peer values
**   read from the corresponding cursor.
**
**   Depending on the window-frame in question, all three cursors may not
**   be required. In this case both WindowCodeArg.csr and reg are set to
**   0.
*/
struct WindowCodeArg {
  Parse *pParse;             /* Parse context */
  Window *pMWin;             /* First in list of functions being processed */
  Vdbe *pVdbe;               /* VDBE object */
  int addrGosub;             /* OP_Gosub to this address to return one row */
  int regGosub;              /* Register used with OP_Gosub(addrGosub) */
  int regArg;                /* First in array of accumulator registers */
  int eDelete;               /* See above */
  int regRowid;

  WindowCsrAndReg start;
  WindowCsrAndReg current;
  WindowCsrAndReg end;
};

/*
** Generate VM code to read the window frames peer values from cursor csr into
** an array of registers starting at reg.
*/
static void windowReadPeerValues(
  WindowCodeArg *p,
  int csr,
  int reg
){
  Window *pMWin = p->pMWin;
  ExprList *pOrderBy = pMWin->pOrderBy;
  if( pOrderBy ){
    Vdbe *v = sqlite3GetVdbe(p->pParse);
    ExprList *pPart = pMWin->pPartition;
    int iColOff = pMWin->nBufferCol + (pPart ? pPart->nExpr : 0);
    int i;
    for(i=0; i<pOrderBy->nExpr; i++){
      sqlite3VdbeAddOp3(v, OP_Column, csr, iColOff+i, reg+i);
    }
  }
}

/*
** Generate VM code to invoke either xStep() (if bInverse is 0) or
** xInverse (if bInverse is non-zero) for each window function in the
** linked list starting at pMWin. Or, for built-in window functions
** that do not use the standard function API, generate the required
** inline VM code.
**
** If argument csr is greater than or equal to 0, then argument reg is
** the first register in an array of registers guaranteed to be large
** enough to hold the array of arguments for each function. In this case
** the arguments are extracted from the current row of csr into the
** array of registers before invoking OP_AggStep or OP_AggInverse
**
** Or, if csr is less than zero, then the array of registers at reg is
** already populated with all columns from the current row of the sub-query.
**
** If argument regPartSize is non-zero, then it is a register containing the
** number of rows in the current partition.
*/
static void windowAggStep(
  WindowCodeArg *p,
  Window *pMWin,                  /* Linked list of window functions */
  int csr,                        /* Read arguments from this cursor */
  int bInverse,                   /* True to invoke xInverse instead of xStep */
  int reg                         /* Array of registers */
){
  Parse *pParse = p->pParse;
  Vdbe *v = sqlite3GetVdbe(pParse);
  Window *pWin;
  for(pWin=pMWin; pWin; pWin=pWin->pNextWin){
    FuncDef *pFunc = pWin->pWFunc;
    int regArg;
    int nArg = pWin->bExprArgs ? 0 : windowArgCount(pWin);
    int i;

    assert( bInverse==0 || pWin->eStart!=TK_UNBOUNDED );

    /* All OVER clauses in the same window function aggregate step must
    ** be the same. */
    assert( pWin==pMWin || sqlite3WindowCompare(pParse,pWin,pMWin,0)!=1 );

    for(i=0; i<nArg; i++){
      if( i!=1 || pFunc->zName!=nth_valueName ){
        sqlite3VdbeAddOp3(v, OP_Column, csr, pWin->iArgCol+i, reg+i);
      }else{
        sqlite3VdbeAddOp3(v, OP_Column, pMWin->iEphCsr, pWin->iArgCol+i, reg+i);
      }
    }
    regArg = reg;

    if( pMWin->regStartRowid==0
     && (pFunc->funcFlags & SQLITE_FUNC_MINMAX)
     && (pWin->eStart!=TK_UNBOUNDED)
    ){
      int addrIsNull = sqlite3VdbeAddOp1(v, OP_IsNull, regArg);
      VdbeCoverage(v);
      if( bInverse==0 ){
        sqlite3VdbeAddOp2(v, OP_AddImm, pWin->regApp+1, 1);
        sqlite3VdbeAddOp2(v, OP_SCopy, regArg, pWin->regApp);
        sqlite3VdbeAddOp3(v, OP_MakeRecord, pWin->regApp, 2, pWin->regApp+2);
        sqlite3VdbeAddOp2(v, OP_IdxInsert, pWin->csrApp, pWin->regApp+2);
      }else{
        sqlite3VdbeAddOp4Int(v, OP_SeekGE, pWin->csrApp, 0, regArg, 1);
        VdbeCoverageNeverTaken(v);
        sqlite3VdbeAddOp1(v, OP_Delete, pWin->csrApp);
        sqlite3VdbeJumpHere(v, sqlite3VdbeCurrentAddr(v)-2);
      }
      sqlite3VdbeJumpHere(v, addrIsNull);
    }else if( pWin->regApp ){
      assert( pFunc->zName==nth_valueName
           || pFunc->zName==first_valueName
      );
      assert( bInverse==0 || bInverse==1 );
      sqlite3VdbeAddOp2(v, OP_AddImm, pWin->regApp+1-bInverse, 1);
    }else if( pFunc->xSFunc!=noopStepFunc ){
      int addrIf = 0;
      if( pWin->pFilter ){
        int regTmp;
        assert( ExprUseXList(pWin->pOwner) );
        assert( pWin->bExprArgs || !nArg ||nArg==pWin->pOwner->x.pList->nExpr );
        assert( pWin->bExprArgs || nArg  ||pWin->pOwner->x.pList==0 );
        regTmp = sqlite3GetTempReg(pParse);
        sqlite3VdbeAddOp3(v, OP_Column, csr, pWin->iArgCol+nArg,regTmp);
        addrIf = sqlite3VdbeAddOp3(v, OP_IfNot, regTmp, 0, 1);
        VdbeCoverage(v);
        sqlite3ReleaseTempReg(pParse, regTmp);
      }

      if( pWin->bExprArgs ){
        int iOp = sqlite3VdbeCurrentAddr(v);
        int iEnd;

        assert( ExprUseXList(pWin->pOwner) );
        nArg = pWin->pOwner->x.pList->nExpr;
        regArg = sqlite3GetTempRange(pParse, nArg);
        sqlite3ExprCodeExprList(pParse, pWin->pOwner->x.pList, regArg, 0, 0);

        for(iEnd=sqlite3VdbeCurrentAddr(v); iOp<iEnd; iOp++){
          VdbeOp *pOp = sqlite3VdbeGetOp(v, iOp);
          if( pOp->opcode==OP_Column && pOp->p1==pMWin->iEphCsr ){
            pOp->p1 = csr;
          }
        }
      }
      if( pFunc->funcFlags & SQLITE_FUNC_NEEDCOLL ){
        CollSeq *pColl;
        assert( nArg>0 );
        assert( ExprUseXList(pWin->pOwner) );
        pColl = sqlite3ExprNNCollSeq(pParse, pWin->pOwner->x.pList->a[0].pExpr);
        sqlite3VdbeAddOp4(v, OP_CollSeq, 0,0,0, (const char*)pColl, P4_COLLSEQ);
      }
      sqlite3VdbeAddOp3(v, bInverse? OP_AggInverse : OP_AggStep,
                        bInverse, regArg, pWin->regAccum);
      sqlite3VdbeAppendP4(v, pFunc, P4_FUNCDEF);
      sqlite3VdbeChangeP5(v, (u8)nArg);
      if( pWin->bExprArgs ){
        sqlite3ReleaseTempRange(pParse, regArg, nArg);
      }
      if( addrIf ) sqlite3VdbeJumpHere(v, addrIf);
    }
  }
}

/*
** Values that may be passed as the second argument to windowCodeOp().
*/
#define WINDOW_RETURN_ROW 1
#define WINDOW_AGGINVERSE 2
#define WINDOW_AGGSTEP    3

/*
** Generate VM code to invoke either xValue() (bFin==0) or xFinalize()
** (bFin==1) for each window function in the linked list starting at
** pMWin. Or, for built-in window-functions that do not use the standard
** API, generate the equivalent VM code.
*/
static void windowAggFinal(WindowCodeArg *p, int bFin){
  Parse *pParse = p->pParse;
  Window *pMWin = p->pMWin;
  Vdbe *v = sqlite3GetVdbe(pParse);
  Window *pWin;

  for(pWin=pMWin; pWin; pWin=pWin->pNextWin){
    if( pMWin->regStartRowid==0
     && (pWin->pWFunc->funcFlags & SQLITE_FUNC_MINMAX)
     && (pWin->eStart!=TK_UNBOUNDED)
    ){
      sqlite3VdbeAddOp2(v, OP_Null, 0, pWin->regResult);
      sqlite3VdbeAddOp1(v, OP_Last, pWin->csrApp);
      VdbeCoverage(v);
      sqlite3VdbeAddOp3(v, OP_Column, pWin->csrApp, 0, pWin->regResult);
      sqlite3VdbeJumpHere(v, sqlite3VdbeCurrentAddr(v)-2);
    }else if( pWin->regApp ){
      assert( pMWin->regStartRowid==0 );
    }else{
      int nArg = windowArgCount(pWin);
      if( bFin ){
        sqlite3VdbeAddOp2(v, OP_AggFinal, pWin->regAccum, nArg);
        sqlite3VdbeAppendP4(v, pWin->pWFunc, P4_FUNCDEF);
        sqlite3VdbeAddOp2(v, OP_Copy, pWin->regAccum, pWin->regResult);
        sqlite3VdbeAddOp2(v, OP_Null, 0, pWin->regAccum);
      }else{
        sqlite3VdbeAddOp3(v, OP_AggValue,pWin->regAccum,nArg,pWin->regResult);
        sqlite3VdbeAppendP4(v, pWin->pWFunc, P4_FUNCDEF);
      }
    }
  }
}

/*
** Generate code to calculate the current values of all window functions in the
** p->pMWin list by doing a full scan of the current window frame. Store the
** results in the Window.regResult registers, ready to return the upper
** layer.
*/
static void windowFullScan(WindowCodeArg *p){
  Window *pWin;
  Parse *pParse = p->pParse;
  Window *pMWin = p->pMWin;
  Vdbe *v = p->pVdbe;

  int regCRowid = 0;              /* Current rowid value */
  int regCPeer = 0;               /* Current peer values */
  int regRowid = 0;               /* AggStep rowid value */
  int regPeer = 0;                /* AggStep peer values */

  int nPeer;
  int lblNext;
  int lblBrk;
  int addrNext;
  int csr;

  VdbeModuleComment((v, "windowFullScan begin"));

  assert( pMWin!=0 );
  csr = pMWin->csrApp;
  nPeer = (pMWin->pOrderBy ? pMWin->pOrderBy->nExpr : 0);

  lblNext = sqlite3VdbeMakeLabel(pParse);
  lblBrk = sqlite3VdbeMakeLabel(pParse);

  regCRowid = sqlite3GetTempReg(pParse);
  regRowid = sqlite3GetTempReg(pParse);
  if( nPeer ){
    regCPeer = sqlite3GetTempRange(pParse, nPeer);
    regPeer = sqlite3GetTempRange(pParse, nPeer);
  }

  sqlite3VdbeAddOp2(v, OP_Rowid, pMWin->iEphCsr, regCRowid);
  windowReadPeerValues(p, pMWin->iEphCsr, regCPeer);

  for(pWin=pMWin; pWin; pWin=pWin->pNextWin){
    sqlite3VdbeAddOp2(v, OP_Null, 0, pWin->regAccum);
  }

  sqlite3VdbeAddOp3(v, OP_SeekGE, csr, lblBrk, pMWin->regStartRowid);
  VdbeCoverage(v);
  addrNext = sqlite3VdbeCurrentAddr(v);
  sqlite3VdbeAddOp2(v, OP_Rowid, csr, regRowid);
  sqlite3VdbeAddOp3(v, OP_Gt, pMWin->regEndRowid, lblBrk, regRowid);
  VdbeCoverageNeverNull(v);

  if( pMWin->eExclude==TK_CURRENT ){
    sqlite3VdbeAddOp3(v, OP_Eq, regCRowid, lblNext, regRowid);
    VdbeCoverageNeverNull(v);
  }else if( pMWin->eExclude!=TK_NO ){
    int addr;
    int addrEq = 0;
    KeyInfo *pKeyInfo = 0;

    if( pMWin->pOrderBy ){
      pKeyInfo = sqlite3KeyInfoFromExprList(pParse, pMWin->pOrderBy, 0, 0);
    }
    if( pMWin->eExclude==TK_TIES ){
      addrEq = sqlite3VdbeAddOp3(v, OP_Eq, regCRowid, 0, regRowid);
      VdbeCoverageNeverNull(v);
    }
    if( pKeyInfo ){
      windowReadPeerValues(p, csr, regPeer);
      sqlite3VdbeAddOp3(v, OP_Compare, regPeer, regCPeer, nPeer);
      sqlite3VdbeAppendP4(v, (void*)pKeyInfo, P4_KEYINFO);
      addr = sqlite3VdbeCurrentAddr(v)+1;
      sqlite3VdbeAddOp3(v, OP_Jump, addr, lblNext, addr);
      VdbeCoverageEqNe(v);
    }else{
      sqlite3VdbeAddOp2(v, OP_Goto, 0, lblNext);
    }
    if( addrEq ) sqlite3VdbeJumpHere(v, addrEq);
  }

  windowAggStep(p, pMWin, csr, 0, p->regArg);

  sqlite3VdbeResolveLabel(v, lblNext);
  sqlite3VdbeAddOp2(v, OP_Next, csr, addrNext);
  VdbeCoverage(v);
  sqlite3VdbeJumpHere(v, addrNext-1);
  sqlite3VdbeJumpHere(v, addrNext+1);
  sqlite3ReleaseTempReg(pParse, regRowid);
  sqlite3ReleaseTempReg(pParse, regCRowid);
  if( nPeer ){
    sqlite3ReleaseTempRange(pParse, regPeer, nPeer);
    sqlite3ReleaseTempRange(pParse, regCPeer, nPeer);
  }

  windowAggFinal(p, 1);
  VdbeModuleComment((v, "windowFullScan end"));
}
