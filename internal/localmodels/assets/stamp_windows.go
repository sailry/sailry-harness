package assets

import (
	"io/fs"
	"syscall"
)

func nativeOf(info fs.FileInfo) native {
	if info.IsDir() {
		return native{}
	}
	return native{created: info.Sys().(*syscall.Win32FileAttributeData).CreationTime.Nanoseconds()}
}
