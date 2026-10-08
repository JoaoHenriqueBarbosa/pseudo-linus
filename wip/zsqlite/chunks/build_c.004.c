
/*
** This routine is called by the parser while in the middle of
** parsing a CREATE TABLE statement.  A "NOT NULL" constraint has
** been seen on a column.  This routine sets the notNull flag on
** the column currently under construction.
*/
SQLITE_PRIVATE void sqlite3AddNotNull(Parse *pParse, int onError){
  Table *p;
  Column *pCol;
  p = pParse->pNewTable;
  if( p==0 || NEVER(p->nCol<1) ) return;
  pCol = &p->aCol[p->nCol-1];
  pCol->notNull = (u8)onError;
  p->tabFlags |= TF_HasNotNull;

  /* Set the uniqNotNull flag on any UNIQUE or PK indexes already created
  ** on this column.  */
  if( pCol->colFlags & COLFLAG_UNIQUE ){
    Index *pIdx;
    for(pIdx=p->pIndex; pIdx; pIdx=pIdx->pNext){
      assert( pIdx->nKeyCol==1 && pIdx->onError!=OE_None );
      if( pIdx->aiColumn[0]==p->nCol-1 ){
        pIdx->uniqNotNull = 1;
      }
    }
  }
}

/*
** Scan the column type name zType (length nType) and return the
** associated affinity type.
**
** This routine does a case-independent search of zType for the
** substrings in the following table. If one of the substrings is
** found, the corresponding affinity is returned. If zType contains
** more than one of the substrings, entries toward the top of
** the table take priority. For example, if zType is 'BLOBINT',
** SQLITE_AFF_INTEGER is returned.
**
** Substring     | Affinity
** --------------------------------
** 'INT'         | SQLITE_AFF_INTEGER
** 'CHAR'        | SQLITE_AFF_TEXT
** 'CLOB'        | SQLITE_AFF_TEXT
** 'TEXT'        | SQLITE_AFF_TEXT
** 'BLOB'        | SQLITE_AFF_BLOB
** 'REAL'        | SQLITE_AFF_REAL
** 'FLOA'        | SQLITE_AFF_REAL
** 'DOUB'        | SQLITE_AFF_REAL
**
** If none of the substrings in the above table are found,
** SQLITE_AFF_NUMERIC is returned.
*/
SQLITE_PRIVATE char sqlite3AffinityType(const char *zIn, Column *pCol){
  u32 h = 0;
  char aff = SQLITE_AFF_NUMERIC;
  const char *zChar = 0;

  assert( zIn!=0 );
  while( zIn[0] ){
    u8 x = *(u8*)zIn;
    h = (h<<8) + sqlite3UpperToLower[x];
    zIn++;
    if( h==(('c'<<24)+('h'<<16)+('a'<<8)+'r') ){             /* CHAR */
      aff = SQLITE_AFF_TEXT;
      zChar = zIn;
    }else if( h==(('c'<<24)+('l'<<16)+('o'<<8)+'b') ){       /* CLOB */
      aff = SQLITE_AFF_TEXT;
    }else if( h==(('t'<<24)+('e'<<16)+('x'<<8)+'t') ){       /* TEXT */
      aff = SQLITE_AFF_TEXT;
    }else if( h==(('b'<<24)+('l'<<16)+('o'<<8)+'b')          /* BLOB */
        && (aff==SQLITE_AFF_NUMERIC || aff==SQLITE_AFF_REAL) ){
      aff = SQLITE_AFF_BLOB;
      if( zIn[0]=='(' ) zChar = zIn;
#ifndef SQLITE_OMIT_FLOATING_POINT
    }else if( h==(('r'<<24)+('e'<<16)+('a'<<8)+'l')          /* REAL */
        && aff==SQLITE_AFF_NUMERIC ){
      aff = SQLITE_AFF_REAL;
    }else if( h==(('f'<<24)+('l'<<16)+('o'<<8)+'a')          /* FLOA */
        && aff==SQLITE_AFF_NUMERIC ){
      aff = SQLITE_AFF_REAL;
    }else if( h==(('d'<<24)+('o'<<16)+('u'<<8)+'b')          /* DOUB */
        && aff==SQLITE_AFF_NUMERIC ){
      aff = SQLITE_AFF_REAL;
#endif
    }else if( (h&0x00FFFFFF)==(('i'<<16)+('n'<<8)+'t') ){    /* INT */
      aff = SQLITE_AFF_INTEGER;
      break;
    }
  }

  /* If pCol is not NULL, store an estimate of the field size.  The
  ** estimate is scaled so that the size of an integer is 1.  */
  if( pCol ){
    int v = 0;   /* default size is approx 4 bytes */
    if( aff<SQLITE_AFF_NUMERIC ){
      if( zChar ){
        while( zChar[0] ){
          if( sqlite3Isdigit(zChar[0]) ){
            /* BLOB(k), VARCHAR(k), CHAR(k) -> r=(k/4+1) */
            sqlite3GetInt32(zChar, &v);
            break;
          }
          zChar++;
        }
      }else{
        v = 16;   /* BLOB, TEXT, CLOB -> r=5  (approx 20 bytes)*/
      }
    }
#ifdef SQLITE_ENABLE_SORTER_REFERENCES
    if( v>=sqlite3GlobalConfig.szSorterRef ){
      pCol->colFlags |= COLFLAG_SORTERREF;
    }
#endif
    v = v/4 + 1;
    if( v>255 ) v = 255;
    pCol->szEst = v;
  }
  return aff;
}

/*
** The expression is the default value for the most recently added column
** of the table currently under construction.
**
** Default value expressions must be constant.  Raise an exception if this
** is not the case.
**
** This routine is called by the parser while in the middle of
** parsing a CREATE TABLE statement.
*/
SQLITE_PRIVATE void sqlite3AddDefaultValue(
  Parse *pParse,           /* Parsing context */
  Expr *pExpr,             /* The parsed expression of the default value */
  const char *zStart,      /* Start of the default value text */
  const char *zEnd         /* First character past end of default value text */
){
  Table *p;
  Column *pCol;
  sqlite3 *db = pParse->db;
  p = pParse->pNewTable;
  if( p!=0 ){
    int isInit = db->init.busy && db->init.iDb!=1;
    pCol = &(p->aCol[p->nCol-1]);
    if( !sqlite3ExprIsConstantOrFunction(pExpr, isInit) ){
      sqlite3ErrorMsg(pParse, "default value of column [%s] is not constant",
          pCol->zCnName);
#ifndef SQLITE_OMIT_GENERATED_COLUMNS
    }else if( pCol->colFlags & COLFLAG_GENERATED ){
      testcase( pCol->colFlags & COLFLAG_VIRTUAL );
      testcase( pCol->colFlags & COLFLAG_STORED );
      sqlite3ErrorMsg(pParse, "cannot use DEFAULT on a generated column");
#endif
    }else{
      /* A copy of pExpr is used instead of the original, as pExpr contains
      ** tokens that point to volatile memory.
      */
      Expr x, *pDfltExpr;
      memset(&x, 0, sizeof(x));
      x.op = TK_SPAN;
      x.u.zToken = sqlite3DbSpanDup(db, zStart, zEnd);
      x.pLeft = pExpr;
      x.flags = EP_Skip;
      pDfltExpr = sqlite3ExprDup(db, &x, EXPRDUP_REDUCE);
      sqlite3DbFree(db, x.u.zToken);
      sqlite3ColumnSetExpr(pParse, p, pCol, pDfltExpr);
    }
  }
  if( IN_RENAME_OBJECT ){
    sqlite3RenameExprUnmap(pParse, pExpr);
  }
  sqlite3ExprDelete(db, pExpr);
}

/*
** Backwards Compatibility Hack:
**
** Historical versions of SQLite accepted strings as column names in
** indexes and PRIMARY KEY constraints and in UNIQUE constraints.  Example:
**
**     CREATE TABLE xyz(a,b,c,d,e,PRIMARY KEY('a'),UNIQUE('b','c' COLLATE trim)
**     CREATE INDEX abc ON xyz('c','d' DESC,'e' COLLATE nocase DESC);
**
** This is goofy.  But to preserve backwards compatibility we continue to
** accept it.  This routine does the necessary conversion.  It converts
** the expression given in its argument from a TK_STRING into a TK_ID
** if the expression is just a TK_STRING with an optional COLLATE clause.
** If the expression is anything other than TK_STRING, the expression is
** unchanged.
*/
static void sqlite3StringToId(Expr *p){
  if( p->op==TK_STRING ){
    p->op = TK_ID;
  }else if( p->op==TK_COLLATE && p->pLeft->op==TK_STRING ){
    p->pLeft->op = TK_ID;
  }
}

/*
** Tag the given column as being part of the PRIMARY KEY
*/
static void makeColumnPartOfPrimaryKey(Parse *pParse, Column *pCol){
  pCol->colFlags |= COLFLAG_PRIMKEY;
#ifndef SQLITE_OMIT_GENERATED_COLUMNS
  if( pCol->colFlags & COLFLAG_GENERATED ){
    testcase( pCol->colFlags & COLFLAG_VIRTUAL );
    testcase( pCol->colFlags & COLFLAG_STORED );
    sqlite3ErrorMsg(pParse,
      "generated columns cannot be part of the PRIMARY KEY");
  }
#endif
}

/*
** Designate the PRIMARY KEY for the table.  pList is a list of names
** of columns that form the primary key.  If pList is NULL, then the
** most recently added column of the table is the primary key.
**
** A table can have at most one primary key.  If the table already has
** a primary key (and this is the second primary key) then create an
** error.
**
** If the PRIMARY KEY is on a single column whose datatype is INTEGER,
** then we will try to use that column as the rowid.  Set the Table.iPKey
** field of the table under construction to be the index of the
** INTEGER PRIMARY KEY column.  Table.iPKey is set to -1 if there is
** no INTEGER PRIMARY KEY.
**
** If the key is not an INTEGER PRIMARY KEY, then create a unique
** index for the key.  No index is created for INTEGER PRIMARY KEYs.
*/
SQLITE_PRIVATE void sqlite3AddPrimaryKey(
  Parse *pParse,    /* Parsing context */
  ExprList *pList,  /* List of field names to be indexed */
  int onError,      /* What to do with a uniqueness conflict */
  int autoInc,      /* True if the AUTOINCREMENT keyword is present */
  int sortOrder     /* SQLITE_SO_ASC or SQLITE_SO_DESC */
){
  Table *pTab = pParse->pNewTable;
  Column *pCol = 0;
  int iCol = -1, i;
  int nTerm;
  if( pTab==0 ) goto primary_key_exit;
  if( pTab->tabFlags & TF_HasPrimaryKey ){
    sqlite3ErrorMsg(pParse,
      "table \"%s\" has more than one primary key", pTab->zName);
    goto primary_key_exit;
  }
  pTab->tabFlags |= TF_HasPrimaryKey;
  if( pList==0 ){
    iCol = pTab->nCol - 1;
    pCol = &pTab->aCol[iCol];
    makeColumnPartOfPrimaryKey(pParse, pCol);
    nTerm = 1;
  }else{
    nTerm = pList->nExpr;
    for(i=0; i<nTerm; i++){
      Expr *pCExpr = sqlite3ExprSkipCollate(pList->a[i].pExpr);
      assert( pCExpr!=0 );
      sqlite3StringToId(pCExpr);
      if( pCExpr->op==TK_ID ){
        const char *zCName;
        assert( !ExprHasProperty(pCExpr, EP_IntValue) );
        zCName = pCExpr->u.zToken;
        for(iCol=0; iCol<pTab->nCol; iCol++){
          if( sqlite3StrICmp(zCName, pTab->aCol[iCol].zCnName)==0 ){
            pCol = &pTab->aCol[iCol];
            makeColumnPartOfPrimaryKey(pParse, pCol);
            break;
          }
        }
      }
    }
  }
  if( nTerm==1
   && pCol
   && pCol->eCType==COLTYPE_INTEGER
   && sortOrder!=SQLITE_SO_DESC
  ){
    if( IN_RENAME_OBJECT && pList ){
      Expr *pCExpr = sqlite3ExprSkipCollate(pList->a[0].pExpr);
      sqlite3RenameTokenRemap(pParse, &pTab->iPKey, pCExpr);
    }
    pTab->iPKey = iCol;
    pTab->keyConf = (u8)onError;
    assert( autoInc==0 || autoInc==1 );
    pTab->tabFlags |= autoInc*TF_Autoincrement;
    if( pList ) pParse->iPkSortOrder = pList->a[0].fg.sortFlags;
    (void)sqlite3HasExplicitNulls(pParse, pList);
  }else if( autoInc ){
#ifndef SQLITE_OMIT_AUTOINCREMENT
    sqlite3ErrorMsg(pParse, "AUTOINCREMENT is only allowed on an "
       "INTEGER PRIMARY KEY");
#endif
  }else{
    sqlite3CreateIndex(pParse, 0, 0, 0, pList, onError, 0,
                           0, sortOrder, 0, SQLITE_IDXTYPE_PRIMARYKEY);
    pList = 0;
  }

primary_key_exit:
  sqlite3ExprListDelete(pParse->db, pList);
  return;
}

/*
** Add a new CHECK constraint to the table currently under construction.
*/
SQLITE_PRIVATE void sqlite3AddCheckConstraint(
  Parse *pParse,      /* Parsing context */
  Expr *pCheckExpr,   /* The check expression */
  const char *zStart, /* Opening "(" */
  const char *zEnd    /* Closing ")" */
){
#ifndef SQLITE_OMIT_CHECK
  Table *pTab = pParse->pNewTable;
  sqlite3 *db = pParse->db;
  if( pTab && !IN_DECLARE_VTAB
   && !sqlite3BtreeIsReadonly(db->aDb[db->init.iDb].pBt)
  ){
    pTab->pCheck = sqlite3ExprListAppend(pParse, pTab->pCheck, pCheckExpr);
    if( pParse->constraintName.n ){
      sqlite3ExprListSetName(pParse, pTab->pCheck, &pParse->constraintName, 1);
    }else{
      Token t;
      for(zStart++; sqlite3Isspace(zStart[0]); zStart++){}
      while( sqlite3Isspace(zEnd[-1]) ){ zEnd--; }
      t.z = zStart;
      t.n = (int)(zEnd - t.z);
      sqlite3ExprListSetName(pParse, pTab->pCheck, &t, 1);
    }
  }else
#endif
  {
    sqlite3ExprDelete(pParse->db, pCheckExpr);
  }
}

/*
** Set the collation function of the most recently parsed table column
** to the CollSeq given.
*/
SQLITE_PRIVATE void sqlite3AddCollateType(Parse *pParse, Token *pToken){
  Table *p;
  int i;
  char *zColl;              /* Dequoted name of collation sequence */
  sqlite3 *db;

  if( (p = pParse->pNewTable)==0 || IN_RENAME_OBJECT ) return;
  i = p->nCol-1;
  db = pParse->db;
  zColl = sqlite3NameFromToken(db, pToken);
  if( !zColl ) return;

  if( sqlite3LocateCollSeq(pParse, zColl) ){
    Index *pIdx;
    sqlite3ColumnSetColl(db, &p->aCol[i], zColl);

    /* If the column is declared as "<name> PRIMARY KEY COLLATE <type>",
    ** then an index may have been created on this column before the
    ** collation type was added. Correct this if it is the case.
    */
    for(pIdx=p->pIndex; pIdx; pIdx=pIdx->pNext){
      assert( pIdx->nKeyCol==1 );
      if( pIdx->aiColumn[0]==i ){
        pIdx->azColl[0] = sqlite3ColumnColl(&p->aCol[i]);
      }
    }
  }
  sqlite3DbFree(db, zColl);
}
