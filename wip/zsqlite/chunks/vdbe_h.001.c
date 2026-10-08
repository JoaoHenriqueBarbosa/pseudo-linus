**                                     // in distinguishing equal and not-equal.
**
** Every VDBE branch operation must be tagged with one of the macros above.
** If not, then when "make test" is run with -DSQLITE_VDBE_COVERAGE and
** -DSQLITE_DEBUG then an ALWAYS() will fail in the vdbeTakeBranch()
** routine in vdbe.c, alerting the developer to the missed tag.
**
** During testing, the test application will invoke
** sqlite3_test_control(SQLITE_TESTCTRL_VDBE_COVERAGE,...) to set a callback
** routine that is invoked as each bytecode branch is taken.  The callback
** contains the sqlite3.c source line number of the VdbeCoverage macro and
** flags to indicate whether or not the branch was taken.  The test application
** is responsible for keeping track of this and reporting byte-code branches
** that are never taken.
**
** See the VdbeBranchTaken() macro and vdbeTakeBranch() function in the
** vdbe.c source file for additional information.
*/
#ifdef SQLITE_VDBE_COVERAGE
SQLITE_PRIVATE   void sqlite3VdbeSetLineNumber(Vdbe*,int);
# define VdbeCoverage(v) sqlite3VdbeSetLineNumber(v,__LINE__)
# define VdbeCoverageIf(v,x) if(x)sqlite3VdbeSetLineNumber(v,__LINE__)
# define VdbeCoverageAlwaysTaken(v) \
         sqlite3VdbeSetLineNumber(v,__LINE__|0x5000000);
# define VdbeCoverageNeverTaken(v) \
         sqlite3VdbeSetLineNumber(v,__LINE__|0x6000000);
# define VdbeCoverageNeverNull(v) \
         sqlite3VdbeSetLineNumber(v,__LINE__|0x4000000);
# define VdbeCoverageNeverNullIf(v,x) \
         if(x)sqlite3VdbeSetLineNumber(v,__LINE__|0x4000000);
# define VdbeCoverageEqNe(v) \
         sqlite3VdbeSetLineNumber(v,__LINE__|0x8000000);
# define VDBE_OFFSET_LINENO(x) (__LINE__+x)
#else
# define VdbeCoverage(v)
# define VdbeCoverageIf(v,x)
# define VdbeCoverageAlwaysTaken(v)
# define VdbeCoverageNeverTaken(v)
# define VdbeCoverageNeverNull(v)
# define VdbeCoverageNeverNullIf(v,x)
# define VdbeCoverageEqNe(v)
# define VDBE_OFFSET_LINENO(x) 0
#endif

#ifdef SQLITE_ENABLE_STMT_SCANSTATUS
SQLITE_PRIVATE void sqlite3VdbeScanStatus(Vdbe*, int, int, int, LogEst, const char*);
SQLITE_PRIVATE void sqlite3VdbeScanStatusRange(Vdbe*, int, int, int);
SQLITE_PRIVATE void sqlite3VdbeScanStatusCounters(Vdbe*, int, int, int);
#else
# define sqlite3VdbeScanStatus(a,b,c,d,e,f)
# define sqlite3VdbeScanStatusRange(a,b,c,d)
# define sqlite3VdbeScanStatusCounters(a,b,c,d)
#endif

#if defined(SQLITE_DEBUG) || defined(VDBE_PROFILE)
SQLITE_PRIVATE void sqlite3VdbePrintOp(FILE*, int, VdbeOp*);
#endif

#if defined(SQLITE_ENABLE_CURSOR_HINTS) && defined(SQLITE_DEBUG)
SQLITE_PRIVATE int sqlite3CursorRangeHintExprCheck(Walker *pWalker, Expr *pExpr);
#endif

#endif /* SQLITE_VDBE_H */

