
/* Opcode: TypeCheck P1 P2 P3 P4 *
** Synopsis: typecheck(r[P1@P2])
**
** Apply affinities to the range of P2 registers beginning with P1.
** Take the affinities from the Table object in P4.  If any value
** cannot be coerced into the correct type, then raise an error.
**
** This opcode is similar to OP_Affinity except that this opcode
** forces the register type to the Table column type.  This is used
** to implement "strict affinity".
**
** GENERATED ALWAYS AS ... STATIC columns are only checked if P3
** is zero.  When P3 is non-zero, no type checking occurs for
** static generated columns.  Virtual columns are computed at query time
** and so they are never checked.
**
** Preconditions:
**
** <ul>
** <li> P2 should be the number of non-virtual columns in the
**      table of P4.
** <li> Table P4 should be a STRICT table.
** </ul>
**
** If any precondition is false, an assertion fault occurs.
*/
case OP_TypeCheck: {
  Table *pTab;
  Column *aCol;
  int i;

  assert( pOp->p4type==P4_TABLE );
  pTab = pOp->p4.pTab;
  assert( pTab->tabFlags & TF_Strict );
  assert( pTab->nNVCol==pOp->p2 );
  aCol = pTab->aCol;
  pIn1 = &aMem[pOp->p1];
  for(i=0; i<pTab->nCol; i++){
    if( aCol[i].colFlags & COLFLAG_GENERATED ){
      if( aCol[i].colFlags & COLFLAG_VIRTUAL ) continue;
      if( pOp->p3 ){ pIn1++; continue; }
    }
    assert( pIn1 < &aMem[pOp->p1+pOp->p2] );
    applyAffinity(pIn1, aCol[i].affinity, encoding);
    if( (pIn1->flags & MEM_Null)==0 ){
      switch( aCol[i].eCType ){
        case COLTYPE_BLOB: {
          if( (pIn1->flags & MEM_Blob)==0 ) goto vdbe_type_error;
          break;
        }
        case COLTYPE_INTEGER:
        case COLTYPE_INT: {
          if( (pIn1->flags & MEM_Int)==0 ) goto vdbe_type_error;
          break;
        }
        case COLTYPE_TEXT: {
          if( (pIn1->flags & MEM_Str)==0 ) goto vdbe_type_error;
          break;
        }
        case COLTYPE_REAL: {
          testcase( (pIn1->flags & (MEM_Real|MEM_IntReal))==MEM_Real );
          assert( (pIn1->flags & MEM_IntReal)==0 );
          if( pIn1->flags & MEM_Int ){
            /* When applying REAL affinity, if the result is still an MEM_Int
            ** that will fit in 6 bytes, then change the type to MEM_IntReal
            ** so that we keep the high-resolution integer value but know that
            ** the type really wants to be REAL. */
            testcase( pIn1->u.i==140737488355328LL );
            testcase( pIn1->u.i==140737488355327LL );
            testcase( pIn1->u.i==-140737488355328LL );
            testcase( pIn1->u.i==-140737488355329LL );
            if( pIn1->u.i<=140737488355327LL && pIn1->u.i>=-140737488355328LL){
              pIn1->flags |= MEM_IntReal;
              pIn1->flags &= ~MEM_Int;
            }else{
              pIn1->u.r = (double)pIn1->u.i;
              pIn1->flags |= MEM_Real;
              pIn1->flags &= ~MEM_Int;
            }
          }else if( (pIn1->flags & (MEM_Real|MEM_IntReal))==0 ){
            goto vdbe_type_error;
          }
          break;
        }
        default: {
          /* COLTYPE_ANY.  Accept anything. */
          break;
        }
      }
    }
    REGISTER_TRACE((int)(pIn1-aMem), pIn1);
    pIn1++;
  }
  assert( pIn1 == &aMem[pOp->p1+pOp->p2] );
  break;

vdbe_type_error:
  sqlite3VdbeError(p, "cannot store %s value in %s column %s.%s",
     vdbeMemTypeName(pIn1), sqlite3StdType[aCol[i].eCType-1],
     pTab->zName, aCol[i].zCnName);
  rc = SQLITE_CONSTRAINT_DATATYPE;
  goto abort_due_to_error;
}

/* Opcode: Affinity P1 P2 * P4 *
** Synopsis: affinity(r[P1@P2])
**
** Apply affinities to a range of P2 registers starting with P1.
**
** P4 is a string that is P2 characters long. The N-th character of the
** string indicates the column affinity that should be used for the N-th
** memory cell in the range.
*/
case OP_Affinity: {
  const char *zAffinity;   /* The affinity to be applied */

  zAffinity = pOp->p4.z;
  assert( zAffinity!=0 );
  assert( pOp->p2>0 );
  assert( zAffinity[pOp->p2]==0 );
  pIn1 = &aMem[pOp->p1];
  while( 1 /*exit-by-break*/ ){
    assert( pIn1 <= &p->aMem[(p->nMem+1 - p->nCursor)] );
    assert( zAffinity[0]==SQLITE_AFF_NONE || memIsValid(pIn1) );
    applyAffinity(pIn1, zAffinity[0], encoding);
    if( zAffinity[0]==SQLITE_AFF_REAL && (pIn1->flags & MEM_Int)!=0 ){
      /* When applying REAL affinity, if the result is still an MEM_Int
      ** that will fit in 6 bytes, then change the type to MEM_IntReal
      ** so that we keep the high-resolution integer value but know that
      ** the type really wants to be REAL. */
      testcase( pIn1->u.i==140737488355328LL );
      testcase( pIn1->u.i==140737488355327LL );
      testcase( pIn1->u.i==-140737488355328LL );
      testcase( pIn1->u.i==-140737488355329LL );
      if( pIn1->u.i<=140737488355327LL && pIn1->u.i>=-140737488355328LL ){
        pIn1->flags |= MEM_IntReal;
        pIn1->flags &= ~MEM_Int;
      }else{
        pIn1->u.r = (double)pIn1->u.i;
        pIn1->flags |= MEM_Real;
        pIn1->flags &= ~(MEM_Int|MEM_Str);
      }
    }
    REGISTER_TRACE((int)(pIn1-aMem), pIn1);
    zAffinity++;
    if( zAffinity[0]==0 ) break;
    pIn1++;
  }
  break;
}

/* Opcode: MakeRecord P1 P2 P3 P4 *
** Synopsis: r[P3]=mkrec(r[P1@P2])
**
** Convert P2 registers beginning with P1 into the [record format]
** use as a data record in a database table or as a key
** in an index.  The OP_Column opcode can decode the record later.
**
** P4 may be a string that is P2 characters long.  The N-th character of the
** string indicates the column affinity that should be used for the N-th
** field of the index key.
**
** The mapping from character to affinity is given by the SQLITE_AFF_
** macros defined in sqliteInt.h.
**
** If P4 is NULL then all index fields have the affinity BLOB.
**
** The meaning of P5 depends on whether or not the SQLITE_ENABLE_NULL_TRIM
** compile-time option is enabled:
**
**   * If SQLITE_ENABLE_NULL_TRIM is enabled, then the P5 is the index
**     of the right-most table that can be null-trimmed.
**
**   * If SQLITE_ENABLE_NULL_TRIM is omitted, then P5 has the value
**     OPFLAG_NOCHNG_MAGIC if the OP_MakeRecord opcode is allowed to
**     accept no-change records with serial_type 10.  This value is
**     only used inside an assert() and does not affect the end result.
*/
case OP_MakeRecord: {
  Mem *pRec;             /* The new record */
  u64 nData;             /* Number of bytes of data space */
  int nHdr;              /* Number of bytes of header space */
  i64 nByte;             /* Data space required for this record */
  i64 nZero;             /* Number of zero bytes at the end of the record */
  int nVarint;           /* Number of bytes in a varint */
  u32 serial_type;       /* Type field */
  Mem *pData0;           /* First field to be combined into the record */
  Mem *pLast;            /* Last field of the record */
  int nField;            /* Number of fields in the record */
  char *zAffinity;       /* The affinity string for the record */
  u32 len;               /* Length of a field */
  u8 *zHdr;              /* Where to write next byte of the header */
  u8 *zPayload;          /* Where to write next byte of the payload */

  /* Assuming the record contains N fields, the record format looks
  ** like this:
  **
  ** ------------------------------------------------------------------------
  ** | hdr-size | type 0 | type 1 | ... | type N-1 | data0 | ... | data N-1 |
  ** ------------------------------------------------------------------------
  **
  ** Data(0) is taken from register P1.  Data(1) comes from register P1+1
  ** and so forth.
  **
  ** Each type field is a varint representing the serial type of the
  ** corresponding data element (see sqlite3VdbeSerialType()). The
  ** hdr-size field is also a varint which is the offset from the beginning
  ** of the record to data0.
  */
  nData = 0;         /* Number of bytes of data space */
  nHdr = 0;          /* Number of bytes of header space */
  nZero = 0;         /* Number of zero bytes at the end of the record */
  nField = pOp->p1;
  zAffinity = pOp->p4.z;
  assert( nField>0 && pOp->p2>0 && pOp->p2+nField<=(p->nMem+1 - p->nCursor)+1 );
  pData0 = &aMem[nField];
  nField = pOp->p2;
  pLast = &pData0[nField-1];

  /* Identify the output register */
  assert( pOp->p3<pOp->p1 || pOp->p3>=pOp->p1+pOp->p2 );
  pOut = &aMem[pOp->p3];
  memAboutToChange(p, pOut);

  /* Apply the requested affinity to all inputs
  */
  assert( pData0<=pLast );
  if( zAffinity ){
    pRec = pData0;
    do{
      applyAffinity(pRec, zAffinity[0], encoding);
      if( zAffinity[0]==SQLITE_AFF_REAL && (pRec->flags & MEM_Int) ){
        pRec->flags |= MEM_IntReal;
        pRec->flags &= ~(MEM_Int);
      }
      REGISTER_TRACE((int)(pRec-aMem), pRec);
      zAffinity++;
      pRec++;
      assert( zAffinity[0]==0 || pRec<=pLast );
    }while( zAffinity[0] );
  }

#ifdef SQLITE_ENABLE_NULL_TRIM
  /* NULLs can be safely trimmed from the end of the record, as long as
  ** as the schema format is 2 or more and none of the omitted columns
  ** have a non-NULL default value.  Also, the record must be left with
  ** at least one field.  If P5>0 then it will be one more than the
  ** index of the right-most column with a non-NULL default value */
  if( pOp->p5 ){
    while( (pLast->flags & MEM_Null)!=0 && nField>pOp->p5 ){
      pLast--;
      nField--;
    }
  }
#endif

  /* Loop through the elements that will make up the record to figure
  ** out how much space is required for the new record.  After this loop,
  ** the Mem.uTemp field of each term should hold the serial-type that will
  ** be used for that term in the generated record:
  **
  **   Mem.uTemp value    type
  **   ---------------    ---------------
  **      0               NULL
  **      1               1-byte signed integer
  **      2               2-byte signed integer
  **      3               3-byte signed integer
  **      4               4-byte signed integer
  **      5               6-byte signed integer
  **      6               8-byte signed integer
  **      7               IEEE float
  **      8               Integer constant 0
  **      9               Integer constant 1
  **     10,11            reserved for expansion
  **    N>=12 and even    BLOB
  **    N>=13 and odd     text
  **
  ** The following additional values are computed:
  **     nHdr        Number of bytes needed for the record header
  **     nData       Number of bytes of data space needed for the record
  **     nZero       Zero bytes at the end of the record
  */
  pRec = pLast;
  do{
    assert( memIsValid(pRec) );
    if( pRec->flags & MEM_Null ){
      if( pRec->flags & MEM_Zero ){
        /* Values with MEM_Null and MEM_Zero are created by xColumn virtual
        ** table methods that never invoke sqlite3_result_xxxxx() while
        ** computing an unchanging column value in an UPDATE statement.
        ** Give such values a special internal-use-only serial-type of 10
        ** so that they can be passed through to xUpdate and have
        ** a true sqlite3_value_nochange(). */
#ifndef SQLITE_ENABLE_NULL_TRIM
        assert( pOp->p5==OPFLAG_NOCHNG_MAGIC || CORRUPT_DB );
#endif
        pRec->uTemp = 10;
      }else{
        pRec->uTemp = 0;
      }
      nHdr++;
    }else if( pRec->flags & (MEM_Int|MEM_IntReal) ){
      /* Figure out whether to use 1, 2, 4, 6 or 8 bytes. */
      i64 i = pRec->u.i;
      u64 uu;
      testcase( pRec->flags & MEM_Int );
      testcase( pRec->flags & MEM_IntReal );
      if( i<0 ){
        uu = ~i;
      }else{
        uu = i;
      }
      nHdr++;
      testcase( uu==127 );               testcase( uu==128 );
      testcase( uu==32767 );             testcase( uu==32768 );
      testcase( uu==8388607 );           testcase( uu==8388608 );
      testcase( uu==2147483647 );        testcase( uu==2147483648LL );
      testcase( uu==140737488355327LL ); testcase( uu==140737488355328LL );
      if( uu<=127 ){
        if( (i&1)==i && p->minWriteFileFormat>=4 ){
          pRec->uTemp = 8+(u32)uu;
        }else{
          nData++;
          pRec->uTemp = 1;
        }
      }else if( uu<=32767 ){
        nData += 2;
        pRec->uTemp = 2;
      }else if( uu<=8388607 ){
        nData += 3;
        pRec->uTemp = 3;
      }else if( uu<=2147483647 ){
        nData += 4;
        pRec->uTemp = 4;
      }else if( uu<=140737488355327LL ){
        nData += 6;
        pRec->uTemp = 5;
      }else{
        nData += 8;
        if( pRec->flags & MEM_IntReal ){
          /* If the value is IntReal and is going to take up 8 bytes to store
          ** as an integer, then we might as well make it an 8-byte floating
          ** point value */
          pRec->u.r = (double)pRec->u.i;
          pRec->flags &= ~MEM_IntReal;
          pRec->flags |= MEM_Real;
          pRec->uTemp = 7;
        }else{
          pRec->uTemp = 6;
        }
      }
    }else if( pRec->flags & MEM_Real ){
      nHdr++;
      nData += 8;
      pRec->uTemp = 7;
    }else{
      assert( db->mallocFailed || pRec->flags&(MEM_Str|MEM_Blob) );
      assert( pRec->n>=0 );
      len = (u32)pRec->n;
      serial_type = (len*2) + 12 + ((pRec->flags & MEM_Str)!=0);
      if( pRec->flags & MEM_Zero ){
        serial_type += pRec->u.nZero*2;
        if( nData ){
          if( sqlite3VdbeMemExpandBlob(pRec) ) goto no_mem;
          len += pRec->u.nZero;
        }else{
          nZero += pRec->u.nZero;
        }
      }
      nData += len;
      nHdr += sqlite3VarintLen(serial_type);
      pRec->uTemp = serial_type;
    }
    if( pRec==pData0 ) break;
    pRec--;
  }while(1);

  /* EVIDENCE-OF: R-22564-11647 The header begins with a single varint
  ** which determines the total number of bytes in the header. The varint
  ** value is the size of the header in bytes including the size varint
  ** itself. */
  testcase( nHdr==126 );
  testcase( nHdr==127 );
  if( nHdr<=126 ){
    /* The common case */
    nHdr += 1;
  }else{
    /* Rare case of a really large header */
    nVarint = sqlite3VarintLen(nHdr);
    nHdr += nVarint;
    if( nVarint<sqlite3VarintLen(nHdr) ) nHdr++;
  }
  nByte = nHdr+nData;

  /* Make sure the output register has a buffer large enough to store
  ** the new record. The output register (pOp->p3) is not allowed to
  ** be one of the input registers (because the following call to
  ** sqlite3VdbeMemClearAndResize() could clobber the value before it is used).
  */
  if( nByte+nZero<=pOut->szMalloc ){
    /* The output register is already large enough to hold the record.
    ** No error checks or buffer enlargement is required */
    pOut->z = pOut->zMalloc;
  }else{
    /* Need to make sure that the output is not too big and then enlarge
    ** the output register to hold the full result */
    if( nByte+nZero>db->aLimit[SQLITE_LIMIT_LENGTH] ){
      goto too_big;
    }
    if( sqlite3VdbeMemClearAndResize(pOut, (int)nByte) ){
      goto no_mem;
    }
  }
  pOut->n = (int)nByte;
  pOut->flags = MEM_Blob;
  if( nZero ){
    pOut->u.nZero = nZero;
    pOut->flags |= MEM_Zero;
  }
  UPDATE_MAX_BLOBSIZE(pOut);
  zHdr = (u8 *)pOut->z;
  zPayload = zHdr + nHdr;

  /* Write the record */
  if( nHdr<0x80 ){
    *(zHdr++) = nHdr;
  }else{
    zHdr += sqlite3PutVarint(zHdr,nHdr);
  }
  assert( pData0<=pLast );
  pRec = pData0;
  while( 1 /*exit-by-break*/ ){
    serial_type = pRec->uTemp;
    /* EVIDENCE-OF: R-06529-47362 Following the size varint are one or more
    ** additional varints, one per column.
    ** EVIDENCE-OF: R-64536-51728 The values for each column in the record
    ** immediately follow the header. */
    if( serial_type<=7 ){
      *(zHdr++) = serial_type;
      if( serial_type==0 ){
        /* NULL value.  No change in zPayload */
      }else{
        u64 v;
        if( serial_type==7 ){
          assert( sizeof(v)==sizeof(pRec->u.r) );
          memcpy(&v, &pRec->u.r, sizeof(v));
          swapMixedEndianFloat(v);
        }else{
          v = pRec->u.i;
        }
        len = sqlite3SmallTypeSizes[serial_type];
        assert( len>=1 && len<=8 && len!=5 && len!=7 );
        switch( len ){
          default: zPayload[7] = (u8)(v&0xff); v >>= 8;
                   zPayload[6] = (u8)(v&0xff); v >>= 8;
                   /* no break */ deliberate_fall_through
          case 6:  zPayload[5] = (u8)(v&0xff); v >>= 8;
                   zPayload[4] = (u8)(v&0xff); v >>= 8;
                   /* no break */ deliberate_fall_through
          case 4:  zPayload[3] = (u8)(v&0xff); v >>= 8;
                   /* no break */ deliberate_fall_through
          case 3:  zPayload[2] = (u8)(v&0xff); v >>= 8;
                   /* no break */ deliberate_fall_through
          case 2:  zPayload[1] = (u8)(v&0xff); v >>= 8;
                   /* no break */ deliberate_fall_through
          case 1:  zPayload[0] = (u8)(v&0xff);
        }
        zPayload += len;
      }
    }else if( serial_type<0x80 ){
      *(zHdr++) = serial_type;
      if( serial_type>=14 && pRec->n>0 ){
        assert( pRec->z!=0 );
        memcpy(zPayload, pRec->z, pRec->n);
        zPayload += pRec->n;
      }
    }else{
      zHdr += sqlite3PutVarint(zHdr, serial_type);
      if( pRec->n ){
        assert( pRec->z!=0 );
        memcpy(zPayload, pRec->z, pRec->n);
        zPayload += pRec->n;
      }
    }
    if( pRec==pLast ) break;
    pRec++;
  }
  assert( nHdr==(int)(zHdr - (u8*)pOut->z) );
  assert( nByte==(int)(zPayload - (u8*)pOut->z) );

  assert( pOp->p3>0 && pOp->p3<=(p->nMem+1 - p->nCursor) );
  REGISTER_TRACE(pOp->p3, pOut);
  break;
}
