
/*
** Called by the parser to compile a DETACH statement.
**
**     DETACH pDbname
*/
SQLITE_PRIVATE void sqlite3Detach(Parse *pParse, Expr *pDbname){
  static const FuncDef detach_func = {
    1,                /* nArg */
    SQLITE_UTF8,      /* funcFlags */
    0,                /* pUserData */
    0,                /* pNext */
    detachFunc,       /* xSFunc */
    0,                /* xFinalize */
    0, 0,             /* xValue, xInverse */
    "sqlite_detach",  /* zName */
    {0}
  };
  codeAttach(pParse, SQLITE_DETACH, &detach_func, pDbname, 0, 0, pDbname);
}

/*
** Called by the parser to compile an ATTACH statement.
**
**     ATTACH p AS pDbname KEY pKey
*/
SQLITE_PRIVATE void sqlite3Attach(Parse *pParse, Expr *p, Expr *pDbname, Expr *pKey){
  static const FuncDef attach_func = {
    3,                /* nArg */
    SQLITE_UTF8,      /* funcFlags */
    0,                /* pUserData */
    0,                /* pNext */
    attachFunc,       /* xSFunc */
    0,                /* xFinalize */
    0, 0,             /* xValue, xInverse */
    "sqlite_attach",  /* zName */
    {0}
  };
  codeAttach(pParse, SQLITE_ATTACH, &attach_func, p, p, pDbname, pKey);
}
#endif /* SQLITE_OMIT_ATTACH */

/*
** Expression callback used by sqlite3FixAAAA() routines.
*/
static int fixExprCb(Walker *p, Expr *pExpr){
  DbFixer *pFix = p->u.pFix;
  if( !pFix->bTemp ) ExprSetProperty(pExpr, EP_FromDDL);
  if( pExpr->op==TK_VARIABLE ){
    if( pFix->pParse->db->init.busy ){
      pExpr->op = TK_NULL;
    }else{
      sqlite3ErrorMsg(pFix->pParse, "%s cannot use variables", pFix->zType);
      return WRC_Abort;
    }
  }
  return WRC_Continue;
}

/*
** Select callback used by sqlite3FixAAAA() routines.
*/
static int fixSelectCb(Walker *p, Select *pSelect){
  DbFixer *pFix = p->u.pFix;
  int i;
  SrcItem *pItem;
  sqlite3 *db = pFix->pParse->db;
  int iDb = sqlite3FindDbName(db, pFix->zDb);
  SrcList *pList = pSelect->pSrc;

  if( NEVER(pList==0) ) return WRC_Continue;
  for(i=0, pItem=pList->a; i<pList->nSrc; i++, pItem++){
    if( pFix->bTemp==0 ){
      if( pItem->zDatabase ){
        if( iDb!=sqlite3FindDbName(db, pItem->zDatabase) ){
          sqlite3ErrorMsg(pFix->pParse,
              "%s %T cannot reference objects in database %s",
              pFix->zType, pFix->pName, pItem->zDatabase);
          return WRC_Abort;
        }
        sqlite3DbFree(db, pItem->zDatabase);
        pItem->zDatabase = 0;
        pItem->fg.notCte = 1;
      }
      pItem->pSchema = pFix->pSchema;
      pItem->fg.fromDDL = 1;
    }
#if !defined(SQLITE_OMIT_VIEW) || !defined(SQLITE_OMIT_TRIGGER)
    if( pList->a[i].fg.isUsing==0
     && sqlite3WalkExpr(&pFix->w, pList->a[i].u3.pOn)
    ){
      return WRC_Abort;
    }
#endif
  }
  if( pSelect->pWith ){
    for(i=0; i<pSelect->pWith->nCte; i++){
      if( sqlite3WalkSelect(p, pSelect->pWith->a[i].pSelect) ){
        return WRC_Abort;
      }
    }
  }
  return WRC_Continue;
}

/*
** Initialize a DbFixer structure.  This routine must be called prior
** to passing the structure to one of the sqliteFixAAAA() routines below.
*/
SQLITE_PRIVATE void sqlite3FixInit(
  DbFixer *pFix,      /* The fixer to be initialized */
  Parse *pParse,      /* Error messages will be written here */
  int iDb,            /* This is the database that must be used */
  const char *zType,  /* "view", "trigger", or "index" */
  const Token *pName  /* Name of the view, trigger, or index */
){
  sqlite3 *db = pParse->db;
  assert( db->nDb>iDb );
  pFix->pParse = pParse;
  pFix->zDb = db->aDb[iDb].zDbSName;
  pFix->pSchema = db->aDb[iDb].pSchema;
  pFix->zType = zType;
  pFix->pName = pName;
  pFix->bTemp = (iDb==1);
  pFix->w.pParse = pParse;
  pFix->w.xExprCallback = fixExprCb;
  pFix->w.xSelectCallback = fixSelectCb;
  pFix->w.xSelectCallback2 = sqlite3WalkWinDefnDummyCallback;
  pFix->w.walkerDepth = 0;
  pFix->w.eCode = 0;
  pFix->w.u.pFix = pFix;
}

/*
** The following set of routines walk through the parse tree and assign
** a specific database to all table references where the database name
** was left unspecified in the original SQL statement.  The pFix structure
** must have been initialized by a prior call to sqlite3FixInit().
**
** These routines are used to make sure that an index, trigger, or
** view in one database does not refer to objects in a different database.
** (Exception: indices, triggers, and views in the TEMP database are
** allowed to refer to anything.)  If a reference is explicitly made
** to an object in a different database, an error message is added to
** pParse->zErrMsg and these routines return non-zero.  If everything
** checks out, these routines return 0.
*/
SQLITE_PRIVATE int sqlite3FixSrcList(
  DbFixer *pFix,       /* Context of the fixation */
  SrcList *pList       /* The Source list to check and modify */
){
  int res = 0;
  if( pList ){
    Select s;
    memset(&s, 0, sizeof(s));
    s.pSrc = pList;
    res = sqlite3WalkSelect(&pFix->w, &s);
  }
  return res;
}
#if !defined(SQLITE_OMIT_VIEW) || !defined(SQLITE_OMIT_TRIGGER)
SQLITE_PRIVATE int sqlite3FixSelect(
  DbFixer *pFix,       /* Context of the fixation */
  Select *pSelect      /* The SELECT statement to be fixed to one database */
){
  return sqlite3WalkSelect(&pFix->w, pSelect);
}
SQLITE_PRIVATE int sqlite3FixExpr(
  DbFixer *pFix,     /* Context of the fixation */
  Expr *pExpr        /* The expression to be fixed to one database */
){
  return sqlite3WalkExpr(&pFix->w, pExpr);
}
#endif

#ifndef SQLITE_OMIT_TRIGGER
SQLITE_PRIVATE int sqlite3FixTriggerStep(
  DbFixer *pFix,     /* Context of the fixation */
  TriggerStep *pStep /* The trigger step be fixed to one database */
){
  while( pStep ){
    if( sqlite3WalkSelect(&pFix->w, pStep->pSelect)
     || sqlite3WalkExpr(&pFix->w, pStep->pWhere)
     || sqlite3WalkExprList(&pFix->w, pStep->pExprList)
     || sqlite3FixSrcList(pFix, pStep->pFrom)
    ){
      return 1;
    }
#ifndef SQLITE_OMIT_UPSERT
    {
      Upsert *pUp;
      for(pUp=pStep->pUpsert; pUp; pUp=pUp->pNextUpsert){
        if( sqlite3WalkExprList(&pFix->w, pUp->pUpsertTarget)
         || sqlite3WalkExpr(&pFix->w, pUp->pUpsertTargetWhere)
         || sqlite3WalkExprList(&pFix->w, pUp->pUpsertSet)
         || sqlite3WalkExpr(&pFix->w, pUp->pUpsertWhere)
        ){
          return 1;
        }
      }
    }
#endif
    pStep = pStep->pNext;
  }

  return 0;
}
#endif

