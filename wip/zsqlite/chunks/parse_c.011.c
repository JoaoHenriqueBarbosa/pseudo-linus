        break;
      case 229: /* case_exprlist ::= case_exprlist WHEN expr THEN expr */
{
  yymsp[-4].minor.yy14 = sqlite3ExprListAppend(pParse,yymsp[-4].minor.yy14, yymsp[-2].minor.yy454);
  yymsp[-4].minor.yy14 = sqlite3ExprListAppend(pParse,yymsp[-4].minor.yy14, yymsp[0].minor.yy454);
}
        break;
      case 230: /* case_exprlist ::= WHEN expr THEN expr */
{
  yymsp[-3].minor.yy14 = sqlite3ExprListAppend(pParse,0, yymsp[-2].minor.yy454);
  yymsp[-3].minor.yy14 = sqlite3ExprListAppend(pParse,yymsp[-3].minor.yy14, yymsp[0].minor.yy454);
}
        break;
      case 235: /* nexprlist ::= nexprlist COMMA expr */
{yymsp[-2].minor.yy14 = sqlite3ExprListAppend(pParse,yymsp[-2].minor.yy14,yymsp[0].minor.yy454);}
        break;
      case 236: /* nexprlist ::= expr */
{yymsp[0].minor.yy14 = sqlite3ExprListAppend(pParse,0,yymsp[0].minor.yy454); /*A-overwrites-Y*/}
        break;
      case 238: /* paren_exprlist ::= LP exprlist RP */
      case 243: /* eidlist_opt ::= LP eidlist RP */ yytestcase(yyruleno==243);
{yymsp[-2].minor.yy14 = yymsp[-1].minor.yy14;}
        break;
      case 239: /* cmd ::= createkw uniqueflag INDEX ifnotexists nm dbnm ON nm LP sortlist RP where_opt */
{
  sqlite3CreateIndex(pParse, &yymsp[-7].minor.yy0, &yymsp[-6].minor.yy0,
                     sqlite3SrcListAppend(pParse,0,&yymsp[-4].minor.yy0,0), yymsp[-2].minor.yy14, yymsp[-10].minor.yy144,
                      &yymsp[-11].minor.yy0, yymsp[0].minor.yy454, SQLITE_SO_ASC, yymsp[-8].minor.yy144, SQLITE_IDXTYPE_APPDEF);
  if( IN_RENAME_OBJECT && pParse->pNewIndex ){
    sqlite3RenameTokenMap(pParse, pParse->pNewIndex->zName, &yymsp[-4].minor.yy0);
  }
}
        break;
      case 240: /* uniqueflag ::= UNIQUE */
      case 282: /* raisetype ::= ABORT */ yytestcase(yyruleno==282);
{yymsp[0].minor.yy144 = OE_Abort;}
        break;
      case 241: /* uniqueflag ::= */
{yymsp[1].minor.yy144 = OE_None;}
        break;
      case 244: /* eidlist ::= eidlist COMMA nm collate sortorder */
{
  yymsp[-4].minor.yy14 = parserAddExprIdListTerm(pParse, yymsp[-4].minor.yy14, &yymsp[-2].minor.yy0, yymsp[-1].minor.yy144, yymsp[0].minor.yy144);
}
        break;
      case 245: /* eidlist ::= nm collate sortorder */
{
  yymsp[-2].minor.yy14 = parserAddExprIdListTerm(pParse, 0, &yymsp[-2].minor.yy0, yymsp[-1].minor.yy144, yymsp[0].minor.yy144); /*A-overwrites-Y*/
}
        break;
      case 248: /* cmd ::= DROP INDEX ifexists fullname */
{sqlite3DropIndex(pParse, yymsp[0].minor.yy203, yymsp[-1].minor.yy144);}
        break;
      case 249: /* cmd ::= VACUUM vinto */
{sqlite3Vacuum(pParse,0,yymsp[0].minor.yy454);}
        break;
      case 250: /* cmd ::= VACUUM nm vinto */
{sqlite3Vacuum(pParse,&yymsp[-1].minor.yy0,yymsp[0].minor.yy454);}
        break;
      case 253: /* cmd ::= PRAGMA nm dbnm */
{sqlite3Pragma(pParse,&yymsp[-1].minor.yy0,&yymsp[0].minor.yy0,0,0);}
        break;
      case 254: /* cmd ::= PRAGMA nm dbnm EQ nmnum */
{sqlite3Pragma(pParse,&yymsp[-3].minor.yy0,&yymsp[-2].minor.yy0,&yymsp[0].minor.yy0,0);}
        break;
      case 255: /* cmd ::= PRAGMA nm dbnm LP nmnum RP */
{sqlite3Pragma(pParse,&yymsp[-4].minor.yy0,&yymsp[-3].minor.yy0,&yymsp[-1].minor.yy0,0);}
        break;
      case 256: /* cmd ::= PRAGMA nm dbnm EQ minus_num */
{sqlite3Pragma(pParse,&yymsp[-3].minor.yy0,&yymsp[-2].minor.yy0,&yymsp[0].minor.yy0,1);}
        break;
      case 257: /* cmd ::= PRAGMA nm dbnm LP minus_num RP */
{sqlite3Pragma(pParse,&yymsp[-4].minor.yy0,&yymsp[-3].minor.yy0,&yymsp[-1].minor.yy0,1);}
        break;
      case 260: /* cmd ::= createkw trigger_decl BEGIN trigger_cmd_list END */
{
  Token all;
  all.z = yymsp[-3].minor.yy0.z;
  all.n = (int)(yymsp[0].minor.yy0.z - yymsp[-3].minor.yy0.z) + yymsp[0].minor.yy0.n;
  sqlite3FinishTrigger(pParse, yymsp[-1].minor.yy427, &all);
}
        break;
      case 261: /* trigger_decl ::= temp TRIGGER ifnotexists nm dbnm trigger_time trigger_event ON fullname foreach_clause when_clause */
{
  sqlite3BeginTrigger(pParse, &yymsp[-7].minor.yy0, &yymsp[-6].minor.yy0, yymsp[-5].minor.yy144, yymsp[-4].minor.yy286.a, yymsp[-4].minor.yy286.b, yymsp[-2].minor.yy203, yymsp[0].minor.yy454, yymsp[-10].minor.yy144, yymsp[-8].minor.yy144);
  yymsp[-10].minor.yy0 = (yymsp[-6].minor.yy0.n==0?yymsp[-7].minor.yy0:yymsp[-6].minor.yy0); /*A-overwrites-T*/
}
        break;
      case 262: /* trigger_time ::= BEFORE|AFTER */
{ yymsp[0].minor.yy144 = yymsp[0].major; /*A-overwrites-X*/ }
        break;
      case 263: /* trigger_time ::= INSTEAD OF */
{ yymsp[-1].minor.yy144 = TK_INSTEAD;}
        break;
      case 264: /* trigger_time ::= */
{ yymsp[1].minor.yy144 = TK_BEFORE; }
        break;
      case 265: /* trigger_event ::= DELETE|INSERT */
      case 266: /* trigger_event ::= UPDATE */ yytestcase(yyruleno==266);
{yymsp[0].minor.yy286.a = yymsp[0].major; /*A-overwrites-X*/ yymsp[0].minor.yy286.b = 0;}
        break;
      case 267: /* trigger_event ::= UPDATE OF idlist */
{yymsp[-2].minor.yy286.a = TK_UPDATE; yymsp[-2].minor.yy286.b = yymsp[0].minor.yy132;}
        break;
      case 268: /* when_clause ::= */
      case 287: /* key_opt ::= */ yytestcase(yyruleno==287);
{ yymsp[1].minor.yy454 = 0; }
        break;
      case 269: /* when_clause ::= WHEN expr */
      case 288: /* key_opt ::= KEY expr */ yytestcase(yyruleno==288);
{ yymsp[-1].minor.yy454 = yymsp[0].minor.yy454; }
        break;
      case 270: /* trigger_cmd_list ::= trigger_cmd_list trigger_cmd SEMI */
{
  assert( yymsp[-2].minor.yy427!=0 );
  yymsp[-2].minor.yy427->pLast->pNext = yymsp[-1].minor.yy427;
  yymsp[-2].minor.yy427->pLast = yymsp[-1].minor.yy427;
}
        break;
      case 271: /* trigger_cmd_list ::= trigger_cmd SEMI */
{
  assert( yymsp[-1].minor.yy427!=0 );
  yymsp[-1].minor.yy427->pLast = yymsp[-1].minor.yy427;
}
        break;
      case 272: /* trnm ::= nm DOT nm */
{
  yymsp[-2].minor.yy0 = yymsp[0].minor.yy0;
  sqlite3ErrorMsg(pParse,
        "qualified table names are not allowed on INSERT, UPDATE, and DELETE "
        "statements within triggers");
}
        break;
      case 273: /* tridxby ::= INDEXED BY nm */
{
  sqlite3ErrorMsg(pParse,
        "the INDEXED BY clause is not allowed on UPDATE or DELETE statements "
        "within triggers");
}
        break;
      case 274: /* tridxby ::= NOT INDEXED */
{
  sqlite3ErrorMsg(pParse,
        "the NOT INDEXED clause is not allowed on UPDATE or DELETE statements "
        "within triggers");
}
        break;
      case 275: /* trigger_cmd ::= UPDATE orconf trnm tridxby SET setlist from where_opt scanpt */
{yylhsminor.yy427 = sqlite3TriggerUpdateStep(pParse, &yymsp[-6].minor.yy0, yymsp[-2].minor.yy203, yymsp[-3].minor.yy14, yymsp[-1].minor.yy454, yymsp[-7].minor.yy144, yymsp[-8].minor.yy0.z, yymsp[0].minor.yy168);}
  yymsp[-8].minor.yy427 = yylhsminor.yy427;
        break;
      case 276: /* trigger_cmd ::= scanpt insert_cmd INTO trnm idlist_opt select upsert scanpt */
{
   yylhsminor.yy427 = sqlite3TriggerInsertStep(pParse,&yymsp[-4].minor.yy0,yymsp[-3].minor.yy132,yymsp[-2].minor.yy555,yymsp[-6].minor.yy144,yymsp[-1].minor.yy122,yymsp[-7].minor.yy168,yymsp[0].minor.yy168);/*yylhsminor.yy427-overwrites-yymsp[-6].minor.yy144*/
}
  yymsp[-7].minor.yy427 = yylhsminor.yy427;
        break;
      case 277: /* trigger_cmd ::= DELETE FROM trnm tridxby where_opt scanpt */
{yylhsminor.yy427 = sqlite3TriggerDeleteStep(pParse, &yymsp[-3].minor.yy0, yymsp[-1].minor.yy454, yymsp[-5].minor.yy0.z, yymsp[0].minor.yy168);}
  yymsp[-5].minor.yy427 = yylhsminor.yy427;
        break;
      case 278: /* trigger_cmd ::= scanpt select scanpt */
{yylhsminor.yy427 = sqlite3TriggerSelectStep(pParse->db, yymsp[-1].minor.yy555, yymsp[-2].minor.yy168, yymsp[0].minor.yy168); /*yylhsminor.yy427-overwrites-yymsp[-1].minor.yy555*/}
  yymsp[-2].minor.yy427 = yylhsminor.yy427;
        break;
      case 279: /* expr ::= RAISE LP IGNORE RP */
{
  yymsp[-3].minor.yy454 = sqlite3PExpr(pParse, TK_RAISE, 0, 0);
  if( yymsp[-3].minor.yy454 ){
    yymsp[-3].minor.yy454->affExpr = OE_Ignore;
  }
}
        break;
      case 280: /* expr ::= RAISE LP raisetype COMMA nm RP */
{
  yymsp[-5].minor.yy454 = sqlite3ExprAlloc(pParse->db, TK_RAISE, &yymsp[-1].minor.yy0, 1);
  if( yymsp[-5].minor.yy454 ) {
    yymsp[-5].minor.yy454->affExpr = (char)yymsp[-3].minor.yy144;
  }
}
        break;
      case 281: /* raisetype ::= ROLLBACK */
{yymsp[0].minor.yy144 = OE_Rollback;}
        break;
      case 283: /* raisetype ::= FAIL */
{yymsp[0].minor.yy144 = OE_Fail;}
        break;
      case 284: /* cmd ::= DROP TRIGGER ifexists fullname */
{
  sqlite3DropTrigger(pParse,yymsp[0].minor.yy203,yymsp[-1].minor.yy144);
}
        break;
      case 285: /* cmd ::= ATTACH database_kw_opt expr AS expr key_opt */
{
  sqlite3Attach(pParse, yymsp[-3].minor.yy454, yymsp[-1].minor.yy454, yymsp[0].minor.yy454);
}
        break;
      case 286: /* cmd ::= DETACH database_kw_opt expr */
{
  sqlite3Detach(pParse, yymsp[0].minor.yy454);
}
        break;
      case 289: /* cmd ::= REINDEX */
{sqlite3Reindex(pParse, 0, 0);}
        break;
      case 290: /* cmd ::= REINDEX nm dbnm */
{sqlite3Reindex(pParse, &yymsp[-1].minor.yy0, &yymsp[0].minor.yy0);}
        break;
      case 291: /* cmd ::= ANALYZE */
{sqlite3Analyze(pParse, 0, 0);}
        break;
      case 292: /* cmd ::= ANALYZE nm dbnm */
{sqlite3Analyze(pParse, &yymsp[-1].minor.yy0, &yymsp[0].minor.yy0);}
        break;
      case 293: /* cmd ::= ALTER TABLE fullname RENAME TO nm */
{
  sqlite3AlterRenameTable(pParse,yymsp[-3].minor.yy203,&yymsp[0].minor.yy0);
}
        break;
      case 294: /* cmd ::= ALTER TABLE add_column_fullname ADD kwcolumn_opt columnname carglist */
{
  yymsp[-1].minor.yy0.n = (int)(pParse->sLastToken.z-yymsp[-1].minor.yy0.z) + pParse->sLastToken.n;
  sqlite3AlterFinishAddColumn(pParse, &yymsp[-1].minor.yy0);
}
        break;
      case 295: /* cmd ::= ALTER TABLE fullname DROP kwcolumn_opt nm */
{
  sqlite3AlterDropColumn(pParse, yymsp[-3].minor.yy203, &yymsp[0].minor.yy0);
}
        break;
      case 296: /* add_column_fullname ::= fullname */
{
  disableLookaside(pParse);
  sqlite3AlterBeginAddColumn(pParse, yymsp[0].minor.yy203);
}
        break;
      case 297: /* cmd ::= ALTER TABLE fullname RENAME kwcolumn_opt nm TO nm */
{
  sqlite3AlterRenameColumn(pParse, yymsp[-5].minor.yy203, &yymsp[-2].minor.yy0, &yymsp[0].minor.yy0);
}
        break;
      case 298: /* cmd ::= create_vtab */
{sqlite3VtabFinishParse(pParse,0);}
        break;
      case 299: /* cmd ::= create_vtab LP vtabarglist RP */
{sqlite3VtabFinishParse(pParse,&yymsp[0].minor.yy0);}
        break;
      case 300: /* create_vtab ::= createkw VIRTUAL TABLE ifnotexists nm dbnm USING nm */
{
    sqlite3VtabBeginParse(pParse, &yymsp[-3].minor.yy0, &yymsp[-2].minor.yy0, &yymsp[0].minor.yy0, yymsp[-4].minor.yy144);
}
        break;
      case 301: /* vtabarg ::= */
{sqlite3VtabArgInit(pParse);}
        break;
      case 302: /* vtabargtoken ::= ANY */
      case 303: /* vtabargtoken ::= lp anylist RP */ yytestcase(yyruleno==303);
      case 304: /* lp ::= LP */ yytestcase(yyruleno==304);
{sqlite3VtabArgExtend(pParse,&yymsp[0].minor.yy0);}
        break;
      case 305: /* with ::= WITH wqlist */
      case 306: /* with ::= WITH RECURSIVE wqlist */ yytestcase(yyruleno==306);
{ sqlite3WithPush(pParse, yymsp[0].minor.yy59, 1); }
        break;
      case 307: /* wqas ::= AS */
{yymsp[0].minor.yy462 = M10d_Any;}
        break;
      case 308: /* wqas ::= AS MATERIALIZED */
{yymsp[-1].minor.yy462 = M10d_Yes;}
        break;
      case 309: /* wqas ::= AS NOT MATERIALIZED */
{yymsp[-2].minor.yy462 = M10d_No;}
        break;
      case 310: /* wqitem ::= withnm eidlist_opt wqas LP select RP */
{
  yymsp[-5].minor.yy67 = sqlite3CteNew(pParse, &yymsp[-5].minor.yy0, yymsp[-4].minor.yy14, yymsp[-1].minor.yy555, yymsp[-3].minor.yy462); /*A-overwrites-X*/
}
        break;
      case 311: /* withnm ::= nm */
{pParse->bHasWith = 1;}
        break;
      case 312: /* wqlist ::= wqitem */
{
  yymsp[0].minor.yy59 = sqlite3WithAdd(pParse, 0, yymsp[0].minor.yy67); /*A-overwrites-X*/
}
        break;
      case 313: /* wqlist ::= wqlist COMMA wqitem */
{
  yymsp[-2].minor.yy59 = sqlite3WithAdd(pParse, yymsp[-2].minor.yy59, yymsp[0].minor.yy67);
}
        break;
      case 314: /* windowdefn_list ::= windowdefn_list COMMA windowdefn */
{
  assert( yymsp[0].minor.yy211!=0 );
  sqlite3WindowChain(pParse, yymsp[0].minor.yy211, yymsp[-2].minor.yy211);
  yymsp[0].minor.yy211->pNextWin = yymsp[-2].minor.yy211;
  yylhsminor.yy211 = yymsp[0].minor.yy211;
}
  yymsp[-2].minor.yy211 = yylhsminor.yy211;
        break;
      case 315: /* windowdefn ::= nm AS LP window RP */
{
  if( ALWAYS(yymsp[-1].minor.yy211) ){
    yymsp[-1].minor.yy211->zName = sqlite3DbStrNDup(pParse->db, yymsp[-4].minor.yy0.z, yymsp[-4].minor.yy0.n);
  }
  yylhsminor.yy211 = yymsp[-1].minor.yy211;
}
  yymsp[-4].minor.yy211 = yylhsminor.yy211;
        break;
      case 316: /* window ::= PARTITION BY nexprlist orderby_opt frame_opt */
{
  yymsp[-4].minor.yy211 = sqlite3WindowAssemble(pParse, yymsp[0].minor.yy211, yymsp[-2].minor.yy14, yymsp[-1].minor.yy14, 0);
}
        break;
      case 317: /* window ::= nm PARTITION BY nexprlist orderby_opt frame_opt */
{
  yylhsminor.yy211 = sqlite3WindowAssemble(pParse, yymsp[0].minor.yy211, yymsp[-2].minor.yy14, yymsp[-1].minor.yy14, &yymsp[-5].minor.yy0);
}
  yymsp[-5].minor.yy211 = yylhsminor.yy211;
        break;
      case 318: /* window ::= ORDER BY sortlist frame_opt */
{
  yymsp[-3].minor.yy211 = sqlite3WindowAssemble(pParse, yymsp[0].minor.yy211, 0, yymsp[-1].minor.yy14, 0);
}
        break;
      case 319: /* window ::= nm ORDER BY sortlist frame_opt */
{
  yylhsminor.yy211 = sqlite3WindowAssemble(pParse, yymsp[0].minor.yy211, 0, yymsp[-1].minor.yy14, &yymsp[-4].minor.yy0);
}
  yymsp[-4].minor.yy211 = yylhsminor.yy211;
        break;
      case 320: /* window ::= nm frame_opt */
{
  yylhsminor.yy211 = sqlite3WindowAssemble(pParse, yymsp[0].minor.yy211, 0, 0, &yymsp[-1].minor.yy0);
}
  yymsp[-1].minor.yy211 = yylhsminor.yy211;
        break;
      case 321: /* frame_opt ::= */
{
  yymsp[1].minor.yy211 = sqlite3WindowAlloc(pParse, 0, TK_UNBOUNDED, 0, TK_CURRENT, 0, 0);
}
        break;
      case 322: /* frame_opt ::= range_or_rows frame_bound_s frame_exclude_opt */
{
  yylhsminor.yy211 = sqlite3WindowAlloc(pParse, yymsp[-2].minor.yy144, yymsp[-1].minor.yy509.eType, yymsp[-1].minor.yy509.pExpr, TK_CURRENT, 0, yymsp[0].minor.yy462);
}
  yymsp[-2].minor.yy211 = yylhsminor.yy211;
        break;
      case 323: /* frame_opt ::= range_or_rows BETWEEN frame_bound_s AND frame_bound_e frame_exclude_opt */
{
  yylhsminor.yy211 = sqlite3WindowAlloc(pParse, yymsp[-5].minor.yy144, yymsp[-3].minor.yy509.eType, yymsp[-3].minor.yy509.pExpr, yymsp[-1].minor.yy509.eType, yymsp[-1].minor.yy509.pExpr, yymsp[0].minor.yy462);
}
