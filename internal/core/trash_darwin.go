package core

import (
	"errors"
	"runtime"
	"sync"
	"unsafe"

	"github.com/ebitengine/purego"
	"github.com/ebitengine/purego/objc"
)

var loadTrashFoundation = sync.OnceValues(func() (uintptr, error) {
	return purego.Dlopen("/System/Library/Frameworks/Foundation.framework/Foundation", purego.RTLD_NOW|purego.RTLD_GLOBAL)
})

var (
	trashNew            = objc.RegisterName("new")
	trashDrain          = objc.RegisterName("drain")
	trashUTF8String     = objc.RegisterName("stringWithUTF8String:")
	trashFileURL        = objc.RegisterName("fileURLWithPath:")
	trashDefaultManager = objc.RegisterName("defaultManager")
	trashItemAtURL      = objc.RegisterName("trashItemAtURL:resultingItemURL:error:")
	trashPath           = objc.RegisterName("path")
	trashUTF8Bytes      = objc.RegisterName("UTF8String")
)

func trashNative(target string) (string, error) {
	if _, err := loadTrashFoundation(); err != nil {
		return "", err
	}
	runtime.LockOSThread()
	defer runtime.UnlockOSThread()
	pool := objc.ID(objc.GetClass("NSAutoreleasePool")).Send(trashNew)
	defer pool.Send(trashDrain)
	path := objc.ID(objc.GetClass("NSString")).Send(trashUTF8String, target)
	url := objc.ID(objc.GetClass("NSURL")).Send(trashFileURL, path)
	manager := objc.ID(objc.GetClass("NSFileManager")).Send(trashDefaultManager)
	var destination, nativeError objc.ID
	if !objc.Send[bool](manager, trashItemAtURL, url, unsafe.Pointer(&destination), unsafe.Pointer(&nativeError)) {
		return "", errors.New("native Trash operation failed")
	}
	if destination == 0 {
		return "", nil
	}
	return objc.Send[string](destination.Send(trashPath), trashUTF8Bytes), nil
}
