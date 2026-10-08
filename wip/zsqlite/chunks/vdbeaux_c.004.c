
/*
** If the last opcode is "op" and it is not a jump destination,
** then remove it.  Return true if and only if an opcode was removed.
*/
SQLITE_PRIVATE int sqlite3VdbeDeletePriorOpcode(Vdbe *p, u8 op){
  if( p->nOp>0 && p->aOp[p->nOp-1].opcode==op ){
    return sqlite3VdbeChangeToNoop(p, p->nOp-1);
  }else{
    return 0;
  }
}

#ifdef SQLITE_DEBUG
/*
** Generate an OP_ReleaseReg opcode to indicate that a range of
** registers, except any identified by mask, are no longer in use.
*/
SQLITE_PRIVATE void sqlite3VdbeReleaseRegisters(
  Parse *pParse,       /* Parsing context */
  int iFirst,          /* Index of first register to be released */
  int N,               /* Number of registers to release */
  u32 mask,            /* Mask of registers to NOT release */
  int bUndefine        /* If true, mark registers as undefined */
){
  if( N==0 || OptimizationDisabled(pParse->db, SQLITE_ReleaseReg) ) return;
  assert( pParse->pVdbe );
  assert( iFirst>=1 );
  assert( iFirst+N-1<=pParse->nMem );
  if( N<=31 && mask!=0 ){
    while( N>0 && (mask&1)!=0 ){
      mask >>= 1;
      iFirst++;
      N--;
    }
    while( N>0 && N<=32 && (mask & MASKBIT32(N-1))!=0 ){
      mask &= ~MASKBIT32(N-1);
      N--;
    }
  }
  if( N>0 ){
    sqlite3VdbeAddOp3(pParse->pVdbe, OP_ReleaseReg, iFirst, N, *(int*)&mask);
    if( bUndefine ) sqlite3VdbeChangeP5(pParse->pVdbe, 1);
  }
}
#endif /* SQLITE_DEBUG */

/*
** Change the value of the P4 operand for a specific instruction.
** This routine is useful when a large program is loaded from a
** static array using sqlite3VdbeAddOpList but we want to make a
** few minor changes to the program.
**
** If n>=0 then the P4 operand is dynamic, meaning that a copy of
** the string is made into memory obtained from sqlite3_malloc().
** A value of n==0 means copy bytes of zP4 up to and including the
** first null byte.  If n>0 then copy n+1 bytes of zP4.
**
** Other values of n (P4_STATIC, P4_COLLSEQ etc.) indicate that zP4 points
** to a string or structure that is guaranteed to exist for the lifetime of
** the Vdbe. In these cases we can just copy the pointer.
**
** If addr<0 then change P4 on the most recently inserted instruction.
*/
static void SQLITE_NOINLINE vdbeChangeP4Full(
  Vdbe *p,
  Op *pOp,
  const char *zP4,
  int n
){
  if( pOp->p4type ){
    assert( pOp->p4type > P4_FREE_IF_LE );
    pOp->p4type = 0;
    pOp->p4.p = 0;
  }
  if( n<0 ){
    sqlite3VdbeChangeP4(p, (int)(pOp - p->aOp), zP4, n);
  }else{
    if( n==0 ) n = sqlite3Strlen30(zP4);
    pOp->p4.z = sqlite3DbStrNDup(p->db, zP4, n);
    pOp->p4type = P4_DYNAMIC;
  }
}
SQLITE_PRIVATE void sqlite3VdbeChangeP4(Vdbe *p, int addr, const char *zP4, int n){
  Op *pOp;
  sqlite3 *db;
  assert( p!=0 );
  db = p->db;
  assert( p->eVdbeState==VDBE_INIT_STATE );
  assert( p->aOp!=0 || db->mallocFailed );
  if( db->mallocFailed ){
    if( n!=P4_VTAB ) freeP4(db, n, (void*)*(char**)&zP4);
    return;
  }
  assert( p->nOp>0 );
  assert( addr<p->nOp );
  if( addr<0 ){
    addr = p->nOp - 1;
  }
  pOp = &p->aOp[addr];
  if( n>=0 || pOp->p4type ){
    vdbeChangeP4Full(p, pOp, zP4, n);
    return;
  }
  if( n==P4_INT32 ){
    /* Note: this cast is safe, because the origin data point was an int
    ** that was cast to a (const char *). */
    pOp->p4.i = SQLITE_PTR_TO_INT(zP4);
    pOp->p4type = P4_INT32;
  }else if( zP4!=0 ){
    assert( n<0 );
    pOp->p4.p = (void*)zP4;
    pOp->p4type = (signed char)n;
    if( n==P4_VTAB ) sqlite3VtabLock((VTable*)zP4);
  }
}

/*
** Change the P4 operand of the most recently coded instruction
** to the value defined by the arguments.  This is a high-speed
** version of sqlite3VdbeChangeP4().
**
** The P4 operand must not have been previously defined.  And the new
** P4 must not be P4_INT32.  Use sqlite3VdbeChangeP4() in either of
** those cases.
*/
SQLITE_PRIVATE void sqlite3VdbeAppendP4(Vdbe *p, void *pP4, int n){
  VdbeOp *pOp;
  assert( n!=P4_INT32 && n!=P4_VTAB );
  assert( n<=0 );
  if( p->db->mallocFailed ){
    freeP4(p->db, n, pP4);
  }else{
    assert( pP4!=0 || n==P4_DYNAMIC );
    assert( p->nOp>0 );
    pOp = &p->aOp[p->nOp-1];
    assert( pOp->p4type==P4_NOTUSED );
    pOp->p4type = n;
    pOp->p4.p = pP4;
  }
}

/*
** Set the P4 on the most recently added opcode to the KeyInfo for the
** index given.
*/
SQLITE_PRIVATE void sqlite3VdbeSetP4KeyInfo(Parse *pParse, Index *pIdx){
  Vdbe *v = pParse->pVdbe;
  KeyInfo *pKeyInfo;
  assert( v!=0 );
  assert( pIdx!=0 );
  pKeyInfo = sqlite3KeyInfoOfIndex(pParse, pIdx);
  if( pKeyInfo ) sqlite3VdbeAppendP4(v, pKeyInfo, P4_KEYINFO);
}

#ifdef SQLITE_ENABLE_EXPLAIN_COMMENTS
/*
** Change the comment on the most recently coded instruction.  Or
** insert a No-op and add the comment to that new instruction.  This
** makes the code easier to read during debugging.  None of this happens
** in a production build.
*/
static void vdbeVComment(Vdbe *p, const char *zFormat, va_list ap){
  assert( p->nOp>0 || p->aOp==0 );
  assert( p->aOp==0 || p->aOp[p->nOp-1].zComment==0 || p->pParse->nErr>0 );
  if( p->nOp ){
    assert( p->aOp );
    sqlite3DbFree(p->db, p->aOp[p->nOp-1].zComment);
    p->aOp[p->nOp-1].zComment = sqlite3VMPrintf(p->db, zFormat, ap);
  }
}
SQLITE_PRIVATE void sqlite3VdbeComment(Vdbe *p, const char *zFormat, ...){
  va_list ap;
  if( p ){
    va_start(ap, zFormat);
    vdbeVComment(p, zFormat, ap);
    va_end(ap);
  }
}
SQLITE_PRIVATE void sqlite3VdbeNoopComment(Vdbe *p, const char *zFormat, ...){
  va_list ap;
  if( p ){
    sqlite3VdbeAddOp0(p, OP_Noop);
    va_start(ap, zFormat);
    vdbeVComment(p, zFormat, ap);
    va_end(ap);
  }
}
#endif  /* NDEBUG */

#ifdef SQLITE_VDBE_COVERAGE
/*
** Set the value if the iSrcLine field for the previously coded instruction.
*/
SQLITE_PRIVATE void sqlite3VdbeSetLineNumber(Vdbe *v, int iLine){
  sqlite3VdbeGetLastOp(v)->iSrcLine = iLine;
}
#endif /* SQLITE_VDBE_COVERAGE */

/*
** Return the opcode for a given address.  The address must be non-negative.
** See sqlite3VdbeGetLastOp() to get the most recently added opcode.
**
** If a memory allocation error has occurred prior to the calling of this
** routine, then a pointer to a dummy VdbeOp will be returned.  That opcode
** is readable but not writable, though it is cast to a writable value.
** The return of a dummy opcode allows the call to continue functioning
** after an OOM fault without having to check to see if the return from
** this routine is a valid pointer.  But because the dummy.opcode is 0,
** dummy will never be written to.  This is verified by code inspection and
** by running with Valgrind.
*/
SQLITE_PRIVATE VdbeOp *sqlite3VdbeGetOp(Vdbe *p, int addr){
  /* C89 specifies that the constant "dummy" will be initialized to all
  ** zeros, which is correct.  MSVC generates a warning, nevertheless. */
  static VdbeOp dummy;  /* Ignore the MSVC warning about no initializer */
  assert( p->eVdbeState==VDBE_INIT_STATE );
  assert( (addr>=0 && addr<p->nOp) || p->db->mallocFailed );
  if( p->db->mallocFailed ){
    return (VdbeOp*)&dummy;
  }else{
    return &p->aOp[addr];
  }
}

/* Return the most recently added opcode
*/
SQLITE_PRIVATE VdbeOp *sqlite3VdbeGetLastOp(Vdbe *p){
  return sqlite3VdbeGetOp(p, p->nOp - 1);
}

#if defined(SQLITE_ENABLE_EXPLAIN_COMMENTS)
/*
** Return an integer value for one of the parameters to the opcode pOp
** determined by character c.
*/
static int translateP(char c, const Op *pOp){
  if( c=='1' ) return pOp->p1;
  if( c=='2' ) return pOp->p2;
  if( c=='3' ) return pOp->p3;
  if( c=='4' ) return pOp->p4.i;
  return pOp->p5;
}

/*
** Compute a string for the "comment" field of a VDBE opcode listing.
**
** The Synopsis: field in comments in the vdbe.c source file gets converted
** to an extra string that is appended to the sqlite3OpcodeName().  In the
** absence of other comments, this synopsis becomes the comment on the opcode.
** Some translation occurs:
**
**       "PX"      ->  "r[X]"
**       "PX@PY"   ->  "r[X..X+Y-1]"  or "r[x]" if y is 0 or 1
**       "PX@PY+1" ->  "r[X..X+Y]"    or "r[x]" if y is 0
**       "PY..PY"  ->  "r[X..Y]"      or "r[x]" if y<=x
*/
SQLITE_PRIVATE char *sqlite3VdbeDisplayComment(
  sqlite3 *db,       /* Optional - Oom error reporting only */
  const Op *pOp,     /* The opcode to be commented */
  const char *zP4    /* Previously obtained value for P4 */
){
  const char *zOpName;
  const char *zSynopsis;
  int nOpName;
  int ii;
  char zAlt[50];
  StrAccum x;

  sqlite3StrAccumInit(&x, 0, 0, 0, SQLITE_MAX_LENGTH);
  zOpName = sqlite3OpcodeName(pOp->opcode);
  nOpName = sqlite3Strlen30(zOpName);
  if( zOpName[nOpName+1] ){
    int seenCom = 0;
    char c;
    zSynopsis = zOpName + nOpName + 1;
    if( strncmp(zSynopsis,"IF ",3)==0 ){
      sqlite3_snprintf(sizeof(zAlt), zAlt, "if %s goto P2", zSynopsis+3);
      zSynopsis = zAlt;
    }
    for(ii=0; (c = zSynopsis[ii])!=0; ii++){
      if( c=='P' ){
        c = zSynopsis[++ii];
        if( c=='4' ){
          sqlite3_str_appendall(&x, zP4);
        }else if( c=='X' ){
          if( pOp->zComment && pOp->zComment[0] ){
            sqlite3_str_appendall(&x, pOp->zComment);
            seenCom = 1;
            break;
          }
        }else{
          int v1 = translateP(c, pOp);
          int v2;
          if( strncmp(zSynopsis+ii+1, "@P", 2)==0 ){
            ii += 3;
            v2 = translateP(zSynopsis[ii], pOp);
            if( strncmp(zSynopsis+ii+1,"+1",2)==0 ){
              ii += 2;
              v2++;
            }
            if( v2<2 ){
              sqlite3_str_appendf(&x, "%d", v1);
            }else{
              sqlite3_str_appendf(&x, "%d..%d", v1, v1+v2-1);
            }
          }else if( strncmp(zSynopsis+ii+1, "@NP", 3)==0 ){
            sqlite3_context *pCtx = pOp->p4.pCtx;
            if( pOp->p4type!=P4_FUNCCTX || pCtx->argc==1 ){
              sqlite3_str_appendf(&x, "%d", v1);
            }else if( pCtx->argc>1 ){
              sqlite3_str_appendf(&x, "%d..%d", v1, v1+pCtx->argc-1);
            }else if( x.accError==0 ){
              assert( x.nChar>2 );
              x.nChar -= 2;
              ii++;
            }
            ii += 3;
          }else{
            sqlite3_str_appendf(&x, "%d", v1);
            if( strncmp(zSynopsis+ii+1, "..P3", 4)==0 && pOp->p3==0 ){
              ii += 4;
            }
          }
        }
      }else{
        sqlite3_str_appendchar(&x, 1, c);
      }
    }
    if( !seenCom && pOp->zComment ){
      sqlite3_str_appendf(&x, "; %s", pOp->zComment);
    }
  }else if( pOp->zComment ){
    sqlite3_str_appendall(&x, pOp->zComment);
  }
  if( (x.accError & SQLITE_NOMEM)!=0 && db!=0 ){
    sqlite3OomFault(db);
  }
  return sqlite3StrAccumFinish(&x);
}
#endif /* SQLITE_ENABLE_EXPLAIN_COMMENTS */

#if VDBE_DISPLAY_P4 && defined(SQLITE_ENABLE_CURSOR_HINTS)
/*
** Translate the P4.pExpr value for an OP_CursorHint opcode into text
** that can be displayed in the P4 column of EXPLAIN output.
*/
static void displayP4Expr(StrAccum *p, Expr *pExpr){
  const char *zOp = 0;
  switch( pExpr->op ){
    case TK_STRING:
      assert( !ExprHasProperty(pExpr, EP_IntValue) );
      sqlite3_str_appendf(p, "%Q", pExpr->u.zToken);
      break;
    case TK_INTEGER:
      sqlite3_str_appendf(p, "%d", pExpr->u.iValue);
      break;
    case TK_NULL:
      sqlite3_str_appendf(p, "NULL");
      break;
    case TK_REGISTER: {
      sqlite3_str_appendf(p, "r[%d]", pExpr->iTable);
      break;
    }
    case TK_COLUMN: {
      if( pExpr->iColumn<0 ){
        sqlite3_str_appendf(p, "rowid");
      }else{
        sqlite3_str_appendf(p, "c%d", (int)pExpr->iColumn);
      }
      break;
    }
    case TK_LT:      zOp = "LT";      break;
    case TK_LE:      zOp = "LE";      break;
    case TK_GT:      zOp = "GT";      break;
    case TK_GE:      zOp = "GE";      break;
    case TK_NE:      zOp = "NE";      break;
    case TK_EQ:      zOp = "EQ";      break;
    case TK_IS:      zOp = "IS";      break;
    case TK_ISNOT:   zOp = "ISNOT";   break;
    case TK_AND:     zOp = "AND";     break;
    case TK_OR:      zOp = "OR";      break;
    case TK_PLUS:    zOp = "ADD";     break;
    case TK_STAR:    zOp = "MUL";     break;
    case TK_MINUS:   zOp = "SUB";     break;
    case TK_REM:     zOp = "REM";     break;
    case TK_BITAND:  zOp = "BITAND";  break;
    case TK_BITOR:   zOp = "BITOR";   break;
    case TK_SLASH:   zOp = "DIV";     break;
    case TK_LSHIFT:  zOp = "LSHIFT";  break;
    case TK_RSHIFT:  zOp = "RSHIFT";  break;
    case TK_CONCAT:  zOp = "CONCAT";  break;
    case TK_UMINUS:  zOp = "MINUS";   break;
    case TK_UPLUS:   zOp = "PLUS";    break;
    case TK_BITNOT:  zOp = "BITNOT";  break;
    case TK_NOT:     zOp = "NOT";     break;
    case TK_ISNULL:  zOp = "ISNULL";  break;
    case TK_NOTNULL: zOp = "NOTNULL"; break;

    default:
      sqlite3_str_appendf(p, "%s", "expr");
      break;
  }

  if( zOp ){
    sqlite3_str_appendf(p, "%s(", zOp);
    displayP4Expr(p, pExpr->pLeft);
    if( pExpr->pRight ){
      sqlite3_str_append(p, ",", 1);
      displayP4Expr(p, pExpr->pRight);
    }
    sqlite3_str_append(p, ")", 1);
  }
}
