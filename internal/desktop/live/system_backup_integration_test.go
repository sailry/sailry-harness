//go:build integration

package live

import (
	"bufio"
	"bytes"
	"context"
	"encoding/xml"
	"fmt"
	"io"
	"io/fs"
	"net/http"
	"net/http/httptest"
	"os"
	"path/filepath"
	"slices"
	"strconv"
	"strings"
	"sync"
	"testing"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/jonboulle/clockwork"
	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/preferences"
	"github.com/sailry/sailry-harness/internal/desktop/settings"
	"github.com/sailry/sailry-harness/internal/desktop/shell"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/node"
	"github.com/sailry/sailry-harness/internal/protocol"
	"golang.org/x/net/webdav"
)

const (
	backupUIIdentity   = "backup-user"
	backupUISecret     = "backup-secret-value"
	backupUIToken      = "backup-session-token"
	backupUIPassphrase = "correct horse battery"
	backupUISealed     = "age-encryption.org/v1"
)

// backupUIWebDAV is the in-process WebDAV server of
// internal/node/backup/remote_integration_test.go, copied because that
// fixture is a test file of another package. dropped counts the requests
// that carried no or wrong credentials.
func backupUIWebDAV(t *testing.T) (*httptest.Server, string) {
	t.Helper()
	directory := t.TempDir()
	handler := &webdav.Handler{FileSystem: webdav.Dir(directory), LockSystem: webdav.NewMemLS()}
	server := httptest.NewServer(http.HandlerFunc(func(writer http.ResponseWriter, request *http.Request) {
		identity, secret, ok := request.BasicAuth()
		if !ok || identity != backupUIIdentity || secret != backupUISecret {
			writer.Header().Set("WWW-Authenticate", `Basic realm="backup"`)
			http.Error(writer, "unauthorized", http.StatusUnauthorized)
			return
		}
		handler.ServeHTTP(writer, request)
	}))
	t.Cleanup(server.Close)
	return server, directory
}

// backupUIS3 is the minimal in-process S3-compatible handler of the same
// test, copied likewise: path-style PUT, GET, HEAD and DELETE of objects and
// ListObjectsV2 with a delimiter. It checks that requests carry a SigV4
// authorization naming the access key and, when one is expected, the
// session token; it does not verify signatures.
type backupUIS3 struct {
	mu      sync.Mutex
	bucket  string
	token   string
	objects map[string][]byte
}

func (store *backupUIS3) ServeHTTP(writer http.ResponseWriter, request *http.Request) {
	if !strings.HasPrefix(request.Header.Get("Authorization"), "AWS4-HMAC-SHA256 Credential="+backupUIIdentity+"/") ||
		request.Header.Get("X-Amz-Security-Token") != store.token {
		http.Error(writer, "<Error><Code>AccessDenied</Code></Error>", http.StatusForbidden)
		return
	}
	bucket, key, _ := strings.Cut(strings.TrimPrefix(request.URL.Path, "/"), "/")
	if bucket != store.bucket {
		http.Error(writer, "<Error><Code>NoSuchBucket</Code></Error>", http.StatusNotFound)
		return
	}
	store.mu.Lock()
	defer store.mu.Unlock()
	switch {
	case request.Method == http.MethodGet && key == "":
		store.list(writer, request)
	case request.Method == http.MethodPut:
		body, err := backupUIS3Body(request)
		if err != nil {
			http.Error(writer, "<Error><Code>IncompleteBody</Code></Error>", http.StatusBadRequest)
			return
		}
		store.objects[key] = body
		writer.Header().Set("ETag", `"fixture"`)
	case request.Method == http.MethodDelete:
		delete(store.objects, key)
		writer.WriteHeader(http.StatusNoContent)
	case request.Method == http.MethodGet || request.Method == http.MethodHead:
		body, ok := store.objects[key]
		if !ok {
			http.Error(writer, "<Error><Code>NoSuchKey</Code></Error>", http.StatusNotFound)
			return
		}
		writer.Header().Set("ETag", `"fixture"`)
		http.ServeContent(writer, request, "", time.Unix(1_700_000_000, 0), bytes.NewReader(body))
	default:
		http.Error(writer, "<Error><Code>MethodNotAllowed</Code></Error>", http.StatusMethodNotAllowed)
	}
}

func (store *backupUIS3) list(writer http.ResponseWriter, request *http.Request) {
	type content struct {
		Key          string
		Size         int
		LastModified string
		ETag         string
	}
	type prefix struct{ Prefix string }
	result := struct {
		XMLName        xml.Name `xml:"ListBucketResult"`
		Name           string
		Prefix         string
		Delimiter      string
		IsTruncated    bool
		Contents       []content
		CommonPrefixes []prefix
	}{Name: store.bucket, Prefix: request.URL.Query().Get("prefix"), Delimiter: request.URL.Query().Get("delimiter")}
	keys := make([]string, 0, len(store.objects))
	for key := range store.objects {
		keys = append(keys, key)
	}
	slices.Sort(keys)
	for _, key := range keys {
		rest, ok := strings.CutPrefix(key, result.Prefix)
		if !ok {
			continue
		}
		if index := strings.Index(rest, result.Delimiter); result.Delimiter != "" && index >= 0 {
			common := prefix{Prefix: result.Prefix + rest[:index+1]}
			if !slices.Contains(result.CommonPrefixes, common) {
				result.CommonPrefixes = append(result.CommonPrefixes, common)
			}
			continue
		}
		result.Contents = append(result.Contents, content{Key: key, Size: len(store.objects[key]), LastModified: "2023-11-14T22:13:20.000Z", ETag: `"fixture"`})
	}
	writer.Header().Set("Content-Type", "application/xml")
	_ = xml.NewEncoder(writer).Encode(result)
}

// backupUIS3Body reads a plain or aws-chunked request body.
func backupUIS3Body(request *http.Request) ([]byte, error) {
	if !strings.HasPrefix(request.Header.Get("X-Amz-Content-Sha256"), "STREAMING-") {
		return io.ReadAll(request.Body)
	}
	reader := bufio.NewReader(request.Body)
	var body []byte
	for {
		line, err := reader.ReadString('\n')
		if err != nil {
			return nil, err
		}
		field, _, _ := strings.Cut(strings.TrimSpace(line), ";")
		size, err := strconv.ParseInt(field, 16, 64)
		if err != nil {
			return nil, err
		}
		if size == 0 {
			_, _ = io.Copy(io.Discard, reader)
			return body, nil
		}
		chunk := make([]byte, size+2)
		if _, err := io.ReadFull(reader, chunk); err != nil {
			return nil, err
		}
		body = append(body, chunk[:size]...)
	}
}

// backupUIProfile is what a restored profile must hold again.
type backupUIProfile struct {
	node      protocol.NodeID
	sessions  []string
	projects  []string
	providers []string
	local     string
	schedule  protocol.BackupSchedule
	sealed    bool
}

func backupUIState(t *testing.T, ctx context.Context, controller *client.Client) backupUIProfile {
	t.Helper()
	snapshot := controller.Snapshot()
	state := backupUIProfile{node: snapshot.Node}
	for _, session := range snapshot.Sessions {
		state.sessions = append(state.sessions, fmt.Sprintf("%s %q archived=%v", session.ID, session.Activity.Title, session.Archived))
	}
	for _, project := range snapshot.Projects {
		state.projects = append(state.projects, fmt.Sprintf("%s %q %s", project.ID, project.Name, project.Path))
	}
	for _, provider := range snapshot.Providers {
		state.providers = append(state.providers, fmt.Sprintf("%s %q %s", provider.ID, provider.Name, provider.Endpoint))
	}
	slices.Sort(state.sessions)
	slices.Sort(state.projects)
	slices.Sort(state.providers)
	output, err := controller.Do(ctx, protocol.Command{Kind: "read_backup_settings"})
	if err != nil {
		t.Fatal(err)
	}
	saved, valid := detail[protocol.BackupSettings](output.Data)
	if !valid {
		t.Fatalf("backup settings were not returned: %+v", output)
	}
	if saved.Local != nil {
		state.local = saved.Local.Directory
	}
	if saved.Schedule != nil {
		state.schedule = *saved.Schedule
	}
	state.sealed = saved.PassphraseConfigured
	return state
}

func (state backupUIProfile) String() string {
	return fmt.Sprintf("node %s sessions %v projects %v providers %v local %q schedule %+v passphrase %v", state.node, state.sessions, state.projects, state.providers, state.local, state.schedule, state.sealed)
}

// backupUIRestored starts a second real Node on a restored profile and
// fails unless it holds what the source held when the backup was made.
func backupUIRestored(t *testing.T, ctx context.Context, path string, expected backupUIProfile) {
	t.Helper()
	restored, err := node.Start(ctx, path)
	if err != nil {
		t.Fatalf("the restored profile %s does not start: %v", path, err)
	}
	defer restored.Stop(context.Background())
	controller := client.New(link.NewLocal(restored.ID(), restored.ID(), restored))
	if err := controller.Start(ctx); err != nil {
		t.Fatal(err)
	}
	defer controller.Close()
	if got := backupUIState(t, ctx, controller); got.String() != expected.String() {
		t.Fatalf("the restored profile differs:\n got %s\nwant %s", got, expected)
	}
}

// backupUISealedFiles are the files under root that begin with the age
// header; it fails when any file under root holds one of the plain markers.
func backupUISealedFiles(t *testing.T, root string, plain ...string) (sealed, files int) {
	t.Helper()
	err := filepath.WalkDir(root, func(path string, entry fs.DirEntry, err error) error {
		if err != nil || entry.IsDir() {
			return err
		}
		body, err := os.ReadFile(path)
		if err != nil {
			return err
		}
		files++
		if bytes.HasPrefix(body, []byte(backupUISealed)) {
			sealed++
		}
		for _, marker := range plain {
			if bytes.Contains(body, []byte(marker)) {
				t.Fatalf("%s holds %q in the clear", path, marker)
			}
		}
		return nil
	})
	if err != nil {
		t.Fatal(err)
	}
	return sealed, files
}

// TestSystemBackupUI is the B13 acceptance of Settings › Backup against a
// real Node: SBAK-001 to SBAK-021.
//
// Substitutions: the native save panel and file picker are the Runtime's own
// seams (NativeFileActions.SaveFile and PickFiles) answered by the test with
// paths under t.TempDir(); the backup folder is chosen in the in-app picker,
// which reads the real Node's directories; the WebDAV
// and S3 targets are the in-process fakes above; the Node's dispatch clock
// is a fake clock so the scheduled run can be reached.
func TestSystemBackupUI(t *testing.T) {
	ctx, cancel := context.WithTimeout(t.Context(), 600*time.Second)
	defer cancel()
	root, err := filepath.EvalSymlinks(t.TempDir())
	if err != nil {
		t.Fatal(err)
	}
	profile := filepath.Join(root, "profile")
	clock := clockwork.NewFakeClockAt(time.Date(2026, 3, 7, 12, 0, 0, 0, time.UTC))
	desk := newSystemDesk(t, ctx, profile, node.Options{DispatchClock: clock})
	project := desk.project("Backup workspace")
	provider, model := desk.provider("http://127.0.0.1:9/v1")
	alpha := desk.session(project, provider, model, "Alpha notes")
	beta := desk.session(project, provider, model, "Beta notes")
	gamma := desk.session(project, provider, model, "Gamma notes")
	desk.do(protocol.Command{Kind: "set_session_archived", Data: protocol.CommandSetSessionArchivedPayload{Session: beta.ID, ExpectedRevision: beta.Revision, Archived: true}})

	saved := func() protocol.BackupSettings {
		t.Helper()
		value, valid := detail[protocol.BackupSettings](desk.do(protocol.Command{Kind: "read_backup_settings"}).Data)
		if !valid {
			t.Fatal("backup settings were not returned")
		}
		return value
	}
	backups := func() protocol.BackupPage {
		t.Helper()
		page, valid := detail[protocol.BackupPage](desk.do(protocol.Command{Kind: "list_backups"}).Data)
		if !valid {
			t.Fatal("backups were not returned")
		}
		return page
	}

	var window *terminalUI
	var saveTo, saveName, pickFile string
	var saves, opens int
	open := func() {
		window = openTerminalWindow(t, ctx, desk.owner, profile, alpha, nil)
		window.runtime.SetNativeFileActions(NativeFileActions{
			SaveFile: func(name string, selected func(string)) {
				saves++
				saveName = name
				selected(saveTo)
			},
			PickFiles: func(selected func([]string)) {
				opens++
				selected([]string{pickFile})
			},
		})
		showSection(window, settings.Backup)
		settle(window, "the backup page reads the Node", systemUIWait, func() bool {
			return window.runtime.backupSettings.known && !window.runtime.App.Settings.Backup.Pending
		})
	}
	open()
	defer func() { window.close() }()
	idle := func(label string) {
		t.Helper()
		settle(window, label, systemUIWait, func() bool {
			state := &window.runtime.backupSettings
			return state.known && !state.busy && !state.reading && !state.stale && !window.runtime.App.Settings.Backup.Pending
		})
	}
	dialog := func(text string) bool { return shown(window, text) > 0 }
	configure := func() {
		t.Helper()
		idle("the page before Configure")
		clickRow(window, "Backup storage", "Configure")
		settle(window, "the storage dialog", systemUIWait, func() bool { return dialog("Encryption passphrase") })
	}
	save := func(expected string) {
		t.Helper()
		idle("the page before Save")
		footerAction(window, "Cancel", "Save")
		toast(window, expected)
	}
	backUp := func(expected string) {
		t.Helper()
		idle("the page before Back up now")
		clickRow(window, "Back up now", "Back up now")
		toast(window, expected)
		idle("the page after Back up now")
	}
	restore := func(expected string) {
		t.Helper()
		idle("the page before Restore")
		footerAction(window, "Cancel", "Restore")
		toast(window, expected)
		idle("the page after Restore")
	}

	// (1) SBAK-001, SBAK-013, SBAK-014, SBAK-016: the page of a Node without
	// a target or passphrase.
	for _, text := range []string{"Backup storage", "Automatic backup", "Export data", "Back up now", "Restore from file", "Restore from storage", "Archived sessions", "Archive all sessions", "Delete all sessions"} {
		if shown(window, text) == 0 {
			t.Fatalf("the page does not show %q; visible text: %q", text, window.tester.Texts())
		}
	}
	if configures := window.buttons("Configure"); len(configures) != 3 {
		t.Fatalf("an unconfigured Node offers %d Configure actions, want storage, export and back up now; elements: %s", len(configures), window.elements())
	}
	if action, found := rowAction(window, "Restore from storage", "Restore"); !found || !action.Disabled {
		t.Fatalf("Restore from storage without a target: %+v found %v", action, found)
	}
	if toggle, found := toggled(window, "Automatic backup"); !found || toggle.Checked || shown(window, "Daily time") != 0 {
		t.Fatalf("the schedule of a new profile: %+v found %v", toggle, found)
	}
	if before := saved(); before.PassphraseConfigured || before.Local != nil || before.Destination != nil {
		t.Fatalf("a new profile's backup settings: %+v", before)
	}
	clickRow(window, "Export data", "Configure")
	settle(window, "Export without a passphrase opens the storage dialog", systemUIWait, func() bool { return dialog("Encryption passphrase") })
	press(window, "Cancel")
	clickRow(window, "Back up now", "Configure")
	settle(window, "Back up now without a target opens the storage dialog", systemUIWait, func() bool { return dialog("Encryption passphrase") })
	press(window, "Cancel")
	if saves != 0 || len(backups().Records) != 0 {
		t.Fatalf("an unconfigured Node exported: %d save panels, %+v", saves, backups())
	}
	t.Log("step 1 passed: the page loaded from the Node; without a passphrase Export and Back up now open Configure and Restore from storage is disabled")

	// (2) SBAK-003, SBAK-007, SBAK-008, SBAK-009: a local folder target.
	// choose picks an existing directory in the in-app picker: the path typed
	// into its address, then its confirmation.
	choose := func(path string) {
		t.Helper()
		if err := os.MkdirAll(path, 0o755); err != nil {
			t.Fatal(err)
		}
		if path, err = filepath.EvalSymlinks(path); err != nil {
			t.Fatal(err)
		}
		address, confirm := i18n.In("en", "directory_address"), i18n.In("en", "directory_title")
		shown := window.runtime.App.Sidebar.Picker
		pressIn(window, "Choose folder")
		settle(window, "the directory picker lists a directory", systemUIWait, func() bool {
			return shown.Showing() && len(window.named(ui.RoleButton, address)) == 1
		})
		window.clickOn(window.named(ui.RoleButton, address)[0])
		window.frame()
		window.tester.Key(ui.Cmd, ui.KeyA)
		window.tester.Type(path)
		window.tester.Key(0, ui.KeyEnter)
		settle(window, "the picker shows the typed directory", systemUIWait, func() bool {
			return len(window.named(ui.RoleButton, address)) == 1 && window.tester.HasText(path)
		})
		found := window.named(ui.RoleButton, confirm)
		if len(found) == 0 {
			t.Fatalf("the picker has no %q; elements: %s", confirm, window.elements())
		}
		window.clickOn(found[len(found)-1])
		settle(window, "the picker closes", systemUIWait, func() bool { return !shown.Showing() })
	}
	inside := filepath.Join(profile, "backups-inside")
	folder := filepath.Join(root, "backups")
	configure()
	press(window, "Local")
	settle(window, "the local folder field", systemUIWait, func() bool { return len(window.named(ui.RoleTextField, "Node backup folder")) == 1 })
	footerAction(window, "Cancel", "Save")
	toast(window, "Check the backup configuration")
	choose(inside)
	settle(window, "Choose folder fills the path", systemUIWait, func() bool { return labelled(window, "Node backup folder").Value == inside })
	footerAction(window, "Cancel", "Save")
	toast(window, "Enter a backup passphrase")
	fill(window, "Encryption passphrase", "short", true)
	fill(window, "Confirm passphrase", "short", true)
	footerAction(window, "Cancel", "Save")
	toast(window, "Use at least 8 characters")
	fill(window, "Encryption passphrase", backupUIPassphrase, true)
	fill(window, "Confirm passphrase", "another passphrase", true)
	footerAction(window, "Cancel", "Save")
	toast(window, "Passphrases do not match")
	if value := labelled(window, "Node backup folder").Value; value != inside || saved().Revision != 0 {
		t.Fatalf("refused drafts changed the dialog or the Node: folder %q, Node %+v", value, saved())
	}
	fill(window, "Confirm passphrase", backupUIPassphrase, true)
	// A folder inside the live profile is refused on save: the Node stores
	// nothing and the dialog keeps its draft.
	save("Backup access denied")
	if value := labelled(window, "Node backup folder").Value; !dialog("Encryption passphrase") || value != inside {
		t.Fatalf("the refused save closed the dialog or lost its folder: %q; elements: %s", value, window.elements())
	}
	if now := saved(); now.Revision != 0 || now.Local != nil || now.PassphraseConfigured {
		t.Fatalf("the Node stored a target inside its profile: %+v", now)
	}
	if left, err := os.ReadDir(inside); err != nil || len(left) != 0 {
		t.Fatalf("the refused save wrote into %s: %v %v", inside, left, err)
	}
	// The kept draft, with a folder outside the profile, saves.
	earlier := filepath.Join(root, "backups-earlier")
	choose(earlier)
	settle(window, "Choose folder replaces the refused path", systemUIWait, func() bool { return labelled(window, "Node backup folder").Value == earlier })
	save("Settings saved")
	settle(window, "the saved storage dialog closes", systemUIWait, func() bool { return !dialog("Encryption passphrase") })
	if now := saved(); now.Local == nil || now.Local.Directory != earlier || !now.Local.Enabled || now.OnlineEnabled || !now.PassphraseConfigured || now.CredentialsConfigured {
		t.Fatalf("the Node after saving a local target: %+v", now)
	}
	// SBAK-002: the saved target with its secrets empty; a blank passphrase
	// keeps the saved one.
	configure()
	if value := labelled(window, "Node backup folder").Value; value != earlier {
		t.Fatalf("the dialog shows the folder %q, want the saved %q", value, earlier)
	}
	for _, label := range []string{"Encryption passphrase", "Confirm passphrase"} {
		if field := labelled(window, label); field.Value != "" || field.Placeholder != "Keep existing" {
			t.Fatalf("the saved %s shows as %+v", label, field)
		}
	}
	if leaked(window, backupUIPassphrase) {
		t.Fatalf("the saved passphrase shows; elements: %s", window.elements())
	}
	choose(folder)
	settle(window, "Choose folder replaces the path", systemUIWait, func() bool { return labelled(window, "Node backup folder").Value == folder })
	save("Settings saved")
	if now := saved(); now.Local == nil || now.Local.Directory != folder || !now.PassphraseConfigured {
		t.Fatalf("the Node after replacing the folder: %+v", now)
	}
	t.Log("step 2 passed: the storage dialog refused an empty folder, a missing, short and mismatched passphrase with their toasts and kept the draft; Choose folder filled the path; a folder inside the live profile was refused on save with Backup access denied, the Node stored nothing and the draft stayed; the same draft with a folder outside the profile saved the local target; saved secrets show only as Keep existing and a blank passphrase keeps the saved one")

	// (3) SBAK-001, SBAK-014, SBAK-016: Back up now, pending state and the
	// stored backups.
	expected := backupUIState(t, ctx, desk.setup)
	release := window.gate.hold("export_backup")
	idle("the page before Back up now")
	clickRow(window, "Back up now", "Back up now")
	settle(window, "the running backup disables the page", systemUIWait, func() bool {
		if !window.runtime.App.Settings.Backup.Pending {
			return false
		}
		for anchor, name := range map[string]string{"Backup storage": "Configure", "Export data": "Export", "Back up now": "Back up now", "Restore from file": "Restore", "Restore from storage": "Restore", "Archived sessions": "Manage", "Archive all sessions": "Archive all"} {
			if action, found := rowAction(window, anchor, name); !found || !action.Disabled {
				return false
			}
		}
		return window.one(ui.RoleButton, "Delete all").Disabled
	})
	release()
	toast(window, "Backup exported")
	idle("the page after Back up now")
	page := backups()
	if len(page.Records) != 1 || page.Locations[page.Records[0].ID] != (protocol.BackupLocations{Local: true}) {
		t.Fatalf("the Node lists %+v after one local backup", page)
	}
	first := page.Records[0]
	if sealed, files := backupUISealedFiles(t, folder, "Alpha notes", "Backup workspace", "SQLite format 3", "fixture-provider-key"); sealed == 0 {
		t.Fatalf("no age-encrypted file among the %d files of %s", files, folder)
	}
	if _, err := os.Stat(filepath.Join(folder, first.ID+".json")); err != nil {
		t.Fatalf("the backup's marker is not in the folder: %v", err)
	}
	clickRow(window, "Restore from storage", "Restore")
	created := time.UnixMilli(int64(first.CreatedMs)).Local().Format("2006-01-02 15:04")
	settle(window, "the stored backup's row", systemUIWait, func() bool {
		return shown(window, created) == 1 && len(window.buttons("Local")) == 1 && len(window.buttons("Online backup")) == 0
	})
	t.Logf("step 3 passed: while the backup ran every action was disabled; Back up now wrote the encrypted backup %s into %s and Restore from storage lists it as %q with its Local copy", first.ID, folder, created)

	// (4) SBAK-016: restore from the stored record into a new profile.
	restoredRecord := filepath.Join(root, "restored-record")
	press(window, "Local")
	settle(window, "the restore form", systemUIWait, func() bool { return len(window.named(ui.RoleTextField, "New absolute profile path")) == 1 })
	if shown(window, "sailry-mac.local") != 1 || shown(window, "Restore to an empty directory. The copy retains this Node identity; run only one copy at a time") != 1 {
		t.Fatalf("the restore form does not name its host and consequence once; visible text: %q", window.tester.Texts())
	}
	if field := labelled(window, "Encryption passphrase"); field.Value != "" || field.Placeholder != "Keep existing" {
		t.Fatalf("the restore passphrase shows as %+v", field)
	}
	footerAction(window, "Cancel", "Restore")
	toast(window, "Enter a new absolute profile path")
	fill(window, "New absolute profile path", restoredRecord, false)
	fill(window, "Encryption passphrase", "a wrong passphrase", true)
	restore("Check the backup configuration")
	if value := labelled(window, "New absolute profile path").Value; value != restoredRecord {
		t.Fatalf("a refused restore dropped the form: %q", value)
	}
	if entries, _ := os.ReadDir(restoredRecord); len(entries) != 0 {
		t.Fatalf("a wrong passphrase left %d entries in %s", len(entries), restoredRecord)
	}
	fill(window, "Encryption passphrase", "", true)
	fill(window, "New absolute profile path", profile, false)
	restore("Backup settings or destination changed; reload to review")
	fill(window, "New absolute profile path", filepath.Join(profile, "restored-inside"), false)
	restore("Backup access denied")
	fill(window, "New absolute profile path", restoredRecord, false)
	restore("Profile restored")
	settle(window, "the restore dialogs close", systemUIWait, func() bool {
		return len(window.named(ui.RoleTextField, "New absolute profile path")) == 0 && shown(window, created) == 0
	})
	backupUIRestored(t, ctx, restoredRecord, expected)
	t.Logf("step 4 passed: the restore form names the host and the consequence; an empty path, a wrong passphrase, the live profile and a path inside it were refused with their toasts and kept the form; the stored backup restored into %s and a second Node on it holds %s", restoredRecord, expected)

	// (5) SBAK-010, SBAK-011, SBAK-012: the schedule is saved as it changes,
	// shows in a new Runtime and runs at its time.
	revision := saved().Revision
	toggle, _ := toggled(window, "Automatic backup")
	window.clickOn(toggle)
	settle(window, "the switch saves the schedule", systemUIWait, func() bool {
		now := saved()
		return now.Schedule != nil && now.Schedule.Enabled && now.Revision == revision+1
	})
	idle("the page after the switch")
	if now := saved(); now.Schedule.Timezone != "UTC" || now.Schedule.Hour != 3 || now.Schedule.Minute != 0 {
		t.Fatalf("the default schedule: %+v", now.Schedule)
	}
	noToast(window, "Settings saved")
	fill(window, "Time zone", "Asia/Shanghai", false)
	settle(window, "the rested timezone is saved", systemUIWait, func() bool { return saved().Schedule.Timezone == "Asia/Shanghai" })
	idle("the page after the timezone")
	if now := saved(); now.Revision != revision+2 {
		t.Fatalf("the timezone took %d saves", now.Revision-revision-1)
	}
	// The time input's segments are spin buttons: hours, minutes and the
	// half of the day, left to right.
	var segments []ui.AccessInfo
	for _, entry := range window.tester.AccessAll() {
		if strings.HasPrefix(entry.Name, "Daily time ") && entry.Focusable && entry.Bounds.W > 0 {
			segments = append(segments, entry)
		}
	}
	if len(segments) != 3 {
		t.Fatalf("the time input exposes %d segments; elements: %s", len(segments), window.elements())
	}
	slices.SortFunc(segments, func(a, b ui.AccessInfo) int { return int(a.Bounds.X - b.Bounds.X) })
	window.clickOn(segments[0])
	// Up steps the focused segment: 3 to 8 in the hours, 00 to 30 in the
	// minutes, AM to PM in the half of the day.
	for index, steps := range []int{5, 30, 1} {
		window.clickOn(segments[index])
		for range steps {
			window.tester.Key(0, ui.KeyUp)
			window.frame()
		}
	}
	if at := window.runtime.App.Settings.Backup.Draft.Time; at.Hour() != 20 || at.Minute() != 30 {
		t.Fatalf("the time input holds %s after typing 08:30 PM; visible text: %q", at.Format("15:04"), window.tester.Texts())
	}
	settle(window, "the rested time is saved", systemUIWait, func() bool {
		now := saved().Schedule
		return now.Hour == 20 && now.Minute == 30
	})
	idle("the page after the time")
	if now := saved(); now.Revision > revision+5 || !now.Schedule.Enabled || now.Schedule.Timezone != "Asia/Shanghai" {
		t.Fatalf("the Node after the time: %+v, %d saves since the timezone", now.Schedule, now.Revision-revision-2)
	}
	// SBAK-011: the time is changed again by typing its digits, 09:15 PM,
	// and that change is saved too.
	stepped := saved().Revision
	for index, digits := range [][]ui.Key{{ui.Key0, ui.Key9}, {ui.Key1, ui.Key5}} {
		window.clickOn(segments[index])
		for _, digit := range digits {
			window.tester.Key(0, digit)
			window.frame()
		}
	}
	if at := window.runtime.App.Settings.Backup.Draft.Time; at.Hour() != 21 || at.Minute() != 15 {
		t.Fatalf("the time input holds %s after typing 09:15; visible text: %q", at.Format("15:04"), window.tester.Texts())
	}
	settle(window, "the typed time is saved", systemUIWait, func() bool {
		now := saved().Schedule
		return now.Hour == 21 && now.Minute == 15
	})
	idle("the page after the typed time")
	if now := saved(); now.Revision == stepped || now.Revision > stepped+2 || !now.Schedule.Enabled || now.Schedule.Timezone != "Asia/Shanghai" {
		t.Fatalf("the Node after the typed time: %+v, %d saves", now.Schedule, now.Revision-stepped)
	}
	window.close()
	open()
	if toggle, found := toggled(window, "Automatic backup"); !found || !toggle.Checked || labelled(window, "Time zone").Value != "Asia/Shanghai" {
		t.Fatalf("a new Runtime shows the schedule as %+v, timezone %q", toggle, labelled(window, "Time zone").Value)
	}
	if at := window.runtime.App.Settings.Backup.Saved.Time; at.Hour() != 21 || at.Minute() != 15 {
		t.Fatalf("a new Runtime shows the time %s", at.Format("15:04"))
	}
	// 21:15 in Asia/Shanghai is 13:15 UTC; the Node's clock stands at 12:00.
	// The times saved before, 03:00 and 20:30 (12:30 UTC), no longer run:
	// the clock passes 12:30 in steps and nothing is backed up until 13:15.
	for range 74 {
		clock.Advance(time.Minute)
		time.Sleep(5 * time.Millisecond)
	}
	time.Sleep(200 * time.Millisecond)
	if page := backups(); len(page.Records) != 1 {
		t.Fatalf("backups at 13:14 UTC, before the saved time: %+v", page)
	}
	deadline := time.Now().Add(30 * time.Second)
	for len(backups().Records) != 2 {
		if time.Now().After(deadline) {
			t.Fatalf("no scheduled backup ran: %+v", backups())
		}
		// The dispatcher may be between timers; each step lets a new one expire.
		clock.Advance(time.Second)
		time.Sleep(20 * time.Millisecond)
	}
	if at := clock.Now().UTC(); at.Hour() != 13 || at.Minute() < 14 || at.Minute() > 16 {
		t.Fatalf("the scheduled backup ran at %s on the Node's clock", at.Format("15:04:05"))
	}
	t.Logf("step 5 passed: the switch saved the schedule at once and the timezone and time once they rested; the time stepped to 20:30 and then typed as 21:15 was saved each time; a new Runtime shows 21:15; the Node's clock passed 20:30 Asia/Shanghai without a backup and ran one at 21:15 (%s UTC)", clock.Now().UTC().Format("15:04:05"))

	// (6) SBAK-013: Export writes the sealed archive where the save panel
	// said.
	if err := os.MkdirAll(filepath.Join(root, "exports"), 0o700); err != nil {
		t.Fatal(err)
	}
	saveTo = filepath.Join(root, "exports", "chosen.age")
	expected = backupUIState(t, ctx, desk.setup)
	idle("the page before Export")
	clickRow(window, "Export data", "Export")
	toast(window, "Backup exported")
	idle("the page after Export")
	if saves != 1 || saveName != "sailry-backup.age" {
		t.Fatalf("the save panel ran %d times proposing %q", saves, saveName)
	}
	archive, err := os.ReadFile(saveTo)
	if err != nil || !bytes.HasPrefix(archive, []byte(backupUISealed)) {
		t.Fatalf("the exported file is not a sealed archive: %d bytes, %v", len(archive), err)
	}
	for _, marker := range []string{"Alpha notes", "Backup workspace", "SQLite format 3", "fixture-provider-key"} {
		if bytes.Contains(archive, []byte(marker)) {
			t.Fatalf("the exported archive holds %q in the clear", marker)
		}
	}
	if entries, _ := os.ReadDir(filepath.Dir(saveTo)); len(entries) != 1 {
		t.Fatalf("the export left %d files beside the archive", len(entries)-1)
	}
	t.Logf("step 6 passed: Export proposed sailry-backup.age and wrote a %d-byte age archive to the chosen path", len(archive))

	// (7) SBAK-015: restore from that file into a new profile.
	restoredFile := filepath.Join(root, "restored-file")
	pickFile = saveTo
	clickRow(window, "Restore from file", "Restore")
	settle(window, "the restore form of a chosen file", systemUIWait, func() bool { return len(window.named(ui.RoleTextField, "New absolute profile path")) == 1 })
	if opens != 1 {
		t.Fatalf("the file picker ran %d times", opens)
	}
	fill(window, "New absolute profile path", restoredFile, false)
	fill(window, "Encryption passphrase", "a wrong passphrase", true)
	restore("Check the backup configuration")
	if entries, _ := os.ReadDir(restoredFile); len(entries) != 0 {
		t.Fatalf("a wrong passphrase left %d entries in %s", len(entries), restoredFile)
	}
	fill(window, "Encryption passphrase", backupUIPassphrase, true)
	restore("Profile restored")
	settle(window, "the restore form closes", systemUIWait, func() bool { return len(window.named(ui.RoleTextField, "New absolute profile path")) == 0 })
	backupUIRestored(t, ctx, restoredFile, expected)
	t.Logf("step 7 passed: the chosen archive was refused with a wrong passphrase and restored with the typed one into %s; a second Node on it holds %s", restoredFile, expected)

	// (8) SBAK-009: a stale revision is a conflict and keeps the draft.
	second := filepath.Join(root, "backups-second")
	configure()
	choose(second)
	settle(window, "the drafted folder", systemUIWait, func() bool { return labelled(window, "Node backup folder").Value == second })
	stale := saved()
	stale.Schedule.Minute = 45
	desk.do(protocol.Command{Kind: "save_backup_settings", Data: protocol.BackupUpdate{Settings: stale, Identity: backupKeep(), Secret: backupKeep(), SessionToken: backupKeep(), Passphrase: backupKeep()}})
	footerAction(window, "Cancel", "Save")
	toast(window, "Backup settings or destination changed; reload to review")
	if value := labelled(window, "Node backup folder").Value; value != second || saved().Local.Directory != folder {
		t.Fatalf("the conflict lost the draft or saved it: dialog %q, Node %q", value, saved().Local.Directory)
	}
	noToast(window, "Settings saved")
	save("Settings saved")
	if now := saved(); now.Local.Directory != second || now.Schedule.Minute != 45 {
		t.Fatalf("the Node after the retried save: %+v %+v", now.Local, now.Schedule)
	}
	t.Log("step 8 passed: saving over a newer revision showed the conflict toast, kept the dialog and its draft and saved nothing; the next Save stored the draft over the reloaded settings")

	// (9) SBAK-003 to SBAK-006, SBAK-014, SBAK-016: WebDAV.
	dav, davDirectory := backupUIWebDAV(t)
	configure()
	press(window, "WebDAV")
	settle(window, "the WebDAV fields", systemUIWait, func() bool { return len(window.named(ui.RoleTextField, "Endpoint")) == 1 })
	if len(window.named(ui.RoleTextField, "Bucket")) != 0 || len(window.named(ui.RoleTextField, "Session token")) != 0 {
		t.Fatalf("WebDAV shows S3 fields; elements: %s", window.elements())
	}
	footerAction(window, "Cancel", "Save")
	toast(window, "Check the backup configuration")
	fill(window, "Endpoint", dav.URL, false)
	fill(window, "Prefix", "team/profile", false)
	fill(window, "Username or access key ID", backupUIIdentity, false)
	fill(window, "Password or secret key", backupUISecret, true)
	save("Settings saved")
	if now := saved(); now.Destination == nil || now.Destination.Kind != protocol.BackupKindWebdav || now.Destination.Endpoint != dav.URL || now.Destination.Root != "team/profile" ||
		!now.OnlineEnabled || !now.CredentialsConfigured || now.Local == nil || now.Local.Enabled {
		t.Fatalf("the Node after saving WebDAV: %+v", now)
	}
	configure()
	for _, label := range []string{"Username or access key ID", "Password or secret key"} {
		if field := labelled(window, label); field.Value != "" || field.Placeholder != "Keep existing" {
			t.Fatalf("the saved %s shows as %+v", label, field)
		}
	}
	if leaked(window, backupUISecret) || leaked(window, backupUIIdentity) || leaked(window, backupUIPassphrase) {
		t.Fatalf("saved credentials show; elements: %s", window.elements())
	}
	if len(window.buttons("Clear credentials")) != 1 {
		t.Fatalf("saved credentials offer no Clear credentials; elements: %s", window.elements())
	}
	press(window, "Cancel")
	expected = backupUIState(t, ctx, desk.setup)
	known := map[string]bool{}
	for _, record := range backups().Records {
		known[record.ID] = true
	}
	backUp("Backup exported")
	online := ""
	page = backups()
	for _, record := range page.Records {
		if !known[record.ID] {
			online = record.ID
		}
	}
	if online == "" || page.Locations[online] != (protocol.BackupLocations{Online: true}) {
		t.Fatalf("the Node lists %+v after a WebDAV backup", page)
	}
	if _, err := os.Stat(filepath.Join(davDirectory, "team", "profile", online+".json")); err != nil {
		t.Fatalf("the WebDAV server holds no marker of %s: %v", online, err)
	}
	if sealed, files := backupUISealedFiles(t, davDirectory, "Alpha notes", "Backup workspace", "SQLite format 3", "fixture-provider-key"); sealed == 0 {
		t.Fatalf("no age-encrypted file among the %d files on the WebDAV server", files)
	}
	restoredDAV := filepath.Join(root, "restored-webdav")
	clickRow(window, "Restore from storage", "Restore")
	settle(window, "the WebDAV backup's row", systemUIWait, func() bool { return len(window.buttons("Online backup")) == 1 && len(window.buttons("Local")) == 0 })
	press(window, "Online backup")
	settle(window, "the restore form", systemUIWait, func() bool { return len(window.named(ui.RoleTextField, "New absolute profile path")) == 1 })
	fill(window, "New absolute profile path", restoredDAV, false)
	restore("Profile restored")
	backupUIRestored(t, ctx, restoredDAV, expected)
	// A typed secret replaces the stored one; a blank one keeps it.
	configure()
	fill(window, "Password or secret key", "a-wrong-secret", true)
	save("Settings saved")
	backUp("Backup access denied")
	configure()
	fill(window, "Password or secret key", backupUISecret, true)
	save("Settings saved")
	configure()
	fill(window, "Prefix", "team/second", false)
	save("Settings saved")
	backUp("Backup exported")
	if entries, _ := os.ReadDir(filepath.Join(davDirectory, "team", "second")); len(entries) == 0 {
		t.Fatal("a backup with the kept credentials did not reach the new prefix")
	}
	configure()
	pressIn(window, "Clear credentials")
	settle(window, "cleared credentials drop their placeholder", systemUIWait, func() bool {
		return labelled(window, "Password or secret key").Placeholder == "" && len(window.buttons("Clear credentials")) == 0
	})
	save("Settings saved")
	if now := saved(); now.CredentialsConfigured || now.Destination == nil || now.Destination.Root != "team/second" || !now.PassphraseConfigured {
		t.Fatalf("the Node after Clear credentials: %+v", now)
	}
	backUp("Backup access denied")
	t.Logf("step 9 passed: WebDAV refused a missing endpoint, saved its fields and credentials, showed them only as Keep existing, backed up to the server, listed and restored the online copy into %s; a typed secret replaced the stored one, a blank one kept it, and Clear credentials removed them", restoredDAV)

	// (10) SBAK-003 to SBAK-006, SBAK-014, SBAK-016: S3.
	objects := &backupUIS3{bucket: "profile-backups", token: backupUIToken, objects: map[string][]byte{}}
	s3 := httptest.NewServer(objects)
	t.Cleanup(s3.Close)
	configure()
	press(window, "S3")
	settle(window, "the S3 fields", systemUIWait, func() bool {
		return len(window.named(ui.RoleTextField, "Bucket")) == 1 && len(window.named(ui.RoleTextField, "Region")) == 1 && len(window.named(ui.RoleTextField, "Session token")) == 1
	})
	fill(window, "Endpoint", s3.URL, false)
	fill(window, "Prefix", "team/profile", false)
	footerAction(window, "Cancel", "Save")
	toast(window, "Check the backup configuration")
	fill(window, "Bucket", objects.bucket, false)
	fill(window, "Region", "us-east-1", false)
	fill(window, "Username or access key ID", backupUIIdentity, false)
	fill(window, "Password or secret key", backupUISecret, true)
	fill(window, "Session token", backupUIToken, true)
	save("Settings saved")
	if now := saved(); now.Destination == nil || now.Destination.Kind != protocol.BackupKindS3 || now.Destination.Endpoint != s3.URL || now.Destination.Bucket != objects.bucket ||
		now.Destination.Region != "us-east-1" || now.Destination.Root != "team/profile" || !now.OnlineEnabled || !now.CredentialsConfigured {
		t.Fatalf("the Node after saving S3: %+v", now)
	}
	configure()
	for _, label := range []string{"Username or access key ID", "Password or secret key", "Session token"} {
		if field := labelled(window, label); field.Value != "" || field.Placeholder != "Keep existing" {
			t.Fatalf("the saved %s shows as %+v", label, field)
		}
	}
	if leaked(window, backupUISecret) || leaked(window, backupUIIdentity) || leaked(window, backupUIToken) {
		t.Fatalf("saved credentials show; elements: %s", window.elements())
	}
	press(window, "Cancel")
	expected = backupUIState(t, ctx, desk.setup)
	backUp("Backup exported")
	objects.mu.Lock()
	stored, markers := len(objects.objects), 0
	for key, body := range objects.objects {
		if strings.HasPrefix(key, "team/profile/") && strings.HasSuffix(key, ".json") {
			markers++
		}
		for _, marker := range []string{"Alpha notes", "Backup workspace", "SQLite format 3", "fixture-provider-key"} {
			if bytes.Contains(body, []byte(marker)) {
				t.Fatalf("the S3 object %s holds %q in the clear", key, marker)
			}
		}
	}
	objects.mu.Unlock()
	if markers != 1 || stored < 2 {
		t.Fatalf("the S3 fake holds %d objects and %d markers after one backup", stored, markers)
	}
	restoredS3 := filepath.Join(root, "restored-s3")
	clickRow(window, "Restore from storage", "Restore")
	settle(window, "the S3 backup's row", systemUIWait, func() bool { return len(window.buttons("Online backup")) == 1 })
	press(window, "Online backup")
	settle(window, "the restore form", systemUIWait, func() bool { return len(window.named(ui.RoleTextField, "New absolute profile path")) == 1 })
	fill(window, "New absolute profile path", restoredS3, false)
	restore("Profile restored")
	backupUIRestored(t, ctx, restoredS3, expected)
	configure()
	pressIn(window, "Clear credentials")
	save("Settings saved")
	if now := saved(); now.CredentialsConfigured || now.Destination == nil || now.Destination.Kind != protocol.BackupKindS3 {
		t.Fatalf("the Node after Clear credentials: %+v", now)
	}
	t.Logf("step 10 passed: S3 refused a missing bucket, saved its fields, credentials and session token, showed them only as Keep existing, backed up %d objects to the fake, restored the online copy into %s, and Clear credentials removed them", stored, restoredS3)

	// (11) SBAK-017 to SBAK-021: the sessions.
	archived := func(id protocol.SessionID) bool {
		session, found := desk.current(id)
		return found && session.Archived
	}
	clickRow(window, "Archived sessions", "Manage")
	settle(window, "the archived session's row", systemUIWait, func() bool {
		return shown(window, "Beta notes") == 1 && shown(window, "Alpha notes") == 0 && shown(window, "Gamma notes") == 0 && len(window.buttons("Unarchive")) == 1
	})
	clickRow(window, "Beta notes", "Unarchive")
	toast(window, "Session unarchived")
	settle(window, "the unarchived session leaves the list", systemUIWait, func() bool {
		return shown(window, "No archived sessions") == 1 && shown(window, "Beta notes") == 0 && !archived(beta.ID)
	})
	closeDialog(window)
	idle("the page before Archive all")
	clickRow(window, "Archive all sessions", "Archive all")
	toast(window, "Sessions archived")
	settle(window, "every session is archived", systemUIWait, func() bool {
		action, found := rowAction(window, "Archive all sessions", "Archive all")
		return archived(alpha.ID) && archived(beta.ID) && archived(gamma.ID) && found && action.Disabled
	})
	clickRow(window, "Archived sessions", "Manage")
	settle(window, "the archived sessions' rows", systemUIWait, func() bool {
		return shown(window, "Alpha notes") == 1 && shown(window, "Beta notes") == 1 && shown(window, "Gamma notes") == 1 && len(window.buttons("Delete")) == 3
	})
	clickRow(window, "Gamma notes", "Delete")
	settle(window, "the confirmation naming the session", systemUIWait, func() bool {
		return shown(window, "Gamma notes?") > 0 && shown(window, "Delete this session and its child sessions. This cannot be undone; project files and account settings are kept") == 1
	})
	if _, found := desk.current(gamma.ID); !found {
		t.Fatal("the session was deleted before its confirmation")
	}
	footerAction(window, "Cancel", "Delete")
	toast(window, "Sessions deleted")
	settle(window, "the deleted session leaves the list", systemUIWait, func() bool {
		_, found := desk.current(gamma.ID)
		return !found && shown(window, "Gamma notes") == 0 && len(window.buttons("Delete")) == 2
	})
	closeDialog(window)
	idle("the page before Delete all")
	press(window, "Delete all")
	settle(window, "the confirmation naming every session", systemUIWait, func() bool {
		return shown(window, "Delete all sessions?") > 0 && shown(window, "Delete all sessions, including archived sessions. This cannot be undone; project files and account settings are kept") == 1
	})
	if len(desk.setup.Snapshot().Sessions) != 2 {
		t.Fatalf("sessions before the confirmation: %d", len(desk.setup.Snapshot().Sessions))
	}
	footerAction(window, "Cancel", "Delete all")
	toast(window, "Sessions deleted")
	settle(window, "every session is deleted", systemUIWait, func() bool { return len(desk.setup.Snapshot().Sessions) == 0 })
	if len(desk.setup.Snapshot().Projects) != 1 || len(desk.setup.Snapshot().Providers) != 1 {
		t.Fatal("deleting the sessions removed a project or a provider")
	}
	// SBAK-021: the sidebar is empty at once and after a relaunch. The
	// relaunch stops the Node and starts another on the same profile, with
	// a new Client, Runtime and window.
	empty := func(label string, app *shell.App, texts []string) {
		t.Helper()
		// A window that opens without a session starts on an unsent new
		// conversation draft; it is no session and holds no title.
		conversations := 0
		for _, cv := range app.Data.Unassigned {
			if !strings.HasPrefix(cv.ID, "draft/") {
				conversations++
			}
		}
		for _, project := range app.Data.Projects {
			for _, cv := range project.Conversations {
				if !strings.HasPrefix(cv.ID, "draft/") {
					conversations++
				}
			}
		}
		if conversations != 0 || len(app.Data.Projects) != 1 || app.Data.Projects[0].Name != "Backup workspace" {
			t.Fatalf("%s: the sidebar holds %d conversations in %d projects", label, conversations, len(app.Data.Projects))
		}
		for _, title := range []string{"Alpha notes", "Beta notes", "Gamma notes"} {
			if slices.Contains(texts, title) {
				t.Fatalf("%s: the window still shows %q", label, title)
			}
		}
		if !slices.Contains(texts, "Backup workspace") {
			t.Fatalf("%s: the sidebar lost the project; visible text: %q", label, texts)
		}
	}
	settle(window, "the window's sidebar empties", systemUIWait, func() bool {
		return len(window.runtime.App.Data.Unassigned) == 0 && len(window.runtime.App.Data.Projects) == 1 && len(window.runtime.App.Data.Projects[0].Conversations) == 0
	})
	window.runtime.App.Navigate(shell.ConversationPage)
	window.frame()
	empty("after Delete all", window.runtime.App, window.tester.Texts())
	window.close()
	if err := desk.setup.Close(); err != nil {
		t.Fatal(err)
	}
	if err := desk.owner.Stop(ctx); err != nil {
		t.Fatalf("the Node did not stop for the relaunch: %v", err)
	}
	owner, err := node.StartWithOptions(ctx, profile, node.Options{})
	if err != nil {
		t.Fatalf("the profile does not start again: %v", err)
	}
	defer owner.Stop(context.Background())
	relaunched := client.New(link.NewLocal(owner.ID(), owner.ID(), owner))
	if err := relaunched.Start(ctx); err != nil {
		t.Fatal(err)
	}
	defer relaunched.Close()
	if snapshot := relaunched.Snapshot(); len(snapshot.Sessions) != 0 || len(snapshot.Projects) != 1 || len(snapshot.Providers) != 1 {
		t.Fatalf("the relaunched Node lists %d sessions, %d projects and %d providers", len(snapshot.Sessions), len(snapshot.Projects), len(snapshot.Providers))
	}
	again, err := NewWithPreferences(ctx, relaunched, preferences.Open(filepath.Join(profile, "desktop", "preferences.json")))
	if err != nil {
		t.Fatal(err)
	}
	defer again.Close()
	again.App.NavOpen = true
	tester := ui.NewTester(again.App.View, 1440, 1000)
	tester.SetLocale("en")
	for deadline := time.Now().Add(systemUIWait); ; time.Sleep(10 * time.Millisecond) {
		tester.Advance(16 * time.Millisecond)
		tester.Frame()
		if len(again.App.Data.Projects) == 1 && slices.Contains(tester.Texts(), "Backup workspace") {
			break
		}
		if time.Now().After(deadline) {
			t.Fatalf("the relaunched window did not show the project; visible text: %q", tester.Texts())
		}
	}
	for range 20 {
		tester.Advance(16 * time.Millisecond)
		tester.Frame()
	}
	empty("after the relaunch", again.App, tester.Texts())
	t.Log("step 11 passed: Manage listed the archived session; Unarchive returned it; Archive all archived every session and disabled itself; Delete asked once naming the session and its consequence; Delete all asked once naming the consequence and emptied the sidebar; a Node started again on the same profile lists no session and its new window's sidebar shows the project with no session, only the unsent new conversation draft a window opens on")
}
