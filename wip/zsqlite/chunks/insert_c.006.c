        sqlite3UpsertDoUpdate(pParse, pUpsert, pTab, 0, iDataCur);
        /* no break */ deliberate_fall_through
      }
#endif
      case OE_Ignore: {
        testcase( onError==OE_Ignore );
        sqlite3VdbeGoto(v, ignoreDest);
        break;
      }
    }
    sqlite3VdbeResolveLabel(v, addrRowidOk);
    if( pUpsert && pUpsertClause!=pUpsert ){
      upsertIpkReturn = sqlite3VdbeAddOp0(v, OP_Goto);
    }else if( ipkTop ){
      ipkBottom = sqlite3VdbeAddOp0(v, OP_Goto);
      sqlite3VdbeJumpHere(v, ipkTop-1);
    }
  }

  /* Test all UNIQUE constraints by creating entries for each UNIQUE
  ** index and making sure that duplicate entries do not already exist.
  ** Compute the revised record entries for indices as we go.
  **
  ** This loop also handles the case of the PRIMARY KEY index for a
  ** WITHOUT ROWID table.
  */
  for(pIdx = indexIteratorFirst(&sIdxIter, &ix);
      pIdx;
      pIdx = indexIteratorNext(&sIdxIter, &ix)
  ){
    int regIdx;          /* Range of registers holding content for pIdx */
    int regR;            /* Range of registers holding conflicting PK */
    int iThisCur;        /* Cursor for this UNIQUE index */
    int addrUniqueOk;    /* Jump here if the UNIQUE constraint is satisfied */
    int addrConflictCk;  /* First opcode in the conflict check logic */

    if( aRegIdx[ix]==0 ) continue;  /* Skip indices that do not change */
    if( pUpsert ){
      pUpsertClause = sqlite3UpsertOfIndex(pUpsert, pIdx);
      if( upsertIpkDelay && pUpsertClause==pUpsert ){
        sqlite3VdbeJumpHere(v, upsertIpkDelay);
      }
    }
    addrUniqueOk = sqlite3VdbeMakeLabel(pParse);
    if( bAffinityDone==0 ){
      sqlite3TableAffinity(v, pTab, regNewData+1);
      bAffinityDone = 1;
    }
    VdbeNoopComment((v, "prep index %s", pIdx->zName));
    iThisCur = iIdxCur+ix;


    /* Skip partial indices for which the WHERE clause is not true */
    if( pIdx->pPartIdxWhere ){
      sqlite3VdbeAddOp2(v, OP_Null, 0, aRegIdx[ix]);
      pParse->iSelfTab = -(regNewData+1);
      sqlite3ExprIfFalseDup(pParse, pIdx->pPartIdxWhere, addrUniqueOk,
                            SQLITE_JUMPIFNULL);
      pParse->iSelfTab = 0;
    }

    /* Create a record for this index entry as it should appear after
    ** the insert or update.  Store that record in the aRegIdx[ix] register
    */
    regIdx = aRegIdx[ix]+1;
    for(i=0; i<pIdx->nColumn; i++){
      int iField = pIdx->aiColumn[i];
      int x;
      if( iField==XN_EXPR ){
        pParse->iSelfTab = -(regNewData+1);
        sqlite3ExprCodeCopy(pParse, pIdx->aColExpr->a[i].pExpr, regIdx+i);
        pParse->iSelfTab = 0;
        VdbeComment((v, "%s column %d", pIdx->zName, i));
      }else if( iField==XN_ROWID || iField==pTab->iPKey ){
        x = regNewData;
        sqlite3VdbeAddOp2(v, OP_IntCopy, x, regIdx+i);
        VdbeComment((v, "rowid"));
      }else{
        testcase( sqlite3TableColumnToStorage(pTab, iField)!=iField );
        x = sqlite3TableColumnToStorage(pTab, iField) + regNewData + 1;
        sqlite3VdbeAddOp2(v, OP_SCopy, x, regIdx+i);
        VdbeComment((v, "%s", pTab->aCol[iField].zCnName));
      }
    }
    sqlite3VdbeAddOp3(v, OP_MakeRecord, regIdx, pIdx->nColumn, aRegIdx[ix]);
    VdbeComment((v, "for %s", pIdx->zName));
#ifdef SQLITE_ENABLE_NULL_TRIM
    if( pIdx->idxType==SQLITE_IDXTYPE_PRIMARYKEY ){
      sqlite3SetMakeRecordP5(v, pIdx->pTable);
    }
#endif
    sqlite3VdbeReleaseRegisters(pParse, regIdx, pIdx->nColumn, 0, 0);

    /* In an UPDATE operation, if this index is the PRIMARY KEY index
    ** of a WITHOUT ROWID table and there has been no change the
    ** primary key, then no collision is possible.  The collision detection
    ** logic below can all be skipped. */
    if( isUpdate && pPk==pIdx && pkChng==0 ){
      sqlite3VdbeResolveLabel(v, addrUniqueOk);
      continue;
    }

    /* Find out what action to take in case there is a uniqueness conflict */
    onError = pIdx->onError;
    if( onError==OE_None ){
      sqlite3VdbeResolveLabel(v, addrUniqueOk);
      continue;  /* pIdx is not a UNIQUE index */
    }
    if( overrideError!=OE_Default ){
      onError = overrideError;
    }else if( onError==OE_Default ){
      onError = OE_Abort;
    }

    /* Figure out if the upsert clause applies to this index */
    if( pUpsertClause ){
      if( pUpsertClause->isDoUpdate==0 ){
        onError = OE_Ignore;  /* DO NOTHING is the same as INSERT OR IGNORE */
      }else{
        onError = OE_Update;  /* DO UPDATE */
      }
    }

    /* Collision detection may be omitted if all of the following are true:
    **   (1) The conflict resolution algorithm is REPLACE
    **   (2) The table is a WITHOUT ROWID table
    **   (3) There are no secondary indexes on the table
    **   (4) No delete triggers need to be fired if there is a conflict
    **   (5) No FK constraint counters need to be updated if a conflict occurs.
    **
    ** This is not possible for ENABLE_PREUPDATE_HOOK builds, as the row
    ** must be explicitly deleted in order to ensure any pre-update hook
    ** is invoked.  */
    assert( IsOrdinaryTable(pTab) );
#ifndef SQLITE_ENABLE_PREUPDATE_HOOK
    if( (ix==0 && pIdx->pNext==0)                   /* Condition 3 */
     && pPk==pIdx                                   /* Condition 2 */
     && onError==OE_Replace                         /* Condition 1 */
     && ( 0==(db->flags&SQLITE_RecTriggers) ||      /* Condition 4 */
          0==sqlite3TriggersExist(pParse, pTab, TK_DELETE, 0, 0))
     && ( 0==(db->flags&SQLITE_ForeignKeys) ||      /* Condition 5 */
         (0==pTab->u.tab.pFKey && 0==sqlite3FkReferences(pTab)))
    ){
      sqlite3VdbeResolveLabel(v, addrUniqueOk);
      continue;
    }
#endif /* ifndef SQLITE_ENABLE_PREUPDATE_HOOK */

    /* Check to see if the new index entry will be unique */
    sqlite3VdbeVerifyAbortable(v, onError);
    addrConflictCk =
      sqlite3VdbeAddOp4Int(v, OP_NoConflict, iThisCur, addrUniqueOk,
                           regIdx, pIdx->nKeyCol); VdbeCoverage(v);

    /* Generate code to handle collisions */
    regR = pIdx==pPk ? regIdx : sqlite3GetTempRange(pParse, nPkField);
    if( isUpdate || onError==OE_Replace ){
      if( HasRowid(pTab) ){
        sqlite3VdbeAddOp2(v, OP_IdxRowid, iThisCur, regR);
        /* Conflict only if the rowid of the existing index entry
        ** is different from old-rowid */
        if( isUpdate ){
          sqlite3VdbeAddOp3(v, OP_Eq, regR, addrUniqueOk, regOldData);
          sqlite3VdbeChangeP5(v, SQLITE_NOTNULL);
          VdbeCoverage(v);
        }
      }else{
        int x;
        /* Extract the PRIMARY KEY from the end of the index entry and
        ** store it in registers regR..regR+nPk-1 */
        if( pIdx!=pPk ){
          for(i=0; i<pPk->nKeyCol; i++){
            assert( pPk->aiColumn[i]>=0 );
            x = sqlite3TableColumnToIndex(pIdx, pPk->aiColumn[i]);
            sqlite3VdbeAddOp3(v, OP_Column, iThisCur, x, regR+i);
            VdbeComment((v, "%s.%s", pTab->zName,
                         pTab->aCol[pPk->aiColumn[i]].zCnName));
          }
        }
        if( isUpdate ){
          /* If currently processing the PRIMARY KEY of a WITHOUT ROWID
          ** table, only conflict if the new PRIMARY KEY values are actually
          ** different from the old.  See TH3 withoutrowid04.test.
          **
          ** For a UNIQUE index, only conflict if the PRIMARY KEY values
          ** of the matched index row are different from the original PRIMARY
          ** KEY values of this row before the update.  */
          int addrJump = sqlite3VdbeCurrentAddr(v)+pPk->nKeyCol;
          int op = OP_Ne;
          int regCmp = (IsPrimaryKeyIndex(pIdx) ? regIdx : regR);

          for(i=0; i<pPk->nKeyCol; i++){
            char *p4 = (char*)sqlite3LocateCollSeq(pParse, pPk->azColl[i]);
            x = pPk->aiColumn[i];
            assert( x>=0 );
            if( i==(pPk->nKeyCol-1) ){
              addrJump = addrUniqueOk;
              op = OP_Eq;
            }
            x = sqlite3TableColumnToStorage(pTab, x);
            sqlite3VdbeAddOp4(v, op,
                regOldData+1+x, addrJump, regCmp+i, p4, P4_COLLSEQ
            );
            sqlite3VdbeChangeP5(v, SQLITE_NOTNULL);
            VdbeCoverageIf(v, op==OP_Eq);
            VdbeCoverageIf(v, op==OP_Ne);
          }
        }
      }
    }

    /* Generate code that executes if the new index entry is not unique */
    assert( onError==OE_Rollback || onError==OE_Abort || onError==OE_Fail
        || onError==OE_Ignore || onError==OE_Replace || onError==OE_Update );
    switch( onError ){
      case OE_Rollback:
      case OE_Abort:
      case OE_Fail: {
        testcase( onError==OE_Rollback );
        testcase( onError==OE_Abort );
        testcase( onError==OE_Fail );
        sqlite3UniqueConstraint(pParse, onError, pIdx);
        break;
      }
#ifndef SQLITE_OMIT_UPSERT
      case OE_Update: {
        sqlite3UpsertDoUpdate(pParse, pUpsert, pTab, pIdx, iIdxCur+ix);
        /* no break */ deliberate_fall_through
      }
#endif
      case OE_Ignore: {
        testcase( onError==OE_Ignore );
        sqlite3VdbeGoto(v, ignoreDest);
        break;
      }
      default: {
        int nConflictCk;   /* Number of opcodes in conflict check logic */

        assert( onError==OE_Replace );
        nConflictCk = sqlite3VdbeCurrentAddr(v) - addrConflictCk;
        assert( nConflictCk>0 || db->mallocFailed );
        testcase( nConflictCk<=0 );
        testcase( nConflictCk>1 );
        if( regTrigCnt ){
          sqlite3MultiWrite(pParse);
          nReplaceTrig++;
        }
        if( pTrigger && isUpdate ){
          sqlite3VdbeAddOp1(v, OP_CursorLock, iDataCur);
        }
        sqlite3GenerateRowDelete(pParse, pTab, pTrigger, iDataCur, iIdxCur,
            regR, nPkField, 0, OE_Replace,
            (pIdx==pPk ? ONEPASS_SINGLE : ONEPASS_OFF), iThisCur);
        if( pTrigger && isUpdate ){
          sqlite3VdbeAddOp1(v, OP_CursorUnlock, iDataCur);
        }
        if( regTrigCnt ){
          int addrBypass;  /* Jump destination to bypass recheck logic */

          sqlite3VdbeAddOp2(v, OP_AddImm, regTrigCnt, 1); /* incr trigger cnt */
          addrBypass = sqlite3VdbeAddOp0(v, OP_Goto);  /* Bypass recheck */
          VdbeComment((v, "bypass recheck"));

          /* Here we insert code that will be invoked after all constraint
          ** checks have run, if and only if one or more replace triggers
          ** fired. */
          sqlite3VdbeResolveLabel(v, lblRecheckOk);
          lblRecheckOk = sqlite3VdbeMakeLabel(pParse);
          if( pIdx->pPartIdxWhere ){
            /* Bypass the recheck if this partial index is not defined
            ** for the current row */
            sqlite3VdbeAddOp2(v, OP_IsNull, regIdx-1, lblRecheckOk);
            VdbeCoverage(v);
          }
          /* Copy the constraint check code from above, except change
          ** the constraint-ok jump destination to be the address of
          ** the next retest block */
          while( nConflictCk>0 ){
            VdbeOp x;    /* Conflict check opcode to copy */
            /* The sqlite3VdbeAddOp4() call might reallocate the opcode array.
            ** Hence, make a complete copy of the opcode, rather than using
            ** a pointer to the opcode. */
            x = *sqlite3VdbeGetOp(v, addrConflictCk);
            if( x.opcode!=OP_IdxRowid ){
              int p2;      /* New P2 value for copied conflict check opcode */
              const char *zP4;
              if( sqlite3OpcodeProperty[x.opcode]&OPFLG_JUMP ){
                p2 = lblRecheckOk;
              }else{
                p2 = x.p2;
              }
              zP4 = x.p4type==P4_INT32 ? SQLITE_INT_TO_PTR(x.p4.i) : x.p4.z;
              sqlite3VdbeAddOp4(v, x.opcode, x.p1, p2, x.p3, zP4, x.p4type);
              sqlite3VdbeChangeP5(v, x.p5);
              VdbeCoverageIf(v, p2!=x.p2);
            }
            nConflictCk--;
            addrConflictCk++;
          }
          /* If the retest fails, issue an abort */
          sqlite3UniqueConstraint(pParse, OE_Abort, pIdx);

          sqlite3VdbeJumpHere(v, addrBypass); /* Terminate the recheck bypass */
        }
        seenReplace = 1;
        break;
      }
    }
    sqlite3VdbeResolveLabel(v, addrUniqueOk);
    if( regR!=regIdx ) sqlite3ReleaseTempRange(pParse, regR, nPkField);
    if( pUpsertClause
     && upsertIpkReturn
     && sqlite3UpsertNextIsIPK(pUpsertClause)
    ){
      sqlite3VdbeGoto(v, upsertIpkDelay+1);
      sqlite3VdbeJumpHere(v, upsertIpkReturn);
      upsertIpkReturn = 0;
    }
  }

  /* If the IPK constraint is a REPLACE, run it last */
  if( ipkTop ){
    sqlite3VdbeGoto(v, ipkTop);
    VdbeComment((v, "Do IPK REPLACE"));
    assert( ipkBottom>0 );
    sqlite3VdbeJumpHere(v, ipkBottom);
  }

  /* Recheck all uniqueness constraints after replace triggers have run */
  testcase( regTrigCnt!=0 && nReplaceTrig==0 );
  assert( regTrigCnt!=0 || nReplaceTrig==0 );
  if( nReplaceTrig ){
    sqlite3VdbeAddOp2(v, OP_IfNot, regTrigCnt, lblRecheckOk);VdbeCoverage(v);
    if( !pPk ){
      if( isUpdate ){
        sqlite3VdbeAddOp3(v, OP_Eq, regNewData, addrRecheck, regOldData);
        sqlite3VdbeChangeP5(v, SQLITE_NOTNULL);
        VdbeCoverage(v);
      }
      sqlite3VdbeAddOp3(v, OP_NotExists, iDataCur, addrRecheck, regNewData);
      VdbeCoverage(v);
      sqlite3RowidConstraint(pParse, OE_Abort, pTab);
    }else{
      sqlite3VdbeGoto(v, addrRecheck);
    }
    sqlite3VdbeResolveLabel(v, lblRecheckOk);
  }

  /* Generate the table record */
  if( HasRowid(pTab) ){
    int regRec = aRegIdx[ix];
    sqlite3VdbeAddOp3(v, OP_MakeRecord, regNewData+1, pTab->nNVCol, regRec);
    sqlite3SetMakeRecordP5(v, pTab);
    if( !bAffinityDone ){
      sqlite3TableAffinity(v, pTab, 0);
    }
  }

  *pbMayReplace = seenReplace;
  VdbeModuleComment((v, "END: GenCnstCks(%d)", seenReplace));
}
