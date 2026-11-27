package node

import (
	"os"
	"strings"

	"github.com/sailry/sailry-harness/internal/core"
	"golang.org/x/sys/windows"
)

func (migration *legacyProfileMigration) acquireSource(path string) (func() error, error) {
	return migration.journal.AcquireWindowsMigrationLock(path, false)
}

func (migration *legacyProfileMigration) acquirePublished(completed bool) (func() error, error) {
	return migration.journal.AcquireWindowsMigrationLock(migration.target, completed)
}

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
	// No REPLACE_EXISTING: even an empty directory or dangling link wins.
	return moveLegacyProfile(source, target, windows.MOVEFILE_WRITE_THROUGH)
}

func replaceLegacyProfileState(source, target string) error {
	return moveLegacyProfile(source, target, windows.MOVEFILE_REPLACE_EXISTING|windows.MOVEFILE_WRITE_THROUGH)
}

func moveLegacyProfile(source, target string, flags uint32) error {
	from, err := legacyProfileWindowsName(source)
	if err != nil {
		return err
	}
	to, err := legacyProfileWindowsName(target)
	if err != nil {
		return err
	}
	return windows.MoveFileEx(from, to, flags)
}

func validateLegacyProfileFile(path string, _ os.FileInfo) error {
	return core.ValidateDatabase(path)
}

func syncLegacyProfileDirectory(string) error { return nil }
