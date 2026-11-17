package icons

import (
	"strings"

	"github.com/egoist/mygo/ui"
)

// Platform uses the same Simple Icons marks for hosts and controller devices.
func Platform(platform string) *ui.SVG {
	switch strings.ToLower(strings.TrimSpace(platform)) {
	case "darwin", "macos", "mac os", "mac os x", "ios":
		return Apple
	case "windows":
		return Windows
	case "linux":
		return Linux
	case "android":
		return Android
	}
	return Device
}
