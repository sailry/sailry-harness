package node

import (
	"bytes"
	"context"
	"database/sql"
	"encoding/hex"
	"encoding/json"
	"errors"
	"maps"
	"os"
	"path/filepath"
	"reflect"
	"slices"
	"strings"
	"testing"

	git "github.com/go-git/go-git/v5"
	sdk "github.com/modelcontextprotocol/go-sdk/mcp"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/node/admission"
	"github.com/sailry/sailry-harness/internal/node/mcp"
	"github.com/sailry/sailry-harness/internal/node/process"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
	"lukechampine.com/blake3"
)

func nodeFixture(t *testing.T) *Node {
	t.Helper()
	value, err := startFixtureNode(context.Background(), filepath.Join(t.TempDir(), "profile"))
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() {
		if err := value.Stop(context.Background()); err != nil {
			t.Error(err)
		}
	})
	return value
}

func dispatchFixture(t *testing.T, node *Node, caller protocol.NodeID, request protocol.Request) link.Response {
	t.Helper()
	accepted, err := node.Dispatch(context.Background(), caller, request)
	if err != nil {
		t.Fatal(err)
	}
	value, ok := <-accepted.Completion
	if !ok {
		t.Fatal("completion was lost")
	}
	return value
}

func commandFixture(t *testing.T, node *Node, command protocol.Command) protocol.Output {
	t.Helper()
	value := dispatchFixture(t, node, node.ID(), protocol.NewRequest(node.ID(), command))
	if value.Fault != nil {
		t.Fatal(value.Fault)
	}
	return value.Output
}

func sessionFixture(t *testing.T, node *Node) protocol.Session {
	t.Helper()
	config := protocol.DefaultSessionConfig()
	config.Provider = protocol.NewProviderID()
	config.Model = "fixture"
	config.Mode = protocol.WorkModeCode
	config.Permission = protocol.PermissionProject
	return commandFixture(t, node, protocol.Command{Kind: "create_session", Data: protocol.CommandCreateSessionPayload{Config: &config}}).Data.(protocol.Session)
}

func snapshotFixture(t *testing.T, node *Node) protocol.Snapshot {
	t.Helper()
	value, err := node.Snapshot(context.Background())
	if err != nil {
		t.Fatal(err)
	}
	return value
}

// Over 1 s: each Node start installs and verifies the bundled packages.
func TestAdmissionSameRequestID(t *testing.T) {
	t.Parallel()
	for _, test := range []struct {
		name     string
		settings protocol.PowerSettings
		want     protocol.ErrorCode
	}{
		{"completed success", protocol.PowerSettings{Revision: 0, PreventSleep: false}, ""},
		{"completed failure", protocol.PowerSettings{Revision: 99, PreventSleep: false}, protocol.ErrorCodeRevisionConflict},
	} {
		t.Run(test.name, func(t *testing.T) {
			node := nodeFixture(t)
			request := protocol.NewRequest(node.ID(), protocol.Command{Kind: "save_power_settings", Data: test.settings})
			first := dispatchFixture(t, node, node.ID(), request)
			before := snapshotFixture(t, node)
			second := dispatchFixture(t, node, node.ID(), request)
			firstResult := protocol.Result[protocol.Output, protocol.Fault]{Err: first.Fault}
			secondResult := protocol.Result[protocol.Output, protocol.Fault]{Err: second.Fault}
			if first.Fault == nil {
				firstResult.Ok = &first.Output
			}
			if second.Fault == nil {
				secondResult.Ok = &second.Output
			}
			firstBody, _ := json.Marshal(firstResult)
			secondBody, _ := json.Marshal(secondResult)
			if string(firstBody) != string(secondBody) {
				t.Fatalf("replay differs: %#v %#v", first, second)
			}
			if test.want != "" && (first.Fault == nil || first.Fault.Code != test.want) {
				t.Fatalf("fault = %v", first.Fault)
			}
			if snapshotFixture(t, node).Cursor != before.Cursor {
				t.Fatal("replay emitted another event")
			}
			request.Command.Data = protocol.PowerSettings{Revision: 1, PreventSleep: true}
			_, err := node.Dispatch(context.Background(), node.ID(), request)
			var conflict *protocol.Fault
			if !errors.As(err, &conflict) || conflict.Code != protocol.ErrorCodeConflict {
				t.Fatalf("different content = %v", err)
			}
			var other protocol.NodeID
			other[0] = 99
			request.Command.Data = test.settings
			accepted, err := node.Dispatch(context.Background(), other, request)
			if err != nil {
				t.Fatal(err)
			}
			<-accepted.Completion
			var count int
			if err := node.profile.Store.Node.Read(context.Background(), func(db *sql.Conn) error {
				return db.QueryRowContext(context.Background(), "SELECT count(*) FROM requests WHERE id=?1", request.ID.String()).Scan(&count)
			}); err != nil {
				t.Fatal(err)
			}
			if count != 2 {
				t.Fatalf("caller-scoped admissions = %d", count)
			}
		})
	}
	// Every command that can carry a secret is admitted by digest: nothing in
	// the ledger holds the secret, replay returns the recorded outcome, and a
	// different secret under the same identifier is a conflict.
	t.Run("secret-bearing commands are kept by digest", func(t *testing.T) {
		node := nodeFixture(t)
		ledger := func() string {
			var text bytes.Buffer
			err := node.profile.Store.Node.Read(context.Background(), func(db *sql.Conn) error {
				rows, err := db.QueryContext(context.Background(), "SELECT body,coalesce(result,'') FROM requests")
				if err != nil {
					return err
				}
				defer rows.Close()
				for rows.Next() {
					var body, result []byte
					if err := rows.Scan(&body, &result); err != nil {
						return err
					}
					text.Write(body)
					text.Write(result)
				}
				return rows.Err()
			})
			if err != nil {
				t.Fatal(err)
			}
			return text.String()
		}
		outcome := func(request protocol.Request) (string, error) {
			accepted, err := node.Dispatch(context.Background(), node.ID(), request)
			if err != nil {
				return "", err
			}
			value := <-accepted.Completion
			result := protocol.Result[protocol.Output, protocol.Fault]{Err: value.Fault}
			if value.Fault == nil {
				result.Ok = &value.Output
			}
			body, err := json.Marshal(result)
			return string(body), err
		}
		commands := secretCommands("ledger-canary")
		for _, kind := range protocol.CommandKinds() {
			if _, built := commands[kind]; admission.DigestOnly(kind) != built {
				t.Errorf("%s: kept by digest = %t, exercised here = %t", kind, admission.DigestOnly(kind), built)
			}
		}
		var refused []string
		for _, kind := range slices.Sorted(maps.Keys(commands)) {
			request := protocol.NewRequest(node.ID(), commands[kind])
			encoded, err := json.Marshal(request)
			if err != nil || !bytes.Contains(encoded, []byte("ledger-canary")) {
				t.Fatalf("%s request = %s, %v", kind, encoded, err)
			}
			first, dispatchErr := outcome(request)
			if text := ledger(); strings.Contains(text, "ledger-canary") {
				t.Fatalf("the ledger holds the secret of %s: %s", kind, text)
			}
			var body []byte
			caller := node.ID()
			err = node.profile.Store.Node.Read(context.Background(), func(db *sql.Conn) error {
				return db.QueryRowContext(context.Background(), "SELECT body FROM requests WHERE caller=?1 AND id=?2", caller[:], request.ID.String()).Scan(&body)
			})
			if errors.Is(err, sql.ErrNoRows) && dispatchErr != nil {
				// Refused before admission: there is nothing to replay.
				refused = append(refused, kind)
				continue
			}
			digest := blake3.Sum256(encoded)
			if err != nil || dispatchErr != nil || string(body) != "credential-v1:"+hex.EncodeToString(digest[:]) {
				t.Fatalf("%s admission = %q, %v, %v", kind, body, err, dispatchErr)
			}
			if replayed, err := outcome(request); err != nil || replayed != first {
				t.Fatalf("%s replay = %s, %v, want %s", kind, replayed, err, first)
			}
			inspected := commandFixture(t, node, protocol.Command{Kind: "inspect_request", Data: protocol.CommandInspectRequestPayload{ID: request.ID, Digest: hex.EncodeToString(digest[:])}})
			if inspected.Data.(protocol.OutputRequestOutcomePayload).Outcome.Kind != "completed" {
				t.Fatalf("%s inspection = %#v", kind, inspected)
			}
			request.Command = secretCommands("another-secret")[kind]
			var conflict *protocol.Fault
			if _, err := outcome(request); !errors.As(err, &conflict) || conflict.Code != protocol.ErrorCodeConflict {
				t.Fatalf("%s with another secret = %v", kind, err)
			}
		}
		// A restore names a backup that does not exist here and is refused
		// before admission; it reaches the ledger through the same call.
		if want := []string{"restore_backup", "restore_backup_file"}; !slices.Equal(refused, want) {
			t.Fatalf("refused before admission: %v, want %v", refused, want)
		}
		if text := ledger(); strings.Contains(text, "ledger-canary") || strings.Contains(text, "another-secret") {
			t.Fatalf("the ledger holds a secret: %s", text)
		}
	})
	t.Run("project lifecycle and worktree ownership", func(t *testing.T) {
		node := nodeFixture(t)
		directory := t.TempDir()
		other := t.TempDir()
		if _, err := git.PlainInit(directory, false); err != nil {
			t.Fatal(err)
		}
		if _, err := git.PlainInit(other, false); err != nil {
			t.Fatal(err)
		}
		draft := protocol.ProjectsDraft{Name: "Project", Path: directory, Appearance: protocol.DefaultProjectsAppearance(), Source: protocol.ProjectsSource{Kind: "local"}}
		project := commandFixture(t, node, protocol.Command{Kind: "create_project", Data: draft}).Data.(protocol.Project)
		value := snapshotFixture(t, node)
		if len(value.Projects) != 1 || len(value.Worktrees) != 1 || !value.Worktrees[0].Main || value.Worktrees[0].Project == nil || *value.Worktrees[0].Project != project.ID {
			t.Fatalf("main worktree snapshot = %#v", value)
		}
		commandFixture(t, node, protocol.Command{Kind: "register_worktree", Data: protocol.CommandRegisterWorktreePayload{Project: project.ID, Path: project.Path}})
		for _, command := range []protocol.Command{{Kind: "create_project", Data: draft}, {Kind: "register_worktree", Data: protocol.CommandRegisterWorktreePayload{Project: project.ID, Path: other}}} {
			response := dispatchFixture(t, node, node.ID(), protocol.NewRequest(node.ID(), command))
			if response.Fault == nil || response.Fault.Code != protocol.ErrorCodeConflict {
				t.Fatalf("registration conflict = %#v", response)
			}
		}
		updated := commandFixture(t, node, protocol.Command{Kind: "update_project", Data: protocol.CommandUpdateProjectPayload{Expected: project, Name: "Updated", Path: project.Path, Appearance: project.Appearance}}).Data.(protocol.Project)
		stale := dispatchFixture(t, node, node.ID(), protocol.NewRequest(node.ID(), protocol.Command{Kind: "remove_project", Data: protocol.CommandRemoveProjectPayload{Expected: project}}))
		if stale.Fault == nil || stale.Fault.Code != protocol.ErrorCodeRevisionConflict {
			t.Fatalf("complete expected-project conflict = %#v", stale)
		}
		commandFixture(t, node, protocol.Command{Kind: "remove_project", Data: protocol.CommandRemoveProjectPayload{Expected: updated}})
		if len(snapshotFixture(t, node).Projects) != 0 {
			t.Fatal("removed project remained visible")
		}
		if _, err := os.Stat(directory); err != nil {
			t.Fatal("project removal changed its directory", err)
		}
		restored := commandFixture(t, node, protocol.Command{Kind: "create_project", Data: draft}).Data.(protocol.Project)
		if restored.ID != project.ID {
			t.Fatal("restoring a project changed its identity")
		}
		path := node.profile.Ownership.Path
		if err := node.Stop(context.Background()); err != nil {
			t.Fatal(err)
		}
		reopened, err := startFixtureNode(context.Background(), path)
		if err != nil {
			t.Fatal(err)
		}
		defer reopened.Stop(context.Background())
		value = snapshotFixture(t, reopened)
		if len(value.Projects) != 1 || value.Projects[0].ID != project.ID || value.Cursor != 4 {
			t.Fatalf("reopened project = %#v", value)
		}
	})
	t.Run("registered projects, conversation location, media roles and diagnostics", func(t *testing.T) {
		node := nodeFixture(t)
		refused := func(command protocol.Command, want protocol.ErrorCode) {
			t.Helper()
			response := dispatchFixture(t, node, node.ID(), protocol.NewRequest(node.ID(), command))
			if response.Fault == nil || response.Fault.Code != want {
				t.Fatalf("%s = %#v, want %s", command.Kind, response, want)
			}
		}
		root, linked := t.TempDir(), t.TempDir()
		if _, err := git.PlainInit(root, false); err != nil {
			t.Fatal(err)
		}
		// The layout `git worktree add` leaves for a linked worktree.
		private := filepath.Join(root, ".git", "worktrees", "linked")
		if err := os.MkdirAll(private, 0o755); err != nil {
			t.Fatal(err)
		}
		for path, body := range map[string]string{
			filepath.Join(private, "commondir"): "../..\n",
			filepath.Join(private, "HEAD"):      "ref: refs/heads/linked\n",
			filepath.Join(private, "gitdir"):    filepath.Join(linked, ".git") + "\n",
			filepath.Join(linked, ".git"):       "gitdir: " + private + "\n",
		} {
			if err := os.WriteFile(path, []byte(body), 0o644); err != nil {
				t.Fatal(err)
			}
		}
		registration := protocol.Command{Kind: "register_project", Data: protocol.CommandRegisterProjectPayload{Name: "Registered", Path: root}}
		project := commandFixture(t, node, registration).Data.(protocol.Project)
		if project.Name != "Registered" || project.Appearance != protocol.DefaultProjectsAppearance() {
			t.Fatalf("registered project = %#v", project)
		}
		refused(registration, protocol.ErrorCodeConflict)
		refused(protocol.Command{Kind: "register_project", Data: protocol.CommandRegisterProjectPayload{Name: "Relative", Path: "relative"}}, protocol.ErrorCodeInvalidRequest)
		foreign := commandFixture(t, node, protocol.Command{Kind: "register_project", Data: protocol.CommandRegisterProjectPayload{Name: "Foreign", Path: t.TempDir()}}).Data.(protocol.Project)
		tree := commandFixture(t, node, protocol.Command{Kind: "register_worktree", Data: protocol.CommandRegisterWorktreePayload{Project: project.ID, Path: linked}}).Data.(protocol.Worktree)
		var outside protocol.WorktreeID
		for _, worktree := range snapshotFixture(t, node).Worktrees {
			if worktree.Project != nil && *worktree.Project == foreign.ID {
				outside = worktree.ID
			}
		}
		config := sessionFixture(t, node).Config
		session := commandFixture(t, node, protocol.Command{Kind: "create_session", Data: protocol.CommandCreateSessionPayload{Project: &project.ID, Config: &config}}).Data.(protocol.Session)
		move := func(worktree protocol.WorktreeID, revision uint64) protocol.Command {
			return protocol.Command{Kind: "move_conversation", Data: protocol.CommandMoveConversationPayload{Session: session.ID, Worktree: worktree, ExpectedRevision: revision}}
		}
		if same := commandFixture(t, node, move(session.Worktree, session.Revision)).Data.(protocol.Session); same.Revision != session.Revision {
			t.Fatalf("moving to the current worktree revised the conversation: %#v", same)
		}
		refused(move(outside, session.Revision), protocol.ErrorCodeNotFound)
		refused(move(tree.ID, session.Revision+1), protocol.ErrorCodeRevisionConflict)
		refused(protocol.Command{Kind: "fork_conversation_at", Data: protocol.CommandForkConversationAtPayload{Session: session.ID, Worktree: outside, ExpectedRevision: session.Revision}}, protocol.ErrorCodeNotFound)
		moved := commandFixture(t, node, move(tree.ID, session.Revision)).Data.(protocol.Session)
		if moved.Worktree != tree.ID || moved.Revision != session.Revision+1 || moved.Project == nil || *moved.Project != project.ID {
			t.Fatalf("moved conversation = %#v", moved)
		}
		if err := os.RemoveAll(root); err != nil {
			t.Fatal(err)
		}
		// The conversation started in the main worktree, whose directory is gone.
		refused(move(session.Worktree, moved.Revision), protocol.ErrorCodeNotFound)
		refused(protocol.Command{Kind: "fork_conversation_at", Data: protocol.CommandForkConversationAtPayload{Session: session.ID, Worktree: session.Worktree, ExpectedRevision: moved.Revision}}, protocol.ErrorCodeNotFound)

		secret := protocol.NewSecret("media-key")
		provider := protocol.DefaultConversationProvider()
		provider.ID, provider.Name, provider.Enabled = protocol.NewProviderID(), "Media fixture", true
		provider.Api, provider.Authentication, provider.Endpoint = protocol.ConversationModelApiChatCompletions, protocol.AuthenticationApiKey, "https://media.test/v1"
		seeing, blind := protocol.DefaultConversationModel(), protocol.DefaultConversationModel()
		seeing.ID, seeing.Vision, seeing.Context, seeing.Output = "seeing", true, 128000, 4096
		blind.ID, blind.Context, blind.Output = "blind", 128000, 4096
		sightless, capabilities := false, protocol.DefaultConversationDiscoveryCapabilities()
		capabilities.Vision, blind.Capabilities = &sightless, &capabilities
		provider.Models, provider.DefaultModel = []protocol.ConversationModel{seeing, blind}, "seeing"
		commandFixture(t, node, protocol.Command{Kind: "save_provider", Data: protocol.CommandSaveProviderPayload{Provider: provider, Secret: &secret}})
		if initial := commandFixture(t, node, protocol.Command{Kind: "read_media_settings"}).Data.(protocol.MediaSettings); initial.Revision != 0 || len(initial.Bindings) != 0 {
			t.Fatalf("initial media settings = %#v", initial)
		}
		candidates := commandFixture(t, node, protocol.Command{Kind: "list_media_models"}).Data.([]protocol.MediaCandidate)
		want := []protocol.MediaCandidate{
			{Provider: provider.ID, ProviderName: provider.Name, Model: "seeing", Kinds: []protocol.MediaKind{protocol.MediaKindVision}},
		}
		if !reflect.DeepEqual(candidates, want) {
			t.Fatalf("media candidates = %#v", candidates)
		}
		bind := func(revision uint64, model string) protocol.Command {
			return protocol.Command{Kind: "save_media_settings", Data: protocol.MediaSettings{Revision: revision, Bindings: map[protocol.MediaKind]*protocol.MediaBinding{
				protocol.MediaKindVision: {Provider: provider.ID, Model: model},
			}}}
		}
		refused(bind(0, "blind"), protocol.ErrorCodeInvalidRequest)
		refused(bind(0, "absent"), protocol.ErrorCodeNotConfigured)
		refused(bind(7, "seeing"), protocol.ErrorCodeRevisionConflict)
		saved := commandFixture(t, node, bind(0, "seeing")).Data.(protocol.MediaSettings)
		if saved.Revision != 1 || !reflect.DeepEqual(snapshotFixture(t, node).MediaSettings, saved) {
			t.Fatalf("saved media settings = %#v", saved)
		}
		if read := commandFixture(t, node, protocol.Command{Kind: "read_media_settings"}).Data.(protocol.MediaSettings); !reflect.DeepEqual(read, saved) {
			t.Fatalf("read media settings = %#v", read)
		}

		report := protocol.DefaultNotificationSystemSystemDiagnostic()
		report.Incident, report.Source, report.Phase = protocol.NewRequestID(), protocol.NotificationSystemSystemSourceGitInspection, protocol.NotificationSystemDiagnosticPhaseInspection
		report.Reason, report.Code, report.Version, report.Platform = protocol.NotificationSystemDiagnosticReasonTimeout, protocol.ErrorCodeUnavailable, "1.0.0", "darwin-arm64"
		diagnostic := protocol.Command{Kind: "report_diagnostic", Data: report}
		notice := commandFixture(t, node, diagnostic).Data.(protocol.NotificationNotice)
		repeated := commandFixture(t, node, diagnostic).Data.(protocol.NotificationNotice)
		notices := snapshotFixture(t, node).Notifications
		if repeated.ID != notice.ID || len(notices) != 1 || notices[0].ID != notice.ID || notice.Content.Kind != protocol.NotificationKindError {
			t.Fatalf("diagnostic notices = %#v", notices)
		}
		report.Incident, report.Reason, report.Code = protocol.NewRequestID(), protocol.NotificationSystemDiagnosticReasonCancelled, protocol.ErrorCodeCancelled
		if output := commandFixture(t, node, protocol.Command{Kind: "report_diagnostic", Data: report}); output.Kind != "notifications_updated" || len(snapshotFixture(t, node).Notifications) != 1 {
			t.Fatalf("cancelled diagnostic = %#v", output)
		}
		report.Incident, report.Platform = protocol.NewRequestID(), "not a token"
		refused(protocol.Command{Kind: "report_diagnostic", Data: report}, protocol.ErrorCodeInvalidRequest)
	})
	t.Run("project and captured worktree catalogs", func(t *testing.T) {
		node := nodeFixture(t)
		var projects []protocol.Project
		for _, name := range []string{"First", "Second"} {
			draft := protocol.ProjectsDraft{Name: name, Path: t.TempDir(), Appearance: protocol.DefaultProjectsAppearance(), Source: protocol.ProjectsSource{Kind: "local"}}
			projects = append(projects, commandFixture(t, node, protocol.Command{Kind: "create_project", Data: draft}).Data.(protocol.Project))
		}
		session := sessionFixture(t, node)
		snapshot := snapshotFixture(t, node)
		var captured protocol.WorktreeID
		for _, worktree := range snapshot.Worktrees {
			if worktree.Project != nil && *worktree.Project == projects[0].ID {
				captured = worktree.ID
			}
		}
		for _, test := range []struct {
			name     string
			command  protocol.Command
			projects int
			trees    int
			captured protocol.WorktreeID
		}{
			{"registered projects", protocol.Command{Kind: "read_project_catalog"}, 2, 2, protocol.WorktreeID{}},
			{"captured project", protocol.Command{Kind: "read_worktree_catalog", Data: protocol.CommandReadWorktreeCatalogPayload{Worktree: captured}}, 1, 1, captured},
			{"scratch workspace", protocol.Command{Kind: "read_worktree_catalog", Data: protocol.CommandReadWorktreeCatalogPayload{Worktree: session.Worktree}}, 0, 1, session.Worktree},
		} {
			t.Run(test.name, func(t *testing.T) {
				output := commandFixture(t, node, test.command)
				catalog, ok := output.Data.(protocol.ProjectsCatalog)
				if output.Kind != "project_catalog" || !ok || len(catalog.Projects) != test.projects || len(catalog.Worktrees) != test.trees {
					t.Fatalf("catalog = %#v", output)
				}
				if test.captured != (protocol.WorktreeID{}) && catalog.Worktrees[0].ID != test.captured {
					t.Fatalf("catalog selected another worktree: %#v", catalog)
				}
				if len(catalog.Projects) == 1 && catalog.Projects[0].ID != projects[0].ID {
					t.Fatalf("catalog selected another project: %#v", catalog)
				}
			})
		}
		request := protocol.NewRequest(node.ID(), protocol.Command{Kind: "read_worktree_catalog", Data: protocol.CommandReadWorktreeCatalogPayload{Worktree: protocol.NewWorktreeID()}})
		result := dispatchFixture(t, node, node.ID(), request)
		if result.Fault == nil || result.Fault.Code != protocol.ErrorCodeNotFound {
			t.Fatalf("missing captured worktree = %#v", result)
		}
		if current := snapshotFixture(t, node).Cursor; current != snapshot.Cursor {
			t.Fatalf("read-only catalogs changed the Node cursor: %d != %d", current, snapshot.Cursor)
		}
	})
	t.Run("notification replay and settings revisions", func(t *testing.T) {
		node := nodeFixture(t)
		ctx := context.Background()
		info := protocol.DefaultPluginInfo()
		info.Summary.Name = "fixture"
		info.Summary.Digest = strings.Repeat("a", 64)
		info.Summary.Enabled = true
		info.Summary.Revision = 1
		body, err := json.Marshal(info)
		if err != nil {
			t.Fatal(err)
		}
		if err := node.profile.Store.Node.Write(ctx, func(db *sql.Conn) error {
			_, err := db.ExecContext(ctx, "INSERT INTO plugins(name,revision,settings_revision,installed,body) VALUES(?1,1,0,1,?2)", info.Summary.Name, body)
			return err
		}); err != nil {
			t.Fatal(err)
		}
		content := protocol.NotificationDraft{Title: "Fixture", Message: "Retained details", Kind: protocol.NotificationKindInfo, Details: &protocol.NotificationDetails{Kind: "text", Data: protocol.NotificationDetailsTextPayload{Body: "Technical details"}}}
		request := protocol.NewRequest(node.ID(), protocol.Command{Kind: "publish_notification", Data: protocol.CommandPublishNotificationPayload{Package: protocol.PluginReference{Name: info.Summary.Name, Digest: info.Summary.Digest}, Content: content}})
		first := dispatchFixture(t, node, node.ID(), request)
		if first.Fault != nil {
			t.Fatal(first.Fault)
		}
		if result := dispatchFixture(t, node, node.ID(), request); result.Fault != nil {
			t.Fatal(result.Fault)
		}
		value := snapshotFixture(t, node)
		if value.Cursor != 1 || len(value.Notifications) != 1 {
			t.Fatalf("notification replay = %#v", value)
		}
		notice := value.Notifications[0]
		if notice.Content.Details == nil || notice.Content.Details.Data.(*protocol.NotificationDetailsTextPayload).Body != "Technical details" {
			t.Fatal("notification lost typed details")
		}
		commandFixture(t, node, protocol.Command{Kind: "dismiss_read_notifications", Data: protocol.CommandDismissReadNotificationsPayload{IDs: []protocol.NotificationID{notice.ID}}})
		if len(snapshotFixture(t, node).Notifications) != 1 {
			t.Fatal("dismiss-read removed an unread notice")
		}
		commandFixture(t, node, protocol.Command{Kind: "mark_notifications_read", Data: protocol.CommandMarkNotificationsReadPayload{IDs: []protocol.NotificationID{notice.ID, notice.ID}}})
		if value := snapshotFixture(t, node); value.Cursor != 2 || !value.Notifications[0].Read {
			t.Fatal("read event did not record one actual change")
		}
		commandFixture(t, node, protocol.Command{Kind: "dismiss_read_notifications", Data: protocol.CommandDismissReadNotificationsPayload{IDs: []protocol.NotificationID{notice.ID}}})
		if value := snapshotFixture(t, node); value.Cursor != 3 || len(value.Notifications) != 0 {
			t.Fatal("selected read notice was not dismissed")
		}
		settings := commandFixture(t, node, protocol.Command{Kind: "read_telemetry_settings"}).Data.(protocol.TelemetrySettings)
		saved := commandFixture(t, node, protocol.Command{Kind: "save_telemetry_settings", Data: protocol.TelemetryUpdate{Revision: settings.Revision, Errors: false, Metrics: false}}).Data.(protocol.TelemetrySettings)
		if saved.InstallationID != settings.InstallationID || saved.Revision != settings.Revision+1 || saved.Errors || saved.Metrics {
			t.Fatalf("telemetry update = %#v", saved)
		}
		stale := dispatchFixture(t, node, node.ID(), protocol.NewRequest(node.ID(), protocol.Command{Kind: "save_telemetry_settings", Data: protocol.TelemetryUpdate{Revision: settings.Revision, Errors: true, Metrics: true}}))
		if stale.Fault == nil || stale.Fault.Code != protocol.ErrorCodeRevisionConflict {
			t.Fatalf("telemetry conflict = %#v", stale)
		}
	})
}

// Over 1 s: each Node start installs and verifies the bundled packages.
func TestReceiptAfterDurableAdmission(t *testing.T) {
	t.Parallel()
	node := nodeFixture(t)
	ctx, cancel := context.WithCancel(context.Background())
	request := protocol.NewRequest(node.ID(), protocol.Command{Kind: "save_power_settings", Data: protocol.PowerSettings{PreventSleep: false}})
	accepted, err := node.Dispatch(ctx, node.ID(), request)
	if err != nil {
		t.Fatal(err)
	}
	if !accepted.Receipt.Durable || accepted.Receipt.ID != request.ID {
		t.Fatalf("receipt = %#v", accepted.Receipt)
	}
	cancel()
	var status string
	caller := node.ID()
	if err := node.profile.Store.Node.Read(context.Background(), func(db *sql.Conn) error {
		return db.QueryRowContext(context.Background(), "SELECT status FROM requests WHERE caller=?1 AND id=?2", caller[:], request.ID.String()).Scan(&status)
	}); err != nil {
		t.Fatal(err)
	}
	if status != "admitted" && status != "completed" {
		t.Fatalf("admission status = %s", status)
	}
	result := <-accepted.Completion
	if result.Fault != nil {
		t.Fatal(result.Fault)
	}
	if snapshotFixture(t, node).PowerSettingsRevision != 1 {
		t.Fatal("observer cancellation cancelled admitted work")
	}
	// A busy project worker delays neither the admission of later commands,
	// nor reads, nor the commands of the other lane.
	release := node.locks.registration()
	held := true
	defer func() {
		if held {
			release()
		}
	}()
	var pending []link.Admission
	for range 2 {
		draft := protocol.ProjectsDraft{Name: "Project", Path: t.TempDir(), Appearance: protocol.DefaultProjectsAppearance(), Source: protocol.ProjectsSource{Kind: "local"}}
		accepted, err := node.Dispatch(context.Background(), node.ID(), protocol.NewRequest(node.ID(), protocol.Command{Kind: "create_project", Data: draft}))
		if err != nil {
			t.Fatal(err)
		}
		pending = append(pending, accepted)
	}
	if len(snapshotFixture(t, node).Projects) != 0 {
		t.Fatal("a project was created while the worker was held")
	}
	sessionFixture(t, node)
	held = false
	release()
	for _, accepted := range pending {
		if result := <-accepted.Completion; result.Fault != nil {
			t.Fatal(result.Fault)
		}
	}
	if len(snapshotFixture(t, node).Projects) != 2 {
		t.Fatal("queued commands did not run")
	}
}

// Over 1 s: each Node start installs and verifies the bundled packages.
func TestOutcomeUnknownAfterCrash(t *testing.T) {
	t.Parallel()
	ctx := context.Background()
	path := filepath.Join(t.TempDir(), "profile")
	node, err := startFixtureNode(ctx, path)
	if err != nil {
		t.Fatal(err)
	}
	id := node.ID()
	request := protocol.NewRequest(id, protocol.Command{Kind: "save_power_settings", Data: protocol.PowerSettings{PreventSleep: false}})
	body, err := json.Marshal(request)
	if err != nil {
		t.Fatal(err)
	}
	// A secret-bearing request interrupted after its admission, and the rows
	// an earlier build left for such commands: their complete request.
	const legacyKey = "legacy-ledger-key"
	encode := func(request protocol.Request) []byte {
		t.Helper()
		body, err := json.Marshal(request)
		if err != nil {
			t.Fatal(err)
		}
		return body
	}
	interrupted := protocol.NewRequest(id, secretCommands("interrupted-ledger-key")["save_provider"])
	legacyCompleted := protocol.NewRequest(id, secretCommands(legacyKey)["save_provider"])
	legacyAdmitted := protocol.NewRequest(id, secretCommands(legacyKey)["put_credential"])
	recorded, err := json.Marshal(protocol.Result[protocol.Output, protocol.Fault]{Err: &protocol.Fault{Code: protocol.ErrorCodeConflict, Message: "recorded before the update"}})
	if err != nil {
		t.Fatal(err)
	}
	if err := node.profile.Store.Node.Write(ctx, func(db *sql.Conn) error {
		if _, err := admission.Admit(ctx, db, id, request, body); err != nil {
			return err
		}
		if _, err := admission.Admit(ctx, db, id, interrupted, encode(interrupted)); err != nil {
			return err
		}
		if _, err := db.ExecContext(ctx, "INSERT INTO requests(caller,id,body,status,result) VALUES(?1,?2,?3,'completed',?4)", id[:], legacyCompleted.ID.String(), encode(legacyCompleted), string(recorded)); err != nil {
			return err
		}
		_, err := db.ExecContext(ctx, "INSERT INTO requests(caller,id,body,status) VALUES(?1,?2,?3,'admitted')", id[:], legacyAdmitted.ID.String(), encode(legacyAdmitted))
		return err
	}); err != nil {
		t.Fatal(err)
	}
	if err := node.Stop(ctx); err != nil {
		t.Fatal(err)
	}
	// stored names the files below the profile's storage that hold the text.
	stored := func(text string) (found []string) {
		t.Helper()
		err := filepath.WalkDir(filepath.Join(path, "storage"), func(file string, entry os.DirEntry, err error) error {
			if err != nil || !entry.Type().IsRegular() {
				return err
			}
			content, err := os.ReadFile(file)
			if bytes.Contains(content, []byte(text)) {
				found = append(found, file)
			}
			return err
		})
		if err != nil {
			t.Fatal(err)
		}
		return found
	}
	backups := func() int {
		t.Helper()
		entries, err := os.ReadDir(filepath.Join(path, "storage", "backups"))
		if err != nil && !errors.Is(err, os.ErrNotExist) {
			t.Fatal(err)
		}
		return len(entries)
	}
	if len(stored(legacyKey)) == 0 || len(stored("interrupted-ledger-key")) != 0 || backups() != 0 {
		t.Fatalf("before the update: legacy secret in %v, interrupted secret in %v, %d backups", stored(legacyKey), stored("interrupted-ledger-key"), backups())
	}
	restarted, err := startFixtureNode(ctx, path)
	if err != nil {
		t.Fatal(err)
	}
	stopped := false
	defer func() {
		if !stopped {
			restarted.Stop(ctx)
		}
	}()
	result := dispatchFixture(t, restarted, id, request)
	if result.Fault == nil || result.Fault.Code != protocol.ErrorCodeOutcomeUnknown {
		t.Fatalf("recovery = %#v", result)
	}
	for name, unfinished := range map[string]protocol.Request{"interrupted": interrupted, "legacy": legacyAdmitted} {
		result := dispatchFixture(t, restarted, id, unfinished)
		if result.Fault == nil || result.Fault.Code != protocol.ErrorCodeOutcomeUnknown {
			t.Fatalf("%s secret-bearing recovery = %#v", name, result)
		}
	}
	// The update kept the replay identity of the rows it replaced.
	if result := dispatchFixture(t, restarted, id, legacyCompleted); result.Fault == nil || result.Fault.Message != "recorded before the update" {
		t.Fatalf("legacy replay = %#v", result)
	}
	for _, legacy := range []protocol.Request{legacyCompleted, legacyAdmitted} {
		changed := legacy
		changed.Command = secretCommands("another-secret")[legacy.Command.Kind]
		var conflict *protocol.Fault
		if _, err := restarted.Dispatch(ctx, id, changed); !errors.As(err, &conflict) || conflict.Code != protocol.ErrorCodeConflict {
			t.Fatalf("legacy %s with another secret = %v", legacy.Command.Kind, err)
		}
		digest := blake3.Sum256(encode(legacy))
		var kept []byte
		if err := restarted.profile.Store.Node.Read(ctx, func(db *sql.Conn) error {
			return db.QueryRowContext(ctx, "SELECT body FROM requests WHERE caller=?1 AND id=?2", id[:], legacy.ID.String()).Scan(&kept)
		}); err != nil || string(kept) != "credential-v1:"+hex.EncodeToString(digest[:]) {
			t.Fatalf("legacy %s body = %q, %v", legacy.Command.Kind, kept, err)
		}
		inspection := commandFixture(t, restarted, protocol.Command{Kind: "inspect_request", Data: protocol.CommandInspectRequestPayload{ID: legacy.ID, Digest: hex.EncodeToString(digest[:])}})
		if want := map[string]string{"save_provider": "completed", "put_credential": "unknown"}[legacy.Command.Kind]; inspection.Data.(protocol.OutputRequestOutcomePayload).Outcome.Kind != want {
			t.Fatalf("legacy %s inspection = %#v, want %s", legacy.Command.Kind, inspection, want)
		}
	}
	if providers := commandFixture(t, restarted, protocol.Command{Kind: "list_providers"}).Data.([]protocol.ConversationProvider); len(providers) != 0 {
		t.Fatalf("a replayed admission ran again: %#v", providers)
	}
	if snapshotFixture(t, restarted).Cursor != 0 {
		t.Fatal("unfinished admission was replayed")
	}
	digest := blake3.Sum256(body)
	inspection := commandFixture(t, restarted, protocol.Command{Kind: "inspect_request", Data: protocol.CommandInspectRequestPayload{ID: request.ID, Digest: hex.EncodeToString(digest[:])}})
	if inspection.Data.(protocol.OutputRequestOutcomePayload).Outcome.Kind != "unknown" {
		t.Fatalf("inspection = %#v", inspection)
	}
	for _, digest := range []string{"bad", string(make([]byte, 64))} {
		value := dispatchFixture(t, restarted, id, protocol.NewRequest(id, protocol.Command{Kind: "inspect_request", Data: protocol.CommandInspectRequestPayload{ID: request.ID, Digest: digest}}))
		if value.Fault == nil || value.Fault.Code != protocol.ErrorCodeInvalidRequest {
			t.Fatalf("invalid digest accepted: %#v", value)
		}
	}
	// Neither the database, its log, nor the backup that preceded the update
	// keeps the secret, and a later start has nothing left to replace.
	stopped = true
	if err := restarted.Stop(ctx); err != nil {
		t.Fatal(err)
	}
	if files := stored(legacyKey); len(files) != 0 || backups() != 1 {
		t.Fatalf("after the update: legacy secret in %v, %d backups", files, backups())
	}
	again, err := startFixtureNode(ctx, path)
	if err != nil {
		t.Fatal(err)
	}
	if err := again.Stop(ctx); err != nil {
		t.Fatal(err)
	}
	if files := stored(legacyKey); len(files) != 0 || backups() != 1 {
		t.Fatalf("after another start: legacy secret in %v, %d backups", files, backups())
	}
}

// Over 1 s: each Node start installs and verifies the bundled packages.
func TestRenameRevisionConflict(t *testing.T) {
	t.Parallel()
	node := nodeFixture(t)
	missing := dispatchFixture(t, node, node.ID(), protocol.NewRequest(node.ID(), protocol.Command{Kind: "create_session", Data: protocol.CommandCreateSessionPayload{}}))
	if missing.Fault == nil || missing.Fault.Code != protocol.ErrorCodeInvalidRequest || len(snapshotFixture(t, node).Sessions) != 0 {
		t.Fatal("missing defaults created a placeholder session")
	}
	session := sessionFixture(t, node)
	defaults := commandFixture(t, node, protocol.Command{Kind: "set_defaults", Data: protocol.CommandSetDefaultsPayload{ExpectedRevision: 0, Config: session.Config}}).Data.(protocol.Defaults)
	if defaults.Revision != 1 || defaults.Config == nil {
		t.Fatal("defaults were not saved")
	}
	inherited := commandFixture(t, node, protocol.Command{Kind: "create_session", Data: protocol.CommandCreateSessionPayload{}}).Data.(protocol.Session)
	if !reflect.DeepEqual(inherited.Config, session.Config) {
		t.Fatal("session did not inherit Node defaults")
	}
	if err := node.profile.Store.Node.Write(context.Background(), func(db *sql.Conn) error {
		_, err := db.ExecContext(context.Background(), "UPDATE session_revisions SET config=CAST(json_set(config,'$.retired_private',?2) AS BLOB) WHERE session=?1 AND revision=1", session.ID.String(), "preserved")
		return err
	}); err != nil {
		t.Fatal(err)
	}
	renamed := commandFixture(t, node, protocol.Command{Kind: "rename_session", Data: protocol.CommandRenameSessionPayload{Session: session.ID, ExpectedRevision: 1, Title: "  Renamed session  "}}).Data.(protocol.Session)
	if renamed.Revision != 2 || renamed.Activity.Title != "Renamed session" {
		t.Fatalf("rename = %#v", renamed)
	}
	for _, test := range []struct {
		revision uint64
		title    string
		code     protocol.ErrorCode
	}{{1, "stale", protocol.ErrorCodeRevisionConflict}, {2, " \n ", protocol.ErrorCodeInvalidRequest}, {2, "bad\x00title", protocol.ErrorCodeInvalidRequest}} {
		value := dispatchFixture(t, node, node.ID(), protocol.NewRequest(node.ID(), protocol.Command{Kind: "rename_session", Data: protocol.CommandRenameSessionPayload{Session: session.ID, ExpectedRevision: test.revision, Title: test.title}}))
		if value.Fault == nil || value.Fault.Code != test.code {
			t.Fatalf("rename fault = %#v", value)
		}
	}
	current := commandFixture(t, node, protocol.Command{Kind: "read_session", Data: protocol.CommandReadSessionPayload{Session: session.ID}}).Data.(protocol.Session)
	if !reflect.DeepEqual(current, renamed) {
		t.Fatal("refused rename changed the session")
	}
	var revisions int
	if err := node.profile.Store.Node.Read(context.Background(), func(db *sql.Conn) error {
		return db.QueryRowContext(context.Background(), "SELECT count(*) FROM session_revisions WHERE session=?1", session.ID.String()).Scan(&revisions)
	}); err != nil {
		t.Fatal(err)
	}
	if revisions != 2 {
		t.Fatalf("immutable revisions = %d", revisions)
	}
	var preserved string
	if err := node.profile.Store.Node.Read(context.Background(), func(db *sql.Conn) error {
		return db.QueryRowContext(context.Background(), "SELECT json_extract(config,'$.retired_private') FROM session_revisions WHERE session=?1 AND revision=2", session.ID.String()).Scan(&preserved)
	}); err != nil {
		t.Fatal(err)
	}
	if preserved != "preserved" {
		t.Fatal("session revision dropped a retired private field")
	}
}

// Over 1 s: each Node start installs and verifies the bundled packages.
func TestArchiveAndRemoveEvents(t *testing.T) {
	t.Parallel()
	node := nodeFixture(t)
	first, second := sessionFixture(t, node), sessionFixture(t, node)
	baseline := snapshotFixture(t, node).Cursor
	failure := dispatchFixture(t, node, node.ID(), protocol.NewRequest(node.ID(), protocol.Command{Kind: "archive_sessions", Data: protocol.CommandArchiveSessionsPayload{Expected: []protocol.SessionRevision{{Session: first.ID, Revision: 1}, {Session: second.ID, Revision: 99}}}}))
	if failure.Fault == nil || failure.Fault.Code != protocol.ErrorCodeRevisionConflict {
		t.Fatalf("batch refusal = %#v", failure)
	}
	value := snapshotFixture(t, node)
	if value.Cursor != baseline || value.Sessions[0].Archived || value.Sessions[1].Archived {
		t.Fatal("failed batch applied a partial archive")
	}
	commandFixture(t, node, protocol.Command{Kind: "archive_sessions", Data: protocol.CommandArchiveSessionsPayload{Expected: []protocol.SessionRevision{{Session: first.ID, Revision: 1}, {Session: second.ID, Revision: 1}}}})
	value = snapshotFixture(t, node)
	if value.Cursor != baseline+2 || !value.Sessions[0].Archived || !value.Sessions[1].Archived {
		t.Fatal("archive events do not match changed sessions")
	}
	attention := commandFixture(t, node, protocol.Command{Kind: "set_session_read", Data: protocol.CommandSetSessionReadPayload{Session: first.ID, ExpectedRevision: 0, Read: false}}).Data.(protocol.Session)
	if attention.Revision != 2 || attention.Activity.Attention.Revision != 1 || !attention.Activity.Attention.Unread {
		t.Fatalf("independent attention = %#v", attention)
	}
	commandFixture(t, node, protocol.Command{Kind: "remove_sessions", Data: protocol.CommandRemoveSessionsPayload{Expected: []protocol.SessionRevision{{Session: first.ID, Revision: 2}, {Session: second.ID, Revision: 2}}}})
	value = snapshotFixture(t, node)
	if len(value.Sessions) != 0 || value.Cursor != baseline+5 {
		t.Fatalf("remove snapshot = %#v", value)
	}
	missing := dispatchFixture(t, node, node.ID(), protocol.NewRequest(node.ID(), protocol.Command{Kind: "read_session", Data: protocol.CommandReadSessionPayload{Session: first.ID}}))
	if missing.Fault == nil || missing.Fault.Code != protocol.ErrorCodeNotFound {
		t.Fatal("removed session remains readable")
	}
	var revisions int
	if err := node.profile.Store.Node.Read(context.Background(), func(db *sql.Conn) error {
		return db.QueryRowContext(context.Background(), "SELECT count(*) FROM session_revisions").Scan(&revisions)
	}); err != nil {
		t.Fatal(err)
	}
	if revisions != 4 {
		t.Fatal("removal deleted immutable history")
	}
	t.Run("project-scoped manual order", func(t *testing.T) {
		node := nodeFixture(t)
		project := commandFixture(t, node, protocol.Command{Kind: "create_project", Data: protocol.ProjectsDraft{Name: "Project", Path: t.TempDir(), Appearance: protocol.DefaultProjectsAppearance(), Source: protocol.ProjectsSource{Kind: "local"}}}).Data.(protocol.Project)
		config := first.Config
		one := commandFixture(t, node, protocol.Command{Kind: "create_session", Data: protocol.CommandCreateSessionPayload{Project: &project.ID, Config: &config}}).Data.(protocol.Session)
		two := commandFixture(t, node, protocol.Command{Kind: "create_session", Data: protocol.CommandCreateSessionPayload{Project: &project.ID, Config: &config}}).Data.(protocol.Session)
		unassigned := sessionFixture(t, node)
		command := protocol.Command{Kind: "set_session_order", Data: protocol.CommandSetSessionOrderPayload{Project: project.ID, Expected: []protocol.SessionID{two.ID, one.ID}, Sessions: []protocol.SessionID{one.ID, two.ID}}}
		ordered := commandFixture(t, node, command).Data.([]protocol.SessionID)
		if !reflect.DeepEqual(ordered, []protocol.SessionID{unassigned.ID, one.ID, two.ID}) {
			t.Fatalf("project order = %v", ordered)
		}
		stale := dispatchFixture(t, node, node.ID(), protocol.NewRequest(node.ID(), command))
		if stale.Fault == nil || stale.Fault.Code != protocol.ErrorCodeRevisionConflict {
			t.Fatalf("order conflict = %#v", stale)
		}
		bad := dispatchFixture(t, node, node.ID(), protocol.NewRequest(node.ID(), protocol.Command{Kind: "set_session_order", Data: protocol.CommandSetSessionOrderPayload{Project: project.ID, Expected: []protocol.SessionID{one.ID, two.ID}, Sessions: []protocol.SessionID{one.ID, one.ID}}}))
		if bad.Fault == nil || bad.Fault.Code != protocol.ErrorCodeInvalidRequest {
			t.Fatalf("duplicate order = %#v", bad)
		}
		values := snapshotFixture(t, node).Sessions
		if values[0].ID != unassigned.ID || values[1].ID != one.ID || values[2].ID != two.ID {
			t.Fatal("snapshot did not retain authority order")
		}
	})
}

// Over 1 s: each Node start installs and verifies the bundled packages.
func TestSnapshotCursor(t *testing.T) {
	t.Parallel()
	node := nodeFixture(t)
	ctx := context.Background()
	failures := make(chan error, 1)
	go func() {
		for revision := uint64(0); revision < 16; revision++ {
			request := protocol.NewRequest(node.ID(), protocol.Command{Kind: "save_power_settings", Data: protocol.PowerSettings{Revision: revision, PreventSleep: revision%2 == 0}})
			accepted, err := node.Dispatch(ctx, node.ID(), request)
			if err != nil {
				failures <- err
				return
			}
			result := <-accepted.Completion
			if result.Fault != nil {
				failures <- result.Fault
				return
			}
		}
		failures <- nil
	}()
	for index := 0; index < 16; index++ {
		value := snapshotFixture(t, node)
		if value.Cursor != value.PowerSettingsRevision {
			t.Fatalf("snapshot mixed commits: cursor %d revision %d", value.Cursor, value.PowerSettingsRevision)
		}
	}
	if err := <-failures; err != nil {
		t.Fatal(err)
	}
	if value := snapshotFixture(t, node); value.Cursor != 16 || value.PowerSettingsRevision != 16 {
		t.Fatalf("final cursor = %d revision = %d", value.Cursor, value.PowerSettingsRevision)
	}
	t.Run("stored activity", func(t *testing.T) {
		node := nodeFixture(t)
		session := sessionFixture(t, node)
		caller := node.ID()
		body := func(value any) []byte {
			t.Helper()
			encoded, err := json.Marshal(value)
			if err != nil {
				t.Fatal(err)
			}
			return encoded
		}
		type statement struct {
			query string
			args  []any
		}
		write := func(statements ...statement) {
			t.Helper()
			if err := node.profile.Store.Node.Write(ctx, func(db *sql.Conn) error {
				for _, item := range statements {
					if _, err := db.ExecContext(ctx, item.query, item.args...); err != nil {
						return err
					}
				}
				return nil
			}); err != nil {
				t.Fatal(err)
			}
		}
		activity := func(id protocol.SessionID) protocol.ActivitySummary {
			t.Helper()
			for _, value := range snapshotFixture(t, node).Sessions {
				if value.ID == id {
					return value.Activity
				}
			}
			t.Fatal("session missing from snapshot")
			return protocol.ActivitySummary{}
		}
		statuses := []protocol.ConversationStatus{protocol.ConversationStatusRunning, protocol.ConversationStatusCompleted, protocol.ConversationStatusQueued, protocol.ConversationStatusRunning}
		runs := make([]protocol.ConversationRun, len(statuses))
		requests := make([]protocol.Request, len(statuses))
		statements := []statement{}
		for index, status := range statuses {
			run := protocol.ConversationRun{Worktree: session.Worktree, Turn: protocol.TurnID{15: byte(index + 1)}, Kind: protocol.ConversationRunKindTask, Session: session.ID, Sequence: uint64(index + 1), Revision: session.Revision, Status: status, Notices: []protocol.ConversationBackgroundNotice{}}
			if index == 3 {
				run.Joined = &runs[0].Turn
			}
			runs[index] = run
			request := protocol.Request{Version: 1, ID: protocol.RequestID{15: byte(index + 1)}, Target: node.ID(), Command: protocol.Command{Kind: "submit_turn", Data: protocol.CommandSubmitTurnPayload{Session: session.ID, ExpectedRevision: session.Revision, Message: protocol.ConversationInput{Text: " First\n task "}}}}
			requests[index] = request
			statements = append(statements,
				statement{"INSERT INTO requests(caller,id,body,status) VALUES(?1,?2,?3,'admitted')", []any{caller[:], request.ID.String(), body(request)}},
				statement{"INSERT INTO turns(rowid,id,kind,plugins,roles,session,revision,request,caller) VALUES(?1,?2,'task',X'5B5D',?3,?4,?5,?6,?7)", []any{run.Sequence, run.Turn.String(), body(session.Roles), session.ID.String(), session.Revision, request.ID.String(), caller[:]}},
				statement{"INSERT INTO conversation_turns(session,turn) VALUES(?1,?2)", []any{session.ID.String(), run.Turn.String()}},
				statement{"INSERT INTO agent_runs(turn,ready,body,hooks) VALUES(?1,0,?2,?3)", []any{run.Turn.String(), body(run), body(protocol.DefaultHooksSettings())}},
			)
		}
		approval := protocol.ConversationApproval{ID: protocol.ApprovalID{15: 1}, Session: session.ID, Turn: runs[0].Turn, Entry: "approval-entry", State: protocol.ConversationApprovalStatePending, Source: protocol.ConversationApprovalSourceUser}
		question := protocol.ConversationQuestion{ID: protocol.QuestionID{15: 1}, Session: session.ID, Turn: runs[0].Turn, Entry: "question-entry", State: protocol.ConversationQuestionState{Kind: "pending"}}
		binding := map[string]any{"owner": protocol.PluginReference{Name: "fixture", Digest: strings.Repeat("a", 64), SettingsRevision: 1}, "tool": "fixture", "operation": nil}
		statements = append(statements,
			statement{"INSERT INTO agent_approvals(id,turn,call,body,binding,request,claimed) VALUES(?1,?2,'approval-call',?3,?4,?5,0)", []any{approval.ID.String(), approval.Turn.String(), body(approval), body(binding), string(body(map[string]any{"name": "fixture", "arguments": map[string]any{}}))}},
			statement{"INSERT INTO agent_questions(id,turn,call,body) VALUES(?1,?2,'question-call',?3)", []any{question.ID.String(), question.Turn.String(), body(question)}},
			statement{"INSERT INTO agent_pending(turn,position,revision,request,caller) VALUES(?1,1,1,?2,?3)", []any{runs[2].Turn.String(), requests[2].ID.String(), caller[:]}},
			statement{"INSERT INTO session_attention(session,revision,unread) VALUES(?1,7,1)", []any{session.ID.String()}},
		)
		write(statements...)
		for _, step := range []struct {
			name     string
			status   protocol.ConversationStatus
			approved bool
			next     bool
			selected int
			waiting  protocol.ActivityWaiting
			queued   uint32
		}{
			{"approval precedes question and newer outcomes", protocol.ConversationStatusRunning, false, false, 0, protocol.ActivityWaitingApproval, 1},
			{"resolved approval reveals question", protocol.ConversationStatusRunning, true, false, 0, protocol.ActivityWaitingInput, 1},
			{"stopping still waits for input", protocol.ConversationStatusStopping, true, false, 0, protocol.ActivityWaitingInput, 1},
			{"latest finished outcome precedes queue", protocol.ConversationStatusCompleted, true, false, 1, "", 1},
			{"next started turn replaces outcome", protocol.ConversationStatusCompleted, true, true, 2, "", 0},
		} {
			t.Run(step.name, func(t *testing.T) {
				runs[0].Status = step.status
				approval.State = protocol.ConversationApprovalStatePending
				if step.approved {
					approval.State = protocol.ConversationApprovalStateApproved
				}
				changes := []statement{
					{"UPDATE agent_runs SET body=?2 WHERE turn=?1", []any{runs[0].Turn.String(), body(runs[0])}},
					{"UPDATE agent_approvals SET body=?2 WHERE id=?1", []any{approval.ID.String(), body(approval)}},
				}
				if step.next {
					runs[2].Status = protocol.ConversationStatusRunning
					changes = append(changes,
						statement{"UPDATE agent_runs SET body=?2 WHERE turn=?1", []any{runs[2].Turn.String(), body(runs[2])}},
						statement{"DELETE FROM agent_pending WHERE turn=?1", []any{runs[2].Turn.String()}},
					)
				}
				write(changes...)
				want := protocol.ActivitySummary{Title: "First task", Attention: protocol.ActivityAttention{Revision: 7, Unread: true}, Run: &runs[step.selected], Queued: step.queued}
				if step.waiting != "" {
					want.Waiting = &step.waiting
				}
				if got := activity(session.ID); !reflect.DeepEqual(got, want) {
					t.Fatalf("stored activity = %#v; want %#v", got, want)
				}
			})
		}
		fork := sessionFixture(t, node)
		write(statement{"INSERT INTO conversation_turns(session,turn) VALUES(?1,?2)", []any{fork.ID.String(), runs[2].Turn.String()}})
		projected := activity(fork.ID)
		if projected.Run == nil || projected.Run.Session != fork.ID || projected.Run.Origin == nil || *projected.Run.Origin != session.ID || projected.Run.Turn != runs[2].Turn {
			t.Fatalf("inherited activity projection = %#v", projected)
		}
		canonical := map[string]any{
			"id": "stored-user-input", "created": "2026-01-01T00:00:00Z", "turn": runs[0].Turn,
			"provenance": map[string]any{"author": "user", "branch": ""},
			"metadata":   map[string]any{"input": map[string]any{"text_part": 2}},
			"kind":       "message", "role": "user", "part_count": 3,
			"content": []any{map[string]any{"index": 0, "type": "text", "text": "Context"}, map[string]any{"index": 2, "type": "text", "text": " Canonical\n task "}},
		}
		requests[0].Command = protocol.Command{Kind: "resolve_question", Data: protocol.CommandResolveQuestionPayload{Session: session.ID, Question: question.ID, Response: protocol.ConversationQuestionResponse{Kind: "cancel"}}}
		write(
			statement{"UPDATE requests SET body=?3 WHERE caller=?1 AND id=?2", []any{caller[:], requests[0].ID.String(), body(requests[0])}},
			statement{"INSERT INTO agent_events(sequence,session,id,turn,body) VALUES(1,?1,'stored-user-input',?2,?3)", []any{session.ID.String(), runs[0].Turn.String(), body(canonical)}},
		)
		if got := activity(session.ID).Title; got != "Canonical task" {
			t.Fatalf("canonical indexed input title = %q", got)
		}
		requests[0].Command = protocol.Command{Kind: "resolve_question", Data: protocol.CommandResolveQuestionPayload{Session: session.ID, Question: question.ID, Response: protocol.ConversationQuestionResponse{Kind: "start_coding", Data: protocol.ConversationQuestionResponseStartCodingPayload{ExpectedRevision: session.Revision, Message: protocol.ConversationInput{Text: " Resolved\n task "}}}}}
		write(statement{"UPDATE requests SET body=?3 WHERE caller=?1 AND id=?2", []any{caller[:], requests[0].ID.String(), body(requests[0])}})
		if got := activity(session.ID).Title; got != "Resolved task" {
			t.Fatalf("resolved input title = %q", got)
		}
		write(statement{"INSERT INTO agent_state(owner,key,body) VALUES(?1,'sailry.session_title',?2)", []any{session.ID.String(), body(" Assigned\n title ")}})
		if got := activity(session.ID).Title; got != "Assigned title" {
			t.Fatalf("assigned title = %q", got)
		}
	})
}

// Over 1 s: each Node start installs and verifies the bundled packages.
func TestResetRequiredRecovers(t *testing.T) {
	t.Parallel()
	node := nodeFixture(t)
	ctx := context.Background()
	subscription, err := node.Subscribe(ctx, node.ID(), protocol.Topic{Kind: "node"})
	if err != nil {
		t.Fatal(err)
	}
	defer subscription.Close()
	initial, err := subscription.Next(ctx)
	if err != nil || initial.Kind != "snapshot" {
		t.Fatalf("initial = %#v %v", initial, err)
	}
	changes := make([]protocol.Event, 130)
	for index := range changes {
		changes[index] = protocol.Event{Kind: "power_settings_changed", Data: protocol.EventPowerSettingsChangedPayload{Revision: uint64(index + 1)}}
	}
	if err := node.profile.Store.Node.Write(ctx, func(db *sql.Conn) error { _, err := store.RecordEvents(ctx, db, node.ID(), changes); return err }); err != nil {
		t.Fatal(err)
	}
	node.publish()
	for cursor := uint64(1); cursor <= 128; cursor++ {
		update, err := subscription.Next(ctx)
		if err != nil || update.Kind != "event" || update.Data.(protocol.EventEnvelope).Cursor != cursor {
			t.Fatalf("catch-up cursor %d = %#v %v", cursor, update, err)
		}
	}
	if err := node.profile.Store.Node.Write(ctx, func(db *sql.Conn) error {
		_, err := db.ExecContext(ctx, "DELETE FROM events WHERE cursor=129")
		return err
	}); err != nil {
		t.Fatal(err)
	}
	reset, err := subscription.Next(ctx)
	if err != nil || reset.Kind != "reset_required" {
		t.Fatalf("gap = %#v %v", reset, err)
	}
	fresh, err := node.Subscribe(ctx, node.ID(), protocol.Topic{Kind: "node"})
	if err != nil {
		t.Fatal(err)
	}
	defer fresh.Close()
	update, err := fresh.Next(ctx)
	if err != nil || update.Data.(protocol.Snapshot).Cursor != 130 {
		t.Fatalf("recovery = %#v %v", update, err)
	}
	commandFixture(t, node, protocol.Command{Kind: "save_power_settings", Data: protocol.PowerSettings{PreventSleep: false}})
	update, err = fresh.Next(ctx)
	if err != nil || update.Kind != "event" || update.Data.(protocol.EventEnvelope).Cursor != 131 {
		t.Fatalf("resumed stream = %#v %v", update, err)
	}
}

func startFixtureNode(ctx context.Context, path string) (*Node, error) {
	environment := process.NewEnvironment([]string{"PATH=/usr/bin:/bin"})
	return StartWithOptions(ctx, path, Options{Environment: &environment, MCP: fixtureMCPOptions()})
}

func fixtureMCPOptions() *mcp.Options {
	return &mcp.Options{Open: func(context.Context, mcp.Launch, func(context.Context, *sdk.ElicitRequest) (*sdk.ElicitResult, error)) (*sdk.ClientSession, error) {
		return nil, &protocol.Fault{Code: protocol.ErrorCodeUnavailable, Message: "fixture MCP server is unavailable"}
	}}
}

// secretCommands builds one request of every command the ledger keeps by
// digest, each carrying the given text where its secret belongs.
func secretCommands(canary string) map[string]protocol.Command {
	secret := protocol.NewSecret(canary)
	replace := protocol.PluginSettingsSecretUpdate{Kind: "replace", Data: &secret}
	provider := protocol.DefaultConversationProvider()
	provider.ID, provider.Name, provider.Api = protocol.NewProviderID(), "Ledger", protocol.ConversationModelApiChatCompletions
	provider.Authentication, provider.Endpoint = protocol.AuthenticationApiKey, "https://ledger.invalid/v1"
	database := protocol.DefaultCommandSaveDatabasePayload()
	database.Profile.ID, database.Profile.Name, database.Password = protocol.NewDatabaseID(), "Ledger", &secret
	database.Profile.Connection = protocol.DatabaseConnection{Kind: "network", Data: &protocol.DatabaseConnectionNetworkPayload{
		Engine: protocol.DatabaseEnginePostgres, Host: "ledger.invalid", Port: 5432, Database: "app", Username: "reader", Tls: protocol.DatabaseTlsRequire,
	}}
	ssh := protocol.DefaultCommandSaveSshPayload()
	ssh.Profile.ID, ssh.Profile.Name, ssh.Profile.Host, ssh.Profile.Username = protocol.NewSshID(), "Ledger", "ledger.invalid", "ledger"
	ssh.Profile.Authentication = protocol.SshAuthenticationPassword
	ssh.Credential = &protocol.SshCredential{Kind: "password", Data: &protocol.SshCredentialPasswordPayload{Password: secret}}
	search := protocol.DefaultSearchUpdate()
	search.Credentials = map[protocol.SearchDriver]protocol.SearchCredentials{protocol.SearchDriverExa: {Key: replace, Basic: protocol.PluginSettingsSecretUpdate{Kind: "keep"}}}
	backup := protocol.DefaultBackupUpdate()
	keep := protocol.PluginSettingsSecretUpdate{Kind: "keep"}
	backup.Identity, backup.Secret, backup.SessionToken, backup.Passphrase = keep, keep, keep, replace
	settings := protocol.DefaultCommandSavePluginSettingsPayload()
	settings.Package.Name, settings.Secrets = "ledger", map[string]protocol.PluginSettingsSecretUpdate{"token": replace}
	servers := protocol.DefaultCommandSavePluginMcpPayload()
	servers.Package.Name = "ledger"
	servers.Configuration.Servers = map[string]protocol.PluginMcpServer{"ledger": {Kind: "stdio", Data: &protocol.PluginMcpServerStdioPayload{
		Command: "ledger", Args: []string{}, Env: map[string]string{"TOKEN": canary},
	}}}
	install := protocol.DefaultCommandInstallMcpPayload()
	install.Name, install.Secrets = "ledger", map[string]protocol.PluginSettingsSecretUpdate{"token": replace}
	definition := protocol.DefaultPluginMcpDefinitionStdioPayload()
	definition.Command = "ledger"
	install.Definition = protocol.PluginMcpDefinition{Kind: "stdio", Data: &definition}
	input := protocol.DefaultPluginConnectorInput()
	input.Secrets = map[string]protocol.Secret{"token": secret}
	login := protocol.DefaultCommandStartConnectorLoginPayload()
	login.Package.Name, login.Connector, login.Method, login.Input = "ledger", "ledger", protocol.PluginConnectorKindSecret, input
	restore := protocol.DefaultCommandRestoreBackupPayload()
	restore.ID, restore.Passphrase, restore.Location = "ledger", &secret, protocol.BackupLocationLocal
	file := protocol.DefaultCommandRestoreBackupFilePayload()
	file.Passphrase = &secret
	handler := protocol.DefaultDispatchHandler()
	handler.Name, handler.Queue = "ledger", "ledger"
	handler.Callback.Completion, handler.Callback.Command = protocol.DispatchCompletionCommand, &protocol.Command{Kind: "save_database", Data: &database}
	schedule := protocol.DispatchCommand{Kind: "save_handler", Data: &handler}
	dispatch := protocol.DefaultCommandDispatchPayload()
	dispatch.Package.Name, dispatch.Action = "ledger", schedule
	config := protocol.DefaultSessionConfig()
	config.Provider, config.Model, config.Mode, config.Permission = provider.ID, "fixture", protocol.WorkModeCode, protocol.PermissionProject
	imported := protocol.DefaultSessionImport()
	imported.Source, imported.Config, imported.Provider, imported.Secret = protocol.NodeID{1}, config, provider, &secret
	return map[string]protocol.Command{
		"put_credential":          {Kind: "put_credential", Data: &protocol.CommandPutCredentialPayload{ID: protocol.NewCredentialID(), Provider: provider.ID, Secret: secret}},
		"save_provider":           {Kind: "save_provider", Data: &protocol.CommandSaveProviderPayload{Provider: provider, Secret: &secret}},
		"complete_provider_login": {Kind: "complete_provider_login", Data: &protocol.CommandCompleteProviderLoginPayload{Attempt: protocol.NewRequestID(), Callback: secret}},
		"import_session":          {Kind: "import_session", Data: &imported},
		"save_ssh":                {Kind: "save_ssh", Data: &ssh},
		"save_database":           {Kind: "save_database", Data: &database},
		"save_search_settings":    {Kind: "save_search_settings", Data: &search},
		"save_plugin_settings":    {Kind: "save_plugin_settings", Data: &settings},
		"save_plugin_mcp":         {Kind: "save_plugin_mcp", Data: &servers},
		"install_mcp":             {Kind: "install_mcp", Data: &install},
		"start_connector_login":   {Kind: "start_connector_login", Data: &login},
		"submit_connector_input":  {Kind: "submit_connector_input", Data: &protocol.CommandSubmitConnectorInputPayload{Attempt: protocol.NewRequestID(), Input: secret}},
		"update_connection":       {Kind: "update_connection", Data: &protocol.CommandUpdateConnectionPayload{Connection: protocol.NewRequestID(), Input: input}},
		"save_backup_settings":    {Kind: "save_backup_settings", Data: &backup},
		"restore_backup":          {Kind: "restore_backup", Data: &restore},
		"restore_backup_file":     {Kind: "restore_backup_file", Data: &file},
		"dispatch":                {Kind: "dispatch", Data: &dispatch},
		"plugin_transaction": {Kind: "plugin_transaction", Data: &protocol.CommandPluginTransactionPayload{
			Operations: []protocol.PluginTransactionOperation{{Kind: "dispatch", Data: &schedule}},
		}},
	}
}
