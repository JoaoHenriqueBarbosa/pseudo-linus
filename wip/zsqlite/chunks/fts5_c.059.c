
/**************************************************************************
** Start of porter stemmer implementation.
*/

/* Any tokens larger than this (in bytes) are passed through without
** stemming. */
#define FTS5_PORTER_MAX_TOKEN 64

typedef struct PorterTokenizer PorterTokenizer;
struct PorterTokenizer {
  fts5_tokenizer tokenizer;       /* Parent tokenizer module */
  Fts5Tokenizer *pTokenizer;      /* Parent tokenizer instance */
  char aBuf[FTS5_PORTER_MAX_TOKEN + 64];
};

/*
** Delete a "porter" tokenizer.
*/
static void fts5PorterDelete(Fts5Tokenizer *pTok){
  if( pTok ){
    PorterTokenizer *p = (PorterTokenizer*)pTok;
    if( p->pTokenizer ){
      p->tokenizer.xDelete(p->pTokenizer);
    }
    sqlite3_free(p);
  }
}

/*
** Create a "porter" tokenizer.
*/
static int fts5PorterCreate(
  void *pCtx,
  const char **azArg, int nArg,
  Fts5Tokenizer **ppOut
){
  fts5_api *pApi = (fts5_api*)pCtx;
  int rc = SQLITE_OK;
  PorterTokenizer *pRet;
  void *pUserdata = 0;
  const char *zBase = "unicode61";

  if( nArg>0 ){
    zBase = azArg[0];
  }

  pRet = (PorterTokenizer*)sqlite3_malloc(sizeof(PorterTokenizer));
  if( pRet ){
    memset(pRet, 0, sizeof(PorterTokenizer));
    rc = pApi->xFindTokenizer(pApi, zBase, &pUserdata, &pRet->tokenizer);
  }else{
    rc = SQLITE_NOMEM;
  }
  if( rc==SQLITE_OK ){
    int nArg2 = (nArg>0 ? nArg-1 : 0);
    const char **azArg2 = (nArg2 ? &azArg[1] : 0);
    rc = pRet->tokenizer.xCreate(pUserdata, azArg2, nArg2, &pRet->pTokenizer);
  }

  if( rc!=SQLITE_OK ){
    fts5PorterDelete((Fts5Tokenizer*)pRet);
    pRet = 0;
  }
  *ppOut = (Fts5Tokenizer*)pRet;
  return rc;
}

typedef struct PorterContext PorterContext;
struct PorterContext {
  void *pCtx;
  int (*xToken)(void*, int, const char*, int, int, int);
  char *aBuf;
};

typedef struct PorterRule PorterRule;
struct PorterRule {
  const char *zSuffix;
  int nSuffix;
  int (*xCond)(char *zStem, int nStem);
  const char *zOutput;
  int nOutput;
};

#if 0
static int fts5PorterApply(char *aBuf, int *pnBuf, PorterRule *aRule){
  int ret = -1;
  int nBuf = *pnBuf;
  PorterRule *p;

  for(p=aRule; p->zSuffix; p++){
    assert( strlen(p->zSuffix)==p->nSuffix );
    assert( strlen(p->zOutput)==p->nOutput );
    if( nBuf<p->nSuffix ) continue;
    if( 0==memcmp(&aBuf[nBuf - p->nSuffix], p->zSuffix, p->nSuffix) ) break;
  }

  if( p->zSuffix ){
    int nStem = nBuf - p->nSuffix;
    if( p->xCond==0 || p->xCond(aBuf, nStem) ){
      memcpy(&aBuf[nStem], p->zOutput, p->nOutput);
      *pnBuf = nStem + p->nOutput;
      ret = p - aRule;
    }
  }

  return ret;
}
#endif

static int fts5PorterIsVowel(char c, int bYIsVowel){
  return (
      c=='a' || c=='e' || c=='i' || c=='o' || c=='u' || (bYIsVowel && c=='y')
  );
}

static int fts5PorterGobbleVC(char *zStem, int nStem, int bPrevCons){
  int i;
  int bCons = bPrevCons;

  /* Scan for a vowel */
  for(i=0; i<nStem; i++){
    if( 0==(bCons = !fts5PorterIsVowel(zStem[i], bCons)) ) break;
  }

  /* Scan for a consonent */
  for(i++; i<nStem; i++){
    if( (bCons = !fts5PorterIsVowel(zStem[i], bCons)) ) return i+1;
  }
  return 0;
}

/* porter rule condition: (m > 0) */
static int fts5Porter_MGt0(char *zStem, int nStem){
  return !!fts5PorterGobbleVC(zStem, nStem, 0);
}

/* porter rule condition: (m > 1) */
static int fts5Porter_MGt1(char *zStem, int nStem){
  int n;
  n = fts5PorterGobbleVC(zStem, nStem, 0);
  if( n && fts5PorterGobbleVC(&zStem[n], nStem-n, 1) ){
    return 1;
  }
  return 0;
}

/* porter rule condition: (m = 1) */
static int fts5Porter_MEq1(char *zStem, int nStem){
  int n;
  n = fts5PorterGobbleVC(zStem, nStem, 0);
  if( n && 0==fts5PorterGobbleVC(&zStem[n], nStem-n, 1) ){
    return 1;
  }
  return 0;
}

/* porter rule condition: (*o) */
static int fts5Porter_Ostar(char *zStem, int nStem){
  if( zStem[nStem-1]=='w' || zStem[nStem-1]=='x' || zStem[nStem-1]=='y' ){
    return 0;
  }else{
    int i;
    int mask = 0;
    int bCons = 0;
    for(i=0; i<nStem; i++){
      bCons = !fts5PorterIsVowel(zStem[i], bCons);
      assert( bCons==0 || bCons==1 );
      mask = (mask << 1) + bCons;
    }
    return ((mask & 0x0007)==0x0005);
  }
}

/* porter rule condition: (m > 1 and (*S or *T)) */
static int fts5Porter_MGt1_and_S_or_T(char *zStem, int nStem){
  assert( nStem>0 );
  return (zStem[nStem-1]=='s' || zStem[nStem-1]=='t')
      && fts5Porter_MGt1(zStem, nStem);
}

/* porter rule condition: (*v*) */
static int fts5Porter_Vowel(char *zStem, int nStem){
  int i;
  for(i=0; i<nStem; i++){
    if( fts5PorterIsVowel(zStem[i], i>0) ){
      return 1;
    }
  }
  return 0;
}


/**************************************************************************
***************************************************************************
** GENERATED CODE STARTS HERE (mkportersteps.tcl)
*/

static int fts5PorterStep4(char *aBuf, int *pnBuf){
  int ret = 0;
  int nBuf = *pnBuf;
  switch( aBuf[nBuf-2] ){

    case 'a':
      if( nBuf>2 && 0==memcmp("al", &aBuf[nBuf-2], 2) ){
        if( fts5Porter_MGt1(aBuf, nBuf-2) ){
          *pnBuf = nBuf - 2;
        }
      }
      break;

    case 'c':
      if( nBuf>4 && 0==memcmp("ance", &aBuf[nBuf-4], 4) ){
        if( fts5Porter_MGt1(aBuf, nBuf-4) ){
          *pnBuf = nBuf - 4;
        }
      }else if( nBuf>4 && 0==memcmp("ence", &aBuf[nBuf-4], 4) ){
        if( fts5Porter_MGt1(aBuf, nBuf-4) ){
          *pnBuf = nBuf - 4;
        }
      }
      break;

    case 'e':
      if( nBuf>2 && 0==memcmp("er", &aBuf[nBuf-2], 2) ){
        if( fts5Porter_MGt1(aBuf, nBuf-2) ){
          *pnBuf = nBuf - 2;
        }
      }
      break;

    case 'i':
      if( nBuf>2 && 0==memcmp("ic", &aBuf[nBuf-2], 2) ){
        if( fts5Porter_MGt1(aBuf, nBuf-2) ){
          *pnBuf = nBuf - 2;
        }
      }
      break;

    case 'l':
      if( nBuf>4 && 0==memcmp("able", &aBuf[nBuf-4], 4) ){
        if( fts5Porter_MGt1(aBuf, nBuf-4) ){
          *pnBuf = nBuf - 4;
        }
      }else if( nBuf>4 && 0==memcmp("ible", &aBuf[nBuf-4], 4) ){
        if( fts5Porter_MGt1(aBuf, nBuf-4) ){
          *pnBuf = nBuf - 4;
        }
      }
      break;

    case 'n':
      if( nBuf>3 && 0==memcmp("ant", &aBuf[nBuf-3], 3) ){
        if( fts5Porter_MGt1(aBuf, nBuf-3) ){
          *pnBuf = nBuf - 3;
        }
      }else if( nBuf>5 && 0==memcmp("ement", &aBuf[nBuf-5], 5) ){
        if( fts5Porter_MGt1(aBuf, nBuf-5) ){
          *pnBuf = nBuf - 5;
        }
      }else if( nBuf>4 && 0==memcmp("ment", &aBuf[nBuf-4], 4) ){
        if( fts5Porter_MGt1(aBuf, nBuf-4) ){
          *pnBuf = nBuf - 4;
        }
      }else if( nBuf>3 && 0==memcmp("ent", &aBuf[nBuf-3], 3) ){
        if( fts5Porter_MGt1(aBuf, nBuf-3) ){
          *pnBuf = nBuf - 3;
        }
      }
      break;

    case 'o':
      if( nBuf>3 && 0==memcmp("ion", &aBuf[nBuf-3], 3) ){
        if( fts5Porter_MGt1_and_S_or_T(aBuf, nBuf-3) ){
          *pnBuf = nBuf - 3;
        }
      }else if( nBuf>2 && 0==memcmp("ou", &aBuf[nBuf-2], 2) ){
        if( fts5Porter_MGt1(aBuf, nBuf-2) ){
          *pnBuf = nBuf - 2;
        }
      }
      break;

    case 's':
      if( nBuf>3 && 0==memcmp("ism", &aBuf[nBuf-3], 3) ){
        if( fts5Porter_MGt1(aBuf, nBuf-3) ){
          *pnBuf = nBuf - 3;
        }
      }
      break;

    case 't':
      if( nBuf>3 && 0==memcmp("ate", &aBuf[nBuf-3], 3) ){
        if( fts5Porter_MGt1(aBuf, nBuf-3) ){
          *pnBuf = nBuf - 3;
        }
      }else if( nBuf>3 && 0==memcmp("iti", &aBuf[nBuf-3], 3) ){
        if( fts5Porter_MGt1(aBuf, nBuf-3) ){
          *pnBuf = nBuf - 3;
        }
      }
      break;

    case 'u':
      if( nBuf>3 && 0==memcmp("ous", &aBuf[nBuf-3], 3) ){
        if( fts5Porter_MGt1(aBuf, nBuf-3) ){
          *pnBuf = nBuf - 3;
        }
      }
      break;

    case 'v':
      if( nBuf>3 && 0==memcmp("ive", &aBuf[nBuf-3], 3) ){
        if( fts5Porter_MGt1(aBuf, nBuf-3) ){
          *pnBuf = nBuf - 3;
        }
      }
      break;

    case 'z':
      if( nBuf>3 && 0==memcmp("ize", &aBuf[nBuf-3], 3) ){
        if( fts5Porter_MGt1(aBuf, nBuf-3) ){
          *pnBuf = nBuf - 3;
        }
      }
      break;

  }
  return ret;
}


static int fts5PorterStep1B2(char *aBuf, int *pnBuf){
  int ret = 0;
  int nBuf = *pnBuf;
  switch( aBuf[nBuf-2] ){

    case 'a':
      if( nBuf>2 && 0==memcmp("at", &aBuf[nBuf-2], 2) ){
        memcpy(&aBuf[nBuf-2], "ate", 3);
        *pnBuf = nBuf - 2 + 3;
        ret = 1;
      }
      break;

    case 'b':
      if( nBuf>2 && 0==memcmp("bl", &aBuf[nBuf-2], 2) ){
        memcpy(&aBuf[nBuf-2], "ble", 3);
        *pnBuf = nBuf - 2 + 3;
        ret = 1;
      }
      break;

    case 'i':
      if( nBuf>2 && 0==memcmp("iz", &aBuf[nBuf-2], 2) ){
        memcpy(&aBuf[nBuf-2], "ize", 3);
        *pnBuf = nBuf - 2 + 3;
        ret = 1;
      }
      break;

  }
  return ret;
}
