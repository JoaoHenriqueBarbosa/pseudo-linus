
/*
** This function is the implementation of both the xConnect and xCreate
** methods of the FTS3 virtual table.
**
** The argv[] array contains the following:
**
**   argv[0]   -> module name  ("fts3" or "fts4")
**   argv[1]   -> database name
**   argv[2]   -> table name
**   argv[...] -> "column name" and other module argument fields.
*/
static int fts3InitVtab(
  int isCreate,                   /* True for xCreate, false for xConnect */
  sqlite3 *db,                    /* The SQLite database connection */
  void *pAux,                     /* Hash table containing tokenizers */
  int argc,                       /* Number of elements in argv array */
  const char * const *argv,       /* xCreate/xConnect argument array */
  sqlite3_vtab **ppVTab,          /* Write the resulting vtab structure here */
  char **pzErr                    /* Write any error message here */
){
  Fts3Hash *pHash = &((Fts3HashWrapper*)pAux)->hash;
  Fts3Table *p = 0;               /* Pointer to allocated vtab */
  int rc = SQLITE_OK;             /* Return code */
  int i;                          /* Iterator variable */
  sqlite3_int64 nByte;            /* Size of allocation used for *p */
  int iCol;                       /* Column index */
  int nString = 0;                /* Bytes required to hold all column names */
  int nCol = 0;                   /* Number of columns in the FTS table */
  char *zCsr;                     /* Space for holding column names */
  int nDb;                        /* Bytes required to hold database name */
  int nName;                      /* Bytes required to hold table name */
  int isFts4 = (argv[0][3]=='4'); /* True for FTS4, false for FTS3 */
  const char **aCol;              /* Array of column names */
  sqlite3_tokenizer *pTokenizer = 0;        /* Tokenizer for this table */

  int nIndex = 0;                 /* Size of aIndex[] array */
  struct Fts3Index *aIndex = 0;   /* Array of indexes for this table */

  /* The results of parsing supported FTS4 key=value options: */
  int bNoDocsize = 0;             /* True to omit %_docsize table */
  int bDescIdx = 0;               /* True to store descending indexes */
  char *zPrefix = 0;              /* Prefix parameter value (or NULL) */
  char *zCompress = 0;            /* compress=? parameter (or NULL) */
  char *zUncompress = 0;          /* uncompress=? parameter (or NULL) */
  char *zContent = 0;             /* content=? parameter (or NULL) */
  char *zLanguageid = 0;          /* languageid=? parameter (or NULL) */
  char **azNotindexed = 0;        /* The set of notindexed= columns */
  int nNotindexed = 0;            /* Size of azNotindexed[] array */

  assert( strlen(argv[0])==4 );
  assert( (sqlite3_strnicmp(argv[0], "fts4", 4)==0 && isFts4)
       || (sqlite3_strnicmp(argv[0], "fts3", 4)==0 && !isFts4)
  );

  nDb = (int)strlen(argv[1]) + 1;
  nName = (int)strlen(argv[2]) + 1;

  nByte = sizeof(const char *) * (argc-2);
  aCol = (const char **)sqlite3_malloc64(nByte);
  if( aCol ){
    memset((void*)aCol, 0, nByte);
    azNotindexed = (char **)sqlite3_malloc64(nByte);
  }
  if( azNotindexed ){
    memset(azNotindexed, 0, nByte);
  }
  if( !aCol || !azNotindexed ){
    rc = SQLITE_NOMEM;
    goto fts3_init_out;
  }

  /* Loop through all of the arguments passed by the user to the FTS3/4
  ** module (i.e. all the column names and special arguments). This loop
  ** does the following:
  **
  **   + Figures out the number of columns the FTSX table will have, and
  **     the number of bytes of space that must be allocated to store copies
  **     of the column names.
  **
  **   + If there is a tokenizer specification included in the arguments,
  **     initializes the tokenizer pTokenizer.
  */
  for(i=3; rc==SQLITE_OK && i<argc; i++){
    char const *z = argv[i];
    int nKey;
    char *zVal;

    /* Check if this is a tokenizer specification */
    if( !pTokenizer
     && strlen(z)>8
     && 0==sqlite3_strnicmp(z, "tokenize", 8)
     && 0==sqlite3Fts3IsIdChar(z[8])
    ){
      rc = sqlite3Fts3InitTokenizer(pHash, &z[9], &pTokenizer, pzErr);
    }

    /* Check if it is an FTS4 special argument. */
    else if( isFts4 && fts3IsSpecialColumn(z, &nKey, &zVal) ){
      struct Fts4Option {
        const char *zOpt;
        int nOpt;
      } aFts4Opt[] = {
        { "matchinfo",   9 },     /* 0 -> MATCHINFO */
        { "prefix",      6 },     /* 1 -> PREFIX */
        { "compress",    8 },     /* 2 -> COMPRESS */
        { "uncompress", 10 },     /* 3 -> UNCOMPRESS */
        { "order",       5 },     /* 4 -> ORDER */
        { "content",     7 },     /* 5 -> CONTENT */
        { "languageid", 10 },     /* 6 -> LANGUAGEID */
        { "notindexed", 10 }      /* 7 -> NOTINDEXED */
      };

      int iOpt;
      if( !zVal ){
        rc = SQLITE_NOMEM;
      }else{
        for(iOpt=0; iOpt<SizeofArray(aFts4Opt); iOpt++){
          struct Fts4Option *pOp = &aFts4Opt[iOpt];
          if( nKey==pOp->nOpt && !sqlite3_strnicmp(z, pOp->zOpt, pOp->nOpt) ){
            break;
          }
        }
        switch( iOpt ){
          case 0:               /* MATCHINFO */
            if( strlen(zVal)!=4 || sqlite3_strnicmp(zVal, "fts3", 4) ){
              sqlite3Fts3ErrMsg(pzErr, "unrecognized matchinfo: %s", zVal);
              rc = SQLITE_ERROR;
            }
            bNoDocsize = 1;
            break;

          case 1:               /* PREFIX */
            sqlite3_free(zPrefix);
            zPrefix = zVal;
            zVal = 0;
            break;

          case 2:               /* COMPRESS */
            sqlite3_free(zCompress);
            zCompress = zVal;
            zVal = 0;
            break;

          case 3:               /* UNCOMPRESS */
            sqlite3_free(zUncompress);
            zUncompress = zVal;
            zVal = 0;
            break;

          case 4:               /* ORDER */
            if( (strlen(zVal)!=3 || sqlite3_strnicmp(zVal, "asc", 3))
             && (strlen(zVal)!=4 || sqlite3_strnicmp(zVal, "desc", 4))
            ){
              sqlite3Fts3ErrMsg(pzErr, "unrecognized order: %s", zVal);
              rc = SQLITE_ERROR;
            }
            bDescIdx = (zVal[0]=='d' || zVal[0]=='D');
            break;

          case 5:              /* CONTENT */
            sqlite3_free(zContent);
            zContent = zVal;
            zVal = 0;
            break;

          case 6:              /* LANGUAGEID */
            assert( iOpt==6 );
            sqlite3_free(zLanguageid);
            zLanguageid = zVal;
            zVal = 0;
            break;

          case 7:              /* NOTINDEXED */
            azNotindexed[nNotindexed++] = zVal;
            zVal = 0;
            break;

          default:
            assert( iOpt==SizeofArray(aFts4Opt) );
            sqlite3Fts3ErrMsg(pzErr, "unrecognized parameter: %s", z);
            rc = SQLITE_ERROR;
            break;
        }
        sqlite3_free(zVal);
      }
    }

    /* Otherwise, the argument is a column name. */
    else {
      nString += (int)(strlen(z) + 1);
      aCol[nCol++] = z;
    }
  }

  /* If a content=xxx option was specified, the following:
  **
  **   1. Ignore any compress= and uncompress= options.
  **
  **   2. If no column names were specified as part of the CREATE VIRTUAL
  **      TABLE statement, use all columns from the content table.
  */
  if( rc==SQLITE_OK && zContent ){
    sqlite3_free(zCompress);
    sqlite3_free(zUncompress);
    zCompress = 0;
    zUncompress = 0;
    if( nCol==0 ){
      sqlite3_free((void*)aCol);
      aCol = 0;
      rc = fts3ContentColumns(db, argv[1], zContent,&aCol,&nCol,&nString,pzErr);

      /* If a languageid= option was specified, remove the language id
      ** column from the aCol[] array. */
      if( rc==SQLITE_OK && zLanguageid ){
        int j;
        for(j=0; j<nCol; j++){
          if( sqlite3_stricmp(zLanguageid, aCol[j])==0 ){
            int k;
            for(k=j; k<nCol; k++) aCol[k] = aCol[k+1];
            nCol--;
            break;
          }
        }
      }
    }
  }
  if( rc!=SQLITE_OK ) goto fts3_init_out;

  if( nCol==0 ){
    assert( nString==0 );
    aCol[0] = "content";
    nString = 8;
    nCol = 1;
  }

  if( pTokenizer==0 ){
    rc = sqlite3Fts3InitTokenizer(pHash, "simple", &pTokenizer, pzErr);
    if( rc!=SQLITE_OK ) goto fts3_init_out;
  }
  assert( pTokenizer );

  rc = fts3PrefixParameter(zPrefix, &nIndex, &aIndex);
  if( rc==SQLITE_ERROR ){
    assert( zPrefix );
    sqlite3Fts3ErrMsg(pzErr, "error parsing prefix parameter: %s", zPrefix);
  }
  if( rc!=SQLITE_OK ) goto fts3_init_out;

  /* Allocate and populate the Fts3Table structure. */
  nByte = sizeof(Fts3Table) +                  /* Fts3Table */
          nCol * sizeof(char *) +              /* azColumn */
          nIndex * sizeof(struct Fts3Index) +  /* aIndex */
          nCol * sizeof(u8) +                  /* abNotindexed */
          nName +                              /* zName */
          nDb +                                /* zDb */
          nString;                             /* Space for azColumn strings */
  p = (Fts3Table*)sqlite3_malloc64(nByte);
  if( p==0 ){
    rc = SQLITE_NOMEM;
    goto fts3_init_out;
  }
  memset(p, 0, nByte);
  p->db = db;
  p->nColumn = nCol;
  p->nPendingData = 0;
  p->azColumn = (char **)&p[1];
  p->pTokenizer = pTokenizer;
  p->nMaxPendingData = FTS3_MAX_PENDING_DATA;
  p->bHasDocsize = (isFts4 && bNoDocsize==0);
  p->bHasStat = (u8)isFts4;
  p->bFts4 = (u8)isFts4;
  p->bDescIdx = (u8)bDescIdx;
  p->nAutoincrmerge = 0xff;   /* 0xff means setting unknown */
  p->zContentTbl = zContent;
  p->zLanguageid = zLanguageid;
  zContent = 0;
  zLanguageid = 0;
  TESTONLY( p->inTransaction = -1 );
  TESTONLY( p->mxSavepoint = -1 );

  p->aIndex = (struct Fts3Index *)&p->azColumn[nCol];
  memcpy(p->aIndex, aIndex, sizeof(struct Fts3Index) * nIndex);
  p->nIndex = nIndex;
  for(i=0; i<nIndex; i++){
    fts3HashInit(&p->aIndex[i].hPending, FTS3_HASH_STRING, 1);
  }
  p->abNotindexed = (u8 *)&p->aIndex[nIndex];

  /* Fill in the zName and zDb fields of the vtab structure. */
  zCsr = (char *)&p->abNotindexed[nCol];
  p->zName = zCsr;
  memcpy(zCsr, argv[2], nName);
  zCsr += nName;
  p->zDb = zCsr;
  memcpy(zCsr, argv[1], nDb);
  zCsr += nDb;

  /* Fill in the azColumn array */
  for(iCol=0; iCol<nCol; iCol++){
    char *z;
    int n = 0;
    z = (char *)sqlite3Fts3NextToken(aCol[iCol], &n);
    if( n>0 ){
      memcpy(zCsr, z, n);
    }
    zCsr[n] = '\0';
    sqlite3Fts3Dequote(zCsr);
    p->azColumn[iCol] = zCsr;
    zCsr += n+1;
    assert( zCsr <= &((char *)p)[nByte] );
  }

  /* Fill in the abNotindexed array */
  for(iCol=0; iCol<nCol; iCol++){
    int n = (int)strlen(p->azColumn[iCol]);
    for(i=0; i<nNotindexed; i++){
      char *zNot = azNotindexed[i];
      if( zNot && n==(int)strlen(zNot)
       && 0==sqlite3_strnicmp(p->azColumn[iCol], zNot, n)
      ){
        p->abNotindexed[iCol] = 1;
        sqlite3_free(zNot);
        azNotindexed[i] = 0;
      }
    }
  }
  for(i=0; i<nNotindexed; i++){
    if( azNotindexed[i] ){
      sqlite3Fts3ErrMsg(pzErr, "no such column: %s", azNotindexed[i]);
      rc = SQLITE_ERROR;
    }
  }

  if( rc==SQLITE_OK && (zCompress==0)!=(zUncompress==0) ){
    char const *zMiss = (zCompress==0 ? "compress" : "uncompress");
    rc = SQLITE_ERROR;
    sqlite3Fts3ErrMsg(pzErr, "missing %s parameter in fts4 constructor", zMiss);
  }
  p->zReadExprlist = fts3ReadExprList(p, zUncompress, &rc);
  p->zWriteExprlist = fts3WriteExprList(p, zCompress, &rc);
  if( rc!=SQLITE_OK ) goto fts3_init_out;

  /* If this is an xCreate call, create the underlying tables in the
  ** database. TODO: For xConnect(), it could verify that said tables exist.
  */
  if( isCreate ){
    rc = fts3CreateTables(p);
  }

  /* Check to see if a legacy fts3 table has been "upgraded" by the
  ** addition of a %_stat table so that it can use incremental merge.
  */
  if( !isFts4 && !isCreate ){
    p->bHasStat = 2;
  }

  /* Figure out the page-size for the database. This is required in order to
  ** estimate the cost of loading large doclists from the database.  */
  fts3DatabasePageSize(&rc, p);
  p->nNodeSize = p->nPgsz-35;

#if defined(SQLITE_DEBUG)||defined(SQLITE_TEST)
  p->nMergeCount = FTS3_MERGE_COUNT;
#endif

  /* Declare the table schema to SQLite. */
  fts3DeclareVtab(&rc, p);

fts3_init_out:
  sqlite3_free(zPrefix);
  sqlite3_free(aIndex);
  sqlite3_free(zCompress);
  sqlite3_free(zUncompress);
  sqlite3_free(zContent);
  sqlite3_free(zLanguageid);
  for(i=0; i<nNotindexed; i++) sqlite3_free(azNotindexed[i]);
  sqlite3_free((void *)aCol);
  sqlite3_free((void *)azNotindexed);
  if( rc!=SQLITE_OK ){
    if( p ){
      fts3DisconnectMethod((sqlite3_vtab *)p);
    }else if( pTokenizer ){
      pTokenizer->pModule->xDestroy(pTokenizer);
    }
  }else{
    assert( p->pSegments==0 );
    *ppVTab = &p->base;
  }
  return rc;
}
