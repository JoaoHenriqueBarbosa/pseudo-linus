
/*
** Clear information from a Cte object, but do not deallocate storage
** for the object itself.
*/
static void cteClear(sqlite3 *db, Cte *pCte){
  assert( pCte!=0 );
  sqlite3ExprListDelete(db, pCte->pCols);
  sqlite3SelectDelete(db, pCte->pSelect);
  sqlite3DbFree(db, pCte->zName);
}

/*
** Free the contents of the CTE object passed as the second argument.
*/
SQLITE_PRIVATE void sqlite3CteDelete(sqlite3 *db, Cte *pCte){
  assert( pCte!=0 );
  cteClear(db, pCte);
  sqlite3DbFree(db, pCte);
}

/*
** This routine is invoked once per CTE by the parser while parsing a
** WITH clause.  The CTE described by the third argument is added to
** the WITH clause of the second argument.  If the second argument is
** NULL, then a new WITH argument is created.
*/
SQLITE_PRIVATE With *sqlite3WithAdd(
  Parse *pParse,          /* Parsing context */
  With *pWith,            /* Existing WITH clause, or NULL */
  Cte *pCte               /* CTE to add to the WITH clause */
){
  sqlite3 *db = pParse->db;
  With *pNew;
  char *zName;

  if( pCte==0 ){
    return pWith;
  }

  /* Check that the CTE name is unique within this WITH clause. If
  ** not, store an error in the Parse structure. */
  zName = pCte->zName;
  if( zName && pWith ){
    int i;
    for(i=0; i<pWith->nCte; i++){
      if( sqlite3StrICmp(zName, pWith->a[i].zName)==0 ){
        sqlite3ErrorMsg(pParse, "duplicate WITH table name: %s", zName);
      }
    }
  }

  if( pWith ){
    sqlite3_int64 nByte = sizeof(*pWith) + (sizeof(pWith->a[1]) * pWith->nCte);
    pNew = sqlite3DbRealloc(db, pWith, nByte);
  }else{
    pNew = sqlite3DbMallocZero(db, sizeof(*pWith));
  }
  assert( (pNew!=0 && zName!=0) || db->mallocFailed );

  if( db->mallocFailed ){
    sqlite3CteDelete(db, pCte);
    pNew = pWith;
  }else{
    pNew->a[pNew->nCte++] = *pCte;
    sqlite3DbFree(db, pCte);
  }

  return pNew;
}

/*
** Free the contents of the With object passed as the second argument.
*/
SQLITE_PRIVATE void sqlite3WithDelete(sqlite3 *db, With *pWith){
  if( pWith ){
    int i;
    for(i=0; i<pWith->nCte; i++){
      cteClear(db, &pWith->a[i]);
    }
    sqlite3DbFree(db, pWith);
  }
}
SQLITE_PRIVATE void sqlite3WithDeleteGeneric(sqlite3 *db, void *pWith){
  sqlite3WithDelete(db, (With*)pWith);
}
#endif /* !defined(SQLITE_OMIT_CTE) */

