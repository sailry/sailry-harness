//go:build !windows

package preferences

import (
	"errors"
	"syscall"
)

func writeFailureReason(err error) string {
	if errors.Is(err, syscall.ENOSPC) {
		return "no_space"
	}
	return "other"
}
