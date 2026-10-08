#endif /* SQLITE_ENABLE_PREUPDATE_HOOK */

#ifdef SQLITE_ENABLE_PREUPDATE_HOOK
/*
** This function is designed to be called from within a pre-update callback
** only. It returns zero if the change that caused the callback was made
** immediately by a user SQL statement. Or, if the change was made by a
** trigger program, it returns the number of trigger programs currently
** on the stack (1 for a top-level trigger, 2 for a trigger fired by a
** top-level trigger etc.).
**
** For the purposes of the previous paragraph, a foreign key CASCADE, SET NULL
** or SET DEFAULT action is considered a trigger.
*/
SQLITE_API int sqlite3_preupdate_depth(sqlite3 *db){
  PreUpdate *p;
#ifdef SQLITE_ENABLE_API_ARMOR
  p = db!=0 ? db->pPreUpdate : 0;
#else
  p = db->pPreUpdate;
#endif
  return (p ? p->v->nFrame : 0);
}
#endif /* SQLITE_ENABLE_PREUPDATE_HOOK */

#ifdef SQLITE_ENABLE_PREUPDATE_HOOK
/*
** This function is designed to be called from within a pre-update callback
** only.
*/
SQLITE_API int sqlite3_preupdate_blobwrite(sqlite3 *db){
  PreUpdate *p;
#ifdef SQLITE_ENABLE_API_ARMOR
  p = db!=0 ? db->pPreUpdate : 0;
#else
  p = db->pPreUpdate;
#endif
  return (p ? p->iBlobWrite : -1);
}
#endif

#ifdef SQLITE_ENABLE_PREUPDATE_HOOK
/*
** This function is called from within a pre-update callback to retrieve
** a field of the row currently being updated or inserted.
*/
SQLITE_API int sqlite3_preupdate_new(sqlite3 *db, int iIdx, sqlite3_value **ppValue){
  PreUpdate *p;
  int rc = SQLITE_OK;
  Mem *pMem;

#ifdef SQLITE_ENABLE_API_ARMOR
  if( db==0 || ppValue==0 ){
    return SQLITE_MISUSE_BKPT;
  }
#endif
  p = db->pPreUpdate;
  if( !p || p->op==SQLITE_DELETE ){
    rc = SQLITE_MISUSE_BKPT;
    goto preupdate_new_out;
  }
  if( p->pPk && p->op!=SQLITE_UPDATE ){
    iIdx = sqlite3TableColumnToIndex(p->pPk, iIdx);
  }
  if( iIdx>=p->pCsr->nField || iIdx<0 ){
    rc = SQLITE_RANGE;
    goto preupdate_new_out;
  }

  if( p->op==SQLITE_INSERT ){
    /* For an INSERT, memory cell p->iNewReg contains the serialized record
    ** that is being inserted. Deserialize it. */
    UnpackedRecord *pUnpack = p->pNewUnpacked;
    if( !pUnpack ){
      Mem *pData = &p->v->aMem[p->iNewReg];
      rc = ExpandBlob(pData);
      if( rc!=SQLITE_OK ) goto preupdate_new_out;
      pUnpack = vdbeUnpackRecord(&p->keyinfo, pData->n, pData->z);
      if( !pUnpack ){
        rc = SQLITE_NOMEM;
        goto preupdate_new_out;
      }
      p->pNewUnpacked = pUnpack;
    }
    pMem = &pUnpack->aMem[iIdx];
    if( iIdx==p->pTab->iPKey ){
      sqlite3VdbeMemSetInt64(pMem, p->iKey2);
    }else if( iIdx>=pUnpack->nField ){
      pMem = (sqlite3_value *)columnNullValue();
    }
  }else{
    /* For an UPDATE, memory cell (p->iNewReg+1+iIdx) contains the required
    ** value. Make a copy of the cell contents and return a pointer to it.
    ** It is not safe to return a pointer to the memory cell itself as the
    ** caller may modify the value text encoding.
    */
    assert( p->op==SQLITE_UPDATE );
    if( !p->aNew ){
      p->aNew = (Mem *)sqlite3DbMallocZero(db, sizeof(Mem) * p->pCsr->nField);
      if( !p->aNew ){
        rc = SQLITE_NOMEM;
        goto preupdate_new_out;
      }
    }
    assert( iIdx>=0 && iIdx<p->pCsr->nField );
    pMem = &p->aNew[iIdx];
    if( pMem->flags==0 ){
      if( iIdx==p->pTab->iPKey ){
        sqlite3VdbeMemSetInt64(pMem, p->iKey2);
      }else{
        rc = sqlite3VdbeMemCopy(pMem, &p->v->aMem[p->iNewReg+1+iIdx]);
        if( rc!=SQLITE_OK ) goto preupdate_new_out;
      }
    }
  }
  *ppValue = pMem;

 preupdate_new_out:
  sqlite3Error(db, rc);
  return sqlite3ApiExit(db, rc);
}
#endif /* SQLITE_ENABLE_PREUPDATE_HOOK */

#ifdef SQLITE_ENABLE_STMT_SCANSTATUS
/*
** Return status data for a single loop within query pStmt.
*/
SQLITE_API int sqlite3_stmt_scanstatus_v2(
  sqlite3_stmt *pStmt,            /* Prepared statement being queried */
  int iScan,                      /* Index of loop to report on */
  int iScanStatusOp,              /* Which metric to return */
  int flags,
  void *pOut                      /* OUT: Write the answer here */
){
  Vdbe *p = (Vdbe*)pStmt;
  VdbeOp *aOp;
  int nOp;
  ScanStatus *pScan = 0;
  int idx;

#ifdef SQLITE_ENABLE_API_ARMOR
  if( p==0 || pOut==0
      || iScanStatusOp<SQLITE_SCANSTAT_NLOOP
      || iScanStatusOp>SQLITE_SCANSTAT_NCYCLE ){
    return 1;
  }
#endif
  aOp = p->aOp;
  nOp = p->nOp;
  if( p->pFrame ){
    VdbeFrame *pFrame;
    for(pFrame=p->pFrame; pFrame->pParent; pFrame=pFrame->pParent);
    aOp = pFrame->aOp;
    nOp = pFrame->nOp;
  }

  if( iScan<0 ){
    int ii;
    if( iScanStatusOp==SQLITE_SCANSTAT_NCYCLE ){
      i64 res = 0;
      for(ii=0; ii<nOp; ii++){
        res += aOp[ii].nCycle;
      }
      *(i64*)pOut = res;
      return 0;
    }
    return 1;
  }
  if( flags & SQLITE_SCANSTAT_COMPLEX ){
    idx = iScan;
  }else{
    /* If the COMPLEX flag is clear, then this function must ignore any
    ** ScanStatus structures with ScanStatus.addrLoop set to 0. */
    for(idx=0; idx<p->nScan; idx++){
      pScan = &p->aScan[idx];
      if( pScan->zName ){
        iScan--;
        if( iScan<0 ) break;
      }
    }
  }
  if( idx>=p->nScan ) return 1;
  assert( pScan==0 || pScan==&p->aScan[idx] );
  pScan = &p->aScan[idx];

  switch( iScanStatusOp ){
    case SQLITE_SCANSTAT_NLOOP: {
      if( pScan->addrLoop>0 ){
        *(sqlite3_int64*)pOut = aOp[pScan->addrLoop].nExec;
      }else{
        *(sqlite3_int64*)pOut = -1;
      }
      break;
    }
    case SQLITE_SCANSTAT_NVISIT: {
      if( pScan->addrVisit>0 ){
        *(sqlite3_int64*)pOut = aOp[pScan->addrVisit].nExec;
      }else{
        *(sqlite3_int64*)pOut = -1;
      }
      break;
    }
    case SQLITE_SCANSTAT_EST: {
      double r = 1.0;
      LogEst x = pScan->nEst;
      while( x<100 ){
        x += 10;
        r *= 0.5;
      }
      *(double*)pOut = r*sqlite3LogEstToInt(x);
      break;
    }
    case SQLITE_SCANSTAT_NAME: {
      *(const char**)pOut = pScan->zName;
      break;
    }
    case SQLITE_SCANSTAT_EXPLAIN: {
      if( pScan->addrExplain ){
        *(const char**)pOut = aOp[ pScan->addrExplain ].p4.z;
      }else{
        *(const char**)pOut = 0;
      }
      break;
    }
    case SQLITE_SCANSTAT_SELECTID: {
      if( pScan->addrExplain ){
        *(int*)pOut = aOp[ pScan->addrExplain ].p1;
      }else{
        *(int*)pOut = -1;
      }
      break;
    }
    case SQLITE_SCANSTAT_PARENTID: {
      if( pScan->addrExplain ){
        *(int*)pOut = aOp[ pScan->addrExplain ].p2;
      }else{
        *(int*)pOut = -1;
      }
      break;
    }
    case SQLITE_SCANSTAT_NCYCLE: {
      i64 res = 0;
      if( pScan->aAddrRange[0]==0 ){
        res = -1;
      }else{
        int ii;
        for(ii=0; ii<ArraySize(pScan->aAddrRange); ii+=2){
          int iIns = pScan->aAddrRange[ii];
          int iEnd = pScan->aAddrRange[ii+1];
          if( iIns==0 ) break;
          if( iIns>0 ){
            while( iIns<=iEnd ){
              res += aOp[iIns].nCycle;
              iIns++;
            }
          }else{
            int iOp;
            for(iOp=0; iOp<nOp; iOp++){
              Op *pOp = &aOp[iOp];
              if( pOp->p1!=iEnd ) continue;
              if( (sqlite3OpcodeProperty[pOp->opcode] & OPFLG_NCYCLE)==0 ){
                continue;
              }
              res += aOp[iOp].nCycle;
            }
          }
        }
      }
      *(i64*)pOut = res;
      break;
    }
    default: {
      return 1;
    }
  }
  return 0;
}

/*
** Return status data for a single loop within query pStmt.
*/
SQLITE_API int sqlite3_stmt_scanstatus(
  sqlite3_stmt *pStmt,            /* Prepared statement being queried */
  int iScan,                      /* Index of loop to report on */
  int iScanStatusOp,              /* Which metric to return */
  void *pOut                      /* OUT: Write the answer here */
){
  return sqlite3_stmt_scanstatus_v2(pStmt, iScan, iScanStatusOp, 0, pOut);
}

/*
** Zero all counters associated with the sqlite3_stmt_scanstatus() data.
*/
SQLITE_API void sqlite3_stmt_scanstatus_reset(sqlite3_stmt *pStmt){
  Vdbe *p = (Vdbe*)pStmt;
  int ii;
  for(ii=0; p!=0 && ii<p->nOp; ii++){
    Op *pOp = &p->aOp[ii];
    pOp->nExec = 0;
    pOp->nCycle = 0;
  }
}
#endif /* SQLITE_ENABLE_STMT_SCANSTATUS */

