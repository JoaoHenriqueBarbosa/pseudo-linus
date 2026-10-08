
/*
** SQL function:     geopoly_json(X)
**
** Interpret X as a polygon and render it as a JSON array
** of coordinates.  Or, if X is not a valid polygon, return NULL.
*/
static void geopolyJsonFunc(
  sqlite3_context *context,
  int argc,
  sqlite3_value **argv
){
  GeoPoly *p = geopolyFuncParam(context, argv[0], 0);
  (void)argc;
  if( p ){
    sqlite3 *db = sqlite3_context_db_handle(context);
    sqlite3_str *x = sqlite3_str_new(db);
    int i;
    sqlite3_str_append(x, "[", 1);
    for(i=0; i<p->nVertex; i++){
      sqlite3_str_appendf(x, "[%!g,%!g],", GeoX(p,i), GeoY(p,i));
    }
    sqlite3_str_appendf(x, "[%!g,%!g]]", GeoX(p,0), GeoY(p,0));
    sqlite3_result_text(context, sqlite3_str_finish(x), -1, sqlite3_free);
    sqlite3_free(p);
  }
}

/*
** SQL function:     geopoly_svg(X, ....)
**
** Interpret X as a polygon and render it as a SVG <polyline>.
** Additional arguments are added as attributes to the <polyline>.
*/
static void geopolySvgFunc(
  sqlite3_context *context,
  int argc,
  sqlite3_value **argv
){
  GeoPoly *p;
  if( argc<1 ) return;
  p = geopolyFuncParam(context, argv[0], 0);
  if( p ){
    sqlite3 *db = sqlite3_context_db_handle(context);
    sqlite3_str *x = sqlite3_str_new(db);
    int i;
    char cSep = '\'';
    sqlite3_str_appendf(x, "<polyline points=");
    for(i=0; i<p->nVertex; i++){
      sqlite3_str_appendf(x, "%c%g,%g", cSep, GeoX(p,i), GeoY(p,i));
      cSep = ' ';
    }
    sqlite3_str_appendf(x, " %g,%g'", GeoX(p,0), GeoY(p,0));
    for(i=1; i<argc; i++){
      const char *z = (const char*)sqlite3_value_text(argv[i]);
      if( z && z[0] ){
        sqlite3_str_appendf(x, " %s", z);
      }
    }
    sqlite3_str_appendf(x, "></polyline>");
    sqlite3_result_text(context, sqlite3_str_finish(x), -1, sqlite3_free);
    sqlite3_free(p);
  }
}

/*
** SQL Function:      geopoly_xform(poly, A, B, C, D, E, F)
**
** Transform and/or translate a polygon as follows:
**
**      x1 = A*x0 + B*y0 + E
**      y1 = C*x0 + D*y0 + F
**
** For a translation:
**
**      geopoly_xform(poly, 1, 0, 0, 1, x-offset, y-offset)
**
** Rotate by R around the point (0,0):
**
**      geopoly_xform(poly, cos(R), sin(R), -sin(R), cos(R), 0, 0)
*/
static void geopolyXformFunc(
  sqlite3_context *context,
  int argc,
  sqlite3_value **argv
){
  GeoPoly *p = geopolyFuncParam(context, argv[0], 0);
  double A = sqlite3_value_double(argv[1]);
  double B = sqlite3_value_double(argv[2]);
  double C = sqlite3_value_double(argv[3]);
  double D = sqlite3_value_double(argv[4]);
  double E = sqlite3_value_double(argv[5]);
  double F = sqlite3_value_double(argv[6]);
  GeoCoord x1, y1, x0, y0;
  int ii;
  (void)argc;
  if( p ){
    for(ii=0; ii<p->nVertex; ii++){
      x0 = GeoX(p,ii);
      y0 = GeoY(p,ii);
      x1 = (GeoCoord)(A*x0 + B*y0 + E);
      y1 = (GeoCoord)(C*x0 + D*y0 + F);
      GeoX(p,ii) = x1;
      GeoY(p,ii) = y1;
    }
    sqlite3_result_blob(context, p->hdr,
       4+8*p->nVertex, SQLITE_TRANSIENT);
    sqlite3_free(p);
  }
}

/*
** Compute the area enclosed by the polygon.
**
** This routine can also be used to detect polygons that rotate in
** the wrong direction.  Polygons are suppose to be counter-clockwise (CCW).
** This routine returns a negative value for clockwise (CW) polygons.
*/
static double geopolyArea(GeoPoly *p){
  double rArea = 0.0;
  int ii;
  for(ii=0; ii<p->nVertex-1; ii++){
    rArea += (GeoX(p,ii) - GeoX(p,ii+1))           /* (x0 - x1) */
              * (GeoY(p,ii) + GeoY(p,ii+1))        /* (y0 + y1) */
              * 0.5;
  }
  rArea += (GeoX(p,ii) - GeoX(p,0))                /* (xN - x0) */
           * (GeoY(p,ii) + GeoY(p,0))              /* (yN + y0) */
           * 0.5;
  return rArea;
}

/*
** Implementation of the geopoly_area(X) function.
**
** If the input is a well-formed Geopoly BLOB then return the area
** enclosed by the polygon.  If the polygon circulates clockwise instead
** of counterclockwise (as it should) then return the negative of the
** enclosed area.  Otherwise return NULL.
*/
static void geopolyAreaFunc(
  sqlite3_context *context,
  int argc,
  sqlite3_value **argv
){
  GeoPoly *p = geopolyFuncParam(context, argv[0], 0);
  (void)argc;
  if( p ){
    sqlite3_result_double(context, geopolyArea(p));
    sqlite3_free(p);
  }
}

/*
** Implementation of the geopoly_ccw(X) function.
**
** If the rotation of polygon X is clockwise (incorrect) instead of
** counter-clockwise (the correct winding order according to RFC7946)
** then reverse the order of the vertexes in polygon X.
**
** In other words, this routine returns a CCW polygon regardless of the
** winding order of its input.
**
** Use this routine to sanitize historical inputs that that sometimes
** contain polygons that wind in the wrong direction.
*/
static void geopolyCcwFunc(
  sqlite3_context *context,
  int argc,
  sqlite3_value **argv
){
  GeoPoly *p = geopolyFuncParam(context, argv[0], 0);
  (void)argc;
  if( p ){
    if( geopolyArea(p)<0.0 ){
      int ii, jj;
      for(ii=1, jj=p->nVertex-1; ii<jj; ii++, jj--){
        GeoCoord t = GeoX(p,ii);
        GeoX(p,ii) = GeoX(p,jj);
        GeoX(p,jj) = t;
        t = GeoY(p,ii);
        GeoY(p,ii) = GeoY(p,jj);
        GeoY(p,jj) = t;
      }
    }
    sqlite3_result_blob(context, p->hdr,
       4+8*p->nVertex, SQLITE_TRANSIENT);
    sqlite3_free(p);
  }
}

#define GEOPOLY_PI 3.1415926535897932385

/* Fast approximation for sine(X) for X between -0.5*pi and 2*pi
*/
static double geopolySine(double r){
  assert( r>=-0.5*GEOPOLY_PI && r<=2.0*GEOPOLY_PI );
  if( r>=1.5*GEOPOLY_PI ){
    r -= 2.0*GEOPOLY_PI;
  }
  if( r>=0.5*GEOPOLY_PI ){
    return -geopolySine(r-GEOPOLY_PI);
  }else{
    double r2 = r*r;
    double r3 = r2*r;
    double r5 = r3*r2;
    return 0.9996949*r - 0.1656700*r3 + 0.0075134*r5;
  }
}

/*
** Function:   geopoly_regular(X,Y,R,N)
**
** Construct a simple, convex, regular polygon centered at X, Y
** with circumradius R and with N sides.
*/
static void geopolyRegularFunc(
  sqlite3_context *context,
  int argc,
  sqlite3_value **argv
){
  double x = sqlite3_value_double(argv[0]);
  double y = sqlite3_value_double(argv[1]);
  double r = sqlite3_value_double(argv[2]);
  int n = sqlite3_value_int(argv[3]);
  int i;
  GeoPoly *p;
  (void)argc;

  if( n<3 || r<=0.0 ) return;
  if( n>1000 ) n = 1000;
  p = sqlite3_malloc64( sizeof(*p) + (n-1)*2*sizeof(GeoCoord) );
  if( p==0 ){
    sqlite3_result_error_nomem(context);
    return;
  }
  i = 1;
  p->hdr[0] = *(unsigned char*)&i;
  p->hdr[1] = 0;
  p->hdr[2] = (n>>8)&0xff;
  p->hdr[3] = n&0xff;
  for(i=0; i<n; i++){
    double rAngle = 2.0*GEOPOLY_PI*i/n;
    GeoX(p,i) = x - r*geopolySine(rAngle-0.5*GEOPOLY_PI);
    GeoY(p,i) = y + r*geopolySine(rAngle);
  }
  sqlite3_result_blob(context, p->hdr, 4+8*n, SQLITE_TRANSIENT);
  sqlite3_free(p);
}

/*
** If pPoly is a polygon, compute its bounding box. Then:
**
**    (1) if aCoord!=0 store the bounding box in aCoord, returning NULL
**    (2) otherwise, compute a GeoPoly for the bounding box and return the
**        new GeoPoly
**
** If pPoly is NULL but aCoord is not NULL, then compute a new GeoPoly from
** the bounding box in aCoord and return a pointer to that GeoPoly.
*/
static GeoPoly *geopolyBBox(
  sqlite3_context *context,   /* For recording the error */
  sqlite3_value *pPoly,       /* The polygon */
  RtreeCoord *aCoord,         /* Results here */
  int *pRc                    /* Error code here */
){
  GeoPoly *pOut = 0;
  GeoPoly *p;
  float mnX, mxX, mnY, mxY;
  if( pPoly==0 && aCoord!=0 ){
    p = 0;
    mnX = aCoord[0].f;
    mxX = aCoord[1].f;
    mnY = aCoord[2].f;
    mxY = aCoord[3].f;
    goto geopolyBboxFill;
  }else{
    p = geopolyFuncParam(context, pPoly, pRc);
  }
  if( p ){
    int ii;
    mnX = mxX = GeoX(p,0);
    mnY = mxY = GeoY(p,0);
    for(ii=1; ii<p->nVertex; ii++){
      double r = GeoX(p,ii);
      if( r<mnX ) mnX = (float)r;
      else if( r>mxX ) mxX = (float)r;
      r = GeoY(p,ii);
      if( r<mnY ) mnY = (float)r;
      else if( r>mxY ) mxY = (float)r;
    }
    if( pRc ) *pRc = SQLITE_OK;
    if( aCoord==0 ){
      geopolyBboxFill:
      pOut = sqlite3_realloc64(p, GEOPOLY_SZ(4));
      if( pOut==0 ){
        sqlite3_free(p);
        if( context ) sqlite3_result_error_nomem(context);
        if( pRc ) *pRc = SQLITE_NOMEM;
        return 0;
      }
      pOut->nVertex = 4;
      ii = 1;
      pOut->hdr[0] = *(unsigned char*)&ii;
      pOut->hdr[1] = 0;
      pOut->hdr[2] = 0;
      pOut->hdr[3] = 4;
      GeoX(pOut,0) = mnX;
      GeoY(pOut,0) = mnY;
      GeoX(pOut,1) = mxX;
      GeoY(pOut,1) = mnY;
      GeoX(pOut,2) = mxX;
      GeoY(pOut,2) = mxY;
      GeoX(pOut,3) = mnX;
      GeoY(pOut,3) = mxY;
    }else{
      sqlite3_free(p);
      aCoord[0].f = mnX;
      aCoord[1].f = mxX;
      aCoord[2].f = mnY;
      aCoord[3].f = mxY;
    }
  }else if( aCoord ){
    memset(aCoord, 0, sizeof(RtreeCoord)*4);
  }
  return pOut;
}

/*
** Implementation of the geopoly_bbox(X) SQL function.
*/
static void geopolyBBoxFunc(
  sqlite3_context *context,
  int argc,
  sqlite3_value **argv
){
  GeoPoly *p = geopolyBBox(context, argv[0], 0, 0);
  (void)argc;
  if( p ){
    sqlite3_result_blob(context, p->hdr,
       4+8*p->nVertex, SQLITE_TRANSIENT);
    sqlite3_free(p);
  }
}

/*
** State vector for the geopoly_group_bbox() aggregate function.
*/
typedef struct GeoBBox GeoBBox;
struct GeoBBox {
  int isInit;
  RtreeCoord a[4];
};
