package application

import (
	"unsafe"

	"golang.org/x/sys/windows"
)

var sendMessageTimeout = windows.NewLazySystemDLL("user32.dll").NewProc("SendMessageTimeoutW")

// MyGo locks the main goroutine to the UI thread before startup. During a
// native modal loop its active window can be the dialog itself. A synchronous
// WM_NULL tests whether that window handles a sent message independently of
// MyGo's posted dispatch. Only the outcome leaves this probe.
func nativeWindowProbe() func() (bool, bool) {
	thread := windows.GetCurrentThreadId()
	return func() (bool, bool) {
		var info windows.GUIThreadInfo
		info.Size = uint32(unsafe.Sizeof(info))
		if windows.GetGUIThreadInfo(thread, &info) != nil || info.Active == 0 {
			return false, false
		}
		var pid uint32
		if tid, err := windows.GetWindowThreadProcessId(info.Active, &pid); err != nil || tid != thread || pid != windows.GetCurrentProcessId() {
			return false, false
		}
		const wmNull, smtoBlock, smtoErrorOnExit = 0x0000, 0x0001, 0x0020
		ok, _, err := sendMessageTimeout.Call(uintptr(info.Active), wmNull, 0, 0, smtoBlock|smtoErrorOnExit, 250, 0)
		return ok != 0, ok != 0 || err == windows.ERROR_TIMEOUT
	}
}
