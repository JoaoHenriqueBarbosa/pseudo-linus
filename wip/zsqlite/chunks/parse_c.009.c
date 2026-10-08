        break;
      case 88: /* selectnowith ::= selectnowith multiselect_op oneselect */
{
  Select *pRhs = yymsp[0].minor.yy555;
  Select *pLhs = yymsp[-2].minor.yy555;
  if( pRhs && pRhs->pPrior ){
    SrcList *pFrom;
    Token x;
    x.n = 0;
    parserDoubleLinkSelect(pParse, pRhs);
    pFrom = sqlite3SrcListAppendFromTerm(pParse,0,0,0,&x,pRhs,0);
    pRhs = sqlite3SelectNew(pParse,0,pFrom,0,0,0,0,0,0);
  }
  if( pRhs ){
    pRhs->op = (u8)yymsp[-1].minor.yy144;
    pRhs->pPrior = pLhs;
    if( ALWAYS(pLhs) ) pLhs->selFlags &= ~SF_MultiValue;
    pRhs->selFlags &= ~SF_MultiValue;
    if( yymsp[-1].minor.yy144!=TK_ALL ) pParse->hasCompound = 1;
  }else{
    sqlite3SelectDelete(pParse->db, pLhs);
  }
  yymsp[-2].minor.yy555 = pRhs;
}
        break;
      case 89: /* multiselect_op ::= UNION */
      case 91: /* multiselect_op ::= EXCEPT|INTERSECT */ yytestcase(yyruleno==91);
{yymsp[0].minor.yy144 = yymsp[0].major; /*A-overwrites-OP*/}
        break;
      case 90: /* multiselect_op ::= UNION ALL */
{yymsp[-1].minor.yy144 = TK_ALL;}
        break;
      case 92: /* oneselect ::= SELECT distinct selcollist from where_opt groupby_opt having_opt orderby_opt limit_opt */
{
  yymsp[-8].minor.yy555 = sqlite3SelectNew(pParse,yymsp[-6].minor.yy14,yymsp[-5].minor.yy203,yymsp[-4].minor.yy454,yymsp[-3].minor.yy14,yymsp[-2].minor.yy454,yymsp[-1].minor.yy14,yymsp[-7].minor.yy144,yymsp[0].minor.yy454);
}
        break;
      case 93: /* oneselect ::= SELECT distinct selcollist from where_opt groupby_opt having_opt window_clause orderby_opt limit_opt */
{
  yymsp[-9].minor.yy555 = sqlite3SelectNew(pParse,yymsp[-7].minor.yy14,yymsp[-6].minor.yy203,yymsp[-5].minor.yy454,yymsp[-4].minor.yy14,yymsp[-3].minor.yy454,yymsp[-1].minor.yy14,yymsp[-8].minor.yy144,yymsp[0].minor.yy454);
  if( yymsp[-9].minor.yy555 ){
    yymsp[-9].minor.yy555->pWinDefn = yymsp[-2].minor.yy211;
  }else{
    sqlite3WindowListDelete(pParse->db, yymsp[-2].minor.yy211);
  }
}
        break;
      case 94: /* values ::= VALUES LP nexprlist RP */
{
  yymsp[-3].minor.yy555 = sqlite3SelectNew(pParse,yymsp[-1].minor.yy14,0,0,0,0,0,SF_Values,0);
}
        break;
      case 95: /* oneselect ::= mvalues */
{
  sqlite3MultiValuesEnd(pParse, yymsp[0].minor.yy555);
}
        break;
      case 96: /* mvalues ::= values COMMA LP nexprlist RP */
      case 97: /* mvalues ::= mvalues COMMA LP nexprlist RP */ yytestcase(yyruleno==97);
{
  yymsp[-4].minor.yy555 = sqlite3MultiValues(pParse, yymsp[-4].minor.yy555, yymsp[-1].minor.yy14);
}
        break;
      case 98: /* distinct ::= DISTINCT */
{yymsp[0].minor.yy144 = SF_Distinct;}
        break;
      case 99: /* distinct ::= ALL */
{yymsp[0].minor.yy144 = SF_All;}
        break;
      case 101: /* sclp ::= */
      case 134: /* orderby_opt ::= */ yytestcase(yyruleno==134);
      case 144: /* groupby_opt ::= */ yytestcase(yyruleno==144);
      case 234: /* exprlist ::= */ yytestcase(yyruleno==234);
      case 237: /* paren_exprlist ::= */ yytestcase(yyruleno==237);
      case 242: /* eidlist_opt ::= */ yytestcase(yyruleno==242);
{yymsp[1].minor.yy14 = 0;}
        break;
      case 102: /* selcollist ::= sclp scanpt expr scanpt as */
{
   yymsp[-4].minor.yy14 = sqlite3ExprListAppend(pParse, yymsp[-4].minor.yy14, yymsp[-2].minor.yy454);
   if( yymsp[0].minor.yy0.n>0 ) sqlite3ExprListSetName(pParse, yymsp[-4].minor.yy14, &yymsp[0].minor.yy0, 1);
   sqlite3ExprListSetSpan(pParse,yymsp[-4].minor.yy14,yymsp[-3].minor.yy168,yymsp[-1].minor.yy168);
}
        break;
      case 103: /* selcollist ::= sclp scanpt STAR */
{
  Expr *p = sqlite3Expr(pParse->db, TK_ASTERISK, 0);
  sqlite3ExprSetErrorOffset(p, (int)(yymsp[0].minor.yy0.z - pParse->zTail));
  yymsp[-2].minor.yy14 = sqlite3ExprListAppend(pParse, yymsp[-2].minor.yy14, p);
}
        break;
      case 104: /* selcollist ::= sclp scanpt nm DOT STAR */
{
  Expr *pRight, *pLeft, *pDot;
  pRight = sqlite3PExpr(pParse, TK_ASTERISK, 0, 0);
  sqlite3ExprSetErrorOffset(pRight, (int)(yymsp[0].minor.yy0.z - pParse->zTail));
  pLeft = tokenExpr(pParse, TK_ID, yymsp[-2].minor.yy0);
  pDot = sqlite3PExpr(pParse, TK_DOT, pLeft, pRight);
  yymsp[-4].minor.yy14 = sqlite3ExprListAppend(pParse,yymsp[-4].minor.yy14, pDot);
}
        break;
      case 105: /* as ::= AS nm */
      case 117: /* dbnm ::= DOT nm */ yytestcase(yyruleno==117);
      case 258: /* plus_num ::= PLUS INTEGER|FLOAT */ yytestcase(yyruleno==258);
      case 259: /* minus_num ::= MINUS INTEGER|FLOAT */ yytestcase(yyruleno==259);
{yymsp[-1].minor.yy0 = yymsp[0].minor.yy0;}
        break;
      case 107: /* from ::= */
      case 110: /* stl_prefix ::= */ yytestcase(yyruleno==110);
{yymsp[1].minor.yy203 = 0;}
        break;
      case 108: /* from ::= FROM seltablist */
{
  yymsp[-1].minor.yy203 = yymsp[0].minor.yy203;
  sqlite3SrcListShiftJoinType(pParse,yymsp[-1].minor.yy203);
}
        break;
      case 109: /* stl_prefix ::= seltablist joinop */
{
   if( ALWAYS(yymsp[-1].minor.yy203 && yymsp[-1].minor.yy203->nSrc>0) ) yymsp[-1].minor.yy203->a[yymsp[-1].minor.yy203->nSrc-1].fg.jointype = (u8)yymsp[0].minor.yy144;
}
        break;
      case 111: /* seltablist ::= stl_prefix nm dbnm as on_using */
{
  yymsp[-4].minor.yy203 = sqlite3SrcListAppendFromTerm(pParse,yymsp[-4].minor.yy203,&yymsp[-3].minor.yy0,&yymsp[-2].minor.yy0,&yymsp[-1].minor.yy0,0,&yymsp[0].minor.yy269);
}
        break;
      case 112: /* seltablist ::= stl_prefix nm dbnm as indexed_by on_using */
{
  yymsp[-5].minor.yy203 = sqlite3SrcListAppendFromTerm(pParse,yymsp[-5].minor.yy203,&yymsp[-4].minor.yy0,&yymsp[-3].minor.yy0,&yymsp[-2].minor.yy0,0,&yymsp[0].minor.yy269);
  sqlite3SrcListIndexedBy(pParse, yymsp[-5].minor.yy203, &yymsp[-1].minor.yy0);
}
        break;
      case 113: /* seltablist ::= stl_prefix nm dbnm LP exprlist RP as on_using */
{
  yymsp[-7].minor.yy203 = sqlite3SrcListAppendFromTerm(pParse,yymsp[-7].minor.yy203,&yymsp[-6].minor.yy0,&yymsp[-5].minor.yy0,&yymsp[-1].minor.yy0,0,&yymsp[0].minor.yy269);
  sqlite3SrcListFuncArgs(pParse, yymsp[-7].minor.yy203, yymsp[-3].minor.yy14);
}
        break;
      case 114: /* seltablist ::= stl_prefix LP select RP as on_using */
{
    yymsp[-5].minor.yy203 = sqlite3SrcListAppendFromTerm(pParse,yymsp[-5].minor.yy203,0,0,&yymsp[-1].minor.yy0,yymsp[-3].minor.yy555,&yymsp[0].minor.yy269);
  }
        break;
      case 115: /* seltablist ::= stl_prefix LP seltablist RP as on_using */
{
    if( yymsp[-5].minor.yy203==0 && yymsp[-1].minor.yy0.n==0 && yymsp[0].minor.yy269.pOn==0 && yymsp[0].minor.yy269.pUsing==0 ){
      yymsp[-5].minor.yy203 = yymsp[-3].minor.yy203;
    }else if( ALWAYS(yymsp[-3].minor.yy203!=0) && yymsp[-3].minor.yy203->nSrc==1 ){
      yymsp[-5].minor.yy203 = sqlite3SrcListAppendFromTerm(pParse,yymsp[-5].minor.yy203,0,0,&yymsp[-1].minor.yy0,0,&yymsp[0].minor.yy269);
      if( yymsp[-5].minor.yy203 ){
        SrcItem *pNew = &yymsp[-5].minor.yy203->a[yymsp[-5].minor.yy203->nSrc-1];
        SrcItem *pOld = yymsp[-3].minor.yy203->a;
        pNew->zName = pOld->zName;
        pNew->zDatabase = pOld->zDatabase;
        pNew->pSelect = pOld->pSelect;
        if( pNew->pSelect && (pNew->pSelect->selFlags & SF_NestedFrom)!=0 ){
          pNew->fg.isNestedFrom = 1;
        }
        if( pOld->fg.isTabFunc ){
          pNew->u1.pFuncArg = pOld->u1.pFuncArg;
          pOld->u1.pFuncArg = 0;
          pOld->fg.isTabFunc = 0;
          pNew->fg.isTabFunc = 1;
        }
        pOld->zName = pOld->zDatabase = 0;
        pOld->pSelect = 0;
      }
      sqlite3SrcListDelete(pParse->db, yymsp[-3].minor.yy203);
    }else{
      Select *pSubquery;
      sqlite3SrcListShiftJoinType(pParse,yymsp[-3].minor.yy203);
      pSubquery = sqlite3SelectNew(pParse,0,yymsp[-3].minor.yy203,0,0,0,0,SF_NestedFrom,0);
      yymsp[-5].minor.yy203 = sqlite3SrcListAppendFromTerm(pParse,yymsp[-5].minor.yy203,0,0,&yymsp[-1].minor.yy0,pSubquery,&yymsp[0].minor.yy269);
    }
  }
        break;
      case 116: /* dbnm ::= */
      case 131: /* indexed_opt ::= */ yytestcase(yyruleno==131);
{yymsp[1].minor.yy0.z=0; yymsp[1].minor.yy0.n=0;}
        break;
      case 118: /* fullname ::= nm */
{
  yylhsminor.yy203 = sqlite3SrcListAppend(pParse,0,&yymsp[0].minor.yy0,0);
  if( IN_RENAME_OBJECT && yylhsminor.yy203 ) sqlite3RenameTokenMap(pParse, yylhsminor.yy203->a[0].zName, &yymsp[0].minor.yy0);
}
  yymsp[0].minor.yy203 = yylhsminor.yy203;
        break;
      case 119: /* fullname ::= nm DOT nm */
{
  yylhsminor.yy203 = sqlite3SrcListAppend(pParse,0,&yymsp[-2].minor.yy0,&yymsp[0].minor.yy0);
  if( IN_RENAME_OBJECT && yylhsminor.yy203 ) sqlite3RenameTokenMap(pParse, yylhsminor.yy203->a[0].zName, &yymsp[0].minor.yy0);
}
  yymsp[-2].minor.yy203 = yylhsminor.yy203;
        break;
      case 120: /* xfullname ::= nm */
{yymsp[0].minor.yy203 = sqlite3SrcListAppend(pParse,0,&yymsp[0].minor.yy0,0); /*A-overwrites-X*/}
        break;
      case 121: /* xfullname ::= nm DOT nm */
{yymsp[-2].minor.yy203 = sqlite3SrcListAppend(pParse,0,&yymsp[-2].minor.yy0,&yymsp[0].minor.yy0); /*A-overwrites-X*/}
        break;
      case 122: /* xfullname ::= nm DOT nm AS nm */
{
   yymsp[-4].minor.yy203 = sqlite3SrcListAppend(pParse,0,&yymsp[-4].minor.yy0,&yymsp[-2].minor.yy0); /*A-overwrites-X*/
   if( yymsp[-4].minor.yy203 ) yymsp[-4].minor.yy203->a[0].zAlias = sqlite3NameFromToken(pParse->db, &yymsp[0].minor.yy0);
}
        break;
      case 123: /* xfullname ::= nm AS nm */
{
   yymsp[-2].minor.yy203 = sqlite3SrcListAppend(pParse,0,&yymsp[-2].minor.yy0,0); /*A-overwrites-X*/
   if( yymsp[-2].minor.yy203 ) yymsp[-2].minor.yy203->a[0].zAlias = sqlite3NameFromToken(pParse->db, &yymsp[0].minor.yy0);
}
        break;
      case 124: /* joinop ::= COMMA|JOIN */
{ yymsp[0].minor.yy144 = JT_INNER; }
        break;
      case 125: /* joinop ::= JOIN_KW JOIN */
{yymsp[-1].minor.yy144 = sqlite3JoinType(pParse,&yymsp[-1].minor.yy0,0,0);  /*X-overwrites-A*/}
        break;
      case 126: /* joinop ::= JOIN_KW nm JOIN */
{yymsp[-2].minor.yy144 = sqlite3JoinType(pParse,&yymsp[-2].minor.yy0,&yymsp[-1].minor.yy0,0); /*X-overwrites-A*/}
        break;
      case 127: /* joinop ::= JOIN_KW nm nm JOIN */
{yymsp[-3].minor.yy144 = sqlite3JoinType(pParse,&yymsp[-3].minor.yy0,&yymsp[-2].minor.yy0,&yymsp[-1].minor.yy0);/*X-overwrites-A*/}
        break;
      case 128: /* on_using ::= ON expr */
{yymsp[-1].minor.yy269.pOn = yymsp[0].minor.yy454; yymsp[-1].minor.yy269.pUsing = 0;}
        break;
      case 129: /* on_using ::= USING LP idlist RP */
{yymsp[-3].minor.yy269.pOn = 0; yymsp[-3].minor.yy269.pUsing = yymsp[-1].minor.yy132;}
        break;
      case 130: /* on_using ::= */
{yymsp[1].minor.yy269.pOn = 0; yymsp[1].minor.yy269.pUsing = 0;}
        break;
      case 132: /* indexed_by ::= INDEXED BY nm */
{yymsp[-2].minor.yy0 = yymsp[0].minor.yy0;}
        break;
      case 133: /* indexed_by ::= NOT INDEXED */
{yymsp[-1].minor.yy0.z=0; yymsp[-1].minor.yy0.n=1;}
        break;
      case 135: /* orderby_opt ::= ORDER BY sortlist */
      case 145: /* groupby_opt ::= GROUP BY nexprlist */ yytestcase(yyruleno==145);
{yymsp[-2].minor.yy14 = yymsp[0].minor.yy14;}
        break;
      case 136: /* sortlist ::= sortlist COMMA expr sortorder nulls */
{
  yymsp[-4].minor.yy14 = sqlite3ExprListAppend(pParse,yymsp[-4].minor.yy14,yymsp[-2].minor.yy454);
  sqlite3ExprListSetSortOrder(yymsp[-4].minor.yy14,yymsp[-1].minor.yy144,yymsp[0].minor.yy144);
}
        break;
      case 137: /* sortlist ::= expr sortorder nulls */
{
  yymsp[-2].minor.yy14 = sqlite3ExprListAppend(pParse,0,yymsp[-2].minor.yy454); /*A-overwrites-Y*/
  sqlite3ExprListSetSortOrder(yymsp[-2].minor.yy14,yymsp[-1].minor.yy144,yymsp[0].minor.yy144);
}
        break;
      case 138: /* sortorder ::= ASC */
{yymsp[0].minor.yy144 = SQLITE_SO_ASC;}
        break;
      case 139: /* sortorder ::= DESC */
{yymsp[0].minor.yy144 = SQLITE_SO_DESC;}
        break;
      case 140: /* sortorder ::= */
      case 143: /* nulls ::= */ yytestcase(yyruleno==143);
{yymsp[1].minor.yy144 = SQLITE_SO_UNDEFINED;}
        break;
      case 141: /* nulls ::= NULLS FIRST */
{yymsp[-1].minor.yy144 = SQLITE_SO_ASC;}
        break;
      case 142: /* nulls ::= NULLS LAST */
{yymsp[-1].minor.yy144 = SQLITE_SO_DESC;}
        break;
      case 146: /* having_opt ::= */
      case 148: /* limit_opt ::= */ yytestcase(yyruleno==148);
      case 153: /* where_opt ::= */ yytestcase(yyruleno==153);
      case 155: /* where_opt_ret ::= */ yytestcase(yyruleno==155);
      case 232: /* case_else ::= */ yytestcase(yyruleno==232);
      case 233: /* case_operand ::= */ yytestcase(yyruleno==233);
      case 252: /* vinto ::= */ yytestcase(yyruleno==252);
{yymsp[1].minor.yy454 = 0;}
        break;
      case 147: /* having_opt ::= HAVING expr */
      case 154: /* where_opt ::= WHERE expr */ yytestcase(yyruleno==154);
      case 156: /* where_opt_ret ::= WHERE expr */ yytestcase(yyruleno==156);
      case 231: /* case_else ::= ELSE expr */ yytestcase(yyruleno==231);
      case 251: /* vinto ::= INTO expr */ yytestcase(yyruleno==251);
{yymsp[-1].minor.yy454 = yymsp[0].minor.yy454;}
        break;
      case 149: /* limit_opt ::= LIMIT expr */
{yymsp[-1].minor.yy454 = sqlite3PExpr(pParse,TK_LIMIT,yymsp[0].minor.yy454,0);}
        break;
      case 150: /* limit_opt ::= LIMIT expr OFFSET expr */
{yymsp[-3].minor.yy454 = sqlite3PExpr(pParse,TK_LIMIT,yymsp[-2].minor.yy454,yymsp[0].minor.yy454);}
        break;
      case 151: /* limit_opt ::= LIMIT expr COMMA expr */
{yymsp[-3].minor.yy454 = sqlite3PExpr(pParse,TK_LIMIT,yymsp[0].minor.yy454,yymsp[-2].minor.yy454);}
        break;
      case 152: /* cmd ::= with DELETE FROM xfullname indexed_opt where_opt_ret */
{
  sqlite3SrcListIndexedBy(pParse, yymsp[-2].minor.yy203, &yymsp[-1].minor.yy0);
  sqlite3DeleteFrom(pParse,yymsp[-2].minor.yy203,yymsp[0].minor.yy454,0,0);
}
        break;
      case 157: /* where_opt_ret ::= RETURNING selcollist */
{sqlite3AddReturning(pParse,yymsp[0].minor.yy14); yymsp[-1].minor.yy454 = 0;}
        break;
      case 158: /* where_opt_ret ::= WHERE expr RETURNING selcollist */
{sqlite3AddReturning(pParse,yymsp[0].minor.yy14); yymsp[-3].minor.yy454 = yymsp[-2].minor.yy454;}
        break;
      case 159: /* cmd ::= with UPDATE orconf xfullname indexed_opt SET setlist from where_opt_ret */
{
  sqlite3SrcListIndexedBy(pParse, yymsp[-5].minor.yy203, &yymsp[-4].minor.yy0);
  sqlite3ExprListCheckLength(pParse,yymsp[-2].minor.yy14,"set list");
  if( yymsp[-1].minor.yy203 ){
    SrcList *pFromClause = yymsp[-1].minor.yy203;
    if( pFromClause->nSrc>1 ){
      Select *pSubquery;
      Token as;
      pSubquery = sqlite3SelectNew(pParse,0,pFromClause,0,0,0,0,SF_NestedFrom,0);
      as.n = 0;
      as.z = 0;
      pFromClause = sqlite3SrcListAppendFromTerm(pParse,0,0,0,&as,pSubquery,0);
    }
    yymsp[-5].minor.yy203 = sqlite3SrcListAppendList(pParse, yymsp[-5].minor.yy203, pFromClause);
  }
  sqlite3Update(pParse,yymsp[-5].minor.yy203,yymsp[-2].minor.yy14,yymsp[0].minor.yy454,yymsp[-6].minor.yy144,0,0,0);
}
        break;
      case 160: /* setlist ::= setlist COMMA nm EQ expr */
{
  yymsp[-4].minor.yy14 = sqlite3ExprListAppend(pParse, yymsp[-4].minor.yy14, yymsp[0].minor.yy454);
  sqlite3ExprListSetName(pParse, yymsp[-4].minor.yy14, &yymsp[-2].minor.yy0, 1);
}
        break;
      case 161: /* setlist ::= setlist COMMA LP idlist RP EQ expr */
{
  yymsp[-6].minor.yy14 = sqlite3ExprListAppendVector(pParse, yymsp[-6].minor.yy14, yymsp[-3].minor.yy132, yymsp[0].minor.yy454);
}
        break;
      case 162: /* setlist ::= nm EQ expr */
{
  yylhsminor.yy14 = sqlite3ExprListAppend(pParse, 0, yymsp[0].minor.yy454);
  sqlite3ExprListSetName(pParse, yylhsminor.yy14, &yymsp[-2].minor.yy0, 1);
}
  yymsp[-2].minor.yy14 = yylhsminor.yy14;
        break;
      case 163: /* setlist ::= LP idlist RP EQ expr */
{
  yymsp[-4].minor.yy14 = sqlite3ExprListAppendVector(pParse, 0, yymsp[-3].minor.yy132, yymsp[0].minor.yy454);
}
        break;
      case 164: /* cmd ::= with insert_cmd INTO xfullname idlist_opt select upsert */
{
  sqlite3Insert(pParse, yymsp[-3].minor.yy203, yymsp[-1].minor.yy555, yymsp[-2].minor.yy132, yymsp[-5].minor.yy144, yymsp[0].minor.yy122);
}
