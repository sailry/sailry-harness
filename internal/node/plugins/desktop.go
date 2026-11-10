package plugins

import (
	"context"
	"encoding/base64"
	"strings"
	"unicode/utf8"

	"github.com/sailry/sailry-harness/internal/protocol"
)

func (manager *Manager) ReadDesktop(ctx context.Context, info protocol.PluginInfo, surface protocol.PluginDesktopSurface) (protocol.PluginDesktopBundle, error) {
	bundle := protocol.DefaultPluginDesktopBundle()
	bundle.Package = PackageReference(info)
	if info.Extension == nil || info.Extension.Desktop == nil {
		return bundle, &protocol.Fault{Code: protocol.ErrorCodeNotConfigured, Message: "plugin has no desktop view"}
	}
	manifest := info.Extension.Desktop
	if err := protocol.ValidatePluginDesktopManifest(*manifest); err != nil {
		return bundle, err
	}
	var entry *string
	switch surface.Kind {
	case "workspace":
		entry = manifest.Entry
		if entry == nil {
			return bundle, packageFault("plugin view requires a native renderer")
		}
	case "settings":
		if info.Extension.SettingsPage != nil {
			entry = info.Extension.SettingsPage.Entry
		}
		if entry == nil {
			return bundle, packageFault("plugin has no settings entry")
		}
	case "composer", "project", "provider":
		entry = manifest.UiEntry
		if entry == nil {
			return bundle, packageFault("plugin has no contribution entry")
		}
	case "card":
		if len(info.Extension.Cards) == 0 {
			return bundle, packageFault("plugin has no card entry")
		}
		entry = &info.Extension.Cards[0].Entry
	case "permissions":
		return bundle, packageFault("permission contributions have no desktop view")
	default:
		return bundle, packageFault("invalid plugin desktop surface")
	}
	bundle.Entry = *entry
	entries, err := manager.captured(ctx, bundle.Package)
	if err != nil {
		return bundle, err
	}
	files := make(map[string][]byte, len(entries))
	for _, resource := range entries {
		if !resource.directory {
			files[resource.name] = resource.data
		}
	}
	remaining, imageRemaining := protocol.PluginDesktopMaxBytes, protocol.PluginDesktopMaxImageBytes
	for _, name := range manifest.Resources {
		if err := ctx.Err(); err != nil {
			return bundle, err
		}
		content, exists := files[name]
		if !exists {
			return bundle, &protocol.Fault{Code: protocol.ErrorCodeUnavailable, Message: "plugin desktop resource is unavailable"}
		}
		if strings.HasSuffix(name, ".png") {
			if len(content) > imageRemaining/4*3 {
				return bundle, packageFault("plugin image resources exceed limit")
			}
			encoded := base64.StdEncoding.EncodeToString(content)
			imageRemaining -= len(encoded)
			bundle.Images[name] = encoded
		} else {
			if len(content) > remaining {
				return bundle, packageFault("plugin desktop resources exceed limit")
			}
			if !utf8.Valid(content) {
				return bundle, packageFault("plugin desktop resource is not UTF-8 text")
			}
			remaining -= len(content)
			bundle.Files[name] = string(content)
		}
	}
	return bundle, ctx.Err()
}
