//go:build integration && unix

package application

import (
	"bytes"
	"context"
	"crypto/ed25519"
	"crypto/sha256"
	"database/sql"
	_ "embed"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"image"
	"image/png"
	"io/fs"
	"os"
	"path/filepath"
	"slices"
	"strings"
	"testing"

	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/core"
	"github.com/sailry/sailry-harness/internal/desktop/preferences"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/localmodels"
	"github.com/sailry/sailry-harness/internal/node"
	"github.com/sailry/sailry-harness/internal/node/logging"
	"github.com/sailry/sailry-harness/internal/node/plugins"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
	"lukechampine.com/blake3"
)

//go:embed testdata/rust-node-schema.sql
var rustNodeSchema string

//go:embed testdata/v0.2.13-code-index-schema.sql
var legacyCodeIndexSchema string

const (
	rustSession    = "0197c0de-0000-7000-8000-000000000001"
	rustWorktree   = "0197c0de-0000-7000-8000-000000000002"
	rustAttachment = "0197c0de-0000-7000-8000-000000000004"
	rustPlugin     = "rust-upgrade-fixture"
	rustTitle      = "Preserved Rust conversation"
)

// rustPreferences is desktop/preferences.json as the Rust application's
// serde_json writer produces it: every field of its v1 Data in declaration
// order, absent options as null.
const rustPreferences = `{
  "version": 1,
  "notifications": [
    true,
    false,
    true,
    true
  ],
  "toast_seconds": 8,
  "pairing": "",
  "iroh_relays": null,
  "devices": {
    "5d0f6e2a9c3b4f1e8a7d6c5b4a39281706f5e4d3c2b1a0918273645546372819": {
      "name": "Studio",
      "execution": true,
      "platform": "macos"
    }
  },
  "terminal": {
    "font_size": 15,
    "font_family": "JetBrains Mono",
    "paste_protection": true
  },
  "appearance": {
    "package": "ink-dragon",
    "mode": "dark"
  },
  "theme_effects": true,
  "composer": {
    "model": null,
    "mode": "code",
    "permission": "ask"
  },
  "feature_pins": {
    "terminal": true
  },
  "feature_order": [
    "files",
    "terminal"
  ],
  "browser_persistent": false,
  "surfaces": {
    "main": 83.5,
    "sidebar": null
  },
  "shortcuts": {
    "new_session": "cmd-n"
  },
  "dictation": {
    "microphone": null,
    "language": "auto",
    "model": null
  },
  "pins": {
    "5d0f6e2a9c3b4f1e8a7d6c5b4a39281706f5e4d3c2b1a0918273645546372819/0197c0de-0000-7000-8000-000000000001": {
      "pinned": true,
      "order": 0
    }
  },
  "artifact_drafts": null,
  "plugin_directories": null,
  "message_display": "compact",
  "session_order": "updated",
  "message_text_size": 16,
  "file_picker": {
    "grid": true,
    "hidden": false
  },
  "language": "chinese",
  "update_channel": "stable",
  "welcome_seen": true,
  "sidebar_metrics": true
}`

// rustProfile writes the files a Rust-era ~/.sailry holds, following
// runtime-core/src/profile.rs and the owners of each directory, and returns
// the profile path. Its native databases use the historical Rust schema.
func rustProfile(t *testing.T) string {
	t.Helper()
	profile := filepath.Join(t.TempDir(), ".sailry")
	seed := make([]byte, ed25519.SeedSize)
	for index := range seed {
		seed[index] = byte(index + 1)
	}
	files := map[string][]byte{
		"node.lock":                {},
		"link.key":                 seed,
		"desktop/preferences.json": []byte(rustPreferences),
		"plugins/data/" + rustPlugin + "/state.json":                         []byte(`{"count":1}`),
		"attachments/" + rustAttachment:                                      []byte("attachment bytes\n"),
		"models/embedding/bge-small/files/model.onnx":                        []byte("onnx"),
		"models/decision/laya/model.gguf":                                    []byte("gguf"),
		"models/ocr/ppocr/model.onnx":                                        []byte("onnx"),
		"models/sensevoice-int8/model.onnx":                                  []byte("onnx"),
		"models/sensevoice-int8/tokens.txt":                                  []byte("a\n"),
		"models/speech.json":                                                 []byte(`{"version":1}`),
		"python/environments/0a1b2c/pyvenv.cfg":                              []byte("home = /bundle\n"),
		"python/environments/0a1b2c/receipt.json":                            []byte(`{"version":1}`),
		"python/cache/wheel.whl":                                             []byte("wheel"),
		"python/staging/leftover/file":                                       []byte("staged"),
		"worktrees/0197c0de-0000-7000-8000-000000000002/README.md":           []byte("managed worktree\n"),
		"workspaces/sessions/0197c0de-0000-7000-8000-000000000001/notes.txt": []byte("session files\n"),
		"assistants/9e107d9d372bb6826bd81d3542a419d6/workspace/draft.md":     []byte("assistant files\n"),
		"artifacts/0197c0de-0000-7000-8000-000000000003/index.html":          []byte("<p>artifact</p>"),
		"logs/desktop/sailry.2026-11-10.log":                                 []byte("rust log line\n"),
		"logs/host/sailry.2026-11-10.log":                                    []byte("rust host log line\n"),
		"tools/chromium/128.0/chrome":                                        []byte("browser"),
		"updater/receipt.json":                                               []byte(`{"version":"0.1.4"}`),
		"tmp/leftover.bin":                                                   []byte("temporary"),
	}
	// An installed theme is theme.json and its declared resources, the files
	// the Rust installer writes. The fixture needs no external theme source.
	files["desktop/themes/ink-dragon/theme.json"] = []byte(`{
		"version":1,"id":"ink-dragon","name":"Installed theme","package_version":"0.1.0",
		"license":{"author":"Sailry","name":"CC0-1.0","text":"Synthetic upgrade fixture artwork, dedicated to the public domain under CC0 1.0"},
		"light":{"theme":{"mode":"light","colors":{"background":"#edf5fb"}},"assets":{"background.content":{"path":"assets/preserved.png"}}},
		"dark":{"theme":{"mode":"dark","colors":{"background":"#18242f"}},"assets":{"background.content":{"path":"assets/preserved.png"}}}
	}`)
	var artwork bytes.Buffer
	if err := png.Encode(&artwork, image.NewRGBA(image.Rect(0, 0, 2, 2))); err != nil {
		t.Fatal(err)
	}
	files["desktop/themes/ink-dragon/assets/preserved.png"] = artwork.Bytes()
	for name, body := range files {
		path := filepath.Join(profile, filepath.FromSlash(name))
		if err := os.MkdirAll(filepath.Dir(path), 0o700); err != nil {
			t.Fatal(err)
		}
		if err := os.WriteFile(path, body, 0o600); err != nil {
			t.Fatal(err)
		}
	}
	rustDatabases(t, profile, seed)
	return profile
}

func rustDatabases(t *testing.T, profile string, seed []byte) {
	t.Helper()
	ctx := t.Context()
	source := t.TempDir()
	if err := os.WriteFile(filepath.Join(source, "plugin.json"), []byte(`{"$schema":"`+plugins.ManifestSchema+`","name":"`+rustPlugin+`","version":"0.1.0"}`), 0o600); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(source, "main.js"), []byte("export default {}\n"), 0o600); err != nil {
		t.Fatal(err)
	}
	info, err := plugins.NewManager(profile).Install(ctx, source, "", rustPlugin)
	if err != nil {
		t.Fatal(err)
	}
	info.Summary.Revision, info.Summary.SettingsRevision, info.Summary.Enabled = 1, 1, false
	encode := func(value any) []byte {
		t.Helper()
		body, err := json.Marshal(value)
		if err != nil {
			t.Fatal(err)
		}
		return body
	}
	public := ed25519.NewKeyFromSeed(seed).Public().(ed25519.PublicKey)
	attachment := protocol.DefaultAttachment()
	attachment.ID, err = protocol.ParseAttachmentID(rustAttachment)
	if err != nil {
		t.Fatal(err)
	}
	attachment.Spec.Worktree, err = protocol.ParseWorktreeID(rustWorktree)
	if err != nil {
		t.Fatal(err)
	}
	attachment.Spec.Name, attachment.Spec.MediaType = "kept.txt", "text/plain"
	attachment.Spec.Size = uint64(len("attachment bytes\n"))
	attachment.Spec.Revision = fmt.Sprintf("%x", blake3.Sum256([]byte("attachment bytes\n")))
	telemetryBody := encode(protocol.DefaultTelemetrySettings())
	modelsBody := encode(protocol.DefaultModelsLocalSettings())
	config := protocol.DefaultSessionConfig()
	config.Provider, err = protocol.ParseProviderID("0197c0de-0000-7000-8000-000000000006")
	if err != nil {
		t.Fatal(err)
	}
	config.Model, config.Mode, config.Permission = "fixture-model", protocol.WorkModeCode, protocol.PermissionAsk
	configBody := encode(config)
	rolesBody := encode(protocol.DefaultRoleSnapshot())
	titleBody := encode(rustTitle)
	attachmentBody := encode(attachment)
	pluginBody := encode(info)
	create := func(path, schema string, populate func(*sql.Conn) error) {
		t.Helper()
		if err := core.PrivateDirectory(filepath.Dir(path)); err != nil {
			t.Fatal(err)
		}
		handle, err := store.OpenHandle(ctx, path)
		if err != nil {
			t.Fatal(err)
		}
		defer handle.Close()
		if err := handle.Write(ctx, func(db *sql.Conn) error {
			if _, err := db.ExecContext(ctx, schema); err != nil {
				return err
			}
			if populate != nil {
				return populate(db)
			}
			return nil
		}); err != nil {
			t.Fatal(err)
		}
		closed, err := handle.Pause(ctx)
		if err != nil {
			t.Fatal(err)
		}
		if err := closed.Close(); err != nil {
			t.Fatal(err)
		}
	}
	populate := func(db *sql.Conn) error {
		if _, err := db.ExecContext(ctx, "PRAGMA application_id=0x5341494c; PRAGMA user_version=1"); err != nil {
			return err
		}
		for _, statement := range []struct {
			query string
			args  []any
		}{
			{"INSERT INTO node(singleton,identity,plugin_defaults,tool_defaults,tool_metadata_names) VALUES(1,?1,1,1,1)", []any{public}},
			{"INSERT INTO telemetry_settings VALUES(1,?1)", []any{telemetryBody}},
			{"INSERT INTO local_model_settings VALUES(1,?1)", []any{modelsBody}},
			{"INSERT INTO worktrees(id,path,main) VALUES(?1,?2,1)", []any{rustWorktree, filepath.Join(profile, "worktrees", rustWorktree)}},
			{"INSERT INTO sessions(rowid,id,worktree,revision) VALUES(1,?1,?2,1)", []any{rustSession, rustWorktree}},
			{"INSERT INTO counters(namespace,value) VALUES('sessions',1)", nil},
			{"INSERT INTO session_revisions(session,revision,worktree,config,roles) VALUES(?1,1,?2,?3,?4)", []any{rustSession, rustWorktree, configBody, rolesBody}},
			{"INSERT INTO agent_state(owner,key,body) VALUES(?1,'sailry.session_title',?2)", []any{rustSession, titleBody}},
			{"INSERT INTO attachments(id,worktree,caller,body) VALUES(?1,?2,?3,?4)", []any{rustAttachment, rustWorktree, public, attachmentBody}},
			{"INSERT INTO plugins(name,revision,settings_revision,installed,body) VALUES(?1,1,1,1,?2)", []any{rustPlugin, pluginBody}},
			{"INSERT INTO plugin_packages(digest,name,body) VALUES(?1,?2,?3)", []any{info.Summary.Digest, rustPlugin, pluginBody}},
			{"INSERT INTO plugin_settings(plugin,revision,body) VALUES(?1,1,?2)", []any{rustPlugin, []byte(`{"schema":null,"values":{},"slots":{},"mcp_targets":{},"mcp":null}`)}},
			{"INSERT INTO plugin_values(name,key,revision,value) VALUES(?1,'kept',1,?2)", []any{rustPlugin, []byte(`{"count":1}`)}},
		} {
			if _, err := db.ExecContext(ctx, statement.query, statement.args...); err != nil {
				return err
			}
		}
		var modern int
		if err := db.QueryRowContext(ctx, "SELECT count(*) FROM pragma_table_info('sessions') WHERE name='owner'").Scan(&modern); err != nil {
			return err
		}
		if modern != 0 {
			return errors.New("Rust fixture already includes the new owner column")
		}
		return nil
	}
	create(filepath.Join(profile, "storage", "node.turso"), rustNodeSchema, populate)
	create(filepath.Join(profile, "storage", "knowledge.turso"), store.KnowledgeSchema, nil)
	create(filepath.Join(profile, "cache", "code-index", rustWorktree, "index.turso"), legacyCodeIndexSchema, nil)
	create(filepath.Join(profile, "storage", "backups", "node-before-upgrade-0197c0de-0000-7000-8000-00000000000a.turso"), rustNodeSchema, nil)
}

// snapshot records every file of the profile outside tmp, which both
// implementations treat as disposable.
func snapshot(t *testing.T, profile string) map[string]string {
	t.Helper()
	files := map[string]string{}
	err := filepath.WalkDir(profile, func(path string, entry fs.DirEntry, err error) error {
		if err != nil {
			return err
		}
		relative, err := filepath.Rel(profile, path)
		if err != nil {
			return err
		}
		if entry.IsDir() {
			if relative == "tmp" {
				return filepath.SkipDir
			}
			return nil
		}
		if entry.Type()&fs.ModeSocket != 0 {
			return nil
		}
		body, err := os.ReadFile(path)
		if err != nil {
			return err
		}
		info, err := entry.Info()
		if err != nil {
			return err
		}
		sum := sha256.Sum256(body)
		files[filepath.ToSlash(relative)] = info.Mode().String() + " " + hex.EncodeToString(sum[:])
		return nil
	})
	if err != nil {
		t.Fatal(err)
	}
	return files
}

// preserved checks every retained file byte and permission. Successful startup
// may change the two native database families through additive schema sync;
// refused startup must preserve those files byte for byte as well.
func preserved(t *testing.T, profile string, before map[string]string, started bool) []string {
	t.Helper()
	after := snapshot(t, profile)
	for name, state := range before {
		if started && (slices.Contains(core.DatabaseFiles("storage/node.turso"), name) || slices.Contains(core.DatabaseFiles("storage/knowledge.turso"), name)) {
			continue
		}
		switch current, present := after[name]; {
		case !present:
			t.Errorf("%s was deleted", name)
		case current != state:
			t.Errorf("%s changed: %s, was %s", name, current, state)
		}
	}
	var added []string
	for name := range after {
		if _, existed := before[name]; !existed {
			added = append(added, name)
		}
	}
	return added
}

func assertRustProjection(t *testing.T, backend *desktopBackend) {
	t.Helper()
	projection, err := backend.Node.Snapshot(t.Context())
	if err != nil {
		t.Fatal(err)
	}
	session := slices.IndexFunc(projection.Sessions, func(value protocol.Session) bool { return value.ID.String() == rustSession })
	if session < 0 || projection.Sessions[session].Activity.Title != rustTitle || projection.Sessions[session].Worktree.String() != rustWorktree {
		t.Fatalf("Rust session was not projected: %+v", projection.Sessions)
	}
	installed := slices.IndexFunc(projection.Plugins, func(value protocol.PluginSummary) bool { return value.Name == rustPlugin })
	if installed < 0 || projection.Plugins[installed].Enabled || projection.Plugins[installed].Revision != 1 || projection.Plugins[installed].SettingsRevision != 1 {
		t.Fatalf("Rust plugin configuration was not projected: %+v", projection.Plugins)
	}
}

func assertRustStored(t *testing.T, profile string) {
	t.Helper()
	ctx := t.Context()
	handle, err := store.OpenHandle(ctx, filepath.Join(profile, "storage", "node.turso"))
	if err != nil {
		t.Fatal(err)
	}
	defer handle.Close()
	if err := handle.Read(ctx, func(db *sql.Conn) error {
		var owner, local, state, title string
		if err := db.QueryRowContext(ctx, "SELECT id FROM users WHERE local=1").Scan(&local); err != nil {
			return err
		}
		if err := db.QueryRowContext(ctx, "SELECT owner,state FROM sessions WHERE id=?1", rustSession).Scan(&owner, &state); err != nil {
			return err
		}
		if owner != local || state != "active" {
			return errors.New("additive session owner did not retain the local user's session")
		}
		if err := db.QueryRowContext(ctx, "SELECT body FROM agent_state WHERE owner=?1 AND key='sailry.session_title'", rustSession).Scan(&title); err != nil {
			return err
		}
		if title != `"`+rustTitle+`"` {
			return errors.New("Rust conversation title changed")
		}
		for _, query := range []string{
			"SELECT count(*) FROM plugins WHERE name='" + rustPlugin + "' AND installed=1 AND revision=1 AND settings_revision=1",
			"SELECT count(*) FROM plugin_user_installations WHERE name='" + rustPlugin + "' AND user=(SELECT id FROM users WHERE local=1) AND installed=1 AND revision=1 AND settings_revision=1",
			"SELECT count(*) FROM plugin_settings WHERE plugin='" + rustPlugin + "' AND revision=1",
			"SELECT count(*) FROM plugin_user_settings WHERE plugin='" + rustPlugin + "' AND user=(SELECT id FROM users WHERE local=1) AND revision=1",
		} {
			var count int
			if err := db.QueryRowContext(ctx, query).Scan(&count); err != nil {
				return err
			}
			if count != 1 {
				return fmt.Errorf("Rust record was not retained: %s", query)
			}
		}
		var legacy, current []byte
		if err := db.QueryRowContext(ctx, "SELECT value FROM plugin_values WHERE name=?1 AND key='kept'", rustPlugin).Scan(&legacy); err != nil {
			return err
		}
		if err := db.QueryRowContext(ctx, "SELECT value FROM plugin_space_values WHERE name=?1 AND key='kept' AND space='user:'||(SELECT id FROM users WHERE local=1)", rustPlugin).Scan(&current); err != nil {
			return err
		}
		if !bytes.Equal(legacy, []byte(`{"count":1}`)) || !bytes.Equal(current, legacy) {
			return errors.New("Rust plugin data or its local user copy changed")
		}
		var attachment protocol.Attachment
		var body []byte
		if err := db.QueryRowContext(ctx, "SELECT body FROM attachments WHERE id=?1", rustAttachment).Scan(&body); err != nil {
			return err
		}
		if err := json.Unmarshal(body, &attachment); err != nil {
			return err
		}
		if attachment.ID.String() != rustAttachment || attachment.Spec.Worktree.String() != rustWorktree || attachment.Spec.Size != uint64(len("attachment bytes\n")) {
			return errors.New("Rust attachment metadata changed")
		}
		return nil
	}); err != nil {
		t.Fatal(err)
	}
}

// startDesktop runs the Desktop backend start of openBackend with a local
// network scope, so the test needs no relay.
func startDesktop(t *testing.T, profile string) (*desktopBackend, error) {
	t.Helper()
	backend := &desktopBackend{Path: profile, Preferences: preferences.ForStartup(profile)}
	backend.logs, _ = logging.Install(profile, "desktop")
	options := node.Options{
		LocalModels: localmodels.Options{Capabilities: node.LocalModelCapabilities()},
		Network:     &link.NetworkScope{Bind: "127.0.0.1:0"},
	}
	var err error
	backend.Node, err = node.StartWithOptions(context.Background(), profile, options)
	if err != nil {
		backend.closeLogs()
		return nil, err
	}
	backend.Client = client.New(link.NewLocal(backend.Node.ID(), backend.Node.ID(), backend.Node))
	if err := backend.Client.Start(context.Background()); err != nil {
		_ = backend.Close()
		return nil, err
	}
	return backend, nil
}

// TestRustProfileUpgradeInPlace starts the Go Node on a synthetic profile
// written the way the Rust application left ~/.sailry.
func TestRustProfileUpgradeInPlace(t *testing.T) {
	t.Run("compatible profile", func(t *testing.T) {
		profile := rustProfile(t)
		before := snapshot(t, profile)
		originals := map[string]fs.FileInfo{}
		for _, name := range []string{"node.turso", "knowledge.turso"} {
			info, err := os.Stat(filepath.Join(profile, "storage", name))
			if err != nil {
				t.Fatal(err)
			}
			originals[name] = info
		}
		orphans := []string{
			filepath.Join("plugins", "packages", strings.Repeat("a", 64), "main.js"),
			filepath.Join("attachments", "0197c0de-0000-7000-8000-000000000005"),
		}
		for round := 1; round <= 2; round++ {
			if round == 2 {
				// Resources published without a committed reference are still
				// collected on an ordinary restart of the reused database.
				for _, orphan := range orphans {
					if err := os.MkdirAll(filepath.Dir(filepath.Join(profile, orphan)), 0o700); err != nil {
						t.Fatal(err)
					}
					if err := os.WriteFile(filepath.Join(profile, orphan), []byte("unreferenced current resource"), 0o600); err != nil {
						t.Fatal(err)
					}
				}
			}
			backend, err := startDesktop(t, profile)
			if err != nil {
				t.Fatalf("start %d: %v", round, err)
			}
			seed := make([]byte, ed25519.SeedSize)
			for index := range seed {
				seed[index] = byte(index + 1)
			}
			public := ed25519.NewKeyFromSeed(seed).Public().(ed25519.PublicKey)
			if identity := backend.Node.ID(); hex.EncodeToString(identity[:]) != hex.EncodeToString(public) {
				t.Errorf("Node identity %x does not come from the Rust link.key", identity)
			}
			if endpoint := backend.Node.Link(); endpoint == nil {
				t.Error("upgraded profile has no Link endpoint")
			} else if address, err := endpoint.Address(t.Context()); err != nil || address.ID != backend.Node.ID() || len(address.Addrs) == 0 {
				t.Errorf("upgraded Link endpoint = %v, %v", address, err)
			}
			stored := backend.Preferences
			if !stored.Readable || stored.Err != nil {
				t.Fatalf("Rust preferences were not read: %v", stored.Err)
			}
			data := stored.Data
			if data.ToastSeconds != 8 || data.Notifications != [4]bool{true, false, true, true} || data.Terminal.FontSize != 15 ||
				data.Terminal.FontFamily != "JetBrains Mono" || data.Appearance.Package != "ink-dragon" || data.Appearance.Mode != "dark" || data.Language == nil || *data.Language != "chinese" ||
				data.MessageTextSize == nil || *data.MessageTextSize != 16 || data.WelcomeSeen == nil || !*data.WelcomeSeen ||
				!data.SidebarMetrics || data.Devices == nil || len(*data.Devices) != 1 || data.Pins == nil || len(*data.Pins) != 1 ||
				data.Surfaces == nil || data.Surfaces.Main != 83.5 || data.Composer == nil || data.Dictation == nil {
				t.Errorf("Rust preferences were misread: %+v", data)
			}
			if err := stored.FinishStartup(); err != nil {
				t.Errorf("finish startup: %v", err)
			}
			// The theme the Rust application installed and selected loads.
			installed, err := theme.NewProfile(filepath.Dir(stored.Path)).Installed(context.Background())
			if err != nil || len(installed) != 1 || installed[0].Package.ID != data.Appearance.Package {
				t.Errorf("installed themes = %d, %v", len(installed), err)
			}
			assertRustProjection(t, backend)
			preserved(t, profile, before, true)
			for name, original := range originals {
				current, err := os.Stat(filepath.Join(profile, "storage", name))
				if err != nil || !os.SameFile(original, current) {
					t.Errorf("original %s was replaced: %v", name, err)
				}
			}
			for _, forbidden := range []string{"rust-archive", "node.sqlite", "knowledge.sqlite"} {
				if _, err := os.Lstat(filepath.Join(profile, "storage", forbidden)); !errors.Is(err, os.ErrNotExist) {
					t.Errorf("startup created unexpected storage %s: %v", forbidden, err)
				}
			}
			if round == 2 {
				for _, orphan := range orphans {
					if _, err := os.Stat(filepath.Join(profile, orphan)); !errors.Is(err, os.ErrNotExist) {
						t.Errorf("unreferenced resource %s was not collected: %v", orphan, err)
					}
				}
			}
			// A second owner, such as the Rust application started beside
			// this one, is refused by the shared advisory lock.
			var busy *core.ProfileInUseError
			if _, err := node.Start(context.Background(), profile); !errors.As(err, &busy) {
				t.Errorf("second owner = %v", err)
			}
			if err := backend.Close(); err != nil {
				t.Fatalf("stop %d: %v", round, err)
			}
			assertRustStored(t, profile)
			added := map[string]int{}
			for _, name := range preserved(t, profile, before, true) {
				parts := strings.SplitN(name, "/", 3)
				added[strings.Join(parts[:min(len(parts), 2)], "/")]++
			}
			_, temporary := os.Stat(filepath.Join(profile, "tmp", "leftover.bin"))
			t.Logf("round %d: files added by the Go Node: %v; tmp leftover: %v", round, added, temporary)
		}
	})

	// An incompatible shared file refuses the start, or the one feature
	// that reads it, without changing any file.
	for name, damage := range map[string]struct {
		change func(t *testing.T, profile string)
		fault  string
	}{
		"truncated identity": {func(t *testing.T, profile string) {
			overwrite(t, filepath.Join(profile, "link.key"), []byte("truncated"), 0o600)
		}, "invalid identity file length; refusing identity replacement"},
		"shared identity": {func(t *testing.T, profile string) {
			if err := os.Chmod(filepath.Join(profile, "link.key"), 0o644); err != nil {
				t.Fatal(err)
			}
		}, "identity file permissions must exclude group and other users"},
		"linked lock": {func(t *testing.T, profile string) {
			lock := filepath.Join(profile, "node.lock")
			if err := os.Remove(lock); err != nil {
				t.Fatal(err)
			}
			if err := os.Symlink(filepath.Join(profile, "link.key"), lock); err != nil {
				t.Fatal(err)
			}
		}, "lock must be a regular file"},
		"controller profile": {func(t *testing.T, profile string) {
			overwrite(t, filepath.Join(profile, "peers.json"), []byte(`{"version":1,"peers":[]}`), 0o600)
		}, "a controller profile cannot be opened as a Node"},
		"shared profile directory": {func(t *testing.T, profile string) {
			if err := os.Chmod(filepath.Join(profile, "storage"), 0o755); err != nil {
				t.Fatal(err)
			}
		}, "data directory permissions must exclude group and other users"},
	} {
		t.Run(name, func(t *testing.T) {
			profile := rustProfile(t)
			damage.change(t, profile)
			before := snapshot(t, profile)
			backend, err := startDesktop(t, profile)
			if err == nil {
				_ = backend.Close()
				t.Fatal("the Node started")
			}
			if !strings.Contains(err.Error(), damage.fault) {
				t.Errorf("fault = %v, want %q", err, damage.fault)
			}
			added := preserved(t, profile, before, false)
			for _, file := range added {
				if !strings.HasPrefix(file, "logs/desktop/") {
					t.Errorf("refused start created %s", file)
				}
			}
		})
	}

	t.Run("unsupported preferences", func(t *testing.T) {
		profile := rustProfile(t)
		path := filepath.Join(profile, "desktop", "preferences.json")
		overwrite(t, path, []byte(strings.Replace(rustPreferences, `"version": 1`, `"version": 2`, 1)), 0o600)
		before := snapshot(t, profile)
		backend, err := startDesktop(t, profile)
		if err != nil {
			t.Fatalf("start: %v", err)
		}
		stored := backend.Preferences
		if stored.Readable || stored.Err == nil || !strings.Contains(stored.Err.Error(), "unsupported desktop preferences version") {
			t.Errorf("preferences = readable %v, %v", stored.Readable, stored.Err)
		}
		// Unreadable preferences are never overwritten with defaults.
		stored.Data.ToastSeconds = 3
		if err := stored.Save(); err == nil {
			t.Error("unreadable preferences were saved")
		}
		if err := stored.FinishStartup(); err == nil {
			t.Error("startup reported readable preferences")
		}
		if err := backend.Close(); err != nil {
			t.Fatal(err)
		}
		preserved(t, profile, before, true)
	})
}

func overwrite(t *testing.T, path string, body []byte, mode os.FileMode) {
	t.Helper()
	if err := os.WriteFile(path, body, mode); err != nil {
		t.Fatal(err)
	}
	if err := os.Chmod(path, mode); err != nil {
		t.Fatal(err)
	}
}
