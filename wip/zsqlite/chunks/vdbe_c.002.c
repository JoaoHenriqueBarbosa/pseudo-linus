

/*
** Return the symbolic name for the data type of a pMem
*/
static const char *vdbeMemTypeName(Mem *pMem){
  static const char *azTypes[] = {
      /* SQLITE_INTEGER */ "INT",
      /* SQLITE_FLOAT   */ "REAL",
      /* SQLITE_TEXT    */ "TEXT",
      /* SQLITE_BLOB    */ "BLOB",
      /* SQLITE_NULL    */ "NULL"
  };
  return azTypes[sqlite3_value_type(pMem)-1];
}

/*
** Execute as much of a VDBE program as we can.
** This is the core of sqlite3_step().
*/
SQLITE_PRIVATE int sqlite3VdbeExec(
  Vdbe *p                    /* The VDBE */
){
  Op *aOp = p->aOp;          /* Copy of p->aOp */
  Op *pOp = aOp;             /* Current operation */
#ifdef SQLITE_DEBUG
  Op *pOrigOp;               /* Value of pOp at the top of the loop */
  int nExtraDelete = 0;      /* Verifies FORDELETE and AUXDELETE flags */
  u8 iCompareIsInit = 0;     /* iCompare is initialized */
#endif
  int rc = SQLITE_OK;        /* Value to return */
  sqlite3 *db = p->db;       /* The database */
  u8 resetSchemaOnFault = 0; /* Reset schema after an error if positive */
  u8 encoding = ENC(db);     /* The database encoding */
  int iCompare = 0;          /* Result of last comparison */
  u64 nVmStep = 0;           /* Number of virtual machine steps */
#ifndef SQLITE_OMIT_PROGRESS_CALLBACK
  u64 nProgressLimit;        /* Invoke xProgress() when nVmStep reaches this */
#endif
  Mem *aMem = p->aMem;       /* Copy of p->aMem */
  Mem *pIn1 = 0;             /* 1st input operand */
  Mem *pIn2 = 0;             /* 2nd input operand */
  Mem *pIn3 = 0;             /* 3rd input operand */
  Mem *pOut = 0;             /* Output operand */
  u32 colCacheCtr = 0;       /* Column cache counter */
#if defined(SQLITE_ENABLE_STMT_SCANSTATUS) || defined(VDBE_PROFILE)
  u64 *pnCycle = 0;
  int bStmtScanStatus = IS_STMT_SCANSTATUS(db)!=0;
#endif
  /*** INSERT STACK UNION HERE ***/

  assert( p->eVdbeState==VDBE_RUN_STATE );  /* sqlite3_step() verifies this */
  if( DbMaskNonZero(p->lockMask) ){
    sqlite3VdbeEnter(p);
  }
#ifndef SQLITE_OMIT_PROGRESS_CALLBACK
  if( db->xProgress ){
    u32 iPrior = p->aCounter[SQLITE_STMTSTATUS_VM_STEP];
    assert( 0 < db->nProgressOps );
    nProgressLimit = db->nProgressOps - (iPrior % db->nProgressOps);
  }else{
    nProgressLimit = LARGEST_UINT64;
  }
#endif
  if( p->rc==SQLITE_NOMEM ){
    /* This happens if a malloc() inside a call to sqlite3_column_text() or
    ** sqlite3_column_text16() failed.  */
    goto no_mem;
  }
  assert( p->rc==SQLITE_OK || (p->rc&0xff)==SQLITE_BUSY );
  testcase( p->rc!=SQLITE_OK );
  p->rc = SQLITE_OK;
  assert( p->bIsReader || p->readOnly!=0 );
  p->iCurrentTime = 0;
  assert( p->explain==0 );
  db->busyHandler.nBusy = 0;
  if( AtomicLoad(&db->u1.isInterrupted) ) goto abort_due_to_interrupt;
  sqlite3VdbeIOTraceSql(p);
#ifdef SQLITE_DEBUG
  sqlite3BeginBenignMalloc();
  if( p->pc==0
   && (p->db->flags & (SQLITE_VdbeListing|SQLITE_VdbeEQP|SQLITE_VdbeTrace))!=0
  ){
    int i;
    int once = 1;
    sqlite3VdbePrintSql(p);
    if( p->db->flags & SQLITE_VdbeListing ){
      printf("VDBE Program Listing:\n");
      for(i=0; i<p->nOp; i++){
        sqlite3VdbePrintOp(stdout, i, &aOp[i]);
      }
    }
    if( p->db->flags & SQLITE_VdbeEQP ){
      for(i=0; i<p->nOp; i++){
        if( aOp[i].opcode==OP_Explain ){
          if( once ) printf("VDBE Query Plan:\n");
          printf("%s\n", aOp[i].p4.z);
          once = 0;
        }
      }
    }
    if( p->db->flags & SQLITE_VdbeTrace )  printf("VDBE Trace:\n");
  }
  sqlite3EndBenignMalloc();
#endif
  for(pOp=&aOp[p->pc]; 1; pOp++){
    /* Errors are detected by individual opcodes, with an immediate
    ** jumps to abort_due_to_error. */
    assert( rc==SQLITE_OK );

    assert( pOp>=aOp && pOp<&aOp[p->nOp]);
    nVmStep++;

#if defined(VDBE_PROFILE)
    pOp->nExec++;
    pnCycle = &pOp->nCycle;
    if( sqlite3NProfileCnt==0 ) *pnCycle -= sqlite3Hwtime();
#elif defined(SQLITE_ENABLE_STMT_SCANSTATUS)
    if( bStmtScanStatus ){
      pOp->nExec++;
      pnCycle = &pOp->nCycle;
      *pnCycle -= sqlite3Hwtime();
    }
#endif

    /* Only allow tracing if SQLITE_DEBUG is defined.
    */
#ifdef SQLITE_DEBUG
    if( db->flags & SQLITE_VdbeTrace ){
      sqlite3VdbePrintOp(stdout, (int)(pOp - aOp), pOp);
      test_trace_breakpoint((int)(pOp - aOp),pOp,p);
    }
#endif


    /* Check to see if we need to simulate an interrupt.  This only happens
    ** if we have a special test build.
    */
#ifdef SQLITE_TEST
    if( sqlite3_interrupt_count>0 ){
      sqlite3_interrupt_count--;
      if( sqlite3_interrupt_count==0 ){
        sqlite3_interrupt(db);
      }
    }
#endif

    /* Sanity checking on other operands */
#ifdef SQLITE_DEBUG
    {
      u8 opProperty = sqlite3OpcodeProperty[pOp->opcode];
      if( (opProperty & OPFLG_IN1)!=0 ){
        assert( pOp->p1>0 );
        assert( pOp->p1<=(p->nMem+1 - p->nCursor) );
        assert( memIsValid(&aMem[pOp->p1]) );
        assert( sqlite3VdbeCheckMemInvariants(&aMem[pOp->p1]) );
        REGISTER_TRACE(pOp->p1, &aMem[pOp->p1]);
      }
      if( (opProperty & OPFLG_IN2)!=0 ){
        assert( pOp->p2>0 );
        assert( pOp->p2<=(p->nMem+1 - p->nCursor) );
        assert( memIsValid(&aMem[pOp->p2]) );
        assert( sqlite3VdbeCheckMemInvariants(&aMem[pOp->p2]) );
        REGISTER_TRACE(pOp->p2, &aMem[pOp->p2]);
      }
      if( (opProperty & OPFLG_IN3)!=0 ){
        assert( pOp->p3>0 );
        assert( pOp->p3<=(p->nMem+1 - p->nCursor) );
        assert( memIsValid(&aMem[pOp->p3]) );
        assert( sqlite3VdbeCheckMemInvariants(&aMem[pOp->p3]) );
        REGISTER_TRACE(pOp->p3, &aMem[pOp->p3]);
      }
      if( (opProperty & OPFLG_OUT2)!=0 ){
        assert( pOp->p2>0 );
        assert( pOp->p2<=(p->nMem+1 - p->nCursor) );
        memAboutToChange(p, &aMem[pOp->p2]);
      }
      if( (opProperty & OPFLG_OUT3)!=0 ){
        assert( pOp->p3>0 );
        assert( pOp->p3<=(p->nMem+1 - p->nCursor) );
        memAboutToChange(p, &aMem[pOp->p3]);
      }
    }
#endif
#ifdef SQLITE_DEBUG
    pOrigOp = pOp;
#endif

    switch( pOp->opcode ){

/*****************************************************************************
** What follows is a massive switch statement where each case implements a
** separate instruction in the virtual machine.  If we follow the usual
** indentation conventions, each case should be indented by 6 spaces.  But
** that is a lot of wasted space on the left margin.  So the code within
** the switch statement will break with convention and be flush-left. Another
** big comment (similar to this one) will mark the point in the code where
** we transition back to normal indentation.
**
** The formatting of each case is important.  The makefile for SQLite
** generates two C files "opcodes.h" and "opcodes.c" by scanning this
** file looking for lines that begin with "case OP_".  The opcodes.h files
** will be filled with #defines that give unique integer values to each
** opcode and the opcodes.c file is filled with an array of strings where
** each string is the symbolic name for the corresponding opcode.  If the
** case statement is followed by a comment of the form "/# same as ... #/"
** that comment is used to determine the particular value of the opcode.
**
** Other keywords in the comment that follows each case are used to
** construct the OPFLG_INITIALIZER value that initializes opcodeProperty[].
** Keywords include: in1, in2, in3, out2, out3.  See
** the mkopcodeh.awk script for additional information.
**
** Documentation about VDBE opcodes is generated by scanning this file
** for lines of that contain "Opcode:".  That line and all subsequent
** comment lines are used in the generation of the opcode.html documentation
** file.
**
** SUMMARY:
**
**     Formatting is important to scripts that scan this file.
**     Do not deviate from the formatting style currently in use.
**
*****************************************************************************/

/* Opcode:  Goto * P2 * * *
**
** An unconditional jump to address P2.
** The next instruction executed will be
** the one at index P2 from the beginning of
** the program.
**
** The P1 parameter is not actually used by this opcode.  However, it
** is sometimes set to 1 instead of 0 as a hint to the command-line shell
** that this Goto is the bottom of a loop and that the lines from P2 down
** to the current line should be indented for EXPLAIN output.
*/
case OP_Goto: {             /* jump */

#ifdef SQLITE_DEBUG
  /* In debugging mode, when the p5 flags is set on an OP_Goto, that
  ** means we should really jump back to the preceding OP_ReleaseReg
  ** instruction. */
  if( pOp->p5 ){
    assert( pOp->p2 < (int)(pOp - aOp) );
    assert( pOp->p2 > 1 );
    pOp = &aOp[pOp->p2 - 2];
    assert( pOp[1].opcode==OP_ReleaseReg );
    goto check_for_interrupt;
  }
#endif

jump_to_p2_and_check_for_interrupt:
  pOp = &aOp[pOp->p2 - 1];

  /* Opcodes that are used as the bottom of a loop (OP_Next, OP_Prev,
  ** OP_VNext, or OP_SorterNext) all jump here upon
  ** completion.  Check to see if sqlite3_interrupt() has been called
  ** or if the progress callback needs to be invoked.
  **
  ** This code uses unstructured "goto" statements and does not look clean.
  ** But that is not due to sloppy coding habits. The code is written this
  ** way for performance, to avoid having to run the interrupt and progress
  ** checks on every opcode.  This helps sqlite3_step() to run about 1.5%
  ** faster according to "valgrind --tool=cachegrind" */
check_for_interrupt:
  if( AtomicLoad(&db->u1.isInterrupted) ) goto abort_due_to_interrupt;
#ifndef SQLITE_OMIT_PROGRESS_CALLBACK
  /* Call the progress callback if it is configured and the required number
  ** of VDBE ops have been executed (either since this invocation of
  ** sqlite3VdbeExec() or since last time the progress callback was called).
  ** If the progress callback returns non-zero, exit the virtual machine with
  ** a return code SQLITE_ABORT.
  */
  while( nVmStep>=nProgressLimit && db->xProgress!=0 ){
    assert( db->nProgressOps!=0 );
    nProgressLimit += db->nProgressOps;
    if( db->xProgress(db->pProgressArg) ){
      nProgressLimit = LARGEST_UINT64;
      rc = SQLITE_INTERRUPT;
      goto abort_due_to_error;
    }
  }
#endif

  break;
}

/* Opcode:  Gosub P1 P2 * * *
**
** Write the current address onto register P1
** and then jump to address P2.
*/
case OP_Gosub: {            /* jump */
  assert( pOp->p1>0 && pOp->p1<=(p->nMem+1 - p->nCursor) );
  pIn1 = &aMem[pOp->p1];
  assert( VdbeMemDynamic(pIn1)==0 );
  memAboutToChange(p, pIn1);
  pIn1->flags = MEM_Int;
  pIn1->u.i = (int)(pOp-aOp);
  REGISTER_TRACE(pOp->p1, pIn1);
  goto jump_to_p2_and_check_for_interrupt;
}

/* Opcode:  Return P1 P2 P3 * *
**
** Jump to the address stored in register P1.  If P1 is a return address
** register, then this accomplishes a return from a subroutine.
**
** If P3 is 1, then the jump is only taken if register P1 holds an integer
** values, otherwise execution falls through to the next opcode, and the
** OP_Return becomes a no-op. If P3 is 0, then register P1 must hold an
** integer or else an assert() is raised.  P3 should be set to 1 when
** this opcode is used in combination with OP_BeginSubrtn, and set to 0
** otherwise.
**
** The value in register P1 is unchanged by this opcode.
**
** P2 is not used by the byte-code engine.  However, if P2 is positive
** and also less than the current address, then the "EXPLAIN" output
** formatter in the CLI will indent all opcodes from the P2 opcode up
** to be not including the current Return.   P2 should be the first opcode
** in the subroutine from which this opcode is returning.  Thus the P2
** value is a byte-code indentation hint.  See tag-20220407a in
** wherecode.c and shell.c.
*/
case OP_Return: {           /* in1 */
  pIn1 = &aMem[pOp->p1];
  if( pIn1->flags & MEM_Int ){
    if( pOp->p3 ){ VdbeBranchTaken(1, 2); }
    pOp = &aOp[pIn1->u.i];
  }else if( ALWAYS(pOp->p3) ){
    VdbeBranchTaken(0, 2);
  }
  break;
}

/* Opcode: InitCoroutine P1 P2 P3 * *
**
** Set up register P1 so that it will Yield to the coroutine
** located at address P3.
**
** If P2!=0 then the coroutine implementation immediately follows
** this opcode.  So jump over the coroutine implementation to
** address P2.
**
** See also: EndCoroutine
*/
case OP_InitCoroutine: {     /* jump0 */
  assert( pOp->p1>0 &&  pOp->p1<=(p->nMem+1 - p->nCursor) );
  assert( pOp->p2>=0 && pOp->p2<p->nOp );
  assert( pOp->p3>=0 && pOp->p3<p->nOp );
  pOut = &aMem[pOp->p1];
  assert( !VdbeMemDynamic(pOut) );
  pOut->u.i = pOp->p3 - 1;
  pOut->flags = MEM_Int;
  if( pOp->p2==0 ) break;

  /* Most jump operations do a goto to this spot in order to update
  ** the pOp pointer. */
jump_to_p2:
  assert( pOp->p2>0 );       /* There are never any jumps to instruction 0 */
  assert( pOp->p2<p->nOp );  /* Jumps must be in range */
  pOp = &aOp[pOp->p2 - 1];
  break;
}
