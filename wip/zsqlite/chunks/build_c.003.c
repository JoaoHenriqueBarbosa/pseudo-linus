#endif

/*
** Insert a single OP_JournalMode query opcode in order to force the
** prepared statement to return false for sqlite3_stmt_readonly().  This
** is used by CREATE TABLE IF NOT EXISTS and similar if the table already
** exists, so that the prepared statement for CREATE TABLE IF NOT EXISTS
** will return false for sqlite3_stmt_readonly() even if that statement
** is a read-only no-op.
*/
static void sqlite3ForceNotReadOnly(Parse *pParse){
  int iReg = ++pParse->nMem;
  Vdbe *v = sqlite3GetVdbe(pParse);
  if( v ){
    sqlite3VdbeAddOp3(v, OP_JournalMode, 0, iReg, PAGER_JOURNALMODE_QUERY);
    sqlite3VdbeUsesBtree(v, 0);
  }
}

/*
** Begin constructing a new table representation in memory.  This is
** the first of several action routines that get called in response
** to a CREATE TABLE statement.  In particular, this routine is called
** after seeing tokens "CREATE" and "TABLE" and the table name. The isTemp
** flag is true if the table should be stored in the auxiliary database
** file instead of in the main database file.  This is normally the case
** when the "TEMP" or "TEMPORARY" keyword occurs in between
** CREATE and TABLE.
**
** The new table record is initialized and put in pParse->pNewTable.
** As more of the CREATE TABLE statement is parsed, additional action
** routines will be called to add more information to this record.
** At the end of the CREATE TABLE statement, the sqlite3EndTable() routine
** is called to complete the construction of the new table record.
*/
SQLITE_PRIVATE void sqlite3StartTable(
  Parse *pParse,   /* Parser context */
  Token *pName1,   /* First part of the name of the table or view */
  Token *pName2,   /* Second part of the name of the table or view */
  int isTemp,      /* True if this is a TEMP table */
  int isView,      /* True if this is a VIEW */
  int isVirtual,   /* True if this is a VIRTUAL table */
  int noErr        /* Do nothing if table already exists */
){
  Table *pTable;
  char *zName = 0; /* The name of the new table */
  sqlite3 *db = pParse->db;
  Vdbe *v;
  int iDb;         /* Database number to create the table in */
  Token *pName;    /* Unqualified name of the table to create */

  if( db->init.busy && db->init.newTnum==1 ){
    /* Special case:  Parsing the sqlite_schema or sqlite_temp_schema schema */
    iDb = db->init.iDb;
    zName = sqlite3DbStrDup(db, SCHEMA_TABLE(iDb));
    pName = pName1;
  }else{
    /* The common case */
    iDb = sqlite3TwoPartName(pParse, pName1, pName2, &pName);
    if( iDb<0 ) return;
    if( !OMIT_TEMPDB && isTemp && pName2->n>0 && iDb!=1 ){
      /* If creating a temp table, the name may not be qualified. Unless
      ** the database name is "temp" anyway.  */
      sqlite3ErrorMsg(pParse, "temporary table name must be unqualified");
      return;
    }
    if( !OMIT_TEMPDB && isTemp ) iDb = 1;
    zName = sqlite3NameFromToken(db, pName);
    if( IN_RENAME_OBJECT ){
      sqlite3RenameTokenMap(pParse, (void*)zName, pName);
    }
  }
  pParse->sNameToken = *pName;
  if( zName==0 ) return;
  if( sqlite3CheckObjectName(pParse, zName, isView?"view":"table", zName) ){
    goto begin_table_error;
  }
  if( db->init.iDb==1 ) isTemp = 1;
#ifndef SQLITE_OMIT_AUTHORIZATION
  assert( isTemp==0 || isTemp==1 );
  assert( isView==0 || isView==1 );
  {
    static const u8 aCode[] = {
       SQLITE_CREATE_TABLE,
       SQLITE_CREATE_TEMP_TABLE,
       SQLITE_CREATE_VIEW,
       SQLITE_CREATE_TEMP_VIEW
    };
    char *zDb = db->aDb[iDb].zDbSName;
    if( sqlite3AuthCheck(pParse, SQLITE_INSERT, SCHEMA_TABLE(isTemp), 0, zDb) ){
      goto begin_table_error;
    }
    if( !isVirtual && sqlite3AuthCheck(pParse, (int)aCode[isTemp+2*isView],
                                       zName, 0, zDb) ){
      goto begin_table_error;
    }
  }
#endif

  /* Make sure the new table name does not collide with an existing
  ** index or table name in the same database.  Issue an error message if
  ** it does. The exception is if the statement being parsed was passed
  ** to an sqlite3_declare_vtab() call. In that case only the column names
  ** and types will be used, so there is no need to test for namespace
  ** collisions.
  */
  if( !IN_SPECIAL_PARSE ){
    char *zDb = db->aDb[iDb].zDbSName;
    if( SQLITE_OK!=sqlite3ReadSchema(pParse) ){
      goto begin_table_error;
    }
    pTable = sqlite3FindTable(db, zName, zDb);
    if( pTable ){
      if( !noErr ){
        sqlite3ErrorMsg(pParse, "%s %T already exists",
                        (IsView(pTable)? "view" : "table"), pName);
      }else{
        assert( !db->init.busy || CORRUPT_DB );
        sqlite3CodeVerifySchema(pParse, iDb);
        sqlite3ForceNotReadOnly(pParse);
      }
      goto begin_table_error;
    }
    if( sqlite3FindIndex(db, zName, zDb)!=0 ){
      sqlite3ErrorMsg(pParse, "there is already an index named %s", zName);
      goto begin_table_error;
    }
  }

  pTable = sqlite3DbMallocZero(db, sizeof(Table));
  if( pTable==0 ){
    assert( db->mallocFailed );
    pParse->rc = SQLITE_NOMEM_BKPT;
    pParse->nErr++;
    goto begin_table_error;
  }
  pTable->zName = zName;
  pTable->iPKey = -1;
  pTable->pSchema = db->aDb[iDb].pSchema;
  pTable->nTabRef = 1;
#ifdef SQLITE_DEFAULT_ROWEST
  pTable->nRowLogEst = sqlite3LogEst(SQLITE_DEFAULT_ROWEST);
#else
  pTable->nRowLogEst = 200; assert( 200==sqlite3LogEst(1048576) );
#endif
  assert( pParse->pNewTable==0 );
  pParse->pNewTable = pTable;

  /* Begin generating the code that will insert the table record into
  ** the schema table.  Note in particular that we must go ahead
  ** and allocate the record number for the table entry now.  Before any
  ** PRIMARY KEY or UNIQUE keywords are parsed.  Those keywords will cause
  ** indices to be created and the table record must come before the
  ** indices.  Hence, the record number for the table must be allocated
  ** now.
  */
  if( !db->init.busy && (v = sqlite3GetVdbe(pParse))!=0 ){
    int addr1;
    int fileFormat;
    int reg1, reg2, reg3;
    /* nullRow[] is an OP_Record encoding of a row containing 5 NULLs */
    static const char nullRow[] = { 6, 0, 0, 0, 0, 0 };
    sqlite3BeginWriteOperation(pParse, 1, iDb);

#ifndef SQLITE_OMIT_VIRTUALTABLE
    if( isVirtual ){
      sqlite3VdbeAddOp0(v, OP_VBegin);
    }
#endif

    /* If the file format and encoding in the database have not been set,
    ** set them now.
    */
    reg1 = pParse->regRowid = ++pParse->nMem;
    reg2 = pParse->regRoot = ++pParse->nMem;
    reg3 = ++pParse->nMem;
    sqlite3VdbeAddOp3(v, OP_ReadCookie, iDb, reg3, BTREE_FILE_FORMAT);
    sqlite3VdbeUsesBtree(v, iDb);
    addr1 = sqlite3VdbeAddOp1(v, OP_If, reg3); VdbeCoverage(v);
    fileFormat = (db->flags & SQLITE_LegacyFileFmt)!=0 ?
                  1 : SQLITE_MAX_FILE_FORMAT;
    sqlite3VdbeAddOp3(v, OP_SetCookie, iDb, BTREE_FILE_FORMAT, fileFormat);
    sqlite3VdbeAddOp3(v, OP_SetCookie, iDb, BTREE_TEXT_ENCODING, ENC(db));
    sqlite3VdbeJumpHere(v, addr1);

    /* This just creates a place-holder record in the sqlite_schema table.
    ** The record created does not contain anything yet.  It will be replaced
    ** by the real entry in code generated at sqlite3EndTable().
    **
    ** The rowid for the new entry is left in register pParse->regRowid.
    ** The root page number of the new table is left in reg pParse->regRoot.
    ** The rowid and root page number values are needed by the code that
    ** sqlite3EndTable will generate.
    */
#if !defined(SQLITE_OMIT_VIEW) || !defined(SQLITE_OMIT_VIRTUALTABLE)
    if( isView || isVirtual ){
      sqlite3VdbeAddOp2(v, OP_Integer, 0, reg2);
    }else
#endif
    {
      assert( !pParse->bReturning );
      pParse->u1.addrCrTab =
         sqlite3VdbeAddOp3(v, OP_CreateBtree, iDb, reg2, BTREE_INTKEY);
    }
    sqlite3OpenSchemaTable(pParse, iDb);
    sqlite3VdbeAddOp2(v, OP_NewRowid, 0, reg1);
    sqlite3VdbeAddOp4(v, OP_Blob, 6, reg3, 0, nullRow, P4_STATIC);
    sqlite3VdbeAddOp3(v, OP_Insert, 0, reg3, reg1);
    sqlite3VdbeChangeP5(v, OPFLAG_APPEND);
    sqlite3VdbeAddOp0(v, OP_Close);
  }

  /* Normal (non-error) return. */
  return;

  /* If an error occurs, we jump here */
begin_table_error:
  pParse->checkSchema = 1;
  sqlite3DbFree(db, zName);
  return;
}

/* Set properties of a table column based on the (magical)
** name of the column.
*/
#if SQLITE_ENABLE_HIDDEN_COLUMNS
SQLITE_PRIVATE void sqlite3ColumnPropertiesFromName(Table *pTab, Column *pCol){
  if( sqlite3_strnicmp(pCol->zCnName, "__hidden__", 10)==0 ){
    pCol->colFlags |= COLFLAG_HIDDEN;
    if( pTab ) pTab->tabFlags |= TF_HasHidden;
  }else if( pTab && pCol!=pTab->aCol && (pCol[-1].colFlags & COLFLAG_HIDDEN) ){
    pTab->tabFlags |= TF_OOOHidden;
  }
}
#endif

/*
** Clean up the data structures associated with the RETURNING clause.
*/
static void sqlite3DeleteReturning(sqlite3 *db, void *pArg){
  Returning *pRet = (Returning*)pArg;
  Hash *pHash;
  pHash = &(db->aDb[1].pSchema->trigHash);
  sqlite3HashInsert(pHash, pRet->zName, 0);
  sqlite3ExprListDelete(db, pRet->pReturnEL);
  sqlite3DbFree(db, pRet);
}

/*
** Add the RETURNING clause to the parse currently underway.
**
** This routine creates a special TEMP trigger that will fire for each row
** of the DML statement.  That TEMP trigger contains a single SELECT
** statement with a result set that is the argument of the RETURNING clause.
** The trigger has the Trigger.bReturning flag and an opcode of
** TK_RETURNING instead of TK_SELECT, so that the trigger code generator
** knows to handle it specially.  The TEMP trigger is automatically
** removed at the end of the parse.
**
** When this routine is called, we do not yet know if the RETURNING clause
** is attached to a DELETE, INSERT, or UPDATE, so construct it as a
** RETURNING trigger instead.  It will then be converted into the appropriate
** type on the first call to sqlite3TriggersExist().
*/
SQLITE_PRIVATE void sqlite3AddReturning(Parse *pParse, ExprList *pList){
  Returning *pRet;
  Hash *pHash;
  sqlite3 *db = pParse->db;
  if( pParse->pNewTrigger ){
    sqlite3ErrorMsg(pParse, "cannot use RETURNING in a trigger");
  }else{
    assert( pParse->bReturning==0 || pParse->ifNotExists );
  }
  pParse->bReturning = 1;
  pRet = sqlite3DbMallocZero(db, sizeof(*pRet));
  if( pRet==0 ){
    sqlite3ExprListDelete(db, pList);
    return;
  }
  pParse->u1.pReturning = pRet;
  pRet->pParse = pParse;
  pRet->pReturnEL = pList;
  sqlite3ParserAddCleanup(pParse, sqlite3DeleteReturning, pRet);
  testcase( pParse->earlyCleanup );
  if( db->mallocFailed ) return;
  sqlite3_snprintf(sizeof(pRet->zName), pRet->zName,
                   "sqlite_returning_%p", pParse);
  pRet->retTrig.zName = pRet->zName;
  pRet->retTrig.op = TK_RETURNING;
  pRet->retTrig.tr_tm = TRIGGER_AFTER;
  pRet->retTrig.bReturning = 1;
  pRet->retTrig.pSchema = db->aDb[1].pSchema;
  pRet->retTrig.pTabSchema = db->aDb[1].pSchema;
  pRet->retTrig.step_list = &pRet->retTStep;
  pRet->retTStep.op = TK_RETURNING;
  pRet->retTStep.pTrig = &pRet->retTrig;
  pRet->retTStep.pExprList = pList;
  pHash = &(db->aDb[1].pSchema->trigHash);
  assert( sqlite3HashFind(pHash, pRet->zName)==0
          || pParse->nErr  || pParse->ifNotExists );
  if( sqlite3HashInsert(pHash, pRet->zName, &pRet->retTrig)
          ==&pRet->retTrig ){
    sqlite3OomFault(db);
  }
}

/*
** Add a new column to the table currently being constructed.
**
** The parser calls this routine once for each column declaration
** in a CREATE TABLE statement.  sqlite3StartTable() gets called
** first to get things going.  Then this routine is called for each
** column.
*/
SQLITE_PRIVATE void sqlite3AddColumn(Parse *pParse, Token sName, Token sType){
  Table *p;
  int i;
  char *z;
  char *zType;
  Column *pCol;
  sqlite3 *db = pParse->db;
  u8 hName;
  Column *aNew;
  u8 eType = COLTYPE_CUSTOM;
  u8 szEst = 1;
  char affinity = SQLITE_AFF_BLOB;

  if( (p = pParse->pNewTable)==0 ) return;
  if( p->nCol+1>db->aLimit[SQLITE_LIMIT_COLUMN] ){
    sqlite3ErrorMsg(pParse, "too many columns on %s", p->zName);
    return;
  }
  if( !IN_RENAME_OBJECT ) sqlite3DequoteToken(&sName);

  /* Because keywords GENERATE ALWAYS can be converted into identifiers
  ** by the parser, we can sometimes end up with a typename that ends
  ** with "generated always".  Check for this case and omit the surplus
  ** text. */
  if( sType.n>=16
   && sqlite3_strnicmp(sType.z+(sType.n-6),"always",6)==0
  ){
    sType.n -= 6;
    while( ALWAYS(sType.n>0) && sqlite3Isspace(sType.z[sType.n-1]) ) sType.n--;
    if( sType.n>=9
     && sqlite3_strnicmp(sType.z+(sType.n-9),"generated",9)==0
    ){
      sType.n -= 9;
      while( sType.n>0 && sqlite3Isspace(sType.z[sType.n-1]) ) sType.n--;
    }
  }

  /* Check for standard typenames.  For standard typenames we will
  ** set the Column.eType field rather than storing the typename after
  ** the column name, in order to save space. */
  if( sType.n>=3 ){
    sqlite3DequoteToken(&sType);
    for(i=0; i<SQLITE_N_STDTYPE; i++){
       if( sType.n==sqlite3StdTypeLen[i]
        && sqlite3_strnicmp(sType.z, sqlite3StdType[i], sType.n)==0
       ){
         sType.n = 0;
         eType = i+1;
         affinity = sqlite3StdTypeAffinity[i];
         if( affinity<=SQLITE_AFF_TEXT ) szEst = 5;
         break;
       }
    }
  }

  z = sqlite3DbMallocRaw(db, (i64)sName.n + 1 + (i64)sType.n + (sType.n>0) );
  if( z==0 ) return;
  if( IN_RENAME_OBJECT ) sqlite3RenameTokenMap(pParse, (void*)z, &sName);
  memcpy(z, sName.z, sName.n);
  z[sName.n] = 0;
  sqlite3Dequote(z);
  hName = sqlite3StrIHash(z);
  for(i=0; i<p->nCol; i++){
    if( p->aCol[i].hName==hName && sqlite3StrICmp(z, p->aCol[i].zCnName)==0 ){
      sqlite3ErrorMsg(pParse, "duplicate column name: %s", z);
      sqlite3DbFree(db, z);
      return;
    }
  }
  aNew = sqlite3DbRealloc(db,p->aCol,((i64)p->nCol+1)*sizeof(p->aCol[0]));
  if( aNew==0 ){
    sqlite3DbFree(db, z);
    return;
  }
  p->aCol = aNew;
  pCol = &p->aCol[p->nCol];
  memset(pCol, 0, sizeof(p->aCol[0]));
  pCol->zCnName = z;
  pCol->hName = hName;
  sqlite3ColumnPropertiesFromName(p, pCol);

  if( sType.n==0 ){
    /* If there is no type specified, columns have the default affinity
    ** 'BLOB' with a default size of 4 bytes. */
    pCol->affinity = affinity;
    pCol->eCType = eType;
    pCol->szEst = szEst;
#ifdef SQLITE_ENABLE_SORTER_REFERENCES
    if( affinity==SQLITE_AFF_BLOB ){
      if( 4>=sqlite3GlobalConfig.szSorterRef ){
        pCol->colFlags |= COLFLAG_SORTERREF;
      }
    }
#endif
  }else{
    zType = z + sqlite3Strlen30(z) + 1;
    memcpy(zType, sType.z, sType.n);
    zType[sType.n] = 0;
    sqlite3Dequote(zType);
    pCol->affinity = sqlite3AffinityType(zType, pCol);
    pCol->colFlags |= COLFLAG_HASTYPE;
  }
  p->nCol++;
  p->nNVCol++;
  pParse->constraintName.n = 0;
}
