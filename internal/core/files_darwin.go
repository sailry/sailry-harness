package core

import (
	"io/fs"
	"os"
	"syscall"
	"time"
)

func fileNativeVersion(_ *os.File, metadata fs.FileInfo) (any, any, error) {
	stat := metadata.Sys().(*syscall.Stat_t)
	created := fileVersionTime(time.Unix(stat.Birthtimespec.Sec, stat.Birthtimespec.Nsec))
	return created, []any{uint32(stat.Mode), stat.Ctimespec.Sec, stat.Ctimespec.Nsec}, nil
}
