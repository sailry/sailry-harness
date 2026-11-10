package store

import (
	"context"
	"database/sql"
	"slices"

	"github.com/sailry/sailry-harness/internal/node/plugins"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func ResolvePluginViewInfo(ctx context.Context, db *sql.Conn, reference protocol.PluginReference, surface protocol.PluginDesktopSurface) (protocol.PluginInfo, error) {
	if surface.Kind == "card" {
		receipt, ok := pluginViewCardReceipt(surface.Data)
		if !ok {
			return protocol.PluginInfo{}, invalid("invalid plugin card receipt")
		}
		info, summary, err := CapturedPluginCard(ctx, db, receipt)
		if err != nil {
			return protocol.PluginInfo{}, err
		}
		if summary.Package != reference {
			return protocol.PluginInfo{}, fault(protocol.ErrorCodePermissionDenied, "card package does not match its receipt")
		}
		info.Summary.SettingsRevision = summary.Package.SettingsRevision
		info.Extension.Cards = slices.DeleteFunc(slices.Clone(info.Extension.Cards), func(declaration protocol.PluginCardDeclaration) bool {
			return declaration.Key != summary.Key
		})
		return info, nil
	}
	info, err := ReadPlugin(ctx, db, reference.Name)
	if err != nil {
		return protocol.PluginInfo{}, err
	}
	if info == nil {
		return protocol.PluginInfo{}, fault(protocol.ErrorCodeNotFound, "plugin is not installed")
	}
	if err := plugins.EnsureCompatible(info.Summary.Compatibility); err != nil {
		return protocol.PluginInfo{}, err
	}
	if !info.Summary.Enabled && surface.Kind != "settings" {
		return protocol.PluginInfo{}, fault(protocol.ErrorCodeNotConfigured, "plugin is disabled")
	}
	if plugins.PackageReference(*info) != reference {
		return protocol.PluginInfo{}, fault(protocol.ErrorCodeRevisionConflict, "plugin version changed")
	}
	if info.Extension == nil || info.Extension.Desktop == nil {
		return protocol.PluginInfo{}, fault(protocol.ErrorCodeNotConfigured, "plugin has no desktop view")
	}
	if surface.Kind == "settings" && (info.Extension.SettingsPage == nil || info.Extension.SettingsPage.Entry == nil) {
		return protocol.PluginInfo{}, fault(protocol.ErrorCodeNotConfigured, "plugin has no settings view")
	}
	if surface.Kind == "composer" || surface.Kind == "provider" || surface.Kind == "project" {
		matches := slices.ContainsFunc(info.Extension.Ui, func(entry protocol.PluginUiContribution) bool {
			project := entry.Slot == protocol.PluginUiSlotProject || entry.Slot == protocol.PluginUiSlotProjectMenu
			switch surface.Kind {
			case "provider":
				return entry.Slot == protocol.PluginUiSlotProvider
			case "project":
				return project
			default:
				return !project && entry.Slot != protocol.PluginUiSlotProvider
			}
		})
		if !matches || info.Extension.Desktop.UiEntry == nil {
			return protocol.PluginInfo{}, fault(protocol.ErrorCodeNotConfigured, "plugin has no contributions for this surface")
		}
	}
	return *info, nil
}

func pluginViewCardReceipt(input any) (protocol.PluginCardReceipt, bool) {
	switch value := input.(type) {
	case protocol.PluginCardReceipt:
		return value, true
	case *protocol.PluginCardReceipt:
		if value != nil {
			return *value, true
		}
	}
	return protocol.PluginCardReceipt{}, false
}
