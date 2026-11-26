package remindersnode

import (
	"context"
	"database/sql"
	"encoding/json"
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
	notified  []protocol.NotificationDraft

	catalog       protocol.ProjectsCatalog
	forward       func(target protocol.NodeID, cmd protocol.Command) (protocol.Output, error)
	configApprove bool
	lastTxOps     []protocol.PluginTransactionOperation
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

func (h *fakeHost) ForwardModuleCommand(_ context.Context, target protocol.NodeID, cmd protocol.Command) (protocol.Output, error) {
	if h.forward == nil {
		return protocol.Output{}, fault(protocol.ErrorCodeUnavailable, "offline")
	}
	return h.forward(target, cmd)
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
		case "notify":
			draft := op.Data.(protocol.NotificationDraft)
			h.notified = append(h.notified, draft)
			outputs = append(outputs, protocol.Output{Kind: "notification", Data: draft})
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
				if s.Enabled && s.Timing.Kind == "once" {
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
			}
		}
	}
	return outputs, nil
}

func (h *fakeHost) PluginStorageTransaction(ctx context.Context, _ *store.AgentInvocation, ns string, ops []protocol.PluginTransactionOperation) ([]protocol.Output, error) {
	return h.StorageTransaction(ctx, ns, ops)
}

func testNodeID(b byte) protocol.NodeID {
	var id protocol.NodeID
	id[0] = b
	return id
}

func TestRemindersMetadataAndCatalog(t *testing.T) {
	t.Parallel()
	m := Module()
	if m.ID() != "reminders" || !slices.Equal(m.ReplacesPlugins(), []string{"reminders"}) {
		t.Fatalf("identity mismatch: id=%q replaces=%v", m.ID(), m.ReplacesPlugins())
	}
	if dp, ok := m.(moduleapi.DispatchProvider); !ok || dp.NativeDispatchNamespace() != "reminders" {
		t.Fatal("NativeDispatchNamespace mismatch")
	}
	ch := m.(moduleapi.CommandHandler)
	for _, cmd := range []string{"list_reminders", "read_reminder", "save_reminder", "remove_reminder", "read_reminder_history", "deliver_reminder"} {
		if !ch.HandlesCommand(cmd) {
			t.Errorf("HandlesCommand(%q) = false", cmd)
		}
	}
	catalog := m.Catalog()
	if len(catalog) != 1 || catalog[0].Name != "reminders" || catalog[0].LegacyAlias() != operations.Alias("reminders", "reminders") {
		t.Fatalf("catalog = %+v", catalog)
	}
}

func TestRemindersCommandsDeliverHistoryAndReschedule(t *testing.T) {
	t.Parallel()
	local := testNodeID(1)
	now := time.UnixMilli(1_700_000_000_000)
	host := newFakeHost(local, now)
	projID := protocol.NewProjectID()
	host.catalog.Projects = []protocol.Project{{ID: projID, Name: "Docs"}}

	m := Module().(moduleapi.CommandHandler)
	exec := func(kind string, data any) (protocol.Output, error) {
		out, _, err := m.ExecuteCommand(t.Context(), host, local, protocol.NewRequest(local, protocol.Command{Kind: kind, Data: data}))
		return out, err
	}

	due := now.UnixMilli() + 60_000
	id := newID()
	savedOut, err := exec("save_reminder", &protocol.CommandSaveReminderPayload{
		Reminder: protocol.Reminder{
			ID:      id,
			Project: &projID,
			Title:   "  Ship release  ",
			Message: "Check changelog",
			DueMs:   &due,
		},
		Revision: 0,
	})
	if err != nil {
		t.Fatalf("save_reminder: %v", err)
	}
	saved := savedOut.Data.(protocol.ReminderEntry)
	if saved.Revision == nil || *saved.Revision != 1 || saved.Reminder.Title != "Ship release" {
		t.Fatalf("saved = %+v", saved)
	}

	// Delivery before due time skips.
	deliverPayload, _ := json.Marshal(map[string]any{"id": id, "revision": "1"})
	delOut, err := exec("deliver_reminder", &protocol.CommandDeliverReminderPayload{Input: deliverPayload})
	if err != nil || !delOut.Data.(protocol.ReminderDelivery).Skipped {
		t.Fatalf("early deliver = %+v, err=%v", delOut.Data, err)
	}

	// Advance clock to due time -> delivers notification and stamps NotifiedMs.
	host.now = time.UnixMilli(due)
	delOut, err = exec("deliver_reminder", &protocol.CommandDeliverReminderPayload{Input: deliverPayload})
	if err != nil || !delOut.Data.(protocol.ReminderDelivery).Delivered {
		t.Fatalf("on-time deliver = %+v, err=%v", delOut.Data, err)
	}
	if len(host.notified) != 1 || host.notified[0].Title != "Ship release" || host.notified[0].Presentation == nil || !host.notified[0].Presentation.Persistent {
		t.Fatalf("notified = %+v", host.notified)
	}

	// Duplicate delivery skips because NotifiedMs is already set.
	delOut, err = exec("deliver_reminder", &protocol.CommandDeliverReminderPayload{Input: deliverPayload})
	if err != nil || !delOut.Data.(protocol.ReminderDelivery).Skipped {
		t.Fatalf("duplicate deliver = %+v, err=%v", delOut.Data, err)
	}

	// Simulate fired one-shot schedule (NextMs == nil) then reschedule -> removes & recreates schedule.
	s := host.schedules[id]
	s.NextMs = nil
	host.schedules[id] = s
	newDue := due + 120_000
	updatedOut, err := exec("save_reminder", &protocol.CommandSaveReminderPayload{
		Reminder: protocol.Reminder{
			ID:      id,
			Project: &projID,
			Title:   "Ship release",
			Message: "Check changelog",
			DueMs:   &newDue,
		},
		Revision: 2,
	})
	if err != nil {
		t.Fatalf("reschedule save_reminder: %v", err)
	}
	updated := updatedOut.Data.(protocol.ReminderEntry)
	if updated.Reminder.NotifiedMs != nil {
		t.Fatalf("changing due_ms should clear NotifiedMs: %+v", updated.Reminder)
	}
	if len(host.lastTxOps) != 4 || host.lastTxOps[1].Data.(protocol.DispatchCommand).Kind != "remove_schedule" {
		t.Fatalf("expected remove_schedule when recreating fired one-shot: %+v", host.lastTxOps)
	}

	// History omits jobs whose receipt returned {skipped: true} (both native & legacy).
	deliveredJob := protocol.DefaultDispatchJob()
	deliveredJob.ID = protocol.NewJobID()
	deliveredJob.Handler = id
	deliveredJob.Status = protocol.DispatchStatusCompleted
	skippedJob := protocol.DefaultDispatchJob()
	skippedJob.ID = protocol.NewJobID()
	skippedJob.Handler = id
	skippedJob.Status = protocol.DispatchStatusCompleted
	host.jobs = []protocol.DispatchJob{deliveredJob, skippedJob}
	host.results[deliveredJob.ID] = protocol.RequestOutcome{
		Kind: "completed",
		Data: protocol.Result[protocol.Output, protocol.Fault]{
			Ok: &protocol.Output{Kind: "reminder_delivery", Data: protocol.ReminderDelivery{Delivered: true}},
		},
	}
	host.results[skippedJob.ID] = protocol.RequestOutcome{
		Kind: "completed",
		Data: protocol.Result[protocol.Output, protocol.Fault]{
			Ok: &protocol.Output{Kind: "reminder_delivery", Data: protocol.ReminderDelivery{Skipped: true}},
		},
	}
	histOut, err := exec("read_reminder_history", &protocol.CommandReadReminderHistoryPayload{Handler: id})
	if err != nil {
		t.Fatalf("read_reminder_history: %v", err)
	}
	hist := histOut.Data.(protocol.ReminderHistory)
	if len(hist.Items) != 1 || hist.Items[0].ID != deliveredJob.ID {
		t.Fatalf("history should omit skipped jobs: %+v", hist.Items)
	}

	// Remove reminder removes KV, schedule, and handler.
	if _, err := exec("remove_reminder", &protocol.CommandRemoveReminderPayload{ID: id, Revision: 3}); err != nil {
		t.Fatalf("remove_reminder: %v", err)
	}
	if _, exists := host.kv["reminder/"+id]; exists || len(host.schedules) != 0 || len(host.handlers) != 0 {
		t.Fatalf("removal left artifacts: kv=%d sched=%d handlers=%d", len(host.kv), len(host.schedules), len(host.handlers))
	}
}

func TestRemindersAgentToolScopingAndNullRejection(t *testing.T) {
	t.Parallel()
	local := testNodeID(1)
	now := time.UnixMilli(1_700_000_000_000)
	host := newFakeHost(local, now)
	projA := protocol.NewProjectID()
	projB := protocol.NewProjectID()

	inv := &store.AgentInvocation{
		Session:  protocol.Session{ID: protocol.NewSessionID()},
		Worktree: protocol.Worktree{ID: protocol.NewWorktreeID(), Project: &projA},
		Run:      protocol.ConversationRun{Turn: protocol.NewTurnID(), Worktree: protocol.NewWorktreeID()},
		Turn:     protocol.DefaultQueuedTurn(),
	}

	m := Module()
	// Disabled legacy plugin suppresses tool.
	host.pluginInstalled, host.pluginCaptured = true, false
	if tools, err := m.ToolsForTurn(t.Context(), host, inv); err != nil || len(tools) != 0 {
		t.Fatalf("expected hidden tool when disabled: tools=%d err=%v", len(tools), err)
	}
	host.pluginInstalled = false

	tools, err := m.ToolsForTurn(t.Context(), host, inv)
	if err != nil || len(tools) != 1 {
		t.Fatalf("ToolsForTurn: tools=%d err=%v", len(tools), err)
	}
	tool := tools[0]
	if tool.Confirm(canonical.ToolRequest{Arguments: json.RawMessage(`{"action":"list"}`)}) {
		t.Fatal("list should not require confirm")
	}
	if !tool.Confirm(canonical.ToolRequest{Arguments: json.RawMessage(`{"action":"create"}`)}) {
		t.Fatal("create should require confirm")
	}

	callTool := func(args string) map[string]any {
		res := tool.Execute(t.Context(), canonical.ToolRequest{ID: "c1", Name: "reminders", Arguments: json.RawMessage(args)})
		return res.Value.(map[string]any)
	}

	// Reject null for non-nullable boolean/string fields.
	if res := callTool(`{"action":"list","completed":null}`); res["isError"] != true {
		t.Fatalf("expected error for completed:null, got %+v", res)
	}
	if res := callTool(`{"action":"create","title":"Test","message":null}`); res["isError"] != true {
		t.Fatalf("expected error for message:null, got %+v", res)
	}

	// Create reminder in projA.
	created := callTool(`{"action":"create","title":"Buy milk","message":"2%"}`)
	if created["isError"] == true {
		t.Fatalf("create failed: %+v", created)
	}
	id := created["id"].(string)

	// Seed a reminder in projB and verify project isolation.
	otherID := newID()
	otherRaw, _ := json.Marshal(protocol.Reminder{ID: otherID, ExecutionNode: &local, Project: &projB, Title: "Other"})
	host.kv["reminder/"+otherID] = fakeKVRecord{rev: 1, raw: otherRaw}

	listed := callTool(`{"action":"list"}`)
	items := listed["items"].([]map[string]any)
	if len(items) != 1 || items[0]["id"] != id {
		t.Fatalf("list leaked cross-project reminder: %+v", items)
	}
	denied := callTool(`{"action":"delete","id":"` + otherID + `","revision":"1"}`)
	if denied["isError"] != true || denied["error"].(map[string]any)["code"] != string(protocol.ErrorCodePermissionDenied) {
		t.Fatalf("expected permission_denied: %+v", denied)
	}

	// Update and delete own reminder.
	updated := callTool(`{"action":"update","id":"` + id + `","revision":"1","completed":true}`)
	if updated["isError"] == true || updated["completed"] != true {
		t.Fatalf("update failed: %+v", updated)
	}
	deleted := callTool(`{"action":"delete","id":"` + id + `","revision":"2"}`)
	if deleted["isError"] == true || deleted["removed"] != true {
		t.Fatalf("delete failed: %+v", deleted)
	}
}
