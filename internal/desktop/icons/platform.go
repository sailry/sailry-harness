package icons

import (
	"strings"

	"github.com/egoist/mygo/ui"
)

// Platform uses the same Simple Icons marks for hosts and controller devices,
// by the platform's family (Family).
func Platform(platform string) *ui.SVG {
	switch Family(platform) {
	case "macos", "ios":
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

// linuxDistributions are the distribution IDs a Node reports as its system
// (gopsutil's platform, from os-release), each a Linux.
var linuxDistributions = map[string]bool{
	"debian": true, "ubuntu": true, "linuxmint": true, "pop": true, "elementary": true, "neon": true, "raspbian": true,
	"fedora": true, "rhel": true, "redhat": true, "centos": true, "rocky": true, "almalinux": true, "oracle": true, "amazon": true,
	"opensuse": true, "opensuse-leap": true, "opensuse-tumbleweed": true, "suse": true, "sles": true,
	"arch": true, "manjaro": true, "endeavouros": true, "alpine": true, "gentoo": true, "nixos": true, "void": true, "slackware": true,
}

// Family is a platform's family, "" when unknown: macos, ios, windows,
// linux or android. It reads an OS (darwin, linux), a system's name
// ("Microsoft Windows 11 Pro") or a Linux distribution's ID (debian), so
// every view shows a host by the same mark whichever of them it was told.
func Family(platform string) string {
	value := strings.ToLower(strings.TrimSpace(platform))
	switch {
	case value == "darwin" || value == "macos" || value == "mac os" || value == "mac os x" || value == "osx":
		return "macos"
	case value == "ios" || value == "ipados":
		return "ios"
	case strings.Contains(value, "windows"):
		return "windows"
	case value == "android":
		return "android"
	case value == "linux" || strings.Contains(value, "linux") || linuxDistributions[value] || linuxDistributions[strings.Fields(value + " _")[0]]:
		return "linux"
	}
	return ""
}
