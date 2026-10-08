

/* Opcode: SeekScan  P1 P2 * * P5
** Synopsis: Scan-ahead up to P1 rows
**
** This opcode is a prefix opcode to OP_SeekGE.  In other words, this
** opcode must be immediately followed by OP_SeekGE. This constraint is
** checked by assert() statements.
**
** This opcode uses the P1 through P4 operands of the subsequent
** OP_SeekGE.  In the text that follows, the operands of the subsequent
** OP_SeekGE opcode are denoted as SeekOP.P1 through SeekOP.P4.   Only
** the P1, P2 and P5 operands of this opcode are also used, and  are called
** This.P1, This.P2 and This.P5.
**
** This opcode helps to optimize IN operators on a multi-column index
** where the IN operator is on the later terms of the index by avoiding
** unnecessary seeks on the btree, substituting steps to the next row
** of the b-tree instead.  A correct answer is obtained if this opcode
** is omitted or is a no-op.
**
** The SeekGE.P3 and SeekGE.P4 operands identify an unpacked key which
** is the desired entry that we want the cursor SeekGE.P1 to be pointing
** to.  Call this SeekGE.P3/P4 row the "target".
**
** If the SeekGE.P1 cursor is not currently pointing to a valid row,
** then this opcode is a no-op and control passes through into the OP_SeekGE.
**
** If the SeekGE.P1 cursor is pointing to a valid row, then that row
** might be the target row, or it might be near and slightly before the
** target row, or it might be after the target row.  If the cursor is
** currently before the target row, then this opcode attempts to position
** the cursor on or after the target row by invoking sqlite3BtreeStep()
** on the cursor between 1 and This.P1 times.
**
** The This.P5 parameter is a flag that indicates what to do if the
** cursor ends up pointing at a valid row that is past the target
** row.  If This.P5 is false (0) then a jump is made to SeekGE.P2.  If
** This.P5 is true (non-zero) then a jump is made to This.P2.  The P5==0
** case occurs when there are no inequality constraints to the right of
** the IN constraint.  The jump to SeekGE.P2 ends the loop.  The P5!=0 case
** occurs when there are inequality constraints to the right of the IN
** operator.  In that case, the This.P2 will point either directly to or
** to setup code prior to the OP_IdxGT or OP_IdxGE opcode that checks for
** loop terminate.
**
** Possible outcomes from this opcode:<ol>
**
** <li> If the cursor is initially not pointed to any valid row, then
**      fall through into the subsequent OP_SeekGE opcode.
**
** <li> If the cursor is left pointing to a row that is before the target
**      row, even after making as many as This.P1 calls to
**      sqlite3BtreeNext(), then also fall through into OP_SeekGE.
**
** <li> If the cursor is left pointing at the target row, either because it
**      was at the target row to begin with or because one or more
**      sqlite3BtreeNext() calls moved the cursor to the target row,
**      then jump to This.P2..,
**
** <li> If the cursor started out before the target row and a call to
**      to sqlite3BtreeNext() moved the cursor off the end of the index
**      (indicating that the target row definitely does not exist in the
**      btree) then jump to SeekGE.P2, ending the loop.
**
** <li> If the cursor ends up on a valid row that is past the target row
**      (indicating that the target row does not exist in the btree) then
**      jump to SeekOP.P2 if This.P5==0 or to This.P2 if This.P5>0.
** </ol>
*/
case OP_SeekScan: {          /* ncycle */
  VdbeCursor *pC;
  int res;
  int nStep;
  UnpackedRecord r;

  assert( pOp[1].opcode==OP_SeekGE );

  /* If pOp->p5 is clear, then pOp->p2 points to the first instruction past the
  ** OP_IdxGT that follows the OP_SeekGE. Otherwise, it points to the first
  ** opcode past the OP_SeekGE itself.  */
  assert( pOp->p2>=(int)(pOp-aOp)+2 );
#ifdef SQLITE_DEBUG
  if( pOp->p5==0 ){
    /* There are no inequality constraints following the IN constraint. */
    assert( pOp[1].p1==aOp[pOp->p2-1].p1 );
    assert( pOp[1].p2==aOp[pOp->p2-1].p2 );
    assert( pOp[1].p3==aOp[pOp->p2-1].p3 );
    assert( aOp[pOp->p2-1].opcode==OP_IdxGT
         || aOp[pOp->p2-1].opcode==OP_IdxGE );
    testcase( aOp[pOp->p2-1].opcode==OP_IdxGE );
  }else{
    /* There are inequality constraints.  */
    assert( pOp->p2==(int)(pOp-aOp)+2 );
    assert( aOp[pOp->p2-1].opcode==OP_SeekGE );
  }
#endif

  assert( pOp->p1>0 );
  pC = p->apCsr[pOp[1].p1];
  assert( pC!=0 );
  assert( pC->eCurType==CURTYPE_BTREE );
  assert( !pC->isTable );
  if( !sqlite3BtreeCursorIsValidNN(pC->uc.pCursor) ){
#ifdef SQLITE_DEBUG
     if( db->flags&SQLITE_VdbeTrace ){
       printf("... cursor not valid - fall through\n");
     }
#endif
    break;
  }
  nStep = pOp->p1;
  assert( nStep>=1 );
  r.pKeyInfo = pC->pKeyInfo;
  r.nField = (u16)pOp[1].p4.i;
  r.default_rc = 0;
  r.aMem = &aMem[pOp[1].p3];
#ifdef SQLITE_DEBUG
  {
    int i;
    for(i=0; i<r.nField; i++){
      assert( memIsValid(&r.aMem[i]) );
      REGISTER_TRACE(pOp[1].p3+i, &aMem[pOp[1].p3+i]);
    }
  }
#endif
  res = 0;  /* Not needed.  Only used to silence a warning. */
  while(1){
    rc = sqlite3VdbeIdxKeyCompare(db, pC, &r, &res);
    if( rc ) goto abort_due_to_error;
    if( res>0 && pOp->p5==0 ){
      seekscan_search_fail:
      /* Jump to SeekGE.P2, ending the loop */
#ifdef SQLITE_DEBUG
      if( db->flags&SQLITE_VdbeTrace ){
        printf("... %d steps and then skip\n", pOp->p1 - nStep);
      }
#endif
      VdbeBranchTaken(1,3);
      pOp++;
      goto jump_to_p2;
    }
    if( res>=0 ){
      /* Jump to This.P2, bypassing the OP_SeekGE opcode */
#ifdef SQLITE_DEBUG
      if( db->flags&SQLITE_VdbeTrace ){
        printf("... %d steps and then success\n", pOp->p1 - nStep);
      }
#endif
      VdbeBranchTaken(2,3);
      goto jump_to_p2;
      break;
    }
    if( nStep<=0 ){
#ifdef SQLITE_DEBUG
      if( db->flags&SQLITE_VdbeTrace ){
        printf("... fall through after %d steps\n", pOp->p1);
      }
#endif
      VdbeBranchTaken(0,3);
      break;
    }
    nStep--;
    pC->cacheStatus = CACHE_STALE;
    rc = sqlite3BtreeNext(pC->uc.pCursor, 0);
    if( rc ){
      if( rc==SQLITE_DONE ){
        rc = SQLITE_OK;
        goto seekscan_search_fail;
      }else{
        goto abort_due_to_error;
      }
    }
  }

  break;
}


/* Opcode: SeekHit P1 P2 P3 * *
** Synopsis: set P2<=seekHit<=P3
**
** Increase or decrease the seekHit value for cursor P1, if necessary,
** so that it is no less than P2 and no greater than P3.
**
** The seekHit integer represents the maximum of terms in an index for which
** there is known to be at least one match.  If the seekHit value is smaller
** than the total number of equality terms in an index lookup, then the
** OP_IfNoHope opcode might run to see if the IN loop can be abandoned
** early, thus saving work.  This is part of the IN-early-out optimization.
**
** P1 must be a valid b-tree cursor.
*/
case OP_SeekHit: {           /* ncycle */
  VdbeCursor *pC;
  assert( pOp->p1>=0 && pOp->p1<p->nCursor );
  pC = p->apCsr[pOp->p1];
  assert( pC!=0 );
  assert( pOp->p3>=pOp->p2 );
  if( pC->seekHit<pOp->p2 ){
#ifdef SQLITE_DEBUG
    if( db->flags&SQLITE_VdbeTrace ){
      printf("seekHit changes from %d to %d\n", pC->seekHit, pOp->p2);
    }
#endif
    pC->seekHit = pOp->p2;
  }else if( pC->seekHit>pOp->p3 ){
#ifdef SQLITE_DEBUG
    if( db->flags&SQLITE_VdbeTrace ){
      printf("seekHit changes from %d to %d\n", pC->seekHit, pOp->p3);
    }
#endif
    pC->seekHit = pOp->p3;
  }
  break;
}

/* Opcode: IfNotOpen P1 P2 * * *
** Synopsis: if( !csr[P1] ) goto P2
**
** If cursor P1 is not open or if P1 is set to a NULL row using the
** OP_NullRow opcode, then jump to instruction P2. Otherwise, fall through.
*/
case OP_IfNotOpen: {        /* jump */
  VdbeCursor *pCur;

  assert( pOp->p1>=0 && pOp->p1<p->nCursor );
  pCur = p->apCsr[pOp->p1];
  VdbeBranchTaken(pCur==0 || pCur->nullRow, 2);
  if( pCur==0 || pCur->nullRow ){
    goto jump_to_p2_and_check_for_interrupt;
  }
  break;
}

/* Opcode: Found P1 P2 P3 P4 *
** Synopsis: key=r[P3@P4]
**
** If P4==0 then register P3 holds a blob constructed by MakeRecord.  If
** P4>0 then register P3 is the first of P4 registers that form an unpacked
** record.
**
** Cursor P1 is on an index btree.  If the record identified by P3 and P4
** is a prefix of any entry in P1 then a jump is made to P2 and
** P1 is left pointing at the matching entry.
**
** This operation leaves the cursor in a state where it can be
** advanced in the forward direction.  The Next instruction will work,
** but not the Prev instruction.
**
** See also: NotFound, NoConflict, NotExists. SeekGe
*/
/* Opcode: NotFound P1 P2 P3 P4 *
** Synopsis: key=r[P3@P4]
**
** If P4==0 then register P3 holds a blob constructed by MakeRecord.  If
** P4>0 then register P3 is the first of P4 registers that form an unpacked
** record.
**
** Cursor P1 is on an index btree.  If the record identified by P3 and P4
** is not the prefix of any entry in P1 then a jump is made to P2.  If P1
** does contain an entry whose prefix matches the P3/P4 record then control
** falls through to the next instruction and P1 is left pointing at the
** matching entry.
**
** This operation leaves the cursor in a state where it cannot be
** advanced in either direction.  In other words, the Next and Prev
** opcodes do not work after this operation.
**
** See also: Found, NotExists, NoConflict, IfNoHope
*/
/* Opcode: IfNoHope P1 P2 P3 P4 *
** Synopsis: key=r[P3@P4]
**
** Register P3 is the first of P4 registers that form an unpacked
** record.  Cursor P1 is an index btree.  P2 is a jump destination.
** In other words, the operands to this opcode are the same as the
** operands to OP_NotFound and OP_IdxGT.
**
** This opcode is an optimization attempt only.  If this opcode always
** falls through, the correct answer is still obtained, but extra work
** is performed.
**
** A value of N in the seekHit flag of cursor P1 means that there exists
** a key P3:N that will match some record in the index.  We want to know
** if it is possible for a record P3:P4 to match some record in the
** index.  If it is not possible, we can skip some work.  So if seekHit
** is less than P4, attempt to find out if a match is possible by running
** OP_NotFound.
**
** This opcode is used in IN clause processing for a multi-column key.
** If an IN clause is attached to an element of the key other than the
** left-most element, and if there are no matches on the most recent
** seek over the whole key, then it might be that one of the key element
** to the left is prohibiting a match, and hence there is "no hope" of
** any match regardless of how many IN clause elements are checked.
** In such a case, we abandon the IN clause search early, using this
** opcode.  The opcode name comes from the fact that the
** jump is taken if there is "no hope" of achieving a match.
**
** See also: NotFound, SeekHit
*/
/* Opcode: NoConflict P1 P2 P3 P4 *
** Synopsis: key=r[P3@P4]
**
** If P4==0 then register P3 holds a blob constructed by MakeRecord.  If
** P4>0 then register P3 is the first of P4 registers that form an unpacked
** record.
**
** Cursor P1 is on an index btree.  If the record identified by P3 and P4
** contains any NULL value, jump immediately to P2.  If all terms of the
** record are not-NULL then a check is done to determine if any row in the
** P1 index btree has a matching key prefix.  If there are no matches, jump
** immediately to P2.  If there is a match, fall through and leave the P1
** cursor pointing to the matching row.
**
** This opcode is similar to OP_NotFound with the exceptions that the
** branch is always taken if any part of the search key input is NULL.
**
** This operation leaves the cursor in a state where it cannot be
** advanced in either direction.  In other words, the Next and Prev
** opcodes do not work after this operation.
**
** See also: NotFound, Found, NotExists
*/
case OP_IfNoHope: {     /* jump, in3, ncycle */
  VdbeCursor *pC;
  assert( pOp->p1>=0 && pOp->p1<p->nCursor );
  pC = p->apCsr[pOp->p1];
  assert( pC!=0 );
#ifdef SQLITE_DEBUG
  if( db->flags&SQLITE_VdbeTrace ){
    printf("seekHit is %d\n", pC->seekHit);
  }
#endif
  if( pC->seekHit>=pOp->p4.i ) break;
  /* Fall through into OP_NotFound */
  /* no break */ deliberate_fall_through
}
case OP_NoConflict:     /* jump, in3, ncycle */
case OP_NotFound:       /* jump, in3, ncycle */
case OP_Found: {        /* jump, in3, ncycle */
  int alreadyExists;
  int ii;
  VdbeCursor *pC;
  UnpackedRecord *pIdxKey;
  UnpackedRecord r;

#ifdef SQLITE_TEST
  if( pOp->opcode!=OP_NoConflict ) sqlite3_found_count++;
#endif

  assert( pOp->p1>=0 && pOp->p1<p->nCursor );
  assert( pOp->p4type==P4_INT32 );
  pC = p->apCsr[pOp->p1];
  assert( pC!=0 );
#ifdef SQLITE_DEBUG
  pC->seekOp = pOp->opcode;
#endif
  r.aMem = &aMem[pOp->p3];
  assert( pC->eCurType==CURTYPE_BTREE );
  assert( pC->uc.pCursor!=0 );
  assert( pC->isTable==0 );
  r.nField = (u16)pOp->p4.i;
  if( r.nField>0 ){
    /* Key values in an array of registers */
    r.pKeyInfo = pC->pKeyInfo;
    r.default_rc = 0;
#ifdef SQLITE_DEBUG
    for(ii=0; ii<r.nField; ii++){
      assert( memIsValid(&r.aMem[ii]) );
      assert( (r.aMem[ii].flags & MEM_Zero)==0 || r.aMem[ii].n==0 );
      if( ii ) REGISTER_TRACE(pOp->p3+ii, &r.aMem[ii]);
    }
#endif
    rc = sqlite3BtreeIndexMoveto(pC->uc.pCursor, &r, &pC->seekResult);
  }else{
    /* Composite key generated by OP_MakeRecord */
    assert( r.aMem->flags & MEM_Blob );
    assert( pOp->opcode!=OP_NoConflict );
    rc = ExpandBlob(r.aMem);
    assert( rc==SQLITE_OK || rc==SQLITE_NOMEM );
    if( rc ) goto no_mem;
    pIdxKey = sqlite3VdbeAllocUnpackedRecord(pC->pKeyInfo);
    if( pIdxKey==0 ) goto no_mem;
    sqlite3VdbeRecordUnpack(pC->pKeyInfo, r.aMem->n, r.aMem->z, pIdxKey);
    pIdxKey->default_rc = 0;
    rc = sqlite3BtreeIndexMoveto(pC->uc.pCursor, pIdxKey, &pC->seekResult);
    sqlite3DbFreeNN(db, pIdxKey);
  }
  if( rc!=SQLITE_OK ){
    goto abort_due_to_error;
  }
  alreadyExists = (pC->seekResult==0);
  pC->nullRow = 1-alreadyExists;
  pC->deferredMoveto = 0;
  pC->cacheStatus = CACHE_STALE;
  if( pOp->opcode==OP_Found ){
    VdbeBranchTaken(alreadyExists!=0,2);
    if( alreadyExists ) goto jump_to_p2;
  }else{
    if( !alreadyExists ){
      VdbeBranchTaken(1,2);
      goto jump_to_p2;
    }
    if( pOp->opcode==OP_NoConflict ){
      /* For the OP_NoConflict opcode, take the jump if any of the
      ** input fields are NULL, since any key with a NULL will not
      ** conflict */
      for(ii=0; ii<r.nField; ii++){
        if( r.aMem[ii].flags & MEM_Null ){
          VdbeBranchTaken(1,2);
          goto jump_to_p2;
        }
      }
    }
    VdbeBranchTaken(0,2);
    if( pOp->opcode==OP_IfNoHope ){
      pC->seekHit = pOp->p4.i;
    }
  }
  break;
}
