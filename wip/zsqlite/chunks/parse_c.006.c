
/*
** Find the appropriate action for a parser given the non-terminal
** look-ahead token iLookAhead.
*/
static YYACTIONTYPE yy_find_reduce_action(
  YYACTIONTYPE stateno,     /* Current state number */
  YYCODETYPE iLookAhead     /* The look-ahead token */
){
  int i;
#ifdef YYERRORSYMBOL
  if( stateno>YY_REDUCE_COUNT ){
    return yy_default[stateno];
  }
#else
  assert( stateno<=YY_REDUCE_COUNT );
#endif
  i = yy_reduce_ofst[stateno];
  assert( iLookAhead!=YYNOCODE );
  i += iLookAhead;
#ifdef YYERRORSYMBOL
  if( i<0 || i>=YY_ACTTAB_COUNT || yy_lookahead[i]!=iLookAhead ){
    return yy_default[stateno];
  }
#else
  assert( i>=0 && i<YY_ACTTAB_COUNT );
  assert( yy_lookahead[i]==iLookAhead );
#endif
  return yy_action[i];
}

/*
** The following routine is called if the stack overflows.
*/
static void yyStackOverflow(yyParser *yypParser){
   sqlite3ParserARG_FETCH
   sqlite3ParserCTX_FETCH
#ifndef NDEBUG
   if( yyTraceFILE ){
     fprintf(yyTraceFILE,"%sStack Overflow!\n",yyTracePrompt);
   }
#endif
   while( yypParser->yytos>yypParser->yystack ) yy_pop_parser_stack(yypParser);
   /* Here code is inserted which will execute if the parser
   ** stack every overflows */
/******** Begin %stack_overflow code ******************************************/

  sqlite3OomFault(pParse->db);
/******** End %stack_overflow code ********************************************/
   sqlite3ParserARG_STORE /* Suppress warning about unused %extra_argument var */
   sqlite3ParserCTX_STORE
}

/*
** Print tracing information for a SHIFT action
*/
#ifndef NDEBUG
static void yyTraceShift(yyParser *yypParser, int yyNewState, const char *zTag){
  if( yyTraceFILE ){
    if( yyNewState<YYNSTATE ){
      fprintf(yyTraceFILE,"%s%s '%s', go to state %d\n",
         yyTracePrompt, zTag, yyTokenName[yypParser->yytos->major],
         yyNewState);
    }else{
      fprintf(yyTraceFILE,"%s%s '%s', pending reduce %d\n",
         yyTracePrompt, zTag, yyTokenName[yypParser->yytos->major],
         yyNewState - YY_MIN_REDUCE);
    }
  }
}
#else
# define yyTraceShift(X,Y,Z)
#endif

/*
** Perform a shift action.
*/
static void yy_shift(
  yyParser *yypParser,          /* The parser to be shifted */
  YYACTIONTYPE yyNewState,      /* The new state to shift in */
  YYCODETYPE yyMajor,           /* The major token to shift in */
  sqlite3ParserTOKENTYPE yyMinor        /* The minor token to shift in */
){
  yyStackEntry *yytos;
  yypParser->yytos++;
#ifdef YYTRACKMAXSTACKDEPTH
  if( (int)(yypParser->yytos - yypParser->yystack)>yypParser->yyhwm ){
    yypParser->yyhwm++;
    assert( yypParser->yyhwm == (int)(yypParser->yytos - yypParser->yystack) );
  }
#endif
  yytos = yypParser->yytos;
  if( yytos>yypParser->yystackEnd ){
    if( yyGrowStack(yypParser) ){
      yypParser->yytos--;
      yyStackOverflow(yypParser);
      return;
    }
    yytos = yypParser->yytos;
    assert( yytos <= yypParser->yystackEnd );
  }
  if( yyNewState > YY_MAX_SHIFT ){
    yyNewState += YY_MIN_REDUCE - YY_MIN_SHIFTREDUCE;
  }
  yytos->stateno = yyNewState;
  yytos->major = yyMajor;
  yytos->minor.yy0 = yyMinor;
  yyTraceShift(yypParser, yyNewState, "Shift");
}

/* For rule J, yyRuleInfoLhs[J] contains the symbol on the left-hand side
** of that rule */
static const YYCODETYPE yyRuleInfoLhs[] = {
   190,  /* (0) explain ::= EXPLAIN */
   190,  /* (1) explain ::= EXPLAIN QUERY PLAN */
   189,  /* (2) cmdx ::= cmd */
   191,  /* (3) cmd ::= BEGIN transtype trans_opt */
   192,  /* (4) transtype ::= */
   192,  /* (5) transtype ::= DEFERRED */
   192,  /* (6) transtype ::= IMMEDIATE */
   192,  /* (7) transtype ::= EXCLUSIVE */
   191,  /* (8) cmd ::= COMMIT|END trans_opt */
   191,  /* (9) cmd ::= ROLLBACK trans_opt */
   191,  /* (10) cmd ::= SAVEPOINT nm */
   191,  /* (11) cmd ::= RELEASE savepoint_opt nm */
   191,  /* (12) cmd ::= ROLLBACK trans_opt TO savepoint_opt nm */
   196,  /* (13) create_table ::= createkw temp TABLE ifnotexists nm dbnm */
   198,  /* (14) createkw ::= CREATE */
   200,  /* (15) ifnotexists ::= */
   200,  /* (16) ifnotexists ::= IF NOT EXISTS */
   199,  /* (17) temp ::= TEMP */
   199,  /* (18) temp ::= */
   197,  /* (19) create_table_args ::= LP columnlist conslist_opt RP table_option_set */
   197,  /* (20) create_table_args ::= AS select */
   204,  /* (21) table_option_set ::= */
   204,  /* (22) table_option_set ::= table_option_set COMMA table_option */
   206,  /* (23) table_option ::= WITHOUT nm */
   206,  /* (24) table_option ::= nm */
   207,  /* (25) columnname ::= nm typetoken */
   209,  /* (26) typetoken ::= */
   209,  /* (27) typetoken ::= typename LP signed RP */
   209,  /* (28) typetoken ::= typename LP signed COMMA signed RP */
   210,  /* (29) typename ::= typename ID|STRING */
   214,  /* (30) scanpt ::= */
   215,  /* (31) scantok ::= */
   216,  /* (32) ccons ::= CONSTRAINT nm */
   216,  /* (33) ccons ::= DEFAULT scantok term */
   216,  /* (34) ccons ::= DEFAULT LP expr RP */
   216,  /* (35) ccons ::= DEFAULT PLUS scantok term */
   216,  /* (36) ccons ::= DEFAULT MINUS scantok term */
   216,  /* (37) ccons ::= DEFAULT scantok ID|INDEXED */
   216,  /* (38) ccons ::= NOT NULL onconf */
   216,  /* (39) ccons ::= PRIMARY KEY sortorder onconf autoinc */
   216,  /* (40) ccons ::= UNIQUE onconf */
   216,  /* (41) ccons ::= CHECK LP expr RP */
   216,  /* (42) ccons ::= REFERENCES nm eidlist_opt refargs */
   216,  /* (43) ccons ::= defer_subclause */
   216,  /* (44) ccons ::= COLLATE ID|STRING */
   225,  /* (45) generated ::= LP expr RP */
   225,  /* (46) generated ::= LP expr RP ID */
   221,  /* (47) autoinc ::= */
   221,  /* (48) autoinc ::= AUTOINCR */
   223,  /* (49) refargs ::= */
   223,  /* (50) refargs ::= refargs refarg */
   226,  /* (51) refarg ::= MATCH nm */
   226,  /* (52) refarg ::= ON INSERT refact */
   226,  /* (53) refarg ::= ON DELETE refact */
   226,  /* (54) refarg ::= ON UPDATE refact */
   227,  /* (55) refact ::= SET NULL */
   227,  /* (56) refact ::= SET DEFAULT */
   227,  /* (57) refact ::= CASCADE */
   227,  /* (58) refact ::= RESTRICT */
   227,  /* (59) refact ::= NO ACTION */
   224,  /* (60) defer_subclause ::= NOT DEFERRABLE init_deferred_pred_opt */
   224,  /* (61) defer_subclause ::= DEFERRABLE init_deferred_pred_opt */
   228,  /* (62) init_deferred_pred_opt ::= */
   228,  /* (63) init_deferred_pred_opt ::= INITIALLY DEFERRED */
   228,  /* (64) init_deferred_pred_opt ::= INITIALLY IMMEDIATE */
   203,  /* (65) conslist_opt ::= */
   230,  /* (66) tconscomma ::= COMMA */
   231,  /* (67) tcons ::= CONSTRAINT nm */
   231,  /* (68) tcons ::= PRIMARY KEY LP sortlist autoinc RP onconf */
   231,  /* (69) tcons ::= UNIQUE LP sortlist RP onconf */
   231,  /* (70) tcons ::= CHECK LP expr RP onconf */
   231,  /* (71) tcons ::= FOREIGN KEY LP eidlist RP REFERENCES nm eidlist_opt refargs defer_subclause_opt */
   234,  /* (72) defer_subclause_opt ::= */
   219,  /* (73) onconf ::= */
   219,  /* (74) onconf ::= ON CONFLICT resolvetype */
   235,  /* (75) orconf ::= */
   235,  /* (76) orconf ::= OR resolvetype */
   236,  /* (77) resolvetype ::= IGNORE */
   236,  /* (78) resolvetype ::= REPLACE */
   191,  /* (79) cmd ::= DROP TABLE ifexists fullname */
   238,  /* (80) ifexists ::= IF EXISTS */
   238,  /* (81) ifexists ::= */
   191,  /* (82) cmd ::= createkw temp VIEW ifnotexists nm dbnm eidlist_opt AS select */
   191,  /* (83) cmd ::= DROP VIEW ifexists fullname */
   191,  /* (84) cmd ::= select */
   205,  /* (85) select ::= WITH wqlist selectnowith */
   205,  /* (86) select ::= WITH RECURSIVE wqlist selectnowith */
   205,  /* (87) select ::= selectnowith */
   240,  /* (88) selectnowith ::= selectnowith multiselect_op oneselect */
   243,  /* (89) multiselect_op ::= UNION */
   243,  /* (90) multiselect_op ::= UNION ALL */
   243,  /* (91) multiselect_op ::= EXCEPT|INTERSECT */
   241,  /* (92) oneselect ::= SELECT distinct selcollist from where_opt groupby_opt having_opt orderby_opt limit_opt */
   241,  /* (93) oneselect ::= SELECT distinct selcollist from where_opt groupby_opt having_opt window_clause orderby_opt limit_opt */
   253,  /* (94) values ::= VALUES LP nexprlist RP */
   241,  /* (95) oneselect ::= mvalues */
   255,  /* (96) mvalues ::= values COMMA LP nexprlist RP */
   255,  /* (97) mvalues ::= mvalues COMMA LP nexprlist RP */
   244,  /* (98) distinct ::= DISTINCT */
   244,  /* (99) distinct ::= ALL */
   244,  /* (100) distinct ::= */
   256,  /* (101) sclp ::= */
   245,  /* (102) selcollist ::= sclp scanpt expr scanpt as */
   245,  /* (103) selcollist ::= sclp scanpt STAR */
   245,  /* (104) selcollist ::= sclp scanpt nm DOT STAR */
   257,  /* (105) as ::= AS nm */
   257,  /* (106) as ::= */
   246,  /* (107) from ::= */
   246,  /* (108) from ::= FROM seltablist */
   259,  /* (109) stl_prefix ::= seltablist joinop */
   259,  /* (110) stl_prefix ::= */
   258,  /* (111) seltablist ::= stl_prefix nm dbnm as on_using */
   258,  /* (112) seltablist ::= stl_prefix nm dbnm as indexed_by on_using */
   258,  /* (113) seltablist ::= stl_prefix nm dbnm LP exprlist RP as on_using */
   258,  /* (114) seltablist ::= stl_prefix LP select RP as on_using */
   258,  /* (115) seltablist ::= stl_prefix LP seltablist RP as on_using */
   201,  /* (116) dbnm ::= */
   201,  /* (117) dbnm ::= DOT nm */
   239,  /* (118) fullname ::= nm */
   239,  /* (119) fullname ::= nm DOT nm */
   264,  /* (120) xfullname ::= nm */
   264,  /* (121) xfullname ::= nm DOT nm */
   264,  /* (122) xfullname ::= nm DOT nm AS nm */
   264,  /* (123) xfullname ::= nm AS nm */
   260,  /* (124) joinop ::= COMMA|JOIN */
   260,  /* (125) joinop ::= JOIN_KW JOIN */
   260,  /* (126) joinop ::= JOIN_KW nm JOIN */
   260,  /* (127) joinop ::= JOIN_KW nm nm JOIN */
   261,  /* (128) on_using ::= ON expr */
   261,  /* (129) on_using ::= USING LP idlist RP */
   261,  /* (130) on_using ::= */
   266,  /* (131) indexed_opt ::= */
   262,  /* (132) indexed_by ::= INDEXED BY nm */
   262,  /* (133) indexed_by ::= NOT INDEXED */
   250,  /* (134) orderby_opt ::= */
   250,  /* (135) orderby_opt ::= ORDER BY sortlist */
   232,  /* (136) sortlist ::= sortlist COMMA expr sortorder nulls */
   232,  /* (137) sortlist ::= expr sortorder nulls */
   220,  /* (138) sortorder ::= ASC */
   220,  /* (139) sortorder ::= DESC */
   220,  /* (140) sortorder ::= */
   267,  /* (141) nulls ::= NULLS FIRST */
   267,  /* (142) nulls ::= NULLS LAST */
   267,  /* (143) nulls ::= */
   248,  /* (144) groupby_opt ::= */
   248,  /* (145) groupby_opt ::= GROUP BY nexprlist */
   249,  /* (146) having_opt ::= */
   249,  /* (147) having_opt ::= HAVING expr */
   251,  /* (148) limit_opt ::= */
   251,  /* (149) limit_opt ::= LIMIT expr */
   251,  /* (150) limit_opt ::= LIMIT expr OFFSET expr */
   251,  /* (151) limit_opt ::= LIMIT expr COMMA expr */
   191,  /* (152) cmd ::= with DELETE FROM xfullname indexed_opt where_opt_ret */
   247,  /* (153) where_opt ::= */
   247,  /* (154) where_opt ::= WHERE expr */
   269,  /* (155) where_opt_ret ::= */
   269,  /* (156) where_opt_ret ::= WHERE expr */
   269,  /* (157) where_opt_ret ::= RETURNING selcollist */
   269,  /* (158) where_opt_ret ::= WHERE expr RETURNING selcollist */
   191,  /* (159) cmd ::= with UPDATE orconf xfullname indexed_opt SET setlist from where_opt_ret */
   270,  /* (160) setlist ::= setlist COMMA nm EQ expr */
   270,  /* (161) setlist ::= setlist COMMA LP idlist RP EQ expr */
   270,  /* (162) setlist ::= nm EQ expr */
   270,  /* (163) setlist ::= LP idlist RP EQ expr */
   191,  /* (164) cmd ::= with insert_cmd INTO xfullname idlist_opt select upsert */
   191,  /* (165) cmd ::= with insert_cmd INTO xfullname idlist_opt DEFAULT VALUES returning */
   273,  /* (166) upsert ::= */
   273,  /* (167) upsert ::= RETURNING selcollist */
   273,  /* (168) upsert ::= ON CONFLICT LP sortlist RP where_opt DO UPDATE SET setlist where_opt upsert */
   273,  /* (169) upsert ::= ON CONFLICT LP sortlist RP where_opt DO NOTHING upsert */
   273,  /* (170) upsert ::= ON CONFLICT DO NOTHING returning */
   273,  /* (171) upsert ::= ON CONFLICT DO UPDATE SET setlist where_opt returning */
   274,  /* (172) returning ::= RETURNING selcollist */
   271,  /* (173) insert_cmd ::= INSERT orconf */
   271,  /* (174) insert_cmd ::= REPLACE */
   272,  /* (175) idlist_opt ::= */
   272,  /* (176) idlist_opt ::= LP idlist RP */
   265,  /* (177) idlist ::= idlist COMMA nm */
   265,  /* (178) idlist ::= nm */
   218,  /* (179) expr ::= LP expr RP */
   218,  /* (180) expr ::= ID|INDEXED|JOIN_KW */
   218,  /* (181) expr ::= nm DOT nm */
   218,  /* (182) expr ::= nm DOT nm DOT nm */
   217,  /* (183) term ::= NULL|FLOAT|BLOB */
   217,  /* (184) term ::= STRING */
   217,  /* (185) term ::= INTEGER */
   218,  /* (186) expr ::= VARIABLE */
   218,  /* (187) expr ::= expr COLLATE ID|STRING */
   218,  /* (188) expr ::= CAST LP expr AS typetoken RP */
   218,  /* (189) expr ::= ID|INDEXED|JOIN_KW LP distinct exprlist RP */
   218,  /* (190) expr ::= ID|INDEXED|JOIN_KW LP distinct exprlist ORDER BY sortlist RP */
   218,  /* (191) expr ::= ID|INDEXED|JOIN_KW LP STAR RP */
   218,  /* (192) expr ::= ID|INDEXED|JOIN_KW LP distinct exprlist RP filter_over */
   218,  /* (193) expr ::= ID|INDEXED|JOIN_KW LP distinct exprlist ORDER BY sortlist RP filter_over */
   218,  /* (194) expr ::= ID|INDEXED|JOIN_KW LP STAR RP filter_over */
   217,  /* (195) term ::= CTIME_KW */
   218,  /* (196) expr ::= LP nexprlist COMMA expr RP */
   218,  /* (197) expr ::= expr AND expr */
   218,  /* (198) expr ::= expr OR expr */
   218,  /* (199) expr ::= expr LT|GT|GE|LE expr */
   218,  /* (200) expr ::= expr EQ|NE expr */
   218,  /* (201) expr ::= expr BITAND|BITOR|LSHIFT|RSHIFT expr */
   218,  /* (202) expr ::= expr PLUS|MINUS expr */
   218,  /* (203) expr ::= expr STAR|SLASH|REM expr */
   218,  /* (204) expr ::= expr CONCAT expr */
   276,  /* (205) likeop ::= NOT LIKE_KW|MATCH */
   218,  /* (206) expr ::= expr likeop expr */
   218,  /* (207) expr ::= expr likeop expr ESCAPE expr */
   218,  /* (208) expr ::= expr ISNULL|NOTNULL */
   218,  /* (209) expr ::= expr NOT NULL */
   218,  /* (210) expr ::= expr IS expr */
   218,  /* (211) expr ::= expr IS NOT expr */
   218,  /* (212) expr ::= expr IS NOT DISTINCT FROM expr */
   218,  /* (213) expr ::= expr IS DISTINCT FROM expr */
   218,  /* (214) expr ::= NOT expr */
   218,  /* (215) expr ::= BITNOT expr */
   218,  /* (216) expr ::= PLUS|MINUS expr */
   218,  /* (217) expr ::= expr PTR expr */
   277,  /* (218) between_op ::= BETWEEN */
   277,  /* (219) between_op ::= NOT BETWEEN */
   218,  /* (220) expr ::= expr between_op expr AND expr */
   278,  /* (221) in_op ::= IN */
   278,  /* (222) in_op ::= NOT IN */
   218,  /* (223) expr ::= expr in_op LP exprlist RP */
   218,  /* (224) expr ::= LP select RP */
   218,  /* (225) expr ::= expr in_op LP select RP */
   218,  /* (226) expr ::= expr in_op nm dbnm paren_exprlist */
   218,  /* (227) expr ::= EXISTS LP select RP */
   218,  /* (228) expr ::= CASE case_operand case_exprlist case_else END */
   281,  /* (229) case_exprlist ::= case_exprlist WHEN expr THEN expr */
   281,  /* (230) case_exprlist ::= WHEN expr THEN expr */
   282,  /* (231) case_else ::= ELSE expr */
   282,  /* (232) case_else ::= */
   280,  /* (233) case_operand ::= */
   263,  /* (234) exprlist ::= */
   254,  /* (235) nexprlist ::= nexprlist COMMA expr */
   254,  /* (236) nexprlist ::= expr */
   279,  /* (237) paren_exprlist ::= */
   279,  /* (238) paren_exprlist ::= LP exprlist RP */
   191,  /* (239) cmd ::= createkw uniqueflag INDEX ifnotexists nm dbnm ON nm LP sortlist RP where_opt */
   283,  /* (240) uniqueflag ::= UNIQUE */
   283,  /* (241) uniqueflag ::= */
   222,  /* (242) eidlist_opt ::= */
   222,  /* (243) eidlist_opt ::= LP eidlist RP */
   233,  /* (244) eidlist ::= eidlist COMMA nm collate sortorder */
   233,  /* (245) eidlist ::= nm collate sortorder */
   284,  /* (246) collate ::= */
   284,  /* (247) collate ::= COLLATE ID|STRING */
   191,  /* (248) cmd ::= DROP INDEX ifexists fullname */
   191,  /* (249) cmd ::= VACUUM vinto */
   191,  /* (250) cmd ::= VACUUM nm vinto */
   285,  /* (251) vinto ::= INTO expr */
   285,  /* (252) vinto ::= */
   191,  /* (253) cmd ::= PRAGMA nm dbnm */
   191,  /* (254) cmd ::= PRAGMA nm dbnm EQ nmnum */
   191,  /* (255) cmd ::= PRAGMA nm dbnm LP nmnum RP */
   191,  /* (256) cmd ::= PRAGMA nm dbnm EQ minus_num */
   191,  /* (257) cmd ::= PRAGMA nm dbnm LP minus_num RP */
   212,  /* (258) plus_num ::= PLUS INTEGER|FLOAT */
   213,  /* (259) minus_num ::= MINUS INTEGER|FLOAT */
   191,  /* (260) cmd ::= createkw trigger_decl BEGIN trigger_cmd_list END */
   287,  /* (261) trigger_decl ::= temp TRIGGER ifnotexists nm dbnm trigger_time trigger_event ON fullname foreach_clause when_clause */
   289,  /* (262) trigger_time ::= BEFORE|AFTER */
   289,  /* (263) trigger_time ::= INSTEAD OF */
   289,  /* (264) trigger_time ::= */
   290,  /* (265) trigger_event ::= DELETE|INSERT */
   290,  /* (266) trigger_event ::= UPDATE */
   290,  /* (267) trigger_event ::= UPDATE OF idlist */
   292,  /* (268) when_clause ::= */
   292,  /* (269) when_clause ::= WHEN expr */
   288,  /* (270) trigger_cmd_list ::= trigger_cmd_list trigger_cmd SEMI */
   288,  /* (271) trigger_cmd_list ::= trigger_cmd SEMI */
   294,  /* (272) trnm ::= nm DOT nm */
   295,  /* (273) tridxby ::= INDEXED BY nm */
   295,  /* (274) tridxby ::= NOT INDEXED */
   293,  /* (275) trigger_cmd ::= UPDATE orconf trnm tridxby SET setlist from where_opt scanpt */
   293,  /* (276) trigger_cmd ::= scanpt insert_cmd INTO trnm idlist_opt select upsert scanpt */
   293,  /* (277) trigger_cmd ::= DELETE FROM trnm tridxby where_opt scanpt */
   293,  /* (278) trigger_cmd ::= scanpt select scanpt */
   218,  /* (279) expr ::= RAISE LP IGNORE RP */
   218,  /* (280) expr ::= RAISE LP raisetype COMMA nm RP */
   237,  /* (281) raisetype ::= ROLLBACK */
   237,  /* (282) raisetype ::= ABORT */
   237,  /* (283) raisetype ::= FAIL */
   191,  /* (284) cmd ::= DROP TRIGGER ifexists fullname */
   191,  /* (285) cmd ::= ATTACH database_kw_opt expr AS expr key_opt */
   191,  /* (286) cmd ::= DETACH database_kw_opt expr */
   297,  /* (287) key_opt ::= */
   297,  /* (288) key_opt ::= KEY expr */
   191,  /* (289) cmd ::= REINDEX */
   191,  /* (290) cmd ::= REINDEX nm dbnm */
   191,  /* (291) cmd ::= ANALYZE */
   191,  /* (292) cmd ::= ANALYZE nm dbnm */
   191,  /* (293) cmd ::= ALTER TABLE fullname RENAME TO nm */
   191,  /* (294) cmd ::= ALTER TABLE add_column_fullname ADD kwcolumn_opt columnname carglist */
   191,  /* (295) cmd ::= ALTER TABLE fullname DROP kwcolumn_opt nm */
   298,  /* (296) add_column_fullname ::= fullname */
   191,  /* (297) cmd ::= ALTER TABLE fullname RENAME kwcolumn_opt nm TO nm */
   191,  /* (298) cmd ::= create_vtab */
   191,  /* (299) cmd ::= create_vtab LP vtabarglist RP */
   300,  /* (300) create_vtab ::= createkw VIRTUAL TABLE ifnotexists nm dbnm USING nm */
   302,  /* (301) vtabarg ::= */
   303,  /* (302) vtabargtoken ::= ANY */
   303,  /* (303) vtabargtoken ::= lp anylist RP */
   304,  /* (304) lp ::= LP */
   268,  /* (305) with ::= WITH wqlist */
   268,  /* (306) with ::= WITH RECURSIVE wqlist */
   307,  /* (307) wqas ::= AS */
   307,  /* (308) wqas ::= AS MATERIALIZED */
   307,  /* (309) wqas ::= AS NOT MATERIALIZED */
   306,  /* (310) wqitem ::= withnm eidlist_opt wqas LP select RP */
   308,  /* (311) withnm ::= nm */
   242,  /* (312) wqlist ::= wqitem */
   242,  /* (313) wqlist ::= wqlist COMMA wqitem */
   309,  /* (314) windowdefn_list ::= windowdefn_list COMMA windowdefn */
   310,  /* (315) windowdefn ::= nm AS LP window RP */
   311,  /* (316) window ::= PARTITION BY nexprlist orderby_opt frame_opt */
   311,  /* (317) window ::= nm PARTITION BY nexprlist orderby_opt frame_opt */
   311,  /* (318) window ::= ORDER BY sortlist frame_opt */
   311,  /* (319) window ::= nm ORDER BY sortlist frame_opt */
   311,  /* (320) window ::= nm frame_opt */
   312,  /* (321) frame_opt ::= */
   312,  /* (322) frame_opt ::= range_or_rows frame_bound_s frame_exclude_opt */
   312,  /* (323) frame_opt ::= range_or_rows BETWEEN frame_bound_s AND frame_bound_e frame_exclude_opt */
   316,  /* (324) range_or_rows ::= RANGE|ROWS|GROUPS */
   318,  /* (325) frame_bound_s ::= frame_bound */
   318,  /* (326) frame_bound_s ::= UNBOUNDED PRECEDING */
   319,  /* (327) frame_bound_e ::= frame_bound */
   319,  /* (328) frame_bound_e ::= UNBOUNDED FOLLOWING */
   317,  /* (329) frame_bound ::= expr PRECEDING|FOLLOWING */
   317,  /* (330) frame_bound ::= CURRENT ROW */
   320,  /* (331) frame_exclude_opt ::= */
   320,  /* (332) frame_exclude_opt ::= EXCLUDE frame_exclude */
   321,  /* (333) frame_exclude ::= NO OTHERS */
   321,  /* (334) frame_exclude ::= CURRENT ROW */
   321,  /* (335) frame_exclude ::= GROUP|TIES */
   252,  /* (336) window_clause ::= WINDOW windowdefn_list */
   275,  /* (337) filter_over ::= filter_clause over_clause */
   275,  /* (338) filter_over ::= over_clause */
   275,  /* (339) filter_over ::= filter_clause */
   315,  /* (340) over_clause ::= OVER LP window RP */
   315,  /* (341) over_clause ::= OVER nm */
   314,  /* (342) filter_clause ::= FILTER LP WHERE expr RP */
   217,  /* (343) term ::= QNUMBER */
   186,  /* (344) input ::= cmdlist */
   187,  /* (345) cmdlist ::= cmdlist ecmd */
   187,  /* (346) cmdlist ::= ecmd */
   188,  /* (347) ecmd ::= SEMI */
   188,  /* (348) ecmd ::= cmdx SEMI */
   188,  /* (349) ecmd ::= explain cmdx SEMI */
   193,  /* (350) trans_opt ::= */
   193,  /* (351) trans_opt ::= TRANSACTION */
   193,  /* (352) trans_opt ::= TRANSACTION nm */
   195,  /* (353) savepoint_opt ::= SAVEPOINT */
   195,  /* (354) savepoint_opt ::= */
   191,  /* (355) cmd ::= create_table create_table_args */
   204,  /* (356) table_option_set ::= table_option */
   202,  /* (357) columnlist ::= columnlist COMMA columnname carglist */
   202,  /* (358) columnlist ::= columnname carglist */
   194,  /* (359) nm ::= ID|INDEXED|JOIN_KW */
   194,  /* (360) nm ::= STRING */
   209,  /* (361) typetoken ::= typename */
   210,  /* (362) typename ::= ID|STRING */
   211,  /* (363) signed ::= plus_num */
   211,  /* (364) signed ::= minus_num */
   208,  /* (365) carglist ::= carglist ccons */
   208,  /* (366) carglist ::= */
   216,  /* (367) ccons ::= NULL onconf */
   216,  /* (368) ccons ::= GENERATED ALWAYS AS generated */
   216,  /* (369) ccons ::= AS generated */
   203,  /* (370) conslist_opt ::= COMMA conslist */
   229,  /* (371) conslist ::= conslist tconscomma tcons */
   229,  /* (372) conslist ::= tcons */
   230,  /* (373) tconscomma ::= */
   234,  /* (374) defer_subclause_opt ::= defer_subclause */
   236,  /* (375) resolvetype ::= raisetype */
   240,  /* (376) selectnowith ::= oneselect */
   241,  /* (377) oneselect ::= values */
   256,  /* (378) sclp ::= selcollist COMMA */
   257,  /* (379) as ::= ID|STRING */
   266,  /* (380) indexed_opt ::= indexed_by */
   274,  /* (381) returning ::= */
   218,  /* (382) expr ::= term */
   276,  /* (383) likeop ::= LIKE_KW|MATCH */
   280,  /* (384) case_operand ::= expr */
   263,  /* (385) exprlist ::= nexprlist */
   286,  /* (386) nmnum ::= plus_num */
   286,  /* (387) nmnum ::= nm */
   286,  /* (388) nmnum ::= ON */
   286,  /* (389) nmnum ::= DELETE */
   286,  /* (390) nmnum ::= DEFAULT */
   212,  /* (391) plus_num ::= INTEGER|FLOAT */
   291,  /* (392) foreach_clause ::= */
   291,  /* (393) foreach_clause ::= FOR EACH ROW */
   294,  /* (394) trnm ::= nm */
   295,  /* (395) tridxby ::= */
   296,  /* (396) database_kw_opt ::= DATABASE */
   296,  /* (397) database_kw_opt ::= */
   299,  /* (398) kwcolumn_opt ::= */
   299,  /* (399) kwcolumn_opt ::= COLUMNKW */
   301,  /* (400) vtabarglist ::= vtabarg */
   301,  /* (401) vtabarglist ::= vtabarglist COMMA vtabarg */
   302,  /* (402) vtabarg ::= vtabarg vtabargtoken */
   305,  /* (403) anylist ::= */
   305,  /* (404) anylist ::= anylist LP anylist RP */
   305,  /* (405) anylist ::= anylist ANY */
   268,  /* (406) with ::= */
   309,  /* (407) windowdefn_list ::= windowdefn */
   311,  /* (408) window ::= frame_opt */
};
