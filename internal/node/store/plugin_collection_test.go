package store

import (
	"context"
	"database/sql"
	"encoding/json"
	"errors"
	"fmt"
	"maps"
	"slices"
	"strings"
	"testing"

	"github.com/sailry/sailry-harness/internal/protocol"
)

func testPluginCollection(t *testing.T) {
	storage, _, session, turn, _ := agentHistoryFixture(t, protocol.WorkModeCode)
	ctx := context.Background()
	digest := func(char byte) string { return strings.Repeat(string(char), 64) }
	reference := func(name string, char byte, settings uint64) []byte {
		body, err := json.Marshal(protocol.PluginReference{Name: name, Digest: digest(char), SettingsRevision: settings})
		if err != nil {
			t.Fatal(err)
		}
		return body
	}
	err := storage.Node.Write(ctx, func(db *sql.Conn) error {
		exec := func(query string, args ...any) error {
			_, err := db.ExecContext(ctx, query, args...)
			return err
		}
		version := func(name string, char byte) error {
			body, err := encode(pluginTestInfo(name, char))
			if err != nil {
				return err
			}
			return exec("INSERT INTO plugin_packages(digest,name,body) VALUES(?1,?2,?3)", digest(char), name, body)
		}
		settings := func(name string, revisions ...int) error {
			for _, revision := range revisions {
				if err := exec("INSERT INTO plugin_settings(plugin,revision,body) VALUES(?1,?2,X'7B7D')", name, revision); err != nil {
					return err
				}
			}
			return nil
		}
		// "kept" is installed at its second version and third settings
		// revision; the fixture turn captured the first of each.
		first, err := InstallPlugin(ctx, db, pluginTestInfo("kept", 'a'), 0, true)
		if err != nil {
			return err
		}
		if err = settings("kept", 1, 2, 3); err != nil {
			return err
		}
		if _, err = InstallPlugin(ctx, db, pluginTestInfo("kept", 'b'), first.Summary.Revision, true); err != nil {
			return err
		}
		if err = exec("UPDATE turns SET plugins=?2 WHERE id=?1", turn.String(), "["+string(reference("kept", 'a', 1))+"]"); err != nil {
			return err
		}
		// A queued callback holds its version; a finished one does not.
		for index, job := range []struct {
			name, status string
			char         byte
		}{{"queued", "queued", 'c'}, {"finished", "completed", 'd'}} {
			if err = version(job.name, job.char); err != nil {
				return err
			}
			if err = exec("INSERT INTO dispatch_jobs(id,package,queue,status,body,callback,reference,sequence) VALUES(?1,?2,'default',?3,X'7B7D',X'7B7D',?4,?5)",
				protocol.NewJobID().String(), job.name, job.status, reference(job.name, job.char, 0), index+1); err != nil {
				return err
			}
		}
		if err = exec("INSERT INTO dispatch_jobs(id,package,queue,status,body,callback,reference,sequence) VALUES(?1,'node','default','queued',X'7B7D',X'7B7D',X'6E756C6C',3)", protocol.NewJobID().String()); err != nil {
			return err
		}
		// A run's provider binding and a session's assistant each hold one.
		if err = errors.Join(version("bound", 'e'), settings("bound", 3, 4), version("assistant", '1')); err != nil {
			return err
		}
		if err = exec("UPDATE agent_runs SET provider=?2 WHERE turn=?1", turn.String(), `{"binding":{"package":`+string(reference("bound", 'e', 4))+`}}`); err != nil {
			return err
		}
		if err = exec("UPDATE session_revisions SET config=json_set(config,'$.assistant',json(?2)) WHERE session=?1", session.ID.String(), `{"package":`+string(reference("assistant", '1', 0))+`}`); err != nil {
			return err
		}
		// A removed package keeps its configuration, not its version.
		gone, err := InstallPlugin(ctx, db, pluginTestInfo("gone", '2'), 0, true)
		if err != nil {
			return err
		}
		if err = settings("gone", 1); err != nil {
			return err
		}
		if gone, err = InstallPlugin(ctx, db, pluginTestInfo("gone", '2'), gone.Summary.Revision, true); err != nil {
			return err
		}
		if _, err = RemovePlugin(ctx, db, "gone", gone.Summary.Revision); err != nil {
			return err
		}
		// Nothing refers to "stale".
		if err = errors.Join(version("stale", 'f'), settings("stale", 1)); err != nil {
			return err
		}
		expected := []string{digest('1'), digest('a'), digest('b'), digest('c'), digest('e')}
		for range 2 {
			retained, err := CollectPlugins(ctx, db)
			if err != nil {
				return err
			}
			if kept := slices.Sorted(maps.Keys(retained)); !slices.Equal(kept, expected) {
				return fmt.Errorf("retained digests: %v", kept)
			}
			var stored, revisions []string
			for query, values := range map[string]*[]string{
				"SELECT digest FROM plugin_packages ORDER BY digest":                         &stored,
				"SELECT plugin||'/'||revision FROM plugin_settings ORDER BY plugin,revision": &revisions,
			} {
				rows, err := db.QueryContext(ctx, query)
				if err != nil {
					return err
				}
				for rows.Next() {
					var value string
					if err = rows.Scan(&value); err != nil {
						rows.Close()
						return err
					}
					*values = append(*values, value)
				}
				if err = closeRows(rows); err != nil {
					return err
				}
			}
			if !slices.Equal(stored, expected) {
				return fmt.Errorf("stored versions: %v", stored)
			}
			if !slices.Equal(revisions, []string{"bound/4", "gone/1", "kept/1", "kept/3"}) {
				return fmt.Errorf("stored settings revisions: %v", revisions)
			}
		}
		if _, err = ReadPluginVersion(ctx, db, protocol.PluginReference{Name: "kept", Digest: digest('a'), SettingsRevision: 1}); err != nil {
			return fmt.Errorf("the turn's capture: %w", err)
		}
		var absent *protocol.Fault
		if _, err = ReadPluginVersion(ctx, db, protocol.PluginReference{Name: "stale", Digest: digest('f')}); !errors.As(err, &absent) || absent.Code != protocol.ErrorCodeNotConfigured {
			return fmt.Errorf("the collected version: %v", err)
		}
		// Reinstalling the removed package restores its configuration.
		restored, err := InstallPlugin(ctx, db, pluginTestInfo("gone", '2'), gone.Summary.Revision+1, true)
		if err != nil {
			return err
		}
		if _, err = ReadPluginVersion(ctx, db, protocol.PluginReference{Name: "gone", Digest: digest('2'), SettingsRevision: restored.Summary.SettingsRevision}); err != nil || restored.Summary.SettingsRevision != 1 {
			return fmt.Errorf("the reinstalled package: %d %v", restored.Summary.SettingsRevision, err)
		}
		return nil
	})
	if err != nil {
		t.Fatal(err)
	}
}
