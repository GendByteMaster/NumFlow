#include "numflow-kbd-filter.h"

#if defined(ALLOC_PRAGMA)
#pragma alloc_text(INIT, DriverEntry)
#pragma alloc_text(PAGE, NumFlowKbdFilterEvtDeviceAdd)
#endif

_Use_decl_annotations_
NTSTATUS
DriverEntry(
    PDRIVER_OBJECT DriverObject,
    PUNICODE_STRING RegistryPath
    )
{
    WDF_DRIVER_CONFIG config;
    WDF_OBJECT_ATTRIBUTES attributes;

    WDF_DRIVER_CONFIG_INIT(&config, NumFlowKbdFilterEvtDeviceAdd);
    WDF_OBJECT_ATTRIBUTES_INIT(&attributes);

    return WdfDriverCreate(
        DriverObject,
        RegistryPath,
        &attributes,
        &config,
        WDF_NO_HANDLE);
}

_Use_decl_annotations_
NTSTATUS
NumFlowKbdFilterEvtDeviceAdd(
    WDFDRIVER Driver,
    PWDFDEVICE_INIT DeviceInit
    )
{
    WDFDEVICE device;
    WDF_IO_QUEUE_CONFIG queueConfig;
    WDF_OBJECT_ATTRIBUTES deviceAttributes;
    WDF_PNPPOWER_EVENT_CALLBACKS pnpPowerCallbacks;
    PNUMFLOW_DEVICE_CONTEXT context;
    NTSTATUS status;

    PAGED_CODE();
    UNREFERENCED_PARAMETER(Driver);

    // This is a filter device. KMDF forwards PnP and power requests that the
    // driver does not handle, while the default queue below explicitly forwards
    // every queued request without inspecting or modifying its buffers.
    WdfFdoInitSetFilter(DeviceInit);

    WDF_PNPPOWER_EVENT_CALLBACKS_INIT(&pnpPowerCallbacks);
    pnpPowerCallbacks.EvtDeviceD0Entry = NumFlowKbdFilterEvtDeviceD0Entry;
    pnpPowerCallbacks.EvtDeviceD0Exit = NumFlowKbdFilterEvtDeviceD0Exit;
    WdfDeviceInitSetPnpPowerEventCallbacks(DeviceInit, &pnpPowerCallbacks);

    WDF_OBJECT_ATTRIBUTES_INIT_CONTEXT_TYPE(
        &deviceAttributes,
        NUMFLOW_DEVICE_CONTEXT);
    deviceAttributes.EvtCleanupCallback = NumFlowKbdFilterEvtDeviceCleanup;

    status = WdfDeviceCreate(&DeviceInit, &deviceAttributes, &device);
    if (!NT_SUCCESS(status)) {
        return status;
    }

    context = NumFlowGetDeviceContext(device);
    InterlockedExchange(&context->Mode, NumFlowFilterModePassThrough);
    InterlockedExchange(&context->InD0, FALSE);

    WDF_IO_QUEUE_CONFIG_INIT_DEFAULT_QUEUE(
        &queueConfig,
        WdfIoQueueDispatchParallel);
    queueConfig.EvtIoDefault = NumFlowKbdFilterEvtIoDefault;

    return WdfIoQueueCreate(
        device,
        &queueConfig,
        WDF_NO_OBJECT_ATTRIBUTES,
        WDF_NO_HANDLE);
}

_Use_decl_annotations_
VOID
NumFlowKbdFilterEvtDeviceCleanup(
    WDFOBJECT DeviceObject
    )
{
    PNUMFLOW_DEVICE_CONTEXT context;

    context = NumFlowGetDeviceContext((WDFDEVICE)DeviceObject);
    InterlockedExchange(&context->Mode, NumFlowFilterModePassThrough);
    InterlockedExchange(&context->InD0, FALSE);
}

_Use_decl_annotations_
NTSTATUS
NumFlowKbdFilterEvtDeviceD0Entry(
    WDFDEVICE Device,
    WDF_POWER_DEVICE_STATE PreviousState
    )
{
    PNUMFLOW_DEVICE_CONTEXT context;

    UNREFERENCED_PARAMETER(PreviousState);

    context = NumFlowGetDeviceContext(Device);
    InterlockedExchange(&context->Mode, NumFlowFilterModePassThrough);
    InterlockedExchange(&context->InD0, TRUE);

    return STATUS_SUCCESS;
}

_Use_decl_annotations_
NTSTATUS
NumFlowKbdFilterEvtDeviceD0Exit(
    WDFDEVICE Device,
    WDF_POWER_DEVICE_STATE TargetState
    )
{
    PNUMFLOW_DEVICE_CONTEXT context;

    UNREFERENCED_PARAMETER(TargetState);

    context = NumFlowGetDeviceContext(Device);
    InterlockedExchange(&context->Mode, NumFlowFilterModePassThrough);
    InterlockedExchange(&context->InD0, FALSE);

    return STATUS_SUCCESS;
}

_Use_decl_annotations_
VOID
NumFlowKbdFilterEvtIoDefault(
    WDFQUEUE Queue,
    WDFREQUEST Request
    )
{
    WDF_REQUEST_SEND_OPTIONS options;
    WDFDEVICE device;

    device = WdfIoQueueGetDevice(Queue);

    WdfRequestFormatRequestUsingCurrentType(Request);

    WDF_REQUEST_SEND_OPTIONS_INIT(
        &options,
        WDF_REQUEST_SEND_OPTION_SEND_AND_FORGET);

    if (!WdfRequestSend(
            Request,
            WdfDeviceGetIoTarget(device),
            &options)) {
        WdfRequestComplete(Request, WdfRequestGetStatus(Request));
    }
}
