/*
    Que datatypes tenga tipos registrados (tras AddDataTypes REFRESH, con el AmigaOS del usuario): abre una imagen
    ILBM del AmigaOS del usuario con NewDTObject y pregunta su tamaño. Solo tiene sentido con el AmigaOS del usuario
    montado (AmigaOS:); sin él, lo dice y acaba con 0. Acaba con 0 si la abre, o con el IoErr si no.
*/
#include <exec/types.h>
#include <exec/memory.h>
#include <dos/dos.h>
#include <datatypes/datatypes.h>
#include <datatypes/datatypesclass.h>
#include <datatypes/pictureclass.h>
#include <intuition/classusr.h>
#include <proto/exec.h>
#include <proto/dos.h>
#include <proto/datatypes.h>
#include <proto/intuition.h>

struct ExecBase *SysBase;
struct DosLibrary *DOSBase;
struct Library *DataTypesBase;
struct IntuitionBase *IntuitionBase;

static void di(const char *f, LONG a, LONG b, LONG c)
{
    LONG v[3] = { a, b, c };
    VPrintf((CONST_STRPTR)f, v);
}

#define IMAGEN "AmigaOS:MUI/Images/default_OS3/HardDisk.mbr"

LONG inicio(void)
{
    SysBase = *(struct ExecBase **)4;
    DOSBase = (struct DosLibrary *)OpenLibrary((CONST_STRPTR)"dos.library", 36);
    if (!DOSBase) return 20;
    LONG r = 0;
    BPTR l = Lock((CONST_STRPTR)IMAGEN, SHARED_LOCK);
    if (!l) {
        di("  [caja-amiga] dtprueba: sin el AmigaOS del usuario: no se prueba\n", 0, 0, 0);
        CloseLibrary((struct Library *)DOSBase);
        return 0;
    }
    UnLock(l);
    IntuitionBase = (struct IntuitionBase *)OpenLibrary((CONST_STRPTR)"intuition.library", 39);
    DataTypesBase = OpenLibrary((CONST_STRPTR)"datatypes.library", 39);
    if (!DataTypesBase || !IntuitionBase) {
        di("  [caja-amiga] dtprueba: datatypes.library no abre\n", 0, 0, 0);
        r = 10;
    } else {
        Object *o = NewDTObject((APTR)IMAGEN, DTA_GroupID, GID_PICTURE, PDTA_Remap, FALSE, TAG_DONE);
        if (!o) {
            r = IoErr();
            di("  [caja-amiga] dtprueba: NewDTObject de una ILBM da 0 (IoErr %ld)\n", r, 0, 0);
        } else {
            struct BitMapHeader *bmh = 0;
            STRPTR nombre = 0;
            struct DataType *dt = 0;
            GetDTAttrs(o, PDTA_BitMapHeader, (ULONG)&bmh, DTA_DataType, (ULONG)&dt, TAG_DONE);
            if (dt) nombre = dt->dtn_Header->dth_Name;
            di("  [caja-amiga] dtprueba: una ILBM abre por datatypes (%s), de %ld x %ld\n", (LONG)(nombre ? nombre : (STRPTR)"?"),
               bmh ? bmh->bmh_Width : -1, bmh ? bmh->bmh_Height : -1);
            DisposeDTObject(o);
        }
    }
    if (DataTypesBase) CloseLibrary(DataTypesBase);
    if (IntuitionBase) CloseLibrary((struct Library *)IntuitionBase);
    CloseLibrary((struct Library *)DOSBase);
    return r;
}
