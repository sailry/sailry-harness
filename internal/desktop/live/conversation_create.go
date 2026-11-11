package live

import (
	"encoding/json"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/composer"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
)

type conversationCreation struct {
	key     string
	command protocol.Command
}

func creationKey(c *composer.State, m composer.Message) string {
	value, _ := json.Marshal(struct {
		Input                                  protocol.ConversationInput
		Host, Project, Worktree, Model, Effort string
		Mode                                   composer.Mode
		Permission                             composer.Permission
		Features                               map[string]string
	}{conversationInput(m), c.Host, c.Project, c.Worktree, m.Model, m.Effort, m.Mode, m.Permission, m.Features})
	return string(value)
}

func (x *conversationActions) create(command protocol.Command, m composer.Message, finish func(protocol.Output, bool)) {
	before := x.cv.ID
	x.r.conversationCreating[before] = true
	x.r.executeScoped("create/"+before, command, func(c *ui.Context, out protocol.Output, err error) {
		delete(x.r.conversationCreating, before)
		if err != nil {
			x.r.report(c, err, "session_changed")
			if faultCode(err) != protocol.ErrorCodeOutcomeUnknown {
				delete(x.r.conversationCreations, before)
			}
			return
		}
		session, valid := detail[protocol.Session](out.Data)
		if !valid {
			x.r.report(c, &link.FaultError{Fault: protocol.Fault{Code: protocol.ErrorCodeOutcomeUnknown, Message: "Session creation result is unavailable"}}, "session_changed")
			return
		}
		delete(x.r.conversationCreations, before)
		x.r.admittedSessions[session.ID.String()] = session
		x.r.App.ReplaceConversationID(before, session.ID.String())
		x.cv.ID = session.ID.String()
		x.r.composerInitialized[x.cv.ID] = true
		x.r.composerRevisions[x.cv.ID] = session.Revision
		x.r.composerBases[x.comp] = session.Config
		key, _ := json.Marshal(session.Config)
		x.r.composerObserved[x.cv.ID] = string(key)
		// The created session stays selected only where its draft still is.
		x.r.project()
		if x.created != nil {
			x.created(session)
		}
		x.r.withComposerTeam(session, x.comp, func(current protocol.Session) {
			x.turn(protocol.Command{Kind: "submit_turn", Data: protocol.CommandSubmitTurnPayload{Session: current.ID, ExpectedRevision: current.Revision, Message: conversationInput(m)}}, finish)
		})
	})
}
