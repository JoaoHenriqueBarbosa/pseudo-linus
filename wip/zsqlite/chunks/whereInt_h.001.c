
/*
** A WhereTerm with eOperator==WO_OR has its u.pOrInfo pointer set to
** a dynamically allocated instance of the following structure.
*/
struct WhereOrInfo {
  WhereClause wc;          /* Decomposition into subterms */
  Bitmask indexable;       /* Bitmask of all indexable tables in the clause */
};

/*
** A WhereTerm with eOperator==WO_AND has its u.pAndInfo pointer set to
** a dynamically allocated instance of the following structure.
*/
struct WhereAndInfo {
  WhereClause wc;          /* The subexpression broken out */
};

/*
** An instance of the following structure keeps track of a mapping
** between VDBE cursor numbers and bits of the bitmasks in WhereTerm.
**
** The VDBE cursor numbers are small integers contained in
** SrcItem.iCursor and Expr.iTable fields.  For any given WHERE
** clause, the cursor numbers might not begin with 0 and they might
** contain gaps in the numbering sequence.  But we want to make maximum
** use of the bits in our bitmasks.  This structure provides a mapping
** from the sparse cursor numbers into consecutive integers beginning
** with 0.
**
** If WhereMaskSet.ix[A]==B it means that The A-th bit of a Bitmask
** corresponds VDBE cursor number B.  The A-th bit of a bitmask is 1<<A.
**
** For example, if the WHERE clause expression used these VDBE
** cursors:  4, 5, 8, 29, 57, 73.  Then the  WhereMaskSet structure
** would map those cursor numbers into bits 0 through 5.
**
** Note that the mapping is not necessarily ordered.  In the example
** above, the mapping might go like this:  4->3, 5->1, 8->2, 29->0,
** 57->5, 73->4.  Or one of 719 other combinations might be used. It
** does not really matter.  What is important is that sparse cursor
** numbers all get mapped into bit numbers that begin with 0 and contain
** no gaps.
*/
struct WhereMaskSet {
  int bVarSelect;               /* Used by sqlite3WhereExprUsage() */
  int n;                        /* Number of assigned cursor values */
  int ix[BMS];                  /* Cursor assigned to each bit */
};

/*
** This object is a convenience wrapper holding all information needed
** to construct WhereLoop objects for a particular query.
*/
struct WhereLoopBuilder {
  WhereInfo *pWInfo;        /* Information about this WHERE */
  WhereClause *pWC;         /* WHERE clause terms */
  WhereLoop *pNew;          /* Template WhereLoop */
  WhereOrSet *pOrSet;       /* Record best loops here, if not NULL */
#ifdef SQLITE_ENABLE_STAT4
  UnpackedRecord *pRec;     /* Probe for stat4 (if required) */
  int nRecValid;            /* Number of valid fields currently in pRec */
#endif
  unsigned char bldFlags1;  /* First set of SQLITE_BLDF_* flags */
  unsigned char bldFlags2;  /* Second set of SQLITE_BLDF_* flags */
  unsigned int iPlanLimit;  /* Search limiter */
};

/* Allowed values for WhereLoopBuider.bldFlags */
#define SQLITE_BLDF1_INDEXED  0x0001   /* An index is used */
#define SQLITE_BLDF1_UNIQUE   0x0002   /* All keys of a UNIQUE index used */

#define SQLITE_BLDF2_2NDPASS  0x0004   /* Second builder pass needed */

/* The WhereLoopBuilder.iPlanLimit is used to limit the number of
** index+constraint combinations the query planner will consider for a
** particular query.  If this parameter is unlimited, then certain
** pathological queries can spend excess time in the sqlite3WhereBegin()
** routine.  The limit is high enough that is should not impact real-world
** queries.
**
** SQLITE_QUERY_PLANNER_LIMIT is the baseline limit.  The limit is
** increased by SQLITE_QUERY_PLANNER_LIMIT_INCR before each term of the FROM
** clause is processed, so that every table in a join is guaranteed to be
** able to propose a some index+constraint combinations even if the initial
** baseline limit was exhausted by prior tables of the join.
*/
#ifndef SQLITE_QUERY_PLANNER_LIMIT
# define SQLITE_QUERY_PLANNER_LIMIT 20000
#endif
#ifndef SQLITE_QUERY_PLANNER_LIMIT_INCR
# define SQLITE_QUERY_PLANNER_LIMIT_INCR 1000
#endif

/*
** The WHERE clause processing routine has two halves.  The
** first part does the start of the WHERE loop and the second
** half does the tail of the WHERE loop.  An instance of
** this structure is returned by the first half and passed
** into the second half to give some continuity.
**
** An instance of this object holds the complete state of the query
** planner.
*/
struct WhereInfo {
  Parse *pParse;            /* Parsing and code generating context */
  SrcList *pTabList;        /* List of tables in the join */
  ExprList *pOrderBy;       /* The ORDER BY clause or NULL */
  ExprList *pResultSet;     /* Result set of the query */
#if WHERETRACE_ENABLED
  Expr *pWhere;             /* The complete WHERE clause */
#endif
  Select *pSelect;          /* The entire SELECT statement containing WHERE */
  int aiCurOnePass[2];      /* OP_OpenWrite cursors for the ONEPASS opt */
  int iContinue;            /* Jump here to continue with next record */
  int iBreak;               /* Jump here to break out of the loop */
  int savedNQueryLoop;      /* pParse->nQueryLoop outside the WHERE loop */
  u16 wctrlFlags;           /* Flags originally passed to sqlite3WhereBegin() */
  LogEst iLimit;            /* LIMIT if wctrlFlags has WHERE_USE_LIMIT */
  u8 nLevel;                /* Number of nested loop */
  i8 nOBSat;                /* Number of ORDER BY terms satisfied by indices */
  u8 eOnePass;              /* ONEPASS_OFF, or _SINGLE, or _MULTI */
  u8 eDistinct;             /* One of the WHERE_DISTINCT_* values */
  unsigned bDeferredSeek :1;   /* Uses OP_DeferredSeek */
  unsigned untestedTerms :1;   /* Not all WHERE terms resolved by outer loop */
  unsigned bOrderedInnerLoop:1;/* True if only the inner-most loop is ordered */
  unsigned sorted :1;          /* True if really sorted (not just grouped) */
  LogEst nRowOut;           /* Estimated number of output rows */
  int iTop;                 /* The very beginning of the WHERE loop */
  int iEndWhere;            /* End of the WHERE clause itself */
  WhereLoop *pLoops;        /* List of all WhereLoop objects */
  WhereMemBlock *pMemToFree;/* Memory to free when this object destroyed */
  Bitmask revMask;          /* Mask of ORDER BY terms that need reversing */
  WhereClause sWC;          /* Decomposition of the WHERE clause */
  WhereMaskSet sMaskSet;    /* Map cursor numbers to bitmasks */
  WhereLevel a[1];          /* Information about each nest loop in WHERE */
};

/*
** Private interfaces - callable only by other where.c routines.
**
** where.c:
*/
SQLITE_PRIVATE Bitmask sqlite3WhereGetMask(WhereMaskSet*,int);
#ifdef WHERETRACE_ENABLED
SQLITE_PRIVATE void sqlite3WhereClausePrint(WhereClause *pWC);
SQLITE_PRIVATE void sqlite3WhereTermPrint(WhereTerm *pTerm, int iTerm);
SQLITE_PRIVATE void sqlite3WhereLoopPrint(const WhereLoop *p, const WhereClause *pWC);
#endif
SQLITE_PRIVATE WhereTerm *sqlite3WhereFindTerm(
  WhereClause *pWC,     /* The WHERE clause to be searched */
  int iCur,             /* Cursor number of LHS */
  int iColumn,          /* Column number of LHS */
  Bitmask notReady,     /* RHS must not overlap with this mask */
  u32 op,               /* Mask of WO_xx values describing operator */
  Index *pIdx           /* Must be compatible with this index, if not NULL */
);
SQLITE_PRIVATE void *sqlite3WhereMalloc(WhereInfo *pWInfo, u64 nByte);
SQLITE_PRIVATE void *sqlite3WhereRealloc(WhereInfo *pWInfo, void *pOld, u64 nByte);

/* wherecode.c: */
#ifndef SQLITE_OMIT_EXPLAIN
SQLITE_PRIVATE int sqlite3WhereExplainOneScan(
  Parse *pParse,                  /* Parse context */
  SrcList *pTabList,              /* Table list this loop refers to */
  WhereLevel *pLevel,             /* Scan to write OP_Explain opcode for */
  u16 wctrlFlags                  /* Flags passed to sqlite3WhereBegin() */
);
SQLITE_PRIVATE int sqlite3WhereExplainBloomFilter(
  const Parse *pParse,            /* Parse context */
  const WhereInfo *pWInfo,        /* WHERE clause */
  const WhereLevel *pLevel        /* Bloom filter on this level */
);
#else
# define sqlite3WhereExplainOneScan(u,v,w,x) 0
# define sqlite3WhereExplainBloomFilter(u,v,w) 0
#endif /* SQLITE_OMIT_EXPLAIN */
#ifdef SQLITE_ENABLE_STMT_SCANSTATUS
SQLITE_PRIVATE void sqlite3WhereAddScanStatus(
  Vdbe *v,                        /* Vdbe to add scanstatus entry to */
  SrcList *pSrclist,              /* FROM clause pLvl reads data from */
  WhereLevel *pLvl,               /* Level to add scanstatus() entry for */
  int addrExplain                 /* Address of OP_Explain (or 0) */
);
#else
# define sqlite3WhereAddScanStatus(a, b, c, d) ((void)d)
#endif
SQLITE_PRIVATE Bitmask sqlite3WhereCodeOneLoopStart(
  Parse *pParse,       /* Parsing context */
  Vdbe *v,             /* Prepared statement under construction */
  WhereInfo *pWInfo,   /* Complete information about the WHERE clause */
  int iLevel,          /* Which level of pWInfo->a[] should be coded */
  WhereLevel *pLevel,  /* The current level pointer */
  Bitmask notReady     /* Which tables are currently available */
);
SQLITE_PRIVATE SQLITE_NOINLINE void sqlite3WhereRightJoinLoop(
  WhereInfo *pWInfo,
  int iLevel,
  WhereLevel *pLevel
);

/* whereexpr.c: */
SQLITE_PRIVATE void sqlite3WhereClauseInit(WhereClause*,WhereInfo*);
SQLITE_PRIVATE void sqlite3WhereClauseClear(WhereClause*);
SQLITE_PRIVATE void sqlite3WhereSplit(WhereClause*,Expr*,u8);
SQLITE_PRIVATE void sqlite3WhereAddLimit(WhereClause*, Select*);
SQLITE_PRIVATE Bitmask sqlite3WhereExprUsage(WhereMaskSet*, Expr*);
SQLITE_PRIVATE Bitmask sqlite3WhereExprUsageNN(WhereMaskSet*, Expr*);
SQLITE_PRIVATE Bitmask sqlite3WhereExprListUsage(WhereMaskSet*, ExprList*);
SQLITE_PRIVATE void sqlite3WhereExprAnalyze(SrcList*, WhereClause*);
SQLITE_PRIVATE void sqlite3WhereTabFuncArgs(Parse*, SrcItem*, WhereClause*);





/*
** Bitmasks for the operators on WhereTerm objects.  These are all
** operators that are of interest to the query planner.  An
** OR-ed combination of these values can be used when searching for
** particular WhereTerms within a WhereClause.
**
** Value constraints:
**     WO_EQ    == SQLITE_INDEX_CONSTRAINT_EQ
**     WO_LT    == SQLITE_INDEX_CONSTRAINT_LT
**     WO_LE    == SQLITE_INDEX_CONSTRAINT_LE
**     WO_GT    == SQLITE_INDEX_CONSTRAINT_GT
**     WO_GE    == SQLITE_INDEX_CONSTRAINT_GE
*/
#define WO_IN     0x0001
#define WO_EQ     0x0002
#define WO_LT     (WO_EQ<<(TK_LT-TK_EQ))
#define WO_LE     (WO_EQ<<(TK_LE-TK_EQ))
#define WO_GT     (WO_EQ<<(TK_GT-TK_EQ))
#define WO_GE     (WO_EQ<<(TK_GE-TK_EQ))
#define WO_AUX    0x0040       /* Op useful to virtual tables only */
#define WO_IS     0x0080
#define WO_ISNULL 0x0100
#define WO_OR     0x0200       /* Two or more OR-connected terms */
#define WO_AND    0x0400       /* Two or more AND-connected terms */
#define WO_EQUIV  0x0800       /* Of the form A==B, both columns */
#define WO_NOOP   0x1000       /* This term does not restrict search space */
#define WO_ROWVAL 0x2000       /* A row-value term */

#define WO_ALL    0x3fff       /* Mask of all possible WO_* values */
#define WO_SINGLE 0x01ff       /* Mask of all non-compound WO_* values */

/*
** These are definitions of bits in the WhereLoop.wsFlags field.
** The particular combination of bits in each WhereLoop help to
** determine the algorithm that WhereLoop represents.
*/
#define WHERE_COLUMN_EQ    0x00000001  /* x=EXPR */
#define WHERE_COLUMN_RANGE 0x00000002  /* x<EXPR and/or x>EXPR */
#define WHERE_COLUMN_IN    0x00000004  /* x IN (...) */
#define WHERE_COLUMN_NULL  0x00000008  /* x IS NULL */
#define WHERE_CONSTRAINT   0x0000000f  /* Any of the WHERE_COLUMN_xxx values */
#define WHERE_TOP_LIMIT    0x00000010  /* x<EXPR or x<=EXPR constraint */
#define WHERE_BTM_LIMIT    0x00000020  /* x>EXPR or x>=EXPR constraint */
#define WHERE_BOTH_LIMIT   0x00000030  /* Both x>EXPR and x<EXPR */
#define WHERE_IDX_ONLY     0x00000040  /* Use index only - omit table */
#define WHERE_IPK          0x00000100  /* x is the INTEGER PRIMARY KEY */
#define WHERE_INDEXED      0x00000200  /* WhereLoop.u.btree.pIndex is valid */
#define WHERE_VIRTUALTABLE 0x00000400  /* WhereLoop.u.vtab is valid */
#define WHERE_IN_ABLE      0x00000800  /* Able to support an IN operator */
#define WHERE_ONEROW       0x00001000  /* Selects no more than one row */
#define WHERE_MULTI_OR     0x00002000  /* OR using multiple indices */
#define WHERE_AUTO_INDEX   0x00004000  /* Uses an ephemeral index */
#define WHERE_SKIPSCAN     0x00008000  /* Uses the skip-scan algorithm */
#define WHERE_UNQ_WANTED   0x00010000  /* WHERE_ONEROW would have been helpful*/
#define WHERE_PARTIALIDX   0x00020000  /* The automatic index is partial */
#define WHERE_IN_EARLYOUT  0x00040000  /* Perhaps quit IN loops early */
#define WHERE_BIGNULL_SORT 0x00080000  /* Column nEq of index is BIGNULL */
#define WHERE_IN_SEEKSCAN  0x00100000  /* Seek-scan optimization for IN */
#define WHERE_TRANSCONS    0x00200000  /* Uses a transitive constraint */
#define WHERE_BLOOMFILTER  0x00400000  /* Consider using a Bloom-filter */
#define WHERE_SELFCULL     0x00800000  /* nOut reduced by extra WHERE terms */
#define WHERE_OMIT_OFFSET  0x01000000  /* Set offset counter to zero */
                      /*   0x02000000  -- available for reuse */
#define WHERE_EXPRIDX      0x04000000  /* Uses an index-on-expressions */

#endif /* !defined(SQLITE_WHEREINT_H) */

