package assets

import (
	"io/fs"
	"syscall"
)

func nativeOf(info fs.FileInfo) native {
	status := info.Sys().(*syscall.Stat_t)
	value := native{device: uint64(status.Dev), inode: status.Ino}
	if !info.IsDir() {
		value.seconds, value.nanoseconds = status.Ctimespec.Sec, status.Ctimespec.Nsec
	}
	return value
}
