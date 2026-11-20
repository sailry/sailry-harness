package live

import (
	"slices"
	"testing"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/desktop/composer"
	"github.com/sailry/sailry-harness/internal/desktop/conversation"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/preferences"
	"github.com/sailry/sailry-harness/internal/desktop/shell"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestComposerAttachmentIdentity(t *testing.T) {
	first := &composer.Attachment{Name: "pasted.png", Data: []byte("image"), Status: composer.Preparing}
	state := &composer.State{Draft: "message", Attachments: []*composer.Attachment{first}}
	captured := composerFingerprint(state)
	first.ID, first.Status, first.Progress = "published", composer.Ready, 1
	first.Data = nil
	if composerFingerprint(state) != captured {
		t.Fatal("upload progress changed the identity of the draft")
	}
	copy := *first
	state.Attachments = []*composer.Attachment{&copy}
	if composerFingerprint(state) == captured {
		t.Fatal("replacing an attachment retained the old draft identity")
	}
	state.Attachments = nil
	if composerFingerprint(state) == captured {
		t.Fatal("removing an attachment retained the old draft identity")
	}
}

func TestOutgoingMessageProjection(t *testing.T) {
	node, session, turn, request := protocol.NodeID{1}, protocol.SessionID{2}, protocol.TurnID{3}, protocol.RequestID{4}
	snapshot := protocol.DefaultSnapshot()
	snapshot.Node = node
	controller := client.New(&conversationTestTransport{snapshot: snapshot})
	defer controller.Close()
	cv := &viewmodel.Conversation{Node: node.String(), ID: session.String()}
	r := &Runtime{controller: controller, snapshotRead: true, snapshotCopy: snapshot,
		App: shell.NewLive(&viewmodel.Data{Workspace: &viewmodel.Workspace{}, Unassigned: []*viewmodel.Conversation{cv}})}
	comp := r.App.Composer(cv.ID)
	file := &composer.Attachment{ID: "document", Name: "note.txt", Mime: "text/plain", Size: 21, Status: composer.Ready}
	image := &composer.Attachment{ID: "picture", Name: "picture.png", Mime: "image/png", Status: composer.Ready}
	message := composer.Message{Text: "Captured message", Attachments: []*composer.Attachment{file, image}}
	actions := &conversationActions{r: r, cv: cv, comp: comp}
	actions.stageOutgoing(message)
	pending := r.composerOutgoing[cv.ID]
	pending.request = request
	comp.Draft = "Typed after sending"
	file.Name, image.Name = "changed.txt", "changed.png"
	r.projectOutgoing(cv, protocol.ConversationPage{})
	if len(cv.Turns) != 1 || cv.Turns[0].User != message.Text || cv.Running() || !cv.Turns[0].InputOnly ||
		len(cv.Turns[0].Files) != 1 || cv.Turns[0].Files[0].Path != "note.txt" ||
		len(cv.Turns[0].Images) != 1 || cv.Turns[0].Images[0].Name != "picture.png" {
		t.Fatalf("captured input changed or invented assistant work: %+v", cv.Turns)
	}
	// The Node's run can arrive before the command response or user entry.
	// Another request with the same text must not consume this local input.
	other := protocol.TurnID{5}
	r.snapshotCopy.Turns = []protocol.TurnSummary{{ID: other, Session: session, Request: protocol.RequestID{9}}}
	page := protocol.DefaultConversationPage()
	page.Session = session
	page.Runs = []protocol.ConversationRun{{Session: session, Turn: other, Kind: "task", Status: "running"}}
	project := func() {
		cv.Turns, cv.Session, cv.Waiting, cv.Queue = Conversation(client.ConversationState{Connected: true, Snapshot: protocol.ConversationSnapshot{Page: page}})
		r.projectOutgoing(cv, page)
	}
	project()
	if len(cv.Turns) != 2 || cv.Turns[0].User != "" || cv.Turns[1].User != message.Text {
		t.Fatal("another request claimed the pending message", cv.Turns)
	}
	r.snapshotCopy.Turns = append(r.snapshotCopy.Turns, protocol.TurnSummary{ID: turn, Session: session, Request: request})
	page.Runs = append(page.Runs, protocol.ConversationRun{Session: session, Turn: turn, Kind: "task", Status: "running"})
	project()
	if len(cv.Turns) != 2 || cv.Turns[1].ID != turn.String() || cv.Turns[1].User != message.Text || !cv.Turns[1].Reply.Status.Active() || len(cv.Turns[1].Files) != 1 {
		t.Fatal("run arriving before its input did not adopt the captured message", cv.Turns)
	}
	page.Queue.Items = []protocol.ConversationPending{{Turn: turn, Kind: "task", Preview: message.Text}}
	project()
	if len(cv.Turns) != 2 || cv.Turns[1].User != message.Text || r.composerOutgoing[cv.ID] == nil {
		t.Fatal("a stale queue item hid the active run's captured input", cv.Turns)
	}
	page.Queue.Items = nil
	page.Entries = []protocol.ConversationEntry{{Turn: turn, ID: "canonical-user", Author: "user", Parts: []protocol.ConversationPart{{Kind: "text", Data: "Canonical message"}}}}
	project()
	project()
	if r.composerOutgoing[cv.ID] != nil || len(cv.Turns) != 2 || cv.Turns[1].User != "Canonical message" || comp.Draft != "Typed after sending" {
		t.Fatal("canonical handoff duplicated input or replaced the newer draft", cv.Turns, comp.Draft)
	}
	t.Run("prerequisite failure preserves the newer draft", func(t *testing.T) {
		actions.stageOutgoing(message)
		r.discardUnsubmitted(cv.ID)
		if r.composerOutgoing[cv.ID] != nil || comp.Draft != "Typed after sending" || len(cv.Turns) != 2 {
			t.Fatal("failed prerequisite left its local input or replaced a newer draft")
		}
	})
	t.Run("idle admission keeps the message in the transcript", func(t *testing.T) {
		// Every turn is admitted queued; on an idle session it starts next,
		// so its message stays ahead of the reply that follows.
		actions.stageOutgoing(message)
		r.composerOutgoing[cv.ID].request = request
		r.snapshotCopy.Turns = nil
		page.Entries = nil
		page.Runs = []protocol.ConversationRun{{Session: session, Turn: turn, Kind: "task", Status: "queued"}}
		page.Queue.Items = []protocol.ConversationPending{{Turn: turn, Request: &request, Kind: "task", Preview: "Captured message"}}
		project()
		if len(cv.Turns) != 1 || cv.Turns[0].User != message.Text || cv.Running() || len(cv.Queue) != 0 || r.composerOutgoing[cv.ID] == nil {
			t.Fatal("an admitted turn about to start left the transcript", cv.Turns, cv.Queue)
		}
		// It starts before its user entry arrives: the reply shows under
		// the captured message, never alone.
		page.Queue.Items = nil
		page.Runs[0].Status = "running"
		project()
		if len(cv.Turns) != 1 || cv.Turns[0].ID != turn.String() || cv.Turns[0].User != message.Text || !cv.Turns[0].Reply.Status.Active() {
			t.Fatal("the starting run showed without its message", cv.Turns)
		}
		page.Entries = []protocol.ConversationEntry{{Turn: turn, ID: "canonical-user", Author: "user", Parts: []protocol.ConversationPart{{Kind: "text", Data: "Captured message"}}}}
		project()
		project()
		if r.composerOutgoing[cv.ID] != nil || len(cv.Turns) != 1 || cv.Turns[0].User != message.Text {
			t.Fatal("canonical handoff lost or duplicated the message", cv.Turns)
		}
	})
	t.Run("a finishing run keeps the message in the transcript", func(t *testing.T) {
		// A run the client had not yet seen end does not send an idle message
		// to the tray and back.
		actions.stageOutgoing(message)
		r.composerOutgoing[cv.ID].request = request
		r.snapshotCopy.Turns = nil
		page.Entries = nil
		page.Runs = []protocol.ConversationRun{{Session: session, Turn: other, Kind: "task", Status: "stopping"}, {Session: session, Turn: turn, Kind: "task", Status: "queued"}}
		page.Queue.Items = []protocol.ConversationPending{{Turn: turn, Request: &request, Kind: "task", Preview: "Captured message"}}
		project()
		if !slices.ContainsFunc(cv.Turns, func(t conversation.Turn) bool { return t.User == message.Text }) || r.composerOutgoing[cv.ID] == nil || len(cv.Queue) != 0 {
			t.Fatal("a finishing run moved the idle message to the tray", cv.Turns, cv.Queue)
		}
	})
	t.Run("another running run moves the message to the tray", func(t *testing.T) {
		// The client looked idle, but the Node still runs another turn: the
		// message waits and stays editable and withdrawable in the tray.
		actions.stageOutgoing(message)
		r.composerOutgoing[cv.ID].request = request
		r.snapshotCopy.Turns = nil
		page.Entries = nil
		page.Runs = []protocol.ConversationRun{{Session: session, Turn: other, Kind: "task", Status: "running"}, {Session: session, Turn: turn, Kind: "task", Status: "queued"}}
		page.Queue.Items = []protocol.ConversationPending{{Turn: turn, Request: &request, Kind: "task", Preview: "Captured message"}}
		project()
		if slices.ContainsFunc(cv.Turns, func(t conversation.Turn) bool { return t.User == message.Text }) || r.composerOutgoing[cv.ID] != nil ||
			len(cv.Queue) != 1 || cv.Queue[0].ID != turn.String() {
			t.Fatal("a waiting message left the tray while another run was running", cv.Turns, cv.Queue)
		}
	})
	t.Run("queued admission stays in the tray", func(t *testing.T) {
		// In a paused queue, or behind another item, the turn waits.
		for _, paused := range []bool{false, true} {
			actions.stageOutgoing(message)
			r.composerOutgoing[cv.ID].request = request
			r.snapshotCopy.Turns = nil
			page.Entries = nil
			page.Runs = []protocol.ConversationRun{{Session: session, Turn: turn, Kind: "task", Status: "queued"}}
			page.Queue.Paused = paused
			page.Queue.Items = []protocol.ConversationPending{{Turn: turn, Request: &request, Kind: "task", Preview: "Captured message"}}
			if !paused {
				page.Queue.Items = append([]protocol.ConversationPending{{Turn: protocol.TurnID{6}, Kind: "task", Preview: "Earlier"}}, page.Queue.Items...)
			}
			project()
			if slices.ContainsFunc(cv.Turns, func(t conversation.Turn) bool { return t.User == message.Text }) || r.composerOutgoing[cv.ID] != nil || len(cv.Queue) != len(page.Queue.Items) {
				t.Fatal("queued admission became an active transcript message", paused, cv.Turns, cv.Queue)
			}
		}
		page.Queue.Paused = false
	})
}

func TestOutgoingPrerequisiteFailurePreservesNewDraft(t *testing.T) {
	for _, command := range []string{"set_session_config", "set_session_team"} {
		t.Run(command, func(t *testing.T) {
			node, sessionID, provider := protocol.NodeID{1}, protocol.SessionID{2}, protocol.ProviderID{3}
			model := protocol.DefaultConversationModel()
			model.ID = "fixture"
			config := protocol.DefaultSessionConfig()
			config.Provider, config.Model, config.Effort = provider, model.ID, model.DefaultEffort
			config.Mode, config.Permission = protocol.WorkModeCode, protocol.PermissionAsk
			session := protocol.DefaultSession()
			session.ID, session.Worktree, session.Revision, session.Config = sessionID, protocol.WorktreeID{4}, 1, config
			snapshot := protocol.DefaultSnapshot()
			snapshot.Node, snapshot.Sessions = node, []protocol.Session{session}
			snapshot.Providers = []protocol.ConversationProvider{{ID: provider, Enabled: true, Models: []protocol.ConversationModel{model}}}
			team := protocol.TeamID{5}
			snapshot.Teams = []protocol.TeamProfile{{ID: team, Key: "review", Name: "Review", Revision: 1}}
			transport := &conversationTestTransport{snapshot: snapshot}
			transport.handle = func(request protocol.Request) (link.Response, bool) {
				if request.Command.Kind == command {
					return link.Response{Fault: &protocol.Fault{Code: protocol.ErrorCodeConflict, Message: "Captured prerequisite failure"}}, true
				}
				return link.Response{}, false
			}
			controller := client.New(transport)
			if err := controller.Start(t.Context()); err != nil {
				t.Fatal(err)
			}
			defer controller.Close()
			cv := &viewmodel.Conversation{Node: node.String(), ID: sessionID.String()}
			r := &Runtime{ctx: t.Context(), controller: controller,
				App: shell.NewLive(&viewmodel.Data{Workspace: &viewmodel.Workspace{}, Unassigned: []*viewmodel.Conversation{cv}}), Preferences: &preferences.Store{},
				busy: map[string]bool{}, uncertain: map[string]protocol.Request{},
				composerObserved: map[string]string{}, composerSaving: map[string]bool{}, composerRevisions: map[string]uint64{},
				composerWaiters: map[string][]func(bool){}, admittedSessions: map[string]protocol.Session{}, teams: newTeamState()}
			r.initAttachments()
			defer r.closeAttachments()
			comp := r.App.Composer(cv.ID)
			comp.Host, comp.Model = cv.Node, modelKey(provider, model.ID)
			if command == "set_session_config" {
				comp.Mode = composer.Plan
			} else {
				comp.Team = team.String()
			}
			const original = "  Captured before prerequisite @note @element\n\n"
			refs := []composer.Reference{{ID: "note.txt", Label: "note", Kind: composer.File}, {ID: "element/1", Label: "element", Kind: composer.Element}}
			file := &composer.Attachment{ID: protocol.AttachmentID{6}.String(), Name: "element.txt", Mime: "text/plain", Chip: refs[1].ID, Status: composer.Ready}
			comp.Draft, comp.References, comp.Attachments = original, slices.Clone(refs), []*composer.Attachment{file}
			newRef := composer.Reference{ID: "new.txt", Label: "new", Kind: composer.File}
			const newer = "A newer unsent draft @new "
			restoreLabel := i18n.In("en", "chat_restore_draft")
			showComposer, deliverOutcomes := true, false
			tester := ui.NewTester(func(c *ui.Context) {
				theme.Apply(c, "", theme.Light)
				if deliverOutcomes {
					r.mu.Lock()
					callbacks := r.queue
					r.queue = nil
					r.mu.Unlock()
					for _, callback := range callbacks {
						callback(c)
					}
				}
				if showComposer {
					actions := &conversationActions{r: r, c: c, cv: cv, comp: comp}
					ctx := &composer.Context{Models: []composer.Model{{ID: comp.Model, Name: model.ID}}, Docked: true, References: []composer.Reference{newRef}}
					ui.Column(c).Fill().Justify(ui.End).Children(func() { composer.View(c, comp, ctx, actions) })
				}
				kit.ToastHost(c)
			}, 760, 460)
			tester.SetLocale("en")
			tester.SetPreferences(ui.Preferences{ReduceMotion: true, ReduceTransparency: true})
			tester.Frame()
			if err := tester.Click(i18n.In("en", "send")); err != nil {
				t.Fatal(err)
			}
			if r.composerOutgoing[cv.ID] == nil {
				t.Fatal("send did not stage its input")
			}
			area, ok := tester.AccessByRole(ui.RoleTextField, i18n.In("en", "composer"))
			if !ok {
				t.Fatal("the composer has no draft input")
			}
			b := area.Bounds
			tester.ClickAt(b.X+b.W/2, b.Y+b.H/2)
			tester.Key(ui.Cmd, ui.KeyA)
			tester.Type("A newer unsent draft @ne")
			tester.Frame()
			if _, ok := tester.AccessByRole(ui.RoleOption, newRef.Label); !ok {
				t.Fatal("typing did not open the reference picker", tester.Texts())
			}
			tester.Key(0, ui.KeyEnter) // accept the reference before using the activity strip above the picker
			tester.Frame()
			if comp.Draft != newer || tester.HasText(i18n.In("en", "reference_title")) || tester.HasText(restoreLabel) {
				t.Fatalf("the accepted reference did not close the picker, or an in-flight send offered recovery: draft=%q %v", comp.Draft, tester.Texts())
			}
			newFile := &composer.Attachment{ID: protocol.AttachmentID{7}.String(), Name: "new.txt", Status: composer.Ready}
			comp.Attachments = append(comp.Attachments, newFile)
			tester.Frame()
			if !slices.Equal(comp.Attachments, []*composer.Attachment{newFile}) {
				t.Fatal("the older draft's file stayed on the newer draft")
			}
			showComposer, deliverOutcomes = false, true
			r.workers.Wait()
			tester.Frame()
			if comp.Draft != newer || len(comp.References) != 1 || comp.References[0].ID != newRef.ID ||
				!slices.Equal(comp.Attachments, []*composer.Attachment{newFile}) || r.composerOutgoing[cv.ID] != nil || len(cv.Turns) != 0 {
				t.Fatalf("failed prerequisite changed new input or kept the local message: draft=%q turns=%v", comp.Draft, cv.Turns)
			}
			for _, item := range tester.AccessAll() {
				if item.Name == restoreLabel {
					t.Fatalf("a prerequisite failure put recovery in a toast: %v", tester.Texts())
				}
			}
			failures := 0
			for _, text := range tester.Texts() {
				if text == i18n.In("en", "session_changed") {
					failures++
				}
			}
			if failures != 1 {
				t.Fatalf("one prerequisite failure produced %d outcome toasts", failures)
			}
			tester.Advance(10 * time.Second)
			showComposer = true
			tester.Frame()
			if err := tester.Click(restoreLabel); err != nil {
				t.Fatal(err, tester.Texts())
			}
			tester.Frame()
			findRestore := func(disabled bool) ui.AccessInfo {
				t.Helper()
				for _, item := range tester.AccessAll() {
					if item.Role == ui.RoleButton && item.Name == restoreLabel && !item.Expanded && item.Disabled == disabled {
						return item
					}
				}
				t.Fatalf("no recovery action with disabled=%v: %v", disabled, tester.Texts())
				return ui.AccessInfo{}
			}
			blocked := findRestore(true).Bounds
			tester.ClickAt(blocked.X+blocked.W/2, blocked.Y+blocked.H/2)
			if comp.Draft != newer || !slices.Equal(comp.Attachments, []*composer.Attachment{newFile}) {
				t.Fatal("recovery overwrote new input")
			}
			comp.Draft, comp.References, comp.Attachments = "", nil, nil
			tester.Frame()
			if comp.Draft != "" || !tester.HasText(file.Name) {
				t.Fatal("the recovery panel lost its file or restored without a choice", tester.Texts())
			}
			button := findRestore(false).Bounds
			tester.ClickAt(button.X+button.W/2, button.Y+button.H/2)
			tester.Frame()
			if comp.Draft != original || !slices.Equal(comp.Attachments, []*composer.Attachment{file}) ||
				!slices.EqualFunc(comp.References, refs, func(a, b composer.Reference) bool { return a.ID == b.ID && a.Label == b.Label && a.Kind == b.Kind }) || tester.HasText(restoreLabel) {
				t.Fatalf("recovery did not restore the captured draft once: draft=%q references=%v files=%v", comp.Draft, comp.References, comp.Attachments)
			}

			r.workers.Wait()
			transport.mu.Lock()
			defer transport.mu.Unlock()
			prerequisites := 0
			for _, request := range transport.requests {
				switch request.Command.Kind {
				case command:
					prerequisites++
				case "submit_turn", "discard_attachment":
					t.Fatalf("failure or recovery dispatched %q", request.Command.Kind)
				}
			}
			if prerequisites != 1 {
				t.Fatalf("expected one %s prerequisite, got %d", command, prerequisites)
			}
		})
	}
}

// A guiding or queued send leaves the sending row once the Node shows it as a
// queue row or as a transcript entry, even before its command response.
func TestOutgoingLandedBeforeResponse(t *testing.T) {
	session, turn, request := protocol.SessionID{2}, protocol.TurnID{3}, protocol.RequestID{4}
	snapshot := protocol.DefaultSnapshot()
	controller := client.New(&conversationTestTransport{snapshot: snapshot})
	defer controller.Close()
	r := &Runtime{controller: controller, snapshotRead: true, snapshotCopy: snapshot}
	pending := &conversationOutgoing{Queued: composer.Queued{Text: "day?", Guided: true}}
	page := protocol.DefaultConversationPage()
	page.Session = session
	if r.outgoingLanded(pending, page) {
		t.Fatal("an unsent message landed")
	}
	pending.request = request
	if r.outgoingLanded(pending, page) {
		t.Fatal("a message the Node has not shown landed")
	}
	page.Queue.Items = []protocol.ConversationPending{{Turn: turn, Request: &request, Kind: "task", Preview: "day?"}}
	if !r.outgoingLanded(pending, page) {
		t.Fatal("a queued row repeated the sending row")
	}
	page.Queue.Items = nil
	r.snapshotCopy.Turns = []protocol.TurnSummary{{ID: turn, Session: session, Request: request}}
	if r.outgoingLanded(pending, page) {
		t.Fatal("a turn without its entry landed")
	}
	page.Entries = []protocol.ConversationEntry{{Turn: turn, ID: "guidance", Author: "user", Parts: []protocol.ConversationPart{{Kind: "text", Data: "day?"}}}}
	if !r.outgoingLanded(pending, page) {
		t.Fatal("consumed guidance repeated the sending row")
	}
}
