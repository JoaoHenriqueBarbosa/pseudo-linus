#endif /* fts5YYGROWABLESTACK */

#if !fts5YYGROWABLESTACK
/* For builds that do no have a growable stack, fts5yyGrowStack always
** returns an error.
*/
# define fts5yyGrowStack(X) 1
#endif

/* Datatype of the argument to the memory allocated passed as the
** second argument to sqlite3Fts5ParserAlloc() below.  This can be changed by
** putting an appropriate #define in the %include section of the input
** grammar.
*/
#ifndef fts5YYMALLOCARGTYPE
# define fts5YYMALLOCARGTYPE size_t
#endif

/* Initialize a new parser that has already been allocated.
*/
static void sqlite3Fts5ParserInit(void *fts5yypRawParser sqlite3Fts5ParserCTX_PDECL){
  fts5yyParser *fts5yypParser = (fts5yyParser*)fts5yypRawParser;
  sqlite3Fts5ParserCTX_STORE
#ifdef fts5YYTRACKMAXSTACKDEPTH
  fts5yypParser->fts5yyhwm = 0;
#endif
  fts5yypParser->fts5yystack = fts5yypParser->fts5yystk0;
  fts5yypParser->fts5yystackEnd = &fts5yypParser->fts5yystack[fts5YYSTACKDEPTH-1];
#ifndef fts5YYNOERRORRECOVERY
  fts5yypParser->fts5yyerrcnt = -1;
#endif
  fts5yypParser->fts5yytos = fts5yypParser->fts5yystack;
  fts5yypParser->fts5yystack[0].stateno = 0;
  fts5yypParser->fts5yystack[0].major = 0;
}

#ifndef sqlite3Fts5Parser_ENGINEALWAYSONSTACK
/*
** This function allocates a new parser.
** The only argument is a pointer to a function which works like
** malloc.
**
** Inputs:
** A pointer to the function used to allocate memory.
**
** Outputs:
** A pointer to a parser.  This pointer is used in subsequent calls
** to sqlite3Fts5Parser and sqlite3Fts5ParserFree.
*/
static void *sqlite3Fts5ParserAlloc(void *(*mallocProc)(fts5YYMALLOCARGTYPE) sqlite3Fts5ParserCTX_PDECL){
  fts5yyParser *fts5yypParser;
  fts5yypParser = (fts5yyParser*)(*mallocProc)( (fts5YYMALLOCARGTYPE)sizeof(fts5yyParser) );
  if( fts5yypParser ){
    sqlite3Fts5ParserCTX_STORE
    sqlite3Fts5ParserInit(fts5yypParser sqlite3Fts5ParserCTX_PARAM);
  }
  return (void*)fts5yypParser;
}
#endif /* sqlite3Fts5Parser_ENGINEALWAYSONSTACK */


/* The following function deletes the "minor type" or semantic value
** associated with a symbol.  The symbol can be either a terminal
** or nonterminal. "fts5yymajor" is the symbol code, and "fts5yypminor" is
** a pointer to the value to be deleted.  The code used to do the
** deletions is derived from the %destructor and/or %token_destructor
** directives of the input grammar.
*/
static void fts5yy_destructor(
  fts5yyParser *fts5yypParser,    /* The parser */
  fts5YYCODETYPE fts5yymajor,     /* Type code for object to destroy */
  fts5YYMINORTYPE *fts5yypminor   /* The object to be destroyed */
){
  sqlite3Fts5ParserARG_FETCH
  sqlite3Fts5ParserCTX_FETCH
  switch( fts5yymajor ){
    /* Here is inserted the actions which take place when a
    ** terminal or non-terminal is destroyed.  This can happen
    ** when the symbol is popped from the stack during a
    ** reduce or during error processing or when a parser is
    ** being destroyed before it is finished parsing.
    **
    ** Note: during a reduce, the only symbols destroyed are those
    ** which appear on the RHS of the rule, but which are *not* used
    ** inside the C code.
    */
/********* Begin destructor definitions ***************************************/
    case 16: /* input */
{
 (void)pParse;
}
      break;
    case 17: /* expr */
    case 18: /* cnearset */
    case 19: /* exprlist */
{
 sqlite3Fts5ParseNodeFree((fts5yypminor->fts5yy24));
}
      break;
    case 20: /* colset */
    case 21: /* colsetlist */
{
 sqlite3_free((fts5yypminor->fts5yy11));
}
      break;
    case 22: /* nearset */
    case 23: /* nearphrases */
{
 sqlite3Fts5ParseNearsetFree((fts5yypminor->fts5yy46));
}
      break;
    case 24: /* phrase */
{
 sqlite3Fts5ParsePhraseFree((fts5yypminor->fts5yy53));
}
      break;
/********* End destructor definitions *****************************************/
    default:  break;   /* If no destructor action specified: do nothing */
  }
}

/*
** Pop the parser's stack once.
**
** If there is a destructor routine associated with the token which
** is popped from the stack, then call it.
*/
static void fts5yy_pop_parser_stack(fts5yyParser *pParser){
  fts5yyStackEntry *fts5yytos;
  assert( pParser->fts5yytos!=0 );
  assert( pParser->fts5yytos > pParser->fts5yystack );
  fts5yytos = pParser->fts5yytos--;
#ifndef NDEBUG
  if( fts5yyTraceFILE ){
    fprintf(fts5yyTraceFILE,"%sPopping %s\n",
      fts5yyTracePrompt,
      fts5yyTokenName[fts5yytos->major]);
  }
#endif
  fts5yy_destructor(pParser, fts5yytos->major, &fts5yytos->minor);
}

/*
** Clear all secondary memory allocations from the parser
*/
static void sqlite3Fts5ParserFinalize(void *p){
  fts5yyParser *pParser = (fts5yyParser*)p;

  /* In-lined version of calling fts5yy_pop_parser_stack() for each
  ** element left in the stack */
  fts5yyStackEntry *fts5yytos = pParser->fts5yytos;
  while( fts5yytos>pParser->fts5yystack ){
#ifndef NDEBUG
    if( fts5yyTraceFILE ){
      fprintf(fts5yyTraceFILE,"%sPopping %s\n",
        fts5yyTracePrompt,
        fts5yyTokenName[fts5yytos->major]);
    }
#endif
    if( fts5yytos->major>=fts5YY_MIN_DSTRCTR ){
      fts5yy_destructor(pParser, fts5yytos->major, &fts5yytos->minor);
    }
    fts5yytos--;
  }

#if fts5YYGROWABLESTACK
  if( pParser->fts5yystack!=pParser->fts5yystk0 ) fts5YYFREE(pParser->fts5yystack);
#endif
}

#ifndef sqlite3Fts5Parser_ENGINEALWAYSONSTACK
/*
** Deallocate and destroy a parser.  Destructors are called for
** all stack elements before shutting the parser down.
**
** If the fts5YYPARSEFREENEVERNULL macro exists (for example because it
** is defined in a %include section of the input grammar) then it is
** assumed that the input pointer is never NULL.
*/
static void sqlite3Fts5ParserFree(
  void *p,                    /* The parser to be deleted */
  void (*freeProc)(void*)     /* Function used to reclaim memory */
){
#ifndef fts5YYPARSEFREENEVERNULL
  if( p==0 ) return;
#endif
  sqlite3Fts5ParserFinalize(p);
  (*freeProc)(p);
}
#endif /* sqlite3Fts5Parser_ENGINEALWAYSONSTACK */

/*
** Return the peak depth of the stack for a parser.
*/
#ifdef fts5YYTRACKMAXSTACKDEPTH
static int sqlite3Fts5ParserStackPeak(void *p){
  fts5yyParser *pParser = (fts5yyParser*)p;
  return pParser->fts5yyhwm;
}
#endif

/* This array of booleans keeps track of the parser statement
** coverage.  The element fts5yycoverage[X][Y] is set when the parser
** is in state X and has a lookahead token Y.  In a well-tested
** systems, every element of this matrix should end up being set.
*/
#if defined(fts5YYCOVERAGE)
static unsigned char fts5yycoverage[fts5YYNSTATE][fts5YYNFTS5TOKEN];
#endif

/*
** Write into out a description of every state/lookahead combination that
**
**   (1)  has not been used by the parser, and
**   (2)  is not a syntax error.
**
** Return the number of missed state/lookahead combinations.
*/
#if defined(fts5YYCOVERAGE)
static int sqlite3Fts5ParserCoverage(FILE *out){
  int stateno, iLookAhead, i;
  int nMissed = 0;
  for(stateno=0; stateno<fts5YYNSTATE; stateno++){
    i = fts5yy_shift_ofst[stateno];
    for(iLookAhead=0; iLookAhead<fts5YYNFTS5TOKEN; iLookAhead++){
      if( fts5yy_lookahead[i+iLookAhead]!=iLookAhead ) continue;
      if( fts5yycoverage[stateno][iLookAhead]==0 ) nMissed++;
      if( out ){
        fprintf(out,"State %d lookahead %s %s\n", stateno,
                fts5yyTokenName[iLookAhead],
                fts5yycoverage[stateno][iLookAhead] ? "ok" : "missed");
      }
    }
  }
  return nMissed;
}
#endif

/*
** Find the appropriate action for a parser given the terminal
** look-ahead token iLookAhead.
*/
static fts5YYACTIONTYPE fts5yy_find_shift_action(
  fts5YYCODETYPE iLookAhead,    /* The look-ahead token */
  fts5YYACTIONTYPE stateno      /* Current state number */
){
  int i;

  if( stateno>fts5YY_MAX_SHIFT ) return stateno;
  assert( stateno <= fts5YY_SHIFT_COUNT );
#if defined(fts5YYCOVERAGE)
  fts5yycoverage[stateno][iLookAhead] = 1;
#endif
  do{
    i = fts5yy_shift_ofst[stateno];
    assert( i>=0 );
    assert( i<=fts5YY_ACTTAB_COUNT );
    assert( i+fts5YYNFTS5TOKEN<=(int)fts5YY_NLOOKAHEAD );
    assert( iLookAhead!=fts5YYNOCODE );
    assert( iLookAhead < fts5YYNFTS5TOKEN );
    i += iLookAhead;
    assert( i<(int)fts5YY_NLOOKAHEAD );
    if( fts5yy_lookahead[i]!=iLookAhead ){
#ifdef fts5YYFALLBACK
      fts5YYCODETYPE iFallback;            /* Fallback token */
      assert( iLookAhead<sizeof(fts5yyFallback)/sizeof(fts5yyFallback[0]) );
      iFallback = fts5yyFallback[iLookAhead];
      if( iFallback!=0 ){
#ifndef NDEBUG
        if( fts5yyTraceFILE ){
          fprintf(fts5yyTraceFILE, "%sFALLBACK %s => %s\n",
             fts5yyTracePrompt, fts5yyTokenName[iLookAhead], fts5yyTokenName[iFallback]);
        }
#endif
        assert( fts5yyFallback[iFallback]==0 ); /* Fallback loop must terminate */
        iLookAhead = iFallback;
        continue;
      }
#endif
#ifdef fts5YYWILDCARD
      {
        int j = i - iLookAhead + fts5YYWILDCARD;
        assert( j<(int)(sizeof(fts5yy_lookahead)/sizeof(fts5yy_lookahead[0])) );
        if( fts5yy_lookahead[j]==fts5YYWILDCARD && iLookAhead>0 ){
#ifndef NDEBUG
          if( fts5yyTraceFILE ){
            fprintf(fts5yyTraceFILE, "%sWILDCARD %s => %s\n",
               fts5yyTracePrompt, fts5yyTokenName[iLookAhead],
               fts5yyTokenName[fts5YYWILDCARD]);
          }
#endif /* NDEBUG */
          return fts5yy_action[j];
        }
      }
#endif /* fts5YYWILDCARD */
      return fts5yy_default[stateno];
    }else{
      assert( i>=0 && i<(int)(sizeof(fts5yy_action)/sizeof(fts5yy_action[0])) );
      return fts5yy_action[i];
    }
  }while(1);
}

/*
** Find the appropriate action for a parser given the non-terminal
** look-ahead token iLookAhead.
*/
static fts5YYACTIONTYPE fts5yy_find_reduce_action(
  fts5YYACTIONTYPE stateno,     /* Current state number */
  fts5YYCODETYPE iLookAhead     /* The look-ahead token */
){
  int i;
#ifdef fts5YYERRORSYMBOL
  if( stateno>fts5YY_REDUCE_COUNT ){
    return fts5yy_default[stateno];
  }
#else
  assert( stateno<=fts5YY_REDUCE_COUNT );
#endif
  i = fts5yy_reduce_ofst[stateno];
  assert( iLookAhead!=fts5YYNOCODE );
  i += iLookAhead;
#ifdef fts5YYERRORSYMBOL
  if( i<0 || i>=fts5YY_ACTTAB_COUNT || fts5yy_lookahead[i]!=iLookAhead ){
    return fts5yy_default[stateno];
  }
#else
  assert( i>=0 && i<fts5YY_ACTTAB_COUNT );
  assert( fts5yy_lookahead[i]==iLookAhead );
#endif
  return fts5yy_action[i];
}

/*
** The following routine is called if the stack overflows.
*/
static void fts5yyStackOverflow(fts5yyParser *fts5yypParser){
   sqlite3Fts5ParserARG_FETCH
   sqlite3Fts5ParserCTX_FETCH
#ifndef NDEBUG
   if( fts5yyTraceFILE ){
     fprintf(fts5yyTraceFILE,"%sStack Overflow!\n",fts5yyTracePrompt);
   }
#endif
   while( fts5yypParser->fts5yytos>fts5yypParser->fts5yystack ) fts5yy_pop_parser_stack(fts5yypParser);
   /* Here code is inserted which will execute if the parser
   ** stack every overflows */
/******** Begin %stack_overflow code ******************************************/

  sqlite3Fts5ParseError(pParse, "fts5: parser stack overflow");
/******** End %stack_overflow code ********************************************/
   sqlite3Fts5ParserARG_STORE /* Suppress warning about unused %extra_argument var */
   sqlite3Fts5ParserCTX_STORE
}
