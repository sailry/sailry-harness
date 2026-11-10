//go:build !windows

package core

import (
	"errors"
	"syscall"
)

func fileCrossDevice(err error) bool { return errors.Is(err, syscall.EXDEV) }
