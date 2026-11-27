//go:build unix

package node

import (
	"os"
	"syscall"

	"github.com/sailry/sailry-harness/internal/core"
)

func (*legacyProfileMigration) acquireSource(path string) (func() error, error) {
	profile, err := core.AcquireProfile(path)
	if err != nil {
		return nil, err
	}
	return profile.Close, nil
}

func (*legacyProfileMigration) acquirePublished(bool) (func() error, error) { return nil, nil }

func replaceLegacyProfileState(source, target string) error { return os.Rename(source, target) }

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
