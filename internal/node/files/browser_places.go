package files

import (
	"os"
	"path/filepath"
	"strings"

	"github.com/sailry/sailry-harness/internal/protocol"
)

// place is a standard folder of the account the Node runs as, before it is
// known to exist.
type place struct {
	kind protocol.FileBrowserLocationKind
	path string
}

// userPlaces are the standard folders of the account whose home is home, by
// the rules of goos: on macOS the fixed folders of the home and the system's
// Applications; on Linux the XDG user directories; elsewhere the folders of
// those names under the home. The home itself is not among them.
func userPlaces(goos, home string, getenv func(string) string) []place {
	desktop, documents, downloads := filepath.Join(home, "Desktop"), filepath.Join(home, "Documents"), filepath.Join(home, "Downloads")
	switch goos {
	case "darwin":
		return []place{{protocol.FileBrowserLocationKindDesktop, desktop}, {protocol.FileBrowserLocationKindDocuments, documents},
			{protocol.FileBrowserLocationKindDownloads, downloads}, {protocol.FileBrowserLocationKindApplications, "/Applications"}}
	case "linux":
		config := getenv("XDG_CONFIG_HOME")
		if !filepath.IsAbs(config) {
			config = filepath.Join(home, ".config")
		}
		var listed map[string]string
		if data, err := os.ReadFile(filepath.Join(config, "user-dirs.dirs")); err == nil {
			listed = xdgUserDirs(string(data), home)
		}
		resolve := func(name, fallback string) string {
			if path := listed[name]; path != "" {
				return path
			}
			if path := getenv(name); filepath.IsAbs(path) {
				return filepath.Clean(path)
			}
			return fallback
		}
		desktop, documents, downloads = resolve("XDG_DESKTOP_DIR", desktop), resolve("XDG_DOCUMENTS_DIR", documents), resolve("XDG_DOWNLOAD_DIR", downloads)
	}
	return []place{{protocol.FileBrowserLocationKindDesktop, desktop}, {protocol.FileBrowserLocationKindDocuments, documents},
		{protocol.FileBrowserLocationKindDownloads, downloads}}
}

// xdgUserDirs reads the directories of a user-dirs.dirs file, by their
// variable: lines of the form XDG_NAME_DIR="$HOME/path" or
// XDG_NAME_DIR="/path", as xdg-user-dirs writes them. Any other line is
// skipped. A directory set to the home itself is one its user turned off.
func xdgUserDirs(data, home string) map[string]string {
	out := map[string]string{}
	for _, line := range strings.Split(data, "\n") {
		name, value, found := strings.Cut(strings.TrimSpace(line), "=")
		name, value = strings.TrimSpace(name), strings.TrimSpace(value)
		if !found || !strings.HasPrefix(name, "XDG_") || !strings.HasSuffix(name, "_DIR") ||
			len(value) < 2 || value[0] != '"' || value[len(value)-1] != '"' {
			continue
		}
		var path strings.Builder
		escaped := false
		for _, r := range value[1 : len(value)-1] {
			if r == '\\' && !escaped {
				escaped = true
				continue
			}
			escaped = false
			path.WriteRune(r)
		}
		text := path.String()
		if rest, relative := strings.CutPrefix(text, "$HOME"); relative && (rest == "" || rest[0] == '/') {
			text = home + rest
		}
		if filepath.IsAbs(text) {
			out[name] = filepath.Clean(text)
		}
	}
	return out
}
