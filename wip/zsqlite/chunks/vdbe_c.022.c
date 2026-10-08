
/* Opcode: ClrSubtype P1 * * * *
** Synopsis:  r[P1].subtype = 0
**
** Clear the subtype from register P1.
*/
case OP_ClrSubtype: {   /* in1 */
  pIn1 = &aMem[pOp->p1];
  pIn1->flags &= ~MEM_Subtype;
  break;
}

/* Opcode: GetSubtype P1 P2 * * *
** Synopsis:  r[P2] = r[P1].subtype
**
** Extract the subtype value from register P1 and write that subtype
** into register P2.  If P1 has no subtype, then P1 gets a NULL.
*/
case OP_GetSubtype: {   /* in1 out2 */
  pIn1 = &aMem[pOp->p1];
  pOut = &aMem[pOp->p2];
  if( pIn1->flags & MEM_Subtype ){
    sqlite3VdbeMemSetInt64(pOut, pIn1->eSubtype);
  }else{
    sqlite3VdbeMemSetNull(pOut);
  }
  break;
}

/* Opcode: SetSubtype P1 P2 * * *
** Synopsis:  r[P2].subtype = r[P1]
**
** Set the subtype value of register P2 to the integer from register P1.
** If P1 is NULL, clear the subtype from p2.
*/
case OP_SetSubtype: {   /* in1 out2 */
  pIn1 = &aMem[pOp->p1];
  pOut = &aMem[pOp->p2];
  if( pIn1->flags & MEM_Null ){
    pOut->flags &= ~MEM_Subtype;
  }else{
    assert( pIn1->flags & MEM_Int );
    pOut->flags |= MEM_Subtype;
    pOut->eSubtype = (u8)(pIn1->u.i & 0xff);
  }
  break;
}

/* Opcode: FilterAdd P1 * P3 P4 *
** Synopsis: filter(P1) += key(P3@P4)
**
** Compute a hash on the P4 registers starting with r[P3] and
** add that hash to the bloom filter contained in r[P1].
*/
case OP_FilterAdd: {
  u64 h;

  assert( pOp->p1>0 && pOp->p1<=(p->nMem+1 - p->nCursor) );
  pIn1 = &aMem[pOp->p1];
  assert( pIn1->flags & MEM_Blob );
  assert( pIn1->n>0 );
  h = filterHash(aMem, pOp);
#ifdef SQLITE_DEBUG
  if( db->flags&SQLITE_VdbeTrace ){
    int ii;
    for(ii=pOp->p3; ii<pOp->p3+pOp->p4.i; ii++){
      registerTrace(ii, &aMem[ii]);
    }
    printf("hash: %llu modulo %d -> %u\n", h, pIn1->n, (int)(h%pIn1->n));
  }
#endif
  h %= (pIn1->n*8);
  pIn1->z[h/8] |= 1<<(h&7);
  break;
}

/* Opcode: Filter P1 P2 P3 P4 *
** Synopsis: if key(P3@P4) not in filter(P1) goto P2
**
** Compute a hash on the key contained in the P4 registers starting
** with r[P3].  Check to see if that hash is found in the
** bloom filter hosted by register P1.  If it is not present then
** maybe jump to P2.  Otherwise fall through.
**
** False negatives are harmless.  It is always safe to fall through,
** even if the value is in the bloom filter.  A false negative causes
** more CPU cycles to be used, but it should still yield the correct
** answer.  However, an incorrect answer may well arise from a
** false positive - if the jump is taken when it should fall through.
*/
case OP_Filter: {          /* jump */
  u64 h;

  assert( pOp->p1>0 && pOp->p1<=(p->nMem+1 - p->nCursor) );
  pIn1 = &aMem[pOp->p1];
  assert( (pIn1->flags & MEM_Blob)!=0 );
  assert( pIn1->n >= 1 );
  h = filterHash(aMem, pOp);
#ifdef SQLITE_DEBUG
  if( db->flags&SQLITE_VdbeTrace ){
    int ii;
    for(ii=pOp->p3; ii<pOp->p3+pOp->p4.i; ii++){
      registerTrace(ii, &aMem[ii]);
    }
    printf("hash: %llu modulo %d -> %u\n", h, pIn1->n, (int)(h%pIn1->n));
  }
#endif
  h %= (pIn1->n*8);
  if( (pIn1->z[h/8] & (1<<(h&7)))==0 ){
    VdbeBranchTaken(1, 2);
    p->aCounter[SQLITE_STMTSTATUS_FILTER_HIT]++;
    goto jump_to_p2;
  }else{
    p->aCounter[SQLITE_STMTSTATUS_FILTER_MISS]++;
    VdbeBranchTaken(0, 2);
  }
  break;
}

/* Opcode: Trace P1 P2 * P4 *
**
** Write P4 on the statement trace output if statement tracing is
** enabled.
**
** Operand P1 must be 0x7fffffff and P2 must positive.
*/
/* Opcode: Init P1 P2 P3 P4 *
** Synopsis: Start at P2
**
** Programs contain a single instance of this opcode as the very first
** opcode.
**
** If tracing is enabled (by the sqlite3_trace()) interface, then
** the UTF-8 string contained in P4 is emitted on the trace callback.
** Or if P4 is blank, use the string returned by sqlite3_sql().
**
** If P2 is not zero, jump to instruction P2.
**
** Increment the value of P1 so that OP_Once opcodes will jump the
** first time they are evaluated for this run.
**
** If P3 is not zero, then it is an address to jump to if an SQLITE_CORRUPT
** error is encountered.
*/
case OP_Trace:
case OP_Init: {          /* jump0 */
  int i;
#ifndef SQLITE_OMIT_TRACE
  char *zTrace;
#endif

  /* If the P4 argument is not NULL, then it must be an SQL comment string.
  ** The "--" string is broken up to prevent false-positives with srcck1.c.
  **
  ** This assert() provides evidence for:
  ** EVIDENCE-OF: R-50676-09860 The callback can compute the same text that
  ** would have been returned by the legacy sqlite3_trace() interface by
  ** using the X argument when X begins with "--" and invoking
  ** sqlite3_expanded_sql(P) otherwise.
  */
  assert( pOp->p4.z==0 || strncmp(pOp->p4.z, "-" "- ", 3)==0 );

  /* OP_Init is always instruction 0 */
  assert( pOp==p->aOp || pOp->opcode==OP_Trace );

#ifndef SQLITE_OMIT_TRACE
  if( (db->mTrace & (SQLITE_TRACE_STMT|SQLITE_TRACE_LEGACY))!=0
   && p->minWriteFileFormat!=254  /* tag-20220401a */
   && (zTrace = (pOp->p4.z ? pOp->p4.z : p->zSql))!=0
  ){
#ifndef SQLITE_OMIT_DEPRECATED
    if( db->mTrace & SQLITE_TRACE_LEGACY ){
      char *z = sqlite3VdbeExpandSql(p, zTrace);
      db->trace.xLegacy(db->pTraceArg, z);
      sqlite3_free(z);
    }else
#endif
    if( db->nVdbeExec>1 ){
      char *z = sqlite3MPrintf(db, "-- %s", zTrace);
      (void)db->trace.xV2(SQLITE_TRACE_STMT, db->pTraceArg, p, z);
      sqlite3DbFree(db, z);
    }else{
      (void)db->trace.xV2(SQLITE_TRACE_STMT, db->pTraceArg, p, zTrace);
    }
  }
#ifdef SQLITE_USE_FCNTL_TRACE
  zTrace = (pOp->p4.z ? pOp->p4.z : p->zSql);
  if( zTrace ){
    int j;
    for(j=0; j<db->nDb; j++){
      if( DbMaskTest(p->btreeMask, j)==0 ) continue;
      sqlite3_file_control(db, db->aDb[j].zDbSName, SQLITE_FCNTL_TRACE, zTrace);
    }
  }
#endif /* SQLITE_USE_FCNTL_TRACE */
#ifdef SQLITE_DEBUG
  if( (db->flags & SQLITE_SqlTrace)!=0
   && (zTrace = (pOp->p4.z ? pOp->p4.z : p->zSql))!=0
  ){
    sqlite3DebugPrintf("SQL-trace: %s\n", zTrace);
  }
#endif /* SQLITE_DEBUG */
#endif /* SQLITE_OMIT_TRACE */
  assert( pOp->p2>0 );
  if( pOp->p1>=sqlite3GlobalConfig.iOnceResetThreshold ){
    if( pOp->opcode==OP_Trace ) break;
    for(i=1; i<p->nOp; i++){
      if( p->aOp[i].opcode==OP_Once ) p->aOp[i].p1 = 0;
    }
    pOp->p1 = 0;
  }
  pOp->p1++;
  p->aCounter[SQLITE_STMTSTATUS_RUN]++;
  goto jump_to_p2;
}

#ifdef SQLITE_ENABLE_CURSOR_HINTS
/* Opcode: CursorHint P1 * * P4 *
**
** Provide a hint to cursor P1 that it only needs to return rows that
** satisfy the Expr in P4.  TK_REGISTER terms in the P4 expression refer
** to values currently held in registers.  TK_COLUMN terms in the P4
** expression refer to columns in the b-tree to which cursor P1 is pointing.
*/
case OP_CursorHint: {
  VdbeCursor *pC;

  assert( pOp->p1>=0 && pOp->p1<p->nCursor );
  assert( pOp->p4type==P4_EXPR );
  pC = p->apCsr[pOp->p1];
  if( pC ){
    assert( pC->eCurType==CURTYPE_BTREE );
    sqlite3BtreeCursorHint(pC->uc.pCursor, BTREE_HINT_RANGE,
                           pOp->p4.pExpr, aMem);
  }
  break;
}
#endif /* SQLITE_ENABLE_CURSOR_HINTS */

#ifdef SQLITE_DEBUG
/* Opcode:  Abortable   * * * * *
**
** Verify that an Abort can happen.  Assert if an Abort at this point
** might cause database corruption.  This opcode only appears in debugging
** builds.
**
** An Abort is safe if either there have been no writes, or if there is
** an active statement journal.
*/
case OP_Abortable: {
  sqlite3VdbeAssertAbortable(p);
  break;
}
#endif

#ifdef SQLITE_DEBUG
/* Opcode:  ReleaseReg   P1 P2 P3 * P5
** Synopsis: release r[P1@P2] mask P3
**
** Release registers from service.  Any content that was in the
** the registers is unreliable after this opcode completes.
**
** The registers released will be the P2 registers starting at P1,
** except if bit ii of P3 set, then do not release register P1+ii.
** In other words, P3 is a mask of registers to preserve.
**
** Releasing a register clears the Mem.pScopyFrom pointer.  That means
** that if the content of the released register was set using OP_SCopy,
** a change to the value of the source register for the OP_SCopy will no longer
** generate an assertion fault in sqlite3VdbeMemAboutToChange().
**
** If P5 is set, then all released registers have their type set
** to MEM_Undefined so that any subsequent attempt to read the released
** register (before it is reinitialized) will generate an assertion fault.
**
** P5 ought to be set on every call to this opcode.
** However, there are places in the code generator will release registers
** before their are used, under the (valid) assumption that the registers
** will not be reallocated for some other purpose before they are used and
** hence are safe to release.
**
** This opcode is only available in testing and debugging builds.  It is
** not generated for release builds.  The purpose of this opcode is to help
** validate the generated bytecode.  This opcode does not actually contribute
** to computing an answer.
*/
case OP_ReleaseReg: {
  Mem *pMem;
  int i;
  u32 constMask;
  assert( pOp->p1>0 );
  assert( pOp->p1+pOp->p2<=(p->nMem+1 - p->nCursor)+1 );
  pMem = &aMem[pOp->p1];
  constMask = pOp->p3;
  for(i=0; i<pOp->p2; i++, pMem++){
    if( i>=32 || (constMask & MASKBIT32(i))==0 ){
      pMem->pScopyFrom = 0;
      if( i<32 && pOp->p5 ) MemSetTypeFlag(pMem, MEM_Undefined);
    }
  }
  break;
}
#endif

/* Opcode: Noop * * * * *
**
** Do nothing.  This instruction is often useful as a jump
** destination.
*/
/*
** The magic Explain opcode are only inserted when explain==2 (which
** is to say when the EXPLAIN QUERY PLAN syntax is used.)
** This opcode records information from the optimizer.  It is the
** the same as a no-op.  This opcodesnever appears in a real VM program.
*/
default: {          /* This is really OP_Noop, OP_Explain */
  assert( pOp->opcode==OP_Noop || pOp->opcode==OP_Explain );

  break;
}

/*****************************************************************************
** The cases of the switch statement above this line should all be indented
** by 6 spaces.  But the left-most 6 spaces have been removed to improve the
** readability.  From this point on down, the normal indentation rules are
** restored.
*****************************************************************************/
    }

#if defined(VDBE_PROFILE)
    *pnCycle += sqlite3NProfileCnt ? sqlite3NProfileCnt : sqlite3Hwtime();
    pnCycle = 0;
#elif defined(SQLITE_ENABLE_STMT_SCANSTATUS)
    if( pnCycle ){
      *pnCycle += sqlite3Hwtime();
      pnCycle = 0;
    }
#endif

    /* The following code adds nothing to the actual functionality
    ** of the program.  It is only here for testing and debugging.
    ** On the other hand, it does burn CPU cycles every time through
    ** the evaluator loop.  So we can leave it out when NDEBUG is defined.
    */
#ifndef NDEBUG
    assert( pOp>=&aOp[-1] && pOp<&aOp[p->nOp-1] );

#ifdef SQLITE_DEBUG
    if( db->flags & SQLITE_VdbeTrace ){
      u8 opProperty = sqlite3OpcodeProperty[pOrigOp->opcode];
      if( rc!=0 ) printf("rc=%d\n",rc);
      if( opProperty & (OPFLG_OUT2) ){
        registerTrace(pOrigOp->p2, &aMem[pOrigOp->p2]);
      }
      if( opProperty & OPFLG_OUT3 ){
        registerTrace(pOrigOp->p3, &aMem[pOrigOp->p3]);
      }
      if( opProperty==0xff ){
        /* Never happens.  This code exists to avoid a harmless linkage
        ** warning about sqlite3VdbeRegisterDump() being defined but not
        ** used. */
        sqlite3VdbeRegisterDump(p);
      }
    }
#endif  /* SQLITE_DEBUG */
#endif  /* NDEBUG */
  }  /* The end of the for(;;) loop the loops through opcodes */

  /* If we reach this point, it means that execution is finished with
  ** an error of some kind.
  */
abort_due_to_error:
  if( db->mallocFailed ){
    rc = SQLITE_NOMEM_BKPT;
  }else if( rc==SQLITE_IOERR_CORRUPTFS ){
    rc = SQLITE_CORRUPT_BKPT;
  }
  assert( rc );
#ifdef SQLITE_DEBUG
  if( db->flags & SQLITE_VdbeTrace ){
    const char *zTrace = p->zSql;
    if( zTrace==0 ){
      if( aOp[0].opcode==OP_Trace ){
        zTrace = aOp[0].p4.z;
      }
      if( zTrace==0 ) zTrace = "???";
    }
    printf("ABORT-due-to-error (rc=%d): %s\n", rc, zTrace);
  }
#endif
  if( p->zErrMsg==0 && rc!=SQLITE_IOERR_NOMEM ){
    sqlite3VdbeError(p, "%s", sqlite3ErrStr(rc));
  }
  p->rc = rc;
  sqlite3SystemError(db, rc);
  testcase( sqlite3GlobalConfig.xLog!=0 );
  sqlite3_log(rc, "statement aborts at %d: [%s] %s",
                   (int)(pOp - aOp), p->zSql, p->zErrMsg);
  if( p->eVdbeState==VDBE_RUN_STATE ) sqlite3VdbeHalt(p);
  if( rc==SQLITE_IOERR_NOMEM ) sqlite3OomFault(db);
  if( rc==SQLITE_CORRUPT && db->autoCommit==0 ){
    db->flags |= SQLITE_CorruptRdOnly;
  }
  rc = SQLITE_ERROR;
  if( resetSchemaOnFault>0 ){
    sqlite3ResetOneSchema(db, resetSchemaOnFault-1);
  }

  /* This is the only way out of this procedure.  We have to
  ** release the mutexes on btrees that were acquired at the
  ** top. */
vdbe_return:
#if defined(VDBE_PROFILE)
  if( pnCycle ){
    *pnCycle += sqlite3NProfileCnt ? sqlite3NProfileCnt : sqlite3Hwtime();
    pnCycle = 0;
  }
#elif defined(SQLITE_ENABLE_STMT_SCANSTATUS)
  if( pnCycle ){
    *pnCycle += sqlite3Hwtime();
    pnCycle = 0;
  }
#endif

#ifndef SQLITE_OMIT_PROGRESS_CALLBACK
  while( nVmStep>=nProgressLimit && db->xProgress!=0 ){
    nProgressLimit += db->nProgressOps;
    if( db->xProgress(db->pProgressArg) ){
      nProgressLimit = LARGEST_UINT64;
      rc = SQLITE_INTERRUPT;
      goto abort_due_to_error;
    }
  }
#endif
  p->aCounter[SQLITE_STMTSTATUS_VM_STEP] += (int)nVmStep;
  if( DbMaskNonZero(p->lockMask) ){
    sqlite3VdbeLeave(p);
  }
  assert( rc!=SQLITE_OK || nExtraDelete==0
       || sqlite3_strlike("DELETE%",p->zSql,0)!=0
  );
  return rc;

  /* Jump to here if a string or blob larger than SQLITE_MAX_LENGTH
  ** is encountered.
  */
too_big:
  sqlite3VdbeError(p, "string or blob too big");
  rc = SQLITE_TOOBIG;
  goto abort_due_to_error;

  /* Jump to here if a malloc() fails.
  */
no_mem:
  sqlite3OomFault(db);
  sqlite3VdbeError(p, "out of memory");
  rc = SQLITE_NOMEM_BKPT;
  goto abort_due_to_error;

  /* Jump to here if the sqlite3_interrupt() API sets the interrupt
  ** flag.
  */
abort_due_to_interrupt:
  assert( AtomicLoad(&db->u1.isInterrupted) );
  rc = SQLITE_INTERRUPT;
  goto abort_due_to_error;
}
