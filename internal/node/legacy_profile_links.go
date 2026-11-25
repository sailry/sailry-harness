package node

import (
	"fmt"
	"os"
	"path/filepath"
)

func checkLegacyProfileRootLinks(profile string, aliases ...string) error {
	entries, err := os.ReadDir(profile)
	if err != nil {
		return err
	}
	for _, entry := range entries {
		if entry.Type()&os.ModeSymlink == 0 {
			continue
		}
		path := filepath.Join(profile, entry.Name())
		target, err := os.Readlink(path)
		if err != nil {
			return err
		}
		if !filepath.IsAbs(target) {
			// Local relative links move with their containing directory.
			if filepath.IsLocal(target) {
				continue
			}
			target = filepath.Join(profile, target)
		}
		for _, original := range append([]string{profile}, aliases...) {
			relative, err := filepath.Rel(original, target)
			if err == nil && (relative == "." || filepath.IsLocal(relative)) {
				return fmt.Errorf("legacy profile link %q depends on the old absolute profile path; make it profile-relative before retrying", path)
			}
		}
	}
	return nil
}
