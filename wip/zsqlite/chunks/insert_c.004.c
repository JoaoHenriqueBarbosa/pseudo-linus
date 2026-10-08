
/* Make sure "isView" and other macros defined above are undefined. Otherwise
** they may interfere with compilation of other functions in this file
** (or in another file, if this file becomes part of the amalgamation).  */
#ifdef isView
 #undef isView
#endif
#ifdef pTrigger
 #undef pTrigger
#endif
#ifdef tmask
 #undef tmask
#endif

/*
** Meanings of bits in of pWalker->eCode for
** sqlite3ExprReferencesUpdatedColumn()
*/
#define CKCNSTRNT_COLUMN   0x01    /* CHECK constraint uses a changing column */
#define CKCNSTRNT_ROWID    0x02    /* CHECK constraint references the ROWID */

/* This is the Walker callback from sqlite3ExprReferencesUpdatedColumn().
*  Set bit 0x01 of pWalker->eCode if pWalker->eCode to 0 and if this
** expression node references any of the
** columns that are being modified by an UPDATE statement.
*/
static int checkConstraintExprNode(Walker *pWalker, Expr *pExpr){
  if( pExpr->op==TK_COLUMN ){
    assert( pExpr->iColumn>=0 || pExpr->iColumn==-1 );
    if( pExpr->iColumn>=0 ){
      if( pWalker->u.aiCol[pExpr->iColumn]>=0 ){
        pWalker->eCode |= CKCNSTRNT_COLUMN;
      }
    }else{
      pWalker->eCode |= CKCNSTRNT_ROWID;
    }
  }
  return WRC_Continue;
}

/*
** pExpr is a CHECK constraint on a row that is being UPDATE-ed.  The
** only columns that are modified by the UPDATE are those for which
** aiChng[i]>=0, and also the ROWID is modified if chngRowid is true.
**
** Return true if CHECK constraint pExpr uses any of the
** changing columns (or the rowid if it is changing).  In other words,
** return true if this CHECK constraint must be validated for
** the new row in the UPDATE statement.
**
** 2018-09-15: pExpr might also be an expression for an index-on-expressions.
** The operation of this routine is the same - return true if an only if
** the expression uses one or more of columns identified by the second and
** third arguments.
*/
SQLITE_PRIVATE int sqlite3ExprReferencesUpdatedColumn(
  Expr *pExpr,    /* The expression to be checked */
  int *aiChng,    /* aiChng[x]>=0 if column x changed by the UPDATE */
  int chngRowid   /* True if UPDATE changes the rowid */
){
  Walker w;
  memset(&w, 0, sizeof(w));
  w.eCode = 0;
  w.xExprCallback = checkConstraintExprNode;
  w.u.aiCol = aiChng;
  sqlite3WalkExpr(&w, pExpr);
  if( !chngRowid ){
    testcase( (w.eCode & CKCNSTRNT_ROWID)!=0 );
    w.eCode &= ~CKCNSTRNT_ROWID;
  }
  testcase( w.eCode==0 );
  testcase( w.eCode==CKCNSTRNT_COLUMN );
  testcase( w.eCode==CKCNSTRNT_ROWID );
  testcase( w.eCode==(CKCNSTRNT_ROWID|CKCNSTRNT_COLUMN) );
  return w.eCode!=0;
}

/*
** The sqlite3GenerateConstraintChecks() routine usually wants to visit
** the indexes of a table in the order provided in the Table->pIndex list.
** However, sometimes (rarely - when there is an upsert) it wants to visit
** the indexes in a different order.  The following data structures accomplish
** this.
**
** The IndexIterator object is used to walk through all of the indexes
** of a table in either Index.pNext order, or in some other order established
** by an array of IndexListTerm objects.
*/
typedef struct IndexListTerm IndexListTerm;
typedef struct IndexIterator IndexIterator;
struct IndexIterator {
  int eType;    /* 0 for Index.pNext list.  1 for an array of IndexListTerm */
  int i;        /* Index of the current item from the list */
  union {
    struct {    /* Use this object for eType==0: A Index.pNext list */
      Index *pIdx;   /* The current Index */
    } lx;
    struct {    /* Use this object for eType==1; Array of IndexListTerm */
      int nIdx;               /* Size of the array */
      IndexListTerm *aIdx;    /* Array of IndexListTerms */
    } ax;
  } u;
};

/* When IndexIterator.eType==1, then each index is an array of instances
** of the following object
*/
struct IndexListTerm {
  Index *p;  /* The index */
  int ix;    /* Which entry in the original Table.pIndex list is this index*/
};

/* Return the first index on the list */
static Index *indexIteratorFirst(IndexIterator *pIter, int *pIx){
  assert( pIter->i==0 );
  if( pIter->eType ){
    *pIx = pIter->u.ax.aIdx[0].ix;
    return pIter->u.ax.aIdx[0].p;
  }else{
    *pIx = 0;
    return pIter->u.lx.pIdx;
  }
}

/* Return the next index from the list.  Return NULL when out of indexes */
static Index *indexIteratorNext(IndexIterator *pIter, int *pIx){
  if( pIter->eType ){
    int i = ++pIter->i;
    if( i>=pIter->u.ax.nIdx ){
      *pIx = i;
      return 0;
    }
    *pIx = pIter->u.ax.aIdx[i].ix;
    return pIter->u.ax.aIdx[i].p;
  }else{
    ++(*pIx);
    pIter->u.lx.pIdx = pIter->u.lx.pIdx->pNext;
    return pIter->u.lx.pIdx;
  }
}

/*
** Generate code to do constraint checks prior to an INSERT or an UPDATE
** on table pTab.
**
** The regNewData parameter is the first register in a range that contains
** the data to be inserted or the data after the update.  There will be
** pTab->nCol+1 registers in this range.  The first register (the one
** that regNewData points to) will contain the new rowid, or NULL in the
** case of a WITHOUT ROWID table.  The second register in the range will
** contain the content of the first table column.  The third register will
** contain the content of the second table column.  And so forth.
**
** The regOldData parameter is similar to regNewData except that it contains
** the data prior to an UPDATE rather than afterwards.  regOldData is zero
** for an INSERT.  This routine can distinguish between UPDATE and INSERT by
** checking regOldData for zero.
**
** For an UPDATE, the pkChng boolean is true if the true primary key (the
** rowid for a normal table or the PRIMARY KEY for a WITHOUT ROWID table)
** might be modified by the UPDATE.  If pkChng is false, then the key of
** the iDataCur content table is guaranteed to be unchanged by the UPDATE.
**
** For an INSERT, the pkChng boolean indicates whether or not the rowid
** was explicitly specified as part of the INSERT statement.  If pkChng
** is zero, it means that the either rowid is computed automatically or
** that the table is a WITHOUT ROWID table and has no rowid.  On an INSERT,
** pkChng will only be true if the INSERT statement provides an integer
** value for either the rowid column or its INTEGER PRIMARY KEY alias.
**
** The code generated by this routine will store new index entries into
** registers identified by aRegIdx[].  No index entry is created for
** indices where aRegIdx[i]==0.  The order of indices in aRegIdx[] is
** the same as the order of indices on the linked list of indices
** at pTab->pIndex.
**
** (2019-05-07) The generated code also creates a new record for the
** main table, if pTab is a rowid table, and stores that record in the
** register identified by aRegIdx[nIdx] - in other words in the first
** entry of aRegIdx[] past the last index.  It is important that the
** record be generated during constraint checks to avoid affinity changes
** to the register content that occur after constraint checks but before
** the new record is inserted.
**
** The caller must have already opened writeable cursors on the main
** table and all applicable indices (that is to say, all indices for which
** aRegIdx[] is not zero).  iDataCur is the cursor for the main table when
** inserting or updating a rowid table, or the cursor for the PRIMARY KEY
** index when operating on a WITHOUT ROWID table.  iIdxCur is the cursor
** for the first index in the pTab->pIndex list.  Cursors for other indices
** are at iIdxCur+N for the N-th element of the pTab->pIndex list.
**
** This routine also generates code to check constraints.  NOT NULL,
** CHECK, and UNIQUE constraints are all checked.  If a constraint fails,
** then the appropriate action is performed.  There are five possible
** actions: ROLLBACK, ABORT, FAIL, REPLACE, and IGNORE.
**
**  Constraint type  Action       What Happens
**  ---------------  ----------   ----------------------------------------
**  any              ROLLBACK     The current transaction is rolled back and
**                                sqlite3_step() returns immediately with a
**                                return code of SQLITE_CONSTRAINT.
**
**  any              ABORT        Back out changes from the current command
**                                only (do not do a complete rollback) then
**                                cause sqlite3_step() to return immediately
**                                with SQLITE_CONSTRAINT.
**
**  any              FAIL         Sqlite3_step() returns immediately with a
**                                return code of SQLITE_CONSTRAINT.  The
**                                transaction is not rolled back and any
**                                changes to prior rows are retained.
**
**  any              IGNORE       The attempt in insert or update the current
**                                row is skipped, without throwing an error.
**                                Processing continues with the next row.
**                                (There is an immediate jump to ignoreDest.)
**
**  NOT NULL         REPLACE      The NULL value is replace by the default
**                                value for that column.  If the default value
**                                is NULL, the action is the same as ABORT.
**
**  UNIQUE           REPLACE      The other row that conflicts with the row
**                                being inserted is removed.
**
**  CHECK            REPLACE      Illegal.  The results in an exception.
**
** Which action to take is determined by the overrideError parameter.
** Or if overrideError==OE_Default, then the pParse->onError parameter
** is used.  Or if pParse->onError==OE_Default then the onError value
** for the constraint is used.
*/
SQLITE_PRIVATE void sqlite3GenerateConstraintChecks(
  Parse *pParse,       /* The parser context */
  Table *pTab,         /* The table being inserted or updated */
  int *aRegIdx,        /* Use register aRegIdx[i] for index i.  0 for unused */
  int iDataCur,        /* Canonical data cursor (main table or PK index) */
  int iIdxCur,         /* First index cursor */
  int regNewData,      /* First register in a range holding values to insert */
  int regOldData,      /* Previous content.  0 for INSERTs */
  u8 pkChng,           /* Non-zero if the rowid or PRIMARY KEY changed */
  u8 overrideError,    /* Override onError to this if not OE_Default */
  int ignoreDest,      /* Jump to this label on an OE_Ignore resolution */
  int *pbMayReplace,   /* OUT: Set to true if constraint may cause a replace */
  int *aiChng,         /* column i is unchanged if aiChng[i]<0 */
  Upsert *pUpsert      /* ON CONFLICT clauses, if any.  NULL otherwise */
){
  Vdbe *v;             /* VDBE under construction */
  Index *pIdx;         /* Pointer to one of the indices */
  Index *pPk = 0;      /* The PRIMARY KEY index for WITHOUT ROWID tables */
  sqlite3 *db;         /* Database connection */
  int i;               /* loop counter */
  int ix;              /* Index loop counter */
  int nCol;            /* Number of columns */
  int onError;         /* Conflict resolution strategy */
  int seenReplace = 0; /* True if REPLACE is used to resolve INT PK conflict */
  int nPkField;        /* Number of fields in PRIMARY KEY. 1 for ROWID tables */
  Upsert *pUpsertClause = 0;  /* The specific ON CONFLICT clause for pIdx */
  u8 isUpdate;           /* True if this is an UPDATE operation */
  u8 bAffinityDone = 0;  /* True if the OP_Affinity operation has been run */
  int upsertIpkReturn = 0; /* Address of Goto at end of IPK uniqueness check */
  int upsertIpkDelay = 0;  /* Address of Goto to bypass initial IPK check */
  int ipkTop = 0;        /* Top of the IPK uniqueness check */
  int ipkBottom = 0;     /* OP_Goto at the end of the IPK uniqueness check */
  /* Variables associated with retesting uniqueness constraints after
  ** replace triggers fire have run */
  int regTrigCnt;       /* Register used to count replace trigger invocations */
  int addrRecheck = 0;  /* Jump here to recheck all uniqueness constraints */
  int lblRecheckOk = 0; /* Each recheck jumps to this label if it passes */
  Trigger *pTrigger;    /* List of DELETE triggers on the table pTab */
  int nReplaceTrig = 0; /* Number of replace triggers coded */
  IndexIterator sIdxIter;  /* Index iterator */

  isUpdate = regOldData!=0;
  db = pParse->db;
  v = pParse->pVdbe;
  assert( v!=0 );
  assert( !IsView(pTab) );  /* This table is not a VIEW */
  nCol = pTab->nCol;

  /* pPk is the PRIMARY KEY index for WITHOUT ROWID tables and NULL for
  ** normal rowid tables.  nPkField is the number of key fields in the
  ** pPk index or 1 for a rowid table.  In other words, nPkField is the
  ** number of fields in the true primary key of the table. */
  if( HasRowid(pTab) ){
    pPk = 0;
    nPkField = 1;
  }else{
    pPk = sqlite3PrimaryKeyIndex(pTab);
    nPkField = pPk->nKeyCol;
  }

  /* Record that this module has started */
  VdbeModuleComment((v, "BEGIN: GenCnstCks(%d,%d,%d,%d,%d)",
                     iDataCur, iIdxCur, regNewData, regOldData, pkChng));

  /* Test all NOT NULL constraints.
  */
  if( pTab->tabFlags & TF_HasNotNull ){
    int b2ndPass = 0;         /* True if currently running 2nd pass */
    int nSeenReplace = 0;     /* Number of ON CONFLICT REPLACE operations */
    int nGenerated = 0;       /* Number of generated columns with NOT NULL */
    while(1){  /* Make 2 passes over columns. Exit loop via "break" */
      for(i=0; i<nCol; i++){
        int iReg;                        /* Register holding column value */
        Column *pCol = &pTab->aCol[i];   /* The column to check for NOT NULL */
        int isGenerated;                 /* non-zero if column is generated */
        onError = pCol->notNull;
        if( onError==OE_None ) continue; /* No NOT NULL on this column */
        if( i==pTab->iPKey ){
          continue;        /* ROWID is never NULL */
        }
        isGenerated = pCol->colFlags & COLFLAG_GENERATED;
        if( isGenerated && !b2ndPass ){
          nGenerated++;
          continue;        /* Generated columns processed on 2nd pass */
        }
        if( aiChng && aiChng[i]<0 && !isGenerated ){
          /* Do not check NOT NULL on columns that do not change */
          continue;
        }
        if( overrideError!=OE_Default ){
          onError = overrideError;
        }else if( onError==OE_Default ){
          onError = OE_Abort;
        }
        if( onError==OE_Replace ){
          if( b2ndPass        /* REPLACE becomes ABORT on the 2nd pass */
           || pCol->iDflt==0  /* REPLACE is ABORT if no DEFAULT value */
          ){
            testcase( pCol->colFlags & COLFLAG_VIRTUAL );
            testcase( pCol->colFlags & COLFLAG_STORED );
            testcase( pCol->colFlags & COLFLAG_GENERATED );
            onError = OE_Abort;
          }else{
            assert( !isGenerated );
          }
        }else if( b2ndPass && !isGenerated ){
          continue;
        }
        assert( onError==OE_Rollback || onError==OE_Abort || onError==OE_Fail
            || onError==OE_Ignore || onError==OE_Replace );
        testcase( i!=sqlite3TableColumnToStorage(pTab, i) );
        iReg = sqlite3TableColumnToStorage(pTab, i) + regNewData + 1;
        switch( onError ){
          case OE_Replace: {
            int addr1 = sqlite3VdbeAddOp1(v, OP_NotNull, iReg);
            VdbeCoverage(v);
            assert( (pCol->colFlags & COLFLAG_GENERATED)==0 );
            nSeenReplace++;
