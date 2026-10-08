
/*
** Remove entries from the sqlite_statN tables (for N in (1,2,3))
** after a DROP INDEX or DROP TABLE command.
*/
static void sqlite3ClearStatTables(
  Parse *pParse,         /* The parsing context */
  int iDb,               /* The database number */
  const char *zType,     /* "idx" or "tbl" */
  const char *zName      /* Name of index or table */
){
  int i;
  const char *zDbName = pParse->db->aDb[iDb].zDbSName;
  for(i=1; i<=4; i++){
    char zTab[24];
    sqlite3_snprintf(sizeof(zTab),zTab,"sqlite_stat%d",i);
    if( sqlite3FindTable(pParse->db, zTab, zDbName) ){
      sqlite3NestedParse(pParse,
        "DELETE FROM %Q.%s WHERE %s=%Q",
        zDbName, zTab, zType, zName
      );
    }
  }
}

/*
** Generate code to drop a table.
*/
SQLITE_PRIVATE void sqlite3CodeDropTable(Parse *pParse, Table *pTab, int iDb, int isView){
  Vdbe *v;
  sqlite3 *db = pParse->db;
  Trigger *pTrigger;
  Db *pDb = &db->aDb[iDb];

  v = sqlite3GetVdbe(pParse);
  assert( v!=0 );
  sqlite3BeginWriteOperation(pParse, 1, iDb);

#ifndef SQLITE_OMIT_VIRTUALTABLE
  if( IsVirtual(pTab) ){
    sqlite3VdbeAddOp0(v, OP_VBegin);
  }
#endif

  /* Drop all triggers associated with the table being dropped. Code
  ** is generated to remove entries from sqlite_schema and/or
  ** sqlite_temp_schema if required.
  */
  pTrigger = sqlite3TriggerList(pParse, pTab);
  while( pTrigger ){
    assert( pTrigger->pSchema==pTab->pSchema ||
        pTrigger->pSchema==db->aDb[1].pSchema );
    sqlite3DropTriggerPtr(pParse, pTrigger);
    pTrigger = pTrigger->pNext;
  }

#ifndef SQLITE_OMIT_AUTOINCREMENT
  /* Remove any entries of the sqlite_sequence table associated with
  ** the table being dropped. This is done before the table is dropped
  ** at the btree level, in case the sqlite_sequence table needs to
  ** move as a result of the drop (can happen in auto-vacuum mode).
  */
  if( pTab->tabFlags & TF_Autoincrement ){
    sqlite3NestedParse(pParse,
      "DELETE FROM %Q.sqlite_sequence WHERE name=%Q",
      pDb->zDbSName, pTab->zName
    );
  }
#endif

  /* Drop all entries in the schema table that refer to the
  ** table. The program name loops through the schema table and deletes
  ** every row that refers to a table of the same name as the one being
  ** dropped. Triggers are handled separately because a trigger can be
  ** created in the temp database that refers to a table in another
  ** database.
  */
  sqlite3NestedParse(pParse,
      "DELETE FROM %Q." LEGACY_SCHEMA_TABLE
      " WHERE tbl_name=%Q and type!='trigger'",
      pDb->zDbSName, pTab->zName);
  if( !isView && !IsVirtual(pTab) ){
    destroyTable(pParse, pTab);
  }

  /* Remove the table entry from SQLite's internal schema and modify
  ** the schema cookie.
  */
  if( IsVirtual(pTab) ){
    sqlite3VdbeAddOp4(v, OP_VDestroy, iDb, 0, 0, pTab->zName, 0);
    sqlite3MayAbort(pParse);
  }
  sqlite3VdbeAddOp4(v, OP_DropTable, iDb, 0, 0, pTab->zName, 0);
  sqlite3ChangeCookie(pParse, iDb);
  sqliteViewResetAll(db, iDb);
}

/*
** Return TRUE if shadow tables should be read-only in the current
** context.
*/
SQLITE_PRIVATE int sqlite3ReadOnlyShadowTables(sqlite3 *db){
#ifndef SQLITE_OMIT_VIRTUALTABLE
  if( (db->flags & SQLITE_Defensive)!=0
   && db->pVtabCtx==0
   && db->nVdbeExec==0
   && !sqlite3VtabInSync(db)
  ){
    return 1;
  }
#endif
  return 0;
}

/*
** Return true if it is not allowed to drop the given table
*/
static int tableMayNotBeDropped(sqlite3 *db, Table *pTab){
  if( sqlite3StrNICmp(pTab->zName, "sqlite_", 7)==0 ){
    if( sqlite3StrNICmp(pTab->zName+7, "stat", 4)==0 ) return 0;
    if( sqlite3StrNICmp(pTab->zName+7, "parameters", 10)==0 ) return 0;
    return 1;
  }
  if( (pTab->tabFlags & TF_Shadow)!=0 && sqlite3ReadOnlyShadowTables(db) ){
    return 1;
  }
  if( pTab->tabFlags & TF_Eponymous ){
    return 1;
  }
  return 0;
}

/*
** This routine is called to do the work of a DROP TABLE statement.
** pName is the name of the table to be dropped.
*/
SQLITE_PRIVATE void sqlite3DropTable(Parse *pParse, SrcList *pName, int isView, int noErr){
  Table *pTab;
  Vdbe *v;
  sqlite3 *db = pParse->db;
  int iDb;

  if( db->mallocFailed ){
    goto exit_drop_table;
  }
  assert( pParse->nErr==0 );
  assert( pName->nSrc==1 );
  if( sqlite3ReadSchema(pParse) ) goto exit_drop_table;
  if( noErr ) db->suppressErr++;
  assert( isView==0 || isView==LOCATE_VIEW );
  pTab = sqlite3LocateTableItem(pParse, isView, &pName->a[0]);
  if( noErr ) db->suppressErr--;

  if( pTab==0 ){
    if( noErr ){
      sqlite3CodeVerifyNamedSchema(pParse, pName->a[0].zDatabase);
      sqlite3ForceNotReadOnly(pParse);
    }
    goto exit_drop_table;
  }
  iDb = sqlite3SchemaToIndex(db, pTab->pSchema);
  assert( iDb>=0 && iDb<db->nDb );

  /* If pTab is a virtual table, call ViewGetColumnNames() to ensure
  ** it is initialized.
  */
  if( IsVirtual(pTab) && sqlite3ViewGetColumnNames(pParse, pTab) ){
    goto exit_drop_table;
  }
#ifndef SQLITE_OMIT_AUTHORIZATION
  {
    int code;
    const char *zTab = SCHEMA_TABLE(iDb);
    const char *zDb = db->aDb[iDb].zDbSName;
    const char *zArg2 = 0;
    if( sqlite3AuthCheck(pParse, SQLITE_DELETE, zTab, 0, zDb)){
      goto exit_drop_table;
    }
    if( isView ){
      if( !OMIT_TEMPDB && iDb==1 ){
        code = SQLITE_DROP_TEMP_VIEW;
      }else{
        code = SQLITE_DROP_VIEW;
      }
#ifndef SQLITE_OMIT_VIRTUALTABLE
    }else if( IsVirtual(pTab) ){
      code = SQLITE_DROP_VTABLE;
      zArg2 = sqlite3GetVTable(db, pTab)->pMod->zName;
#endif
    }else{
      if( !OMIT_TEMPDB && iDb==1 ){
        code = SQLITE_DROP_TEMP_TABLE;
      }else{
        code = SQLITE_DROP_TABLE;
      }
    }
    if( sqlite3AuthCheck(pParse, code, pTab->zName, zArg2, zDb) ){
      goto exit_drop_table;
    }
    if( sqlite3AuthCheck(pParse, SQLITE_DELETE, pTab->zName, 0, zDb) ){
      goto exit_drop_table;
    }
  }
#endif
  if( tableMayNotBeDropped(db, pTab) ){
    sqlite3ErrorMsg(pParse, "table %s may not be dropped", pTab->zName);
    goto exit_drop_table;
  }

#ifndef SQLITE_OMIT_VIEW
  /* Ensure DROP TABLE is not used on a view, and DROP VIEW is not used
  ** on a table.
  */
  if( isView && !IsView(pTab) ){
    sqlite3ErrorMsg(pParse, "use DROP TABLE to delete table %s", pTab->zName);
    goto exit_drop_table;
  }
  if( !isView && IsView(pTab) ){
    sqlite3ErrorMsg(pParse, "use DROP VIEW to delete view %s", pTab->zName);
    goto exit_drop_table;
  }
#endif

  /* Generate code to remove the table from the schema table
  ** on disk.
  */
  v = sqlite3GetVdbe(pParse);
  if( v ){
    sqlite3BeginWriteOperation(pParse, 1, iDb);
    if( !isView ){
      sqlite3ClearStatTables(pParse, iDb, "tbl", pTab->zName);
      sqlite3FkDropTable(pParse, pName, pTab);
    }
    sqlite3CodeDropTable(pParse, pTab, iDb, isView);
  }

exit_drop_table:
  sqlite3SrcListDelete(db, pName);
}

/*
** This routine is called to create a new foreign key on the table
** currently under construction.  pFromCol determines which columns
** in the current table point to the foreign key.  If pFromCol==0 then
** connect the key to the last column inserted.  pTo is the name of
** the table referred to (a.k.a the "parent" table).  pToCol is a list
** of tables in the parent pTo table.  flags contains all
** information about the conflict resolution algorithms specified
** in the ON DELETE, ON UPDATE and ON INSERT clauses.
**
** An FKey structure is created and added to the table currently
** under construction in the pParse->pNewTable field.
**
** The foreign key is set for IMMEDIATE processing.  A subsequent call
** to sqlite3DeferForeignKey() might change this to DEFERRED.
*/
SQLITE_PRIVATE void sqlite3CreateForeignKey(
  Parse *pParse,       /* Parsing context */
  ExprList *pFromCol,  /* Columns in this table that point to other table */
  Token *pTo,          /* Name of the other table */
  ExprList *pToCol,    /* Columns in the other table */
  int flags            /* Conflict resolution algorithms. */
){
  sqlite3 *db = pParse->db;
#ifndef SQLITE_OMIT_FOREIGN_KEY
  FKey *pFKey = 0;
  FKey *pNextTo;
  Table *p = pParse->pNewTable;
  i64 nByte;
  int i;
  int nCol;
  char *z;

  assert( pTo!=0 );
  if( p==0 || IN_DECLARE_VTAB ) goto fk_end;
  if( pFromCol==0 ){
    int iCol = p->nCol-1;
    if( NEVER(iCol<0) ) goto fk_end;
    if( pToCol && pToCol->nExpr!=1 ){
      sqlite3ErrorMsg(pParse, "foreign key on %s"
         " should reference only one column of table %T",
         p->aCol[iCol].zCnName, pTo);
      goto fk_end;
    }
    nCol = 1;
  }else if( pToCol && pToCol->nExpr!=pFromCol->nExpr ){
    sqlite3ErrorMsg(pParse,
        "number of columns in foreign key does not match the number of "
        "columns in the referenced table");
    goto fk_end;
  }else{
    nCol = pFromCol->nExpr;
  }
  nByte = sizeof(*pFKey) + (nCol-1)*sizeof(pFKey->aCol[0]) + pTo->n + 1;
  if( pToCol ){
    for(i=0; i<pToCol->nExpr; i++){
      nByte += sqlite3Strlen30(pToCol->a[i].zEName) + 1;
    }
  }
  pFKey = sqlite3DbMallocZero(db, nByte );
  if( pFKey==0 ){
    goto fk_end;
  }
  pFKey->pFrom = p;
  assert( IsOrdinaryTable(p) );
  pFKey->pNextFrom = p->u.tab.pFKey;
  z = (char*)&pFKey->aCol[nCol];
  pFKey->zTo = z;
  if( IN_RENAME_OBJECT ){
    sqlite3RenameTokenMap(pParse, (void*)z, pTo);
  }
  memcpy(z, pTo->z, pTo->n);
  z[pTo->n] = 0;
  sqlite3Dequote(z);
  z += pTo->n+1;
  pFKey->nCol = nCol;
  if( pFromCol==0 ){
    pFKey->aCol[0].iFrom = p->nCol-1;
  }else{
    for(i=0; i<nCol; i++){
      int j;
      for(j=0; j<p->nCol; j++){
        if( sqlite3StrICmp(p->aCol[j].zCnName, pFromCol->a[i].zEName)==0 ){
          pFKey->aCol[i].iFrom = j;
          break;
        }
      }
      if( j>=p->nCol ){
        sqlite3ErrorMsg(pParse,
          "unknown column \"%s\" in foreign key definition",
          pFromCol->a[i].zEName);
        goto fk_end;
      }
      if( IN_RENAME_OBJECT ){
        sqlite3RenameTokenRemap(pParse, &pFKey->aCol[i], pFromCol->a[i].zEName);
      }
    }
  }
  if( pToCol ){
    for(i=0; i<nCol; i++){
      int n = sqlite3Strlen30(pToCol->a[i].zEName);
      pFKey->aCol[i].zCol = z;
      if( IN_RENAME_OBJECT ){
        sqlite3RenameTokenRemap(pParse, z, pToCol->a[i].zEName);
      }
      memcpy(z, pToCol->a[i].zEName, n);
      z[n] = 0;
      z += n+1;
    }
  }
  pFKey->isDeferred = 0;
  pFKey->aAction[0] = (u8)(flags & 0xff);            /* ON DELETE action */
  pFKey->aAction[1] = (u8)((flags >> 8 ) & 0xff);    /* ON UPDATE action */

  assert( sqlite3SchemaMutexHeld(db, 0, p->pSchema) );
  pNextTo = (FKey *)sqlite3HashInsert(&p->pSchema->fkeyHash,
      pFKey->zTo, (void *)pFKey
  );
  if( pNextTo==pFKey ){
    sqlite3OomFault(db);
    goto fk_end;
  }
  if( pNextTo ){
    assert( pNextTo->pPrevTo==0 );
    pFKey->pNextTo = pNextTo;
    pNextTo->pPrevTo = pFKey;
  }

  /* Link the foreign key to the table as the last step.
  */
  assert( IsOrdinaryTable(p) );
  p->u.tab.pFKey = pFKey;
  pFKey = 0;

fk_end:
  sqlite3DbFree(db, pFKey);
#endif /* !defined(SQLITE_OMIT_FOREIGN_KEY) */
  sqlite3ExprListDelete(db, pFromCol);
  sqlite3ExprListDelete(db, pToCol);
}
