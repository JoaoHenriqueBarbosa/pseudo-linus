
/*
** Locate the in-memory structure that describes a particular database
** table given the name of that table and (optionally) the name of the
** database containing the table.  Return NULL if not found.  Also leave an
** error message in pParse->zErrMsg.
**
** The difference between this routine and sqlite3FindTable() is that this
** routine leaves an error message in pParse->zErrMsg where
** sqlite3FindTable() does not.
*/
SQLITE_PRIVATE Table *sqlite3LocateTable(
  Parse *pParse,         /* context in which to report errors */
  u32 flags,             /* LOCATE_VIEW or LOCATE_NOERR */
  const char *zName,     /* Name of the table we are looking for */
  const char *zDbase     /* Name of the database.  Might be NULL */
){
  Table *p;
  sqlite3 *db = pParse->db;

  /* Read the database schema. If an error occurs, leave an error message
  ** and code in pParse and return NULL. */
  if( (db->mDbFlags & DBFLAG_SchemaKnownOk)==0
   && SQLITE_OK!=sqlite3ReadSchema(pParse)
  ){
    return 0;
  }

  p = sqlite3FindTable(db, zName, zDbase);
  if( p==0 ){
#ifndef SQLITE_OMIT_VIRTUALTABLE
    /* If zName is the not the name of a table in the schema created using
    ** CREATE, then check to see if it is the name of an virtual table that
    ** can be an eponymous virtual table. */
    if( (pParse->prepFlags & SQLITE_PREPARE_NO_VTAB)==0 && db->init.busy==0 ){
      Module *pMod = (Module*)sqlite3HashFind(&db->aModule, zName);
      if( pMod==0 && sqlite3_strnicmp(zName, "pragma_", 7)==0 ){
        pMod = sqlite3PragmaVtabRegister(db, zName);
      }
      if( pMod && sqlite3VtabEponymousTableInit(pParse, pMod) ){
        testcase( pMod->pEpoTab==0 );
        return pMod->pEpoTab;
      }
    }
#endif
    if( flags & LOCATE_NOERR ) return 0;
    pParse->checkSchema = 1;
  }else if( IsVirtual(p) && (pParse->prepFlags & SQLITE_PREPARE_NO_VTAB)!=0 ){
    p = 0;
  }

  if( p==0 ){
    const char *zMsg = flags & LOCATE_VIEW ? "no such view" : "no such table";
    if( zDbase ){
      sqlite3ErrorMsg(pParse, "%s: %s.%s", zMsg, zDbase, zName);
    }else{
      sqlite3ErrorMsg(pParse, "%s: %s", zMsg, zName);
    }
  }else{
    assert( HasRowid(p) || p->iPKey<0 );
  }

  return p;
}

/*
** Locate the table identified by *p.
**
** This is a wrapper around sqlite3LocateTable(). The difference between
** sqlite3LocateTable() and this function is that this function restricts
** the search to schema (p->pSchema) if it is not NULL. p->pSchema may be
** non-NULL if it is part of a view or trigger program definition. See
** sqlite3FixSrcList() for details.
*/
SQLITE_PRIVATE Table *sqlite3LocateTableItem(
  Parse *pParse,
  u32 flags,
  SrcItem *p
){
  const char *zDb;
  assert( p->pSchema==0 || p->zDatabase==0 );
  if( p->pSchema ){
    int iDb = sqlite3SchemaToIndex(pParse->db, p->pSchema);
    zDb = pParse->db->aDb[iDb].zDbSName;
  }else{
    zDb = p->zDatabase;
  }
  return sqlite3LocateTable(pParse, flags, p->zName, zDb);
}

/*
** Return the preferred table name for system tables.  Translate legacy
** names into the new preferred names, as appropriate.
*/
SQLITE_PRIVATE const char *sqlite3PreferredTableName(const char *zName){
  if( sqlite3StrNICmp(zName, "sqlite_", 7)==0 ){
    if( sqlite3StrICmp(zName+7, &LEGACY_SCHEMA_TABLE[7])==0 ){
      return PREFERRED_SCHEMA_TABLE;
    }
    if( sqlite3StrICmp(zName+7, &LEGACY_TEMP_SCHEMA_TABLE[7])==0 ){
      return PREFERRED_TEMP_SCHEMA_TABLE;
    }
  }
  return zName;
}

/*
** Locate the in-memory structure that describes
** a particular index given the name of that index
** and the name of the database that contains the index.
** Return NULL if not found.
**
** If zDatabase is 0, all databases are searched for the
** table and the first matching index is returned.  (No checking
** for duplicate index names is done.)  The search order is
** TEMP first, then MAIN, then any auxiliary databases added
** using the ATTACH command.
*/
SQLITE_PRIVATE Index *sqlite3FindIndex(sqlite3 *db, const char *zName, const char *zDb){
  Index *p = 0;
  int i;
  /* All mutexes are required for schema access.  Make sure we hold them. */
  assert( zDb!=0 || sqlite3BtreeHoldsAllMutexes(db) );
  for(i=OMIT_TEMPDB; i<db->nDb; i++){
    int j = (i<2) ? i^1 : i;  /* Search TEMP before MAIN */
    Schema *pSchema = db->aDb[j].pSchema;
    assert( pSchema );
    if( zDb && sqlite3DbIsNamed(db, j, zDb)==0 ) continue;
    assert( sqlite3SchemaMutexHeld(db, j, 0) );
    p = sqlite3HashFind(&pSchema->idxHash, zName);
    if( p ) break;
  }
  return p;
}

/*
** Reclaim the memory used by an index
*/
SQLITE_PRIVATE void sqlite3FreeIndex(sqlite3 *db, Index *p){
#ifndef SQLITE_OMIT_ANALYZE
  sqlite3DeleteIndexSamples(db, p);
#endif
  sqlite3ExprDelete(db, p->pPartIdxWhere);
  sqlite3ExprListDelete(db, p->aColExpr);
  sqlite3DbFree(db, p->zColAff);
  if( p->isResized ) sqlite3DbFree(db, (void *)p->azColl);
#ifdef SQLITE_ENABLE_STAT4
  sqlite3_free(p->aiRowEst);
#endif
  sqlite3DbFree(db, p);
}

/*
** For the index called zIdxName which is found in the database iDb,
** unlike that index from its Table then remove the index from
** the index hash table and free all memory structures associated
** with the index.
*/
SQLITE_PRIVATE void sqlite3UnlinkAndDeleteIndex(sqlite3 *db, int iDb, const char *zIdxName){
  Index *pIndex;
  Hash *pHash;

  assert( sqlite3SchemaMutexHeld(db, iDb, 0) );
  pHash = &db->aDb[iDb].pSchema->idxHash;
  pIndex = sqlite3HashInsert(pHash, zIdxName, 0);
  if( ALWAYS(pIndex) ){
    if( pIndex->pTable->pIndex==pIndex ){
      pIndex->pTable->pIndex = pIndex->pNext;
    }else{
      Index *p;
      /* Justification of ALWAYS();  The index must be on the list of
      ** indices. */
      p = pIndex->pTable->pIndex;
      while( ALWAYS(p) && p->pNext!=pIndex ){ p = p->pNext; }
      if( ALWAYS(p && p->pNext==pIndex) ){
        p->pNext = pIndex->pNext;
      }
    }
    sqlite3FreeIndex(db, pIndex);
  }
  db->mDbFlags |= DBFLAG_SchemaChange;
}

/*
** Look through the list of open database files in db->aDb[] and if
** any have been closed, remove them from the list.  Reallocate the
** db->aDb[] structure to a smaller size, if possible.
**
** Entry 0 (the "main" database) and entry 1 (the "temp" database)
** are never candidates for being collapsed.
*/
SQLITE_PRIVATE void sqlite3CollapseDatabaseArray(sqlite3 *db){
  int i, j;
  for(i=j=2; i<db->nDb; i++){
    struct Db *pDb = &db->aDb[i];
    if( pDb->pBt==0 ){
      sqlite3DbFree(db, pDb->zDbSName);
      pDb->zDbSName = 0;
      continue;
    }
    if( j<i ){
      db->aDb[j] = db->aDb[i];
    }
    j++;
  }
  db->nDb = j;
  if( db->nDb<=2 && db->aDb!=db->aDbStatic ){
    memcpy(db->aDbStatic, db->aDb, 2*sizeof(db->aDb[0]));
    sqlite3DbFree(db, db->aDb);
    db->aDb = db->aDbStatic;
  }
}

/*
** Reset the schema for the database at index iDb.  Also reset the
** TEMP schema.  The reset is deferred if db->nSchemaLock is not zero.
** Deferred resets may be run by calling with iDb<0.
*/
SQLITE_PRIVATE void sqlite3ResetOneSchema(sqlite3 *db, int iDb){
  int i;
  assert( iDb<db->nDb );

  if( iDb>=0 ){
    assert( sqlite3SchemaMutexHeld(db, iDb, 0) );
    DbSetProperty(db, iDb, DB_ResetWanted);
    DbSetProperty(db, 1, DB_ResetWanted);
    db->mDbFlags &= ~DBFLAG_SchemaKnownOk;
  }

  if( db->nSchemaLock==0 ){
    for(i=0; i<db->nDb; i++){
      if( DbHasProperty(db, i, DB_ResetWanted) ){
        sqlite3SchemaClear(db->aDb[i].pSchema);
      }
    }
  }
}

/*
** Erase all schema information from all attached databases (including
** "main" and "temp") for a single database connection.
*/
SQLITE_PRIVATE void sqlite3ResetAllSchemasOfConnection(sqlite3 *db){
  int i;
  sqlite3BtreeEnterAll(db);
  for(i=0; i<db->nDb; i++){
    Db *pDb = &db->aDb[i];
    if( pDb->pSchema ){
      if( db->nSchemaLock==0 ){
        sqlite3SchemaClear(pDb->pSchema);
      }else{
        DbSetProperty(db, i, DB_ResetWanted);
      }
    }
  }
  db->mDbFlags &= ~(DBFLAG_SchemaChange|DBFLAG_SchemaKnownOk);
  sqlite3VtabUnlockList(db);
  sqlite3BtreeLeaveAll(db);
  if( db->nSchemaLock==0 ){
    sqlite3CollapseDatabaseArray(db);
  }
}

/*
** This routine is called when a commit occurs.
*/
SQLITE_PRIVATE void sqlite3CommitInternalChanges(sqlite3 *db){
  db->mDbFlags &= ~DBFLAG_SchemaChange;
}

/*
** Set the expression associated with a column.  This is usually
** the DEFAULT value, but might also be the expression that computes
** the value for a generated column.
*/
SQLITE_PRIVATE void sqlite3ColumnSetExpr(
  Parse *pParse,    /* Parsing context */
  Table *pTab,      /* The table containing the column */
  Column *pCol,     /* The column to receive the new DEFAULT expression */
  Expr *pExpr       /* The new default expression */
){
  ExprList *pList;
  assert( IsOrdinaryTable(pTab) );
  pList = pTab->u.tab.pDfltList;
  if( pCol->iDflt==0
   || NEVER(pList==0)
   || NEVER(pList->nExpr<pCol->iDflt)
  ){
    pCol->iDflt = pList==0 ? 1 : pList->nExpr+1;
    pTab->u.tab.pDfltList = sqlite3ExprListAppend(pParse, pList, pExpr);
  }else{
    sqlite3ExprDelete(pParse->db, pList->a[pCol->iDflt-1].pExpr);
    pList->a[pCol->iDflt-1].pExpr = pExpr;
  }
}

/*
** Return the expression associated with a column.  The expression might be
** the DEFAULT clause or the AS clause of a generated column.
** Return NULL if the column has no associated expression.
*/
SQLITE_PRIVATE Expr *sqlite3ColumnExpr(Table *pTab, Column *pCol){
  if( pCol->iDflt==0 ) return 0;
  if( !IsOrdinaryTable(pTab) ) return 0;
  if( NEVER(pTab->u.tab.pDfltList==0) ) return 0;
  if( NEVER(pTab->u.tab.pDfltList->nExpr<pCol->iDflt) ) return 0;
  return pTab->u.tab.pDfltList->a[pCol->iDflt-1].pExpr;
}

/*
** Set the collating sequence name for a column.
*/
SQLITE_PRIVATE void sqlite3ColumnSetColl(
  sqlite3 *db,
  Column *pCol,
  const char *zColl
){
  i64 nColl;
  i64 n;
  char *zNew;
  assert( zColl!=0 );
  n = sqlite3Strlen30(pCol->zCnName) + 1;
  if( pCol->colFlags & COLFLAG_HASTYPE ){
    n += sqlite3Strlen30(pCol->zCnName+n) + 1;
  }
  nColl = sqlite3Strlen30(zColl) + 1;
  zNew = sqlite3DbRealloc(db, pCol->zCnName, nColl+n);
  if( zNew ){
    pCol->zCnName = zNew;
    memcpy(pCol->zCnName + n, zColl, nColl);
    pCol->colFlags |= COLFLAG_HASCOLL;
  }
}

/*
** Return the collating sequence name for a column
*/
SQLITE_PRIVATE const char *sqlite3ColumnColl(Column *pCol){
  const char *z;
  if( (pCol->colFlags & COLFLAG_HASCOLL)==0 ) return 0;
  z = pCol->zCnName;
  while( *z ){ z++; }
  if( pCol->colFlags & COLFLAG_HASTYPE ){
    do{ z++; }while( *z );
  }
  return z+1;
}

/*
** Delete memory allocated for the column names of a table or view (the
** Table.aCol[] array).
*/
SQLITE_PRIVATE void sqlite3DeleteColumnNames(sqlite3 *db, Table *pTable){
  int i;
  Column *pCol;
  assert( pTable!=0 );
  assert( db!=0 );
  if( (pCol = pTable->aCol)!=0 ){
    for(i=0; i<pTable->nCol; i++, pCol++){
      assert( pCol->zCnName==0 || pCol->hName==sqlite3StrIHash(pCol->zCnName) );
      sqlite3DbFree(db, pCol->zCnName);
    }
    sqlite3DbNNFreeNN(db, pTable->aCol);
    if( IsOrdinaryTable(pTable) ){
      sqlite3ExprListDelete(db, pTable->u.tab.pDfltList);
    }
    if( db->pnBytesFreed==0 ){
      pTable->aCol = 0;
      pTable->nCol = 0;
      if( IsOrdinaryTable(pTable) ){
        pTable->u.tab.pDfltList = 0;
      }
    }
  }
}
