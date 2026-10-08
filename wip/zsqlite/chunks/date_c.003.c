
/*
** Process time function arguments.  argv[0] is a date-time stamp.
** argv[1] and following are modifiers.  Parse them all and write
** the resulting time into the DateTime structure p.  Return 0
** on success and 1 if there are any errors.
**
** If there are zero parameters (if even argv[0] is undefined)
** then assume a default value of "now" for argv[0].
*/
static int isDate(
  sqlite3_context *context,
  int argc,
  sqlite3_value **argv,
  DateTime *p
){
  int i, n;
  const unsigned char *z;
  int eType;
  memset(p, 0, sizeof(*p));
  if( argc==0 ){
    if( !sqlite3NotPureFunc(context) ) return 1;
    return setDateTimeToCurrent(context, p);
  }
  if( (eType = sqlite3_value_type(argv[0]))==SQLITE_FLOAT
                   || eType==SQLITE_INTEGER ){
    setRawDateNumber(p, sqlite3_value_double(argv[0]));
  }else{
    z = sqlite3_value_text(argv[0]);
    if( !z || parseDateOrTime(context, (char*)z, p) ){
      return 1;
    }
  }
  for(i=1; i<argc; i++){
    z = sqlite3_value_text(argv[i]);
    n = sqlite3_value_bytes(argv[i]);
    if( z==0 || parseModifier(context, (char*)z, n, p, i) ) return 1;
  }
  computeJD(p);
  if( p->isError || !validJulianDay(p->iJD) ) return 1;
  if( argc==1 && p->validYMD && p->D>28 ){
    /* Make sure a YYYY-MM-DD is normalized.
    ** Example: 2023-02-31 -> 2023-03-03 */
    assert( p->validJD );
    p->validYMD = 0;
  }
  return 0;
}


/*
** The following routines implement the various date and time functions
** of SQLite.
*/

/*
**    julianday( TIMESTRING, MOD, MOD, ...)
**
** Return the julian day number of the date specified in the arguments
*/
static void juliandayFunc(
  sqlite3_context *context,
  int argc,
  sqlite3_value **argv
){
  DateTime x;
  if( isDate(context, argc, argv, &x)==0 ){
    computeJD(&x);
    sqlite3_result_double(context, x.iJD/86400000.0);
  }
}

/*
**    unixepoch( TIMESTRING, MOD, MOD, ...)
**
** Return the number of seconds (including fractional seconds) since
** the unix epoch of 1970-01-01 00:00:00 GMT.
*/
static void unixepochFunc(
  sqlite3_context *context,
  int argc,
  sqlite3_value **argv
){
  DateTime x;
  if( isDate(context, argc, argv, &x)==0 ){
    computeJD(&x);
    if( x.useSubsec ){
      sqlite3_result_double(context, (x.iJD - 21086676*(i64)10000000)/1000.0);
    }else{
      sqlite3_result_int64(context, x.iJD/1000 - 21086676*(i64)10000);
    }
  }
}

/*
**    datetime( TIMESTRING, MOD, MOD, ...)
**
** Return YYYY-MM-DD HH:MM:SS
*/
static void datetimeFunc(
  sqlite3_context *context,
  int argc,
  sqlite3_value **argv
){
  DateTime x;
  if( isDate(context, argc, argv, &x)==0 ){
    int Y, s, n;
    char zBuf[32];
    computeYMD_HMS(&x);
    Y = x.Y;
    if( Y<0 ) Y = -Y;
    zBuf[1] = '0' + (Y/1000)%10;
    zBuf[2] = '0' + (Y/100)%10;
    zBuf[3] = '0' + (Y/10)%10;
    zBuf[4] = '0' + (Y)%10;
    zBuf[5] = '-';
    zBuf[6] = '0' + (x.M/10)%10;
    zBuf[7] = '0' + (x.M)%10;
    zBuf[8] = '-';
    zBuf[9] = '0' + (x.D/10)%10;
    zBuf[10] = '0' + (x.D)%10;
    zBuf[11] = ' ';
    zBuf[12] = '0' + (x.h/10)%10;
    zBuf[13] = '0' + (x.h)%10;
    zBuf[14] = ':';
    zBuf[15] = '0' + (x.m/10)%10;
    zBuf[16] = '0' + (x.m)%10;
    zBuf[17] = ':';
    if( x.useSubsec ){
      s = (int)(1000.0*x.s + 0.5);
      zBuf[18] = '0' + (s/10000)%10;
      zBuf[19] = '0' + (s/1000)%10;
      zBuf[20] = '.';
      zBuf[21] = '0' + (s/100)%10;
      zBuf[22] = '0' + (s/10)%10;
      zBuf[23] = '0' + (s)%10;
      zBuf[24] = 0;
      n = 24;
    }else{
      s = (int)x.s;
      zBuf[18] = '0' + (s/10)%10;
      zBuf[19] = '0' + (s)%10;
      zBuf[20] = 0;
      n = 20;
    }
    if( x.Y<0 ){
      zBuf[0] = '-';
      sqlite3_result_text(context, zBuf, n, SQLITE_TRANSIENT);
    }else{
      sqlite3_result_text(context, &zBuf[1], n-1, SQLITE_TRANSIENT);
    }
  }
}

/*
**    time( TIMESTRING, MOD, MOD, ...)
**
** Return HH:MM:SS
*/
static void timeFunc(
  sqlite3_context *context,
  int argc,
  sqlite3_value **argv
){
  DateTime x;
  if( isDate(context, argc, argv, &x)==0 ){
    int s, n;
    char zBuf[16];
    computeHMS(&x);
    zBuf[0] = '0' + (x.h/10)%10;
    zBuf[1] = '0' + (x.h)%10;
    zBuf[2] = ':';
    zBuf[3] = '0' + (x.m/10)%10;
    zBuf[4] = '0' + (x.m)%10;
    zBuf[5] = ':';
    if( x.useSubsec ){
      s = (int)(1000.0*x.s + 0.5);
      zBuf[6] = '0' + (s/10000)%10;
      zBuf[7] = '0' + (s/1000)%10;
      zBuf[8] = '.';
      zBuf[9] = '0' + (s/100)%10;
      zBuf[10] = '0' + (s/10)%10;
      zBuf[11] = '0' + (s)%10;
      zBuf[12] = 0;
      n = 12;
    }else{
      s = (int)x.s;
      zBuf[6] = '0' + (s/10)%10;
      zBuf[7] = '0' + (s)%10;
      zBuf[8] = 0;
      n = 8;
    }
    sqlite3_result_text(context, zBuf, n, SQLITE_TRANSIENT);
  }
}

/*
**    date( TIMESTRING, MOD, MOD, ...)
**
** Return YYYY-MM-DD
*/
static void dateFunc(
  sqlite3_context *context,
  int argc,
  sqlite3_value **argv
){
  DateTime x;
  if( isDate(context, argc, argv, &x)==0 ){
    int Y;
    char zBuf[16];
    computeYMD(&x);
    Y = x.Y;
    if( Y<0 ) Y = -Y;
    zBuf[1] = '0' + (Y/1000)%10;
    zBuf[2] = '0' + (Y/100)%10;
    zBuf[3] = '0' + (Y/10)%10;
    zBuf[4] = '0' + (Y)%10;
    zBuf[5] = '-';
    zBuf[6] = '0' + (x.M/10)%10;
    zBuf[7] = '0' + (x.M)%10;
    zBuf[8] = '-';
    zBuf[9] = '0' + (x.D/10)%10;
    zBuf[10] = '0' + (x.D)%10;
    zBuf[11] = 0;
    if( x.Y<0 ){
      zBuf[0] = '-';
      sqlite3_result_text(context, zBuf, 11, SQLITE_TRANSIENT);
    }else{
      sqlite3_result_text(context, &zBuf[1], 10, SQLITE_TRANSIENT);
    }
  }
}

/*
** Compute the number of days after the most recent January 1.
**
** In other words, compute the zero-based day number for the
** current year:
**
**   Jan01 = 0,  Jan02 = 1, ..., Jan31 = 30, Feb01 = 31, ...
**   Dec31 = 364 or 365.
*/
static int daysAfterJan01(DateTime *pDate){
  DateTime jan01 = *pDate;
  assert( jan01.validYMD );
  assert( jan01.validHMS );
  assert( pDate->validJD );
  jan01.validJD = 0;
  jan01.M = 1;
  jan01.D = 1;
  computeJD(&jan01);
  return (int)((pDate->iJD-jan01.iJD+43200000)/86400000);
}

/*
** Return the number of days after the most recent Monday.
**
** In other words, return the day of the week according
** to this code:
**
**   0=Monday, 1=Tuesday, 2=Wednesday, ..., 6=Sunday.
*/
static int daysAfterMonday(DateTime *pDate){
  assert( pDate->validJD );
  return (int)((pDate->iJD+43200000)/86400000) % 7;
}

/*
** Return the number of days after the most recent Sunday.
**
** In other words, return the day of the week according
** to this code:
**
**   0=Sunday, 1=Monday, 2=Tues, ..., 6=Saturday
*/
static int daysAfterSunday(DateTime *pDate){
  assert( pDate->validJD );
  return (int)((pDate->iJD+129600000)/86400000) % 7;
}

/*
**    strftime( FORMAT, TIMESTRING, MOD, MOD, ...)
**
** Return a string described by FORMAT.  Conversions as follows:
**
**   %d  day of month  01-31
**   %e  day of month  1-31
**   %f  ** fractional seconds  SS.SSS
**   %F  ISO date.  YYYY-MM-DD
**   %G  ISO year corresponding to %V 0000-9999.
**   %g  2-digit ISO year corresponding to %V 00-99
**   %H  hour 00-24
**   %k  hour  0-24  (leading zero converted to space)
**   %I  hour 01-12
**   %j  day of year 001-366
**   %J  ** julian day number
**   %l  hour  1-12  (leading zero converted to space)
**   %m  month 01-12
**   %M  minute 00-59
**   %p  "am" or "pm"
**   %P  "AM" or "PM"
**   %R  time as HH:MM
**   %s  seconds since 1970-01-01
**   %S  seconds 00-59
**   %T  time as HH:MM:SS
**   %u  day of week 1-7  Monday==1, Sunday==7
**   %w  day of week 0-6  Sunday==0, Monday==1
**   %U  week of year 00-53  (First Sunday is start of week 01)
**   %V  week of year 01-53  (First week containing Thursday is week 01)
**   %W  week of year 00-53  (First Monday is start of week 01)
**   %Y  year 0000-9999
**   %%  %
*/
static void strftimeFunc(
  sqlite3_context *context,
  int argc,
  sqlite3_value **argv
){
  DateTime x;
  size_t i,j;
  sqlite3 *db;
  const char *zFmt;
  sqlite3_str sRes;


  if( argc==0 ) return;
  zFmt = (const char*)sqlite3_value_text(argv[0]);
  if( zFmt==0 || isDate(context, argc-1, argv+1, &x) ) return;
  db = sqlite3_context_db_handle(context);
  sqlite3StrAccumInit(&sRes, 0, 0, 0, db->aLimit[SQLITE_LIMIT_LENGTH]);

  computeJD(&x);
  computeYMD_HMS(&x);
  for(i=j=0; zFmt[i]; i++){
    char cf;
    if( zFmt[i]!='%' ) continue;
    if( j<i ) sqlite3_str_append(&sRes, zFmt+j, (int)(i-j));
    i++;
    j = i + 1;
    cf = zFmt[i];
    switch( cf ){
      case 'd':  /* Fall thru */
      case 'e': {
        sqlite3_str_appendf(&sRes, cf=='d' ? "%02d" : "%2d", x.D);
        break;
      }
      case 'f': {  /* Fractional seconds.  (Non-standard) */
        double s = x.s;
        if( s>59.999 ) s = 59.999;
        sqlite3_str_appendf(&sRes, "%06.3f", s);
        break;
      }
      case 'F': {
        sqlite3_str_appendf(&sRes, "%04d-%02d-%02d", x.Y, x.M, x.D);
        break;
      }
      case 'G': /* Fall thru */
      case 'g': {
        DateTime y = x;
        assert( y.validJD );
        /* Move y so that it is the Thursday in the same week as x */
        y.iJD += (3 - daysAfterMonday(&x))*86400000;
        y.validYMD = 0;
        computeYMD(&y);
        if( cf=='g' ){
          sqlite3_str_appendf(&sRes, "%02d", y.Y%100);
        }else{
          sqlite3_str_appendf(&sRes, "%04d", y.Y);
        }
        break;
      }
      case 'H':
      case 'k': {
        sqlite3_str_appendf(&sRes, cf=='H' ? "%02d" : "%2d", x.h);
        break;
      }
      case 'I': /* Fall thru */
      case 'l': {
        int h = x.h;
        if( h>12 ) h -= 12;
        if( h==0 ) h = 12;
        sqlite3_str_appendf(&sRes, cf=='I' ? "%02d" : "%2d", h);
        break;
      }
      case 'j': {  /* Day of year.  Jan01==1, Jan02==2, and so forth */
        sqlite3_str_appendf(&sRes,"%03d",daysAfterJan01(&x)+1);
        break;
      }
      case 'J': {  /* Julian day number.  (Non-standard) */
        sqlite3_str_appendf(&sRes,"%.16g",x.iJD/86400000.0);
        break;
      }
      case 'm': {
        sqlite3_str_appendf(&sRes,"%02d",x.M);
        break;
      }
      case 'M': {
        sqlite3_str_appendf(&sRes,"%02d",x.m);
        break;
      }
      case 'p': /* Fall thru */
      case 'P': {
        if( x.h>=12 ){
          sqlite3_str_append(&sRes, cf=='p' ? "PM" : "pm", 2);
        }else{
          sqlite3_str_append(&sRes, cf=='p' ? "AM" : "am", 2);
        }
        break;
      }
      case 'R': {
        sqlite3_str_appendf(&sRes, "%02d:%02d", x.h, x.m);
        break;
      }
      case 's': {
        if( x.useSubsec ){
          sqlite3_str_appendf(&sRes,"%.3f",
                (x.iJD - 21086676*(i64)10000000)/1000.0);
        }else{
          i64 iS = (i64)(x.iJD/1000 - 21086676*(i64)10000);
          sqlite3_str_appendf(&sRes,"%lld",iS);
        }
        break;
      }
      case 'S': {
        sqlite3_str_appendf(&sRes,"%02d",(int)x.s);
        break;
      }
      case 'T': {
        sqlite3_str_appendf(&sRes,"%02d:%02d:%02d", x.h, x.m, (int)x.s);
        break;
      }
      case 'u':    /* Day of week.  1 to 7.  Monday==1, Sunday==7 */
      case 'w': {  /* Day of week.  0 to 6.  Sunday==0, Monday==1 */
        char c = (char)daysAfterSunday(&x) + '0';
        if( c=='0' && cf=='u' ) c = '7';
        sqlite3_str_appendchar(&sRes, 1, c);
        break;
      }
      case 'U': {  /* Week num. 00-53. First Sun of the year is week 01 */
        sqlite3_str_appendf(&sRes,"%02d",
              (daysAfterJan01(&x)-daysAfterSunday(&x)+7)/7);
        break;
      }
      case 'V': {  /* Week num. 01-53. First week with a Thur is week 01 */
        DateTime y = x;
        /* Adjust y so that is the Thursday in the same week as x */
        assert( y.validJD );
        y.iJD += (3 - daysAfterMonday(&x))*86400000;
        y.validYMD = 0;
        computeYMD(&y);
        sqlite3_str_appendf(&sRes,"%02d", daysAfterJan01(&y)/7+1);
        break;
      }
      case 'W': {  /* Week num. 00-53. First Mon of the year is week 01 */
        sqlite3_str_appendf(&sRes,"%02d",
           (daysAfterJan01(&x)-daysAfterMonday(&x)+7)/7);
        break;
      }
      case 'Y': {
        sqlite3_str_appendf(&sRes,"%04d",x.Y);
        break;
      }
      case '%': {
        sqlite3_str_appendchar(&sRes, 1, '%');
        break;
      }
      default: {
        sqlite3_str_reset(&sRes);
        return;
      }
    }
  }
  if( j<i ) sqlite3_str_append(&sRes, zFmt+j, (int)(i-j));
  sqlite3ResultStrAccum(context, &sRes);
}
