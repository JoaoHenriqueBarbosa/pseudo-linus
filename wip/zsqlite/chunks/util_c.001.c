
/*
** If the input token p is quoted, try to adjust the token to remove
** the quotes.  This is not always possible:
**
**     "abc"     ->   abc
**     "ab""cd"  ->   (not possible because of the interior "")
**
** Remove the quotes if possible.  This is a optimization.  The overall
** system should still return the correct answer even if this routine
** is always a no-op.
*/
SQLITE_PRIVATE void sqlite3DequoteToken(Token *p){
  unsigned int i;
  if( p->n<2 ) return;
  if( !sqlite3Isquote(p->z[0]) ) return;
  for(i=1; i<p->n-1; i++){
    if( sqlite3Isquote(p->z[i]) ) return;
  }
  p->n -= 2;
  p->z++;
}

/*
** Generate a Token object from a string
*/
SQLITE_PRIVATE void sqlite3TokenInit(Token *p, char *z){
  p->z = z;
  p->n = sqlite3Strlen30(z);
}

/* Convenient short-hand */
#define UpperToLower sqlite3UpperToLower

/*
** Some systems have stricmp().  Others have strcasecmp().  Because
** there is no consistency, we will define our own.
**
** IMPLEMENTATION-OF: R-30243-02494 The sqlite3_stricmp() and
** sqlite3_strnicmp() APIs allow applications and extensions to compare
** the contents of two buffers containing UTF-8 strings in a
** case-independent fashion, using the same definition of "case
** independence" that SQLite uses internally when comparing identifiers.
*/
SQLITE_API int sqlite3_stricmp(const char *zLeft, const char *zRight){
  if( zLeft==0 ){
    return zRight ? -1 : 0;
  }else if( zRight==0 ){
    return 1;
  }
  return sqlite3StrICmp(zLeft, zRight);
}
SQLITE_PRIVATE int sqlite3StrICmp(const char *zLeft, const char *zRight){
  unsigned char *a, *b;
  int c, x;
  a = (unsigned char *)zLeft;
  b = (unsigned char *)zRight;
  for(;;){
    c = *a;
    x = *b;
    if( c==x ){
      if( c==0 ) break;
    }else{
      c = (int)UpperToLower[c] - (int)UpperToLower[x];
      if( c ) break;
    }
    a++;
    b++;
  }
  return c;
}
SQLITE_API int sqlite3_strnicmp(const char *zLeft, const char *zRight, int N){
  register unsigned char *a, *b;
  if( zLeft==0 ){
    return zRight ? -1 : 0;
  }else if( zRight==0 ){
    return 1;
  }
  a = (unsigned char *)zLeft;
  b = (unsigned char *)zRight;
  while( N-- > 0 && *a!=0 && UpperToLower[*a]==UpperToLower[*b]){ a++; b++; }
  return N<0 ? 0 : UpperToLower[*a] - UpperToLower[*b];
}

/*
** Compute an 8-bit hash on a string that is insensitive to case differences
*/
SQLITE_PRIVATE u8 sqlite3StrIHash(const char *z){
  u8 h = 0;
  if( z==0 ) return 0;
  while( z[0] ){
    h += UpperToLower[(unsigned char)z[0]];
    z++;
  }
  return h;
}

/* Double-Double multiplication.  (x[0],x[1]) *= (y,yy)
**
** Reference:
**   T. J. Dekker, "A Floating-Point Technique for Extending the
**   Available Precision".  1971-07-26.
*/
static void dekkerMul2(volatile double *x, double y, double yy){
  /*
  ** The "volatile" keywords on parameter x[] and on local variables
  ** below are needed force intermediate results to be truncated to
  ** binary64 rather than be carried around in an extended-precision
  ** format.  The truncation is necessary for the Dekker algorithm to
  ** work.  Intel x86 floating point might omit the truncation without
  ** the use of volatile.
  */
  volatile double tx, ty, p, q, c, cc;
  double hx, hy;
  u64 m;
  memcpy(&m, (void*)&x[0], 8);
  m &= 0xfffffffffc000000LL;
  memcpy(&hx, &m, 8);
  tx = x[0] - hx;
  memcpy(&m, &y, 8);
  m &= 0xfffffffffc000000LL;
  memcpy(&hy, &m, 8);
  ty = y - hy;
  p = hx*hy;
  q = hx*ty + tx*hy;
  c = p+q;
  cc = p - c + q + tx*ty;
  cc = x[0]*yy + x[1]*y + cc;
  x[0] = c + cc;
  x[1] = c - x[0];
  x[1] += cc;
}

/*
** The string z[] is an text representation of a real number.
** Convert this string to a double and write it into *pResult.
**
** The string z[] is length bytes in length (bytes, not characters) and
** uses the encoding enc.  The string is not necessarily zero-terminated.
**
** Return TRUE if the result is a valid real number (or integer) and FALSE
** if the string is empty or contains extraneous text.  More specifically
** return
**      1          =>  The input string is a pure integer
**      2 or more  =>  The input has a decimal point or eNNN clause
**      0 or less  =>  The input string is not a valid number
**     -1          =>  Not a valid number, but has a valid prefix which
**                     includes a decimal point and/or an eNNN clause
**
** Valid numbers are in one of these formats:
**
**    [+-]digits[E[+-]digits]
**    [+-]digits.[digits][E[+-]digits]
**    [+-].digits[E[+-]digits]
**
** Leading and trailing whitespace is ignored for the purpose of determining
** validity.
**
** If some prefix of the input string is a valid number, this routine
** returns FALSE but it still converts the prefix and writes the result
** into *pResult.
*/
#if defined(_MSC_VER)
#pragma warning(disable : 4756)
#endif
SQLITE_PRIVATE int sqlite3AtoF(const char *z, double *pResult, int length, u8 enc){
#ifndef SQLITE_OMIT_FLOATING_POINT
  int incr;
  const char *zEnd;
  /* sign * significand * (10 ^ (esign * exponent)) */
  int sign = 1;    /* sign of significand */
  u64 s = 0;       /* significand */
  int d = 0;       /* adjust exponent for shifting decimal point */
  int esign = 1;   /* sign of exponent */
  int e = 0;       /* exponent */
  int eValid = 1;  /* True exponent is either not used or is well-formed */
  int nDigit = 0;  /* Number of digits processed */
  int eType = 1;   /* 1: pure integer,  2+: fractional  -1 or less: bad UTF16 */

  assert( enc==SQLITE_UTF8 || enc==SQLITE_UTF16LE || enc==SQLITE_UTF16BE );
  *pResult = 0.0;   /* Default return value, in case of an error */
  if( length==0 ) return 0;

  if( enc==SQLITE_UTF8 ){
    incr = 1;
    zEnd = z + length;
  }else{
    int i;
    incr = 2;
    length &= ~1;
    assert( SQLITE_UTF16LE==2 && SQLITE_UTF16BE==3 );
    testcase( enc==SQLITE_UTF16LE );
    testcase( enc==SQLITE_UTF16BE );
    for(i=3-enc; i<length && z[i]==0; i+=2){}
    if( i<length ) eType = -100;
    zEnd = &z[i^1];
    z += (enc&1);
  }

  /* skip leading spaces */
  while( z<zEnd && sqlite3Isspace(*z) ) z+=incr;
  if( z>=zEnd ) return 0;

  /* get sign of significand */
  if( *z=='-' ){
    sign = -1;
    z+=incr;
  }else if( *z=='+' ){
    z+=incr;
  }

  /* copy max significant digits to significand */
  while( z<zEnd && sqlite3Isdigit(*z) ){
    s = s*10 + (*z - '0');
    z+=incr; nDigit++;
    if( s>=((LARGEST_UINT64-9)/10) ){
      /* skip non-significant significand digits
      ** (increase exponent by d to shift decimal left) */
      while( z<zEnd && sqlite3Isdigit(*z) ){ z+=incr; d++; }
    }
  }
  if( z>=zEnd ) goto do_atof_calc;

  /* if decimal point is present */
  if( *z=='.' ){
    z+=incr;
    eType++;
    /* copy digits from after decimal to significand
    ** (decrease exponent by d to shift decimal right) */
    while( z<zEnd && sqlite3Isdigit(*z) ){
      if( s<((LARGEST_UINT64-9)/10) ){
        s = s*10 + (*z - '0');
        d--;
        nDigit++;
      }
      z+=incr;
    }
  }
  if( z>=zEnd ) goto do_atof_calc;

  /* if exponent is present */
  if( *z=='e' || *z=='E' ){
    z+=incr;
    eValid = 0;
    eType++;

    /* This branch is needed to avoid a (harmless) buffer overread.  The
    ** special comment alerts the mutation tester that the correct answer
    ** is obtained even if the branch is omitted */
    if( z>=zEnd ) goto do_atof_calc;              /*PREVENTS-HARMLESS-OVERREAD*/

    /* get sign of exponent */
    if( *z=='-' ){
      esign = -1;
      z+=incr;
    }else if( *z=='+' ){
      z+=incr;
    }
    /* copy digits to exponent */
    while( z<zEnd && sqlite3Isdigit(*z) ){
      e = e<10000 ? (e*10 + (*z - '0')) : 10000;
      z+=incr;
      eValid = 1;
    }
  }

  /* skip trailing spaces */
  while( z<zEnd && sqlite3Isspace(*z) ) z+=incr;

do_atof_calc:
  /* Zero is a special case */
  if( s==0 ){
    *pResult = sign<0 ? -0.0 : +0.0;
    goto atof_return;
  }

  /* adjust exponent by d, and update sign */
  e = (e*esign) + d;

  /* Try to adjust the exponent to make it smaller */
  while( e>0 && s<(LARGEST_UINT64/10) ){
    s *= 10;
    e--;
  }
  while( e<0 && (s%10)==0 ){
    s /= 10;
    e++;
  }

  if( e==0 ){
    *pResult = s;
  }else if( sqlite3Config.bUseLongDouble ){
    LONGDOUBLE_TYPE r = (LONGDOUBLE_TYPE)s;
    if( e>0 ){
      while( e>=100  ){ e-=100; r *= 1.0e+100L; }
      while( e>=10   ){ e-=10;  r *= 1.0e+10L;  }
      while( e>=1    ){ e-=1;   r *= 1.0e+01L;  }
    }else{
      while( e<=-100 ){ e+=100; r *= 1.0e-100L; }
      while( e<=-10  ){ e+=10;  r *= 1.0e-10L;  }
      while( e<=-1   ){ e+=1;   r *= 1.0e-01L;  }
    }
    assert( r>=0.0 );
    if( r>+1.7976931348623157081452742373e+308L ){
#ifdef INFINITY
      *pResult = +INFINITY;
#else
      *pResult = 1.0e308*10.0;
#endif
    }else{
      *pResult = (double)r;
    }
  }else{
    double rr[2];
    u64 s2;
    rr[0] = (double)s;
    s2 = (u64)rr[0];
#if defined(_MSC_VER) && _MSC_VER<1700
    if( s2==0x8000000000000000LL ){ s2 = 2*(u64)(0.5*rr[0]); }
#endif
    rr[1] = s>=s2 ? (double)(s - s2) : -(double)(s2 - s);
    if( e>0 ){
      while( e>=100  ){
        e -= 100;
        dekkerMul2(rr, 1.0e+100, -1.5902891109759918046e+83);
      }
      while( e>=10   ){
        e -= 10;
        dekkerMul2(rr, 1.0e+10, 0.0);
      }
      while( e>=1    ){
        e -= 1;
        dekkerMul2(rr, 1.0e+01, 0.0);
      }
    }else{
      while( e<=-100 ){
        e += 100;
        dekkerMul2(rr, 1.0e-100, -1.99918998026028836196e-117);
      }
      while( e<=-10  ){
        e += 10;
        dekkerMul2(rr, 1.0e-10, -3.6432197315497741579e-27);
      }
      while( e<=-1   ){
        e += 1;
        dekkerMul2(rr, 1.0e-01, -5.5511151231257827021e-18);
      }
    }
    *pResult = rr[0]+rr[1];
    if( sqlite3IsNaN(*pResult) ) *pResult = 1e300*1e300;
  }
  if( sign<0 ) *pResult = -*pResult;
  assert( !sqlite3IsNaN(*pResult) );

atof_return:
  /* return true if number and no extra non-whitespace characters after */
  if( z==zEnd && nDigit>0 && eValid && eType>0 ){
    return eType;
  }else if( eType>=2 && (eType==3 || eValid) && nDigit>0 ){
    return -1;
  }else{
    return 0;
  }
#else
  return !sqlite3Atoi64(z, pResult, length, enc);
#endif /* SQLITE_OMIT_FLOATING_POINT */
}
