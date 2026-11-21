package live

import (
	"context"
	"strings"
	"sync/atomic"
	"testing"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/desktop/composer"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/preferences"
	"github.com/sailry/sailry-harness/internal/desktop/settings"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
)

type providerSwitchTransport struct {
	selectionTestTransport
	subscriptions atomic.Int32
	reads         chan chan link.Response
}

func (s *providerSwitchTransport) Subscribe(ctx context.Context, topic protocol.Topic) (link.Subscription, error) {
	if topic.Kind == "node" && s.subscriptions.Add(1) != 1 {
		return nil, &protocol.Fault{Code: protocol.ErrorCodeInvalidRequest, Message: "unexpected second Node subscription"}
	}
	return s.selectionTestTransport.Subscribe(ctx, topic)
}

func (s *providerSwitchTransport) Dispatch(ctx context.Context, request protocol.Request) (link.Admission, error) {
	if request.Command.Kind != "read_provider_directory" {
		return s.conversationTestTransport.Dispatch(ctx, request)
	}
	completion := make(chan link.Response, 1)
	select {
	case s.reads <- completion:
		return link.Admission{Receipt: protocol.Receipt{ID: request.ID}, Completion: completion}, nil
	case <-ctx.Done():
		return link.Admission{}, ctx.Err()
	}
}

func TestProviderSaveWhileSwitchingSession(t *testing.T) {
	for _, test := range []struct {
		name     string
		width    int
		settings bool
	}{{"composer", 960, false}, {"session settings", 400, true}} {
		t.Run(test.name, func(t *testing.T) {
			snapshot := selectionSnapshot("Before saving", "After saving")
			snapshot.ProviderDirectory = true
			old := providerRouteFixture()
			old.Enabled = true
			snapshot.Providers = []protocol.ConversationProvider{old}
			for i := range snapshot.Sessions {
				snapshot.Sessions[i].Config.Provider = old.ID
				snapshot.Sessions[i].Config.Model = old.Models[0].ID
			}
			source := &providerSwitchTransport{reads: make(chan chan link.Response, 1)}
			source.snapshot = snapshot
			source.node = &conversationTestSubscription{updates: make(chan protocol.Update, 1), closed: make(chan struct{})}
			source.node.updates <- protocol.Update{Kind: "snapshot", Data: snapshot}
			var saved protocol.ConversationProvider
			source.handle = func(request protocol.Request) (link.Response, bool) {
				if request.Command.Kind != "save_provider" {
					return link.Response{}, false
				}
				input, _ := detail[protocol.CommandSaveProviderPayload](request.Command.Data)
				saved = input.Provider
				saved.Revision = 1
				return link.Response{Output: protocol.Output{Kind: "provider", Data: saved}}, true
			}
			controller := client.New(source)
			if err := controller.Start(t.Context()); err != nil {
				t.Fatal(err)
			}
			defer controller.Close()
			r, err := NewWithPreferences(t.Context(), controller, preferences.Open(t.TempDir()+"/preferences.json"))
			if err != nil {
				t.Fatal(err)
			}
			defer r.Close()
			first, second := snapshot.Sessions[0].ID.String(), snapshot.Sessions[1].ID.String()
			r.App.OpenConversation(first)
			window := ui.NewTesterWith(func(c *ui.Context) {
				r.drain(c)
				id := r.App.FocusedConversation()
				cv, state := r.App.Data.Find(id), r.App.Composer(id)
				ui.Column(c).Fill().Justify(ui.End).Children(func() {
					ctx, actions := r.App.Hooks.Conversation(c, cv, state)
					composer.View(c, state, ctx, actions)
				})
				r.finishFrame()
				ui.ToastViewportBase(c, func(ui.Element, []ui.Toast) {})
			}, test.width, 700, ui.TesterOptions{Preferences: ui.Preferences{ReduceMotion: true, ReduceTransparency: true}})
			window.SetLocale("en")
			window.Frame()
			r.App.Composer(first).Draft = "Keep this input"
			r.App.Settings.Open(settings.Providers)
			completed := false
			r.App.Settings.ProviderActions.Save(settings.ProviderDraft{Channel: viewmodel.Channel{
				ID: protocol.ProviderID{8}.String(), Name: "New channel", API: "responses", Endpoint: "https://models.example.test/v1", Auth: viewmodel.AuthKey, Enabled: true,
				Models: []viewmodel.ChannelModel{{ID: "new-model", Context: 128000, Output: 16384}},
			}}, func(outcome settings.ProviderSaveResult) { completed = len(outcome.Succeeded) == 1 })
			var completion chan link.Response
			select {
			case completion = <-source.reads:
			case <-time.After(2 * time.Second):
				t.Fatal("save did not request its provider directory")
			}
			r.App.OpenConversation(second)
			window.Frame()
			if r.App.FocusedConversation() != second || completed || r.App.Composer(first).Draft != "Keep this input" {
				t.Fatal("pending directory read interfered with switching sessions")
			}
			completion <- link.Response{Output: protocol.Output{Kind: "provider_directory", Data: protocol.ProviderDirectory{
				Node: snapshot.Node, Cursor: snapshot.Cursor + 1, Providers: []protocol.ConversationProvider{old, saved},
			}}}
			r.workers.Wait()
			window.Frame()
			if !completed || source.subscriptions.Load() != 1 || controller.Snapshot().Cursor != snapshot.Cursor {
				t.Fatal("save did not finish on the existing observation with its events held back")
			}
			click := func(role ui.Role, name string, prefix bool) {
				t.Helper()
				for _, row := range window.AccessAll() {
					if row.Role == role && (row.Name == name || prefix && strings.HasPrefix(row.Name, name)) {
						window.ClickAt(row.Bounds.X+row.Bounds.W/2, row.Bounds.Y+row.Bounds.H/2)
						window.Frame()
						return
					}
				}
				t.Fatalf("missing control %q", name)
			}
			if test.settings {
				click(ui.RoleButton, i18n.In("en", "composer_settings")+" · ", true)
			} else {
				click(ui.RoleButton, i18n.In("en", "composer_model_settings")+": ", true)
			}
			click(ui.RoleButton, r.App.Composer(second).Chosen(r.App.Data.Models).Name, false)
			if _, ok := window.AccessByRole(ui.RoleButton, "new-model"); !ok {
				click(ui.RoleDisclosure, "New channel", false)
			}
			if _, ok := window.AccessByRole(ui.RoleButton, "new-model"); !ok {
				t.Fatal("saved model was missing from the model picker in the newly focused session")
			}
		})
	}
}
