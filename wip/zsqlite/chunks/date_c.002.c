**     weekday N
**     unixepoch
**     auto
**     localtime
**     utc
**     subsec
**     subsecond
**
** Return 0 on success and 1 if there is any kind of error. If the error
** is in a system call (i.e. localtime()), then an error message is written
** to context pCtx. If the error is an unrecognized modifier, no error is
** written to pCtx.
*/
static int parseModifier(
  sqlite3_context *pCtx,      /* Function context */
  const char *z,              /* The text of the modifier */
  int n,                      /* Length of zMod in bytes */
  DateTime *p,                /* The date/time value to be modified */
  int idx                     /* Parameter index of the modifier */
){
  int rc = 1;
  double r;
  switch(sqlite3UpperToLower[(u8)z[0]] ){
    case 'a': {
      /*
      **    auto
      **
      ** If rawS is available, then interpret as a julian day number, or
      ** a unix timestamp, depending on its magnitude.
      */
      if( sqlite3_stricmp(z, "auto")==0 ){
        if( idx>1 ) return 1; /* IMP: R-33611-57934 */
        autoAdjustDate(p);
        rc = 0;
      }
      break;
    }
    case 'c': {
      /*
      **    ceiling
      **
      ** Resolve day-of-month overflow by rolling forward into the next
      ** month.  As this is the default action, this modifier is really
      ** a no-op that is only included for symmetry.  See "floor".
      */
      if( sqlite3_stricmp(z, "ceiling")==0 ){
        computeJD(p);
        clearYMD_HMS_TZ(p);
        rc = 0;
        p->nFloor = 0;
      }
      break;
    }
    case 'f': {
      /*
      **    floor
      **
      ** Resolve day-of-month overflow by rolling back to the end of the
      ** previous month.
      */
      if( sqlite3_stricmp(z, "floor")==0 ){
        computeJD(p);
        p->iJD -= p->nFloor*86400000;
        clearYMD_HMS_TZ(p);
        rc = 0;
      }
      break;
    }
    case 'j': {
      /*
      **    julianday
      **
      ** Always interpret the prior number as a julian-day value.  If this
      ** is not the first modifier, or if the prior argument is not a numeric
      ** value in the allowed range of julian day numbers understood by
      ** SQLite (0..5373484.5) then the result will be NULL.
      */
      if( sqlite3_stricmp(z, "julianday")==0 ){
        if( idx>1 ) return 1;  /* IMP: R-31176-64601 */
        if( p->validJD && p->rawS ){
          rc = 0;
          p->rawS = 0;
        }
      }
      break;
    }
#ifndef SQLITE_OMIT_LOCALTIME
    case 'l': {
      /*    localtime
      **
      ** Assuming the current time value is UTC (a.k.a. GMT), shift it to
      ** show local time.
      */
      if( sqlite3_stricmp(z, "localtime")==0 && sqlite3NotPureFunc(pCtx) ){
        rc = p->isLocal ? SQLITE_OK : toLocaltime(p, pCtx);
        p->isUtc = 0;
        p->isLocal = 1;
      }
      break;
    }
#endif
    case 'u': {
      /*
      **    unixepoch
      **
      ** Treat the current value of p->s as the number of
      ** seconds since 1970.  Convert to a real julian day number.
      */
      if( sqlite3_stricmp(z, "unixepoch")==0 && p->rawS ){
        if( idx>1 ) return 1;  /* IMP: R-49255-55373 */
        r = p->s*1000.0 + 210866760000000.0;
        if( r>=0.0 && r<464269060800000.0 ){
          clearYMD_HMS_TZ(p);
          p->iJD = (sqlite3_int64)(r + 0.5);
          p->validJD = 1;
          p->rawS = 0;
          rc = 0;
        }
      }
#ifndef SQLITE_OMIT_LOCALTIME
      else if( sqlite3_stricmp(z, "utc")==0 && sqlite3NotPureFunc(pCtx) ){
        if( p->isUtc==0 ){
          i64 iOrigJD;              /* Original localtime */
          i64 iGuess;               /* Guess at the corresponding utc time */
          int cnt = 0;              /* Safety to prevent infinite loop */
          i64 iErr;                 /* Guess is off by this much */

          computeJD(p);
          iGuess = iOrigJD = p->iJD;
          iErr = 0;
          do{
            DateTime new;
            memset(&new, 0, sizeof(new));
            iGuess -= iErr;
            new.iJD = iGuess;
            new.validJD = 1;
            rc = toLocaltime(&new, pCtx);
            if( rc ) return rc;
            computeJD(&new);
            iErr = new.iJD - iOrigJD;
          }while( iErr && cnt++<3 );
          memset(p, 0, sizeof(*p));
          p->iJD = iGuess;
          p->validJD = 1;
          p->isUtc = 1;
          p->isLocal = 0;
        }
        rc = SQLITE_OK;
      }
#endif
      break;
    }
    case 'w': {
      /*
      **    weekday N
      **
      ** Move the date to the same time on the next occurrence of
      ** weekday N where 0==Sunday, 1==Monday, and so forth.  If the
      ** date is already on the appropriate weekday, this is a no-op.
      */
      if( sqlite3_strnicmp(z, "weekday ", 8)==0
               && sqlite3AtoF(&z[8], &r, sqlite3Strlen30(&z[8]), SQLITE_UTF8)>0
               && r>=0.0 && r<7.0 && (n=(int)r)==r ){
        sqlite3_int64 Z;
        computeYMD_HMS(p);
        p->tz = 0;
        p->validJD = 0;
        computeJD(p);
        Z = ((p->iJD + 129600000)/86400000) % 7;
        if( Z>n ) Z -= 7;
        p->iJD += (n - Z)*86400000;
        clearYMD_HMS_TZ(p);
        rc = 0;
      }
      break;
    }
    case 's': {
      /*
      **    start of TTTTT
      **
      ** Move the date backwards to the beginning of the current day,
      ** or month or year.
      **
      **    subsecond
      **    subsec
      **
      ** Show subsecond precision in the output of datetime() and
      ** unixepoch() and strftime('%s').
      */
      if( sqlite3_strnicmp(z, "start of ", 9)!=0 ){
        if( sqlite3_stricmp(z, "subsec")==0
         || sqlite3_stricmp(z, "subsecond")==0
        ){
          p->useSubsec = 1;
          rc = 0;
        }
        break;
      }
      if( !p->validJD && !p->validYMD && !p->validHMS ) break;
      z += 9;
      computeYMD(p);
      p->validHMS = 1;
      p->h = p->m = 0;
      p->s = 0.0;
      p->rawS = 0;
      p->tz = 0;
      p->validJD = 0;
      if( sqlite3_stricmp(z,"month")==0 ){
        p->D = 1;
        rc = 0;
      }else if( sqlite3_stricmp(z,"year")==0 ){
        p->M = 1;
        p->D = 1;
        rc = 0;
      }else if( sqlite3_stricmp(z,"day")==0 ){
        rc = 0;
      }
      break;
    }
    case '+':
    case '-':
    case '0':
    case '1':
    case '2':
    case '3':
    case '4':
    case '5':
    case '6':
    case '7':
    case '8':
    case '9': {
      double rRounder;
      int i;
      int Y,M,D,h,m,x;
      const char *z2 = z;
      char z0 = z[0];
      for(n=1; z[n]; n++){
        if( z[n]==':' ) break;
        if( sqlite3Isspace(z[n]) ) break;
        if( z[n]=='-' ){
          if( n==5 && getDigits(&z[1], "40f", &Y)==1 ) break;
          if( n==6 && getDigits(&z[1], "50f", &Y)==1 ) break;
        }
      }
      if( sqlite3AtoF(z, &r, n, SQLITE_UTF8)<=0 ){
        assert( rc==1 );
        break;
      }
      if( z[n]=='-' ){
        /* A modifier of the form (+|-)YYYY-MM-DD adds or subtracts the
        ** specified number of years, months, and days.  MM is limited to
        ** the range 0-11 and DD is limited to 0-30.
        */
        if( z0!='+' && z0!='-' ) break;  /* Must start with +/- */
        if( n==5 ){
          if( getDigits(&z[1], "40f-20a-20d", &Y, &M, &D)!=3 ) break;
        }else{
          assert( n==6 );
          if( getDigits(&z[1], "50f-20a-20d", &Y, &M, &D)!=3 ) break;
          z++;
        }
        if( M>=12 ) break;                   /* M range 0..11 */
        if( D>=31 ) break;                   /* D range 0..30 */
        computeYMD_HMS(p);
        p->validJD = 0;
        if( z0=='-' ){
          p->Y -= Y;
          p->M -= M;
          D = -D;
        }else{
          p->Y += Y;
          p->M += M;
        }
        x = p->M>0 ? (p->M-1)/12 : (p->M-12)/12;
        p->Y += x;
        p->M -= x*12;
        computeFloor(p);
        computeJD(p);
        p->validHMS = 0;
        p->validYMD = 0;
        p->iJD += (i64)D*86400000;
        if( z[11]==0 ){
          rc = 0;
          break;
        }
        if( sqlite3Isspace(z[11])
         && getDigits(&z[12], "20c:20e", &h, &m)==2
        ){
          z2 = &z[12];
          n = 2;
        }else{
          break;
        }
      }
      if( z2[n]==':' ){
        /* A modifier of the form (+|-)HH:MM:SS.FFF adds (or subtracts) the
        ** specified number of hours, minutes, seconds, and fractional seconds
        ** to the time.  The ".FFF" may be omitted.  The ":SS.FFF" may be
        ** omitted.
        */

        DateTime tx;
        sqlite3_int64 day;
        if( !sqlite3Isdigit(*z2) ) z2++;
        memset(&tx, 0, sizeof(tx));
        if( parseHhMmSs(z2, &tx) ) break;
        computeJD(&tx);
        tx.iJD -= 43200000;
        day = tx.iJD/86400000;
        tx.iJD -= day*86400000;
        if( z0=='-' ) tx.iJD = -tx.iJD;
        computeJD(p);
        clearYMD_HMS_TZ(p);
        p->iJD += tx.iJD;
        rc = 0;
        break;
      }

      /* If control reaches this point, it means the transformation is
      ** one of the forms like "+NNN days".  */
      z += n;
      while( sqlite3Isspace(*z) ) z++;
      n = sqlite3Strlen30(z);
      if( n<3 || n>10 ) break;
      if( sqlite3UpperToLower[(u8)z[n-1]]=='s' ) n--;
      computeJD(p);
      assert( rc==1 );
      rRounder = r<0 ? -0.5 : +0.5;
      p->nFloor = 0;
      for(i=0; i<ArraySize(aXformType); i++){
        if( aXformType[i].nName==n
         && sqlite3_strnicmp(aXformType[i].zName, z, n)==0
         && r>-aXformType[i].rLimit && r<aXformType[i].rLimit
        ){
          switch( i ){
            case 4: { /* Special processing to add months */
              assert( strcmp(aXformType[4].zName,"month")==0 );
              computeYMD_HMS(p);
              p->M += (int)r;
              x = p->M>0 ? (p->M-1)/12 : (p->M-12)/12;
              p->Y += x;
              p->M -= x*12;
              computeFloor(p);
              p->validJD = 0;
              r -= (int)r;
              break;
            }
            case 5: { /* Special processing to add years */
              int y = (int)r;
              assert( strcmp(aXformType[5].zName,"year")==0 );
              computeYMD_HMS(p);
              assert( p->M>=0 && p->M<=12 );
              p->Y += y;
              computeFloor(p);
              p->validJD = 0;
              r -= (int)r;
              break;
            }
          }
          computeJD(p);
          p->iJD += (sqlite3_int64)(r*1000.0*aXformType[i].rXform + rRounder);
          rc = 0;
          break;
        }
      }
      clearYMD_HMS_TZ(p);
      break;
    }
    default: {
      break;
    }
  }
  return rc;
}
