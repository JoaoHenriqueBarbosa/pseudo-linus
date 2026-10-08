
/*
** The following is executed when the parser accepts
*/
static void fts5yy_accept(
  fts5yyParser *fts5yypParser           /* The parser */
){
  sqlite3Fts5ParserARG_FETCH
  sqlite3Fts5ParserCTX_FETCH
#ifndef NDEBUG
  if( fts5yyTraceFILE ){
    fprintf(fts5yyTraceFILE,"%sAccept!\n",fts5yyTracePrompt);
  }
#endif
#ifndef fts5YYNOERRORRECOVERY
  fts5yypParser->fts5yyerrcnt = -1;
#endif
  assert( fts5yypParser->fts5yytos==fts5yypParser->fts5yystack );
  /* Here code is inserted which will be executed whenever the
  ** parser accepts */
/*********** Begin %parse_accept code *****************************************/
/*********** End %parse_accept code *******************************************/
  sqlite3Fts5ParserARG_STORE /* Suppress warning about unused %extra_argument variable */
  sqlite3Fts5ParserCTX_STORE
}

/* The main parser program.
** The first argument is a pointer to a structure obtained from
** "sqlite3Fts5ParserAlloc" which describes the current state of the parser.
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
static void sqlite3Fts5Parser(
  void *fts5yyp,                   /* The parser */
  int fts5yymajor,                 /* The major token code number */
  sqlite3Fts5ParserFTS5TOKENTYPE fts5yyminor       /* The value for the token */
  sqlite3Fts5ParserARG_PDECL               /* Optional %extra_argument parameter */
){
  fts5YYMINORTYPE fts5yyminorunion;
  fts5YYACTIONTYPE fts5yyact;   /* The parser action. */
#if !defined(fts5YYERRORSYMBOL) && !defined(fts5YYNOERRORRECOVERY)
  int fts5yyendofinput;     /* True if we are at the end of input */
#endif
#ifdef fts5YYERRORSYMBOL
  int fts5yyerrorhit = 0;   /* True if fts5yymajor has invoked an error */
#endif
  fts5yyParser *fts5yypParser = (fts5yyParser*)fts5yyp;  /* The parser */
  sqlite3Fts5ParserCTX_FETCH
  sqlite3Fts5ParserARG_STORE

  assert( fts5yypParser->fts5yytos!=0 );
#if !defined(fts5YYERRORSYMBOL) && !defined(fts5YYNOERRORRECOVERY)
  fts5yyendofinput = (fts5yymajor==0);
#endif

  fts5yyact = fts5yypParser->fts5yytos->stateno;
#ifndef NDEBUG
  if( fts5yyTraceFILE ){
    if( fts5yyact < fts5YY_MIN_REDUCE ){
      fprintf(fts5yyTraceFILE,"%sInput '%s' in state %d\n",
              fts5yyTracePrompt,fts5yyTokenName[fts5yymajor],fts5yyact);
    }else{
      fprintf(fts5yyTraceFILE,"%sInput '%s' with pending reduce %d\n",
              fts5yyTracePrompt,fts5yyTokenName[fts5yymajor],fts5yyact-fts5YY_MIN_REDUCE);
    }
  }
#endif

  while(1){ /* Exit by "break" */
    assert( fts5yypParser->fts5yytos>=fts5yypParser->fts5yystack );
    assert( fts5yyact==fts5yypParser->fts5yytos->stateno );
    fts5yyact = fts5yy_find_shift_action((fts5YYCODETYPE)fts5yymajor,fts5yyact);
    if( fts5yyact >= fts5YY_MIN_REDUCE ){
      unsigned int fts5yyruleno = fts5yyact - fts5YY_MIN_REDUCE; /* Reduce by this rule */
#ifndef NDEBUG
      assert( fts5yyruleno<(int)(sizeof(fts5yyRuleName)/sizeof(fts5yyRuleName[0])) );
      if( fts5yyTraceFILE ){
        int fts5yysize = fts5yyRuleInfoNRhs[fts5yyruleno];
        if( fts5yysize ){
          fprintf(fts5yyTraceFILE, "%sReduce %d [%s]%s, pop back to state %d.\n",
            fts5yyTracePrompt,
            fts5yyruleno, fts5yyRuleName[fts5yyruleno],
            fts5yyruleno<fts5YYNRULE_WITH_ACTION ? "" : " without external action",
            fts5yypParser->fts5yytos[fts5yysize].stateno);
        }else{
          fprintf(fts5yyTraceFILE, "%sReduce %d [%s]%s.\n",
            fts5yyTracePrompt, fts5yyruleno, fts5yyRuleName[fts5yyruleno],
            fts5yyruleno<fts5YYNRULE_WITH_ACTION ? "" : " without external action");
        }
      }
#endif /* NDEBUG */

      /* Check that the stack is large enough to grow by a single entry
      ** if the RHS of the rule is empty.  This ensures that there is room
      ** enough on the stack to push the LHS value */
      if( fts5yyRuleInfoNRhs[fts5yyruleno]==0 ){
#ifdef fts5YYTRACKMAXSTACKDEPTH
        if( (int)(fts5yypParser->fts5yytos - fts5yypParser->fts5yystack)>fts5yypParser->fts5yyhwm ){
          fts5yypParser->fts5yyhwm++;
          assert( fts5yypParser->fts5yyhwm ==
                  (int)(fts5yypParser->fts5yytos - fts5yypParser->fts5yystack));
        }
#endif
        if( fts5yypParser->fts5yytos>=fts5yypParser->fts5yystackEnd ){
          if( fts5yyGrowStack(fts5yypParser) ){
            fts5yyStackOverflow(fts5yypParser);
            break;
          }
        }
      }
      fts5yyact = fts5yy_reduce(fts5yypParser,fts5yyruleno,fts5yymajor,fts5yyminor sqlite3Fts5ParserCTX_PARAM);
    }else if( fts5yyact <= fts5YY_MAX_SHIFTREDUCE ){
      fts5yy_shift(fts5yypParser,fts5yyact,(fts5YYCODETYPE)fts5yymajor,fts5yyminor);
#ifndef fts5YYNOERRORRECOVERY
      fts5yypParser->fts5yyerrcnt--;
#endif
      break;
    }else if( fts5yyact==fts5YY_ACCEPT_ACTION ){
      fts5yypParser->fts5yytos--;
      fts5yy_accept(fts5yypParser);
      return;
    }else{
      assert( fts5yyact == fts5YY_ERROR_ACTION );
      fts5yyminorunion.fts5yy0 = fts5yyminor;
#ifdef fts5YYERRORSYMBOL
      int fts5yymx;
#endif
#ifndef NDEBUG
      if( fts5yyTraceFILE ){
        fprintf(fts5yyTraceFILE,"%sSyntax Error!\n",fts5yyTracePrompt);
      }
#endif
#ifdef fts5YYERRORSYMBOL
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
      if( fts5yypParser->fts5yyerrcnt<0 ){
        fts5yy_syntax_error(fts5yypParser,fts5yymajor,fts5yyminor);
      }
      fts5yymx = fts5yypParser->fts5yytos->major;
      if( fts5yymx==fts5YYERRORSYMBOL || fts5yyerrorhit ){
#ifndef NDEBUG
        if( fts5yyTraceFILE ){
          fprintf(fts5yyTraceFILE,"%sDiscard input token %s\n",
             fts5yyTracePrompt,fts5yyTokenName[fts5yymajor]);
        }
#endif
        fts5yy_destructor(fts5yypParser, (fts5YYCODETYPE)fts5yymajor, &fts5yyminorunion);
        fts5yymajor = fts5YYNOCODE;
      }else{
        while( fts5yypParser->fts5yytos > fts5yypParser->fts5yystack ){
          fts5yyact = fts5yy_find_reduce_action(fts5yypParser->fts5yytos->stateno,
                                        fts5YYERRORSYMBOL);
          if( fts5yyact<=fts5YY_MAX_SHIFTREDUCE ) break;
          fts5yy_pop_parser_stack(fts5yypParser);
        }
        if( fts5yypParser->fts5yytos <= fts5yypParser->fts5yystack || fts5yymajor==0 ){
          fts5yy_destructor(fts5yypParser,(fts5YYCODETYPE)fts5yymajor,&fts5yyminorunion);
          fts5yy_parse_failed(fts5yypParser);
#ifndef fts5YYNOERRORRECOVERY
          fts5yypParser->fts5yyerrcnt = -1;
#endif
          fts5yymajor = fts5YYNOCODE;
        }else if( fts5yymx!=fts5YYERRORSYMBOL ){
          fts5yy_shift(fts5yypParser,fts5yyact,fts5YYERRORSYMBOL,fts5yyminor);
        }
      }
      fts5yypParser->fts5yyerrcnt = 3;
      fts5yyerrorhit = 1;
      if( fts5yymajor==fts5YYNOCODE ) break;
      fts5yyact = fts5yypParser->fts5yytos->stateno;
#elif defined(fts5YYNOERRORRECOVERY)
      /* If the fts5YYNOERRORRECOVERY macro is defined, then do not attempt to
      ** do any kind of error recovery.  Instead, simply invoke the syntax
      ** error routine and continue going as if nothing had happened.
      **
      ** Applications can set this macro (for example inside %include) if
      ** they intend to abandon the parse upon the first syntax error seen.
      */
      fts5yy_syntax_error(fts5yypParser,fts5yymajor, fts5yyminor);
      fts5yy_destructor(fts5yypParser,(fts5YYCODETYPE)fts5yymajor,&fts5yyminorunion);
      break;
#else  /* fts5YYERRORSYMBOL is not defined */
      /* This is what we do if the grammar does not define ERROR:
      **
      **  * Report an error message, and throw away the input token.
      **
      **  * If the input token is $, then fail the parse.
      **
      ** As before, subsequent error messages are suppressed until
      ** three input tokens have been successfully shifted.
      */
      if( fts5yypParser->fts5yyerrcnt<=0 ){
        fts5yy_syntax_error(fts5yypParser,fts5yymajor, fts5yyminor);
      }
      fts5yypParser->fts5yyerrcnt = 3;
      fts5yy_destructor(fts5yypParser,(fts5YYCODETYPE)fts5yymajor,&fts5yyminorunion);
      if( fts5yyendofinput ){
        fts5yy_parse_failed(fts5yypParser);
#ifndef fts5YYNOERRORRECOVERY
        fts5yypParser->fts5yyerrcnt = -1;
#endif
      }
      break;
#endif
    }
  }
#ifndef NDEBUG
  if( fts5yyTraceFILE ){
    fts5yyStackEntry *i;
    char cDiv = '[';
    fprintf(fts5yyTraceFILE,"%sReturn. Stack=",fts5yyTracePrompt);
    for(i=&fts5yypParser->fts5yystack[1]; i<=fts5yypParser->fts5yytos; i++){
      fprintf(fts5yyTraceFILE,"%c%s", cDiv, fts5yyTokenName[i->major]);
      cDiv = ' ';
    }
    fprintf(fts5yyTraceFILE,"]\n");
  }
#endif
  return;
}

/*
** Return the fallback token corresponding to canonical token iToken, or
** 0 if iToken has no fallback.
*/
static int sqlite3Fts5ParserFallback(int iToken){
#ifdef fts5YYFALLBACK
  assert( iToken<(int)(sizeof(fts5yyFallback)/sizeof(fts5yyFallback[0])) );
  return fts5yyFallback[iToken];
#else
  (void)iToken;
  return 0;
#endif
}

/*
** 2014 May 31
**
** The author disclaims copyright to this source code.  In place of
** a legal notice, here is a blessing:
**
**    May you do good and not evil.
**    May you find forgiveness for yourself and forgive others.
**    May you share freely, never taking more than you give.
**
******************************************************************************
*/


/* #include "fts5Int.h" */
#include <math.h>                 /* amalgamator: keep */

/*
** Object used to iterate through all "coalesced phrase instances" in
** a single column of the current row. If the phrase instances in the
** column being considered do not overlap, this object simply iterates
** through them. Or, if they do overlap (share one or more tokens in
** common), each set of overlapping instances is treated as a single
** match. See documentation for the highlight() auxiliary function for
** details.
**
** Usage is:
**
**   for(rc = fts5CInstIterNext(pApi, pFts, iCol, &iter);
**      (rc==SQLITE_OK && 0==fts5CInstIterEof(&iter);
**      rc = fts5CInstIterNext(&iter)
**   ){
**     printf("instance starts at %d, ends at %d\n", iter.iStart, iter.iEnd);
**   }
**
*/
typedef struct CInstIter CInstIter;
struct CInstIter {
  const Fts5ExtensionApi *pApi;   /* API offered by current FTS version */
  Fts5Context *pFts;              /* First arg to pass to pApi functions */
  int iCol;                       /* Column to search */
  int iInst;                      /* Next phrase instance index */
  int nInst;                      /* Total number of phrase instances */

  /* Output variables */
  int iStart;                     /* First token in coalesced phrase instance */
  int iEnd;                       /* Last token in coalesced phrase instance */
};

/*
** Advance the iterator to the next coalesced phrase instance. Return
** an SQLite error code if an error occurs, or SQLITE_OK otherwise.
*/
static int fts5CInstIterNext(CInstIter *pIter){
  int rc = SQLITE_OK;
  pIter->iStart = -1;
  pIter->iEnd = -1;

  while( rc==SQLITE_OK && pIter->iInst<pIter->nInst ){
    int ip; int ic; int io;
    rc = pIter->pApi->xInst(pIter->pFts, pIter->iInst, &ip, &ic, &io);
    if( rc==SQLITE_OK ){
      if( ic==pIter->iCol ){
        int iEnd = io - 1 + pIter->pApi->xPhraseSize(pIter->pFts, ip);
        if( pIter->iStart<0 ){
          pIter->iStart = io;
          pIter->iEnd = iEnd;
        }else if( io<=pIter->iEnd ){
          if( iEnd>pIter->iEnd ) pIter->iEnd = iEnd;
        }else{
          break;
        }
      }
      pIter->iInst++;
    }
  }

  return rc;
}

/*
** Initialize the iterator object indicated by the final parameter to
** iterate through coalesced phrase instances in column iCol.
*/
static int fts5CInstIterInit(
  const Fts5ExtensionApi *pApi,
  Fts5Context *pFts,
  int iCol,
  CInstIter *pIter
){
  int rc;

  memset(pIter, 0, sizeof(CInstIter));
  pIter->pApi = pApi;
  pIter->pFts = pFts;
  pIter->iCol = iCol;
  rc = pApi->xInstCount(pFts, &pIter->nInst);

  if( rc==SQLITE_OK ){
    rc = fts5CInstIterNext(pIter);
  }

  return rc;
}
