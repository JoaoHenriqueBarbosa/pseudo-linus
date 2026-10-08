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
** to handle UPDATE statements.
*/
/* #include "sqliteInt.h" */

#ifndef SQLITE_OMIT_VIRTUALTABLE
/* Forward declaration */
static void updateVirtualTable(
  Parse *pParse,       /* The parsing context */
  SrcList *pSrc,       /* The virtual table to be modified */
  Table *pTab,         /* The virtual table */
  ExprList *pChanges,  /* The columns to change in the UPDATE statement */
  Expr *pRowidExpr,    /* Expression used to recompute the rowid */
  int *aXRef,          /* Mapping from columns of pTab to entries in pChanges */
  Expr *pWhere,        /* WHERE clause of the UPDATE statement */
  int onError          /* ON CONFLICT strategy */
);
#endif /* SQLITE_OMIT_VIRTUALTABLE */

/*
** The most recently coded instruction was an OP_Column to retrieve the
** i-th column of table pTab. This routine sets the P4 parameter of the
** OP_Column to the default value, if any.
**
** The default value of a column is specified by a DEFAULT clause in the
** column definition. This was either supplied by the user when the table
** was created, or added later to the table definition by an ALTER TABLE
** command. If the latter, then the row-records in the table btree on disk
** may not contain a value for the column and the default value, taken
** from the P4 parameter of the OP_Column instruction, is returned instead.
** If the former, then all row-records are guaranteed to include a value
** for the column and the P4 value is not required.
**
** Column definitions created by an ALTER TABLE command may only have
** literal default values specified: a number, null or a string. (If a more
** complicated default expression value was provided, it is evaluated
** when the ALTER TABLE is executed and one of the literal values written
** into the sqlite_schema table.)
**
** Therefore, the P4 parameter is only required if the default value for
** the column is a literal number, string or null. The sqlite3ValueFromExpr()
** function is capable of transforming these types of expressions into
** sqlite3_value objects.
**
** If column as REAL affinity and the table is an ordinary b-tree table
** (not a virtual table) then the value might have been stored as an
** integer.  In that case, add an OP_RealAffinity opcode to make sure
** it has been converted into REAL.
*/
SQLITE_PRIVATE void sqlite3ColumnDefault(Vdbe *v, Table *pTab, int i, int iReg){
  Column *pCol;
  assert( pTab!=0 );
  assert( pTab->nCol>i );
  pCol = &pTab->aCol[i];
  if( pCol->iDflt ){
    sqlite3_value *pValue = 0;
    u8 enc = ENC(sqlite3VdbeDb(v));
    assert( !IsView(pTab) );
    VdbeComment((v, "%s.%s", pTab->zName, pCol->zCnName));
    assert( i<pTab->nCol );
    sqlite3ValueFromExpr(sqlite3VdbeDb(v),
                         sqlite3ColumnExpr(pTab,pCol), enc,
                         pCol->affinity, &pValue);
    if( pValue ){
      sqlite3VdbeAppendP4(v, pValue, P4_MEM);
    }
  }
#ifndef SQLITE_OMIT_FLOATING_POINT
  if( pCol->affinity==SQLITE_AFF_REAL && !IsVirtual(pTab) ){
    sqlite3VdbeAddOp1(v, OP_RealAffinity, iReg);
  }
#endif
}

/*
** Check to see if column iCol of index pIdx references any of the
** columns defined by aXRef and chngRowid.  Return true if it does
** and false if not.  This is an optimization.  False-positives are a
** performance degradation, but false-negatives can result in a corrupt
** index and incorrect answers.
**
** aXRef[j] will be non-negative if column j of the original table is
** being updated.  chngRowid will be true if the rowid of the table is
** being updated.
*/
static int indexColumnIsBeingUpdated(
  Index *pIdx,      /* The index to check */
  int iCol,         /* Which column of the index to check */
  int *aXRef,       /* aXRef[j]>=0 if column j is being updated */
  int chngRowid     /* true if the rowid is being updated */
){
  i16 iIdxCol = pIdx->aiColumn[iCol];
  assert( iIdxCol!=XN_ROWID ); /* Cannot index rowid */
  if( iIdxCol>=0 ){
    return aXRef[iIdxCol]>=0;
  }
  assert( iIdxCol==XN_EXPR );
  assert( pIdx->aColExpr!=0 );
  assert( pIdx->aColExpr->a[iCol].pExpr!=0 );
  return sqlite3ExprReferencesUpdatedColumn(pIdx->aColExpr->a[iCol].pExpr,
                                            aXRef,chngRowid);
}

/*
** Check to see if index pIdx is a partial index whose conditional
** expression might change values due to an UPDATE.  Return true if
** the index is subject to change and false if the index is guaranteed
** to be unchanged.  This is an optimization.  False-positives are a
** performance degradation, but false-negatives can result in a corrupt
** index and incorrect answers.
**
** aXRef[j] will be non-negative if column j of the original table is
** being updated.  chngRowid will be true if the rowid of the table is
** being updated.
*/
static int indexWhereClauseMightChange(
  Index *pIdx,      /* The index to check */
  int *aXRef,       /* aXRef[j]>=0 if column j is being updated */
  int chngRowid     /* true if the rowid is being updated */
){
  if( pIdx->pPartIdxWhere==0 ) return 0;
  return sqlite3ExprReferencesUpdatedColumn(pIdx->pPartIdxWhere,
                                            aXRef, chngRowid);
}

/*
** Allocate and return a pointer to an expression of type TK_ROW with
** Expr.iColumn set to value (iCol+1). The resolver will modify the
** expression to be a TK_COLUMN reading column iCol of the first
** table in the source-list (pSrc->a[0]).
*/
static Expr *exprRowColumn(Parse *pParse, int iCol){
  Expr *pRet = sqlite3PExpr(pParse, TK_ROW, 0, 0);
  if( pRet ) pRet->iColumn = iCol+1;
  return pRet;
}

/*
** Assuming both the pLimit and pOrderBy parameters are NULL, this function
** generates VM code to run the query:
**
**   SELECT <other-columns>, pChanges FROM pTabList WHERE pWhere
**
** and write the results to the ephemeral table already opened as cursor
** iEph. None of pChanges, pTabList or pWhere are modified or consumed by
** this function, they must be deleted by the caller.
**
** Or, if pLimit and pOrderBy are not NULL, and pTab is not a view:
**
**   SELECT <other-columns>, pChanges FROM pTabList
**   WHERE pWhere
**   GROUP BY <other-columns>
**   ORDER BY pOrderBy LIMIT pLimit
**
** If pTab is a view, the GROUP BY clause is omitted.
**
** Exactly how results are written to table iEph, and exactly what
** the <other-columns> in the query above are is determined by the type
** of table pTabList->a[0].pTab.
**
** If the table is a WITHOUT ROWID table, then argument pPk must be its
** PRIMARY KEY. In this case <other-columns> are the primary key columns
** of the table, in order. The results of the query are written to ephemeral
** table iEph as index keys, using OP_IdxInsert.
**
** If the table is actually a view, then <other-columns> are all columns of
** the view. The results are written to the ephemeral table iEph as records
** with automatically assigned integer keys.
**
** If the table is a virtual or ordinary intkey table, then <other-columns>
** is its rowid. For a virtual table, the results are written to iEph as
** records with automatically assigned integer keys For intkey tables, the
** rowid value in <other-columns> is used as the integer key, and the
** remaining fields make up the table record.
*/
static void updateFromSelect(
  Parse *pParse,                  /* Parse context */
  int iEph,                       /* Cursor for open eph. table */
  Index *pPk,                     /* PK if table 0 is WITHOUT ROWID */
  ExprList *pChanges,             /* List of expressions to return */
  SrcList *pTabList,              /* List of tables to select from */
  Expr *pWhere,                   /* WHERE clause for query */
  ExprList *pOrderBy,             /* ORDER BY clause */
  Expr *pLimit                    /* LIMIT clause */
){
  int i;
  SelectDest dest;
  Select *pSelect = 0;
  ExprList *pList = 0;
  ExprList *pGrp = 0;
  Expr *pLimit2 = 0;
  ExprList *pOrderBy2 = 0;
  sqlite3 *db = pParse->db;
  Table *pTab = pTabList->a[0].pTab;
  SrcList *pSrc;
  Expr *pWhere2;
  int eDest;

#ifdef SQLITE_ENABLE_UPDATE_DELETE_LIMIT
  if( pOrderBy && pLimit==0 ) {
    sqlite3ErrorMsg(pParse, "ORDER BY without LIMIT on UPDATE");
    return;
  }
  pOrderBy2 = sqlite3ExprListDup(db, pOrderBy, 0);
  pLimit2 = sqlite3ExprDup(db, pLimit, 0);
#else
  UNUSED_PARAMETER(pOrderBy);
  UNUSED_PARAMETER(pLimit);
#endif

  pSrc = sqlite3SrcListDup(db, pTabList, 0);
  pWhere2 = sqlite3ExprDup(db, pWhere, 0);

  assert( pTabList->nSrc>1 );
  if( pSrc ){
    assert( pSrc->a[0].fg.notCte );
    pSrc->a[0].iCursor = -1;
    pSrc->a[0].pTab->nTabRef--;
    pSrc->a[0].pTab = 0;
  }
  if( pPk ){
    for(i=0; i<pPk->nKeyCol; i++){
      Expr *pNew = exprRowColumn(pParse, pPk->aiColumn[i]);
#ifdef SQLITE_ENABLE_UPDATE_DELETE_LIMIT
      if( pLimit ){
        pGrp = sqlite3ExprListAppend(pParse, pGrp, sqlite3ExprDup(db, pNew, 0));
      }
#endif
      pList = sqlite3ExprListAppend(pParse, pList, pNew);
    }
    eDest = IsVirtual(pTab) ? SRT_Table : SRT_Upfrom;
  }else if( IsView(pTab) ){
    for(i=0; i<pTab->nCol; i++){
      pList = sqlite3ExprListAppend(pParse, pList, exprRowColumn(pParse, i));
    }
    eDest = SRT_Table;
  }else{
    eDest = IsVirtual(pTab) ? SRT_Table : SRT_Upfrom;
    pList = sqlite3ExprListAppend(pParse, 0, sqlite3PExpr(pParse,TK_ROW,0,0));
#ifdef SQLITE_ENABLE_UPDATE_DELETE_LIMIT
    if( pLimit ){
      pGrp = sqlite3ExprListAppend(pParse, 0, sqlite3PExpr(pParse,TK_ROW,0,0));
    }
#endif
  }
  assert( pChanges!=0 || pParse->db->mallocFailed );
  if( pChanges ){
    for(i=0; i<pChanges->nExpr; i++){
      pList = sqlite3ExprListAppend(pParse, pList,
          sqlite3ExprDup(db, pChanges->a[i].pExpr, 0)
      );
    }
  }
  pSelect = sqlite3SelectNew(pParse, pList,
      pSrc, pWhere2, pGrp, 0, pOrderBy2,
      SF_UFSrcCheck|SF_IncludeHidden|SF_UpdateFrom, pLimit2
  );
  if( pSelect ) pSelect->selFlags |= SF_OrderByReqd;
  sqlite3SelectDestInit(&dest, eDest, iEph);
  dest.iSDParm2 = (pPk ? pPk->nKeyCol : -1);
  sqlite3Select(pParse, pSelect, &dest);
  sqlite3SelectDelete(db, pSelect);
}

/*
** Process an UPDATE statement.
**
**   UPDATE OR IGNORE tbl SET a=b, c=d FROM tbl2... WHERE e<5 AND f NOT NULL;
**          \_______/ \_/     \______/      \_____/       \________________/
**           onError   |      pChanges         |                pWhere
**                     \_______________________/
**                               pTabList
*/
SQLITE_PRIVATE void sqlite3Update(
  Parse *pParse,         /* The parser context */
  SrcList *pTabList,     /* The table in which we should change things */
  ExprList *pChanges,    /* Things to be changed */
  Expr *pWhere,          /* The WHERE clause.  May be null */
  int onError,           /* How to handle constraint errors */
  ExprList *pOrderBy,    /* ORDER BY clause. May be null */
  Expr *pLimit,          /* LIMIT clause. May be null */
  Upsert *pUpsert        /* ON CONFLICT clause, or null */
){
  int i, j, k;           /* Loop counters */
  Table *pTab;           /* The table to be updated */
  int addrTop = 0;       /* VDBE instruction address of the start of the loop */
  WhereInfo *pWInfo = 0; /* Information about the WHERE clause */
  Vdbe *v;               /* The virtual database engine */
  Index *pIdx;           /* For looping over indices */
  Index *pPk;            /* The PRIMARY KEY index for WITHOUT ROWID tables */
  int nIdx;              /* Number of indices that need updating */
  int nAllIdx;           /* Total number of indexes */
  int iBaseCur;          /* Base cursor number */
  int iDataCur;          /* Cursor for the canonical data btree */
  int iIdxCur;           /* Cursor for the first index */
  sqlite3 *db;           /* The database structure */
  int *aRegIdx = 0;      /* Registers for to each index and the main table */
  int *aXRef = 0;        /* aXRef[i] is the index in pChanges->a[] of the
                         ** an expression for the i-th column of the table.
                         ** aXRef[i]==-1 if the i-th column is not changed. */
  u8 *aToOpen;           /* 1 for tables and indices to be opened */
  u8 chngPk;             /* PRIMARY KEY changed in a WITHOUT ROWID table */
  u8 chngRowid;          /* Rowid changed in a normal table */
  u8 chngKey;            /* Either chngPk or chngRowid */
  Expr *pRowidExpr = 0;  /* Expression defining the new record number */
  int iRowidExpr = -1;   /* Index of "rowid=" (or IPK) assignment in pChanges */
  AuthContext sContext;  /* The authorization context */
  NameContext sNC;       /* The name-context to resolve expressions in */
  int iDb;               /* Database containing the table being updated */
  int eOnePass;          /* ONEPASS_XXX value from where.c */
  int hasFK;             /* True if foreign key processing is required */
  int labelBreak;        /* Jump here to break out of UPDATE loop */
  int labelContinue;     /* Jump here to continue next step of UPDATE loop */
  int flags;             /* Flags for sqlite3WhereBegin() */

#ifndef SQLITE_OMIT_TRIGGER
  int isView;            /* True when updating a view (INSTEAD OF trigger) */
  Trigger *pTrigger;     /* List of triggers on pTab, if required */
  int tmask;             /* Mask of TRIGGER_BEFORE|TRIGGER_AFTER */
#endif
  int newmask;           /* Mask of NEW.* columns accessed by BEFORE triggers */
  int iEph = 0;          /* Ephemeral table holding all primary key values */
  int nKey = 0;          /* Number of elements in regKey for WITHOUT ROWID */
  int aiCurOnePass[2];   /* The write cursors opened by WHERE_ONEPASS */
  int addrOpen = 0;      /* Address of OP_OpenEphemeral */
  int iPk = 0;           /* First of nPk cells holding PRIMARY KEY value */
  i16 nPk = 0;           /* Number of components of the PRIMARY KEY */
  int bReplace = 0;      /* True if REPLACE conflict resolution might happen */
  int bFinishSeek = 1;   /* The OP_FinishSeek opcode is needed */
  int nChangeFrom = 0;   /* If there is a FROM, pChanges->nExpr, else 0 */

  /* Register Allocations */
  int regRowCount = 0;   /* A count of rows changed */
  int regOldRowid = 0;   /* The old rowid */
  int regNewRowid = 0;   /* The new rowid */
  int regNew = 0;        /* Content of the NEW.* table in triggers */
  int regOld = 0;        /* Content of OLD.* table in triggers */
  int regRowSet = 0;     /* Rowset of rows to be updated */
  int regKey = 0;        /* composite PRIMARY KEY value */
