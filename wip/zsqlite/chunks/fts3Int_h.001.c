
#define FTS3_EVAL_FILTER    0
#define FTS3_EVAL_NEXT      1
#define FTS3_EVAL_MATCHINFO 2

/*
** The Fts3Cursor.eSearch member is always set to one of the following.
** Actualy, Fts3Cursor.eSearch can be greater than or equal to
** FTS3_FULLTEXT_SEARCH.  If so, then Fts3Cursor.eSearch - 2 is the index
** of the column to be searched.  For example, in
**
**     CREATE VIRTUAL TABLE ex1 USING fts3(a,b,c,d);
**     SELECT docid FROM ex1 WHERE b MATCH 'one two three';
**
** Because the LHS of the MATCH operator is 2nd column "b",
** Fts3Cursor.eSearch will be set to FTS3_FULLTEXT_SEARCH+1.  (+0 for a,
** +1 for b, +2 for c, +3 for d.)  If the LHS of MATCH were "ex1"
** indicating that all columns should be searched,
** then eSearch would be set to FTS3_FULLTEXT_SEARCH+4.
*/
#define FTS3_FULLSCAN_SEARCH 0    /* Linear scan of %_content table */
#define FTS3_DOCID_SEARCH    1    /* Lookup by rowid on %_content table */
#define FTS3_FULLTEXT_SEARCH 2    /* Full-text index search */

/*
** The lower 16-bits of the sqlite3_index_info.idxNum value set by
** the xBestIndex() method contains the Fts3Cursor.eSearch value described
** above. The upper 16-bits contain a combination of the following
** bits, used to describe extra constraints on full-text searches.
*/
#define FTS3_HAVE_LANGID    0x00010000      /* languageid=? */
#define FTS3_HAVE_DOCID_GE  0x00020000      /* docid>=? */
#define FTS3_HAVE_DOCID_LE  0x00040000      /* docid<=? */

struct Fts3Doclist {
  char *aAll;                    /* Array containing doclist (or NULL) */
  int nAll;                      /* Size of a[] in bytes */
  char *pNextDocid;              /* Pointer to next docid */

  sqlite3_int64 iDocid;          /* Current docid (if pList!=0) */
  int bFreeList;                 /* True if pList should be sqlite3_free()d */
  char *pList;                   /* Pointer to position list following iDocid */
  int nList;                     /* Length of position list */
};

/*
** A "phrase" is a sequence of one or more tokens that must match in
** sequence.  A single token is the base case and the most common case.
** For a sequence of tokens contained in double-quotes (i.e. "one two three")
** nToken will be the number of tokens in the string.
*/
struct Fts3PhraseToken {
  char *z;                        /* Text of the token */
  int n;                          /* Number of bytes in buffer z */
  int isPrefix;                   /* True if token ends with a "*" character */
  int bFirst;                     /* True if token must appear at position 0 */

  /* Variables above this point are populated when the expression is
  ** parsed (by code in fts3_expr.c). Below this point the variables are
  ** used when evaluating the expression. */
  Fts3DeferredToken *pDeferred;   /* Deferred token object for this token */
  Fts3MultiSegReader *pSegcsr;    /* Segment-reader for this token */
};

struct Fts3Phrase {
  /* Cache of doclist for this phrase. */
  Fts3Doclist doclist;
  int bIncr;                 /* True if doclist is loaded incrementally */
  int iDoclistToken;

  /* Used by sqlite3Fts3EvalPhrasePoslist() if this is a descendent of an
  ** OR condition.  */
  char *pOrPoslist;
  i64 iOrDocid;

  /* Variables below this point are populated by fts3_expr.c when parsing
  ** a MATCH expression. Everything above is part of the evaluation phase.
  */
  int nToken;                /* Number of tokens in the phrase */
  int iColumn;               /* Index of column this phrase must match */
  Fts3PhraseToken aToken[1]; /* One entry for each token in the phrase */
};

/*
** A tree of these objects forms the RHS of a MATCH operator.
**
** If Fts3Expr.eType is FTSQUERY_PHRASE and isLoaded is true, then aDoclist
** points to a malloced buffer, size nDoclist bytes, containing the results
** of this phrase query in FTS3 doclist format. As usual, the initial
** "Length" field found in doclists stored on disk is omitted from this
** buffer.
**
** Variable aMI is used only for FTSQUERY_NEAR nodes to store the global
** matchinfo data. If it is not NULL, it points to an array of size nCol*3,
** where nCol is the number of columns in the queried FTS table. The array
** is populated as follows:
**
**   aMI[iCol*3 + 0] = Undefined
**   aMI[iCol*3 + 1] = Number of occurrences
**   aMI[iCol*3 + 2] = Number of rows containing at least one instance
**
** The aMI array is allocated using sqlite3_malloc(). It should be freed
** when the expression node is.
*/
struct Fts3Expr {
  int eType;                 /* One of the FTSQUERY_XXX values defined below */
  int nNear;                 /* Valid if eType==FTSQUERY_NEAR */
  Fts3Expr *pParent;         /* pParent->pLeft==this or pParent->pRight==this */
  Fts3Expr *pLeft;           /* Left operand */
  Fts3Expr *pRight;          /* Right operand */
  Fts3Phrase *pPhrase;       /* Valid if eType==FTSQUERY_PHRASE */

  /* The following are used by the fts3_eval.c module. */
  sqlite3_int64 iDocid;      /* Current docid */
  u8 bEof;                   /* True this expression is at EOF already */
  u8 bStart;                 /* True if iDocid is valid */
  u8 bDeferred;              /* True if this expression is entirely deferred */

  /* The following are used by the fts3_snippet.c module. */
  int iPhrase;               /* Index of this phrase in matchinfo() results */
  u32 *aMI;                  /* See above */
};

/*
** Candidate values for Fts3Query.eType. Note that the order of the first
** four values is in order of precedence when parsing expressions. For
** example, the following:
**
**   "a OR b AND c NOT d NEAR e"
**
** is equivalent to:
**
**   "a OR (b AND (c NOT (d NEAR e)))"
*/
#define FTSQUERY_NEAR   1
#define FTSQUERY_NOT    2
#define FTSQUERY_AND    3
#define FTSQUERY_OR     4
#define FTSQUERY_PHRASE 5


/* fts3_write.c */
SQLITE_PRIVATE int sqlite3Fts3UpdateMethod(sqlite3_vtab*,int,sqlite3_value**,sqlite3_int64*);
SQLITE_PRIVATE int sqlite3Fts3PendingTermsFlush(Fts3Table *);
SQLITE_PRIVATE void sqlite3Fts3PendingTermsClear(Fts3Table *);
SQLITE_PRIVATE int sqlite3Fts3Optimize(Fts3Table *);
SQLITE_PRIVATE int sqlite3Fts3SegReaderNew(int, int, sqlite3_int64,
  sqlite3_int64, sqlite3_int64, const char *, int, Fts3SegReader**);
SQLITE_PRIVATE int sqlite3Fts3SegReaderPending(
  Fts3Table*,int,const char*,int,int,Fts3SegReader**);
SQLITE_PRIVATE void sqlite3Fts3SegReaderFree(Fts3SegReader *);
SQLITE_PRIVATE int sqlite3Fts3AllSegdirs(Fts3Table*, int, int, int, sqlite3_stmt **);
SQLITE_PRIVATE int sqlite3Fts3ReadBlock(Fts3Table*, sqlite3_int64, char **, int*, int*);

SQLITE_PRIVATE int sqlite3Fts3SelectDoctotal(Fts3Table *, sqlite3_stmt **);
SQLITE_PRIVATE int sqlite3Fts3SelectDocsize(Fts3Table *, sqlite3_int64, sqlite3_stmt **);

#ifndef SQLITE_DISABLE_FTS4_DEFERRED
SQLITE_PRIVATE void sqlite3Fts3FreeDeferredTokens(Fts3Cursor *);
SQLITE_PRIVATE int sqlite3Fts3DeferToken(Fts3Cursor *, Fts3PhraseToken *, int);
SQLITE_PRIVATE int sqlite3Fts3CacheDeferredDoclists(Fts3Cursor *);
SQLITE_PRIVATE void sqlite3Fts3FreeDeferredDoclists(Fts3Cursor *);
SQLITE_PRIVATE int sqlite3Fts3DeferredTokenList(Fts3DeferredToken *, char **, int *);
#else
# define sqlite3Fts3FreeDeferredTokens(x)
# define sqlite3Fts3DeferToken(x,y,z) SQLITE_OK
# define sqlite3Fts3CacheDeferredDoclists(x) SQLITE_OK
# define sqlite3Fts3FreeDeferredDoclists(x)
# define sqlite3Fts3DeferredTokenList(x,y,z) SQLITE_OK
#endif

SQLITE_PRIVATE void sqlite3Fts3SegmentsClose(Fts3Table *);
SQLITE_PRIVATE int sqlite3Fts3MaxLevel(Fts3Table *, int *);

/* Special values interpreted by sqlite3SegReaderCursor() */
#define FTS3_SEGCURSOR_PENDING        -1
#define FTS3_SEGCURSOR_ALL            -2

SQLITE_PRIVATE int sqlite3Fts3SegReaderStart(Fts3Table*, Fts3MultiSegReader*, Fts3SegFilter*);
SQLITE_PRIVATE int sqlite3Fts3SegReaderStep(Fts3Table *, Fts3MultiSegReader *);
SQLITE_PRIVATE void sqlite3Fts3SegReaderFinish(Fts3MultiSegReader *);

SQLITE_PRIVATE int sqlite3Fts3SegReaderCursor(Fts3Table *,
    int, int, int, const char *, int, int, int, Fts3MultiSegReader *);

/* Flags allowed as part of the 4th argument to SegmentReaderIterate() */
#define FTS3_SEGMENT_REQUIRE_POS   0x00000001
#define FTS3_SEGMENT_IGNORE_EMPTY  0x00000002
#define FTS3_SEGMENT_COLUMN_FILTER 0x00000004
#define FTS3_SEGMENT_PREFIX        0x00000008
#define FTS3_SEGMENT_SCAN          0x00000010
#define FTS3_SEGMENT_FIRST         0x00000020

/* Type passed as 4th argument to SegmentReaderIterate() */
struct Fts3SegFilter {
  const char *zTerm;
  int nTerm;
  int iCol;
  int flags;
};

struct Fts3MultiSegReader {
  /* Used internally by sqlite3Fts3SegReaderXXX() calls */
  Fts3SegReader **apSegment;      /* Array of Fts3SegReader objects */
  int nSegment;                   /* Size of apSegment array */
  int nAdvance;                   /* How many seg-readers to advance */
  Fts3SegFilter *pFilter;         /* Pointer to filter object */
  char *aBuffer;                  /* Buffer to merge doclists in */
  i64 nBuffer;                    /* Allocated size of aBuffer[] in bytes */

  int iColFilter;                 /* If >=0, filter for this column */
  int bRestart;

  /* Used by fts3.c only. */
  int nCost;                      /* Cost of running iterator */
  int bLookup;                    /* True if a lookup of a single entry. */

  /* Output values. Valid only after Fts3SegReaderStep() returns SQLITE_ROW. */
  char *zTerm;                    /* Pointer to term buffer */
  int nTerm;                      /* Size of zTerm in bytes */
  char *aDoclist;                 /* Pointer to doclist buffer */
  int nDoclist;                   /* Size of aDoclist[] in bytes */
};

SQLITE_PRIVATE int sqlite3Fts3Incrmerge(Fts3Table*,int,int);

#define fts3GetVarint32(p, piVal) (                                           \
  (*(u8*)(p)&0x80) ? sqlite3Fts3GetVarint32(p, piVal) : (*piVal=*(u8*)(p), 1) \
)

/* fts3.c */
SQLITE_PRIVATE void sqlite3Fts3ErrMsg(char**,const char*,...);
SQLITE_PRIVATE int sqlite3Fts3PutVarint(char *, sqlite3_int64);
SQLITE_PRIVATE int sqlite3Fts3GetVarint(const char *, sqlite_int64 *);
SQLITE_PRIVATE int sqlite3Fts3GetVarintU(const char *, sqlite_uint64 *);
SQLITE_PRIVATE int sqlite3Fts3GetVarintBounded(const char*,const char*,sqlite3_int64*);
SQLITE_PRIVATE int sqlite3Fts3GetVarint32(const char *, int *);
SQLITE_PRIVATE int sqlite3Fts3VarintLen(sqlite3_uint64);
SQLITE_PRIVATE void sqlite3Fts3Dequote(char *);
SQLITE_PRIVATE void sqlite3Fts3DoclistPrev(int,char*,int,char**,sqlite3_int64*,int*,u8*);
SQLITE_PRIVATE int sqlite3Fts3EvalPhraseStats(Fts3Cursor *, Fts3Expr *, u32 *);
SQLITE_PRIVATE int sqlite3Fts3FirstFilter(sqlite3_int64, char *, int, char *);
SQLITE_PRIVATE void sqlite3Fts3CreateStatTable(int*, Fts3Table*);
SQLITE_PRIVATE int sqlite3Fts3EvalTestDeferred(Fts3Cursor *pCsr, int *pRc);
SQLITE_PRIVATE int sqlite3Fts3ReadInt(const char *z, int *pnOut);

/* fts3_tokenizer.c */
SQLITE_PRIVATE const char *sqlite3Fts3NextToken(const char *, int *);
SQLITE_PRIVATE int sqlite3Fts3InitHashTable(sqlite3 *, Fts3Hash *, const char *);
SQLITE_PRIVATE int sqlite3Fts3InitTokenizer(Fts3Hash *pHash, const char *,
    sqlite3_tokenizer **, char **
);
SQLITE_PRIVATE int sqlite3Fts3IsIdChar(char);

/* fts3_snippet.c */
SQLITE_PRIVATE void sqlite3Fts3Offsets(sqlite3_context*, Fts3Cursor*);
SQLITE_PRIVATE void sqlite3Fts3Snippet(sqlite3_context *, Fts3Cursor *, const char *,
  const char *, const char *, int, int
);
SQLITE_PRIVATE void sqlite3Fts3Matchinfo(sqlite3_context *, Fts3Cursor *, const char *);
SQLITE_PRIVATE void sqlite3Fts3MIBufferFree(MatchinfoBuffer *p);

/* fts3_expr.c */
SQLITE_PRIVATE int sqlite3Fts3ExprParse(sqlite3_tokenizer *, int,
  char **, int, int, int, const char *, int, Fts3Expr **, char **
);
SQLITE_PRIVATE void sqlite3Fts3ExprFree(Fts3Expr *);
#ifdef SQLITE_TEST
SQLITE_PRIVATE int sqlite3Fts3ExprInitTestInterface(sqlite3 *db, Fts3Hash*);
SQLITE_PRIVATE int sqlite3Fts3InitTerm(sqlite3 *db);
#endif
SQLITE_PRIVATE void *sqlite3Fts3MallocZero(i64 nByte);

SQLITE_PRIVATE int sqlite3Fts3OpenTokenizer(sqlite3_tokenizer *, int, const char *, int,
  sqlite3_tokenizer_cursor **
);

/* fts3_aux.c */
SQLITE_PRIVATE int sqlite3Fts3InitAux(sqlite3 *db);

SQLITE_PRIVATE void sqlite3Fts3EvalPhraseCleanup(Fts3Phrase *);

SQLITE_PRIVATE int sqlite3Fts3MsrIncrStart(
    Fts3Table*, Fts3MultiSegReader*, int, const char*, int);
SQLITE_PRIVATE int sqlite3Fts3MsrIncrNext(
    Fts3Table *, Fts3MultiSegReader *, sqlite3_int64 *, char **, int *);
SQLITE_PRIVATE int sqlite3Fts3EvalPhrasePoslist(Fts3Cursor *, Fts3Expr *, int iCol, char **);
SQLITE_PRIVATE int sqlite3Fts3MsrOvfl(Fts3Cursor *, Fts3MultiSegReader *, int *);
SQLITE_PRIVATE int sqlite3Fts3MsrIncrRestart(Fts3MultiSegReader *pCsr);

/* fts3_tokenize_vtab.c */
SQLITE_PRIVATE int sqlite3Fts3InitTok(sqlite3*, Fts3Hash *, void(*xDestroy)(void*));

/* fts3_unicode2.c (functions generated by parsing unicode text files) */
#ifndef SQLITE_DISABLE_FTS3_UNICODE
SQLITE_PRIVATE int sqlite3FtsUnicodeFold(int, int);
SQLITE_PRIVATE int sqlite3FtsUnicodeIsalnum(int);
SQLITE_PRIVATE int sqlite3FtsUnicodeIsdiacritic(int);
#endif

SQLITE_PRIVATE int sqlite3Fts3ExprIterate(Fts3Expr*, int (*x)(Fts3Expr*,int,void*), void*);

SQLITE_PRIVATE int sqlite3Fts3IntegrityCheck(Fts3Table *p, int *pbOk);

#endif /* !SQLITE_CORE || SQLITE_ENABLE_FTS3 */
#endif /* _FTSINT_H */

