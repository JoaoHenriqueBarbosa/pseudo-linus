
/*
** The output variable *ppExpr is populated with an allocated Fts3Expr
** structure, or set to 0 if the end of the input buffer is reached.
**
** Returns an SQLite error code. SQLITE_OK if everything works, SQLITE_NOMEM
** if a malloc failure occurs, or SQLITE_ERROR if a parse error is encountered.
** If SQLITE_ERROR is returned, pContext is populated with an error message.
*/
static int getNextNode(
  ParseContext *pParse,                   /* fts3 query parse context */
  const char *z, int n,                   /* Input string */
  Fts3Expr **ppExpr,                      /* OUT: expression */
  int *pnConsumed                         /* OUT: Number of bytes consumed */
){
  static const struct Fts3Keyword {
    char *z;                              /* Keyword text */
    unsigned char n;                      /* Length of the keyword */
    unsigned char parenOnly;              /* Only valid in paren mode */
    unsigned char eType;                  /* Keyword code */
  } aKeyword[] = {
    { "OR" ,  2, 0, FTSQUERY_OR   },
    { "AND",  3, 1, FTSQUERY_AND  },
    { "NOT",  3, 1, FTSQUERY_NOT  },
    { "NEAR", 4, 0, FTSQUERY_NEAR }
  };
  int ii;
  int iCol;
  int iColLen;
  int rc;
  Fts3Expr *pRet = 0;

  const char *zInput = z;
  int nInput = n;

  pParse->isNot = 0;

  /* Skip over any whitespace before checking for a keyword, an open or
  ** close bracket, or a quoted string.
  */
  while( nInput>0 && fts3isspace(*zInput) ){
    nInput--;
    zInput++;
  }
  if( nInput==0 ){
    return SQLITE_DONE;
  }

  /* See if we are dealing with a keyword. */
  for(ii=0; ii<(int)(sizeof(aKeyword)/sizeof(struct Fts3Keyword)); ii++){
    const struct Fts3Keyword *pKey = &aKeyword[ii];

    if( (pKey->parenOnly & ~sqlite3_fts3_enable_parentheses)!=0 ){
      continue;
    }

    if( nInput>=pKey->n && 0==memcmp(zInput, pKey->z, pKey->n) ){
      int nNear = SQLITE_FTS3_DEFAULT_NEAR_PARAM;
      int nKey = pKey->n;
      char cNext;

      /* If this is a "NEAR" keyword, check for an explicit nearness. */
      if( pKey->eType==FTSQUERY_NEAR ){
        assert( nKey==4 );
        if( zInput[4]=='/' && zInput[5]>='0' && zInput[5]<='9' ){
          nKey += 1+sqlite3Fts3ReadInt(&zInput[nKey+1], &nNear);
        }
      }

      /* At this point this is probably a keyword. But for that to be true,
      ** the next byte must contain either whitespace, an open or close
      ** parenthesis, a quote character, or EOF.
      */
      cNext = zInput[nKey];
      if( fts3isspace(cNext)
       || cNext=='"' || cNext=='(' || cNext==')' || cNext==0
      ){
        pRet = (Fts3Expr *)sqlite3Fts3MallocZero(sizeof(Fts3Expr));
        if( !pRet ){
          return SQLITE_NOMEM;
        }
        pRet->eType = pKey->eType;
        pRet->nNear = nNear;
        *ppExpr = pRet;
        *pnConsumed = (int)((zInput - z) + nKey);
        return SQLITE_OK;
      }

      /* Turns out that wasn't a keyword after all. This happens if the
      ** user has supplied a token such as "ORacle". Continue.
      */
    }
  }

  /* See if we are dealing with a quoted phrase. If this is the case, then
  ** search for the closing quote and pass the whole string to getNextString()
  ** for processing. This is easy to do, as fts3 has no syntax for escaping
  ** a quote character embedded in a string.
  */
  if( *zInput=='"' ){
    for(ii=1; ii<nInput && zInput[ii]!='"'; ii++);
    *pnConsumed = (int)((zInput - z) + ii + 1);
    if( ii==nInput ){
      return SQLITE_ERROR;
    }
    return getNextString(pParse, &zInput[1], ii-1, ppExpr);
  }

  if( sqlite3_fts3_enable_parentheses ){
    if( *zInput=='(' ){
      int nConsumed = 0;
      pParse->nNest++;
#if !defined(SQLITE_MAX_EXPR_DEPTH)
      if( pParse->nNest>1000 ) return SQLITE_ERROR;
#elif SQLITE_MAX_EXPR_DEPTH>0
      if( pParse->nNest>SQLITE_MAX_EXPR_DEPTH ) return SQLITE_ERROR;
#endif
      rc = fts3ExprParse(pParse, zInput+1, nInput-1, ppExpr, &nConsumed);
      *pnConsumed = (int)(zInput - z) + 1 + nConsumed;
      return rc;
    }else if( *zInput==')' ){
      pParse->nNest--;
      *pnConsumed = (int)((zInput - z) + 1);
      *ppExpr = 0;
      return SQLITE_DONE;
    }
  }

  /* If control flows to this point, this must be a regular token, or
  ** the end of the input. Read a regular token using the sqlite3_tokenizer
  ** interface. Before doing so, figure out if there is an explicit
  ** column specifier for the token.
  **
  ** TODO: Strangely, it is not possible to associate a column specifier
  ** with a quoted phrase, only with a single token. Not sure if this was
  ** an implementation artifact or an intentional decision when fts3 was
  ** first implemented. Whichever it was, this module duplicates the
  ** limitation.
  */
  iCol = pParse->iDefaultCol;
  iColLen = 0;
  for(ii=0; ii<pParse->nCol; ii++){
    const char *zStr = pParse->azCol[ii];
    int nStr = (int)strlen(zStr);
    if( nInput>nStr && zInput[nStr]==':'
     && sqlite3_strnicmp(zStr, zInput, nStr)==0
    ){
      iCol = ii;
      iColLen = (int)((zInput - z) + nStr + 1);
      break;
    }
  }
  rc = getNextToken(pParse, iCol, &z[iColLen], n-iColLen, ppExpr, pnConsumed);
  *pnConsumed += iColLen;
  return rc;
}

/*
** The argument is an Fts3Expr structure for a binary operator (any type
** except an FTSQUERY_PHRASE). Return an integer value representing the
** precedence of the operator. Lower values have a higher precedence (i.e.
** group more tightly). For example, in the C language, the == operator
** groups more tightly than ||, and would therefore have a higher precedence.
**
** When using the new fts3 query syntax (when SQLITE_ENABLE_FTS3_PARENTHESIS
** is defined), the order of the operators in precedence from highest to
** lowest is:
**
**   NEAR
**   NOT
**   AND (including implicit ANDs)
**   OR
**
** Note that when using the old query syntax, the OR operator has a higher
** precedence than the AND operator.
*/
static int opPrecedence(Fts3Expr *p){
  assert( p->eType!=FTSQUERY_PHRASE );
  if( sqlite3_fts3_enable_parentheses ){
    return p->eType;
  }else if( p->eType==FTSQUERY_NEAR ){
    return 1;
  }else if( p->eType==FTSQUERY_OR ){
    return 2;
  }
  assert( p->eType==FTSQUERY_AND );
  return 3;
}

/*
** Argument ppHead contains a pointer to the current head of a query
** expression tree being parsed. pPrev is the expression node most recently
** inserted into the tree. This function adds pNew, which is always a binary
** operator node, into the expression tree based on the relative precedence
** of pNew and the existing nodes of the tree. This may result in the head
** of the tree changing, in which case *ppHead is set to the new root node.
*/
static void insertBinaryOperator(
  Fts3Expr **ppHead,       /* Pointer to the root node of a tree */
  Fts3Expr *pPrev,         /* Node most recently inserted into the tree */
  Fts3Expr *pNew           /* New binary node to insert into expression tree */
){
  Fts3Expr *pSplit = pPrev;
  while( pSplit->pParent && opPrecedence(pSplit->pParent)<=opPrecedence(pNew) ){
    pSplit = pSplit->pParent;
  }

  if( pSplit->pParent ){
    assert( pSplit->pParent->pRight==pSplit );
    pSplit->pParent->pRight = pNew;
    pNew->pParent = pSplit->pParent;
  }else{
    *ppHead = pNew;
  }
  pNew->pLeft = pSplit;
  pSplit->pParent = pNew;
}

/*
** Parse the fts3 query expression found in buffer z, length n. This function
** returns either when the end of the buffer is reached or an unmatched
** closing bracket - ')' - is encountered.
**
** If successful, SQLITE_OK is returned, *ppExpr is set to point to the
** parsed form of the expression and *pnConsumed is set to the number of
** bytes read from buffer z. Otherwise, *ppExpr is set to 0 and SQLITE_NOMEM
** (out of memory error) or SQLITE_ERROR (parse error) is returned.
*/
static int fts3ExprParse(
  ParseContext *pParse,                   /* fts3 query parse context */
  const char *z, int n,                   /* Text of MATCH query */
  Fts3Expr **ppExpr,                      /* OUT: Parsed query structure */
  int *pnConsumed                         /* OUT: Number of bytes consumed */
){
  Fts3Expr *pRet = 0;
  Fts3Expr *pPrev = 0;
  Fts3Expr *pNotBranch = 0;               /* Only used in legacy parse mode */
  int nIn = n;
  const char *zIn = z;
  int rc = SQLITE_OK;
  int isRequirePhrase = 1;

  while( rc==SQLITE_OK ){
    Fts3Expr *p = 0;
    int nByte = 0;

    rc = getNextNode(pParse, zIn, nIn, &p, &nByte);
    assert( nByte>0 || (rc!=SQLITE_OK && p==0) );
    if( rc==SQLITE_OK ){
      if( p ){
        int isPhrase;

        if( !sqlite3_fts3_enable_parentheses
            && p->eType==FTSQUERY_PHRASE && pParse->isNot
        ){
          /* Create an implicit NOT operator. */
          Fts3Expr *pNot = sqlite3Fts3MallocZero(sizeof(Fts3Expr));
          if( !pNot ){
            sqlite3Fts3ExprFree(p);
            rc = SQLITE_NOMEM;
            goto exprparse_out;
          }
          pNot->eType = FTSQUERY_NOT;
          pNot->pRight = p;
          p->pParent = pNot;
          if( pNotBranch ){
            pNot->pLeft = pNotBranch;
            pNotBranch->pParent = pNot;
          }
          pNotBranch = pNot;
          p = pPrev;
        }else{
          int eType = p->eType;
          isPhrase = (eType==FTSQUERY_PHRASE || p->pLeft);

          /* The isRequirePhrase variable is set to true if a phrase or
          ** an expression contained in parenthesis is required. If a
          ** binary operator (AND, OR, NOT or NEAR) is encounted when
          ** isRequirePhrase is set, this is a syntax error.
          */
          if( !isPhrase && isRequirePhrase ){
            sqlite3Fts3ExprFree(p);
            rc = SQLITE_ERROR;
            goto exprparse_out;
          }

          if( isPhrase && !isRequirePhrase ){
            /* Insert an implicit AND operator. */
            Fts3Expr *pAnd;
            assert( pRet && pPrev );
            pAnd = sqlite3Fts3MallocZero(sizeof(Fts3Expr));
            if( !pAnd ){
              sqlite3Fts3ExprFree(p);
              rc = SQLITE_NOMEM;
              goto exprparse_out;
            }
            pAnd->eType = FTSQUERY_AND;
            insertBinaryOperator(&pRet, pPrev, pAnd);
            pPrev = pAnd;
          }

          /* This test catches attempts to make either operand of a NEAR
           ** operator something other than a phrase. For example, either of
           ** the following:
           **
           **    (bracketed expression) NEAR phrase
           **    phrase NEAR (bracketed expression)
           **
           ** Return an error in either case.
           */
          if( pPrev && (
            (eType==FTSQUERY_NEAR && !isPhrase && pPrev->eType!=FTSQUERY_PHRASE)
         || (eType!=FTSQUERY_PHRASE && isPhrase && pPrev->eType==FTSQUERY_NEAR)
          )){
            sqlite3Fts3ExprFree(p);
            rc = SQLITE_ERROR;
            goto exprparse_out;
          }

          if( isPhrase ){
            if( pRet ){
              assert( pPrev && pPrev->pLeft && pPrev->pRight==0 );
              pPrev->pRight = p;
              p->pParent = pPrev;
            }else{
              pRet = p;
            }
          }else{
            insertBinaryOperator(&pRet, pPrev, p);
          }
          isRequirePhrase = !isPhrase;
        }
        pPrev = p;
      }
      assert( nByte>0 );
    }
    assert( rc!=SQLITE_OK || (nByte>0 && nByte<=nIn) );
    nIn -= nByte;
    zIn += nByte;
  }

  if( rc==SQLITE_DONE && pRet && isRequirePhrase ){
    rc = SQLITE_ERROR;
  }

  if( rc==SQLITE_DONE ){
    rc = SQLITE_OK;
    if( !sqlite3_fts3_enable_parentheses && pNotBranch ){
      if( !pRet ){
        rc = SQLITE_ERROR;
      }else{
        Fts3Expr *pIter = pNotBranch;
        while( pIter->pLeft ){
          pIter = pIter->pLeft;
        }
        pIter->pLeft = pRet;
        pRet->pParent = pIter;
        pRet = pNotBranch;
      }
    }
  }
  *pnConsumed = n - nIn;

exprparse_out:
  if( rc!=SQLITE_OK ){
    sqlite3Fts3ExprFree(pRet);
    sqlite3Fts3ExprFree(pNotBranch);
    pRet = 0;
  }
  *ppExpr = pRet;
  return rc;
}
