
/* Opcode: ElseEq * P2 * * *
**
** This opcode must follow an OP_Lt or OP_Gt comparison operator.  There
** can be zero or more OP_ReleaseReg opcodes intervening, but no other
** opcodes are allowed to occur between this instruction and the previous
** OP_Lt or OP_Gt.
**
** If the result of an OP_Eq comparison on the same two operands as
** the prior OP_Lt or OP_Gt would have been true, then jump to P2.  If
** the result of an OP_Eq comparison on the two previous operands
** would have been false or NULL, then fall through.
*/
case OP_ElseEq: {       /* same as TK_ESCAPE, jump */

#ifdef SQLITE_DEBUG
  /* Verify the preconditions of this opcode - that it follows an OP_Lt or
  ** OP_Gt with zero or more intervening OP_ReleaseReg opcodes */
  int iAddr;
  for(iAddr = (int)(pOp - aOp) - 1; ALWAYS(iAddr>=0); iAddr--){
    if( aOp[iAddr].opcode==OP_ReleaseReg ) continue;
    assert( aOp[iAddr].opcode==OP_Lt || aOp[iAddr].opcode==OP_Gt );
    break;
  }
#endif /* SQLITE_DEBUG */
  assert( iCompareIsInit );
  VdbeBranchTaken(iCompare==0, 2);
  if( iCompare==0 ) goto jump_to_p2;
  break;
}


/* Opcode: Permutation * * * P4 *
**
** Set the permutation used by the OP_Compare operator in the next
** instruction.  The permutation is stored in the P4 operand.
**
** The permutation is only valid for the next opcode which must be
** an OP_Compare that has the OPFLAG_PERMUTE bit set in P5.
**
** The first integer in the P4 integer array is the length of the array
** and does not become part of the permutation.
*/
case OP_Permutation: {
  assert( pOp->p4type==P4_INTARRAY );
  assert( pOp->p4.ai );
  assert( pOp[1].opcode==OP_Compare );
  assert( pOp[1].p5 & OPFLAG_PERMUTE );
  break;
}

/* Opcode: Compare P1 P2 P3 P4 P5
** Synopsis: r[P1@P3] <-> r[P2@P3]
**
** Compare two vectors of registers in reg(P1)..reg(P1+P3-1) (call this
** vector "A") and in reg(P2)..reg(P2+P3-1) ("B").  Save the result of
** the comparison for use by the next OP_Jump instruct.
**
** If P5 has the OPFLAG_PERMUTE bit set, then the order of comparison is
** determined by the most recent OP_Permutation operator.  If the
** OPFLAG_PERMUTE bit is clear, then register are compared in sequential
** order.
**
** P4 is a KeyInfo structure that defines collating sequences and sort
** orders for the comparison.  The permutation applies to registers
** only.  The KeyInfo elements are used sequentially.
**
** The comparison is a sort comparison, so NULLs compare equal,
** NULLs are less than numbers, numbers are less than strings,
** and strings are less than blobs.
**
** This opcode must be immediately followed by an OP_Jump opcode.
*/
case OP_Compare: {
  int n;
  int i;
  int p1;
  int p2;
  const KeyInfo *pKeyInfo;
  u32 idx;
  CollSeq *pColl;    /* Collating sequence to use on this term */
  int bRev;          /* True for DESCENDING sort order */
  u32 *aPermute;     /* The permutation */

  if( (pOp->p5 & OPFLAG_PERMUTE)==0 ){
    aPermute = 0;
  }else{
    assert( pOp>aOp );
    assert( pOp[-1].opcode==OP_Permutation );
    assert( pOp[-1].p4type==P4_INTARRAY );
    aPermute = pOp[-1].p4.ai + 1;
    assert( aPermute!=0 );
  }
  n = pOp->p3;
  pKeyInfo = pOp->p4.pKeyInfo;
  assert( n>0 );
  assert( pKeyInfo!=0 );
  p1 = pOp->p1;
  p2 = pOp->p2;
#ifdef SQLITE_DEBUG
  if( aPermute ){
    int k, mx = 0;
    for(k=0; k<n; k++) if( aPermute[k]>(u32)mx ) mx = aPermute[k];
    assert( p1>0 && p1+mx<=(p->nMem+1 - p->nCursor)+1 );
    assert( p2>0 && p2+mx<=(p->nMem+1 - p->nCursor)+1 );
  }else{
    assert( p1>0 && p1+n<=(p->nMem+1 - p->nCursor)+1 );
    assert( p2>0 && p2+n<=(p->nMem+1 - p->nCursor)+1 );
  }
#endif /* SQLITE_DEBUG */
  for(i=0; i<n; i++){
    idx = aPermute ? aPermute[i] : (u32)i;
    assert( memIsValid(&aMem[p1+idx]) );
    assert( memIsValid(&aMem[p2+idx]) );
    REGISTER_TRACE(p1+idx, &aMem[p1+idx]);
    REGISTER_TRACE(p2+idx, &aMem[p2+idx]);
    assert( i<pKeyInfo->nKeyField );
    pColl = pKeyInfo->aColl[i];
    bRev = (pKeyInfo->aSortFlags[i] & KEYINFO_ORDER_DESC);
    iCompare = sqlite3MemCompare(&aMem[p1+idx], &aMem[p2+idx], pColl);
    VVA_ONLY( iCompareIsInit = 1; )
    if( iCompare ){
      if( (pKeyInfo->aSortFlags[i] & KEYINFO_ORDER_BIGNULL)
       && ((aMem[p1+idx].flags & MEM_Null) || (aMem[p2+idx].flags & MEM_Null))
      ){
        iCompare = -iCompare;
      }
      if( bRev ) iCompare = -iCompare;
      break;
    }
  }
  assert( pOp[1].opcode==OP_Jump );
  break;
}

/* Opcode: Jump P1 P2 P3 * *
**
** Jump to the instruction at address P1, P2, or P3 depending on whether
** in the most recent OP_Compare instruction the P1 vector was less than,
** equal to, or greater than the P2 vector, respectively.
**
** This opcode must immediately follow an OP_Compare opcode.
*/
case OP_Jump: {             /* jump */
  assert( pOp>aOp && pOp[-1].opcode==OP_Compare );
  assert( iCompareIsInit );
  if( iCompare<0 ){
    VdbeBranchTaken(0,4); pOp = &aOp[pOp->p1 - 1];
  }else if( iCompare==0 ){
    VdbeBranchTaken(1,4); pOp = &aOp[pOp->p2 - 1];
  }else{
    VdbeBranchTaken(2,4); pOp = &aOp[pOp->p3 - 1];
  }
  break;
}

/* Opcode: And P1 P2 P3 * *
** Synopsis: r[P3]=(r[P1] && r[P2])
**
** Take the logical AND of the values in registers P1 and P2 and
** write the result into register P3.
**
** If either P1 or P2 is 0 (false) then the result is 0 even if
** the other input is NULL.  A NULL and true or two NULLs give
** a NULL output.
*/
/* Opcode: Or P1 P2 P3 * *
** Synopsis: r[P3]=(r[P1] || r[P2])
**
** Take the logical OR of the values in register P1 and P2 and
** store the answer in register P3.
**
** If either P1 or P2 is nonzero (true) then the result is 1 (true)
** even if the other input is NULL.  A NULL and false or two NULLs
** give a NULL output.
*/
case OP_And:              /* same as TK_AND, in1, in2, out3 */
case OP_Or: {             /* same as TK_OR, in1, in2, out3 */
  int v1;    /* Left operand:  0==FALSE, 1==TRUE, 2==UNKNOWN or NULL */
  int v2;    /* Right operand: 0==FALSE, 1==TRUE, 2==UNKNOWN or NULL */

  v1 = sqlite3VdbeBooleanValue(&aMem[pOp->p1], 2);
  v2 = sqlite3VdbeBooleanValue(&aMem[pOp->p2], 2);
  if( pOp->opcode==OP_And ){
    static const unsigned char and_logic[] = { 0, 0, 0, 0, 1, 2, 0, 2, 2 };
    v1 = and_logic[v1*3+v2];
  }else{
    static const unsigned char or_logic[] = { 0, 1, 2, 1, 1, 1, 2, 1, 2 };
    v1 = or_logic[v1*3+v2];
  }
  pOut = &aMem[pOp->p3];
  if( v1==2 ){
    MemSetTypeFlag(pOut, MEM_Null);
  }else{
    pOut->u.i = v1;
    MemSetTypeFlag(pOut, MEM_Int);
  }
  break;
}

/* Opcode: IsTrue P1 P2 P3 P4 *
** Synopsis: r[P2] = coalesce(r[P1]==TRUE,P3) ^ P4
**
** This opcode implements the IS TRUE, IS FALSE, IS NOT TRUE, and
** IS NOT FALSE operators.
**
** Interpret the value in register P1 as a boolean value.  Store that
** boolean (a 0 or 1) in register P2.  Or if the value in register P1 is
** NULL, then the P3 is stored in register P2.  Invert the answer if P4
** is 1.
**
** The logic is summarized like this:
**
** <ul>
** <li> If P3==0 and P4==0  then  r[P2] := r[P1] IS TRUE
** <li> If P3==1 and P4==1  then  r[P2] := r[P1] IS FALSE
** <li> If P3==0 and P4==1  then  r[P2] := r[P1] IS NOT TRUE
** <li> If P3==1 and P4==0  then  r[P2] := r[P1] IS NOT FALSE
** </ul>
*/
case OP_IsTrue: {               /* in1, out2 */
  assert( pOp->p4type==P4_INT32 );
  assert( pOp->p4.i==0 || pOp->p4.i==1 );
  assert( pOp->p3==0 || pOp->p3==1 );
  sqlite3VdbeMemSetInt64(&aMem[pOp->p2],
      sqlite3VdbeBooleanValue(&aMem[pOp->p1], pOp->p3) ^ pOp->p4.i);
  break;
}

/* Opcode: Not P1 P2 * * *
** Synopsis: r[P2]= !r[P1]
**
** Interpret the value in register P1 as a boolean value.  Store the
** boolean complement in register P2.  If the value in register P1 is
** NULL, then a NULL is stored in P2.
*/
case OP_Not: {                /* same as TK_NOT, in1, out2 */
  pIn1 = &aMem[pOp->p1];
  pOut = &aMem[pOp->p2];
  if( (pIn1->flags & MEM_Null)==0 ){
    sqlite3VdbeMemSetInt64(pOut, !sqlite3VdbeBooleanValue(pIn1,0));
  }else{
    sqlite3VdbeMemSetNull(pOut);
  }
  break;
}

/* Opcode: BitNot P1 P2 * * *
** Synopsis: r[P2]= ~r[P1]
**
** Interpret the content of register P1 as an integer.  Store the
** ones-complement of the P1 value into register P2.  If P1 holds
** a NULL then store a NULL in P2.
*/
case OP_BitNot: {             /* same as TK_BITNOT, in1, out2 */
  pIn1 = &aMem[pOp->p1];
  pOut = &aMem[pOp->p2];
  sqlite3VdbeMemSetNull(pOut);
  if( (pIn1->flags & MEM_Null)==0 ){
    pOut->flags = MEM_Int;
    pOut->u.i = ~sqlite3VdbeIntValue(pIn1);
  }
  break;
}

/* Opcode: Once P1 P2 * * *
**
** Fall through to the next instruction the first time this opcode is
** encountered on each invocation of the byte-code program.  Jump to P2
** on the second and all subsequent encounters during the same invocation.
**
** Top-level programs determine first invocation by comparing the P1
** operand against the P1 operand on the OP_Init opcode at the beginning
** of the program.  If the P1 values differ, then fall through and make
** the P1 of this opcode equal to the P1 of OP_Init.  If P1 values are
** the same then take the jump.
**
** For subprograms, there is a bitmask in the VdbeFrame that determines
** whether or not the jump should be taken.  The bitmask is necessary
** because the self-altering code trick does not work for recursive
** triggers.
*/
case OP_Once: {             /* jump */
  u32 iAddr;                /* Address of this instruction */
  assert( p->aOp[0].opcode==OP_Init );
  if( p->pFrame ){
    iAddr = (int)(pOp - p->aOp);
    if( (p->pFrame->aOnce[iAddr/8] & (1<<(iAddr & 7)))!=0 ){
      VdbeBranchTaken(1, 2);
      goto jump_to_p2;
    }
    p->pFrame->aOnce[iAddr/8] |= 1<<(iAddr & 7);
  }else{
    if( p->aOp[0].p1==pOp->p1 ){
      VdbeBranchTaken(1, 2);
      goto jump_to_p2;
    }
  }
  VdbeBranchTaken(0, 2);
  pOp->p1 = p->aOp[0].p1;
  break;
}

/* Opcode: If P1 P2 P3 * *
**
** Jump to P2 if the value in register P1 is true.  The value
** is considered true if it is numeric and non-zero.  If the value
** in P1 is NULL then take the jump if and only if P3 is non-zero.
*/
case OP_If:  {               /* jump, in1 */
  int c;
  c = sqlite3VdbeBooleanValue(&aMem[pOp->p1], pOp->p3);
  VdbeBranchTaken(c!=0, 2);
  if( c ) goto jump_to_p2;
  break;
}

/* Opcode: IfNot P1 P2 P3 * *
**
** Jump to P2 if the value in register P1 is False.  The value
** is considered false if it has a numeric value of zero.  If the value
** in P1 is NULL then take the jump if and only if P3 is non-zero.
*/
case OP_IfNot: {            /* jump, in1 */
  int c;
  c = !sqlite3VdbeBooleanValue(&aMem[pOp->p1], !pOp->p3);
  VdbeBranchTaken(c!=0, 2);
  if( c ) goto jump_to_p2;
  break;
}

/* Opcode: IsNull P1 P2 * * *
** Synopsis: if r[P1]==NULL goto P2
**
** Jump to P2 if the value in register P1 is NULL.
*/
case OP_IsNull: {            /* same as TK_ISNULL, jump, in1 */
  pIn1 = &aMem[pOp->p1];
  VdbeBranchTaken( (pIn1->flags & MEM_Null)!=0, 2);
  if( (pIn1->flags & MEM_Null)!=0 ){
    goto jump_to_p2;
  }
  break;
}

/* Opcode: IsType P1 P2 P3 P4 P5
** Synopsis: if typeof(P1.P3) in P5 goto P2
**
** Jump to P2 if the type of a column in a btree is one of the types specified
** by the P5 bitmask.
**
** P1 is normally a cursor on a btree for which the row decode cache is
** valid through at least column P3.  In other words, there should have been
** a prior OP_Column for column P3 or greater.  If the cursor is not valid,
** then this opcode might give spurious results.
** The the btree row has fewer than P3 columns, then use P4 as the
** datatype.
**
** If P1 is -1, then P3 is a register number and the datatype is taken
** from the value in that register.
**
** P5 is a bitmask of data types.  SQLITE_INTEGER is the least significant
** (0x01) bit. SQLITE_FLOAT is the 0x02 bit. SQLITE_TEXT is 0x04.
** SQLITE_BLOB is 0x08.  SQLITE_NULL is 0x10.
**
** WARNING: This opcode does not reliably distinguish between NULL and REAL
** when P1>=0.  If the database contains a NaN value, this opcode will think
** that the datatype is REAL when it should be NULL.  When P1<0 and the value
** is already stored in register P3, then this opcode does reliably
** distinguish between NULL and REAL.  The problem only arises then P1>=0.
**
** Take the jump to address P2 if and only if the datatype of the
** value determined by P1 and P3 corresponds to one of the bits in the
** P5 bitmask.
**
*/
case OP_IsType: {        /* jump */
  VdbeCursor *pC;
  u16 typeMask;
  u32 serialType;

  assert( pOp->p1>=(-1) && pOp->p1<p->nCursor );
  assert( pOp->p1>=0 || (pOp->p3>=0 && pOp->p3<=(p->nMem+1 - p->nCursor)) );
  if( pOp->p1>=0 ){
    pC = p->apCsr[pOp->p1];
    assert( pC!=0 );
    assert( pOp->p3>=0 );
    if( pOp->p3<pC->nHdrParsed ){
      serialType = pC->aType[pOp->p3];
      if( serialType>=12 ){
        if( serialType&1 ){
          typeMask = 0x04;   /* SQLITE_TEXT */
        }else{
          typeMask = 0x08;   /* SQLITE_BLOB */
        }
      }else{
        static const unsigned char aMask[] = {
           0x10, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x2,
           0x01, 0x01, 0x10, 0x10
        };
        testcase( serialType==0 );
        testcase( serialType==1 );
        testcase( serialType==2 );
        testcase( serialType==3 );
        testcase( serialType==4 );
        testcase( serialType==5 );
        testcase( serialType==6 );
        testcase( serialType==7 );
        testcase( serialType==8 );
        testcase( serialType==9 );
        testcase( serialType==10 );
        testcase( serialType==11 );
        typeMask = aMask[serialType];
      }
    }else{
      typeMask = 1 << (pOp->p4.i - 1);
      testcase( typeMask==0x01 );
      testcase( typeMask==0x02 );
      testcase( typeMask==0x04 );
      testcase( typeMask==0x08 );
      testcase( typeMask==0x10 );
    }
  }else{
    assert( memIsValid(&aMem[pOp->p3]) );
    typeMask = 1 << (sqlite3_value_type((sqlite3_value*)&aMem[pOp->p3])-1);
    testcase( typeMask==0x01 );
    testcase( typeMask==0x02 );
    testcase( typeMask==0x04 );
    testcase( typeMask==0x08 );
    testcase( typeMask==0x10 );
  }
  VdbeBranchTaken( (typeMask & pOp->p5)!=0, 2);
  if( typeMask & pOp->p5 ){
    goto jump_to_p2;
  }
  break;
}
