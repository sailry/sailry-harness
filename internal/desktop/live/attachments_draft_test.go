package live

import (
	"bytes"
	"context"
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
)

func TestDraftAttachmentStartsConversationOnlyOnSend(t *testing.T) {
	for _, test := range []struct {
		name, receipt string
		withProject   bool
	}{
		{name: "projectless"},
		{name: "in project", withProject: true},
		{name: "late snapshot proves first admission", receipt: "admitted"},
		{name: "terminal unknown releases the draft", receipt: "unknown"},
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
					if test.receipt == "" {
						return link.Response{}, false
					}
					if test.receipt == "admitted" {
						admitted = base.admitStart(request).Data.(protocol.QueuedTurn)
					}
					return link.Response{Fault: &protocol.Fault{Code: protocol.ErrorCodeOutcomeUnknown, Message: "first acknowledgement was lost"}}, true
				case "inspect_request":
					query, _ := detail[protocol.CommandInspectRequestPayload](request.Command.Data)
					result := protocol.RequestOutcome{Kind: test.receipt}
					output = protocol.Output{Kind: "request_outcome", Data: protocol.OutputRequestOutcomePayload{ID: query.ID, Outcome: result}}
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
			var actions *conversationActions
			tester := ui.NewTester(func(c *ui.Context) {
				view = c
				r.drain(c)
				_, action := r.conversation(c, cv, comp)
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
			base.mu.Lock()
			requests := slices.Clone(base.requests)
			base.mu.Unlock()
			var starts []protocol.Request
			for _, request := range requests {
				switch request.Command.Kind {
				case "start_session":
					starts = append(starts, request)
				case "create_session", "submit_turn", "set_session_team":
					t.Fatalf("first Send split admission or changed the selected mode: %s", request.Command.Kind)
				}
			}
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
			if test.receipt == "unknown" {
				if cv.ID != draft || actions.attachmentsLocked() || len(comp.Attachments) != 1 || comp.Attachments[0].Status != composer.Ready {
					t.Fatal("a terminal unknown result did not unlock the original draft and attachment")
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
