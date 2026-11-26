//go:build darwin || windows

package node

import "path/filepath"

// Each existing directory supplies the lookup rules for its children. A
// missing suffix inherits the rules of its deepest existing ancestor.
func workspacePathKey(existing, missing string) string {
	key := existingPathKey(existing)
	if missing != "" {
		key = filepath.Join(key, workspaceNameKey(existing, missing))
	}
	return key
}

func existingPathKey(path string) string {
	parent := filepath.Dir(path)
	if parent == path {
		return path
	}
	base := filepath.Base(path)
	key := existingPathKey(parent)
	name := workspaceNameKey(parent, base)
	if key == parent && name == base {
		return path
	}
	return filepath.Join(key, name)
}
