package preferences

import (
	"errors"

	"golang.org/x/sys/windows"
)

func writeFailureReason(err error) string {
	switch {
	case errors.Is(err, windows.ERROR_DISK_FULL), errors.Is(err, windows.ERROR_HANDLE_DISK_FULL):
		return "no_space"
	case errors.Is(err, windows.ERROR_SHARING_VIOLATION), errors.Is(err, windows.ERROR_LOCK_VIOLATION):
		return "file_locked"
	default:
		return "other"
	}
}
