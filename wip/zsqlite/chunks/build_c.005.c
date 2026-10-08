
/* Change the most recently parsed column to be a GENERATED ALWAYS AS
** column.
*/
SQLITE_PRIVATE void sqlite3AddGenerated(Parse *pParse, Expr *pExpr, Token *pType){
#ifndef SQLITE_OMIT_GENERATED_COLUMNS
  u8 eType = COLFLAG_VIRTUAL;
  Table *pTab = pParse->pNewTable;
  Column *pCol;
  if( pTab==0 ){
    /* generated column in an CREATE TABLE IF NOT EXISTS that already exists */
    goto generated_done;
  }
  pCol = &(pTab->aCol[pTab->nCol-1]);
  if( IN_DECLARE_VTAB ){
    sqlite3ErrorMsg(pParse, "virtual tables cannot use computed columns");
    goto generated_done;
  }
  if( pCol->iDflt>0 ) goto generated_error;
  if( pType ){
    if( pType->n==7 && sqlite3StrNICmp("virtual",pType->z,7)==0 ){
      /* no-op */
    }else if( pType->n==6 && sqlite3StrNICmp("stored",pType->z,6)==0 ){
      eType = COLFLAG_STORED;
    }else{
      goto generated_error;
    }
  }
  if( eType==COLFLAG_VIRTUAL ) pTab->nNVCol--;
  pCol->colFlags |= eType;
  assert( TF_HasVirtual==COLFLAG_VIRTUAL );
  assert( TF_HasStored==COLFLAG_STORED );
  pTab->tabFlags |= eType;
  if( pCol->colFlags & COLFLAG_PRIMKEY ){
    makeColumnPartOfPrimaryKey(pParse, pCol); /* For the error message */
  }
  if( ALWAYS(pExpr) && pExpr->op==TK_ID ){
    /* The value of a generated column needs to be a real expression, not
    ** just a reference to another column, in order for covering index
    ** optimizations to work correctly.  So if the value is not an expression,
    ** turn it into one by adding a unary "+" operator. */
    pExpr = sqlite3PExpr(pParse, TK_UPLUS, pExpr, 0);
  }
  if( pExpr && pExpr->op!=TK_RAISE ) pExpr->affExpr = pCol->affinity;
  sqlite3ColumnSetExpr(pParse, pTab, pCol, pExpr);
  pExpr = 0;
  goto generated_done;

generated_error:
  sqlite3ErrorMsg(pParse, "error in generated column \"%s\"",
                  pCol->zCnName);
generated_done:
  sqlite3ExprDelete(pParse->db, pExpr);
#else
  /* Throw and error for the GENERATED ALWAYS AS clause if the
  ** SQLITE_OMIT_GENERATED_COLUMNS compile-time option is used. */
  sqlite3ErrorMsg(pParse, "generated columns not supported");
  sqlite3ExprDelete(pParse->db, pExpr);
#endif
}

/*
** Generate code that will increment the schema cookie.
**
** The schema cookie is used to determine when the schema for the
** database changes.  After each schema change, the cookie value
** changes.  When a process first reads the schema it records the
** cookie.  Thereafter, whenever it goes to access the database,
** it checks the cookie to make sure the schema has not changed
** since it was last read.
**
** This plan is not completely bullet-proof.  It is possible for
** the schema to change multiple times and for the cookie to be
** set back to prior value.  But schema changes are infrequent
** and the probability of hitting the same cookie value is only
** 1 chance in 2^32.  So we're safe enough.
**
** IMPLEMENTATION-OF: R-34230-56049 SQLite automatically increments
** the schema-version whenever the schema changes.
*/
SQLITE_PRIVATE void sqlite3ChangeCookie(Parse *pParse, int iDb){
  sqlite3 *db = pParse->db;
  Vdbe *v = pParse->pVdbe;
  assert( sqlite3SchemaMutexHeld(db, iDb, 0) );
  sqlite3VdbeAddOp3(v, OP_SetCookie, iDb, BTREE_SCHEMA_VERSION,
                   (int)(1+(unsigned)db->aDb[iDb].pSchema->schema_cookie));
}

/*
** Measure the number of characters needed to output the given
** identifier.  The number returned includes any quotes used
** but does not include the null terminator.
**
** The estimate is conservative.  It might be larger that what is
** really needed.
*/
static int identLength(const char *z){
  int n;
  for(n=0; *z; n++, z++){
    if( *z=='"' ){ n++; }
  }
  return n + 2;
}

/*
** The first parameter is a pointer to an output buffer. The second
** parameter is a pointer to an integer that contains the offset at
** which to write into the output buffer. This function copies the
** nul-terminated string pointed to by the third parameter, zSignedIdent,
** to the specified offset in the buffer and updates *pIdx to refer
** to the first byte after the last byte written before returning.
**
** If the string zSignedIdent consists entirely of alphanumeric
** characters, does not begin with a digit and is not an SQL keyword,
** then it is copied to the output buffer exactly as it is. Otherwise,
** it is quoted using double-quotes.
*/
static void identPut(char *z, int *pIdx, char *zSignedIdent){
  unsigned char *zIdent = (unsigned char*)zSignedIdent;
  int i, j, needQuote;
  i = *pIdx;

  for(j=0; zIdent[j]; j++){
    if( !sqlite3Isalnum(zIdent[j]) && zIdent[j]!='_' ) break;
  }
  needQuote = sqlite3Isdigit(zIdent[0])
            || sqlite3KeywordCode(zIdent, j)!=TK_ID
            || zIdent[j]!=0
            || j==0;

  if( needQuote ) z[i++] = '"';
  for(j=0; zIdent[j]; j++){
    z[i++] = zIdent[j];
    if( zIdent[j]=='"' ) z[i++] = '"';
  }
  if( needQuote ) z[i++] = '"';
  z[i] = 0;
  *pIdx = i;
}

/*
** Generate a CREATE TABLE statement appropriate for the given
** table.  Memory to hold the text of the statement is obtained
** from sqliteMalloc() and must be freed by the calling function.
*/
static char *createTableStmt(sqlite3 *db, Table *p){
  int i, k, n;
  char *zStmt;
  char *zSep, *zSep2, *zEnd;
  Column *pCol;
  n = 0;
  for(pCol = p->aCol, i=0; i<p->nCol; i++, pCol++){
    n += identLength(pCol->zCnName) + 5;
  }
  n += identLength(p->zName);
  if( n<50 ){
    zSep = "";
    zSep2 = ",";
    zEnd = ")";
  }else{
    zSep = "\n  ";
    zSep2 = ",\n  ";
    zEnd = "\n)";
  }
  n += 35 + 6*p->nCol;
  zStmt = sqlite3DbMallocRaw(0, n);
  if( zStmt==0 ){
    sqlite3OomFault(db);
    return 0;
  }
  sqlite3_snprintf(n, zStmt, "CREATE TABLE ");
  k = sqlite3Strlen30(zStmt);
  identPut(zStmt, &k, p->zName);
  zStmt[k++] = '(';
  for(pCol=p->aCol, i=0; i<p->nCol; i++, pCol++){
    static const char * const azType[] = {
        /* SQLITE_AFF_BLOB    */ "",
        /* SQLITE_AFF_TEXT    */ " TEXT",
        /* SQLITE_AFF_NUMERIC */ " NUM",
        /* SQLITE_AFF_INTEGER */ " INT",
        /* SQLITE_AFF_REAL    */ " REAL",
        /* SQLITE_AFF_FLEXNUM */ " NUM",
    };
    int len;
    const char *zType;

    sqlite3_snprintf(n-k, &zStmt[k], zSep);
    k += sqlite3Strlen30(&zStmt[k]);
    zSep = zSep2;
    identPut(zStmt, &k, pCol->zCnName);
    assert( pCol->affinity-SQLITE_AFF_BLOB >= 0 );
    assert( pCol->affinity-SQLITE_AFF_BLOB < ArraySize(azType) );
    testcase( pCol->affinity==SQLITE_AFF_BLOB );
    testcase( pCol->affinity==SQLITE_AFF_TEXT );
    testcase( pCol->affinity==SQLITE_AFF_NUMERIC );
    testcase( pCol->affinity==SQLITE_AFF_INTEGER );
    testcase( pCol->affinity==SQLITE_AFF_REAL );
    testcase( pCol->affinity==SQLITE_AFF_FLEXNUM );

    zType = azType[pCol->affinity - SQLITE_AFF_BLOB];
    len = sqlite3Strlen30(zType);
    assert( pCol->affinity==SQLITE_AFF_BLOB
            || pCol->affinity==SQLITE_AFF_FLEXNUM
            || pCol->affinity==sqlite3AffinityType(zType, 0) );
    memcpy(&zStmt[k], zType, len);
    k += len;
    assert( k<=n );
  }
  sqlite3_snprintf(n-k, &zStmt[k], "%s", zEnd);
  return zStmt;
}

/*
** Resize an Index object to hold N columns total.  Return SQLITE_OK
** on success and SQLITE_NOMEM on an OOM error.
*/
static int resizeIndexObject(sqlite3 *db, Index *pIdx, int N){
  char *zExtra;
  int nByte;
  if( pIdx->nColumn>=N ) return SQLITE_OK;
  assert( pIdx->isResized==0 );
  nByte = (sizeof(char*) + sizeof(LogEst) + sizeof(i16) + 1)*N;
  zExtra = sqlite3DbMallocZero(db, nByte);
  if( zExtra==0 ) return SQLITE_NOMEM_BKPT;
  memcpy(zExtra, pIdx->azColl, sizeof(char*)*pIdx->nColumn);
  pIdx->azColl = (const char**)zExtra;
  zExtra += sizeof(char*)*N;
  memcpy(zExtra, pIdx->aiRowLogEst, sizeof(LogEst)*(pIdx->nKeyCol+1));
  pIdx->aiRowLogEst = (LogEst*)zExtra;
  zExtra += sizeof(LogEst)*N;
  memcpy(zExtra, pIdx->aiColumn, sizeof(i16)*pIdx->nColumn);
  pIdx->aiColumn = (i16*)zExtra;
  zExtra += sizeof(i16)*N;
  memcpy(zExtra, pIdx->aSortOrder, pIdx->nColumn);
  pIdx->aSortOrder = (u8*)zExtra;
  pIdx->nColumn = N;
  pIdx->isResized = 1;
  return SQLITE_OK;
}

/*
** Estimate the total row width for a table.
*/
static void estimateTableWidth(Table *pTab){
  unsigned wTable = 0;
  const Column *pTabCol;
  int i;
  for(i=pTab->nCol, pTabCol=pTab->aCol; i>0; i--, pTabCol++){
    wTable += pTabCol->szEst;
  }
  if( pTab->iPKey<0 ) wTable++;
  pTab->szTabRow = sqlite3LogEst(wTable*4);
}

/*
** Estimate the average size of a row for an index.
*/
static void estimateIndexWidth(Index *pIdx){
  unsigned wIndex = 0;
  int i;
  const Column *aCol = pIdx->pTable->aCol;
  for(i=0; i<pIdx->nColumn; i++){
    i16 x = pIdx->aiColumn[i];
    assert( x<pIdx->pTable->nCol );
    wIndex += x<0 ? 1 : aCol[x].szEst;
  }
  pIdx->szIdxRow = sqlite3LogEst(wIndex*4);
}

/* Return true if column number x is any of the first nCol entries of aiCol[].
** This is used to determine if the column number x appears in any of the
** first nCol entries of an index.
*/
static int hasColumn(const i16 *aiCol, int nCol, int x){
  while( nCol-- > 0 ){
    if( x==*(aiCol++) ){
      return 1;
    }
  }
  return 0;
}

/*
** Return true if any of the first nKey entries of index pIdx exactly
** match the iCol-th entry of pPk.  pPk is always a WITHOUT ROWID
** PRIMARY KEY index.  pIdx is an index on the same table.  pIdx may
** or may not be the same index as pPk.
**
** The first nKey entries of pIdx are guaranteed to be ordinary columns,
** not a rowid or expression.
**
** This routine differs from hasColumn() in that both the column and the
** collating sequence must match for this routine, but for hasColumn() only
** the column name must match.
*/
static int isDupColumn(Index *pIdx, int nKey, Index *pPk, int iCol){
  int i, j;
  assert( nKey<=pIdx->nColumn );
  assert( iCol<MAX(pPk->nColumn,pPk->nKeyCol) );
  assert( pPk->idxType==SQLITE_IDXTYPE_PRIMARYKEY );
  assert( pPk->pTable->tabFlags & TF_WithoutRowid );
  assert( pPk->pTable==pIdx->pTable );
  testcase( pPk==pIdx );
  j = pPk->aiColumn[iCol];
  assert( j!=XN_ROWID && j!=XN_EXPR );
  for(i=0; i<nKey; i++){
    assert( pIdx->aiColumn[i]>=0 || j>=0 );
    if( pIdx->aiColumn[i]==j
     && sqlite3StrICmp(pIdx->azColl[i], pPk->azColl[iCol])==0
    ){
      return 1;
    }
  }
  return 0;
}

/* Recompute the colNotIdxed field of the Index.
**
** colNotIdxed is a bitmask that has a 0 bit representing each indexed
** columns that are within the first 63 columns of the table and a 1 for
** all other bits (all columns that are not in the index).  The
** high-order bit of colNotIdxed is always 1.  All unindexed columns
** of the table have a 1.
**
** 2019-10-24:  For the purpose of this computation, virtual columns are
** not considered to be covered by the index, even if they are in the
** index, because we do not trust the logic in whereIndexExprTrans() to be
** able to find all instances of a reference to the indexed table column
** and convert them into references to the index.  Hence we always want
** the actual table at hand in order to recompute the virtual column, if
** necessary.
**
** The colNotIdxed mask is AND-ed with the SrcList.a[].colUsed mask
** to determine if the index is covering index.
*/
static void recomputeColumnsNotIndexed(Index *pIdx){
  Bitmask m = 0;
  int j;
  Table *pTab = pIdx->pTable;
  for(j=pIdx->nColumn-1; j>=0; j--){
    int x = pIdx->aiColumn[j];
    if( x>=0 && (pTab->aCol[x].colFlags & COLFLAG_VIRTUAL)==0 ){
      testcase( x==BMS-1 );
      testcase( x==BMS-2 );
      if( x<BMS-1 ) m |= MASKBIT(x);
    }
  }
  pIdx->colNotIdxed = ~m;
  assert( (pIdx->colNotIdxed>>63)==1 );  /* See note-20221022-a */
}

/*
** This routine runs at the end of parsing a CREATE TABLE statement that
** has a WITHOUT ROWID clause.  The job of this routine is to convert both
** internal schema data structures and the generated VDBE code so that they
** are appropriate for a WITHOUT ROWID table instead of a rowid table.
** Changes include:
**
**     (1)  Set all columns of the PRIMARY KEY schema object to be NOT NULL.
**     (2)  Convert P3 parameter of the OP_CreateBtree from BTREE_INTKEY
**          into BTREE_BLOBKEY.
**     (3)  Bypass the creation of the sqlite_schema table entry
**          for the PRIMARY KEY as the primary key index is now
**          identified by the sqlite_schema table entry of the table itself.
**     (4)  Set the Index.tnum of the PRIMARY KEY Index object in the
**          schema to the rootpage from the main table.
**     (5)  Add all table columns to the PRIMARY KEY Index object
**          so that the PRIMARY KEY is a covering index.  The surplus
**          columns are part of KeyInfo.nAllField and are not used for
**          sorting or lookup or uniqueness checks.
**     (6)  Replace the rowid tail on all automatically generated UNIQUE
**          indices with the PRIMARY KEY columns.
**
** For virtual tables, only (1) is performed.
*/
static void convertToWithoutRowidTable(Parse *pParse, Table *pTab){
  Index *pIdx;
  Index *pPk;
  int nPk;
  int nExtra;
  int i, j;
  sqlite3 *db = pParse->db;
  Vdbe *v = pParse->pVdbe;

  /* Mark every PRIMARY KEY column as NOT NULL (except for imposter tables)
  */
  if( !db->init.imposterTable ){
    for(i=0; i<pTab->nCol; i++){
      if( (pTab->aCol[i].colFlags & COLFLAG_PRIMKEY)!=0
       && (pTab->aCol[i].notNull==OE_None)
      ){
        pTab->aCol[i].notNull = OE_Abort;
      }
    }
    pTab->tabFlags |= TF_HasNotNull;
  }

  /* Convert the P3 operand of the OP_CreateBtree opcode from BTREE_INTKEY
  ** into BTREE_BLOBKEY.
  */
  assert( !pParse->bReturning );
  if( pParse->u1.addrCrTab ){
    assert( v );
    sqlite3VdbeChangeP3(v, pParse->u1.addrCrTab, BTREE_BLOBKEY);
  }

  /* Locate the PRIMARY KEY index.  Or, if this table was originally
  ** an INTEGER PRIMARY KEY table, create a new PRIMARY KEY index.
  */
  if( pTab->iPKey>=0 ){
    ExprList *pList;
    Token ipkToken;
    sqlite3TokenInit(&ipkToken, pTab->aCol[pTab->iPKey].zCnName);
    pList = sqlite3ExprListAppend(pParse, 0,
                  sqlite3ExprAlloc(db, TK_ID, &ipkToken, 0));
    if( pList==0 ){
      pTab->tabFlags &= ~TF_WithoutRowid;
      return;
    }
    if( IN_RENAME_OBJECT ){
      sqlite3RenameTokenRemap(pParse, pList->a[0].pExpr, &pTab->iPKey);
    }
    pList->a[0].fg.sortFlags = pParse->iPkSortOrder;
    assert( pParse->pNewTable==pTab );
    pTab->iPKey = -1;
    sqlite3CreateIndex(pParse, 0, 0, 0, pList, pTab->keyConf, 0, 0, 0, 0,
                       SQLITE_IDXTYPE_PRIMARYKEY);
    if( pParse->nErr ){
      pTab->tabFlags &= ~TF_WithoutRowid;
      return;
    }
    assert( db->mallocFailed==0 );
    pPk = sqlite3PrimaryKeyIndex(pTab);
    assert( pPk->nKeyCol==1 );
  }else{
    pPk = sqlite3PrimaryKeyIndex(pTab);
    assert( pPk!=0 );

    /*
    ** Remove all redundant columns from the PRIMARY KEY.  For example, change
    ** "PRIMARY KEY(a,b,a,b,c,b,c,d)" into just "PRIMARY KEY(a,b,c,d)".  Later
    ** code assumes the PRIMARY KEY contains no repeated columns.
    */
    for(i=j=1; i<pPk->nKeyCol; i++){
      if( isDupColumn(pPk, j, pPk, i) ){
        pPk->nColumn--;
      }else{
        testcase( hasColumn(pPk->aiColumn, j, pPk->aiColumn[i]) );
        pPk->azColl[j] = pPk->azColl[i];
        pPk->aSortOrder[j] = pPk->aSortOrder[i];
        pPk->aiColumn[j++] = pPk->aiColumn[i];
      }
    }
    pPk->nKeyCol = j;
  }
  assert( pPk!=0 );
  pPk->isCovering = 1;
  if( !db->init.imposterTable ) pPk->uniqNotNull = 1;
  nPk = pPk->nColumn = pPk->nKeyCol;

  /* Bypass the creation of the PRIMARY KEY btree and the sqlite_schema
  ** table entry. This is only required if currently generating VDBE
  ** code for a CREATE TABLE (not when parsing one as part of reading
  ** a database schema).  */
  if( v && pPk->tnum>0 ){
    assert( db->init.busy==0 );
    sqlite3VdbeChangeOpcode(v, (int)pPk->tnum, OP_Goto);
  }

  /* The root page of the PRIMARY KEY is the table root page */
  pPk->tnum = pTab->tnum;

  /* Update the in-memory representation of all UNIQUE indices by converting
  ** the final rowid column into one or more columns of the PRIMARY KEY.
  */
  for(pIdx=pTab->pIndex; pIdx; pIdx=pIdx->pNext){
    int n;
    if( IsPrimaryKeyIndex(pIdx) ) continue;
    for(i=n=0; i<nPk; i++){
      if( !isDupColumn(pIdx, pIdx->nKeyCol, pPk, i) ){
        testcase( hasColumn(pIdx->aiColumn, pIdx->nKeyCol, pPk->aiColumn[i]) );
        n++;
      }
    }
    if( n==0 ){
      /* This index is a superset of the primary key */
      pIdx->nColumn = pIdx->nKeyCol;
      continue;
    }
    if( resizeIndexObject(db, pIdx, pIdx->nKeyCol+n) ) return;
    for(i=0, j=pIdx->nKeyCol; i<nPk; i++){
      if( !isDupColumn(pIdx, pIdx->nKeyCol, pPk, i) ){
        testcase( hasColumn(pIdx->aiColumn, pIdx->nKeyCol, pPk->aiColumn[i]) );
        pIdx->aiColumn[j] = pPk->aiColumn[i];
        pIdx->azColl[j] = pPk->azColl[i];
        if( pPk->aSortOrder[i] ){
          /* See ticket https://www.sqlite.org/src/info/bba7b69f9849b5bf */
          pIdx->bAscKeyBug = 1;
        }
        j++;
      }
    }
    assert( pIdx->nColumn>=pIdx->nKeyCol+n );
    assert( pIdx->nColumn>=j );
  }

  /* Add all table columns to the PRIMARY KEY index
  */
  nExtra = 0;
  for(i=0; i<pTab->nCol; i++){
    if( !hasColumn(pPk->aiColumn, nPk, i)
     && (pTab->aCol[i].colFlags & COLFLAG_VIRTUAL)==0 ) nExtra++;
  }
  if( resizeIndexObject(db, pPk, nPk+nExtra) ) return;
  for(i=0, j=nPk; i<pTab->nCol; i++){
    if( !hasColumn(pPk->aiColumn, j, i)
     && (pTab->aCol[i].colFlags & COLFLAG_VIRTUAL)==0
    ){
      assert( j<pPk->nColumn );
      pPk->aiColumn[j] = i;
      pPk->azColl[j] = sqlite3StrBINARY;
      j++;
    }
  }
  assert( pPk->nColumn==j );
  assert( pTab->nNVCol<=j );
  recomputeColumnsNotIndexed(pPk);
}
