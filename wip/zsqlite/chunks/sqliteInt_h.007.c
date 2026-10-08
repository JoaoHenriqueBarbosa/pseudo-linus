
/*
** Allowed values for Index.idxType
*/
#define SQLITE_IDXTYPE_APPDEF      0   /* Created using CREATE INDEX */
#define SQLITE_IDXTYPE_UNIQUE      1   /* Implements a UNIQUE constraint */
#define SQLITE_IDXTYPE_PRIMARYKEY  2   /* Is the PRIMARY KEY for the table */
#define SQLITE_IDXTYPE_IPK         3   /* INTEGER PRIMARY KEY index */

/* Return true if index X is a PRIMARY KEY index */
#define IsPrimaryKeyIndex(X)  ((X)->idxType==SQLITE_IDXTYPE_PRIMARYKEY)

/* Return true if index X is a UNIQUE index */
#define IsUniqueIndex(X)      ((X)->onError!=OE_None)

/* The Index.aiColumn[] values are normally positive integer.  But
** there are some negative values that have special meaning:
*/
#define XN_ROWID     (-1)     /* Indexed column is the rowid */
#define XN_EXPR      (-2)     /* Indexed column is an expression */

/*
** Each sample stored in the sqlite_stat4 table is represented in memory
** using a structure of this type.  See documentation at the top of the
** analyze.c source file for additional information.
*/
struct IndexSample {
  void *p;          /* Pointer to sampled record */
  int n;            /* Size of record in bytes */
  tRowcnt *anEq;    /* Est. number of rows where the key equals this sample */
  tRowcnt *anLt;    /* Est. number of rows where key is less than this sample */
  tRowcnt *anDLt;   /* Est. number of distinct keys less than this sample */
};

/*
** Possible values to use within the flags argument to sqlite3GetToken().
*/
#define SQLITE_TOKEN_QUOTED    0x1 /* Token is a quoted identifier. */
#define SQLITE_TOKEN_KEYWORD   0x2 /* Token is a keyword. */

/*
** Each token coming out of the lexer is an instance of
** this structure.  Tokens are also used as part of an expression.
**
** The memory that "z" points to is owned by other objects.  Take care
** that the owner of the "z" string does not deallocate the string before
** the Token goes out of scope!  Very often, the "z" points to some place
** in the middle of the Parse.zSql text.  But it might also point to a
** static string.
*/
struct Token {
  const char *z;     /* Text of the token.  Not NULL-terminated! */
  unsigned int n;    /* Number of characters in this token */
};

/*
** An instance of this structure contains information needed to generate
** code for a SELECT that contains aggregate functions.
**
** If Expr.op==TK_AGG_COLUMN or TK_AGG_FUNCTION then Expr.pAggInfo is a
** pointer to this structure.  The Expr.iAgg field is the index in
** AggInfo.aCol[] or AggInfo.aFunc[] of information needed to generate
** code for that node.
**
** AggInfo.pGroupBy and AggInfo.aFunc.pExpr point to fields within the
** original Select structure that describes the SELECT statement.  These
** fields do not need to be freed when deallocating the AggInfo structure.
*/
struct AggInfo {
  u8 directMode;          /* Direct rendering mode means take data directly
                          ** from source tables rather than from accumulators */
  u8 useSortingIdx;       /* In direct mode, reference the sorting index rather
                          ** than the source table */
  u16 nSortingColumn;     /* Number of columns in the sorting index */
  int sortingIdx;         /* Cursor number of the sorting index */
  int sortingIdxPTab;     /* Cursor number of pseudo-table */
  int iFirstReg;          /* First register in range for aCol[] and aFunc[] */
  ExprList *pGroupBy;     /* The group by clause */
  struct AggInfo_col {    /* For each column used in source tables */
    Table *pTab;             /* Source table */
    Expr *pCExpr;            /* The original expression */
    int iTable;              /* Cursor number of the source table */
    i16 iColumn;             /* Column number within the source table */
    i16 iSorterColumn;       /* Column number in the sorting index */
  } *aCol;
  int nColumn;            /* Number of used entries in aCol[] */
  int nAccumulator;       /* Number of columns that show through to the output.
                          ** Additional columns are used only as parameters to
                          ** aggregate functions */
  struct AggInfo_func {   /* For each aggregate function */
    Expr *pFExpr;            /* Expression encoding the function */
    FuncDef *pFunc;          /* The aggregate function implementation */
    int iDistinct;           /* Ephemeral table used to enforce DISTINCT */
    int iDistAddr;           /* Address of OP_OpenEphemeral */
    int iOBTab;              /* Ephemeral table to implement ORDER BY */
    u8 bOBPayload;           /* iOBTab has payload columns separate from key */
    u8 bOBUnique;            /* Enforce uniqueness on iOBTab keys */
    u8 bUseSubtype;          /* Transfer subtype info through sorter */
  } *aFunc;
  int nFunc;              /* Number of entries in aFunc[] */
  u32 selId;              /* Select to which this AggInfo belongs */
#ifdef SQLITE_DEBUG
  Select *pSelect;        /* SELECT statement that this AggInfo supports */
#endif
};

/*
** Macros to compute aCol[] and aFunc[] register numbers.
**
** These macros should not be used prior to the call to
** assignAggregateRegisters() that computes the value of pAggInfo->iFirstReg.
** The assert()s that are part of this macro verify that constraint.
*/
#define AggInfoColumnReg(A,I)  (assert((A)->iFirstReg),(A)->iFirstReg+(I))
#define AggInfoFuncReg(A,I)    \
                      (assert((A)->iFirstReg),(A)->iFirstReg+(A)->nColumn+(I))

/*
** The datatype ynVar is a signed integer, either 16-bit or 32-bit.
** Usually it is 16-bits.  But if SQLITE_MAX_VARIABLE_NUMBER is greater
** than 32767 we have to make it 32-bit.  16-bit is preferred because
** it uses less memory in the Expr object, which is a big memory user
** in systems with lots of prepared statements.  And few applications
** need more than about 10 or 20 variables.  But some extreme users want
** to have prepared statements with over 32766 variables, and for them
** the option is available (at compile-time).
*/
#if SQLITE_MAX_VARIABLE_NUMBER<32767
typedef i16 ynVar;
#else
typedef int ynVar;
#endif

/*
** Each node of an expression in the parse tree is an instance
** of this structure.
**
** Expr.op is the opcode. The integer parser token codes are reused
** as opcodes here. For example, the parser defines TK_GE to be an integer
** code representing the ">=" operator. This same integer code is reused
** to represent the greater-than-or-equal-to operator in the expression
** tree.
**
** If the expression is an SQL literal (TK_INTEGER, TK_FLOAT, TK_BLOB,
** or TK_STRING), then Expr.u.zToken contains the text of the SQL literal. If
** the expression is a variable (TK_VARIABLE), then Expr.u.zToken contains the
** variable name. Finally, if the expression is an SQL function (TK_FUNCTION),
** then Expr.u.zToken contains the name of the function.
**
** Expr.pRight and Expr.pLeft are the left and right subexpressions of a
** binary operator. Either or both may be NULL.
**
** Expr.x.pList is a list of arguments if the expression is an SQL function,
** a CASE expression or an IN expression of the form "<lhs> IN (<y>, <z>...)".
** Expr.x.pSelect is used if the expression is a sub-select or an expression of
** the form "<lhs> IN (SELECT ...)". If the EP_xIsSelect bit is set in the
** Expr.flags mask, then Expr.x.pSelect is valid. Otherwise, Expr.x.pList is
** valid.
**
** An expression of the form ID or ID.ID refers to a column in a table.
** For such expressions, Expr.op is set to TK_COLUMN and Expr.iTable is
** the integer cursor number of a VDBE cursor pointing to that table and
** Expr.iColumn is the column number for the specific column.  If the
** expression is used as a result in an aggregate SELECT, then the
** value is also stored in the Expr.iAgg column in the aggregate so that
** it can be accessed after all aggregates are computed.
**
** If the expression is an unbound variable marker (a question mark
** character '?' in the original SQL) then the Expr.iTable holds the index
** number for that variable.
**
** If the expression is a subquery then Expr.iColumn holds an integer
** register number containing the result of the subquery.  If the
** subquery gives a constant result, then iTable is -1.  If the subquery
** gives a different answer at different times during statement processing
** then iTable is the address of a subroutine that computes the subquery.
**
** If the Expr is of type OP_Column, and the table it is selecting from
** is a disk table or the "old.*" pseudo-table, then pTab points to the
** corresponding table definition.
**
** ALLOCATION NOTES:
**
** Expr objects can use a lot of memory space in database schema.  To
** help reduce memory requirements, sometimes an Expr object will be
** truncated.  And to reduce the number of memory allocations, sometimes
** two or more Expr objects will be stored in a single memory allocation,
** together with Expr.u.zToken strings.
**
** If the EP_Reduced and EP_TokenOnly flags are set when
** an Expr object is truncated.  When EP_Reduced is set, then all
** the child Expr objects in the Expr.pLeft and Expr.pRight subtrees
** are contained within the same memory allocation.  Note, however, that
** the subtrees in Expr.x.pList or Expr.x.pSelect are always separately
** allocated, regardless of whether or not EP_Reduced is set.
*/
struct Expr {
  u8 op;                 /* Operation performed by this node */
  char affExpr;          /* affinity, or RAISE type */
  u8 op2;                /* TK_REGISTER/TK_TRUTH: original value of Expr.op
                         ** TK_COLUMN: the value of p5 for OP_Column
                         ** TK_AGG_FUNCTION: nesting depth
                         ** TK_FUNCTION: NC_SelfRef flag if needs OP_PureFunc */
#ifdef SQLITE_DEBUG
  u8 vvaFlags;           /* Verification flags. */
#endif
  u32 flags;             /* Various flags.  EP_* See below */
  union {
    char *zToken;          /* Token value. Zero terminated and dequoted */
    int iValue;            /* Non-negative integer value if EP_IntValue */
  } u;

  /* If the EP_TokenOnly flag is set in the Expr.flags mask, then no
  ** space is allocated for the fields below this point. An attempt to
  ** access them will result in a segfault or malfunction.
  *********************************************************************/

  Expr *pLeft;           /* Left subnode */
  Expr *pRight;          /* Right subnode */
  union {
    ExprList *pList;     /* op = IN, EXISTS, SELECT, CASE, FUNCTION, BETWEEN */
    Select *pSelect;     /* EP_xIsSelect and op = IN, EXISTS, SELECT */
  } x;

  /* If the EP_Reduced flag is set in the Expr.flags mask, then no
  ** space is allocated for the fields below this point. An attempt to
  ** access them will result in a segfault or malfunction.
  *********************************************************************/

#if SQLITE_MAX_EXPR_DEPTH>0
  int nHeight;           /* Height of the tree headed by this node */
#endif
  int iTable;            /* TK_COLUMN: cursor number of table holding column
                         ** TK_REGISTER: register number
                         ** TK_TRIGGER: 1 -> new, 0 -> old
                         ** EP_Unlikely:  134217728 times likelihood
                         ** TK_IN: ephemeral table holding RHS
                         ** TK_SELECT_COLUMN: Number of columns on the LHS
                         ** TK_SELECT: 1st register of result vector */
  ynVar iColumn;         /* TK_COLUMN: column index.  -1 for rowid.
                         ** TK_VARIABLE: variable number (always >= 1).
                         ** TK_SELECT_COLUMN: column of the result vector */
  i16 iAgg;              /* Which entry in pAggInfo->aCol[] or ->aFunc[] */
  union {
    int iJoin;             /* If EP_OuterON or EP_InnerON, the right table */
    int iOfst;             /* else: start of token from start of statement */
  } w;
  AggInfo *pAggInfo;     /* Used by TK_AGG_COLUMN and TK_AGG_FUNCTION */
  union {
    Table *pTab;           /* TK_COLUMN: Table containing column. Can be NULL
                           ** for a column of an index on an expression */
    Window *pWin;          /* EP_WinFunc: Window/Filter defn for a function */
    struct {               /* TK_IN, TK_SELECT, and TK_EXISTS */
      int iAddr;             /* Subroutine entry address */
      int regReturn;         /* Register used to hold return address */
    } sub;
  } y;
};

/* The following are the meanings of bits in the Expr.flags field.
** Value restrictions:
**
**          EP_Agg == NC_HasAgg == SF_HasAgg
**          EP_Win == NC_HasWin
*/
#define EP_OuterON    0x000001 /* Originates in ON/USING clause of outer join */
#define EP_InnerON    0x000002 /* Originates in ON/USING of an inner join */
#define EP_Distinct   0x000004 /* Aggregate function with DISTINCT keyword */
#define EP_HasFunc    0x000008 /* Contains one or more functions of any kind */
#define EP_Agg        0x000010 /* Contains one or more aggregate functions */
#define EP_FixedCol   0x000020 /* TK_Column with a known fixed value */
#define EP_VarSelect  0x000040 /* pSelect is correlated, not constant */
#define EP_DblQuoted  0x000080 /* token.z was originally in "..." */
#define EP_InfixFunc  0x000100 /* True for an infix function: LIKE, GLOB, etc */
#define EP_Collate    0x000200 /* Tree contains a TK_COLLATE operator */
#define EP_Commuted   0x000400 /* Comparison operator has been commuted */
#define EP_IntValue   0x000800 /* Integer value contained in u.iValue */
#define EP_xIsSelect  0x001000 /* x.pSelect is valid (otherwise x.pList is) */
#define EP_Skip       0x002000 /* Operator does not contribute to affinity */
#define EP_Reduced    0x004000 /* Expr struct EXPR_REDUCEDSIZE bytes only */
#define EP_Win        0x008000 /* Contains window functions */
#define EP_TokenOnly  0x010000 /* Expr struct EXPR_TOKENONLYSIZE bytes only */
#define EP_FullSize   0x020000 /* Expr structure must remain full sized */
#define EP_IfNullRow  0x040000 /* The TK_IF_NULL_ROW opcode */
#define EP_Unlikely   0x080000 /* unlikely() or likelihood() function */
#define EP_ConstFunc  0x100000 /* A SQLITE_FUNC_CONSTANT or _SLOCHNG function */
#define EP_CanBeNull  0x200000 /* Can be null despite NOT NULL constraint */
#define EP_Subquery   0x400000 /* Tree contains a TK_SELECT operator */
#define EP_Leaf       0x800000 /* Expr.pLeft, .pRight, .u.pSelect all NULL */
#define EP_WinFunc   0x1000000 /* TK_FUNCTION with Expr.y.pWin set */
#define EP_Subrtn    0x2000000 /* Uses Expr.y.sub. TK_IN, _SELECT, or _EXISTS */
#define EP_Quoted    0x4000000 /* TK_ID was originally quoted */
#define EP_Static    0x8000000 /* Held in memory not obtained from malloc() */
#define EP_IsTrue   0x10000000 /* Always has boolean value of TRUE */
#define EP_IsFalse  0x20000000 /* Always has boolean value of FALSE */
#define EP_FromDDL  0x40000000 /* Originates from sqlite_schema */
               /*   0x80000000 // Available */

/* The EP_Propagate mask is a set of properties that automatically propagate
** upwards into parent nodes.
*/
#define EP_Propagate (EP_Collate|EP_Subquery|EP_HasFunc)

/* Macros can be used to test, set, or clear bits in the
** Expr.flags field.
*/
#define ExprHasProperty(E,P)     (((E)->flags&(P))!=0)
#define ExprHasAllProperty(E,P)  (((E)->flags&(P))==(P))
#define ExprSetProperty(E,P)     (E)->flags|=(P)
#define ExprClearProperty(E,P)   (E)->flags&=~(P)
#define ExprAlwaysTrue(E)   (((E)->flags&(EP_OuterON|EP_IsTrue))==EP_IsTrue)
#define ExprAlwaysFalse(E)  (((E)->flags&(EP_OuterON|EP_IsFalse))==EP_IsFalse)
#define ExprIsFullSize(E)   (((E)->flags&(EP_Reduced|EP_TokenOnly))==0)

/* Macros used to ensure that the correct members of unions are accessed
** in Expr.
*/
#define ExprUseUToken(E)    (((E)->flags&EP_IntValue)==0)
#define ExprUseUValue(E)    (((E)->flags&EP_IntValue)!=0)
#define ExprUseWOfst(E)     (((E)->flags&(EP_InnerON|EP_OuterON))==0)
#define ExprUseWJoin(E)     (((E)->flags&(EP_InnerON|EP_OuterON))!=0)
#define ExprUseXList(E)     (((E)->flags&EP_xIsSelect)==0)
#define ExprUseXSelect(E)   (((E)->flags&EP_xIsSelect)!=0)
#define ExprUseYTab(E)      (((E)->flags&(EP_WinFunc|EP_Subrtn))==0)
#define ExprUseYWin(E)      (((E)->flags&EP_WinFunc)!=0)
#define ExprUseYSub(E)      (((E)->flags&EP_Subrtn)!=0)

/* Flags for use with Expr.vvaFlags
*/
#define EP_NoReduce   0x01  /* Cannot EXPRDUP_REDUCE this Expr */
#define EP_Immutable  0x02  /* Do not change this Expr node */

/* The ExprSetVVAProperty() macro is used for Verification, Validation,
** and Accreditation only.  It works like ExprSetProperty() during VVA
** processes but is a no-op for delivery.
*/
#ifdef SQLITE_DEBUG
# define ExprSetVVAProperty(E,P)   (E)->vvaFlags|=(P)
# define ExprHasVVAProperty(E,P)   (((E)->vvaFlags&(P))!=0)
# define ExprClearVVAProperties(E) (E)->vvaFlags = 0
#else
# define ExprSetVVAProperty(E,P)
# define ExprHasVVAProperty(E,P)   0
# define ExprClearVVAProperties(E)
#endif

/*
** Macros to determine the number of bytes required by a normal Expr
** struct, an Expr struct with the EP_Reduced flag set in Expr.flags
** and an Expr struct with the EP_TokenOnly flag set.
*/
#define EXPR_FULLSIZE           sizeof(Expr)           /* Full size */
#define EXPR_REDUCEDSIZE        offsetof(Expr,iTable)  /* Common features */
#define EXPR_TOKENONLYSIZE      offsetof(Expr,pLeft)   /* Fewer features */

/*
** Flags passed to the sqlite3ExprDup() function. See the header comment
** above sqlite3ExprDup() for details.
*/
#define EXPRDUP_REDUCE         0x0001  /* Used reduced-size Expr nodes */

/*
** True if the expression passed as an argument was a function with
** an OVER() clause (a window function).
*/
#ifdef SQLITE_OMIT_WINDOWFUNC
# define IsWindowFunc(p) 0
#else
# define IsWindowFunc(p) ( \
    ExprHasProperty((p), EP_WinFunc) && p->y.pWin->eFrmType!=TK_FILTER \
 )
#endif

/*
** A list of expressions.  Each expression may optionally have a
** name.  An expr/name combination can be used in several ways, such
** as the list of "expr AS ID" fields following a "SELECT" or in the
** list of "ID = expr" items in an UPDATE.  A list of expressions can
** also be used as the argument to a function, in which case the a.zName
** field is not used.
**
** In order to try to keep memory usage down, the Expr.a.zEName field
** is used for multiple purposes:
**
**     eEName          Usage
**    ----------       -------------------------
**    ENAME_NAME       (1) the AS of result set column
**                     (2) COLUMN= of an UPDATE
**
**    ENAME_TAB        DB.TABLE.NAME used to resolve names
**                     of subqueries
**
**    ENAME_SPAN       Text of the original result set
**                     expression.
*/
struct ExprList {
  int nExpr;             /* Number of expressions on the list */
  int nAlloc;            /* Number of a[] slots allocated */
  struct ExprList_item { /* For each expression in the list */
    Expr *pExpr;            /* The parse tree for this expression */
    char *zEName;           /* Token associated with this expression */
    struct {
      u8 sortFlags;           /* Mask of KEYINFO_ORDER_* flags */
      unsigned eEName :2;     /* Meaning of zEName */
      unsigned done :1;       /* Indicates when processing is finished */
      unsigned reusable :1;   /* Constant expression is reusable */
      unsigned bSorterRef :1; /* Defer evaluation until after sorting */
      unsigned bNulls :1;     /* True if explicit "NULLS FIRST/LAST" */
      unsigned bUsed :1;      /* This column used in a SF_NestedFrom subquery */
      unsigned bUsingTerm:1;  /* Term from the USING clause of a NestedFrom */
      unsigned bNoExpand: 1;  /* Term is an auxiliary in NestedFrom and should
                              ** not be expanded by "*" in parent queries */
    } fg;
    union {
      struct {             /* Used by any ExprList other than Parse.pConsExpr */
        u16 iOrderByCol;      /* For ORDER BY, column number in result set */
        u16 iAlias;           /* Index into Parse.aAlias[] for zName */
      } x;
      int iConstExprReg;   /* Register in which Expr value is cached. Used only
                           ** by Parse.pConstExpr */
    } u;
  } a[1];                  /* One slot for each expression in the list */
};
