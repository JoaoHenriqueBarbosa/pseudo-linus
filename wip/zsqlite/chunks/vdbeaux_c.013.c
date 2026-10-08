
/*
** Set a flag in the vdbe to update the change counter when it is finalised
** or reset.
*/
SQLITE_PRIVATE void sqlite3VdbeCountChanges(Vdbe *v){
  v->changeCntOn = 1;
}

/*
** Mark every prepared statement associated with a database connection
** as expired.
**
** An expired statement means that recompilation of the statement is
** recommend.  Statements expire when things happen that make their
** programs obsolete.  Removing user-defined functions or collating
** sequences, or changing an authorization function are the types of
** things that make prepared statements obsolete.
**
** If iCode is 1, then expiration is advisory.  The statement should
** be reprepared before being restarted, but if it is already running
** it is allowed to run to completion.
**
** Internally, this function just sets the Vdbe.expired flag on all
** prepared statements.  The flag is set to 1 for an immediate expiration
** and set to 2 for an advisory expiration.
*/
SQLITE_PRIVATE void sqlite3ExpirePreparedStatements(sqlite3 *db, int iCode){
  Vdbe *p;
  for(p = db->pVdbe; p; p=p->pVNext){
    p->expired = iCode+1;
  }
}

/*
** Return the database associated with the Vdbe.
*/
SQLITE_PRIVATE sqlite3 *sqlite3VdbeDb(Vdbe *v){
  return v->db;
}

/*
** Return the SQLITE_PREPARE flags for a Vdbe.
*/
SQLITE_PRIVATE u8 sqlite3VdbePrepareFlags(Vdbe *v){
  return v->prepFlags;
}

/*
** Return a pointer to an sqlite3_value structure containing the value bound
** parameter iVar of VM v. Except, if the value is an SQL NULL, return
** 0 instead. Unless it is NULL, apply affinity aff (one of the SQLITE_AFF_*
** constants) to the value before returning it.
**
** The returned value must be freed by the caller using sqlite3ValueFree().
*/
SQLITE_PRIVATE sqlite3_value *sqlite3VdbeGetBoundValue(Vdbe *v, int iVar, u8 aff){
  assert( iVar>0 );
  if( v ){
    Mem *pMem = &v->aVar[iVar-1];
    assert( (v->db->flags & SQLITE_EnableQPSG)==0
         || (v->db->mDbFlags & DBFLAG_InternalFunc)!=0 );
    if( 0==(pMem->flags & MEM_Null) ){
      sqlite3_value *pRet = sqlite3ValueNew(v->db);
      if( pRet ){
        sqlite3VdbeMemCopy((Mem *)pRet, pMem);
        sqlite3ValueApplyAffinity(pRet, aff, SQLITE_UTF8);
      }
      return pRet;
    }
  }
  return 0;
}

/*
** Configure SQL variable iVar so that binding a new value to it signals
** to sqlite3_reoptimize() that re-preparing the statement may result
** in a better query plan.
*/
SQLITE_PRIVATE void sqlite3VdbeSetVarmask(Vdbe *v, int iVar){
  assert( iVar>0 );
  assert( (v->db->flags & SQLITE_EnableQPSG)==0
       || (v->db->mDbFlags & DBFLAG_InternalFunc)!=0 );
  if( iVar>=32 ){
    v->expmask |= 0x80000000;
  }else{
    v->expmask |= ((u32)1 << (iVar-1));
  }
}

/*
** Cause a function to throw an error if it was call from OP_PureFunc
** rather than OP_Function.
**
** OP_PureFunc means that the function must be deterministic, and should
** throw an error if it is given inputs that would make it non-deterministic.
** This routine is invoked by date/time functions that use non-deterministic
** features such as 'now'.
*/
SQLITE_PRIVATE int sqlite3NotPureFunc(sqlite3_context *pCtx){
  const VdbeOp *pOp;
#ifdef SQLITE_ENABLE_STAT4
  if( pCtx->pVdbe==0 ) return 1;
#endif
  pOp = pCtx->pVdbe->aOp + pCtx->iOp;
  if( pOp->opcode==OP_PureFunc ){
    const char *zContext;
    char *zMsg;
    if( pOp->p5 & NC_IsCheck ){
      zContext = "a CHECK constraint";
    }else if( pOp->p5 & NC_GenCol ){
      zContext = "a generated column";
    }else{
      zContext = "an index";
    }
    zMsg = sqlite3_mprintf("non-deterministic use of %s() in %s",
                           pCtx->pFunc->zName, zContext);
    sqlite3_result_error(pCtx, zMsg, -1);
    sqlite3_free(zMsg);
    return 0;
  }
  return 1;
}

#if defined(SQLITE_ENABLE_CURSOR_HINTS) && defined(SQLITE_DEBUG)
/*
** This Walker callback is used to help verify that calls to
** sqlite3BtreeCursorHint() with opcode BTREE_HINT_RANGE have
** byte-code register values correctly initialized.
*/
SQLITE_PRIVATE int sqlite3CursorRangeHintExprCheck(Walker *pWalker, Expr *pExpr){
  if( pExpr->op==TK_REGISTER ){
    assert( (pWalker->u.aMem[pExpr->iTable].flags & MEM_Undefined)==0 );
  }
  return WRC_Continue;
}
#endif /* SQLITE_ENABLE_CURSOR_HINTS && SQLITE_DEBUG */

#ifndef SQLITE_OMIT_VIRTUALTABLE
/*
** Transfer error message text from an sqlite3_vtab.zErrMsg (text stored
** in memory obtained from sqlite3_malloc) into a Vdbe.zErrMsg (text stored
** in memory obtained from sqlite3DbMalloc).
*/
SQLITE_PRIVATE void sqlite3VtabImportErrmsg(Vdbe *p, sqlite3_vtab *pVtab){
  if( pVtab->zErrMsg ){
    sqlite3 *db = p->db;
    sqlite3DbFree(db, p->zErrMsg);
    p->zErrMsg = sqlite3DbStrDup(db, pVtab->zErrMsg);
    sqlite3_free(pVtab->zErrMsg);
    pVtab->zErrMsg = 0;
  }
}
#endif /* SQLITE_OMIT_VIRTUALTABLE */

#ifdef SQLITE_ENABLE_PREUPDATE_HOOK

/*
** If the second argument is not NULL, release any allocations associated
** with the memory cells in the p->aMem[] array. Also free the UnpackedRecord
** structure itself, using sqlite3DbFree().
**
** This function is used to free UnpackedRecord structures allocated by
** the vdbeUnpackRecord() function found in vdbeapi.c.
*/
static void vdbeFreeUnpacked(sqlite3 *db, int nField, UnpackedRecord *p){
  assert( db!=0 );
  if( p ){
    int i;
    for(i=0; i<nField; i++){
      Mem *pMem = &p->aMem[i];
      if( pMem->zMalloc ) sqlite3VdbeMemReleaseMalloc(pMem);
    }
    sqlite3DbNNFreeNN(db, p);
  }
}
#endif /* SQLITE_ENABLE_PREUPDATE_HOOK */

#ifdef SQLITE_ENABLE_PREUPDATE_HOOK
/*
** Invoke the pre-update hook. If this is an UPDATE or DELETE pre-update call,
** then cursor passed as the second argument should point to the row about
** to be update or deleted. If the application calls sqlite3_preupdate_old(),
** the required value will be read from the row the cursor points to.
*/
SQLITE_PRIVATE void sqlite3VdbePreUpdateHook(
  Vdbe *v,                        /* Vdbe pre-update hook is invoked by */
  VdbeCursor *pCsr,               /* Cursor to grab old.* values from */
  int op,                         /* SQLITE_INSERT, UPDATE or DELETE */
  const char *zDb,                /* Database name */
  Table *pTab,                    /* Modified table */
  i64 iKey1,                      /* Initial key value */
  int iReg,                       /* Register for new.* record */
  int iBlobWrite
){
  sqlite3 *db = v->db;
  i64 iKey2;
  PreUpdate preupdate;
  const char *zTbl = pTab->zName;
  static const u8 fakeSortOrder = 0;
#ifdef SQLITE_DEBUG
  int nRealCol;
  if( pTab->tabFlags & TF_WithoutRowid ){
    nRealCol = sqlite3PrimaryKeyIndex(pTab)->nColumn;
  }else if( pTab->tabFlags & TF_HasVirtual ){
    nRealCol = pTab->nNVCol;
  }else{
    nRealCol = pTab->nCol;
  }
#endif

  assert( db->pPreUpdate==0 );
  memset(&preupdate, 0, sizeof(PreUpdate));
  if( HasRowid(pTab)==0 ){
    iKey1 = iKey2 = 0;
    preupdate.pPk = sqlite3PrimaryKeyIndex(pTab);
  }else{
    if( op==SQLITE_UPDATE ){
      iKey2 = v->aMem[iReg].u.i;
    }else{
      iKey2 = iKey1;
    }
  }

  assert( pCsr!=0 );
  assert( pCsr->eCurType==CURTYPE_BTREE );
  assert( pCsr->nField==nRealCol
       || (pCsr->nField==nRealCol+1 && op==SQLITE_DELETE && iReg==-1)
  );

  preupdate.v = v;
  preupdate.pCsr = pCsr;
  preupdate.op = op;
  preupdate.iNewReg = iReg;
  preupdate.keyinfo.db = db;
  preupdate.keyinfo.enc = ENC(db);
  preupdate.keyinfo.nKeyField = pTab->nCol;
  preupdate.keyinfo.aSortFlags = (u8*)&fakeSortOrder;
  preupdate.iKey1 = iKey1;
  preupdate.iKey2 = iKey2;
  preupdate.pTab = pTab;
  preupdate.iBlobWrite = iBlobWrite;

  db->pPreUpdate = &preupdate;
  db->xPreUpdateCallback(db->pPreUpdateArg, db, op, zDb, zTbl, iKey1, iKey2);
  db->pPreUpdate = 0;
  sqlite3DbFree(db, preupdate.aRecord);
  vdbeFreeUnpacked(db, preupdate.keyinfo.nKeyField+1, preupdate.pUnpacked);
  vdbeFreeUnpacked(db, preupdate.keyinfo.nKeyField+1, preupdate.pNewUnpacked);
  if( preupdate.aNew ){
    int i;
    for(i=0; i<pCsr->nField; i++){
      sqlite3VdbeMemRelease(&preupdate.aNew[i]);
    }
    sqlite3DbNNFreeNN(db, preupdate.aNew);
  }
}
#endif /* SQLITE_ENABLE_PREUPDATE_HOOK */

