static void percent_rankValueFunc(sqlite3_context *pCtx){
  struct CallCount *p;
  p = (struct CallCount*)sqlite3_aggregate_context(pCtx, sizeof(*p));
  if( p ){
    p->nValue = p->nStep;
    if( p->nTotal>1 ){
      double r = (double)p->nValue / (double)(p->nTotal-1);
      sqlite3_result_double(pCtx, r);
    }else{
      sqlite3_result_double(pCtx, 0.0);
    }
  }
}
#define percent_rankFinalizeFunc percent_rankValueFunc

/*
** Implementation of built-in window function cume_dist(). Assumes that
** the window frame has been set to:
**
**   GROUPS BETWEEN 1 FOLLOWING AND UNBOUNDED FOLLOWING
*/
static void cume_distStepFunc(
  sqlite3_context *pCtx,
  int nArg,
  sqlite3_value **apArg
){
  struct CallCount *p;
  UNUSED_PARAMETER(nArg); assert( nArg==0 );
  UNUSED_PARAMETER(apArg);
  p = (struct CallCount*)sqlite3_aggregate_context(pCtx, sizeof(*p));
  if( p ){
    p->nTotal++;
  }
}
static void cume_distInvFunc(
  sqlite3_context *pCtx,
  int nArg,
  sqlite3_value **apArg
){
  struct CallCount *p;
  UNUSED_PARAMETER(nArg); assert( nArg==0 );
  UNUSED_PARAMETER(apArg);
  p = (struct CallCount*)sqlite3_aggregate_context(pCtx, sizeof(*p));
  p->nStep++;
}
static void cume_distValueFunc(sqlite3_context *pCtx){
  struct CallCount *p;
  p = (struct CallCount*)sqlite3_aggregate_context(pCtx, 0);
  if( p ){
    double r = (double)(p->nStep) / (double)(p->nTotal);
    sqlite3_result_double(pCtx, r);
  }
}
#define cume_distFinalizeFunc cume_distValueFunc

/*
** Context object for ntile() window function.
*/
struct NtileCtx {
  i64 nTotal;                     /* Total rows in partition */
  i64 nParam;                     /* Parameter passed to ntile(N) */
  i64 iRow;                       /* Current row */
};

/*
** Implementation of ntile(). This assumes that the window frame has
** been coerced to:
**
**   ROWS CURRENT ROW AND UNBOUNDED FOLLOWING
*/
static void ntileStepFunc(
  sqlite3_context *pCtx,
  int nArg,
  sqlite3_value **apArg
){
  struct NtileCtx *p;
  assert( nArg==1 ); UNUSED_PARAMETER(nArg);
  p = (struct NtileCtx*)sqlite3_aggregate_context(pCtx, sizeof(*p));
  if( p ){
    if( p->nTotal==0 ){
      p->nParam = sqlite3_value_int64(apArg[0]);
      if( p->nParam<=0 ){
        sqlite3_result_error(
            pCtx, "argument of ntile must be a positive integer", -1
        );
      }
    }
    p->nTotal++;
  }
}
static void ntileInvFunc(
  sqlite3_context *pCtx,
  int nArg,
  sqlite3_value **apArg
){
  struct NtileCtx *p;
  assert( nArg==1 ); UNUSED_PARAMETER(nArg);
  UNUSED_PARAMETER(apArg);
  p = (struct NtileCtx*)sqlite3_aggregate_context(pCtx, sizeof(*p));
  p->iRow++;
}
static void ntileValueFunc(sqlite3_context *pCtx){
  struct NtileCtx *p;
  p = (struct NtileCtx*)sqlite3_aggregate_context(pCtx, sizeof(*p));
  if( p && p->nParam>0 ){
    int nSize = (p->nTotal / p->nParam);
    if( nSize==0 ){
      sqlite3_result_int64(pCtx, p->iRow+1);
    }else{
      i64 nLarge = p->nTotal - p->nParam*nSize;
      i64 iSmall = nLarge*(nSize+1);
      i64 iRow = p->iRow;

      assert( (nLarge*(nSize+1) + (p->nParam-nLarge)*nSize)==p->nTotal );

      if( iRow<iSmall ){
        sqlite3_result_int64(pCtx, 1 + iRow/(nSize+1));
      }else{
        sqlite3_result_int64(pCtx, 1 + nLarge + (iRow-iSmall)/nSize);
      }
    }
  }
}
#define ntileFinalizeFunc ntileValueFunc

/*
** Context object for last_value() window function.
*/
struct LastValueCtx {
  sqlite3_value *pVal;
  int nVal;
};

/*
** Implementation of last_value().
*/
static void last_valueStepFunc(
  sqlite3_context *pCtx,
  int nArg,
  sqlite3_value **apArg
){
  struct LastValueCtx *p;
  UNUSED_PARAMETER(nArg);
  p = (struct LastValueCtx*)sqlite3_aggregate_context(pCtx, sizeof(*p));
  if( p ){
    sqlite3_value_free(p->pVal);
    p->pVal = sqlite3_value_dup(apArg[0]);
    if( p->pVal==0 ){
      sqlite3_result_error_nomem(pCtx);
    }else{
      p->nVal++;
    }
  }
}
static void last_valueInvFunc(
  sqlite3_context *pCtx,
  int nArg,
  sqlite3_value **apArg
){
  struct LastValueCtx *p;
  UNUSED_PARAMETER(nArg);
  UNUSED_PARAMETER(apArg);
  p = (struct LastValueCtx*)sqlite3_aggregate_context(pCtx, sizeof(*p));
  if( ALWAYS(p) ){
    p->nVal--;
    if( p->nVal==0 ){
      sqlite3_value_free(p->pVal);
      p->pVal = 0;
    }
  }
}
static void last_valueValueFunc(sqlite3_context *pCtx){
  struct LastValueCtx *p;
  p = (struct LastValueCtx*)sqlite3_aggregate_context(pCtx, 0);
  if( p && p->pVal ){
    sqlite3_result_value(pCtx, p->pVal);
  }
}
static void last_valueFinalizeFunc(sqlite3_context *pCtx){
  struct LastValueCtx *p;
  p = (struct LastValueCtx*)sqlite3_aggregate_context(pCtx, sizeof(*p));
  if( p && p->pVal ){
    sqlite3_result_value(pCtx, p->pVal);
    sqlite3_value_free(p->pVal);
    p->pVal = 0;
  }
}

/*
** Static names for the built-in window function names.  These static
** names are used, rather than string literals, so that FuncDef objects
** can be associated with a particular window function by direct
** comparison of the zName pointer.  Example:
**
**       if( pFuncDef->zName==row_valueName ){ ... }
*/
static const char row_numberName[] =   "row_number";
static const char dense_rankName[] =   "dense_rank";
static const char rankName[] =         "rank";
static const char percent_rankName[] = "percent_rank";
static const char cume_distName[] =    "cume_dist";
static const char ntileName[] =        "ntile";
static const char last_valueName[] =   "last_value";
static const char nth_valueName[] =    "nth_value";
static const char first_valueName[] =  "first_value";
static const char leadName[] =         "lead";
static const char lagName[] =          "lag";

/*
** No-op implementations of xStep() and xFinalize().  Used as place-holders
** for built-in window functions that never call those interfaces.
**
** The noopValueFunc() is called but is expected to do nothing.  The
** noopStepFunc() is never called, and so it is marked with NO_TEST to
** let the test coverage routine know not to expect this function to be
** invoked.
*/
static void noopStepFunc(    /*NO_TEST*/
  sqlite3_context *p,        /*NO_TEST*/
  int n,                     /*NO_TEST*/
  sqlite3_value **a          /*NO_TEST*/
){                           /*NO_TEST*/
  UNUSED_PARAMETER(p);       /*NO_TEST*/
  UNUSED_PARAMETER(n);       /*NO_TEST*/
  UNUSED_PARAMETER(a);       /*NO_TEST*/
  assert(0);                 /*NO_TEST*/
}                            /*NO_TEST*/
static void noopValueFunc(sqlite3_context *p){ UNUSED_PARAMETER(p); /*no-op*/ }

/* Window functions that use all window interfaces: xStep, xFinal,
** xValue, and xInverse */
#define WINDOWFUNCALL(name,nArg,extra) {                                   \
  nArg, (SQLITE_FUNC_BUILTIN|SQLITE_UTF8|SQLITE_FUNC_WINDOW|extra), 0, 0,  \
  name ## StepFunc, name ## FinalizeFunc, name ## ValueFunc,               \
  name ## InvFunc, name ## Name, {0}                                       \
}

/* Window functions that are implemented using bytecode and thus have
** no-op routines for their methods */
#define WINDOWFUNCNOOP(name,nArg,extra) {                                  \
  nArg, (SQLITE_FUNC_BUILTIN|SQLITE_UTF8|SQLITE_FUNC_WINDOW|extra), 0, 0,  \
  noopStepFunc, noopValueFunc, noopValueFunc,                              \
  noopStepFunc, name ## Name, {0}                                          \
}

/* Window functions that use all window interfaces: xStep, the
** same routine for xFinalize and xValue and which never call
** xInverse. */
#define WINDOWFUNCX(name,nArg,extra) {                                     \
  nArg, (SQLITE_FUNC_BUILTIN|SQLITE_UTF8|SQLITE_FUNC_WINDOW|extra), 0, 0,  \
  name ## StepFunc, name ## ValueFunc, name ## ValueFunc,                  \
  noopStepFunc, name ## Name, {0}                                          \
}


/*
** Register those built-in window functions that are not also aggregates.
*/
SQLITE_PRIVATE void sqlite3WindowFunctions(void){
  static FuncDef aWindowFuncs[] = {
    WINDOWFUNCX(row_number, 0, 0),
    WINDOWFUNCX(dense_rank, 0, 0),
    WINDOWFUNCX(rank, 0, 0),
    WINDOWFUNCALL(percent_rank, 0, 0),
    WINDOWFUNCALL(cume_dist, 0, 0),
    WINDOWFUNCALL(ntile, 1, 0),
    WINDOWFUNCALL(last_value, 1, 0),
    WINDOWFUNCALL(nth_value, 2, 0),
    WINDOWFUNCALL(first_value, 1, 0),
    WINDOWFUNCNOOP(lead, 1, 0),
    WINDOWFUNCNOOP(lead, 2, 0),
    WINDOWFUNCNOOP(lead, 3, 0),
    WINDOWFUNCNOOP(lag, 1, 0),
    WINDOWFUNCNOOP(lag, 2, 0),
    WINDOWFUNCNOOP(lag, 3, 0),
  };
  sqlite3InsertBuiltinFuncs(aWindowFuncs, ArraySize(aWindowFuncs));
}

static Window *windowFind(Parse *pParse, Window *pList, const char *zName){
  Window *p;
  for(p=pList; p; p=p->pNextWin){
    if( sqlite3StrICmp(p->zName, zName)==0 ) break;
  }
  if( p==0 ){
    sqlite3ErrorMsg(pParse, "no such window: %s", zName);
  }
  return p;
}

/*
** This function is called immediately after resolving the function name
** for a window function within a SELECT statement. Argument pList is a
** linked list of WINDOW definitions for the current SELECT statement.
** Argument pFunc is the function definition just resolved and pWin
** is the Window object representing the associated OVER clause. This
** function updates the contents of pWin as follows:
**
**   * If the OVER clause referred to a named window (as in "max(x) OVER win"),
**     search list pList for a matching WINDOW definition, and update pWin
**     accordingly. If no such WINDOW clause can be found, leave an error
**     in pParse.
**
**   * If the function is a built-in window function that requires the
**     window to be coerced (see "BUILT-IN WINDOW FUNCTIONS" at the top
**     of this file), pWin is updated here.
*/
SQLITE_PRIVATE void sqlite3WindowUpdate(
  Parse *pParse,
  Window *pList,                  /* List of named windows for this SELECT */
  Window *pWin,                   /* Window frame to update */
  FuncDef *pFunc                  /* Window function definition */
){
  if( pWin->zName && pWin->eFrmType==0 ){
    Window *p = windowFind(pParse, pList, pWin->zName);
    if( p==0 ) return;
    pWin->pPartition = sqlite3ExprListDup(pParse->db, p->pPartition, 0);
    pWin->pOrderBy = sqlite3ExprListDup(pParse->db, p->pOrderBy, 0);
    pWin->pStart = sqlite3ExprDup(pParse->db, p->pStart, 0);
    pWin->pEnd = sqlite3ExprDup(pParse->db, p->pEnd, 0);
    pWin->eStart = p->eStart;
    pWin->eEnd = p->eEnd;
    pWin->eFrmType = p->eFrmType;
    pWin->eExclude = p->eExclude;
  }else{
    sqlite3WindowChain(pParse, pWin, pList);
  }
  if( (pWin->eFrmType==TK_RANGE)
   && (pWin->pStart || pWin->pEnd)
   && (pWin->pOrderBy==0 || pWin->pOrderBy->nExpr!=1)
  ){
    sqlite3ErrorMsg(pParse,
      "RANGE with offset PRECEDING/FOLLOWING requires one ORDER BY expression"
    );
  }else
  if( pFunc->funcFlags & SQLITE_FUNC_WINDOW ){
    sqlite3 *db = pParse->db;
    if( pWin->pFilter ){
      sqlite3ErrorMsg(pParse,
          "FILTER clause may only be used with aggregate window functions"
      );
    }else{
      struct WindowUpdate {
        const char *zFunc;
        int eFrmType;
        int eStart;
        int eEnd;
      } aUp[] = {
        { row_numberName,   TK_ROWS,   TK_UNBOUNDED, TK_CURRENT },
        { dense_rankName,   TK_RANGE,  TK_UNBOUNDED, TK_CURRENT },
        { rankName,         TK_RANGE,  TK_UNBOUNDED, TK_CURRENT },
        { percent_rankName, TK_GROUPS, TK_CURRENT,   TK_UNBOUNDED },
        { cume_distName,    TK_GROUPS, TK_FOLLOWING, TK_UNBOUNDED },
        { ntileName,        TK_ROWS,   TK_CURRENT,   TK_UNBOUNDED },
        { leadName,         TK_ROWS,   TK_UNBOUNDED, TK_UNBOUNDED },
        { lagName,          TK_ROWS,   TK_UNBOUNDED, TK_CURRENT },
      };
      int i;
      for(i=0; i<ArraySize(aUp); i++){
        if( pFunc->zName==aUp[i].zFunc ){
          sqlite3ExprDelete(db, pWin->pStart);
          sqlite3ExprDelete(db, pWin->pEnd);
          pWin->pEnd = pWin->pStart = 0;
          pWin->eFrmType = aUp[i].eFrmType;
          pWin->eStart = aUp[i].eStart;
          pWin->eEnd = aUp[i].eEnd;
          pWin->eExclude = 0;
          if( pWin->eStart==TK_FOLLOWING ){
            pWin->pStart = sqlite3Expr(db, TK_INTEGER, "1");
          }
          break;
        }
      }
    }
  }
  pWin->pWFunc = pFunc;
}
