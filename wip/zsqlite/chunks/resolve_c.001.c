        assert( pTab->nCol>0 || pParse->nErr );
        assert( (int)pItem->fg.isNestedFrom == IsNestedFrom(pItem->pSelect) );
        if( pItem->fg.isNestedFrom ){
          /* In this case, pItem is a subquery that has been formed from a
          ** parenthesized subset of the FROM clause terms.  Example:
          **   .... FROM t1 LEFT JOIN (t2 RIGHT JOIN t3 USING(x)) USING(y) ...
          **                          \_________________________/
          **             This pItem -------------^
          */
          int hit = 0;
          assert( pItem->pSelect!=0 );
          pEList = pItem->pSelect->pEList;
          assert( pEList!=0 );
          assert( pEList->nExpr==pTab->nCol );
          for(j=0; j<pEList->nExpr; j++){
            int bRowid = 0;       /* True if possible rowid match */
            if( !sqlite3MatchEName(&pEList->a[j], zCol, zTab, zDb, &bRowid) ){
              continue;
            }
            if( bRowid==0 ){
              if( cnt>0 ){
                if( pItem->fg.isUsing==0
                 || sqlite3IdListIndex(pItem->u3.pUsing, zCol)<0
                ){
                  /* Two or more tables have the same column name which is
                  ** not joined by USING.  This is an error.  Signal as much
                  ** by clearing pFJMatch and letting cnt go above 1. */
                  sqlite3ExprListDelete(db, pFJMatch);
                  pFJMatch = 0;
                }else
                if( (pItem->fg.jointype & JT_RIGHT)==0 ){
                  /* An INNER or LEFT JOIN.  Use the left-most table */
                  continue;
                }else
                if( (pItem->fg.jointype & JT_LEFT)==0 ){
                  /* A RIGHT JOIN.  Use the right-most table */
                  cnt = 0;
                  sqlite3ExprListDelete(db, pFJMatch);
                  pFJMatch = 0;
                }else{
                  /* For a FULL JOIN, we must construct a coalesce() func */
                  extendFJMatch(pParse, &pFJMatch, pMatch, pExpr->iColumn);
                }
              }
              cnt++;
              hit = 1;
            }else if( cnt>0 ){
              /* This is a potential rowid match, but there has already been
              ** a real match found. So this can be ignored.  */
              continue;
            }
            cntTab++;
            pMatch = pItem;
            pExpr->iColumn = j;
            pEList->a[j].fg.bUsed = 1;

            /* rowid cannot be part of a USING clause - assert() this. */
            assert( bRowid==0 || pEList->a[j].fg.bUsingTerm==0 );
            if( pEList->a[j].fg.bUsingTerm ) break;
          }
          if( hit || zTab==0 ) continue;
        }
        assert( zDb==0 || zTab!=0 );
        if( zTab ){
          if( zDb ){
            if( pTab->pSchema!=pSchema ) continue;
            if( pSchema==0 && strcmp(zDb,"*")!=0 ) continue;
          }
          if( pItem->zAlias!=0 ){
            if( sqlite3StrICmp(zTab, pItem->zAlias)!=0 ){
              continue;
            }
          }else if( sqlite3StrICmp(zTab, pTab->zName)!=0 ){
            if( pTab->tnum!=1 ) continue;
            if( !isValidSchemaTableName(zTab, pTab, zDb) ) continue;
          }
          assert( ExprUseYTab(pExpr) );
          if( IN_RENAME_OBJECT && pItem->zAlias ){
            sqlite3RenameTokenRemap(pParse, 0, (void*)&pExpr->y.pTab);
          }
        }
        hCol = sqlite3StrIHash(zCol);
        for(j=0, pCol=pTab->aCol; j<pTab->nCol; j++, pCol++){
          if( pCol->hName==hCol
           && sqlite3StrICmp(pCol->zCnName, zCol)==0
          ){
            if( cnt>0 ){
              if( pItem->fg.isUsing==0
               || sqlite3IdListIndex(pItem->u3.pUsing, zCol)<0
              ){
                /* Two or more tables have the same column name which is
                ** not joined by USING.  This is an error.  Signal as much
                ** by clearing pFJMatch and letting cnt go above 1. */
                sqlite3ExprListDelete(db, pFJMatch);
                pFJMatch = 0;
              }else
              if( (pItem->fg.jointype & JT_RIGHT)==0 ){
                /* An INNER or LEFT JOIN.  Use the left-most table */
                continue;
              }else
              if( (pItem->fg.jointype & JT_LEFT)==0 ){
                /* A RIGHT JOIN.  Use the right-most table */
                cnt = 0;
                sqlite3ExprListDelete(db, pFJMatch);
                pFJMatch = 0;
              }else{
                /* For a FULL JOIN, we must construct a coalesce() func */
                extendFJMatch(pParse, &pFJMatch, pMatch, pExpr->iColumn);
              }
            }
            cnt++;
            pMatch = pItem;
            /* Substitute the rowid (column -1) for the INTEGER PRIMARY KEY */
            pExpr->iColumn = j==pTab->iPKey ? -1 : (i16)j;
            if( pItem->fg.isNestedFrom ){
              sqlite3SrcItemColumnUsed(pItem, j);
            }
            break;
          }
        }
        if( 0==cnt && VisibleRowid(pTab) ){
          /* pTab is a potential ROWID match.  Keep track of it and match
          ** the ROWID later if that seems appropriate.  (Search for "cntTab"
          ** to find related code.)  Only allow a ROWID match if there is
          ** a single ROWID match candidate.
          */
#ifdef SQLITE_ALLOW_ROWID_IN_VIEW
          /* In SQLITE_ALLOW_ROWID_IN_VIEW mode, allow a ROWID match
          ** if there is a single VIEW candidate or if there is a single
          ** non-VIEW candidate plus multiple VIEW candidates.  In other
          ** words non-VIEW candidate terms take precedence over VIEWs.
          */
          if( cntTab==0
           || (cntTab==1
               && ALWAYS(pMatch!=0)
               && ALWAYS(pMatch->pTab!=0)
               && (pMatch->pTab->tabFlags & TF_Ephemeral)!=0
               && (pTab->tabFlags & TF_Ephemeral)==0)
          ){
            cntTab = 1;
            pMatch = pItem;
          }else{
            cntTab++;
          }
#else
          /* The (much more common) non-SQLITE_ALLOW_ROWID_IN_VIEW case is
          ** simpler since we require exactly one candidate, which will
          ** always be a non-VIEW
          */
          cntTab++;
          pMatch = pItem;
#endif
        }
      }
      if( pMatch ){
        pExpr->iTable = pMatch->iCursor;
        assert( ExprUseYTab(pExpr) );
        pExpr->y.pTab = pMatch->pTab;
        if( (pMatch->fg.jointype & (JT_LEFT|JT_LTORJ))!=0 ){
          ExprSetProperty(pExpr, EP_CanBeNull);
        }
        pSchema = pExpr->y.pTab->pSchema;
      }
    } /* if( pSrcList ) */

#if !defined(SQLITE_OMIT_TRIGGER) || !defined(SQLITE_OMIT_UPSERT)
    /* If we have not already resolved the name, then maybe
    ** it is a new.* or old.* trigger argument reference.  Or
    ** maybe it is an excluded.* from an upsert.  Or maybe it is
    ** a reference in the RETURNING clause to a table being modified.
    */
    if( cnt==0 && zDb==0 ){
      pTab = 0;
#ifndef SQLITE_OMIT_TRIGGER
      if( pParse->pTriggerTab!=0 ){
        int op = pParse->eTriggerOp;
        assert( op==TK_DELETE || op==TK_UPDATE || op==TK_INSERT );
        if( pParse->bReturning ){
          if( (pNC->ncFlags & NC_UBaseReg)!=0
           && ALWAYS(zTab==0
                     || sqlite3StrICmp(zTab,pParse->pTriggerTab->zName)==0
                     || isValidSchemaTableName(zTab, pParse->pTriggerTab, 0))
          ){
            pExpr->iTable = op!=TK_DELETE;
            pTab = pParse->pTriggerTab;
          }
        }else if( op!=TK_DELETE && zTab && sqlite3StrICmp("new",zTab) == 0 ){
          pExpr->iTable = 1;
          pTab = pParse->pTriggerTab;
        }else if( op!=TK_INSERT && zTab && sqlite3StrICmp("old",zTab)==0 ){
          pExpr->iTable = 0;
          pTab = pParse->pTriggerTab;
        }
      }
#endif /* SQLITE_OMIT_TRIGGER */
#ifndef SQLITE_OMIT_UPSERT
      if( (pNC->ncFlags & NC_UUpsert)!=0 && zTab!=0 ){
        Upsert *pUpsert = pNC->uNC.pUpsert;
        if( pUpsert && sqlite3StrICmp("excluded",zTab)==0 ){
          pTab = pUpsert->pUpsertSrc->a[0].pTab;
          pExpr->iTable = EXCLUDED_TABLE_NUMBER;
        }
      }
#endif /* SQLITE_OMIT_UPSERT */

      if( pTab ){
        int iCol;
        u8 hCol = sqlite3StrIHash(zCol);
        pSchema = pTab->pSchema;
        cntTab++;
        for(iCol=0, pCol=pTab->aCol; iCol<pTab->nCol; iCol++, pCol++){
          if( pCol->hName==hCol
           && sqlite3StrICmp(pCol->zCnName, zCol)==0
          ){
            if( iCol==pTab->iPKey ){
              iCol = -1;
            }
            break;
          }
        }
        if( iCol>=pTab->nCol && sqlite3IsRowid(zCol) && VisibleRowid(pTab) ){
          /* IMP: R-51414-32910 */
          iCol = -1;
        }
        if( iCol<pTab->nCol ){
          cnt++;
          pMatch = 0;
#ifndef SQLITE_OMIT_UPSERT
          if( pExpr->iTable==EXCLUDED_TABLE_NUMBER ){
            testcase( iCol==(-1) );
            assert( ExprUseYTab(pExpr) );
            if( IN_RENAME_OBJECT ){
              pExpr->iColumn = iCol;
              pExpr->y.pTab = pTab;
              eNewExprOp = TK_COLUMN;
            }else{
              pExpr->iTable = pNC->uNC.pUpsert->regData +
                 sqlite3TableColumnToStorage(pTab, iCol);
              eNewExprOp = TK_REGISTER;
            }
          }else
#endif /* SQLITE_OMIT_UPSERT */
          {
            assert( ExprUseYTab(pExpr) );
            pExpr->y.pTab = pTab;
            if( pParse->bReturning ){
              eNewExprOp = TK_REGISTER;
              pExpr->op2 = TK_COLUMN;
              pExpr->iColumn = iCol;
              pExpr->iTable = pNC->uNC.iBaseReg + (pTab->nCol+1)*pExpr->iTable +
                 sqlite3TableColumnToStorage(pTab, iCol) + 1;
            }else{
              pExpr->iColumn = (i16)iCol;
              eNewExprOp = TK_TRIGGER;
#ifndef SQLITE_OMIT_TRIGGER
              if( iCol<0 ){
                pExpr->affExpr = SQLITE_AFF_INTEGER;
              }else if( pExpr->iTable==0 ){
                testcase( iCol==31 );
                testcase( iCol==32 );
                pParse->oldmask |= (iCol>=32 ? 0xffffffff : (((u32)1)<<iCol));
              }else{
                testcase( iCol==31 );
                testcase( iCol==32 );
                pParse->newmask |= (iCol>=32 ? 0xffffffff : (((u32)1)<<iCol));
              }
#endif /* SQLITE_OMIT_TRIGGER */
            }
          }
        }
      }
    }
#endif /* !defined(SQLITE_OMIT_TRIGGER) || !defined(SQLITE_OMIT_UPSERT) */

    /*
    ** Perhaps the name is a reference to the ROWID
    */
    if( cnt==0
     && cntTab>=1
     && pMatch
     && (pNC->ncFlags & (NC_IdxExpr|NC_GenCol))==0
     && sqlite3IsRowid(zCol)
     && ALWAYS(VisibleRowid(pMatch->pTab) || pMatch->fg.isNestedFrom)
    ){
      cnt = cntTab;
#if SQLITE_ALLOW_ROWID_IN_VIEW+0==2
      if( pMatch->pTab!=0 && IsView(pMatch->pTab) ){
        eNewExprOp = TK_NULL;
      }
#endif
      if( pMatch->fg.isNestedFrom==0 ) pExpr->iColumn = -1;
      pExpr->affExpr = SQLITE_AFF_INTEGER;
    }

    /*
    ** If the input is of the form Z (not Y.Z or X.Y.Z) then the name Z
    ** might refer to an result-set alias.  This happens, for example, when
    ** we are resolving names in the WHERE clause of the following command:
    **
    **     SELECT a+b AS x FROM table WHERE x<10;
    **
    ** In cases like this, replace pExpr with a copy of the expression that
    ** forms the result set entry ("a+b" in the example) and return immediately.
    ** Note that the expression in the result set should have already been
    ** resolved by the time the WHERE clause is resolved.
    **
    ** The ability to use an output result-set column in the WHERE, GROUP BY,
    ** or HAVING clauses, or as part of a larger expression in the ORDER BY
    ** clause is not standard SQL.  This is a (goofy) SQLite extension, that
    ** is supported for backwards compatibility only. Hence, we issue a warning
    ** on sqlite3_log() whenever the capability is used.
    */
    if( cnt==0
     && (pNC->ncFlags & NC_UEList)!=0
     && zTab==0
    ){
      pEList = pNC->uNC.pEList;
      assert( pEList!=0 );
      for(j=0; j<pEList->nExpr; j++){
        char *zAs = pEList->a[j].zEName;
        if( pEList->a[j].fg.eEName==ENAME_NAME
         && sqlite3_stricmp(zAs, zCol)==0
        ){
          Expr *pOrig;
          assert( pExpr->pLeft==0 && pExpr->pRight==0 );
          assert( ExprUseXList(pExpr)==0 || pExpr->x.pList==0 );
          assert( ExprUseXSelect(pExpr)==0 || pExpr->x.pSelect==0 );
          pOrig = pEList->a[j].pExpr;
          if( (pNC->ncFlags&NC_AllowAgg)==0 && ExprHasProperty(pOrig, EP_Agg) ){
            sqlite3ErrorMsg(pParse, "misuse of aliased aggregate %s", zAs);
            return WRC_Abort;
          }
          if( ExprHasProperty(pOrig, EP_Win)
           && ((pNC->ncFlags&NC_AllowWin)==0 || pNC!=pTopNC )
          ){
            sqlite3ErrorMsg(pParse, "misuse of aliased window function %s",zAs);
            return WRC_Abort;
          }
          if( sqlite3ExprVectorSize(pOrig)!=1 ){
            sqlite3ErrorMsg(pParse, "row value misused");
            return WRC_Abort;
          }
          resolveAlias(pParse, pEList, j, pExpr, nSubquery);
          cnt = 1;
          pMatch = 0;
          assert( zTab==0 && zDb==0 );
          if( IN_RENAME_OBJECT ){
            sqlite3RenameTokenRemap(pParse, 0, (void*)pExpr);
          }
          goto lookupname_end;
        }
      }
    }

    /* Advance to the next name context.  The loop will exit when either
    ** we have a match (cnt>0) or when we run out of name contexts.
    */
    if( cnt ) break;
    pNC = pNC->pNext;
    nSubquery++;
  }while( pNC );


  /*
  ** If X and Y are NULL (in other words if only the column name Z is
  ** supplied) and the value of Z is enclosed in double-quotes, then
  ** Z is a string literal if it doesn't match any column names.  In that
  ** case, we need to return right away and not make any changes to
  ** pExpr.
  **
  ** Because no reference was made to outer contexts, the pNC->nRef
  ** fields are not changed in any context.
  */
  if( cnt==0 && zTab==0 ){
    assert( pExpr->op==TK_ID );
    if( ExprHasProperty(pExpr,EP_DblQuoted)
     && areDoubleQuotedStringsEnabled(db, pTopNC)
    ){
      /* If a double-quoted identifier does not match any known column name,
      ** then treat it as a string.
      **
      ** This hack was added in the early days of SQLite in a misguided attempt
      ** to be compatible with MySQL 3.x, which used double-quotes for strings.
      ** I now sorely regret putting in this hack. The effect of this hack is
      ** that misspelled identifier names are silently converted into strings
      ** rather than causing an error, to the frustration of countless
      ** programmers. To all those frustrated programmers, my apologies.
      **
      ** Someday, I hope to get rid of this hack. Unfortunately there is
      ** a huge amount of legacy SQL that uses it. So for now, we just
      ** issue a warning.
      */
      sqlite3_log(SQLITE_WARNING,
        "double-quoted string literal: \"%w\"", zCol);
#ifdef SQLITE_ENABLE_NORMALIZE
      sqlite3VdbeAddDblquoteStr(db, pParse->pVdbe, zCol);
#endif
      pExpr->op = TK_STRING;
      memset(&pExpr->y, 0, sizeof(pExpr->y));
      return WRC_Prune;
    }
    if( sqlite3ExprIdToTrueFalse(pExpr) ){
      return WRC_Prune;
    }
  }

  /*
  ** cnt==0 means there was not match.
  ** cnt>1 means there were two or more matches.
  **
  ** cnt==0 is always an error.  cnt>1 is often an error, but might
  ** be multiple matches for a NATURAL LEFT JOIN or a LEFT JOIN USING.
  */
  assert( pFJMatch==0 || cnt>0 );
  assert( !ExprHasProperty(pExpr, EP_xIsSelect|EP_IntValue) );
  if( cnt!=1 ){
    const char *zErr;
    if( pFJMatch ){
      if( pFJMatch->nExpr==cnt-1 ){
        if( ExprHasProperty(pExpr,EP_Leaf) ){
          ExprClearProperty(pExpr,EP_Leaf);
        }else{
          sqlite3ExprDelete(db, pExpr->pLeft);
          pExpr->pLeft = 0;
          sqlite3ExprDelete(db, pExpr->pRight);
          pExpr->pRight = 0;
        }
        extendFJMatch(pParse, &pFJMatch, pMatch, pExpr->iColumn);
        pExpr->op = TK_FUNCTION;
        pExpr->u.zToken = "coalesce";
        pExpr->x.pList = pFJMatch;
        cnt = 1;
        goto lookupname_end;
      }else{
        sqlite3ExprListDelete(db, pFJMatch);
        pFJMatch = 0;
      }
    }
    zErr = cnt==0 ? "no such column" : "ambiguous column name";
    if( zDb ){
      sqlite3ErrorMsg(pParse, "%s: %s.%s.%s", zErr, zDb, zTab, zCol);
    }else if( zTab ){
      sqlite3ErrorMsg(pParse, "%s: %s.%s", zErr, zTab, zCol);
    }else if( cnt==0 && ExprHasProperty(pRight,EP_DblQuoted) ){
      sqlite3ErrorMsg(pParse, "%s: \"%s\" - should this be a"
                              " string literal in single-quotes?",
                              zErr, zCol);
    }else{
      sqlite3ErrorMsg(pParse, "%s: %s", zErr, zCol);
    }
    sqlite3RecordErrorOffsetOfExpr(pParse->db, pExpr);
    pParse->checkSchema = 1;
    pTopNC->nNcErr++;
    eNewExprOp = TK_NULL;
  }
  assert( pFJMatch==0 );

  /* Remove all substructure from pExpr */
  if( !ExprHasProperty(pExpr,(EP_TokenOnly|EP_Leaf)) ){
    sqlite3ExprDelete(db, pExpr->pLeft);
    pExpr->pLeft = 0;
    sqlite3ExprDelete(db, pExpr->pRight);
    pExpr->pRight = 0;
    ExprSetProperty(pExpr, EP_Leaf);
  }

  /* If a column from a table in pSrcList is referenced, then record
  ** this fact in the pSrcList.a[].colUsed bitmask.  Column 0 causes
  ** bit 0 to be set.  Column 1 sets bit 1.  And so forth.  Bit 63 is
  ** set if the 63rd or any subsequent column is used.
  **
  ** The colUsed mask is an optimization used to help determine if an
  ** index is a covering index.  The correct answer is still obtained
  ** if the mask contains extra set bits.  However, it is important to
  ** avoid setting bits beyond the maximum column number of the table.
  ** (See ticket [b92e5e8ec2cdbaa1]).
  **
  ** If a generated column is referenced, set bits for every column
  ** of the table.
  */
  if( pMatch ){
    if( pExpr->iColumn>=0 ){
      pMatch->colUsed |= sqlite3ExprColUsed(pExpr);
    }else{
      pMatch->fg.rowidUsed = 1;
    }
  }

  pExpr->op = eNewExprOp;
lookupname_end:
  if( cnt==1 ){
    assert( pNC!=0 );
#ifndef SQLITE_OMIT_AUTHORIZATION
    if( pParse->db->xAuth
     && (pExpr->op==TK_COLUMN || pExpr->op==TK_TRIGGER)
    ){
      sqlite3AuthRead(pParse, pExpr, pSchema, pNC->pSrcList);
    }
#endif
    /* Increment the nRef value on all name contexts from TopNC up to
    ** the point where the name matched. */
    for(;;){
      assert( pTopNC!=0 );
      pTopNC->nRef++;
      if( pTopNC==pNC ) break;
      pTopNC = pTopNC->pNext;
    }
    return WRC_Prune;
  } else {
    return WRC_Abort;
  }
}
