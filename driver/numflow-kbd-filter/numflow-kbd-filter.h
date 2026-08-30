#pragma once

#include <ntddk.h>
#include <wdf.h>

typedef enum _NUMFLOW_FILTER_MODE {
    NumFlowFilterModePassThrough = 0
} NUMFLOW_FILTER_MODE;

typedef struct _NUMFLOW_DEVICE_CONTEXT {
    volatile LONG Mode;
    volatile LONG InD0;
} NUMFLOW_DEVICE_CONTEXT, *PNUMFLOW_DEVICE_CONTEXT;

WDF_DECLARE_CONTEXT_TYPE_WITH_NAME(NUMFLOW_DEVICE_CONTEXT, NumFlowGetDeviceContext)

DRIVER_INITIALIZE DriverEntry;

EVT_WDF_DRIVER_DEVICE_ADD NumFlowKbdFilterEvtDeviceAdd;
EVT_WDF_OBJECT_CONTEXT_CLEANUP NumFlowKbdFilterEvtDeviceCleanup;
EVT_WDF_DEVICE_D0_ENTRY NumFlowKbdFilterEvtDeviceD0Entry;
EVT_WDF_DEVICE_D0_EXIT NumFlowKbdFilterEvtDeviceD0Exit;
EVT_WDF_IO_QUEUE_IO_DEFAULT NumFlowKbdFilterEvtIoDefault;
