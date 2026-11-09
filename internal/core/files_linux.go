package core

import (
	"io/fs"
	"os"
	"syscall"
	"time"

	"golang.org/x/sys/unix"
)

func fileNativeVersion(file *os.File, metadata fs.FileInfo) (any, any, error) {
	stat := metadata.Sys().(*syscall.Stat_t)
	var extended unix.Statx_t
	var created any
	if err := unix.Statx(int(file.Fd()), "", unix.AT_EMPTY_PATH, unix.STATX_BTIME, &extended); err == nil && extended.Mask&unix.STATX_BTIME != 0 {
		created = fileVersionTime(time.Unix(extended.Btime.Sec, int64(extended.Btime.Nsec)))
	}
	return created, []any{stat.Mode, stat.Ctim.Sec, stat.Ctim.Nsec}, nil
}
