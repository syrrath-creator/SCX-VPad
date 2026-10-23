#ifndef SCX_STATUS_H
#define SCX_STATUS_H
#include "scx_windows.h"
/* One mapping for the Rust codec/runtime boundary. Authorization failures are
 * handled separately, before calling Rust or changing any producer state. */
static __inline NTSTATUS ScxStatus(uint32_t result)
{
    switch (result) {
    case SCX_VPAD_OK: return STATUS_SUCCESS;
    case SCX_VPAD_INVALID_POINTER:
    case SCX_VPAD_INVALID_PACKET: return STATUS_INVALID_PARAMETER;
    case SCX_VPAD_INVALID_LENGTH: return STATUS_INVALID_BUFFER_SIZE;
    case SCX_VPAD_BUFFER_TOO_SMALL: return STATUS_BUFFER_TOO_SMALL;
    case SCX_VPAD_UNSUPPORTED: return STATUS_NOT_SUPPORTED;
    case SCX_VPAD_QUEUE_FULL: return STATUS_DEVICE_BUSY;
    default: return STATUS_INTERNAL_ERROR;
    }
}
#endif
