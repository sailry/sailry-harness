package live

import (
	"encoding/json"
	"slices"
	"strings"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/composer"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
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

// composerProviders are the providers a composer of this Node resolves its
// model in: the Node's own; on another Node the local Node's as well, whose
// configuration a new session there is forwarded with; and the private
// provider of each session that was created that way.
func (r *Runtime) composerProviders() []protocol.ConversationProvider {
	snapshot := r.snapshot()
	providers := slices.Clone(snapshot.Providers)
	if r.parent != nil {
		providers = append(providers, r.parent.snapshot().Providers...)
	}
	private := func(session protocol.Session) {
		if session.Profile != nil && !slices.ContainsFunc(providers, func(p protocol.ConversationProvider) bool { return p.ID == session.Profile.Provider.ID }) {
			providers = append(providers, session.Profile.Provider)
		}
	}
	for _, session := range snapshot.Sessions {
		private(session)
	}
	for _, session := range r.admittedSessions {
		private(session)
	}
	return providers
}

// forwards reports whether provider is the local Node's while this Node is
// another: a session created with it is forwarded by the local Node.
func (r *Runtime) forwards(provider protocol.ProviderID) bool {
	is := func(p protocol.ConversationProvider) bool { return p.ID == provider }
	return r.parent != nil && !slices.ContainsFunc(r.snapshot().Providers, is) && slices.ContainsFunc(r.parent.snapshot().Providers, is)
}

// offeredModels are the models conversation id's composer offers: those of
// the Node's own providers; for a draft on another Node the local Node's as
// well; for a forwarded session its private provider's.
func (r *Runtime) offeredModels(id string, models []composer.Model) []composer.Model {
	allowed := map[string]bool{}
	for _, provider := range r.snapshot().Providers {
		allowed[provider.ID.String()] = true
	}
	if session, exists := r.session(id); !exists {
		if r.parent != nil {
			for _, provider := range r.parent.snapshot().Providers {
				allowed[provider.ID.String()] = true
			}
		}
	} else if session.Profile != nil {
		allowed[session.Profile.Provider.ID.String()] = true
	}
	var offered []composer.Model
	for _, model := range models {
		if provider, _, _ := strings.Cut(model.ID, "/"); allowed[provider] {
			offered = append(offered, model)
		}
	}
	return offered
}

// memberModels are those of offered a team member may take: nil for all of
// them on the local Node, and on another Node only that Node's own, since a
// member's model is resolved there and the local Node's providers reach it
// only as the lead of a forwarded session.
func (r *Runtime) memberModels(offered []composer.Model) []composer.Model {
	if r.parent == nil {
		return nil
	}
	own := []composer.Model{}
	for _, model := range offered {
		provider, _, _ := strings.Cut(model.ID, "/")
		if slices.ContainsFunc(r.snapshot().Providers, func(p protocol.ConversationProvider) bool { return p.ID.String() == provider }) {
			own = append(own, model)
		}
	}
	return own
}

// creationCommand creates a session on this Node with config: by the Node
// itself where the provider is its own, and forwarded by the local Node,
// which holds the provider at revision and its credential, where it is not.
func (r *Runtime) creationCommand(project *protocol.ProjectID, worktree *protocol.WorktreeID, config protocol.SessionConfig, revision uint64) protocol.Command {
	if r.forwards(config.Provider) {
		return protocol.Command{Kind: "create_session_at", Data: protocol.CommandCreateSessionAtPayload{Target: r.controller.Target(), Project: project, Worktree: worktree, Config: &config, ProviderRevision: revision}}
	}
	return protocol.Command{Kind: "create_session", Data: protocol.CommandCreateSessionPayload{Project: project, Worktree: worktree, Config: &config}}
}

// executeCreation sends a creation command: a forwarded one through the
// local Node's controller, any other through this Node's.
func (r *Runtime) executeCreation(scope string, command protocol.Command, finished func(*ui.Context, protocol.Output, error)) {
	if command.Kind == "create_session_at" && r.parent != nil {
		r.parent.executeScoped(scope, command, finished)
		return
	}
	r.executeScoped(scope, command, finished)
}

// reportCreation toasts why a session was not created. A model that is not
// configured, as a provider the local Node no longer has or an account
// sign-in that stays with the Node it was made on, is its own failure.
func (r *Runtime) reportCreation(c *ui.Context, err error) {
	if faultCode(err) == protocol.ErrorCodeNotConfigured {
		kit.Toast(c, kit.Failed, i18n.T(c, "chat_config_unavailable"))
		return
	}
	r.report(c, err, "session_changed")
}

// adoptForwarded shows a forwarded session's own configuration, whose
// provider is the private copy the execution Node made.
func (r *Runtime) adoptForwarded(comp *composer.State, session protocol.Session) {
	if session.Profile != nil {
		r.applyComposer(comp, session.Config)
	}
}

func (x *conversationActions) create(command protocol.Command, m composer.Message, finish func(protocol.Output, bool)) {
	before := x.cv.ID
	x.r.conversationCreating[before] = true
	x.r.executeCreation("create/"+before, command, func(c *ui.Context, out protocol.Output, err error) {
		delete(x.r.conversationCreating, before)
		if err != nil {
			x.r.reportCreation(c, err)
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
		x.r.browserAdopt(before, session.ID.String())
		x.cv.ID = session.ID.String()
		x.r.composerInitialized[x.cv.ID] = true
		x.r.composerRevisions[x.cv.ID] = session.Revision
		x.r.composerBases[x.comp] = session.Config
		x.r.adoptForwarded(x.comp, session)
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
