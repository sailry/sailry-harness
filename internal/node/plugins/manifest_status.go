package plugins

import (
	"slices"

	"github.com/sailry/sailry-harness/internal/protocol"
)

// validateStatusExtension accepts every slot the contract names. Accepting a
// declaration does not promise Desktop draws it: the provider slot and the
// bundled project controls the application has natively are not mounted
// (docs/plugins.md, Deviations).
func validateStatusExtension(extension protocol.PluginExtension) error {
	if !protocol.ValidPluginUiContributions(extension.Ui) {
		return packageFault("invalid plugin UI declarations")
	}
	hasStatus, requiresView := false, false
	for _, entry := range extension.Ui {
		hasStatus = hasStatus || entry.Slot == protocol.PluginUiSlotStatus
		requiresView = requiresView || entry.Slot != protocol.PluginUiSlotStatus || entry.Kind != protocol.PluginUiKindButton
	}
	var status *protocol.PluginHostStatus
	if extension.Host != nil {
		status = extension.Host.Status
	}
	if hasStatus != (status != nil) || hasStatus && extension.Scope != protocol.PluginScopeHost {
		return packageFault("plugin Status declarations require a host owner")
	}
	if requiresView && (extension.Desktop == nil || extension.Desktop.UiEntry == nil) {
		return packageFault("plugin UI declarations require a desktop UI entry")
	}
	if status == nil {
		return nil
	}
	host := extension.Host
	if !slices.Contains(host.Handlers, status.Read) || !slices.Contains(host.ReadOnly, status.Read) ||
		status.Invoke != nil && (!slices.Contains(host.Handlers, *status.Invoke) || slices.Contains(host.ReadOnly, *status.Invoke)) {
		return packageFault("invalid plugin Status handlers")
	}
	if len(status.Changes) > 3 {
		return packageFault("plugin Status changes exceed their declaration limit")
	}
	for index, change := range status.Changes {
		if !slices.Contains([]protocol.PluginHostChange{protocol.PluginHostChangePackage, protocol.PluginHostChangeConversation, protocol.PluginHostChangeRepository}, change) ||
			slices.Contains(status.Changes[:index], change) {
			return packageFault("invalid plugin Status change")
		}
	}
	return nil
}
