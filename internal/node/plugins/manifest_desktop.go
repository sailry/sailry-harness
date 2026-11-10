package plugins

import (
	"slices"
	"strings"

	"github.com/sailry/sailry-harness/internal/protocol"
)

func validateDesktopExtension(extension protocol.PluginExtension) error {
	desktop := extension.Desktop
	if desktop != nil {
		if err := protocol.ValidatePluginDesktopManifest(*desktop); err != nil {
			return err
		}
		if desktop.NavigationOptions.Surface.Kind == "settings" && (extension.SettingsPage == nil || extension.SettingsPage.Entry == nil) ||
			len(desktop.Previews) != 0 && (extension.Scope != protocol.PluginScopeHost || !slices.Contains(extension.Actions, protocol.PluginActionReadFiles)) ||
			desktop.NavigationOptions.Target == protocol.PluginDesktopNavigationTargetWorktree && extension.Scope != protocol.PluginScopeHost {
			return packageFault("plugin desktop surface is outside its declared scope")
		}
		alternate := len(extension.Cards) != 0 || desktop.UiEntry != nil || extension.SettingsPage != nil && extension.SettingsPage.Entry != nil
		if desktop.Entry == nil && (!alternate || desktop.Navigation != nil || desktop.Panel != nil || len(desktop.Conversations) != 0 || len(desktop.Renderers) != 0) {
			return packageFault("plugin desktop entry is required")
		}
		for _, renderer := range desktop.Renderers {
			var actions []protocol.PluginAction
			allowed := renderer.Create == nil
			switch renderer.Resource {
			case protocol.PluginDesktopResourceKindTerminal:
				allowed = renderer.Create != nil && slices.ContainsFunc(extension.Ui, func(entry protocol.PluginUiContribution) bool {
					return entry.ID == *renderer.Create && entry.Slot == protocol.PluginUiSlotProject && entry.Kind == protocol.PluginUiKindButton
				})
				actions = []protocol.PluginAction{protocol.PluginActionReadTerminals, protocol.PluginActionControlTerminals}
			case protocol.PluginDesktopResourceKindBrowser:
				actions = []protocol.PluginAction{protocol.PluginActionReadBrowser, protocol.PluginActionControlBrowser}
			case protocol.PluginDesktopResourceKindGit:
				allowed = allowed && extension.Scope == protocol.PluginScopeHost
				actions = []protocol.PluginAction{protocol.PluginActionReadGit}
			case protocol.PluginDesktopResourceKindDocuments:
				allowed = allowed && extension.Scope == protocol.PluginScopeHost
				actions = []protocol.PluginAction{protocol.PluginActionReadFiles}
			default:
				allowed = false
			}
			if !allowed || slices.ContainsFunc(actions, func(action protocol.PluginAction) bool { return !slices.Contains(extension.Actions, action) }) {
				return packageFault("plugin desktop renderer is outside its declared actions")
			}
		}
	}
	if settings := extension.SettingsPage; settings != nil {
		if !settings.Navigation.Valid() || settings.Entry != nil && (!namespacePath(*settings.Entry) ||
			!strings.HasSuffix(*settings.Entry, ".js") && !strings.HasSuffix(*settings.Entry, ".mjs") ||
			desktop == nil || !slices.Contains(desktop.Resources, *settings.Entry)) {
			return packageFault("invalid plugin settings page")
		}
	}
	return nil
}
