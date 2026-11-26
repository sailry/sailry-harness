//go:build !darwin && !windows

package files

import "path/filepath"

func LookupPathKey(existing, missing string) string {
	return filepath.Join(existing, missing)
}

func patchPathKey(root, path string) string { return path }
