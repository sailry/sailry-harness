package node

import (
	"errors"
	"fmt"
	"os"
	"path/filepath"

	"github.com/sailry/sailry-harness/internal/core"
)

func checkLegacyProfilePython(profile string) error {
	for _, name := range []string{"python", filepath.Join("python", "environments"), filepath.Join("python", "staging")} {
		path := filepath.Join(profile, name)
		info, err := os.Lstat(path)
		if errors.Is(err, os.ErrNotExist) {
			continue
		}
		if err != nil {
			return err
		}
		if !info.IsDir() {
			return fmt.Errorf("legacy Python storage must be a directory without links at %q", path)
		}
	}
	return nil
}

func quarantineLegacyProfilePython(profile, backup string) error {
	if err := checkLegacyProfilePython(profile); err != nil {
		return err
	}
	// relocate.py bakes the final environment path into console launchers and
	// seals their hashes. Preserve these rebuildable, package-owned environments
	// in the one original backup; Manager.Ensure rebuilds missing environments
	// from pinned requirements, without executing old launchers during migration.
	for _, name := range []string{"environments", "staging"} {
		source := filepath.Join(profile, "python", name)
		exists, err := legacyProfileExists(source)
		if err != nil || !exists {
			if err != nil {
				return err
			}
			continue
		}
		parent := filepath.Join(backup, "python")
		if err := core.PrivateDirectory(parent); err != nil {
			return err
		}
		if err := renameLegacyProfile(source, filepath.Join(parent, name)); err != nil {
			return err
		}
		if err := errors.Join(syncLegacyProfileDirectory(filepath.Dir(source)), syncLegacyProfileDirectory(parent), syncLegacyProfileDirectory(backup)); err != nil {
			return err
		}
	}
	return nil
}
