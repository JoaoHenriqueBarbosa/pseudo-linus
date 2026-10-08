
/*
** Return the name of a wildcard parameter.  Return NULL if the index
** is out of range or if the wildcard is unnamed.
**
** The result is always UTF-8.
*/
SQLITE_API const char *sqlite3_bind_parameter_name(sqlite3_stmt *pStmt, int i){
  Vdbe *p = (Vdbe*)pStmt;
  if( p==0 ) return 0;
  return sqlite3VListNumToName(p->pVList, i);
}

/*
** Given a wildcard parameter name, return the index of the variable
** with that name.  If there is no variable with the given name,
** return 0.
*/
SQLITE_PRIVATE int sqlite3VdbeParameterIndex(Vdbe *p, const char *zName, int nName){
  if( p==0 || zName==0 ) return 0;
  return sqlite3VListNameToNum(p->pVList, zName, nName);
}
SQLITE_API int sqlite3_bind_parameter_index(sqlite3_stmt *pStmt, const char *zName){
  return sqlite3VdbeParameterIndex((Vdbe*)pStmt, zName, sqlite3Strlen30(zName));
}

/*
** Transfer all bindings from the first statement over to the second.
*/
SQLITE_PRIVATE int sqlite3TransferBindings(sqlite3_stmt *pFromStmt, sqlite3_stmt *pToStmt){
  Vdbe *pFrom = (Vdbe*)pFromStmt;
  Vdbe *pTo = (Vdbe*)pToStmt;
  int i;
  assert( pTo->db==pFrom->db );
  assert( pTo->nVar==pFrom->nVar );
  sqlite3_mutex_enter(pTo->db->mutex);
  for(i=0; i<pFrom->nVar; i++){
    sqlite3VdbeMemMove(&pTo->aVar[i], &pFrom->aVar[i]);
  }
  sqlite3_mutex_leave(pTo->db->mutex);
  return SQLITE_OK;
}

#ifndef SQLITE_OMIT_DEPRECATED
/*
** Deprecated external interface.  Internal/core SQLite code
** should call sqlite3TransferBindings.
**
** It is misuse to call this routine with statements from different
** database connections.  But as this is a deprecated interface, we
** will not bother to check for that condition.
**
** If the two statements contain a different number of bindings, then
** an SQLITE_ERROR is returned.  Nothing else can go wrong, so otherwise
** SQLITE_OK is returned.
*/
SQLITE_API int sqlite3_transfer_bindings(sqlite3_stmt *pFromStmt, sqlite3_stmt *pToStmt){
  Vdbe *pFrom = (Vdbe*)pFromStmt;
  Vdbe *pTo = (Vdbe*)pToStmt;
  if( pFrom->nVar!=pTo->nVar ){
    return SQLITE_ERROR;
  }
  assert( (pTo->prepFlags & SQLITE_PREPARE_SAVESQL)!=0 || pTo->expmask==0 );
  if( pTo->expmask ){
    pTo->expired = 1;
  }
  assert( (pFrom->prepFlags & SQLITE_PREPARE_SAVESQL)!=0 || pFrom->expmask==0 );
  if( pFrom->expmask ){
    pFrom->expired = 1;
  }
  return sqlite3TransferBindings(pFromStmt, pToStmt);
}
#endif

/*
** Return the sqlite3* database handle to which the prepared statement given
** in the argument belongs.  This is the same database handle that was
** the first argument to the sqlite3_prepare() that was used to create
** the statement in the first place.
*/
SQLITE_API sqlite3 *sqlite3_db_handle(sqlite3_stmt *pStmt){
  return pStmt ? ((Vdbe*)pStmt)->db : 0;
}

/*
** Return true if the prepared statement is guaranteed to not modify the
** database.
*/
SQLITE_API int sqlite3_stmt_readonly(sqlite3_stmt *pStmt){
  return pStmt ? ((Vdbe*)pStmt)->readOnly : 1;
}

/*
** Return 1 if the statement is an EXPLAIN and return 2 if the
** statement is an EXPLAIN QUERY PLAN
*/
SQLITE_API int sqlite3_stmt_isexplain(sqlite3_stmt *pStmt){
  return pStmt ? ((Vdbe*)pStmt)->explain : 0;
}

/*
** Set the explain mode for a statement.
*/
SQLITE_API int sqlite3_stmt_explain(sqlite3_stmt *pStmt, int eMode){
  Vdbe *v = (Vdbe*)pStmt;
  int rc;
#ifdef SQLITE_ENABLE_API_ARMOR
  if( pStmt==0 ) return SQLITE_MISUSE_BKPT;
#endif
  sqlite3_mutex_enter(v->db->mutex);
  if( ((int)v->explain)==eMode ){
    rc = SQLITE_OK;
  }else if( eMode<0 || eMode>2 ){
    rc = SQLITE_ERROR;
  }else if( (v->prepFlags & SQLITE_PREPARE_SAVESQL)==0 ){
    rc = SQLITE_ERROR;
  }else if( v->eVdbeState!=VDBE_READY_STATE ){
    rc = SQLITE_BUSY;
  }else if( v->nMem>=10 && (eMode!=2 || v->haveEqpOps) ){
    /* No reprepare necessary */
    v->explain = eMode;
    rc = SQLITE_OK;
  }else{
    v->explain = eMode;
    rc = sqlite3Reprepare(v);
    v->haveEqpOps = eMode==2;
  }
  if( v->explain ){
    v->nResColumn = 12 - 4*v->explain;
  }else{
    v->nResColumn = v->nResAlloc;
  }
  sqlite3_mutex_leave(v->db->mutex);
  return rc;
}

/*
** Return true if the prepared statement is in need of being reset.
*/
SQLITE_API int sqlite3_stmt_busy(sqlite3_stmt *pStmt){
  Vdbe *v = (Vdbe*)pStmt;
  return v!=0 && v->eVdbeState==VDBE_RUN_STATE;
}

/*
** Return a pointer to the next prepared statement after pStmt associated
** with database connection pDb.  If pStmt is NULL, return the first
** prepared statement for the database connection.  Return NULL if there
** are no more.
*/
SQLITE_API sqlite3_stmt *sqlite3_next_stmt(sqlite3 *pDb, sqlite3_stmt *pStmt){
  sqlite3_stmt *pNext;
#ifdef SQLITE_ENABLE_API_ARMOR
  if( !sqlite3SafetyCheckOk(pDb) ){
    (void)SQLITE_MISUSE_BKPT;
    return 0;
  }
#endif
  sqlite3_mutex_enter(pDb->mutex);
  if( pStmt==0 ){
    pNext = (sqlite3_stmt*)pDb->pVdbe;
  }else{
    pNext = (sqlite3_stmt*)((Vdbe*)pStmt)->pVNext;
  }
  sqlite3_mutex_leave(pDb->mutex);
  return pNext;
}

/*
** Return the value of a status counter for a prepared statement
*/
SQLITE_API int sqlite3_stmt_status(sqlite3_stmt *pStmt, int op, int resetFlag){
  Vdbe *pVdbe = (Vdbe*)pStmt;
  u32 v;
#ifdef SQLITE_ENABLE_API_ARMOR
  if( !pStmt
   || (op!=SQLITE_STMTSTATUS_MEMUSED && (op<0||op>=ArraySize(pVdbe->aCounter)))
  ){
    (void)SQLITE_MISUSE_BKPT;
    return 0;
  }
#endif
  if( op==SQLITE_STMTSTATUS_MEMUSED ){
    sqlite3 *db = pVdbe->db;
    sqlite3_mutex_enter(db->mutex);
    v = 0;
    db->pnBytesFreed = (int*)&v;
    assert( db->lookaside.pEnd==db->lookaside.pTrueEnd );
    db->lookaside.pEnd = db->lookaside.pStart;
    sqlite3VdbeDelete(pVdbe);
    db->pnBytesFreed = 0;
    db->lookaside.pEnd = db->lookaside.pTrueEnd;
    sqlite3_mutex_leave(db->mutex);
  }else{
    v = pVdbe->aCounter[op];
    if( resetFlag ) pVdbe->aCounter[op] = 0;
  }
  return (int)v;
}

/*
** Return the SQL associated with a prepared statement
*/
SQLITE_API const char *sqlite3_sql(sqlite3_stmt *pStmt){
  Vdbe *p = (Vdbe *)pStmt;
  return p ? p->zSql : 0;
}

/*
** Return the SQL associated with a prepared statement with
** bound parameters expanded.  Space to hold the returned string is
** obtained from sqlite3_malloc().  The caller is responsible for
** freeing the returned string by passing it to sqlite3_free().
**
** The SQLITE_TRACE_SIZE_LIMIT puts an upper bound on the size of
** expanded bound parameters.
*/
SQLITE_API char *sqlite3_expanded_sql(sqlite3_stmt *pStmt){
#ifdef SQLITE_OMIT_TRACE
  return 0;
#else
  char *z = 0;
  const char *zSql = sqlite3_sql(pStmt);
  if( zSql ){
    Vdbe *p = (Vdbe *)pStmt;
    sqlite3_mutex_enter(p->db->mutex);
    z = sqlite3VdbeExpandSql(p, zSql);
    sqlite3_mutex_leave(p->db->mutex);
  }
  return z;
#endif
}

#ifdef SQLITE_ENABLE_NORMALIZE
/*
** Return the normalized SQL associated with a prepared statement.
*/
SQLITE_API const char *sqlite3_normalized_sql(sqlite3_stmt *pStmt){
  Vdbe *p = (Vdbe *)pStmt;
  if( p==0 ) return 0;
  if( p->zNormSql==0 && ALWAYS(p->zSql!=0) ){
    sqlite3_mutex_enter(p->db->mutex);
    p->zNormSql = sqlite3Normalize(p, p->zSql);
    sqlite3_mutex_leave(p->db->mutex);
  }
  return p->zNormSql;
}
#endif /* SQLITE_ENABLE_NORMALIZE */

#ifdef SQLITE_ENABLE_PREUPDATE_HOOK
/*
** Allocate and populate an UnpackedRecord structure based on the serialized
** record in nKey/pKey. Return a pointer to the new UnpackedRecord structure
** if successful, or a NULL pointer if an OOM error is encountered.
*/
static UnpackedRecord *vdbeUnpackRecord(
  KeyInfo *pKeyInfo,
  int nKey,
  const void *pKey
){
  UnpackedRecord *pRet;           /* Return value */

  pRet = sqlite3VdbeAllocUnpackedRecord(pKeyInfo);
  if( pRet ){
    memset(pRet->aMem, 0, sizeof(Mem)*(pKeyInfo->nKeyField+1));
    sqlite3VdbeRecordUnpack(pKeyInfo, nKey, pKey, pRet);
  }
  return pRet;
}

/*
** This function is called from within a pre-update callback to retrieve
** a field of the row currently being updated or deleted.
*/
SQLITE_API int sqlite3_preupdate_old(sqlite3 *db, int iIdx, sqlite3_value **ppValue){
  PreUpdate *p;
  Mem *pMem;
  int rc = SQLITE_OK;

#ifdef SQLITE_ENABLE_API_ARMOR
  if( db==0 || ppValue==0 ){
    return SQLITE_MISUSE_BKPT;
  }
#endif
  p = db->pPreUpdate;
  /* Test that this call is being made from within an SQLITE_DELETE or
  ** SQLITE_UPDATE pre-update callback, and that iIdx is within range. */
  if( !p || p->op==SQLITE_INSERT ){
    rc = SQLITE_MISUSE_BKPT;
    goto preupdate_old_out;
  }
  if( p->pPk ){
    iIdx = sqlite3TableColumnToIndex(p->pPk, iIdx);
  }
  if( iIdx>=p->pCsr->nField || iIdx<0 ){
    rc = SQLITE_RANGE;
    goto preupdate_old_out;
  }

  /* If the old.* record has not yet been loaded into memory, do so now. */
  if( p->pUnpacked==0 ){
    u32 nRec;
    u8 *aRec;

    assert( p->pCsr->eCurType==CURTYPE_BTREE );
    nRec = sqlite3BtreePayloadSize(p->pCsr->uc.pCursor);
    aRec = sqlite3DbMallocRaw(db, nRec);
    if( !aRec ) goto preupdate_old_out;
    rc = sqlite3BtreePayload(p->pCsr->uc.pCursor, 0, nRec, aRec);
    if( rc==SQLITE_OK ){
      p->pUnpacked = vdbeUnpackRecord(&p->keyinfo, nRec, aRec);
      if( !p->pUnpacked ) rc = SQLITE_NOMEM;
    }
    if( rc!=SQLITE_OK ){
      sqlite3DbFree(db, aRec);
      goto preupdate_old_out;
    }
    p->aRecord = aRec;
  }

  pMem = *ppValue = &p->pUnpacked->aMem[iIdx];
  if( iIdx==p->pTab->iPKey ){
    sqlite3VdbeMemSetInt64(pMem, p->iKey1);
  }else if( iIdx>=p->pUnpacked->nField ){
    *ppValue = (sqlite3_value *)columnNullValue();
  }else if( p->pTab->aCol[iIdx].affinity==SQLITE_AFF_REAL ){
    if( pMem->flags & (MEM_Int|MEM_IntReal) ){
      testcase( pMem->flags & MEM_Int );
      testcase( pMem->flags & MEM_IntReal );
      sqlite3VdbeMemRealify(pMem);
    }
  }

 preupdate_old_out:
  sqlite3Error(db, rc);
  return sqlite3ApiExit(db, rc);
}
#endif /* SQLITE_ENABLE_PREUPDATE_HOOK */

#ifdef SQLITE_ENABLE_PREUPDATE_HOOK
/*
** This function is called from within a pre-update callback to retrieve
** the number of columns in the row being updated, deleted or inserted.
*/
SQLITE_API int sqlite3_preupdate_count(sqlite3 *db){
  PreUpdate *p;
#ifdef SQLITE_ENABLE_API_ARMOR
  p = db!=0 ? db->pPreUpdate : 0;
#else
  p = db->pPreUpdate;
#endif
  return (p ? p->keyinfo.nKeyField : 0);
}
