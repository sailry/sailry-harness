//go:build windows

package git

import (
	"fmt"
	"os"
	"syscall"
)

func fileIdentity(info os.FileInfo) string {
	if stat, ok := info.Sys().(*syscall.Win32FileAttributeData); ok {
		return fmt.Sprintf("%d:%d", stat.CreationTime.HighDateTime, stat.CreationTime.LowDateTime)
	}
	return ""
}
