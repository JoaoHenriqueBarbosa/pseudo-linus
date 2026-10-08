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
** This file contains routines used for analyzing expressions and
** for generating VDBE code that evaluates expressions in SQLite.
*/
/* #include "sqliteInt.h" */

/* Forward declarations */
static void exprCodeBetween(Parse*,Expr*,int,void(*)(Parse*,Expr*,int,int),int);
static int exprCodeVector(Parse *pParse, Expr *p, int *piToFree);

/*
** Return the affinity character for a single column of a table.
*/
SQLITE_PRIVATE char sqlite3TableColumnAffinity(const Table *pTab, int iCol){
  if( iCol<0 || NEVER(iCol>=pTab->nCol) ) return SQLITE_AFF_INTEGER;
  return pTab->aCol[iCol].affinity;
}

/*
** Return the 'affinity' of the expression pExpr if any.
**
** If pExpr is a column, a reference to a column via an 'AS' alias,
** or a sub-select with a column as the return value, then the
** affinity of that column is returned. Otherwise, 0x00 is returned,
** indicating no affinity for the expression.
**
** i.e. the WHERE clause expressions in the following statements all
** have an affinity:
**
** CREATE TABLE t1(a);
** SELECT * FROM t1 WHERE a;
** SELECT a AS b FROM t1 WHERE b;
** SELECT * FROM t1 WHERE (select a from t1);
*/
SQLITE_PRIVATE char sqlite3ExprAffinity(const Expr *pExpr){
  int op;
  op = pExpr->op;
  while( 1 /* exit-by-break */ ){
    if( op==TK_COLUMN || (op==TK_AGG_COLUMN && pExpr->y.pTab!=0) ){
      assert( ExprUseYTab(pExpr) );
      assert( pExpr->y.pTab!=0 );
      return sqlite3TableColumnAffinity(pExpr->y.pTab, pExpr->iColumn);
    }
    if( op==TK_SELECT ){
      assert( ExprUseXSelect(pExpr) );
      assert( pExpr->x.pSelect!=0 );
      assert( pExpr->x.pSelect->pEList!=0 );
      assert( pExpr->x.pSelect->pEList->a[0].pExpr!=0 );
      return sqlite3ExprAffinity(pExpr->x.pSelect->pEList->a[0].pExpr);
    }
#ifndef SQLITE_OMIT_CAST
    if( op==TK_CAST ){
      assert( !ExprHasProperty(pExpr, EP_IntValue) );
      return sqlite3AffinityType(pExpr->u.zToken, 0);
    }
#endif
    if( op==TK_SELECT_COLUMN ){
      assert( pExpr->pLeft!=0 && ExprUseXSelect(pExpr->pLeft) );
      assert( pExpr->iColumn < pExpr->iTable );
      assert( pExpr->iColumn >= 0 );
      assert( pExpr->iTable==pExpr->pLeft->x.pSelect->pEList->nExpr );
      return sqlite3ExprAffinity(
          pExpr->pLeft->x.pSelect->pEList->a[pExpr->iColumn].pExpr
      );
    }
    if( op==TK_VECTOR ){
      assert( ExprUseXList(pExpr) );
      return sqlite3ExprAffinity(pExpr->x.pList->a[0].pExpr);
    }
    if( ExprHasProperty(pExpr, EP_Skip|EP_IfNullRow) ){
      assert( pExpr->op==TK_COLLATE
           || pExpr->op==TK_IF_NULL_ROW
           || (pExpr->op==TK_REGISTER && pExpr->op2==TK_IF_NULL_ROW) );
      pExpr = pExpr->pLeft;
      op = pExpr->op;
      continue;
    }
    if( op!=TK_REGISTER || (op = pExpr->op2)==TK_REGISTER ) break;
  }
  return pExpr->affExpr;
}

/*
** Make a guess at all the possible datatypes of the result that could
** be returned by an expression.  Return a bitmask indicating the answer:
**
**     0x01         Numeric
**     0x02         Text
**     0x04         Blob
**
** If the expression must return NULL, then 0x00 is returned.
*/
SQLITE_PRIVATE int sqlite3ExprDataType(const Expr *pExpr){
  while( pExpr ){
    switch( pExpr->op ){
      case TK_COLLATE:
      case TK_IF_NULL_ROW:
      case TK_UPLUS:  {
        pExpr = pExpr->pLeft;
        break;
      }
      case TK_NULL: {
        pExpr = 0;
        break;
      }
      case TK_STRING: {
        return 0x02;
      }
      case TK_BLOB: {
        return 0x04;
      }
      case TK_CONCAT: {
        return 0x06;
      }
      case TK_VARIABLE:
      case TK_AGG_FUNCTION:
      case TK_FUNCTION: {
        return 0x07;
      }
      case TK_COLUMN:
      case TK_AGG_COLUMN:
      case TK_SELECT:
      case TK_CAST:
      case TK_SELECT_COLUMN:
      case TK_VECTOR:  {
        int aff = sqlite3ExprAffinity(pExpr);
        if( aff>=SQLITE_AFF_NUMERIC ) return 0x05;
        if( aff==SQLITE_AFF_TEXT )    return 0x06;
        return 0x07;
      }
      case TK_CASE: {
        int res = 0;
        int ii;
        ExprList *pList = pExpr->x.pList;
        assert( ExprUseXList(pExpr) && pList!=0 );
        assert( pList->nExpr > 0);
        for(ii=1; ii<pList->nExpr; ii+=2){
          res |= sqlite3ExprDataType(pList->a[ii].pExpr);
        }
        if( pList->nExpr % 2 ){
          res |= sqlite3ExprDataType(pList->a[pList->nExpr-1].pExpr);
        }
        return res;
      }
      default: {
        return 0x01;
      }
    } /* End of switch(op) */
  } /* End of while(pExpr) */
  return 0x00;
}

/*
** Set the collating sequence for expression pExpr to be the collating
** sequence named by pToken.   Return a pointer to a new Expr node that
** implements the COLLATE operator.
**
** If a memory allocation error occurs, that fact is recorded in pParse->db
** and the pExpr parameter is returned unchanged.
*/
SQLITE_PRIVATE Expr *sqlite3ExprAddCollateToken(
  const Parse *pParse,     /* Parsing context */
  Expr *pExpr,             /* Add the "COLLATE" clause to this expression */
  const Token *pCollName,  /* Name of collating sequence */
  int dequote              /* True to dequote pCollName */
){
  if( pCollName->n>0 ){
    Expr *pNew = sqlite3ExprAlloc(pParse->db, TK_COLLATE, pCollName, dequote);
    if( pNew ){
      pNew->pLeft = pExpr;
      pNew->flags |= EP_Collate|EP_Skip;
      pExpr = pNew;
    }
  }
  return pExpr;
}
SQLITE_PRIVATE Expr *sqlite3ExprAddCollateString(
  const Parse *pParse,  /* Parsing context */
  Expr *pExpr,          /* Add the "COLLATE" clause to this expression */
  const char *zC        /* The collating sequence name */
){
  Token s;
  assert( zC!=0 );
  sqlite3TokenInit(&s, (char*)zC);
  return sqlite3ExprAddCollateToken(pParse, pExpr, &s, 0);
}

/*
** Skip over any TK_COLLATE operators.
*/
SQLITE_PRIVATE Expr *sqlite3ExprSkipCollate(Expr *pExpr){
  while( pExpr && ExprHasProperty(pExpr, EP_Skip) ){
    assert( pExpr->op==TK_COLLATE );
    pExpr = pExpr->pLeft;
  }
  return pExpr;
}

/*
** Skip over any TK_COLLATE operators and/or any unlikely()
** or likelihood() or likely() functions at the root of an
** expression.
*/
SQLITE_PRIVATE Expr *sqlite3ExprSkipCollateAndLikely(Expr *pExpr){
  while( pExpr && ExprHasProperty(pExpr, EP_Skip|EP_Unlikely) ){
    if( ExprHasProperty(pExpr, EP_Unlikely) ){
      assert( ExprUseXList(pExpr) );
      assert( pExpr->x.pList->nExpr>0 );
      assert( pExpr->op==TK_FUNCTION );
      pExpr = pExpr->x.pList->a[0].pExpr;
    }else if( pExpr->op==TK_COLLATE ){
      pExpr = pExpr->pLeft;
    }else{
      break;
    }
  }
  return pExpr;
}

/*
** Return the collation sequence for the expression pExpr. If
** there is no defined collating sequence, return NULL.
**
** See also: sqlite3ExprNNCollSeq()
**
** The sqlite3ExprNNCollSeq() works the same exact that it returns the
** default collation if pExpr has no defined collation.
**
** The collating sequence might be determined by a COLLATE operator
** or by the presence of a column with a defined collating sequence.
** COLLATE operators take first precedence.  Left operands take
** precedence over right operands.
*/
SQLITE_PRIVATE CollSeq *sqlite3ExprCollSeq(Parse *pParse, const Expr *pExpr){
  sqlite3 *db = pParse->db;
  CollSeq *pColl = 0;
  const Expr *p = pExpr;
  while( p ){
    int op = p->op;
    if( op==TK_REGISTER ) op = p->op2;
    if( (op==TK_AGG_COLUMN && p->y.pTab!=0)
     || op==TK_COLUMN || op==TK_TRIGGER
    ){
      int j;
      assert( ExprUseYTab(p) );
      assert( p->y.pTab!=0 );
      if( (j = p->iColumn)>=0 ){
        const char *zColl = sqlite3ColumnColl(&p->y.pTab->aCol[j]);
        pColl = sqlite3FindCollSeq(db, ENC(db), zColl, 0);
      }
      break;
    }
    if( op==TK_CAST || op==TK_UPLUS ){
      p = p->pLeft;
      continue;
    }
    if( op==TK_VECTOR ){
      assert( ExprUseXList(p) );
      p = p->x.pList->a[0].pExpr;
      continue;
    }
    if( op==TK_COLLATE ){
      assert( !ExprHasProperty(p, EP_IntValue) );
      pColl = sqlite3GetCollSeq(pParse, ENC(db), 0, p->u.zToken);
      break;
    }
    if( p->flags & EP_Collate ){
      if( p->pLeft && (p->pLeft->flags & EP_Collate)!=0 ){
        p = p->pLeft;
      }else{
        Expr *pNext  = p->pRight;
        /* The Expr.x union is never used at the same time as Expr.pRight */
        assert( !ExprUseXList(p) || p->x.pList==0 || p->pRight==0 );
        if( ExprUseXList(p) && p->x.pList!=0 && !db->mallocFailed ){
          int i;
          for(i=0; i<p->x.pList->nExpr; i++){
            if( ExprHasProperty(p->x.pList->a[i].pExpr, EP_Collate) ){
              pNext = p->x.pList->a[i].pExpr;
              break;
            }
          }
        }
        p = pNext;
      }
    }else{
      break;
    }
  }
  if( sqlite3CheckCollSeq(pParse, pColl) ){
    pColl = 0;
  }
  return pColl;
}

/*
** Return the collation sequence for the expression pExpr. If
** there is no defined collating sequence, return a pointer to the
** default collation sequence.
**
** See also: sqlite3ExprCollSeq()
**
** The sqlite3ExprCollSeq() routine works the same except that it
** returns NULL if there is no defined collation.
*/
SQLITE_PRIVATE CollSeq *sqlite3ExprNNCollSeq(Parse *pParse, const Expr *pExpr){
  CollSeq *p = sqlite3ExprCollSeq(pParse, pExpr);
  if( p==0 ) p = pParse->db->pDfltColl;
  assert( p!=0 );
  return p;
}

/*
** Return TRUE if the two expressions have equivalent collating sequences.
*/
SQLITE_PRIVATE int sqlite3ExprCollSeqMatch(Parse *pParse, const Expr *pE1, const Expr *pE2){
  CollSeq *pColl1 = sqlite3ExprNNCollSeq(pParse, pE1);
  CollSeq *pColl2 = sqlite3ExprNNCollSeq(pParse, pE2);
  return sqlite3StrICmp(pColl1->zName, pColl2->zName)==0;
}

/*
** pExpr is an operand of a comparison operator.  aff2 is the
** type affinity of the other operand.  This routine returns the
** type affinity that should be used for the comparison operator.
*/
SQLITE_PRIVATE char sqlite3CompareAffinity(const Expr *pExpr, char aff2){
  char aff1 = sqlite3ExprAffinity(pExpr);
  if( aff1>SQLITE_AFF_NONE && aff2>SQLITE_AFF_NONE ){
    /* Both sides of the comparison are columns. If one has numeric
    ** affinity, use that. Otherwise use no affinity.
    */
    if( sqlite3IsNumericAffinity(aff1) || sqlite3IsNumericAffinity(aff2) ){
      return SQLITE_AFF_NUMERIC;
    }else{
      return SQLITE_AFF_BLOB;
    }
  }else{
    /* One side is a column, the other is not. Use the columns affinity. */
    assert( aff1<=SQLITE_AFF_NONE || aff2<=SQLITE_AFF_NONE );
    return (aff1<=SQLITE_AFF_NONE ? aff2 : aff1) | SQLITE_AFF_NONE;
  }
}
