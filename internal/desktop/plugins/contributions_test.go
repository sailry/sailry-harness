package plugins

import (
	"context"
	"encoding/json"
	"errors"
	"reflect"
	"strings"
	"testing"
	"time"

	"github.com/sailry/sailry-harness/internal/protocol"
)

func contributionFixture(t *testing.T, declarations string) (*Registry, protocol.PluginReference, ContributionHooks) {
	t.Helper()
	scope := protocol.DefaultPluginContext()
	scope.Surface = protocol.PluginDesktopSurface{Kind: "composer"}
	worktree, session := protocol.NewWorktreeID(), protocol.NewSessionID()
	scope.Worktree, scope.Session = &worktree, &session
	registry := NewRegistry(RegistryOptions{Node: protocol.NodeID{1}, Context: scope})
	t.Cleanup(func() { _ = registry.Close() })
	reference := protocol.PluginReference{Name: "fixture", Digest: strings.Repeat("a", 64)}
	var entries []protocol.PluginUiContribution
	if err := json.Unmarshal([]byte(declarations), &entries); err != nil {
		t.Fatal(err)
	}
	hooks, err := registry.Hooks(reference, entries)
	if err != nil {
		t.Fatal(err)
	}
	return registry, reference, hooks
}

func contributionPublish(t *testing.T, hooks ContributionHooks, body string) {
	t.Helper()
	if err := hooks.Publish(json.RawMessage(body)); err != nil {
		t.Fatal(err)
	}
}

func contributionNext(t *testing.T, hooks ContributionHooks) protocol.PluginUiEvent {
	t.Helper()
	ctx, cancel := context.WithTimeout(t.Context(), time.Second)
	defer cancel()
	body, err := hooks.Next(ctx)
	if err != nil {
		t.Fatal(err)
	}
	var event protocol.PluginUiEvent
	if err := json.Unmarshal(body, &event); err != nil {
		t.Fatal(err)
	}
	return event
}

func TestContributionSlots(t *testing.T) {
	t.Run("declarations are bounded and capture independent data", func(t *testing.T) {
		registry, reference, _ := contributionFixture(t, `[{"id":"resource","slot":"context","kind":"picker","label":{"label":"Resource"},"handler":"select"}]`)
		for _, body := range []string{
			`[{"id":"same","slot":"composer","kind":"button","label":{"label":"One"},"handler":"one"},{"id":"same","slot":"composer","kind":"button","label":{"label":"Two"},"handler":"two"}]`,
			`[{"id":"bad","slot":"status","kind":"toggle","label":{"label":"Bad"},"handler":"toggle"}]`,
			`[{"id":"bad","slot":"provider","kind":"button","label":{"label":"Bad"},"handler":"button"}]`,
			`[{"id":"bad","slot":"statistics","kind":"metric","label":{"label":"Bad"},"width":239}]`,
			`[{"id":"bad","slot":"commands","kind":"button","label":{"label":"Bad"},"handler":"invoke","command":{"name":"model","kind":"invoke"}}]`,
			`[{"id":"one","slot":"commands","kind":"button","label":{"label":"One"},"handler":"one","command":{"name":"same","kind":"invoke"}},{"id":"two","slot":"commands","kind":"button","label":{"label":"Two"},"handler":"two","command":{"name":"same","kind":"invoke"}}]`,
			`[{"id":"one","slot":"header","kind":"popover","label":{"label":"One"},"handler":"one"},{"id":"two","slot":"composer","kind":"popover","label":{"label":"Two"},"handler":"two"}]`,
		} {
			var declarations []protocol.PluginUiContribution
			if err := json.Unmarshal([]byte(body), &declarations); err != nil {
				t.Fatal(err)
			}
			if _, err := registry.Hooks(reference, declarations); err == nil {
				t.Fatalf("invalid declarations were accepted: %s", body)
			}
		}
		var declarations []protocol.PluginUiContribution
		if err := json.Unmarshal([]byte(`[{"id":"copy","slot":"context","kind":"button","label":{"label":"Captured","locales":{"en":"Captured"}},"handler":"copy"}]`), &declarations); err != nil {
			t.Fatal(err)
		}
		many := make([]protocol.PluginUiContribution, 33)
		for index := range many {
			many[index] = declarations[0]
			many[index].ID = strings.Repeat("a", index+1)
		}
		if _, err := registry.Hooks(reference, many); err == nil {
			t.Fatal("declaration entry limit was not enforced")
		}
		hooks, err := registry.Hooks(reference, declarations)
		if err != nil {
			t.Fatal(err)
		}
		declarations[0].Label.Locales["en"] = "Changed"
		contributionPublish(t, hooks, `[{"id":"copy"}]`)
		if err := registry.SetActivity(reference, true, false); err != nil {
			t.Fatal(err)
		}
		entries := registry.Entries("context")
		if len(entries) != 1 || entries[0].Declaration.Label.Locales["en"] != "Captured" {
			t.Fatal("declaration followed caller mutation")
		}
		entries[0].Declaration.Label.Locales["en"] = "Mutated snapshot"
		if registry.Entries("context")[0].Declaration.Label.Locales["en"] != "Captured" {
			t.Fatal("presentation snapshot mutated retained declaration")
		}
	})
	t.Run("publication activity loading and atomic validation gate actions", func(t *testing.T) {
		registry, reference, hooks := contributionFixture(t, `[{"id":"resource","slot":"context","kind":"picker","label":{"label":"Resource"},"handler":"select"}]`)
		key, ok := registry.Key(reference, "resource")
		if !ok || registry.Ready(reference) || len(registry.Entries("context")) != 0 {
			t.Fatal("unpublished control was exposed")
		}
		if err := registry.Invoke(key, "search", json.RawMessage(`"query"`)); err == nil {
			t.Fatal("inactive unpublished control invoked")
		}
		contributionPublish(t, hooks, `[{"id":"resource","loading":true,"label":{"label":"Selected checkout"}}]`)
		if err := registry.SetActivity(reference, true, false); err != nil {
			t.Fatal(err)
		}
		if entries := registry.Entries("context"); len(entries) != 1 || entries[0].State.Enabled || !entries[0].State.Loading {
			t.Fatal("loading control became actionable")
		}
		if err := registry.Invoke(key, "search", json.RawMessage(`"query"`)); err == nil {
			t.Fatal("loading control invoked")
		}
		contributionPublish(t, hooks, `[{"id":"resource","label":{"label":"Selected checkout"}}]`)
		before := registry.Entries("context")
		for _, body := range []string{
			`[{"id":"resource","label":{"label":"Partial"}},{"id":"missing"}]`,
			`[{"id":"resource","reply_to":1}]`,
			`[{"id":"resource","choices":[{"id":"same","label":{"label":"One"}},{"id":"same","label":{"label":"Two"}}]}]`,
			`[{"id":"resource","value":[]}]`,
			`[{"id":"resource","details":[{"label":{"label":"Forbidden"},"value":"Detail"}]}]`,
		} {
			if hooks.Publish(json.RawMessage(body)) == nil || !reflect.DeepEqual(before, registry.Entries("context")) {
				t.Fatal("invalid batch partially replaced retained state")
			}
		}
		if err := registry.SetActivity(reference, false, false); err != nil {
			t.Fatal(err)
		}
		entries := registry.Entries("context")
		if len(entries) != 1 || entries[0].State.Enabled || entries[0].State.Label.Label != "Selected checkout" {
			t.Fatal("disconnection lost the retained disabled label")
		}
	})
	t.Run("interaction queue is exactly sixteen without consuming failed sequence", func(t *testing.T) {
		registry, reference, hooks := contributionFixture(t, `[{"id":"resource","slot":"context","kind":"picker","label":{"label":"Resource"},"handler":"select"}]`)
		contributionPublish(t, hooks, `[{"id":"resource"}]`)
		if err := registry.SetActivity(reference, true, false); err != nil {
			t.Fatal(err)
		}
		key, _ := registry.Key(reference, "resource")
		for index := 0; index < 16; index++ {
			if err := registry.Invoke(key, "search", json.RawMessage(`"query"`)); err != nil {
				t.Fatal(err)
			}
		}
		var fault *protocol.Fault
		if err := registry.Invoke(key, "search", json.RawMessage(`"overflow"`)); !errors.As(err, &fault) || fault.Code != protocol.ErrorCodeBusy {
			t.Fatal("interaction queue accepted its seventeenth pending item")
		}
		if event := contributionNext(t, hooks); event.Sequence != 1 {
			t.Fatal("first interaction sequence changed")
		}
		if err := registry.Invoke(key, "search", json.RawMessage(`"available"`)); err != nil {
			t.Fatal(err)
		}
		for sequence := uint64(2); sequence <= 17; sequence++ {
			if event := contributionNext(t, hooks); event.Sequence != sequence {
				t.Fatal("rejected enqueue consumed an interaction sequence")
			}
		}
	})
	t.Run("newer replies win and actions stay busy until their reply", func(t *testing.T) {
		registry, reference, hooks := contributionFixture(t, `[{"id":"resource","slot":"context","kind":"picker","label":{"label":"Resource"},"handler":"select"}]`)
		contributionPublish(t, hooks, `[{"id":"resource"}]`)
		if err := registry.SetActivity(reference, true, false); err != nil {
			t.Fatal(err)
		}
		key, _ := registry.Key(reference, "resource")
		for _, query := range []string{`"a"`, `"ab"`} {
			if err := registry.Invoke(key, "search", json.RawMessage(query)); err != nil {
				t.Fatal(err)
			}
			_ = contributionNext(t, hooks)
		}
		contributionPublish(t, hooks, `[{"id":"resource","reply_to":2,"choices":[{"id":"ab","label":{"label":"Latest"}}]}]`)
		contributionPublish(t, hooks, `[{"id":"resource","reply_to":1,"choices":[{"id":"a","label":{"label":"Stale"}}]}]`)
		entries := registry.Entries("context")
		if choices := entries[0].State.Choices; choices == nil || len(*choices) != 1 || (*choices)[0].ID != "ab" {
			t.Fatal("stale search reply replaced current choices")
		}
		if err := registry.Invoke(key, "change", json.RawMessage(`"a"`)); err == nil {
			t.Fatal("stale choice was selected")
		}
		if err := registry.Invoke(key, "change", json.RawMessage(`"ab"`)); err != nil {
			t.Fatal(err)
		}
		event := contributionNext(t, hooks)
		if event.Sequence != 3 || event.Handler != "select" {
			t.Fatal("selection lost its captured handler")
		}
		contributionPublish(t, hooks, `[{"id":"resource","enabled":true}]`)
		if registry.Entries("context")[0].State.Enabled {
			t.Fatal("unsolicited state cleared an outstanding interaction")
		}
		contributionPublish(t, hooks, `[{"id":"resource","reply_to":3}]`)
		if !registry.Entries("context")[0].State.Enabled {
			t.Fatal("matching reply did not release busy state")
		}
	})
	t.Run("restart retires queue callbacks and replies while retaining labels", func(t *testing.T) {
		declarations := `[{"id":"resource","slot":"context","kind":"picker","label":{"label":"Resource"},"handler":"select"}]`
		registry, reference, previous := contributionFixture(t, declarations)
		contributionPublish(t, previous, `[{"id":"resource","label":{"label":"Retained"}}]`)
		if err := registry.SetActivity(reference, true, false); err != nil {
			t.Fatal(err)
		}
		oldKey, _ := registry.Key(reference, "resource")
		if err := registry.Invoke(oldKey, "search", json.RawMessage(`"old"`)); err != nil {
			t.Fatal(err)
		}
		var entries []protocol.PluginUiContribution
		if err := json.Unmarshal([]byte(declarations), &entries); err != nil {
			t.Fatal(err)
		}
		current, err := registry.Hooks(reference, entries)
		if err != nil {
			t.Fatal(err)
		}
		if previous.Publish(json.RawMessage(`[{"id":"resource"}]`)) == nil || registry.Invoke(oldKey, "search", json.RawMessage(`"old"`)) == nil {
			t.Fatal("retired generation retained authority")
		}
		if _, err := previous.Next(t.Context()); err == nil {
			t.Fatal("retired queue delivered an interaction")
		}
		retained := registry.Entries("context")
		if len(retained) != 1 || retained[0].State.Label.Label != "Retained" || !retained[0].State.Loading || retained[0].State.Enabled {
			t.Fatal("restart lost presentation or enabled unpublished state")
		}
		if current.Publish(json.RawMessage(`[{"id":"resource","reply_to":1}]`)) == nil {
			t.Fatal("restart accepted an earlier generation's reply")
		}
		contributionPublish(t, current, `[{"id":"resource"}]`)
		newKey, _ := registry.Key(reference, "resource")
		if newKey.Generation == oldKey.Generation {
			t.Fatal("replacement generation reused callback authority")
		}
		other := NewRegistry(RegistryOptions{Node: registry.node, Context: registry.scope})
		t.Cleanup(func() { _ = other.Close() })
		otherHooks, err := other.Hooks(reference, entries)
		if err != nil {
			t.Fatal(err)
		}
		contributionPublish(t, otherHooks, `[{"id":"resource"}]`)
		if err := other.SetActivity(reference, true, false); err != nil {
			t.Fatal(err)
		}
		otherKey, _ := other.Key(reference, "resource")
		if otherKey.Generation == newKey.Generation || other.Invoke(newKey, "search", json.RawMessage(`"stale owner"`)) == nil {
			t.Fatal("another presentation accepted a captured contribution key")
		}
		if err := other.Invoke(otherKey, "search", json.RawMessage(`"current owner"`)); err != nil {
			t.Fatal(err)
		}
		cancelled, cancel := context.WithCancel(t.Context())
		cancel()
		if _, err := current.Next(cancelled); !errors.Is(err, context.Canceled) {
			t.Fatal("Next ignored its cancelled observer")
		}
		if err := registry.Close(); err != nil {
			t.Fatal(err)
		}
		if _, err := current.Next(t.Context()); err == nil {
			t.Fatal("closed registry retained a pending event observer")
		}
	})
	t.Run("provider state retains exact public scope and removes replacements", func(t *testing.T) {
		registry, reference, hooks := contributionFixture(t, `[{"id":"usage","slot":"provider","kind":"metric","label":{"label":"Usage"}}]`)
		first := protocol.PluginUiProviderScope{Provider: protocol.NewProviderID(), Revision: "3:1", Credential: protocol.CredentialRef{Node: protocol.NodeID{1}, ID: protocol.NewCredentialID()}}
		second := first
		second.Provider = protocol.NewProviderID()
		if err := registry.SetProviderScopes(reference, []protocol.PluginUiProviderScope{first, second}); err != nil {
			t.Fatal(err)
		}
		if err := registry.SetActivity(reference, true, false); err != nil {
			t.Fatal(err)
		}
		publish := func(scopes ...protocol.PluginUiProviderScope) error {
			states := []protocol.PluginUiState{}
			for _, scope := range scopes {
				state := protocol.DefaultPluginUiState()
				state.ID, state.Scope, state.Visible, state.Enabled, state.Value = "usage", &scope, true, true, json.RawMessage(`"35%"`)
				states = append(states, state)
			}
			body, err := json.Marshal(states)
			if err != nil {
				return err
			}
			return hooks.Publish(body)
		}
		if err := publish(first, second); err != nil {
			t.Fatal(err)
		}
		if len(registry.ProviderEntries(first)) != 1 || publish(first, first) == nil {
			t.Fatal("provider composite key or duplicate fence failed")
		}
		replaced := first
		replaced.Revision, replaced.Credential.ID = "4:1", protocol.NewCredentialID()
		if err := registry.SetProviderScopes(reference, []protocol.PluginUiProviderScope{replaced, second}); err != nil {
			t.Fatal(err)
		}
		if len(registry.ProviderEntries(first)) != 0 || publish(first) == nil || publish(replaced) != nil || len(registry.ProviderEntries(second)) != 1 {
			t.Fatal("provider replacement leaked or removed another provider's state")
		}
		invalid := replaced
		invalid.Revision = "04:1"
		if registry.SetProviderScopes(reference, []protocol.PluginUiProviderScope{invalid}) == nil {
			t.Fatal("noncanonical provider revision was accepted")
		}
		if err := registry.SetProviderScopes(reference, nil); err != nil || publish(replaced) == nil || len(registry.ProviderEntries(replaced)) != 0 {
			t.Fatal("credential invalidation retained provider authority")
		}
	})
	t.Run("Status publish is refused and local popover events stay scoped", func(t *testing.T) {
		registry, reference, hooks := contributionFixture(t, `[{"id":"badge","slot":"status","kind":"popover","label":{"label":"Badge"},"handler":"badge"}]`)
		var declaration protocol.PluginUiContribution
		if err := json.Unmarshal([]byte(`{"id":"badge","slot":"status","kind":"popover","label":{"label":"Badge"},"handler":"badge"}`), &declaration); err != nil {
			t.Fatal(err)
		}
		state := protocol.DefaultPluginUiState()
		state.ID = declaration.ID
		if !declaration.Valid() || !state.Valid(declaration) || !protocol.ValidPluginUiStatusState(state, declaration) {
			t.Fatal("shared Status validation rejected declared pending state")
		}
		for _, scenario := range []struct {
			name   string
			change func(*protocol.PluginUiState, *protocol.PluginUiContribution)
		}{
			{"reply", func(state *protocol.PluginUiState, _ *protocol.PluginUiContribution) { state.ReplyTo = new(uint64(1)) }},
			{"provider scope", func(state *protocol.PluginUiState, _ *protocol.PluginUiContribution) {
				state.Scope = &protocol.PluginUiProviderScope{Revision: "1:1"}
			}},
			{"value", func(state *protocol.PluginUiState, _ *protocol.PluginUiContribution) {
				state.Value = json.RawMessage("true")
			}},
			{"choices", func(state *protocol.PluginUiState, _ *protocol.PluginUiContribution) {
				state.Choices = new([]protocol.PluginUiChoice{})
			}},
			{"other slot", func(_ *protocol.PluginUiState, declaration *protocol.PluginUiContribution) {
				declaration.Slot = "composer"
			}},
			{"other declaration", func(state *protocol.PluginUiState, _ *protocol.PluginUiContribution) { state.ID = "other" }},
		} {
			changed, entry := state, declaration
			scenario.change(&changed, &entry)
			if protocol.ValidPluginUiStatusState(changed, entry) {
				t.Fatalf("%s escaped shared Status validation", scenario.name)
			}
		}
		worktree, session := protocol.NewWorktreeID(), protocol.NewSessionID()
		if (protocol.PluginUiStatusScope{}).Valid() || !(protocol.PluginUiStatusScope{Worktree: &worktree}).Valid() ||
			!(protocol.PluginUiStatusScope{Session: &session}).Valid() {
			t.Fatal("shared Status scope validation differs from its contract")
		}
		if err := registry.SetActivity(reference, true, false); err != nil {
			t.Fatal(err)
		}
		if hooks.Publish(json.RawMessage(`[{"id":"badge"}]`)) == nil || len(registry.Entries("status")) != 0 {
			t.Fatal("controller replaced execution Node Status state")
		}
		key, _ := registry.Key(reference, "badge")
		if registry.Invoke(key, "invoke", json.RawMessage("null")) == nil {
			t.Fatal("ordinary contribution action bypassed Node Status ownership")
		}
		if err := registry.ForwardStatus(key, "invoke", json.RawMessage("null")); err != nil {
			t.Fatal(err)
		}
		if event := contributionNext(t, hooks); event.ID != "badge" || event.Kind != "invoke" {
			t.Fatal("local Status interaction changed its captured declaration")
		}
		if err := hooks.Close("badge"); err != nil {
			t.Fatal(err)
		}
		if !reflect.DeepEqual(registry.TakeClosed(reference), []string{"badge"}) || len(registry.TakeClosed(reference)) != 0 || hooks.Close("missing") == nil {
			t.Fatal("local Status close lost declaration scope or consumption")
		}
	})
	t.Run("hidden intents preserve values and duplicate command names disable search actions", func(t *testing.T) {
		registry, reference, hooks := contributionFixture(t, `[{"id":"create","intent":"create_worktree","slot":"composer","kind":"button","label":{"label":"Create"},"handler":"create"},{"id":"command","slot":"commands","kind":"button","label":{"label":"Capture"},"handler":"capture","command":{"name":"capture","kind":"invoke"}}]`)
		contributionPublish(t, hooks, `[{"id":"create","visible":false},{"id":"command"}]`)
		if err := registry.SetActivity(reference, true, false); err != nil {
			t.Fatal(err)
		}
		entry, ok := registry.Intent("create_worktree")
		if !ok || entry.State.Visible || registry.Invoke(entry.Key, "invoke", json.RawMessage("null")) == nil {
			t.Fatal("hidden intent gained ordinary visible-control authority")
		}
		frozen := json.RawMessage(`{"branch":"topic","revision":9007199254740993}`)
		if err := registry.InvokeIntent(entry.Key, frozen); err != nil {
			t.Fatal(err)
		}
		event := contributionNext(t, hooks)
		if string(event.Value) != string(frozen) {
			t.Fatal("intent changed its captured exact JSON value")
		}
		other := reference
		other.Name = "other"
		declaration := registry.Commands()[0].Declaration
		otherHooks, err := registry.Hooks(other, []protocol.PluginUiContribution{declaration})
		if err != nil {
			t.Fatal(err)
		}
		contributionPublish(t, otherHooks, `[{"id":"command"}]`)
		if err := registry.SetActivity(other, true, false); err != nil {
			t.Fatal(err)
		}
		commands := registry.Commands()
		if len(commands) != 2 || commands[0].State.Enabled || commands[1].State.Enabled || commands[0].Key.Package.Name != "fixture" {
			t.Fatal("duplicate command names remained actionable or lost deterministic ordering")
		}
		key, _ := registry.Key(reference, "command")
		key.Node[0] ^= 1
		if registry.Invoke(key, "invoke", json.RawMessage("null")) == nil {
			t.Fatal("another target invoked a captured contribution")
		}
	})
}
