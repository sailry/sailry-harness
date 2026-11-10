package knowledge

import (
	"os"
	"path/filepath"
	"runtime"
	"strings"
)

// Native maps an absolute source path to the directory the system resolves
// it to: macOS reaches /var and /tmp through links into /private.
func Native(path string) (string, error) {
	if !filepath.IsAbs(path) {
		return "", invalid("knowledge source path must be absolute")
	}
	if runtime.GOOS == "darwin" || runtime.GOOS == "ios" {
		for alias, destination := range map[string]string{"/var": "/private/var", "/tmp": "/private/tmp"} {
			relative, inside := strings.CutPrefix(path, alias)
			if !inside || relative != "" && relative[0] != '/' {
				continue
			}
			if metadata, err := os.Lstat(alias); err != nil || metadata.Mode()&os.ModeSymlink == 0 {
				continue
			}
			resolved, err := filepath.EvalSymlinks(alias)
			if err != nil {
				return "", err
			}
			if resolved != destination {
				return "", invalid("unexpected system source alias")
			}
			return filepath.Join(destination, relative), nil
		}
	}
	return path, nil
}

// Excluded reports a file or directory name that is never indexed:
// credentials, keys and tool configuration.
func Excluded(name string) bool {
	name = strings.ToLower(name)
	switch name {
	case ".git", ".ssh", ".gnupg", ".aws", ".azure", ".kube", ".docker", ".config", ".sailry", ".codex", ".claude", ".gemini",
		".npmrc", ".pypirc", ".netrc", ".gitconfig", ".git-credentials", ".ds_store", "credentials", "credentials.json", "credentials.toml", ".env":
		return true
	}
	if strings.HasPrefix(name, ".env.") || strings.HasPrefix(name, "id_rsa") || strings.HasPrefix(name, "id_ed25519") {
		return true
	}
	for _, suffix := range []string{".pem", ".key", ".p12", ".pfx"} {
		if strings.HasSuffix(name, suffix) {
			return true
		}
	}
	return false
}

// Protected reports a path with an excluded component.
func Protected(path string) bool {
	for _, component := range strings.Split(filepath.ToSlash(path), "/") {
		if Excluded(component) {
			return true
		}
	}
	return false
}
