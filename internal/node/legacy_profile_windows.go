package node

import (
	"os"
	"strings"

	"github.com/sailry/sailry-harness/internal/core"
	"golang.org/x/sys/windows"
)

func legacyProfileWindowsName(path string) (*uint16, error) {
	if !strings.HasPrefix(path, `\\?\`) {
		if strings.HasPrefix(path, `\\`) {
			path = `\\?\UNC\` + strings.TrimPrefix(path, `\\`)
		} else {
			path = `\\?\` + path
		}
	}
	return windows.UTF16PtrFromString(path)
}

func renameLegacyProfile(source, target string) error {
	from, err := legacyProfileWindowsName(source)
	if err != nil {
		return err
	}
	to, err := legacyProfileWindowsName(target)
	if err != nil {
		return err
	}
	// No REPLACE_EXISTING: even an empty directory or dangling link wins.
	return windows.MoveFileEx(from, to, windows.MOVEFILE_WRITE_THROUGH)
}

func validateLegacyProfileFile(path string, _ os.FileInfo) error {
	return core.ValidateDatabase(path)
}

func syncLegacyProfileDirectory(string) error { return nil }
