package plugins

import (
	"slices"

	"github.com/sailry/sailry-harness/internal/protocol"
)

// IsLegacyDesktopPackage matches the official content used by legacy Desktop
// views. The extra digests do not add loader requirements or native tool aliases.
func IsLegacyDesktopPackage(name, digest string) bool {
	return IsLegacyOfficialPackage(name, digest) || name == "commands" && IsLegacyNativeToolPackage(name, digest) ||
		slices.Contains(legacyDesktopDigests[name], digest)
}

// LegacyDesktopViewRetired applies to views, not inventory, settings or history.
func LegacyDesktopViewRetired(ref protocol.PluginReference) bool {
	return (ref.Name == "files" || ref.Name == "git") && IsLegacyDesktopPackage(ref.Name, ref.Digest)
}

// LegacyDesktopControlRetired covers the project selector built into every
// composer, including windows without the optional worktree workflow.
func LegacyDesktopControlRetired(ref protocol.PluginReference, slot protocol.PluginUiSlot, id string) bool {
	return ref.Name == "worktrees" && slot == protocol.PluginUiSlotContext && id == "location" && IsLegacyDesktopPackage(ref.Name, ref.Digest)
}

// Verified package trees from f842a50 and bd42d9c in sailry/sailry-plugins.
var legacyDesktopDigests = map[string][]string{
	"files": {
		"4a98357aa49f95702ca6d3641172eec9f0f5a86e7d400d3306e006e78ae2a509",
		"ddb4a529e4e64e2447295e20770b7e8cd0c55058bfcbd5a3b3fe7ca6d590c0fb",
	},
	"git": {
		"4d894eb785dca39ae5610df7c0270133ecece5d2fb93705e36603a3182c99aeb",
		"e07ce1255ec5e5a67ffb339aa02ff773e85d49cfe3eb2498e55a6dbdf8b01b70",
	},
	"ssh": {
		"e1d404ff4466035352b939bf54c5296ad6e4d5e50e71d969005a0e24a255b837",
		"9990c72c27c647a3000cb1385b26550af1e9c5eb2a7cd2823aaed3c21e484a8f",
	},
	"worktrees": {
		"c33eadf2329c796fb2adc35e64f5ecfb548f5eb78a68498f361acbb66bba1ff5",
		"ae42d37635d7990e29b1654a311ac5ace70e47b9a7f1226721a63482406fbb9e",
	},
}
