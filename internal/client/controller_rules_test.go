package client

import (
	"context"
	"encoding/json"
	"errors"
	"testing"
	"testing/synctest"

	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestControllerRulesAndInbox(t *testing.T) {
	t.Parallel()
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
