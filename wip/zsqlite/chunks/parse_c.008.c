
static void yy_accept(yyParser*);  /* Forward Declaration */

/*
** Perform a reduce action and the shift that must immediately
** follow the reduce.
**
** The yyLookahead and yyLookaheadToken parameters provide reduce actions
** access to the lookahead token (if any).  The yyLookahead will be YYNOCODE
** if the lookahead token has already been consumed.  As this procedure is
** only called from one place, optimizing compilers will in-line it, which
** means that the extra parameters have no performance impact.
*/
static YYACTIONTYPE yy_reduce(
  yyParser *yypParser,         /* The parser */
  unsigned int yyruleno,       /* Number of the rule by which to reduce */
  int yyLookahead,             /* Lookahead token, or YYNOCODE if none */
  sqlite3ParserTOKENTYPE yyLookaheadToken  /* Value of the lookahead token */
  sqlite3ParserCTX_PDECL                   /* %extra_context */
){
  int yygoto;                     /* The next state */
  YYACTIONTYPE yyact;             /* The next action */
  yyStackEntry *yymsp;            /* The top of the parser's stack */
  int yysize;                     /* Amount to pop the stack */
  sqlite3ParserARG_FETCH
  (void)yyLookahead;
  (void)yyLookaheadToken;
  yymsp = yypParser->yytos;

  switch( yyruleno ){
  /* Beginning here are the reduction cases.  A typical example
  ** follows:
  **   case 0:
  **  #line <lineno> <grammarfile>
  **     { ... }           // User supplied code
  **  #line <lineno> <thisfile>
  **     break;
  */
/********** Begin reduce actions **********************************************/
        YYMINORTYPE yylhsminor;
      case 0: /* explain ::= EXPLAIN */
{ if( pParse->pReprepare==0 ) pParse->explain = 1; }
        break;
      case 1: /* explain ::= EXPLAIN QUERY PLAN */
{ if( pParse->pReprepare==0 ) pParse->explain = 2; }
        break;
      case 2: /* cmdx ::= cmd */
{ sqlite3FinishCoding(pParse); }
        break;
      case 3: /* cmd ::= BEGIN transtype trans_opt */
{sqlite3BeginTransaction(pParse, yymsp[-1].minor.yy144);}
        break;
      case 4: /* transtype ::= */
{yymsp[1].minor.yy144 = TK_DEFERRED;}
        break;
      case 5: /* transtype ::= DEFERRED */
      case 6: /* transtype ::= IMMEDIATE */ yytestcase(yyruleno==6);
      case 7: /* transtype ::= EXCLUSIVE */ yytestcase(yyruleno==7);
      case 324: /* range_or_rows ::= RANGE|ROWS|GROUPS */ yytestcase(yyruleno==324);
{yymsp[0].minor.yy144 = yymsp[0].major; /*A-overwrites-X*/}
        break;
      case 8: /* cmd ::= COMMIT|END trans_opt */
      case 9: /* cmd ::= ROLLBACK trans_opt */ yytestcase(yyruleno==9);
{sqlite3EndTransaction(pParse,yymsp[-1].major);}
        break;
      case 10: /* cmd ::= SAVEPOINT nm */
{
  sqlite3Savepoint(pParse, SAVEPOINT_BEGIN, &yymsp[0].minor.yy0);
}
        break;
      case 11: /* cmd ::= RELEASE savepoint_opt nm */
{
  sqlite3Savepoint(pParse, SAVEPOINT_RELEASE, &yymsp[0].minor.yy0);
}
        break;
      case 12: /* cmd ::= ROLLBACK trans_opt TO savepoint_opt nm */
{
  sqlite3Savepoint(pParse, SAVEPOINT_ROLLBACK, &yymsp[0].minor.yy0);
}
        break;
      case 13: /* create_table ::= createkw temp TABLE ifnotexists nm dbnm */
{
   sqlite3StartTable(pParse,&yymsp[-1].minor.yy0,&yymsp[0].minor.yy0,yymsp[-4].minor.yy144,0,0,yymsp[-2].minor.yy144);
}
        break;
      case 14: /* createkw ::= CREATE */
{disableLookaside(pParse);}
        break;
      case 15: /* ifnotexists ::= */
      case 18: /* temp ::= */ yytestcase(yyruleno==18);
      case 47: /* autoinc ::= */ yytestcase(yyruleno==47);
      case 62: /* init_deferred_pred_opt ::= */ yytestcase(yyruleno==62);
      case 72: /* defer_subclause_opt ::= */ yytestcase(yyruleno==72);
      case 81: /* ifexists ::= */ yytestcase(yyruleno==81);
      case 100: /* distinct ::= */ yytestcase(yyruleno==100);
      case 246: /* collate ::= */ yytestcase(yyruleno==246);
{yymsp[1].minor.yy144 = 0;}
        break;
      case 16: /* ifnotexists ::= IF NOT EXISTS */
{yymsp[-2].minor.yy144 = 1;}
        break;
      case 17: /* temp ::= TEMP */
{yymsp[0].minor.yy144 = pParse->db->init.busy==0;}
        break;
      case 19: /* create_table_args ::= LP columnlist conslist_opt RP table_option_set */
{
  sqlite3EndTable(pParse,&yymsp[-2].minor.yy0,&yymsp[-1].minor.yy0,yymsp[0].minor.yy391,0);
}
        break;
      case 20: /* create_table_args ::= AS select */
{
  sqlite3EndTable(pParse,0,0,0,yymsp[0].minor.yy555);
  sqlite3SelectDelete(pParse->db, yymsp[0].minor.yy555);
}
        break;
      case 21: /* table_option_set ::= */
{yymsp[1].minor.yy391 = 0;}
        break;
      case 22: /* table_option_set ::= table_option_set COMMA table_option */
{yylhsminor.yy391 = yymsp[-2].minor.yy391|yymsp[0].minor.yy391;}
  yymsp[-2].minor.yy391 = yylhsminor.yy391;
        break;
      case 23: /* table_option ::= WITHOUT nm */
{
  if( yymsp[0].minor.yy0.n==5 && sqlite3_strnicmp(yymsp[0].minor.yy0.z,"rowid",5)==0 ){
    yymsp[-1].minor.yy391 = TF_WithoutRowid | TF_NoVisibleRowid;
  }else{
    yymsp[-1].minor.yy391 = 0;
    sqlite3ErrorMsg(pParse, "unknown table option: %.*s", yymsp[0].minor.yy0.n, yymsp[0].minor.yy0.z);
  }
}
        break;
      case 24: /* table_option ::= nm */
{
  if( yymsp[0].minor.yy0.n==6 && sqlite3_strnicmp(yymsp[0].minor.yy0.z,"strict",6)==0 ){
    yylhsminor.yy391 = TF_Strict;
  }else{
    yylhsminor.yy391 = 0;
    sqlite3ErrorMsg(pParse, "unknown table option: %.*s", yymsp[0].minor.yy0.n, yymsp[0].minor.yy0.z);
  }
}
  yymsp[0].minor.yy391 = yylhsminor.yy391;
        break;
      case 25: /* columnname ::= nm typetoken */
{sqlite3AddColumn(pParse,yymsp[-1].minor.yy0,yymsp[0].minor.yy0);}
        break;
      case 26: /* typetoken ::= */
      case 65: /* conslist_opt ::= */ yytestcase(yyruleno==65);
      case 106: /* as ::= */ yytestcase(yyruleno==106);
{yymsp[1].minor.yy0.n = 0; yymsp[1].minor.yy0.z = 0;}
        break;
      case 27: /* typetoken ::= typename LP signed RP */
{
  yymsp[-3].minor.yy0.n = (int)(&yymsp[0].minor.yy0.z[yymsp[0].minor.yy0.n] - yymsp[-3].minor.yy0.z);
}
        break;
      case 28: /* typetoken ::= typename LP signed COMMA signed RP */
{
  yymsp[-5].minor.yy0.n = (int)(&yymsp[0].minor.yy0.z[yymsp[0].minor.yy0.n] - yymsp[-5].minor.yy0.z);
}
        break;
      case 29: /* typename ::= typename ID|STRING */
{yymsp[-1].minor.yy0.n=yymsp[0].minor.yy0.n+(int)(yymsp[0].minor.yy0.z-yymsp[-1].minor.yy0.z);}
        break;
      case 30: /* scanpt ::= */
{
  assert( yyLookahead!=YYNOCODE );
  yymsp[1].minor.yy168 = yyLookaheadToken.z;
}
        break;
      case 31: /* scantok ::= */
{
  assert( yyLookahead!=YYNOCODE );
  yymsp[1].minor.yy0 = yyLookaheadToken;
}
        break;
      case 32: /* ccons ::= CONSTRAINT nm */
      case 67: /* tcons ::= CONSTRAINT nm */ yytestcase(yyruleno==67);
{pParse->constraintName = yymsp[0].minor.yy0;}
        break;
      case 33: /* ccons ::= DEFAULT scantok term */
{sqlite3AddDefaultValue(pParse,yymsp[0].minor.yy454,yymsp[-1].minor.yy0.z,&yymsp[-1].minor.yy0.z[yymsp[-1].minor.yy0.n]);}
        break;
      case 34: /* ccons ::= DEFAULT LP expr RP */
{sqlite3AddDefaultValue(pParse,yymsp[-1].minor.yy454,yymsp[-2].minor.yy0.z+1,yymsp[0].minor.yy0.z);}
        break;
      case 35: /* ccons ::= DEFAULT PLUS scantok term */
{sqlite3AddDefaultValue(pParse,yymsp[0].minor.yy454,yymsp[-2].minor.yy0.z,&yymsp[-1].minor.yy0.z[yymsp[-1].minor.yy0.n]);}
        break;
      case 36: /* ccons ::= DEFAULT MINUS scantok term */
{
  Expr *p = sqlite3PExpr(pParse, TK_UMINUS, yymsp[0].minor.yy454, 0);
  sqlite3AddDefaultValue(pParse,p,yymsp[-2].minor.yy0.z,&yymsp[-1].minor.yy0.z[yymsp[-1].minor.yy0.n]);
}
        break;
      case 37: /* ccons ::= DEFAULT scantok ID|INDEXED */
{
  Expr *p = tokenExpr(pParse, TK_STRING, yymsp[0].minor.yy0);
  if( p ){
    sqlite3ExprIdToTrueFalse(p);
    testcase( p->op==TK_TRUEFALSE && sqlite3ExprTruthValue(p) );
  }
    sqlite3AddDefaultValue(pParse,p,yymsp[0].minor.yy0.z,yymsp[0].minor.yy0.z+yymsp[0].minor.yy0.n);
}
        break;
      case 38: /* ccons ::= NOT NULL onconf */
{sqlite3AddNotNull(pParse, yymsp[0].minor.yy144);}
        break;
      case 39: /* ccons ::= PRIMARY KEY sortorder onconf autoinc */
{sqlite3AddPrimaryKey(pParse,0,yymsp[-1].minor.yy144,yymsp[0].minor.yy144,yymsp[-2].minor.yy144);}
        break;
      case 40: /* ccons ::= UNIQUE onconf */
{sqlite3CreateIndex(pParse,0,0,0,0,yymsp[0].minor.yy144,0,0,0,0,
                                   SQLITE_IDXTYPE_UNIQUE);}
        break;
      case 41: /* ccons ::= CHECK LP expr RP */
{sqlite3AddCheckConstraint(pParse,yymsp[-1].minor.yy454,yymsp[-2].minor.yy0.z,yymsp[0].minor.yy0.z);}
        break;
      case 42: /* ccons ::= REFERENCES nm eidlist_opt refargs */
{sqlite3CreateForeignKey(pParse,0,&yymsp[-2].minor.yy0,yymsp[-1].minor.yy14,yymsp[0].minor.yy144);}
        break;
      case 43: /* ccons ::= defer_subclause */
{sqlite3DeferForeignKey(pParse,yymsp[0].minor.yy144);}
        break;
      case 44: /* ccons ::= COLLATE ID|STRING */
{sqlite3AddCollateType(pParse, &yymsp[0].minor.yy0);}
        break;
      case 45: /* generated ::= LP expr RP */
{sqlite3AddGenerated(pParse,yymsp[-1].minor.yy454,0);}
        break;
      case 46: /* generated ::= LP expr RP ID */
{sqlite3AddGenerated(pParse,yymsp[-2].minor.yy454,&yymsp[0].minor.yy0);}
        break;
      case 48: /* autoinc ::= AUTOINCR */
{yymsp[0].minor.yy144 = 1;}
        break;
      case 49: /* refargs ::= */
{ yymsp[1].minor.yy144 = OE_None*0x0101; /* EV: R-19803-45884 */}
        break;
      case 50: /* refargs ::= refargs refarg */
{ yymsp[-1].minor.yy144 = (yymsp[-1].minor.yy144 & ~yymsp[0].minor.yy383.mask) | yymsp[0].minor.yy383.value; }
        break;
      case 51: /* refarg ::= MATCH nm */
{ yymsp[-1].minor.yy383.value = 0;     yymsp[-1].minor.yy383.mask = 0x000000; }
        break;
      case 52: /* refarg ::= ON INSERT refact */
{ yymsp[-2].minor.yy383.value = 0;     yymsp[-2].minor.yy383.mask = 0x000000; }
        break;
      case 53: /* refarg ::= ON DELETE refact */
{ yymsp[-2].minor.yy383.value = yymsp[0].minor.yy144;     yymsp[-2].minor.yy383.mask = 0x0000ff; }
        break;
      case 54: /* refarg ::= ON UPDATE refact */
{ yymsp[-2].minor.yy383.value = yymsp[0].minor.yy144<<8;  yymsp[-2].minor.yy383.mask = 0x00ff00; }
        break;
      case 55: /* refact ::= SET NULL */
{ yymsp[-1].minor.yy144 = OE_SetNull;  /* EV: R-33326-45252 */}
        break;
      case 56: /* refact ::= SET DEFAULT */
{ yymsp[-1].minor.yy144 = OE_SetDflt;  /* EV: R-33326-45252 */}
        break;
      case 57: /* refact ::= CASCADE */
{ yymsp[0].minor.yy144 = OE_Cascade;  /* EV: R-33326-45252 */}
        break;
      case 58: /* refact ::= RESTRICT */
{ yymsp[0].minor.yy144 = OE_Restrict; /* EV: R-33326-45252 */}
        break;
      case 59: /* refact ::= NO ACTION */
{ yymsp[-1].minor.yy144 = OE_None;     /* EV: R-33326-45252 */}
        break;
      case 60: /* defer_subclause ::= NOT DEFERRABLE init_deferred_pred_opt */
{yymsp[-2].minor.yy144 = 0;}
        break;
      case 61: /* defer_subclause ::= DEFERRABLE init_deferred_pred_opt */
      case 76: /* orconf ::= OR resolvetype */ yytestcase(yyruleno==76);
      case 173: /* insert_cmd ::= INSERT orconf */ yytestcase(yyruleno==173);
{yymsp[-1].minor.yy144 = yymsp[0].minor.yy144;}
        break;
      case 63: /* init_deferred_pred_opt ::= INITIALLY DEFERRED */
      case 80: /* ifexists ::= IF EXISTS */ yytestcase(yyruleno==80);
      case 219: /* between_op ::= NOT BETWEEN */ yytestcase(yyruleno==219);
      case 222: /* in_op ::= NOT IN */ yytestcase(yyruleno==222);
      case 247: /* collate ::= COLLATE ID|STRING */ yytestcase(yyruleno==247);
{yymsp[-1].minor.yy144 = 1;}
        break;
      case 64: /* init_deferred_pred_opt ::= INITIALLY IMMEDIATE */
{yymsp[-1].minor.yy144 = 0;}
        break;
      case 66: /* tconscomma ::= COMMA */
{pParse->constraintName.n = 0;}
        break;
      case 68: /* tcons ::= PRIMARY KEY LP sortlist autoinc RP onconf */
{sqlite3AddPrimaryKey(pParse,yymsp[-3].minor.yy14,yymsp[0].minor.yy144,yymsp[-2].minor.yy144,0);}
        break;
      case 69: /* tcons ::= UNIQUE LP sortlist RP onconf */
{sqlite3CreateIndex(pParse,0,0,0,yymsp[-2].minor.yy14,yymsp[0].minor.yy144,0,0,0,0,
                                       SQLITE_IDXTYPE_UNIQUE);}
        break;
      case 70: /* tcons ::= CHECK LP expr RP onconf */
{sqlite3AddCheckConstraint(pParse,yymsp[-2].minor.yy454,yymsp[-3].minor.yy0.z,yymsp[-1].minor.yy0.z);}
        break;
      case 71: /* tcons ::= FOREIGN KEY LP eidlist RP REFERENCES nm eidlist_opt refargs defer_subclause_opt */
{
    sqlite3CreateForeignKey(pParse, yymsp[-6].minor.yy14, &yymsp[-3].minor.yy0, yymsp[-2].minor.yy14, yymsp[-1].minor.yy144);
    sqlite3DeferForeignKey(pParse, yymsp[0].minor.yy144);
}
        break;
      case 73: /* onconf ::= */
      case 75: /* orconf ::= */ yytestcase(yyruleno==75);
{yymsp[1].minor.yy144 = OE_Default;}
        break;
      case 74: /* onconf ::= ON CONFLICT resolvetype */
{yymsp[-2].minor.yy144 = yymsp[0].minor.yy144;}
        break;
      case 77: /* resolvetype ::= IGNORE */
{yymsp[0].minor.yy144 = OE_Ignore;}
        break;
      case 78: /* resolvetype ::= REPLACE */
      case 174: /* insert_cmd ::= REPLACE */ yytestcase(yyruleno==174);
{yymsp[0].minor.yy144 = OE_Replace;}
        break;
      case 79: /* cmd ::= DROP TABLE ifexists fullname */
{
  sqlite3DropTable(pParse, yymsp[0].minor.yy203, 0, yymsp[-1].minor.yy144);
}
        break;
      case 82: /* cmd ::= createkw temp VIEW ifnotexists nm dbnm eidlist_opt AS select */
{
  sqlite3CreateView(pParse, &yymsp[-8].minor.yy0, &yymsp[-4].minor.yy0, &yymsp[-3].minor.yy0, yymsp[-2].minor.yy14, yymsp[0].minor.yy555, yymsp[-7].minor.yy144, yymsp[-5].minor.yy144);
}
        break;
      case 83: /* cmd ::= DROP VIEW ifexists fullname */
{
  sqlite3DropTable(pParse, yymsp[0].minor.yy203, 1, yymsp[-1].minor.yy144);
}
        break;
      case 84: /* cmd ::= select */
{
  SelectDest dest = {SRT_Output, 0, 0, 0, 0, 0, 0};
  sqlite3Select(pParse, yymsp[0].minor.yy555, &dest);
  sqlite3SelectDelete(pParse->db, yymsp[0].minor.yy555);
}
        break;
      case 85: /* select ::= WITH wqlist selectnowith */
{yymsp[-2].minor.yy555 = attachWithToSelect(pParse,yymsp[0].minor.yy555,yymsp[-1].minor.yy59);}
        break;
      case 86: /* select ::= WITH RECURSIVE wqlist selectnowith */
{yymsp[-3].minor.yy555 = attachWithToSelect(pParse,yymsp[0].minor.yy555,yymsp[-1].minor.yy59);}
        break;
      case 87: /* select ::= selectnowith */
{
  Select *p = yymsp[0].minor.yy555;
  if( p ){
    parserDoubleLinkSelect(pParse, p);
  }
}
