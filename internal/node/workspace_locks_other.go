//go:build !darwin && !windows

package node

import "path/filepath"

func workspacePathKey(existing, missing string) string {
	return filepath.Join(existing, missing)
}
