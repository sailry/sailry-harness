package live

import (
	"encoding/json"
	"testing"
	"testing/synctest"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/composer"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
)

const submissionAnswers = `[{"name":"approach","label":"Approach","values":[{"value":"a","label":"A"}]}]`

type documentSubmissionFixture struct {
	r       *Runtime
	node    *terminalTabsTestNode
	content protocol.ArtifactContent
	comp    *composer.State
	frame   func(func(*ui.Context))
	fail    string
	code    protocol.ErrorCode
	turn    protocol.RequestID
}

func newDocumentSubmissionFixture(t *testing.T) *documentSubmissionFixture {
	t.Helper()
	content := documentFormContent()
	provider := protocol.ProviderID{1}
	model := protocol.DefaultConversationModel()
	model.ID = "fixture"
	session := protocol.DefaultSession()
	session.ID, session.Revision = content.Artifact.Session, 1
	session.Config.Provider, session.Config.Model = provider, model.ID
	session.Config.Mode, session.Config.Permission = protocol.WorkModeCode, protocol.PermissionAsk
	session.Roles.Execution = protocol.ConversationExecutionModel
	snapshot := protocol.DefaultSnapshot()
	snapshot.Node, snapshot.Sessions = protocol.NodeID{1}, []protocol.Session{session}
	snapshot.Providers = []protocol.ConversationProvider{{ID: provider, Enabled: true, Models: []protocol.ConversationModel{model}}}
	r, node := snapshotFixture(t, snapshot)
	r.App.Data.Unassigned = []*viewmodel.Conversation{{ID: session.ID.String(), Node: snapshot.Node.String()}}
	r.browser = &browserState{sent: map[documentDraftKey]map[string]bool{}, drafts: map[documentDraftKey]map[string]json.RawMessage{}}
	r.composerSaving, r.composerObserved, r.teams = map[string]bool{}, map[string]string{}, newTeamState()
	comp := r.App.Composer(session.ID.String())
	comp.Host, comp.Model, comp.Team = snapshot.Node.String(), modelKey(provider, model.ID), ""
	comp.Mode, comp.Permission = modeChoice(session.Config.Mode), permissionChoice(session.Config.Permission)
	f := &documentSubmissionFixture{r: r, node: node, content: content, comp: comp}
	node.respond = func(request protocol.Request) link.Response {
		if request.Command.Kind == f.fail {
			if request.Command.Kind == "submit_turn" {
				f.turn = request.ID
			}
			return link.Response{Fault: &protocol.Fault{Code: f.code, Message: "fixture rejection"}}
		}
		switch request.Command.Kind {
		case "list_artifact_revisions":
			payload, _ := detail[protocol.CommandListArtifactRevisionsPayload](request.Command.Data)
			page := protocol.ArtifactRevisionPage{Items: []protocol.ArtifactRevision{{ID: payload.ID, Revision: content.Artifact.Revision}}}
			return link.Response{Output: protocol.Output{Kind: "artifact_revisions", Data: page}}
		case "inspect_request":
			query, _ := detail[protocol.CommandInspectRequestPayload](request.Command.Data)
			result := protocol.OutputRequestOutcomePayload{ID: query.ID, Outcome: protocol.RequestOutcome{Kind: "admitted"}}
			return link.Response{Output: protocol.Output{Kind: "request_outcome", Data: result}}
		case "submit_turn":
			queued := protocol.DefaultQueuedTurn()
			queued.Session, queued.ID, queued.Request = session.ID, protocol.TurnID{2}, request.ID
			return link.Response{Output: protocol.Output{Kind: "queued_turn", Data: queued}}
		}
		return link.Response{Fault: &protocol.Fault{Code: protocol.ErrorCodeUnavailable, Message: "unexpected fixture command"}}
	}
	var apply func(*ui.Context)
	tester := ui.NewTesterWith(func(c *ui.Context) {
		theme.Apply(c, "", theme.Light)
		if apply != nil {
			action := apply
			apply = nil
			action(c)
		}
	}, 1, 1, ui.TesterOptions{Preferences: ui.Preferences{ReduceTransparency: true}})
	f.frame = func(action func(*ui.Context)) {
		apply = action
		tester.Frame()
	}
	return f
}

func (f *documentSubmissionFixture) submit(content protocol.ArtifactContent) documentDraftKey {
	source := documentSource{content: content, owner: f.r, conversation: content.Artifact.Session}
	page := &documentView{source: source}
	f.frame(func(c *ui.Context) {
		f.r.submitDocumentForm(c, page, documentFormEvent(content, "submit", "decisions", submissionAnswers))
	})
	page.source = documentSource{} // the page has closed before the async result
	return documentDraftKey{content.Artifact.ID, content.Artifact.Revision}
}

func (f *documentSubmissionFixture) settle() {
	for {
		f.r.workers.Wait()
		f.r.mu.Lock()
		callbacks := f.r.queue
		f.r.queue = nil
		f.r.mu.Unlock()
		if len(callbacks) == 0 {
			return
		}
		f.frame(func(c *ui.Context) {
			for _, callback := range callbacks {
				callback(c)
			}
		})
	}
}

func TestDocumentSubmissionPrerequisiteFailure(t *testing.T) {
	for _, command := range []string{"set_session_config", "set_session_team"} {
		t.Run(command, func(t *testing.T) {
			synctest.Test(t, func(t *testing.T) {
				f := newDocumentSubmissionFixture(t)
				f.fail, f.code = command, protocol.ErrorCodeConflict
				if command == "set_session_config" {
					f.comp.Mode = composer.Plan
				} else {
					f.comp.Team = defaultTeamID
				}
				key := f.submit(f.content)
				f.r.browser.drafts[key] = map[string]json.RawMessage{"decisions": json.RawMessage(submissionAnswers)}
				f.settle()
				if len(f.node.sent(command)) != 1 || len(f.node.sent("submit_turn")) != 0 {
					t.Fatal("did not stop at the rejected prerequisite", f.node.commands)
				}
				if _, locked := f.r.browser.sent[key]["decisions"]; locked || f.r.browser.drafts[key]["decisions"] == nil {
					t.Fatal("a failed prerequisite locked the form or lost its answers")
				}
				f.comp.Mode, f.comp.Team = modeChoice(protocol.WorkModeCode), ""
				f.submit(f.content)
				f.settle()
				if len(f.node.sent("submit_turn")) != 1 || !f.r.browser.sent[key]["decisions"] || f.r.browser.drafts[key]["decisions"] != nil {
					t.Fatal("retry did not submit and acknowledge the answer exactly once", f.node.commands)
				}
			})
		})
	}
}

func TestDocumentSubmissionRequiresAdmittedSession(t *testing.T) {
	synctest.Test(t, func(t *testing.T) {
		f := newDocumentSubmissionFixture(t)
		for _, id := range []string{"draft/unadmitted", "invalid", (protocol.SessionID{9}).String()} {
			cv := &viewmodel.Conversation{ID: id, Node: f.r.controller.Target().String()}
			calls := 0
			f.frame(func(c *ui.Context) {
				x := &conversationActions{r: f.r, c: c, cv: cv, comp: f.comp, settled: func(accepted bool) {
					calls++
					if accepted {
						t.Errorf("accepted a reply to %q", id)
					}
				}}
				x.submit(f.comp.Reply(f.r.models(), "answers"), false, false, documentReplyDraft)
			})
			f.settle()
			if calls != 1 || len(f.node.commands) != 0 {
				t.Fatalf("reply to %q: %d outcomes, commands %v", id, calls, f.node.commands)
			}
		}
	})
}

func TestDocumentSubmissionHistoryAcrossRevisions(t *testing.T) {
	synctest.Test(t, func(t *testing.T) {
		f := newDocumentSubmissionFixture(t)
		first := f.submit(f.content)
		f.settle()
		if !f.r.browser.sent[first]["decisions"] {
			t.Fatal("first form was not accepted", f.node.commands)
		}
		for revision := uint32(4); revision < 260; revision++ {
			f.r.browser.sent[documentDraftKey{first.artifact, revision}] = map[string]bool{"decisions": true}
		}
		pendingContent := documentFormContent()
		pending := f.submit(pendingContent)
		f.submit(documentFormContent()) // another form before the pending submission settles
		if len(f.r.browser.sent) != 259 {
			t.Fatalf("expected all accepted and pending forms to remain, got %d", len(f.r.browser.sent))
		}
		if !f.r.browser.sent[first]["decisions"] {
			t.Fatal("opening another revision forgot an admitted form")
		}
		if accepted, exists := f.r.browser.sent[pending]["decisions"]; !exists || accepted {
			t.Fatal("opening another revision forgot or prematurely accepted an in-flight form")
		}
		f.submit(pendingContent)
		f.submit(f.content)
		f.settle()
		if len(f.node.sent("submit_turn")) != 3 || len(f.node.sent("list_artifact_revisions")) != 3 {
			t.Fatal("revisiting an accepted or pending form dispatched another request", f.node.commands)
		}
	})
}

func TestDocumentSubmissionUnknownTurn(t *testing.T) {
	synctest.Test(t, func(t *testing.T) {
		f := newDocumentSubmissionFixture(t)
		f.fail, f.code = "submit_turn", protocol.ErrorCodeOutcomeUnknown
		key := f.submit(f.content)
		f.r.browser.drafts[key] = map[string]json.RawMessage{"decisions": json.RawMessage(submissionAnswers)}
		f.settle()
		inspections := f.node.sent("inspect_request")
		if len(inspections) != 1 || inspections[0].Data.(protocol.CommandInspectRequestPayload).ID != f.turn {
			t.Fatal("pending turn was not inspected by its original request ID", inspections)
		}
		if accepted, exists := f.r.browser.sent[key]["decisions"]; !exists || accepted || f.r.browser.drafts[key]["decisions"] == nil {
			t.Fatal("an unknown turn was unlocked, acknowledged, or lost its answers")
		}
		f.submit(f.content)
		f.settle()
		if len(f.node.sent("submit_turn")) != 1 || len(f.node.sent("list_artifact_revisions")) != 1 {
			t.Fatal("an uncertain turn was sent twice", f.node.commands)
		}
		f.frame(func(*ui.Context) {
			f.r.settleTurnRetry(f.content.Artifact.Session.String(), f.comp, []protocol.TurnSummary{{Request: f.turn}}, protocol.ConversationPage{})
		})
		if !f.r.browser.sent[key]["decisions"] || f.r.browser.drafts[key]["decisions"] != nil {
			t.Fatal("observing the original request did not acknowledge its form")
		}
	})
}
