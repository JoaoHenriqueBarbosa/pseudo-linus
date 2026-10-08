/*
** 2001 September 15
**
** The author disclaims copyright to this source code.  In place of
** a legal notice, here is a blessing:
**
**    May you do good and not evil.
**    May you find forgiveness for yourself and forgive others.
**    May you share freely, never taking more than you give.
**
*************************************************************************
** This file contains C code routines that are called by the parser
** to handle INSERT statements in SQLite.
*/
/* #include "sqliteInt.h" */

/*
** Generate code that will
**
**   (1) acquire a lock for table pTab then
**   (2) open pTab as cursor iCur.
**
** If pTab is a WITHOUT ROWID table, then it is the PRIMARY KEY index
** for that table that is actually opened.
*/
SQLITE_PRIVATE void sqlite3OpenTable(
  Parse *pParse,  /* Generate code into this VDBE */
  int iCur,       /* The cursor number of the table */
  int iDb,        /* The database index in sqlite3.aDb[] */
  Table *pTab,    /* The table to be opened */
  int opcode      /* OP_OpenRead or OP_OpenWrite */
){
  Vdbe *v;
  assert( !IsVirtual(pTab) );
  assert( pParse->pVdbe!=0 );
  v = pParse->pVdbe;
  assert( opcode==OP_OpenWrite || opcode==OP_OpenRead );
  if( !pParse->db->noSharedCache ){
    sqlite3TableLock(pParse, iDb, pTab->tnum,
                     (opcode==OP_OpenWrite)?1:0, pTab->zName);
  }
  if( HasRowid(pTab) ){
    sqlite3VdbeAddOp4Int(v, opcode, iCur, pTab->tnum, iDb, pTab->nNVCol);
    VdbeComment((v, "%s", pTab->zName));
  }else{
    Index *pPk = sqlite3PrimaryKeyIndex(pTab);
    assert( pPk!=0 );
    assert( pPk->tnum==pTab->tnum || CORRUPT_DB );
    sqlite3VdbeAddOp3(v, opcode, iCur, pPk->tnum, iDb);
    sqlite3VdbeSetP4KeyInfo(pParse, pPk);
    VdbeComment((v, "%s", pTab->zName));
  }
}

/*
** Return a pointer to the column affinity string associated with index
** pIdx. A column affinity string has one character for each column in
** the table, according to the affinity of the column:
**
**  Character      Column affinity
**  ------------------------------
**  'A'            BLOB
**  'B'            TEXT
**  'C'            NUMERIC
**  'D'            INTEGER
**  'F'            REAL
**
** An extra 'D' is appended to the end of the string to cover the
** rowid that appears as the last column in every index.
**
** Memory for the buffer containing the column index affinity string
** is managed along with the rest of the Index structure. It will be
** released when sqlite3DeleteIndex() is called.
*/
static SQLITE_NOINLINE const char *computeIndexAffStr(sqlite3 *db, Index *pIdx){
  /* The first time a column affinity string for a particular index is
  ** required, it is allocated and populated here. It is then stored as
  ** a member of the Index structure for subsequent use.
  **
  ** The column affinity string will eventually be deleted by
  ** sqliteDeleteIndex() when the Index structure itself is cleaned
  ** up.
  */
  int n;
  Table *pTab = pIdx->pTable;
  pIdx->zColAff = (char *)sqlite3DbMallocRaw(0, pIdx->nColumn+1);
  if( !pIdx->zColAff ){
    sqlite3OomFault(db);
    return 0;
  }
  for(n=0; n<pIdx->nColumn; n++){
    i16 x = pIdx->aiColumn[n];
    char aff;
    if( x>=0 ){
      aff = pTab->aCol[x].affinity;
    }else if( x==XN_ROWID ){
      aff = SQLITE_AFF_INTEGER;
    }else{
      assert( x==XN_EXPR );
      assert( pIdx->bHasExpr );
      assert( pIdx->aColExpr!=0 );
      aff = sqlite3ExprAffinity(pIdx->aColExpr->a[n].pExpr);
    }
    if( aff<SQLITE_AFF_BLOB ) aff = SQLITE_AFF_BLOB;
    if( aff>SQLITE_AFF_NUMERIC) aff = SQLITE_AFF_NUMERIC;
    pIdx->zColAff[n] = aff;
  }
  pIdx->zColAff[n] = 0;
  return pIdx->zColAff;
}
SQLITE_PRIVATE const char *sqlite3IndexAffinityStr(sqlite3 *db, Index *pIdx){
  if( !pIdx->zColAff ) return computeIndexAffStr(db, pIdx);
  return pIdx->zColAff;
}


/*
** Compute an affinity string for a table.   Space is obtained
** from sqlite3DbMalloc().  The caller is responsible for freeing
** the space when done.
*/
SQLITE_PRIVATE char *sqlite3TableAffinityStr(sqlite3 *db, const Table *pTab){
  char *zColAff;
  zColAff = (char *)sqlite3DbMallocRaw(db, pTab->nCol+1);
  if( zColAff ){
    int i, j;
    for(i=j=0; i<pTab->nCol; i++){
      if( (pTab->aCol[i].colFlags & COLFLAG_VIRTUAL)==0 ){
        zColAff[j++] = pTab->aCol[i].affinity;
      }
    }
    do{
      zColAff[j--] = 0;
    }while( j>=0 && zColAff[j]<=SQLITE_AFF_BLOB );
  }
  return zColAff;
}

/*
** Make changes to the evolving bytecode to do affinity transformations
** of values that are about to be gathered into a row for table pTab.
**
** For ordinary (legacy, non-strict) tables:
** -----------------------------------------
**
** Compute the affinity string for table pTab, if it has not already been
** computed.  As an optimization, omit trailing SQLITE_AFF_BLOB affinities.
**
** If the affinity string is empty (because it was all SQLITE_AFF_BLOB entries
** which were then optimized out) then this routine becomes a no-op.
**
** Otherwise if iReg>0 then code an OP_Affinity opcode that will set the
** affinities for register iReg and following.  Or if iReg==0,
** then just set the P4 operand of the previous opcode (which should  be
** an OP_MakeRecord) to the affinity string.
**
** A column affinity string has one character per column:
**
**    Character      Column affinity
**    ---------      ---------------
**    'A'            BLOB
**    'B'            TEXT
**    'C'            NUMERIC
**    'D'            INTEGER
**    'E'            REAL
**
** For STRICT tables:
** ------------------
**
** Generate an appropriate OP_TypeCheck opcode that will verify the
** datatypes against the column definitions in pTab.  If iReg==0, that
** means an OP_MakeRecord opcode has already been generated and should be
** the last opcode generated.  The new OP_TypeCheck needs to be inserted
** before the OP_MakeRecord.  The new OP_TypeCheck should use the same
** register set as the OP_MakeRecord.  If iReg>0 then register iReg is
** the first of a series of registers that will form the new record.
** Apply the type checking to that array of registers.
*/
SQLITE_PRIVATE void sqlite3TableAffinity(Vdbe *v, Table *pTab, int iReg){
  int i;
  char *zColAff;
  if( pTab->tabFlags & TF_Strict ){
    if( iReg==0 ){
      /* Move the previous opcode (which should be OP_MakeRecord) forward
      ** by one slot and insert a new OP_TypeCheck where the current
      ** OP_MakeRecord is found */
      VdbeOp *pPrev;
      sqlite3VdbeAppendP4(v, pTab, P4_TABLE);
      pPrev = sqlite3VdbeGetLastOp(v);
      assert( pPrev!=0 );
      assert( pPrev->opcode==OP_MakeRecord || sqlite3VdbeDb(v)->mallocFailed );
      pPrev->opcode = OP_TypeCheck;
      sqlite3VdbeAddOp3(v, OP_MakeRecord, pPrev->p1, pPrev->p2, pPrev->p3);
    }else{
      /* Insert an isolated OP_Typecheck */
      sqlite3VdbeAddOp2(v, OP_TypeCheck, iReg, pTab->nNVCol);
      sqlite3VdbeAppendP4(v, pTab, P4_TABLE);
    }
    return;
  }
  zColAff = pTab->zColAff;
  if( zColAff==0 ){
    zColAff = sqlite3TableAffinityStr(0, pTab);
    if( !zColAff ){
      sqlite3OomFault(sqlite3VdbeDb(v));
      return;
    }
    pTab->zColAff = zColAff;
  }
  assert( zColAff!=0 );
  i = sqlite3Strlen30NN(zColAff);
  if( i ){
    if( iReg ){
      sqlite3VdbeAddOp4(v, OP_Affinity, iReg, i, 0, zColAff, i);
    }else{
      assert( sqlite3VdbeGetLastOp(v)->opcode==OP_MakeRecord
              || sqlite3VdbeDb(v)->mallocFailed );
      sqlite3VdbeChangeP4(v, -1, zColAff, i);
    }
  }
}

/*
** Return non-zero if the table pTab in database iDb or any of its indices
** have been opened at any point in the VDBE program. This is used to see if
** a statement of the form  "INSERT INTO <iDb, pTab> SELECT ..." can
** run without using a temporary table for the results of the SELECT.
*/
static int readsTable(Parse *p, int iDb, Table *pTab){
  Vdbe *v = sqlite3GetVdbe(p);
  int i;
  int iEnd = sqlite3VdbeCurrentAddr(v);
#ifndef SQLITE_OMIT_VIRTUALTABLE
  VTable *pVTab = IsVirtual(pTab) ? sqlite3GetVTable(p->db, pTab) : 0;
#endif

  for(i=1; i<iEnd; i++){
    VdbeOp *pOp = sqlite3VdbeGetOp(v, i);
    assert( pOp!=0 );
    if( pOp->opcode==OP_OpenRead && pOp->p3==iDb ){
      Index *pIndex;
      Pgno tnum = pOp->p2;
      if( tnum==pTab->tnum ){
        return 1;
      }
      for(pIndex=pTab->pIndex; pIndex; pIndex=pIndex->pNext){
        if( tnum==pIndex->tnum ){
          return 1;
        }
      }
    }
#ifndef SQLITE_OMIT_VIRTUALTABLE
    if( pOp->opcode==OP_VOpen && pOp->p4.pVtab==pVTab ){
      assert( pOp->p4.pVtab!=0 );
      assert( pOp->p4type==P4_VTAB );
      return 1;
    }
#endif
  }
  return 0;
}

/* This walker callback will compute the union of colFlags flags for all
** referenced columns in a CHECK constraint or generated column expression.
*/
static int exprColumnFlagUnion(Walker *pWalker, Expr *pExpr){
  if( pExpr->op==TK_COLUMN && pExpr->iColumn>=0 ){
    assert( pExpr->iColumn < pWalker->u.pTab->nCol );
    pWalker->eCode |= pWalker->u.pTab->aCol[pExpr->iColumn].colFlags;
  }
  return WRC_Continue;
}

#ifndef SQLITE_OMIT_GENERATED_COLUMNS
/*
** All regular columns for table pTab have been puts into registers
** starting with iRegStore.  The registers that correspond to STORED
** or VIRTUAL columns have not yet been initialized.  This routine goes
** back and computes the values for those columns based on the previously
** computed normal columns.
*/
SQLITE_PRIVATE void sqlite3ComputeGeneratedColumns(
  Parse *pParse,    /* Parsing context */
  int iRegStore,    /* Register holding the first column */
  Table *pTab       /* The table */
){
  int i;
  Walker w;
  Column *pRedo;
  int eProgress;
  VdbeOp *pOp;

  assert( pTab->tabFlags & TF_HasGenerated );
  testcase( pTab->tabFlags & TF_HasVirtual );
  testcase( pTab->tabFlags & TF_HasStored );

  /* Before computing generated columns, first go through and make sure
  ** that appropriate affinity has been applied to the regular columns
  */
  sqlite3TableAffinity(pParse->pVdbe, pTab, iRegStore);
  if( (pTab->tabFlags & TF_HasStored)!=0 ){
    pOp = sqlite3VdbeGetLastOp(pParse->pVdbe);
    if( pOp->opcode==OP_Affinity ){
      /* Change the OP_Affinity argument to '@' (NONE) for all stored
      ** columns.  '@' is the no-op affinity and those columns have not
      ** yet been computed. */
      int ii, jj;
      char *zP4 = pOp->p4.z;
      assert( zP4!=0 );
      assert( pOp->p4type==P4_DYNAMIC );
      for(ii=jj=0; zP4[jj]; ii++){
        if( pTab->aCol[ii].colFlags & COLFLAG_VIRTUAL ){
          continue;
        }
        if( pTab->aCol[ii].colFlags & COLFLAG_STORED ){
          zP4[jj] = SQLITE_AFF_NONE;
        }
        jj++;
      }
    }else if( pOp->opcode==OP_TypeCheck ){
      /* If an OP_TypeCheck was generated because the table is STRICT,
      ** then set the P3 operand to indicate that generated columns should
      ** not be checked */
      pOp->p3 = 1;
    }
  }

  /* Because there can be multiple generated columns that refer to one another,
  ** this is a two-pass algorithm.  On the first pass, mark all generated
  ** columns as "not available".
  */
  for(i=0; i<pTab->nCol; i++){
    if( pTab->aCol[i].colFlags & COLFLAG_GENERATED ){
      testcase( pTab->aCol[i].colFlags & COLFLAG_VIRTUAL );
      testcase( pTab->aCol[i].colFlags & COLFLAG_STORED );
      pTab->aCol[i].colFlags |= COLFLAG_NOTAVAIL;
    }
  }

  w.u.pTab = pTab;
  w.xExprCallback = exprColumnFlagUnion;
  w.xSelectCallback = 0;
  w.xSelectCallback2 = 0;

  /* On the second pass, compute the value of each NOT-AVAILABLE column.
  ** Companion code in the TK_COLUMN case of sqlite3ExprCodeTarget() will
  ** compute dependencies and mark remove the COLSPAN_NOTAVAIL mark, as
  ** they are needed.
  */
  pParse->iSelfTab = -iRegStore;
  do{
    eProgress = 0;
    pRedo = 0;
    for(i=0; i<pTab->nCol; i++){
      Column *pCol = pTab->aCol + i;
      if( (pCol->colFlags & COLFLAG_NOTAVAIL)!=0 ){
        int x;
        pCol->colFlags |= COLFLAG_BUSY;
        w.eCode = 0;
        sqlite3WalkExpr(&w, sqlite3ColumnExpr(pTab, pCol));
        pCol->colFlags &= ~COLFLAG_BUSY;
        if( w.eCode & COLFLAG_NOTAVAIL ){
          pRedo = pCol;
          continue;
        }
        eProgress = 1;
        assert( pCol->colFlags & COLFLAG_GENERATED );
        x = sqlite3TableColumnToStorage(pTab, i) + iRegStore;
        sqlite3ExprCodeGeneratedColumn(pParse, pTab, pCol, x);
        pCol->colFlags &= ~COLFLAG_NOTAVAIL;
      }
    }
  }while( pRedo && eProgress );
  if( pRedo ){
    sqlite3ErrorMsg(pParse, "generated column loop on \"%s\"", pRedo->zCnName);
  }
  pParse->iSelfTab = 0;
}
