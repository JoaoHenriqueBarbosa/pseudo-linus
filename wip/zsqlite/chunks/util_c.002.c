#if defined(_MSC_VER)
#pragma warning(default : 4756)
#endif

/*
** Render an signed 64-bit integer as text.  Store the result in zOut[] and
** return the length of the string that was stored, in bytes.  The value
** returned does not include the zero terminator at the end of the output
** string.
**
** The caller must ensure that zOut[] is at least 21 bytes in size.
*/
SQLITE_PRIVATE int sqlite3Int64ToText(i64 v, char *zOut){
  int i;
  u64 x;
  char zTemp[22];
  if( v<0 ){
    x = (v==SMALLEST_INT64) ? ((u64)1)<<63 : (u64)-v;
  }else{
    x = v;
  }
  i = sizeof(zTemp)-2;
  zTemp[sizeof(zTemp)-1] = 0;
  while( 1 /*exit-by-break*/ ){
    zTemp[i] = (x%10) + '0';
    x = x/10;
    if( x==0 ) break;
    i--;
  };
  if( v<0 ) zTemp[--i] = '-';
  memcpy(zOut, &zTemp[i], sizeof(zTemp)-i);
  return sizeof(zTemp)-1-i;
}

/*
** Compare the 19-character string zNum against the text representation
** value 2^63:  9223372036854775808.  Return negative, zero, or positive
** if zNum is less than, equal to, or greater than the string.
** Note that zNum must contain exactly 19 characters.
**
** Unlike memcmp() this routine is guaranteed to return the difference
** in the values of the last digit if the only difference is in the
** last digit.  So, for example,
**
**      compare2pow63("9223372036854775800", 1)
**
** will return -8.
*/
static int compare2pow63(const char *zNum, int incr){
  int c = 0;
  int i;
                    /* 012345678901234567 */
  const char *pow63 = "922337203685477580";
  for(i=0; c==0 && i<18; i++){
    c = (zNum[i*incr]-pow63[i])*10;
  }
  if( c==0 ){
    c = zNum[18*incr] - '8';
    testcase( c==(-1) );
    testcase( c==0 );
    testcase( c==(+1) );
  }
  return c;
}

/*
** Convert zNum to a 64-bit signed integer.  zNum must be decimal. This
** routine does *not* accept hexadecimal notation.
**
** Returns:
**
**    -1    Not even a prefix of the input text looks like an integer
**     0    Successful transformation.  Fits in a 64-bit signed integer.
**     1    Excess non-space text after the integer value
**     2    Integer too large for a 64-bit signed integer or is malformed
**     3    Special case of 9223372036854775808
**
** length is the number of bytes in the string (bytes, not characters).
** The string is not necessarily zero-terminated.  The encoding is
** given by enc.
*/
SQLITE_PRIVATE int sqlite3Atoi64(const char *zNum, i64 *pNum, int length, u8 enc){
  int incr;
  u64 u = 0;
  int neg = 0; /* assume positive */
  int i;
  int c = 0;
  int nonNum = 0;  /* True if input contains UTF16 with high byte non-zero */
  int rc;          /* Baseline return code */
  const char *zStart;
  const char *zEnd = zNum + length;
  assert( enc==SQLITE_UTF8 || enc==SQLITE_UTF16LE || enc==SQLITE_UTF16BE );
  if( enc==SQLITE_UTF8 ){
    incr = 1;
  }else{
    incr = 2;
    length &= ~1;
    assert( SQLITE_UTF16LE==2 && SQLITE_UTF16BE==3 );
    for(i=3-enc; i<length && zNum[i]==0; i+=2){}
    nonNum = i<length;
    zEnd = &zNum[i^1];
    zNum += (enc&1);
  }
  while( zNum<zEnd && sqlite3Isspace(*zNum) ) zNum+=incr;
  if( zNum<zEnd ){
    if( *zNum=='-' ){
      neg = 1;
      zNum+=incr;
    }else if( *zNum=='+' ){
      zNum+=incr;
    }
  }
  zStart = zNum;
  while( zNum<zEnd && zNum[0]=='0' ){ zNum+=incr; } /* Skip leading zeros. */
  for(i=0; &zNum[i]<zEnd && (c=zNum[i])>='0' && c<='9'; i+=incr){
    u = u*10 + c - '0';
  }
  testcase( i==18*incr );
  testcase( i==19*incr );
  testcase( i==20*incr );
  if( u>LARGEST_INT64 ){
    /* This test and assignment is needed only to suppress UB warnings
    ** from clang and -fsanitize=undefined.  This test and assignment make
    ** the code a little larger and slower, and no harm comes from omitting
    ** them, but we must appease the undefined-behavior pharisees. */
    *pNum = neg ? SMALLEST_INT64 : LARGEST_INT64;
  }else if( neg ){
    *pNum = -(i64)u;
  }else{
    *pNum = (i64)u;
  }
  rc = 0;
  if( i==0 && zStart==zNum ){    /* No digits */
    rc = -1;
  }else if( nonNum ){            /* UTF16 with high-order bytes non-zero */
    rc = 1;
  }else if( &zNum[i]<zEnd ){     /* Extra bytes at the end */
    int jj = i;
    do{
      if( !sqlite3Isspace(zNum[jj]) ){
        rc = 1;          /* Extra non-space text after the integer */
        break;
      }
      jj += incr;
    }while( &zNum[jj]<zEnd );
  }
  if( i<19*incr ){
    /* Less than 19 digits, so we know that it fits in 64 bits */
    assert( u<=LARGEST_INT64 );
    return rc;
  }else{
    /* zNum is a 19-digit numbers.  Compare it against 9223372036854775808. */
    c = i>19*incr ? 1 : compare2pow63(zNum, incr);
    if( c<0 ){
      /* zNum is less than 9223372036854775808 so it fits */
      assert( u<=LARGEST_INT64 );
      return rc;
    }else{
      *pNum = neg ? SMALLEST_INT64 : LARGEST_INT64;
      if( c>0 ){
        /* zNum is greater than 9223372036854775808 so it overflows */
        return 2;
      }else{
        /* zNum is exactly 9223372036854775808.  Fits if negative.  The
        ** special case 2 overflow if positive */
        assert( u-1==LARGEST_INT64 );
        return neg ? rc : 3;
      }
    }
  }
}

/*
** Transform a UTF-8 integer literal, in either decimal or hexadecimal,
** into a 64-bit signed integer.  This routine accepts hexadecimal literals,
** whereas sqlite3Atoi64() does not.
**
** Returns:
**
**     0    Successful transformation.  Fits in a 64-bit signed integer.
**     1    Excess text after the integer value
**     2    Integer too large for a 64-bit signed integer or is malformed
**     3    Special case of 9223372036854775808
*/
SQLITE_PRIVATE int sqlite3DecOrHexToI64(const char *z, i64 *pOut){
#ifndef SQLITE_OMIT_HEX_INTEGER
  if( z[0]=='0'
   && (z[1]=='x' || z[1]=='X')
  ){
    u64 u = 0;
    int i, k;
    for(i=2; z[i]=='0'; i++){}
    for(k=i; sqlite3Isxdigit(z[k]); k++){
      u = u*16 + sqlite3HexToInt(z[k]);
    }
    memcpy(pOut, &u, 8);
    if( k-i>16 ) return 2;
    if( z[k]!=0 ) return 1;
    return 0;
  }else
#endif /* SQLITE_OMIT_HEX_INTEGER */
  {
    int n = (int)(0x3fffffff&strspn(z,"+- \n\t0123456789"));
    if( z[n] ) n++;
    return sqlite3Atoi64(z, pOut, n, SQLITE_UTF8);
  }
}

/*
** If zNum represents an integer that will fit in 32-bits, then set
** *pValue to that integer and return true.  Otherwise return false.
**
** This routine accepts both decimal and hexadecimal notation for integers.
**
** Any non-numeric characters that following zNum are ignored.
** This is different from sqlite3Atoi64() which requires the
** input number to be zero-terminated.
*/
SQLITE_PRIVATE int sqlite3GetInt32(const char *zNum, int *pValue){
  sqlite_int64 v = 0;
  int i, c;
  int neg = 0;
  if( zNum[0]=='-' ){
    neg = 1;
    zNum++;
  }else if( zNum[0]=='+' ){
    zNum++;
  }
#ifndef SQLITE_OMIT_HEX_INTEGER
  else if( zNum[0]=='0'
        && (zNum[1]=='x' || zNum[1]=='X')
        && sqlite3Isxdigit(zNum[2])
  ){
    u32 u = 0;
    zNum += 2;
    while( zNum[0]=='0' ) zNum++;
    for(i=0; i<8 && sqlite3Isxdigit(zNum[i]); i++){
      u = u*16 + sqlite3HexToInt(zNum[i]);
    }
    if( (u&0x80000000)==0 && sqlite3Isxdigit(zNum[i])==0 ){
      memcpy(pValue, &u, 4);
      return 1;
    }else{
      return 0;
    }
  }
#endif
  if( !sqlite3Isdigit(zNum[0]) ) return 0;
  while( zNum[0]=='0' ) zNum++;
  for(i=0; i<11 && (c = zNum[i] - '0')>=0 && c<=9; i++){
    v = v*10 + c;
  }

  /* The longest decimal representation of a 32 bit integer is 10 digits:
  **
  **             1234567890
  **     2^31 -> 2147483648
  */
  testcase( i==10 );
  if( i>10 ){
    return 0;
  }
  testcase( v-neg==2147483647 );
  if( v-neg>2147483647 ){
    return 0;
  }
  if( neg ){
    v = -v;
  }
  *pValue = (int)v;
  return 1;
}

/*
** Return a 32-bit integer value extracted from a string.  If the
** string is not an integer, just return 0.
*/
SQLITE_PRIVATE int sqlite3Atoi(const char *z){
  int x = 0;
  sqlite3GetInt32(z, &x);
  return x;
}

/*
** Decode a floating-point value into an approximate decimal
** representation.
**
** Round the decimal representation to n significant digits if
** n is positive.  Or round to -n signficant digits after the
** decimal point if n is negative.  No rounding is performed if
** n is zero.
**
** The significant digits of the decimal representation are
** stored in p->z[] which is a often (but not always) a pointer
** into the middle of p->zBuf[].  There are p->n significant digits.
** The p->z[] array is *not* zero-terminated.
*/
SQLITE_PRIVATE void sqlite3FpDecode(FpDecode *p, double r, int iRound, int mxRound){
  int i;
  u64 v;
  int e, exp = 0;
  p->isSpecial = 0;
  p->z = p->zBuf;

  /* Convert negative numbers to positive.  Deal with Infinity, 0.0, and
  ** NaN. */
  if( r<0.0 ){
    p->sign = '-';
    r = -r;
  }else if( r==0.0 ){
    p->sign = '+';
    p->n = 1;
    p->iDP = 1;
    p->z = "0";
    return;
  }else{
    p->sign = '+';
  }
  memcpy(&v,&r,8);
  e = v>>52;
  if( (e&0x7ff)==0x7ff ){
    p->isSpecial = 1 + (v!=0x7ff0000000000000LL);
    p->n = 0;
    p->iDP = 0;
    return;
  }

  /* Multiply r by powers of ten until it lands somewhere in between
  ** 1.0e+19 and 1.0e+17.
  */
  if( sqlite3Config.bUseLongDouble ){
    LONGDOUBLE_TYPE rr = r;
    if( rr>=1.0e+19 ){
      while( rr>=1.0e+119L ){ exp+=100; rr *= 1.0e-100L; }
      while( rr>=1.0e+29L  ){ exp+=10;  rr *= 1.0e-10L;  }
      while( rr>=1.0e+19L  ){ exp++;    rr *= 1.0e-1L;   }
    }else{
      while( rr<1.0e-97L   ){ exp-=100; rr *= 1.0e+100L; }
      while( rr<1.0e+07L   ){ exp-=10;  rr *= 1.0e+10L;  }
      while( rr<1.0e+17L   ){ exp--;    rr *= 1.0e+1L;   }
    }
    v = (u64)rr;
  }else{
    /* If high-precision floating point is not available using "long double",
    ** then use Dekker-style double-double computation to increase the
    ** precision.
    **
    ** The error terms on constants like 1.0e+100 computed using the
    ** decimal extension, for example as follows:
    **
    **   SELECT decimal_exp(decimal_sub('1.0e+100',decimal(1.0e+100)));
    */
    double rr[2];
    rr[0] = r;
    rr[1] = 0.0;
    if( rr[0]>9.223372036854774784e+18 ){
      while( rr[0]>9.223372036854774784e+118 ){
        exp += 100;
        dekkerMul2(rr, 1.0e-100, -1.99918998026028836196e-117);
      }
      while( rr[0]>9.223372036854774784e+28 ){
        exp += 10;
        dekkerMul2(rr, 1.0e-10, -3.6432197315497741579e-27);
      }
      while( rr[0]>9.223372036854774784e+18 ){
        exp += 1;
        dekkerMul2(rr, 1.0e-01, -5.5511151231257827021e-18);
      }
    }else{
      while( rr[0]<9.223372036854774784e-83  ){
        exp -= 100;
        dekkerMul2(rr, 1.0e+100, -1.5902891109759918046e+83);
      }
      while( rr[0]<9.223372036854774784e+07  ){
        exp -= 10;
        dekkerMul2(rr, 1.0e+10, 0.0);
      }
      while( rr[0]<9.22337203685477478e+17  ){
        exp -= 1;
        dekkerMul2(rr, 1.0e+01, 0.0);
      }
    }
    v = rr[1]<0.0 ? (u64)rr[0]-(u64)(-rr[1]) : (u64)rr[0]+(u64)rr[1];
  }


  /* Extract significant digits. */
  i = sizeof(p->zBuf)-1;
  assert( v>0 );
  while( v ){  p->zBuf[i--] = (v%10) + '0'; v /= 10; }
  assert( i>=0 && i<sizeof(p->zBuf)-1 );
  p->n = sizeof(p->zBuf) - 1 - i;
  assert( p->n>0 );
  assert( p->n<sizeof(p->zBuf) );
  p->iDP = p->n + exp;
  if( iRound<=0 ){
    iRound = p->iDP - iRound;
    if( iRound==0 && p->zBuf[i+1]>='5' ){
      iRound = 1;
      p->zBuf[i--] = '0';
      p->n++;
      p->iDP++;
    }
  }
  if( iRound>0 && (iRound<p->n || p->n>mxRound) ){
    char *z = &p->zBuf[i+1];
    if( iRound>mxRound ) iRound = mxRound;
    p->n = iRound;
    if( z[iRound]>='5' ){
      int j = iRound-1;
      while( 1 /*exit-by-break*/ ){
        z[j]++;
        if( z[j]<='9' ) break;
        z[j] = '0';
        if( j==0 ){
          p->z[i--] = '1';
          p->n++;
          p->iDP++;
          break;
        }else{
          j--;
        }
      }
    }
  }
  p->z = &p->zBuf[i+1];
  assert( i+p->n < sizeof(p->zBuf) );
  while( ALWAYS(p->n>0) && p->z[p->n-1]=='0' ){ p->n--; }
}
