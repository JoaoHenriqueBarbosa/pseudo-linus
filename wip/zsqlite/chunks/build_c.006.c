

#ifndef SQLITE_OMIT_VIRTUALTABLE
/*
** Return true if pTab is a virtual table and zName is a shadow table name
** for that virtual table.
*/
SQLITE_PRIVATE int sqlite3IsShadowTableOf(sqlite3 *db, Table *pTab, const char *zName){
  int nName;                    /* Length of zName */
  Module *pMod;                 /* Module for the virtual table */

  if( !IsVirtual(pTab) ) return 0;
  nName = sqlite3Strlen30(pTab->zName);
  if( sqlite3_strnicmp(zName, pTab->zName, nName)!=0 ) return 0;
  if( zName[nName]!='_' ) return 0;
  pMod = (Module*)sqlite3HashFind(&db->aModule, pTab->u.vtab.azArg[0]);
  if( pMod==0 ) return 0;
  if( pMod->pModule->iVersion<3 ) return 0;
  if( pMod->pModule->xShadowName==0 ) return 0;
  return pMod->pModule->xShadowName(zName+nName+1);
}
#endif /* ifndef SQLITE_OMIT_VIRTUALTABLE */

#ifndef SQLITE_OMIT_VIRTUALTABLE
/*
** Table pTab is a virtual table.  If it the virtual table implementation
** exists and has an xShadowName method, then loop over all other ordinary
** tables within the same schema looking for shadow tables of pTab, and mark
** any shadow tables seen using the TF_Shadow flag.
*/
SQLITE_PRIVATE void sqlite3MarkAllShadowTablesOf(sqlite3 *db, Table *pTab){
  int nName;                    /* Length of pTab->zName */
  Module *pMod;                 /* Module for the virtual table */
  HashElem *k;                  /* For looping through the symbol table */

  assert( IsVirtual(pTab) );
  pMod = (Module*)sqlite3HashFind(&db->aModule, pTab->u.vtab.azArg[0]);
  if( pMod==0 ) return;
  if( NEVER(pMod->pModule==0) ) return;
  if( pMod->pModule->iVersion<3 ) return;
  if( pMod->pModule->xShadowName==0 ) return;
  assert( pTab->zName!=0 );
  nName = sqlite3Strlen30(pTab->zName);
  for(k=sqliteHashFirst(&pTab->pSchema->tblHash); k; k=sqliteHashNext(k)){
    Table *pOther = sqliteHashData(k);
    assert( pOther->zName!=0 );
    if( !IsOrdinaryTable(pOther) ) continue;
    if( pOther->tabFlags & TF_Shadow ) continue;
    if( sqlite3StrNICmp(pOther->zName, pTab->zName, nName)==0
     && pOther->zName[nName]=='_'
     && pMod->pModule->xShadowName(pOther->zName+nName+1)
    ){
      pOther->tabFlags |= TF_Shadow;
    }
  }
}
#endif /* ifndef SQLITE_OMIT_VIRTUALTABLE */

#ifndef SQLITE_OMIT_VIRTUALTABLE
/*
** Return true if zName is a shadow table name in the current database
** connection.
**
** zName is temporarily modified while this routine is running, but is
** restored to its original value prior to this routine returning.
*/
SQLITE_PRIVATE int sqlite3ShadowTableName(sqlite3 *db, const char *zName){
  char *zTail;                  /* Pointer to the last "_" in zName */
  Table *pTab;                  /* Table that zName is a shadow of */
  zTail = strrchr(zName, '_');
  if( zTail==0 ) return 0;
  *zTail = 0;
  pTab = sqlite3FindTable(db, zName, 0);
  *zTail = '_';
  if( pTab==0 ) return 0;
  if( !IsVirtual(pTab) ) return 0;
  return sqlite3IsShadowTableOf(db, pTab, zName);
}
#endif /* ifndef SQLITE_OMIT_VIRTUALTABLE */


#ifdef SQLITE_DEBUG
/*
** Mark all nodes of an expression as EP_Immutable, indicating that
** they should not be changed.  Expressions attached to a table or
** index definition are tagged this way to help ensure that we do
** not pass them into code generator routines by mistake.
*/
static int markImmutableExprStep(Walker *pWalker, Expr *pExpr){
  (void)pWalker;
  ExprSetVVAProperty(pExpr, EP_Immutable);
  return WRC_Continue;
}
static void markExprListImmutable(ExprList *pList){
  if( pList ){
    Walker w;
    memset(&w, 0, sizeof(w));
    w.xExprCallback = markImmutableExprStep;
    w.xSelectCallback = sqlite3SelectWalkNoop;
    w.xSelectCallback2 = 0;
    sqlite3WalkExprList(&w, pList);
  }
}
#else
#define markExprListImmutable(X)  /* no-op */
#endif /* SQLITE_DEBUG */


/*
** This routine is called to report the final ")" that terminates
** a CREATE TABLE statement.
**
** The table structure that other action routines have been building
** is added to the internal hash tables, assuming no errors have
** occurred.
**
** An entry for the table is made in the schema table on disk, unless
** this is a temporary table or db->init.busy==1.  When db->init.busy==1
** it means we are reading the sqlite_schema table because we just
** connected to the database or because the sqlite_schema table has
** recently changed, so the entry for this table already exists in
** the sqlite_schema table.  We do not want to create it again.
**
** If the pSelect argument is not NULL, it means that this routine
** was called to create a table generated from a
** "CREATE TABLE ... AS SELECT ..." statement.  The column names of
** the new table will match the result set of the SELECT.
*/
SQLITE_PRIVATE void sqlite3EndTable(
  Parse *pParse,          /* Parse context */
  Token *pCons,           /* The ',' token after the last column defn. */
  Token *pEnd,            /* The ')' before options in the CREATE TABLE */
  u32 tabOpts,            /* Extra table options. Usually 0. */
  Select *pSelect         /* Select from a "CREATE ... AS SELECT" */
){
  Table *p;                 /* The new table */
  sqlite3 *db = pParse->db; /* The database connection */
  int iDb;                  /* Database in which the table lives */
  Index *pIdx;              /* An implied index of the table */

  if( pEnd==0 && pSelect==0 ){
    return;
  }
  p = pParse->pNewTable;
  if( p==0 ) return;

  if( pSelect==0 && sqlite3ShadowTableName(db, p->zName) ){
    p->tabFlags |= TF_Shadow;
  }

  /* If the db->init.busy is 1 it means we are reading the SQL off the
  ** "sqlite_schema" or "sqlite_temp_schema" table on the disk.
  ** So do not write to the disk again.  Extract the root page number
  ** for the table from the db->init.newTnum field.  (The page number
  ** should have been put there by the sqliteOpenCb routine.)
  **
  ** If the root page number is 1, that means this is the sqlite_schema
  ** table itself.  So mark it read-only.
  */
  if( db->init.busy ){
    if( pSelect || (!IsOrdinaryTable(p) && db->init.newTnum) ){
      sqlite3ErrorMsg(pParse, "");
      return;
    }
    p->tnum = db->init.newTnum;
    if( p->tnum==1 ) p->tabFlags |= TF_Readonly;
  }

  /* Special processing for tables that include the STRICT keyword:
  **
  **   *  Do not allow custom column datatypes.  Every column must have
  **      a datatype that is one of INT, INTEGER, REAL, TEXT, or BLOB.
  **
  **   *  If a PRIMARY KEY is defined, other than the INTEGER PRIMARY KEY,
  **      then all columns of the PRIMARY KEY must have a NOT NULL
  **      constraint.
  */
  if( tabOpts & TF_Strict ){
    int ii;
    p->tabFlags |= TF_Strict;
    for(ii=0; ii<p->nCol; ii++){
      Column *pCol = &p->aCol[ii];
      if( pCol->eCType==COLTYPE_CUSTOM ){
        if( pCol->colFlags & COLFLAG_HASTYPE ){
          sqlite3ErrorMsg(pParse,
            "unknown datatype for %s.%s: \"%s\"",
            p->zName, pCol->zCnName, sqlite3ColumnType(pCol, "")
          );
        }else{
          sqlite3ErrorMsg(pParse, "missing datatype for %s.%s",
                          p->zName, pCol->zCnName);
        }
        return;
      }else if( pCol->eCType==COLTYPE_ANY ){
        pCol->affinity = SQLITE_AFF_BLOB;
      }
      if( (pCol->colFlags & COLFLAG_PRIMKEY)!=0
       && p->iPKey!=ii
       && pCol->notNull == OE_None
      ){
        pCol->notNull = OE_Abort;
        p->tabFlags |= TF_HasNotNull;
      }
    }
  }

  assert( (p->tabFlags & TF_HasPrimaryKey)==0
       || p->iPKey>=0 || sqlite3PrimaryKeyIndex(p)!=0 );
  assert( (p->tabFlags & TF_HasPrimaryKey)!=0
       || (p->iPKey<0 && sqlite3PrimaryKeyIndex(p)==0) );

  /* Special processing for WITHOUT ROWID Tables */
  if( tabOpts & TF_WithoutRowid ){
    if( (p->tabFlags & TF_Autoincrement) ){
      sqlite3ErrorMsg(pParse,
          "AUTOINCREMENT not allowed on WITHOUT ROWID tables");
      return;
    }
    if( (p->tabFlags & TF_HasPrimaryKey)==0 ){
      sqlite3ErrorMsg(pParse, "PRIMARY KEY missing on table %s", p->zName);
      return;
    }
    p->tabFlags |= TF_WithoutRowid | TF_NoVisibleRowid;
    convertToWithoutRowidTable(pParse, p);
  }
  iDb = sqlite3SchemaToIndex(db, p->pSchema);

#ifndef SQLITE_OMIT_CHECK
  /* Resolve names in all CHECK constraint expressions.
  */
  if( p->pCheck ){
    sqlite3ResolveSelfReference(pParse, p, NC_IsCheck, 0, p->pCheck);
    if( pParse->nErr ){
      /* If errors are seen, delete the CHECK constraints now, else they might
      ** actually be used if PRAGMA writable_schema=ON is set. */
      sqlite3ExprListDelete(db, p->pCheck);
      p->pCheck = 0;
    }else{
      markExprListImmutable(p->pCheck);
    }
  }
#endif /* !defined(SQLITE_OMIT_CHECK) */
#ifndef SQLITE_OMIT_GENERATED_COLUMNS
  if( p->tabFlags & TF_HasGenerated ){
    int ii, nNG = 0;
    testcase( p->tabFlags & TF_HasVirtual );
    testcase( p->tabFlags & TF_HasStored );
    for(ii=0; ii<p->nCol; ii++){
      u32 colFlags = p->aCol[ii].colFlags;
      if( (colFlags & COLFLAG_GENERATED)!=0 ){
        Expr *pX = sqlite3ColumnExpr(p, &p->aCol[ii]);
        testcase( colFlags & COLFLAG_VIRTUAL );
        testcase( colFlags & COLFLAG_STORED );
        if( sqlite3ResolveSelfReference(pParse, p, NC_GenCol, pX, 0) ){
          /* If there are errors in resolving the expression, change the
          ** expression to a NULL.  This prevents code generators that operate
          ** on the expression from inserting extra parts into the expression
          ** tree that have been allocated from lookaside memory, which is
          ** illegal in a schema and will lead to errors or heap corruption
          ** when the database connection closes. */
          sqlite3ColumnSetExpr(pParse, p, &p->aCol[ii],
               sqlite3ExprAlloc(db, TK_NULL, 0, 0));
        }
      }else{
        nNG++;
      }
    }
    if( nNG==0 ){
      sqlite3ErrorMsg(pParse, "must have at least one non-generated column");
      return;
    }
  }
#endif

  /* Estimate the average row size for the table and for all implied indices */
  estimateTableWidth(p);
  for(pIdx=p->pIndex; pIdx; pIdx=pIdx->pNext){
    estimateIndexWidth(pIdx);
  }

  /* If not initializing, then create a record for the new table
  ** in the schema table of the database.
  **
  ** If this is a TEMPORARY table, write the entry into the auxiliary
  ** file instead of into the main database file.
  */
  if( !db->init.busy ){
    int n;
    Vdbe *v;
    char *zType;    /* "view" or "table" */
    char *zType2;   /* "VIEW" or "TABLE" */
    char *zStmt;    /* Text of the CREATE TABLE or CREATE VIEW statement */

    v = sqlite3GetVdbe(pParse);
    if( NEVER(v==0) ) return;

    sqlite3VdbeAddOp1(v, OP_Close, 0);

    /*
    ** Initialize zType for the new view or table.
    */
    if( IsOrdinaryTable(p) ){
      /* A regular table */
      zType = "table";
      zType2 = "TABLE";
#ifndef SQLITE_OMIT_VIEW
    }else{
      /* A view */
      zType = "view";
      zType2 = "VIEW";
#endif
    }

    /* If this is a CREATE TABLE xx AS SELECT ..., execute the SELECT
    ** statement to populate the new table. The root-page number for the
    ** new table is in register pParse->regRoot.
    **
    ** Once the SELECT has been coded by sqlite3Select(), it is in a
    ** suitable state to query for the column names and types to be used
    ** by the new table.
    **
    ** A shared-cache write-lock is not required to write to the new table,
    ** as a schema-lock must have already been obtained to create it. Since
    ** a schema-lock excludes all other database users, the write-lock would
    ** be redundant.
    */
    if( pSelect ){
      SelectDest dest;    /* Where the SELECT should store results */
      int regYield;       /* Register holding co-routine entry-point */
      int addrTop;        /* Top of the co-routine */
      int regRec;         /* A record to be insert into the new table */
      int regRowid;       /* Rowid of the next row to insert */
      int addrInsLoop;    /* Top of the loop for inserting rows */
      Table *pSelTab;     /* A table that describes the SELECT results */
      int iCsr;           /* Write cursor on the new table */

      if( IN_SPECIAL_PARSE ){
        pParse->rc = SQLITE_ERROR;
        pParse->nErr++;
        return;
      }
      iCsr = pParse->nTab++;
      regYield = ++pParse->nMem;
      regRec = ++pParse->nMem;
      regRowid = ++pParse->nMem;
      sqlite3MayAbort(pParse);
      sqlite3VdbeAddOp3(v, OP_OpenWrite, iCsr, pParse->regRoot, iDb);
      sqlite3VdbeChangeP5(v, OPFLAG_P2ISREG);
      addrTop = sqlite3VdbeCurrentAddr(v) + 1;
      sqlite3VdbeAddOp3(v, OP_InitCoroutine, regYield, 0, addrTop);
      if( pParse->nErr ) return;
      pSelTab = sqlite3ResultSetOfSelect(pParse, pSelect, SQLITE_AFF_BLOB);
      if( pSelTab==0 ) return;
      assert( p->aCol==0 );
      p->nCol = p->nNVCol = pSelTab->nCol;
      p->aCol = pSelTab->aCol;
      pSelTab->nCol = 0;
      pSelTab->aCol = 0;
      sqlite3DeleteTable(db, pSelTab);
      sqlite3SelectDestInit(&dest, SRT_Coroutine, regYield);
      sqlite3Select(pParse, pSelect, &dest);
      if( pParse->nErr ) return;
      sqlite3VdbeEndCoroutine(v, regYield);
      sqlite3VdbeJumpHere(v, addrTop - 1);
      addrInsLoop = sqlite3VdbeAddOp1(v, OP_Yield, dest.iSDParm);
      VdbeCoverage(v);
      sqlite3VdbeAddOp3(v, OP_MakeRecord, dest.iSdst, dest.nSdst, regRec);
      sqlite3TableAffinity(v, p, 0);
      sqlite3VdbeAddOp2(v, OP_NewRowid, iCsr, regRowid);
      sqlite3VdbeAddOp3(v, OP_Insert, iCsr, regRec, regRowid);
      sqlite3VdbeGoto(v, addrInsLoop);
      sqlite3VdbeJumpHere(v, addrInsLoop);
      sqlite3VdbeAddOp1(v, OP_Close, iCsr);
    }

    /* Compute the complete text of the CREATE statement */
    if( pSelect ){
      zStmt = createTableStmt(db, p);
    }else{
      Token *pEnd2 = tabOpts ? &pParse->sLastToken : pEnd;
      n = (int)(pEnd2->z - pParse->sNameToken.z);
      if( pEnd2->z[0]!=';' ) n += pEnd2->n;
      zStmt = sqlite3MPrintf(db,
          "CREATE %s %.*s", zType2, n, pParse->sNameToken.z
      );
    }

    /* A slot for the record has already been allocated in the
    ** schema table.  We just need to update that slot with all
    ** the information we've collected.
    */
    sqlite3NestedParse(pParse,
      "UPDATE %Q." LEGACY_SCHEMA_TABLE
      " SET type='%s', name=%Q, tbl_name=%Q, rootpage=#%d, sql=%Q"
      " WHERE rowid=#%d",
      db->aDb[iDb].zDbSName,
      zType,
      p->zName,
      p->zName,
      pParse->regRoot,
      zStmt,
      pParse->regRowid
    );
    sqlite3DbFree(db, zStmt);
    sqlite3ChangeCookie(pParse, iDb);

#ifndef SQLITE_OMIT_AUTOINCREMENT
    /* Check to see if we need to create an sqlite_sequence table for
    ** keeping track of autoincrement keys.
    */
    if( (p->tabFlags & TF_Autoincrement)!=0 && !IN_SPECIAL_PARSE ){
      Db *pDb = &db->aDb[iDb];
      assert( sqlite3SchemaMutexHeld(db, iDb, 0) );
      if( pDb->pSchema->pSeqTab==0 ){
        sqlite3NestedParse(pParse,
          "CREATE TABLE %Q.sqlite_sequence(name,seq)",
          pDb->zDbSName
        );
      }
    }
#endif

    /* Reparse everything to update our internal data structures */
    sqlite3VdbeAddParseSchemaOp(v, iDb,
           sqlite3MPrintf(db, "tbl_name='%q' AND type!='trigger'", p->zName),0);

    /* Test for cycles in generated columns and illegal expressions
    ** in CHECK constraints and in DEFAULT clauses. */
    if( p->tabFlags & TF_HasGenerated ){
      sqlite3VdbeAddOp4(v, OP_SqlExec, 0x0001, 0, 0,
             sqlite3MPrintf(db, "SELECT*FROM\"%w\".\"%w\"",
                   db->aDb[iDb].zDbSName, p->zName), P4_DYNAMIC);
    }
  }

  /* Add the table to the in-memory representation of the database.
  */
  if( db->init.busy ){
    Table *pOld;
    Schema *pSchema = p->pSchema;
    assert( sqlite3SchemaMutexHeld(db, iDb, 0) );
    assert( HasRowid(p) || p->iPKey<0 );
    pOld = sqlite3HashInsert(&pSchema->tblHash, p->zName, p);
    if( pOld ){
      assert( p==pOld );  /* Malloc must have failed inside HashInsert() */
      sqlite3OomFault(db);
      return;
    }
    pParse->pNewTable = 0;
    db->mDbFlags |= DBFLAG_SchemaChange;

    /* If this is the magic sqlite_sequence table used by autoincrement,
    ** then record a pointer to this table in the main database structure
    ** so that INSERT can find the table easily.  */
    assert( !pParse->nested );
#ifndef SQLITE_OMIT_AUTOINCREMENT
    if( strcmp(p->zName, "sqlite_sequence")==0 ){
      assert( sqlite3SchemaMutexHeld(db, iDb, 0) );
      p->pSchema->pSeqTab = p;
    }
#endif
  }

#ifndef SQLITE_OMIT_ALTERTABLE
  if( !pSelect && IsOrdinaryTable(p) ){
    assert( pCons && pEnd );
    if( pCons->z==0 ){
      pCons = pEnd;
    }
    p->u.tab.addColOffset = 13 + (int)(pCons->z - pParse->sNameToken.z);
  }
#endif
}
