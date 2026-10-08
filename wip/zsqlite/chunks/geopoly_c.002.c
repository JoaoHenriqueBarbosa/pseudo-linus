

/*
** Implementation of the geopoly_group_bbox(X) aggregate SQL function.
*/
static void geopolyBBoxStep(
  sqlite3_context *context,
  int argc,
  sqlite3_value **argv
){
  RtreeCoord a[4];
  int rc = SQLITE_OK;
  (void)argc;
  (void)geopolyBBox(context, argv[0], a, &rc);
  if( rc==SQLITE_OK ){
    GeoBBox *pBBox;
    pBBox = (GeoBBox*)sqlite3_aggregate_context(context, sizeof(*pBBox));
    if( pBBox==0 ) return;
    if( pBBox->isInit==0 ){
      pBBox->isInit = 1;
      memcpy(pBBox->a, a, sizeof(RtreeCoord)*4);
    }else{
      if( a[0].f < pBBox->a[0].f ) pBBox->a[0] = a[0];
      if( a[1].f > pBBox->a[1].f ) pBBox->a[1] = a[1];
      if( a[2].f < pBBox->a[2].f ) pBBox->a[2] = a[2];
      if( a[3].f > pBBox->a[3].f ) pBBox->a[3] = a[3];
    }
  }
}
static void geopolyBBoxFinal(
  sqlite3_context *context
){
  GeoPoly *p;
  GeoBBox *pBBox;
  pBBox = (GeoBBox*)sqlite3_aggregate_context(context, 0);
  if( pBBox==0 ) return;
  p = geopolyBBox(context, 0, pBBox->a, 0);
  if( p ){
    sqlite3_result_blob(context, p->hdr,
       4+8*p->nVertex, SQLITE_TRANSIENT);
    sqlite3_free(p);
  }
}


/*
** Determine if point (x0,y0) is beneath line segment (x1,y1)->(x2,y2).
** Returns:
**
**    +2  x0,y0 is on the line segement
**
**    +1  x0,y0 is beneath line segment
**
**    0   x0,y0 is not on or beneath the line segment or the line segment
**        is vertical and x0,y0 is not on the line segment
**
** The left-most coordinate min(x1,x2) is not considered to be part of
** the line segment for the purposes of this analysis.
*/
static int pointBeneathLine(
  double x0, double y0,
  double x1, double y1,
  double x2, double y2
){
  double y;
  if( x0==x1 && y0==y1 ) return 2;
  if( x1<x2 ){
    if( x0<=x1 || x0>x2 ) return 0;
  }else if( x1>x2 ){
    if( x0<=x2 || x0>x1 ) return 0;
  }else{
    /* Vertical line segment */
    if( x0!=x1 ) return 0;
    if( y0<y1 && y0<y2 ) return 0;
    if( y0>y1 && y0>y2 ) return 0;
    return 2;
  }
  y = y1 + (y2-y1)*(x0-x1)/(x2-x1);
  if( y0==y ) return 2;
  if( y0<y ) return 1;
  return 0;
}

/*
** SQL function:    geopoly_contains_point(P,X,Y)
**
** Return +2 if point X,Y is within polygon P.
** Return +1 if point X,Y is on the polygon boundary.
** Return 0 if point X,Y is outside the polygon
*/
static void geopolyContainsPointFunc(
  sqlite3_context *context,
  int argc,
  sqlite3_value **argv
){
  GeoPoly *p1 = geopolyFuncParam(context, argv[0], 0);
  double x0 = sqlite3_value_double(argv[1]);
  double y0 = sqlite3_value_double(argv[2]);
  int v = 0;
  int cnt = 0;
  int ii;
  (void)argc;

  if( p1==0 ) return;
  for(ii=0; ii<p1->nVertex-1; ii++){
    v = pointBeneathLine(x0,y0,GeoX(p1,ii), GeoY(p1,ii),
                               GeoX(p1,ii+1),GeoY(p1,ii+1));
    if( v==2 ) break;
    cnt += v;
  }
  if( v!=2 ){
    v = pointBeneathLine(x0,y0,GeoX(p1,ii), GeoY(p1,ii),
                               GeoX(p1,0),  GeoY(p1,0));
  }
  if( v==2 ){
    sqlite3_result_int(context, 1);
  }else if( ((v+cnt)&1)==0 ){
    sqlite3_result_int(context, 0);
  }else{
    sqlite3_result_int(context, 2);
  }
  sqlite3_free(p1);
}

/* Forward declaration */
static int geopolyOverlap(GeoPoly *p1, GeoPoly *p2);

/*
** SQL function:    geopoly_within(P1,P2)
**
** Return +2 if P1 and P2 are the same polygon
** Return +1 if P2 is contained within P1
** Return 0 if any part of P2 is on the outside of P1
**
*/
static void geopolyWithinFunc(
  sqlite3_context *context,
  int argc,
  sqlite3_value **argv
){
  GeoPoly *p1 = geopolyFuncParam(context, argv[0], 0);
  GeoPoly *p2 = geopolyFuncParam(context, argv[1], 0);
  (void)argc;
  if( p1 && p2 ){
    int x = geopolyOverlap(p1, p2);
    if( x<0 ){
      sqlite3_result_error_nomem(context);
    }else{
      sqlite3_result_int(context, x==2 ? 1 : x==4 ? 2 : 0);
    }
  }
  sqlite3_free(p1);
  sqlite3_free(p2);
}

/* Objects used by the overlap algorihm. */
typedef struct GeoEvent GeoEvent;
typedef struct GeoSegment GeoSegment;
typedef struct GeoOverlap GeoOverlap;
struct GeoEvent {
  double x;              /* X coordinate at which event occurs */
  int eType;             /* 0 for ADD, 1 for REMOVE */
  GeoSegment *pSeg;      /* The segment to be added or removed */
  GeoEvent *pNext;       /* Next event in the sorted list */
};
struct GeoSegment {
  double C, B;           /* y = C*x + B */
  double y;              /* Current y value */
  float y0;              /* Initial y value */
  unsigned char side;    /* 1 for p1, 2 for p2 */
  unsigned int idx;      /* Which segment within the side */
  GeoSegment *pNext;     /* Next segment in a list sorted by y */
};
struct GeoOverlap {
  GeoEvent *aEvent;          /* Array of all events */
  GeoSegment *aSegment;      /* Array of all segments */
  int nEvent;                /* Number of events */
  int nSegment;              /* Number of segments */
};

/*
** Add a single segment and its associated events.
*/
static void geopolyAddOneSegment(
  GeoOverlap *p,
  GeoCoord x0,
  GeoCoord y0,
  GeoCoord x1,
  GeoCoord y1,
  unsigned char side,
  unsigned int idx
){
  GeoSegment *pSeg;
  GeoEvent *pEvent;
  if( x0==x1 ) return;  /* Ignore vertical segments */
  if( x0>x1 ){
    GeoCoord t = x0;
    x0 = x1;
    x1 = t;
    t = y0;
    y0 = y1;
    y1 = t;
  }
  pSeg = p->aSegment + p->nSegment;
  p->nSegment++;
  pSeg->C = (y1-y0)/(x1-x0);
  pSeg->B = y1 - x1*pSeg->C;
  pSeg->y0 = y0;
  pSeg->side = side;
  pSeg->idx = idx;
  pEvent = p->aEvent + p->nEvent;
  p->nEvent++;
  pEvent->x = x0;
  pEvent->eType = 0;
  pEvent->pSeg = pSeg;
  pEvent = p->aEvent + p->nEvent;
  p->nEvent++;
  pEvent->x = x1;
  pEvent->eType = 1;
  pEvent->pSeg = pSeg;
}



/*
** Insert all segments and events for polygon pPoly.
*/
static void geopolyAddSegments(
  GeoOverlap *p,          /* Add segments to this Overlap object */
  GeoPoly *pPoly,         /* Take all segments from this polygon */
  unsigned char side      /* The side of pPoly */
){
  unsigned int i;
  GeoCoord *x;
  for(i=0; i<(unsigned)pPoly->nVertex-1; i++){
    x = &GeoX(pPoly,i);
    geopolyAddOneSegment(p, x[0], x[1], x[2], x[3], side, i);
  }
  x = &GeoX(pPoly,i);
  geopolyAddOneSegment(p, x[0], x[1], pPoly->a[0], pPoly->a[1], side, i);
}

/*
** Merge two lists of sorted events by X coordinate
*/
static GeoEvent *geopolyEventMerge(GeoEvent *pLeft, GeoEvent *pRight){
  GeoEvent head, *pLast;
  head.pNext = 0;
  pLast = &head;
  while( pRight && pLeft ){
    if( pRight->x <= pLeft->x ){
      pLast->pNext = pRight;
      pLast = pRight;
      pRight = pRight->pNext;
    }else{
      pLast->pNext = pLeft;
      pLast = pLeft;
      pLeft = pLeft->pNext;
    }
  }
  pLast->pNext = pRight ? pRight : pLeft;
  return head.pNext;
}

/*
** Sort an array of nEvent event objects into a list.
*/
static GeoEvent *geopolySortEventsByX(GeoEvent *aEvent, int nEvent){
  int mx = 0;
  int i, j;
  GeoEvent *p;
  GeoEvent *a[50];
  for(i=0; i<nEvent; i++){
    p = &aEvent[i];
    p->pNext = 0;
    for(j=0; j<mx && a[j]; j++){
      p = geopolyEventMerge(a[j], p);
      a[j] = 0;
    }
    a[j] = p;
    if( j>=mx ) mx = j+1;
  }
  p = 0;
  for(i=0; i<mx; i++){
    p = geopolyEventMerge(a[i], p);
  }
  return p;
}

/*
** Merge two lists of sorted segments by Y, and then by C.
*/
static GeoSegment *geopolySegmentMerge(GeoSegment *pLeft, GeoSegment *pRight){
  GeoSegment head, *pLast;
  head.pNext = 0;
  pLast = &head;
  while( pRight && pLeft ){
    double r = pRight->y - pLeft->y;
    if( r==0.0 ) r = pRight->C - pLeft->C;
    if( r<0.0 ){
      pLast->pNext = pRight;
      pLast = pRight;
      pRight = pRight->pNext;
    }else{
      pLast->pNext = pLeft;
      pLast = pLeft;
      pLeft = pLeft->pNext;
    }
  }
  pLast->pNext = pRight ? pRight : pLeft;
  return head.pNext;
}

/*
** Sort a list of GeoSegments in order of increasing Y and in the event of
** a tie, increasing C (slope).
*/
static GeoSegment *geopolySortSegmentsByYAndC(GeoSegment *pList){
  int mx = 0;
  int i;
  GeoSegment *p;
  GeoSegment *a[50];
  while( pList ){
    p = pList;
    pList = pList->pNext;
    p->pNext = 0;
    for(i=0; i<mx && a[i]; i++){
      p = geopolySegmentMerge(a[i], p);
      a[i] = 0;
    }
    a[i] = p;
    if( i>=mx ) mx = i+1;
  }
  p = 0;
  for(i=0; i<mx; i++){
    p = geopolySegmentMerge(a[i], p);
  }
  return p;
}

/*
** Determine the overlap between two polygons
*/
static int geopolyOverlap(GeoPoly *p1, GeoPoly *p2){
  sqlite3_int64 nVertex = p1->nVertex + p2->nVertex + 2;
  GeoOverlap *p;
  sqlite3_int64 nByte;
  GeoEvent *pThisEvent;
  double rX;
  int rc = 0;
  int needSort = 0;
  GeoSegment *pActive = 0;
  GeoSegment *pSeg;
  unsigned char aOverlap[4];

  nByte = sizeof(GeoEvent)*nVertex*2
           + sizeof(GeoSegment)*nVertex
           + sizeof(GeoOverlap);
  p = sqlite3_malloc64( nByte );
  if( p==0 ) return -1;
  p->aEvent = (GeoEvent*)&p[1];
  p->aSegment = (GeoSegment*)&p->aEvent[nVertex*2];
  p->nEvent = p->nSegment = 0;
  geopolyAddSegments(p, p1, 1);
  geopolyAddSegments(p, p2, 2);
  pThisEvent = geopolySortEventsByX(p->aEvent, p->nEvent);
  rX = pThisEvent && pThisEvent->x==0.0 ? -1.0 : 0.0;
  memset(aOverlap, 0, sizeof(aOverlap));
  while( pThisEvent ){
    if( pThisEvent->x!=rX ){
      GeoSegment *pPrev = 0;
      int iMask = 0;
      GEODEBUG(("Distinct X: %g\n", pThisEvent->x));
      rX = pThisEvent->x;
      if( needSort ){
        GEODEBUG(("SORT\n"));
        pActive = geopolySortSegmentsByYAndC(pActive);
        needSort = 0;
      }
      for(pSeg=pActive; pSeg; pSeg=pSeg->pNext){
        if( pPrev ){
          if( pPrev->y!=pSeg->y ){
            GEODEBUG(("MASK: %d\n", iMask));
            aOverlap[iMask] = 1;
          }
        }
        iMask ^= pSeg->side;
        pPrev = pSeg;
      }
      pPrev = 0;
      for(pSeg=pActive; pSeg; pSeg=pSeg->pNext){
        double y = pSeg->C*rX + pSeg->B;
        GEODEBUG(("Segment %d.%d %g->%g\n", pSeg->side, pSeg->idx, pSeg->y, y));
        pSeg->y = y;
        if( pPrev ){
          if( pPrev->y>pSeg->y && pPrev->side!=pSeg->side ){
            rc = 1;
            GEODEBUG(("Crossing: %d.%d and %d.%d\n",
                    pPrev->side, pPrev->idx,
                    pSeg->side, pSeg->idx));
            goto geopolyOverlapDone;
          }else if( pPrev->y!=pSeg->y ){
            GEODEBUG(("MASK: %d\n", iMask));
            aOverlap[iMask] = 1;
          }
        }
        iMask ^= pSeg->side;
        pPrev = pSeg;
      }
    }
    GEODEBUG(("%s %d.%d C=%g B=%g\n",
      pThisEvent->eType ? "RM " : "ADD",
      pThisEvent->pSeg->side, pThisEvent->pSeg->idx,
      pThisEvent->pSeg->C,
      pThisEvent->pSeg->B));
    if( pThisEvent->eType==0 ){
      /* Add a segment */
      pSeg = pThisEvent->pSeg;
      pSeg->y = pSeg->y0;
      pSeg->pNext = pActive;
      pActive = pSeg;
      needSort = 1;
    }else{
      /* Remove a segment */
      if( pActive==pThisEvent->pSeg ){
        pActive = ALWAYS(pActive) ? pActive->pNext : 0;
      }else{
        for(pSeg=pActive; pSeg; pSeg=pSeg->pNext){
          if( pSeg->pNext==pThisEvent->pSeg ){
            pSeg->pNext = ALWAYS(pSeg->pNext) ? pSeg->pNext->pNext : 0;
            break;
          }
        }
      }
    }
    pThisEvent = pThisEvent->pNext;
  }
  if( aOverlap[3]==0 ){
    rc = 0;
  }else if( aOverlap[1]!=0 && aOverlap[2]==0 ){
    rc = 3;
  }else if( aOverlap[1]==0 && aOverlap[2]!=0 ){
    rc = 2;
  }else if( aOverlap[1]==0 && aOverlap[2]==0 ){
    rc = 4;
  }else{
    rc = 1;
  }

geopolyOverlapDone:
  sqlite3_free(p);
  return rc;
}
