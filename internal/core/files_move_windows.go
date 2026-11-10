package core

import (
	"errors"
	"golang.org/x/sys/windows"
)

func fileCrossDevice(err error) bool { return errors.Is(err, windows.ERROR_NOT_SAME_DEVICE) }
