/* Original SCX per-device UMDF upper-filter experiment. Inactive by default.
 * Policy is read-only here, protected by an administrator/SYSTEM-only ACL.
 * Every authorized HID handle carries a random 256-bit capability, never a PID.
 */
#include "../../driver/scx_windows.h"
#include <bcrypt.h>
#include <initguid.h>
#include <devpkey.h>
typedef struct { uint64_t Lease,Deadline; uint8_t Capability[32]; } SOURCE_POLICY;
extern uint32_t scx_source_decode(const uint8_t*,size_t,uint64_t,SOURCE_POLICY*);
extern uint32_t scx_source_allowed(const SOURCE_POLICY*,uint64_t,const uint8_t*,uint32_t);
_Static_assert(sizeof(SOURCE_POLICY)==48,"Rust policy ABI");
static const GUID SourceInterface={0x8cfcc3b9,0xd6f8,0x46d2,{0x9b,0xaa,0xd6,0x4a,0x3e,0x7f,0x5c,0x51}};
#define SOURCE_STATUS CTL_CODE(FILE_DEVICE_UNKNOWN,0x940,METHOD_BUFFERED,FILE_READ_ACCESS)
#define SOURCE_AUTH CTL_CODE(FILE_DEVICE_UNKNOWN,0x941,METHOD_BUFFERED,FILE_WRITE_ACCESS)
typedef struct { SRWLOCK Lock; SOURCE_POLICY Policy; WCHAR Path[160]; volatile LONG64 Allowed,Denied; volatile LONG Closing; WDFTIMER Timer; } SOURCE;
WDF_DECLARE_CONTEXT_TYPE_WITH_NAME(SOURCE,Source);
typedef struct { SRWLOCK Lock; uint8_t Capability[32]; } SOURCE_FILE;
WDF_DECLARE_CONTEXT_TYPE_WITH_NAME(SOURCE_FILE,SourceFile);
typedef struct { uint8_t Capability[32]; BOOLEAN Sensitive; } REQUEST_SOURCE;
WDF_DECLARE_CONTEXT_TYPE_WITH_NAME(REQUEST_SOURCE,RequestSource);
DRIVER_INITIALIZE DriverEntry;
EVT_WDF_DRIVER_DEVICE_ADD SourceAdd;
EVT_WDF_OBJECT_CONTEXT_CLEANUP SourceCleanup;
EVT_WDF_OBJECT_CONTEXT_CLEANUP SourceFileCleanup;
EVT_WDF_TIMER SourceTick;
EVT_WDF_IO_QUEUE_IO_DEFAULT SourceIo;
EVT_WDF_REQUEST_COMPLETION_ROUTINE SourceComplete;
static BOOLEAN Allowed(SOURCE*ctx,const uint8_t*capability,BOOLEAN authenticate) {
 BOOLEAN allowed;AcquireSRWLockShared(&ctx->Lock);
 allowed=(BOOLEAN)scx_source_allowed(&ctx->Policy,GetTickCount64(),capability,authenticate);
 ReleaseSRWLockShared(&ctx->Lock);
 if(allowed)InterlockedIncrement64(&ctx->Allowed);else InterlockedIncrement64(&ctx->Denied);
 return allowed;
}
static void Refresh(SOURCE*ctx) {
 HKEY key=NULL;DWORD type=0,size=64;uint8_t bytes[64]={0};SOURCE_POLICY next={0};
 if(ctx->Path[0] && RegOpenKeyExW(HKEY_LOCAL_MACHINE,ctx->Path,0,KEY_QUERY_VALUE|KEY_WOW64_64KEY,&key)==ERROR_SUCCESS) {
  LSTATUS code=RegQueryValueExW(key,L"Lease",NULL,&type,bytes,&size);RegCloseKey(key);
  if(code==ERROR_SUCCESS && type==REG_BINARY)scx_source_decode(bytes,size,GetTickCount64(),&next);
 }
 AcquireSRWLockExclusive(&ctx->Lock);ctx->Policy=next;ReleaseSRWLockExclusive(&ctx->Lock);
 SecureZeroMemory(bytes,sizeof(bytes));SecureZeroMemory(&next,sizeof(next));
}
void SourceTick(WDFTIMER timer) {
 SOURCE*ctx=Source((WDFDEVICE)WdfTimerGetParentObject(timer));
 if(!InterlockedCompareExchange(&ctx->Closing,0,0))Refresh(ctx);
 if(!InterlockedCompareExchange(&ctx->Closing,0,0))WdfTimerStart(timer,WDF_REL_TIMEOUT_IN_MS(250));
}
void SourceCleanup(WDFOBJECT object) {
 SOURCE*ctx=Source(object);InterlockedExchange(&ctx->Closing,1);
 if(ctx->Timer){WdfTimerStop(ctx->Timer,TRUE);WdfTimerStop(ctx->Timer,FALSE);}
 AcquireSRWLockExclusive(&ctx->Lock);SecureZeroMemory(&ctx->Policy,sizeof(ctx->Policy));ReleaseSRWLockExclusive(&ctx->Lock);
}
void SourceFileCleanup(WDFOBJECT object) {SOURCE_FILE*f=SourceFile(object);SecureZeroMemory(f->Capability,sizeof(f->Capability));}
void SourceComplete(WDFREQUEST request,WDFIOTARGET target,PWDF_REQUEST_COMPLETION_PARAMS result,WDFCONTEXT unused) {
 REQUEST_SOURCE*r=RequestSource(request);NTSTATUS status=result->IoStatus.Status;ULONG_PTR count=result->IoStatus.Information;
 UNREFERENCED_PARAMETER(unused);
 /* Recheck pending reads after policy activation or rotation; never return their payload. */
 if(r->Sensitive && !Allowed(Source(WdfIoTargetGetDevice(target)),r->Capability,FALSE)){status=STATUS_ACCESS_DENIED;count=0;}
 SecureZeroMemory(r->Capability,sizeof(r->Capability));WdfRequestCompleteWithInformation(request,status,count);
}
void SourceIo(WDFQUEUE queue,WDFREQUEST request) {
 WDFDEVICE device=WdfIoQueueGetDevice(queue);SOURCE*ctx=Source(device);WDF_REQUEST_PARAMETERS params;
 REQUEST_SOURCE*r=RequestSource(request);WDFFILEOBJECT file=WdfRequestGetFileObject(request);ULONG code=0;NTSTATUS status;
 WDF_REQUEST_PARAMETERS_INIT(&params);WdfRequestGetParameters(request,&params);ZeroMemory(r,sizeof(*r));
 if(params.Type==WdfRequestTypeDeviceControl)code=params.Parameters.DeviceIoControl.IoControlCode;
 if(code==SOURCE_STATUS){
  uint64_t data[8]={2,0,0,0,0,0,0,0};PVOID output=NULL;size_t capacity=0;
  AcquireSRWLockShared(&ctx->Lock);data[1]=ctx->Policy.Lease;data[2]=ctx->Policy.Deadline;ReleaseSRWLockShared(&ctx->Lock);
  data[3]=GetTickCount64();data[4]=ctx->Path[0]!=0;data[5]=(uint64_t)InterlockedCompareExchange64(&ctx->Allowed,0,0);data[6]=(uint64_t)InterlockedCompareExchange64(&ctx->Denied,0,0);
  status=WdfRequestRetrieveOutputBuffer(request,sizeof(data),&output,&capacity);
  if(NT_SUCCESS(status)&&output&&capacity>=sizeof(data)){memcpy(output,data,sizeof(data));WdfRequestCompleteWithInformation(request,STATUS_SUCCESS,sizeof(data));}
  else WdfRequestComplete(request,NT_SUCCESS(status)?STATUS_BUFFER_TOO_SMALL:status);
  return;
 }
 if(code==SOURCE_AUTH){
  PVOID input=NULL;size_t length=0;status=WdfRequestRetrieveInputBuffer(request,32,&input,&length);
  if(!NT_SUCCESS(status) || !file || !input || length!=32 || !Allowed(ctx,(const uint8_t*)input,TRUE)){WdfRequestComplete(request,STATUS_ACCESS_DENIED);return;}
  SOURCE_FILE*f=SourceFile(file);AcquireSRWLockExclusive(&f->Lock);memcpy(f->Capability,input,32);ReleaseSRWLockExclusive(&f->Lock);
  WdfRequestComplete(request,STATUS_SUCCESS);return;
 }
 r->Sensitive=params.Type==WdfRequestTypeRead || params.Type==WdfRequestTypeWrite || code==IOCTL_HID_GET_INPUT_REPORT || code==IOCTL_HID_SET_OUTPUT_REPORT || code==IOCTL_HID_SET_FEATURE;
 if(r->Sensitive){
  if(file){SOURCE_FILE*f=SourceFile(file);AcquireSRWLockShared(&f->Lock);memcpy(r->Capability,f->Capability,32);ReleaseSRWLockShared(&f->Lock);}
  if(!Allowed(ctx,r->Capability,FALSE)){SecureZeroMemory(r->Capability,32);WdfRequestCompleteWithInformation(request,STATUS_ACCESS_DENIED,0);return;}
 }
 WdfRequestFormatRequestUsingCurrentType(request);WdfRequestSetCompletionRoutine(request,SourceComplete,NULL);
 if(!WdfRequestSend(request,WdfDeviceGetIoTarget(device),WDF_NO_SEND_OPTIONS)){SecureZeroMemory(r->Capability,32);WdfRequestComplete(request,WdfRequestGetStatus(request));}
}
static void PolicyPath(WDFDEVICE device,SOURCE*ctx) {
 WDF_DEVICE_PROPERTY_DATA property;DEVPROPTYPE type;ULONG needed;WCHAR id[1024]={0};uint8_t hash[32];BCRYPT_ALG_HANDLE algorithm=NULL;
 static const WCHAR prefix[]=L"SOFTWARE\\SCX\\VirtualPad\\IsolationLeases\\";static const WCHAR hex[]=L"0123456789abcdef";size_t i,len;
 WDF_DEVICE_PROPERTY_DATA_INIT(&property,&DEVPKEY_Device_InstanceId);
 if(!NT_SUCCESS(WdfDeviceQueryPropertyEx(device,&property,sizeof(id),id,&needed,&type)) || type!=DEVPROP_TYPE_STRING || needed<sizeof(WCHAR) || needed>sizeof(id) || id[needed/sizeof(WCHAR)-1]!=0)return;
 len=needed/sizeof(WCHAR)-1;for(i=0;i<len;++i)if(id[i]>=L'a'&&id[i]<=L'z')id[i]-=32;
 if(!NT_SUCCESS(BCryptOpenAlgorithmProvider(&algorithm,BCRYPT_SHA256_ALGORITHM,NULL,0)))return;
 NTSTATUS status=BCryptHash(algorithm,NULL,0,(PUCHAR)id,(ULONG)(len*sizeof(WCHAR)),hash,sizeof(hash));BCryptCloseAlgorithmProvider(algorithm,0);
 if(!NT_SUCCESS(status))return;
 memcpy(ctx->Path,prefix,sizeof(prefix));len=sizeof(prefix)/sizeof(WCHAR)-1;
 for(i=0;i<32;++i){ctx->Path[len+2*i]=hex[hash[i]>>4];ctx->Path[len+2*i+1]=hex[hash[i]&15];}ctx->Path[len+64]=0;
}
NTSTATUS SourceAdd(WDFDRIVER driver,PWDFDEVICE_INIT init) {
 WDFDEVICE device;WDF_OBJECT_ATTRIBUTES attributes,requestAttributes,timerAttributes,fileAttributes;WDF_FILEOBJECT_CONFIG files;WDF_IO_QUEUE_CONFIG queue;WDF_TIMER_CONFIG timer;NTSTATUS status;SOURCE*ctx;
 UNREFERENCED_PARAMETER(driver);WdfFdoInitSetFilter(init);
 WDF_OBJECT_ATTRIBUTES_INIT_CONTEXT_TYPE(&requestAttributes,REQUEST_SOURCE);WdfDeviceInitSetRequestAttributes(init,&requestAttributes);
 WDF_OBJECT_ATTRIBUTES_INIT_CONTEXT_TYPE(&fileAttributes,SOURCE_FILE);fileAttributes.EvtCleanupCallback=SourceFileCleanup;
 WDF_FILEOBJECT_CONFIG_INIT(&files,WDF_NO_EVENT_CALLBACK,WDF_NO_EVENT_CALLBACK,WDF_NO_EVENT_CALLBACK);
 files.FileObjectClass=WdfFileObjectWdfCannotUseFsContexts;files.AutoForwardCleanupClose=WdfTrue;
 WdfDeviceInitSetFileObjectConfig(init,&files,&fileAttributes);
 WDF_OBJECT_ATTRIBUTES_INIT_CONTEXT_TYPE(&attributes,SOURCE);attributes.EvtCleanupCallback=SourceCleanup;attributes.ExecutionLevel=WdfExecutionLevelPassive;attributes.SynchronizationScope=WdfSynchronizationScopeNone;
 status=WdfDeviceCreate(&init,&attributes,&device);if(!NT_SUCCESS(status))return status;
 ctx=Source(device);InitializeSRWLock(&ctx->Lock);PolicyPath(device,ctx);Refresh(ctx);
 WDF_IO_QUEUE_CONFIG_INIT_DEFAULT_QUEUE(&queue,WdfIoQueueDispatchParallel);queue.PowerManaged=WdfFalse;queue.EvtIoDefault=SourceIo;
 status=WdfIoQueueCreate(device,&queue,WDF_NO_OBJECT_ATTRIBUTES,WDF_NO_HANDLE);if(!NT_SUCCESS(status))return status;
 status=WdfDeviceCreateDeviceInterface(device,&SourceInterface,NULL);if(!NT_SUCCESS(status))return status;
 WDF_TIMER_CONFIG_INIT(&timer,SourceTick);timer.AutomaticSerialization=FALSE;
 WDF_OBJECT_ATTRIBUTES_INIT(&timerAttributes);timerAttributes.ParentObject=device;
 status=WdfTimerCreate(&timer,&timerAttributes,&ctx->Timer);if(!NT_SUCCESS(status))return status;
 WdfTimerStart(ctx->Timer,WDF_REL_TIMEOUT_IN_MS(250));return STATUS_SUCCESS;
}
NTSTATUS DriverEntry(PDRIVER_OBJECT object,PUNICODE_STRING path){WDF_DRIVER_CONFIG config;WDF_DRIVER_CONFIG_INIT(&config,SourceAdd);return WdfDriverCreate(object,path,WDF_NO_OBJECT_ATTRIBUTES,&config,WDF_NO_HANDLE);}
