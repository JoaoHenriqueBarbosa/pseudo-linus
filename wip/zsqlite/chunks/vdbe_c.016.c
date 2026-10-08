
/* Opcode: SorterInsert P1 P2 * * *
** Synopsis: key=r[P2]
**
** Register P2 holds an SQL index key made using the
** MakeRecord instructions.  This opcode writes that key
** into the sorter P1.  Data for the entry is nil.
*/
case OP_SorterInsert: {     /* in2 */
  VdbeCursor *pC;

  assert( pOp->p1>=0 && pOp->p1<p->nCursor );
  pC = p->apCsr[pOp->p1];
  sqlite3VdbeIncrWriteCounter(p, pC);
  assert( pC!=0 );
  assert( isSorter(pC) );
  pIn2 = &aMem[pOp->p2];
  assert( pIn2->flags & MEM_Blob );
  assert( pC->isTable==0 );
  rc = ExpandBlob(pIn2);
  if( rc ) goto abort_due_to_error;
  rc = sqlite3VdbeSorterWrite(pC, pIn2);
  if( rc) goto abort_due_to_error;
  break;
}

/* Opcode: IdxDelete P1 P2 P3 * P5
** Synopsis: key=r[P2@P3]
**
** The content of P3 registers starting at register P2 form
** an unpacked index key. This opcode removes that entry from the
** index opened by cursor P1.
**
** If P5 is not zero, then raise an SQLITE_CORRUPT_INDEX error
** if no matching index entry is found.  This happens when running
** an UPDATE or DELETE statement and the index entry to be updated
** or deleted is not found.  For some uses of IdxDelete
** (example:  the EXCEPT operator) it does not matter that no matching
** entry is found.  For those cases, P5 is zero.  Also, do not raise
** this (self-correcting and non-critical) error if in writable_schema mode.
*/
case OP_IdxDelete: {
  VdbeCursor *pC;
  BtCursor *pCrsr;
  int res;
  UnpackedRecord r;

  assert( pOp->p3>0 );
  assert( pOp->p2>0 && pOp->p2+pOp->p3<=(p->nMem+1 - p->nCursor)+1 );
  assert( pOp->p1>=0 && pOp->p1<p->nCursor );
  pC = p->apCsr[pOp->p1];
  assert( pC!=0 );
  assert( pC->eCurType==CURTYPE_BTREE );
  sqlite3VdbeIncrWriteCounter(p, pC);
  pCrsr = pC->uc.pCursor;
  assert( pCrsr!=0 );
  r.pKeyInfo = pC->pKeyInfo;
  r.nField = (u16)pOp->p3;
  r.default_rc = 0;
  r.aMem = &aMem[pOp->p2];
  rc = sqlite3BtreeIndexMoveto(pCrsr, &r, &res);
  if( rc ) goto abort_due_to_error;
  if( res==0 ){
    rc = sqlite3BtreeDelete(pCrsr, BTREE_AUXDELETE);
    if( rc ) goto abort_due_to_error;
  }else if( pOp->p5 && !sqlite3WritableSchema(db) ){
    rc = sqlite3ReportError(SQLITE_CORRUPT_INDEX, __LINE__, "index corruption");
    goto abort_due_to_error;
  }
  assert( pC->deferredMoveto==0 );
  pC->cacheStatus = CACHE_STALE;
  pC->seekResult = 0;
  break;
}

/* Opcode: DeferredSeek P1 * P3 P4 *
** Synopsis: Move P3 to P1.rowid if needed
**
** P1 is an open index cursor and P3 is a cursor on the corresponding
** table.  This opcode does a deferred seek of the P3 table cursor
** to the row that corresponds to the current row of P1.
**
** This is a deferred seek.  Nothing actually happens until
** the cursor is used to read a record.  That way, if no reads
** occur, no unnecessary I/O happens.
**
** P4 may be an array of integers (type P4_INTARRAY) containing
** one entry for each column in the P3 table.  If array entry a(i)
** is non-zero, then reading column a(i)-1 from cursor P3 is
** equivalent to performing the deferred seek and then reading column i
** from P1.  This information is stored in P3 and used to redirect
** reads against P3 over to P1, thus possibly avoiding the need to
** seek and read cursor P3.
*/
/* Opcode: IdxRowid P1 P2 * * *
** Synopsis: r[P2]=rowid
**
** Write into register P2 an integer which is the last entry in the record at
** the end of the index key pointed to by cursor P1.  This integer should be
** the rowid of the table entry to which this index entry points.
**
** See also: Rowid, MakeRecord.
*/
case OP_DeferredSeek:         /* ncycle */
case OP_IdxRowid: {           /* out2, ncycle */
  VdbeCursor *pC;             /* The P1 index cursor */
  VdbeCursor *pTabCur;        /* The P2 table cursor (OP_DeferredSeek only) */
  i64 rowid;                  /* Rowid that P1 current points to */

  assert( pOp->p1>=0 && pOp->p1<p->nCursor );
  pC = p->apCsr[pOp->p1];
  assert( pC!=0 );
  assert( pC->eCurType==CURTYPE_BTREE || IsNullCursor(pC) );
  assert( pC->uc.pCursor!=0 );
  assert( pC->isTable==0 || IsNullCursor(pC) );
  assert( pC->deferredMoveto==0 );
  assert( !pC->nullRow || pOp->opcode==OP_IdxRowid );

  /* The IdxRowid and Seek opcodes are combined because of the commonality
  ** of sqlite3VdbeCursorRestore() and sqlite3VdbeIdxRowid(). */
  rc = sqlite3VdbeCursorRestore(pC);

  /* sqlite3VdbeCursorRestore() may fail if the cursor has been disturbed
  ** since it was last positioned and an error (e.g. OOM or an IO error)
  ** occurs while trying to reposition it. */
  if( rc!=SQLITE_OK ) goto abort_due_to_error;

  if( !pC->nullRow ){
    rowid = 0;  /* Not needed.  Only used to silence a warning. */
    rc = sqlite3VdbeIdxRowid(db, pC->uc.pCursor, &rowid);
    if( rc!=SQLITE_OK ){
      goto abort_due_to_error;
    }
    if( pOp->opcode==OP_DeferredSeek ){
      assert( pOp->p3>=0 && pOp->p3<p->nCursor );
      pTabCur = p->apCsr[pOp->p3];
      assert( pTabCur!=0 );
      assert( pTabCur->eCurType==CURTYPE_BTREE );
      assert( pTabCur->uc.pCursor!=0 );
      assert( pTabCur->isTable );
      pTabCur->nullRow = 0;
      pTabCur->movetoTarget = rowid;
      pTabCur->deferredMoveto = 1;
      pTabCur->cacheStatus = CACHE_STALE;
      assert( pOp->p4type==P4_INTARRAY || pOp->p4.ai==0 );
      assert( !pTabCur->isEphemeral );
      pTabCur->ub.aAltMap = pOp->p4.ai;
      assert( !pC->isEphemeral );
      pTabCur->pAltCursor = pC;
    }else{
      pOut = out2Prerelease(p, pOp);
      pOut->u.i = rowid;
    }
  }else{
    assert( pOp->opcode==OP_IdxRowid );
    sqlite3VdbeMemSetNull(&aMem[pOp->p2]);
  }
  break;
}

/* Opcode: FinishSeek P1 * * * *
**
** If cursor P1 was previously moved via OP_DeferredSeek, complete that
** seek operation now, without further delay.  If the cursor seek has
** already occurred, this instruction is a no-op.
*/
case OP_FinishSeek: {        /* ncycle */
  VdbeCursor *pC;            /* The P1 index cursor */

  assert( pOp->p1>=0 && pOp->p1<p->nCursor );
  pC = p->apCsr[pOp->p1];
  if( pC->deferredMoveto ){
    rc = sqlite3VdbeFinishMoveto(pC);
    if( rc ) goto abort_due_to_error;
  }
  break;
}

/* Opcode: IdxGE P1 P2 P3 P4 *
** Synopsis: key=r[P3@P4]
**
** The P4 register values beginning with P3 form an unpacked index
** key that omits the PRIMARY KEY.  Compare this key value against the index
** that P1 is currently pointing to, ignoring the PRIMARY KEY or ROWID
** fields at the end.
**
** If the P1 index entry is greater than or equal to the key value
** then jump to P2.  Otherwise fall through to the next instruction.
*/
/* Opcode: IdxGT P1 P2 P3 P4 *
** Synopsis: key=r[P3@P4]
**
** The P4 register values beginning with P3 form an unpacked index
** key that omits the PRIMARY KEY.  Compare this key value against the index
** that P1 is currently pointing to, ignoring the PRIMARY KEY or ROWID
** fields at the end.
**
** If the P1 index entry is greater than the key value
** then jump to P2.  Otherwise fall through to the next instruction.
*/
/* Opcode: IdxLT P1 P2 P3 P4 *
** Synopsis: key=r[P3@P4]
**
** The P4 register values beginning with P3 form an unpacked index
** key that omits the PRIMARY KEY or ROWID.  Compare this key value against
** the index that P1 is currently pointing to, ignoring the PRIMARY KEY or
** ROWID on the P1 index.
**
** If the P1 index entry is less than the key value then jump to P2.
** Otherwise fall through to the next instruction.
*/
/* Opcode: IdxLE P1 P2 P3 P4 *
** Synopsis: key=r[P3@P4]
**
** The P4 register values beginning with P3 form an unpacked index
** key that omits the PRIMARY KEY or ROWID.  Compare this key value against
** the index that P1 is currently pointing to, ignoring the PRIMARY KEY or
** ROWID on the P1 index.
**
** If the P1 index entry is less than or equal to the key value then jump
** to P2. Otherwise fall through to the next instruction.
*/
case OP_IdxLE:          /* jump, ncycle */
case OP_IdxGT:          /* jump, ncycle */
case OP_IdxLT:          /* jump, ncycle */
case OP_IdxGE:  {       /* jump, ncycle */
  VdbeCursor *pC;
  int res;
  UnpackedRecord r;

  assert( pOp->p1>=0 && pOp->p1<p->nCursor );
  pC = p->apCsr[pOp->p1];
  assert( pC!=0 );
  assert( pC->isOrdered );
  assert( pC->eCurType==CURTYPE_BTREE );
  assert( pC->uc.pCursor!=0);
  assert( pC->deferredMoveto==0 );
  assert( pOp->p4type==P4_INT32 );
  r.pKeyInfo = pC->pKeyInfo;
  r.nField = (u16)pOp->p4.i;
  if( pOp->opcode<OP_IdxLT ){
    assert( pOp->opcode==OP_IdxLE || pOp->opcode==OP_IdxGT );
    r.default_rc = -1;
  }else{
    assert( pOp->opcode==OP_IdxGE || pOp->opcode==OP_IdxLT );
    r.default_rc = 0;
  }
  r.aMem = &aMem[pOp->p3];
#ifdef SQLITE_DEBUG
  {
    int i;
    for(i=0; i<r.nField; i++){
      assert( memIsValid(&r.aMem[i]) );
      REGISTER_TRACE(pOp->p3+i, &aMem[pOp->p3+i]);
    }
  }
#endif

  /* Inlined version of sqlite3VdbeIdxKeyCompare() */
  {
    i64 nCellKey = 0;
    BtCursor *pCur;
    Mem m;

    assert( pC->eCurType==CURTYPE_BTREE );
    pCur = pC->uc.pCursor;
    assert( sqlite3BtreeCursorIsValid(pCur) );
    nCellKey = sqlite3BtreePayloadSize(pCur);
    /* nCellKey will always be between 0 and 0xffffffff because of the way
    ** that btreeParseCellPtr() and sqlite3GetVarint32() are implemented */
    if( nCellKey<=0 || nCellKey>0x7fffffff ){
      rc = SQLITE_CORRUPT_BKPT;
      goto abort_due_to_error;
    }
    sqlite3VdbeMemInit(&m, db, 0);
    rc = sqlite3VdbeMemFromBtreeZeroOffset(pCur, (u32)nCellKey, &m);
    if( rc ) goto abort_due_to_error;
    res = sqlite3VdbeRecordCompareWithSkip(m.n, m.z, &r, 0);
    sqlite3VdbeMemReleaseMalloc(&m);
  }
  /* End of inlined sqlite3VdbeIdxKeyCompare() */

  assert( (OP_IdxLE&1)==(OP_IdxLT&1) && (OP_IdxGE&1)==(OP_IdxGT&1) );
  if( (pOp->opcode&1)==(OP_IdxLT&1) ){
    assert( pOp->opcode==OP_IdxLE || pOp->opcode==OP_IdxLT );
    res = -res;
  }else{
    assert( pOp->opcode==OP_IdxGE || pOp->opcode==OP_IdxGT );
    res++;
  }
  VdbeBranchTaken(res>0,2);
  assert( rc==SQLITE_OK );
  if( res>0 ) goto jump_to_p2;
  break;
}

/* Opcode: Destroy P1 P2 P3 * *
**
** Delete an entire database table or index whose root page in the database
** file is given by P1.
**
** The table being destroyed is in the main database file if P3==0.  If
** P3==1 then the table to be destroyed is in the auxiliary database file
** that is used to store tables create using CREATE TEMPORARY TABLE.
**
** If AUTOVACUUM is enabled then it is possible that another root page
** might be moved into the newly deleted root page in order to keep all
** root pages contiguous at the beginning of the database.  The former
** value of the root page that moved - its value before the move occurred -
** is stored in register P2. If no page movement was required (because the
** table being dropped was already the last one in the database) then a
** zero is stored in register P2.  If AUTOVACUUM is disabled then a zero
** is stored in register P2.
**
** This opcode throws an error if there are any active reader VMs when
** it is invoked. This is done to avoid the difficulty associated with
** updating existing cursors when a root page is moved in an AUTOVACUUM
** database. This error is thrown even if the database is not an AUTOVACUUM
** db in order to avoid introducing an incompatibility between autovacuum
** and non-autovacuum modes.
**
** See also: Clear
*/
case OP_Destroy: {     /* out2 */
  int iMoved;
  int iDb;

  sqlite3VdbeIncrWriteCounter(p, 0);
  assert( p->readOnly==0 );
  assert( pOp->p1>1 );
  pOut = out2Prerelease(p, pOp);
  pOut->flags = MEM_Null;
  if( db->nVdbeRead > db->nVDestroy+1 ){
    rc = SQLITE_LOCKED;
    p->errorAction = OE_Abort;
    goto abort_due_to_error;
  }else{
    iDb = pOp->p3;
    assert( DbMaskTest(p->btreeMask, iDb) );
    iMoved = 0;  /* Not needed.  Only to silence a warning. */
    rc = sqlite3BtreeDropTable(db->aDb[iDb].pBt, pOp->p1, &iMoved);
    pOut->flags = MEM_Int;
    pOut->u.i = iMoved;
    if( rc ) goto abort_due_to_error;
#ifndef SQLITE_OMIT_AUTOVACUUM
    if( iMoved!=0 ){
      sqlite3RootPageMoved(db, iDb, iMoved, pOp->p1);
      /* All OP_Destroy operations occur on the same btree */
      assert( resetSchemaOnFault==0 || resetSchemaOnFault==iDb+1 );
      resetSchemaOnFault = iDb+1;
    }
#endif
  }
  break;
}
