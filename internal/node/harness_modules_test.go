package node

import (
	"context"
	"database/sql"
	"encoding/json"
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"slices"
	"strings"
	"sync"
	"testing"
	"time"

	"charm.land/fantasy"
	"github.com/jonboulle/clockwork"
	hostassembly "github.com/sailry/sailry-harness/internal/assembly/host"
	"github.com/sailry/sailry-harness/internal/node/agent/model"
	"github.com/sailry/sailry-harness/internal/node/plugins"
	"github.com/sailry/sailry-harness/internal/node/process"
	"github.com/sailry/sailry-harness/internal/node/providers"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
	providerstatusnode "github.com/sailry/sailry-harness/modules/provider-status/node"
	"lukechampine.com/blake3"
)

func startHarnessModuleNode(t *testing.T, clock clockwork.Clock, modelFn func(context.Context, model.Binding) (fantasy.LanguageModel, error)) *Node {
	t.Helper()
	environment := process.NewEnvironment([]string{"PATH=/usr/bin:/bin", "HOME=" + t.TempDir()})
	opts := Options{
		ProductID:     "harness",
		Headless:      true,
		Assembly:      hostassembly.Harness(),
		DispatchClock: clock,
		Environment:   &environment,
		MCP:           fixtureMCPOptions(),
		Model:         modelFn,
		Process: process.Options{
			After: func(time.Duration) <-chan time.Time { return make(chan time.Time) },
			Run: func(context.Context, process.Launch, func(protocol.ProcessOutputOutputStream, []byte), <-chan []byte) (protocol.ProcessCompletion, error) {
				return protocol.ProcessCompletion{}, errors.New("test must not launch processes")
			},
		},
	}
	owner, err := StartWithOptions(t.Context(), filepath.Join(t.TempDir(), "profile"), opts)
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() {
		if err := owner.Stop(context.Background()); err != nil {
			t.Error(err)
		}
	})
	return owner
}

type harnessToolScript struct {
	fantasy.LanguageModel
	mu          sync.Mutex
	calls       [][2]string // [toolName, argumentsJSON]
	results     []string
	seenPrompts []string
}

func (s *harnessToolScript) Stream(_ context.Context, call fantasy.Call) (fantasy.StreamResponse, error) {
	s.mu.Lock()
	defer s.mu.Unlock()
	var promptBuf strings.Builder
	for _, msg := range call.Prompt {
		for _, part := range msg.Content {
			if txt, ok := fantasy.AsMessagePart[fantasy.TextPart](part); ok {
				promptBuf.WriteString(txt.Text)
				promptBuf.WriteByte('\n')
			}
			if res, ok := fantasy.AsMessagePart[fantasy.ToolResultPart](part); ok && strings.HasPrefix(res.ToolCallID, "harness-call-") {
				if txt, ok := fantasy.AsToolResultOutputType[fantasy.ToolResultOutputContentText](res.Output); ok {
					s.results = append(s.results, txt.Text)
				}
			}
		}
	}
	s.seenPrompts = append(s.seenPrompts, promptBuf.String())

	part := fantasy.StreamPart{Type: fantasy.StreamPartTypeTextDelta, ID: "answer", Delta: "Done"}
	if len(s.calls) > 0 {
		targetName, arguments := s.calls[0][0], s.calls[0][1]
		callID := fmt.Sprintf("harness-call-%d", len(s.results))
		if slices.ContainsFunc(call.Tools, func(tool fantasy.Tool) bool { return tool.GetName() == targetName }) {
			part = fantasy.StreamPart{Type: fantasy.StreamPartTypeToolCall, ID: callID, ToolCallName: targetName, ToolCallInput: arguments}
			s.calls = s.calls[1:]
		} else if slices.Contains(callableTools(call), targetName) {
			part = fantasy.StreamPart{Type: fantasy.StreamPartTypeToolCall, ID: callID, ToolCallName: "call_tool", ToolCallInput: `{"name":"` + targetName + `","arguments":` + arguments + `}`}
			s.calls = s.calls[1:]
		} else {
			part = fantasy.StreamPart{Type: fantasy.StreamPartTypeToolCall, ID: "discover", ToolCallName: "tool_search", ToolCallInput: `{"query":"select:` + targetName + `"}`}
		}
	}
	return func(yield func(fantasy.StreamPart) bool) {
		if yield(part) {
			yield(fantasy.StreamPart{Type: fantasy.StreamPartTypeFinish, FinishReason: fantasy.FinishReasonStop})
		}
	}, nil
}

func runAndApproveSession(t *testing.T, owner *Node, start protocol.ConversationStart) protocol.SessionID {
	t.Helper()
	ctx := t.Context()
	sub, err := owner.Subscribe(ctx, owner.ID(), protocol.Topic{Kind: "node"})
	if err != nil {
		t.Fatal(err)
	}
	defer sub.Close()
	sessionID := commandFixture(t, owner, protocol.Command{Kind: "start_session", Data: start}).Data.(protocol.QueuedTurn).Session
	for completed := false; !completed; {
		var history protocol.ConversationHistory
		if err := owner.profile.Store.Node.Read(ctx, func(db *sql.Conn) error {
			var err error
			history, err = store.ReadConversation(ctx, db, sessionID, nil, 100)
			return err
		}); err != nil {
			t.Fatal(err)
		}
		for _, approval := range history.Page.Approvals {
			if approval.State == protocol.ConversationApprovalStatePending {
				commandFixture(t, owner, protocol.Command{
					Kind: "resolve_approval",
					Data: protocol.CommandResolveApprovalPayload{
						Session:  sessionID,
						Approval: approval.ID,
						Decision: protocol.ConversationDecision{Kind: "approve"},
					},
				})
			}
		}
		for _, run := range history.Page.Runs {
			switch run.Status {
			case protocol.ConversationStatusCompleted:
				completed = true
			case protocol.ConversationStatusFailed, protocol.ConversationStatusInterrupted, protocol.ConversationStatusCancelled:
				t.Fatalf("conversation turn failed: %+v", run)
			}
		}
		if !completed {
			if _, err := sub.Next(ctx); err != nil {
				t.Fatal(err)
			}
		}
	}
	return sessionID
}

func seedFixtureModelProvider(t *testing.T, owner *Node) protocol.ConversationProvider {
	t.Helper()
	ctx := t.Context()
	provider := protocol.DefaultConversationProvider()
	provider.ID = protocol.NewProviderID()
	provider.Revision = 1
	provider.Enabled = true
	provider.Name = "Fixture"
	provider.Api = protocol.ConversationModelApiChatCompletions
	provider.Authentication = protocol.AuthenticationApiKey
	provider.Endpoint = "http://fixture.invalid/v1"
	provider.DefaultModel = "fixture"
	m := protocol.DefaultConversationModel()
	m.ID, m.Context, m.Output, m.Tools = "fixture", 32000, 8000, true
	provider.Models = []protocol.ConversationModel{m}
	if err := owner.profile.Store.Node.Write(ctx, func(db *sql.Conn) error {
		body, err := json.Marshal(map[string]any{
			"id": provider.ID, "provider": provider, "listed": true, "authorizations": map[string]any{}, "quota": nil,
		})
		if err != nil {
			return err
		}
		_, err = db.ExecContext(ctx, "INSERT INTO providers(id,body) VALUES(?1,?2)", provider.ID.String(), body)
		return err
	}); err != nil {
		t.Fatal(err)
	}
	return provider
}

func TestHarnessProviderStatusAccountUsage(t *testing.T) {
	t.Parallel()
	owner := startHarnessModuleNode(t, clockwork.NewFakeClockAt(time.Date(2026, 4, 15, 12, 0, 0, 0, time.UTC)), nil)
	ctx := t.Context()

	// Install a minimal claude_oauth auth fixture so provider login capture and
	// renewal validation run through the real store lifecycle.
	authDir := t.TempDir()
	authDocs := map[string]string{
		"plugin.json":                 `{"$schema":"` + plugins.ManifestSchema + `","name":"claude-auth-fixture","version":"0.1.0","extensions":{"dev.sailry.platform":{"api_version":"v1","actions":["models.authenticate","credentials.inspect"],"model_auth":[{"id":"claude","authentication":"claude_oauth","display":{"label":"Claude OAuth"},"client":{"client_id":"claude_fixture"},"scopes":["user:inference"],"flow":{"kind":"script","script":{"entry":"dev.sailry.platform/auth.js","resources":[]},"handler":"step","refresh":true,"requests":{"token":{"url":"https://auth.example.test/token","method":"GET"}}}}]}}}`,
		"dev.sailry.platform/auth.js": `export function step() { return {kind:"reject",code:"invalid_request"}; }`,
	}
	for name, body := range authDocs {
		p := filepath.Join(authDir, filepath.FromSlash(name))
		if err := os.MkdirAll(filepath.Dir(p), 0o700); err != nil {
			t.Fatal(err)
		}
		if err := os.WriteFile(p, []byte(body), 0o600); err != nil {
			t.Fatal(err)
		}
	}
	authInfo, err := owner.plugins.manager.Install(owner.pluginContext(ctx), authDir, "", "claude-auth-fixture")
	if err != nil || len(authInfo.Issues) != 0 {
		t.Fatalf("claude auth fixture: %v, %+v", err, authInfo.Issues)
	}
	if err := owner.profile.Store.Node.Write(ctx, func(db *sql.Conn) error {
		_, err := store.InstallPlugin(ctx, db, authInfo, 0, true)
		return err
	}); err != nil {
		t.Fatal(err)
	}

	// Create and authorize a Claude OAuth provider through the real store APIs.
	oauthProv := protocol.DefaultConversationProvider()
	oauthProv.ID = protocol.NewProviderID()
	oauthProv.Enabled = true
	oauthProv.Name = "Claude OAuth"
	oauthProv.Api = protocol.ConversationModelApiAnthropic
	oauthProv.Authentication = protocol.AuthenticationClaudeOAuth
	oauthProv.Endpoint = "https://api.anthropic.com"
	oauthProv.DefaultModel = "claude-sonnet"
	oauthModel := protocol.DefaultConversationModel()
	oauthModel.ID, oauthModel.Context, oauthModel.Output, oauthModel.Tools = "claude-sonnet", 200000, 8000, true
	oauthProv.Models = []protocol.ConversationModel{oauthModel}
	oauthProv = commandFixture(t, owner, protocol.Command{
		Kind: "save_provider",
		Data: &protocol.CommandSaveProviderPayload{Provider: oauthProv, ExpectedRevision: 0},
	}).Data.(protocol.ConversationProvider)

	futureExpiry := uint64(time.Now().Add(2 * time.Hour).UnixMilli())
	if err := owner.profile.Store.Node.Write(ctx, func(db *sql.Conn) error {
		loginCapture, err := store.CaptureProviderLogin(ctx, db, oauthProv.ID, oauthProv.Revision, protocol.NewRequestID())
		if err != nil {
			return err
		}
		_, err = store.CompleteProviderLogin(ctx, db, owner.ID(), loginCapture, providers.AccountGrant{
			Kind:        protocol.AuthenticationClaudeOAuth,
			Access:      "oauth-access-token-fixture",
			Refresh:     "oauth-refresh-token-fixture",
			ClientID:    "claude_fixture",
			Subject:     "subject-fixture",
			Scopes:      []string{"user:inference"},
			ExpiresAtMs: futureExpiry,
		})
		return err
	}); err != nil {
		t.Fatalf("CompleteProviderLogin: %v", err)
	}
	// Reload the updated provider revision after credential completion.
	for _, p := range commandFixture(t, owner, protocol.Command{Kind: "list_providers"}).Data.([]protocol.ConversationProvider) {
		if p.ID == oauthProv.ID {
			oauthProv = p
			break
		}
	}

	// Pre-populate the provider resource cache using the exact native key so the
	// command exercises CaptureNativeProviderResource, prepareProviderHTTP, cache
	// lookup, CheckProviderResource, and DecodeAccountUsage without network I/O.
	var capture store.ProviderResourceCapture
	if err := owner.profile.Store.Node.Read(ctx, func(db *sql.Conn) error {
		var err error
		capture, err = store.CaptureNativeProviderResource(ctx, db, owner.ID(), oauthProv.ID, oauthProv.Revision, providerstatusnode.EndpointFor)
		return err
	}); err != nil {
		t.Fatalf("CaptureNativeProviderResource: %v", err)
	}
	prepared, err := prepareProviderHTTP(ctx, capture, nil)
	if err != nil {
		t.Fatalf("prepareProviderHTTP: %v", err)
	}
	keyBody, err := json.Marshal([]any{"provider-status", prepared.URL.String(), prepared.Method, prepared.Headers, prepared.Body, capture.Credential.ID, capture.Credential.Revision})
	if err != nil {
		t.Fatal(err)
	}
	cacheKey := blake3.Sum256(keyBody)
	owner.auth.mu.Lock()
	owner.auth.resources[cacheKey] = &providerDocument{
		result: protocol.PluginHttpResponse{
			Status: 200,
			Body: `{
				"five_hour": {"utilization": 35, "resets_at": "2026-04-15T17:00:00Z"},
				"seven_day": {"utilization": "12.5", "resets_at": "2026-04-22T00:00:00Z"},
				"extra_usage": {"used_credits": 250, "monthly_limit": 1000, "currency": "USD", "is_enabled": true}
			}`,
		},
		fetched: time.Now(),
		used:    time.Now(),
	}
	owner.auth.mu.Unlock()

	// Dispatch read_provider_account_usage and verify decoded windows + extra usage.
	out := commandFixture(t, owner, protocol.Command{
		Kind: "read_provider_account_usage",
		Data: &protocol.CommandReadProviderAccountUsagePayload{
			Provider:         oauthProv.ID,
			ExpectedRevision: oauthProv.Revision,
		},
	})
	if out.Kind != "provider_account_usage" {
		t.Fatalf("output kind = %q", out.Kind)
	}
	usage := out.Data.(protocol.ProviderAccountUsage)
	if len(usage.Windows) != 2 || usage.Windows[0].Key != "five_hour" || *usage.Windows[0].Used != 35 ||
		usage.Windows[1].Key != "seven_day" || *usage.Windows[1].Used != 12.5 {
		t.Fatalf("decoded windows = %+v", usage.Windows)
	}
	if usage.Extra == nil || *usage.Extra.Used != 250 || *usage.Extra.Limit != 1000 || *usage.Extra.Currency != "USD" || !*usage.Extra.Enabled {
		t.Fatalf("decoded extra = %+v", usage.Extra)
	}

	// Second call reuses the cached response cleanly.
	again := commandFixture(t, owner, protocol.Command{
		Kind: "read_provider_account_usage",
		Data: &protocol.CommandReadProviderAccountUsagePayload{
			Provider:         oauthProv.ID,
			ExpectedRevision: oauthProv.Revision,
		},
	}).Data.(protocol.ProviderAccountUsage)
	if len(again.Windows) != 2 || *again.Windows[0].Used != 35 {
		t.Fatalf("cached repeat usage = %+v", again)
	}

	// Stale revision fails with revision_conflict.
	staleResp := dispatchFixture(t, owner, owner.ID(), protocol.NewRequest(owner.ID(), protocol.Command{
		Kind: "read_provider_account_usage",
		Data: &protocol.CommandReadProviderAccountUsagePayload{
			Provider:         oauthProv.ID,
			ExpectedRevision: oauthProv.Revision + 99,
		},
	}))
	if staleResp.Fault == nil || staleResp.Fault.Code != protocol.ErrorCodeRevisionConflict {
		t.Fatalf("expected revision_conflict, got %+v", staleResp)
	}

	// Non-OAuth API key provider is denied.
	apiProv := protocol.DefaultConversationProvider()
	apiProv.ID = protocol.NewProviderID()
	apiProv.Enabled = true
	apiProv.Name = "API Key Fixture"
	apiProv.Api = protocol.ConversationModelApiChatCompletions
	apiProv.Authentication = protocol.AuthenticationApiKey
	apiProv.Endpoint = "http://fixture.invalid/v1"
	apiProv.DefaultModel = "fixture"
	apiModel := protocol.DefaultConversationModel()
	apiModel.ID, apiModel.Context, apiModel.Output, apiModel.Tools = "fixture", 32000, 8000, true
	apiProv.Models = []protocol.ConversationModel{apiModel}
	apiKey := protocol.NewSecret("sk-test")
	apiProv = commandFixture(t, owner, protocol.Command{
		Kind: "save_provider",
		Data: &protocol.CommandSaveProviderPayload{Provider: apiProv, ExpectedRevision: 0, Secret: &apiKey},
	}).Data.(protocol.ConversationProvider)
	apiResp := dispatchFixture(t, owner, owner.ID(), protocol.NewRequest(owner.ID(), protocol.Command{
		Kind: "read_provider_account_usage",
		Data: &protocol.CommandReadProviderAccountUsagePayload{
			Provider:         apiProv.ID,
			ExpectedRevision: apiProv.Revision,
		},
	}))
	if apiResp.Fault == nil || apiResp.Fault.Code != protocol.ErrorCodePermissionDenied {
		t.Fatalf("expected permission_denied for api_key provider, got %+v", apiResp)
	}
}

func TestHarnessRemindersEndToEndAndLegacyContinuity(t *testing.T) {
	t.Parallel()
	clock := clockwork.NewFakeClockAt(time.Now().Truncate(time.Second))
	script := &harnessToolScript{}
	owner := startHarnessModuleNode(t, clock, func(context.Context, model.Binding) (fantasy.LanguageModel, error) {
		return script, nil
	})
	ctx := t.Context()

	// 1. Native reminders CRUD + fake DispatchClock schedule firing + notification delivery.
	remID := protocol.NewRequestID().String()
	dueMs := clock.Now().Add(10 * time.Minute).UnixMilli()
	saved := commandFixture(t, owner, protocol.Command{
		Kind: "save_reminder",
		Data: &protocol.CommandSaveReminderPayload{
			Reminder: protocol.Reminder{
				ID:      remID,
				Title:   "Native standup",
				Message: "Join standup call",
				DueMs:   &dueMs,
			},
			Revision: 0,
		},
	}).Data.(protocol.ReminderEntry)
	if saved.Revision == nil || *saved.Revision != 1 {
		t.Fatalf("saved reminder = %+v", saved)
	}

	readBack := commandFixture(t, owner, protocol.Command{
		Kind: "read_reminder",
		Data: &protocol.CommandReadReminderPayload{ID: remID},
	}).Data.(protocol.ReminderEntry)
	if readBack.Reminder.Title != "Native standup" {
		t.Fatalf("read_reminder = %+v", readBack)
	}

	// Advance clock past due time and wait for native deliver_reminder to complete.
	waitNotification := func(t *testing.T, handlerID, wantTitle string) {
		t.Helper()
		deadline := time.Now().Add(15 * time.Second)
		for {
			owner.CheckSchedules()
			var notices []protocol.NotificationNotice
			var failedJob *protocol.DispatchJob
			var completedJob bool
			if err := owner.profile.Store.Node.Read(ctx, func(db *sql.Conn) error {
				var err error
				notices, err = store.ReadNotifications(ctx, db)
				if err != nil {
					return err
				}
				rows, err := db.QueryContext(ctx, "SELECT body FROM dispatch_jobs WHERE package='reminders' AND json_extract(body,'$.handler')=?1", handlerID)
				if err != nil {
					return err
				}
				defer rows.Close()
				for rows.Next() {
					var body []byte
					var job protocol.DispatchJob
					if err := rows.Scan(&body); err != nil {
						return err
					}
					if err := json.Unmarshal(body, &job); err != nil {
						return err
					}
					switch job.Status {
					case protocol.DispatchStatusCompleted:
						completedJob = true
					case protocol.DispatchStatusFailed, protocol.DispatchStatusCancelled:
						cp := job
						failedJob = &cp
					}
				}
				return rows.Err()
			}); err != nil {
				t.Fatal(err)
			}
			if failedJob != nil {
				t.Fatalf("reminder job %s ended %s: %+v", handlerID, failedJob.Status, failedJob.Error)
			}
			if completedJob {
				for _, n := range notices {
					if n.Content.Title == wantTitle {
						return
					}
				}
			}
			if time.Now().After(deadline) {
				t.Fatalf("notification %q did not complete (completedJob=%v); got %+v", wantTitle, completedJob, notices)
			}
			time.Sleep(15 * time.Millisecond)
		}
	}
	clock.Advance(11 * time.Minute)
	waitNotification(t, remID, "Native standup")

	delivered := commandFixture(t, owner, protocol.Command{
		Kind: "read_reminder",
		Data: &protocol.CommandReadReminderPayload{ID: remID},
	}).Data.(protocol.ReminderEntry)
	if delivered.Reminder.NotifiedMs == nil || *delivered.Reminder.NotifiedMs < dueMs {
		t.Fatalf("reminder NotifiedMs not stamped: %+v", delivered.Reminder)
	}
	hist := commandFixture(t, owner, protocol.Command{
		Kind: "read_reminder_history",
		Data: &protocol.CommandReadReminderHistoryPayload{Handler: remID},
	}).Data.(protocol.ReminderHistory)
	if len(hist.Items) != 1 || hist.Items[0].Status != protocol.DispatchStatusCompleted {
		t.Fatalf("reminder history = %+v", hist)
	}
	commandFixture(t, owner, protocol.Command{
		Kind: "remove_reminder",
		Data: &protocol.CommandRemoveReminderPayload{ID: remID, Revision: *delivered.Revision},
	})

	// 2. Legacy reminders plugin call_plugin("notify") rewrite continuity after plugin retirement.
	legacyInfo := installPluginFixture(t, owner, "reminders")
	legacyScope := protocol.DefaultPluginContext()
	legacyScope.Package = plugins.PackageReference(legacyInfo)
	legacyID := protocol.NewRequestID().String()
	legacyDue := clock.Now().Add(30 * time.Minute).UnixMilli()
	saveBody, _ := json.Marshal(map[string]any{
		"id": legacyID, "revision": "0", "project": nil,
		"title": "Legacy migrated reminder", "message": "Survives plugin retirement",
		"completed": false, "due_ms": legacyDue,
	})
	saveReq := protocol.NewRequest(owner.ID(), protocol.Command{
		Kind: "call_plugin",
		Data: protocol.CommandCallPluginPayload{Handler: "save", Input: saveBody},
	})
	saveReq.Plugin = &legacyScope
	saveResp := dispatchFixture(t, owner, owner.ID(), saveReq)
	if saveResp.Fault != nil {
		t.Fatalf("legacy reminders save: %v", saveResp.Fault)
	}
	// Retire the legacy plugin.
	commandFixture(t, owner, protocol.Command{
		Kind: "remove_plugin",
		Data: protocol.CommandRemovePluginPayload{Name: legacyInfo.Summary.Name, ExpectedRevision: legacyInfo.Summary.Revision},
	})
	// Native read still sees the legacy record.
	migrated := commandFixture(t, owner, protocol.Command{
		Kind: "read_reminder",
		Data: &protocol.CommandReadReminderPayload{ID: legacyID},
	}).Data.(protocol.ReminderEntry)
	if migrated.Reminder.Title != "Legacy migrated reminder" {
		t.Fatalf("migrated reminder = %+v", migrated)
	}
	// Advance clock -> dispatch fires legacy call_plugin("notify") handler, rewritten to native deliver_reminder!
	clock.Advance(31 * time.Minute)
	waitNotification(t, legacyID, "Legacy migrated reminder")

	// 3. Agent tool scoping: create and list reminders scoped strictly to Project A.
	provider := seedFixtureModelProvider(t, owner)
	rootA, _ := filepath.EvalSymlinks(t.TempDir())
	rootB, _ := filepath.EvalSymlinks(t.TempDir())
	projA := commandFixture(t, owner, protocol.Command{Kind: "create_project", Data: protocol.ProjectsDraft{
		Name: "Alpha", Path: rootA, Source: protocol.ProjectsSource{Kind: "local"}, Appearance: protocol.DefaultProjectsAppearance(),
	}}).Data.(protocol.Project)
	projB := commandFixture(t, owner, protocol.Command{Kind: "create_project", Data: protocol.ProjectsDraft{
		Name: "Beta", Path: rootB, Source: protocol.ProjectsSource{Kind: "local"}, Appearance: protocol.DefaultProjectsAppearance(),
	}}).Data.(protocol.Project)
	// Seed a reminder in Project B that must stay hidden from Project A's turn.
	projBRemID := protocol.NewRequestID().String()
	commandFixture(t, owner, protocol.Command{
		Kind: "save_reminder",
		Data: &protocol.CommandSaveReminderPayload{
			Reminder: protocol.Reminder{ID: projBRemID, Project: &projB.ID, Title: "Beta secret"},
			Revision: 0,
		},
	})

	script.mu.Lock()
	script.calls = [][2]string{
		{"reminders", `{"action":"create","title":"Alpha task","message":"Check docs"}`},
		{"reminders", `{"action":"list"}`},
	}
	script.mu.Unlock()
	cfg := protocol.DefaultSessionConfig()
	cfg.Provider, cfg.Model, cfg.Mode, cfg.Permission = provider.ID, "fixture", protocol.WorkModeCode, protocol.PermissionAsk
	start := protocol.DefaultConversationStart()
	start.Project, start.Config, start.Title = &projA.ID, &cfg, "Reminders tool test"
	start.Message = protocol.ConversationInput{Text: "Add a reminder for Alpha"}
	start.Execution = protocol.ConversationExecutionModel
	runAndApproveSession(t, owner, start)

	script.mu.Lock()
	results := slices.Clone(script.results)
	script.mu.Unlock()
	if len(results) < 2 {
		t.Fatalf("expected 2 tool results, got %v", results)
	}
	if !strings.Contains(results[0], "Alpha task") {
		t.Fatalf("create result = %s", results[0])
	}
	if !strings.Contains(results[1], "Alpha task") || strings.Contains(results[1], "Beta secret") {
		t.Fatalf("list result leaked cross-project reminder: %s", results[1])
	}
}

func TestHarnessScheduledTasksEndToEndAndLegacyContinuity(t *testing.T) {
	t.Parallel()
	clock := clockwork.NewFakeClockAt(time.Now().Truncate(time.Second))
	script := &harnessToolScript{}
	owner := startHarnessModuleNode(t, clock, func(context.Context, model.Binding) (fantasy.LanguageModel, error) {
		return script, nil
	})
	ctx := t.Context()

	provider := seedFixtureModelProvider(t, owner)
	root, _ := filepath.EvalSymlinks(t.TempDir())
	proj := commandFixture(t, owner, protocol.Command{Kind: "create_project", Data: protocol.ProjectsDraft{
		Name: "Scheduler", Path: root, Source: protocol.ProjectsSource{Kind: "local"}, Appearance: protocol.DefaultProjectsAppearance(),
	}}).Data.(protocol.Project)
	cfg := protocol.DefaultSessionConfig()
	cfg.Provider, cfg.Model, cfg.Mode, cfg.Permission = provider.ID, "fixture", protocol.WorkModeCode, protocol.PermissionAsk
	worktree := commandFixture(t, owner, protocol.Command{
		Kind: "create_session",
		Data: protocol.CommandCreateSessionPayload{Project: &proj.ID, Config: &cfg},
	}).Data.(protocol.Session).Worktree

	waitTaskSession := func(t *testing.T, taskID string, wantJobs int) protocol.Session {
		t.Helper()
		deadline := time.Now().Add(30 * time.Second)
		for {
			owner.CheckSchedules()
			var jobs []protocol.DispatchJob
			var session protocol.Session
			var foundSession bool
			if err := owner.profile.Store.Node.Read(ctx, func(db *sql.Conn) error {
				rows, err := db.QueryContext(ctx, "SELECT body FROM dispatch_jobs WHERE package='scheduled-tasks' AND json_extract(body,'$.handler')=?1", taskID)
				if err != nil {
					return err
				}
				defer rows.Close()
				for rows.Next() {
					var body []byte
					var job protocol.DispatchJob
					if err := rows.Scan(&body); err != nil {
						return err
					}
					if err := json.Unmarshal(body, &job); err != nil {
						return err
					}
					jobs = append(jobs, job)
				}
				if err := rows.Err(); err != nil || len(jobs) < wantJobs {
					return err
				}
				for _, job := range jobs {
					if job.Status != protocol.DispatchStatusCompleted {
						return nil
					}
				}
				var sid string
				if err := db.QueryRowContext(ctx, "SELECT session FROM session_associations WHERE namespace='scheduled-tasks' AND kind='task' AND identity=?1 ORDER BY rowid DESC LIMIT 1", taskID).Scan(&sid); err != nil {
					return err
				}
				parsed, err := protocol.ParseSessionID(sid)
				if err != nil {
					return err
				}
				session, err = store.ReadSession(ctx, db, parsed)
				foundSession = err == nil
				return err
			}); err != nil {
				t.Fatal(err)
			}
			for _, job := range jobs {
				if job.Status == protocol.DispatchStatusFailed || job.Status == protocol.DispatchStatusCancelled {
					t.Fatalf("task %s job ended %s: %v", taskID, job.Status, job.Error)
				}
			}
			if foundSession {
				return session
			}
			if time.Now().After(deadline) {
				t.Fatalf("task %s did not complete %d jobs; got %+v", taskID, wantJobs, jobs)
			}
			time.Sleep(15 * time.Millisecond)
		}
	}

	// 1. Native CRUD + manual run_scheduled_task + scheduled fire via fake DispatchClock.
	taskID := protocol.NewRequestID().String()
	fireAt := clock.Now().Add(time.Hour).UnixMilli()
	saved := commandFixture(t, owner, protocol.Command{
		Kind: "save_scheduled_task",
		Data: &protocol.CommandSaveScheduledTaskPayload{
			Task: protocol.ScheduledTask{
				ID:        taskID,
				Name:      "Nightly build check",
				Prompt:    "Check build status",
				Project:   &proj.ID,
				Worktree:  &worktree,
				Config:    cfg,
				Execution: protocol.ConversationExecutionModel,
				Timing:    protocol.DispatchTiming{Kind: "once", Data: protocol.DispatchTimingOncePayload{AtMs: fireAt}},
				Enabled:   true,
				Queue:     "default",
			},
			Revision: 0,
		},
	}).Data.(protocol.ScheduledTaskEntry)
	if saved.Revision == nil || *saved.Revision != 1 {
		t.Fatalf("saved task = %+v", saved)
	}

	// Manual run immediately enqueues and starts an associated session.
	runJob := commandFixture(t, owner, protocol.Command{
		Kind: "run_scheduled_task",
		Data: &protocol.CommandRunScheduledTaskPayload{ID: taskID, Revision: saved.Revision},
	}).Data.(protocol.DispatchJob)
	if runJob.Handler != taskID {
		t.Fatalf("run_scheduled_task job = %+v", runJob)
	}
	manualSession := waitTaskSession(t, taskID, 1)
	if manualSession.Worktree != worktree || manualSession.Association == nil ||
		*manualSession.Association != (protocol.ConversationAssociation{Namespace: "scheduled-tasks", Kind: "task", ID: taskID}) {
		t.Fatalf("manual run session = %+v", manualSession)
	}
	hist := commandFixture(t, owner, protocol.Command{
		Kind: "read_scheduled_task_history",
		Data: &protocol.CommandReadScheduledTaskHistoryPayload{Handler: &taskID, Limit: 10},
	}).Data.(protocol.ScheduledTaskHistory)
	if len(hist.Items) != 1 || hist.Items[0].Session == nil || *hist.Items[0].Session != manualSession.ID || hist.Items[0].Turn == nil {
		t.Fatalf("manual run history = %+v", hist)
	}

	// Scheduled fire at fireAt creates a second run + associated session.
	clock.Advance(time.Hour + time.Second)
	scheduledSession := waitTaskSession(t, taskID, 2)
	if scheduledSession.ID == manualSession.ID || scheduledSession.Worktree != worktree {
		t.Fatalf("scheduled fire session = %+v (manual=%s)", scheduledSession, manualSession.ID)
	}

	// 2. Agent tool scoping, turn instruction injection, and team inheritance.
	role := protocol.DefaultRoleProfile()
	role.ID, role.Key, role.Name = protocol.NewRoleID(), "reviewer", "Reviewer"
	role = commandFixture(t, owner, protocol.Command{Kind: "put_role", Data: protocol.CommandPutRolePayload{Role: role}}).Data.(protocol.RoleProfile)
	team := protocol.DefaultTeamProfile()
	team.ID, team.Key, team.Name, team.Members = protocol.NewTeamID(), "review", "Review", []protocol.RoleID{role.ID}
	team = commandFixture(t, owner, protocol.Command{Kind: "put_team", Data: protocol.CommandPutTeamPayload{Team: team}}).Data.(protocol.TeamProfile)

	toolFireAt := clock.Now().Add(2 * time.Hour).UnixMilli()
	createArgs, _ := json.Marshal(map[string]any{
		"action": "create", "name": "Team review", "prompt": "Review open PRs",
		"timing": map[string]any{"kind": "once", "data": map[string]any{"at_ms": toolFireAt}},
	})
	script.mu.Lock()
	script.calls = [][2]string{{"scheduled_tasks", string(createArgs)}}
	script.results = nil
	script.seenPrompts = nil
	script.mu.Unlock()

	start := protocol.DefaultConversationStart()
	start.Project, start.Config, start.Title = &proj.ID, &cfg, "Schedule review"
	start.Message = protocol.ConversationInput{Text: "Schedule a team review"}
	start.Execution, start.Team = protocol.ConversationExecutionTeam, &protocol.TeamSelection{ID: team.ID}
	runAndApproveSession(t, owner, start)

	script.mu.Lock()
	toolResults := slices.Clone(script.results)
	seenPrompts := slices.Clone(script.seenPrompts)
	script.mu.Unlock()
	if len(toolResults) != 1 {
		t.Fatalf("expected 1 scheduled_tasks tool result, got %v", toolResults)
	}
	if !slices.ContainsFunc(seenPrompts, func(p string) bool { return strings.Contains(p, "Use the turn's UTC start time") }) {
		t.Fatalf("turn instruction missing from prompts: %v", seenPrompts)
	}
	var createdTask struct {
		ID        string              `json:"id"`
		Execution string              `json:"execution"`
		Project   *protocol.ProjectID `json:"project"`
		Team      *struct {
			ID protocol.TeamID `json:"id"`
		} `json:"team"`
	}
	if err := json.Unmarshal([]byte(toolResults[0]), &createdTask); err != nil || createdTask.ID == "" ||
		createdTask.Execution != "team" || createdTask.Team == nil || createdTask.Team.ID != team.ID ||
		createdTask.Project == nil || *createdTask.Project != proj.ID {
		t.Fatalf("tool created unexpected task: %s (%v)", toolResults[0], err)
	}
	clock.Advance(2*time.Hour + time.Second)
	teamSession := waitTaskSession(t, createdTask.ID, 1)
	if teamSession.Roles.Execution != protocol.ConversationExecutionTeam || teamSession.Roles.Team == nil ||
		teamSession.Roles.Team.ID != team.ID || len(teamSession.Roles.Profiles) != 1 || teamSession.Roles.Profiles[0].ID != role.ID {
		t.Fatalf("fired team task lost team roster: %+v", teamSession.Roles)
	}

	// 3. Legacy plugin retirement continuity: task created under legacy plugin fires natively after removal.
	legacyInfo := installPluginFixture(t, owner, "scheduled-tasks")
	legacyScope := protocol.DefaultPluginContext()
	legacyScope.Package = plugins.PackageReference(legacyInfo)
	legacyTaskID := protocol.NewRequestID().String()
	legacyFireAt := clock.Now().Add(3 * time.Hour).UnixMilli()
	saveBody, _ := json.Marshal(map[string]any{
		"id": legacyTaskID, "revision": "0", "name": "Migrated task", "prompt": "Run after retirement",
		"project": proj.ID, "worktree": worktree, "config": cfg, "execution": "model",
		"enabled": true, "queue": "default",
		"timing": map[string]any{"kind": "once", "data": map[string]any{"at_ms": legacyFireAt}},
	})
	saveReq := protocol.NewRequest(owner.ID(), protocol.Command{
		Kind: "call_plugin",
		Data: protocol.CommandCallPluginPayload{Handler: "save", Input: saveBody},
	})
	saveReq.Plugin = &legacyScope
	if resp := dispatchFixture(t, owner, owner.ID(), saveReq); resp.Fault != nil {
		t.Fatalf("legacy scheduled-tasks save: %v", resp.Fault)
	}
	// Retire legacy plugin.
	commandFixture(t, owner, protocol.Command{
		Kind: "remove_plugin",
		Data: protocol.CommandRemovePluginPayload{Name: legacyInfo.Summary.Name, ExpectedRevision: legacyInfo.Summary.Revision},
	})
	// Native list still finds the migrated task and fake clock fires it natively.
	listed := commandFixture(t, owner, protocol.Command{
		Kind: "list_scheduled_tasks",
		Data: &protocol.CommandListScheduledTasksPayload{},
	}).Data.(protocol.ScheduledTaskPage)
	if !slices.ContainsFunc(listed.Items, func(e protocol.ScheduledTaskEntry) bool { return e.Task.ID == legacyTaskID }) {
		t.Fatalf("migrated task missing from native list: %+v", listed.Items)
	}
	clock.Advance(3*time.Hour + time.Second)
	migratedSession := waitTaskSession(t, legacyTaskID, 1)
	if migratedSession.Association == nil || migratedSession.Association.ID != legacyTaskID {
		t.Fatalf("migrated task session = %+v", migratedSession)
	}
}

func TestHarnessThirdPartyPluginNamespaceIsolation(t *testing.T) {
	t.Parallel()
	owner := startHarnessModuleNode(t, clockwork.NewFakeClockAt(time.Now()), nil)
	ctx := t.Context()

	installThirdParty := func(name string) {
		t.Helper()
		dir := filepath.Join(t.TempDir(), name)
		if err := os.MkdirAll(dir, 0o755); err != nil {
			t.Fatal(err)
		}
		manifest := fmt.Sprintf(`{
			"$schema": "https://agent-plugins.org/schemas/1.0.0/plugin.schema.json",
			"name": %q,
			"version": "1.0.0",
			"description": "Third-party package occupying %s",
			"extensions": {
				"dev.sailry.platform": {
					"api_version": "v1",
					"scope": "host"
				}
			}
		}`, name, name)
		if err := os.WriteFile(filepath.Join(dir, "plugin.json"), []byte(manifest), 0o644); err != nil {
			t.Fatal(err)
		}
		info, err := owner.plugins.manager.Install(owner.pluginContext(ctx), dir, "", name)
		if err != nil {
			t.Fatalf("install third-party %s: %v", name, err)
		}
		if err := owner.profile.Store.Node.Write(ctx, func(db *sql.Conn) error {
			_, err := store.InstallPlugin(ctx, db, info, 0, true)
			return err
		}); err != nil {
			t.Fatalf("store third-party %s: %v", name, err)
		}
	}

	installThirdParty("scheduled-tasks")
	installThirdParty("reminders")

	// Native commands MUST NOT borrow namespaces occupied by third-party packages.
	for _, cmd := range []protocol.Command{
		{Kind: "list_scheduled_tasks", Data: &protocol.CommandListScheduledTasksPayload{}},
		{Kind: "list_reminders", Data: &protocol.CommandListRemindersPayload{}},
	} {
		resp := dispatchFixture(t, owner, owner.ID(), protocol.NewRequest(owner.ID(), cmd))
		if resp.Fault == nil || resp.Fault.Code != protocol.ErrorCodeNotConfigured {
			t.Fatalf("command %s should fail with not_configured on third-party namespace, got %+v", cmd.Kind, resp)
		}
	}

	// Native agent tools MUST NOT be offered for namespaces occupied by third-party packages.
	var localUser protocol.User
	if err := owner.profile.Store.Node.Read(ctx, func(db *sql.Conn) error {
		var err error
		localUser, err = store.CurrentUser(ctx, db)
		return err
	}); err != nil {
		t.Fatal(err)
	}
	provider := seedFixtureModelProvider(t, owner)
	cfg := protocol.DefaultSessionConfig()
	cfg.Provider, cfg.Model, cfg.Mode, cfg.Permission = provider.ID, "fixture", protocol.WorkModeCode, protocol.PermissionAsk
	inv := &store.AgentInvocation{
		User:        localUser,
		StorageUser: localUser.ID,
		Session:     protocol.Session{ID: protocol.NewSessionID()},
		Worktree:    protocol.Worktree{ID: protocol.NewWorktreeID()},
		Run:         protocol.ConversationRun{Turn: protocol.NewTurnID(), Worktree: protocol.NewWorktreeID()},
		Turn:        protocol.DefaultQueuedTurn(),
	}
	inv.Turn.Config = cfg
	tools, err := owner.moduleTools(store.WithUser(ctx, localUser), inv)
	if err != nil {
		t.Fatalf("moduleTools: %v", err)
	}
	for _, tool := range tools {
		if tool.Definition.Name == "scheduled_tasks" || tool.Definition.Name == "reminders" {
			t.Fatalf("native tool %q leaked into turn despite third-party package occupying namespace", tool.Definition.Name)
		}
	}
}
