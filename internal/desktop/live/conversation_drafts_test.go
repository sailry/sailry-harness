package live

import (
	"slices"
	"testing"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/composer"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/panes"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestUntouchedDraftPruningReleasesComposer(t *testing.T) {
	snapshot := selectionSnapshot("Existing conversation")
	model := protocol.DefaultConversationModel()
	model.ID = "fixture"
	provider := protocol.ProviderID{2}
	snapshot.Providers = []protocol.ConversationProvider{{ID: provider, Enabled: true, Models: []protocol.ConversationModel{model}}}
	config := protocol.DefaultSessionConfig()
	config.Provider, config.Model, config.Effort = provider, model.ID, model.DefaultEffort
	snapshot.Defaults.Config, snapshot.Sessions[0].Config = &config, config

	f := selectionWindow(t, snapshot)
	r, app := f.r, f.r.App
	existing := snapshot.Sessions[0].ID.String()
	r.OpenSession(existing)
	f.window.Frame()
	existingComposer := app.Composer(existing)
	type counts struct{ open, initialized, observed, bases, teams int }
	resources := func() counts {
		return counts{len(r.openDrafts), len(r.composerInitialized), len(r.composerObserved), len(r.composerBases), len(r.teams.composers)}
	}
	baseline := resources()
	last := ""
	for cycle := range 8 {
		app.Sidebar.Actions.NewDraft(nil)
		draft := app.Sidebar.Selected
		f.window.Frame()
		if cv := app.Data.Find(draft); cv == nil || !unsentDraft(cv) || draft == last || !r.composerInitialized[draft] {
			t.Fatalf("cycle %d did not show and initialize a new draft: %q", cycle, draft)
		}
		last = draft
		comp := app.Composer(draft)
		if _, ok := r.composerBases[comp]; !ok || r.composerObserved[draft] == "" || r.teams.composers[draft] == nil {
			t.Fatal("fixture did not initialize the draft's config and team state")
		}

		r.OpenSession(existing)
		f.window.Frame()
		if _, tracked := r.openDrafts[draft]; tracked || app.Data.Find(draft) != nil {
			t.Fatalf("cycle %d did not discard untouched draft %q", cycle, draft)
		}
		if app.DropComposer(draft) != nil {
			t.Errorf("cycle %d left an App composer for discarded draft %q", cycle, draft)
		}
		if got := resources(); got != baseline {
			t.Fatalf("cycle %d retained draft resources: got %+v, want %+v", cycle, got, baseline)
		}
		if app.FocusedConversation() != existing || app.Composer(existing) != existingComposer {
			t.Fatal("discarding the draft changed the admitted conversation's composer")
		}
	}
}

func TestDraftPruningRetainsInput(t *testing.T) {
	file := &composer.Attachment{Name: "note.txt", Status: composer.Ready}
	ref := composer.Reference{ID: "note.txt", Label: "note", Kind: composer.File}
	for _, input := range []struct {
		name, text  string
		attachments []*composer.Attachment
		references  []composer.Reference
	}{
		{name: "text", text: "Retain this input"},
		{name: "whitespace", text: " \n "},
		{name: "attachment only", attachments: []*composer.Attachment{file}},
		{name: "reference only", references: []composer.Reference{ref}},
	} {
		t.Run(input.name, func(t *testing.T) {
			snapshot := selectionSnapshot("Existing conversation")
			f := selectionWindow(t, snapshot)
			r, app := f.r, f.r.App
			r.newDraft(nil)
			draft := app.Sidebar.Selected
			f.window.Frame()
			comp := app.Composer(draft)
			comp.Draft, comp.Attachments, comp.References = input.text, slices.Clone(input.attachments), slices.Clone(input.references)

			r.OpenSession(snapshot.Sessions[0].ID.String())
			f.window.Frame()
			r.newDraft(nil)
			if _, tracked := r.openDrafts[draft]; !tracked || app.Sidebar.Selected != draft || app.Composer(draft) != comp || !r.composerInitialized[draft] {
				t.Fatal("hiding a draft with input did not leave it available to New session")
			}
			if comp.Draft != input.text || !slices.Equal(comp.Attachments, input.attachments) ||
				!slices.EqualFunc(comp.References, input.references, func(a, b composer.Reference) bool { return a.ID == b.ID && a.Label == b.Label }) {
				t.Fatal("reopening the hidden draft lost its input")
			}
		})
	}
}

func TestDraftPruningRetainsFirstSendRecovery(t *testing.T) {
	snapshot := selectionSnapshot("Existing conversation")
	f := selectionWindow(t, snapshot)
	r, app := f.r, f.r.App
	r.newDraft(nil)
	draft := app.Sidebar.Selected
	f.window.Frame()
	comp := app.Composer(draft)
	// First send was captured and the editor was cleared before navigating away.
	r.conversationCreating[draft] = true
	r.OpenSession(snapshot.Sessions[0].ID.String())
	f.window.Frame()
	if app.Data.Find(draft) == nil || app.Composer(draft) != comp {
		t.Fatal("hiding the pending first send discarded its draft")
	}

	delete(r.conversationCreating, draft)
	comp.KeepFailedDraft(composer.Message{Text: "Captured first message"}, false)
	f.window.Frame()
	r.newDraft(nil)
	if app.Sidebar.Selected != draft || app.Composer(draft) != comp || comp.Draft != "" {
		t.Fatal("hiding a draft with recovery-only input replaced or rewrote it")
	}
	f.window.Frame()
	if _, ok := f.window.AccessByRole(ui.RoleButton, i18n.In("en", "chat_restore_draft")); !ok {
		t.Fatal("the reopened draft no longer offers to recover the first send")
	}
}

func TestDraftPruningRetainsKeptTab(t *testing.T) {
	snapshot := selectionSnapshot("Existing conversation")
	f := selectionWindow(t, snapshot)
	r, app := f.r, f.r.App
	r.newDraft(nil)
	draft := app.Sidebar.Selected
	f.window.Frame()
	comp := app.Composer(draft)
	target := panes.Target{Kind: panes.Session, ID: draft}
	prefs := app.Prefs()
	prefs.Tabs = append(prefs.Tabs, target)
	app.SetPrefs(prefs)

	r.OpenSession(snapshot.Sessions[0].ID.String())
	f.window.Frame()
	if r.draftsShown[draft] || app.Data.Find(draft) == nil || app.Composer(draft) != comp || !r.composerInitialized[draft] {
		t.Fatal("a kept tab did not preserve its hidden draft")
	}

	prefs = app.Prefs()
	prefs.Tabs = slices.DeleteFunc(prefs.Tabs, func(t panes.Target) bool { return t == target })
	app.SetPrefs(prefs)
	f.window.Frame()
	if _, tracked := r.openDrafts[draft]; tracked || app.Data.Find(draft) != nil || r.composerInitialized[draft] || app.DropComposer(draft) != nil {
		t.Fatal("releasing a kept tab left its empty, unselected draft behind")
	}
}

func TestDraftPruningRetainsHiddenSplit(t *testing.T) {
	snapshot := selectionSnapshot("Split companion", "Outside conversation")
	inside, outside := snapshot.Sessions[0].ID.String(), snapshot.Sessions[1].ID.String()
	f := selectionWindow(t, snapshot)
	r, app := f.r, f.r.App
	r.newDraft(nil)
	draft := app.Sidebar.Selected
	f.window.Frame()
	comp := app.Composer(draft)
	target := panes.Target{Kind: panes.Session, ID: draft}
	companion := panes.Target{Kind: panes.Session, ID: inside}
	prefs := app.Prefs()
	prefs.Panes = []*panes.Node{selectionSplit(target, companion)}
	app.SetPrefs(prefs)

	r.OpenSession(inside)
	f.window.Frame()
	if !r.draftsShown[draft] {
		t.Fatal("the split did not show the draft alongside the other pane")
	}
	r.OpenSession(outside)
	f.window.Frame()
	if r.draftsShown[draft] || app.FocusedConversation() != outside {
		t.Fatal("the draft's split did not become a background workspace")
	}
	if groups := app.Prefs().Panes; len(groups) != 1 || !slices.Equal(groups[0].Targets(), []panes.Target{target, companion}) ||
		app.Data.Find(draft) == nil || app.Composer(draft) != comp || !r.composerInitialized[draft] {
		t.Fatal("hiding the split discarded its untouched draft or layout")
	}
}
