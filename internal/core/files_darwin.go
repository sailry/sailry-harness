package core

import (
	"io/fs"
	"os"
	"sync"
	"syscall"
	"time"

	"github.com/ebitengine/purego"
	"golang.org/x/sys/unix"
)

func fileNativeVersion(_ *os.File, metadata fs.FileInfo) (any, any, error) {
	stat := metadata.Sys().(*syscall.Stat_t)
	created := fileVersionTime(time.Unix(stat.Birthtimespec.Sec, stat.Birthtimespec.Nsec))
	return created, []any{uint32(stat.Mode), stat.Ctimespec.Sec, stat.Ctimespec.Nsec}, nil
}

// getattrlist(2): a nonzero write generation proves identical data for the
// same object, unlike timestamps alone. Unavailable attributes and zero (for
// example, mmap on a supporting volume) cannot be used for this comparison.
var getFileGeneration = sync.OnceValue(func() func(int32, *unix.Attrlist, *[7]uint32, uintptr, uint32) int32 {
	address, err := purego.Dlsym(purego.RTLD_DEFAULT, "fgetattrlist")
	if err != nil {
		return nil
	}
	var call func(int32, *unix.Attrlist, *[7]uint32, uintptr, uint32) int32
	purego.RegisterFunc(&call, address)
	return call
})

func fileStamp(file *os.File, metadata fs.FileInfo) FileStamp {
	call := getFileGeneration()
	if call == nil {
		return FileStamp{}
	}
	attrs := unix.Attrlist{Bitmapcount: unix.ATTR_BIT_MAP_COUNT, Commonattr: unix.ATTR_CMN_RETURNED_ATTRS | unix.ATTR_CMN_GEN_COUNT}
	// Packed length, returned attribute set (five bitmaps), then generation.
	var result [7]uint32
	const size = 7 * 4
	if call(int32(file.Fd()), &attrs, &result, size, unix.FSOPT_ATTR_CMN_EXTENDED) != 0 || result[0] != size || result[1] != attrs.Commonattr || result[6] == 0 {
		return FileStamp{}
	}
	stat := metadata.Sys().(*syscall.Stat_t)
	return FileStamp{device: uint64(stat.Dev), inode: stat.Ino, created: stat.Birthtimespec.Nano(), changed: stat.Ctimespec.Nano(), mtime: stat.Mtimespec.Nano(), size: stat.Size, generation: result[6]}
}
