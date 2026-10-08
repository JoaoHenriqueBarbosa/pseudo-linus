          assert( !db->init.busy );
          sqlite3CodeVerifySchema(pParse, iDb);
          sqlite3ForceNotReadOnly(pParse);
        }
        goto exit_create_index;
      }
    }
  }else{
    int n;
    Index *pLoop;
    for(pLoop=pTab->pIndex, n=1; pLoop; pLoop=pLoop->pNext, n++){}
    zName = sqlite3MPrintf(db, "sqlite_autoindex_%s_%d", pTab->zName, n);
    if( zName==0 ){
      goto exit_create_index;
    }

    /* Automatic index names generated from within sqlite3_declare_vtab()
    ** must have names that are distinct from normal automatic index names.
    ** The following statement converts "sqlite3_autoindex..." into
    ** "sqlite3_butoindex..." in order to make the names distinct.
    ** The "vtab_err.test" test demonstrates the need of this statement. */
    if( IN_SPECIAL_PARSE ) zName[7]++;
  }

  /* Check for authorization to create an index.
  */
#ifndef SQLITE_OMIT_AUTHORIZATION
  if( !IN_RENAME_OBJECT ){
    const char *zDb = pDb->zDbSName;
    if( sqlite3AuthCheck(pParse, SQLITE_INSERT, SCHEMA_TABLE(iDb), 0, zDb) ){
      goto exit_create_index;
    }
    i = SQLITE_CREATE_INDEX;
    if( !OMIT_TEMPDB && iDb==1 ) i = SQLITE_CREATE_TEMP_INDEX;
    if( sqlite3AuthCheck(pParse, i, zName, pTab->zName, zDb) ){
      goto exit_create_index;
    }
  }
#endif

  /* If pList==0, it means this routine was called to make a primary
  ** key out of the last column added to the table under construction.
  ** So create a fake list to simulate this.
  */
  if( pList==0 ){
    Token prevCol;
    Column *pCol = &pTab->aCol[pTab->nCol-1];
    pCol->colFlags |= COLFLAG_UNIQUE;
    sqlite3TokenInit(&prevCol, pCol->zCnName);
    pList = sqlite3ExprListAppend(pParse, 0,
              sqlite3ExprAlloc(db, TK_ID, &prevCol, 0));
    if( pList==0 ) goto exit_create_index;
    assert( pList->nExpr==1 );
    sqlite3ExprListSetSortOrder(pList, sortOrder, SQLITE_SO_UNDEFINED);
  }else{
    sqlite3ExprListCheckLength(pParse, pList, "index");
    if( pParse->nErr ) goto exit_create_index;
  }

  /* Figure out how many bytes of space are required to store explicitly
  ** specified collation sequence names.
  */
  for(i=0; i<pList->nExpr; i++){
    Expr *pExpr = pList->a[i].pExpr;
    assert( pExpr!=0 );
    if( pExpr->op==TK_COLLATE ){
      assert( !ExprHasProperty(pExpr, EP_IntValue) );
      nExtra += (1 + sqlite3Strlen30(pExpr->u.zToken));
    }
  }

  /*
  ** Allocate the index structure.
  */
  nName = sqlite3Strlen30(zName);
  nExtraCol = pPk ? pPk->nKeyCol : 1;
  assert( pList->nExpr + nExtraCol <= 32767 /* Fits in i16 */ );
  pIndex = sqlite3AllocateIndexObject(db, pList->nExpr + nExtraCol,
                                      nName + nExtra + 1, &zExtra);
  if( db->mallocFailed ){
    goto exit_create_index;
  }
  assert( EIGHT_BYTE_ALIGNMENT(pIndex->aiRowLogEst) );
  assert( EIGHT_BYTE_ALIGNMENT(pIndex->azColl) );
  pIndex->zName = zExtra;
  zExtra += nName + 1;
  memcpy(pIndex->zName, zName, nName+1);
  pIndex->pTable = pTab;
  pIndex->onError = (u8)onError;
  pIndex->uniqNotNull = onError!=OE_None;
  pIndex->idxType = idxType;
  pIndex->pSchema = db->aDb[iDb].pSchema;
  pIndex->nKeyCol = pList->nExpr;
  if( pPIWhere ){
    sqlite3ResolveSelfReference(pParse, pTab, NC_PartIdx, pPIWhere, 0);
    pIndex->pPartIdxWhere = pPIWhere;
    pPIWhere = 0;
  }
  assert( sqlite3SchemaMutexHeld(db, iDb, 0) );

  /* Check to see if we should honor DESC requests on index columns
  */
  if( pDb->pSchema->file_format>=4 ){
    sortOrderMask = -1;   /* Honor DESC */
  }else{
    sortOrderMask = 0;    /* Ignore DESC */
  }

  /* Analyze the list of expressions that form the terms of the index and
  ** report any errors.  In the common case where the expression is exactly
  ** a table column, store that column in aiColumn[].  For general expressions,
  ** populate pIndex->aColExpr and store XN_EXPR (-2) in aiColumn[].
  **
  ** TODO: Issue a warning if two or more columns of the index are identical.
  ** TODO: Issue a warning if the table primary key is used as part of the
  ** index key.
  */
  pListItem = pList->a;
  if( IN_RENAME_OBJECT ){
    pIndex->aColExpr = pList;
    pList = 0;
  }
  for(i=0; i<pIndex->nKeyCol; i++, pListItem++){
    Expr *pCExpr;                  /* The i-th index expression */
    int requestedSortOrder;        /* ASC or DESC on the i-th expression */
    const char *zColl;             /* Collation sequence name */

    sqlite3StringToId(pListItem->pExpr);
    sqlite3ResolveSelfReference(pParse, pTab, NC_IdxExpr, pListItem->pExpr, 0);
    if( pParse->nErr ) goto exit_create_index;
    pCExpr = sqlite3ExprSkipCollate(pListItem->pExpr);
    if( pCExpr->op!=TK_COLUMN ){
      if( pTab==pParse->pNewTable ){
        sqlite3ErrorMsg(pParse, "expressions prohibited in PRIMARY KEY and "
                                "UNIQUE constraints");
        goto exit_create_index;
      }
      if( pIndex->aColExpr==0 ){
        pIndex->aColExpr = pList;
        pList = 0;
      }
      j = XN_EXPR;
      pIndex->aiColumn[i] = XN_EXPR;
      pIndex->uniqNotNull = 0;
      pIndex->bHasExpr = 1;
    }else{
      j = pCExpr->iColumn;
      assert( j<=0x7fff );
      if( j<0 ){
        j = pTab->iPKey;
      }else{
        if( pTab->aCol[j].notNull==0 ){
          pIndex->uniqNotNull = 0;
        }
        if( pTab->aCol[j].colFlags & COLFLAG_VIRTUAL ){
          pIndex->bHasVCol = 1;
          pIndex->bHasExpr = 1;
        }
      }
      pIndex->aiColumn[i] = (i16)j;
    }
    zColl = 0;
    if( pListItem->pExpr->op==TK_COLLATE ){
      int nColl;
      assert( !ExprHasProperty(pListItem->pExpr, EP_IntValue) );
      zColl = pListItem->pExpr->u.zToken;
      nColl = sqlite3Strlen30(zColl) + 1;
      assert( nExtra>=nColl );
      memcpy(zExtra, zColl, nColl);
      zColl = zExtra;
      zExtra += nColl;
      nExtra -= nColl;
    }else if( j>=0 ){
      zColl = sqlite3ColumnColl(&pTab->aCol[j]);
    }
    if( !zColl ) zColl = sqlite3StrBINARY;
    if( !db->init.busy && !sqlite3LocateCollSeq(pParse, zColl) ){
      goto exit_create_index;
    }
    pIndex->azColl[i] = zColl;
    requestedSortOrder = pListItem->fg.sortFlags & sortOrderMask;
    pIndex->aSortOrder[i] = (u8)requestedSortOrder;
  }

  /* Append the table key to the end of the index.  For WITHOUT ROWID
  ** tables (when pPk!=0) this will be the declared PRIMARY KEY.  For
  ** normal tables (when pPk==0) this will be the rowid.
  */
  if( pPk ){
    for(j=0; j<pPk->nKeyCol; j++){
      int x = pPk->aiColumn[j];
      assert( x>=0 );
      if( isDupColumn(pIndex, pIndex->nKeyCol, pPk, j) ){
        pIndex->nColumn--;
      }else{
        testcase( hasColumn(pIndex->aiColumn,pIndex->nKeyCol,x) );
        pIndex->aiColumn[i] = x;
        pIndex->azColl[i] = pPk->azColl[j];
        pIndex->aSortOrder[i] = pPk->aSortOrder[j];
        i++;
      }
    }
    assert( i==pIndex->nColumn );
  }else{
    pIndex->aiColumn[i] = XN_ROWID;
    pIndex->azColl[i] = sqlite3StrBINARY;
  }
  sqlite3DefaultRowEst(pIndex);
  if( pParse->pNewTable==0 ) estimateIndexWidth(pIndex);

  /* If this index contains every column of its table, then mark
  ** it as a covering index */
  assert( HasRowid(pTab)
      || pTab->iPKey<0 || sqlite3TableColumnToIndex(pIndex, pTab->iPKey)>=0 );
  recomputeColumnsNotIndexed(pIndex);
  if( pTblName!=0 && pIndex->nColumn>=pTab->nCol ){
    pIndex->isCovering = 1;
    for(j=0; j<pTab->nCol; j++){
      if( j==pTab->iPKey ) continue;
      if( sqlite3TableColumnToIndex(pIndex,j)>=0 ) continue;
      pIndex->isCovering = 0;
      break;
    }
  }

  if( pTab==pParse->pNewTable ){
    /* This routine has been called to create an automatic index as a
    ** result of a PRIMARY KEY or UNIQUE clause on a column definition, or
    ** a PRIMARY KEY or UNIQUE clause following the column definitions.
    ** i.e. one of:
    **
    ** CREATE TABLE t(x PRIMARY KEY, y);
    ** CREATE TABLE t(x, y, UNIQUE(x, y));
    **
    ** Either way, check to see if the table already has such an index. If
    ** so, don't bother creating this one. This only applies to
    ** automatically created indices. Users can do as they wish with
    ** explicit indices.
    **
    ** Two UNIQUE or PRIMARY KEY constraints are considered equivalent
    ** (and thus suppressing the second one) even if they have different
    ** sort orders.
    **
    ** If there are different collating sequences or if the columns of
    ** the constraint occur in different orders, then the constraints are
    ** considered distinct and both result in separate indices.
    */
    Index *pIdx;
    for(pIdx=pTab->pIndex; pIdx; pIdx=pIdx->pNext){
      int k;
      assert( IsUniqueIndex(pIdx) );
      assert( pIdx->idxType!=SQLITE_IDXTYPE_APPDEF );
      assert( IsUniqueIndex(pIndex) );

      if( pIdx->nKeyCol!=pIndex->nKeyCol ) continue;
      for(k=0; k<pIdx->nKeyCol; k++){
        const char *z1;
        const char *z2;
        assert( pIdx->aiColumn[k]>=0 );
        if( pIdx->aiColumn[k]!=pIndex->aiColumn[k] ) break;
        z1 = pIdx->azColl[k];
        z2 = pIndex->azColl[k];
        if( sqlite3StrICmp(z1, z2) ) break;
      }
      if( k==pIdx->nKeyCol ){
        if( pIdx->onError!=pIndex->onError ){
          /* This constraint creates the same index as a previous
          ** constraint specified somewhere in the CREATE TABLE statement.
          ** However the ON CONFLICT clauses are different. If both this
          ** constraint and the previous equivalent constraint have explicit
          ** ON CONFLICT clauses this is an error. Otherwise, use the
          ** explicitly specified behavior for the index.
          */
          if( !(pIdx->onError==OE_Default || pIndex->onError==OE_Default) ){
            sqlite3ErrorMsg(pParse,
                "conflicting ON CONFLICT clauses specified", 0);
          }
          if( pIdx->onError==OE_Default ){
            pIdx->onError = pIndex->onError;
          }
        }
        if( idxType==SQLITE_IDXTYPE_PRIMARYKEY ) pIdx->idxType = idxType;
        if( IN_RENAME_OBJECT ){
          pIndex->pNext = pParse->pNewIndex;
          pParse->pNewIndex = pIndex;
          pIndex = 0;
        }
        goto exit_create_index;
      }
    }
  }

  if( !IN_RENAME_OBJECT ){

    /* Link the new Index structure to its table and to the other
    ** in-memory database structures.
    */
    assert( pParse->nErr==0 );
    if( db->init.busy ){
      Index *p;
      assert( !IN_SPECIAL_PARSE );
      assert( sqlite3SchemaMutexHeld(db, 0, pIndex->pSchema) );
      if( pTblName!=0 ){
        pIndex->tnum = db->init.newTnum;
        if( sqlite3IndexHasDuplicateRootPage(pIndex) ){
          sqlite3ErrorMsg(pParse, "invalid rootpage");
          pParse->rc = SQLITE_CORRUPT_BKPT;
          goto exit_create_index;
        }
      }
      p = sqlite3HashInsert(&pIndex->pSchema->idxHash,
          pIndex->zName, pIndex);
      if( p ){
        assert( p==pIndex );  /* Malloc must have failed */
        sqlite3OomFault(db);
        goto exit_create_index;
      }
      db->mDbFlags |= DBFLAG_SchemaChange;
    }

    /* If this is the initial CREATE INDEX statement (or CREATE TABLE if the
    ** index is an implied index for a UNIQUE or PRIMARY KEY constraint) then
    ** emit code to allocate the index rootpage on disk and make an entry for
    ** the index in the sqlite_schema table and populate the index with
    ** content.  But, do not do this if we are simply reading the sqlite_schema
    ** table to parse the schema, or if this index is the PRIMARY KEY index
    ** of a WITHOUT ROWID table.
    **
    ** If pTblName==0 it means this index is generated as an implied PRIMARY KEY
    ** or UNIQUE index in a CREATE TABLE statement.  Since the table
    ** has just been created, it contains no data and the index initialization
    ** step can be skipped.
    */
    else if( HasRowid(pTab) || pTblName!=0 ){
      Vdbe *v;
      char *zStmt;
      int iMem = ++pParse->nMem;

      v = sqlite3GetVdbe(pParse);
      if( v==0 ) goto exit_create_index;

      sqlite3BeginWriteOperation(pParse, 1, iDb);

      /* Create the rootpage for the index using CreateIndex. But before
      ** doing so, code a Noop instruction and store its address in
      ** Index.tnum. This is required in case this index is actually a
      ** PRIMARY KEY and the table is actually a WITHOUT ROWID table. In
      ** that case the convertToWithoutRowidTable() routine will replace
      ** the Noop with a Goto to jump over the VDBE code generated below. */
      pIndex->tnum = (Pgno)sqlite3VdbeAddOp0(v, OP_Noop);
      sqlite3VdbeAddOp3(v, OP_CreateBtree, iDb, iMem, BTREE_BLOBKEY);

      /* Gather the complete text of the CREATE INDEX statement into
      ** the zStmt variable
      */
      assert( pName!=0 || pStart==0 );
      if( pStart ){
        int n = (int)(pParse->sLastToken.z - pName->z) + pParse->sLastToken.n;
        if( pName->z[n-1]==';' ) n--;
        /* A named index with an explicit CREATE INDEX statement */
        zStmt = sqlite3MPrintf(db, "CREATE%s INDEX %.*s",
            onError==OE_None ? "" : " UNIQUE", n, pName->z);
      }else{
        /* An automatic index created by a PRIMARY KEY or UNIQUE constraint */
        /* zStmt = sqlite3MPrintf(""); */
        zStmt = 0;
      }

      /* Add an entry in sqlite_schema for this index
      */
      sqlite3NestedParse(pParse,
         "INSERT INTO %Q." LEGACY_SCHEMA_TABLE " VALUES('index',%Q,%Q,#%d,%Q);",
         db->aDb[iDb].zDbSName,
         pIndex->zName,
         pTab->zName,
         iMem,
         zStmt
      );
      sqlite3DbFree(db, zStmt);

      /* Fill the index with data and reparse the schema. Code an OP_Expire
      ** to invalidate all pre-compiled statements.
      */
      if( pTblName ){
        sqlite3RefillIndex(pParse, pIndex, iMem);
        sqlite3ChangeCookie(pParse, iDb);
        sqlite3VdbeAddParseSchemaOp(v, iDb,
            sqlite3MPrintf(db, "name='%q' AND type='index'", pIndex->zName), 0);
        sqlite3VdbeAddOp2(v, OP_Expire, 0, 1);
      }

      sqlite3VdbeJumpHere(v, (int)pIndex->tnum);
    }
  }
  if( db->init.busy || pTblName==0 ){
    pIndex->pNext = pTab->pIndex;
    pTab->pIndex = pIndex;
    pIndex = 0;
  }
  else if( IN_RENAME_OBJECT ){
    assert( pParse->pNewIndex==0 );
    pParse->pNewIndex = pIndex;
    pIndex = 0;
  }

  /* Clean up before exiting */
exit_create_index:
  if( pIndex ) sqlite3FreeIndex(db, pIndex);
  if( pTab ){
    /* Ensure all REPLACE indexes on pTab are at the end of the pIndex list.
    ** The list was already ordered when this routine was entered, so at this
    ** point at most a single index (the newly added index) will be out of
    ** order.  So we have to reorder at most one index. */
    Index **ppFrom;
    Index *pThis;
    for(ppFrom=&pTab->pIndex; (pThis = *ppFrom)!=0; ppFrom=&pThis->pNext){
      Index *pNext;
      if( pThis->onError!=OE_Replace ) continue;
      while( (pNext = pThis->pNext)!=0 && pNext->onError!=OE_Replace ){
        *ppFrom = pNext;
        pThis->pNext = pNext->pNext;
        pNext->pNext = pThis;
        ppFrom = &pNext->pNext;
      }
      break;
    }
#ifdef SQLITE_DEBUG
    /* Verify that all REPLACE indexes really are now at the end
    ** of the index list.  In other words, no other index type ever
    ** comes after a REPLACE index on the list. */
    for(pThis = pTab->pIndex; pThis; pThis=pThis->pNext){
      assert( pThis->onError!=OE_Replace
           || pThis->pNext==0
           || pThis->pNext->onError==OE_Replace );
    }
#endif
  }
  sqlite3ExprDelete(db, pPIWhere);
  sqlite3ExprListDelete(db, pList);
  sqlite3SrcListDelete(db, pTblName);
  sqlite3DbFree(db, zName);
}
