package live

import (
	"bytes"
	"database/sql"
	"encoding/hex"
	"encoding/json"
	"errors"
	"image/png"
	"log"
	"log/slog"
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"

	"github.com/egoist/mygo"
	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/chrome"
	desktopplugins "github.com/sailry/sailry-harness/internal/desktop/plugins"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// TestBrowserSettings drives the browser package's settings services with a
// synthetic Chrome profile, a fixed password and a cookie store in memory:
// it reads no real profile and no Keychain.
func TestBrowserSettings(t *testing.T) {
	var logged bytes.Buffer
	previous, flags, output := slog.Default(), log.Flags(), log.Writer()
	slog.SetDefault(slog.New(slog.NewTextHandler(&logged, &slog.HandlerOptions{Level: slog.LevelDebug})))
	t.Cleanup(func() {
		slog.SetDefault(previous)
		log.SetFlags(flags)
		log.SetOutput(output)
	})

	// A Chrome profile directory with one decryptable cookie, one
	// partitioned, one expired and one of another cipher version.
	chromeRoot := t.TempDir()
	if err := os.MkdirAll(filepath.Join(chromeRoot, "Default"), 0o755); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(chromeRoot, "Local State"), []byte(`{"profile":{"info_cache":{"Default":{"name":"Fixture person"}}}}`), 0o600); err != nil {
		t.Fatal(err)
	}
	cipher, err := hex.DecodeString("763130cbe0d3934b1d9921743baffaf66d630303d3497336a024272f8327f166749f1af504654cc643d5543c05231db3432f4c")
	if err != nil {
		t.Fatal(err)
	}
	db, err := sql.Open("sqlite", filepath.Join(chromeRoot, "Default", "Cookies"))
	if err != nil {
		t.Fatal(err)
	}
	if _, err := db.Exec(`CREATE TABLE meta(key LONGVARCHAR, value LONGVARCHAR); INSERT INTO meta VALUES('version',24);
CREATE TABLE cookies(host_key TEXT,name TEXT,value TEXT,encrypted_value BLOB,path TEXT,is_secure INTEGER,is_httponly INTEGER,samesite INTEGER,expires_utc INTEGER,has_expires INTEGER,top_frame_site_key TEXT);
INSERT INTO cookies VALUES('.example.test','login','',?1,'/',1,1,1,0,0,'');
INSERT INTO cookies VALUES('.example.test','partitioned','',?1,'/',1,1,1,0,0,'https://top.test');
INSERT INTO cookies VALUES('.example.test','expired','',?1,'/',1,1,1,1,1,'');
INSERT INTO cookies VALUES('.example.test','other','',x'763131aa','/',1,1,1,0,0,'');`, cipher); err != nil {
		t.Fatal(err)
	}
	if err := db.Close(); err != nil {
		t.Fatal(err)
	}

	info := protocol.DefaultPluginInfo()
	info.Summary.Name, info.Summary.Digest, info.Summary.Enabled = "browser", strings.Repeat("d", 64), true
	info.Summary.Compatibility.Kind = "compatible"
	info.Extension = new(protocol.DefaultPluginExtension())
	info.Extension.Actions = []protocol.PluginAction{protocol.PluginActionReadBrowser, protocol.PluginActionControlBrowser}
	entry := "dev.sailry.platform/desktop/settings.js"
	if err := json.Unmarshal([]byte(`{"navigation":{"label":"Browser","locales":{"zh-CN":"浏览器"}},"entry":"`+entry+`"}`), &info.Extension.SettingsPage); err != nil {
		t.Fatal(err)
	}
	snapshot := protocol.DefaultSnapshot()
	snapshot.Node, snapshot.Plugins = protocol.NodeID{1}, []protocol.PluginSummary{info.Summary}
	source := &pluginViewTestNode{snapshot: snapshot, bundle: make(chan link.Response, 1)}
	root := pluginViewFixture(t, source)
	root.packages.infos[info.Summary.Name] = info
	scope := protocol.DefaultPluginContext()
	scope.Package, scope.Surface.Kind = packageReference(info.Summary), "settings"

	store := map[string]mygo.Cookie{}
	var password []byte
	sources := &browserSources{
		supported: true,
		profiles:  func() ([]chrome.Profile, error) { return chrome.Discover(chromeRoot) },
		password: func() ([]byte, error) {
			password = []byte("isolated-password")
			return password, nil
		},
		set: func(cookie mygo.Cookie) error {
			store[cookie.Domain+cookie.Path+cookie.Name] = cookie
			return nil
		},
		list: func() ([]mygo.Cookie, error) {
			cookies := make([]mygo.Cookie, 0, len(store))
			for _, cookie := range store {
				cookies = append(cookies, cookie)
			}
			return cookies, nil
		},
	}
	root.browserSources = sources

	// The headless window stands for the UI thread: each frame runs what
	// the services posted, then shows the settings page once it is opened.
	var page *pluginSettingsEntry
	tester := ui.NewTesterWith(func(c *ui.Context) {
		root.mu.Lock()
		queued := root.queue
		root.queue = nil
		root.mu.Unlock()
		for _, apply := range queued {
			apply(c)
		}
		if page != nil {
			root.renderPluginSettings(c, *page)
		}
	}, 900, 420, ui.TesterOptions{Preferences: ui.Preferences{ReduceTransparency: true}})
	tester.SetLocale("zh-CN")

	// call runs a host operation as a package callback does, off the UI
	// thread.
	call := func(t *testing.T, operations map[string]desktopplugins.HostOperation, name string, arguments ...any) (string, error) {
		t.Helper()
		encoded := make([]json.RawMessage, len(arguments))
		for index, argument := range arguments {
			encoded[index], _ = json.Marshal(argument)
		}
		type reply struct {
			value json.RawMessage
			err   error
		}
		done := make(chan reply, 1)
		go func() {
			value, err := operations["sailry/ui."+name](t.Context(), encoded)
			done <- reply{value, err}
		}()
		deadline := time.After(10 * time.Second)
		for {
			select {
			case result := <-done:
				return string(result.value), result.err
			case <-deadline:
				t.Fatalf("%s did not answer", name)
			case <-time.After(time.Millisecond):
				tester.Frame()
			}
		}
	}

	t.Run("answers the settings page as the reference host does", func(t *testing.T) {
		operations := root.pluginNativeBinding(t.Context(), scope, "").Operations
		if value, err := call(t, operations, "readBrowserSettings"); err != nil || value != `{"enabled":true,"persistent":true,"supported":true}` {
			t.Fatalf("default settings: %s, %v", value, err)
		}
		if value, err := call(t, operations, "setBrowserPersistent", false); err != nil || value != "null" ||
			root.Preferences.Data.BrowserPersistent == nil || *root.Preferences.Data.BrowserPersistent {
			t.Fatalf("persistent was not stored: %s, %v", value, err)
		}
		if value, _ := call(t, operations, "readBrowserSettings"); value != `{"enabled":true,"persistent":false,"supported":true}` {
			t.Fatalf("stored settings: %s", value)
		}
		if _, err := call(t, operations, "importBrowserProfile", "unlisted"); !errors.Is(err, chrome.ErrMissing) {
			t.Fatalf("an unlisted profile ID: %v", err)
		}
		value, err := call(t, operations, "listBrowserProfiles")
		var profiles []struct{ ID, Name string }
		if err != nil || json.Unmarshal([]byte(value), &profiles) != nil || len(profiles) != 1 || profiles[0].Name != "Fixture person" ||
			strings.Contains(value, chromeRoot) {
			t.Fatalf("profiles: %s, %v", value, err)
		}
		before, _ := os.ReadFile(filepath.Join(chromeRoot, "Default", "Cookies"))
		if value, err := call(t, operations, "importBrowserProfile", profiles[0].ID); err != nil || value != `{"count":1,"skipped":3}` {
			t.Fatalf("import: %s, %v", value, err)
		}
		after, _ := os.ReadFile(filepath.Join(chromeRoot, "Default", "Cookies"))
		installed := store[".example.test/login"]
		if len(store) != 1 || installed.Value != "fixture-login" || !installed.Secure || !installed.HTTPOnly ||
			installed.SameSite != mygo.CookieSameSiteLax || !installed.Expires.IsZero() || !bytes.Equal(before, after) {
			t.Fatalf("installed cookies %+v, database changed: %t", store, !bytes.Equal(before, after))
		}
		if bytes.ContainsFunc(password, func(r rune) bool { return r != 0 }) {
			t.Fatal("the Chrome password was not overwritten")
		}

		// A store that kept nothing: every cookie counts as skipped.
		clear(store)
		sources.set = func(mygo.Cookie) error { return errors.New("refused") }
		if value, err := call(t, operations, "importBrowserProfile", profiles[0].ID); err != nil || value != `{"count":0,"skipped":4}` {
			t.Fatalf("refused cookies: %s, %v", value, err)
		}
		sources.list = func() ([]mygo.Cookie, error) { return nil, errors.New("store") }
		if _, err := call(t, operations, "importBrowserProfile", profiles[0].ID); !errors.Is(err, errBrowserImportFailed) {
			t.Fatalf("an unreadable store: %v", err)
		}
		// The user refusing the Keychain prompt is silent; a denied key is not.
		sources.password = func() ([]byte, error) { return nil, errBrowserImportCancelled }
		if value, err := call(t, operations, "importBrowserProfile", profiles[0].ID); err != nil || value != "null" {
			t.Fatalf("a refused Keychain prompt: %s, %v", value, err)
		}
		sources.password = func() ([]byte, error) { return nil, chrome.ErrKeyDenied }
		if _, err := call(t, operations, "importBrowserProfile", profiles[0].ID); !errors.Is(err, chrome.ErrKeyDenied) {
			t.Fatalf("a denied key: %v", err)
		}

		// Another system: the page reads its state and nothing else works.
		sources.supported = false
		if value, _ := call(t, operations, "readBrowserSettings"); value != `{"enabled":true,"persistent":false,"supported":false}` {
			t.Fatalf("unsupported settings: %s", value)
		}
		for name, arguments := range map[string][]any{"setBrowserPersistent": {true}, "listBrowserProfiles": nil, "importBrowserProfile": {profiles[0].ID}} {
			if _, err := call(t, operations, name, arguments...); !errors.Is(err, errBrowserImportUnavailable) {
				t.Fatalf("%s on an unsupported system: %v", name, err)
			}
		}
		sources.supported = true

		// Only the settings view of a package with the browser actions.
		workspace := scope
		workspace.Surface.Kind = "workspace"
		outside := root.pluginNativeBinding(t.Context(), workspace, "").Operations
		for name, arguments := range map[string][]any{"readBrowserSettings": nil, "setBrowserPersistent": {true}, "listBrowserProfiles": nil, "importBrowserProfile": {profiles[0].ID}} {
			if _, err := call(t, outside, name, arguments...); err == nil || !strings.Contains(err.Error(), "browser capability is unavailable in this view") {
				t.Fatalf("%s outside settings: %v", name, err)
			}
		}
		if text := logged.String(); text != "" {
			t.Fatalf("the import logged: %s", text)
		}
	})

	t.Run("renders the package page in Chinese", func(t *testing.T) {
		value := true
		root.Preferences.Data.BrowserPersistent = &value
		files := map[string]string{}
		for _, name := range []string{"settings.js", "locales.js"} {
			body, err := os.ReadFile(filepath.Join("..", "..", "..", "plugins", "browser", "dev.sailry.platform", "desktop", name))
			if err != nil {
				t.Fatal(err)
			}
			files["dev.sailry.platform/desktop/"+name] = string(body)
		}
		source.bundle <- link.Response{Output: protocol.Output{Kind: "plugin_view", Data: protocol.PluginDesktopBundle{
			Package: scope.Package, Entry: entry, Files: files,
		}}}
		root.pluginViews.locale = "zh-CN"
		page = &pluginSettingsEntry{section: "plugin/browser", owner: root, info: info, scope: scope}
		root.pluginViews.settings[page.section] = *page
		deadline := time.Now().Add(20 * time.Second)
		for !tester.HasText("从 Chrome 导入") {
			if time.Now().After(deadline) {
				t.Fatalf("the page did not show its import control; visible text: %q", tester.Texts())
			}
			time.Sleep(5 * time.Millisecond)
			tester.Advance(16 * time.Millisecond)
			tester.Frame()
		}
		retain, exists := tester.AccessByRole(ui.RoleSwitch, "")
		if !exists || retain.Disabled {
			t.Fatalf("the persistence switch is missing or disabled; visible text: %q", tester.Texts())
		}
		for _, text := range tester.Texts() {
			if strings.Contains(text, "暂不支持") {
				t.Fatalf("the page reports an unsupported system: %q", tester.Texts())
			}
		}
		shots := filepath.Join("..", "..", "..", ".runtime", "ui-shots")
		if err := os.MkdirAll(shots, 0o755); err != nil {
			t.Fatal(err)
		}
		file, err := os.Create(filepath.Join(shots, "browser-settings-zh-CN.png"))
		if err != nil {
			t.Fatal(err)
		}
		defer file.Close()
		if err := png.Encode(file, tester.Image()); err != nil {
			t.Fatal(err)
		}
	})
}
