        break;
      case 165: /* cmd ::= with insert_cmd INTO xfullname idlist_opt DEFAULT VALUES returning */
{
  sqlite3Insert(pParse, yymsp[-4].minor.yy203, 0, yymsp[-3].minor.yy132, yymsp[-6].minor.yy144, 0);
}
        break;
      case 166: /* upsert ::= */
{ yymsp[1].minor.yy122 = 0; }
        break;
      case 167: /* upsert ::= RETURNING selcollist */
{ yymsp[-1].minor.yy122 = 0; sqlite3AddReturning(pParse,yymsp[0].minor.yy14); }
        break;
      case 168: /* upsert ::= ON CONFLICT LP sortlist RP where_opt DO UPDATE SET setlist where_opt upsert */
{ yymsp[-11].minor.yy122 = sqlite3UpsertNew(pParse->db,yymsp[-8].minor.yy14,yymsp[-6].minor.yy454,yymsp[-2].minor.yy14,yymsp[-1].minor.yy454,yymsp[0].minor.yy122);}
        break;
      case 169: /* upsert ::= ON CONFLICT LP sortlist RP where_opt DO NOTHING upsert */
{ yymsp[-8].minor.yy122 = sqlite3UpsertNew(pParse->db,yymsp[-5].minor.yy14,yymsp[-3].minor.yy454,0,0,yymsp[0].minor.yy122); }
        break;
      case 170: /* upsert ::= ON CONFLICT DO NOTHING returning */
{ yymsp[-4].minor.yy122 = sqlite3UpsertNew(pParse->db,0,0,0,0,0); }
        break;
      case 171: /* upsert ::= ON CONFLICT DO UPDATE SET setlist where_opt returning */
{ yymsp[-7].minor.yy122 = sqlite3UpsertNew(pParse->db,0,0,yymsp[-2].minor.yy14,yymsp[-1].minor.yy454,0);}
        break;
      case 172: /* returning ::= RETURNING selcollist */
{sqlite3AddReturning(pParse,yymsp[0].minor.yy14);}
        break;
      case 175: /* idlist_opt ::= */
{yymsp[1].minor.yy132 = 0;}
        break;
      case 176: /* idlist_opt ::= LP idlist RP */
{yymsp[-2].minor.yy132 = yymsp[-1].minor.yy132;}
        break;
      case 177: /* idlist ::= idlist COMMA nm */
{yymsp[-2].minor.yy132 = sqlite3IdListAppend(pParse,yymsp[-2].minor.yy132,&yymsp[0].minor.yy0);}
        break;
      case 178: /* idlist ::= nm */
{yymsp[0].minor.yy132 = sqlite3IdListAppend(pParse,0,&yymsp[0].minor.yy0); /*A-overwrites-Y*/}
        break;
      case 179: /* expr ::= LP expr RP */
{yymsp[-2].minor.yy454 = yymsp[-1].minor.yy454;}
        break;
      case 180: /* expr ::= ID|INDEXED|JOIN_KW */
{yymsp[0].minor.yy454=tokenExpr(pParse,TK_ID,yymsp[0].minor.yy0); /*A-overwrites-X*/}
        break;
      case 181: /* expr ::= nm DOT nm */
{
  Expr *temp1 = tokenExpr(pParse,TK_ID,yymsp[-2].minor.yy0);
  Expr *temp2 = tokenExpr(pParse,TK_ID,yymsp[0].minor.yy0);
  yylhsminor.yy454 = sqlite3PExpr(pParse, TK_DOT, temp1, temp2);
}
  yymsp[-2].minor.yy454 = yylhsminor.yy454;
        break;
      case 182: /* expr ::= nm DOT nm DOT nm */
{
  Expr *temp1 = tokenExpr(pParse,TK_ID,yymsp[-4].minor.yy0);
  Expr *temp2 = tokenExpr(pParse,TK_ID,yymsp[-2].minor.yy0);
  Expr *temp3 = tokenExpr(pParse,TK_ID,yymsp[0].minor.yy0);
  Expr *temp4 = sqlite3PExpr(pParse, TK_DOT, temp2, temp3);
  if( IN_RENAME_OBJECT ){
    sqlite3RenameTokenRemap(pParse, 0, temp1);
  }
  yylhsminor.yy454 = sqlite3PExpr(pParse, TK_DOT, temp1, temp4);
}
  yymsp[-4].minor.yy454 = yylhsminor.yy454;
        break;
      case 183: /* term ::= NULL|FLOAT|BLOB */
      case 184: /* term ::= STRING */ yytestcase(yyruleno==184);
{yymsp[0].minor.yy454=tokenExpr(pParse,yymsp[0].major,yymsp[0].minor.yy0); /*A-overwrites-X*/}
        break;
      case 185: /* term ::= INTEGER */
{
  yylhsminor.yy454 = sqlite3ExprAlloc(pParse->db, TK_INTEGER, &yymsp[0].minor.yy0, 1);
  if( yylhsminor.yy454 ) yylhsminor.yy454->w.iOfst = (int)(yymsp[0].minor.yy0.z - pParse->zTail);
}
  yymsp[0].minor.yy454 = yylhsminor.yy454;
        break;
      case 186: /* expr ::= VARIABLE */
{
  if( !(yymsp[0].minor.yy0.z[0]=='#' && sqlite3Isdigit(yymsp[0].minor.yy0.z[1])) ){
    u32 n = yymsp[0].minor.yy0.n;
    yymsp[0].minor.yy454 = tokenExpr(pParse, TK_VARIABLE, yymsp[0].minor.yy0);
    sqlite3ExprAssignVarNumber(pParse, yymsp[0].minor.yy454, n);
  }else{
    /* When doing a nested parse, one can include terms in an expression
    ** that look like this:   #1 #2 ...  These terms refer to registers
    ** in the virtual machine.  #N is the N-th register. */
    Token t = yymsp[0].minor.yy0; /*A-overwrites-X*/
    assert( t.n>=2 );
    if( pParse->nested==0 ){
      sqlite3ErrorMsg(pParse, "near \"%T\": syntax error", &t);
      yymsp[0].minor.yy454 = 0;
    }else{
      yymsp[0].minor.yy454 = sqlite3PExpr(pParse, TK_REGISTER, 0, 0);
      if( yymsp[0].minor.yy454 ) sqlite3GetInt32(&t.z[1], &yymsp[0].minor.yy454->iTable);
    }
  }
}
        break;
      case 187: /* expr ::= expr COLLATE ID|STRING */
{
  yymsp[-2].minor.yy454 = sqlite3ExprAddCollateToken(pParse, yymsp[-2].minor.yy454, &yymsp[0].minor.yy0, 1);
}
        break;
      case 188: /* expr ::= CAST LP expr AS typetoken RP */
{
  yymsp[-5].minor.yy454 = sqlite3ExprAlloc(pParse->db, TK_CAST, &yymsp[-1].minor.yy0, 1);
  sqlite3ExprAttachSubtrees(pParse->db, yymsp[-5].minor.yy454, yymsp[-3].minor.yy454, 0);
}
        break;
      case 189: /* expr ::= ID|INDEXED|JOIN_KW LP distinct exprlist RP */
{
  yylhsminor.yy454 = sqlite3ExprFunction(pParse, yymsp[-1].minor.yy14, &yymsp[-4].minor.yy0, yymsp[-2].minor.yy144);
}
  yymsp[-4].minor.yy454 = yylhsminor.yy454;
        break;
      case 190: /* expr ::= ID|INDEXED|JOIN_KW LP distinct exprlist ORDER BY sortlist RP */
{
  yylhsminor.yy454 = sqlite3ExprFunction(pParse, yymsp[-4].minor.yy14, &yymsp[-7].minor.yy0, yymsp[-5].minor.yy144);
  sqlite3ExprAddFunctionOrderBy(pParse, yylhsminor.yy454, yymsp[-1].minor.yy14);
}
  yymsp[-7].minor.yy454 = yylhsminor.yy454;
        break;
      case 191: /* expr ::= ID|INDEXED|JOIN_KW LP STAR RP */
{
  yylhsminor.yy454 = sqlite3ExprFunction(pParse, 0, &yymsp[-3].minor.yy0, 0);
}
  yymsp[-3].minor.yy454 = yylhsminor.yy454;
        break;
      case 192: /* expr ::= ID|INDEXED|JOIN_KW LP distinct exprlist RP filter_over */
{
  yylhsminor.yy454 = sqlite3ExprFunction(pParse, yymsp[-2].minor.yy14, &yymsp[-5].minor.yy0, yymsp[-3].minor.yy144);
  sqlite3WindowAttach(pParse, yylhsminor.yy454, yymsp[0].minor.yy211);
}
  yymsp[-5].minor.yy454 = yylhsminor.yy454;
        break;
      case 193: /* expr ::= ID|INDEXED|JOIN_KW LP distinct exprlist ORDER BY sortlist RP filter_over */
{
  yylhsminor.yy454 = sqlite3ExprFunction(pParse, yymsp[-5].minor.yy14, &yymsp[-8].minor.yy0, yymsp[-6].minor.yy144);
  sqlite3WindowAttach(pParse, yylhsminor.yy454, yymsp[0].minor.yy211);
  sqlite3ExprAddFunctionOrderBy(pParse, yylhsminor.yy454, yymsp[-2].minor.yy14);
}
  yymsp[-8].minor.yy454 = yylhsminor.yy454;
        break;
      case 194: /* expr ::= ID|INDEXED|JOIN_KW LP STAR RP filter_over */
{
  yylhsminor.yy454 = sqlite3ExprFunction(pParse, 0, &yymsp[-4].minor.yy0, 0);
  sqlite3WindowAttach(pParse, yylhsminor.yy454, yymsp[0].minor.yy211);
}
  yymsp[-4].minor.yy454 = yylhsminor.yy454;
        break;
      case 195: /* term ::= CTIME_KW */
{
  yylhsminor.yy454 = sqlite3ExprFunction(pParse, 0, &yymsp[0].minor.yy0, 0);
}
  yymsp[0].minor.yy454 = yylhsminor.yy454;
        break;
      case 196: /* expr ::= LP nexprlist COMMA expr RP */
{
  ExprList *pList = sqlite3ExprListAppend(pParse, yymsp[-3].minor.yy14, yymsp[-1].minor.yy454);
  yymsp[-4].minor.yy454 = sqlite3PExpr(pParse, TK_VECTOR, 0, 0);
  if( yymsp[-4].minor.yy454 ){
    yymsp[-4].minor.yy454->x.pList = pList;
    if( ALWAYS(pList->nExpr) ){
      yymsp[-4].minor.yy454->flags |= pList->a[0].pExpr->flags & EP_Propagate;
    }
  }else{
    sqlite3ExprListDelete(pParse->db, pList);
  }
}
        break;
      case 197: /* expr ::= expr AND expr */
{yymsp[-2].minor.yy454=sqlite3ExprAnd(pParse,yymsp[-2].minor.yy454,yymsp[0].minor.yy454);}
        break;
      case 198: /* expr ::= expr OR expr */
      case 199: /* expr ::= expr LT|GT|GE|LE expr */ yytestcase(yyruleno==199);
      case 200: /* expr ::= expr EQ|NE expr */ yytestcase(yyruleno==200);
      case 201: /* expr ::= expr BITAND|BITOR|LSHIFT|RSHIFT expr */ yytestcase(yyruleno==201);
      case 202: /* expr ::= expr PLUS|MINUS expr */ yytestcase(yyruleno==202);
      case 203: /* expr ::= expr STAR|SLASH|REM expr */ yytestcase(yyruleno==203);
      case 204: /* expr ::= expr CONCAT expr */ yytestcase(yyruleno==204);
{yymsp[-2].minor.yy454=sqlite3PExpr(pParse,yymsp[-1].major,yymsp[-2].minor.yy454,yymsp[0].minor.yy454);}
        break;
      case 205: /* likeop ::= NOT LIKE_KW|MATCH */
{yymsp[-1].minor.yy0=yymsp[0].minor.yy0; yymsp[-1].minor.yy0.n|=0x80000000; /*yymsp[-1].minor.yy0-overwrite-yymsp[0].minor.yy0*/}
        break;
      case 206: /* expr ::= expr likeop expr */
{
  ExprList *pList;
  int bNot = yymsp[-1].minor.yy0.n & 0x80000000;
  yymsp[-1].minor.yy0.n &= 0x7fffffff;
  pList = sqlite3ExprListAppend(pParse,0, yymsp[0].minor.yy454);
  pList = sqlite3ExprListAppend(pParse,pList, yymsp[-2].minor.yy454);
  yymsp[-2].minor.yy454 = sqlite3ExprFunction(pParse, pList, &yymsp[-1].minor.yy0, 0);
  if( bNot ) yymsp[-2].minor.yy454 = sqlite3PExpr(pParse, TK_NOT, yymsp[-2].minor.yy454, 0);
  if( yymsp[-2].minor.yy454 ) yymsp[-2].minor.yy454->flags |= EP_InfixFunc;
}
        break;
      case 207: /* expr ::= expr likeop expr ESCAPE expr */
{
  ExprList *pList;
  int bNot = yymsp[-3].minor.yy0.n & 0x80000000;
  yymsp[-3].minor.yy0.n &= 0x7fffffff;
  pList = sqlite3ExprListAppend(pParse,0, yymsp[-2].minor.yy454);
  pList = sqlite3ExprListAppend(pParse,pList, yymsp[-4].minor.yy454);
  pList = sqlite3ExprListAppend(pParse,pList, yymsp[0].minor.yy454);
  yymsp[-4].minor.yy454 = sqlite3ExprFunction(pParse, pList, &yymsp[-3].minor.yy0, 0);
  if( bNot ) yymsp[-4].minor.yy454 = sqlite3PExpr(pParse, TK_NOT, yymsp[-4].minor.yy454, 0);
  if( yymsp[-4].minor.yy454 ) yymsp[-4].minor.yy454->flags |= EP_InfixFunc;
}
        break;
      case 208: /* expr ::= expr ISNULL|NOTNULL */
{yymsp[-1].minor.yy454 = sqlite3PExpr(pParse,yymsp[0].major,yymsp[-1].minor.yy454,0);}
        break;
      case 209: /* expr ::= expr NOT NULL */
{yymsp[-2].minor.yy454 = sqlite3PExpr(pParse,TK_NOTNULL,yymsp[-2].minor.yy454,0);}
        break;
      case 210: /* expr ::= expr IS expr */
{
  yymsp[-2].minor.yy454 = sqlite3PExpr(pParse,TK_IS,yymsp[-2].minor.yy454,yymsp[0].minor.yy454);
  binaryToUnaryIfNull(pParse, yymsp[0].minor.yy454, yymsp[-2].minor.yy454, TK_ISNULL);
}
        break;
      case 211: /* expr ::= expr IS NOT expr */
{
  yymsp[-3].minor.yy454 = sqlite3PExpr(pParse,TK_ISNOT,yymsp[-3].minor.yy454,yymsp[0].minor.yy454);
  binaryToUnaryIfNull(pParse, yymsp[0].minor.yy454, yymsp[-3].minor.yy454, TK_NOTNULL);
}
        break;
      case 212: /* expr ::= expr IS NOT DISTINCT FROM expr */
{
  yymsp[-5].minor.yy454 = sqlite3PExpr(pParse,TK_IS,yymsp[-5].minor.yy454,yymsp[0].minor.yy454);
  binaryToUnaryIfNull(pParse, yymsp[0].minor.yy454, yymsp[-5].minor.yy454, TK_ISNULL);
}
        break;
      case 213: /* expr ::= expr IS DISTINCT FROM expr */
{
  yymsp[-4].minor.yy454 = sqlite3PExpr(pParse,TK_ISNOT,yymsp[-4].minor.yy454,yymsp[0].minor.yy454);
  binaryToUnaryIfNull(pParse, yymsp[0].minor.yy454, yymsp[-4].minor.yy454, TK_NOTNULL);
}
        break;
      case 214: /* expr ::= NOT expr */
      case 215: /* expr ::= BITNOT expr */ yytestcase(yyruleno==215);
{yymsp[-1].minor.yy454 = sqlite3PExpr(pParse, yymsp[-1].major, yymsp[0].minor.yy454, 0);/*A-overwrites-B*/}
        break;
      case 216: /* expr ::= PLUS|MINUS expr */
{
  Expr *p = yymsp[0].minor.yy454;
  u8 op = yymsp[-1].major + (TK_UPLUS-TK_PLUS);
  assert( TK_UPLUS>TK_PLUS );
  assert( TK_UMINUS == TK_MINUS + (TK_UPLUS - TK_PLUS) );
  if( p && p->op==TK_UPLUS ){
    p->op = op;
    yymsp[-1].minor.yy454 = p;
  }else{
    yymsp[-1].minor.yy454 = sqlite3PExpr(pParse, op, p, 0);
    /*A-overwrites-B*/
  }
}
        break;
      case 217: /* expr ::= expr PTR expr */
{
  ExprList *pList = sqlite3ExprListAppend(pParse, 0, yymsp[-2].minor.yy454);
  pList = sqlite3ExprListAppend(pParse, pList, yymsp[0].minor.yy454);
  yylhsminor.yy454 = sqlite3ExprFunction(pParse, pList, &yymsp[-1].minor.yy0, 0);
}
  yymsp[-2].minor.yy454 = yylhsminor.yy454;
        break;
      case 218: /* between_op ::= BETWEEN */
      case 221: /* in_op ::= IN */ yytestcase(yyruleno==221);
{yymsp[0].minor.yy144 = 0;}
        break;
      case 220: /* expr ::= expr between_op expr AND expr */
{
  ExprList *pList = sqlite3ExprListAppend(pParse,0, yymsp[-2].minor.yy454);
  pList = sqlite3ExprListAppend(pParse,pList, yymsp[0].minor.yy454);
  yymsp[-4].minor.yy454 = sqlite3PExpr(pParse, TK_BETWEEN, yymsp[-4].minor.yy454, 0);
  if( yymsp[-4].minor.yy454 ){
    yymsp[-4].minor.yy454->x.pList = pList;
  }else{
    sqlite3ExprListDelete(pParse->db, pList);
  }
  if( yymsp[-3].minor.yy144 ) yymsp[-4].minor.yy454 = sqlite3PExpr(pParse, TK_NOT, yymsp[-4].minor.yy454, 0);
}
        break;
      case 223: /* expr ::= expr in_op LP exprlist RP */
{
    if( yymsp[-1].minor.yy14==0 ){
      /* Expressions of the form
      **
      **      expr1 IN ()
      **      expr1 NOT IN ()
      **
      ** simplify to constants 0 (false) and 1 (true), respectively,
      ** regardless of the value of expr1.
      */
      sqlite3ExprUnmapAndDelete(pParse, yymsp[-4].minor.yy454);
      yymsp[-4].minor.yy454 = sqlite3Expr(pParse->db, TK_STRING, yymsp[-3].minor.yy144 ? "true" : "false");
      if( yymsp[-4].minor.yy454 ) sqlite3ExprIdToTrueFalse(yymsp[-4].minor.yy454);
    }else{
      Expr *pRHS = yymsp[-1].minor.yy14->a[0].pExpr;
      if( yymsp[-1].minor.yy14->nExpr==1 && sqlite3ExprIsConstant(pParse,pRHS) && yymsp[-4].minor.yy454->op!=TK_VECTOR ){
        yymsp[-1].minor.yy14->a[0].pExpr = 0;
        sqlite3ExprListDelete(pParse->db, yymsp[-1].minor.yy14);
        pRHS = sqlite3PExpr(pParse, TK_UPLUS, pRHS, 0);
        yymsp[-4].minor.yy454 = sqlite3PExpr(pParse, TK_EQ, yymsp[-4].minor.yy454, pRHS);
      }else if( yymsp[-1].minor.yy14->nExpr==1 && pRHS->op==TK_SELECT ){
        yymsp[-4].minor.yy454 = sqlite3PExpr(pParse, TK_IN, yymsp[-4].minor.yy454, 0);
        sqlite3PExprAddSelect(pParse, yymsp[-4].minor.yy454, pRHS->x.pSelect);
        pRHS->x.pSelect = 0;
        sqlite3ExprListDelete(pParse->db, yymsp[-1].minor.yy14);
      }else{
        yymsp[-4].minor.yy454 = sqlite3PExpr(pParse, TK_IN, yymsp[-4].minor.yy454, 0);
        if( yymsp[-4].minor.yy454==0 ){
          sqlite3ExprListDelete(pParse->db, yymsp[-1].minor.yy14);
        }else if( yymsp[-4].minor.yy454->pLeft->op==TK_VECTOR ){
          int nExpr = yymsp[-4].minor.yy454->pLeft->x.pList->nExpr;
          Select *pSelectRHS = sqlite3ExprListToValues(pParse, nExpr, yymsp[-1].minor.yy14);
          if( pSelectRHS ){
            parserDoubleLinkSelect(pParse, pSelectRHS);
            sqlite3PExprAddSelect(pParse, yymsp[-4].minor.yy454, pSelectRHS);
          }
        }else{
          yymsp[-4].minor.yy454->x.pList = yymsp[-1].minor.yy14;
          sqlite3ExprSetHeightAndFlags(pParse, yymsp[-4].minor.yy454);
        }
      }
      if( yymsp[-3].minor.yy144 ) yymsp[-4].minor.yy454 = sqlite3PExpr(pParse, TK_NOT, yymsp[-4].minor.yy454, 0);
    }
  }
        break;
      case 224: /* expr ::= LP select RP */
{
    yymsp[-2].minor.yy454 = sqlite3PExpr(pParse, TK_SELECT, 0, 0);
    sqlite3PExprAddSelect(pParse, yymsp[-2].minor.yy454, yymsp[-1].minor.yy555);
  }
        break;
      case 225: /* expr ::= expr in_op LP select RP */
{
    yymsp[-4].minor.yy454 = sqlite3PExpr(pParse, TK_IN, yymsp[-4].minor.yy454, 0);
    sqlite3PExprAddSelect(pParse, yymsp[-4].minor.yy454, yymsp[-1].minor.yy555);
    if( yymsp[-3].minor.yy144 ) yymsp[-4].minor.yy454 = sqlite3PExpr(pParse, TK_NOT, yymsp[-4].minor.yy454, 0);
  }
        break;
      case 226: /* expr ::= expr in_op nm dbnm paren_exprlist */
{
    SrcList *pSrc = sqlite3SrcListAppend(pParse, 0,&yymsp[-2].minor.yy0,&yymsp[-1].minor.yy0);
    Select *pSelect = sqlite3SelectNew(pParse, 0,pSrc,0,0,0,0,0,0);
    if( yymsp[0].minor.yy14 )  sqlite3SrcListFuncArgs(pParse, pSelect ? pSrc : 0, yymsp[0].minor.yy14);
    yymsp[-4].minor.yy454 = sqlite3PExpr(pParse, TK_IN, yymsp[-4].minor.yy454, 0);
    sqlite3PExprAddSelect(pParse, yymsp[-4].minor.yy454, pSelect);
    if( yymsp[-3].minor.yy144 ) yymsp[-4].minor.yy454 = sqlite3PExpr(pParse, TK_NOT, yymsp[-4].minor.yy454, 0);
  }
        break;
      case 227: /* expr ::= EXISTS LP select RP */
{
    Expr *p;
    p = yymsp[-3].minor.yy454 = sqlite3PExpr(pParse, TK_EXISTS, 0, 0);
    sqlite3PExprAddSelect(pParse, p, yymsp[-1].minor.yy555);
  }
        break;
      case 228: /* expr ::= CASE case_operand case_exprlist case_else END */
{
  yymsp[-4].minor.yy454 = sqlite3PExpr(pParse, TK_CASE, yymsp[-3].minor.yy454, 0);
  if( yymsp[-4].minor.yy454 ){
    yymsp[-4].minor.yy454->x.pList = yymsp[-1].minor.yy454 ? sqlite3ExprListAppend(pParse,yymsp[-2].minor.yy14,yymsp[-1].minor.yy454) : yymsp[-2].minor.yy14;
    sqlite3ExprSetHeightAndFlags(pParse, yymsp[-4].minor.yy454);
  }else{
    sqlite3ExprListDelete(pParse->db, yymsp[-2].minor.yy14);
    sqlite3ExprDelete(pParse->db, yymsp[-1].minor.yy454);
  }
}
