    }

    /*
    ** At this point, variables are initialized as follows:
    **
    **   flag_alternateform          TRUE if a '#' is present.
    **   flag_altform2               TRUE if a '!' is present.
    **   flag_prefix                 '+' or ' ' or zero
    **   flag_leftjustify            TRUE if a '-' is present or if the
    **                               field width was negative.
    **   flag_zeropad                TRUE if the width began with 0.
    **   flag_long                   1 for "l", 2 for "ll"
    **   width                       The specified field width.  This is
    **                               always non-negative.  Zero is the default.
    **   precision                   The specified precision.  The default
    **                               is -1.
    **   xtype                       The class of the conversion.
    **   infop                       Pointer to the appropriate info struct.
    */
    assert( width>=0 );
    assert( precision>=(-1) );
    switch( xtype ){
      case etPOINTER:
        flag_long = sizeof(char*)==sizeof(i64) ? 2 :
                     sizeof(char*)==sizeof(long int) ? 1 : 0;
        /* no break */ deliberate_fall_through
      case etORDINAL:
      case etRADIX:
        cThousand = 0;
        /* no break */ deliberate_fall_through
      case etDECIMAL:
        if( infop->flags & FLAG_SIGNED ){
          i64 v;
          if( bArgList ){
            v = getIntArg(pArgList);
          }else if( flag_long ){
            if( flag_long==2 ){
              v = va_arg(ap,i64) ;
            }else{
              v = va_arg(ap,long int);
            }
          }else{
            v = va_arg(ap,int);
          }
          if( v<0 ){
            testcase( v==SMALLEST_INT64 );
            testcase( v==(-1) );
            longvalue = ~v;
            longvalue++;
            prefix = '-';
          }else{
            longvalue = v;
            prefix = flag_prefix;
          }
        }else{
          if( bArgList ){
            longvalue = (u64)getIntArg(pArgList);
          }else if( flag_long ){
            if( flag_long==2 ){
              longvalue = va_arg(ap,u64);
            }else{
              longvalue = va_arg(ap,unsigned long int);
            }
          }else{
            longvalue = va_arg(ap,unsigned int);
          }
          prefix = 0;
        }
        if( longvalue==0 ) flag_alternateform = 0;
        if( flag_zeropad && precision<width-(prefix!=0) ){
          precision = width-(prefix!=0);
        }
        if( precision<etBUFSIZE-10-etBUFSIZE/3 ){
          nOut = etBUFSIZE;
          zOut = buf;
        }else{
          u64 n;
          n = (u64)precision + 10;
          if( cThousand ) n += precision/3;
          zOut = zExtra = printfTempBuf(pAccum, n);
          if( zOut==0 ) return;
          nOut = (int)n;
        }
        bufpt = &zOut[nOut-1];
        if( xtype==etORDINAL ){
          static const char zOrd[] = "thstndrd";
          int x = (int)(longvalue % 10);
          if( x>=4 || (longvalue/10)%10==1 ){
            x = 0;
          }
          *(--bufpt) = zOrd[x*2+1];
          *(--bufpt) = zOrd[x*2];
        }
        {
          const char *cset = &aDigits[infop->charset];
          u8 base = infop->base;
          do{                                           /* Convert to ascii */
            *(--bufpt) = cset[longvalue%base];
            longvalue = longvalue/base;
          }while( longvalue>0 );
        }
        length = (int)(&zOut[nOut-1]-bufpt);
        while( precision>length ){
          *(--bufpt) = '0';                             /* Zero pad */
          length++;
        }
        if( cThousand ){
          int nn = (length - 1)/3;  /* Number of "," to insert */
          int ix = (length - 1)%3 + 1;
          bufpt -= nn;
          for(idx=0; nn>0; idx++){
            bufpt[idx] = bufpt[idx+nn];
            ix--;
            if( ix==0 ){
              bufpt[++idx] = cThousand;
              nn--;
              ix = 3;
            }
          }
        }
        if( prefix ) *(--bufpt) = prefix;               /* Add sign */
        if( flag_alternateform && infop->prefix ){      /* Add "0" or "0x" */
          const char *pre;
          char x;
          pre = &aPrefix[infop->prefix];
          for(; (x=(*pre))!=0; pre++) *(--bufpt) = x;
        }
        length = (int)(&zOut[nOut-1]-bufpt);
        break;
      case etFLOAT:
      case etEXP:
      case etGENERIC: {
        FpDecode s;
        int iRound;
        int j;

        if( bArgList ){
          realvalue = getDoubleArg(pArgList);
        }else{
          realvalue = va_arg(ap,double);
        }
        if( precision<0 ) precision = 6;         /* Set default precision */
#ifdef SQLITE_FP_PRECISION_LIMIT
        if( precision>SQLITE_FP_PRECISION_LIMIT ){
          precision = SQLITE_FP_PRECISION_LIMIT;
        }
#endif
        if( xtype==etFLOAT ){
          iRound = -precision;
        }else if( xtype==etGENERIC ){
          if( precision==0 ) precision = 1;
          iRound = precision;
        }else{
          iRound = precision+1;
        }
        sqlite3FpDecode(&s, realvalue, iRound, flag_altform2 ? 26 : 16);
        if( s.isSpecial ){
          if( s.isSpecial==2 ){
            bufpt = flag_zeropad ? "null" : "NaN";
            length = sqlite3Strlen30(bufpt);
            break;
          }else if( flag_zeropad ){
            s.z[0] = '9';
            s.iDP = 1000;
            s.n = 1;
          }else{
            memcpy(buf, "-Inf", 5);
            bufpt = buf;
            if( s.sign=='-' ){
              /* no-op */
            }else if( flag_prefix ){
              buf[0] = flag_prefix;
            }else{
              bufpt++;
            }
            length = sqlite3Strlen30(bufpt);
            break;
          }
        }
        if( s.sign=='-' ){
          prefix = '-';
        }else{
          prefix = flag_prefix;
        }

        exp = s.iDP-1;

        /*
        ** If the field type is etGENERIC, then convert to either etEXP
        ** or etFLOAT, as appropriate.
        */
        if( xtype==etGENERIC ){
          assert( precision>0 );
          precision--;
          flag_rtz = !flag_alternateform;
          if( exp<-4 || exp>precision ){
            xtype = etEXP;
          }else{
            precision = precision - exp;
            xtype = etFLOAT;
          }
        }else{
          flag_rtz = flag_altform2;
        }
        if( xtype==etEXP ){
          e2 = 0;
        }else{
          e2 = s.iDP - 1;
        }
        bufpt = buf;
        {
          i64 szBufNeeded;           /* Size of a temporary buffer needed */
          szBufNeeded = MAX(e2,0)+(i64)precision+(i64)width+15;
          if( cThousand && e2>0 ) szBufNeeded += (e2+2)/3;
          if( szBufNeeded > etBUFSIZE ){
            bufpt = zExtra = printfTempBuf(pAccum, szBufNeeded);
            if( bufpt==0 ) return;
          }
        }
        zOut = bufpt;
        flag_dp = (precision>0 ?1:0) | flag_alternateform | flag_altform2;
        /* The sign in front of the number */
        if( prefix ){
          *(bufpt++) = prefix;
        }
        /* Digits prior to the decimal point */
        j = 0;
        if( e2<0 ){
          *(bufpt++) = '0';
        }else{
          for(; e2>=0; e2--){
            *(bufpt++) = j<s.n ? s.z[j++] : '0';
            if( cThousand && (e2%3)==0 && e2>1 ) *(bufpt++) = ',';
          }
        }
        /* The decimal point */
        if( flag_dp ){
          *(bufpt++) = '.';
        }
        /* "0" digits after the decimal point but before the first
        ** significant digit of the number */
        for(e2++; e2<0 && precision>0; precision--, e2++){
          *(bufpt++) = '0';
        }
        /* Significant digits after the decimal point */
        while( (precision--)>0 ){
          *(bufpt++) = j<s.n ? s.z[j++] : '0';
        }
        /* Remove trailing zeros and the "." if no digits follow the "." */
        if( flag_rtz && flag_dp ){
          while( bufpt[-1]=='0' ) *(--bufpt) = 0;
          assert( bufpt>zOut );
          if( bufpt[-1]=='.' ){
            if( flag_altform2 ){
              *(bufpt++) = '0';
            }else{
              *(--bufpt) = 0;
            }
          }
        }
        /* Add the "eNNN" suffix */
        if( xtype==etEXP ){
          exp = s.iDP - 1;
          *(bufpt++) = aDigits[infop->charset];
          if( exp<0 ){
            *(bufpt++) = '-'; exp = -exp;
          }else{
            *(bufpt++) = '+';
          }
          if( exp>=100 ){
            *(bufpt++) = (char)((exp/100)+'0');        /* 100's digit */
            exp %= 100;
          }
          *(bufpt++) = (char)(exp/10+'0');             /* 10's digit */
          *(bufpt++) = (char)(exp%10+'0');             /* 1's digit */
        }
        *bufpt = 0;

        /* The converted number is in buf[] and zero terminated. Output it.
        ** Note that the number is in the usual order, not reversed as with
        ** integer conversions. */
        length = (int)(bufpt-zOut);
        bufpt = zOut;

        /* Special case:  Add leading zeros if the flag_zeropad flag is
        ** set and we are not left justified */
        if( flag_zeropad && !flag_leftjustify && length < width){
          int i;
          int nPad = width - length;
          for(i=width; i>=nPad; i--){
            bufpt[i] = bufpt[i-nPad];
          }
          i = prefix!=0;
          while( nPad-- ) bufpt[i++] = '0';
          length = width;
        }
        break;
      }
      case etSIZE:
        if( !bArgList ){
          *(va_arg(ap,int*)) = pAccum->nChar;
        }
        length = width = 0;
        break;
      case etPERCENT:
        buf[0] = '%';
        bufpt = buf;
        length = 1;
        break;
      case etCHARX:
        if( bArgList ){
          bufpt = getTextArg(pArgList);
          length = 1;
          if( bufpt ){
            buf[0] = c = *(bufpt++);
            if( (c&0xc0)==0xc0 ){
              while( length<4 && (bufpt[0]&0xc0)==0x80 ){
                buf[length++] = *(bufpt++);
              }
            }
          }else{
            buf[0] = 0;
          }
        }else{
          unsigned int ch = va_arg(ap,unsigned int);
          if( ch<0x00080 ){
            buf[0] = ch & 0xff;
            length = 1;
          }else if( ch<0x00800 ){
            buf[0] = 0xc0 + (u8)((ch>>6)&0x1f);
            buf[1] = 0x80 + (u8)(ch & 0x3f);
            length = 2;
          }else if( ch<0x10000 ){
            buf[0] = 0xe0 + (u8)((ch>>12)&0x0f);
            buf[1] = 0x80 + (u8)((ch>>6) & 0x3f);
            buf[2] = 0x80 + (u8)(ch & 0x3f);
            length = 3;
          }else{
            buf[0] = 0xf0 + (u8)((ch>>18) & 0x07);
            buf[1] = 0x80 + (u8)((ch>>12) & 0x3f);
            buf[2] = 0x80 + (u8)((ch>>6) & 0x3f);
            buf[3] = 0x80 + (u8)(ch & 0x3f);
            length = 4;
          }
        }
        if( precision>1 ){
          i64 nPrior = 1;
          width -= precision-1;
          if( width>1 && !flag_leftjustify ){
            sqlite3_str_appendchar(pAccum, width-1, ' ');
            width = 0;
          }
          sqlite3_str_append(pAccum, buf, length);
          precision--;
          while( precision > 1 ){
            i64 nCopyBytes;
            if( nPrior > precision-1 ) nPrior = precision - 1;
            nCopyBytes = length*nPrior;
            if( nCopyBytes + pAccum->nChar >= pAccum->nAlloc ){
              sqlite3StrAccumEnlarge(pAccum, nCopyBytes);
            }
            if( pAccum->accError ) break;
            sqlite3_str_append(pAccum,
                 &pAccum->zText[pAccum->nChar-nCopyBytes], nCopyBytes);
            precision -= nPrior;
            nPrior *= 2;
          }
        }
        bufpt = buf;
        flag_altform2 = 1;
        goto adjust_width_for_utf8;
      case etSTRING:
      case etDYNSTRING:
        if( bArgList ){
          bufpt = getTextArg(pArgList);
          xtype = etSTRING;
        }else{
          bufpt = va_arg(ap,char*);
        }
        if( bufpt==0 ){
          bufpt = "";
        }else if( xtype==etDYNSTRING ){
          if( pAccum->nChar==0
           && pAccum->mxAlloc
           && width==0
           && precision<0
           && pAccum->accError==0
          ){
            /* Special optimization for sqlite3_mprintf("%z..."):
            ** Extend an existing memory allocation rather than creating
            ** a new one. */
            assert( (pAccum->printfFlags&SQLITE_PRINTF_MALLOCED)==0 );
            pAccum->zText = bufpt;
            pAccum->nAlloc = sqlite3DbMallocSize(pAccum->db, bufpt);
            pAccum->nChar = 0x7fffffff & (int)strlen(bufpt);
            pAccum->printfFlags |= SQLITE_PRINTF_MALLOCED;
            length = 0;
            break;
          }
          zExtra = bufpt;
        }
        if( precision>=0 ){
          if( flag_altform2 ){
            /* Set length to the number of bytes needed in order to display
            ** precision characters */
            unsigned char *z = (unsigned char*)bufpt;
            while( precision-- > 0 && z[0] ){
              SQLITE_SKIP_UTF8(z);
            }
            length = (int)(z - (unsigned char*)bufpt);
          }else{
            for(length=0; length<precision && bufpt[length]; length++){}
          }
        }else{
          length = 0x7fffffff & (int)strlen(bufpt);
        }
      adjust_width_for_utf8:
        if( flag_altform2 && width>0 ){
          /* Adjust width to account for extra bytes in UTF-8 characters */
          int ii = length - 1;
          while( ii>=0 ) if( (bufpt[ii--] & 0xc0)==0x80 ) width++;
        }
        break;
      case etSQLESCAPE:           /* %q: Escape ' characters */
      case etSQLESCAPE2:          /* %Q: Escape ' and enclose in '...' */
      case etSQLESCAPE3: {        /* %w: Escape " characters */
        i64 i, j, k, n;
        int needQuote, isnull;
        char ch;
        char q = ((xtype==etSQLESCAPE3)?'"':'\'');   /* Quote character */
        char *escarg;

        if( bArgList ){
          escarg = getTextArg(pArgList);
        }else{
          escarg = va_arg(ap,char*);
        }
        isnull = escarg==0;
        if( isnull ) escarg = (xtype==etSQLESCAPE2 ? "NULL" : "(NULL)");
        /* For %q, %Q, and %w, the precision is the number of bytes (or
        ** characters if the ! flags is present) to use from the input.
        ** Because of the extra quoting characters inserted, the number
        ** of output characters may be larger than the precision.
        */
        k = precision;
        for(i=n=0; k!=0 && (ch=escarg[i])!=0; i++, k--){
          if( ch==q )  n++;
          if( flag_altform2 && (ch&0xc0)==0xc0 ){
            while( (escarg[i+1]&0xc0)==0x80 ){ i++; }
          }
        }
        needQuote = !isnull && xtype==etSQLESCAPE2;
        n += i + 3;
        if( n>etBUFSIZE ){
          bufpt = zExtra = printfTempBuf(pAccum, n);
          if( bufpt==0 ) return;
        }else{
          bufpt = buf;
        }
        j = 0;
        if( needQuote ) bufpt[j++] = q;
        k = i;
        for(i=0; i<k; i++){
          bufpt[j++] = ch = escarg[i];
          if( ch==q ) bufpt[j++] = ch;
        }
        if( needQuote ) bufpt[j++] = q;
        bufpt[j] = 0;
        length = j;
        goto adjust_width_for_utf8;
      }
      case etTOKEN: {
        if( (pAccum->printfFlags & SQLITE_PRINTF_INTERNAL)==0 ) return;
        if( flag_alternateform ){
          /* %#T means an Expr pointer that uses Expr.u.zToken */
          Expr *pExpr = va_arg(ap,Expr*);
          if( ALWAYS(pExpr) && ALWAYS(!ExprHasProperty(pExpr,EP_IntValue)) ){
            sqlite3_str_appendall(pAccum, (const char*)pExpr->u.zToken);
            sqlite3RecordErrorOffsetOfExpr(pAccum->db, pExpr);
          }
        }else{
          /* %T means a Token pointer */
          Token *pToken = va_arg(ap, Token*);
          assert( bArgList==0 );
          if( pToken && pToken->n ){
            sqlite3_str_append(pAccum, (const char*)pToken->z, pToken->n);
            sqlite3RecordErrorByteOffset(pAccum->db, pToken->z);
          }
        }
        length = width = 0;
        break;
      }
      case etSRCITEM: {
        SrcItem *pItem;
        if( (pAccum->printfFlags & SQLITE_PRINTF_INTERNAL)==0 ) return;
        pItem = va_arg(ap, SrcItem*);
        assert( bArgList==0 );
        if( pItem->zAlias && !flag_altform2 ){
          sqlite3_str_appendall(pAccum, pItem->zAlias);
        }else if( pItem->zName ){
          if( pItem->zDatabase ){
            sqlite3_str_appendall(pAccum, pItem->zDatabase);
            sqlite3_str_append(pAccum, ".", 1);
          }
          sqlite3_str_appendall(pAccum, pItem->zName);
        }else if( pItem->zAlias ){
          sqlite3_str_appendall(pAccum, pItem->zAlias);
        }else{
          Select *pSel = pItem->pSelect;
          assert( pSel!=0 ); /* Because of tag-20240424-1 */
          if( pSel->selFlags & SF_NestedFrom ){
            sqlite3_str_appendf(pAccum, "(join-%u)", pSel->selId);
          }else if( pSel->selFlags & SF_MultiValue ){
            assert( !pItem->fg.isTabFunc && !pItem->fg.isIndexedBy );
            sqlite3_str_appendf(pAccum, "%u-ROW VALUES CLAUSE",
                                pItem->u1.nRow);
          }else{
            sqlite3_str_appendf(pAccum, "(subquery-%u)", pSel->selId);
          }
        }
        length = width = 0;
        break;
      }
      default: {
        assert( xtype==etINVALID );
        return;
      }
    }/* End switch over the format type */
    /*
    ** The text of the conversion is pointed to by "bufpt" and is
    ** "length" characters long.  The field width is "width".  Do
    ** the output.  Both length and width are in bytes, not characters,
    ** at this point.  If the "!" flag was present on string conversions
    ** indicating that width and precision should be expressed in characters,
    ** then the values have been translated prior to reaching this point.
    */
    width -= length;
    if( width>0 ){
      if( !flag_leftjustify ) sqlite3_str_appendchar(pAccum, width, ' ');
      sqlite3_str_append(pAccum, bufpt, length);
      if( flag_leftjustify ) sqlite3_str_appendchar(pAccum, width, ' ');
    }else{
      sqlite3_str_append(pAccum, bufpt, length);
    }

    if( zExtra ){
      sqlite3DbFree(pAccum->db, zExtra);
      zExtra = 0;
    }
  }/* End for loop over the format string */
} /* End of function */
