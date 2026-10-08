
/*
** This function checks if argument pFrom refers to a CTE declared by
** a WITH clause on the stack currently maintained by the parser (on the
** pParse->pWith linked list).  And if currently processing a CTE
** CTE expression, through routine checks to see if the reference is
** a recursive reference to the CTE.
**
** If pFrom matches a CTE according to either of these two above, pFrom->pTab
** and other fields are populated accordingly.
**
** Return 0 if no match is found.
** Return 1 if a match is found.
** Return 2 if an error condition is detected.
*/
static int resolveFromTermToCte(
  Parse *pParse,                  /* The parsing context */
  Walker *pWalker,                /* Current tree walker */
  SrcItem *pFrom                  /* The FROM clause term to check */
){
  Cte *pCte;               /* Matched CTE (or NULL if no match) */
  With *pWith;             /* The matching WITH */

  assert( pFrom->pTab==0 );
  if( pParse->pWith==0 ){
    /* There are no WITH clauses in the stack.  No match is possible */
    return 0;
  }
  if( pParse->nErr ){
    /* Prior errors might have left pParse->pWith in a goofy state, so
    ** go no further. */
    return 0;
  }
  if( pFrom->zDatabase!=0 ){
    /* The FROM term contains a schema qualifier (ex: main.t1) and so
    ** it cannot possibly be a CTE reference. */
    return 0;
  }
  if( pFrom->fg.notCte ){
    /* The FROM term is specifically excluded from matching a CTE.
    **   (1)  It is part of a trigger that used to have zDatabase but had
    **        zDatabase removed by sqlite3FixTriggerStep().
    **   (2)  This is the first term in the FROM clause of an UPDATE.
    */
    return 0;
  }
  pCte = searchWith(pParse->pWith, pFrom, &pWith);
  if( pCte ){
    sqlite3 *db = pParse->db;
    Table *pTab;
    ExprList *pEList;
    Select *pSel;
    Select *pLeft;                /* Left-most SELECT statement */
    Select *pRecTerm;             /* Left-most recursive term */
    int bMayRecursive;            /* True if compound joined by UNION [ALL] */
    With *pSavedWith;             /* Initial value of pParse->pWith */
    int iRecTab = -1;             /* Cursor for recursive table */
    CteUse *pCteUse;

    /* If pCte->zCteErr is non-NULL at this point, then this is an illegal
    ** recursive reference to CTE pCte. Leave an error in pParse and return
    ** early. If pCte->zCteErr is NULL, then this is not a recursive reference.
    ** In this case, proceed.  */
    if( pCte->zCteErr ){
      sqlite3ErrorMsg(pParse, pCte->zCteErr, pCte->zName);
      return 2;
    }
    if( cannotBeFunction(pParse, pFrom) ) return 2;

    assert( pFrom->pTab==0 );
    pTab = sqlite3DbMallocZero(db, sizeof(Table));
    if( pTab==0 ) return 2;
    pCteUse = pCte->pUse;
    if( pCteUse==0 ){
      pCte->pUse = pCteUse = sqlite3DbMallocZero(db, sizeof(pCteUse[0]));
      if( pCteUse==0
       || sqlite3ParserAddCleanup(pParse,sqlite3DbFree,pCteUse)==0
      ){
        sqlite3DbFree(db, pTab);
        return 2;
      }
      pCteUse->eM10d = pCte->eM10d;
    }
    pFrom->pTab = pTab;
    pTab->nTabRef = 1;
    pTab->zName = sqlite3DbStrDup(db, pCte->zName);
    pTab->iPKey = -1;
    pTab->nRowLogEst = 200; assert( 200==sqlite3LogEst(1048576) );
    pTab->tabFlags |= TF_Ephemeral | TF_NoVisibleRowid;
    pFrom->pSelect = sqlite3SelectDup(db, pCte->pSelect, 0);
    if( db->mallocFailed ) return 2;
    pFrom->pSelect->selFlags |= SF_CopyCte;
    assert( pFrom->pSelect );
    if( pFrom->fg.isIndexedBy ){
      sqlite3ErrorMsg(pParse, "no such index: \"%s\"", pFrom->u1.zIndexedBy);
      return 2;
    }
    pFrom->fg.isCte = 1;
    pFrom->u2.pCteUse = pCteUse;
    pCteUse->nUse++;

    /* Check if this is a recursive CTE. */
    pRecTerm = pSel = pFrom->pSelect;
    bMayRecursive = ( pSel->op==TK_ALL || pSel->op==TK_UNION );
    while( bMayRecursive && pRecTerm->op==pSel->op ){
      int i;
      SrcList *pSrc = pRecTerm->pSrc;
      assert( pRecTerm->pPrior!=0 );
      for(i=0; i<pSrc->nSrc; i++){
        SrcItem *pItem = &pSrc->a[i];
        if( pItem->zDatabase==0
         && pItem->zName!=0
         && 0==sqlite3StrICmp(pItem->zName, pCte->zName)
        ){
          pItem->pTab = pTab;
          pTab->nTabRef++;
          pItem->fg.isRecursive = 1;
          if( pRecTerm->selFlags & SF_Recursive ){
            sqlite3ErrorMsg(pParse,
               "multiple references to recursive table: %s", pCte->zName
            );
            return 2;
          }
          pRecTerm->selFlags |= SF_Recursive;
          if( iRecTab<0 ) iRecTab = pParse->nTab++;
          pItem->iCursor = iRecTab;
        }
      }
      if( (pRecTerm->selFlags & SF_Recursive)==0 ) break;
      pRecTerm = pRecTerm->pPrior;
    }

    pCte->zCteErr = "circular reference: %s";
    pSavedWith = pParse->pWith;
    pParse->pWith = pWith;
    if( pSel->selFlags & SF_Recursive ){
      int rc;
      assert( pRecTerm!=0 );
      assert( (pRecTerm->selFlags & SF_Recursive)==0 );
      assert( pRecTerm->pNext!=0 );
      assert( (pRecTerm->pNext->selFlags & SF_Recursive)!=0 );
      assert( pRecTerm->pWith==0 );
      pRecTerm->pWith = pSel->pWith;
      rc = sqlite3WalkSelect(pWalker, pRecTerm);
      pRecTerm->pWith = 0;
      if( rc ){
        pParse->pWith = pSavedWith;
        return 2;
      }
    }else{
      if( sqlite3WalkSelect(pWalker, pSel) ){
        pParse->pWith = pSavedWith;
        return 2;
      }
    }
    pParse->pWith = pWith;

    for(pLeft=pSel; pLeft->pPrior; pLeft=pLeft->pPrior);
    pEList = pLeft->pEList;
    if( pCte->pCols ){
      if( pEList && pEList->nExpr!=pCte->pCols->nExpr ){
        sqlite3ErrorMsg(pParse, "table %s has %d values for %d columns",
            pCte->zName, pEList->nExpr, pCte->pCols->nExpr
        );
        pParse->pWith = pSavedWith;
        return 2;
      }
      pEList = pCte->pCols;
    }

    sqlite3ColumnsFromExprList(pParse, pEList, &pTab->nCol, &pTab->aCol);
    if( bMayRecursive ){
      if( pSel->selFlags & SF_Recursive ){
        pCte->zCteErr = "multiple recursive references: %s";
      }else{
        pCte->zCteErr = "recursive reference in a subquery: %s";
      }
      sqlite3WalkSelect(pWalker, pSel);
    }
    pCte->zCteErr = 0;
    pParse->pWith = pSavedWith;
    return 1;  /* Success */
  }
  return 0;  /* No match */
}
#endif

#ifndef SQLITE_OMIT_CTE
/*
** If the SELECT passed as the second argument has an associated WITH
** clause, pop it from the stack stored as part of the Parse object.
**
** This function is used as the xSelectCallback2() callback by
** sqlite3SelectExpand() when walking a SELECT tree to resolve table
** names and other FROM clause elements.
*/
SQLITE_PRIVATE void sqlite3SelectPopWith(Walker *pWalker, Select *p){
  Parse *pParse = pWalker->pParse;
  if( OK_IF_ALWAYS_TRUE(pParse->pWith) && p->pPrior==0 ){
    With *pWith = findRightmost(p)->pWith;
    if( pWith!=0 ){
      assert( pParse->pWith==pWith || pParse->nErr );
      pParse->pWith = pWith->pOuter;
    }
  }
}
#endif

/*
** The SrcItem structure passed as the second argument represents a
** sub-query in the FROM clause of a SELECT statement. This function
** allocates and populates the SrcItem.pTab object. If successful,
** SQLITE_OK is returned. Otherwise, if an OOM error is encountered,
** SQLITE_NOMEM.
*/
SQLITE_PRIVATE int sqlite3ExpandSubquery(Parse *pParse, SrcItem *pFrom){
  Select *pSel = pFrom->pSelect;
  Table *pTab;

  assert( pSel );
  pFrom->pTab = pTab = sqlite3DbMallocZero(pParse->db, sizeof(Table));
  if( pTab==0 ) return SQLITE_NOMEM;
  pTab->nTabRef = 1;
  if( pFrom->zAlias ){
    pTab->zName = sqlite3DbStrDup(pParse->db, pFrom->zAlias);
  }else{
    pTab->zName = sqlite3MPrintf(pParse->db, "%!S", pFrom);
  }
  while( pSel->pPrior ){ pSel = pSel->pPrior; }
  sqlite3ColumnsFromExprList(pParse, pSel->pEList,&pTab->nCol,&pTab->aCol);
  pTab->iPKey = -1;
  pTab->eTabType = TABTYP_VIEW;
  pTab->nRowLogEst = 200; assert( 200==sqlite3LogEst(1048576) );
#ifndef SQLITE_ALLOW_ROWID_IN_VIEW
  /* The usual case - do not allow ROWID on a subquery */
  pTab->tabFlags |= TF_Ephemeral | TF_NoVisibleRowid;
#else
  /* Legacy compatibility mode */
  pTab->tabFlags |= TF_Ephemeral | sqlite3Config.mNoVisibleRowid;
#endif
  return pParse->nErr ? SQLITE_ERROR : SQLITE_OK;
}


/*
** Check the N SrcItem objects to the right of pBase.  (N might be zero!)
** If any of those SrcItem objects have a USING clause containing zName
** then return true.
**
** If N is zero, or none of the N SrcItem objects to the right of pBase
** contains a USING clause, or if none of the USING clauses contain zName,
** then return false.
*/
static int inAnyUsingClause(
  const char *zName, /* Name we are looking for */
  SrcItem *pBase,    /* The base SrcItem.  Looking at pBase[1] and following */
  int N              /* How many SrcItems to check */
){
  while( N>0 ){
    N--;
    pBase++;
    if( pBase->fg.isUsing==0 ) continue;
    if( NEVER(pBase->u3.pUsing==0) ) continue;
    if( sqlite3IdListIndex(pBase->u3.pUsing, zName)>=0 ) return 1;
  }
  return 0;
}


/*
** This routine is a Walker callback for "expanding" a SELECT statement.
** "Expanding" means to do the following:
**
**    (1)  Make sure VDBE cursor numbers have been assigned to every
**         element of the FROM clause.
**
**    (2)  Fill in the pTabList->a[].pTab fields in the SrcList that
**         defines FROM clause.  When views appear in the FROM clause,
**         fill pTabList->a[].pSelect with a copy of the SELECT statement
**         that implements the view.  A copy is made of the view's SELECT
**         statement so that we can freely modify or delete that statement
**         without worrying about messing up the persistent representation
**         of the view.
**
**    (3)  Add terms to the WHERE clause to accommodate the NATURAL keyword
**         on joins and the ON and USING clause of joins.
**
**    (4)  Scan the list of columns in the result set (pEList) looking
**         for instances of the "*" operator or the TABLE.* operator.
**         If found, expand each "*" to be every column in every table
**         and TABLE.* to be every column in TABLE.
**
*/
static int selectExpander(Walker *pWalker, Select *p){
  Parse *pParse = pWalker->pParse;
  int i, j, k, rc;
  SrcList *pTabList;
  ExprList *pEList;
  SrcItem *pFrom;
  sqlite3 *db = pParse->db;
  Expr *pE, *pRight, *pExpr;
  u16 selFlags = p->selFlags;
  u32 elistFlags = 0;

  p->selFlags |= SF_Expanded;
  if( db->mallocFailed  ){
    return WRC_Abort;
  }
  assert( p->pSrc!=0 );
  if( (selFlags & SF_Expanded)!=0 ){
    return WRC_Prune;
  }
  if( pWalker->eCode ){
    /* Renumber selId because it has been copied from a view */
    p->selId = ++pParse->nSelect;
  }
  pTabList = p->pSrc;
  pEList = p->pEList;
  if( pParse->pWith && (p->selFlags & SF_View) ){
    if( p->pWith==0 ){
      p->pWith = (With*)sqlite3DbMallocZero(db, sizeof(With));
      if( p->pWith==0 ){
        return WRC_Abort;
      }
    }
    p->pWith->bView = 1;
  }
  sqlite3WithPush(pParse, p->pWith, 0);

  /* Make sure cursor numbers have been assigned to all entries in
  ** the FROM clause of the SELECT statement.
  */
  sqlite3SrcListAssignCursors(pParse, pTabList);

  /* Look up every table named in the FROM clause of the select.  If
  ** an entry of the FROM clause is a subquery instead of a table or view,
  ** then create a transient table structure to describe the subquery.
  */
  for(i=0, pFrom=pTabList->a; i<pTabList->nSrc; i++, pFrom++){
    Table *pTab;
    assert( pFrom->fg.isRecursive==0 || pFrom->pTab!=0 );
    if( pFrom->pTab ) continue;
    assert( pFrom->fg.isRecursive==0 );
    if( pFrom->zName==0 ){
#ifndef SQLITE_OMIT_SUBQUERY
      Select *pSel = pFrom->pSelect;
      /* A sub-query in the FROM clause of a SELECT */
      assert( pSel!=0 );
      assert( pFrom->pTab==0 );
      if( sqlite3WalkSelect(pWalker, pSel) ) return WRC_Abort;
      if( sqlite3ExpandSubquery(pParse, pFrom) ) return WRC_Abort;
#endif
#ifndef SQLITE_OMIT_CTE
    }else if( (rc = resolveFromTermToCte(pParse, pWalker, pFrom))!=0 ){
      if( rc>1 ) return WRC_Abort;
      pTab = pFrom->pTab;
      assert( pTab!=0 );
#endif
    }else{
      /* An ordinary table or view name in the FROM clause */
      assert( pFrom->pTab==0 );
      pFrom->pTab = pTab = sqlite3LocateTableItem(pParse, 0, pFrom);
      if( pTab==0 ) return WRC_Abort;
      if( pTab->nTabRef>=0xffff ){
        sqlite3ErrorMsg(pParse, "too many references to \"%s\": max 65535",
           pTab->zName);
        pFrom->pTab = 0;
        return WRC_Abort;
      }
      pTab->nTabRef++;
      if( !IsVirtual(pTab) && cannotBeFunction(pParse, pFrom) ){
        return WRC_Abort;
      }
#if !defined(SQLITE_OMIT_VIEW) || !defined(SQLITE_OMIT_VIRTUALTABLE)
      if( !IsOrdinaryTable(pTab) ){
        i16 nCol;
        u8 eCodeOrig = pWalker->eCode;
        if( sqlite3ViewGetColumnNames(pParse, pTab) ) return WRC_Abort;
        assert( pFrom->pSelect==0 );
        if( IsView(pTab) ){
          if( (db->flags & SQLITE_EnableView)==0
           && pTab->pSchema!=db->aDb[1].pSchema
          ){
            sqlite3ErrorMsg(pParse, "access to view \"%s\" prohibited",
              pTab->zName);
          }
          pFrom->pSelect = sqlite3SelectDup(db, pTab->u.view.pSelect, 0);
        }
#ifndef SQLITE_OMIT_VIRTUALTABLE
        else if( ALWAYS(IsVirtual(pTab))
         && pFrom->fg.fromDDL
         && ALWAYS(pTab->u.vtab.p!=0)
         && pTab->u.vtab.p->eVtabRisk > ((db->flags & SQLITE_TrustedSchema)!=0)
        ){
          sqlite3ErrorMsg(pParse, "unsafe use of virtual table \"%s\"",
                                  pTab->zName);
        }
        assert( SQLITE_VTABRISK_Normal==1 && SQLITE_VTABRISK_High==2 );
#endif
        nCol = pTab->nCol;
        pTab->nCol = -1;
        pWalker->eCode = 1;  /* Turn on Select.selId renumbering */
        sqlite3WalkSelect(pWalker, pFrom->pSelect);
        pWalker->eCode = eCodeOrig;
        pTab->nCol = nCol;
      }
#endif
    }

    /* Locate the index named by the INDEXED BY clause, if any. */
    if( pFrom->fg.isIndexedBy && sqlite3IndexedByLookup(pParse, pFrom) ){
      return WRC_Abort;
    }
  }

  /* Process NATURAL keywords, and ON and USING clauses of joins.
  */
  assert( db->mallocFailed==0 || pParse->nErr!=0 );
  if( pParse->nErr || sqlite3ProcessJoin(pParse, p) ){
    return WRC_Abort;
  }

  /* For every "*" that occurs in the column list, insert the names of
  ** all columns in all tables.  And for every TABLE.* insert the names
  ** of all columns in TABLE.  The parser inserted a special expression
  ** with the TK_ASTERISK operator for each "*" that it found in the column
  ** list.  The following code just has to locate the TK_ASTERISK
  ** expressions and expand each one to the list of all columns in
  ** all tables.
  **
  ** The first loop just checks to see if there are any "*" operators
  ** that need expanding.
  */
  for(k=0; k<pEList->nExpr; k++){
    pE = pEList->a[k].pExpr;
    if( pE->op==TK_ASTERISK ) break;
    assert( pE->op!=TK_DOT || pE->pRight!=0 );
    assert( pE->op!=TK_DOT || (pE->pLeft!=0 && pE->pLeft->op==TK_ID) );
    if( pE->op==TK_DOT && pE->pRight->op==TK_ASTERISK ) break;
    elistFlags |= pE->flags;
  }
  if( k<pEList->nExpr ){
    /*
    ** If we get here it means the result set contains one or more "*"
    ** operators that need to be expanded.  Loop through each expression
    ** in the result set and expand them one by one.
    */
    struct ExprList_item *a = pEList->a;
    ExprList *pNew = 0;
    int flags = pParse->db->flags;
    int longNames = (flags & SQLITE_FullColNames)!=0
                      && (flags & SQLITE_ShortColNames)==0;

    for(k=0; k<pEList->nExpr; k++){
      pE = a[k].pExpr;
      elistFlags |= pE->flags;
      pRight = pE->pRight;
      assert( pE->op!=TK_DOT || pRight!=0 );
      if( pE->op!=TK_ASTERISK
       && (pE->op!=TK_DOT || pRight->op!=TK_ASTERISK)
      ){
        /* This particular expression does not need to be expanded.
        */
        pNew = sqlite3ExprListAppend(pParse, pNew, a[k].pExpr);
        if( pNew ){
          pNew->a[pNew->nExpr-1].zEName = a[k].zEName;
          pNew->a[pNew->nExpr-1].fg.eEName = a[k].fg.eEName;
          a[k].zEName = 0;
        }
        a[k].pExpr = 0;
      }else{
        /* This expression is a "*" or a "TABLE.*" and needs to be
        ** expanded. */
        int tableSeen = 0;      /* Set to 1 when TABLE matches */
        char *zTName = 0;       /* text of name of TABLE */
        int iErrOfst;
        if( pE->op==TK_DOT ){
          assert( (selFlags & SF_NestedFrom)==0 );
          assert( pE->pLeft!=0 );
          assert( !ExprHasProperty(pE->pLeft, EP_IntValue) );
          zTName = pE->pLeft->u.zToken;
          assert( ExprUseWOfst(pE->pLeft) );
          iErrOfst = pE->pRight->w.iOfst;
        }else{
          assert( ExprUseWOfst(pE) );
          iErrOfst = pE->w.iOfst;
        }
        for(i=0, pFrom=pTabList->a; i<pTabList->nSrc; i++, pFrom++){
          int nAdd;                    /* Number of cols including rowid */
          Table *pTab = pFrom->pTab;   /* Table for this data source */
          ExprList *pNestedFrom;       /* Result-set of a nested FROM clause */
          char *zTabName;              /* AS name for this data source */
          const char *zSchemaName = 0; /* Schema name for this data source */
          int iDb;                     /* Schema index for this data src */
          IdList *pUsing;              /* USING clause for pFrom[1] */

          if( (zTabName = pFrom->zAlias)==0 ){
            zTabName = pTab->zName;
          }
          if( db->mallocFailed ) break;
          assert( (int)pFrom->fg.isNestedFrom == IsNestedFrom(pFrom->pSelect) );
          if( pFrom->fg.isNestedFrom ){
            assert( pFrom->pSelect!=0 );
            pNestedFrom = pFrom->pSelect->pEList;
            assert( pNestedFrom!=0 );
            assert( pNestedFrom->nExpr==pTab->nCol );
            assert( VisibleRowid(pTab)==0 || ViewCanHaveRowid );
          }else{
            if( zTName && sqlite3StrICmp(zTName, zTabName)!=0 ){
              continue;
            }
            pNestedFrom = 0;
            iDb = sqlite3SchemaToIndex(db, pTab->pSchema);
            zSchemaName = iDb>=0 ? db->aDb[iDb].zDbSName : "*";
          }
          if( i+1<pTabList->nSrc
           && pFrom[1].fg.isUsing
           && (selFlags & SF_NestedFrom)!=0
          ){
            int ii;
            pUsing = pFrom[1].u3.pUsing;
            for(ii=0; ii<pUsing->nId; ii++){
              const char *zUName = pUsing->a[ii].zName;
              pRight = sqlite3Expr(db, TK_ID, zUName);
              sqlite3ExprSetErrorOffset(pRight, iErrOfst);
              pNew = sqlite3ExprListAppend(pParse, pNew, pRight);
              if( pNew ){
                struct ExprList_item *pX = &pNew->a[pNew->nExpr-1];
                assert( pX->zEName==0 );
                pX->zEName = sqlite3MPrintf(db,"..%s", zUName);
                pX->fg.eEName = ENAME_TAB;
                pX->fg.bUsingTerm = 1;
              }
            }
          }else{
            pUsing = 0;
          }

          nAdd = pTab->nCol;
          if( VisibleRowid(pTab) && (selFlags & SF_NestedFrom)!=0 ) nAdd++;
          for(j=0; j<nAdd; j++){
            const char *zName;
            struct ExprList_item *pX; /* Newly added ExprList term */

            if( j==pTab->nCol ){
              zName = sqlite3RowidAlias(pTab);
              if( zName==0 ) continue;
            }else{
              zName = pTab->aCol[j].zCnName;

              /* If pTab is actually an SF_NestedFrom sub-select, do not
              ** expand any ENAME_ROWID columns.  */
              if( pNestedFrom && pNestedFrom->a[j].fg.eEName==ENAME_ROWID ){
                continue;
              }

              if( zTName
               && pNestedFrom
               && sqlite3MatchEName(&pNestedFrom->a[j], 0, zTName, 0, 0)==0
              ){
                continue;
              }

              /* If a column is marked as 'hidden', omit it from the expanded
              ** result-set list unless the SELECT has the SF_IncludeHidden
              ** bit set.
              */
              if( (p->selFlags & SF_IncludeHidden)==0
                && IsHiddenColumn(&pTab->aCol[j])
              ){
                continue;
              }
              if( (pTab->aCol[j].colFlags & COLFLAG_NOEXPAND)!=0
               && zTName==0
               && (selFlags & (SF_NestedFrom))==0
              ){
                continue;
              }
            }
            assert( zName );
            tableSeen = 1;

            if( i>0 && zTName==0 && (selFlags & SF_NestedFrom)==0 ){
              if( pFrom->fg.isUsing
               && sqlite3IdListIndex(pFrom->u3.pUsing, zName)>=0
              ){
                /* In a join with a USING clause, omit columns in the
                ** using clause from the table on the right. */
                continue;
              }
            }
            pRight = sqlite3Expr(db, TK_ID, zName);
            if( (pTabList->nSrc>1
                 && (  (pFrom->fg.jointype & JT_LTORJ)==0
                     || (selFlags & SF_NestedFrom)!=0
                     || !inAnyUsingClause(zName,pFrom,pTabList->nSrc-i-1)
                    )
                )
             || IN_RENAME_OBJECT
            ){
              Expr *pLeft;
              pLeft = sqlite3Expr(db, TK_ID, zTabName);
              pExpr = sqlite3PExpr(pParse, TK_DOT, pLeft, pRight);
              if( IN_RENAME_OBJECT && pE->pLeft ){
                sqlite3RenameTokenRemap(pParse, pLeft, pE->pLeft);
              }
              if( zSchemaName ){
                pLeft = sqlite3Expr(db, TK_ID, zSchemaName);
                pExpr = sqlite3PExpr(pParse, TK_DOT, pLeft, pExpr);
              }
            }else{
              pExpr = pRight;
            }
            sqlite3ExprSetErrorOffset(pExpr, iErrOfst);
            pNew = sqlite3ExprListAppend(pParse, pNew, pExpr);
            if( pNew==0 ){
              break;  /* OOM */
            }
            pX = &pNew->a[pNew->nExpr-1];
            assert( pX->zEName==0 );
            if( (selFlags & SF_NestedFrom)!=0 && !IN_RENAME_OBJECT ){
              if( pNestedFrom && (!ViewCanHaveRowid || j<pNestedFrom->nExpr) ){
                assert( j<pNestedFrom->nExpr );
                pX->zEName = sqlite3DbStrDup(db, pNestedFrom->a[j].zEName);
                testcase( pX->zEName==0 );
              }else{
                pX->zEName = sqlite3MPrintf(db, "%s.%s.%s",
                                           zSchemaName, zTabName, zName);
                testcase( pX->zEName==0 );
              }
              pX->fg.eEName = (j==pTab->nCol ? ENAME_ROWID : ENAME_TAB);
              if( (pFrom->fg.isUsing
                   && sqlite3IdListIndex(pFrom->u3.pUsing, zName)>=0)
               || (pUsing && sqlite3IdListIndex(pUsing, zName)>=0)
               || (j<pTab->nCol && (pTab->aCol[j].colFlags & COLFLAG_NOEXPAND))
              ){
                pX->fg.bNoExpand = 1;
              }
            }else if( longNames ){
              pX->zEName = sqlite3MPrintf(db, "%s.%s", zTabName, zName);
              pX->fg.eEName = ENAME_NAME;
            }else{
              pX->zEName = sqlite3DbStrDup(db, zName);
              pX->fg.eEName = ENAME_NAME;
            }
          }
        }
        if( !tableSeen ){
          if( zTName ){
            sqlite3ErrorMsg(pParse, "no such table: %s", zTName);
          }else{
            sqlite3ErrorMsg(pParse, "no tables specified");
          }
        }
      }
    }
    sqlite3ExprListDelete(db, pEList);
    p->pEList = pNew;
  }
  if( p->pEList ){
    if( p->pEList->nExpr>db->aLimit[SQLITE_LIMIT_COLUMN] ){
      sqlite3ErrorMsg(pParse, "too many columns in result set");
      return WRC_Abort;
    }
    if( (elistFlags & (EP_HasFunc|EP_Subquery))!=0 ){
      p->selFlags |= SF_ComplexResult;
    }
  }
#if TREETRACE_ENABLED
  if( sqlite3TreeTrace & 0x8 ){
    TREETRACE(0x8,pParse,p,("After result-set wildcard expansion:\n"));
    sqlite3TreeViewSelect(0, p, 0);
  }
#endif
  return WRC_Continue;
}
