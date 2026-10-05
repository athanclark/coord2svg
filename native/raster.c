/* Narrow C ABI compiled against installed GDAL headers. No GDAL-owned
 * allocation crosses the boundary. Every call owns and closes its datasets. */
#include <gdal.h>
#include <gdalwarper.h>
#include <ogr_srs_api.h>
#include <cpl_conv.h>
#include <cpl_error.h>
#include <math.h>
#include <stdio.h>
#include <string.h>
#include <ctype.h>
static int same(const char *a,const char *b) {
    while (*a && *b) {if (tolower((unsigned char)*a++)!=tolower((unsigned char)*b++)) return 0;}
    return *a==*b;
}
static int fail(char *error,size_t length,const char *message) {
    snprintf(error,length,"%s: %s",message,CPLGetLastErrorMsg());return 0;
}
void c2s_register(void) {GDALAllRegister();GDALSetCacheMax64(128*1024*1024);}
int c2s_validate(const char *path,char *error,size_t length) {
    GDALDatasetH ds=GDALOpen(path,GA_ReadOnly);
    if (!ds) return fail(error,length,"cannot open raster");
    int count=GDALGetRasterCount(ds);GDALClose(ds);
    if (count!=1) return fail(error,length,"expected one elevation band");
    return 1;
}
static char *wkt(const char *definition) {
    OGRSpatialReferenceH srs=OSRNewSpatialReference(NULL);char *result=NULL;
    if (OSRSetFromUserInput(srs,definition)==OGRERR_NONE) OSRExportToWkt(srs,&result);
    OSRDestroySpatialReference(srs);return result;
}
int c2s_warp(const char *path,const char *projection,const char *declared_datum,int size,double radius,
             double *values,char *error,size_t length) {
    GDALDatasetH src=NULL,dst=NULL;char *target_wkt=NULL;int success=0;
    src=GDALOpen(path,GA_ReadOnly);
    if (!src) {fail(error,length,"cannot open raster");goto done;}
    if (GDALGetRasterCount(src)!=1) {fail(error,length,"expected one elevation band");goto done;}
    const char *stored_datum=GDALGetMetadataItem(src,"VERTICAL_REFERENCE",NULL);
    if (stored_datum && stored_datum[0] && !same(stored_datum,declared_datum)) {
        fail(error,length,"declared vertical datum conflicts with raster metadata");goto done;
    }
    const char *source_wkt=GDALGetProjectionRef(src);double affine[6];
    if (!source_wkt || !source_wkt[0] || GDALGetGeoTransform(src,affine)!=CE_None) {
        fail(error,length,"raster needs CRS and affine georeferencing");goto done;
    }
    GDALRasterBandH band=GDALGetRasterBand(src,1);const char *unit=GDALGetRasterUnitType(band);
    if (unit && unit[0] && strcmp(unit,"m") && strcmp(unit,"metre") && strcmp(unit,"meter") && strcmp(unit,"metres") && strcmp(unit,"meters")) {
        fail(error,length,"elevation unit must be metres");goto done;
    }
    int has_scale=0,has_offset=0;
    double scale=GDALGetRasterScale(band,&has_scale),offset=GDALGetRasterOffset(band,&has_offset);
    if (!has_scale) scale=1.;
    if (!has_offset) offset=0.;
    if (!isfinite(scale) || !isfinite(offset)) {fail(error,length,"invalid raster scale/offset");goto done;}
    target_wkt=wkt(projection);
    if (!target_wkt) {fail(error,length,"invalid target projection");goto done;}
    dst=GDALCreate(GDALGetDriverByName("MEM"),"",size,size,1,GDT_Float64,NULL);
    if (!dst) {fail(error,length,"cannot create working raster");goto done;}
    double step=2.*radius/(size-1);double gt[6]={-radius-step/2.,step,0.,radius+step/2.,0.,-step};
    if (GDALSetGeoTransform(dst,gt)!=CE_None || GDALSetProjection(dst,target_wkt)!=CE_None) {
        fail(error,length,"cannot georeference working raster");goto done;
    }
    GDALRasterBandH output=GDALGetRasterBand(dst,1);
    GDALSetRasterNoDataValue(output,NAN);GDALFillRaster(output,NAN,0.);
    if (GDALReprojectImage(src,NULL,dst,NULL,GRA_Bilinear,0.,0.,NULL,NULL,NULL)!=CE_None) {
        fail(error,length,"bilinear reprojection failed");goto done;
    }
    if (GDALRasterIO(output,GF_Read,0,0,size,size,values,size,size,GDT_Float64,0,0)!=CE_None) {
        fail(error,length,"cannot read working raster");goto done;
    }
    for (size_t i=0;i<(size_t)size*(size_t)size;i++) if (isfinite(values[i])) values[i]=values[i]*scale+offset;
    success=1;
done:
    if (dst) GDALClose(dst);
    if (src) GDALClose(src);
    if (target_wkt) CPLFree(target_wkt);
    return success;
}
int c2s_write(const char *path,const char *projection,int width,int height,
              const double *affine,const double *values,double nodata,
              double scale,double offset,const char *datum,const char *unit,
              char *error,size_t length) {
    char *target_wkt=wkt(projection);GDALDatasetH ds=NULL;int success=0;
    if (!target_wkt) return fail(error,length,"invalid projection");
    ds=GDALCreate(GDALGetDriverByName("GTiff"),path,width,height,1,GDT_Float64,NULL);
    if (!ds) {fail(error,length,"cannot create GeoTIFF");goto done;}
    if (GDALSetGeoTransform(ds,(double*)affine)!=CE_None || GDALSetProjection(ds,target_wkt)!=CE_None) {fail(error,length,"cannot georeference GeoTIFF");goto done;}
    GDALSetMetadataItem(ds,"VERTICAL_REFERENCE",datum,NULL);GDALSetMetadataItem(ds,"AREA_OR_POINT","Point",NULL);
    GDALRasterBandH band=GDALGetRasterBand(ds,1);
    GDALSetRasterNoDataValue(band,nodata);GDALSetRasterScale(band,scale);GDALSetRasterOffset(band,offset);GDALSetRasterUnitType(band,unit);
    if (GDALRasterIO(band,GF_Write,0,0,width,height,(void*)values,width,height,GDT_Float64,0,0)!=CE_None) {fail(error,length,"cannot write GeoTIFF");goto done;}
    success=1;
done:
    if (ds) GDALClose(ds);
    CPLFree(target_wkt);return success;
}
