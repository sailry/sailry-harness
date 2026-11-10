//go:build darwin || linux

package process

import (
	"bytes"
	"errors"
	"os"
	"runtime"
	"sync"
	"syscall"
	"unsafe"

	"github.com/ebitengine/purego"
)

type account struct{ name, home, shell string }

// os/user does not expose the account's login shell. These layouts mirror the
// public passwd ABI, while getpwuid_r owns resolution through the system account service.
var lookupAccount = sync.OnceValues(func() (func(uint32, *nativePasswd, *byte, uintptr, **nativePasswd) int32, error) {
	library := "libc.so.6"
	if runtime.GOOS == "darwin" {
		library = "/usr/lib/libSystem.B.dylib"
	}
	handle, err := purego.Dlopen(library, purego.RTLD_NOW|purego.RTLD_LOCAL)
	if err != nil {
		return nil, err
	}
	address, err := purego.Dlsym(handle, "getpwuid_r")
	if err != nil {
		_ = purego.Dlclose(handle)
		return nil, err
	}
	var lookup func(uint32, *nativePasswd, *byte, uintptr, **nativePasswd) int32
	purego.RegisterFunc(&lookup, address)
	return lookup, nil
})

func currentAccount() (account, error) {
	lookup, err := lookupAccount()
	if err != nil {
		return account{}, err
	}
	for size := 16 * 1024; size <= 1024*1024; size *= 2 {
		buffer := make([]byte, size)
		var record nativePasswd
		var result *nativePasswd
		status := lookup(uint32(os.Getuid()), &record, &buffer[0], uintptr(len(buffer)), &result)
		if status == int32(syscall.ERANGE) {
			continue
		}
		if status != 0 {
			return account{}, syscall.Errno(status)
		}
		if result == nil {
			return account{}, errors.New("current user account was not found")
		}
		read := func(pointer *byte) (string, error) {
			start, base := uintptr(unsafe.Pointer(pointer)), uintptr(unsafe.Pointer(&buffer[0]))
			if pointer == nil || start < base || start >= base+uintptr(len(buffer)) {
				return "", errors.New("current user account has a missing field")
			}
			value := buffer[start-base:]
			end := bytes.IndexByte(value, 0)
			if end < 0 {
				return "", errors.New("current user account has an unterminated field")
			}
			return string(value[:end]), nil
		}
		name, err := read(record.name)
		if err != nil {
			return account{}, err
		}
		home, err := read(record.home)
		if err != nil {
			return account{}, err
		}
		shell, err := read(record.shell)
		if err != nil {
			return account{}, err
		}
		runtime.KeepAlive(buffer)
		return account{name, home, shell}, nil
	}
	return account{}, errors.New("current user account is too large")
}
