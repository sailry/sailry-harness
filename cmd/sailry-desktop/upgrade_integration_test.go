//go:build integration && unix

package main

import (
	"context"
	"crypto/ed25519"
	"crypto/sha256"
	"encoding/hex"
	"errors"
	"io/fs"
	"os"
	"path"
	"path/filepath"
	"runtime"
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
	"github.com/sailry/sailry-harness/internal/node/store"
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
// the profile path. Database bodies are opaque: the Go Node never opens them.
func rustProfile(t *testing.T) string {
	t.Helper()
	profile := filepath.Join(t.TempDir(), ".sailry")
	seed := make([]byte, ed25519.SeedSize)
	for index := range seed {
		seed[index] = byte(index + 1)
	}
	database := func(name string) []byte {
		return append([]byte("SQLite format 3\x00"), []byte("turso "+name+" pages written by the Rust Node")...)
	}
	files := map[string][]byte{
		"node.lock": {},
		"link.key":  seed,
		// runtime-core database_files: the database, its log and both side files.
		"storage/node.turso":           database("node"),
		"storage/node.db-log":          database("node log"),
		"storage/node.turso-wal":       database("node wal"),
		"storage/node.turso-tshm":      database("node tshm"),
		"storage/knowledge.turso":      database("knowledge"),
		"storage/knowledge.db-log":     database("knowledge log"),
		"storage/knowledge.turso-wal":  database("knowledge wal"),
		"storage/knowledge.turso-tshm": database("knowledge tshm"),
		"storage/backups/node-before-upgrade-0197c0de-0000-7000-8000-00000000000a.turso": database("backup"),
		"desktop/preferences.json": []byte(rustPreferences),
		"plugins/packages/3f0c9a51d2e84b7796a5c4d3e2f1a0b9c8d7e6f5a4b3c2d1e0f9a8b7c6d5e4f3/plugin.json": []byte(`{"name":"dev.sailry.platform","version":"0.1.0"}`),
		"plugins/packages/3f0c9a51d2e84b7796a5c4d3e2f1a0b9c8d7e6f5a4b3c2d1e0f9a8b7c6d5e4f3/main.js":     []byte("export default {}\n"),
		"plugins/data/dev.sailry.platform/state.json":                                                   []byte(`{"count":1}`),
		"models/embedding/bge-small/files/model.onnx":                                                   []byte("onnx"),
		"models/decision/laya/model.gguf":                                                               []byte("gguf"),
		"models/ocr/ppocr/model.onnx":                                                                   []byte("onnx"),
		"models/sensevoice-int8/model.onnx":                                                             []byte("onnx"),
		"models/sensevoice-int8/tokens.txt":                                                             []byte("a\n"),
		"models/speech.json":                                                                            []byte(`{"version":1}`),
		"python/environments/0a1b2c/pyvenv.cfg":                                                         []byte("home = /bundle\n"),
		"python/environments/0a1b2c/receipt.json":                                                       []byte(`{"version":1}`),
		"python/cache/wheel.whl":                                                                        []byte("wheel"),
		"python/staging/leftover/file":                                                                  []byte("staged"),
		"worktrees/0197c0de-0000-7000-8000-000000000002/README.md":                                      []byte("managed worktree\n"),
		"workspaces/sessions/0197c0de-0000-7000-8000-000000000001/notes.txt":                            []byte("session files\n"),
		"assistants/9e107d9d372bb6826bd81d3542a419d6/workspace/draft.md":                                []byte("assistant files\n"),
		"artifacts/0197c0de-0000-7000-8000-000000000003/index.html":                                     []byte("<p>artifact</p>"),
		"cache/code-index/0197c0de-0000-7000-8000-000000000002/index.turso":                             database("code index"),
		"logs/desktop/sailry.2026-11-10.log":                                                            []byte("rust log line\n"),
		"logs/host/sailry.2026-11-10.log":                                                               []byte("rust host log line\n"),
		"tools/chromium/128.0/chrome":                                                                   []byte("browser"),
		"updater/receipt.json":                                                                          []byte(`{"version":"0.1.4"}`),
		"tmp/leftover.bin":                                                                              []byte("temporary"),
	}
	// An installed theme is theme.json and its declared resources, the files
	// the Rust installer writes from an official package.
	source := filepath.Join("..", "..", "themes", "ink-dragon")
	err := filepath.WalkDir(source, func(path string, entry fs.DirEntry, err error) error {
		if err != nil || entry.IsDir() {
			return err
		}
		relative, err := filepath.Rel(source, path)
		if err != nil {
			return err
		}
		if relative = filepath.ToSlash(relative); relative == "theme.json" || strings.HasPrefix(relative, "assets/") {
			files["desktop/themes/ink-dragon/"+relative], err = os.ReadFile(path)
		}
		return err
	})
	if err != nil {
		t.Fatal(err)
	}
	for name, body := range files {
		path := filepath.Join(profile, filepath.FromSlash(name))
		if err := os.MkdirAll(filepath.Dir(path), 0o700); err != nil {
			t.Fatal(err)
		}
		if err := os.WriteFile(path, body, 0o600); err != nil {
			t.Fatal(err)
		}
	}
	return profile
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

// preserved fails when a file of before is missing or changed and returns
// the files that were added. The Rust databases directly in storage may have
// moved to storage/rust-archive under the same names.
func preserved(t *testing.T, profile string, before map[string]string) []string {
	t.Helper()
	after := snapshot(t, profile)
	for name, state := range before {
		// A Node that opened its storage archived the database; a refused
		// start left it where it was.
		if archived := "storage/" + store.RustArchive + strings.TrimPrefix(name, "storage"); path.Dir(name) == "storage" && after[name] == "" {
			before[archived], name = state, archived
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

// startDesktop runs the Desktop backend start of openBackend with a local
// network scope, so the test needs no relay.
func startDesktop(t *testing.T, profile string) (*desktopBackend, error) {
	t.Helper()
	backend := &desktopBackend{Path: profile, Preferences: preferences.ForStartup(profile)}
	backend.logs, _ = logging.Install(profile, "desktop")
	options := node.Options{LocalModels: localmodels.Options{Capabilities: node.LocalModelCapabilities()}}
	library := "libsailry_iroh.so"
	if runtime.GOOS == "darwin" {
		library = "libsailry_iroh.dylib"
	}
	if path, err := filepath.Abs(filepath.Join("..", "..", ".runtime", "native", runtime.GOOS+"-"+runtime.GOARCH, library)); err == nil {
		if _, err := os.Stat(path); err == nil {
			t.Setenv("SAILRY_IROH_LIB", path)
			options.Network = &link.NetworkScope{}
		}
	}
	t.Logf("network: %v", options.Network != nil)
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
		for round := 1; round <= 2; round++ {
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
			preserved(t, profile, before)
			if _, err := os.Stat(filepath.Join(profile, "storage", store.RustArchive, "node.turso")); err != nil {
				t.Errorf("the Rust database was not archived: %v", err)
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
			added := map[string]int{}
			for _, name := range preserved(t, profile, before) {
				parts := strings.SplitN(name, "/", 3)
				added[strings.Join(parts[:min(len(parts), 2)], "/")]++
			}
			_, temporary := os.Stat(filepath.Join(profile, "tmp", "leftover.bin"))
			t.Logf("round %d: files added by the Go Node: %v; tmp leftover: %v", round, added, temporary)
			if _, err := os.Stat(filepath.Join(profile, "storage", "node.sqlite")); err != nil {
				t.Errorf("Go storage: %v", err)
			}
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
			added := preserved(t, profile, before)
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
		preserved(t, profile, before)
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
