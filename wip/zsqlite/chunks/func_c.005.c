static void countFinalize(sqlite3_context *context){
  CountCtx *p;
  p = sqlite3_aggregate_context(context, 0);
  sqlite3_result_int64(context, p ? p->n : 0);
}
#ifndef SQLITE_OMIT_WINDOWFUNC
static void countInverse(sqlite3_context *ctx, int argc, sqlite3_value **argv){
  CountCtx *p;
  p = sqlite3_aggregate_context(ctx, sizeof(*p));
  /* p is always non-NULL since countStep() will have been called first */
  if( (argc==0 || SQLITE_NULL!=sqlite3_value_type(argv[0])) && ALWAYS(p) ){
    p->n--;
#ifdef SQLITE_DEBUG
    p->bInverse = 1;
#endif
  }
}
#else
# define countInverse 0
#endif /* SQLITE_OMIT_WINDOWFUNC */

/*
** Routines to implement min() and max() aggregate functions.
*/
static void minmaxStep(
  sqlite3_context *context,
  int NotUsed,
  sqlite3_value **argv
){
  Mem *pArg  = (Mem *)argv[0];
  Mem *pBest;
  UNUSED_PARAMETER(NotUsed);

  pBest = (Mem *)sqlite3_aggregate_context(context, sizeof(*pBest));
  if( !pBest ) return;

  if( sqlite3_value_type(pArg)==SQLITE_NULL ){
    if( pBest->flags ) sqlite3SkipAccumulatorLoad(context);
  }else if( pBest->flags ){
    int max;
    int cmp;
    CollSeq *pColl = sqlite3GetFuncCollSeq(context);
    /* This step function is used for both the min() and max() aggregates,
    ** the only difference between the two being that the sense of the
    ** comparison is inverted. For the max() aggregate, the
    ** sqlite3_user_data() function returns (void *)-1. For min() it
    ** returns (void *)db, where db is the sqlite3* database pointer.
    ** Therefore the next statement sets variable 'max' to 1 for the max()
    ** aggregate, or 0 for min().
    */
    max = sqlite3_user_data(context)!=0;
    cmp = sqlite3MemCompare(pBest, pArg, pColl);
    if( (max && cmp<0) || (!max && cmp>0) ){
      sqlite3VdbeMemCopy(pBest, pArg);
    }else{
      sqlite3SkipAccumulatorLoad(context);
    }
  }else{
    pBest->db = sqlite3_context_db_handle(context);
    sqlite3VdbeMemCopy(pBest, pArg);
  }
}
static void minMaxValueFinalize(sqlite3_context *context, int bValue){
  sqlite3_value *pRes;
  pRes = (sqlite3_value *)sqlite3_aggregate_context(context, 0);
  if( pRes ){
    if( pRes->flags ){
      sqlite3_result_value(context, pRes);
    }
    if( bValue==0 ) sqlite3VdbeMemRelease(pRes);
  }
}
#ifndef SQLITE_OMIT_WINDOWFUNC
static void minMaxValue(sqlite3_context *context){
  minMaxValueFinalize(context, 1);
}
#else
# define minMaxValue 0
#endif /* SQLITE_OMIT_WINDOWFUNC */
static void minMaxFinalize(sqlite3_context *context){
  minMaxValueFinalize(context, 0);
}

/*
** group_concat(EXPR, ?SEPARATOR?)
** string_agg(EXPR, SEPARATOR)
**
** The SEPARATOR goes before the EXPR string.  This is tragic.  The
** groupConcatInverse() implementation would have been easier if the
** SEPARATOR were appended after EXPR.  And the order is undocumented,
** so we could change it, in theory.  But the old behavior has been
** around for so long that we dare not, for fear of breaking something.
*/
typedef struct {
  StrAccum str;          /* The accumulated concatenation */
#ifndef SQLITE_OMIT_WINDOWFUNC
  int nAccum;            /* Number of strings presently concatenated */
  int nFirstSepLength;   /* Used to detect separator length change */
  /* If pnSepLengths!=0, refs an array of inter-string separator lengths,
  ** stored as actually incorporated into presently accumulated result.
  ** (Hence, its slots in use number nAccum-1 between method calls.)
  ** If pnSepLengths==0, nFirstSepLength is the length used throughout.
  */
  int *pnSepLengths;
#endif
} GroupConcatCtx;

static void groupConcatStep(
  sqlite3_context *context,
  int argc,
  sqlite3_value **argv
){
  const char *zVal;
  GroupConcatCtx *pGCC;
  const char *zSep;
  int nVal, nSep;
  assert( argc==1 || argc==2 );
  if( sqlite3_value_type(argv[0])==SQLITE_NULL ) return;
  pGCC = (GroupConcatCtx*)sqlite3_aggregate_context(context, sizeof(*pGCC));
  if( pGCC ){
    sqlite3 *db = sqlite3_context_db_handle(context);
    int firstTerm = pGCC->str.mxAlloc==0;
    pGCC->str.mxAlloc = db->aLimit[SQLITE_LIMIT_LENGTH];
    if( argc==1 ){
      if( !firstTerm ){
        sqlite3_str_appendchar(&pGCC->str, 1, ',');
      }
#ifndef SQLITE_OMIT_WINDOWFUNC
      else{
        pGCC->nFirstSepLength = 1;
      }
#endif
    }else if( !firstTerm ){
      zSep = (char*)sqlite3_value_text(argv[1]);
      nSep = sqlite3_value_bytes(argv[1]);
      if( zSep ){
        sqlite3_str_append(&pGCC->str, zSep, nSep);
      }
#ifndef SQLITE_OMIT_WINDOWFUNC
      else{
        nSep = 0;
      }
      if( nSep != pGCC->nFirstSepLength || pGCC->pnSepLengths != 0 ){
        int *pnsl = pGCC->pnSepLengths;
        if( pnsl == 0 ){
          /* First separator length variation seen, start tracking them. */
          pnsl = (int*)sqlite3_malloc64((pGCC->nAccum+1) * sizeof(int));
          if( pnsl!=0 ){
            int i = 0, nA = pGCC->nAccum-1;
            while( i<nA ) pnsl[i++] = pGCC->nFirstSepLength;
          }
        }else{
          pnsl = (int*)sqlite3_realloc64(pnsl, pGCC->nAccum * sizeof(int));
        }
        if( pnsl!=0 ){
          if( ALWAYS(pGCC->nAccum>0) ){
            pnsl[pGCC->nAccum-1] = nSep;
          }
          pGCC->pnSepLengths = pnsl;
        }else{
          sqlite3StrAccumSetError(&pGCC->str, SQLITE_NOMEM);
        }
      }
#endif
    }
#ifndef SQLITE_OMIT_WINDOWFUNC
    else{
      pGCC->nFirstSepLength = sqlite3_value_bytes(argv[1]);
    }
    pGCC->nAccum += 1;
#endif
    zVal = (char*)sqlite3_value_text(argv[0]);
    nVal = sqlite3_value_bytes(argv[0]);
    if( zVal ) sqlite3_str_append(&pGCC->str, zVal, nVal);
  }
}

#ifndef SQLITE_OMIT_WINDOWFUNC
static void groupConcatInverse(
  sqlite3_context *context,
  int argc,
  sqlite3_value **argv
){
  GroupConcatCtx *pGCC;
  assert( argc==1 || argc==2 );
  (void)argc;  /* Suppress unused parameter warning */
  if( sqlite3_value_type(argv[0])==SQLITE_NULL ) return;
  pGCC = (GroupConcatCtx*)sqlite3_aggregate_context(context, sizeof(*pGCC));
  /* pGCC is always non-NULL since groupConcatStep() will have always
  ** run first to initialize it */
  if( ALWAYS(pGCC) ){
    int nVS;
    /* Must call sqlite3_value_text() to convert the argument into text prior
    ** to invoking sqlite3_value_bytes(), in case the text encoding is UTF16 */
    (void)sqlite3_value_text(argv[0]);
    nVS = sqlite3_value_bytes(argv[0]);
    pGCC->nAccum -= 1;
    if( pGCC->pnSepLengths!=0 ){
      assert(pGCC->nAccum >= 0);
      if( pGCC->nAccum>0 ){
        nVS += *pGCC->pnSepLengths;
        memmove(pGCC->pnSepLengths, pGCC->pnSepLengths+1,
               (pGCC->nAccum-1)*sizeof(int));
      }
    }else{
      /* If removing single accumulated string, harmlessly over-do. */
      nVS += pGCC->nFirstSepLength;
    }
    if( nVS>=(int)pGCC->str.nChar ){
      pGCC->str.nChar = 0;
    }else{
      pGCC->str.nChar -= nVS;
      memmove(pGCC->str.zText, &pGCC->str.zText[nVS], pGCC->str.nChar);
    }
    if( pGCC->str.nChar==0 ){
      pGCC->str.mxAlloc = 0;
      sqlite3_free(pGCC->pnSepLengths);
      pGCC->pnSepLengths = 0;
    }
  }
}
#else
# define groupConcatInverse 0
#endif /* SQLITE_OMIT_WINDOWFUNC */
static void groupConcatFinalize(sqlite3_context *context){
  GroupConcatCtx *pGCC
    = (GroupConcatCtx*)sqlite3_aggregate_context(context, 0);
  if( pGCC ){
    sqlite3ResultStrAccum(context, &pGCC->str);
#ifndef SQLITE_OMIT_WINDOWFUNC
    sqlite3_free(pGCC->pnSepLengths);
#endif
  }
}
#ifndef SQLITE_OMIT_WINDOWFUNC
static void groupConcatValue(sqlite3_context *context){
  GroupConcatCtx *pGCC
    = (GroupConcatCtx*)sqlite3_aggregate_context(context, 0);
  if( pGCC ){
    StrAccum *pAccum = &pGCC->str;
    if( pAccum->accError==SQLITE_TOOBIG ){
      sqlite3_result_error_toobig(context);
    }else if( pAccum->accError==SQLITE_NOMEM ){
      sqlite3_result_error_nomem(context);
    }else if( pGCC->nAccum>0 && pAccum->nChar==0 ){
      sqlite3_result_text(context, "", 1, SQLITE_STATIC);
    }else{
      const char *zText = sqlite3_str_value(pAccum);
      sqlite3_result_text(context, zText, pAccum->nChar, SQLITE_TRANSIENT);
    }
  }
}
#else
# define groupConcatValue 0
#endif /* SQLITE_OMIT_WINDOWFUNC */

/*
** This routine does per-connection function registration.  Most
** of the built-in functions above are part of the global function set.
** This routine only deals with those that are not global.
*/
SQLITE_PRIVATE void sqlite3RegisterPerConnectionBuiltinFunctions(sqlite3 *db){
  int rc = sqlite3_overload_function(db, "MATCH", 2);
  assert( rc==SQLITE_NOMEM || rc==SQLITE_OK );
  if( rc==SQLITE_NOMEM ){
    sqlite3OomFault(db);
  }
}

/*
** Re-register the built-in LIKE functions.  The caseSensitive
** parameter determines whether or not the LIKE operator is case
** sensitive.
*/
SQLITE_PRIVATE void sqlite3RegisterLikeFunctions(sqlite3 *db, int caseSensitive){
  FuncDef *pDef;
  struct compareInfo *pInfo;
  int flags;
  int nArg;
  if( caseSensitive ){
    pInfo = (struct compareInfo*)&likeInfoAlt;
    flags = SQLITE_FUNC_LIKE | SQLITE_FUNC_CASE;
  }else{
    pInfo = (struct compareInfo*)&likeInfoNorm;
    flags = SQLITE_FUNC_LIKE;
  }
  for(nArg=2; nArg<=3; nArg++){
    sqlite3CreateFunc(db, "like", nArg, SQLITE_UTF8, pInfo, likeFunc,
                      0, 0, 0, 0, 0);
    pDef = sqlite3FindFunction(db, "like", nArg, SQLITE_UTF8, 0);
    pDef->funcFlags |= flags;
    pDef->funcFlags &= ~SQLITE_FUNC_UNSAFE;
  }
}

/*
** pExpr points to an expression which implements a function.  If
** it is appropriate to apply the LIKE optimization to that function
** then set aWc[0] through aWc[2] to the wildcard characters and the
** escape character and then return TRUE.  If the function is not a
** LIKE-style function then return FALSE.
**
** The expression "a LIKE b ESCAPE c" is only considered a valid LIKE
** operator if c is a string literal that is exactly one byte in length.
** That one byte is stored in aWc[3].  aWc[3] is set to zero if there is
** no ESCAPE clause.
**
** *pIsNocase is set to true if uppercase and lowercase are equivalent for
** the function (default for LIKE).  If the function makes the distinction
** between uppercase and lowercase (as does GLOB) then *pIsNocase is set to
** false.
*/
SQLITE_PRIVATE int sqlite3IsLikeFunction(sqlite3 *db, Expr *pExpr, int *pIsNocase, char *aWc){
  FuncDef *pDef;
  int nExpr;
  assert( pExpr!=0 );
  assert( pExpr->op==TK_FUNCTION );
  assert( ExprUseXList(pExpr) );
  if( !pExpr->x.pList ){
    return 0;
  }
  nExpr = pExpr->x.pList->nExpr;
  assert( !ExprHasProperty(pExpr, EP_IntValue) );
  pDef = sqlite3FindFunction(db, pExpr->u.zToken, nExpr, SQLITE_UTF8, 0);
#ifdef SQLITE_ENABLE_UNKNOWN_SQL_FUNCTION
  if( pDef==0 ) return 0;
#endif
  if( NEVER(pDef==0) || (pDef->funcFlags & SQLITE_FUNC_LIKE)==0 ){
    return 0;
  }

  /* The memcpy() statement assumes that the wildcard characters are
  ** the first three statements in the compareInfo structure.  The
  ** asserts() that follow verify that assumption
  */
  memcpy(aWc, pDef->pUserData, 3);
  assert( (char*)&likeInfoAlt == (char*)&likeInfoAlt.matchAll );
  assert( &((char*)&likeInfoAlt)[1] == (char*)&likeInfoAlt.matchOne );
  assert( &((char*)&likeInfoAlt)[2] == (char*)&likeInfoAlt.matchSet );

  if( nExpr<3 ){
    aWc[3] = 0;
  }else{
    Expr *pEscape = pExpr->x.pList->a[2].pExpr;
    char *zEscape;
    if( pEscape->op!=TK_STRING ) return 0;
    assert( !ExprHasProperty(pEscape, EP_IntValue) );
    zEscape = pEscape->u.zToken;
    if( zEscape[0]==0 || zEscape[1]!=0 ) return 0;
    if( zEscape[0]==aWc[0] ) return 0;
    if( zEscape[0]==aWc[1] ) return 0;
    aWc[3] = zEscape[0];
  }

  *pIsNocase = (pDef->funcFlags & SQLITE_FUNC_CASE)==0;
  return 1;
}
