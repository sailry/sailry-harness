package live

import (
	"slices"

	"github.com/sailry/sailry-harness/internal/desktop/shell"
	nodeplugins "github.com/sailry/sailry-harness/internal/node/plugins"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func pluginViewsRetired(ref protocol.PluginReference) bool {
	return nodeplugins.LegacyDesktopViewRetired(ref)
}

func (r *Runtime) pluginNavigationReplaced(ref protocol.PluginReference) bool {
	return pluginViewsRetired(ref) || nodeplugins.IsLegacyDesktopPackage(ref.Name, ref.Digest) && r.App.ReplacementFeature(ref.Name) != ""
}

func (r *Runtime) legacyPluginControlReplaced(ref protocol.PluginReference, declaration protocol.PluginUiContribution) bool {
	control := shell.PluginControlReplacement{Package: ref.Name, Slot: declaration.Slot, ID: declaration.ID}
	return nodeplugins.LegacyDesktopControlRetired(ref, declaration.Slot, declaration.ID) ||
		slices.Contains(r.localRuntime().pluginControlReplacements, control) && nodeplugins.IsLegacyDesktopPackage(ref.Name, ref.Digest)
}

// The Terminal panel never uses a package renderer. Its explicitly linked
// launcher has no renderer to open, regardless of the package's identity.
func pluginTerminalLauncher(info protocol.PluginInfo, declaration protocol.PluginUiContribution) bool {
	if declaration.Slot != protocol.PluginUiSlotProject || declaration.Kind != protocol.PluginUiKindButton ||
		info.Extension == nil || info.Extension.Desktop == nil {
		return false
	}
	return slices.ContainsFunc(info.Extension.Desktop.Renderers, func(renderer protocol.PluginDesktopRenderer) bool {
		return renderer.Resource == protocol.PluginDesktopResourceKindTerminal && renderer.Create != nil && *renderer.Create == declaration.ID
	})
}
