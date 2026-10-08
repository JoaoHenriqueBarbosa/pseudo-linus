
/* Opcode: RowCell P1 P2 P3 * *
**
** P1 and P2 are both open cursors. Both must be opened on the same type
** of table - intkey or index. This opcode is used as part of copying
** the current row from P2 into P1. If the cursors are opened on intkey
** tables, register P3 contains the rowid to use with the new record in
** P1. If they are opened on index tables, P3 is not used.
**
** This opcode must be followed by either an Insert or InsertIdx opcode
** with the OPFLAG_PREFORMAT flag set to complete the insert operation.
*/
case OP_RowCell: {
  VdbeCursor *pDest;              /* Cursor to write to */
  VdbeCursor *pSrc;               /* Cursor to read from */
  i64 iKey;                       /* Rowid value to insert with */
  assert( pOp[1].opcode==OP_Insert || pOp[1].opcode==OP_IdxInsert );
  assert( pOp[1].opcode==OP_Insert    || pOp->p3==0 );
  assert( pOp[1].opcode==OP_IdxInsert || pOp->p3>0 );
  assert( pOp[1].p5 & OPFLAG_PREFORMAT );
  pDest = p->apCsr[pOp->p1];
  pSrc = p->apCsr[pOp->p2];
  iKey = pOp->p3 ? aMem[pOp->p3].u.i : 0;
  rc = sqlite3BtreeTransferRow(pDest->uc.pCursor, pSrc->uc.pCursor, iKey);
  if( rc!=SQLITE_OK ) goto abort_due_to_error;
  break;
};

/* Opcode: Delete P1 P2 P3 P4 P5
**
** Delete the record at which the P1 cursor is currently pointing.
**
** If the OPFLAG_SAVEPOSITION bit of the P5 parameter is set, then
** the cursor will be left pointing at  either the next or the previous
** record in the table. If it is left pointing at the next record, then
** the next Next instruction will be a no-op. As a result, in this case
** it is ok to delete a record from within a Next loop. If
** OPFLAG_SAVEPOSITION bit of P5 is clear, then the cursor will be
** left in an undefined state.
**
** If the OPFLAG_AUXDELETE bit is set on P5, that indicates that this
** delete is one of several associated with deleting a table row and
** all its associated index entries.  Exactly one of those deletes is
** the "primary" delete.  The others are all on OPFLAG_FORDELETE
** cursors or else are marked with the AUXDELETE flag.
**
** If the OPFLAG_NCHANGE (0x01) flag of P2 (NB: P2 not P5) is set, then
** the row change count is incremented (otherwise not).
**
** If the OPFLAG_ISNOOP (0x40) flag of P2 (not P5!) is set, then the
** pre-update-hook for deletes is run, but the btree is otherwise unchanged.
** This happens when the OP_Delete is to be shortly followed by an OP_Insert
** with the same key, causing the btree entry to be overwritten.
**
** P1 must not be pseudo-table.  It has to be a real table with
** multiple rows.
**
** If P4 is not NULL then it points to a Table object. In this case either
** the update or pre-update hook, or both, may be invoked. The P1 cursor must
** have been positioned using OP_NotFound prior to invoking this opcode in
** this case. Specifically, if one is configured, the pre-update hook is
** invoked if P4 is not NULL. The update-hook is invoked if one is configured,
** P4 is not NULL, and the OPFLAG_NCHANGE flag is set in P2.
**
** If the OPFLAG_ISUPDATE flag is set in P2, then P3 contains the address
** of the memory cell that contains the value that the rowid of the row will
** be set to by the update.
*/
case OP_Delete: {
  VdbeCursor *pC;
  const char *zDb;
  Table *pTab;
  int opflags;

  opflags = pOp->p2;
  assert( pOp->p1>=0 && pOp->p1<p->nCursor );
  pC = p->apCsr[pOp->p1];
  assert( pC!=0 );
  assert( pC->eCurType==CURTYPE_BTREE );
  assert( pC->uc.pCursor!=0 );
  assert( pC->deferredMoveto==0 );
  sqlite3VdbeIncrWriteCounter(p, pC);

#ifdef SQLITE_DEBUG
  if( pOp->p4type==P4_TABLE
   && HasRowid(pOp->p4.pTab)
   && pOp->p5==0
   && sqlite3BtreeCursorIsValidNN(pC->uc.pCursor)
  ){
    /* If p5 is zero, the seek operation that positioned the cursor prior to
    ** OP_Delete will have also set the pC->movetoTarget field to the rowid of
    ** the row that is being deleted */
    i64 iKey = sqlite3BtreeIntegerKey(pC->uc.pCursor);
    assert( CORRUPT_DB || pC->movetoTarget==iKey );
  }
#endif

  /* If the update-hook or pre-update-hook will be invoked, set zDb to
  ** the name of the db to pass as to it. Also set local pTab to a copy
  ** of p4.pTab. Finally, if p5 is true, indicating that this cursor was
  ** last moved with OP_Next or OP_Prev, not Seek or NotFound, set
  ** VdbeCursor.movetoTarget to the current rowid.  */
  if( pOp->p4type==P4_TABLE && HAS_UPDATE_HOOK(db) ){
    assert( pC->iDb>=0 );
    assert( pOp->p4.pTab!=0 );
    zDb = db->aDb[pC->iDb].zDbSName;
    pTab = pOp->p4.pTab;
    if( (pOp->p5 & OPFLAG_SAVEPOSITION)!=0 && pC->isTable ){
      pC->movetoTarget = sqlite3BtreeIntegerKey(pC->uc.pCursor);
    }
  }else{
    zDb = 0;
    pTab = 0;
  }

#ifdef SQLITE_ENABLE_PREUPDATE_HOOK
  /* Invoke the pre-update-hook if required. */
  assert( db->xPreUpdateCallback==0 || pTab==pOp->p4.pTab );
  if( db->xPreUpdateCallback && pTab ){
    assert( !(opflags & OPFLAG_ISUPDATE)
         || HasRowid(pTab)==0
         || (aMem[pOp->p3].flags & MEM_Int)
    );
    sqlite3VdbePreUpdateHook(p, pC,
        (opflags & OPFLAG_ISUPDATE) ? SQLITE_UPDATE : SQLITE_DELETE,
        zDb, pTab, pC->movetoTarget,
        pOp->p3, -1
    );
  }
  if( opflags & OPFLAG_ISNOOP ) break;
#endif

  /* Only flags that can be set are SAVEPOISTION and AUXDELETE */
  assert( (pOp->p5 & ~(OPFLAG_SAVEPOSITION|OPFLAG_AUXDELETE))==0 );
  assert( OPFLAG_SAVEPOSITION==BTREE_SAVEPOSITION );
  assert( OPFLAG_AUXDELETE==BTREE_AUXDELETE );

#ifdef SQLITE_DEBUG
  if( p->pFrame==0 ){
    if( pC->isEphemeral==0
        && (pOp->p5 & OPFLAG_AUXDELETE)==0
        && (pC->wrFlag & OPFLAG_FORDELETE)==0
      ){
      nExtraDelete++;
    }
    if( pOp->p2 & OPFLAG_NCHANGE ){
      nExtraDelete--;
    }
  }
#endif

  rc = sqlite3BtreeDelete(pC->uc.pCursor, pOp->p5);
  pC->cacheStatus = CACHE_STALE;
  colCacheCtr++;
  pC->seekResult = 0;
  if( rc ) goto abort_due_to_error;

  /* Invoke the update-hook if required. */
  if( opflags & OPFLAG_NCHANGE ){
    p->nChange++;
    if( db->xUpdateCallback && ALWAYS(pTab!=0) && HasRowid(pTab) ){
      db->xUpdateCallback(db->pUpdateArg, SQLITE_DELETE, zDb, pTab->zName,
          pC->movetoTarget);
      assert( pC->iDb>=0 );
    }
  }

  break;
}
/* Opcode: ResetCount * * * * *
**
** The value of the change counter is copied to the database handle
** change counter (returned by subsequent calls to sqlite3_changes()).
** Then the VMs internal change counter resets to 0.
** This is used by trigger programs.
*/
case OP_ResetCount: {
  sqlite3VdbeSetChanges(db, p->nChange);
  p->nChange = 0;
  break;
}

/* Opcode: SorterCompare P1 P2 P3 P4
** Synopsis: if key(P1)!=trim(r[P3],P4) goto P2
**
** P1 is a sorter cursor. This instruction compares a prefix of the
** record blob in register P3 against a prefix of the entry that
** the sorter cursor currently points to.  Only the first P4 fields
** of r[P3] and the sorter record are compared.
**
** If either P3 or the sorter contains a NULL in one of their significant
** fields (not counting the P4 fields at the end which are ignored) then
** the comparison is assumed to be equal.
**
** Fall through to next instruction if the two records compare equal to
** each other.  Jump to P2 if they are different.
*/
case OP_SorterCompare: {
  VdbeCursor *pC;
  int res;
  int nKeyCol;

  pC = p->apCsr[pOp->p1];
  assert( isSorter(pC) );
  assert( pOp->p4type==P4_INT32 );
  pIn3 = &aMem[pOp->p3];
  nKeyCol = pOp->p4.i;
  res = 0;
  rc = sqlite3VdbeSorterCompare(pC, pIn3, nKeyCol, &res);
  VdbeBranchTaken(res!=0,2);
  if( rc ) goto abort_due_to_error;
  if( res ) goto jump_to_p2;
  break;
};

/* Opcode: SorterData P1 P2 P3 * *
** Synopsis: r[P2]=data
**
** Write into register P2 the current sorter data for sorter cursor P1.
** Then clear the column header cache on cursor P3.
**
** This opcode is normally used to move a record out of the sorter and into
** a register that is the source for a pseudo-table cursor created using
** OpenPseudo.  That pseudo-table cursor is the one that is identified by
** parameter P3.  Clearing the P3 column cache as part of this opcode saves
** us from having to issue a separate NullRow instruction to clear that cache.
*/
case OP_SorterData: {       /* ncycle */
  VdbeCursor *pC;

  pOut = &aMem[pOp->p2];
  pC = p->apCsr[pOp->p1];
  assert( isSorter(pC) );
  rc = sqlite3VdbeSorterRowkey(pC, pOut);
  assert( rc!=SQLITE_OK || (pOut->flags & MEM_Blob) );
  assert( pOp->p1>=0 && pOp->p1<p->nCursor );
  if( rc ) goto abort_due_to_error;
  p->apCsr[pOp->p3]->cacheStatus = CACHE_STALE;
  break;
}

/* Opcode: RowData P1 P2 P3 * *
** Synopsis: r[P2]=data
**
** Write into register P2 the complete row content for the row at
** which cursor P1 is currently pointing.
** There is no interpretation of the data.
** It is just copied onto the P2 register exactly as
** it is found in the database file.
**
** If cursor P1 is an index, then the content is the key of the row.
** If cursor P2 is a table, then the content extracted is the data.
**
** If the P1 cursor must be pointing to a valid row (not a NULL row)
** of a real table, not a pseudo-table.
**
** If P3!=0 then this opcode is allowed to make an ephemeral pointer
** into the database page.  That means that the content of the output
** register will be invalidated as soon as the cursor moves - including
** moves caused by other cursors that "save" the current cursors
** position in order that they can write to the same table.  If P3==0
** then a copy of the data is made into memory.  P3!=0 is faster, but
** P3==0 is safer.
**
** If P3!=0 then the content of the P2 register is unsuitable for use
** in OP_Result and any OP_Result will invalidate the P2 register content.
** The P2 register content is invalidated by opcodes like OP_Function or
** by any use of another cursor pointing to the same table.
*/
case OP_RowData: {
  VdbeCursor *pC;
  BtCursor *pCrsr;
  u32 n;

  pOut = out2Prerelease(p, pOp);

  assert( pOp->p1>=0 && pOp->p1<p->nCursor );
  pC = p->apCsr[pOp->p1];
  assert( pC!=0 );
  assert( pC->eCurType==CURTYPE_BTREE );
  assert( isSorter(pC)==0 );
  assert( pC->nullRow==0 );
  assert( pC->uc.pCursor!=0 );
  pCrsr = pC->uc.pCursor;

  /* The OP_RowData opcodes always follow OP_NotExists or
  ** OP_SeekRowid or OP_Rewind/Op_Next with no intervening instructions
  ** that might invalidate the cursor.
  ** If this where not the case, on of the following assert()s
  ** would fail.  Should this ever change (because of changes in the code
  ** generator) then the fix would be to insert a call to
  ** sqlite3VdbeCursorMoveto().
  */
  assert( pC->deferredMoveto==0 );
  assert( sqlite3BtreeCursorIsValid(pCrsr) );

  n = sqlite3BtreePayloadSize(pCrsr);
  if( n>(u32)db->aLimit[SQLITE_LIMIT_LENGTH] ){
    goto too_big;
  }
  testcase( n==0 );
  rc = sqlite3VdbeMemFromBtreeZeroOffset(pCrsr, n, pOut);
  if( rc ) goto abort_due_to_error;
  if( !pOp->p3 ) Deephemeralize(pOut);
  UPDATE_MAX_BLOBSIZE(pOut);
  REGISTER_TRACE(pOp->p2, pOut);
  break;
}

/* Opcode: Rowid P1 P2 * * *
** Synopsis: r[P2]=PX rowid of P1
**
** Store in register P2 an integer which is the key of the table entry that
** P1 is currently point to.
**
** P1 can be either an ordinary table or a virtual table.  There used to
** be a separate OP_VRowid opcode for use with virtual tables, but this
** one opcode now works for both table types.
*/
case OP_Rowid: {                 /* out2, ncycle */
  VdbeCursor *pC;
  i64 v;
  sqlite3_vtab *pVtab;
  const sqlite3_module *pModule;

  pOut = out2Prerelease(p, pOp);
  assert( pOp->p1>=0 && pOp->p1<p->nCursor );
  pC = p->apCsr[pOp->p1];
  assert( pC!=0 );
  assert( pC->eCurType!=CURTYPE_PSEUDO || pC->nullRow );
  if( pC->nullRow ){
    pOut->flags = MEM_Null;
    break;
  }else if( pC->deferredMoveto ){
    v = pC->movetoTarget;
#ifndef SQLITE_OMIT_VIRTUALTABLE
  }else if( pC->eCurType==CURTYPE_VTAB ){
    assert( pC->uc.pVCur!=0 );
    pVtab = pC->uc.pVCur->pVtab;
    pModule = pVtab->pModule;
    assert( pModule->xRowid );
    rc = pModule->xRowid(pC->uc.pVCur, &v);
    sqlite3VtabImportErrmsg(p, pVtab);
    if( rc ) goto abort_due_to_error;
#endif /* SQLITE_OMIT_VIRTUALTABLE */
  }else{
    assert( pC->eCurType==CURTYPE_BTREE );
    assert( pC->uc.pCursor!=0 );
    rc = sqlite3VdbeCursorRestore(pC);
    if( rc ) goto abort_due_to_error;
    if( pC->nullRow ){
      pOut->flags = MEM_Null;
      break;
    }
    v = sqlite3BtreeIntegerKey(pC->uc.pCursor);
  }
  pOut->u.i = v;
  break;
}
