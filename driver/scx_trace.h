#ifndef SCX_TRACE_H
#define SCX_TRACE_H
#include "scx_device.h"
void ScxTraceInitialize(void);
void ScxTraceShutdown(void);
void ScxTraceRequest(SCX_DEVICE *ctx, WDFREQUEST request,
    ULONG code, size_t inLength, size_t outLength, NTSTATUS status);
EVT_WDF_DRIVER_UNLOAD ScxTraceUnload;
#endif
