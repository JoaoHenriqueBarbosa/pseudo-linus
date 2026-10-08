SQLITE_PRIVATE Trigger *sqlite3TriggersExist(
  Parse *pParse,          /* Parse context */
  Table *pTab,            /* The table the contains the triggers */
  int op,                 /* one of TK_DELETE, TK_INSERT, TK_UPDATE */
  ExprList *pChanges,     /* Columns that change in an UPDATE statement */
  int *pMask              /* OUT: Mask of TRIGGER_BEFORE|TRIGGER_AFTER */
){
  assert( pTab!=0 );
  if( (pTab->pTrigger==0 && !tempTriggersExist(pParse->db))
   || pParse->disableTriggers
  ){
    if( pMask ) *pMask = 0;
    return 0;
  }
  return triggersReallyExist(pParse,pTab,op,pChanges,pMask);
}

/*
** Convert the pStep->zTarget string into a SrcList and return a pointer
** to that SrcList.
**
** This routine adds a specific database name, if needed, to the target when
** forming the SrcList.  This prevents a trigger in one database from
** referring to a target in another database.  An exception is when the
** trigger is in TEMP in which case it can refer to any other database it
** wants.
*/
SQLITE_PRIVATE SrcList *sqlite3TriggerStepSrc(
  Parse *pParse,       /* The parsing context */
  TriggerStep *pStep   /* The trigger containing the target token */
){
  sqlite3 *db = pParse->db;
  SrcList *pSrc;                  /* SrcList to be returned */
  char *zName = sqlite3DbStrDup(db, pStep->zTarget);
  pSrc = sqlite3SrcListAppend(pParse, 0, 0, 0);
  assert( pSrc==0 || pSrc->nSrc==1 );
  assert( zName || pSrc==0 );
  if( pSrc ){
    Schema *pSchema = pStep->pTrig->pSchema;
    pSrc->a[0].zName = zName;
    if( pSchema!=db->aDb[1].pSchema ){
      pSrc->a[0].pSchema = pSchema;
    }
    if( pStep->pFrom ){
      SrcList *pDup = sqlite3SrcListDup(db, pStep->pFrom, 0);
      if( pDup && pDup->nSrc>1 && !IN_RENAME_OBJECT ){
        Select *pSubquery;
        Token as;
        pSubquery = sqlite3SelectNew(pParse,0,pDup,0,0,0,0,SF_NestedFrom,0);
        as.n = 0;
        as.z = 0;
        pDup = sqlite3SrcListAppendFromTerm(pParse,0,0,0,&as,pSubquery,0);
      }
      pSrc = sqlite3SrcListAppendList(pParse, pSrc, pDup);
    }
  }else{
    sqlite3DbFree(db, zName);
  }
  return pSrc;
}

/*
** Return true if the pExpr term from the RETURNING clause argument
** list is of the form "*".  Raise an error if the terms if of the
** form "table.*".
*/
static int isAsteriskTerm(
  Parse *pParse,      /* Parsing context */
  Expr *pTerm         /* A term in the RETURNING clause */
){
  assert( pTerm!=0 );
  if( pTerm->op==TK_ASTERISK ) return 1;
  if( pTerm->op!=TK_DOT ) return 0;
  assert( pTerm->pRight!=0 );
  assert( pTerm->pLeft!=0 );
  if( pTerm->pRight->op!=TK_ASTERISK ) return 0;
  sqlite3ErrorMsg(pParse, "RETURNING may not use \"TABLE.*\" wildcards");
  return 1;
}

/* The input list pList is the list of result set terms from a RETURNING
** clause.  The table that we are returning from is pTab.
**
** This routine makes a copy of the pList, and at the same time expands
** any "*" wildcards to be the complete set of columns from pTab.
*/
static ExprList *sqlite3ExpandReturning(
  Parse *pParse,        /* Parsing context */
  ExprList *pList,      /* The arguments to RETURNING */
  Table *pTab           /* The table being updated */
){
  ExprList *pNew = 0;
  sqlite3 *db = pParse->db;
  int i;

  for(i=0; i<pList->nExpr; i++){
    Expr *pOldExpr = pList->a[i].pExpr;
    if( NEVER(pOldExpr==0) ) continue;
    if( isAsteriskTerm(pParse, pOldExpr) ){
      int jj;
      for(jj=0; jj<pTab->nCol; jj++){
        Expr *pNewExpr;
        if( IsHiddenColumn(pTab->aCol+jj) ) continue;
        pNewExpr = sqlite3Expr(db, TK_ID, pTab->aCol[jj].zCnName);
        pNew = sqlite3ExprListAppend(pParse, pNew, pNewExpr);
        if( !db->mallocFailed ){
          struct ExprList_item *pItem = &pNew->a[pNew->nExpr-1];
          pItem->zEName = sqlite3DbStrDup(db, pTab->aCol[jj].zCnName);
          pItem->fg.eEName = ENAME_NAME;
        }
      }
    }else{
      Expr *pNewExpr = sqlite3ExprDup(db, pOldExpr, 0);
      pNew = sqlite3ExprListAppend(pParse, pNew, pNewExpr);
      if( !db->mallocFailed && ALWAYS(pList->a[i].zEName!=0) ){
        struct ExprList_item *pItem = &pNew->a[pNew->nExpr-1];
        pItem->zEName = sqlite3DbStrDup(db, pList->a[i].zEName);
        pItem->fg.eEName = pList->a[i].fg.eEName;
      }
    }
  }
  return pNew;
}

/* If the Expr node is a subquery or an EXISTS operator or an IN operator that
** uses a subquery, and if the subquery is SF_Correlated, then mark the
** expression as EP_VarSelect.
*/
static int sqlite3ReturningSubqueryVarSelect(Walker *NotUsed, Expr *pExpr){
  UNUSED_PARAMETER(NotUsed);
  if( ExprUseXSelect(pExpr)
   && (pExpr->x.pSelect->selFlags & SF_Correlated)!=0
  ){
    testcase( ExprHasProperty(pExpr, EP_VarSelect) );
    ExprSetProperty(pExpr, EP_VarSelect);
  }
  return WRC_Continue;
}


/*
** If the SELECT references the table pWalker->u.pTab, then do two things:
**
**    (1) Mark the SELECT as as SF_Correlated.
**    (2) Set pWalker->eCode to non-zero so that the caller will know
**        that (1) has happened.
*/
static int sqlite3ReturningSubqueryCorrelated(Walker *pWalker, Select *pSelect){
  int i;
  SrcList *pSrc;
  assert( pSelect!=0 );
  pSrc = pSelect->pSrc;
  assert( pSrc!=0 );
  for(i=0; i<pSrc->nSrc; i++){
    if( pSrc->a[i].pTab==pWalker->u.pTab ){
      testcase( pSelect->selFlags & SF_Correlated );
      pSelect->selFlags |= SF_Correlated;
      pWalker->eCode = 1;
      break;
    }
  }
  return WRC_Continue;
}

/*
** Scan the expression list that is the argument to RETURNING looking
** for subqueries that depend on the table which is being modified in the
** statement that is hosting the RETURNING clause (pTab).  Mark all such
** subqueries as SF_Correlated.  If the subqueries are part of an
** expression, mark the expression as EP_VarSelect.
**
** https://sqlite.org/forum/forumpost/2c83569ce8945d39
*/
static void sqlite3ProcessReturningSubqueries(
  ExprList *pEList,
  Table *pTab
){
  Walker w;
  memset(&w, 0, sizeof(w));
  w.xExprCallback = sqlite3ExprWalkNoop;
  w.xSelectCallback = sqlite3ReturningSubqueryCorrelated;
  w.u.pTab = pTab;
  sqlite3WalkExprList(&w, pEList);
  if( w.eCode ){
    w.xExprCallback = sqlite3ReturningSubqueryVarSelect;
    w.xSelectCallback = sqlite3SelectWalkNoop;
    sqlite3WalkExprList(&w, pEList);
  }
}

/*
** Generate code for the RETURNING trigger.  Unlike other triggers
** that invoke a subprogram in the bytecode, the code for RETURNING
** is generated in-line.
*/
static void codeReturningTrigger(
  Parse *pParse,       /* Parse context */
  Trigger *pTrigger,   /* The trigger step that defines the RETURNING */
  Table *pTab,         /* The table to code triggers from */
  int regIn            /* The first in an array of registers */
){
  Vdbe *v = pParse->pVdbe;
  sqlite3 *db = pParse->db;
  ExprList *pNew;
  Returning *pReturning;
  Select sSelect;
  SrcList sFrom;

  assert( v!=0 );
  if( !pParse->bReturning ){
    /* This RETURNING trigger must be for a different statement as
    ** this statement lacks a RETURNING clause. */
    return;
  }
  assert( db->pParse==pParse );
  pReturning = pParse->u1.pReturning;
  if( pTrigger != &(pReturning->retTrig) ){
    /* This RETURNING trigger is for a different statement */
    return;
  }
  memset(&sSelect, 0, sizeof(sSelect));
  memset(&sFrom, 0, sizeof(sFrom));
  sSelect.pEList = sqlite3ExprListDup(db, pReturning->pReturnEL, 0);
  sSelect.pSrc = &sFrom;
  sFrom.nSrc = 1;
  sFrom.a[0].pTab = pTab;
  sFrom.a[0].zName = pTab->zName; /* tag-20240424-1 */
  sFrom.a[0].iCursor = -1;
  sqlite3SelectPrep(pParse, &sSelect, 0);
  if( pParse->nErr==0 ){
    assert( db->mallocFailed==0 );
    sqlite3GenerateColumnNames(pParse, &sSelect);
  }
  sqlite3ExprListDelete(db, sSelect.pEList);
  pNew = sqlite3ExpandReturning(pParse, pReturning->pReturnEL, pTab);
  if( pParse->nErr==0 ){
    NameContext sNC;
    memset(&sNC, 0, sizeof(sNC));
    if( pReturning->nRetCol==0 ){
      pReturning->nRetCol = pNew->nExpr;
      pReturning->iRetCur = pParse->nTab++;
    }
    sNC.pParse = pParse;
    sNC.uNC.iBaseReg = regIn;
    sNC.ncFlags = NC_UBaseReg;
    pParse->eTriggerOp = pTrigger->op;
    pParse->pTriggerTab = pTab;
    if( sqlite3ResolveExprListNames(&sNC, pNew)==SQLITE_OK
     && ALWAYS(!db->mallocFailed)
    ){
      int i;
      int nCol = pNew->nExpr;
      int reg = pParse->nMem+1;
      sqlite3ProcessReturningSubqueries(pNew, pTab);
      pParse->nMem += nCol+2;
      pReturning->iRetReg = reg;
      for(i=0; i<nCol; i++){
        Expr *pCol = pNew->a[i].pExpr;
        assert( pCol!=0 ); /* Due to !db->mallocFailed ~9 lines above */
        sqlite3ExprCodeFactorable(pParse, pCol, reg+i);
        if( sqlite3ExprAffinity(pCol)==SQLITE_AFF_REAL ){
          sqlite3VdbeAddOp1(v, OP_RealAffinity, reg+i);
        }
      }
      sqlite3VdbeAddOp3(v, OP_MakeRecord, reg, i, reg+i);
      sqlite3VdbeAddOp2(v, OP_NewRowid, pReturning->iRetCur, reg+i+1);
      sqlite3VdbeAddOp3(v, OP_Insert, pReturning->iRetCur, reg+i, reg+i+1);
    }
  }
  sqlite3ExprListDelete(db, pNew);
  pParse->eTriggerOp = 0;
  pParse->pTriggerTab = 0;
}



/*
** Generate VDBE code for the statements inside the body of a single
** trigger.
*/
static int codeTriggerProgram(
  Parse *pParse,            /* The parser context */
  TriggerStep *pStepList,   /* List of statements inside the trigger body */
  int orconf                /* Conflict algorithm. (OE_Abort, etc) */
){
  TriggerStep *pStep;
  Vdbe *v = pParse->pVdbe;
  sqlite3 *db = pParse->db;

  assert( pParse->pTriggerTab && pParse->pToplevel );
  assert( pStepList );
  assert( v!=0 );
  for(pStep=pStepList; pStep; pStep=pStep->pNext){
    /* Figure out the ON CONFLICT policy that will be used for this step
    ** of the trigger program. If the statement that caused this trigger
    ** to fire had an explicit ON CONFLICT, then use it. Otherwise, use
    ** the ON CONFLICT policy that was specified as part of the trigger
    ** step statement. Example:
    **
    **   CREATE TRIGGER AFTER INSERT ON t1 BEGIN;
    **     INSERT OR REPLACE INTO t2 VALUES(new.a, new.b);
    **   END;
    **
    **   INSERT INTO t1 ... ;            -- insert into t2 uses REPLACE policy
    **   INSERT OR IGNORE INTO t1 ... ;  -- insert into t2 uses IGNORE policy
    */
    pParse->eOrconf = (orconf==OE_Default)?pStep->orconf:(u8)orconf;
    assert( pParse->okConstFactor==0 );

#ifndef SQLITE_OMIT_TRACE
    if( pStep->zSpan ){
      sqlite3VdbeAddOp4(v, OP_Trace, 0x7fffffff, 1, 0,
                        sqlite3MPrintf(db, "-- %s", pStep->zSpan),
                        P4_DYNAMIC);
    }
#endif

    switch( pStep->op ){
      case TK_UPDATE: {
        sqlite3Update(pParse,
          sqlite3TriggerStepSrc(pParse, pStep),
          sqlite3ExprListDup(db, pStep->pExprList, 0),
          sqlite3ExprDup(db, pStep->pWhere, 0),
          pParse->eOrconf, 0, 0, 0
        );
        sqlite3VdbeAddOp0(v, OP_ResetCount);
        break;
      }
      case TK_INSERT: {
        sqlite3Insert(pParse,
          sqlite3TriggerStepSrc(pParse, pStep),
          sqlite3SelectDup(db, pStep->pSelect, 0),
          sqlite3IdListDup(db, pStep->pIdList),
          pParse->eOrconf,
          sqlite3UpsertDup(db, pStep->pUpsert)
        );
        sqlite3VdbeAddOp0(v, OP_ResetCount);
        break;
      }
      case TK_DELETE: {
        sqlite3DeleteFrom(pParse,
          sqlite3TriggerStepSrc(pParse, pStep),
          sqlite3ExprDup(db, pStep->pWhere, 0), 0, 0
        );
        sqlite3VdbeAddOp0(v, OP_ResetCount);
        break;
      }
      default: assert( pStep->op==TK_SELECT ); {
        SelectDest sDest;
        Select *pSelect = sqlite3SelectDup(db, pStep->pSelect, 0);
        sqlite3SelectDestInit(&sDest, SRT_Discard, 0);
        sqlite3Select(pParse, pSelect, &sDest);
        sqlite3SelectDelete(db, pSelect);
        break;
      }
    }
  }

  return 0;
}
