
/*
** Process statements of the form:
**
**    INSERT INTO table(table) VALUES('merge=A,B');
**
** A and B are integers that decode to be the number of leaf pages
** written for the merge, and the minimum number of segments on a level
** before it will be selected for a merge, respectively.
*/
static int fts3DoIncrmerge(
  Fts3Table *p,                   /* FTS3 table handle */
  const char *zParam              /* Nul-terminated string containing "A,B" */
){
  int rc;
  int nMin = (MergeCount(p) / 2);
  int nMerge = 0;
  const char *z = zParam;

  /* Read the first integer value */
  nMerge = fts3Getint(&z);

  /* If the first integer value is followed by a ',',  read the second
  ** integer value. */
  if( z[0]==',' && z[1]!='\0' ){
    z++;
    nMin = fts3Getint(&z);
  }

  if( z[0]!='\0' || nMin<2 ){
    rc = SQLITE_ERROR;
  }else{
    rc = SQLITE_OK;
    if( !p->bHasStat ){
      assert( p->bFts4==0 );
      sqlite3Fts3CreateStatTable(&rc, p);
    }
    if( rc==SQLITE_OK ){
      rc = sqlite3Fts3Incrmerge(p, nMerge, nMin);
    }
    sqlite3Fts3SegmentsClose(p);
  }
  return rc;
}

/*
** Process statements of the form:
**
**    INSERT INTO table(table) VALUES('automerge=X');
**
** where X is an integer.  X==0 means to turn automerge off.  X!=0 means
** turn it on.  The setting is persistent.
*/
static int fts3DoAutoincrmerge(
  Fts3Table *p,                   /* FTS3 table handle */
  const char *zParam              /* Nul-terminated string containing boolean */
){
  int rc = SQLITE_OK;
  sqlite3_stmt *pStmt = 0;
  p->nAutoincrmerge = fts3Getint(&zParam);
  if( p->nAutoincrmerge==1 || p->nAutoincrmerge>MergeCount(p) ){
    p->nAutoincrmerge = 8;
  }
  if( !p->bHasStat ){
    assert( p->bFts4==0 );
    sqlite3Fts3CreateStatTable(&rc, p);
    if( rc ) return rc;
  }
  rc = fts3SqlStmt(p, SQL_REPLACE_STAT, &pStmt, 0);
  if( rc ) return rc;
  sqlite3_bind_int(pStmt, 1, FTS_STAT_AUTOINCRMERGE);
  sqlite3_bind_int(pStmt, 2, p->nAutoincrmerge);
  sqlite3_step(pStmt);
  rc = sqlite3_reset(pStmt);
  return rc;
}

/*
** Return a 64-bit checksum for the FTS index entry specified by the
** arguments to this function.
*/
static u64 fts3ChecksumEntry(
  const char *zTerm,              /* Pointer to buffer containing term */
  int nTerm,                      /* Size of zTerm in bytes */
  int iLangid,                    /* Language id for current row */
  int iIndex,                     /* Index (0..Fts3Table.nIndex-1) */
  i64 iDocid,                     /* Docid for current row. */
  int iCol,                       /* Column number */
  int iPos                        /* Position */
){
  int i;
  u64 ret = (u64)iDocid;

  ret += (ret<<3) + iLangid;
  ret += (ret<<3) + iIndex;
  ret += (ret<<3) + iCol;
  ret += (ret<<3) + iPos;
  for(i=0; i<nTerm; i++) ret += (ret<<3) + zTerm[i];

  return ret;
}

/*
** Return a checksum of all entries in the FTS index that correspond to
** language id iLangid. The checksum is calculated by XORing the checksums
** of each individual entry (see fts3ChecksumEntry()) together.
**
** If successful, the checksum value is returned and *pRc set to SQLITE_OK.
** Otherwise, if an error occurs, *pRc is set to an SQLite error code. The
** return value is undefined in this case.
*/
static u64 fts3ChecksumIndex(
  Fts3Table *p,                   /* FTS3 table handle */
  int iLangid,                    /* Language id to return cksum for */
  int iIndex,                     /* Index to cksum (0..p->nIndex-1) */
  int *pRc                        /* OUT: Return code */
){
  Fts3SegFilter filter;
  Fts3MultiSegReader csr;
  int rc;
  u64 cksum = 0;

  if( *pRc ) return 0;

  memset(&filter, 0, sizeof(filter));
  memset(&csr, 0, sizeof(csr));
  filter.flags =  FTS3_SEGMENT_REQUIRE_POS|FTS3_SEGMENT_IGNORE_EMPTY;
  filter.flags |= FTS3_SEGMENT_SCAN;

  rc = sqlite3Fts3SegReaderCursor(
      p, iLangid, iIndex, FTS3_SEGCURSOR_ALL, 0, 0, 0, 1,&csr
  );
  if( rc==SQLITE_OK ){
    rc = sqlite3Fts3SegReaderStart(p, &csr, &filter);
  }

  if( rc==SQLITE_OK ){
    while( SQLITE_ROW==(rc = sqlite3Fts3SegReaderStep(p, &csr)) ){
      char *pCsr = csr.aDoclist;
      char *pEnd = &pCsr[csr.nDoclist];

      i64 iDocid = 0;
      i64 iCol = 0;
      u64 iPos = 0;

      pCsr += sqlite3Fts3GetVarint(pCsr, &iDocid);
      while( pCsr<pEnd ){
        u64 iVal = 0;
        pCsr += sqlite3Fts3GetVarintU(pCsr, &iVal);
        if( pCsr<pEnd ){
          if( iVal==0 || iVal==1 ){
            iCol = 0;
            iPos = 0;
            if( iVal ){
              pCsr += sqlite3Fts3GetVarint(pCsr, &iCol);
            }else{
              pCsr += sqlite3Fts3GetVarintU(pCsr, &iVal);
              if( p->bDescIdx ){
                iDocid = (i64)((u64)iDocid - iVal);
              }else{
                iDocid = (i64)((u64)iDocid + iVal);
              }
            }
          }else{
            iPos += (iVal - 2);
            cksum = cksum ^ fts3ChecksumEntry(
                csr.zTerm, csr.nTerm, iLangid, iIndex, iDocid,
                (int)iCol, (int)iPos
            );
          }
        }
      }
    }
  }
  sqlite3Fts3SegReaderFinish(&csr);

  *pRc = rc;
  return cksum;
}

/*
** Check if the contents of the FTS index match the current contents of the
** content table. If no error occurs and the contents do match, set *pbOk
** to true and return SQLITE_OK. Or if the contents do not match, set *pbOk
** to false before returning.
**
** If an error occurs (e.g. an OOM or IO error), return an SQLite error
** code. The final value of *pbOk is undefined in this case.
*/
SQLITE_PRIVATE int sqlite3Fts3IntegrityCheck(Fts3Table *p, int *pbOk){
  int rc = SQLITE_OK;             /* Return code */
  u64 cksum1 = 0;                 /* Checksum based on FTS index contents */
  u64 cksum2 = 0;                 /* Checksum based on %_content contents */
  sqlite3_stmt *pAllLangid = 0;   /* Statement to return all language-ids */

  /* This block calculates the checksum according to the FTS index. */
  rc = fts3SqlStmt(p, SQL_SELECT_ALL_LANGID, &pAllLangid, 0);
  if( rc==SQLITE_OK ){
    int rc2;
    sqlite3_bind_int(pAllLangid, 1, p->iPrevLangid);
    sqlite3_bind_int(pAllLangid, 2, p->nIndex);
    while( rc==SQLITE_OK && sqlite3_step(pAllLangid)==SQLITE_ROW ){
      int iLangid = sqlite3_column_int(pAllLangid, 0);
      int i;
      for(i=0; i<p->nIndex; i++){
        cksum1 = cksum1 ^ fts3ChecksumIndex(p, iLangid, i, &rc);
      }
    }
    rc2 = sqlite3_reset(pAllLangid);
    if( rc==SQLITE_OK ) rc = rc2;
  }

  /* This block calculates the checksum according to the %_content table */
  if( rc==SQLITE_OK ){
    sqlite3_tokenizer_module const *pModule = p->pTokenizer->pModule;
    sqlite3_stmt *pStmt = 0;
    char *zSql;

    zSql = sqlite3_mprintf("SELECT %s" , p->zReadExprlist);
    if( !zSql ){
      rc = SQLITE_NOMEM;
    }else{
      rc = sqlite3_prepare_v2(p->db, zSql, -1, &pStmt, 0);
      sqlite3_free(zSql);
    }

    while( rc==SQLITE_OK && SQLITE_ROW==sqlite3_step(pStmt) ){
      i64 iDocid = sqlite3_column_int64(pStmt, 0);
      int iLang = langidFromSelect(p, pStmt);
      int iCol;

      for(iCol=0; rc==SQLITE_OK && iCol<p->nColumn; iCol++){
        if( p->abNotindexed[iCol]==0 ){
          const char *zText = (const char *)sqlite3_column_text(pStmt, iCol+1);
          sqlite3_tokenizer_cursor *pT = 0;

          rc = sqlite3Fts3OpenTokenizer(p->pTokenizer, iLang, zText, -1, &pT);
          while( rc==SQLITE_OK ){
            char const *zToken;       /* Buffer containing token */
            int nToken = 0;           /* Number of bytes in token */
            int iDum1 = 0, iDum2 = 0; /* Dummy variables */
            int iPos = 0;             /* Position of token in zText */

            rc = pModule->xNext(pT, &zToken, &nToken, &iDum1, &iDum2, &iPos);
            if( rc==SQLITE_OK ){
              int i;
              cksum2 = cksum2 ^ fts3ChecksumEntry(
                  zToken, nToken, iLang, 0, iDocid, iCol, iPos
              );
              for(i=1; i<p->nIndex; i++){
                if( p->aIndex[i].nPrefix<=nToken ){
                  cksum2 = cksum2 ^ fts3ChecksumEntry(
                      zToken, p->aIndex[i].nPrefix, iLang, i, iDocid, iCol, iPos
                  );
                }
              }
            }
          }
          if( pT ) pModule->xClose(pT);
          if( rc==SQLITE_DONE ) rc = SQLITE_OK;
        }
      }
    }

    sqlite3_finalize(pStmt);
  }

  if( rc==SQLITE_CORRUPT_VTAB ){
    rc = SQLITE_OK;
    *pbOk = 0;
  }else{
    *pbOk = (rc==SQLITE_OK && cksum1==cksum2);
  }
  return rc;
}

/*
** Run the integrity-check. If no error occurs and the current contents of
** the FTS index are correct, return SQLITE_OK. Or, if the contents of the
** FTS index are incorrect, return SQLITE_CORRUPT_VTAB.
**
** Or, if an error (e.g. an OOM or IO error) occurs, return an SQLite
** error code.
**
** The integrity-check works as follows. For each token and indexed token
** prefix in the document set, a 64-bit checksum is calculated (by code
** in fts3ChecksumEntry()) based on the following:
**
**     + The index number (0 for the main index, 1 for the first prefix
**       index etc.),
**     + The token (or token prefix) text itself,
**     + The language-id of the row it appears in,
**     + The docid of the row it appears in,
**     + The column it appears in, and
**     + The tokens position within that column.
**
** The checksums for all entries in the index are XORed together to create
** a single checksum for the entire index.
**
** The integrity-check code calculates the same checksum in two ways:
**
**     1. By scanning the contents of the FTS index, and
**     2. By scanning and tokenizing the content table.
**
** If the two checksums are identical, the integrity-check is deemed to have
** passed.
*/
static int fts3DoIntegrityCheck(
  Fts3Table *p                    /* FTS3 table handle */
){
  int rc;
  int bOk = 0;
  rc = sqlite3Fts3IntegrityCheck(p, &bOk);
  if( rc==SQLITE_OK && bOk==0 ) rc = FTS_CORRUPT_VTAB;
  return rc;
}

/*
** Handle a 'special' INSERT of the form:
**
**   "INSERT INTO tbl(tbl) VALUES(<expr>)"
**
** Argument pVal contains the result of <expr>. Currently the only
** meaningful value to insert is the text 'optimize'.
*/
static int fts3SpecialInsert(Fts3Table *p, sqlite3_value *pVal){
  int rc = SQLITE_ERROR;           /* Return Code */
  const char *zVal = (const char *)sqlite3_value_text(pVal);
  int nVal = sqlite3_value_bytes(pVal);

  if( !zVal ){
    return SQLITE_NOMEM;
  }else if( nVal==8 && 0==sqlite3_strnicmp(zVal, "optimize", 8) ){
    rc = fts3DoOptimize(p, 0);
  }else if( nVal==7 && 0==sqlite3_strnicmp(zVal, "rebuild", 7) ){
    rc = fts3DoRebuild(p);
  }else if( nVal==15 && 0==sqlite3_strnicmp(zVal, "integrity-check", 15) ){
    rc = fts3DoIntegrityCheck(p);
  }else if( nVal>6 && 0==sqlite3_strnicmp(zVal, "merge=", 6) ){
    rc = fts3DoIncrmerge(p, &zVal[6]);
  }else if( nVal>10 && 0==sqlite3_strnicmp(zVal, "automerge=", 10) ){
    rc = fts3DoAutoincrmerge(p, &zVal[10]);
  }else if( nVal==5 && 0==sqlite3_strnicmp(zVal, "flush", 5) ){
    rc = sqlite3Fts3PendingTermsFlush(p);
  }
#if defined(SQLITE_DEBUG) || defined(SQLITE_TEST)
  else{
    int v;
    if( nVal>9 && 0==sqlite3_strnicmp(zVal, "nodesize=", 9) ){
      v = atoi(&zVal[9]);
      if( v>=24 && v<=p->nPgsz-35 ) p->nNodeSize = v;
      rc = SQLITE_OK;
    }else if( nVal>11 && 0==sqlite3_strnicmp(zVal, "maxpending=", 9) ){
      v = atoi(&zVal[11]);
      if( v>=64 && v<=FTS3_MAX_PENDING_DATA ) p->nMaxPendingData = v;
      rc = SQLITE_OK;
    }else if( nVal>21 && 0==sqlite3_strnicmp(zVal,"test-no-incr-doclist=",21) ){
      p->bNoIncrDoclist = atoi(&zVal[21]);
      rc = SQLITE_OK;
    }else if( nVal>11 && 0==sqlite3_strnicmp(zVal,"mergecount=",11) ){
      v = atoi(&zVal[11]);
      if( v>=4 && v<=FTS3_MERGE_COUNT && (v&1)==0 ) p->nMergeCount = v;
      rc = SQLITE_OK;
    }
  }
#endif
  return rc;
}
