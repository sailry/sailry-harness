package core

import (
	"fmt"
	"runtime"
	"syscall"
	"unsafe"

	"golang.org/x/sys/windows"
)

var (
	trashOle32        = windows.NewLazySystemDLL("ole32.dll")
	trashShell32      = windows.NewLazySystemDLL("shell32.dll")
	trashInitialize   = trashOle32.NewProc("CoInitializeEx")
	trashUninitialize = trashOle32.NewProc("CoUninitialize")
	trashCreate       = trashOle32.NewProc("CoCreateInstance")
	trashCreateItem   = trashShell32.NewProc("SHCreateItemFromParsingName")
	trashClass        = windows.GUID{Data1: 0x3ad05575, Data2: 0x8857, Data3: 0x4850, Data4: [8]byte{0x92, 0x77, 0x11, 0xb8, 0x5b, 0xdb, 0x8e, 0x09}}
	trashOperationID  = windows.GUID{Data1: 0x947aab5f, Data2: 0x0a5c, Data3: 0x4c13, Data4: [8]byte{0xb4, 0xd6, 0x4b, 0xf7, 0x83, 0x6f, 0xc9, 0xf8}}
	trashShellItemID  = windows.GUID{Data1: 0x43826d1e, Data2: 0xe718, Data3: 0x42ee, Data4: [8]byte{0xbc, 0x55, 0xa1, 0xe2, 0x61, 0xc3, 0x7b, 0xfe}}
)

const (
	trashNoUI                = 1556
	trashNoConnectedElements = 8192
	trashRecycleOnDelete     = 524288
	trashEarlyFailure        = 1048576
)

type trashOperationVTable struct {
	queryInterface, addRef, release                               uintptr
	advise, unadvise, setOperationFlags                           uintptr
	setProgressMessage, setProgressDialog, setProperties          uintptr
	setOwnerWindow, applyPropertiesToItem, applyPropertiesToItems uintptr
	renameItem, renameItems, moveItem, moveItems                  uintptr
	copyItem, copyItems, deleteItem, deleteItems, newItem         uintptr
	performOperations, getAnyOperationsAborted                    uintptr
}

type trashOperationObject struct{ vtable *trashOperationVTable }
type trashShellItem struct{ vtable *[3]uintptr }

func trashNative(target string) (string, error) {
	runtime.LockOSThread()
	defer runtime.UnlockOSThread()
	result, _, _ := trashInitialize.Call(0, 2|4)
	if err := trashHRESULT(result); err != nil {
		return "", err
	}
	defer trashUninitialize.Call()
	var operation *trashOperationObject
	result, _, _ = trashCreate.Call(uintptr(unsafe.Pointer(&trashClass)), 0, 1, uintptr(unsafe.Pointer(&trashOperationID)), uintptr(unsafe.Pointer(&operation)))
	if err := trashHRESULT(result); err != nil {
		return "", err
	}
	defer syscall.SyscallN(operation.vtable.release, uintptr(unsafe.Pointer(operation)))
	if err := trashCOMCall(operation.vtable.setOperationFlags, unsafe.Pointer(operation), trashNoUI|trashNoConnectedElements|trashRecycleOnDelete|trashEarlyFailure); err != nil {
		return "", err
	}
	path, err := windows.UTF16PtrFromString(target)
	if err != nil {
		return "", err
	}
	var item *trashShellItem
	result, _, _ = trashCreateItem.Call(uintptr(unsafe.Pointer(path)), 0, uintptr(unsafe.Pointer(&trashShellItemID)), uintptr(unsafe.Pointer(&item)))
	runtime.KeepAlive(path)
	if err := trashHRESULT(result); err != nil {
		return "", err
	}
	defer syscall.SyscallN(item.vtable[2], uintptr(unsafe.Pointer(item)))
	if err := trashCOMCall(operation.vtable.deleteItem, unsafe.Pointer(operation), uintptr(unsafe.Pointer(item)), 0); err != nil {
		return "", err
	}
	if err := trashCOMCall(operation.vtable.performOperations, unsafe.Pointer(operation)); err != nil {
		return "", err
	}
	var aborted int32
	result, _, _ = syscall.SyscallN(operation.vtable.getAnyOperationsAborted, uintptr(unsafe.Pointer(operation)), uintptr(unsafe.Pointer(&aborted)))
	if err := trashHRESULT(result); err != nil {
		return "", err
	}
	if aborted != 0 {
		return "", fmt.Errorf("native Trash operation was aborted")
	}
	return "", nil
}

func trashCOMCall(method uintptr, receiver unsafe.Pointer, arguments ...uintptr) error {
	values := append([]uintptr{uintptr(receiver)}, arguments...)
	result, _, _ := syscall.SyscallN(method, values...)
	runtime.KeepAlive(receiver)
	return trashHRESULT(result)
}

func trashHRESULT(result uintptr) error {
	if int32(result) < 0 {
		return fmt.Errorf("native Trash operation failed: HRESULT 0x%08x", uint32(result))
	}
	return nil
}
