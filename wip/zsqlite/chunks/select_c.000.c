/*
** 2001 September 15
**
** The author disclaims copyright to this source code.  In place of
** a legal notice, here is a blessing:
**
**    May you do good and not evil.
**    May you find forgiveness for yourself and forgive others.
**    May you share freely, never taking more than you give.
**
*************************************************************************
** This file contains C code routines that are called by the parser
** to handle SELECT statements in SQLite.
*/
/* #include "sqliteInt.h" */

/*
** An instance of the following object is used to record information about
** how to process the DISTINCT keyword, to simplify passing that information
** into the selectInnerLoop() routine.
*/
typedef struct DistinctCtx DistinctCtx;
struct DistinctCtx {
  u8 isTnct;      /* 0: Not distinct. 1: DISTICT  2: DISTINCT and ORDER BY */
  u8 eTnctType;   /* One of the WHERE_DISTINCT_* operators */
  int tabTnct;    /* Ephemeral table used for DISTINCT processing */
  int addrTnct;   /* Address of OP_OpenEphemeral opcode for tabTnct */
};

/*
** An instance of the following object is used to record information about
** the ORDER BY (or GROUP BY) clause of query is being coded.
**
** The aDefer[] array is used by the sorter-references optimization. For
** example, assuming there is no index that can be used for the ORDER BY,
** for the query:
**
**     SELECT a, bigblob FROM t1 ORDER BY a LIMIT 10;
**
** it may be more efficient to add just the "a" values to the sorter, and
** retrieve the associated "bigblob" values directly from table t1 as the
** 10 smallest "a" values are extracted from the sorter.
**
** When the sorter-reference optimization is used, there is one entry in the
** aDefer[] array for each database table that may be read as values are
** extracted from the sorter.
*/
typedef struct SortCtx SortCtx;
struct SortCtx {
  ExprList *pOrderBy;   /* The ORDER BY (or GROUP BY clause) */
  int nOBSat;           /* Number of ORDER BY terms satisfied by indices */
  int iECursor;         /* Cursor number for the sorter */
  int regReturn;        /* Register holding block-output return address */
  int labelBkOut;       /* Start label for the block-output subroutine */
  int addrSortIndex;    /* Address of the OP_SorterOpen or OP_OpenEphemeral */
  int labelDone;        /* Jump here when done, ex: LIMIT reached */
  int labelOBLopt;      /* Jump here when sorter is full */
  u8 sortFlags;         /* Zero or more SORTFLAG_* bits */
#ifdef SQLITE_ENABLE_SORTER_REFERENCES
  u8 nDefer;            /* Number of valid entries in aDefer[] */
  struct DeferredCsr {
    Table *pTab;        /* Table definition */
    int iCsr;           /* Cursor number for table */
    int nKey;           /* Number of PK columns for table pTab (>=1) */
  } aDefer[4];
#endif
  struct RowLoadInfo *pDeferredRowLoad;  /* Deferred row loading info or NULL */
#ifdef SQLITE_ENABLE_STMT_SCANSTATUS
  int addrPush;         /* First instruction to push data into sorter */
  int addrPushEnd;      /* Last instruction that pushes data into sorter */
#endif
};
#define SORTFLAG_UseSorter  0x01   /* Use SorterOpen instead of OpenEphemeral */

/*
** Delete all the content of a Select structure.  Deallocate the structure
** itself depending on the value of bFree
**
** If bFree==1, call sqlite3DbFree() on the p object.
** If bFree==0, Leave the first Select object unfreed
*/
static void clearSelect(sqlite3 *db, Select *p, int bFree){
  assert( db!=0 );
  while( p ){
    Select *pPrior = p->pPrior;
    sqlite3ExprListDelete(db, p->pEList);
    sqlite3SrcListDelete(db, p->pSrc);
    sqlite3ExprDelete(db, p->pWhere);
    sqlite3ExprListDelete(db, p->pGroupBy);
    sqlite3ExprDelete(db, p->pHaving);
    sqlite3ExprListDelete(db, p->pOrderBy);
    sqlite3ExprDelete(db, p->pLimit);
    if( OK_IF_ALWAYS_TRUE(p->pWith) ) sqlite3WithDelete(db, p->pWith);
#ifndef SQLITE_OMIT_WINDOWFUNC
    if( OK_IF_ALWAYS_TRUE(p->pWinDefn) ){
      sqlite3WindowListDelete(db, p->pWinDefn);
    }
    while( p->pWin ){
      assert( p->pWin->ppThis==&p->pWin );
      sqlite3WindowUnlinkFromSelect(p->pWin);
    }
#endif
    if( bFree ) sqlite3DbNNFreeNN(db, p);
    p = pPrior;
    bFree = 1;
  }
}

/*
** Initialize a SelectDest structure.
*/
SQLITE_PRIVATE void sqlite3SelectDestInit(SelectDest *pDest, int eDest, int iParm){
  pDest->eDest = (u8)eDest;
  pDest->iSDParm = iParm;
  pDest->iSDParm2 = 0;
  pDest->zAffSdst = 0;
  pDest->iSdst = 0;
  pDest->nSdst = 0;
}


/*
** Allocate a new Select structure and return a pointer to that
** structure.
*/
SQLITE_PRIVATE Select *sqlite3SelectNew(
  Parse *pParse,        /* Parsing context */
  ExprList *pEList,     /* which columns to include in the result */
  SrcList *pSrc,        /* the FROM clause -- which tables to scan */
  Expr *pWhere,         /* the WHERE clause */
  ExprList *pGroupBy,   /* the GROUP BY clause */
  Expr *pHaving,        /* the HAVING clause */
  ExprList *pOrderBy,   /* the ORDER BY clause */
  u32 selFlags,         /* Flag parameters, such as SF_Distinct */
  Expr *pLimit          /* LIMIT value.  NULL means not used */
){
  Select *pNew, *pAllocated;
  Select standin;
  pAllocated = pNew = sqlite3DbMallocRawNN(pParse->db, sizeof(*pNew) );
  if( pNew==0 ){
    assert( pParse->db->mallocFailed );
    pNew = &standin;
  }
  if( pEList==0 ){
    pEList = sqlite3ExprListAppend(pParse, 0,
                                   sqlite3Expr(pParse->db,TK_ASTERISK,0));
  }
  pNew->pEList = pEList;
  pNew->op = TK_SELECT;
  pNew->selFlags = selFlags;
  pNew->iLimit = 0;
  pNew->iOffset = 0;
  pNew->selId = ++pParse->nSelect;
  pNew->addrOpenEphm[0] = -1;
  pNew->addrOpenEphm[1] = -1;
  pNew->nSelectRow = 0;
  if( pSrc==0 ) pSrc = sqlite3DbMallocZero(pParse->db, sizeof(*pSrc));
  pNew->pSrc = pSrc;
  pNew->pWhere = pWhere;
  pNew->pGroupBy = pGroupBy;
  pNew->pHaving = pHaving;
  pNew->pOrderBy = pOrderBy;
  pNew->pPrior = 0;
  pNew->pNext = 0;
  pNew->pLimit = pLimit;
  pNew->pWith = 0;
#ifndef SQLITE_OMIT_WINDOWFUNC
  pNew->pWin = 0;
  pNew->pWinDefn = 0;
#endif
  if( pParse->db->mallocFailed ) {
    clearSelect(pParse->db, pNew, pNew!=&standin);
    pAllocated = 0;
  }else{
    assert( pNew->pSrc!=0 || pParse->nErr>0 );
  }
  return pAllocated;
}


/*
** Delete the given Select structure and all of its substructures.
*/
SQLITE_PRIVATE void sqlite3SelectDelete(sqlite3 *db, Select *p){
  if( OK_IF_ALWAYS_TRUE(p) ) clearSelect(db, p, 1);
}
SQLITE_PRIVATE void sqlite3SelectDeleteGeneric(sqlite3 *db, void *p){
  if( ALWAYS(p) ) clearSelect(db, (Select*)p, 1);
}

/*
** Return a pointer to the right-most SELECT statement in a compound.
*/
static Select *findRightmost(Select *p){
  while( p->pNext ) p = p->pNext;
  return p;
}

/*
** Given 1 to 3 identifiers preceding the JOIN keyword, determine the
** type of join.  Return an integer constant that expresses that type
** in terms of the following bit values:
**
**     JT_INNER
**     JT_CROSS
**     JT_OUTER
**     JT_NATURAL
**     JT_LEFT
**     JT_RIGHT
**
** A full outer join is the combination of JT_LEFT and JT_RIGHT.
**
** If an illegal or unsupported join type is seen, then still return
** a join type, but put an error in the pParse structure.
**
** These are the valid join types:
**
**
**      pA       pB       pC               Return Value
**     -------  -----    -----             ------------
**     CROSS      -        -                 JT_CROSS
**     INNER      -        -                 JT_INNER
**     LEFT       -        -                 JT_LEFT|JT_OUTER
**     LEFT     OUTER      -                 JT_LEFT|JT_OUTER
**     RIGHT      -        -                 JT_RIGHT|JT_OUTER
**     RIGHT    OUTER      -                 JT_RIGHT|JT_OUTER
**     FULL       -        -                 JT_LEFT|JT_RIGHT|JT_OUTER
**     FULL     OUTER      -                 JT_LEFT|JT_RIGHT|JT_OUTER
**     NATURAL  INNER      -                 JT_NATURAL|JT_INNER
**     NATURAL  LEFT       -                 JT_NATURAL|JT_LEFT|JT_OUTER
**     NATURAL  LEFT     OUTER               JT_NATURAL|JT_LEFT|JT_OUTER
**     NATURAL  RIGHT      -                 JT_NATURAL|JT_RIGHT|JT_OUTER
**     NATURAL  RIGHT    OUTER               JT_NATURAL|JT_RIGHT|JT_OUTER
**     NATURAL  FULL       -                 JT_NATURAL|JT_LEFT|JT_RIGHT
**     NATURAL  FULL     OUTER               JT_NATRUAL|JT_LEFT|JT_RIGHT
**
** To preserve historical compatibly, SQLite also accepts a variety
** of other non-standard and in many cases nonsensical join types.
** This routine makes as much sense at it can from the nonsense join
** type and returns a result.  Examples of accepted nonsense join types
** include but are not limited to:
**
**          INNER CROSS JOIN        ->   same as JOIN
**          NATURAL CROSS JOIN      ->   same as NATURAL JOIN
**          OUTER LEFT JOIN         ->   same as LEFT JOIN
**          LEFT NATURAL JOIN       ->   same as NATURAL LEFT JOIN
**          LEFT RIGHT JOIN         ->   same as FULL JOIN
**          RIGHT OUTER FULL JOIN   ->   same as FULL JOIN
**          CROSS CROSS CROSS JOIN  ->   same as JOIN
**
** The only restrictions on the join type name are:
**
**    *   "INNER" cannot appear together with "OUTER", "LEFT", "RIGHT",
**        or "FULL".
**
**    *   "CROSS" cannot appear together with "OUTER", "LEFT", "RIGHT,
**        or "FULL".
**
**    *   If "OUTER" is present then there must also be one of
**        "LEFT", "RIGHT", or "FULL"
*/
SQLITE_PRIVATE int sqlite3JoinType(Parse *pParse, Token *pA, Token *pB, Token *pC){
  int jointype = 0;
  Token *apAll[3];
  Token *p;
                             /*   0123456789 123456789 123456789 123 */
  static const char zKeyText[] = "naturaleftouterightfullinnercross";
  static const struct {
    u8 i;        /* Beginning of keyword text in zKeyText[] */
    u8 nChar;    /* Length of the keyword in characters */
    u8 code;     /* Join type mask */
  } aKeyword[] = {
    /* (0) natural */ { 0,  7, JT_NATURAL                },
    /* (1) left    */ { 6,  4, JT_LEFT|JT_OUTER          },
    /* (2) outer   */ { 10, 5, JT_OUTER                  },
    /* (3) right   */ { 14, 5, JT_RIGHT|JT_OUTER         },
    /* (4) full    */ { 19, 4, JT_LEFT|JT_RIGHT|JT_OUTER },
    /* (5) inner   */ { 23, 5, JT_INNER                  },
    /* (6) cross   */ { 28, 5, JT_INNER|JT_CROSS         },
  };
  int i, j;
  apAll[0] = pA;
  apAll[1] = pB;
  apAll[2] = pC;
  for(i=0; i<3 && apAll[i]; i++){
    p = apAll[i];
    for(j=0; j<ArraySize(aKeyword); j++){
      if( p->n==aKeyword[j].nChar
          && sqlite3StrNICmp((char*)p->z, &zKeyText[aKeyword[j].i], p->n)==0 ){
        jointype |= aKeyword[j].code;
        break;
      }
    }
    testcase( j==0 || j==1 || j==2 || j==3 || j==4 || j==5 || j==6 );
    if( j>=ArraySize(aKeyword) ){
      jointype |= JT_ERROR;
      break;
    }
  }
  if(
     (jointype & (JT_INNER|JT_OUTER))==(JT_INNER|JT_OUTER) ||
     (jointype & JT_ERROR)!=0 ||
     (jointype & (JT_OUTER|JT_LEFT|JT_RIGHT))==JT_OUTER
  ){
    const char *zSp1 = " ";
    const char *zSp2 = " ";
    if( pB==0 ){ zSp1++; }
    if( pC==0 ){ zSp2++; }
    sqlite3ErrorMsg(pParse, "unknown join type: "
       "%T%s%T%s%T", pA, zSp1, pB, zSp2, pC);
    jointype = JT_INNER;
  }
  return jointype;
}

/*
** Return the index of a column in a table.  Return -1 if the column
** is not contained in the table.
*/
SQLITE_PRIVATE int sqlite3ColumnIndex(Table *pTab, const char *zCol){
  int i;
  u8 h = sqlite3StrIHash(zCol);
  Column *pCol;
  for(pCol=pTab->aCol, i=0; i<pTab->nCol; pCol++, i++){
    if( pCol->hName==h && sqlite3StrICmp(pCol->zCnName, zCol)==0 ) return i;
  }
  return -1;
}

/*
** Mark a subquery result column as having been used.
*/
SQLITE_PRIVATE void sqlite3SrcItemColumnUsed(SrcItem *pItem, int iCol){
  assert( pItem!=0 );
  assert( (int)pItem->fg.isNestedFrom == IsNestedFrom(pItem->pSelect) );
  if( pItem->fg.isNestedFrom ){
    ExprList *pResults;
    assert( pItem->pSelect!=0 );
    pResults = pItem->pSelect->pEList;
    assert( pResults!=0 );
    assert( iCol>=0 && iCol<pResults->nExpr );
    pResults->a[iCol].fg.bUsed = 1;
  }
}

/*
** Search the tables iStart..iEnd (inclusive) in pSrc, looking for a
** table that has a column named zCol.  The search is left-to-right.
** The first match found is returned.
**
** When found, set *piTab and *piCol to the table index and column index
** of the matching column and return TRUE.
**
** If not found, return FALSE.
*/
static int tableAndColumnIndex(
  SrcList *pSrc,       /* Array of tables to search */
  int iStart,          /* First member of pSrc->a[] to check */
  int iEnd,            /* Last member of pSrc->a[] to check */
  const char *zCol,    /* Name of the column we are looking for */
  int *piTab,          /* Write index of pSrc->a[] here */
  int *piCol,          /* Write index of pSrc->a[*piTab].pTab->aCol[] here */
  int bIgnoreHidden    /* Ignore hidden columns */
){
  int i;               /* For looping over tables in pSrc */
  int iCol;            /* Index of column matching zCol */

  assert( iEnd<pSrc->nSrc );
  assert( iStart>=0 );
  assert( (piTab==0)==(piCol==0) );  /* Both or neither are NULL */

  for(i=iStart; i<=iEnd; i++){
    iCol = sqlite3ColumnIndex(pSrc->a[i].pTab, zCol);
    if( iCol>=0
     && (bIgnoreHidden==0 || IsHiddenColumn(&pSrc->a[i].pTab->aCol[iCol])==0)
    ){
      if( piTab ){
        sqlite3SrcItemColumnUsed(&pSrc->a[i], iCol);
        *piTab = i;
        *piCol = iCol;
      }
      return 1;
    }
  }
  return 0;
}
