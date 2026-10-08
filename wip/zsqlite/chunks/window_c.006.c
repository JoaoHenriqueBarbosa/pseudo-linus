

/*
** Allocate and return a duplicate of the Window object indicated by the
** third argument. Set the Window.pOwner field of the new object to
** pOwner.
*/
SQLITE_PRIVATE Window *sqlite3WindowDup(sqlite3 *db, Expr *pOwner, Window *p){
  Window *pNew = 0;
  if( ALWAYS(p) ){
    pNew = sqlite3DbMallocZero(db, sizeof(Window));
    if( pNew ){
      pNew->zName = sqlite3DbStrDup(db, p->zName);
      pNew->zBase = sqlite3DbStrDup(db, p->zBase);
      pNew->pFilter = sqlite3ExprDup(db, p->pFilter, 0);
      pNew->pWFunc = p->pWFunc;
      pNew->pPartition = sqlite3ExprListDup(db, p->pPartition, 0);
      pNew->pOrderBy = sqlite3ExprListDup(db, p->pOrderBy, 0);
      pNew->eFrmType = p->eFrmType;
      pNew->eEnd = p->eEnd;
      pNew->eStart = p->eStart;
      pNew->eExclude = p->eExclude;
      pNew->regResult = p->regResult;
      pNew->regAccum = p->regAccum;
      pNew->iArgCol = p->iArgCol;
      pNew->iEphCsr = p->iEphCsr;
      pNew->bExprArgs = p->bExprArgs;
      pNew->pStart = sqlite3ExprDup(db, p->pStart, 0);
      pNew->pEnd = sqlite3ExprDup(db, p->pEnd, 0);
      pNew->pOwner = pOwner;
      pNew->bImplicitFrame = p->bImplicitFrame;
    }
  }
  return pNew;
}

/*
** Return a copy of the linked list of Window objects passed as the
** second argument.
*/
SQLITE_PRIVATE Window *sqlite3WindowListDup(sqlite3 *db, Window *p){
  Window *pWin;
  Window *pRet = 0;
  Window **pp = &pRet;

  for(pWin=p; pWin; pWin=pWin->pNextWin){
    *pp = sqlite3WindowDup(db, 0, pWin);
    if( *pp==0 ) break;
    pp = &((*pp)->pNextWin);
  }

  return pRet;
}

/*
** Return true if it can be determined at compile time that expression
** pExpr evaluates to a value that, when cast to an integer, is greater
** than zero. False otherwise.
**
** If an OOM error occurs, this function sets the Parse.db.mallocFailed
** flag and returns zero.
*/
static int windowExprGtZero(Parse *pParse, Expr *pExpr){
  int ret = 0;
  sqlite3 *db = pParse->db;
  sqlite3_value *pVal = 0;
  sqlite3ValueFromExpr(db, pExpr, db->enc, SQLITE_AFF_NUMERIC, &pVal);
  if( pVal && sqlite3_value_int(pVal)>0 ){
    ret = 1;
  }
  sqlite3ValueFree(pVal);
  return ret;
}

/*
** sqlite3WhereBegin() has already been called for the SELECT statement
** passed as the second argument when this function is invoked. It generates
** code to populate the Window.regResult register for each window function
** and invoke the sub-routine at instruction addrGosub once for each row.
** sqlite3WhereEnd() is always called before returning.
**
** This function handles several different types of window frames, which
** require slightly different processing. The following pseudo code is
** used to implement window frames of the form:
**
**   ROWS BETWEEN <expr1> PRECEDING AND <expr2> FOLLOWING
**
** Other window frame types use variants of the following:
**
**     ... loop started by sqlite3WhereBegin() ...
**       if( new partition ){
**         Gosub flush
**       }
**       Insert new row into eph table.
**
**       if( first row of partition ){
**         // Rewind three cursors, all open on the eph table.
**         Rewind(csrEnd);
**         Rewind(csrStart);
**         Rewind(csrCurrent);
**
**         regEnd = <expr2>          // FOLLOWING expression
**         regStart = <expr1>        // PRECEDING expression
**       }else{
**         // First time this branch is taken, the eph table contains two
**         // rows. The first row in the partition, which all three cursors
**         // currently point to, and the following row.
**         AGGSTEP
**         if( (regEnd--)<=0 ){
**           RETURN_ROW
**           if( (regStart--)<=0 ){
**             AGGINVERSE
**           }
**         }
**       }
**     }
**     flush:
**       AGGSTEP
**       while( 1 ){
**         RETURN ROW
**         if( csrCurrent is EOF ) break;
**         if( (regStart--)<=0 ){
**           AggInverse(csrStart)
**           Next(csrStart)
**         }
**       }
**
** The pseudo-code above uses the following shorthand:
**
**   AGGSTEP:    invoke the aggregate xStep() function for each window function
**               with arguments read from the current row of cursor csrEnd, then
**               step cursor csrEnd forward one row (i.e. sqlite3BtreeNext()).
**
**   RETURN_ROW: return a row to the caller based on the contents of the
**               current row of csrCurrent and the current state of all
**               aggregates. Then step cursor csrCurrent forward one row.
**
**   AGGINVERSE: invoke the aggregate xInverse() function for each window
**               functions with arguments read from the current row of cursor
**               csrStart. Then step csrStart forward one row.
**
** There are two other ROWS window frames that are handled significantly
** differently from the above - "BETWEEN <expr> PRECEDING AND <expr> PRECEDING"
** and "BETWEEN <expr> FOLLOWING AND <expr> FOLLOWING". These are special
** cases because they change the order in which the three cursors (csrStart,
** csrCurrent and csrEnd) iterate through the ephemeral table. Cases that
** use UNBOUNDED or CURRENT ROW are much simpler variations on one of these
** three.
**
**   ROWS BETWEEN <expr1> PRECEDING AND <expr2> PRECEDING
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
**         if( (regEnd--)<=0 ){
**           AGGSTEP
**         }
**         RETURN_ROW
**         if( (regStart--)<=0 ){
**           AGGINVERSE
**         }
**       }
**     }
**     flush:
**       if( (regEnd--)<=0 ){
**         AGGSTEP
**       }
**       RETURN_ROW
**
**
**   ROWS BETWEEN <expr1> FOLLOWING AND <expr2> FOLLOWING
**
**     ... loop started by sqlite3WhereBegin() ...
**     if( new partition ){
**       Gosub flush
**     }
**     Insert new row into eph table.
**     if( first row of partition ){
**       Rewind(csrEnd) ; Rewind(csrStart) ; Rewind(csrCurrent)
**       regEnd = <expr2>
**       regStart = regEnd - <expr1>
**     }else{
**       AGGSTEP
**       if( (regEnd--)<=0 ){
**         RETURN_ROW
**       }
**       if( (regStart--)<=0 ){
**         AGGINVERSE
**       }
**     }
**   }
**   flush:
**     AGGSTEP
**     while( 1 ){
**       if( (regEnd--)<=0 ){
**         RETURN_ROW
**         if( eof ) break;
**       }
**       if( (regStart--)<=0 ){
**         AGGINVERSE
**         if( eof ) break
**       }
**     }
**     while( !eof csrCurrent ){
**       RETURN_ROW
**     }
**
** For the most part, the patterns above are adapted to support UNBOUNDED by
** assuming that it is equivalent to "infinity PRECEDING/FOLLOWING" and
** CURRENT ROW by assuming that it is equivalent to "0 PRECEDING/FOLLOWING".
** This is optimized of course - branches that will never be taken and
** conditions that are always true are omitted from the VM code. The only
** exceptional case is:
**
**   ROWS BETWEEN <expr1> FOLLOWING AND UNBOUNDED FOLLOWING
**
**     ... loop started by sqlite3WhereBegin() ...
**     if( new partition ){
**       Gosub flush
**     }
**     Insert new row into eph table.
**     if( first row of partition ){
**       Rewind(csrEnd) ; Rewind(csrStart) ; Rewind(csrCurrent)
**       regStart = <expr1>
**     }else{
**       AGGSTEP
**     }
**   }
**   flush:
**     AGGSTEP
**     while( 1 ){
**       if( (regStart--)<=0 ){
**         AGGINVERSE
**         if( eof ) break
**       }
**       RETURN_ROW
**     }
**     while( !eof csrCurrent ){
**       RETURN_ROW
**     }
**
** Also requiring special handling are the cases:
**
**   ROWS BETWEEN <expr1> PRECEDING AND <expr2> PRECEDING
**   ROWS BETWEEN <expr1> FOLLOWING AND <expr2> FOLLOWING
**
** when (expr1 < expr2). This is detected at runtime, not by this function.
** To handle this case, the pseudo-code programs depicted above are modified
** slightly to be:
**
**     ... loop started by sqlite3WhereBegin() ...
**     if( new partition ){
**       Gosub flush
**     }
**     Insert new row into eph table.
**     if( first row of partition ){
**       Rewind(csrEnd) ; Rewind(csrStart) ; Rewind(csrCurrent)
**       regEnd = <expr2>
**       regStart = <expr1>
**       if( regEnd < regStart ){
**         RETURN_ROW
**         delete eph table contents
**         continue
**       }
**     ...
**
** The new "continue" statement in the above jumps to the next iteration
** of the outer loop - the one started by sqlite3WhereBegin().
**
** The various GROUPS cases are implemented using the same patterns as
** ROWS. The VM code is modified slightly so that:
**
**   1. The else branch in the main loop is only taken if the row just
**      added to the ephemeral table is the start of a new group. In
**      other words, it becomes:
**
**         ... loop started by sqlite3WhereBegin() ...
**         if( new partition ){
**           Gosub flush
**         }
**         Insert new row into eph table.
**         if( first row of partition ){
**           Rewind(csrEnd) ; Rewind(csrStart) ; Rewind(csrCurrent)
**           regEnd = <expr2>
**           regStart = <expr1>
**         }else if( new group ){
**           ...
**         }
**       }
**
**   2. Instead of processing a single row, each RETURN_ROW, AGGSTEP or
**      AGGINVERSE step processes the current row of the relevant cursor and
**      all subsequent rows belonging to the same group.
**
** RANGE window frames are a little different again. As for GROUPS, the
** main loop runs once per group only. And RETURN_ROW, AGGSTEP and AGGINVERSE
** deal in groups instead of rows. As for ROWS and GROUPS, there are three
** basic cases:
**
**   RANGE BETWEEN <expr1> PRECEDING AND <expr2> FOLLOWING
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
**           RETURN_ROW
**           while( csrStart.key + regStart) < csrCurrent.key ){
**             AGGINVERSE
**           }
**         }
**       }
**     }
**     flush:
**       AGGSTEP
**       while( 1 ){
**         RETURN ROW
**         if( csrCurrent is EOF ) break;
**           while( csrStart.key + regStart) < csrCurrent.key ){
**             AGGINVERSE
**           }
**         }
**       }
**
** In the above notation, "csr.key" means the current value of the ORDER BY
** expression (there is only ever 1 for a RANGE that uses an <expr> FOLLOWING
** or <expr PRECEDING) read from cursor csr.
**
**   RANGE BETWEEN <expr1> PRECEDING AND <expr2> PRECEDING
**
**     ... loop started by sqlite3WhereBegin() ...
**       if( new partition ){
**         Gosub flush
**       }
**       Insert new row into eph table.
**       if( first row of partition ){
