#ifndef SCX_WINDOWS_H
#define SCX_WINDOWS_H
#define WIN32_NO_STATUS
#pragma warning(push)
#pragma warning(disable: 6553) /* Windows SDK RegOpenKeyEx SAL annotation. */
#include <windows.h>
#pragma warning(pop)
#undef WIN32_NO_STATUS
#pragma warning(push)
/* WDK 2.25 intentionally uses aligned structures and its legacy assert macro. */
#pragma warning(disable: 4324 5104 6553)
#include <wdf.h>
#include <hidport.h>
#pragma warning(pop)
#include <stdint.h>
#include <string.h>
#include "scx_vpad.h"

#endif
