package client

import (
	"context"
	"encoding/json"
	"errors"
	"reflect"
	"strings"
	"testing"
	"testing/synctest"

	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestControllerRulesAndInbox(t *testing.T) {
	t.Parallel()
	t.Run("scoped Status observation", testPluginStatusObserver)
	t.Run("captured activity reads", testActivityReads)
	t.Run("usage scope captures provenance and resolves only an exact remote digest", func(t *testing.T) {
		target := protocol.NodeID{1}
		query := protocol.DefaultUsageQuery()
		query.StartMs, query.EndMs = 0, 1000
		scope := protocol.DefaultPluginContext()
		scope.Package = protocol.PluginReference{Name: "usage-owner", Digest: strings.Repeat("a", 64), SettingsRevision: 4}
		worktree := protocol.NewWorktreeID()
		scope.Worktree = &worktree
		captured := copyValue(scope)
		packageInfo := protocol.DefaultPluginSummary()
		packageInfo.Name, packageInfo.Digest, packageInfo.SettingsRevision = captured.Package.Name, captured.Package.Digest, 9
		inventory := []protocol.PluginSummary{packageInfo}
		transport := &clientTransport{}
		transport.dispatch = func(request protocol.Request) (link.Admission, error) {
			output := protocol.Output{}
			switch request.Command.Kind {
			case "list_plugins":
				if request.Plugin != nil {
					t.Fatal("matching usage inventory inherited plugin provenance")
				}
				output = protocol.Output{Kind: "plugins", Data: inventory}
			case "read_usage":
				report := protocol.DefaultUsageReport()
				report.Node, report.Query = target, query
				output = protocol.Output{Kind: "usage", Data: report}
			default:
				t.Fatalf("unexpected usage command: %s", request.Command.Kind)
			}
			completion := make(chan link.Response, 1)
			completion <- link.Response{Output: output}
			return link.Admission{Receipt: protocol.Receipt{ID: request.ID}, Completion: completion}, nil
		}
		controller := New(transport)
		t.Cleanup(func() { _ = controller.Close() })
		ctx := WithUsageScope(t.Context(), target, scope)
		worktree = protocol.NewWorktreeID()
		scope.Package.SettingsRevision++
		if _, err := controller.ReadUsage(ctx, query); err != nil {
			t.Fatal(err)
		}
		if len(transport.requests) != 1 || !reflect.DeepEqual(transport.requests[0].Plugin, &captured) {
			t.Fatal("usage read lost its exact captured plugin provenance")
		}
		if _, err := controller.ReadUsage(t.Context(), query); err != nil || transport.requests[1].Plugin != nil {
			t.Fatal("scoping one context changed unscoped Client reads")
		}
		matching := WithMatchingUsageScope(t.Context(), target, captured)
		for _, revision := range []uint64{9, 10} {
			inventory[0].SettingsRevision = revision
			if _, err := controller.ReadUsage(matching, query); err != nil {
				t.Fatal(err)
			}
			request := transport.requests[len(transport.requests)-1]
			expected := copyValue(captured)
			expected.Package.SettingsRevision = revision
			if !reflect.DeepEqual(request.Plugin, &expected) {
				t.Fatal("matching usage changed resource scope or reused an earlier settings revision")
			}
		}
		remote := protocol.NodeID{2}
		combined := WithMatchingUsageScope(ctx, remote, captured)
		first, exists := usageScopeFor(combined, target)
		second, matched := usageScopeFor(combined, remote)
		if !exists || !matched || first.resolve || !second.resolve || !reflect.DeepEqual(first.context, captured) || !reflect.DeepEqual(second.context, captured) {
			t.Fatal("binding another usage source replaced a peer's captured scope")
		}
		if _, exists := usageScopeFor(ctx, remote); exists {
			t.Fatal("adding a matching source mutated an existing context")
		}
		inventory[0].Digest = strings.Repeat("b", 64)
		before := len(transport.requests)
		if _, err := controller.ReadUsage(matching, query); faultCode(err) != protocol.ErrorCodeNotConfigured ||
			len(transport.requests) != before+1 || transport.requests[len(transport.requests)-1].Command.Kind != "list_plugins" {
			t.Fatal("usage read substituted another package digest or reached read_usage")
		}
	})
	t.Run("modes_and_links", func(t *testing.T) {
		config := protocol.DefaultSessionConfig()
		config.Mode, config.Permission = protocol.WorkModeCode, protocol.PermissionAsk
		review := ConversationMode{
			Package: protocol.PluginReference{Name: "reviewer"}, Enabled: true,
			Declaration: protocol.PluginModesDeclaration{ID: "review", Option: "mode"},
		}
		modes := []ConversationMode{review}
		selected := SelectMode(config, modes, "reviewer/review")
		if selected == nil || selected.Mode != protocol.WorkModePlan || len(config.Options) != 0 {
			t.Fatal("mode selection changed captured configuration or authority")
		}
		view := ProjectModes(*selected, modes)
		if view.Selected == nil || *view.Selected != review.Key() || len(view.Choices) != 1 {
			t.Fatal("mode projection did not preserve its package key")
		}
		modes[0].Enabled = false
		if SelectMode(config, modes, review.Key()) != nil || len(ProjectModes(*selected, modes).Choices) != 1 {
			t.Fatal("disabled selected mode was lost or newly selectable")
		}
		base := SelectMode(*selected, modes, "code")
		if base == nil || base.Mode != protocol.WorkModeCode || string(base.Options["reviewer"]["mode"]) != `""` {
			t.Fatal("base mode did not clear the captured package mode")
		}
		id := protocol.NewSessionID()
		for _, candidate := range []struct {
			link string
			want bool
		}{
			{"SAILRY://Session/" + id.String(), true},
			{" sailry://session/" + id.String() + "\n", true},
			{"sailry://session/" + id.String() + "/turn", false},
			{"sailry://session/" + id.String() + "?", false},
			{"sailry://session/" + id.String() + "#", false},
			{"sailry://user@session/" + id.String(), false},
		} {
			if actual := SessionLink(candidate.link); (actual != nil) != candidate.want || actual != nil && *actual != id {
				t.Fatalf("session link %q: %v", candidate.link, actual)
			}
		}
	})
	t.Run("inbox_cursors_and_local_reads", func(t *testing.T) {
		node := protocol.NodeID{1}
		notice := Notice{ID: NoticeID{Node: node, Cursor: 1, Target: NoticeTarget{Kind: "session", Session: protocol.NewSessionID()}}, Kind: "completed"}
		var inbox Inbox
		if added := inbox.Merge(node, []Notice{notice}); len(added) != 1 || inbox.Unread() != 1 {
			t.Fatal("initial transient attention was lost")
		}
		if err := inbox.MarkRead(t.Context(), notice.ID, nil); err != nil {
			t.Fatal(err)
		}
		inbox.Merge(node, []Notice{notice})
		if inbox.Unread() != 0 {
			t.Fatal("coalesced Client state restored a local unread attention")
		}
		encoded, err := json.Marshal(notice.ID)
		if err != nil {
			t.Fatal(err)
		}
		var restored NoticeID
		if err := json.Unmarshal(encoded, &restored); err != nil || restored != notice.ID {
			t.Fatal("notification target lost its Rust union representation")
		}
	})
	t.Run("inbox_durable_read_accepts_the_node_answer", func(t *testing.T) {
		transport := &clientTransport{}
		controller := New(transport)
		t.Cleanup(func() { _ = controller.Close() })
		stored := protocol.DefaultNotificationNotice()
		stored.ID, stored.Sequence, stored.Read = protocol.NewNotificationID(), 4, true
		transport.dispatch = func(request protocol.Request) (link.Admission, error) {
			input, err := payload[protocol.CommandMarkNotificationReadPayload](request.Command.Data)
			if request.Command.Kind != "mark_notification_read" || err != nil || input.ID != stored.ID {
				t.Fatalf("unexpected read command: %+v", request.Command)
			}
			completed := make(chan link.Response, 1)
			// The Node, like Rust, answers with the notice it marked.
			completed <- link.Response{Output: protocol.Output{Kind: "notification", Data: stored}}
			return link.Admission{Receipt: protocol.Receipt{ID: request.ID}, Completion: completed}, nil
		}
		notice := Notice{ID: NoticeID{Node: controller.Target(), Cursor: stored.Sequence, Target: NoticeTarget{Kind: "notification", Notification: stored.ID}}, Kind: "notification"}
		var inbox Inbox
		inbox.Merge(controller.Target(), []Notice{notice})
		if err := inbox.MarkRead(t.Context(), notice.ID, map[protocol.NodeID]*Client{controller.Target(): controller}); err != nil {
			t.Fatalf("durable notice could not be marked read: %v", err)
		}
		if inbox.Unread() != 0 {
			t.Fatal("durable notice stayed unread after the Node marked it")
		}
	})
	t.Run("scheduled_wait_is_one_durable_notice", func(t *testing.T) {
		for _, scenario := range []struct {
			name    string
			waiting protocol.ActivityWaiting
			level   protocol.NotificationKind
			kind    string
		}{
			{"approval", protocol.ActivityWaitingApproval, protocol.NotificationKindWarning, "approval"},
			{"input", protocol.ActivityWaitingInput, protocol.NotificationKindInfo, "input"},
		} {
			t.Run(scenario.name, func(t *testing.T) {
				session := protocol.DefaultSession()
				session.ID = protocol.NewSessionID()
				run := protocol.DefaultConversationRun()
				run.Turn, run.Session, run.Status, run.Sequence = protocol.NewTurnID(), session.ID, protocol.ConversationStatusRunning, 1
				session.Activity.Run = &run
				snapshot := protocol.DefaultSnapshot()
				snapshot.Node, snapshot.Cursor, snapshot.Sessions = protocol.NodeID{1}, 1, []protocol.Session{session}
				var feed activityFeed
				feed.observe(snapshot)
				durable := protocol.DefaultNotificationNotice()
				durable.ID, durable.Sequence = protocol.NewNotificationID(), 1
				durable.Content.Kind, durable.Content.Session = scenario.level, &session.ID
				durable.Content.Details = &protocol.NotificationDetails{Kind: "task", Data: &protocol.NotificationDetailsTask{Turn: run.Turn, Status: protocol.ConversationStatusRunning}}
				// The notice is applied first, then the activity it explains.
				snapshot.Cursor, snapshot.Notifications = 2, []protocol.NotificationNotice{durable}
				feed.observe(snapshot)
				waiting := scenario.waiting
				snapshot.Sessions[0].Activity.Waiting = &waiting
				snapshot.Cursor = 3
				feed.observe(snapshot)
				if len(feed.notices) != 1 || feed.notices[0].Kind != scenario.kind || feed.notices[0].ID.Target.Kind != "notification" {
					t.Fatalf("scheduled wait notices: %+v", feed.notices)
				}
				// Without a durable notice the transition still raises its own.
				snapshot.Sessions[0].Activity.Waiting = nil
				snapshot.Cursor, snapshot.Notifications[0].Read = 4, true
				feed.observe(snapshot)
				snapshot.Sessions[0].Activity.Waiting = &waiting
				snapshot.Cursor = 5
				feed.observe(snapshot)
				if len(feed.notices) != 2 || feed.notices[0].ID.Target.Kind != "session" || feed.notices[0].Kind != scenario.kind {
					t.Fatalf("interactive wait notices: %+v", feed.notices)
				}
				// A late controller sees the wait it never observed.
				var late activityFeed
				snapshot.Notifications[0].Read = false
				late.observe(snapshot)
				if len(late.notices) != 1 || late.notices[0].Kind != scenario.kind || late.notices[0].Read {
					t.Fatalf("late controller notices: %+v", late.notices)
				}
			})
		}
	})
	t.Run("queued_pulls_cancel_independently", func(t *testing.T) {
		for _, kind := range []string{"observer", "forwarder"} {
			t.Run(kind, func(t *testing.T) {
				synctest.Test(t, func(t *testing.T) {
					var next func(context.Context) (json.RawMessage, error)
					var publish func()
					const expected = `{"kind":"listening"}`
					if kind == "observer" {
						observer := newObserver(t.Context())
						defer observer.Close()
						observer.read = func() (any, error) {
							return ForwardState{Kind: "listening"}, nil
						}
						next, publish = observer.Next, observer.notify
					} else {
						forwarder := &Forwarder{
							ctx: t.Context(), next: make(chan struct{}, 1), wake: make(chan struct{}, 1),
							state: ForwardState{Kind: "listening"}, version: 1, observed: 1,
						}
						next = forwarder.Next
						publish = func() { forwarder.setState(ForwardState{Kind: "listening"}) }
					}
					type result struct {
						value json.RawMessage
						err   error
					}
					first := make(chan result, 1)
					go func() {
						value, err := next(t.Context())
						first <- result{value: value, err: err}
					}()
					synctest.Wait()
					ctx, cancel := context.WithCancel(t.Context())
					defer cancel()
					queued := make(chan result, 1)
					go func() {
						value, err := next(ctx)
						queued <- result{value: value, err: err}
					}()
					synctest.Wait()
					cancel()
					synctest.Wait()
					select {
					case value := <-queued:
						if !errors.Is(value.err, context.Canceled) || len(value.value) != 0 {
							t.Fatalf("queued pull cancellation: %s, %v", value.value, value.err)
						}
					default:
						t.Fatal("queued pull did not complete after its request was cancelled")
					}
					select {
					case value := <-first:
						t.Fatalf("queued cancellation completed the active pull: %s, %v", value.value, value.err)
					default:
					}
					publish()
					synctest.Wait()
					select {
					case value := <-first:
						if value.err != nil || string(value.value) != expected {
							t.Fatalf("active pull lost its update: %s, %v", value.value, value.err)
						}
					default:
						t.Fatal("active pull did not receive its retained update")
					}
				})
			})
		}
	})
	t.Run("forwarding_shutdown_retains_final_state", func(t *testing.T) {
		synctest.Test(t, func(t *testing.T) {
			ctx, cancel := context.WithCancel(t.Context())
			defer cancel()
			forwarder := &Forwarder{
				ctx: ctx, next: make(chan struct{}, 1), wake: make(chan struct{}, 1),
				state: ForwardState{Kind: "listening"}, version: 1, observed: 1,
			}
			type result struct {
				value json.RawMessage
				err   error
			}
			active := make(chan result, 1)
			go func() {
				value, err := forwarder.Next(t.Context())
				active <- result{value: value, err: err}
			}()
			synctest.Wait()
			queued := make(chan error, 1)
			go func() {
				_, err := forwarder.Next(t.Context())
				queued <- err
			}()
			synctest.Wait()
			cancel()
			synctest.Wait()
			select {
			case err := <-queued:
				if !errors.Is(err, context.Canceled) {
					t.Fatalf("queued pull lifetime cancellation: %v", err)
				}
			default:
				t.Fatal("queued pull did not complete after its owner stopped")
			}
			select {
			case value := <-active:
				t.Fatalf("shutdown discarded the pending final state: %s, %v", value.value, value.err)
			default:
			}
			forwarder.setState(ForwardState{Kind: "closed"})
			synctest.Wait()
			select {
			case value := <-active:
				if value.err != nil || string(value.value) != `{"kind":"closed"}` {
					t.Fatalf("final state was not delivered: %s, %v", value.value, value.err)
				}
			default:
				t.Fatal("active pull did not receive the final state")
			}
			if _, err := forwarder.Next(t.Context()); err == nil {
				t.Fatal("closed forwarding state was delivered more than once")
			}
		})
	})
}
