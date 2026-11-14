package live

import (
	"slices"
	"testing"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/desktop/composer"
	"github.com/sailry/sailry-harness/internal/desktop/conversation"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
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
	if len(cv.Turns) != 1 || cv.Turns[0].User != message.Text || cv.Running() || !cv.Turns[0].Reply.Notice ||
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
	t.Run("queued admission stays in the tray", func(t *testing.T) {
		// Behind an active run, or in a paused queue, the turn waits.
		for _, paused := range []bool{false, true} {
			actions.stageOutgoing(message)
			r.composerOutgoing[cv.ID].request = request
			r.snapshotCopy.Turns = nil
			page.Entries = nil
			page.Runs = []protocol.ConversationRun{{Session: session, Turn: turn, Kind: "task", Status: "queued"}}
			if !paused {
				page.Runs = append(page.Runs, protocol.ConversationRun{Session: session, Turn: other, Kind: "task", Status: "running"})
			}
			page.Queue.Paused = paused
			page.Queue.Items = []protocol.ConversationPending{{Turn: turn, Request: &request, Kind: "task", Preview: "Captured message"}}
			project()
			if slices.ContainsFunc(cv.Turns, func(t conversation.Turn) bool { return t.User == message.Text }) || r.composerOutgoing[cv.ID] != nil || len(cv.Queue) != 1 {
				t.Fatal("queued admission became an active transcript message", paused, cv.Turns, cv.Queue)
			}
		}
		page.Queue.Paused = false
	})
}

func TestOutgoingPrerequisiteFailureRestoresCapturedDraft(t *testing.T) {
	for _, command := range []string{"set_session_config", "set_session_team"} {
		t.Run(command, func(t *testing.T) {
			node, sessionID, provider := protocol.NodeID{1}, protocol.SessionID{2}, protocol.ProviderID{3}
			config := protocol.DefaultSessionConfig()
			config.Provider, config.Model = provider, "fixture"
			config.Mode, config.Permission = protocol.WorkModeCode, protocol.PermissionAsk
			session := protocol.DefaultSession()
			session.ID, session.Worktree, session.Revision, session.Config = sessionID, protocol.WorktreeID{4}, 1, config
			snapshot := protocol.DefaultSnapshot()
			snapshot.Node, snapshot.Sessions = node, []protocol.Session{session}
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
				App:  shell.NewLive(&viewmodel.Data{Workspace: &viewmodel.Workspace{}, Unassigned: []*viewmodel.Conversation{cv}}),
				busy: map[string]bool{}, uncertain: map[string]protocol.Request{},
				composerObserved: map[string]string{}, composerSaving: map[string]bool{}, composerRevisions: map[string]uint64{},
				composerWaiters: map[string][]func(bool){}, admittedSessions: map[string]protocol.Session{}, teams: newTeamState()}
			var view *ui.Context
			tester := ui.NewTester(func(c *ui.Context) {
				view = c
				theme.Apply(c, "", theme.Light)
				r.mu.Lock()
				callbacks := r.queue
				r.queue = nil
				r.mu.Unlock()
				for _, callback := range callbacks {
					callback(c)
				}
				kit.ToastHost(c)
			}, 600, 300)
			tester.SetLocale("en")
			tester.SetPreferences(ui.Preferences{ReduceMotion: true, ReduceTransparency: true})
			tester.Frame()
			comp := r.App.Composer(cv.ID)
			message := composer.Message{Text: "Captured before prerequisite", Attachments: []*composer.Attachment{{ID: "file", Name: "note.txt", Status: composer.Ready}}}
			comp.Draft, comp.Attachments = message.Text, message.Attachments
			actions := &conversationActions{r: r, c: view, cv: cv, comp: comp, restore: &message, restoreDraft: composerFingerprint(comp)}
			actions.stageOutgoing(message)
			comp.Draft, comp.Attachments = "A newer unsent draft", nil
			if command == "set_session_config" {
				changed := config
				changed.Mode = protocol.WorkModePlan
				r.saveComposerConfig(session, changed, nil)
			} else {
				comp.Team = team.String()
				r.withComposerTeam(session, comp, func(protocol.Session) { t.Error("failed team prerequisite continued submission") })
			}
			r.workers.Wait()
			tester.Frame()
			if comp.Draft != "A newer unsent draft" || len(comp.Attachments) != 0 || r.composerOutgoing[cv.ID] != nil || len(cv.Turns) != 0 {
				t.Fatalf("failed prerequisite changed new input or kept the local message: draft=%q turns=%v", comp.Draft, cv.Turns)
			}
			buttons := 0
			for _, item := range tester.AccessAll() {
				if item.Role == ui.RoleButton && item.Name == i18n.In("en", "chat_restore_draft") {
					buttons++
				}
			}
			if buttons != 1 {
				t.Fatalf("captured input has %d recovery actions: %v", buttons, tester.Texts())
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
			comp.Draft = ""
			if err := tester.Click(i18n.In("en", "chat_restore_draft")); err != nil {
				t.Fatal(err)
			}
			tester.Frame()
			if comp.Draft != message.Text || len(comp.Attachments) != 1 || comp.Attachments[0].Name != "note.txt" {
				t.Fatal("recovery did not restore the captured text and attachment", comp.Draft, comp.Attachments)
			}
			transport.mu.Lock()
			defer transport.mu.Unlock()
			for _, request := range transport.requests {
				if request.Command.Kind == "submit_turn" {
					t.Fatal("failed prerequisite submitted or replayed a turn")
				}
			}
		})
	}
}
