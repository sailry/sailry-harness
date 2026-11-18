package live

import (
	"bytes"
	"context"
	"encoding/json"
	"fmt"
	"image"
	"image/png"
	"io"
	"os"
	"path/filepath"
	"slices"
	"testing"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/desktop/composer"
	"github.com/sailry/sailry-harness/internal/desktop/preferences"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
	"lukechampine.com/blake3"
)

func TestDraftAttachmentStartsConversationOnlyOnSend(t *testing.T) {
	for _, test := range []struct {
		name, receipt, recovery string
		withProject             bool
	}{
		{name: "projectless"},
		{name: "in project", withProject: true},
		{name: "late snapshot proves first admission", receipt: "admitted"},
		{name: "terminal unknown releases the draft", receipt: "unknown"},
		{name: "unrecorded first send adopts only a matching late snapshot", receipt: "unrecorded", recovery: "snapshot"},
		{name: "unrecorded first send confirmed not admitted", receipt: "unrecorded", recovery: "not_admitted"},
		{name: "unrecorded first send confirmed failed", receipt: "unrecorded", recovery: "completed"},
		{name: "admitted conversation removed before read", receipt: "removed"},
		{name: "expired upload can be retried", receipt: "missing"},
	} {
		t.Run(test.name, func(t *testing.T) {
			withProject := test.withProject
			node, provider := protocol.NodeID{1}, protocol.ProviderID{2}
			project, worktree := protocol.ProjectID{3}, protocol.WorktreeID{4}
			snapshot := protocol.DefaultSnapshot()
			snapshot.Node = node
			snapshot.Teams = []protocol.TeamProfile{{ID: protocol.TeamID{5}, Name: "Optional team", Revision: 1}}
			snapshot.Providers = []protocol.ConversationProvider{{
				ID: provider, Name: "Provider", Enabled: true, Api: "responses", Authentication: "api_key",
				Models: []protocol.ConversationModel{
					{ID: "default", DefaultEffort: protocol.Effort{Kind: "default"}},
					{ID: "chosen", Vision: true, DefaultEffort: protocol.Effort{Kind: "default"}},
				},
			}}
			folder := t.TempDir()
			if withProject {
				snapshot.Projects = []protocol.Project{{ID: project, Name: "Fixture", Path: folder}}
				snapshot.Worktrees = []protocol.Worktree{{ID: worktree, Project: &project, Path: folder, Main: true}}
			}
			var pngBytes bytes.Buffer
			if err := png.Encode(&pngBytes, image.NewRGBA(image.Rect(0, 0, 4, 3))); err != nil {
				t.Fatal(err)
			}
			source := filepath.Join(folder, "test.png")
			if err := os.WriteFile(source, pngBytes.Bytes(), 0o600); err != nil {
				t.Fatal(err)
			}
			base := &conversationTestTransport{snapshot: snapshot}
			base.conversation = protocol.DefaultConversationSnapshot()
			base.conversation.Node, base.conversation.Page.Session = node, protocol.SessionID{9}
			transport := &draftAttachmentTransport{conversationTestTransport: base, image: pngBytes.Bytes()}
			attachment := protocol.Attachment{ID: protocol.AttachmentID{7}}
			receipt := protocol.RequestOutcome{Kind: test.receipt}
			var admitted protocol.QueuedTurn
			base.handle = func(request protocol.Request) (link.Response, bool) {
				var output protocol.Output
				switch request.Command.Kind {
				case "upload_attachment":
					attachment.Spec, _ = detail[protocol.AttachmentSpec](request.Command.Data)
					output = protocol.Output{Kind: "attachment_upload", Data: protocol.AttachmentUpload{Stream: protocol.StreamID{1}, Spec: attachment.Spec}}
				case "finish_attachment_upload":
					output = protocol.Output{Kind: "attachment", Data: attachment}
				case "download_attachment":
					output = protocol.Output{Kind: "attachment_download", Data: protocol.AttachmentDownload{Stream: protocol.StreamID{2}, Attachment: attachment}}
				case "start_session":
					if test.receipt == "" || receipt.Kind == "not_admitted" || receipt.Kind == "completed" {
						return link.Response{}, false
					}
					if test.receipt == "removed" {
						result := base.admitStart(request)
						delete(base.sessions, result.Data.(protocol.QueuedTurn).Session)
						return link.Response{Output: result}, true
					}
					if test.receipt == "missing" {
						return link.Response{Fault: protocol.NewAttachmentNotFound(attachment.ID)}, true
					}
					if test.receipt == "admitted" || test.recovery == "snapshot" {
						admitted = base.admitStart(request).Data.(protocol.QueuedTurn)
					}
					return link.Response{Fault: &protocol.Fault{Code: protocol.ErrorCodeOutcomeUnknown, Message: "first acknowledgement was lost"}}, true
				case "inspect_request":
					query, _ := detail[protocol.CommandInspectRequestPayload](request.Command.Data)
					if receipt.Kind == "" {
						return link.Response{Fault: &protocol.Fault{Code: protocol.ErrorCodeUnavailable, Message: "inspection is unavailable"}}, true
					}
					output = protocol.Output{Kind: "request_outcome", Data: protocol.OutputRequestOutcomePayload{ID: query.ID, Outcome: receipt}}
				default:
					return link.Response{}, false
				}
				return link.Response{Output: output}, true
			}
			controller := client.New(transport)
			if err := controller.Start(t.Context()); err != nil {
				t.Fatal(err)
			}
			defer controller.Close()
			r, err := NewWithPreferences(t.Context(), controller, preferences.Open(filepath.Join(t.TempDir(), "preferences.json")))
			if err != nil {
				t.Fatal(err)
			}
			defer r.Close()
			var target *viewmodel.Project
			if withProject {
				target = r.App.Data.Projects[0]
			}
			r.newDraft(target)
			draft := r.App.Sidebar.Selected
			cv, comp := r.App.Data.Find(draft), r.App.Composer(draft)
			var view *ui.Context
			var composeContext *composer.Context
			var actions *conversationActions
			tester := ui.NewTester(func(c *ui.Context) {
				view = c
				r.drain(c)
				context, action := r.conversation(c, cv, comp)
				composeContext = context
				actions = action.(*conversationActions)
				r.finishFrame()
			}, 660, 480)
			settle := func() {
				for range 6 {
					r.workers.Wait()
					tester.Frame()
				}
			}
			settle()
			chosen := modelKey(provider, "chosen")
			comp.Model, comp.Team, comp.Mode = chosen, "", composer.Plan
			comp.AttachPaths(view, []string{source}, actions)
			settle()
			if len(comp.Attachments) != 1 || comp.Attachments[0].Status != composer.Ready || comp.Attachments[0].Image == nil {
				t.Fatalf("draft image did not become ready and previewable: %+v", comp.Attachments)
			}
			if cv.ID != draft || r.App.Sidebar.Selected != draft || r.App.Sidebar.Data.Find(draft) != nil || comp.Team != "" || comp.Model != chosen || comp.Mode != composer.Plan {
				t.Fatal("staging changed the selected draft, model or work mode")
			}
			base.mu.Lock()
			before := slices.Clone(base.requests)
			base.mu.Unlock()
			for _, request := range before {
				switch request.Command.Kind {
				case "create_session", "start_session", "submit_turn", "set_session_team":
					t.Fatalf("staging created or changed history: %s", request.Command.Kind)
				case "upload_attachment":
					spec, _ := detail[protocol.AttachmentSpec](request.Command.Data)
					if spec.Worktree != (protocol.WorktreeID{}) || request.Target != node {
						t.Fatalf("draft upload scope = %+v on %v", spec, request.Target)
					}
				case "finish_attachment_upload":
					spec, _ := detail[protocol.CommandFinishAttachmentUploadPayload](request.Command.Data)
					if spec.Worktree != (protocol.WorktreeID{}) {
						t.Fatalf("draft publication scope = %+v", spec)
					}
				}
			}
			if !bytes.Equal(transport.uploaded.Bytes(), pngBytes.Bytes()) {
				t.Fatal("the staged upload changed the attachment bytes")
			}
			message := composer.Message{Attachments: comp.Attachments, Model: chosen, Mode: composer.Plan, Permission: composer.AskFirst}
			if test.recovery != "" {
				message.Text = "Original first message"
			}
			actions.Send(message)
			if !actions.attachmentsLocked() {
				t.Fatal("attachment mutations remained enabled during first admission")
			}
			comp.AttachPaths(view, []string{source}, actions)
			if len(comp.Attachments) != 1 {
				t.Fatal("a later upload slipped into the in-flight draft")
			}
			settle()
			if test.receipt == "admitted" {
				if cv.ID != draft || !actions.attachmentsLocked() {
					t.Fatal("unconfirmed admission released the draft prematurely")
				}
				base.mu.Lock()
				session := base.sessions[admitted.Session]
				base.snapshot.Cursor++
				base.snapshot.Sessions = []protocol.Session{session}
				base.snapshot.Worktrees = []protocol.Worktree{{ID: session.Worktree}}
				base.snapshot.Turns = []protocol.TurnSummary{{ID: admitted.ID, Session: admitted.Session, Request: admitted.Request}}
				base.mu.Unlock()
				if err := controller.Refresh(t.Context()); err != nil {
					t.Fatal(err)
				}
				settle()
			}
			attempts := func() (starts, checks []protocol.Request) {
				base.mu.Lock()
				requests := slices.Clone(base.requests)
				base.mu.Unlock()
				for _, request := range requests {
					switch request.Command.Kind {
					case "start_session":
						starts = append(starts, request)
					case "inspect_request":
						checks = append(checks, request)
					case "create_session", "create_session_at", "submit_turn", "set_session_team":
						t.Fatalf("first Send split admission or changed the selected mode: %s", request.Command.Kind)
					}
				}
				return starts, checks
			}
			starts, _ := attempts()
			if len(starts) != 1 || starts[0].Target != node {
				t.Fatalf("first Send did not admit once on the execution Node: %+v", starts)
			}
			input, _ := detail[protocol.ConversationStart](starts[0].Command.Data)
			if !input.UseExplicitConfig || input.Title != "" || input.Execution != protocol.ConversationExecutionModel ||
				input.Config == nil || input.Config.Model != "chosen" || input.Config.Mode != protocol.WorkModePlan ||
				!slices.Equal(input.Message.Attachments, []protocol.AttachmentID{attachment.ID}) {
				t.Fatalf("first message or configuration was lost: %+v", input)
			}
			if withProject && (input.Project == nil || *input.Project != project || input.Worktree == nil || *input.Worktree != worktree) ||
				!withProject && (input.Project != nil || input.Worktree != nil) {
				t.Fatalf("first Send changed project or worktree: %+v", input)
			}
			if test.receipt == "unknown" || test.receipt == "unrecorded" {
				assertGuard := func() {
					t.Helper()
					if cv.ID != draft || cv.Node != node.String() || !composeContext.LocationLocked || composeContext.SendBlocked ||
						r.conversationCreating[draft] || r.composerOutgoing[draft] != nil || len(cv.Turns) != 0 || actions.attachmentsLocked() ||
						len(comp.Attachments) != 1 || comp.Attachments[0].Status != composer.Ready {
						t.Fatal("an unknown result did not retain the location while releasing the composer and attachment")
					}
				}
				assertGuard()
				if test.recovery == "" {
					return
				}
				original := starts[0].ID
				assertChecks := func(count int) {
					t.Helper()
					starts, checks := attempts()
					if len(starts) != 1 || len(checks) != count {
						t.Fatalf("wanted one write and %d inspections, got %d writes and %d inspections", count, len(starts), len(checks))
					}
					var digest string
					for _, request := range checks {
						query, ok := detail[protocol.CommandInspectRequestPayload](request.Command.Data)
						if !ok || request.Target != node || query.ID != original || query.Digest == "" || digest != "" && digest != query.Digest {
							t.Fatalf("inspection changed the first send's immutable identity: %+v on %v", query, request.Target)
						}
						digest = query.Digest
					}
				}
				setReceipt := func(value protocol.RequestOutcome) {
					base.mu.Lock()
					receipt = value
					base.mu.Unlock()
				}
				assertChecks(1)
				later := message
				later.Text = "Typed after the first send"
				setReceipt(protocol.RequestOutcome{})
				actions.Send(later)
				actions.Send(later)
				assertGuard()
				settle()
				assertGuard()
				assertChecks(2)
				setReceipt(protocol.RequestOutcome{Kind: "unrecorded"})
				r.Refresh()
				settle()
				assertGuard()
				assertChecks(3)
				r.readsConnected = false
				settle()
				assertGuard()
				assertChecks(4)
				if test.recovery == "snapshot" {
					alternate := &conversationTestTransport{snapshot: protocol.DefaultSnapshot()}
					alternate.snapshot.Node, alternate.snapshot.Providers = protocol.NodeID{12}, snapshot.Providers
					other := client.New(alternate)
					t.Cleanup(func() { _ = other.Close() })
					if err := other.Start(t.Context()); err != nil {
						t.Fatal(err)
					}
					bound := r.newBinding(other)
					r.bindings = map[string]*Runtime{other.Target().String(): bound}
					comp.Host = other.Target().String()
					tester.Frame()
					if actions.r != r || !composeContext.LocationLocked || comp.Host != node.String() {
						t.Fatal("an alternate Host displaced the guarded draft's execution binding")
					}
					actions.Send(later)
					bound.workers.Wait()
					settle()
					alternate.mu.Lock()
					wrongNode := slices.ContainsFunc(alternate.requests, func(request protocol.Request) bool {
						return request.Command.Kind == "start_session" || request.Command.Kind == "inspect_request"
					})
					alternate.mu.Unlock()
					if wrongNode || comp.Host != node.String() {
						t.Fatal("alternate Node received the guarded send or its inspection")
					}
					assertGuard()
					assertChecks(5)
					for index, request := range []protocol.RequestID{protocol.NewRequestID(), original} {
						base.mu.Lock()
						session := base.sessions[admitted.Session]
						base.snapshot.Cursor++
						base.snapshot.Sessions = []protocol.Session{session}
						base.snapshot.Worktrees = []protocol.Worktree{{ID: session.Worktree}}
						base.snapshot.Turns = []protocol.TurnSummary{{ID: admitted.ID, Session: admitted.Session, Request: request}}
						base.mu.Unlock()
						if err := controller.Refresh(t.Context()); err != nil {
							t.Fatal(err)
						}
						settle()
						if index == 0 {
							assertGuard()
						}
					}
					if cv.ID != admitted.Session.String() || r.App.Sidebar.Selected != cv.ID || r.App.Composer(cv.ID) != comp ||
						comp.Draft != later.Text || r.guardedCreation(draft) != nil || r.composerOutgoing[cv.ID] != nil {
						t.Fatal("matching snapshot did not adopt the original session without clearing the later input")
					}
					assertChecks(5)
					return
				}
				resolved := protocol.RequestOutcome{Kind: test.recovery}
				if test.recovery == "completed" {
					resolved.Data = protocol.Result[protocol.Output, protocol.Fault]{Err: &protocol.Fault{Code: protocol.ErrorCodeConflict, Message: "first send failed"}}
				}
				setReceipt(resolved)
				actions.Send(later)
				settle()
				assertChecks(5)
				if cv.ID != draft || composeContext.LocationLocked || comp.Draft != later.Text || r.guardedCreation(draft) != nil {
					t.Fatal("definite rejection did not release the guard and retain the edited draft")
				}
				actions.Send(later)
				settle()
				starts, checks := attempts()
				if len(starts) != 2 || starts[1].ID == original || starts[1].Target != node || len(checks) != 5 || cv.ID == draft {
					t.Fatalf("the explicit Send after rejection did not start a fresh request: %+v", starts)
				}
				retry, _ := detail[protocol.ConversationStart](starts[1].Command.Data)
				if retry.Message.Text != later.Text {
					t.Fatalf("the explicit Send prepared the wrong draft: %q", retry.Message.Text)
				}
				return
			}
			if test.receipt == "removed" {
				if cv.ID != draft || actions.attachmentsLocked() || len(comp.Attachments) != 0 || r.composerOutgoing[draft] != nil {
					t.Fatal("a confirmed send left its draft locked after the conversation was removed")
				}
				return
			}
			if test.receipt == "missing" {
				if cv.ID != draft || actions.attachmentsLocked() || len(comp.Attachments) != 1 || comp.Attachments[0].Status != composer.Failed {
					t.Fatal("the missing upload was not made retryable")
				}
				base.mu.Lock()
				attachment.ID = protocol.AttachmentID{8}
				base.mu.Unlock()
				actions.Upload(comp.Attachments[0])
				settle()
				if comp.Attachments[0].ID != attachment.ID.String() || comp.Attachments[0].Status != composer.Ready {
					t.Fatal("retry did not replace the missing upload")
				}
				return
			}
			session, exists := r.session(cv.ID)
			preview, found := r.attachmentDescriptor(cv.ID, attachment.ID.String())
			if cv.ID == draft || r.App.Sidebar.Selected != cv.ID || !exists || !found || preview.worktree != session.Worktree ||
				preview.worktree == (protocol.WorktreeID{}) || len(comp.Attachments) != 0 || comp.Team != "" || comp.Model != chosen || comp.Mode != composer.Plan {
				t.Fatalf("admission did not adopt the draft and preview: %+v %+v", session, preview)
			}
		})
	}
}

func TestFirstSendRecoverySurvivesBindingReplacement(t *testing.T) {
	node := protocol.NodeID{4}
	connect := func(id protocol.NodeID) (*client.Client, *conversationTestTransport) {
		fixture := &conversationTestTransport{snapshot: protocol.DefaultSnapshot()}
		fixture.snapshot.Node = id
		controller := client.New(fixture)
		if err := controller.Start(t.Context()); err != nil {
			t.Fatal(err)
		}
		t.Cleanup(func() { _ = controller.Close() })
		return controller, fixture
	}
	local, _ := connect(protocol.NodeID{1})
	root, err := NewWithPreferences(t.Context(), local, preferences.Open(filepath.Join(t.TempDir(), "preferences.json")))
	if err != nil {
		t.Fatal(err)
	}
	defer root.Close()
	old, _ := connect(node)
	owner := root.newBinding(old)
	root.bindings = map[string]*Runtime{node.String(): owner}
	root.newDraft(nil)
	cv := root.App.Data.Find(root.App.Sidebar.Selected)
	if cv == nil {
		t.Fatal("fixture draft is unavailable")
	}
	comp := root.App.Composer(cv.ID)
	cv.Node, comp.Host, comp.Draft = node.String(), node.String(), "Later input"
	attachment := protocol.Attachment{ID: protocol.AttachmentID{7}, Spec: protocol.AttachmentSpec{Name: "draft.txt"}}
	card := &composer.Attachment{ID: attachment.ID.String(), Name: attachment.Spec.Name, Status: composer.Ready}
	comp.Attachments = []*composer.Attachment{card}
	owner.attachments.items[card] = &attachmentRecord{composer: comp, conversation: cv.ID, ready: &attachment}
	message := composer.Message{Text: "First input", Attachments: []*composer.Attachment{card}}
	input := protocol.DefaultConversationStart()
	input.Message = conversationInput(message)
	request := old.Prepare(protocol.Command{Kind: "start_session", Data: &input})
	body, err := json.Marshal(request)
	if err != nil {
		t.Fatalf("invalid original request: %v", err)
	}
	wantDigest := fmt.Sprintf("%x", blake3.Sum256(body))
	owner.conversationCreations[cv.ID] = &conversationCreation{request: request, dispatched: true, checking: true,
		actions: &conversationActions{r: owner, cv: cv, comp: comp, restore: &message}}
	owner.conversationCreating[cv.ID] = true
	root.retireBinding(node.String(), owner)
	guard := root.guardedCreation(cv.ID)
	if guard == nil || guard.actions != nil || guard.request.ID != request.ID || guard.request.Target != node || !root.draftSubmitted(cv) {
		t.Fatal("retiring the binding lost the original request or retained its callbacks")
	}
	if root.conversationRuntime(cv, comp) != nil {
		t.Fatal("the unresolved draft switched to the local Node while its owner was unavailable")
	}
	replacement, transport := connect(node)
	transport.mu.Lock()
	transport.handle = func(read protocol.Request) (link.Response, bool) {
		if read.Command.Kind != "inspect_request" {
			return link.Response{}, false
		}
		query, _ := detail[protocol.CommandInspectRequestPayload](read.Command.Data)
		return link.Response{Output: protocol.Output{Kind: "request_outcome", Data: protocol.OutputRequestOutcomePayload{ID: query.ID, Outcome: protocol.RequestOutcome{Kind: "unrecorded"}}}}, true
	}
	transport.mu.Unlock()
	bound := root.newBinding(replacement)
	root.bindings[node.String()] = bound
	restored := bound.attachments.items[card]
	descriptor, found := bound.attachmentDescriptor(cv.ID, attachment.ID.String())
	if restored == nil || restored.ready == nil || *restored.ready != attachment || restored.composer != comp ||
		!found || descriptor.attachment == nil || *descriptor.attachment != attachment {
		t.Fatal("reattachment did not restore the original draft's confirmed attachment")
	}
	bound.retryFailedReads()
	bound.workers.Wait()
	transport.mu.Lock()
	requests := slices.Clone(transport.requests)
	transport.mu.Unlock()
	checks := 0
	for _, read := range requests {
		switch read.Command.Kind {
		case "start_session":
			t.Fatal("reconnecting reissued the first send")
		case "inspect_request":
			query, ok := detail[protocol.CommandInspectRequestPayload](read.Command.Data)
			if !ok || query.ID != request.ID || query.Digest != wantDigest || read.Target != node {
				t.Fatalf("reconnected inspection lost its request or Node: %#v", read)
			}
			checks++
		}
	}
	if checks != 1 || root.conversationRuntime(cv, comp) != bound || comp.Draft != "Later input" {
		t.Fatalf("reattachment performed %d checks or lost the draft binding or input", checks)
	}
}

func TestExecuteScopedUsesTypedForwardRecovery(t *testing.T) {
	for _, kind := range []string{"unrecorded", "replayable"} {
		t.Run(kind, func(t *testing.T) {
			node, target := protocol.NodeID{1}, protocol.NodeID{2}
			base := &conversationTestTransport{snapshot: protocol.DefaultSnapshot()}
			base.snapshot.Node = node
			fault := &protocol.Fault{Code: protocol.ErrorCodeOutcomeUnknown, Message: "forwarded result unavailable"}
			var writes []protocol.RequestID
			base.handle = func(request protocol.Request) (link.Response, bool) {
				switch request.Command.Kind {
				case "create_session_at":
					writes = append(writes, request.ID)
					return link.Response{Fault: fault}, true
				case "inspect_request":
					query, _ := detail[protocol.CommandInspectRequestPayload](request.Command.Data)
					outcome := protocol.RequestOutcome{Kind: kind}
					if kind == "replayable" {
						outcome.Data = fault
					}
					return link.Response{Output: protocol.Output{Kind: "request_outcome", Data: protocol.OutputRequestOutcomePayload{ID: query.ID, Outcome: outcome}}}, true
				}
				return link.Response{}, false
			}
			controller := client.New(base)
			defer controller.Close()
			r := &Runtime{ctx: t.Context(), controller: controller, busy: map[string]bool{}, uncertain: map[string]protocol.Request{}}
			tester := ui.NewTester(func(c *ui.Context) {
				r.mu.Lock()
				queue := r.queue
				r.queue = nil
				r.mu.Unlock()
				for _, complete := range queue {
					complete(c)
				}
			}, 320, 240)
			command := protocol.Command{Kind: "create_session_at", Data: &protocol.CommandCreateSessionAtPayload{Target: target}}
			execute := func() protocol.RequestID {
				t.Helper()
				called := false
				id := r.executeScoped("legacy-forward", command, func(_ *ui.Context, _ protocol.Output, err error) {
					called = true
					released := kind == "unrecorded"
					if faultCode(err) != protocol.ErrorCodeOutcomeUnknown || client.Released(err) != released || client.Retained(err) != !released {
						t.Fatalf("unexpected forward resolution: %v", err)
					}
				})
				r.workers.Wait()
				tester.Frame()
				if !called || id == (protocol.RequestID{}) {
					t.Fatal("scoped command did not complete")
				}
				return id
			}
			first := execute()
			if !slices.Equal(writes, []protocol.RequestID{first}) {
				t.Fatalf("scoped execution wrote more than once: %v", writes)
			}
			if kind == "unrecorded" {
				if len(r.uncertain) != 0 {
					t.Fatal("a released forward was kept for replay")
				}
			} else if second := execute(); second != first || !slices.Equal(writes, []protocol.RequestID{first, first}) {
				t.Fatalf("explicit replay did not use the frozen forward's original request: %v", writes)
			}
		})
	}
}

type draftAttachmentTransport struct {
	*conversationTestTransport
	image    []byte
	uploaded bytes.Buffer
}

func (t *draftAttachmentTransport) Open(_ context.Context, id protocol.StreamID) (link.Stream, error) {
	stream, peer := link.StreamPair()
	go func() {
		defer peer.Close()
		if id == (protocol.StreamID{1}) {
			if _, err := io.Copy(&t.uploaded, peer); err == nil {
				_, _ = peer.Write([]byte{protocol.FILE_UPLOAD_READY})
			}
			return
		}
		_, _ = io.Copy(io.Discard, peer)
		_, _ = peer.Write(t.image)
	}()
	return stream, nil
}
