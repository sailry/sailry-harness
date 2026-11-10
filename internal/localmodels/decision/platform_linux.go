package decision

import (
	"fmt"
	"unsafe"

	"github.com/ebitengine/purego"
)

// baseline checks the C library of this process against the locked minimum.
// The symbol is looked up among the images already loaded; a process without
// glibc has no such symbol.
func baseline(minimum *[2]uint32) error {
	symbol, err := purego.Dlsym(purego.RTLD_DEFAULT, "gnu_get_libc_version")
	if err != nil {
		return unavailable("a glibc runtime is required")
	}
	var read func() *byte
	purego.RegisterFunc(&read, symbol)
	if minimum == nil {
		return unavailable("glibc minimum is missing")
	}
	// glibc returns a static nul-terminated version string.
	text := read()
	length := 0
	for text != nil && length < 64 && *(*byte)(unsafe.Add(unsafe.Pointer(text), length)) != 0 {
		length++
	}
	if text == nil || !atLeast(unsafe.String(text, length), *minimum) {
		return unavailable(fmt.Sprintf("glibc %d.%d or newer is required", minimum[0], minimum[1]))
	}
	return nil
}
