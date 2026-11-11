package live

import (
	"slices"
	"testing"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/preferences"
	"github.com/sailry/sailry-harness/internal/desktop/settings"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// TestMediaSettings drives Settings › Providers against a Node that answers
// the media and provider commands: the capability tags a provider save
// sends, and the media rows' options, saves and outcomes.
func TestMediaSettings(t *testing.T) {
	provider := protocol.ProviderID{1}
	model := func(id string, vision bool, generates ...protocol.MediaGeneration) protocol.ConversationModel {
		m := protocol.DefaultConversationModel()
		m.ID, m.Context, m.Output, m.Tools, m.Vision, m.Generates = id, 128000, 16384, true, vision, generates
		return m
	}
	snapshot := protocol.DefaultSnapshot()
	snapshot.Node = protocol.NodeID{1}
	fixture := protocol.DefaultConversationProvider()
	fixture.Api = protocol.ConversationModelApiResponses
	fixture.ID, fixture.Name, fixture.Enabled, fixture.Revision, fixture.Authentication, fixture.DefaultModel = provider, "Fixture", true, 2, protocol.AuthenticationApiKey, "see"
	fixture.Models = []protocol.ConversationModel{model("see", true), model("draw", false, protocol.MediaGenerationImage), model("hosted", true, protocol.MediaGenerationImageTool)}
	snapshot.Providers = []protocol.ConversationProvider{fixture}
	binding := func(id string) *protocol.MediaBinding { return &protocol.MediaBinding{Provider: provider, Model: id} }
	// held is the Node's media settings; failure answers the next save.
	held := protocol.MediaSettings{Revision: 3, Bindings: map[protocol.MediaKind]*protocol.MediaBinding{protocol.MediaKindSpeech: binding("gone")}, Unavailable: []protocol.MediaKind{protocol.MediaKindSpeech}}
	snapshot.MediaSettings = held
	var failure protocol.ErrorCode
	var saves []protocol.MediaSettings
	var providerSaves []protocol.CommandSaveProviderPayload
	// reads counts the reads answered; overtaken, once set, answers them.
	var reads int
	var overtaken *protocol.MediaSettings
	transport := &conversationTestTransport{snapshot: snapshot}
	transport.handle = func(request protocol.Request) (link.Response, bool) {
		switch request.Command.Kind {
		case "read_media_settings":
			reads++
			if overtaken != nil {
				return link.Response{Output: protocol.Output{Kind: "media_settings", Data: *overtaken}}, true
			}
			return link.Response{Output: protocol.Output{Kind: "media_settings", Data: held}}, true
		case "list_media_models":
			return link.Response{Output: protocol.Output{Kind: "media_models", Data: []protocol.MediaCandidate{
				{Provider: provider, ProviderName: "Fixture", Model: "see", Kinds: []protocol.MediaKind{protocol.MediaKindVision}},
				{Provider: provider, ProviderName: "Fixture", Model: "draw", Kinds: []protocol.MediaKind{protocol.MediaKindImage}},
				{Provider: provider, ProviderName: "Fixture", Model: "hosted", Kinds: []protocol.MediaKind{protocol.MediaKindVision}},
			}}}, true
		case "save_media_settings":
			input, _ := detail[protocol.MediaSettings](request.Command.Data)
			saves = append(saves, input)
			if failure != "" {
				return link.Response{Fault: &protocol.Fault{Code: failure, Message: "fixture rejection"}}, true
			}
			held.Revision, held.Bindings, held.Unavailable = input.Revision+1, input.Bindings, nil
			if input.Bindings[protocol.MediaKindSpeech] != nil {
				held.Unavailable = []protocol.MediaKind{protocol.MediaKindSpeech}
			}
			return link.Response{Output: protocol.Output{Kind: "media_settings", Data: held}}, true
		case "save_provider":
			input, _ := detail[protocol.CommandSaveProviderPayload](request.Command.Data)
			providerSaves = append(providerSaves, input)
			return link.Response{Output: protocol.Output{Kind: "provider", Data: input.Provider}}, true
		}
		return link.Response{}, false
	}
	locked := func(read func()) {
		transport.mu.Lock()
		defer transport.mu.Unlock()
		read()
	}
	controller := client.New(transport)
	if err := controller.Start(t.Context()); err != nil {
		t.Fatal(err)
	}
	defer controller.Close()
	r, err := NewWithPreferences(t.Context(), controller, preferences.Open(t.TempDir()+"/preferences.json"))
	if err != nil {
		t.Fatal(err)
	}
	defer r.Close()
	m := r.App.Settings
	m.Open(settings.Providers)
	var toasts []string
	tester := ui.NewTesterWith(func(c *ui.Context) {
		r.mu.Lock()
		queue := r.queue
		r.queue = nil
		r.mu.Unlock()
		for _, post := range queue {
			post(c)
		}
		settings.Page(c, m, r.App.Data, settings.Look{Theme: &r.App.Theme, Mode: &r.App.Mode, Effects: &r.App.Effects, Surfaces: &r.App.Surfaces})
		ui.ToastViewportBase(c, func(_ ui.Element, shown []ui.Toast) {
			toasts = toasts[:0]
			for _, toast := range shown {
				toasts = append(toasts, toast.Title)
			}
		})
	}, 900, 1200, ui.TesterOptions{Preferences: ui.Preferences{ReduceTransparency: true}})
	tester.SetLocale("en")
	await := func(what string, done func() bool) {
		t.Helper()
		deadline := time.Now().Add(10 * time.Second)
		for tester.Frame(); !done(); tester.Frame() {
			if time.Now().After(deadline) {
				t.Fatalf("timed out waiting for %s", what)
			}
			time.Sleep(2 * time.Millisecond)
		}
	}
	items := func() (found []ui.AccessInfo) {
		for _, x := range tester.AccessAll() {
			if x.Role == ui.RoleMenuItem || x.Role == ui.RoleMenuItemRadio || x.Role == ui.RoleMenuItemCheckBox {
				found = append(found, x)
			}
		}
		return
	}
	// open opens a row's select and returns the options it offers.
	open := func(row string) (labels []string) {
		t.Helper()
		for _, x := range tester.AccessAll() {
			if x.Role == ui.RoleMenuButton && x.Name == row {
				tester.ClickAt(x.Bounds.X+x.Bounds.W/2, x.Bounds.Y+x.Bounds.H/2)
				tester.Frame()
				tester.Advance(time.Second)
				for _, item := range items() {
					labels = append(labels, item.Name)
				}
				return
			}
		}
		t.Fatalf("no select named %q", row)
		return
	}
	choose := func(row, label string) {
		t.Helper()
		open(row)
		for _, item := range items() {
			if item.Name == label {
				tester.ClickAt(item.Bounds.X+item.Bounds.W/2, item.Bounds.Y+item.Bounds.H/2)
				tester.Frame()
				tester.Advance(time.Second)
				return
			}
		}
		t.Fatalf("%s has no option %q", row, label)
	}
	shown := func(text string) (count int) {
		for _, x := range tester.AccessAll() {
			if x.Name == text {
				count++
			}
		}
		return
	}
	key := func(id string) string { return modelKey(provider, id) }
	follow := i18n.In("en", "media_follow")
	await("the media rows", func() bool { return m.Media.Revision == 3 && len(m.MediaOptions) == 4 })

	t.Run("rows list the models tagged for their purpose", func(t *testing.T) {
		for row, want := range map[string][]string{
			"Vision": {follow, "Fixture · see", "Fixture · hosted"},
			"Image":  {follow, "Fixture · draw"},
			"Video":  {follow},
			// The saved model the Node no longer offers stays the row's choice.
			"Speech": {follow, "Fixture · gone"},
		} {
			if got := open(row); !slices.Equal(got, want) {
				t.Errorf("%s offers %q, want %q", row, got, want)
			}
			tester.Key(0, ui.KeyEscape)
			tester.Frame()
			tester.Advance(time.Second)
		}
		if m.Media.Saved != [4]string{"", "", "", key("gone")} || m.Media.Draft != m.Media.Saved {
			t.Errorf("projected bindings %q, draft %q", m.Media.Saved, m.Media.Draft)
		}
		if m.MediaUnavailable != [4]bool{false, false, false, true} || shown(i18n.In("en", "media_model_unavailable")) != 1 {
			t.Errorf("unavailable %v, marked on %d rows", m.MediaUnavailable, shown(i18n.In("en", "media_model_unavailable")))
		}
	})

	t.Run("a choice saves every binding at the revision read", func(t *testing.T) {
		choose("Image", "Fixture · draw")
		await("the save", func() bool { return !m.Media.Pending && m.Media.Revision == 4 })
		locked(func() {
			if len(saves) != 1 || saves[0].Revision != 3 || len(saves[0].Bindings) != 2 ||
				*saves[0].Bindings[protocol.MediaKindImage] != *binding("draw") || *saves[0].Bindings[protocol.MediaKindSpeech] != *binding("gone") {
				t.Errorf("save sent %+v", saves)
			}
		})
		if want := [4]string{"", key("draw"), "", key("gone")}; m.Media.Saved != want || m.Media.Draft != want {
			t.Errorf("after the save: saved %q, draft %q", m.Media.Saved, m.Media.Draft)
		}
		if len(toasts) != 0 {
			t.Errorf("a saved choice toasted %q", toasts)
		}
	})

	for _, outcome := range []struct {
		name    string
		code    protocol.ErrorCode
		message string
		row     string
		label   string
		index   int
		chosen  string
	}{
		{"a conflict keeps the draft and toasts", protocol.ErrorCodeRevisionConflict, "media_conflict", "Vision", "Fixture · see", 0, key("see")},
		{"a failure keeps the draft and toasts", protocol.ErrorCodeInternal, "media_failed", "Speech", follow, 3, ""},
		{"an unknown outcome keeps the draft and toasts", protocol.ErrorCodeOutcomeUnknown, "media_unknown", "Image", follow, 1, ""},
	} {
		t.Run(outcome.name, func(t *testing.T) {
			var before int
			locked(func() { failure, before = outcome.code, len(saves) })
			saved := m.Media.Saved
			choose(outcome.row, outcome.label)
			await("the rejected save", func() bool {
				return !m.Media.Pending && !r.media.reading && slices.Contains(toasts, i18n.In("en", outcome.message))
			})
			locked(func() {
				if len(saves) != before+1 || saves[before].Revision != 4 {
					t.Errorf("save sent %+v", saves[before:])
				}
			})
			if m.Media.Draft[outcome.index] != outcome.chosen || m.Media.Saved != saved || m.Media.Revision != 4 {
				t.Errorf("after %s: draft %q, saved %q, revision %d", outcome.code, m.Media.Draft, m.Media.Saved, m.Media.Revision)
			}
		})
	}

	t.Run("a provider save carries the edited capability tags", func(t *testing.T) {
		channel := m.Channels[0]
		channel.Models = slices.Clone(channel.Models)
		edited := &channel.Models[1]
		if edited.ID != "draw" || edited.Vision || !slices.Equal(edited.Generates, []string{viewmodel.GenerateImage}) || edited.CustomCapabilities {
			t.Fatalf("projected model %+v", edited)
		}
		if hosted := channel.Models[2]; !slices.Equal(hosted.Generates, []string{viewmodel.GenerateImageTool}) || !hosted.Vision {
			t.Fatalf("projected hosted model %+v", hosted)
		}
		edited.CustomCapabilities, edited.Vision, edited.Tools, edited.Reasoning = true, true, false, true
		edited.Generates = []string{viewmodel.GenerateImage, viewmodel.GenerateSpeech}
		// The editor sends the API of the method it configures.
		channel.API = string(protocol.ConversationModelApiResponses)
		done := false
		m.ProviderActions.Save(settings.ProviderDraft{Channel: channel, ExpectedRevision: channel.Revision}, func(ok bool) { done = ok })

		await("the provider save", func() bool { return done })
		locked(func() {
			if len(providerSaves) != 1 || providerSaves[0].ExpectedRevision != 2 || len(providerSaves[0].Provider.Models) != 3 {
				t.Fatalf("provider saves %+v", providerSaves)
			}
			sent := providerSaves[0].Provider.Models
			if !sent[1].CustomCapabilities || !sent[1].Vision || sent[1].Tools || !sent[1].Reasoning ||
				!slices.Equal(sent[1].Generates, []protocol.MediaGeneration{protocol.MediaGenerationImage, protocol.MediaGenerationSpeech}) {
				t.Errorf("edited model sent as %+v", sent[1])
			}
			// Models whose tags were not edited stay the Node's to tag.
			if sent[0].CustomCapabilities || sent[2].CustomCapabilities || !slices.Equal(sent[2].Generates, []protocol.MediaGeneration{protocol.MediaGenerationImageTool}) {
				t.Errorf("unedited models sent as %+v, %+v", sent[0], sent[2])
			}
		})

		// Tags reset in the form go out as the Node's to tag again, whatever
		// the boxes showed.
		edited.CustomCapabilities, done = false, false
		m.ProviderActions.Save(settings.ProviderDraft{Channel: channel, ExpectedRevision: channel.Revision}, func(ok bool) { done = ok })
		await("the provider save after a reset", func() bool { return done })
		locked(func() {
			if len(providerSaves) != 2 || providerSaves[1].Provider.Models[1].CustomCapabilities {
				t.Errorf("reset model sent as %+v", providerSaves[len(providerSaves)-1].Provider.Models)
			}
		})
	})

	t.Run("a change during a read is read for, and an older answer is dropped", func(t *testing.T) {
		await("the reads to rest", func() bool { return !r.media.reading })
		var before int
		locked(func() { before = reads })
		current := r.snapshot()
		newer := current
		newer.MediaSettings.Revision++
		r.media.seen = ""
		r.followMedia(current)
		r.followMedia(newer)
		await("the read for the newer snapshot", func() bool {
			done := false
			locked(func() { done = reads >= before+2 })
			return done && !r.media.reading
		})

		saved, revision := m.Media.Saved, m.Media.Revision
		locked(func() {
			before = reads
			overtaken = &protocol.MediaSettings{Revision: revision - 1, Bindings: map[protocol.MediaKind]*protocol.MediaBinding{protocol.MediaKindVision: binding("see")}}
		})
		r.media.seen = ""
		r.followMedia(current)
		await("the overtaken read", func() bool {
			done := false
			locked(func() { done = reads > before })
			return done && !r.media.reading
		})
		if m.Media.Revision != revision || m.Media.Saved != saved {
			t.Errorf("an older answer left revision %d, saved %q; want %d, %q", m.Media.Revision, m.Media.Saved, revision, saved)
		}
	})
}
