//go:build unix

package node

import (
	"os"
	"syscall"

	"github.com/sailry/sailry-harness/internal/core"
)

func validateLegacyProfileFile(path string, info os.FileInfo) error {
	stat := info.Sys().(*syscall.Stat_t)
	if stat.Nlink != 1 || stat.Uid != uint32(os.Geteuid()) {
		return &core.InvalidProfileError{Path: path, Reason: "legacy migration requires current-user files without hard links"}
	}
	return nil
}

func syncLegacyProfileDirectory(path string) error {
	file, err := os.Open(path)
	if err != nil {
		return err
	}
	defer file.Close()
	return file.Sync()
}
