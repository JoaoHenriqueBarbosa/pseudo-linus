
/*
** Allowed values for Expr.a.eEName
*/
#define ENAME_NAME  0       /* The AS clause of a result set */
#define ENAME_SPAN  1       /* Complete text of the result set expression */
#define ENAME_TAB   2       /* "DB.TABLE.NAME" for the result set */
#define ENAME_ROWID 3       /* "DB.TABLE._rowid_" for * expansion of rowid */

/*
** An instance of this structure can hold a simple list of identifiers,
** such as the list "a,b,c" in the following statements:
**
**      INSERT INTO t(a,b,c) VALUES ...;
**      CREATE INDEX idx ON t(a,b,c);
**      CREATE TRIGGER trig BEFORE UPDATE ON t(a,b,c) ...;
**
** The IdList.a.idx field is used when the IdList represents the list of
** column names after a table name in an INSERT statement.  In the statement
**
**     INSERT INTO t(a,b,c) ...
**
** If "a" is the k-th column of table "t", then IdList.a[0].idx==k.
*/
struct IdList {
  int nId;         /* Number of identifiers on the list */
  u8 eU4;          /* Which element of a.u4 is valid */
  struct IdList_item {
    char *zName;      /* Name of the identifier */
    union {
      int idx;          /* Index in some Table.aCol[] of a column named zName */
      Expr *pExpr;      /* Expr to implement a USING variable -- NOT USED */
    } u4;
  } a[1];
};

/*
** Allowed values for IdList.eType, which determines which value of the a.u4
** is valid.
*/
#define EU4_NONE   0   /* Does not use IdList.a.u4 */
#define EU4_IDX    1   /* Uses IdList.a.u4.idx */
#define EU4_EXPR   2   /* Uses IdList.a.u4.pExpr -- NOT CURRENTLY USED */

/*
** The SrcItem object represents a single term in the FROM clause of a query.
** The SrcList object is mostly an array of SrcItems.
**
** The jointype starts out showing the join type between the current table
** and the next table on the list.  The parser builds the list this way.
** But sqlite3SrcListShiftJoinType() later shifts the jointypes so that each
** jointype expresses the join between the table and the previous table.
**
** In the colUsed field, the high-order bit (bit 63) is set if the table
** contains more than 63 columns and the 64-th or later column is used.
**
** Union member validity:
**
**    u1.zIndexedBy      fg.isIndexedBy && !fg.isTabFunc
**    u1.pFuncArg        fg.isTabFunc   && !fg.isIndexedBy
**    u1.nRow            !fg.isTabFunc  && !fg.isIndexedBy
**
**    u2.pIBIndex        fg.isIndexedBy && !fg.isCte
**    u2.pCteUse         fg.isCte       && !fg.isIndexedBy
*/
struct SrcItem {
  Schema *pSchema;  /* Schema to which this item is fixed */
  char *zDatabase;  /* Name of database holding this table */
  char *zName;      /* Name of the table */
  char *zAlias;     /* The "B" part of a "A AS B" phrase.  zName is the "A" */
  Table *pTab;      /* An SQL table corresponding to zName */
  Select *pSelect;  /* A SELECT statement used in place of a table name */
  int addrFillSub;  /* Address of subroutine to manifest a subquery */
  int regReturn;    /* Register holding return address of addrFillSub */
  int regResult;    /* Registers holding results of a co-routine */
  struct {
    u8 jointype;      /* Type of join between this table and the previous */
    unsigned notIndexed :1;    /* True if there is a NOT INDEXED clause */
    unsigned isIndexedBy :1;   /* True if there is an INDEXED BY clause */
    unsigned isTabFunc :1;     /* True if table-valued-function syntax */
    unsigned isCorrelated :1;  /* True if sub-query is correlated */
    unsigned isMaterialized:1; /* This is a materialized view */
    unsigned viaCoroutine :1;  /* Implemented as a co-routine */
    unsigned isRecursive :1;   /* True for recursive reference in WITH */
    unsigned fromDDL :1;       /* Comes from sqlite_schema */
    unsigned isCte :1;         /* This is a CTE */
    unsigned notCte :1;        /* This item may not match a CTE */
    unsigned isUsing :1;       /* u3.pUsing is valid */
    unsigned isOn :1;          /* u3.pOn was once valid and non-NULL */
    unsigned isSynthUsing :1;  /* u3.pUsing is synthesized from NATURAL */
    unsigned isNestedFrom :1;  /* pSelect is a SF_NestedFrom subquery */
    unsigned rowidUsed :1;     /* The ROWID of this table is referenced */
  } fg;
  int iCursor;      /* The VDBE cursor number used to access this table */
  union {
    Expr *pOn;        /* fg.isUsing==0 =>  The ON clause of a join */
    IdList *pUsing;   /* fg.isUsing==1 =>  The USING clause of a join */
  } u3;
  Bitmask colUsed;  /* Bit N set if column N used. Details above for N>62 */
  union {
    char *zIndexedBy;    /* Identifier from "INDEXED BY <zIndex>" clause */
    ExprList *pFuncArg;  /* Arguments to table-valued-function */
    u32 nRow;            /* Number of rows in a VALUES clause */
  } u1;
  union {
    Index *pIBIndex;  /* Index structure corresponding to u1.zIndexedBy */
    CteUse *pCteUse;  /* CTE Usage info when fg.isCte is true */
  } u2;
};

/*
** The OnOrUsing object represents either an ON clause or a USING clause.
** It can never be both at the same time, but it can be neither.
*/
struct OnOrUsing {
  Expr *pOn;         /* The ON clause of a join */
  IdList *pUsing;    /* The USING clause of a join */
};

/*
** This object represents one or more tables that are the source of
** content for an SQL statement.  For example, a single SrcList object
** is used to hold the FROM clause of a SELECT statement.  SrcList also
** represents the target tables for DELETE, INSERT, and UPDATE statements.
**
*/
struct SrcList {
  int nSrc;        /* Number of tables or subqueries in the FROM clause */
  u32 nAlloc;      /* Number of entries allocated in a[] below */
  SrcItem a[1];    /* One entry for each identifier on the list */
};

/*
** Permitted values of the SrcList.a.jointype field
*/
#define JT_INNER     0x01    /* Any kind of inner or cross join */
#define JT_CROSS     0x02    /* Explicit use of the CROSS keyword */
#define JT_NATURAL   0x04    /* True for a "natural" join */
#define JT_LEFT      0x08    /* Left outer join */
#define JT_RIGHT     0x10    /* Right outer join */
#define JT_OUTER     0x20    /* The "OUTER" keyword is present */
#define JT_LTORJ     0x40    /* One of the LEFT operands of a RIGHT JOIN
                             ** Mnemonic: Left Table Of Right Join */
#define JT_ERROR     0x80    /* unknown or unsupported join type */

/*
** Flags appropriate for the wctrlFlags parameter of sqlite3WhereBegin()
** and the WhereInfo.wctrlFlags member.
**
** Value constraints (enforced via assert()):
**     WHERE_USE_LIMIT  == SF_FixedLimit
*/
#define WHERE_ORDERBY_NORMAL   0x0000 /* No-op */
#define WHERE_ORDERBY_MIN      0x0001 /* ORDER BY processing for min() func */
#define WHERE_ORDERBY_MAX      0x0002 /* ORDER BY processing for max() func */
#define WHERE_ONEPASS_DESIRED  0x0004 /* Want to do one-pass UPDATE/DELETE */
#define WHERE_ONEPASS_MULTIROW 0x0008 /* ONEPASS is ok with multiple rows */
#define WHERE_DUPLICATES_OK    0x0010 /* Ok to return a row more than once */
#define WHERE_OR_SUBCLAUSE     0x0020 /* Processing a sub-WHERE as part of
                                      ** the OR optimization  */
#define WHERE_GROUPBY          0x0040 /* pOrderBy is really a GROUP BY */
#define WHERE_DISTINCTBY       0x0080 /* pOrderby is really a DISTINCT clause */
#define WHERE_WANT_DISTINCT    0x0100 /* All output needs to be distinct */
#define WHERE_SORTBYGROUP      0x0200 /* Support sqlite3WhereIsSorted() */
#define WHERE_AGG_DISTINCT     0x0400 /* Query is "SELECT agg(DISTINCT ...)" */
#define WHERE_ORDERBY_LIMIT    0x0800 /* ORDERBY+LIMIT on the inner loop */
#define WHERE_RIGHT_JOIN       0x1000 /* Processing a RIGHT JOIN */
#define WHERE_KEEP_ALL_JOINS   0x2000 /* Do not do the omit-noop-join opt */
#define WHERE_USE_LIMIT        0x4000 /* Use the LIMIT in cost estimates */
                        /*     0x8000    not currently used */

/* Allowed return values from sqlite3WhereIsDistinct()
*/
#define WHERE_DISTINCT_NOOP      0  /* DISTINCT keyword not used */
#define WHERE_DISTINCT_UNIQUE    1  /* No duplicates */
#define WHERE_DISTINCT_ORDERED   2  /* All duplicates are adjacent */
#define WHERE_DISTINCT_UNORDERED 3  /* Duplicates are scattered */

/*
** A NameContext defines a context in which to resolve table and column
** names.  The context consists of a list of tables (the pSrcList) field and
** a list of named expression (pEList).  The named expression list may
** be NULL.  The pSrc corresponds to the FROM clause of a SELECT or
** to the table being operated on by INSERT, UPDATE, or DELETE.  The
** pEList corresponds to the result set of a SELECT and is NULL for
** other statements.
**
** NameContexts can be nested.  When resolving names, the inner-most
** context is searched first.  If no match is found, the next outer
** context is checked.  If there is still no match, the next context
** is checked.  This process continues until either a match is found
** or all contexts are check.  When a match is found, the nRef member of
** the context containing the match is incremented.
**
** Each subquery gets a new NameContext.  The pNext field points to the
** NameContext in the parent query.  Thus the process of scanning the
** NameContext list corresponds to searching through successively outer
** subqueries looking for a match.
*/
struct NameContext {
  Parse *pParse;       /* The parser */
  SrcList *pSrcList;   /* One or more tables used to resolve names */
  union {
    ExprList *pEList;    /* Optional list of result-set columns */
    AggInfo *pAggInfo;   /* Information about aggregates at this level */
    Upsert *pUpsert;     /* ON CONFLICT clause information from an upsert */
    int iBaseReg;        /* For TK_REGISTER when parsing RETURNING */
  } uNC;
  NameContext *pNext;  /* Next outer name context.  NULL for outermost */
  int nRef;            /* Number of names resolved by this context */
  int nNcErr;          /* Number of errors encountered while resolving names */
  int ncFlags;         /* Zero or more NC_* flags defined below */
  u32 nNestedSelect;   /* Number of nested selects using this NC */
  Select *pWinSelect;  /* SELECT statement for any window functions */
};

/*
** Allowed values for the NameContext, ncFlags field.
**
** Value constraints (all checked via assert()):
**    NC_HasAgg    == SF_HasAgg       == EP_Agg
**    NC_MinMaxAgg == SF_MinMaxAgg    == SQLITE_FUNC_MINMAX
**    NC_OrderAgg  == SF_OrderByReqd  == SQLITE_FUNC_ANYORDER
**    NC_HasWin    == EP_Win
**
*/
#define NC_AllowAgg  0x000001 /* Aggregate functions are allowed here */
#define NC_PartIdx   0x000002 /* True if resolving a partial index WHERE */
#define NC_IsCheck   0x000004 /* True if resolving a CHECK constraint */
#define NC_GenCol    0x000008 /* True for a GENERATED ALWAYS AS clause */
#define NC_HasAgg    0x000010 /* One or more aggregate functions seen */
#define NC_IdxExpr   0x000020 /* True if resolving columns of CREATE INDEX */
#define NC_SelfRef   0x00002e /* Combo: PartIdx, isCheck, GenCol, and IdxExpr */
#define NC_Subquery  0x000040 /* A subquery has been seen */
#define NC_UEList    0x000080 /* True if uNC.pEList is used */
#define NC_UAggInfo  0x000100 /* True if uNC.pAggInfo is used */
#define NC_UUpsert   0x000200 /* True if uNC.pUpsert is used */
#define NC_UBaseReg  0x000400 /* True if uNC.iBaseReg is used */
#define NC_MinMaxAgg 0x001000 /* min/max aggregates seen.  See note above */
#define NC_Complex   0x002000 /* True if a function or subquery seen */
#define NC_AllowWin  0x004000 /* Window functions are allowed here */
#define NC_HasWin    0x008000 /* One or more window functions seen */
#define NC_IsDDL     0x010000 /* Resolving names in a CREATE statement */
#define NC_InAggFunc 0x020000 /* True if analyzing arguments to an agg func */
#define NC_FromDDL   0x040000 /* SQL text comes from sqlite_schema */
#define NC_NoSelect  0x080000 /* Do not descend into sub-selects */
#define NC_Where     0x100000 /* Processing WHERE clause of a SELECT */
#define NC_OrderAgg 0x8000000 /* Has an aggregate other than count/min/max */

/*
** An instance of the following object describes a single ON CONFLICT
** clause in an upsert.
**
** The pUpsertTarget field is only set if the ON CONFLICT clause includes
** conflict-target clause.  (In "ON CONFLICT(a,b)" the "(a,b)" is the
** conflict-target clause.)  The pUpsertTargetWhere is the optional
** WHERE clause used to identify partial unique indexes.
**
** pUpsertSet is the list of column=expr terms of the UPDATE statement.
** The pUpsertSet field is NULL for a ON CONFLICT DO NOTHING.  The
** pUpsertWhere is the WHERE clause for the UPDATE and is NULL if the
** WHERE clause is omitted.
*/
struct Upsert {
  ExprList *pUpsertTarget;  /* Optional description of conflict target */
  Expr *pUpsertTargetWhere; /* WHERE clause for partial index targets */
  ExprList *pUpsertSet;     /* The SET clause from an ON CONFLICT UPDATE */
  Expr *pUpsertWhere;       /* WHERE clause for the ON CONFLICT UPDATE */
  Upsert *pNextUpsert;      /* Next ON CONFLICT clause in the list */
  u8 isDoUpdate;            /* True for DO UPDATE.  False for DO NOTHING */
  u8 isDup;                 /* True if 2nd or later with same pUpsertIdx */
  /* Above this point is the parse tree for the ON CONFLICT clauses.
  ** The next group of fields stores intermediate data. */
  void *pToFree;            /* Free memory when deleting the Upsert object */
  /* All fields above are owned by the Upsert object and must be freed
  ** when the Upsert is destroyed.  The fields below are used to transfer
  ** information from the INSERT processing down into the UPDATE processing
  ** while generating code.  The fields below are owned by the INSERT
  ** statement and will be freed by INSERT processing. */
  Index *pUpsertIdx;        /* UNIQUE constraint specified by pUpsertTarget */
  SrcList *pUpsertSrc;      /* Table to be updated */
  int regData;              /* First register holding array of VALUES */
  int iDataCur;             /* Index of the data cursor */
  int iIdxCur;              /* Index of the first index cursor */
};

/*
** An instance of the following structure contains all information
** needed to generate code for a single SELECT statement.
**
** See the header comment on the computeLimitRegisters() routine for a
** detailed description of the meaning of the iLimit and iOffset fields.
**
** addrOpenEphm[] entries contain the address of OP_OpenEphemeral opcodes.
** These addresses must be stored so that we can go back and fill in
** the P4_KEYINFO and P2 parameters later.  Neither the KeyInfo nor
** the number of columns in P2 can be computed at the same time
** as the OP_OpenEphm instruction is coded because not
** enough information about the compound query is known at that point.
** The KeyInfo for addrOpenTran[0] and [1] contains collating sequences
** for the result set.  The KeyInfo for addrOpenEphm[2] contains collating
** sequences for the ORDER BY clause.
*/
struct Select {
  u8 op;                 /* One of: TK_UNION TK_ALL TK_INTERSECT TK_EXCEPT */
  LogEst nSelectRow;     /* Estimated number of result rows */
  u32 selFlags;          /* Various SF_* values */
  int iLimit, iOffset;   /* Memory registers holding LIMIT & OFFSET counters */
  u32 selId;             /* Unique identifier number for this SELECT */
  int addrOpenEphm[2];   /* OP_OpenEphem opcodes related to this select */
  ExprList *pEList;      /* The fields of the result */
  SrcList *pSrc;         /* The FROM clause */
  Expr *pWhere;          /* The WHERE clause */
  ExprList *pGroupBy;    /* The GROUP BY clause */
  Expr *pHaving;         /* The HAVING clause */
  ExprList *pOrderBy;    /* The ORDER BY clause */
  Select *pPrior;        /* Prior select in a compound select statement */
  Select *pNext;         /* Next select to the left in a compound */
  Expr *pLimit;          /* LIMIT expression. NULL means not used. */
  With *pWith;           /* WITH clause attached to this select. Or NULL. */
#ifndef SQLITE_OMIT_WINDOWFUNC
  Window *pWin;          /* List of window functions */
  Window *pWinDefn;      /* List of named window definitions */
#endif
};

/*
** Allowed values for Select.selFlags.  The "SF" prefix stands for
** "Select Flag".
**
** Value constraints (all checked via assert())
**     SF_HasAgg      == NC_HasAgg
**     SF_MinMaxAgg   == NC_MinMaxAgg     == SQLITE_FUNC_MINMAX
**     SF_OrderByReqd == NC_OrderAgg      == SQLITE_FUNC_ANYORDER
**     SF_FixedLimit  == WHERE_USE_LIMIT
*/
#define SF_Distinct      0x0000001 /* Output should be DISTINCT */
#define SF_All           0x0000002 /* Includes the ALL keyword */
#define SF_Resolved      0x0000004 /* Identifiers have been resolved */
#define SF_Aggregate     0x0000008 /* Contains agg functions or a GROUP BY */
#define SF_HasAgg        0x0000010 /* Contains aggregate functions */
#define SF_UsesEphemeral 0x0000020 /* Uses the OpenEphemeral opcode */
#define SF_Expanded      0x0000040 /* sqlite3SelectExpand() called on this */
#define SF_HasTypeInfo   0x0000080 /* FROM subqueries have Table metadata */
#define SF_Compound      0x0000100 /* Part of a compound query */
#define SF_Values        0x0000200 /* Synthesized from VALUES clause */
#define SF_MultiValue    0x0000400 /* Single VALUES term with multiple rows */
#define SF_NestedFrom    0x0000800 /* Part of a parenthesized FROM clause */
#define SF_MinMaxAgg     0x0001000 /* Aggregate containing min() or max() */
#define SF_Recursive     0x0002000 /* The recursive part of a recursive CTE */
#define SF_FixedLimit    0x0004000 /* nSelectRow set by a constant LIMIT */
#define SF_MaybeConvert  0x0008000 /* Need convertCompoundSelectToSubquery() */
#define SF_Converted     0x0010000 /* By convertCompoundSelectToSubquery() */
#define SF_IncludeHidden 0x0020000 /* Include hidden columns in output */
#define SF_ComplexResult 0x0040000 /* Result contains subquery or function */
#define SF_WhereBegin    0x0080000 /* Really a WhereBegin() call.  Debug Only */
#define SF_WinRewrite    0x0100000 /* Window function rewrite accomplished */
#define SF_View          0x0200000 /* SELECT statement is a view */
#define SF_NoopOrderBy   0x0400000 /* ORDER BY is ignored for this query */
#define SF_UFSrcCheck    0x0800000 /* Check pSrc as required by UPDATE...FROM */
#define SF_PushDown      0x1000000 /* Modified by WHERE-clause push-down opt */
#define SF_MultiPart     0x2000000 /* Has multiple incompatible PARTITIONs */
#define SF_CopyCte       0x4000000 /* SELECT statement is a copy of a CTE */
#define SF_OrderByReqd   0x8000000 /* The ORDER BY clause may not be omitted */
#define SF_UpdateFrom   0x10000000 /* Query originates with UPDATE FROM */
#define SF_Correlated   0x20000000 /* True if references the outer context */

/* True if S exists and has SF_NestedFrom */
#define IsNestedFrom(S) ((S)!=0 && ((S)->selFlags&SF_NestedFrom)!=0)

/*
** The results of a SELECT can be distributed in several ways, as defined
** by one of the following macros.  The "SRT" prefix means "SELECT Result
** Type".
**
**     SRT_Union       Store results as a key in a temporary index
**                     identified by pDest->iSDParm.
**
**     SRT_Except      Remove results from the temporary index pDest->iSDParm.
**
**     SRT_Exists      Store a 1 in memory cell pDest->iSDParm if the result
**                     set is not empty.
**
**     SRT_Discard     Throw the results away.  This is used by SELECT
**                     statements within triggers whose only purpose is
**                     the side-effects of functions.
**
**     SRT_Output      Generate a row of output (using the OP_ResultRow
**                     opcode) for each row in the result set.
**
**     SRT_Mem         Only valid if the result is a single column.
**                     Store the first column of the first result row
**                     in register pDest->iSDParm then abandon the rest
**                     of the query.  This destination implies "LIMIT 1".
**
**     SRT_Set         The result must be a single column.  Store each
**                     row of result as the key in table pDest->iSDParm.
**                     Apply the affinity pDest->affSdst before storing
**                     results.  Used to implement "IN (SELECT ...)".
**
**     SRT_EphemTab    Create an temporary table pDest->iSDParm and store
**                     the result there. The cursor is left open after
**                     returning.  This is like SRT_Table except that
**                     this destination uses OP_OpenEphemeral to create
**                     the table first.
**
**     SRT_Coroutine   Generate a co-routine that returns a new row of
**                     results each time it is invoked.  The entry point
**                     of the co-routine is stored in register pDest->iSDParm
**                     and the result row is stored in pDest->nDest registers
**                     starting with pDest->iSdst.
**
**     SRT_Table       Store results in temporary table pDest->iSDParm.
**     SRT_Fifo        This is like SRT_EphemTab except that the table
**                     is assumed to already be open.  SRT_Fifo has
**                     the additional property of being able to ignore
**                     the ORDER BY clause.
**
**     SRT_DistFifo    Store results in a temporary table pDest->iSDParm.
**                     But also use temporary table pDest->iSDParm+1 as
**                     a record of all prior results and ignore any duplicate
**                     rows.  Name means:  "Distinct Fifo".
**
**     SRT_Queue       Store results in priority queue pDest->iSDParm (really
**                     an index).  Append a sequence number so that all entries
**                     are distinct.
**
**     SRT_DistQueue   Store results in priority queue pDest->iSDParm only if
**                     the same record has never been stored before.  The
**                     index at pDest->iSDParm+1 hold all prior stores.
**
**     SRT_Upfrom      Store results in the temporary table already opened by
**                     pDest->iSDParm. If (pDest->iSDParm<0), then the temp
**                     table is an intkey table - in this case the first
**                     column returned by the SELECT is used as the integer
**                     key. If (pDest->iSDParm>0), then the table is an index
**                     table. (pDest->iSDParm) is the number of key columns in
**                     each index record in this case.
*/
#define SRT_Union        1  /* Store result as keys in an index */
#define SRT_Except       2  /* Remove result from a UNION index */
#define SRT_Exists       3  /* Store 1 if the result is not empty */
#define SRT_Discard      4  /* Do not save the results anywhere */
#define SRT_DistFifo     5  /* Like SRT_Fifo, but unique results only */
#define SRT_DistQueue    6  /* Like SRT_Queue, but unique results only */

/* The DISTINCT clause is ignored for all of the above.  Not that
** IgnorableDistinct() implies IgnorableOrderby() */
#define IgnorableDistinct(X) ((X->eDest)<=SRT_DistQueue)

#define SRT_Queue        7  /* Store result in an queue */
#define SRT_Fifo         8  /* Store result as data with an automatic rowid */

/* The ORDER BY clause is ignored for all of the above */
#define IgnorableOrderby(X) ((X->eDest)<=SRT_Fifo)

#define SRT_Output       9  /* Output each row of result */
#define SRT_Mem         10  /* Store result in a memory cell */
#define SRT_Set         11  /* Store results as keys in an index */
#define SRT_EphemTab    12  /* Create transient tab and store like SRT_Table */
#define SRT_Coroutine   13  /* Generate a single row of result */
#define SRT_Table       14  /* Store result as data with an automatic rowid */
#define SRT_Upfrom      15  /* Store result as data with rowid */

/*
** An instance of this object describes where to put of the results of
** a SELECT statement.
*/
struct SelectDest {
  u8 eDest;            /* How to dispose of the results.  One of SRT_* above. */
  int iSDParm;         /* A parameter used by the eDest disposal method */
  int iSDParm2;        /* A second parameter for the eDest disposal method */
  int iSdst;           /* Base register where results are written */
  int nSdst;           /* Number of registers allocated */
  char *zAffSdst;      /* Affinity used for SRT_Set */
  ExprList *pOrderBy;  /* Key columns for SRT_Queue and SRT_DistQueue */
};
