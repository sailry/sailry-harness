//go:build darwin || windows

package files

import (
	"os"
	"path/filepath"
	"strings"
)

// Each existing directory supplies the lookup rules for its children. A
// missing suffix inherits the rules of its deepest existing ancestor.
func LookupPathKey(existing, missing string) string {
	key := existingPathKey(existing)
	if missing != "" {
		key = filepath.Join(key, workspaceNameKey(existing, missing))
	}
	return key
}

func existingPathKey(path string) string {
	parent := filepath.Dir(path)
	if parent == path {
		return volumeKey(path)
	}
	base := filepath.Base(path)
	key := existingPathKey(parent)
	name := workspaceNameKey(parent, base)
	if key == parent && name == base {
		return path
	}
	return filepath.Join(key, name)
}

// Keep the original directory spelling: a symlink is a separate entry, even
// when its target was touched by another hunk. Missing directories inherit the
// lookup rules of the nearest existing parent.
func patchPathKey(parent, path string) string {
	name, rest, nested := strings.Cut(path, "/")
	if !nested {
		return workspaceNameKey(parent, name)
	}
	child := filepath.Join(parent, name)
	if _, err := os.Stat(child); err != nil {
		return workspaceNameKey(parent, path)
	}
	return workspaceNameKey(parent, name) + "/" + patchPathKey(child, rest)
}
