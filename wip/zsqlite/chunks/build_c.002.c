
/*
** Remove the memory data structures associated with the given
** Table.  No changes are made to disk by this routine.
**
** This routine just deletes the data structure.  It does not unlink
** the table data structure from the hash table.  But it does destroy
** memory structures of the indices and foreign keys associated with
** the table.
**
** The db parameter is optional.  It is needed if the Table object
** contains lookaside memory.  (Table objects in the schema do not use
** lookaside memory, but some ephemeral Table objects do.)  Or the
** db parameter can be used with db->pnBytesFreed to measure the memory
** used by the Table object.
*/
static void SQLITE_NOINLINE deleteTable(sqlite3 *db, Table *pTable){
  Index *pIndex, *pNext;

#ifdef SQLITE_DEBUG
  /* Record the number of outstanding lookaside allocations in schema Tables
  ** prior to doing any free() operations. Since schema Tables do not use
  ** lookaside, this number should not change.
  **
  ** If malloc has already failed, it may be that it failed while allocating
  ** a Table object that was going to be marked ephemeral. So do not check
  ** that no lookaside memory is used in this case either. */
  int nLookaside = 0;
  assert( db!=0 );
  if( !db->mallocFailed && (pTable->tabFlags & TF_Ephemeral)==0 ){
    nLookaside = sqlite3LookasideUsed(db, 0);
  }
#endif

  /* Delete all indices associated with this table. */
  for(pIndex = pTable->pIndex; pIndex; pIndex=pNext){
    pNext = pIndex->pNext;
    assert( pIndex->pSchema==pTable->pSchema
         || (IsVirtual(pTable) && pIndex->idxType!=SQLITE_IDXTYPE_APPDEF) );
    if( db->pnBytesFreed==0 && !IsVirtual(pTable) ){
      char *zName = pIndex->zName;
      TESTONLY ( Index *pOld = ) sqlite3HashInsert(
         &pIndex->pSchema->idxHash, zName, 0
      );
      assert( db==0 || sqlite3SchemaMutexHeld(db, 0, pIndex->pSchema) );
      assert( pOld==pIndex || pOld==0 );
    }
    sqlite3FreeIndex(db, pIndex);
  }

  if( IsOrdinaryTable(pTable) ){
    sqlite3FkDelete(db, pTable);
  }
#ifndef SQLITE_OMIT_VIRTUALTABLE
  else if( IsVirtual(pTable) ){
    sqlite3VtabClear(db, pTable);
  }
#endif
  else{
    assert( IsView(pTable) );
    sqlite3SelectDelete(db, pTable->u.view.pSelect);
  }

  /* Delete the Table structure itself.
  */
  sqlite3DeleteColumnNames(db, pTable);
  sqlite3DbFree(db, pTable->zName);
  sqlite3DbFree(db, pTable->zColAff);
  sqlite3ExprListDelete(db, pTable->pCheck);
  sqlite3DbFree(db, pTable);

  /* Verify that no lookaside memory was used by schema tables */
  assert( nLookaside==0 || nLookaside==sqlite3LookasideUsed(db,0) );
}
SQLITE_PRIVATE void sqlite3DeleteTable(sqlite3 *db, Table *pTable){
  /* Do not delete the table until the reference count reaches zero. */
  assert( db!=0 );
  if( !pTable ) return;
  if( db->pnBytesFreed==0 && (--pTable->nTabRef)>0 ) return;
  deleteTable(db, pTable);
}
SQLITE_PRIVATE void sqlite3DeleteTableGeneric(sqlite3 *db, void *pTable){
  sqlite3DeleteTable(db, (Table*)pTable);
}


/*
** Unlink the given table from the hash tables and the delete the
** table structure with all its indices and foreign keys.
*/
SQLITE_PRIVATE void sqlite3UnlinkAndDeleteTable(sqlite3 *db, int iDb, const char *zTabName){
  Table *p;
  Db *pDb;

  assert( db!=0 );
  assert( iDb>=0 && iDb<db->nDb );
  assert( zTabName );
  assert( sqlite3SchemaMutexHeld(db, iDb, 0) );
  testcase( zTabName[0]==0 );  /* Zero-length table names are allowed */
  pDb = &db->aDb[iDb];
  p = sqlite3HashInsert(&pDb->pSchema->tblHash, zTabName, 0);
  sqlite3DeleteTable(db, p);
  db->mDbFlags |= DBFLAG_SchemaChange;
}

/*
** Given a token, return a string that consists of the text of that
** token.  Space to hold the returned string
** is obtained from sqliteMalloc() and must be freed by the calling
** function.
**
** Any quotation marks (ex:  "name", 'name', [name], or `name`) that
** surround the body of the token are removed.
**
** Tokens are often just pointers into the original SQL text and so
** are not \000 terminated and are not persistent.  The returned string
** is \000 terminated and is persistent.
*/
SQLITE_PRIVATE char *sqlite3NameFromToken(sqlite3 *db, const Token *pName){
  char *zName;
  if( pName ){
    zName = sqlite3DbStrNDup(db, (const char*)pName->z, pName->n);
    sqlite3Dequote(zName);
  }else{
    zName = 0;
  }
  return zName;
}

/*
** Open the sqlite_schema table stored in database number iDb for
** writing. The table is opened using cursor 0.
*/
SQLITE_PRIVATE void sqlite3OpenSchemaTable(Parse *p, int iDb){
  Vdbe *v = sqlite3GetVdbe(p);
  sqlite3TableLock(p, iDb, SCHEMA_ROOT, 1, LEGACY_SCHEMA_TABLE);
  sqlite3VdbeAddOp4Int(v, OP_OpenWrite, 0, SCHEMA_ROOT, iDb, 5);
  if( p->nTab==0 ){
    p->nTab = 1;
  }
}

/*
** Parameter zName points to a nul-terminated buffer containing the name
** of a database ("main", "temp" or the name of an attached db). This
** function returns the index of the named database in db->aDb[], or
** -1 if the named db cannot be found.
*/
SQLITE_PRIVATE int sqlite3FindDbName(sqlite3 *db, const char *zName){
  int i = -1;         /* Database number */
  if( zName ){
    Db *pDb;
    for(i=(db->nDb-1), pDb=&db->aDb[i]; i>=0; i--, pDb--){
      if( 0==sqlite3_stricmp(pDb->zDbSName, zName) ) break;
      /* "main" is always an acceptable alias for the primary database
      ** even if it has been renamed using SQLITE_DBCONFIG_MAINDBNAME. */
      if( i==0 && 0==sqlite3_stricmp("main", zName) ) break;
    }
  }
  return i;
}

/*
** The token *pName contains the name of a database (either "main" or
** "temp" or the name of an attached db). This routine returns the
** index of the named database in db->aDb[], or -1 if the named db
** does not exist.
*/
SQLITE_PRIVATE int sqlite3FindDb(sqlite3 *db, Token *pName){
  int i;                               /* Database number */
  char *zName;                         /* Name we are searching for */
  zName = sqlite3NameFromToken(db, pName);
  i = sqlite3FindDbName(db, zName);
  sqlite3DbFree(db, zName);
  return i;
}

/* The table or view or trigger name is passed to this routine via tokens
** pName1 and pName2. If the table name was fully qualified, for example:
**
** CREATE TABLE xxx.yyy (...);
**
** Then pName1 is set to "xxx" and pName2 "yyy". On the other hand if
** the table name is not fully qualified, i.e.:
**
** CREATE TABLE yyy(...);
**
** Then pName1 is set to "yyy" and pName2 is "".
**
** This routine sets the *ppUnqual pointer to point at the token (pName1 or
** pName2) that stores the unqualified table name.  The index of the
** database "xxx" is returned.
*/
SQLITE_PRIVATE int sqlite3TwoPartName(
  Parse *pParse,      /* Parsing and code generating context */
  Token *pName1,      /* The "xxx" in the name "xxx.yyy" or "xxx" */
  Token *pName2,      /* The "yyy" in the name "xxx.yyy" */
  Token **pUnqual     /* Write the unqualified object name here */
){
  int iDb;                    /* Database holding the object */
  sqlite3 *db = pParse->db;

  assert( pName2!=0 );
  if( pName2->n>0 ){
    if( db->init.busy ) {
      sqlite3ErrorMsg(pParse, "corrupt database");
      return -1;
    }
    *pUnqual = pName2;
    iDb = sqlite3FindDb(db, pName1);
    if( iDb<0 ){
      sqlite3ErrorMsg(pParse, "unknown database %T", pName1);
      return -1;
    }
  }else{
    assert( db->init.iDb==0 || db->init.busy || IN_SPECIAL_PARSE
             || (db->mDbFlags & DBFLAG_Vacuum)!=0);
    iDb = db->init.iDb;
    *pUnqual = pName1;
  }
  return iDb;
}

/*
** True if PRAGMA writable_schema is ON
*/
SQLITE_PRIVATE int sqlite3WritableSchema(sqlite3 *db){
  testcase( (db->flags&(SQLITE_WriteSchema|SQLITE_Defensive))==0 );
  testcase( (db->flags&(SQLITE_WriteSchema|SQLITE_Defensive))==
               SQLITE_WriteSchema );
  testcase( (db->flags&(SQLITE_WriteSchema|SQLITE_Defensive))==
               SQLITE_Defensive );
  testcase( (db->flags&(SQLITE_WriteSchema|SQLITE_Defensive))==
               (SQLITE_WriteSchema|SQLITE_Defensive) );
  return (db->flags&(SQLITE_WriteSchema|SQLITE_Defensive))==SQLITE_WriteSchema;
}

/*
** This routine is used to check if the UTF-8 string zName is a legal
** unqualified name for a new schema object (table, index, view or
** trigger). All names are legal except those that begin with the string
** "sqlite_" (in upper, lower or mixed case). This portion of the namespace
** is reserved for internal use.
**
** When parsing the sqlite_schema table, this routine also checks to
** make sure the "type", "name", and "tbl_name" columns are consistent
** with the SQL.
*/
SQLITE_PRIVATE int sqlite3CheckObjectName(
  Parse *pParse,            /* Parsing context */
  const char *zName,        /* Name of the object to check */
  const char *zType,        /* Type of this object */
  const char *zTblName      /* Parent table name for triggers and indexes */
){
  sqlite3 *db = pParse->db;
  if( sqlite3WritableSchema(db)
   || db->init.imposterTable
   || !sqlite3Config.bExtraSchemaChecks
  ){
    /* Skip these error checks for writable_schema=ON */
    return SQLITE_OK;
  }
  if( db->init.busy ){
    if( sqlite3_stricmp(zType, db->init.azInit[0])
     || sqlite3_stricmp(zName, db->init.azInit[1])
     || sqlite3_stricmp(zTblName, db->init.azInit[2])
    ){
      sqlite3ErrorMsg(pParse, ""); /* corruptSchema() will supply the error */
      return SQLITE_ERROR;
    }
  }else{
    if( (pParse->nested==0 && 0==sqlite3StrNICmp(zName, "sqlite_", 7))
     || (sqlite3ReadOnlyShadowTables(db) && sqlite3ShadowTableName(db, zName))
    ){
      sqlite3ErrorMsg(pParse, "object name reserved for internal use: %s",
                      zName);
      return SQLITE_ERROR;
    }

  }
  return SQLITE_OK;
}

/*
** Return the PRIMARY KEY index of a table
*/
SQLITE_PRIVATE Index *sqlite3PrimaryKeyIndex(Table *pTab){
  Index *p;
  for(p=pTab->pIndex; p && !IsPrimaryKeyIndex(p); p=p->pNext){}
  return p;
}

/*
** Convert an table column number into a index column number.  That is,
** for the column iCol in the table (as defined by the CREATE TABLE statement)
** find the (first) offset of that column in index pIdx.  Or return -1
** if column iCol is not used in index pIdx.
*/
SQLITE_PRIVATE i16 sqlite3TableColumnToIndex(Index *pIdx, i16 iCol){
  int i;
  for(i=0; i<pIdx->nColumn; i++){
    if( iCol==pIdx->aiColumn[i] ) return i;
  }
  return -1;
}

#ifndef SQLITE_OMIT_GENERATED_COLUMNS
/* Convert a storage column number into a table column number.
**
** The storage column number (0,1,2,....) is the index of the value
** as it appears in the record on disk.  The true column number
** is the index (0,1,2,...) of the column in the CREATE TABLE statement.
**
** The storage column number is less than the table column number if
** and only there are VIRTUAL columns to the left.
**
** If SQLITE_OMIT_GENERATED_COLUMNS, this routine is a no-op macro.
*/
SQLITE_PRIVATE i16 sqlite3StorageColumnToTable(Table *pTab, i16 iCol){
  if( pTab->tabFlags & TF_HasVirtual ){
    int i;
    for(i=0; i<=iCol; i++){
      if( pTab->aCol[i].colFlags & COLFLAG_VIRTUAL ) iCol++;
    }
  }
  return iCol;
}
#endif

#ifndef SQLITE_OMIT_GENERATED_COLUMNS
/* Convert a table column number into a storage column number.
**
** The storage column number (0,1,2,....) is the index of the value
** as it appears in the record on disk.  Or, if the input column is
** the N-th virtual column (zero-based) then the storage number is
** the number of non-virtual columns in the table plus N.
**
** The true column number is the index (0,1,2,...) of the column in
** the CREATE TABLE statement.
**
** If the input column is a VIRTUAL column, then it should not appear
** in storage.  But the value sometimes is cached in registers that
** follow the range of registers used to construct storage.  This
** avoids computing the same VIRTUAL column multiple times, and provides
** values for use by OP_Param opcodes in triggers.  Hence, if the
** input column is a VIRTUAL table, put it after all the other columns.
**
** In the following, N means "normal column", S means STORED, and
** V means VIRTUAL.  Suppose the CREATE TABLE has columns like this:
**
**        CREATE TABLE ex(N,S,V,N,S,V,N,S,V);
**                     -- 0 1 2 3 4 5 6 7 8
**
** Then the mapping from this function is as follows:
**
**    INPUTS:     0 1 2 3 4 5 6 7 8
**    OUTPUTS:    0 1 6 2 3 7 4 5 8
**
** So, in other words, this routine shifts all the virtual columns to
** the end.
**
** If SQLITE_OMIT_GENERATED_COLUMNS then there are no virtual columns and
** this routine is a no-op macro.  If the pTab does not have any virtual
** columns, then this routine is no-op that always return iCol.  If iCol
** is negative (indicating the ROWID column) then this routine return iCol.
*/
SQLITE_PRIVATE i16 sqlite3TableColumnToStorage(Table *pTab, i16 iCol){
  int i;
  i16 n;
  assert( iCol<pTab->nCol );
  if( (pTab->tabFlags & TF_HasVirtual)==0 || iCol<0 ) return iCol;
  for(i=0, n=0; i<iCol; i++){
    if( (pTab->aCol[i].colFlags & COLFLAG_VIRTUAL)==0 ) n++;
  }
  if( pTab->aCol[i].colFlags & COLFLAG_VIRTUAL ){
    /* iCol is a virtual column itself */
    return pTab->nNVCol + i - n;
  }else{
    /* iCol is a normal or stored column */
    return n;
  }
}
