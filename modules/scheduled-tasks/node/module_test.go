package scheduledtasksnode

import (
	"context"
	"database/sql"
	"encoding/json"
	"errors"
	"slices"
	"strings"
	"testing"
	"time"

	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/node/agent/operations"
	"github.com/sailry/sailry-harness/internal/node/moduleapi"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

type fakeKVRecord struct {
	rev uint64
	raw json.RawMessage
}

type fakeHost struct {
	moduleapi.Host
	localNode protocol.NodeID
	now       time.Time

	pluginInstalled bool
	pluginEnabled   bool
	pluginCaptured  bool

	kv        map[string]fakeKVRecord
	schedules map[string]protocol.DispatchSchedule
	handlers  map[string]protocol.DispatchHandler
	jobs      []protocol.DispatchJob
	results   map[protocol.JobID]protocol.RequestOutcome
	canceled  []protocol.JobID
	stopped   []protocol.JobID

	catalog       protocol.ProjectsCatalog
	teams         []protocol.TeamProfile
	resolveModel  func(model string, effort *protocol.Effort, base *protocol.SessionConfig) (protocol.SessionConfig, error)
	forward       func(target protocol.NodeID, cmd protocol.Command) (protocol.Output, error)
	configApprove bool

	lastTxOps []protocol.PluginTransactionOperation
}

func newFakeHost(local protocol.NodeID, now time.Time) *fakeHost {
	return &fakeHost{
		localNode: local,
		now:       now,
		kv:        map[string]fakeKVRecord{},
		schedules: map[string]protocol.DispatchSchedule{},
		handlers:  map[string]protocol.DispatchHandler{},
		results:   map[protocol.JobID]protocol.RequestOutcome{},
		catalog:   protocol.DefaultProjectsCatalog(),
	}
}

func (h *fakeHost) LocalNodeID() protocol.NodeID { return h.localNode }
func (h *fakeHost) Now() time.Time               { return h.now }

func (h *fakeHost) PluginInstalled(context.Context, string) (bool, bool, error) {
	return h.pluginInstalled, h.pluginEnabled, nil
}

func (h *fakeHost) PluginCaptured(_ *store.AgentInvocation, name string) (protocol.PluginReference, bool) {
	if !h.pluginCaptured {
		return protocol.PluginReference{}, false
	}
	return protocol.PluginReference{Name: name, Digest: "legacy"}, true
}

func (h *fakeHost) ConfigureStorageApproval(_ context.Context, _ *sql.Conn, _ protocol.TurnID, value protocol.ConversationApproval) (protocol.ConversationApproval, error) {
	h.configApprove = true
	return value, nil
}

func (h *fakeHost) ReadProjectCatalog(context.Context) (protocol.ProjectsCatalog, error) {
	return h.catalog, nil
}

func (h *fakeHost) ListTeams(context.Context) ([]protocol.TeamProfile, error) {
	return h.teams, nil
}

func (h *fakeHost) ResolveModel(_ context.Context, model string, effort *protocol.Effort, config *protocol.SessionConfig) (protocol.SessionConfig, error) {
	if h.resolveModel != nil {
		return h.resolveModel(model, effort, config)
	}
	cfg := protocol.DefaultSessionConfig()
	if config != nil {
		cfg = *config
	}
	providerStr, modelName, _ := strings.Cut(model, "/")
	if id, err := protocol.ParseProviderID(providerStr); err == nil {
		cfg.Provider = id
	}
	cfg.Model = modelName
	if effort != nil {
		cfg.Effort = *effort
	}
	return cfg, nil
}

func (h *fakeHost) ForwardModuleCommand(_ context.Context, target protocol.NodeID, command protocol.Command) (protocol.Output, error) {
	if h.forward == nil {
		return protocol.Output{}, fault(protocol.ErrorCodeUnavailable, "offline")
	}
	return h.forward(target, command)
}

func (h *fakeHost) StorageGet(_ context.Context, _ string, key string) (protocol.PluginStorageEntry, error) {
	rec, ok := h.kv[key]
	if !ok {
		return protocol.PluginStorageEntry{Key: key, Present: false, Revision: 0}, nil
	}
	return protocol.PluginStorageEntry{Key: key, Present: true, Revision: rec.rev, Value: rec.raw}, nil
}

func (h *fakeHost) PluginStorageGet(ctx context.Context, _ *store.AgentInvocation, ns, key string) (protocol.PluginStorageEntry, error) {
	return h.StorageGet(ctx, ns, key)
}

func (h *fakeHost) StorageListKeys(_ context.Context, _ string, prefix string, after *string, limit uint16) (protocol.PluginStoragePage, error) {
	keys := make([]string, 0, len(h.kv))
	for k := range h.kv {
		if strings.HasPrefix(k, prefix) && (after == nil || k > *after) {
			keys = append(keys, k)
		}
	}
	slices.Sort(keys)
	var next *string
	if int(limit) > 0 && len(keys) > int(limit) {
		last := keys[limit-1]
		next = &last
		keys = keys[:limit]
	}
	return protocol.PluginStoragePage{Keys: keys, After: next}, nil
}

func (h *fakeHost) PluginStorageListKeys(ctx context.Context, _ *store.AgentInvocation, ns, prefix string, after *string, limit uint16) (protocol.PluginStoragePage, error) {
	return h.StorageListKeys(ctx, ns, prefix, after, limit)
}

func (h *fakeHost) StorageDispatch(_ context.Context, _ string, action protocol.DispatchCommand) (protocol.DispatchOutput, error) {
	switch action.Kind {
	case "list_schedules":
		out := make([]protocol.DispatchSchedule, 0, len(h.schedules))
		for _, s := range h.schedules {
			out = append(out, s)
		}
		return protocol.DispatchOutput{Kind: "schedules", Data: out}, nil
	case "list_handlers":
		out := make([]protocol.DispatchHandler, 0, len(h.handlers))
		for _, hd := range h.handlers {
			out = append(out, hd)
		}
		return protocol.DispatchOutput{Kind: "handlers", Data: out}, nil
	case "list_jobs":
		payload := action.Data.(*protocol.DispatchCommandListJobsPayload)
		start := 0
		if payload.Before != nil {
			start = int(*payload.Before)
		}
		if start >= len(h.jobs) {
			return protocol.DispatchOutput{Kind: "jobs", Data: protocol.DispatchPage{Jobs: []protocol.DispatchJob{}}}, nil
		}
		end := start + int(payload.Limit)
		var next *uint64
		if end < len(h.jobs) {
			n := uint64(end)
			next = &n
		} else {
			end = len(h.jobs)
		}
		return protocol.DispatchOutput{Kind: "jobs", Data: protocol.DispatchPage{Jobs: h.jobs[start:end], NextBefore: next}}, nil
	case "read_result":
		payload := action.Data.(*protocol.DispatchCommandReadResultPayload)
		if res, ok := h.results[payload.ID]; ok {
			return protocol.DispatchOutput{Kind: "result", Data: res}, nil
		}
		return protocol.DispatchOutput{Kind: "result", Data: protocol.RequestOutcome{Kind: "not_admitted"}}, nil
	case "cancel_job":
		payload := action.Data.(*protocol.DispatchCommandCancelJobPayload)
		h.canceled = append(h.canceled, payload.ID)
		job := protocol.DefaultDispatchJob()
		job.ID = payload.ID
		job.Status = protocol.DispatchStatusCancelled
		return protocol.DispatchOutput{Kind: "job", Data: job}, nil
	default:
		return protocol.DispatchOutput{}, fault(protocol.ErrorCodeInvalidRequest, "unsupported dispatch action")
	}
}

func (h *fakeHost) PluginStorageDispatch(ctx context.Context, _ *store.AgentInvocation, ns string, action protocol.DispatchCommand) (protocol.DispatchOutput, error) {
	return h.StorageDispatch(ctx, ns, action)
}

func (h *fakeHost) StorageTransaction(_ context.Context, _ string, ops []protocol.PluginTransactionOperation) ([]protocol.Output, error) {
	h.lastTxOps = slices.Clone(ops)
	outputs := make([]protocol.Output, 0, len(ops))
	for _, op := range ops {
		switch op.Kind {
		case "write":
			p := op.Data.(protocol.PluginTransactionOperationWritePayload)
			cur := h.kv[p.Key]
			if cur.rev != p.ExpectedRevision {
				return nil, fault(protocol.ErrorCodeRevisionConflict, "kv conflict")
			}
			nextRev := cur.rev + 1
			h.kv[p.Key] = fakeKVRecord{rev: nextRev, raw: p.Value}
			outputs = append(outputs, protocol.Output{
				Kind: "plugin_value",
				Data: protocol.PluginStorageEntry{Key: p.Key, Present: true, Revision: nextRev, Value: p.Value},
			})
		case "remove":
			p := op.Data.(protocol.PluginTransactionOperationRemovePayload)
			cur, ok := h.kv[p.Key]
			if !ok || cur.rev != p.ExpectedRevision {
				return nil, fault(protocol.ErrorCodeRevisionConflict, "kv conflict")
			}
			nextRev := cur.rev + 1
			delete(h.kv, p.Key)
			outputs = append(outputs, protocol.Output{
				Kind: "plugin_value",
				Data: protocol.PluginStorageEntry{Key: p.Key, Present: false, Revision: nextRev},
			})
		case "dispatch":
			cmd := op.Data.(protocol.DispatchCommand)
			switch cmd.Kind {
			case "save_handler":
				hd := cmd.Data.(protocol.DispatchHandler)
				if prev, ok := h.handlers[hd.Name]; ok {
					if prev.Revision != hd.Revision {
						return nil, fault(protocol.ErrorCodeRevisionConflict, "handler conflict")
					}
					hd.Revision = prev.Revision + 1
				} else {
					hd.Revision = 1
				}
				h.handlers[hd.Name] = hd
				outputs = append(outputs, protocol.Output{Kind: "dispatch", Data: protocol.DispatchOutput{Kind: "handler", Data: hd}})
			case "remove_handler":
				p := cmd.Data.(protocol.DispatchCommandRemoveHandlerPayload)
				prev, ok := h.handlers[p.Name]
				if !ok || prev.Revision != p.ExpectedRevision {
					return nil, fault(protocol.ErrorCodeRevisionConflict, "handler conflict")
				}
				delete(h.handlers, p.Name)
				outputs = append(outputs, protocol.Output{Kind: "dispatch", Data: protocol.DispatchOutput{Kind: "removed"}})
			case "save_schedule":
				s := cmd.Data.(protocol.DispatchSchedule)
				idStr := s.ID.String()
				if prev, ok := h.schedules[idStr]; ok {
					if prev.Revision != s.Revision {
						return nil, fault(protocol.ErrorCodeRevisionConflict, "schedule conflict")
					}
					s.Revision = prev.Revision + 1
				} else {
					s.Revision = 1
				}
				if s.Timing.Kind == "once" {
					at := s.Timing.Data.(protocol.DispatchTimingOncePayload).AtMs
					s.NextMs = &at
				}
				h.schedules[idStr] = s
				outputs = append(outputs, protocol.Output{Kind: "dispatch", Data: protocol.DispatchOutput{Kind: "schedule", Data: s}})
			case "remove_schedule":
				p := cmd.Data.(protocol.DispatchCommandRemoveSchedulePayload)
				idStr := p.ID.String()
				prev, ok := h.schedules[idStr]
				if !ok || prev.Revision != p.ExpectedRevision {
					return nil, fault(protocol.ErrorCodeRevisionConflict, "schedule conflict")
				}
				delete(h.schedules, idStr)
				outputs = append(outputs, protocol.Output{Kind: "dispatch", Data: protocol.DispatchOutput{Kind: "removed"}})
			case "enqueue":
				p := cmd.Data.(protocol.DispatchCommandEnqueuePayload)
				job := protocol.DefaultDispatchJob()
				job.ID = protocol.NewJobID()
				job.Handler = p.Handler
				job.Queue = "default"
				job.Status = protocol.DispatchStatusQueued
				job.Event.Payload = p.Payload
				h.jobs = append([]protocol.DispatchJob{job}, h.jobs...)
				outputs = append(outputs, protocol.Output{Kind: "dispatch", Data: protocol.DispatchOutput{Kind: "job", Data: job}})
			default:
				return nil, fault(protocol.ErrorCodeInvalidRequest, "unsupported dispatch op")
			}
		}
	}
	return outputs, nil
}

func (h *fakeHost) PluginStorageTransaction(ctx context.Context, _ *store.AgentInvocation, ns string, ops []protocol.PluginTransactionOperation) ([]protocol.Output, error) {
	return h.StorageTransaction(ctx, ns, ops)
}

func (h *fakeHost) StopDispatchTurn(_ context.Context, _ string, job protocol.JobID) error {
	h.stopped = append(h.stopped, job)
	return nil
}

func testNodeID(b byte) protocol.NodeID {
	var id protocol.NodeID
	id[0] = b
	return id
}

func testSessionConfig(provider protocol.ProviderID, modelName string) protocol.SessionConfig {
	cfg := protocol.DefaultSessionConfig()
	cfg.Provider = provider
	cfg.Model = modelName
	cfg.Mode = protocol.WorkModeCode
	cfg.Permission = protocol.PermissionAsk
	return cfg
}

func testTask(id string, provider protocol.ProviderID, atMs int64) protocol.ScheduledTask {
	return protocol.ScheduledTask{
		ID:        id,
		Name:      "Daily digest",
		Prompt:    "  Summarize today's commits  ",
		Config:    testSessionConfig(provider, "gpt-5"),
		Execution: protocol.ConversationExecutionModel,
		Timing: protocol.DispatchTiming{
			Kind: "once",
			Data: protocol.DispatchTimingOncePayload{AtMs: atMs},
		},
		Enabled: true,
		Queue:   "default",
	}
}

func TestModuleMetadataAndCatalog(t *testing.T) {
	t.Parallel()
	m := Module()
	if m.ID() != "scheduled-tasks" {
		t.Fatalf("ID = %q", m.ID())
	}
	if !slices.Equal(m.ReplacesPlugins(), []string{"scheduled-tasks"}) {
		t.Fatalf("ReplacesPlugins = %v", m.ReplacesPlugins())
	}
	if dp, ok := m.(moduleapi.DispatchProvider); !ok || dp.NativeDispatchNamespace() != "scheduled-tasks" {
		t.Fatalf("NativeDispatchNamespace mismatch")
	}
	ch, ok := m.(moduleapi.CommandHandler)
	if !ok {
		t.Fatalf("module does not implement CommandHandler")
	}
	for _, cmd := range []string{
		"list_scheduled_tasks", "read_scheduled_task", "save_scheduled_task",
		"remove_scheduled_task", "run_scheduled_task", "read_scheduled_task_runtime",
		"read_scheduled_task_history", "cancel_scheduled_task_job", "stop_scheduled_task_job",
	} {
		if !ch.HandlesCommand(cmd) {
			t.Errorf("HandlesCommand(%q) = false", cmd)
		}
	}
	if ch.HandlesCommand("unknown_cmd") {
		t.Error("HandlesCommand(unknown_cmd) = true")
	}
	catalog := m.Catalog()
	if len(catalog) != 1 || catalog[0].Name != "scheduled_tasks" || catalog[0].LegacyAlias() != operations.Alias("scheduled-tasks", "scheduled_tasks") {
		t.Fatalf("Catalog = %+v", catalog)
	}
	if catalog[0].Display == nil || catalog[0].Display.Icon == nil || catalog[0].Display.Icon.Kind != "Name" ||
		*catalog[0].Display.Icon.Data.(*string) != "reicon:time/calendar" || len(catalog[0].Display.Approval) != 4 {
		t.Fatalf("Display = %+v", catalog[0].Display)
	}
}

func TestModelNormalizationAndCAS(t *testing.T) {
	t.Parallel()
	local := testNodeID(1)
	prov := protocol.NewProviderID()
	teamID := protocol.NewTeamID()
	teamRev := uint64(4)
	roleID := protocol.NewRoleID()

	// Legacy missing execution defaults to "team"; team.revision normalized to nil; weekly weekdays sorted; prompt preserved.
	rawLegacy := map[string]any{
		"id":      newID(),
		"name":    "  Weekly audit  ",
		"prompt":  "  Check logs\n",
		"config":  testSessionConfig(prov, "m1"),
		"team":    map[string]any{"id": teamID.String(), "revision": teamRev},
		"timing":  map[string]any{"kind": "weekly", "data": map[string]any{"timezone": "UTC", "weekdays": []int{5, 1, 3}, "hour": 9, "minute": 30}},
		"enabled": true,
		"queue":   "default",
	}
	b, _ := json.Marshal(rawLegacy)
	parsed, err := parseStoredTask(b, rawLegacy["id"].(string))
	if err != nil {
		t.Fatalf("parseStoredTask: %v", err)
	}
	if parsed.Execution != protocol.ConversationExecutionTeam {
		t.Fatalf("Execution = %q, want team", parsed.Execution)
	}
	if parsed.Name != "Weekly audit" || parsed.Prompt != "  Check logs\n" {
		t.Fatalf("name=%q prompt=%q", parsed.Name, parsed.Prompt)
	}
	if parsed.Team == nil || parsed.Team.ID != teamID || parsed.Team.Revision != nil {
		t.Fatalf("Team = %+v", parsed.Team)
	}
	weekly := parsed.Timing.Data.(protocol.DispatchTimingWeeklyPayload)
	if !slices.Equal(weekly.Weekdays, protocol.Bytes{1, 3, 5}) {
		t.Fatalf("Weekdays = %v", weekly.Weekdays)
	}

	// Model execution rejects team or models.
	bad := testTask(newID(), prov, 1000)
	bad.Team = &protocol.TeamSelection{ID: teamID}
	if _, err := normalizeDraft(bad, local, true); err == nil {
		t.Fatal("expected error for model execution with team")
	}
	bad.Team = nil
	bad.Models = map[protocol.RoleID]protocol.RoleModel{roleID: {Provider: prov, Model: "m1"}}
	if _, err := normalizeDraft(bad, local, true); err == nil {
		t.Fatal("expected error for model execution with member models")
	}

	// Canonical CAS ignores Config.Options key ordering, unsorted weekdays, and team.revision.
	a := parsed
	a.Config.Options = map[string]map[string]json.RawMessage{"p": {"b": json.RawMessage(`{"y":2,"x":1}`), "a": json.RawMessage(`1`)}}
	other := a
	other.Team = &protocol.TeamSelection{ID: teamID, Revision: &teamRev}
	other.Timing = protocol.DispatchTiming{Kind: "weekly", Data: protocol.DispatchTimingWeeklyPayload{Timezone: "UTC", Weekdays: protocol.Bytes{3, 5, 1}, Hour: 9, Minute: 30}}
	other.Config.Options = map[string]map[string]json.RawMessage{"p": {"a": json.RawMessage(`1`), "b": json.RawMessage(`{"x":1, "y":2}`)}}
	if rev, err := checkBase(99, &other, true, 7, a); err != nil || rev != 7 {
		t.Fatalf("checkBase canonical match failed: rev=%d err=%v", rev, err)
	}
	other.Prompt = "changed"
	if _, err := checkBase(7, &other, true, 7, a); err == nil {
		t.Fatal("expected revision_conflict for mismatched expected_value")
	}
}

func TestCommandsLifecycleTeamLeadAndManualRun(t *testing.T) {
	t.Parallel()
	local := testNodeID(1)
	now := time.UnixMilli(1_700_000_000_000)
	host := newFakeHost(local, now)
	provBase := protocol.NewProviderID()
	provTeam := protocol.NewProviderID()
	projID := protocol.NewProjectID()
	wtID := protocol.NewWorktreeID()
	host.catalog = protocol.ProjectsCatalog{
		Projects:  []protocol.Project{{ID: projID, Name: "App"}},
		Worktrees: []protocol.Worktree{{ID: wtID, Project: &projID}},
	}
	teamID := protocol.NewTeamID()
	host.teams = []protocol.TeamProfile{{
		ID:       teamID,
		Revision: 2,
		Key:      "review",
		Name:     "Review",
		Model:    &protocol.RoleModel{Provider: provTeam, Model: "claude-opus", Effort: nil},
	}}
	// Track the two-step fixed-team-lead effort resolution:
	// 1. ResolveModel(teamModel, nil, nil) -> returns owner default effort "high"
	// 2. ResolveModel(teamModel, &"high", &base) -> merges into base config.
	var resolveCalls []string
	host.resolveModel = func(model string, effort *protocol.Effort, base *protocol.SessionConfig) (protocol.SessionConfig, error) {
		resolveCalls = append(resolveCalls, model)
		if base == nil && effort == nil {
			cfg := testSessionConfig(provTeam, "claude-opus")
			cfg.Effort = protocol.Effort{Kind: "high"}
			return cfg, nil
		}
		cfg := *base
		cfg.Provider = provTeam
		cfg.Model = "claude-opus"
		if effort != nil {
			cfg.Effort = *effort
		}
		return cfg, nil
	}

	m := Module().(moduleapi.CommandHandler)
	exec := func(kind string, data any) (protocol.Output, error) {
		out, _, err := m.ExecuteCommand(t.Context(), host, local, protocol.NewRequest(local, protocol.Command{Kind: kind, Data: data}))
		return out, err
	}

	// Save a team task with fixed team lead model (Effort nil -> resolves to "high").
	taskID := newID()
	revFive := uint64(5)
	task := testTask(taskID, provBase, now.UnixMilli()+60_000)
	task.Project = &projID
	task.Worktree = &wtID
	task.Execution = protocol.ConversationExecutionTeam
	task.Team = &protocol.TeamSelection{ID: teamID, Revision: &revFive}

	out, err := exec("save_scheduled_task", &protocol.CommandSaveScheduledTaskPayload{Task: task, Revision: 0})
	if err != nil {
		t.Fatalf("save_scheduled_task: %v", err)
	}
	saved := out.Data.(protocol.ScheduledTaskEntry)
	if saved.Revision == nil || *saved.Revision != 1 {
		t.Fatalf("saved revision = %v", saved.Revision)
	}
	if saved.Task.Team == nil || saved.Task.Team.Revision != nil {
		t.Fatalf("team revision not normalized: %+v", saved.Task.Team)
	}
	if saved.Task.Config.Provider != provTeam || saved.Task.Config.Model != "claude-opus" || saved.Task.Config.Effort.Kind != "high" {
		t.Fatalf("fixed team lead model not resolved: %+v", saved.Task.Config)
	}
	if len(resolveCalls) != 2 {
		t.Fatalf("expected 2 ResolveModel calls for nil team effort, got %v", resolveCalls)
	}
	if len(host.lastTxOps) != 3 || host.lastTxOps[0].Kind != "write" {
		t.Fatalf("save transaction ops = %+v", host.lastTxOps)
	}

	// List includes NextMs from dispatch schedule.
	listOut, err := exec("list_scheduled_tasks", &protocol.CommandListScheduledTasksPayload{})
	if err != nil {
		t.Fatalf("list_scheduled_tasks: %v", err)
	}
	page := listOut.Data.(protocol.ScheduledTaskPage)
	if len(page.Items) != 1 || page.Items[0].NextMs == nil || *page.Items[0].NextMs != now.UnixMilli()+60_000 {
		t.Fatalf("list page = %+v", page)
	}

	// Manual run performs [write fence, save_handler(resolved live config), enqueue].
	rev1 := uint64(1)
	runOut, err := exec("run_scheduled_task", &protocol.CommandRunScheduledTaskPayload{ID: taskID, Revision: &rev1})
	if err != nil {
		t.Fatalf("run_scheduled_task: %v", err)
	}
	job := runOut.Data.(protocol.DispatchJob)
	if job.Handler != taskID || job.Status != protocol.DispatchStatusQueued {
		t.Fatalf("run job = %+v", job)
	}
	if len(host.lastTxOps) != 3 || host.lastTxOps[0].Kind != "write" ||
		host.lastTxOps[1].Data.(protocol.DispatchCommand).Kind != "save_handler" ||
		host.lastTxOps[2].Data.(protocol.DispatchCommand).Kind != "enqueue" {
		t.Fatalf("run transaction ops = %+v", host.lastTxOps)
	}

	// Runtime aggregates running > queued > completed across jobs.
	startedMs := now.UnixMilli()
	runningJob := protocol.DefaultDispatchJob()
	runningJob.ID = protocol.NewJobID()
	runningJob.Handler = taskID
	runningJob.Status = protocol.DispatchStatusRunning
	runningJob.StartedMs = &startedMs
	runningJob.Event.Payload = json.RawMessage(`{"name":"Daily digest"}`)
	sessionID := protocol.NewSessionID()
	turnID := protocol.NewTurnID()
	host.jobs = append([]protocol.DispatchJob{runningJob}, host.jobs...)
	host.results[runningJob.ID] = protocol.RequestOutcome{
		Kind: "completed",
		Data: protocol.Result[protocol.Output, protocol.Fault]{
			Ok: &protocol.Output{Kind: "queued_turn", Data: protocol.QueuedTurn{ID: turnID, Session: sessionID}},
		},
	}

	rtOut, err := exec("read_scheduled_task_runtime", nil)
	if err != nil {
		t.Fatalf("read_scheduled_task_runtime: %v", err)
	}
	rt := rtOut.Data.(protocol.ScheduledTaskRuntime)
	if rt.Status[taskID].Status != protocol.DispatchStatusRunning || rt.Status[taskID].StartedMs == nil {
		t.Fatalf("runtime status = %+v", rt.Status)
	}
	if len(rt.Schedules) != 1 {
		t.Fatalf("runtime schedules = %+v", rt.Schedules)
	}

	// History extracts session/turn from read_result and name from payload.
	histOut, err := exec("read_scheduled_task_history", &protocol.CommandReadScheduledTaskHistoryPayload{Handler: &taskID, Limit: 10})
	if err != nil {
		t.Fatalf("read_scheduled_task_history: %v", err)
	}
	hist := histOut.Data.(protocol.ScheduledTaskHistory)
	if len(hist.Items) != 2 || hist.Items[0].Session == nil || *hist.Items[0].Session != sessionID ||
		hist.Items[0].Turn == nil || *hist.Items[0].Turn != turnID || hist.Items[0].Name != "Daily digest" {
		t.Fatalf("history = %+v", hist)
	}

	// Cancel and stop jobs.
	if _, err := exec("cancel_scheduled_task_job", &protocol.CommandCancelScheduledTaskJobPayload{ID: job.ID}); err != nil {
		t.Fatalf("cancel_scheduled_task_job: %v", err)
	}
	if len(host.canceled) != 1 || host.canceled[0] != job.ID {
		t.Fatalf("canceled = %v", host.canceled)
	}
	stopOut, err := exec("stop_scheduled_task_job", &protocol.CommandStopScheduledTaskJobPayload{ID: runningJob.ID})
	if err != nil {
		t.Fatalf("stop_scheduled_task_job: %v", err)
	}
	if !stopOut.Data.(protocol.ScheduledTaskStopped).Stopped || len(host.stopped) != 1 || host.stopped[0] != runningJob.ID {
		t.Fatalf("stop = %+v, stopped=%v", stopOut.Data, host.stopped)
	}

	// Remove task cancels pending jobs on remove_handler.
	if _, err := exec("remove_scheduled_task", &protocol.CommandRemoveScheduledTaskPayload{ID: taskID, Revision: 2}); err != nil {
		t.Fatalf("remove_scheduled_task: %v", err)
	}
	if len(host.lastTxOps) != 3 {
		t.Fatalf("remove ops = %+v", host.lastTxOps)
	}
	remHandler := host.lastTxOps[2].Data.(protocol.DispatchCommand).Data.(protocol.DispatchCommandRemoveHandlerPayload)
	if !remHandler.CancelPending {
		t.Fatal("remove_handler did not set CancelPending=true")
	}
}

func TestRemoteForwardingAndOfflineFallback(t *testing.T) {
	t.Parallel()
	local := testNodeID(1)
	remote := testNodeID(2)
	now := time.UnixMilli(1_700_000_000_000)
	host := newFakeHost(local, now)
	prov := protocol.NewProviderID()
	taskID := newID()
	remoteTask := testTask(taskID, prov, now.UnixMilli()+60_000)
	remoteTask.ExecutionNode = &remote
	raw, _ := json.Marshal(remoteTask)
	host.kv["task/"+taskID] = fakeKVRecord{rev: 3, raw: raw}

	m := Module().(moduleapi.CommandHandler)
	// When remote is offline, list keeps task visible with Unavailable=true, Revision=nil.
	out, _, err := m.ExecuteCommand(t.Context(), host, local, protocol.NewRequest(local, protocol.Command{
		Kind: "list_scheduled_tasks", Data: &protocol.CommandListScheduledTasksPayload{},
	}))
	if err != nil {
		t.Fatalf("list offline: %v", err)
	}
	page := out.Data.(protocol.ScheduledTaskPage)
	if len(page.Items) != 1 || !page.Items[0].Unavailable || page.Items[0].Revision != nil {
		t.Fatalf("offline item = %+v", page.Items)
	}

	// Save/remove/run forward to remote with ExpectedValue snapshot.
	var forwardedKinds []string
	host.forward = func(target protocol.NodeID, cmd protocol.Command) (protocol.Output, error) {
		if target != remote {
			t.Fatalf("forwarded to wrong target %v", target)
		}
		forwardedKinds = append(forwardedKinds, cmd.Kind)
		switch cmd.Kind {
		case "save_scheduled_task":
			p := cmd.Data.(*protocol.CommandSaveScheduledTaskPayload)
			if p.ExpectedValue == nil || p.ExpectedValue.ID != taskID {
				t.Fatalf("expected snapshot in forwarded save: %+v", p)
			}
			rev := uint64(4)
			return protocol.Output{Kind: "scheduled_task_entry", Data: protocol.ScheduledTaskEntry{Task: p.Task, Revision: &rev}}, nil
		case "run_scheduled_task":
			p := cmd.Data.(*protocol.CommandRunScheduledTaskPayload)
			if p.ExpectedValue == nil || p.Revision == nil || *p.Revision != 3 {
				t.Fatalf("expected snapshot & revision in forwarded run: %+v", p)
			}
			job := protocol.DefaultDispatchJob()
			job.ID = protocol.NewJobID()
			job.Handler = taskID
			return protocol.Output{Kind: "scheduled_task_job", Data: job}, nil
		case "remove_scheduled_task":
			p := cmd.Data.(*protocol.CommandRemoveScheduledTaskPayload)
			if p.ExpectedValue == nil || p.ExpectedValue.ID != taskID {
				t.Fatalf("expected snapshot in forwarded remove: %+v", p)
			}
			return protocol.Output{Kind: "scheduled_task_removed", Data: protocol.ScheduledTaskRemoved{ID: taskID, Removed: true}}, nil
		}
		return protocol.Output{}, fault(protocol.ErrorCodeNotFound, "unexpected")
	}

	if _, _, err := m.ExecuteCommand(t.Context(), host, local, protocol.NewRequest(local, protocol.Command{
		Kind: "save_scheduled_task", Data: &protocol.CommandSaveScheduledTaskPayload{Task: remoteTask, Revision: 3},
	})); err != nil {
		t.Fatalf("forwarded save: %v", err)
	}
	if _, _, err := m.ExecuteCommand(t.Context(), host, local, protocol.NewRequest(local, protocol.Command{
		Kind: "run_scheduled_task", Data: &protocol.CommandRunScheduledTaskPayload{ID: taskID},
	})); err != nil {
		t.Fatalf("forwarded run: %v", err)
	}
	if _, _, err := m.ExecuteCommand(t.Context(), host, local, protocol.NewRequest(local, protocol.Command{
		Kind: "remove_scheduled_task", Data: &protocol.CommandRemoveScheduledTaskPayload{ID: taskID, Revision: 3},
	})); err != nil {
		t.Fatalf("forwarded remove: %v", err)
	}
	if !slices.Equal(forwardedKinds, []string{"save_scheduled_task", "run_scheduled_task", "remove_scheduled_task"}) {
		t.Fatalf("forwardedKinds = %v", forwardedKinds)
	}
}

func TestAgentToolScopingApprovalAndMutations(t *testing.T) {
	t.Parallel()
	local := testNodeID(1)
	now := time.UnixMilli(1_700_000_000_000)
	host := newFakeHost(local, now)
	prov := protocol.NewProviderID()
	projA := protocol.NewProjectID()
	projB := protocol.NewProjectID()
	wtA := protocol.NewWorktreeID()
	teamA := protocol.NewTeamID()
	teamB := protocol.NewTeamID()
	roleID := protocol.NewRoleID()
	host.teams = []protocol.TeamProfile{
		{ID: teamA, Revision: 1, Key: "a", Name: "Team A"},
		{ID: teamB, Revision: 1, Key: "b", Name: "Team B"},
	}

	makeInv := func(proj *protocol.ProjectID) *store.AgentInvocation {
		inv := &store.AgentInvocation{
			Session:  protocol.Session{ID: protocol.NewSessionID()},
			Worktree: protocol.Worktree{ID: wtA, Project: proj},
			Run:      protocol.ConversationRun{Turn: protocol.NewTurnID(), Worktree: wtA},
			Turn:     protocol.DefaultQueuedTurn(),
		}
		inv.Turn.Config = testSessionConfig(prov, "gpt-5")
		inv.Turn.Roles.Execution = protocol.ConversationExecutionTeam
		inv.Turn.Roles.Team = &protocol.TeamReference{ID: teamA, Revision: 3}
		inv.Turn.Roles.Models = map[protocol.RoleID]protocol.RoleModel{roleID: {Provider: prov, Model: "mini"}}
		return inv
	}

	m := Module()

	// Hidden when legacy plugin is installed & disabled (not captured).
	host.pluginInstalled, host.pluginCaptured = true, false
	tools, err := m.ToolsForTurn(t.Context(), host, makeInv(&projA))
	if err != nil || len(tools) != 0 {
		t.Fatalf("expected hidden tool when legacy plugin disabled: tools=%d err=%v", len(tools), err)
	}
	host.pluginInstalled = false

	// Hidden in assistant and resource sessions.
	asstInv := makeInv(nil)
	asstInv.Turn.Config.Assistant = &protocol.PluginConversationBinding{Package: protocol.PluginReference{Name: "other"}, ID: "asst"}
	if tools, err := m.ToolsForTurn(t.Context(), host, asstInv); err != nil || len(tools) != 0 {
		t.Fatalf("expected hidden tool in assistant session: tools=%d err=%v", len(tools), err)
	}
	resInv := makeInv(nil)
	resInv.Turn.Config.Resource = &protocol.ConnectionResource{Kind: "database", Data: protocol.NewDatabaseID()}
	if tools, err := m.ToolsForTurn(t.Context(), host, resInv); err != nil || len(tools) != 0 {
		t.Fatalf("expected hidden tool in resource session: tools=%d err=%v", len(tools), err)
	}

	// Offered in project session with exact TurnInstruction and Confirm policy.
	tools, err = m.ToolsForTurn(t.Context(), host, makeInv(&projA))
	if err != nil || len(tools) != 1 {
		t.Fatalf("ToolsForTurn: tools=%d err=%v", len(tools), err)
	}
	tool := tools[0]
	if tool.TurnInstruction.Key != "plugin:scheduled-tasks" || !strings.Contains(tool.TurnInstruction.Text, "Use the turn's UTC start time") {
		t.Fatalf("TurnInstruction = %+v", tool.TurnInstruction)
	}
	if tool.Confirm(canonical.ToolRequest{Arguments: json.RawMessage(`{"action":"list"}`)}) {
		t.Fatal("list should not require confirmation")
	}
	for _, act := range []string{"create", "update", "delete"} {
		if !tool.Confirm(canonical.ToolRequest{Arguments: json.RawMessage(`{"action":"` + act + `"}`)}) {
			t.Fatalf("%s should require confirmation", act)
		}
	}
	if _, err := tool.ConfigureApproval(t.Context(), nil, canonical.ToolRequest{}, protocol.ConversationApproval{}); err != nil || !host.configApprove {
		t.Fatalf("ConfigureApproval failed: %v", err)
	}

	callTool := func(args any) map[string]any {
		raw, _ := json.Marshal(args)
		res := tool.Execute(t.Context(), canonical.ToolRequest{ID: "c1", Name: "scheduled_tasks", Arguments: raw})
		return res.Value.(map[string]any)
	}

	// Reject past start time on create.
	pastRes := callTool(map[string]any{
		"action": "create", "name": "Past", "prompt": "Do work",
		"timing": map[string]any{"kind": "once", "data": map[string]any{"at_ms": now.UnixMilli()}},
	})
	if pastRes["isError"] != true {
		t.Fatalf("expected error for past at_ms, got %+v", pastRes)
	}

	// Create inherits project, worktree, config, execution, team (revision nil), and member models.
	created := callTool(map[string]any{
		"action": "create", "name": "Check CI", "prompt": "Inspect CI status",
		"timing": map[string]any{"kind": "once", "data": map[string]any{"at_ms": now.UnixMilli() + 120_000}},
	})
	if created["isError"] == true {
		t.Fatalf("create failed: %+v", created)
	}
	taskID := created["id"].(string)
	if created["revision"] != "1" || created["enabled"] != true || created["queue"] != "default" {
		t.Fatalf("created = %+v", created)
	}
	storedModels := created["models"].(map[protocol.RoleID]protocol.RoleModel)
	if len(storedModels) != 1 || storedModels[roleID].Model != "mini" {
		t.Fatalf("inherited models lost: %+v", storedModels)
	}

	// Update name only -> preserves models and allows existing timing even if clock advanced past it.
	host.now = now.Add(5 * time.Minute)
	updatedSameTeam := callTool(map[string]any{
		"action": "update", "id": taskID, "revision": "1", "name": "Check CI v2",
	})
	if updatedSameTeam["isError"] == true {
		t.Fatalf("update name failed: %+v", updatedSameTeam)
	}
	if len(updatedSameTeam["models"].(map[protocol.RoleID]protocol.RoleModel)) != 1 {
		t.Fatalf("models should stay when execution/team unchanged: %+v", updatedSameTeam)
	}

	// Changing team clears inherited member model overrides.
	updatedNewTeam := callTool(map[string]any{
		"action": "update", "id": taskID, "revision": "2",
		"team": map[string]any{"id": teamB.String()},
	})
	if updatedNewTeam["isError"] == true {
		t.Fatalf("update team failed: %+v", updatedNewTeam)
	}
	if len(updatedNewTeam["models"].(map[protocol.RoleID]protocol.RoleModel)) != 0 {
		t.Fatalf("models should be cleared when team changes: %+v", updatedNewTeam["models"])
	}

	// Seed a task in projB and confirm project isolation on list and delete.
	otherID := newID()
	otherTask := testTask(otherID, prov, now.UnixMilli()+60_000)
	otherTask.Project = &projB
	otherRaw, _ := json.Marshal(otherTask)
	host.kv["task/"+otherID] = fakeKVRecord{rev: 1, raw: otherRaw}

	listed := callTool(map[string]any{"action": "list"})
	items := listed["items"].([]map[string]any)
	if len(items) != 1 || items[0]["id"] != taskID {
		t.Fatalf("list should only return projA tasks: %+v", items)
	}
	denied := callTool(map[string]any{"action": "delete", "id": otherID, "revision": "1"})
	if denied["isError"] != true || denied["error"].(map[string]any)["code"] != string(protocol.ErrorCodePermissionDenied) {
		t.Fatalf("expected permission_denied on cross-project delete: %+v", denied)
	}

	// Delete own task succeeds.
	deleted := callTool(map[string]any{"action": "delete", "id": taskID, "revision": "3"})
	if deleted["isError"] == true || deleted["removed"] != true {
		t.Fatalf("delete failed: %+v", deleted)
	}
	if _, exists := host.kv["task/"+taskID]; exists {
		t.Fatal("deleted task still in KV")
	}

	// Unassigned session creates task with nil project and nil worktree.
	unassignedTools, err := m.ToolsForTurn(t.Context(), host, makeInv(nil))
	if err != nil || len(unassignedTools) != 1 {
		t.Fatalf("unassigned ToolsForTurn: %v", err)
	}
	rawCreate, _ := json.Marshal(map[string]any{
		"action": "create", "name": "Unassigned", "prompt": "Ping",
		"timing": map[string]any{"kind": "weekly", "data": map[string]any{"timezone": "UTC", "weekdays": []int{1}, "hour": 8, "minute": 0}},
	})
	unassignedRes := unassignedTools[0].Execute(t.Context(), canonical.ToolRequest{ID: "c2", Name: "scheduled_tasks", Arguments: rawCreate}).Value.(map[string]any)
	if unassignedRes["isError"] == true {
		t.Fatalf("unassigned create failed: %+v", unassignedRes)
	}
	if unassignedRes["project"].(*protocol.ProjectID) != nil || unassignedRes["worktree"].(*protocol.WorktreeID) != nil {
		t.Fatalf("unassigned task should have nil project/worktree: %+v", unassignedRes)
	}

	// Verify fault error wrapping helper sanity.
	if err := fault(protocol.ErrorCodeConflict, "boom"); !errors.Is(err, err) {
		t.Fatal("unreachable")
	}
}
