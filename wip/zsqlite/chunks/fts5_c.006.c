
/*
** Print tracing information for a SHIFT action
*/
#ifndef NDEBUG
static void fts5yyTraceShift(fts5yyParser *fts5yypParser, int fts5yyNewState, const char *zTag){
  if( fts5yyTraceFILE ){
    if( fts5yyNewState<fts5YYNSTATE ){
      fprintf(fts5yyTraceFILE,"%s%s '%s', go to state %d\n",
         fts5yyTracePrompt, zTag, fts5yyTokenName[fts5yypParser->fts5yytos->major],
         fts5yyNewState);
    }else{
      fprintf(fts5yyTraceFILE,"%s%s '%s', pending reduce %d\n",
         fts5yyTracePrompt, zTag, fts5yyTokenName[fts5yypParser->fts5yytos->major],
         fts5yyNewState - fts5YY_MIN_REDUCE);
    }
  }
}
#else
# define fts5yyTraceShift(X,Y,Z)
#endif

/*
** Perform a shift action.
*/
static void fts5yy_shift(
  fts5yyParser *fts5yypParser,          /* The parser to be shifted */
  fts5YYACTIONTYPE fts5yyNewState,      /* The new state to shift in */
  fts5YYCODETYPE fts5yyMajor,           /* The major token to shift in */
  sqlite3Fts5ParserFTS5TOKENTYPE fts5yyMinor        /* The minor token to shift in */
){
  fts5yyStackEntry *fts5yytos;
  fts5yypParser->fts5yytos++;
#ifdef fts5YYTRACKMAXSTACKDEPTH
  if( (int)(fts5yypParser->fts5yytos - fts5yypParser->fts5yystack)>fts5yypParser->fts5yyhwm ){
    fts5yypParser->fts5yyhwm++;
    assert( fts5yypParser->fts5yyhwm == (int)(fts5yypParser->fts5yytos - fts5yypParser->fts5yystack) );
  }
#endif
  fts5yytos = fts5yypParser->fts5yytos;
  if( fts5yytos>fts5yypParser->fts5yystackEnd ){
    if( fts5yyGrowStack(fts5yypParser) ){
      fts5yypParser->fts5yytos--;
      fts5yyStackOverflow(fts5yypParser);
      return;
    }
    fts5yytos = fts5yypParser->fts5yytos;
    assert( fts5yytos <= fts5yypParser->fts5yystackEnd );
  }
  if( fts5yyNewState > fts5YY_MAX_SHIFT ){
    fts5yyNewState += fts5YY_MIN_REDUCE - fts5YY_MIN_SHIFTREDUCE;
  }
  fts5yytos->stateno = fts5yyNewState;
  fts5yytos->major = fts5yyMajor;
  fts5yytos->minor.fts5yy0 = fts5yyMinor;
  fts5yyTraceShift(fts5yypParser, fts5yyNewState, "Shift");
}

/* For rule J, fts5yyRuleInfoLhs[J] contains the symbol on the left-hand side
** of that rule */
static const fts5YYCODETYPE fts5yyRuleInfoLhs[] = {
    16,  /* (0) input ::= expr */
    20,  /* (1) colset ::= MINUS LCP colsetlist RCP */
    20,  /* (2) colset ::= LCP colsetlist RCP */
    20,  /* (3) colset ::= STRING */
    20,  /* (4) colset ::= MINUS STRING */
    21,  /* (5) colsetlist ::= colsetlist STRING */
    21,  /* (6) colsetlist ::= STRING */
    17,  /* (7) expr ::= expr AND expr */
    17,  /* (8) expr ::= expr OR expr */
    17,  /* (9) expr ::= expr NOT expr */
    17,  /* (10) expr ::= colset COLON LP expr RP */
    17,  /* (11) expr ::= LP expr RP */
    17,  /* (12) expr ::= exprlist */
    19,  /* (13) exprlist ::= cnearset */
    19,  /* (14) exprlist ::= exprlist cnearset */
    18,  /* (15) cnearset ::= nearset */
    18,  /* (16) cnearset ::= colset COLON nearset */
    22,  /* (17) nearset ::= phrase */
    22,  /* (18) nearset ::= CARET phrase */
    22,  /* (19) nearset ::= STRING LP nearphrases neardist_opt RP */
    23,  /* (20) nearphrases ::= phrase */
    23,  /* (21) nearphrases ::= nearphrases phrase */
    25,  /* (22) neardist_opt ::= */
    25,  /* (23) neardist_opt ::= COMMA STRING */
    24,  /* (24) phrase ::= phrase PLUS STRING star_opt */
    24,  /* (25) phrase ::= STRING star_opt */
    26,  /* (26) star_opt ::= STAR */
    26,  /* (27) star_opt ::= */
};

/* For rule J, fts5yyRuleInfoNRhs[J] contains the negative of the number
** of symbols on the right-hand side of that rule. */
static const signed char fts5yyRuleInfoNRhs[] = {
   -1,  /* (0) input ::= expr */
   -4,  /* (1) colset ::= MINUS LCP colsetlist RCP */
   -3,  /* (2) colset ::= LCP colsetlist RCP */
   -1,  /* (3) colset ::= STRING */
   -2,  /* (4) colset ::= MINUS STRING */
   -2,  /* (5) colsetlist ::= colsetlist STRING */
   -1,  /* (6) colsetlist ::= STRING */
   -3,  /* (7) expr ::= expr AND expr */
   -3,  /* (8) expr ::= expr OR expr */
   -3,  /* (9) expr ::= expr NOT expr */
   -5,  /* (10) expr ::= colset COLON LP expr RP */
   -3,  /* (11) expr ::= LP expr RP */
   -1,  /* (12) expr ::= exprlist */
   -1,  /* (13) exprlist ::= cnearset */
   -2,  /* (14) exprlist ::= exprlist cnearset */
   -1,  /* (15) cnearset ::= nearset */
   -3,  /* (16) cnearset ::= colset COLON nearset */
   -1,  /* (17) nearset ::= phrase */
   -2,  /* (18) nearset ::= CARET phrase */
   -5,  /* (19) nearset ::= STRING LP nearphrases neardist_opt RP */
   -1,  /* (20) nearphrases ::= phrase */
   -2,  /* (21) nearphrases ::= nearphrases phrase */
    0,  /* (22) neardist_opt ::= */
   -2,  /* (23) neardist_opt ::= COMMA STRING */
   -4,  /* (24) phrase ::= phrase PLUS STRING star_opt */
   -2,  /* (25) phrase ::= STRING star_opt */
   -1,  /* (26) star_opt ::= STAR */
    0,  /* (27) star_opt ::= */
};

static void fts5yy_accept(fts5yyParser*);  /* Forward Declaration */

/*
** Perform a reduce action and the shift that must immediately
** follow the reduce.
**
** The fts5yyLookahead and fts5yyLookaheadToken parameters provide reduce actions
** access to the lookahead token (if any).  The fts5yyLookahead will be fts5YYNOCODE
** if the lookahead token has already been consumed.  As this procedure is
** only called from one place, optimizing compilers will in-line it, which
** means that the extra parameters have no performance impact.
*/
static fts5YYACTIONTYPE fts5yy_reduce(
  fts5yyParser *fts5yypParser,         /* The parser */
  unsigned int fts5yyruleno,       /* Number of the rule by which to reduce */
  int fts5yyLookahead,             /* Lookahead token, or fts5YYNOCODE if none */
  sqlite3Fts5ParserFTS5TOKENTYPE fts5yyLookaheadToken  /* Value of the lookahead token */
  sqlite3Fts5ParserCTX_PDECL                   /* %extra_context */
){
  int fts5yygoto;                     /* The next state */
  fts5YYACTIONTYPE fts5yyact;             /* The next action */
  fts5yyStackEntry *fts5yymsp;            /* The top of the parser's stack */
  int fts5yysize;                     /* Amount to pop the stack */
  sqlite3Fts5ParserARG_FETCH
  (void)fts5yyLookahead;
  (void)fts5yyLookaheadToken;
  fts5yymsp = fts5yypParser->fts5yytos;

  switch( fts5yyruleno ){
  /* Beginning here are the reduction cases.  A typical example
  ** follows:
  **   case 0:
  **  #line <lineno> <grammarfile>
  **     { ... }           // User supplied code
  **  #line <lineno> <thisfile>
  **     break;
  */
/********** Begin reduce actions **********************************************/
        fts5YYMINORTYPE fts5yylhsminor;
      case 0: /* input ::= expr */
{ sqlite3Fts5ParseFinished(pParse, fts5yymsp[0].minor.fts5yy24); }
        break;
      case 1: /* colset ::= MINUS LCP colsetlist RCP */
{
    fts5yymsp[-3].minor.fts5yy11 = sqlite3Fts5ParseColsetInvert(pParse, fts5yymsp[-1].minor.fts5yy11);
}
        break;
      case 2: /* colset ::= LCP colsetlist RCP */
{ fts5yymsp[-2].minor.fts5yy11 = fts5yymsp[-1].minor.fts5yy11; }
        break;
      case 3: /* colset ::= STRING */
{
  fts5yylhsminor.fts5yy11 = sqlite3Fts5ParseColset(pParse, 0, &fts5yymsp[0].minor.fts5yy0);
}
  fts5yymsp[0].minor.fts5yy11 = fts5yylhsminor.fts5yy11;
        break;
      case 4: /* colset ::= MINUS STRING */
{
  fts5yymsp[-1].minor.fts5yy11 = sqlite3Fts5ParseColset(pParse, 0, &fts5yymsp[0].minor.fts5yy0);
  fts5yymsp[-1].minor.fts5yy11 = sqlite3Fts5ParseColsetInvert(pParse, fts5yymsp[-1].minor.fts5yy11);
}
        break;
      case 5: /* colsetlist ::= colsetlist STRING */
{
  fts5yylhsminor.fts5yy11 = sqlite3Fts5ParseColset(pParse, fts5yymsp[-1].minor.fts5yy11, &fts5yymsp[0].minor.fts5yy0); }
  fts5yymsp[-1].minor.fts5yy11 = fts5yylhsminor.fts5yy11;
        break;
      case 6: /* colsetlist ::= STRING */
{
  fts5yylhsminor.fts5yy11 = sqlite3Fts5ParseColset(pParse, 0, &fts5yymsp[0].minor.fts5yy0);
}
  fts5yymsp[0].minor.fts5yy11 = fts5yylhsminor.fts5yy11;
        break;
      case 7: /* expr ::= expr AND expr */
{
  fts5yylhsminor.fts5yy24 = sqlite3Fts5ParseNode(pParse, FTS5_AND, fts5yymsp[-2].minor.fts5yy24, fts5yymsp[0].minor.fts5yy24, 0);
}
  fts5yymsp[-2].minor.fts5yy24 = fts5yylhsminor.fts5yy24;
        break;
      case 8: /* expr ::= expr OR expr */
{
  fts5yylhsminor.fts5yy24 = sqlite3Fts5ParseNode(pParse, FTS5_OR, fts5yymsp[-2].minor.fts5yy24, fts5yymsp[0].minor.fts5yy24, 0);
}
  fts5yymsp[-2].minor.fts5yy24 = fts5yylhsminor.fts5yy24;
        break;
      case 9: /* expr ::= expr NOT expr */
{
  fts5yylhsminor.fts5yy24 = sqlite3Fts5ParseNode(pParse, FTS5_NOT, fts5yymsp[-2].minor.fts5yy24, fts5yymsp[0].minor.fts5yy24, 0);
}
  fts5yymsp[-2].minor.fts5yy24 = fts5yylhsminor.fts5yy24;
        break;
      case 10: /* expr ::= colset COLON LP expr RP */
{
  sqlite3Fts5ParseSetColset(pParse, fts5yymsp[-1].minor.fts5yy24, fts5yymsp[-4].minor.fts5yy11);
  fts5yylhsminor.fts5yy24 = fts5yymsp[-1].minor.fts5yy24;
}
  fts5yymsp[-4].minor.fts5yy24 = fts5yylhsminor.fts5yy24;
        break;
      case 11: /* expr ::= LP expr RP */
{fts5yymsp[-2].minor.fts5yy24 = fts5yymsp[-1].minor.fts5yy24;}
        break;
      case 12: /* expr ::= exprlist */
      case 13: /* exprlist ::= cnearset */ fts5yytestcase(fts5yyruleno==13);
{fts5yylhsminor.fts5yy24 = fts5yymsp[0].minor.fts5yy24;}
  fts5yymsp[0].minor.fts5yy24 = fts5yylhsminor.fts5yy24;
        break;
      case 14: /* exprlist ::= exprlist cnearset */
{
  fts5yylhsminor.fts5yy24 = sqlite3Fts5ParseImplicitAnd(pParse, fts5yymsp[-1].minor.fts5yy24, fts5yymsp[0].minor.fts5yy24);
}
  fts5yymsp[-1].minor.fts5yy24 = fts5yylhsminor.fts5yy24;
        break;
      case 15: /* cnearset ::= nearset */
{
  fts5yylhsminor.fts5yy24 = sqlite3Fts5ParseNode(pParse, FTS5_STRING, 0, 0, fts5yymsp[0].minor.fts5yy46);
}
  fts5yymsp[0].minor.fts5yy24 = fts5yylhsminor.fts5yy24;
        break;
      case 16: /* cnearset ::= colset COLON nearset */
{
  fts5yylhsminor.fts5yy24 = sqlite3Fts5ParseNode(pParse, FTS5_STRING, 0, 0, fts5yymsp[0].minor.fts5yy46);
  sqlite3Fts5ParseSetColset(pParse, fts5yylhsminor.fts5yy24, fts5yymsp[-2].minor.fts5yy11);
}
  fts5yymsp[-2].minor.fts5yy24 = fts5yylhsminor.fts5yy24;
        break;
      case 17: /* nearset ::= phrase */
{ fts5yylhsminor.fts5yy46 = sqlite3Fts5ParseNearset(pParse, 0, fts5yymsp[0].minor.fts5yy53); }
  fts5yymsp[0].minor.fts5yy46 = fts5yylhsminor.fts5yy46;
        break;
      case 18: /* nearset ::= CARET phrase */
{
  sqlite3Fts5ParseSetCaret(fts5yymsp[0].minor.fts5yy53);
  fts5yymsp[-1].minor.fts5yy46 = sqlite3Fts5ParseNearset(pParse, 0, fts5yymsp[0].minor.fts5yy53);
}
        break;
      case 19: /* nearset ::= STRING LP nearphrases neardist_opt RP */
{
  sqlite3Fts5ParseNear(pParse, &fts5yymsp[-4].minor.fts5yy0);
  sqlite3Fts5ParseSetDistance(pParse, fts5yymsp[-2].minor.fts5yy46, &fts5yymsp[-1].minor.fts5yy0);
  fts5yylhsminor.fts5yy46 = fts5yymsp[-2].minor.fts5yy46;
}
  fts5yymsp[-4].minor.fts5yy46 = fts5yylhsminor.fts5yy46;
        break;
      case 20: /* nearphrases ::= phrase */
{
  fts5yylhsminor.fts5yy46 = sqlite3Fts5ParseNearset(pParse, 0, fts5yymsp[0].minor.fts5yy53);
}
  fts5yymsp[0].minor.fts5yy46 = fts5yylhsminor.fts5yy46;
        break;
      case 21: /* nearphrases ::= nearphrases phrase */
{
  fts5yylhsminor.fts5yy46 = sqlite3Fts5ParseNearset(pParse, fts5yymsp[-1].minor.fts5yy46, fts5yymsp[0].minor.fts5yy53);
}
  fts5yymsp[-1].minor.fts5yy46 = fts5yylhsminor.fts5yy46;
        break;
      case 22: /* neardist_opt ::= */
{ fts5yymsp[1].minor.fts5yy0.p = 0; fts5yymsp[1].minor.fts5yy0.n = 0; }
        break;
      case 23: /* neardist_opt ::= COMMA STRING */
{ fts5yymsp[-1].minor.fts5yy0 = fts5yymsp[0].minor.fts5yy0; }
        break;
      case 24: /* phrase ::= phrase PLUS STRING star_opt */
{
  fts5yylhsminor.fts5yy53 = sqlite3Fts5ParseTerm(pParse, fts5yymsp[-3].minor.fts5yy53, &fts5yymsp[-1].minor.fts5yy0, fts5yymsp[0].minor.fts5yy4);
}
  fts5yymsp[-3].minor.fts5yy53 = fts5yylhsminor.fts5yy53;
        break;
      case 25: /* phrase ::= STRING star_opt */
{
  fts5yylhsminor.fts5yy53 = sqlite3Fts5ParseTerm(pParse, 0, &fts5yymsp[-1].minor.fts5yy0, fts5yymsp[0].minor.fts5yy4);
}
  fts5yymsp[-1].minor.fts5yy53 = fts5yylhsminor.fts5yy53;
        break;
      case 26: /* star_opt ::= STAR */
{ fts5yymsp[0].minor.fts5yy4 = 1; }
        break;
      case 27: /* star_opt ::= */
{ fts5yymsp[1].minor.fts5yy4 = 0; }
        break;
      default:
        break;
/********** End reduce actions ************************************************/
  };
  assert( fts5yyruleno<sizeof(fts5yyRuleInfoLhs)/sizeof(fts5yyRuleInfoLhs[0]) );
  fts5yygoto = fts5yyRuleInfoLhs[fts5yyruleno];
  fts5yysize = fts5yyRuleInfoNRhs[fts5yyruleno];
  fts5yyact = fts5yy_find_reduce_action(fts5yymsp[fts5yysize].stateno,(fts5YYCODETYPE)fts5yygoto);

  /* There are no SHIFTREDUCE actions on nonterminals because the table
  ** generator has simplified them to pure REDUCE actions. */
  assert( !(fts5yyact>fts5YY_MAX_SHIFT && fts5yyact<=fts5YY_MAX_SHIFTREDUCE) );

  /* It is not possible for a REDUCE to be followed by an error */
  assert( fts5yyact!=fts5YY_ERROR_ACTION );

  fts5yymsp += fts5yysize+1;
  fts5yypParser->fts5yytos = fts5yymsp;
  fts5yymsp->stateno = (fts5YYACTIONTYPE)fts5yyact;
  fts5yymsp->major = (fts5YYCODETYPE)fts5yygoto;
  fts5yyTraceShift(fts5yypParser, fts5yyact, "... then shift");
  return fts5yyact;
}

/*
** The following code executes when the parse fails
*/
#ifndef fts5YYNOERRORRECOVERY
static void fts5yy_parse_failed(
  fts5yyParser *fts5yypParser           /* The parser */
){
  sqlite3Fts5ParserARG_FETCH
  sqlite3Fts5ParserCTX_FETCH
#ifndef NDEBUG
  if( fts5yyTraceFILE ){
    fprintf(fts5yyTraceFILE,"%sFail!\n",fts5yyTracePrompt);
  }
#endif
  while( fts5yypParser->fts5yytos>fts5yypParser->fts5yystack ) fts5yy_pop_parser_stack(fts5yypParser);
  /* Here code is inserted which will be executed whenever the
  ** parser fails */
/************ Begin %parse_failure code ***************************************/
/************ End %parse_failure code *****************************************/
  sqlite3Fts5ParserARG_STORE /* Suppress warning about unused %extra_argument variable */
  sqlite3Fts5ParserCTX_STORE
}
#endif /* fts5YYNOERRORRECOVERY */

/*
** The following code executes when a syntax error first occurs.
*/
static void fts5yy_syntax_error(
  fts5yyParser *fts5yypParser,           /* The parser */
  int fts5yymajor,                   /* The major type of the error token */
  sqlite3Fts5ParserFTS5TOKENTYPE fts5yyminor         /* The minor type of the error token */
){
  sqlite3Fts5ParserARG_FETCH
  sqlite3Fts5ParserCTX_FETCH
#define FTS5TOKEN fts5yyminor
/************ Begin %syntax_error code ****************************************/

  UNUSED_PARAM(fts5yymajor); /* Silence a compiler warning */
  sqlite3Fts5ParseError(
    pParse, "fts5: syntax error near \"%.*s\"",FTS5TOKEN.n,FTS5TOKEN.p
  );
/************ End %syntax_error code ******************************************/
  sqlite3Fts5ParserARG_STORE /* Suppress warning about unused %extra_argument variable */
  sqlite3Fts5ParserCTX_STORE
}
