//go:build integration

package node

import (
	"bytes"
	"database/sql"
	"encoding/hex"
	"encoding/json"
	"strings"
	"testing"

	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/node/program"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
	"lukechampine.com/blake3"
)

func TestFollowedPluginChangesPreserveCapturedPrograms(t *testing.T) {
	for _, delivery := range []string{"snapshot", "manifest"} {
		t.Run(delivery, func(t *testing.T) {
			node := nodeFixture(t)
			source := protocol.NodeID{79}
			var user protocol.User
			if err := node.profile.Store.Node.Write(t.Context(), func(db *sql.Conn) error {
				if err := store.PairLinkPeer(t.Context(), db, source, link.DirectionController); err != nil {
					return err
				}
				var err error
				user, err = store.CallerUser(t.Context(), db, node.ID(), source)
				if err != nil {
					return err
				}
				_, err = db.ExecContext(t.Context(), "UPDATE users SET controller=?2 WHERE id=?1", user.ID.String(), source[:])
				return err
			}); err != nil {
				t.Fatal(err)
			}
			info := protocol.DefaultPluginInfo()
			info.Summary.Name, info.Summary.Digest = "followed-version-fixture", strings.Repeat("a", 64)
			info.Summary.Enabled, info.Summary.Revision = true, 1
			other := protocol.DefaultPluginInfo()
			other.Summary.Name, other.Summary.Digest = "unrelated-version-fixture", strings.Repeat("b", 64)
			other.Summary.Enabled, other.Summary.Revision = true, 1
			record := func(info protocol.PluginInfo) protocol.ConfigurationFollowRecord {
				t.Helper()
				body, err := json.Marshal(info)
				if err != nil {
					t.Fatal(err)
				}
				value := protocol.ConfigurationFollowRecord{ID: info.Summary.Name, Revision: info.Summary.Revision, Body: body}
				body, err = json.Marshal(value)
				if err != nil {
					t.Fatal(err)
				}
				sum := blake3.Sum256(body)
				value.Digest = hex.EncodeToString(sum[:])
				return value
			}
			revision := uint64(0)
			apply := func(records ...protocol.ConfigurationFollowRecord) {
				t.Helper()
				revision++
				manifest := protocol.ConfigurationManifest{Source: source, Category: "plugins", Revision: revision, Records: records}
				input := &protocol.ConfigurationFollowCommand{Action: "apply", Manifest: &manifest}
				request := protocol.NewRequest(node.ID(), protocol.Command{Kind: "configuration_follow", Data: input})
				if delivery == "snapshot" {
					var body bytes.Buffer
					if err := writeConfigurationSnapshot(&body, manifest); err != nil {
						t.Fatal(err)
					}
					sum := blake3.Sum256(body.Bytes())
					page := &protocol.ConfigurationSnapshotPage{Root: protocol.NewRequestID(), Source: source, Category: "plugins", Revision: revision,
						Bytes: uint64(body.Len()), Digest: hex.EncodeToString(sum[:]), Data: body.Bytes()}
					request.Command.Data = &protocol.ConfigurationFollowCommand{Action: "apply_page", Page: page}
					request.ID = protocol.ConfigurationPageRequestID(page.Root, 0)
				}
				if result := dispatchFixture(t, node, source, request); result.Fault != nil {
					t.Fatal(result.Fault)
				}
			}
			alive := func(label string, entries ...*program.Entry) {
				t.Helper()
				for _, entry := range entries {
					select {
					case <-entry.Process.Done():
						t.Fatalf("%s interrupted a captured program", label)
					default:
					}
				}
			}
			stopped := func(label string, entry *program.Entry) {
				t.Helper()
				select {
				case <-entry.Process.Done():
				default:
					t.Fatalf("%s retained the invalidated program", label)
				}
			}
			apply(record(info), record(other))
			active := leasePluginProgramFixture(t, node, user.ID, info.Summary.Name)
			unrelated := leasePluginProgramFixture(t, node, user.ID, other.Summary.Name)
			for _, pinned := range []bool{true, false} {
				info.Summary.Pinned, info.Summary.Revision = pinned, info.Summary.Revision+1
				apply(record(info), record(other))
				alive("update preference", active, unrelated)
			}
			info.Summary.Digest, info.Summary.Revision = strings.Repeat("c", 64), info.Summary.Revision+1
			unchanged := record(other)
			unchanged.Body = nil
			apply(record(info), unchanged)
			alive("code replacement", active, unrelated)
			apply(record(info), record(other))
			alive("unchanged snapshot", active, unrelated)
			info.Summary.SettingsRevision, info.Summary.Revision = 1, info.Summary.Revision+1
			apply(record(info), unchanged)
			stopped("settings change", active)
			alive("another package's settings change", unrelated)
			active = leasePluginProgramFixture(t, node, user.ID, info.Summary.Name)
			info.Summary.Enabled, info.Summary.Revision = false, info.Summary.Revision+1
			apply(record(info), unchanged)
			stopped("disable", active)
			alive("another package's disable", unrelated)
			info.Summary.Enabled, info.Summary.Revision = true, info.Summary.Revision+1
			apply(record(info), unchanged)
			active = leasePluginProgramFixture(t, node, user.ID, info.Summary.Name)
			apply(unchanged)
			stopped("removal", active)
			alive("another package's removal", unrelated)
		})
	}
}
