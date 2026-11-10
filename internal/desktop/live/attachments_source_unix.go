//go:build !windows

package live

import (
	"os"
	"syscall"
)

func openLocalAttachment(path string) (*os.File, error) {
	return os.OpenFile(path, os.O_RDONLY|syscall.O_NONBLOCK, 0)
}
