/*
** 2013-11-12
**
** The author disclaims copyright to this source code.  In place of
** a legal notice, here is a blessing:
**
**    May you do good and not evil.
**    May you find forgiveness for yourself and forgive others.
**    May you share freely, never taking more than you give.
**
*************************************************************************
**
** This file contains structure and macro definitions for the query
** planner logic in "where.c".  These definitions are broken out into
** a separate source file for easier editing.
*/
#ifndef SQLITE_WHEREINT_H
#define SQLITE_WHEREINT_H


/* Forward references
*/
typedef struct WhereClause WhereClause;
typedef struct WhereMaskSet WhereMaskSet;
typedef struct WhereOrInfo WhereOrInfo;
typedef struct WhereAndInfo WhereAndInfo;
typedef struct WhereLevel WhereLevel;
typedef struct WhereLoop WhereLoop;
typedef struct WherePath WherePath;
typedef struct WhereTerm WhereTerm;
typedef struct WhereLoopBuilder WhereLoopBuilder;
typedef struct WhereScan WhereScan;
typedef struct WhereOrCost WhereOrCost;
typedef struct WhereOrSet WhereOrSet;
typedef struct WhereMemBlock WhereMemBlock;
typedef struct WhereRightJoin WhereRightJoin;

/*
** This object is a header on a block of allocated memory that will be
** automatically freed when its WInfo object is destructed.
*/
struct WhereMemBlock {
  WhereMemBlock *pNext;      /* Next block in the chain */
  u64 sz;                    /* Bytes of space */
};

/*
** Extra information attached to a WhereLevel that is a RIGHT JOIN.
*/
struct WhereRightJoin {
  int iMatch;          /* Cursor used to determine prior matched rows */
  int regBloom;        /* Bloom filter for iRJMatch */
  int regReturn;       /* Return register for the interior subroutine */
  int addrSubrtn;      /* Starting address for the interior subroutine */
  int endSubrtn;       /* The last opcode in the interior subroutine */
};

/*
** This object contains information needed to implement a single nested
** loop in WHERE clause.
**
** Contrast this object with WhereLoop.  This object describes the
** implementation of the loop.  WhereLoop describes the algorithm.
** This object contains a pointer to the WhereLoop algorithm as one of
** its elements.
**
** The WhereInfo object contains a single instance of this object for
** each term in the FROM clause (which is to say, for each of the
** nested loops as implemented).  The order of WhereLevel objects determines
** the loop nested order, with WhereInfo.a[0] being the outer loop and
** WhereInfo.a[WhereInfo.nLevel-1] being the inner loop.
*/
struct WhereLevel {
  int iLeftJoin;        /* Memory cell used to implement LEFT OUTER JOIN */
  int iTabCur;          /* The VDBE cursor used to access the table */
  int iIdxCur;          /* The VDBE cursor used to access pIdx */
  int addrBrk;          /* Jump here to break out of the loop */
  int addrNxt;          /* Jump here to start the next IN combination */
  int addrSkip;         /* Jump here for next iteration of skip-scan */
  int addrCont;         /* Jump here to continue with the next loop cycle */
  int addrFirst;        /* First instruction of interior of the loop */
  int addrBody;         /* Beginning of the body of this loop */
  int regBignull;       /* big-null flag reg. True if a NULL-scan is needed */
  int addrBignull;      /* Jump here for next part of big-null scan */
#ifndef SQLITE_LIKE_DOESNT_MATCH_BLOBS
  u32 iLikeRepCntr;     /* LIKE range processing counter register (times 2) */
  int addrLikeRep;      /* LIKE range processing address */
#endif
  int regFilter;        /* Bloom filter */
  WhereRightJoin *pRJ;  /* Extra information for RIGHT JOIN */
  u8 iFrom;             /* Which entry in the FROM clause */
  u8 op, p3, p5;        /* Opcode, P3 & P5 of the opcode that ends the loop */
  int p1, p2;           /* Operands of the opcode used to end the loop */
  union {               /* Information that depends on pWLoop->wsFlags */
    struct {
      int nIn;              /* Number of entries in aInLoop[] */
      struct InLoop {
        int iCur;              /* The VDBE cursor used by this IN operator */
        int addrInTop;         /* Top of the IN loop */
        int iBase;             /* Base register of multi-key index record */
        int nPrefix;           /* Number of prior entries in the key */
        u8 eEndLoopOp;         /* IN Loop terminator. OP_Next or OP_Prev */
      } *aInLoop;           /* Information about each nested IN operator */
    } in;                 /* Used when pWLoop->wsFlags&WHERE_IN_ABLE */
    Index *pCoveringIdx;  /* Possible covering index for WHERE_MULTI_OR */
  } u;
  struct WhereLoop *pWLoop;  /* The selected WhereLoop object */
  Bitmask notReady;          /* FROM entries not usable at this level */
#ifdef SQLITE_ENABLE_STMT_SCANSTATUS
  int addrVisit;        /* Address at which row is visited */
#endif
};

/*
** Each instance of this object represents an algorithm for evaluating one
** term of a join.  Every term of the FROM clause will have at least
** one corresponding WhereLoop object (unless INDEXED BY constraints
** prevent a query solution - which is an error) and many terms of the
** FROM clause will have multiple WhereLoop objects, each describing a
** potential way of implementing that FROM-clause term, together with
** dependencies and cost estimates for using the chosen algorithm.
**
** Query planning consists of building up a collection of these WhereLoop
** objects, then computing a particular sequence of WhereLoop objects, with
** one WhereLoop object per FROM clause term, that satisfy all dependencies
** and that minimize the overall cost.
*/
struct WhereLoop {
  Bitmask prereq;       /* Bitmask of other loops that must run first */
  Bitmask maskSelf;     /* Bitmask identifying table iTab */
#ifdef SQLITE_DEBUG
  char cId;             /* Symbolic ID of this loop for debugging use */
#endif
  u8 iTab;              /* Position in FROM clause of table for this loop */
  u8 iSortIdx;          /* Sorting index number.  0==None */
  LogEst rSetup;        /* One-time setup cost (ex: create transient index) */
  LogEst rRun;          /* Cost of running each loop */
  LogEst nOut;          /* Estimated number of output rows */
  union {
    struct {               /* Information for internal btree tables */
      u16 nEq;               /* Number of equality constraints */
      u16 nBtm;              /* Size of BTM vector */
      u16 nTop;              /* Size of TOP vector */
      u16 nDistinctCol;      /* Index columns used to sort for DISTINCT */
      Index *pIndex;         /* Index used, or NULL */
    } btree;
    struct {               /* Information for virtual tables */
      int idxNum;            /* Index number */
      u32 needFree : 1;      /* True if sqlite3_free(idxStr) is needed */
      u32 bOmitOffset : 1;   /* True to let virtual table handle offset */
      i8 isOrdered;          /* True if satisfies ORDER BY */
      u16 omitMask;          /* Terms that may be omitted */
      char *idxStr;          /* Index identifier string */
      u32 mHandleIn;         /* Terms to handle as IN(...) instead of == */
    } vtab;
  } u;
  u32 wsFlags;          /* WHERE_* flags describing the plan */
  u16 nLTerm;           /* Number of entries in aLTerm[] */
  u16 nSkip;            /* Number of NULL aLTerm[] entries */
  /**** whereLoopXfer() copies fields above ***********************/
# define WHERE_LOOP_XFER_SZ offsetof(WhereLoop,nLSlot)
  u16 nLSlot;           /* Number of slots allocated for aLTerm[] */
  WhereTerm **aLTerm;   /* WhereTerms used */
  WhereLoop *pNextLoop; /* Next WhereLoop object in the WhereClause */
  WhereTerm *aLTermSpace[3];  /* Initial aLTerm[] space */
};

/* This object holds the prerequisites and the cost of running a
** subquery on one operand of an OR operator in the WHERE clause.
** See WhereOrSet for additional information
*/
struct WhereOrCost {
  Bitmask prereq;     /* Prerequisites */
  LogEst rRun;        /* Cost of running this subquery */
  LogEst nOut;        /* Number of outputs for this subquery */
};

/* The WhereOrSet object holds a set of possible WhereOrCosts that
** correspond to the subquery(s) of OR-clause processing.  Only the
** best N_OR_COST elements are retained.
*/
#define N_OR_COST 3
struct WhereOrSet {
  u16 n;                      /* Number of valid a[] entries */
  WhereOrCost a[N_OR_COST];   /* Set of best costs */
};

/*
** Each instance of this object holds a sequence of WhereLoop objects
** that implement some or all of a query plan.
**
** Think of each WhereLoop object as a node in a graph with arcs
** showing dependencies and costs for travelling between nodes.  (That is
** not a completely accurate description because WhereLoop costs are a
** vector, not a scalar, and because dependencies are many-to-one, not
** one-to-one as are graph nodes.  But it is a useful visualization aid.)
** Then a WherePath object is a path through the graph that visits some
** or all of the WhereLoop objects once.
**
** The "solver" works by creating the N best WherePath objects of length
** 1.  Then using those as a basis to compute the N best WherePath objects
** of length 2.  And so forth until the length of WherePaths equals the
** number of nodes in the FROM clause.  The best (lowest cost) WherePath
** at the end is the chosen query plan.
*/
struct WherePath {
  Bitmask maskLoop;     /* Bitmask of all WhereLoop objects in this path */
  Bitmask revLoop;      /* aLoop[]s that should be reversed for ORDER BY */
  LogEst nRow;          /* Estimated number of rows generated by this path */
  LogEst rCost;         /* Total cost of this path */
  LogEst rUnsorted;     /* Total cost of this path ignoring sorting costs */
  i8 isOrdered;         /* No. of ORDER BY terms satisfied. -1 for unknown */
  WhereLoop **aLoop;    /* Array of WhereLoop objects implementing this path */
};

/*
** The query generator uses an array of instances of this structure to
** help it analyze the subexpressions of the WHERE clause.  Each WHERE
** clause subexpression is separated from the others by AND operators,
** usually, or sometimes subexpressions separated by OR.
**
** All WhereTerms are collected into a single WhereClause structure.
** The following identity holds:
**
**        WhereTerm.pWC->a[WhereTerm.idx] == WhereTerm
**
** When a term is of the form:
**
**              X <op> <expr>
**
** where X is a column name and <op> is one of certain operators,
** then WhereTerm.leftCursor and WhereTerm.u.leftColumn record the
** cursor number and column number for X.  WhereTerm.eOperator records
** the <op> using a bitmask encoding defined by WO_xxx below.  The
** use of a bitmask encoding for the operator allows us to search
** quickly for terms that match any of several different operators.
**
** A WhereTerm might also be two or more subterms connected by OR:
**
**         (t1.X <op> <expr>) OR (t1.Y <op> <expr>) OR ....
**
** In this second case, wtFlag has the TERM_ORINFO bit set and eOperator==WO_OR
** and the WhereTerm.u.pOrInfo field points to auxiliary information that
** is collected about the OR clause.
**
** If a term in the WHERE clause does not match either of the two previous
** categories, then eOperator==0.  The WhereTerm.pExpr field is still set
** to the original subexpression content and wtFlags is set up appropriately
** but no other fields in the WhereTerm object are meaningful.
**
** When eOperator!=0, prereqRight and prereqAll record sets of cursor numbers,
** but they do so indirectly.  A single WhereMaskSet structure translates
** cursor number into bits and the translated bit is stored in the prereq
** fields.  The translation is used in order to maximize the number of
** bits that will fit in a Bitmask.  The VDBE cursor numbers might be
** spread out over the non-negative integers.  For example, the cursor
** numbers might be 3, 8, 9, 10, 20, 23, 41, and 45.  The WhereMaskSet
** translates these sparse cursor numbers into consecutive integers
** beginning with 0 in order to make the best possible use of the available
** bits in the Bitmask.  So, in the example above, the cursor numbers
** would be mapped into integers 0 through 7.
**
** The number of terms in a join is limited by the number of bits
** in prereqRight and prereqAll.  The default is 64 bits, hence SQLite
** is only able to process joins with 64 or fewer tables.
*/
struct WhereTerm {
  Expr *pExpr;            /* Pointer to the subexpression that is this term */
  WhereClause *pWC;       /* The clause this term is part of */
  LogEst truthProb;       /* Probability of truth for this expression */
  u16 wtFlags;            /* TERM_xxx bit flags.  See below */
  u16 eOperator;          /* A WO_xx value describing <op> */
  u8 nChild;              /* Number of children that must disable us */
  u8 eMatchOp;            /* Op for vtab MATCH/LIKE/GLOB/REGEXP terms */
  int iParent;            /* Disable pWC->a[iParent] when this term disabled */
  int leftCursor;         /* Cursor number of X in "X <op> <expr>" */
  union {
    struct {
      int leftColumn;         /* Column number of X in "X <op> <expr>" */
      int iField;             /* Field in (?,?,?) IN (SELECT...) vector */
    } x;                    /* Opcode other than OP_OR or OP_AND */
    WhereOrInfo *pOrInfo;   /* Extra information if (eOperator & WO_OR)!=0 */
    WhereAndInfo *pAndInfo; /* Extra information if (eOperator& WO_AND)!=0 */
  } u;
  Bitmask prereqRight;    /* Bitmask of tables used by pExpr->pRight */
  Bitmask prereqAll;      /* Bitmask of tables referenced by pExpr */
};

/*
** Allowed values of WhereTerm.wtFlags
*/
#define TERM_DYNAMIC    0x0001 /* Need to call sqlite3ExprDelete(db, pExpr) */
#define TERM_VIRTUAL    0x0002 /* Added by the optimizer.  Do not code */
#define TERM_CODED      0x0004 /* This term is already coded */
#define TERM_COPIED     0x0008 /* Has a child */
#define TERM_ORINFO     0x0010 /* Need to free the WhereTerm.u.pOrInfo object */
#define TERM_ANDINFO    0x0020 /* Need to free the WhereTerm.u.pAndInfo obj */
#define TERM_OK         0x0040 /* Used during OR-clause processing */
#define TERM_VNULL      0x0080 /* Manufactured x>NULL or x<=NULL term */
#define TERM_LIKEOPT    0x0100 /* Virtual terms from the LIKE optimization */
#define TERM_LIKECOND   0x0200 /* Conditionally this LIKE operator term */
#define TERM_LIKE       0x0400 /* The original LIKE operator */
#define TERM_IS         0x0800 /* Term.pExpr is an IS operator */
#define TERM_VARSELECT  0x1000 /* Term.pExpr contains a correlated sub-query */
#define TERM_HEURTRUTH  0x2000 /* Heuristic truthProb used */
#ifdef SQLITE_ENABLE_STAT4
#  define TERM_HIGHTRUTH  0x4000 /* Term excludes few rows */
#else
#  define TERM_HIGHTRUTH  0      /* Only used with STAT4 */
#endif
#define TERM_SLICE      0x8000 /* One slice of a row-value/vector comparison */

/*
** An instance of the WhereScan object is used as an iterator for locating
** terms in the WHERE clause that are useful to the query planner.
*/
struct WhereScan {
  WhereClause *pOrigWC;      /* Original, innermost WhereClause */
  WhereClause *pWC;          /* WhereClause currently being scanned */
  const char *zCollName;     /* Required collating sequence, if not NULL */
  Expr *pIdxExpr;            /* Search for this index expression */
  int k;                     /* Resume scanning at this->pWC->a[this->k] */
  u32 opMask;                /* Acceptable operators */
  char idxaff;               /* Must match this affinity, if zCollName!=NULL */
  unsigned char iEquiv;      /* Current slot in aiCur[] and aiColumn[] */
  unsigned char nEquiv;      /* Number of entries in aiCur[] and aiColumn[] */
  int aiCur[11];             /* Cursors in the equivalence class */
  i16 aiColumn[11];          /* Corresponding column number in the eq-class */
};

/*
** An instance of the following structure holds all information about a
** WHERE clause.  Mostly this is a container for one or more WhereTerms.
**
** Explanation of pOuter:  For a WHERE clause of the form
**
**           a AND ((b AND c) OR (d AND e)) AND f
**
** There are separate WhereClause objects for the whole clause and for
** the subclauses "(b AND c)" and "(d AND e)".  The pOuter field of the
** subclauses points to the WhereClause object for the whole clause.
*/
struct WhereClause {
  WhereInfo *pWInfo;       /* WHERE clause processing context */
  WhereClause *pOuter;     /* Outer conjunction */
  u8 op;                   /* Split operator.  TK_AND or TK_OR */
  u8 hasOr;                /* True if any a[].eOperator is WO_OR */
  int nTerm;               /* Number of terms */
  int nSlot;               /* Number of entries in a[] */
  int nBase;               /* Number of terms through the last non-Virtual */
  WhereTerm *a;            /* Each a[] describes a term of the WHERE clause */
#if defined(SQLITE_SMALL_STACK)
  WhereTerm aStatic[1];    /* Initial static space for a[] */
#else
  WhereTerm aStatic[8];    /* Initial static space for a[] */
#endif
};
