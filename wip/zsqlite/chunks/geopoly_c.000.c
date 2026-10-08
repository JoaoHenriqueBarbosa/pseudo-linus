/*
** 2018-05-25
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
** This file implements an alternative R-Tree virtual table that
** uses polygons to express the boundaries of 2-dimensional objects.
**
** This file is #include-ed onto the end of "rtree.c" so that it has
** access to all of the R-Tree internals.
*/
/* #include <stdlib.h> */

/* Enable -DGEOPOLY_ENABLE_DEBUG for debugging facilities */
#ifdef GEOPOLY_ENABLE_DEBUG
  static int geo_debug = 0;
# define GEODEBUG(X) if(geo_debug)printf X
#else
# define GEODEBUG(X)
#endif

/* Character class routines */
#ifdef sqlite3Isdigit
   /* Use the SQLite core versions if this routine is part of the
   ** SQLite amalgamation */
#  define safe_isdigit(x)  sqlite3Isdigit(x)
#  define safe_isalnum(x)  sqlite3Isalnum(x)
#  define safe_isxdigit(x) sqlite3Isxdigit(x)
#else
   /* Use the standard library for separate compilation */
#include <ctype.h>  /* amalgamator: keep */
#  define safe_isdigit(x)  isdigit((unsigned char)(x))
#  define safe_isalnum(x)  isalnum((unsigned char)(x))
#  define safe_isxdigit(x) isxdigit((unsigned char)(x))
#endif

#ifndef JSON_NULL   /* The following stuff repeats things found in json1 */
/*
** Growing our own isspace() routine this way is twice as fast as
** the library isspace() function.
*/
static const char geopolyIsSpace[] = {
  0, 0, 0, 0, 0, 0, 0, 0,     0, 1, 1, 0, 0, 1, 0, 0,
  0, 0, 0, 0, 0, 0, 0, 0,     0, 0, 0, 0, 0, 0, 0, 0,
  1, 0, 0, 0, 0, 0, 0, 0,     0, 0, 0, 0, 0, 0, 0, 0,
  0, 0, 0, 0, 0, 0, 0, 0,     0, 0, 0, 0, 0, 0, 0, 0,
  0, 0, 0, 0, 0, 0, 0, 0,     0, 0, 0, 0, 0, 0, 0, 0,
  0, 0, 0, 0, 0, 0, 0, 0,     0, 0, 0, 0, 0, 0, 0, 0,
  0, 0, 0, 0, 0, 0, 0, 0,     0, 0, 0, 0, 0, 0, 0, 0,
  0, 0, 0, 0, 0, 0, 0, 0,     0, 0, 0, 0, 0, 0, 0, 0,
  0, 0, 0, 0, 0, 0, 0, 0,     0, 0, 0, 0, 0, 0, 0, 0,
  0, 0, 0, 0, 0, 0, 0, 0,     0, 0, 0, 0, 0, 0, 0, 0,
  0, 0, 0, 0, 0, 0, 0, 0,     0, 0, 0, 0, 0, 0, 0, 0,
  0, 0, 0, 0, 0, 0, 0, 0,     0, 0, 0, 0, 0, 0, 0, 0,
  0, 0, 0, 0, 0, 0, 0, 0,     0, 0, 0, 0, 0, 0, 0, 0,
  0, 0, 0, 0, 0, 0, 0, 0,     0, 0, 0, 0, 0, 0, 0, 0,
  0, 0, 0, 0, 0, 0, 0, 0,     0, 0, 0, 0, 0, 0, 0, 0,
  0, 0, 0, 0, 0, 0, 0, 0,     0, 0, 0, 0, 0, 0, 0, 0,
};
#define fast_isspace(x) (geopolyIsSpace[(unsigned char)x])
#endif /* JSON NULL - back to original code */

/* Compiler and version */
#ifndef GCC_VERSION
#if defined(__GNUC__) && !defined(SQLITE_DISABLE_INTRINSIC)
# define GCC_VERSION (__GNUC__*1000000+__GNUC_MINOR__*1000+__GNUC_PATCHLEVEL__)
#else
# define GCC_VERSION 0
#endif
#endif
#ifndef MSVC_VERSION
#if defined(_MSC_VER) && !defined(SQLITE_DISABLE_INTRINSIC)
# define MSVC_VERSION _MSC_VER
#else
# define MSVC_VERSION 0
#endif
#endif

/* Datatype for coordinates
*/
typedef float GeoCoord;

/*
** Internal representation of a polygon.
**
** The polygon consists of a sequence of vertexes.  There is a line
** segment between each pair of vertexes, and one final segment from
** the last vertex back to the first.  (This differs from the GeoJSON
** standard in which the final vertex is a repeat of the first.)
**
** The polygon follows the right-hand rule.  The area to the right of
** each segment is "outside" and the area to the left is "inside".
**
** The on-disk representation consists of a 4-byte header followed by
** the values.  The 4-byte header is:
**
**      encoding    (1 byte)   0=big-endian, 1=little-endian
**      nvertex     (3 bytes)  Number of vertexes as a big-endian integer
**
** Enough space is allocated for 4 coordinates, to work around over-zealous
** warnings coming from some compiler (notably, clang). In reality, the size
** of each GeoPoly memory allocate is adjusted as necessary so that the
** GeoPoly.a[] array at the end is the appropriate size.
*/
typedef struct GeoPoly GeoPoly;
struct GeoPoly {
  int nVertex;          /* Number of vertexes */
  unsigned char hdr[4]; /* Header for on-disk representation */
  GeoCoord a[8];        /* 2*nVertex values. X (longitude) first, then Y */
};

/* The size of a memory allocation needed for a GeoPoly object sufficient
** to hold N coordinate pairs.
*/
#define GEOPOLY_SZ(N)  (sizeof(GeoPoly) + sizeof(GeoCoord)*2*((N)-4))

/* Macros to access coordinates of a GeoPoly.
** We have to use these macros, rather than just say p->a[i] in order
** to silence (incorrect) UBSAN warnings if the array index is too large.
*/
#define GeoX(P,I)  (((GeoCoord*)(P)->a)[(I)*2])
#define GeoY(P,I)  (((GeoCoord*)(P)->a)[(I)*2+1])


/*
** State of a parse of a GeoJSON input.
*/
typedef struct GeoParse GeoParse;
struct GeoParse {
  const unsigned char *z;   /* Unparsed input */
  int nVertex;              /* Number of vertexes in a[] */
  int nAlloc;               /* Space allocated to a[] */
  int nErr;                 /* Number of errors encountered */
  GeoCoord *a;          /* Array of vertexes.  From sqlite3_malloc64() */
};

/* Do a 4-byte byte swap */
static void geopolySwab32(unsigned char *a){
  unsigned char t = a[0];
  a[0] = a[3];
  a[3] = t;
  t = a[1];
  a[1] = a[2];
  a[2] = t;
}

/* Skip whitespace.  Return the next non-whitespace character. */
static char geopolySkipSpace(GeoParse *p){
  while( fast_isspace(p->z[0]) ) p->z++;
  return p->z[0];
}

/* Parse out a number.  Write the value into *pVal if pVal!=0.
** return non-zero on success and zero if the next token is not a number.
*/
static int geopolyParseNumber(GeoParse *p, GeoCoord *pVal){
  char c = geopolySkipSpace(p);
  const unsigned char *z = p->z;
  int j = 0;
  int seenDP = 0;
  int seenE = 0;
  if( c=='-' ){
    j = 1;
    c = z[j];
  }
  if( c=='0' && z[j+1]>='0' && z[j+1]<='9' ) return 0;
  for(;; j++){
    c = z[j];
    if( safe_isdigit(c) ) continue;
    if( c=='.' ){
      if( z[j-1]=='-' ) return 0;
      if( seenDP ) return 0;
      seenDP = 1;
      continue;
    }
    if( c=='e' || c=='E' ){
      if( z[j-1]<'0' ) return 0;
      if( seenE ) return -1;
      seenDP = seenE = 1;
      c = z[j+1];
      if( c=='+' || c=='-' ){
        j++;
        c = z[j+1];
      }
      if( c<'0' || c>'9' ) return 0;
      continue;
    }
    break;
  }
  if( z[j-1]<'0' ) return 0;
  if( pVal ){
#ifdef SQLITE_AMALGAMATION
     /* The sqlite3AtoF() routine is much much faster than atof(), if it
     ** is available */
     double r;
     (void)sqlite3AtoF((const char*)p->z, &r, j, SQLITE_UTF8);
     *pVal = r;
#else
     *pVal = (GeoCoord)atof((const char*)p->z);
#endif
  }
  p->z += j;
  return 1;
}

/*
** If the input is a well-formed JSON array of coordinates with at least
** four coordinates and where each coordinate is itself a two-value array,
** then convert the JSON into a GeoPoly object and return a pointer to
** that object.
**
** If any error occurs, return NULL.
*/
static GeoPoly *geopolyParseJson(const unsigned char *z, int *pRc){
  GeoParse s;
  int rc = SQLITE_OK;
  memset(&s, 0, sizeof(s));
  s.z = z;
  if( geopolySkipSpace(&s)=='[' ){
    s.z++;
    while( geopolySkipSpace(&s)=='[' ){
      int ii = 0;
      char c;
      s.z++;
      if( s.nVertex>=s.nAlloc ){
        GeoCoord *aNew;
        s.nAlloc = s.nAlloc*2 + 16;
        aNew = sqlite3_realloc64(s.a, s.nAlloc*sizeof(GeoCoord)*2 );
        if( aNew==0 ){
          rc = SQLITE_NOMEM;
          s.nErr++;
          break;
        }
        s.a = aNew;
      }
      while( geopolyParseNumber(&s, ii<=1 ? &s.a[s.nVertex*2+ii] : 0) ){
        ii++;
        if( ii==2 ) s.nVertex++;
        c = geopolySkipSpace(&s);
        s.z++;
        if( c==',' ) continue;
        if( c==']' && ii>=2 ) break;
        s.nErr++;
        rc = SQLITE_ERROR;
        goto parse_json_err;
      }
      if( geopolySkipSpace(&s)==',' ){
        s.z++;
        continue;
      }
      break;
    }
    if( geopolySkipSpace(&s)==']'
     && s.nVertex>=4
     && s.a[0]==s.a[s.nVertex*2-2]
     && s.a[1]==s.a[s.nVertex*2-1]
     && (s.z++, geopolySkipSpace(&s)==0)
    ){
      GeoPoly *pOut;
      int x = 1;
      s.nVertex--;  /* Remove the redundant vertex at the end */
      pOut = sqlite3_malloc64( GEOPOLY_SZ((sqlite3_int64)s.nVertex) );
      x = 1;
      if( pOut==0 ) goto parse_json_err;
      pOut->nVertex = s.nVertex;
      memcpy(pOut->a, s.a, s.nVertex*2*sizeof(GeoCoord));
      pOut->hdr[0] = *(unsigned char*)&x;
      pOut->hdr[1] = (s.nVertex>>16)&0xff;
      pOut->hdr[2] = (s.nVertex>>8)&0xff;
      pOut->hdr[3] = s.nVertex&0xff;
      sqlite3_free(s.a);
      if( pRc ) *pRc = SQLITE_OK;
      return pOut;
    }else{
      s.nErr++;
      rc = SQLITE_ERROR;
    }
  }
parse_json_err:
  if( pRc ) *pRc = rc;
  sqlite3_free(s.a);
  return 0;
}

/*
** Given a function parameter, try to interpret it as a polygon, either
** in the binary format or JSON text.  Compute a GeoPoly object and
** return a pointer to that object.  Or if the input is not a well-formed
** polygon, put an error message in sqlite3_context and return NULL.
*/
static GeoPoly *geopolyFuncParam(
  sqlite3_context *pCtx,      /* Context for error messages */
  sqlite3_value *pVal,        /* The value to decode */
  int *pRc                    /* Write error here */
){
  GeoPoly *p = 0;
  int nByte;
  testcase( pCtx==0 );
  if( sqlite3_value_type(pVal)==SQLITE_BLOB
   && (nByte = sqlite3_value_bytes(pVal))>=(int)(4+6*sizeof(GeoCoord))
  ){
    const unsigned char *a = sqlite3_value_blob(pVal);
    int nVertex;
    if( a==0 ){
      if( pCtx ) sqlite3_result_error_nomem(pCtx);
      return 0;
    }
    nVertex = (a[1]<<16) + (a[2]<<8) + a[3];
    if( (a[0]==0 || a[0]==1)
     && (nVertex*2*sizeof(GeoCoord) + 4)==(unsigned int)nByte
    ){
      p = sqlite3_malloc64( sizeof(*p) + (nVertex-1)*2*sizeof(GeoCoord) );
      if( p==0 ){
        if( pRc ) *pRc = SQLITE_NOMEM;
        if( pCtx ) sqlite3_result_error_nomem(pCtx);
      }else{
        int x = 1;
        p->nVertex = nVertex;
        memcpy(p->hdr, a, nByte);
        if( a[0] != *(unsigned char*)&x ){
          int ii;
          for(ii=0; ii<nVertex; ii++){
            geopolySwab32((unsigned char*)&GeoX(p,ii));
            geopolySwab32((unsigned char*)&GeoY(p,ii));
          }
          p->hdr[0] ^= 1;
        }
      }
    }
    if( pRc ) *pRc = SQLITE_OK;
    return p;
  }else if( sqlite3_value_type(pVal)==SQLITE_TEXT ){
    const unsigned char *zJson = sqlite3_value_text(pVal);
    if( zJson==0 ){
      if( pRc ) *pRc = SQLITE_NOMEM;
      return 0;
    }
    return geopolyParseJson(zJson, pRc);
  }else{
    if( pRc ) *pRc = SQLITE_ERROR;
    return 0;
  }
}

/*
** Implementation of the geopoly_blob(X) function.
**
** If the input is a well-formed Geopoly BLOB or JSON string
** then return the BLOB representation of the polygon.  Otherwise
** return NULL.
*/
static void geopolyBlobFunc(
  sqlite3_context *context,
  int argc,
  sqlite3_value **argv
){
  GeoPoly *p = geopolyFuncParam(context, argv[0], 0);
  (void)argc;
  if( p ){
    sqlite3_result_blob(context, p->hdr,
       4+8*p->nVertex, SQLITE_TRANSIENT);
    sqlite3_free(p);
  }
}
