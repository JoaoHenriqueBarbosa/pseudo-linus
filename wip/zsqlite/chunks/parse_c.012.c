  yymsp[-5].minor.yy211 = yylhsminor.yy211;
        break;
      case 325: /* frame_bound_s ::= frame_bound */
      case 327: /* frame_bound_e ::= frame_bound */ yytestcase(yyruleno==327);
{yylhsminor.yy509 = yymsp[0].minor.yy509;}
  yymsp[0].minor.yy509 = yylhsminor.yy509;
        break;
      case 326: /* frame_bound_s ::= UNBOUNDED PRECEDING */
      case 328: /* frame_bound_e ::= UNBOUNDED FOLLOWING */ yytestcase(yyruleno==328);
      case 330: /* frame_bound ::= CURRENT ROW */ yytestcase(yyruleno==330);
{yylhsminor.yy509.eType = yymsp[-1].major; yylhsminor.yy509.pExpr = 0;}
  yymsp[-1].minor.yy509 = yylhsminor.yy509;
        break;
      case 329: /* frame_bound ::= expr PRECEDING|FOLLOWING */
{yylhsminor.yy509.eType = yymsp[0].major; yylhsminor.yy509.pExpr = yymsp[-1].minor.yy454;}
  yymsp[-1].minor.yy509 = yylhsminor.yy509;
        break;
      case 331: /* frame_exclude_opt ::= */
{yymsp[1].minor.yy462 = 0;}
        break;
      case 332: /* frame_exclude_opt ::= EXCLUDE frame_exclude */
{yymsp[-1].minor.yy462 = yymsp[0].minor.yy462;}
        break;
      case 333: /* frame_exclude ::= NO OTHERS */
      case 334: /* frame_exclude ::= CURRENT ROW */ yytestcase(yyruleno==334);
{yymsp[-1].minor.yy462 = yymsp[-1].major; /*A-overwrites-X*/}
        break;
      case 335: /* frame_exclude ::= GROUP|TIES */
{yymsp[0].minor.yy462 = yymsp[0].major; /*A-overwrites-X*/}
        break;
      case 336: /* window_clause ::= WINDOW windowdefn_list */
{ yymsp[-1].minor.yy211 = yymsp[0].minor.yy211; }
        break;
      case 337: /* filter_over ::= filter_clause over_clause */
{
  if( yymsp[0].minor.yy211 ){
    yymsp[0].minor.yy211->pFilter = yymsp[-1].minor.yy454;
  }else{
    sqlite3ExprDelete(pParse->db, yymsp[-1].minor.yy454);
  }
  yylhsminor.yy211 = yymsp[0].minor.yy211;
}
  yymsp[-1].minor.yy211 = yylhsminor.yy211;
        break;
      case 338: /* filter_over ::= over_clause */
{
  yylhsminor.yy211 = yymsp[0].minor.yy211;
}
  yymsp[0].minor.yy211 = yylhsminor.yy211;
        break;
      case 339: /* filter_over ::= filter_clause */
{
  yylhsminor.yy211 = (Window*)sqlite3DbMallocZero(pParse->db, sizeof(Window));
  if( yylhsminor.yy211 ){
    yylhsminor.yy211->eFrmType = TK_FILTER;
    yylhsminor.yy211->pFilter = yymsp[0].minor.yy454;
  }else{
    sqlite3ExprDelete(pParse->db, yymsp[0].minor.yy454);
  }
}
  yymsp[0].minor.yy211 = yylhsminor.yy211;
        break;
      case 340: /* over_clause ::= OVER LP window RP */
{
  yymsp[-3].minor.yy211 = yymsp[-1].minor.yy211;
  assert( yymsp[-3].minor.yy211!=0 );
}
        break;
      case 341: /* over_clause ::= OVER nm */
{
  yymsp[-1].minor.yy211 = (Window*)sqlite3DbMallocZero(pParse->db, sizeof(Window));
  if( yymsp[-1].minor.yy211 ){
    yymsp[-1].minor.yy211->zName = sqlite3DbStrNDup(pParse->db, yymsp[0].minor.yy0.z, yymsp[0].minor.yy0.n);
  }
}
        break;
      case 342: /* filter_clause ::= FILTER LP WHERE expr RP */
{ yymsp[-4].minor.yy454 = yymsp[-1].minor.yy454; }
        break;
      case 343: /* term ::= QNUMBER */
{
  yylhsminor.yy454=tokenExpr(pParse,yymsp[0].major,yymsp[0].minor.yy0);
  sqlite3DequoteNumber(pParse, yylhsminor.yy454);
}
  yymsp[0].minor.yy454 = yylhsminor.yy454;
        break;
      default:
      /* (344) input ::= cmdlist */ yytestcase(yyruleno==344);
      /* (345) cmdlist ::= cmdlist ecmd */ yytestcase(yyruleno==345);
      /* (346) cmdlist ::= ecmd (OPTIMIZED OUT) */ assert(yyruleno!=346);
      /* (347) ecmd ::= SEMI */ yytestcase(yyruleno==347);
      /* (348) ecmd ::= cmdx SEMI */ yytestcase(yyruleno==348);
      /* (349) ecmd ::= explain cmdx SEMI (NEVER REDUCES) */ assert(yyruleno!=349);
      /* (350) trans_opt ::= */ yytestcase(yyruleno==350);
      /* (351) trans_opt ::= TRANSACTION */ yytestcase(yyruleno==351);
      /* (352) trans_opt ::= TRANSACTION nm */ yytestcase(yyruleno==352);
      /* (353) savepoint_opt ::= SAVEPOINT */ yytestcase(yyruleno==353);
      /* (354) savepoint_opt ::= */ yytestcase(yyruleno==354);
      /* (355) cmd ::= create_table create_table_args */ yytestcase(yyruleno==355);
      /* (356) table_option_set ::= table_option (OPTIMIZED OUT) */ assert(yyruleno!=356);
      /* (357) columnlist ::= columnlist COMMA columnname carglist */ yytestcase(yyruleno==357);
      /* (358) columnlist ::= columnname carglist */ yytestcase(yyruleno==358);
      /* (359) nm ::= ID|INDEXED|JOIN_KW */ yytestcase(yyruleno==359);
      /* (360) nm ::= STRING */ yytestcase(yyruleno==360);
      /* (361) typetoken ::= typename */ yytestcase(yyruleno==361);
      /* (362) typename ::= ID|STRING */ yytestcase(yyruleno==362);
      /* (363) signed ::= plus_num (OPTIMIZED OUT) */ assert(yyruleno!=363);
      /* (364) signed ::= minus_num (OPTIMIZED OUT) */ assert(yyruleno!=364);
      /* (365) carglist ::= carglist ccons */ yytestcase(yyruleno==365);
      /* (366) carglist ::= */ yytestcase(yyruleno==366);
      /* (367) ccons ::= NULL onconf */ yytestcase(yyruleno==367);
      /* (368) ccons ::= GENERATED ALWAYS AS generated */ yytestcase(yyruleno==368);
      /* (369) ccons ::= AS generated */ yytestcase(yyruleno==369);
      /* (370) conslist_opt ::= COMMA conslist */ yytestcase(yyruleno==370);
      /* (371) conslist ::= conslist tconscomma tcons */ yytestcase(yyruleno==371);
      /* (372) conslist ::= tcons (OPTIMIZED OUT) */ assert(yyruleno!=372);
      /* (373) tconscomma ::= */ yytestcase(yyruleno==373);
      /* (374) defer_subclause_opt ::= defer_subclause (OPTIMIZED OUT) */ assert(yyruleno!=374);
      /* (375) resolvetype ::= raisetype (OPTIMIZED OUT) */ assert(yyruleno!=375);
      /* (376) selectnowith ::= oneselect (OPTIMIZED OUT) */ assert(yyruleno!=376);
      /* (377) oneselect ::= values */ yytestcase(yyruleno==377);
      /* (378) sclp ::= selcollist COMMA */ yytestcase(yyruleno==378);
      /* (379) as ::= ID|STRING */ yytestcase(yyruleno==379);
      /* (380) indexed_opt ::= indexed_by (OPTIMIZED OUT) */ assert(yyruleno!=380);
      /* (381) returning ::= */ yytestcase(yyruleno==381);
      /* (382) expr ::= term (OPTIMIZED OUT) */ assert(yyruleno!=382);
      /* (383) likeop ::= LIKE_KW|MATCH */ yytestcase(yyruleno==383);
      /* (384) case_operand ::= expr */ yytestcase(yyruleno==384);
      /* (385) exprlist ::= nexprlist */ yytestcase(yyruleno==385);
      /* (386) nmnum ::= plus_num (OPTIMIZED OUT) */ assert(yyruleno!=386);
      /* (387) nmnum ::= nm (OPTIMIZED OUT) */ assert(yyruleno!=387);
      /* (388) nmnum ::= ON */ yytestcase(yyruleno==388);
      /* (389) nmnum ::= DELETE */ yytestcase(yyruleno==389);
      /* (390) nmnum ::= DEFAULT */ yytestcase(yyruleno==390);
      /* (391) plus_num ::= INTEGER|FLOAT */ yytestcase(yyruleno==391);
      /* (392) foreach_clause ::= */ yytestcase(yyruleno==392);
      /* (393) foreach_clause ::= FOR EACH ROW */ yytestcase(yyruleno==393);
      /* (394) trnm ::= nm */ yytestcase(yyruleno==394);
      /* (395) tridxby ::= */ yytestcase(yyruleno==395);
      /* (396) database_kw_opt ::= DATABASE */ yytestcase(yyruleno==396);
      /* (397) database_kw_opt ::= */ yytestcase(yyruleno==397);
      /* (398) kwcolumn_opt ::= */ yytestcase(yyruleno==398);
      /* (399) kwcolumn_opt ::= COLUMNKW */ yytestcase(yyruleno==399);
      /* (400) vtabarglist ::= vtabarg */ yytestcase(yyruleno==400);
      /* (401) vtabarglist ::= vtabarglist COMMA vtabarg */ yytestcase(yyruleno==401);
      /* (402) vtabarg ::= vtabarg vtabargtoken */ yytestcase(yyruleno==402);
      /* (403) anylist ::= */ yytestcase(yyruleno==403);
      /* (404) anylist ::= anylist LP anylist RP */ yytestcase(yyruleno==404);
      /* (405) anylist ::= anylist ANY */ yytestcase(yyruleno==405);
      /* (406) with ::= */ yytestcase(yyruleno==406);
      /* (407) windowdefn_list ::= windowdefn (OPTIMIZED OUT) */ assert(yyruleno!=407);
      /* (408) window ::= frame_opt (OPTIMIZED OUT) */ assert(yyruleno!=408);
        break;
/********** End reduce actions ************************************************/
  };
  assert( yyruleno<sizeof(yyRuleInfoLhs)/sizeof(yyRuleInfoLhs[0]) );
  yygoto = yyRuleInfoLhs[yyruleno];
  yysize = yyRuleInfoNRhs[yyruleno];
  yyact = yy_find_reduce_action(yymsp[yysize].stateno,(YYCODETYPE)yygoto);

  /* There are no SHIFTREDUCE actions on nonterminals because the table
  ** generator has simplified them to pure REDUCE actions. */
  assert( !(yyact>YY_MAX_SHIFT && yyact<=YY_MAX_SHIFTREDUCE) );

  /* It is not possible for a REDUCE to be followed by an error */
  assert( yyact!=YY_ERROR_ACTION );

  yymsp += yysize+1;
  yypParser->yytos = yymsp;
  yymsp->stateno = (YYACTIONTYPE)yyact;
  yymsp->major = (YYCODETYPE)yygoto;
  yyTraceShift(yypParser, yyact, "... then shift");
  return yyact;
}

/*
** The following code executes when the parse fails
*/
#ifndef YYNOERRORRECOVERY
static void yy_parse_failed(
  yyParser *yypParser           /* The parser */
){
  sqlite3ParserARG_FETCH
  sqlite3ParserCTX_FETCH
#ifndef NDEBUG
  if( yyTraceFILE ){
    fprintf(yyTraceFILE,"%sFail!\n",yyTracePrompt);
  }
#endif
  while( yypParser->yytos>yypParser->yystack ) yy_pop_parser_stack(yypParser);
  /* Here code is inserted which will be executed whenever the
  ** parser fails */
/************ Begin %parse_failure code ***************************************/
/************ End %parse_failure code *****************************************/
  sqlite3ParserARG_STORE /* Suppress warning about unused %extra_argument variable */
  sqlite3ParserCTX_STORE
}
#endif /* YYNOERRORRECOVERY */

/*
** The following code executes when a syntax error first occurs.
*/
static void yy_syntax_error(
  yyParser *yypParser,           /* The parser */
  int yymajor,                   /* The major type of the error token */
  sqlite3ParserTOKENTYPE yyminor         /* The minor type of the error token */
){
  sqlite3ParserARG_FETCH
  sqlite3ParserCTX_FETCH
#define TOKEN yyminor
/************ Begin %syntax_error code ****************************************/

  UNUSED_PARAMETER(yymajor);  /* Silence some compiler warnings */
  if( TOKEN.z[0] ){
    sqlite3ErrorMsg(pParse, "near \"%T\": syntax error", &TOKEN);
  }else{
    sqlite3ErrorMsg(pParse, "incomplete input");
  }
/************ End %syntax_error code ******************************************/
  sqlite3ParserARG_STORE /* Suppress warning about unused %extra_argument variable */
  sqlite3ParserCTX_STORE
}

/*
** The following is executed when the parser accepts
*/
static void yy_accept(
  yyParser *yypParser           /* The parser */
){
  sqlite3ParserARG_FETCH
  sqlite3ParserCTX_FETCH
#ifndef NDEBUG
  if( yyTraceFILE ){
    fprintf(yyTraceFILE,"%sAccept!\n",yyTracePrompt);
  }
#endif
#ifndef YYNOERRORRECOVERY
  yypParser->yyerrcnt = -1;
#endif
  assert( yypParser->yytos==yypParser->yystack );
  /* Here code is inserted which will be executed whenever the
  ** parser accepts */
/*********** Begin %parse_accept code *****************************************/
/*********** End %parse_accept code *******************************************/
  sqlite3ParserARG_STORE /* Suppress warning about unused %extra_argument variable */
  sqlite3ParserCTX_STORE
}

/* The main parser program.
** The first argument is a pointer to a structure obtained from
** "sqlite3ParserAlloc" which describes the current state of the parser.
** The second argument is the major token number.  The third is
** the minor token.  The fourth optional argument is whatever the
** user wants (and specified in the grammar) and is available for
** use by the action routines.
**
** Inputs:
** <ul>
** <li> A pointer to the parser (an opaque structure.)
** <li> The major token number.
** <li> The minor token number.
** <li> An option argument of a grammar-specified type.
** </ul>
**
** Outputs:
** None.
*/
SQLITE_PRIVATE void sqlite3Parser(
  void *yyp,                   /* The parser */
  int yymajor,                 /* The major token code number */
  sqlite3ParserTOKENTYPE yyminor       /* The value for the token */
  sqlite3ParserARG_PDECL               /* Optional %extra_argument parameter */
){
  YYMINORTYPE yyminorunion;
  YYACTIONTYPE yyact;   /* The parser action. */
#if !defined(YYERRORSYMBOL) && !defined(YYNOERRORRECOVERY)
  int yyendofinput;     /* True if we are at the end of input */
#endif
#ifdef YYERRORSYMBOL
  int yyerrorhit = 0;   /* True if yymajor has invoked an error */
#endif
  yyParser *yypParser = (yyParser*)yyp;  /* The parser */
  sqlite3ParserCTX_FETCH
  sqlite3ParserARG_STORE

  assert( yypParser->yytos!=0 );
#if !defined(YYERRORSYMBOL) && !defined(YYNOERRORRECOVERY)
  yyendofinput = (yymajor==0);
#endif

  yyact = yypParser->yytos->stateno;
#ifndef NDEBUG
  if( yyTraceFILE ){
    if( yyact < YY_MIN_REDUCE ){
      fprintf(yyTraceFILE,"%sInput '%s' in state %d\n",
              yyTracePrompt,yyTokenName[yymajor],yyact);
    }else{
      fprintf(yyTraceFILE,"%sInput '%s' with pending reduce %d\n",
              yyTracePrompt,yyTokenName[yymajor],yyact-YY_MIN_REDUCE);
    }
  }
#endif

  while(1){ /* Exit by "break" */
    assert( yypParser->yytos>=yypParser->yystack );
    assert( yyact==yypParser->yytos->stateno );
    yyact = yy_find_shift_action((YYCODETYPE)yymajor,yyact);
    if( yyact >= YY_MIN_REDUCE ){
      unsigned int yyruleno = yyact - YY_MIN_REDUCE; /* Reduce by this rule */
#ifndef NDEBUG
      assert( yyruleno<(int)(sizeof(yyRuleName)/sizeof(yyRuleName[0])) );
      if( yyTraceFILE ){
        int yysize = yyRuleInfoNRhs[yyruleno];
        if( yysize ){
          fprintf(yyTraceFILE, "%sReduce %d [%s]%s, pop back to state %d.\n",
            yyTracePrompt,
            yyruleno, yyRuleName[yyruleno],
            yyruleno<YYNRULE_WITH_ACTION ? "" : " without external action",
            yypParser->yytos[yysize].stateno);
        }else{
          fprintf(yyTraceFILE, "%sReduce %d [%s]%s.\n",
            yyTracePrompt, yyruleno, yyRuleName[yyruleno],
            yyruleno<YYNRULE_WITH_ACTION ? "" : " without external action");
        }
      }
#endif /* NDEBUG */

      /* Check that the stack is large enough to grow by a single entry
      ** if the RHS of the rule is empty.  This ensures that there is room
      ** enough on the stack to push the LHS value */
      if( yyRuleInfoNRhs[yyruleno]==0 ){
#ifdef YYTRACKMAXSTACKDEPTH
        if( (int)(yypParser->yytos - yypParser->yystack)>yypParser->yyhwm ){
          yypParser->yyhwm++;
          assert( yypParser->yyhwm ==
                  (int)(yypParser->yytos - yypParser->yystack));
        }
#endif
        if( yypParser->yytos>=yypParser->yystackEnd ){
          if( yyGrowStack(yypParser) ){
            yyStackOverflow(yypParser);
            break;
          }
        }
      }
      yyact = yy_reduce(yypParser,yyruleno,yymajor,yyminor sqlite3ParserCTX_PARAM);
    }else if( yyact <= YY_MAX_SHIFTREDUCE ){
      yy_shift(yypParser,yyact,(YYCODETYPE)yymajor,yyminor);
#ifndef YYNOERRORRECOVERY
      yypParser->yyerrcnt--;
#endif
      break;
    }else if( yyact==YY_ACCEPT_ACTION ){
      yypParser->yytos--;
      yy_accept(yypParser);
      return;
    }else{
      assert( yyact == YY_ERROR_ACTION );
      yyminorunion.yy0 = yyminor;
#ifdef YYERRORSYMBOL
      int yymx;
#endif
#ifndef NDEBUG
      if( yyTraceFILE ){
        fprintf(yyTraceFILE,"%sSyntax Error!\n",yyTracePrompt);
      }
#endif
#ifdef YYERRORSYMBOL
      /* A syntax error has occurred.
      ** The response to an error depends upon whether or not the
      ** grammar defines an error token "ERROR".
      **
      ** This is what we do if the grammar does define ERROR:
      **
      **  * Call the %syntax_error function.
      **
      **  * Begin popping the stack until we enter a state where
      **    it is legal to shift the error symbol, then shift
      **    the error symbol.
      **
      **  * Set the error count to three.
      **
      **  * Begin accepting and shifting new tokens.  No new error
      **    processing will occur until three tokens have been
      **    shifted successfully.
      **
      */
      if( yypParser->yyerrcnt<0 ){
        yy_syntax_error(yypParser,yymajor,yyminor);
      }
      yymx = yypParser->yytos->major;
      if( yymx==YYERRORSYMBOL || yyerrorhit ){
#ifndef NDEBUG
        if( yyTraceFILE ){
          fprintf(yyTraceFILE,"%sDiscard input token %s\n",
             yyTracePrompt,yyTokenName[yymajor]);
        }
#endif
        yy_destructor(yypParser, (YYCODETYPE)yymajor, &yyminorunion);
        yymajor = YYNOCODE;
      }else{
        while( yypParser->yytos > yypParser->yystack ){
          yyact = yy_find_reduce_action(yypParser->yytos->stateno,
                                        YYERRORSYMBOL);
          if( yyact<=YY_MAX_SHIFTREDUCE ) break;
          yy_pop_parser_stack(yypParser);
        }
        if( yypParser->yytos <= yypParser->yystack || yymajor==0 ){
          yy_destructor(yypParser,(YYCODETYPE)yymajor,&yyminorunion);
          yy_parse_failed(yypParser);
#ifndef YYNOERRORRECOVERY
          yypParser->yyerrcnt = -1;
#endif
          yymajor = YYNOCODE;
        }else if( yymx!=YYERRORSYMBOL ){
          yy_shift(yypParser,yyact,YYERRORSYMBOL,yyminor);
        }
      }
      yypParser->yyerrcnt = 3;
      yyerrorhit = 1;
      if( yymajor==YYNOCODE ) break;
      yyact = yypParser->yytos->stateno;
#elif defined(YYNOERRORRECOVERY)
      /* If the YYNOERRORRECOVERY macro is defined, then do not attempt to
      ** do any kind of error recovery.  Instead, simply invoke the syntax
      ** error routine and continue going as if nothing had happened.
      **
      ** Applications can set this macro (for example inside %include) if
      ** they intend to abandon the parse upon the first syntax error seen.
      */
      yy_syntax_error(yypParser,yymajor, yyminor);
      yy_destructor(yypParser,(YYCODETYPE)yymajor,&yyminorunion);
      break;
#else  /* YYERRORSYMBOL is not defined */
      /* This is what we do if the grammar does not define ERROR:
      **
      **  * Report an error message, and throw away the input token.
      **
      **  * If the input token is $, then fail the parse.
      **
      ** As before, subsequent error messages are suppressed until
      ** three input tokens have been successfully shifted.
      */
      if( yypParser->yyerrcnt<=0 ){
        yy_syntax_error(yypParser,yymajor, yyminor);
      }
      yypParser->yyerrcnt = 3;
      yy_destructor(yypParser,(YYCODETYPE)yymajor,&yyminorunion);
      if( yyendofinput ){
        yy_parse_failed(yypParser);
#ifndef YYNOERRORRECOVERY
        yypParser->yyerrcnt = -1;
#endif
      }
      break;
#endif
    }
  }
#ifndef NDEBUG
  if( yyTraceFILE ){
    yyStackEntry *i;
    char cDiv = '[';
    fprintf(yyTraceFILE,"%sReturn. Stack=",yyTracePrompt);
    for(i=&yypParser->yystack[1]; i<=yypParser->yytos; i++){
      fprintf(yyTraceFILE,"%c%s", cDiv, yyTokenName[i->major]);
      cDiv = ' ';
    }
    fprintf(yyTraceFILE,"]\n");
  }
#endif
  return;
}
