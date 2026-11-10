package decision

import (
	"fmt"
	"strings"

	"golang.org/x/sys/unix"
)

// baseline checks the running macOS against the locked minimum.
func baseline(minimum *[2]uint32) error {
	version, err := unix.Sysctl("kern.osproductversion")
	if err != nil || version == "" {
		return unavailable("macOS version could not be read")
	}
	if minimum == nil {
		return unavailable("macOS minimum is missing")
	}
	if !atLeast(strings.TrimRight(version, "\x00"), *minimum) {
		return unavailable(fmt.Sprintf("macOS %d.%d or newer is required", minimum[0], minimum[1]))
	}
	return nil
}
