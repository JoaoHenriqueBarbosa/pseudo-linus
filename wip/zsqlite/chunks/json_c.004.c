
/*
** Translate a single element of JSON text at pParse->zJson[i] into
** its equivalent binary JSONB representation.  Append the translation into
** pParse->aBlob[] beginning at pParse->nBlob.  The size of
** pParse->aBlob[] is increased as necessary.
**
** Return the index of the first character past the end of the element parsed,
** or one of the following special result codes:
**
**      0    End of input
**     -1    Syntax error or OOM
**     -2    '}' seen   \
**     -3    ']' seen    \___  For these returns, pParse->iErr is set to
**     -4    ',' seen    /     the index in zJson[] of the seen character
**     -5    ':' seen   /
*/
static int jsonTranslateTextToBlob(JsonParse *pParse, u32 i){
  char c;
  u32 j;
  u32 iThis, iStart;
  int x;
  u8 t;
  const char *z = pParse->zJson;
json_parse_restart:
  switch( (u8)z[i] ){
  case '{': {
    /* Parse object */
    iThis = pParse->nBlob;
    jsonBlobAppendNode(pParse, JSONB_OBJECT, pParse->nJson-i, 0);
    if( ++pParse->iDepth > JSON_MAX_DEPTH ){
      pParse->iErr = i;
      return -1;
    }
    iStart = pParse->nBlob;
    for(j=i+1;;j++){
      u32 iBlob = pParse->nBlob;
      x = jsonTranslateTextToBlob(pParse, j);
      if( x<=0 ){
        int op;
        if( x==(-2) ){
          j = pParse->iErr;
          if( pParse->nBlob!=(u32)iStart ) pParse->hasNonstd = 1;
          break;
        }
        j += json5Whitespace(&z[j]);
        op = JSONB_TEXT;
        if( sqlite3JsonId1(z[j])
         || (z[j]=='\\' && jsonIs4HexB(&z[j+1], &op))
        ){
          int k = j+1;
          while( (sqlite3JsonId2(z[k]) && json5Whitespace(&z[k])==0)
            || (z[k]=='\\' && jsonIs4HexB(&z[k+1], &op))
          ){
            k++;
          }
          assert( iBlob==pParse->nBlob );
          jsonBlobAppendNode(pParse, op, k-j, &z[j]);
          pParse->hasNonstd = 1;
          x = k;
        }else{
          if( x!=-1 ) pParse->iErr = j;
          return -1;
        }
      }
      if( pParse->oom ) return -1;
      t = pParse->aBlob[iBlob] & 0x0f;
      if( t<JSONB_TEXT || t>JSONB_TEXTRAW ){
        pParse->iErr = j;
        return -1;
      }
      j = x;
      if( z[j]==':' ){
        j++;
      }else{
        if( jsonIsspace(z[j]) ){
          /* strspn() is not helpful here */
          do{ j++; }while( jsonIsspace(z[j]) );
          if( z[j]==':' ){
            j++;
            goto parse_object_value;
          }
        }
        x = jsonTranslateTextToBlob(pParse, j);
        if( x!=(-5) ){
          if( x!=(-1) ) pParse->iErr = j;
          return -1;
        }
        j = pParse->iErr+1;
      }
    parse_object_value:
      x = jsonTranslateTextToBlob(pParse, j);
      if( x<=0 ){
        if( x!=(-1) ) pParse->iErr = j;
        return -1;
      }
      j = x;
      if( z[j]==',' ){
        continue;
      }else if( z[j]=='}' ){
        break;
      }else{
        if( jsonIsspace(z[j]) ){
          j += 1 + (u32)strspn(&z[j+1], jsonSpaces);
          if( z[j]==',' ){
            continue;
          }else if( z[j]=='}' ){
            break;
          }
        }
        x = jsonTranslateTextToBlob(pParse, j);
        if( x==(-4) ){
          j = pParse->iErr;
          continue;
        }
        if( x==(-2) ){
          j = pParse->iErr;
          break;
        }
      }
      pParse->iErr = j;
      return -1;
    }
    jsonBlobChangePayloadSize(pParse, iThis, pParse->nBlob - iStart);
    pParse->iDepth--;
    return j+1;
  }
  case '[': {
    /* Parse array */
    iThis = pParse->nBlob;
    assert( i<=(u32)pParse->nJson );
    jsonBlobAppendNode(pParse, JSONB_ARRAY, pParse->nJson - i, 0);
    iStart = pParse->nBlob;
    if( pParse->oom ) return -1;
    if( ++pParse->iDepth > JSON_MAX_DEPTH ){
      pParse->iErr = i;
      return -1;
    }
    for(j=i+1;;j++){
      x = jsonTranslateTextToBlob(pParse, j);
      if( x<=0 ){
        if( x==(-3) ){
          j = pParse->iErr;
          if( pParse->nBlob!=iStart ) pParse->hasNonstd = 1;
          break;
        }
        if( x!=(-1) ) pParse->iErr = j;
        return -1;
      }
      j = x;
      if( z[j]==',' ){
        continue;
      }else if( z[j]==']' ){
        break;
      }else{
        if( jsonIsspace(z[j]) ){
          j += 1 + (u32)strspn(&z[j+1], jsonSpaces);
          if( z[j]==',' ){
            continue;
          }else if( z[j]==']' ){
            break;
          }
        }
        x = jsonTranslateTextToBlob(pParse, j);
        if( x==(-4) ){
          j = pParse->iErr;
          continue;
        }
        if( x==(-3) ){
          j = pParse->iErr;
          break;
        }
      }
      pParse->iErr = j;
      return -1;
    }
    jsonBlobChangePayloadSize(pParse, iThis, pParse->nBlob - iStart);
    pParse->iDepth--;
    return j+1;
  }
  case '\'': {
    u8 opcode;
    char cDelim;
    pParse->hasNonstd = 1;
    opcode = JSONB_TEXT;
    goto parse_string;
  case '"':
    /* Parse string */
    opcode = JSONB_TEXT;
  parse_string:
    cDelim = z[i];
    j = i+1;
    while( 1 /*exit-by-break*/ ){
      if( jsonIsOk[(u8)z[j]] ){
        if( !jsonIsOk[(u8)z[j+1]] ){
          j += 1;
        }else if( !jsonIsOk[(u8)z[j+2]] ){
          j += 2;
        }else{
          j += 3;
          continue;
        }
      }
      c = z[j];
      if( c==cDelim ){
        break;
      }else if( c=='\\' ){
        c = z[++j];
        if( c=='"' || c=='\\' || c=='/' || c=='b' || c=='f'
           || c=='n' || c=='r' || c=='t'
           || (c=='u' && jsonIs4Hex(&z[j+1])) ){
          if( opcode==JSONB_TEXT ) opcode = JSONB_TEXTJ;
        }else if( c=='\'' || c=='0' || c=='v' || c=='\n'
           || (0xe2==(u8)c && 0x80==(u8)z[j+1]
                && (0xa8==(u8)z[j+2] || 0xa9==(u8)z[j+2]))
           || (c=='x' && jsonIs2Hex(&z[j+1])) ){
          opcode = JSONB_TEXT5;
          pParse->hasNonstd = 1;
        }else if( c=='\r' ){
          if( z[j+1]=='\n' ) j++;
          opcode = JSONB_TEXT5;
          pParse->hasNonstd = 1;
        }else{
          pParse->iErr = j;
          return -1;
        }
      }else if( c<=0x1f ){
        if( c==0 ){
          pParse->iErr = j;
          return -1;
        }
        /* Control characters are not allowed in canonical JSON string
        ** literals, but are allowed in JSON5 string literals. */
        opcode = JSONB_TEXT5;
        pParse->hasNonstd = 1;
      }else if( c=='"' ){
        opcode = JSONB_TEXT5;
      }
      j++;
    }
    jsonBlobAppendNode(pParse, opcode, j-1-i, &z[i+1]);
    return j+1;
  }
  case 't': {
    if( strncmp(z+i,"true",4)==0 && !sqlite3Isalnum(z[i+4]) ){
      jsonBlobAppendOneByte(pParse, JSONB_TRUE);
      return i+4;
    }
    pParse->iErr = i;
    return -1;
  }
  case 'f': {
    if( strncmp(z+i,"false",5)==0 && !sqlite3Isalnum(z[i+5]) ){
      jsonBlobAppendOneByte(pParse, JSONB_FALSE);
      return i+5;
    }
    pParse->iErr = i;
    return -1;
  }
  case '+': {
    u8 seenE;
    pParse->hasNonstd = 1;
    t = 0x00;            /* Bit 0x01:  JSON5.   Bit 0x02:  FLOAT */
    goto parse_number;
  case '.':
    if( sqlite3Isdigit(z[i+1]) ){
      pParse->hasNonstd = 1;
      t = 0x03;          /* Bit 0x01:  JSON5.   Bit 0x02:  FLOAT */
      seenE = 0;
      goto parse_number_2;
    }
    pParse->iErr = i;
    return -1;
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
  case '9':
    /* Parse number */
    t = 0x00;            /* Bit 0x01:  JSON5.   Bit 0x02:  FLOAT */
  parse_number:
    seenE = 0;
    assert( '-' < '0' );
    assert( '+' < '0' );
    assert( '.' < '0' );
    c = z[i];

    if( c<='0' ){
      if( c=='0' ){
        if( (z[i+1]=='x' || z[i+1]=='X') && sqlite3Isxdigit(z[i+2]) ){
          assert( t==0x00 );
          pParse->hasNonstd = 1;
          t = 0x01;
          for(j=i+3; sqlite3Isxdigit(z[j]); j++){}
          goto parse_number_finish;
        }else if( sqlite3Isdigit(z[i+1]) ){
          pParse->iErr = i+1;
          return -1;
        }
      }else{
        if( !sqlite3Isdigit(z[i+1]) ){
          /* JSON5 allows for "+Infinity" and "-Infinity" using exactly
          ** that case.  SQLite also allows these in any case and it allows
          ** "+inf" and "-inf". */
          if( (z[i+1]=='I' || z[i+1]=='i')
           && sqlite3StrNICmp(&z[i+1], "inf",3)==0
          ){
            pParse->hasNonstd = 1;
            if( z[i]=='-' ){
              jsonBlobAppendNode(pParse, JSONB_FLOAT, 6, "-9e999");
            }else{
              jsonBlobAppendNode(pParse, JSONB_FLOAT, 5, "9e999");
            }
            return i + (sqlite3StrNICmp(&z[i+4],"inity",5)==0 ? 9 : 4);
          }
          if( z[i+1]=='.' ){
            pParse->hasNonstd = 1;
            t |= 0x01;
            goto parse_number_2;
          }
          pParse->iErr = i;
          return -1;
        }
        if( z[i+1]=='0' ){
          if( sqlite3Isdigit(z[i+2]) ){
            pParse->iErr = i+1;
            return -1;
          }else if( (z[i+2]=='x' || z[i+2]=='X') && sqlite3Isxdigit(z[i+3]) ){
            pParse->hasNonstd = 1;
            t |= 0x01;
            for(j=i+4; sqlite3Isxdigit(z[j]); j++){}
            goto parse_number_finish;
          }
        }
      }
    }

  parse_number_2:
    for(j=i+1;; j++){
      c = z[j];
      if( sqlite3Isdigit(c) ) continue;
      if( c=='.' ){
        if( (t & 0x02)!=0 ){
          pParse->iErr = j;
          return -1;
        }
        t |= 0x02;
        continue;
      }
      if( c=='e' || c=='E' ){
        if( z[j-1]<'0' ){
          if( ALWAYS(z[j-1]=='.') && ALWAYS(j-2>=i) && sqlite3Isdigit(z[j-2]) ){
            pParse->hasNonstd = 1;
            t |= 0x01;
          }else{
            pParse->iErr = j;
            return -1;
          }
        }
        if( seenE ){
          pParse->iErr = j;
          return -1;
        }
        t |= 0x02;
        seenE = 1;
        c = z[j+1];
        if( c=='+' || c=='-' ){
          j++;
          c = z[j+1];
        }
        if( c<'0' || c>'9' ){
          pParse->iErr = j;
          return -1;
        }
        continue;
      }
      break;
    }
    if( z[j-1]<'0' ){
      if( ALWAYS(z[j-1]=='.') && ALWAYS(j-2>=i) && sqlite3Isdigit(z[j-2]) ){
        pParse->hasNonstd = 1;
        t |= 0x01;
      }else{
        pParse->iErr = j;
        return -1;
      }
    }
  parse_number_finish:
    assert( JSONB_INT+0x01==JSONB_INT5 );
    assert( JSONB_FLOAT+0x01==JSONB_FLOAT5 );
    assert( JSONB_INT+0x02==JSONB_FLOAT );
    if( z[i]=='+' ) i++;
    jsonBlobAppendNode(pParse, JSONB_INT+t, j-i, &z[i]);
    return j;
  }
  case '}': {
    pParse->iErr = i;
    return -2;  /* End of {...} */
  }
  case ']': {
    pParse->iErr = i;
    return -3;  /* End of [...] */
  }
  case ',': {
    pParse->iErr = i;
    return -4;  /* List separator */
  }
  case ':': {
    pParse->iErr = i;
    return -5;  /* Object label/value separator */
  }
  case 0: {
    return 0;   /* End of file */
  }
  case 0x09:
  case 0x0a:
  case 0x0d:
  case 0x20: {
    i += 1 + (u32)strspn(&z[i+1], jsonSpaces);
    goto json_parse_restart;
  }
  case 0x0b:
  case 0x0c:
  case '/':
  case 0xc2:
  case 0xe1:
  case 0xe2:
  case 0xe3:
  case 0xef: {
    j = json5Whitespace(&z[i]);
    if( j>0 ){
      i += j;
      pParse->hasNonstd = 1;
      goto json_parse_restart;
    }
    pParse->iErr = i;
    return -1;
  }
  case 'n': {
    if( strncmp(z+i,"null",4)==0 && !sqlite3Isalnum(z[i+4]) ){
      jsonBlobAppendOneByte(pParse, JSONB_NULL);
      return i+4;
    }
    /* fall-through into the default case that checks for NaN */
    /* no break */ deliberate_fall_through
  }
  default: {
    u32 k;
    int nn;
    c = z[i];
    for(k=0; k<sizeof(aNanInfName)/sizeof(aNanInfName[0]); k++){
      if( c!=aNanInfName[k].c1 && c!=aNanInfName[k].c2 ) continue;
      nn = aNanInfName[k].n;
      if( sqlite3StrNICmp(&z[i], aNanInfName[k].zMatch, nn)!=0 ){
        continue;
      }
      if( sqlite3Isalnum(z[i+nn]) ) continue;
      if( aNanInfName[k].eType==JSONB_FLOAT ){
        jsonBlobAppendNode(pParse, JSONB_FLOAT, 5, "9e999");
      }else{
        jsonBlobAppendOneByte(pParse, JSONB_NULL);
      }
      pParse->hasNonstd = 1;
      return i + nn;
    }
    pParse->iErr = i;
    return -1;  /* Syntax error */
  }
  } /* End switch(z[i]) */
}
