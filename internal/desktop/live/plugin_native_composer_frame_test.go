package live

import (
	"bytes"
	"context"
	"encoding/json"
	"testing"
	"time"

	"github.com/sailry/sailry-harness/internal/desktop/composer"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
	"github.com/sailry/sailry-harness/internal/protocol"
)

type nativeComposerFrameState struct {
	Cursor     string                 `json:"cursor"`
	Sidebar    bool                   `json:"sidebar"`
	Connected  bool                   `json:"connected"`
	Mode       string                 `json:"mode"`
	Command    *struct{ Name string } `json:"command"`
	Options    map[string]string      `json:"options"`
	Statistics struct {
		ContextTokens string `json:"context_tokens"`
	} `json:"statistics"`
}

type nativeComposerWatchContext struct {
	context.Context
	waiting chan struct{}
}

func (ctx nativeComposerWatchContext) Done() <-chan struct{} {
	select {
	case ctx.waiting <- struct{}{}:
	default:
	}
	return ctx.Context.Done()
}

func TestPluginNativeComposerFrameInvalidation(t *testing.T) {
	fixture := newNativeComposerFrameFixture(t)
	r := fixture.root
	ctx, cancel := context.WithTimeout(t.Context(), 5*time.Second)
	defer cancel()
	read := func() nativeComposerFrameState {
		t.Helper()
		var value nativeComposerFrameState
		body, err := fixture.native.Operations["sailry/ui.readComposer"](ctx, nil)
		if err != nil || json.Unmarshal(body, &value) != nil {
			t.Fatalf("readComposer: %s, %v", body, err)
		}
		return value
	}
	next := func(previous string) nativeComposerFrameState {
		t.Helper()
		var value nativeComposerFrameState
		body, err := fixture.native.Operations["sailry/ui.nextComposerChange"](ctx, []json.RawMessage{json.RawMessage(`"` + previous + `"`)})
		if err != nil || json.Unmarshal(body, &value) != nil || value.Cursor == previous {
			t.Fatalf("nextComposerChange after %s: %s, %v", previous, body, err)
		}
		return value
	}
	previous := read()
	if !previous.Connected || !previous.Sidebar || previous.Command == nil || previous.Command.Name != "sample" ||
		previous.Options["budget"] != "balanced" || previous.Statistics.ContextTokens != "7" {
		t.Fatalf("fixture composer state: %+v", previous)
	}
	modelContext := func(operation string, arguments []json.RawMessage) (string, uint64) {
		t.Helper()
		var value struct {
			Cursor   string `json:"cursor"`
			Provider *struct {
				Revision uint64 `json:"revision,string"`
			} `json:"provider"`
		}
		body, err := fixture.native.Operations["sailry/ui."+operation](ctx, arguments)
		if err != nil || json.Unmarshal(body, &value) != nil || value.Provider == nil {
			t.Fatalf("%s: %s, %v", operation, body, err)
		}
		return value.Cursor, value.Provider.Revision
	}
	modelCursor, revision := modelContext("readModelContext", nil)
	if revision != 1 {
		t.Fatalf("unexpected initial provider revision %d", revision)
	}
	for range 8 {
		fixture.window.Frame()
	}
	if unchanged := read(); unchanged.Cursor != previous.Cursor {
		t.Fatalf("unchanged frames advanced the composer cursor from %s to %s", previous.Cursor, unchanged.Cursor)
	}
	light := fixture.native.Theme()
	corrupted := fixture.native.Theme()
	corrupted[0] = '!'
	if !bytes.Equal(light, fixture.native.Theme()) {
		t.Fatal("mutating a theme response changed the retained projection")
	}
	r.App.Mode = theme.Dark
	fixture.window.Frame()
	if dark := fixture.native.Theme(); bytes.Equal(light, dark) || !bytes.Contains(dark, []byte(`"is_dark":true`)) {
		t.Fatalf("theme change did not refresh native colors: %s", dark)
	}
	if got := read(); got.Cursor != previous.Cursor {
		t.Fatal("a theme-only update advanced the composer cursor")
	}
	r.App.NavOpen = false
	fixture.window.Frame()
	previous = next(previous.Cursor)
	if previous.Sidebar {
		t.Fatal("composer retained the old sidebar layout")
	}
	if nextCursor, _ := modelContext("readModelContext", nil); nextCursor != modelCursor {
		t.Fatal("a sidebar-only update advanced the model cursor")
	}
	if err := fixture.mounted.hooks.Publish(json.RawMessage(`[{"id":"sample","visible":true,"enabled":false}]`)); err != nil {
		t.Fatal(err)
	}
	fixture.window.Frame()
	previous = next(previous.Cursor)
	if previous.Command != nil {
		t.Fatal("a disabled plugin command remained in the composer")
	}
	r.App.Composer(fixture.scope.Session.String()).Mode = composer.Plan
	fixture.window.Frame()
	previous = next(previous.Cursor)
	if previous.Mode != "plan" {
		t.Fatal("composer retained the old mode")
	}
	id, node := *fixture.scope.Session, r.controller.Target()
	comp := r.App.Composer(id.String())
	config := r.snapshot().Sessions[0].Config
	comp.Model = modelKey(config.Provider, config.Model)
	r.composerBases = map[*composer.State]protocol.SessionConfig{}
	for _, option := range []string{"compact", "detailed"} {
		encoded, err := json.Marshal(option)
		if err != nil {
			t.Fatal(err)
		}
		config.Options = map[string]map[string]json.RawMessage{fixture.scope.Package.Name: {"budget": encoded}}
		r.setComposerBase(comp, config)
		fixture.window.Frame()
		previous = next(previous.Cursor)
		if got := previous.Options["budget"]; got != option {
			t.Fatalf("the same model and mode retained old options: %s", got)
		}
	}
	publishNativeComposerFixtureHistory(r.conversationFeeds[id], node, id, 19)
	fixture.window.Frame()
	previous = next(previous.Cursor)
	if previous.Statistics.ContextTokens != "19" {
		t.Fatal("composer retained the old statistics after a same-session publication")
	}
	updated := r.snapshot()
	updated.Providers = append([]protocol.ConversationProvider(nil), updated.Providers...)
	updated.Providers[0].Revision++
	updated.Cursor++
	accepted := make(chan struct{}, 1)
	defer r.controller.OnChange(func() {
		if r.snapshot().Cursor == updated.Cursor {
			select {
			case accepted <- struct{}{}:
			default:
			}
		}
	})()
	fixture.source.updates <- protocol.Update{Kind: "snapshot", Data: updated}
	select {
	case <-accepted:
	case <-ctx.Done():
		t.Fatal("provider update did not reach the Client")
	}
	fixture.window.Frame()
	if cursor, revision := modelContext("nextModelContext", []json.RawMessage{json.RawMessage(`"` + modelCursor + `"`)}); cursor == modelCursor || revision != 2 {
		t.Fatalf("model context retained provider revision %d at cursor %s", revision, cursor)
	}
	if got := read(); got.Cursor != previous.Cursor {
		t.Fatal("provider revision alone advanced the composer cursor")
	}
	view := r.App.Data.Find(id.String())
	view.Node = (protocol.NodeID{9}).String()
	fixture.window.Frame()
	if body, err := fixture.native.Operations["sailry/ui.readModelContext"](ctx, nil); len(body) != 0 || faultCode(err) != protocol.ErrorCodeUnavailable {
		t.Fatalf("readModelContext for another Host: %s, %v", body, err)
	}
	view.Node = node.String()
	fixture.window.Frame()
	if restored := read(); !restored.Connected || restored.Statistics.ContextTokens != "19" {
		t.Fatalf("restored Host returned the wrong composer state: %+v", restored)
	}
	cursor := read().Cursor
	waiting := make(chan error, 1)
	watchCtx := nativeComposerWatchContext{Context: ctx, waiting: make(chan struct{}, 1)}
	go func() {
		_, err := fixture.native.Operations["sailry/ui.nextComposerChange"](watchCtx, []json.RawMessage{json.RawMessage(`"` + cursor + `"`)})
		waiting <- err
	}()
	select {
	case <-watchCtx.waiting:
	case <-ctx.Done():
		t.Fatal("the composer listener did not wait on its current cursor")
	}
	if err := r.Close(); err != nil {
		t.Fatal(err)
	}
	if fixture.owners() != 0 || fixture.mounted.host != nil || fixture.native.Context.Err() == nil {
		t.Fatal("the closed window retained its plugin presentation")
	}
	if theme := fixture.native.Theme(); len(theme) != 0 {
		t.Fatal("the closed binding returned its retained theme")
	}
	if body, err := fixture.native.Operations["sailry/ui.readModelContext"](ctx, nil); len(body) != 0 || faultCode(err) != protocol.ErrorCodeUnavailable {
		t.Fatalf("the closed binding returned a model context: %s, %v", body, err)
	}
	select {
	case err := <-waiting:
		if err == nil || ctx.Err() != nil {
			t.Fatalf("the pending composer reader did not retire with its binding: %v", err)
		}
	case <-ctx.Done():
		t.Fatal("the pending composer reader did not retire with its binding")
	}
}
