package store

import (
	"context"
	"database/sql"
	"math"

	"github.com/sailry/sailry-harness/internal/protocol"
)

// ReadPluginVersion reads an immutable package capture independently of its live
// installation. It never creates inventory or substitutes a newer package.
func ReadPluginVersion(ctx context.Context, db *sql.Conn, reference protocol.PluginReference) (protocol.PluginInfo, error) {
	if _, cached, err := configurationCacheUser(ctx, db); err != nil {
		return protocol.PluginInfo{}, err
	} else if cached {
		info, _, err := followedPackage(ctx, db, reference)
		if err != nil {
			return info, err
		}
		if _, err := cachedPluginSettings(ctx, db, reference); err != nil {
			return info, err
		}
		info.Summary.SettingsRevision = reference.SettingsRevision
		refreshPluginCompatibility(&info)
		if info.Summary.Compatibility.Kind != "compatible" {
			return info, fault(protocol.ErrorCodeNotConfigured, "plugin version is incompatible")
		}
		return info, nil
	}
	info, err := readBody[protocol.PluginInfo](ctx, db, "SELECT body FROM plugin_packages WHERE digest=?1 AND name=?2", reference.Digest, reference.Name)
	if err != nil {
		return protocol.PluginInfo{}, err
	}
	if info == nil {
		return protocol.PluginInfo{}, fault(protocol.ErrorCodeNotConfigured, "plugin version is unavailable on this Node")
	}
	if info.Summary.Name != reference.Name || info.Summary.Digest != reference.Digest {
		return protocol.PluginInfo{}, invalid("stored plugin version identity differs from its capture")
	}
	if info.Summary.Compatibility.Kind != "compatible" {
		return protocol.PluginInfo{}, fault(protocol.ErrorCodeNotConfigured, "plugin version is incompatible")
	}
	if reference.SettingsRevision > math.MaxInt64 {
		return protocol.PluginInfo{}, invalid("invalid plugin configuration revision")
	}
	if reference.SettingsRevision != 0 {
		user, err := pluginUserID(ctx, db)
		if err != nil {
			return protocol.PluginInfo{}, err
		}
		var exists bool
		if err = db.QueryRowContext(ctx, "SELECT EXISTS(SELECT 1 FROM plugin_user_settings WHERE plugin=?1 AND revision=?2 AND user=?3)", reference.Name, reference.SettingsRevision, user).Scan(&exists); err != nil {
			return protocol.PluginInfo{}, err
		}
		if !exists {
			return protocol.PluginInfo{}, fault(protocol.ErrorCodeNotConfigured, "plugin configuration revision is unavailable")
		}
	}
	info.Summary.SettingsRevision = reference.SettingsRevision
	return *info, nil
}
