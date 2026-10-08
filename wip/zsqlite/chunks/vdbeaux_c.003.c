#endif

/*
** This function returns a pointer to the array of opcodes associated with
** the Vdbe passed as the first argument. It is the callers responsibility
** to arrange for the returned array to be eventually freed using the
** vdbeFreeOpArray() function.
**
** Before returning, *pnOp is set to the number of entries in the returned
** array. Also, *pnMaxArg is set to the larger of its current value and
** the number of entries in the Vdbe.apArg[] array required to execute the
** returned program.
*/
SQLITE_PRIVATE VdbeOp *sqlite3VdbeTakeOpArray(Vdbe *p, int *pnOp, int *pnMaxArg){
  VdbeOp *aOp = p->aOp;
  assert( aOp && !p->db->mallocFailed );

  /* Check that sqlite3VdbeUsesBtree() was not called on this VM */
  assert( DbMaskAllZero(p->btreeMask) );

  resolveP2Values(p, pnMaxArg);
  *pnOp = p->nOp;
  p->aOp = 0;
  return aOp;
}

/*
** Add a whole list of operations to the operation stack.  Return a
** pointer to the first operation inserted.
**
** Non-zero P2 arguments to jump instructions are automatically adjusted
** so that the jump target is relative to the first operation inserted.
*/
SQLITE_PRIVATE VdbeOp *sqlite3VdbeAddOpList(
  Vdbe *p,                     /* Add opcodes to the prepared statement */
  int nOp,                     /* Number of opcodes to add */
  VdbeOpList const *aOp,       /* The opcodes to be added */
  int iLineno                  /* Source-file line number of first opcode */
){
  int i;
  VdbeOp *pOut, *pFirst;
  assert( nOp>0 );
  assert( p->eVdbeState==VDBE_INIT_STATE );
  if( p->nOp + nOp > p->nOpAlloc && growOpArray(p, nOp) ){
    return 0;
  }
  pFirst = pOut = &p->aOp[p->nOp];
  for(i=0; i<nOp; i++, aOp++, pOut++){
    pOut->opcode = aOp->opcode;
    pOut->p1 = aOp->p1;
    pOut->p2 = aOp->p2;
    assert( aOp->p2>=0 );
    if( (sqlite3OpcodeProperty[aOp->opcode] & OPFLG_JUMP)!=0 && aOp->p2>0 ){
      pOut->p2 += p->nOp;
    }
    pOut->p3 = aOp->p3;
    pOut->p4type = P4_NOTUSED;
    pOut->p4.p = 0;
    pOut->p5 = 0;
#ifdef SQLITE_ENABLE_EXPLAIN_COMMENTS
    pOut->zComment = 0;
#endif
#ifdef SQLITE_VDBE_COVERAGE
    pOut->iSrcLine = iLineno+i;
#else
    (void)iLineno;
#endif
#ifdef SQLITE_DEBUG
    if( p->db->flags & SQLITE_VdbeAddopTrace ){
      sqlite3VdbePrintOp(0, i+p->nOp, &p->aOp[i+p->nOp]);
    }
#endif
  }
  p->nOp += nOp;
  return pFirst;
}

#if defined(SQLITE_ENABLE_STMT_SCANSTATUS)
/*
** Add an entry to the array of counters managed by sqlite3_stmt_scanstatus().
*/
SQLITE_PRIVATE void sqlite3VdbeScanStatus(
  Vdbe *p,                        /* VM to add scanstatus() to */
  int addrExplain,                /* Address of OP_Explain (or 0) */
  int addrLoop,                   /* Address of loop counter */
  int addrVisit,                  /* Address of rows visited counter */
  LogEst nEst,                    /* Estimated number of output rows */
  const char *zName               /* Name of table or index being scanned */
){
  if( IS_STMT_SCANSTATUS(p->db) ){
    sqlite3_int64 nByte = (p->nScan+1) * sizeof(ScanStatus);
    ScanStatus *aNew;
    aNew = (ScanStatus*)sqlite3DbRealloc(p->db, p->aScan, nByte);
    if( aNew ){
      ScanStatus *pNew = &aNew[p->nScan++];
      memset(pNew, 0, sizeof(ScanStatus));
      pNew->addrExplain = addrExplain;
      pNew->addrLoop = addrLoop;
      pNew->addrVisit = addrVisit;
      pNew->nEst = nEst;
      pNew->zName = sqlite3DbStrDup(p->db, zName);
      p->aScan = aNew;
    }
  }
}

/*
** Add the range of instructions from addrStart to addrEnd (inclusive) to
** the set of those corresponding to the sqlite3_stmt_scanstatus() counters
** associated with the OP_Explain instruction at addrExplain. The
** sum of the sqlite3Hwtime() values for each of these instructions
** will be returned for SQLITE_SCANSTAT_NCYCLE requests.
*/
SQLITE_PRIVATE void sqlite3VdbeScanStatusRange(
  Vdbe *p,
  int addrExplain,
  int addrStart,
  int addrEnd
){
  if( IS_STMT_SCANSTATUS(p->db) ){
    ScanStatus *pScan = 0;
    int ii;
    for(ii=p->nScan-1; ii>=0; ii--){
      pScan = &p->aScan[ii];
      if( pScan->addrExplain==addrExplain ) break;
      pScan = 0;
    }
    if( pScan ){
      if( addrEnd<0 ) addrEnd = sqlite3VdbeCurrentAddr(p)-1;
      for(ii=0; ii<ArraySize(pScan->aAddrRange); ii+=2){
        if( pScan->aAddrRange[ii]==0 ){
          pScan->aAddrRange[ii] = addrStart;
          pScan->aAddrRange[ii+1] = addrEnd;
          break;
        }
      }
    }
  }
}

/*
** Set the addresses for the SQLITE_SCANSTAT_NLOOP and SQLITE_SCANSTAT_NROW
** counters for the query element associated with the OP_Explain at
** addrExplain.
*/
SQLITE_PRIVATE void sqlite3VdbeScanStatusCounters(
  Vdbe *p,
  int addrExplain,
  int addrLoop,
  int addrVisit
){
  if( IS_STMT_SCANSTATUS(p->db) ){
    ScanStatus *pScan = 0;
    int ii;
    for(ii=p->nScan-1; ii>=0; ii--){
      pScan = &p->aScan[ii];
      if( pScan->addrExplain==addrExplain ) break;
      pScan = 0;
    }
    if( pScan ){
      if( addrLoop>0 ) pScan->addrLoop = addrLoop;
      if( addrVisit>0 ) pScan->addrVisit = addrVisit;
    }
  }
}
#endif /* defined(SQLITE_ENABLE_STMT_SCANSTATUS) */


/*
** Change the value of the opcode, or P1, P2, P3, or P5 operands
** for a specific instruction.
*/
SQLITE_PRIVATE void sqlite3VdbeChangeOpcode(Vdbe *p, int addr, u8 iNewOpcode){
  assert( addr>=0 );
  sqlite3VdbeGetOp(p,addr)->opcode = iNewOpcode;
}
SQLITE_PRIVATE void sqlite3VdbeChangeP1(Vdbe *p, int addr, int val){
  assert( addr>=0 );
  sqlite3VdbeGetOp(p,addr)->p1 = val;
}
SQLITE_PRIVATE void sqlite3VdbeChangeP2(Vdbe *p, int addr, int val){
  assert( addr>=0 || p->db->mallocFailed );
  sqlite3VdbeGetOp(p,addr)->p2 = val;
}
SQLITE_PRIVATE void sqlite3VdbeChangeP3(Vdbe *p, int addr, int val){
  assert( addr>=0 );
  sqlite3VdbeGetOp(p,addr)->p3 = val;
}
SQLITE_PRIVATE void sqlite3VdbeChangeP5(Vdbe *p, u16 p5){
  assert( p->nOp>0 || p->db->mallocFailed );
  if( p->nOp>0 ) p->aOp[p->nOp-1].p5 = p5;
}

/*
** If the previous opcode is an OP_Column that delivers results
** into register iDest, then add the OPFLAG_TYPEOFARG flag to that
** opcode.
*/
SQLITE_PRIVATE void sqlite3VdbeTypeofColumn(Vdbe *p, int iDest){
  VdbeOp *pOp = sqlite3VdbeGetLastOp(p);
  if( pOp->p3==iDest && pOp->opcode==OP_Column ){
    pOp->p5 |= OPFLAG_TYPEOFARG;
  }
}

/*
** Change the P2 operand of instruction addr so that it points to
** the address of the next instruction to be coded.
*/
SQLITE_PRIVATE void sqlite3VdbeJumpHere(Vdbe *p, int addr){
  sqlite3VdbeChangeP2(p, addr, p->nOp);
}

/*
** Change the P2 operand of the jump instruction at addr so that
** the jump lands on the next opcode.  Or if the jump instruction was
** the previous opcode (and is thus a no-op) then simply back up
** the next instruction counter by one slot so that the jump is
** overwritten by the next inserted opcode.
**
** This routine is an optimization of sqlite3VdbeJumpHere() that
** strives to omit useless byte-code like this:
**
**        7   Once 0 8 0
**        8   ...
*/
SQLITE_PRIVATE void sqlite3VdbeJumpHereOrPopInst(Vdbe *p, int addr){
  if( addr==p->nOp-1 ){
    assert( p->aOp[addr].opcode==OP_Once
         || p->aOp[addr].opcode==OP_If
         || p->aOp[addr].opcode==OP_FkIfZero );
    assert( p->aOp[addr].p4type==0 );
#ifdef SQLITE_VDBE_COVERAGE
    sqlite3VdbeGetLastOp(p)->iSrcLine = 0;  /* Erase VdbeCoverage() macros */
#endif
    p->nOp--;
  }else{
    sqlite3VdbeChangeP2(p, addr, p->nOp);
  }
}


/*
** If the input FuncDef structure is ephemeral, then free it.  If
** the FuncDef is not ephemeral, then do nothing.
*/
static void freeEphemeralFunction(sqlite3 *db, FuncDef *pDef){
  assert( db!=0 );
  if( (pDef->funcFlags & SQLITE_FUNC_EPHEM)!=0 ){
    sqlite3DbNNFreeNN(db, pDef);
  }
}

/*
** Delete a P4 value if necessary.
*/
static SQLITE_NOINLINE void freeP4Mem(sqlite3 *db, Mem *p){
  if( p->szMalloc ) sqlite3DbFree(db, p->zMalloc);
  sqlite3DbNNFreeNN(db, p);
}
static SQLITE_NOINLINE void freeP4FuncCtx(sqlite3 *db, sqlite3_context *p){
  assert( db!=0 );
  freeEphemeralFunction(db, p->pFunc);
  sqlite3DbNNFreeNN(db, p);
}
static void freeP4(sqlite3 *db, int p4type, void *p4){
  assert( db );
  switch( p4type ){
    case P4_FUNCCTX: {
      freeP4FuncCtx(db, (sqlite3_context*)p4);
      break;
    }
    case P4_REAL:
    case P4_INT64:
    case P4_DYNAMIC:
    case P4_INTARRAY: {
      if( p4 ) sqlite3DbNNFreeNN(db, p4);
      break;
    }
    case P4_KEYINFO: {
      if( db->pnBytesFreed==0 ) sqlite3KeyInfoUnref((KeyInfo*)p4);
      break;
    }
#ifdef SQLITE_ENABLE_CURSOR_HINTS
    case P4_EXPR: {
      sqlite3ExprDelete(db, (Expr*)p4);
      break;
    }
#endif
    case P4_FUNCDEF: {
      freeEphemeralFunction(db, (FuncDef*)p4);
      break;
    }
    case P4_MEM: {
      if( db->pnBytesFreed==0 ){
        sqlite3ValueFree((sqlite3_value*)p4);
      }else{
        freeP4Mem(db, (Mem*)p4);
      }
      break;
    }
    case P4_VTAB : {
      if( db->pnBytesFreed==0 ) sqlite3VtabUnlock((VTable *)p4);
      break;
    }
    case P4_TABLEREF: {
      if( db->pnBytesFreed==0 ) sqlite3DeleteTable(db, (Table*)p4);
      break;
    }
  }
}

/*
** Free the space allocated for aOp and any p4 values allocated for the
** opcodes contained within. If aOp is not NULL it is assumed to contain
** nOp entries.
*/
static void vdbeFreeOpArray(sqlite3 *db, Op *aOp, int nOp){
  assert( nOp>=0 );
  assert( db!=0 );
  if( aOp ){
    Op *pOp = &aOp[nOp-1];
    while(1){  /* Exit via break */
      if( pOp->p4type <= P4_FREE_IF_LE ) freeP4(db, pOp->p4type, pOp->p4.p);
#ifdef SQLITE_ENABLE_EXPLAIN_COMMENTS
      sqlite3DbFree(db, pOp->zComment);
#endif
      if( pOp==aOp ) break;
      pOp--;
    }
    sqlite3DbNNFreeNN(db, aOp);
  }
}

/*
** Link the SubProgram object passed as the second argument into the linked
** list at Vdbe.pSubProgram. This list is used to delete all sub-program
** objects when the VM is no longer required.
*/
SQLITE_PRIVATE void sqlite3VdbeLinkSubProgram(Vdbe *pVdbe, SubProgram *p){
  p->pNext = pVdbe->pProgram;
  pVdbe->pProgram = p;
}

/*
** Return true if the given Vdbe has any SubPrograms.
*/
SQLITE_PRIVATE int sqlite3VdbeHasSubProgram(Vdbe *pVdbe){
  return pVdbe->pProgram!=0;
}

/*
** Change the opcode at addr into OP_Noop
*/
SQLITE_PRIVATE int sqlite3VdbeChangeToNoop(Vdbe *p, int addr){
  VdbeOp *pOp;
  if( p->db->mallocFailed ) return 0;
  assert( addr>=0 && addr<p->nOp );
  pOp = &p->aOp[addr];
  freeP4(p->db, pOp->p4type, pOp->p4.p);
  pOp->p4type = P4_NOTUSED;
  pOp->p4.z = 0;
  pOp->opcode = OP_Noop;
  return 1;
}
