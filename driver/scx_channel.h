#ifndef SCX_CHANNEL_H
#define SCX_CHANNEL_H
#include "scx_device.h"
NTSTATUS ScxChannelStart(WDFDEVICE device, SCX_DEVICE *ctx);
void ScxChannelPublish(SCX_DEVICE *ctx);
EVT_WDF_OBJECT_CONTEXT_CLEANUP ScxCleanup;
#endif
