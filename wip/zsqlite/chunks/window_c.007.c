**         Rewind(csrEnd) ; Rewind(csrStart) ; Rewind(csrCurrent)
**         regEnd = <expr2>
**         regStart = <expr1>
**       }else{
**         while( (csrEnd.key + regEnd) <= csrCurrent.key ){
**           AGGSTEP
**         }
**         while( (csrStart.key + regStart) < csrCurrent.key ){
**           AGGINVERSE
**         }
**         RETURN_ROW
**       }
**     }
**     flush:
**       while( (csrEnd.key + regEnd) <= csrCurrent.key ){
**         AGGSTEP
**       }
**       while( (csrStart.key + regStart) < csrCurrent.key ){
**         AGGINVERSE
**       }
**       RETURN_ROW
**
**   RANGE BETWEEN <expr1> FOLLOWING AND <expr2> FOLLOWING
**
**     ... loop started by sqlite3WhereBegin() ...
**       if( new partition ){
**         Gosub flush
**       }
**       Insert new row into eph table.
**       if( first row of partition ){
**         Rewind(csrEnd) ; Rewind(csrStart) ; Rewind(csrCurrent)
**         regEnd = <expr2>
**         regStart = <expr1>
**       }else{
**         AGGSTEP
**         while( (csrCurrent.key + regEnd) < csrEnd.key ){
**           while( (csrCurrent.key + regStart) > csrStart.key ){
**             AGGINVERSE
**           }
**           RETURN_ROW
**         }
**       }
**     }
**     flush:
**       AGGSTEP
**       while( 1 ){
**         while( (csrCurrent.key + regStart) > csrStart.key ){
**           AGGINVERSE
**           if( eof ) break "while( 1 )" loop.
**         }
**         RETURN_ROW
**       }
**       while( !eof csrCurrent ){
**         RETURN_ROW
**       }
**
** The text above leaves out many details. Refer to the code and comments
** below for a more complete picture.
*/
SQLITE_PRIVATE void sqlite3WindowCodeStep(
  Parse *pParse,                  /* Parse context */
  Select *p,                      /* Rewritten SELECT statement */
  WhereInfo *pWInfo,              /* Context returned by sqlite3WhereBegin() */
  int regGosub,                   /* Register for OP_Gosub */
  int addrGosub                   /* OP_Gosub here to return each row */
){
  Window *pMWin = p->pWin;
  ExprList *pOrderBy = pMWin->pOrderBy;
  Vdbe *v = sqlite3GetVdbe(pParse);
  int csrWrite;                   /* Cursor used to write to eph. table */
  int csrInput = p->pSrc->a[0].iCursor;     /* Cursor of sub-select */
  int nInput = p->pSrc->a[0].pTab->nCol;    /* Number of cols returned by sub */
  int iInput;                               /* To iterate through sub cols */
  int addrNe;                     /* Address of OP_Ne */
  int addrGosubFlush = 0;         /* Address of OP_Gosub to flush: */
  int addrInteger = 0;            /* Address of OP_Integer */
  int addrEmpty;                  /* Address of OP_Rewind in flush: */
  int regNew;                     /* Array of registers holding new input row */
  int regRecord;                  /* regNew array in record form */
  int regNewPeer = 0;             /* Peer values for new row (part of regNew) */
  int regPeer = 0;                /* Peer values for current row */
  int regFlushPart = 0;           /* Register for "Gosub flush_partition" */
  WindowCodeArg s;                /* Context object for sub-routines */
  int lblWhereEnd;                /* Label just before sqlite3WhereEnd() code */
  int regStart = 0;               /* Value of <expr> PRECEDING */
  int regEnd = 0;                 /* Value of <expr> FOLLOWING */

  assert( pMWin->eStart==TK_PRECEDING || pMWin->eStart==TK_CURRENT
       || pMWin->eStart==TK_FOLLOWING || pMWin->eStart==TK_UNBOUNDED
  );
  assert( pMWin->eEnd==TK_FOLLOWING || pMWin->eEnd==TK_CURRENT
       || pMWin->eEnd==TK_UNBOUNDED || pMWin->eEnd==TK_PRECEDING
  );
  assert( pMWin->eExclude==0 || pMWin->eExclude==TK_CURRENT
       || pMWin->eExclude==TK_GROUP || pMWin->eExclude==TK_TIES
       || pMWin->eExclude==TK_NO
  );

  lblWhereEnd = sqlite3VdbeMakeLabel(pParse);

  /* Fill in the context object */
  memset(&s, 0, sizeof(WindowCodeArg));
  s.pParse = pParse;
  s.pMWin = pMWin;
  s.pVdbe = v;
  s.regGosub = regGosub;
  s.addrGosub = addrGosub;
  s.current.csr = pMWin->iEphCsr;
  csrWrite = s.current.csr+1;
  s.start.csr = s.current.csr+2;
  s.end.csr = s.current.csr+3;

  /* Figure out when rows may be deleted from the ephemeral table. There
  ** are four options - they may never be deleted (eDelete==0), they may
  ** be deleted as soon as they are no longer part of the window frame
  ** (eDelete==WINDOW_AGGINVERSE), they may be deleted as after the row
  ** has been returned to the caller (WINDOW_RETURN_ROW), or they may
  ** be deleted after they enter the frame (WINDOW_AGGSTEP). */
  switch( pMWin->eStart ){
    case TK_FOLLOWING:
      if( pMWin->eFrmType!=TK_RANGE
       && windowExprGtZero(pParse, pMWin->pStart)
      ){
        s.eDelete = WINDOW_RETURN_ROW;
      }
      break;
    case TK_UNBOUNDED:
      if( windowCacheFrame(pMWin)==0 ){
        if( pMWin->eEnd==TK_PRECEDING ){
          if( pMWin->eFrmType!=TK_RANGE
           && windowExprGtZero(pParse, pMWin->pEnd)
          ){
            s.eDelete = WINDOW_AGGSTEP;
          }
        }else{
          s.eDelete = WINDOW_RETURN_ROW;
        }
      }
      break;
    default:
      s.eDelete = WINDOW_AGGINVERSE;
      break;
  }

  /* Allocate registers for the array of values from the sub-query, the
  ** same values in record form, and the rowid used to insert said record
  ** into the ephemeral table.  */
  regNew = pParse->nMem+1;
  pParse->nMem += nInput;
  regRecord = ++pParse->nMem;
  s.regRowid = ++pParse->nMem;

  /* If the window frame contains an "<expr> PRECEDING" or "<expr> FOLLOWING"
  ** clause, allocate registers to store the results of evaluating each
  ** <expr>.  */
  if( pMWin->eStart==TK_PRECEDING || pMWin->eStart==TK_FOLLOWING ){
    regStart = ++pParse->nMem;
  }
  if( pMWin->eEnd==TK_PRECEDING || pMWin->eEnd==TK_FOLLOWING ){
    regEnd = ++pParse->nMem;
  }

  /* If this is not a "ROWS BETWEEN ..." frame, then allocate arrays of
  ** registers to store copies of the ORDER BY expressions (peer values)
  ** for the main loop, and for each cursor (start, current and end). */
  if( pMWin->eFrmType!=TK_ROWS ){
    int nPeer = (pOrderBy ? pOrderBy->nExpr : 0);
    regNewPeer = regNew + pMWin->nBufferCol;
    if( pMWin->pPartition ) regNewPeer += pMWin->pPartition->nExpr;
    regPeer = pParse->nMem+1;       pParse->nMem += nPeer;
    s.start.reg = pParse->nMem+1;   pParse->nMem += nPeer;
    s.current.reg = pParse->nMem+1; pParse->nMem += nPeer;
    s.end.reg = pParse->nMem+1;     pParse->nMem += nPeer;
  }

  /* Load the column values for the row returned by the sub-select
  ** into an array of registers starting at regNew. Assemble them into
  ** a record in register regRecord. */
  for(iInput=0; iInput<nInput; iInput++){
    sqlite3VdbeAddOp3(v, OP_Column, csrInput, iInput, regNew+iInput);
  }
  sqlite3VdbeAddOp3(v, OP_MakeRecord, regNew, nInput, regRecord);

  /* An input row has just been read into an array of registers starting
  ** at regNew. If the window has a PARTITION clause, this block generates
  ** VM code to check if the input row is the start of a new partition.
  ** If so, it does an OP_Gosub to an address to be filled in later. The
  ** address of the OP_Gosub is stored in local variable addrGosubFlush. */
  if( pMWin->pPartition ){
    int addr;
    ExprList *pPart = pMWin->pPartition;
    int nPart = pPart->nExpr;
    int regNewPart = regNew + pMWin->nBufferCol;
    KeyInfo *pKeyInfo = sqlite3KeyInfoFromExprList(pParse, pPart, 0, 0);

    regFlushPart = ++pParse->nMem;
    addr = sqlite3VdbeAddOp3(v, OP_Compare, regNewPart, pMWin->regPart, nPart);
    sqlite3VdbeAppendP4(v, (void*)pKeyInfo, P4_KEYINFO);
    sqlite3VdbeAddOp3(v, OP_Jump, addr+2, addr+4, addr+2);
    VdbeCoverageEqNe(v);
    addrGosubFlush = sqlite3VdbeAddOp1(v, OP_Gosub, regFlushPart);
    VdbeComment((v, "call flush_partition"));
    sqlite3VdbeAddOp3(v, OP_Copy, regNewPart, pMWin->regPart, nPart-1);
  }

  /* Insert the new row into the ephemeral table */
  sqlite3VdbeAddOp2(v, OP_NewRowid, csrWrite, s.regRowid);
  sqlite3VdbeAddOp3(v, OP_Insert, csrWrite, regRecord, s.regRowid);
  addrNe = sqlite3VdbeAddOp3(v, OP_Ne, pMWin->regOne, 0, s.regRowid);
  VdbeCoverageNeverNull(v);

  /* This block is run for the first row of each partition */
  s.regArg = windowInitAccum(pParse, pMWin);

  if( regStart ){
    sqlite3ExprCode(pParse, pMWin->pStart, regStart);
    windowCheckValue(pParse, regStart, 0 + (pMWin->eFrmType==TK_RANGE?3:0));
  }
  if( regEnd ){
    sqlite3ExprCode(pParse, pMWin->pEnd, regEnd);
    windowCheckValue(pParse, regEnd, 1 + (pMWin->eFrmType==TK_RANGE?3:0));
  }

  if( pMWin->eFrmType!=TK_RANGE && pMWin->eStart==pMWin->eEnd && regStart ){
    int op = ((pMWin->eStart==TK_FOLLOWING) ? OP_Ge : OP_Le);
    int addrGe = sqlite3VdbeAddOp3(v, op, regStart, 0, regEnd);
    VdbeCoverageNeverNullIf(v, op==OP_Ge); /* NeverNull because bound <expr> */
    VdbeCoverageNeverNullIf(v, op==OP_Le); /*   values previously checked */
    windowAggFinal(&s, 0);
    sqlite3VdbeAddOp1(v, OP_Rewind, s.current.csr);
    windowReturnOneRow(&s);
    sqlite3VdbeAddOp1(v, OP_ResetSorter, s.current.csr);
    sqlite3VdbeAddOp2(v, OP_Goto, 0, lblWhereEnd);
    sqlite3VdbeJumpHere(v, addrGe);
  }
  if( pMWin->eStart==TK_FOLLOWING && pMWin->eFrmType!=TK_RANGE && regEnd ){
    assert( pMWin->eEnd==TK_FOLLOWING );
    sqlite3VdbeAddOp3(v, OP_Subtract, regStart, regEnd, regStart);
  }

  if( pMWin->eStart!=TK_UNBOUNDED ){
    sqlite3VdbeAddOp1(v, OP_Rewind, s.start.csr);
  }
  sqlite3VdbeAddOp1(v, OP_Rewind, s.current.csr);
  sqlite3VdbeAddOp1(v, OP_Rewind, s.end.csr);
  if( regPeer && pOrderBy ){
    sqlite3VdbeAddOp3(v, OP_Copy, regNewPeer, regPeer, pOrderBy->nExpr-1);
    sqlite3VdbeAddOp3(v, OP_Copy, regPeer, s.start.reg, pOrderBy->nExpr-1);
    sqlite3VdbeAddOp3(v, OP_Copy, regPeer, s.current.reg, pOrderBy->nExpr-1);
    sqlite3VdbeAddOp3(v, OP_Copy, regPeer, s.end.reg, pOrderBy->nExpr-1);
  }

  sqlite3VdbeAddOp2(v, OP_Goto, 0, lblWhereEnd);

  sqlite3VdbeJumpHere(v, addrNe);

  /* Beginning of the block executed for the second and subsequent rows. */
  if( regPeer ){
    windowIfNewPeer(pParse, pOrderBy, regNewPeer, regPeer, lblWhereEnd);
  }
  if( pMWin->eStart==TK_FOLLOWING ){
    windowCodeOp(&s, WINDOW_AGGSTEP, 0, 0);
    if( pMWin->eEnd!=TK_UNBOUNDED ){
      if( pMWin->eFrmType==TK_RANGE ){
        int lbl = sqlite3VdbeMakeLabel(pParse);
        int addrNext = sqlite3VdbeCurrentAddr(v);
        windowCodeRangeTest(&s, OP_Ge, s.current.csr, regEnd, s.end.csr, lbl);
        windowCodeOp(&s, WINDOW_AGGINVERSE, regStart, 0);
        windowCodeOp(&s, WINDOW_RETURN_ROW, 0, 0);
        sqlite3VdbeAddOp2(v, OP_Goto, 0, addrNext);
        sqlite3VdbeResolveLabel(v, lbl);
      }else{
        windowCodeOp(&s, WINDOW_RETURN_ROW, regEnd, 0);
        windowCodeOp(&s, WINDOW_AGGINVERSE, regStart, 0);
      }
    }
  }else
  if( pMWin->eEnd==TK_PRECEDING ){
    int bRPS = (pMWin->eStart==TK_PRECEDING && pMWin->eFrmType==TK_RANGE);
    windowCodeOp(&s, WINDOW_AGGSTEP, regEnd, 0);
    if( bRPS ) windowCodeOp(&s, WINDOW_AGGINVERSE, regStart, 0);
    windowCodeOp(&s, WINDOW_RETURN_ROW, 0, 0);
    if( !bRPS ) windowCodeOp(&s, WINDOW_AGGINVERSE, regStart, 0);
  }else{
    int addr = 0;
    windowCodeOp(&s, WINDOW_AGGSTEP, 0, 0);
    if( pMWin->eEnd!=TK_UNBOUNDED ){
      if( pMWin->eFrmType==TK_RANGE ){
        int lbl = 0;
        addr = sqlite3VdbeCurrentAddr(v);
        if( regEnd ){
          lbl = sqlite3VdbeMakeLabel(pParse);
          windowCodeRangeTest(&s, OP_Ge, s.current.csr, regEnd, s.end.csr, lbl);
        }
        windowCodeOp(&s, WINDOW_RETURN_ROW, 0, 0);
        windowCodeOp(&s, WINDOW_AGGINVERSE, regStart, 0);
        if( regEnd ){
          sqlite3VdbeAddOp2(v, OP_Goto, 0, addr);
          sqlite3VdbeResolveLabel(v, lbl);
        }
      }else{
        if( regEnd ){
          addr = sqlite3VdbeAddOp3(v, OP_IfPos, regEnd, 0, 1);
          VdbeCoverage(v);
        }
        windowCodeOp(&s, WINDOW_RETURN_ROW, 0, 0);
        windowCodeOp(&s, WINDOW_AGGINVERSE, regStart, 0);
        if( regEnd ) sqlite3VdbeJumpHere(v, addr);
      }
    }
  }

  /* End of the main input loop */
  sqlite3VdbeResolveLabel(v, lblWhereEnd);
  sqlite3WhereEnd(pWInfo);

  /* Fall through */
  if( pMWin->pPartition ){
    addrInteger = sqlite3VdbeAddOp2(v, OP_Integer, 0, regFlushPart);
    sqlite3VdbeJumpHere(v, addrGosubFlush);
  }

  s.regRowid = 0;
  addrEmpty = sqlite3VdbeAddOp1(v, OP_Rewind, csrWrite);
  VdbeCoverage(v);
  if( pMWin->eEnd==TK_PRECEDING ){
    int bRPS = (pMWin->eStart==TK_PRECEDING && pMWin->eFrmType==TK_RANGE);
    windowCodeOp(&s, WINDOW_AGGSTEP, regEnd, 0);
    if( bRPS ) windowCodeOp(&s, WINDOW_AGGINVERSE, regStart, 0);
    windowCodeOp(&s, WINDOW_RETURN_ROW, 0, 0);
  }else if( pMWin->eStart==TK_FOLLOWING ){
    int addrStart;
    int addrBreak1;
    int addrBreak2;
    int addrBreak3;
    windowCodeOp(&s, WINDOW_AGGSTEP, 0, 0);
    if( pMWin->eFrmType==TK_RANGE ){
      addrStart = sqlite3VdbeCurrentAddr(v);
      addrBreak2 = windowCodeOp(&s, WINDOW_AGGINVERSE, regStart, 1);
      addrBreak1 = windowCodeOp(&s, WINDOW_RETURN_ROW, 0, 1);
    }else
    if( pMWin->eEnd==TK_UNBOUNDED ){
      addrStart = sqlite3VdbeCurrentAddr(v);
      addrBreak1 = windowCodeOp(&s, WINDOW_RETURN_ROW, regStart, 1);
      addrBreak2 = windowCodeOp(&s, WINDOW_AGGINVERSE, 0, 1);
    }else{
      assert( pMWin->eEnd==TK_FOLLOWING );
      addrStart = sqlite3VdbeCurrentAddr(v);
      addrBreak1 = windowCodeOp(&s, WINDOW_RETURN_ROW, regEnd, 1);
      addrBreak2 = windowCodeOp(&s, WINDOW_AGGINVERSE, regStart, 1);
    }
    sqlite3VdbeAddOp2(v, OP_Goto, 0, addrStart);
    sqlite3VdbeJumpHere(v, addrBreak2);
    addrStart = sqlite3VdbeCurrentAddr(v);
    addrBreak3 = windowCodeOp(&s, WINDOW_RETURN_ROW, 0, 1);
    sqlite3VdbeAddOp2(v, OP_Goto, 0, addrStart);
    sqlite3VdbeJumpHere(v, addrBreak1);
    sqlite3VdbeJumpHere(v, addrBreak3);
  }else{
    int addrBreak;
    int addrStart;
    windowCodeOp(&s, WINDOW_AGGSTEP, 0, 0);
    addrStart = sqlite3VdbeCurrentAddr(v);
    addrBreak = windowCodeOp(&s, WINDOW_RETURN_ROW, 0, 1);
    windowCodeOp(&s, WINDOW_AGGINVERSE, regStart, 0);
    sqlite3VdbeAddOp2(v, OP_Goto, 0, addrStart);
    sqlite3VdbeJumpHere(v, addrBreak);
  }
  sqlite3VdbeJumpHere(v, addrEmpty);

  sqlite3VdbeAddOp1(v, OP_ResetSorter, s.current.csr);
  if( pMWin->pPartition ){
    if( pMWin->regStartRowid ){
      sqlite3VdbeAddOp2(v, OP_Integer, 1, pMWin->regStartRowid);
      sqlite3VdbeAddOp2(v, OP_Integer, 0, pMWin->regEndRowid);
    }
    sqlite3VdbeChangeP1(v, addrInteger, sqlite3VdbeCurrentAddr(v));
    sqlite3VdbeAddOp1(v, OP_Return, regFlushPart);
  }
}
