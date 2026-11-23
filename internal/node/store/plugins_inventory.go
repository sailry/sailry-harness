package store

import (
	"context"
	"database/sql"
	"encoding/json"
	"errors"
	"math"
	"time"

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
	user, err := pluginUserID(ctx, db)
	if err != nil {
		return nil, err
	}
	infos, err := readBodies[protocol.PluginInfo](ctx, db, "SELECT body FROM plugin_user_installations WHERE user=?1 AND installed=1 ORDER BY name", user)
	for index := range infos {
		refreshPluginCompatibility(&infos[index])
	}
	return infos, err
}

func ReadPlugin(ctx context.Context, db *sql.Conn, name string) (*protocol.PluginInfo, error) {
	if !plugins.ValidateName(name) {
		return nil, invalid("invalid plugin name")
	}
	user, err := pluginUserID(ctx, db)
	if err != nil {
		return nil, err
	}
	info, err := readBody[protocol.PluginInfo](ctx, db, "SELECT body FROM plugin_user_installations WHERE name=?1 AND user=?2 AND installed=1", name, user)
	if info != nil {
		refreshPluginCompatibility(info)
	}
	return info, err
}

func refreshPluginCompatibility(info *protocol.PluginInfo) {
	info.Summary.Compatibility = plugins.InfoCompatibility(*info)
	for index := range info.Versions {
		info.Versions[index].Compatibility = plugins.CatalogVersionCompatibility(info.Versions[index])
	}
}

func PluginRecorded(ctx context.Context, db *sql.Conn, name string) (bool, error) {
	user, err := pluginUserID(ctx, db)
	if err != nil {
		return false, err
	}
	var recorded bool
	err = db.QueryRowContext(ctx, "SELECT EXISTS(SELECT 1 FROM plugin_user_installations WHERE name=?1 AND user=?2)", name, user).Scan(&recorded)
	return recorded, err
}

func RecordedPluginNames(ctx context.Context, db *sql.Conn) (map[string]bool, error) {
	user, err := pluginUserID(ctx, db)
	if err != nil {
		return nil, err
	}
	rows, err := db.QueryContext(ctx, "SELECT name FROM plugin_user_installations WHERE user=?1", user)
	if err != nil {
		return nil, err
	}
	recorded := map[string]bool{}
	for rows.Next() {
		var name string
		if err := rows.Scan(&name); err != nil {
			_ = rows.Close()
			return nil, err
		}
		recorded[name] = true
	}
	if err := errors.Join(rows.Err(), rows.Close()); err != nil {
		return nil, err
	}
	return recorded, nil
}

func pluginRevision(ctx context.Context, db *sql.Conn, name string) (uint64, uint64, error) {
	user, err := pluginUserID(ctx, db)
	if err != nil {
		return 0, 0, err
	}
	var revision, settings uint64
	err = db.QueryRowContext(ctx, "SELECT revision,settings_revision FROM plugin_user_installations WHERE name=?1 AND user=?2", name, user).Scan(&revision, &settings)
	if errors.Is(err, sql.ErrNoRows) {
		return 0, 0, nil
	}
	return revision, settings, err
}

func savePluginInventory(ctx context.Context, db *sql.Conn, info protocol.PluginInfo, installed bool) error {
	user, err := pluginUserID(ctx, db)
	if err != nil {
		return err
	}
	body, err := encode(info)
	if err != nil {
		return err
	}
	_, err = db.ExecContext(ctx, `INSERT INTO plugin_user_installations(user,name,revision,settings_revision,installed,body) VALUES(?6,?1,?2,?3,?4,?5)
ON CONFLICT(user,name) DO UPDATE SET revision=excluded.revision,settings_revision=excluded.settings_revision,installed=excluded.installed,body=excluded.body`,
		info.Summary.Name, info.Summary.Revision, info.Summary.SettingsRevision, installed, body, user)
	return err
}

func InstallPlugin(ctx context.Context, db *sql.Conn, info protocol.PluginInfo, expectedRevision uint64, enabled bool) (protocol.PluginInfo, error) {
	return installPlugin(ctx, db, info, expectedRevision, enabled, "", protocol.PluginMcpConfiguration{})
}

func InstallCatalogPlugin(ctx context.Context, db *sql.Conn, info protocol.PluginInfo, expectedRevision uint64, mode protocol.PluginInstallMode, declared protocol.PluginMcpConfiguration) (protocol.PluginInfo, error) {
	switch mode {
	case protocol.PluginInstallModeInstall, protocol.PluginInstallModeUpdate, protocol.PluginInstallModeSwitch:
		return installPlugin(ctx, db, info, expectedRevision, true, mode, declared)
	default:
		return protocol.PluginInfo{}, invalid("invalid catalog installation mode")
	}
}

func installPlugin(ctx context.Context, db *sql.Conn, info protocol.PluginInfo, expectedRevision uint64, enabled bool, mode protocol.PluginInstallMode, declared protocol.PluginMcpConfiguration) (protocol.PluginInfo, error) {
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
	current, err := ReadPlugin(ctx, db, info.Summary.Name)
	if err != nil {
		return protocol.PluginInfo{}, err
	}
	// Zero asserts absence from the installed inventory exposed to clients.
	// Removed packages retain their revisions for monotonic reinstall identity.
	if current != nil || expectedRevision != 0 {
		if err := checkRevision(revision, expectedRevision); err != nil {
			return protocol.PluginInfo{}, err
		}
	}
	if current != nil {
		enabled = current.Summary.Enabled
		info.Summary.Pinned = current.Summary.Pinned
	}
	if mode != "" {
		if err := validateCatalogReplacement(ctx, db, current, &info, mode, declared); err != nil {
			return protocol.PluginInfo{}, err
		}
	}
	user, err := pluginUserID(ctx, db)
	if err != nil {
		return protocol.PluginInfo{}, err
	}
	var maximum uint64
	if err := db.QueryRowContext(ctx, "SELECT COALESCE(MAX(revision),0) FROM plugin_user_settings WHERE plugin=?1 AND user=?2", info.Summary.Name, user).Scan(&maximum); err != nil {
		return protocol.PluginInfo{}, err
	}
	if maximum > math.MaxInt64 {
		return protocol.PluginInfo{}, invalid("plugin settings revision exhausted")
	}
	settings = max(settings, maximum)
	metadata := info
	metadata.Summary.Pinned = false
	immutable, err := encode(metadata)
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

// stopPluginContinuations stops the running automatic turns a package admitted
// once the package is disabled or removed. A queued one needs no
// stop: claiming it checks its package again and cancels it.
func stopPluginContinuations(ctx context.Context, db *sql.Conn, name string) error {
	user, err := pluginUserID(ctx, db)
	if err != nil {
		return err
	}
	runs, err := readBodies[protocol.ConversationRun](ctx, db, `SELECT a.body FROM agent_continuations c JOIN agent_runs a ON a.turn=c.turn
JOIN turns t ON t.id=a.turn
WHERE json_extract(c.body,'$.package.name')=?1 AND json_extract(a.body,'$.status')='running' AND coalesce((SELECT user FROM identity_turn_origins o WHERE o.turn=t.id),t.user)=?2`, name, user)
	if err != nil {
		return err
	}
	for _, run := range runs {
		if _, err := interruptAgentTurn(ctx, db, run.Turn, time.Now()); err != nil {
			return err
		}
	}
	return nil
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
	if enabled {
		if err := plugins.EnsureCompatible(info.Summary.Compatibility); err != nil {
			return protocol.PluginInfo{}, err
		}
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
	if !enabled {
		if err := stopPluginContinuations(ctx, db, name); err != nil {
			return protocol.PluginInfo{}, err
		}
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
	// A standalone MCP package owns its connection records, and nothing else
	// can reach them once it is gone. A package with declared connectors keeps
	// its records for a later install.
	if info.McpSource != nil {
		user, err := pluginUserID(ctx, db)
		if err != nil {
			return protocol.PluginSummary{}, err
		}
		if _, err := db.ExecContext(ctx, "DELETE FROM plugin_connections WHERE plugin=?1 AND user=?2", name, user); err != nil {
			return protocol.PluginSummary{}, err
		}
	}
	if err := savePluginInventory(ctx, db, *info, false); err != nil {
		return protocol.PluginSummary{}, err
	}
	return info.Summary, stopPluginContinuations(ctx, db, name)
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
	user, err := pluginUserID(ctx, db)
	if err != nil {
		return nil, err
	}
	var body []byte
	err = db.QueryRowContext(ctx, "SELECT body FROM plugin_user_settings WHERE plugin=?1 AND revision=?2 AND user=?3", name, revision, user).Scan(&body)
	if errors.Is(err, sql.ErrNoRows) {
		return nil, fault(protocol.ErrorCodeNotConfigured, "plugin configuration revision is unavailable")
	}
	return body, err
}
