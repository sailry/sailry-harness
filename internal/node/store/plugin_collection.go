package store

import (
	"context"
	"database/sql"
	"encoding/json"

	"github.com/sailry/sailry-harness/internal/protocol"
)

// pluginHolders lists every durable owner of a package capture: queued and
// running dispatch jobs, admitted turns, the provider bindings of saved
// providers and of the runs, sessions, rosters and media that captured one,
// and bound assistants.
const pluginHolders = `SELECT json_extract(reference,'$') FROM dispatch_jobs WHERE status IN ('queued','running') AND json_type(reference)='object'
UNION SELECT json_extract(body,'$.provider.binding.package') FROM providers WHERE json_type(body,'$.provider.binding.package')='object'
UNION SELECT item.value FROM turns,json_each(turns.plugins) item
UNION SELECT json_extract(provider,'$.binding.package') FROM agent_runs WHERE json_type(provider,'$.binding.package')='object'
UNION SELECT json_extract(profile,'$.provider.binding.package') FROM session_revisions WHERE json_type(profile,'$.provider.binding.package')='object'
UNION SELECT json_extract(item.value,'$.binding.package') FROM session_revisions,json_each(session_revisions.roles,'$.providers') item WHERE json_type(item.value,'$.binding.package')='object'
UNION SELECT json_extract(item.value,'$.binding.package') FROM turns,json_each(turns.roles,'$.providers') item WHERE json_type(item.value,'$.binding.package')='object'
UNION SELECT json_extract(item.value,'$.provider.binding.package') FROM session_media,json_each(session_media.body) item WHERE json_type(item.value,'$.provider.binding.package')='object'
UNION SELECT json_extract(config,'$.assistant.package') FROM session_revisions WHERE json_type(config,'$.assistant.package')='object'`

// CollectPlugins deletes the package versions and settings revisions that no
// installation, queued callback, admitted turn or bound provider or assistant
// refers to, and returns the digests still in use so their directories can be
// kept. A removed package keeps its latest settings revision: reinstalling it
// restores that configuration. The caller runs this only while nothing can
// install or capture a package.
func CollectPlugins(ctx context.Context, db *sql.Conn) (map[string]struct{}, error) {
	type revision struct {
		plugin   string
		revision uint64
	}
	packages, settings := map[string]struct{}{}, map[revision]struct{}{}
	rows, err := db.QueryContext(ctx, "SELECT name,settings_revision,installed,json_extract(body,'$.summary.digest') FROM plugins")
	if err != nil {
		return nil, err
	}
	for rows.Next() {
		var held revision
		var installed bool
		var digest string
		if err = rows.Scan(&held.plugin, &held.revision, &installed, &digest); err != nil {
			rows.Close()
			return nil, err
		}
		if installed {
			packages[digest] = struct{}{}
		}
		if held.revision != 0 {
			settings[held] = struct{}{}
		}
	}
	if err = closeRows(rows); err != nil {
		return nil, err
	}
	rows, err = db.QueryContext(ctx, pluginHolders)
	if err != nil {
		return nil, err
	}
	for rows.Next() {
		var body string
		if err = rows.Scan(&body); err != nil {
			rows.Close()
			return nil, err
		}
		var reference protocol.PluginReference
		if err = json.Unmarshal([]byte(body), &reference); err != nil {
			rows.Close()
			return nil, err
		}
		packages[reference.Digest] = struct{}{}
		if reference.SettingsRevision != 0 {
			settings[revision{reference.Name, reference.SettingsRevision}] = struct{}{}
		}
	}
	if err = closeRows(rows); err != nil {
		return nil, err
	}
	var unused []revision
	rows, err = db.QueryContext(ctx, "SELECT plugin,revision FROM plugin_settings")
	if err != nil {
		return nil, err
	}
	for rows.Next() {
		var stored revision
		if err = rows.Scan(&stored.plugin, &stored.revision); err != nil {
			rows.Close()
			return nil, err
		}
		if _, held := settings[stored]; !held {
			unused = append(unused, stored)
		}
	}
	if err = closeRows(rows); err != nil {
		return nil, err
	}
	for _, stored := range unused {
		if _, err = db.ExecContext(ctx, "DELETE FROM plugin_settings WHERE plugin=?1 AND revision=?2", stored.plugin, stored.revision); err != nil {
			return nil, err
		}
	}
	var digests []string
	rows, err = db.QueryContext(ctx, "SELECT digest FROM plugin_packages")
	if err != nil {
		return nil, err
	}
	for rows.Next() {
		var digest string
		if err = rows.Scan(&digest); err != nil {
			rows.Close()
			return nil, err
		}
		if _, held := packages[digest]; !held {
			digests = append(digests, digest)
		}
	}
	if err = closeRows(rows); err != nil {
		return nil, err
	}
	for _, digest := range digests {
		if _, err = db.ExecContext(ctx, "DELETE FROM plugin_packages WHERE digest=?1", digest); err != nil {
			return nil, err
		}
	}
	return packages, nil
}

func closeRows(rows *sql.Rows) error {
	err := rows.Err()
	rows.Close()
	return err
}
