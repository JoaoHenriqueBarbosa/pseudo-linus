
/*
** Assign VdbeCursor index numbers to all tables in a SrcList
*/
SQLITE_PRIVATE void sqlite3SrcListAssignCursors(Parse *pParse, SrcList *pList){
  int i;
  SrcItem *pItem;
  assert( pList || pParse->db->mallocFailed );
  if( ALWAYS(pList) ){
    for(i=0, pItem=pList->a; i<pList->nSrc; i++, pItem++){
      if( pItem->iCursor>=0 ) continue;
      pItem->iCursor = pParse->nTab++;
      if( pItem->pSelect ){
        sqlite3SrcListAssignCursors(pParse, pItem->pSelect->pSrc);
      }
    }
  }
}

/*
** Delete an entire SrcList including all its substructure.
*/
SQLITE_PRIVATE void sqlite3SrcListDelete(sqlite3 *db, SrcList *pList){
  int i;
  SrcItem *pItem;
  assert( db!=0 );
  if( pList==0 ) return;
  for(pItem=pList->a, i=0; i<pList->nSrc; i++, pItem++){
    if( pItem->zDatabase ) sqlite3DbNNFreeNN(db, pItem->zDatabase);
    if( pItem->zName ) sqlite3DbNNFreeNN(db, pItem->zName);
    if( pItem->zAlias ) sqlite3DbNNFreeNN(db, pItem->zAlias);
    if( pItem->fg.isIndexedBy ) sqlite3DbFree(db, pItem->u1.zIndexedBy);
    if( pItem->fg.isTabFunc ) sqlite3ExprListDelete(db, pItem->u1.pFuncArg);
    sqlite3DeleteTable(db, pItem->pTab);
    if( pItem->pSelect ) sqlite3SelectDelete(db, pItem->pSelect);
    if( pItem->fg.isUsing ){
      sqlite3IdListDelete(db, pItem->u3.pUsing);
    }else if( pItem->u3.pOn ){
      sqlite3ExprDelete(db, pItem->u3.pOn);
    }
  }
  sqlite3DbNNFreeNN(db, pList);
}

/*
** This routine is called by the parser to add a new term to the
** end of a growing FROM clause.  The "p" parameter is the part of
** the FROM clause that has already been constructed.  "p" is NULL
** if this is the first term of the FROM clause.  pTable and pDatabase
** are the name of the table and database named in the FROM clause term.
** pDatabase is NULL if the database name qualifier is missing - the
** usual case.  If the term has an alias, then pAlias points to the
** alias token.  If the term is a subquery, then pSubquery is the
** SELECT statement that the subquery encodes.  The pTable and
** pDatabase parameters are NULL for subqueries.  The pOn and pUsing
** parameters are the content of the ON and USING clauses.
**
** Return a new SrcList which encodes is the FROM with the new
** term added.
*/
SQLITE_PRIVATE SrcList *sqlite3SrcListAppendFromTerm(
  Parse *pParse,          /* Parsing context */
  SrcList *p,             /* The left part of the FROM clause already seen */
  Token *pTable,          /* Name of the table to add to the FROM clause */
  Token *pDatabase,       /* Name of the database containing pTable */
  Token *pAlias,          /* The right-hand side of the AS subexpression */
  Select *pSubquery,      /* A subquery used in place of a table name */
  OnOrUsing *pOnUsing     /* Either the ON clause or the USING clause */
){
  SrcItem *pItem;
  sqlite3 *db = pParse->db;
  if( !p && pOnUsing!=0 && (pOnUsing->pOn || pOnUsing->pUsing) ){
    sqlite3ErrorMsg(pParse, "a JOIN clause is required before %s",
      (pOnUsing->pOn ? "ON" : "USING")
    );
    goto append_from_error;
  }
  p = sqlite3SrcListAppend(pParse, p, pTable, pDatabase);
  if( p==0 ){
    goto append_from_error;
  }
  assert( p->nSrc>0 );
  pItem = &p->a[p->nSrc-1];
  assert( (pTable==0)==(pDatabase==0) );
  assert( pItem->zName==0 || pDatabase!=0 );
  if( IN_RENAME_OBJECT && pItem->zName ){
    Token *pToken = (ALWAYS(pDatabase) && pDatabase->z) ? pDatabase : pTable;
    sqlite3RenameTokenMap(pParse, pItem->zName, pToken);
  }
  assert( pAlias!=0 );
  if( pAlias->n ){
    pItem->zAlias = sqlite3NameFromToken(db, pAlias);
  }
  if( pSubquery ){
    pItem->pSelect = pSubquery;
    if( pSubquery->selFlags & SF_NestedFrom ){
      pItem->fg.isNestedFrom = 1;
    }
  }
  assert( pOnUsing==0 || pOnUsing->pOn==0 || pOnUsing->pUsing==0 );
  assert( pItem->fg.isUsing==0 );
  if( pOnUsing==0 ){
    pItem->u3.pOn = 0;
  }else if( pOnUsing->pUsing ){
    pItem->fg.isUsing = 1;
    pItem->u3.pUsing = pOnUsing->pUsing;
  }else{
    pItem->u3.pOn = pOnUsing->pOn;
  }
  return p;

append_from_error:
  assert( p==0 );
  sqlite3ClearOnOrUsing(db, pOnUsing);
  sqlite3SelectDelete(db, pSubquery);
  return 0;
}

/*
** Add an INDEXED BY or NOT INDEXED clause to the most recently added
** element of the source-list passed as the second argument.
*/
SQLITE_PRIVATE void sqlite3SrcListIndexedBy(Parse *pParse, SrcList *p, Token *pIndexedBy){
  assert( pIndexedBy!=0 );
  if( p && pIndexedBy->n>0 ){
    SrcItem *pItem;
    assert( p->nSrc>0 );
    pItem = &p->a[p->nSrc-1];
    assert( pItem->fg.notIndexed==0 );
    assert( pItem->fg.isIndexedBy==0 );
    assert( pItem->fg.isTabFunc==0 );
    if( pIndexedBy->n==1 && !pIndexedBy->z ){
      /* A "NOT INDEXED" clause was supplied. See parse.y
      ** construct "indexed_opt" for details. */
      pItem->fg.notIndexed = 1;
    }else{
      pItem->u1.zIndexedBy = sqlite3NameFromToken(pParse->db, pIndexedBy);
      pItem->fg.isIndexedBy = 1;
      assert( pItem->fg.isCte==0 );  /* No collision on union u2 */
    }
  }
}

/*
** Append the contents of SrcList p2 to SrcList p1 and return the resulting
** SrcList. Or, if an error occurs, return NULL. In all cases, p1 and p2
** are deleted by this function.
*/
SQLITE_PRIVATE SrcList *sqlite3SrcListAppendList(Parse *pParse, SrcList *p1, SrcList *p2){
  assert( p1 && p1->nSrc==1 );
  if( p2 ){
    SrcList *pNew = sqlite3SrcListEnlarge(pParse, p1, p2->nSrc, 1);
    if( pNew==0 ){
      sqlite3SrcListDelete(pParse->db, p2);
    }else{
      p1 = pNew;
      memcpy(&p1->a[1], p2->a, p2->nSrc*sizeof(SrcItem));
      sqlite3DbFree(pParse->db, p2);
      p1->a[0].fg.jointype |= (JT_LTORJ & p1->a[1].fg.jointype);
    }
  }
  return p1;
}

/*
** Add the list of function arguments to the SrcList entry for a
** table-valued-function.
*/
SQLITE_PRIVATE void sqlite3SrcListFuncArgs(Parse *pParse, SrcList *p, ExprList *pList){
  if( p ){
    SrcItem *pItem = &p->a[p->nSrc-1];
    assert( pItem->fg.notIndexed==0 );
    assert( pItem->fg.isIndexedBy==0 );
    assert( pItem->fg.isTabFunc==0 );
    pItem->u1.pFuncArg = pList;
    pItem->fg.isTabFunc = 1;
  }else{
    sqlite3ExprListDelete(pParse->db, pList);
  }
}

/*
** When building up a FROM clause in the parser, the join operator
** is initially attached to the left operand.  But the code generator
** expects the join operator to be on the right operand.  This routine
** Shifts all join operators from left to right for an entire FROM
** clause.
**
** Example: Suppose the join is like this:
**
**           A natural cross join B
**
** The operator is "natural cross join".  The A and B operands are stored
** in p->a[0] and p->a[1], respectively.  The parser initially stores the
** operator with A.  This routine shifts that operator over to B.
**
** Additional changes:
**
**   *   All tables to the left of the right-most RIGHT JOIN are tagged with
**       JT_LTORJ (mnemonic: Left Table Of Right Join) so that the
**       code generator can easily tell that the table is part of
**       the left operand of at least one RIGHT JOIN.
*/
SQLITE_PRIVATE void sqlite3SrcListShiftJoinType(Parse *pParse, SrcList *p){
  (void)pParse;
  if( p && p->nSrc>1 ){
    int i = p->nSrc-1;
    u8 allFlags = 0;
    do{
      allFlags |= p->a[i].fg.jointype = p->a[i-1].fg.jointype;
    }while( (--i)>0 );
    p->a[0].fg.jointype = 0;

    /* All terms to the left of a RIGHT JOIN should be tagged with the
    ** JT_LTORJ flags */
    if( allFlags & JT_RIGHT ){
      for(i=p->nSrc-1; ALWAYS(i>0) && (p->a[i].fg.jointype&JT_RIGHT)==0; i--){}
      i--;
      assert( i>=0 );
      do{
        p->a[i].fg.jointype |= JT_LTORJ;
      }while( (--i)>=0 );
    }
  }
}

/*
** Generate VDBE code for a BEGIN statement.
*/
SQLITE_PRIVATE void sqlite3BeginTransaction(Parse *pParse, int type){
  sqlite3 *db;
  Vdbe *v;
  int i;

  assert( pParse!=0 );
  db = pParse->db;
  assert( db!=0 );
  if( sqlite3AuthCheck(pParse, SQLITE_TRANSACTION, "BEGIN", 0, 0) ){
    return;
  }
  v = sqlite3GetVdbe(pParse);
  if( !v ) return;
  if( type!=TK_DEFERRED ){
    for(i=0; i<db->nDb; i++){
      int eTxnType;
      Btree *pBt = db->aDb[i].pBt;
      if( pBt && sqlite3BtreeIsReadonly(pBt) ){
        eTxnType = 0;  /* Read txn */
      }else if( type==TK_EXCLUSIVE ){
        eTxnType = 2;  /* Exclusive txn */
      }else{
        eTxnType = 1;  /* Write txn */
      }
      sqlite3VdbeAddOp2(v, OP_Transaction, i, eTxnType);
      sqlite3VdbeUsesBtree(v, i);
    }
  }
  sqlite3VdbeAddOp0(v, OP_AutoCommit);
}

/*
** Generate VDBE code for a COMMIT or ROLLBACK statement.
** Code for ROLLBACK is generated if eType==TK_ROLLBACK.  Otherwise
** code is generated for a COMMIT.
*/
SQLITE_PRIVATE void sqlite3EndTransaction(Parse *pParse, int eType){
  Vdbe *v;
  int isRollback;

  assert( pParse!=0 );
  assert( pParse->db!=0 );
  assert( eType==TK_COMMIT || eType==TK_END || eType==TK_ROLLBACK );
  isRollback = eType==TK_ROLLBACK;
  if( sqlite3AuthCheck(pParse, SQLITE_TRANSACTION,
       isRollback ? "ROLLBACK" : "COMMIT", 0, 0) ){
    return;
  }
  v = sqlite3GetVdbe(pParse);
  if( v ){
    sqlite3VdbeAddOp2(v, OP_AutoCommit, 1, isRollback);
  }
}

/*
** This function is called by the parser when it parses a command to create,
** release or rollback an SQL savepoint.
*/
SQLITE_PRIVATE void sqlite3Savepoint(Parse *pParse, int op, Token *pName){
  char *zName = sqlite3NameFromToken(pParse->db, pName);
  if( zName ){
    Vdbe *v = sqlite3GetVdbe(pParse);
#ifndef SQLITE_OMIT_AUTHORIZATION
    static const char * const az[] = { "BEGIN", "RELEASE", "ROLLBACK" };
    assert( !SAVEPOINT_BEGIN && SAVEPOINT_RELEASE==1 && SAVEPOINT_ROLLBACK==2 );
#endif
    if( !v || sqlite3AuthCheck(pParse, SQLITE_SAVEPOINT, az[op], zName, 0) ){
      sqlite3DbFree(pParse->db, zName);
      return;
    }
    sqlite3VdbeAddOp4(v, OP_Savepoint, op, 0, 0, zName, P4_DYNAMIC);
  }
}

/*
** Make sure the TEMP database is open and available for use.  Return
** the number of errors.  Leave any error messages in the pParse structure.
*/
SQLITE_PRIVATE int sqlite3OpenTempDatabase(Parse *pParse){
  sqlite3 *db = pParse->db;
  if( db->aDb[1].pBt==0 && !pParse->explain ){
    int rc;
    Btree *pBt;
    static const int flags =
          SQLITE_OPEN_READWRITE |
          SQLITE_OPEN_CREATE |
          SQLITE_OPEN_EXCLUSIVE |
          SQLITE_OPEN_DELETEONCLOSE |
          SQLITE_OPEN_TEMP_DB;

    rc = sqlite3BtreeOpen(db->pVfs, 0, db, &pBt, 0, flags);
    if( rc!=SQLITE_OK ){
      sqlite3ErrorMsg(pParse, "unable to open a temporary database "
        "file for storing temporary tables");
      pParse->rc = rc;
      return 1;
    }
    db->aDb[1].pBt = pBt;
    assert( db->aDb[1].pSchema );
    if( SQLITE_NOMEM==sqlite3BtreeSetPageSize(pBt, db->nextPagesize, 0, 0) ){
      sqlite3OomFault(db);
      return 1;
    }
  }
  return 0;
}

/*
** Record the fact that the schema cookie will need to be verified
** for database iDb.  The code to actually verify the schema cookie
** will occur at the end of the top-level VDBE and will be generated
** later, by sqlite3FinishCoding().
*/
static void sqlite3CodeVerifySchemaAtToplevel(Parse *pToplevel, int iDb){
  assert( iDb>=0 && iDb<pToplevel->db->nDb );
  assert( pToplevel->db->aDb[iDb].pBt!=0 || iDb==1 );
  assert( iDb<SQLITE_MAX_DB );
  assert( sqlite3SchemaMutexHeld(pToplevel->db, iDb, 0) );
  if( DbMaskTest(pToplevel->cookieMask, iDb)==0 ){
    DbMaskSet(pToplevel->cookieMask, iDb);
    if( !OMIT_TEMPDB && iDb==1 ){
      sqlite3OpenTempDatabase(pToplevel);
    }
  }
}
