package store

import (
	"context"
	"database/sql"
	"encoding/json"
	"errors"
	"math"

	"github.com/sailry/sailry-harness/internal/node/plugins"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func ReadPlugins(ctx context.Context, db *sql.Conn) ([]protocol.PluginSummary, error) {
	infos, err := ReadPluginInfos(ctx, db)
	if err != nil {
		return nil, err
	}
	result := make([]protocol.PluginSummary, 0, len(infos))
	for _, info := range infos {
		result = append(result, info.Summary)
	}
	return result, nil
}

func ReadPluginInfos(ctx context.Context, db *sql.Conn) ([]protocol.PluginInfo, error) {
	infos, err := readBodies[protocol.PluginInfo](ctx, db, "SELECT body FROM plugins WHERE installed=1 ORDER BY name")
	for index := range infos {
		refreshPluginCompatibility(&infos[index])
	}
	return infos, err
}

func ReadPlugin(ctx context.Context, db *sql.Conn, name string) (*protocol.PluginInfo, error) {
	if !plugins.ValidateName(name) {
		return nil, invalid("invalid plugin name")
	}
	info, err := readBody[protocol.PluginInfo](ctx, db, "SELECT body FROM plugins WHERE name=?1 AND installed=1", name)
	if info != nil {
		refreshPluginCompatibility(info)
	}
	return info, err
}

func refreshPluginCompatibility(info *protocol.PluginInfo) {
	var minimum *string
	if info.Extension != nil {
		minimum = info.Extension.MinSailryVersion
	}
	info.Summary.Compatibility = plugins.Compatibility(minimum)
}

func PluginRecorded(ctx context.Context, db *sql.Conn, name string) (bool, error) {
	var recorded bool
	err := db.QueryRowContext(ctx, "SELECT EXISTS(SELECT 1 FROM plugins WHERE name=?1)", name).Scan(&recorded)
	return recorded, err
}

func pluginRevision(ctx context.Context, db *sql.Conn, name string) (uint64, uint64, error) {
	var revision, settings uint64
	err := db.QueryRowContext(ctx, "SELECT revision,settings_revision FROM plugins WHERE name=?1", name).Scan(&revision, &settings)
	if errors.Is(err, sql.ErrNoRows) {
		return 0, 0, nil
	}
	return revision, settings, err
}

func savePluginInventory(ctx context.Context, db *sql.Conn, info protocol.PluginInfo, installed bool) error {
	body, err := encode(info)
	if err != nil {
		return err
	}
	_, err = db.ExecContext(ctx, `INSERT INTO plugins(name,revision,settings_revision,installed,body) VALUES(?1,?2,?3,?4,?5)
ON CONFLICT(name) DO UPDATE SET revision=excluded.revision,settings_revision=excluded.settings_revision,installed=excluded.installed,body=excluded.body`,
		info.Summary.Name, info.Summary.Revision, info.Summary.SettingsRevision, installed, body)
	return err
}

func InstallPlugin(ctx context.Context, db *sql.Conn, info protocol.PluginInfo, expectedRevision uint64, enabled bool) (protocol.PluginInfo, error) {
	if !plugins.ValidateName(info.Summary.Name) || len(info.Summary.Digest) != 64 {
		return protocol.PluginInfo{}, invalid("invalid plugin package identity")
	}
	refreshPluginCompatibility(&info)
	if err := plugins.EnsureCompatible(info.Summary.Compatibility); err != nil {
		return protocol.PluginInfo{}, err
	}
	revision, settings, err := pluginRevision(ctx, db, info.Summary.Name)
	if err != nil {
		return protocol.PluginInfo{}, err
	}
	if err := checkRevision(revision, expectedRevision); err != nil {
		return protocol.PluginInfo{}, err
	}
	current, err := ReadPlugin(ctx, db, info.Summary.Name)
	if err != nil {
		return protocol.PluginInfo{}, err
	}
	if current != nil {
		enabled = current.Summary.Enabled
	}
	var maximum uint64
	if err := db.QueryRowContext(ctx, "SELECT COALESCE(MAX(revision),0) FROM plugin_settings WHERE plugin=?1", info.Summary.Name).Scan(&maximum); err != nil {
		return protocol.PluginInfo{}, err
	}
	if maximum > math.MaxInt64 {
		return protocol.PluginInfo{}, invalid("plugin settings revision exhausted")
	}
	settings = max(settings, maximum)
	immutable, err := encode(info)
	if err != nil {
		return protocol.PluginInfo{}, err
	}
	var existingName string
	err = db.QueryRowContext(ctx, "SELECT name FROM plugin_packages WHERE digest=?1", info.Summary.Digest).Scan(&existingName)
	if err != nil && !errors.Is(err, sql.ErrNoRows) {
		return protocol.PluginInfo{}, err
	}
	if err == nil && existingName != info.Summary.Name {
		return protocol.PluginInfo{}, invalid("plugin digest belongs to another package")
	}
	if _, err := db.ExecContext(ctx, "INSERT INTO plugin_packages(digest,name,body) VALUES(?1,?2,?3) ON CONFLICT(digest) DO NOTHING", info.Summary.Digest, info.Summary.Name, immutable); err != nil {
		return protocol.PluginInfo{}, err
	}
	info.Summary.Revision, err = nextRevision(revision)
	if err != nil {
		return protocol.PluginInfo{}, err
	}
	info.Summary.SettingsRevision, info.Summary.Enabled = settings, enabled
	if err := savePluginInventory(ctx, db, info, true); err != nil {
		return protocol.PluginInfo{}, err
	}
	return info, nil
}

func SetPluginEnabled(ctx context.Context, db *sql.Conn, name string, expectedRevision uint64, enabled bool) (protocol.PluginInfo, error) {
	info, err := ReadPlugin(ctx, db, name)
	if err != nil {
		return protocol.PluginInfo{}, err
	}
	if info == nil {
		return protocol.PluginInfo{}, fault(protocol.ErrorCodeNotFound, "plugin is not installed")
	}
	if err := checkRevision(info.Summary.Revision, expectedRevision); err != nil {
		return protocol.PluginInfo{}, err
	}
	if info.Summary.Enabled == enabled {
		return *info, nil
	}
	info.Summary.Revision, err = nextRevision(info.Summary.Revision)
	if err != nil {
		return protocol.PluginInfo{}, err
	}
	info.Summary.Enabled = enabled
	if err := savePluginInventory(ctx, db, *info, true); err != nil {
		return protocol.PluginInfo{}, err
	}
	return *info, nil
}

func RemovePlugin(ctx context.Context, db *sql.Conn, name string, expectedRevision uint64) (protocol.PluginSummary, error) {
	info, err := ReadPlugin(ctx, db, name)
	if err != nil {
		return protocol.PluginSummary{}, err
	}
	if info == nil {
		return protocol.PluginSummary{}, fault(protocol.ErrorCodeNotFound, "plugin is not installed")
	}
	if err := checkRevision(info.Summary.Revision, expectedRevision); err != nil {
		return protocol.PluginSummary{}, err
	}
	info.Summary.Revision, err = nextRevision(info.Summary.Revision)
	if err != nil {
		return protocol.PluginSummary{}, err
	}
	info.Summary.Enabled = false
	if err := savePluginInventory(ctx, db, *info, false); err != nil {
		return protocol.PluginSummary{}, err
	}
	return info.Summary, nil
}

func ResolvePluginInfo(ctx context.Context, db *sql.Conn, reference protocol.PluginReference) (protocol.PluginInfo, error) {
	info, err := ReadPluginVersion(ctx, db, reference)
	if err != nil {
		return info, err
	}
	refreshPluginCompatibility(&info)
	if info.Summary.Compatibility.Kind != "compatible" {
		return protocol.PluginInfo{}, fault(protocol.ErrorCodeNotConfigured, "plugin version is incompatible")
	}
	return info, nil
}

func PluginSettingsBody(ctx context.Context, db *sql.Conn, name string, revision uint64) (json.RawMessage, error) {
	var body []byte
	err := db.QueryRowContext(ctx, "SELECT body FROM plugin_settings WHERE plugin=?1 AND revision=?2", name, revision).Scan(&body)
	if errors.Is(err, sql.ErrNoRows) {
		return nil, fault(protocol.ErrorCodeNotConfigured, "plugin configuration revision is unavailable")
	}
	return body, err
}
