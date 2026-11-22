package live

import (
	"github.com/sailry/sailry-harness/internal/assembly/desktop"
	"fmt"
	"path/filepath"
	"reflect"
	"slices"
	"testing"

	"github.com/egoist/mygo/ui"

	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/preferences"
	"github.com/sailry/sailry-harness/internal/desktop/settings"
	"github.com/sailry/sailry-harness/internal/desktop/shell"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/node/providers"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func billingRate(value uint32) *uint32 { return &value }

func billingRateText(bps *uint32) string {
	if bps == nil {
		return "nil"
	}
	return fmt.Sprint(*bps)
}

// The settings show an API-key provider's rate as saved, nil and zero
// apart, and no rate for a sign-in.
func TestProviderBillingRateProjection(t *testing.T) {
	listing := providers.Listing(protocol.ConversationPresetsCatalog{}, false)
	for _, rate := range []*uint32{nil, billingRate(0), billingRate(protocol.DefaultBillingRateBps), billingRate(7500)} {
		provider := providerRouteFixture()
		provider.BillingRateBps = rate
		channel, _ := projectProviderRouteFixture(t, provider, listing)
		if !reflect.DeepEqual(rate, channel.BillingRateBps) {
			t.Fatalf("rate %s projected as %s", billingRateText(rate), billingRateText(channel.BillingRateBps))
		}
		if rate != nil && channel.BillingRateBps == provider.BillingRateBps {
			t.Fatal("the projection shares the snapshot's rate")
		}
	}
	provider := providerRouteFixture()
	provider.Preset, provider.Authentication, provider.BillingRateBps = nil, protocol.AuthenticationChatGptOAuth, billingRate(7500)
	if channel, _ := projectProviderRouteFixture(t, provider, listing); channel.Auth != viewmodel.AuthOAuth || channel.BillingRateBps != nil {
		t.Fatalf("a sign-in projected rate %s (auth %v)", billingRateText(channel.BillingRateBps), channel.Auth)
	}
}

// The protocol draft carries the editor's rate for an API-key provider,
// clearing a saved one the draft leaves out, and none for a sign-in.
func TestProviderBillingRateDraft(t *testing.T) {
	keyed := providerRouteFixture()
	keyed.BillingRateBps = billingRate(7500)
	oauth := providerRouteFixture()
	oauth.ID, oauth.Preset, oauth.Authentication, oauth.BillingRateBps = protocol.ProviderID{4}, nil, protocol.AuthenticationChatGptOAuth, nil
	r := &Runtime{providerDraftBases: map[string]protocol.ConversationProvider{
		keyed.ID.String() + "/8": keyed, oauth.ID.String() + "/8": oauth,
	}}
	draft := func(provider protocol.ConversationProvider, rate *uint32) protocol.ConversationProvider {
		channel := viewmodel.Channel{ID: provider.ID.String(), API: string(provider.Api), Name: provider.Name, Endpoint: provider.Endpoint, Default: 1, BillingRateBps: rate}
		for _, model := range provider.Models {
			channel.Models = append(channel.Models, channelModel(model))
		}
		return r.providerDraft(settings.ProviderDraft{Channel: channel, ExpectedRevision: 8})
	}
	for _, rate := range []*uint32{nil, billingRate(0), billingRate(protocol.DefaultBillingRateBps), billingRate(1234)} {
		got := draft(keyed, rate)
		if !reflect.DeepEqual(rate, got.BillingRateBps) || rate != nil && got.BillingRateBps == rate {
			t.Fatalf("draft rate %s became %s", billingRateText(rate), billingRateText(got.BillingRateBps))
		}
	}
	if *r.providerDraftBases[keyed.ID.String()+"/8"].BillingRateBps != 7500 {
		t.Fatal("a draft changed the captured provider's rate")
	}
	if got := draft(oauth, billingRate(5000)); got.BillingRateBps != nil {
		t.Fatalf("a sign-in draft carried rate %s", billingRateText(got.BillingRateBps))
	}
}

// A Host whose Node predates the rate saves the rest of the provider and
// returns it without the rate. The save is not reported as done; the
// retry expects the returned revision. Nil and an explicit 100% differ.
func TestProviderBillingRateDroppedByOlderNode(t *testing.T) {
	for _, tc := range []struct {
		name     string
		rate     *uint32
		previous *uint32
		drop     bool
		kept     bool
	}{
		{name: "current Node keeps nil", kept: true},
		{name: "current Node preserves a rate on omission", previous: billingRate(7500), kept: true},
		{name: "current Node keeps zero", rate: billingRate(0), kept: true},
		{name: "current Node keeps 75%", rate: billingRate(7500), kept: true},
		{name: "older Node with nil", drop: true, kept: true},
		{name: "older Node drops zero", rate: billingRate(0), drop: true},
		{name: "older Node drops 75%", rate: billingRate(7500), drop: true},
		{name: "older Node drops an explicit 100%", rate: billingRate(protocol.DefaultBillingRateBps), drop: true},
	} {
		t.Run(tc.name, func(t *testing.T) {
			node := protocol.NodeID{1}
			provider := providerRouteFixture()
			provider.BillingRateBps = tc.previous
			snapshot := protocol.DefaultSnapshot()
			snapshot.Node, snapshot.Providers = node, []protocol.ConversationProvider{provider}
			transport := &conversationTestTransport{snapshot: snapshot}
			transport.handle = func(request protocol.Request) (link.Response, bool) {
				if request.Command.Kind != "save_provider" {
					return link.Response{}, false
				}
				input, _ := detail[protocol.CommandSaveProviderPayload](request.Command.Data)
				changed := input.Provider
				if tc.drop {
					changed.BillingRateBps = nil
				} else if changed.BillingRateBps == nil {
					changed.BillingRateBps = transport.snapshot.Providers[0].BillingRateBps
				}
				changed.Revision = transport.snapshot.Providers[0].Revision + 1
				transport.snapshot.Cursor++
				transport.snapshot.Providers = []protocol.ConversationProvider{changed}
				return link.Response{Output: protocol.Output{Kind: "provider", Data: changed}}, true
			}
			controller := client.New(transport)
			if err := controller.Start(t.Context()); err != nil {
				t.Fatal(err)
			}
			t.Cleanup(func() { _ = controller.Close() })
			root, err := NewWithAssembly(t.Context(), controller, preferences.Open(filepath.Join(t.TempDir(), "preferences.json")), desktop.Harness())
			if err != nil {
				t.Fatal(err)
			}
			t.Cleanup(func() { _ = root.Close() })
			root.App.Page = shell.SettingsPage
			root.App.Settings.Open(settings.Providers)
			var shown []ui.Toast
			window := ui.NewTester(func(c *ui.Context) {
				root.drain(c)
				ui.ToastViewportBase(c, func(_ ui.Element, toasts []ui.Toast) { shown = slices.Clone(toasts) })
			}, 320, 240)
			window.SetLocale("en")
			settle := func() {
				for range 5 {
					root.workers.Wait()
					window.Frame()
				}
			}
			expectedRevisions := func() (revisions []uint64) {
				transport.mu.Lock()
				defer transport.mu.Unlock()
				for _, request := range transport.requests {
					if request.Command.Kind == "save_provider" {
						input, _ := detail[protocol.CommandSaveProviderPayload](request.Command.Data)
						revisions = append(revisions, input.ExpectedRevision)
					}
				}
				return revisions
			}
			settle()
			channel := viewmodel.Channel{Node: node.String(), Targets: []string{node.String()}, ID: provider.ID.String(), Revision: provider.Revision,
				Name: provider.Name, API: string(provider.Api), Endpoint: provider.Endpoint, Auth: viewmodel.AuthKey, Enabled: true, KeySet: true, Default: 1, BillingRateBps: tc.rate}
			for _, model := range provider.Models {
				channel.Models = append(channel.Models, channelModel(model))
			}
			draft := settings.ProviderDraft{Channel: channel, Targets: []string{node.String()}, ExpectedRevision: provider.Revision}
			var result settings.ProviderSaveResult
			root.saveManagedProvider(draft, func(saved settings.ProviderSaveResult) { result = saved })
			settle()
			if expected := expectedRevisions(); len(expected) != 1 || expected[0] != provider.Revision {
				t.Fatalf("save requests expected %v", expected)
			}
			saved := controller.Snapshot().Providers[0]
			if tc.kept {
				want := tc.rate
				if want == nil {
					want = tc.previous
				}
				if !slices.Equal(result.Succeeded, []string{node.String()}) || len(result.Failed) != 0 || !reflect.DeepEqual(want, saved.BillingRateBps) {
					t.Fatalf("save result %+v, saved rate %s", result, billingRateText(saved.BillingRateBps))
				}
				return
			}
			if len(result.Succeeded) != 0 || !slices.Equal(result.Failed, []string{node.String()}) || result.Resume == nil ||
				result.TargetRevisions[node.String()] != saved.Revision {
				t.Fatalf("a dropped rate was reported as %+v", result)
			}
			if !slices.ContainsFunc(shown, func(toast ui.Toast) bool { return toast.Title == i18n.In("en", "provider_billing_rate_unsupported") }) ||
				slices.ContainsFunc(shown, func(toast ui.Toast) bool { return toast.Title == i18n.In("en", "provider_save_failed") }) {
				t.Fatalf("a dropped rate showed %+v", shown)
			}
			// The editor's retry of the same draft is sent again at the revision
			// the first save returned, not reused as done nor sent stale.
			resume := result.Resume
			result = settings.ProviderSaveResult{}
			resume(draft, func(saved settings.ProviderSaveResult) { result = saved })
			settle()
			if expected := expectedRevisions(); len(expected) != 2 || expected[1] != saved.Revision || len(result.Succeeded) != 0 || !slices.Equal(result.Failed, []string{node.String()}) {
				t.Fatalf("retry expected %v, result %+v", expectedRevisions(), result)
			}
		})
	}
}
