
static int sqlite3Fts5PoslistNext64(
  const u8 *a, int n,             /* Buffer containing poslist */
  int *pi,                        /* IN/OUT: Offset within a[] */
  i64 *piOff                      /* IN/OUT: Current offset */
){
  int i = *pi;
  assert( a!=0 || i==0 );
  if( i>=n ){
    /* EOF */
    *piOff = -1;
    return 1;
  }else{
    i64 iOff = *piOff;
    u32 iVal;
    assert( a!=0 );
    fts5FastGetVarint32(a, i, iVal);
    if( iVal<=1 ){
      if( iVal==0 ){
        *pi = i;
        return 0;
      }
      fts5FastGetVarint32(a, i, iVal);
      iOff = ((i64)iVal) << 32;
      assert( iOff>=0 );
      fts5FastGetVarint32(a, i, iVal);
      if( iVal<2 ){
        /* This is a corrupt record. So stop parsing it here. */
        *piOff = -1;
        return 1;
      }
      *piOff = iOff + ((iVal-2) & 0x7FFFFFFF);
    }else{
      *piOff = (iOff & (i64)0x7FFFFFFF<<32)+((iOff + (iVal-2)) & 0x7FFFFFFF);
    }
    *pi = i;
    assert_nc( *piOff>=iOff );
    return 0;
  }
}


/*
** Advance the iterator object passed as the only argument. Return true
** if the iterator reaches EOF, or false otherwise.
*/
static int sqlite3Fts5PoslistReaderNext(Fts5PoslistReader *pIter){
  if( sqlite3Fts5PoslistNext64(pIter->a, pIter->n, &pIter->i, &pIter->iPos) ){
    pIter->bEof = 1;
  }
  return pIter->bEof;
}

static int sqlite3Fts5PoslistReaderInit(
  const u8 *a, int n,             /* Poslist buffer to iterate through */
  Fts5PoslistReader *pIter        /* Iterator object to initialize */
){
  memset(pIter, 0, sizeof(*pIter));
  pIter->a = a;
  pIter->n = n;
  sqlite3Fts5PoslistReaderNext(pIter);
  return pIter->bEof;
}

/*
** Append position iPos to the position list being accumulated in buffer
** pBuf, which must be already be large enough to hold the new data.
** The previous position written to this list is *piPrev. *piPrev is set
** to iPos before returning.
*/
static void sqlite3Fts5PoslistSafeAppend(
  Fts5Buffer *pBuf,
  i64 *piPrev,
  i64 iPos
){
  if( iPos>=*piPrev ){
    static const i64 colmask = ((i64)(0x7FFFFFFF)) << 32;
    if( (iPos & colmask) != (*piPrev & colmask) ){
      pBuf->p[pBuf->n++] = 1;
      pBuf->n += sqlite3Fts5PutVarint(&pBuf->p[pBuf->n], (iPos>>32));
      *piPrev = (iPos & colmask);
    }
    pBuf->n += sqlite3Fts5PutVarint(&pBuf->p[pBuf->n], (iPos-*piPrev)+2);
    *piPrev = iPos;
  }
}

static int sqlite3Fts5PoslistWriterAppend(
  Fts5Buffer *pBuf,
  Fts5PoslistWriter *pWriter,
  i64 iPos
){
  int rc = 0;   /* Initialized only to suppress erroneous warning from Clang */
  if( fts5BufferGrow(&rc, pBuf, 5+5+5) ) return rc;
  sqlite3Fts5PoslistSafeAppend(pBuf, &pWriter->iPrev, iPos);
  return SQLITE_OK;
}

static void *sqlite3Fts5MallocZero(int *pRc, sqlite3_int64 nByte){
  void *pRet = 0;
  if( *pRc==SQLITE_OK ){
    pRet = sqlite3_malloc64(nByte);
    if( pRet==0 ){
      if( nByte>0 ) *pRc = SQLITE_NOMEM;
    }else{
      memset(pRet, 0, (size_t)nByte);
    }
  }
  return pRet;
}

/*
** Return a nul-terminated copy of the string indicated by pIn. If nIn
** is non-negative, then it is the length of the string in bytes. Otherwise,
** the length of the string is determined using strlen().
**
** It is the responsibility of the caller to eventually free the returned
** buffer using sqlite3_free(). If an OOM error occurs, NULL is returned.
*/
static char *sqlite3Fts5Strndup(int *pRc, const char *pIn, int nIn){
  char *zRet = 0;
  if( *pRc==SQLITE_OK ){
    if( nIn<0 ){
      nIn = (int)strlen(pIn);
    }
    zRet = (char*)sqlite3_malloc(nIn+1);
    if( zRet ){
      memcpy(zRet, pIn, nIn);
      zRet[nIn] = '\0';
    }else{
      *pRc = SQLITE_NOMEM;
    }
  }
  return zRet;
}


/*
** Return true if character 't' may be part of an FTS5 bareword, or false
** otherwise. Characters that may be part of barewords:
**
**   * All non-ASCII characters,
**   * The 52 upper and lower case ASCII characters, and
**   * The 10 integer ASCII characters.
**   * The underscore character "_" (0x5F).
**   * The unicode "subsitute" character (0x1A).
*/
static int sqlite3Fts5IsBareword(char t){
  u8 aBareword[128] = {
    0, 0, 0, 0, 0, 0, 0, 0,    0, 0, 0, 0, 0, 0, 0, 0,   /* 0x00 .. 0x0F */
    0, 0, 0, 0, 0, 0, 0, 0,    0, 0, 1, 0, 0, 0, 0, 0,   /* 0x10 .. 0x1F */
    0, 0, 0, 0, 0, 0, 0, 0,    0, 0, 0, 0, 0, 0, 0, 0,   /* 0x20 .. 0x2F */
    1, 1, 1, 1, 1, 1, 1, 1,    1, 1, 0, 0, 0, 0, 0, 0,   /* 0x30 .. 0x3F */
    0, 1, 1, 1, 1, 1, 1, 1,    1, 1, 1, 1, 1, 1, 1, 1,   /* 0x40 .. 0x4F */
    1, 1, 1, 1, 1, 1, 1, 1,    1, 1, 1, 0, 0, 0, 0, 1,   /* 0x50 .. 0x5F */
    0, 1, 1, 1, 1, 1, 1, 1,    1, 1, 1, 1, 1, 1, 1, 1,   /* 0x60 .. 0x6F */
    1, 1, 1, 1, 1, 1, 1, 1,    1, 1, 1, 0, 0, 0, 0, 0    /* 0x70 .. 0x7F */
  };

  return (t & 0x80) || aBareword[(int)t];
}


/*************************************************************************
*/
typedef struct Fts5TermsetEntry Fts5TermsetEntry;
struct Fts5TermsetEntry {
  char *pTerm;
  int nTerm;
  int iIdx;                       /* Index (main or aPrefix[] entry) */
  Fts5TermsetEntry *pNext;
};

struct Fts5Termset {
  Fts5TermsetEntry *apHash[512];
};

static int sqlite3Fts5TermsetNew(Fts5Termset **pp){
  int rc = SQLITE_OK;
  *pp = sqlite3Fts5MallocZero(&rc, sizeof(Fts5Termset));
  return rc;
}

static int sqlite3Fts5TermsetAdd(
  Fts5Termset *p,
  int iIdx,
  const char *pTerm, int nTerm,
  int *pbPresent
){
  int rc = SQLITE_OK;
  *pbPresent = 0;
  if( p ){
    int i;
    u32 hash = 13;
    Fts5TermsetEntry *pEntry;

    /* Calculate a hash value for this term. This is the same hash checksum
    ** used by the fts5_hash.c module. This is not important for correct
    ** operation of the module, but is necessary to ensure that some tests
    ** designed to produce hash table collisions really do work.  */
    for(i=nTerm-1; i>=0; i--){
      hash = (hash << 3) ^ hash ^ pTerm[i];
    }
    hash = (hash << 3) ^ hash ^ iIdx;
    hash = hash % ArraySize(p->apHash);

    for(pEntry=p->apHash[hash]; pEntry; pEntry=pEntry->pNext){
      if( pEntry->iIdx==iIdx
          && pEntry->nTerm==nTerm
          && memcmp(pEntry->pTerm, pTerm, nTerm)==0
      ){
        *pbPresent = 1;
        break;
      }
    }

    if( pEntry==0 ){
      pEntry = sqlite3Fts5MallocZero(&rc, sizeof(Fts5TermsetEntry) + nTerm);
      if( pEntry ){
        pEntry->pTerm = (char*)&pEntry[1];
        pEntry->nTerm = nTerm;
        pEntry->iIdx = iIdx;
        memcpy(pEntry->pTerm, pTerm, nTerm);
        pEntry->pNext = p->apHash[hash];
        p->apHash[hash] = pEntry;
      }
    }
  }

  return rc;
}

static void sqlite3Fts5TermsetFree(Fts5Termset *p){
  if( p ){
    u32 i;
    for(i=0; i<ArraySize(p->apHash); i++){
      Fts5TermsetEntry *pEntry = p->apHash[i];
      while( pEntry ){
        Fts5TermsetEntry *pDel = pEntry;
        pEntry = pEntry->pNext;
        sqlite3_free(pDel);
      }
    }
    sqlite3_free(p);
  }
}

/*
** 2014 Jun 09
**
** The author disclaims copyright to this source code.  In place of
** a legal notice, here is a blessing:
**
**    May you do good and not evil.
**    May you find forgiveness for yourself and forgive others.
**    May you share freely, never taking more than you give.
**
******************************************************************************
**
** This is an SQLite module implementing full-text search.
*/


/* #include "fts5Int.h" */

#define FTS5_DEFAULT_PAGE_SIZE   4050
#define FTS5_DEFAULT_AUTOMERGE      4
#define FTS5_DEFAULT_USERMERGE      4
#define FTS5_DEFAULT_CRISISMERGE   16
#define FTS5_DEFAULT_HASHSIZE    (1024*1024)

#define FTS5_DEFAULT_DELETE_AUTOMERGE 10      /* default 10% */

/* Maximum allowed page size */
#define FTS5_MAX_PAGE_SIZE (64*1024)

static int fts5_iswhitespace(char x){
  return (x==' ');
}

static int fts5_isopenquote(char x){
  return (x=='"' || x=='\'' || x=='[' || x=='`');
}

/*
** Argument pIn points to a character that is part of a nul-terminated
** string. Return a pointer to the first character following *pIn in
** the string that is not a white-space character.
*/
static const char *fts5ConfigSkipWhitespace(const char *pIn){
  const char *p = pIn;
  if( p ){
    while( fts5_iswhitespace(*p) ){ p++; }
  }
  return p;
}

/*
** Argument pIn points to a character that is part of a nul-terminated
** string. Return a pointer to the first character following *pIn in
** the string that is not a "bareword" character.
*/
static const char *fts5ConfigSkipBareword(const char *pIn){
  const char *p = pIn;
  while ( sqlite3Fts5IsBareword(*p) ) p++;
  if( p==pIn ) p = 0;
  return p;
}

static int fts5_isdigit(char a){
  return (a>='0' && a<='9');
}



static const char *fts5ConfigSkipLiteral(const char *pIn){
  const char *p = pIn;
  switch( *p ){
    case 'n': case 'N':
      if( sqlite3_strnicmp("null", p, 4)==0 ){
        p = &p[4];
      }else{
        p = 0;
      }
      break;

    case 'x': case 'X':
      p++;
      if( *p=='\'' ){
        p++;
        while( (*p>='a' && *p<='f')
            || (*p>='A' && *p<='F')
            || (*p>='0' && *p<='9')
            ){
          p++;
        }
        if( *p=='\'' && 0==((p-pIn)%2) ){
          p++;
        }else{
          p = 0;
        }
      }else{
        p = 0;
      }
      break;

    case '\'':
      p++;
      while( p ){
        if( *p=='\'' ){
          p++;
          if( *p!='\'' ) break;
        }
        p++;
        if( *p==0 ) p = 0;
      }
      break;

    default:
      /* maybe a number */
      if( *p=='+' || *p=='-' ) p++;
      while( fts5_isdigit(*p) ) p++;

      /* At this point, if the literal was an integer, the parse is
      ** finished. Or, if it is a floating point value, it may continue
      ** with either a decimal point or an 'E' character. */
      if( *p=='.' && fts5_isdigit(p[1]) ){
        p += 2;
        while( fts5_isdigit(*p) ) p++;
      }
      if( p==pIn ) p = 0;

      break;
  }

  return p;
}
