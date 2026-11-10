package computer

import (
	"strconv"
	"strings"
	"sync"
	"syscall"
)

// macOSMajor is the running system's major version, or 0 when unknown.
var macOSMajor = sync.OnceValue(func() int {
	version, err := syscall.Sysctl("kern.osproductversion")
	if err != nil {
		return 0
	}
	major, _, _ := strings.Cut(version, ".")
	value, _ := strconv.Atoi(major)
	return value
})
