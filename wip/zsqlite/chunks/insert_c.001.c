#endif /* SQLITE_OMIT_GENERATED_COLUMNS */


#ifndef SQLITE_OMIT_AUTOINCREMENT
/*
** Locate or create an AutoincInfo structure associated with table pTab
** which is in database iDb.  Return the register number for the register
** that holds the maximum rowid.  Return zero if pTab is not an AUTOINCREMENT
** table.  (Also return zero when doing a VACUUM since we do not want to
** update the AUTOINCREMENT counters during a VACUUM.)
**
** There is at most one AutoincInfo structure per table even if the
** same table is autoincremented multiple times due to inserts within
** triggers.  A new AutoincInfo structure is created if this is the
** first use of table pTab.  On 2nd and subsequent uses, the original
** AutoincInfo structure is used.
**
** Four consecutive registers are allocated:
**
**   (1)  The name of the pTab table.
**   (2)  The maximum ROWID of pTab.
**   (3)  The rowid in sqlite_sequence of pTab
**   (4)  The original value of the max ROWID in pTab, or NULL if none
**
** The 2nd register is the one that is returned.  That is all the
** insert routine needs to know about.
*/
static int autoIncBegin(
  Parse *pParse,      /* Parsing context */
  int iDb,            /* Index of the database holding pTab */
  Table *pTab         /* The table we are writing to */
){
  int memId = 0;      /* Register holding maximum rowid */
  assert( pParse->db->aDb[iDb].pSchema!=0 );
  if( (pTab->tabFlags & TF_Autoincrement)!=0
   && (pParse->db->mDbFlags & DBFLAG_Vacuum)==0
  ){
    Parse *pToplevel = sqlite3ParseToplevel(pParse);
    AutoincInfo *pInfo;
    Table *pSeqTab = pParse->db->aDb[iDb].pSchema->pSeqTab;

    /* Verify that the sqlite_sequence table exists and is an ordinary
    ** rowid table with exactly two columns.
    ** Ticket d8dc2b3a58cd5dc2918a1d4acb 2018-05-23 */
    if( pSeqTab==0
     || !HasRowid(pSeqTab)
     || NEVER(IsVirtual(pSeqTab))
     || pSeqTab->nCol!=2
    ){
      pParse->nErr++;
      pParse->rc = SQLITE_CORRUPT_SEQUENCE;
      return 0;
    }

    pInfo = pToplevel->pAinc;
    while( pInfo && pInfo->pTab!=pTab ){ pInfo = pInfo->pNext; }
    if( pInfo==0 ){
      pInfo = sqlite3DbMallocRawNN(pParse->db, sizeof(*pInfo));
      sqlite3ParserAddCleanup(pToplevel, sqlite3DbFree, pInfo);
      testcase( pParse->earlyCleanup );
      if( pParse->db->mallocFailed ) return 0;
      pInfo->pNext = pToplevel->pAinc;
      pToplevel->pAinc = pInfo;
      pInfo->pTab = pTab;
      pInfo->iDb = iDb;
      pToplevel->nMem++;                  /* Register to hold name of table */
      pInfo->regCtr = ++pToplevel->nMem;  /* Max rowid register */
      pToplevel->nMem +=2;       /* Rowid in sqlite_sequence + orig max val */
    }
    memId = pInfo->regCtr;
  }
  return memId;
}

/*
** This routine generates code that will initialize all of the
** register used by the autoincrement tracker.
*/
SQLITE_PRIVATE void sqlite3AutoincrementBegin(Parse *pParse){
  AutoincInfo *p;            /* Information about an AUTOINCREMENT */
  sqlite3 *db = pParse->db;  /* The database connection */
  Db *pDb;                   /* Database only autoinc table */
  int memId;                 /* Register holding max rowid */
  Vdbe *v = pParse->pVdbe;   /* VDBE under construction */

  /* This routine is never called during trigger-generation.  It is
  ** only called from the top-level */
  assert( pParse->pTriggerTab==0 );
  assert( sqlite3IsToplevel(pParse) );

  assert( v );   /* We failed long ago if this is not so */
  for(p = pParse->pAinc; p; p = p->pNext){
    static const int iLn = VDBE_OFFSET_LINENO(2);
    static const VdbeOpList autoInc[] = {
      /* 0  */ {OP_Null,    0,  0, 0},
      /* 1  */ {OP_Rewind,  0, 10, 0},
      /* 2  */ {OP_Column,  0,  0, 0},
      /* 3  */ {OP_Ne,      0,  9, 0},
      /* 4  */ {OP_Rowid,   0,  0, 0},
      /* 5  */ {OP_Column,  0,  1, 0},
      /* 6  */ {OP_AddImm,  0,  0, 0},
      /* 7  */ {OP_Copy,    0,  0, 0},
      /* 8  */ {OP_Goto,    0, 11, 0},
      /* 9  */ {OP_Next,    0,  2, 0},
      /* 10 */ {OP_Integer, 0,  0, 0},
      /* 11 */ {OP_Close,   0,  0, 0}
    };
    VdbeOp *aOp;
    pDb = &db->aDb[p->iDb];
    memId = p->regCtr;
    assert( sqlite3SchemaMutexHeld(db, 0, pDb->pSchema) );
    sqlite3OpenTable(pParse, 0, p->iDb, pDb->pSchema->pSeqTab, OP_OpenRead);
    sqlite3VdbeLoadString(v, memId-1, p->pTab->zName);
    aOp = sqlite3VdbeAddOpList(v, ArraySize(autoInc), autoInc, iLn);
    if( aOp==0 ) break;
    aOp[0].p2 = memId;
    aOp[0].p3 = memId+2;
    aOp[2].p3 = memId;
    aOp[3].p1 = memId-1;
    aOp[3].p3 = memId;
    aOp[3].p5 = SQLITE_JUMPIFNULL;
    aOp[4].p2 = memId+1;
    aOp[5].p3 = memId;
    aOp[6].p1 = memId;
    aOp[7].p2 = memId+2;
    aOp[7].p1 = memId;
    aOp[10].p2 = memId;
    if( pParse->nTab==0 ) pParse->nTab = 1;
  }
}

/*
** Update the maximum rowid for an autoincrement calculation.
**
** This routine should be called when the regRowid register holds a
** new rowid that is about to be inserted.  If that new rowid is
** larger than the maximum rowid in the memId memory cell, then the
** memory cell is updated.
*/
static void autoIncStep(Parse *pParse, int memId, int regRowid){
  if( memId>0 ){
    sqlite3VdbeAddOp2(pParse->pVdbe, OP_MemMax, memId, regRowid);
  }
}

/*
** This routine generates the code needed to write autoincrement
** maximum rowid values back into the sqlite_sequence register.
** Every statement that might do an INSERT into an autoincrement
** table (either directly or through triggers) needs to call this
** routine just before the "exit" code.
*/
static SQLITE_NOINLINE void autoIncrementEnd(Parse *pParse){
  AutoincInfo *p;
  Vdbe *v = pParse->pVdbe;
  sqlite3 *db = pParse->db;

  assert( v );
  for(p = pParse->pAinc; p; p = p->pNext){
    static const int iLn = VDBE_OFFSET_LINENO(2);
    static const VdbeOpList autoIncEnd[] = {
      /* 0 */ {OP_NotNull,     0, 2, 0},
      /* 1 */ {OP_NewRowid,    0, 0, 0},
      /* 2 */ {OP_MakeRecord,  0, 2, 0},
      /* 3 */ {OP_Insert,      0, 0, 0},
      /* 4 */ {OP_Close,       0, 0, 0}
    };
    VdbeOp *aOp;
    Db *pDb = &db->aDb[p->iDb];
    int iRec;
    int memId = p->regCtr;

    iRec = sqlite3GetTempReg(pParse);
    assert( sqlite3SchemaMutexHeld(db, 0, pDb->pSchema) );
    sqlite3VdbeAddOp3(v, OP_Le, memId+2, sqlite3VdbeCurrentAddr(v)+7, memId);
    VdbeCoverage(v);
    sqlite3OpenTable(pParse, 0, p->iDb, pDb->pSchema->pSeqTab, OP_OpenWrite);
    aOp = sqlite3VdbeAddOpList(v, ArraySize(autoIncEnd), autoIncEnd, iLn);
    if( aOp==0 ) break;
    aOp[0].p1 = memId+1;
    aOp[1].p2 = memId+1;
    aOp[2].p1 = memId-1;
    aOp[2].p3 = iRec;
    aOp[3].p2 = iRec;
    aOp[3].p3 = memId+1;
    aOp[3].p5 = OPFLAG_APPEND;
    sqlite3ReleaseTempReg(pParse, iRec);
  }
}
SQLITE_PRIVATE void sqlite3AutoincrementEnd(Parse *pParse){
  if( pParse->pAinc ) autoIncrementEnd(pParse);
}
#else
/*
** If SQLITE_OMIT_AUTOINCREMENT is defined, then the three routines
** above are all no-ops
*/
# define autoIncBegin(A,B,C) (0)
# define autoIncStep(A,B,C)
#endif /* SQLITE_OMIT_AUTOINCREMENT */

/*
** If argument pVal is a Select object returned by an sqlite3MultiValues()
** that was able to use the co-routine optimization, finish coding the
** co-routine.
*/
SQLITE_PRIVATE void sqlite3MultiValuesEnd(Parse *pParse, Select *pVal){
  if( ALWAYS(pVal) && pVal->pSrc->nSrc>0 ){
    SrcItem *pItem = &pVal->pSrc->a[0];
    sqlite3VdbeEndCoroutine(pParse->pVdbe, pItem->regReturn);
    sqlite3VdbeJumpHere(pParse->pVdbe, pItem->addrFillSub - 1);
  }
}

/*
** Return true if all expressions in the expression-list passed as the
** only argument are constant.
*/
static int exprListIsConstant(Parse *pParse, ExprList *pRow){
  int ii;
  for(ii=0; ii<pRow->nExpr; ii++){
    if( 0==sqlite3ExprIsConstant(pParse, pRow->a[ii].pExpr) ) return 0;
  }
  return 1;
}

/*
** Return true if all expressions in the expression-list passed as the
** only argument are both constant and have no affinity.
*/
static int exprListIsNoAffinity(Parse *pParse, ExprList *pRow){
  int ii;
  if( exprListIsConstant(pParse,pRow)==0 ) return 0;
  for(ii=0; ii<pRow->nExpr; ii++){
    Expr *pExpr = pRow->a[ii].pExpr;
    assert( pExpr->op!=TK_RAISE );
    assert( pExpr->affExpr==0 );
    if( 0!=sqlite3ExprAffinity(pExpr) ) return 0;
  }
  return 1;

}

/*
** This function is called by the parser for the second and subsequent
** rows of a multi-row VALUES clause. Argument pLeft is the part of
** the VALUES clause already parsed, argument pRow is the vector of values
** for the new row. The Select object returned represents the complete
** VALUES clause, including the new row.
**
** There are two ways in which this may be achieved - by incremental
** coding of a co-routine (the "co-routine" method) or by returning a
** Select object equivalent to the following (the "UNION ALL" method):
**
**        "pLeft UNION ALL SELECT pRow"
**
** If the VALUES clause contains a lot of rows, this compound Select
** object may consume a lot of memory.
**
** When the co-routine method is used, each row that will be returned
** by the VALUES clause is coded into part of a co-routine as it is
** passed to this function. The returned Select object is equivalent to:
**
**     SELECT * FROM (
**       Select object to read co-routine
**     )
**
** The co-routine method is used in most cases. Exceptions are:
**
**    a) If the current statement has a WITH clause. This is to avoid
**       statements like:
**
**            WITH cte AS ( VALUES('x'), ('y') ... )
**            SELECT * FROM cte AS a, cte AS b;
**
**       This will not work, as the co-routine uses a hard-coded register
**       for its OP_Yield instructions, and so it is not possible for two
**       cursors to iterate through it concurrently.
**
**    b) The schema is currently being parsed (i.e. the VALUES clause is part
**       of a schema item like a VIEW or TRIGGER). In this case there is no VM
**       being generated when parsing is taking place, and so generating
**       a co-routine is not possible.
**
**    c) There are non-constant expressions in the VALUES clause (e.g.
**       the VALUES clause is part of a correlated sub-query).
**
**    d) One or more of the values in the first row of the VALUES clause
**       has an affinity (i.e. is a CAST expression). This causes problems
**       because the complex rules SQLite uses (see function
**       sqlite3SubqueryColumnTypes() in select.c) to determine the effective
**       affinity of such a column for all rows require access to all values in
**       the column simultaneously.
*/
SQLITE_PRIVATE Select *sqlite3MultiValues(Parse *pParse, Select *pLeft, ExprList *pRow){

  if( pParse->bHasWith                   /* condition (a) above */
   || pParse->db->init.busy              /* condition (b) above */
   || exprListIsConstant(pParse,pRow)==0 /* condition (c) above */
   || (pLeft->pSrc->nSrc==0 &&
       exprListIsNoAffinity(pParse,pLeft->pEList)==0) /* condition (d) above */
   || IN_SPECIAL_PARSE
  ){
    /* The co-routine method cannot be used. Fall back to UNION ALL. */
    Select *pSelect = 0;
    int f = SF_Values | SF_MultiValue;
    if( pLeft->pSrc->nSrc ){
      sqlite3MultiValuesEnd(pParse, pLeft);
      f = SF_Values;
    }else if( pLeft->pPrior ){
      /* In this case set the SF_MultiValue flag only if it was set on pLeft */
      f = (f & pLeft->selFlags);
    }
    pSelect = sqlite3SelectNew(pParse, pRow, 0, 0, 0, 0, 0, f, 0);
    pLeft->selFlags &= ~SF_MultiValue;
    if( pSelect ){
      pSelect->op = TK_ALL;
      pSelect->pPrior = pLeft;
      pLeft = pSelect;
    }
  }else{
    SrcItem *p = 0;               /* SrcItem that reads from co-routine */

    if( pLeft->pSrc->nSrc==0 ){
      /* Co-routine has not yet been started and the special Select object
      ** that accesses the co-routine has not yet been created. This block
      ** does both those things. */
      Vdbe *v = sqlite3GetVdbe(pParse);
      Select *pRet = sqlite3SelectNew(pParse, 0, 0, 0, 0, 0, 0, 0, 0);

      /* Ensure the database schema has been read. This is to ensure we have
      ** the correct text encoding.  */
      if( (pParse->db->mDbFlags & DBFLAG_SchemaKnownOk)==0 ){
        sqlite3ReadSchema(pParse);
      }

      if( pRet ){
        SelectDest dest;
        pRet->pSrc->nSrc = 1;
        pRet->pPrior = pLeft->pPrior;
        pRet->op = pLeft->op;
        if( pRet->pPrior ) pRet->selFlags |= SF_Values;
        pLeft->pPrior = 0;
        pLeft->op = TK_SELECT;
        assert( pLeft->pNext==0 );
        assert( pRet->pNext==0 );
        p = &pRet->pSrc->a[0];
        p->pSelect = pLeft;
        p->fg.viaCoroutine = 1;
        p->addrFillSub = sqlite3VdbeCurrentAddr(v) + 1;
        p->regReturn = ++pParse->nMem;
        p->iCursor = -1;
        p->u1.nRow = 2;
        sqlite3VdbeAddOp3(v,OP_InitCoroutine,p->regReturn,0,p->addrFillSub);
        sqlite3SelectDestInit(&dest, SRT_Coroutine, p->regReturn);

        /* Allocate registers for the output of the co-routine. Do so so
        ** that there are two unused registers immediately before those
        ** used by the co-routine. This allows the code in sqlite3Insert()
        ** to use these registers directly, instead of copying the output
        ** of the co-routine to a separate array for processing.  */
        dest.iSdst = pParse->nMem + 3;
        dest.nSdst = pLeft->pEList->nExpr;
        pParse->nMem += 2 + dest.nSdst;

        pLeft->selFlags |= SF_MultiValue;
        sqlite3Select(pParse, pLeft, &dest);
        p->regResult = dest.iSdst;
        assert( pParse->nErr || dest.iSdst>0 );
        pLeft = pRet;
      }
    }else{
      p = &pLeft->pSrc->a[0];
      assert( !p->fg.isTabFunc && !p->fg.isIndexedBy );
      p->u1.nRow++;
    }

    if( pParse->nErr==0 ){
      assert( p!=0 );
      if( p->pSelect->pEList->nExpr!=pRow->nExpr ){
        sqlite3SelectWrongNumTermsError(pParse, p->pSelect);
      }else{
        sqlite3ExprCodeExprList(pParse, pRow, p->regResult, 0, 0);
        sqlite3VdbeAddOp1(pParse->pVdbe, OP_Yield, p->regReturn);
      }
    }
    sqlite3ExprListDelete(pParse->db, pRow);
  }

  return pLeft;
}
