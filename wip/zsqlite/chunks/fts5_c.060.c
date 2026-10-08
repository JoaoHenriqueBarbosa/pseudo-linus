

static int fts5PorterStep2(char *aBuf, int *pnBuf){
  int ret = 0;
  int nBuf = *pnBuf;
  switch( aBuf[nBuf-2] ){

    case 'a':
      if( nBuf>7 && 0==memcmp("ational", &aBuf[nBuf-7], 7) ){
        if( fts5Porter_MGt0(aBuf, nBuf-7) ){
          memcpy(&aBuf[nBuf-7], "ate", 3);
          *pnBuf = nBuf - 7 + 3;
        }
      }else if( nBuf>6 && 0==memcmp("tional", &aBuf[nBuf-6], 6) ){
        if( fts5Porter_MGt0(aBuf, nBuf-6) ){
          memcpy(&aBuf[nBuf-6], "tion", 4);
          *pnBuf = nBuf - 6 + 4;
        }
      }
      break;

    case 'c':
      if( nBuf>4 && 0==memcmp("enci", &aBuf[nBuf-4], 4) ){
        if( fts5Porter_MGt0(aBuf, nBuf-4) ){
          memcpy(&aBuf[nBuf-4], "ence", 4);
          *pnBuf = nBuf - 4 + 4;
        }
      }else if( nBuf>4 && 0==memcmp("anci", &aBuf[nBuf-4], 4) ){
        if( fts5Porter_MGt0(aBuf, nBuf-4) ){
          memcpy(&aBuf[nBuf-4], "ance", 4);
          *pnBuf = nBuf - 4 + 4;
        }
      }
      break;

    case 'e':
      if( nBuf>4 && 0==memcmp("izer", &aBuf[nBuf-4], 4) ){
        if( fts5Porter_MGt0(aBuf, nBuf-4) ){
          memcpy(&aBuf[nBuf-4], "ize", 3);
          *pnBuf = nBuf - 4 + 3;
        }
      }
      break;

    case 'g':
      if( nBuf>4 && 0==memcmp("logi", &aBuf[nBuf-4], 4) ){
        if( fts5Porter_MGt0(aBuf, nBuf-4) ){
          memcpy(&aBuf[nBuf-4], "log", 3);
          *pnBuf = nBuf - 4 + 3;
        }
      }
      break;

    case 'l':
      if( nBuf>3 && 0==memcmp("bli", &aBuf[nBuf-3], 3) ){
        if( fts5Porter_MGt0(aBuf, nBuf-3) ){
          memcpy(&aBuf[nBuf-3], "ble", 3);
          *pnBuf = nBuf - 3 + 3;
        }
      }else if( nBuf>4 && 0==memcmp("alli", &aBuf[nBuf-4], 4) ){
        if( fts5Porter_MGt0(aBuf, nBuf-4) ){
          memcpy(&aBuf[nBuf-4], "al", 2);
          *pnBuf = nBuf - 4 + 2;
        }
      }else if( nBuf>5 && 0==memcmp("entli", &aBuf[nBuf-5], 5) ){
        if( fts5Porter_MGt0(aBuf, nBuf-5) ){
          memcpy(&aBuf[nBuf-5], "ent", 3);
          *pnBuf = nBuf - 5 + 3;
        }
      }else if( nBuf>3 && 0==memcmp("eli", &aBuf[nBuf-3], 3) ){
        if( fts5Porter_MGt0(aBuf, nBuf-3) ){
          memcpy(&aBuf[nBuf-3], "e", 1);
          *pnBuf = nBuf - 3 + 1;
        }
      }else if( nBuf>5 && 0==memcmp("ousli", &aBuf[nBuf-5], 5) ){
        if( fts5Porter_MGt0(aBuf, nBuf-5) ){
          memcpy(&aBuf[nBuf-5], "ous", 3);
          *pnBuf = nBuf - 5 + 3;
        }
      }
      break;

    case 'o':
      if( nBuf>7 && 0==memcmp("ization", &aBuf[nBuf-7], 7) ){
        if( fts5Porter_MGt0(aBuf, nBuf-7) ){
          memcpy(&aBuf[nBuf-7], "ize", 3);
          *pnBuf = nBuf - 7 + 3;
        }
      }else if( nBuf>5 && 0==memcmp("ation", &aBuf[nBuf-5], 5) ){
        if( fts5Porter_MGt0(aBuf, nBuf-5) ){
          memcpy(&aBuf[nBuf-5], "ate", 3);
          *pnBuf = nBuf - 5 + 3;
        }
      }else if( nBuf>4 && 0==memcmp("ator", &aBuf[nBuf-4], 4) ){
        if( fts5Porter_MGt0(aBuf, nBuf-4) ){
          memcpy(&aBuf[nBuf-4], "ate", 3);
          *pnBuf = nBuf - 4 + 3;
        }
      }
      break;

    case 's':
      if( nBuf>5 && 0==memcmp("alism", &aBuf[nBuf-5], 5) ){
        if( fts5Porter_MGt0(aBuf, nBuf-5) ){
          memcpy(&aBuf[nBuf-5], "al", 2);
          *pnBuf = nBuf - 5 + 2;
        }
      }else if( nBuf>7 && 0==memcmp("iveness", &aBuf[nBuf-7], 7) ){
        if( fts5Porter_MGt0(aBuf, nBuf-7) ){
          memcpy(&aBuf[nBuf-7], "ive", 3);
          *pnBuf = nBuf - 7 + 3;
        }
      }else if( nBuf>7 && 0==memcmp("fulness", &aBuf[nBuf-7], 7) ){
        if( fts5Porter_MGt0(aBuf, nBuf-7) ){
          memcpy(&aBuf[nBuf-7], "ful", 3);
          *pnBuf = nBuf - 7 + 3;
        }
      }else if( nBuf>7 && 0==memcmp("ousness", &aBuf[nBuf-7], 7) ){
        if( fts5Porter_MGt0(aBuf, nBuf-7) ){
          memcpy(&aBuf[nBuf-7], "ous", 3);
          *pnBuf = nBuf - 7 + 3;
        }
      }
      break;

    case 't':
      if( nBuf>5 && 0==memcmp("aliti", &aBuf[nBuf-5], 5) ){
        if( fts5Porter_MGt0(aBuf, nBuf-5) ){
          memcpy(&aBuf[nBuf-5], "al", 2);
          *pnBuf = nBuf - 5 + 2;
        }
      }else if( nBuf>5 && 0==memcmp("iviti", &aBuf[nBuf-5], 5) ){
        if( fts5Porter_MGt0(aBuf, nBuf-5) ){
          memcpy(&aBuf[nBuf-5], "ive", 3);
          *pnBuf = nBuf - 5 + 3;
        }
      }else if( nBuf>6 && 0==memcmp("biliti", &aBuf[nBuf-6], 6) ){
        if( fts5Porter_MGt0(aBuf, nBuf-6) ){
          memcpy(&aBuf[nBuf-6], "ble", 3);
          *pnBuf = nBuf - 6 + 3;
        }
      }
      break;

  }
  return ret;
}


static int fts5PorterStep3(char *aBuf, int *pnBuf){
  int ret = 0;
  int nBuf = *pnBuf;
  switch( aBuf[nBuf-2] ){

    case 'a':
      if( nBuf>4 && 0==memcmp("ical", &aBuf[nBuf-4], 4) ){
        if( fts5Porter_MGt0(aBuf, nBuf-4) ){
          memcpy(&aBuf[nBuf-4], "ic", 2);
          *pnBuf = nBuf - 4 + 2;
        }
      }
      break;

    case 's':
      if( nBuf>4 && 0==memcmp("ness", &aBuf[nBuf-4], 4) ){
        if( fts5Porter_MGt0(aBuf, nBuf-4) ){
          *pnBuf = nBuf - 4;
        }
      }
      break;

    case 't':
      if( nBuf>5 && 0==memcmp("icate", &aBuf[nBuf-5], 5) ){
        if( fts5Porter_MGt0(aBuf, nBuf-5) ){
          memcpy(&aBuf[nBuf-5], "ic", 2);
          *pnBuf = nBuf - 5 + 2;
        }
      }else if( nBuf>5 && 0==memcmp("iciti", &aBuf[nBuf-5], 5) ){
        if( fts5Porter_MGt0(aBuf, nBuf-5) ){
          memcpy(&aBuf[nBuf-5], "ic", 2);
          *pnBuf = nBuf - 5 + 2;
        }
      }
      break;

    case 'u':
      if( nBuf>3 && 0==memcmp("ful", &aBuf[nBuf-3], 3) ){
        if( fts5Porter_MGt0(aBuf, nBuf-3) ){
          *pnBuf = nBuf - 3;
        }
      }
      break;

    case 'v':
      if( nBuf>5 && 0==memcmp("ative", &aBuf[nBuf-5], 5) ){
        if( fts5Porter_MGt0(aBuf, nBuf-5) ){
          *pnBuf = nBuf - 5;
        }
      }
      break;

    case 'z':
      if( nBuf>5 && 0==memcmp("alize", &aBuf[nBuf-5], 5) ){
        if( fts5Porter_MGt0(aBuf, nBuf-5) ){
          memcpy(&aBuf[nBuf-5], "al", 2);
          *pnBuf = nBuf - 5 + 2;
        }
      }
      break;

  }
  return ret;
}


static int fts5PorterStep1B(char *aBuf, int *pnBuf){
  int ret = 0;
  int nBuf = *pnBuf;
  switch( aBuf[nBuf-2] ){

    case 'e':
      if( nBuf>3 && 0==memcmp("eed", &aBuf[nBuf-3], 3) ){
        if( fts5Porter_MGt0(aBuf, nBuf-3) ){
          memcpy(&aBuf[nBuf-3], "ee", 2);
          *pnBuf = nBuf - 3 + 2;
        }
      }else if( nBuf>2 && 0==memcmp("ed", &aBuf[nBuf-2], 2) ){
        if( fts5Porter_Vowel(aBuf, nBuf-2) ){
          *pnBuf = nBuf - 2;
          ret = 1;
        }
      }
      break;

    case 'n':
      if( nBuf>3 && 0==memcmp("ing", &aBuf[nBuf-3], 3) ){
        if( fts5Porter_Vowel(aBuf, nBuf-3) ){
          *pnBuf = nBuf - 3;
          ret = 1;
        }
      }
      break;

  }
  return ret;
}

/*
** GENERATED CODE ENDS HERE (mkportersteps.tcl)
***************************************************************************
**************************************************************************/

static void fts5PorterStep1A(char *aBuf, int *pnBuf){
  int nBuf = *pnBuf;
  if( aBuf[nBuf-1]=='s' ){
    if( aBuf[nBuf-2]=='e' ){
      if( (nBuf>4 && aBuf[nBuf-4]=='s' && aBuf[nBuf-3]=='s')
       || (nBuf>3 && aBuf[nBuf-3]=='i' )
      ){
        *pnBuf = nBuf-2;
      }else{
        *pnBuf = nBuf-1;
      }
    }
    else if( aBuf[nBuf-2]!='s' ){
      *pnBuf = nBuf-1;
    }
  }
}

static int fts5PorterCb(
  void *pCtx,
  int tflags,
  const char *pToken,
  int nToken,
  int iStart,
  int iEnd
){
  PorterContext *p = (PorterContext*)pCtx;

  char *aBuf;
  int nBuf;

  if( nToken>FTS5_PORTER_MAX_TOKEN || nToken<3 ) goto pass_through;
  aBuf = p->aBuf;
  nBuf = nToken;
  memcpy(aBuf, pToken, nBuf);

  /* Step 1. */
  fts5PorterStep1A(aBuf, &nBuf);
  if( fts5PorterStep1B(aBuf, &nBuf) ){
    if( fts5PorterStep1B2(aBuf, &nBuf)==0 ){
      char c = aBuf[nBuf-1];
      if( fts5PorterIsVowel(c, 0)==0
       && c!='l' && c!='s' && c!='z' && c==aBuf[nBuf-2]
      ){
        nBuf--;
      }else if( fts5Porter_MEq1(aBuf, nBuf) && fts5Porter_Ostar(aBuf, nBuf) ){
        aBuf[nBuf++] = 'e';
      }
    }
  }

  /* Step 1C. */
  if( aBuf[nBuf-1]=='y' && fts5Porter_Vowel(aBuf, nBuf-1) ){
    aBuf[nBuf-1] = 'i';
  }

  /* Steps 2 through 4. */
  fts5PorterStep2(aBuf, &nBuf);
  fts5PorterStep3(aBuf, &nBuf);
  fts5PorterStep4(aBuf, &nBuf);

  /* Step 5a. */
  assert( nBuf>0 );
  if( aBuf[nBuf-1]=='e' ){
    if( fts5Porter_MGt1(aBuf, nBuf-1)
     || (fts5Porter_MEq1(aBuf, nBuf-1) && !fts5Porter_Ostar(aBuf, nBuf-1))
    ){
      nBuf--;
    }
  }

  /* Step 5b. */
  if( nBuf>1 && aBuf[nBuf-1]=='l'
   && aBuf[nBuf-2]=='l' && fts5Porter_MGt1(aBuf, nBuf-1)
  ){
    nBuf--;
  }

  return p->xToken(p->pCtx, tflags, aBuf, nBuf, iStart, iEnd);

 pass_through:
  return p->xToken(p->pCtx, tflags, pToken, nToken, iStart, iEnd);
}

/*
** Tokenize using the porter tokenizer.
*/
static int fts5PorterTokenize(
  Fts5Tokenizer *pTokenizer,
  void *pCtx,
  int flags,
  const char *pText, int nText,
  int (*xToken)(void*, int, const char*, int nToken, int iStart, int iEnd)
){
  PorterTokenizer *p = (PorterTokenizer*)pTokenizer;
  PorterContext sCtx;
  sCtx.xToken = xToken;
  sCtx.pCtx = pCtx;
  sCtx.aBuf = p->aBuf;
  return p->tokenizer.xTokenize(
      p->pTokenizer, (void*)&sCtx, flags, pText, nText, fts5PorterCb
  );
}
