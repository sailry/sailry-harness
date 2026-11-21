package live

import (
	"encoding/json"
	"slices"
	"strconv"
	"strings"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/composer"
	"github.com/sailry/sailry-harness/internal/desktop/conversation"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/icons"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/preferences"
	"github.com/sailry/sailry-harness/internal/desktop/settings"
	"github.com/sailry/sailry-harness/internal/desktop/shell"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func (r *Runtime) SetReadAloud(f func(string, string, string)) {
	r.readAloud = f
	for _, bound := range r.bindings {
		bound.SetReadAloud(f)
	}
	if f == nil {
		r.App.Hooks.ReadSelection = nil
		return
	}
	r.App.Hooks.ReadSelection = func(session, text string) { f(session, "selection/"+session, text) }
}
func (r *Runtime) SetReaderActive(f func(string) bool) {
	r.App.Messages.ReaderActive = f
	r.App.Hooks.ReaderActive = f
}
func (r *Runtime) SetExport(f func(string, string, conversation.ExportFormat)) {
	r.exportConversation = f
	if f != nil && r.parent == nil {
		r.exportConversation = func(session, turn string, format conversation.ExportFormat) {
			r.count(protocol.FeatureUseSessionExport)
			f(session, turn, format)
		}
	}
	for _, bound := range r.bindings {
		bound.exportConversation = r.exportConversation
	}
}

func modelKey(provider protocol.ProviderID, model string) string {
	return provider.String() + "/" + model
}

func effortName(e protocol.Effort) string {
	if e.Kind == "budget" {
		if v, ok := detail[int32](e.Data); ok {
			return strconv.FormatInt(int64(v), 10)
		}
	}
	return e.Kind
}
func parseEffort(s string) protocol.Effort {
	if n, err := strconv.ParseInt(s, 10, 32); err == nil {
		return protocol.Effort{Kind: "budget", Data: int32(n)}
	}
	if s == "" {
		s = "default"
	}
	return protocol.Effort{Kind: s}
}

func (r *Runtime) session(id string) (protocol.Session, bool) {
	pending, admitted := r.admittedSessions[id]
	sessionID, err := protocol.ParseSessionID(id)
	if err != nil {
		return pending, admitted
	}
	for _, s := range r.snapshot().Sessions {
		if s.ID == sessionID {
			if admitted && pending.Revision > s.Revision {
				return pending, true
			}
			return s, true
		}
	}
	return pending, admitted
}

func (r *Runtime) conversation(c *ui.Context, cv *viewmodel.Conversation, comp *composer.State) (*composer.Context, shell.ConversationActions) {
	if root := r.localRuntime(); unsentDraft(cv) && root.draftsShown != nil {
		root.draftsShown[cv.ID] = true
	}
	if owner := r.conversationRuntime(cv, comp); owner == nil {
		cv.Session.Connection = conversation.Offline
		// An unadmitted draft can choose another execution host while offline.
		return &composer.Context{Readonly: !unsentDraft(cv), SendBlocked: true, LocationLocked: r.draftSubmitted(cv), Interactions: cv.Waiting, Queue: cv.Queue, Projects: r.composerProjects(), Hosts: r.composerHosts(c)},
			&conversationActions{r: r, c: c, cv: cv, comp: comp}
	} else if owner != r {
		return owner.conversation(c, cv, comp)
	}
	if unsentDraft(cv) {
		// Drafts have no conversation observation to clear an old Host's
		// offline state after the user changes their execution location.
		cv.Session.Connection = conversation.Connected
	}
	snapshot := r.snapshot()
	unsynced := false
	hadTurns := len(cv.Turns) > 0
	id, idErr := protocol.ParseSessionID(cv.ID)
	if idErr == nil {
		r.conversationSeen[id] = true
		if _, ok := r.conversationWatches[id]; !ok {
			r.startConversationFeed(id, c.Locale())
		}
		if feed := r.conversationFeeds[id]; feed != nil {
			feed.setLanguage(c.Locale())
			if r.conversationCopies == nil {
				r.takeConversationFeed(id, feed)
			}
		}
		if state, ok := r.conversationState(id); ok {
			unsynced = !state.Connected
			r.projectWatchedConversation(c, cv, id, state)
			if state.Connected {
				// Elapsed labels need a rebuilt frame, including in plugin panels;
				// paint-only activity animations cannot update projected text.
				for _, turn := range cv.Turns {
					if (turn.Reply.Status == conversation.Running || turn.Reply.Status == conversation.Stopping) && !turn.Sent.IsZero() {
						next := time.Second - turn.Reply.Elapsed%time.Second
						if turn.Reply.Phase == conversation.Compacting && turn.Reply.PhaseElapsed > 0 {
							next = min(next, time.Second-turn.Reply.PhaseElapsed%time.Second)
						}
						c.After(next)
					}
				}
			}
			r.App.Messages.SyncSearch(cv.ID, state.Snapshot.Page.Revision)
			r.settleTurnRetry(cv.ID, comp, snapshot.Turns, state.Snapshot.Page)
		}
	} else {
		r.projectOutgoing(cv, protocol.ConversationPage{})
	}
	cv.Session.ResolveImage = r.resolveConversationImage
	cv.Session.ResolveViewerImage = r.resolveConversationViewerImage
	cv.Session.Embed = func(_, dest, _ string, image bool) (conversation.Embed, bool) { return r.chatEmbed(cv, dest, image) }
	if hadTurns != (len(cv.Turns) > 0) {
		// The header above the transcript uses the presence of its rows.
		c.Invalidate()
	}
	r.projectControls(c, cv, comp)
	if cv.Session.Diagnostic != nil {
		cv.Session.Diagnostic.Title = i18n.T(c, cv.Session.Diagnostic.Title)
	}
	if !r.composerInitialized[cv.ID] {
		if s, ok := r.session(cv.ID); ok {
			r.applyComposer(comp, s.Config)
			r.composerRevisions[cv.ID] = s.Revision
		} else {
			r.initializeDraftComposer(comp)
		}
		r.composerInitialized[cv.ID] = true
	}
	if comp.Model == "" {
		r.defaultComposer(comp)
	}
	if s, ok := r.session(cv.ID); ok && s.Revision != r.composerRevisions[cv.ID] && !r.composerSaving[cv.ID] {
		if key := r.composerKey(comp); key == r.composerObserved[cv.ID] {
			r.applyComposer(comp, s.Config)
			r.composerObserved[cv.ID] = r.composerKey(comp)
		}
		r.composerRevisions[cv.ID] = s.Revision
	}
	ctx := &composer.Context{Models: r.models(), Interactions: cv.Waiting, Queue: cv.Queue, Readonly: !r.controller.Connected(), Repository: composer.Repository{Kind: composer.Directory}, Activity: composer.Activity{Connected: r.controller.Connected(), Unsynced: unsynced}}
	if !r.controller.Connected() {
		cv.Session.Connection = conversation.Offline
		if r.controller.Err() == nil || faultCode(r.controller.Err()) == protocol.ErrorCodeUnavailable || faultCode(r.controller.Err()) == protocol.ErrorCodeBusy {
			cv.Session.Connection = conversation.Reconnecting
		}
		cv.Offline = true
		markOfflineConversation(cv)
		// The Node can disconnect before the conversation observation changes.
		if idErr == nil {
			saved := r.conversationPresentations[cv]
			saved.offline = true
			r.saveConversationPresentation(cv, saved)
		}
	}
	r.projectComposerModels(c, cv, comp, ctx)
	if unsentDraft(cv) {
		ctx.Readonly = false
		ctx.SendBlocked = !r.controller.Connected()
		ctx.LocationLocked = r.draftSubmitted(cv)
	}
	ctx.Hosts = r.composerHosts(c)
	ctx.Projects = r.composerProjects()
	if s, ok := r.session(cv.ID); ok {
		ctx.LocationLocked = true
		comp.Project = ""
		if s.Project != nil {
			comp.Project = s.Project.String()
		}
		ctx.Readonly = ctx.Readonly || delegated(s)
		for _, w := range snapshot.Worktrees {
			if w.ID == s.Worktree {
				comp.Worktree = w.Path
			}
		}
	}
	if len(cv.Turns) > 0 {
		last := cv.Turns[len(cv.Turns)-1]
		if len(last.Images) == 0 && len(last.Files) == 0 {
			ctx.Last = last.User
		}
		for i := len(cv.Turns) - 1; i >= 0; i-- {
			if cv.Turns[i].Reply.Status.Active() {
				for _, b := range cv.Turns[i].Reply.Blocks {
					if b.Kind == conversation.ProgressBlock {
						ctx.Activity.Progress = b.Steps
					}
				}
				break
			}
		}
	}
	ctx.References, ctx.Commands = r.catalogReferences(c, cv, comp)
	comp.Running = cv.Running()
	// Keep model controls durable without treating UI-only location edits as config.
	selectedEffort := ""
	if chosen := comp.Chosen(ctx.Models); chosen.Reasoning {
		selectedEffort = comp.EffortName(nil, chosen)
	}
	teamUnsettled := r.teams != nil && r.teams.composers[cv.ID] != nil && (r.teams.composers[cv.ID].saving || r.teams.composers[cv.ID].pending != nil)
	config, _, err := r.composerConfig(comp, composer.Message{Model: comp.Model, Mode: comp.Mode, Permission: comp.Permission, Features: comp.Features, Effort: selectedEffort})
	// A new turn needs a configured model and no creation under way; the
	// running reply's guidance uses that turn's.
	ctx.SendBlocked = ctx.SendBlocked || !comp.Running && (err != nil || r.conversationCreating[cv.ID] || r.composerOutgoing[cv.ID] != nil)
	ctx.SelectModel = func() {
		r.openNodeSettings(settings.Providers)
	}
	if err == nil && !ctx.Readonly && !teamUnsettled {
		encoded, _ := json.Marshal(config)
		key := string(encoded)
		if before, seen := r.composerObserved[cv.ID]; seen && before != key && !r.composerSaving[cv.ID] {
			if s, exists := r.session(cv.ID); exists {
				r.saveComposerConfig(s, config, nil)
			}
		}
		if !r.composerSaving[cv.ID] || r.composerObserved[cv.ID] == "" {
			r.composerObserved[cv.ID] = key
		}
	}
	r.projectWorkspaceComposer(cv, comp, ctx)
	r.projectComposerFiles(cv, comp, ctx)
	r.projectConnectionReferences(cv, comp, ctx)
	r.projectCommands(cv.ID, comp, &ctx.Activity)
	r.projectTeamComposer(cv, comp, ctx, c.Locale())
	r.projectChildComposer(cv, comp, ctx, c.Locale())
	r.projectInteractionStates(cv, ctx, c.Locale())
	r.projectPluginComposer(c, cv, comp, ctx)
	r.projectContextMeter(cv, comp, config)
	r.bindSkillReferences(cv, comp)
	actions := &conversationActions{r: r, c: c, cv: cv, comp: comp}
	r.projectComposerModes(c, cv, comp, ctx)
	r.projectComposerQueue(actions, ctx)
	ctx.Compact = actions.compact
	ctx.Stale = func(reference composer.Reference) bool { return staleReference(ctx, reference) }
	r.bindComposerAttachments(actions)
	return ctx, actions
}

// composerFamilies are the provider families the model picker groups by.
var composerFamilies = []composer.Family{{ID: "anthropic", Label: "provider_anthropic", Icon: icons.Brand("anthropic")}, {ID: "openai", Label: "provider_openai", Icon: icons.Brand("openai")}, {ID: "gemini", Label: "provider_gemini", Icon: icons.Brand("gemini")}, {ID: "other", Label: "provider_other", Icon: icons.Settings2}}

func modeChoice(mode protocol.WorkMode) composer.Mode {
	switch mode {
	case "plan":
		return composer.Plan
	case "flagship":
		return composer.Flagship
	}
	return composer.Code
}
func permissionChoice(p protocol.Permission) composer.Permission {
	switch p {
	case "project":
		return composer.ProjectAccess
	case "full":
		return composer.FullAccess
	}
	return composer.AskFirst
}
func (r *Runtime) applyComposer(c *composer.State, config protocol.SessionConfig) {
	r.composerBases[c] = config
	c.Model = modelKey(config.Provider, config.Model)
	c.Mode = modeChoice(config.Mode)
	c.Permission = permissionChoice(config.Permission)
	c.Features = map[string]string{}
	for k, v := range config.Features {
		c.Features[k] = v
	}
	for _, m := range r.models() {
		if m.ID == c.Model {
			if i := slices.Index(m.Efforts, effortName(config.Effort)); i >= 0 {
				c.Effort = float64(i)
			}
		}
	}
}

func (r *Runtime) composerConfig(comp *composer.State, m composer.Message) (protocol.SessionConfig, uint64, error) {
	p, model, ok := r.composerModel(m.Model)
	if !ok {
		return protocol.SessionConfig{}, 0, &link.FaultError{Fault: protocol.Fault{Code: protocol.ErrorCodeNotConfigured, Message: "Select a configured model"}}
	}
	config := protocol.DefaultSessionConfig()
	if base, ok := r.composerBases[comp]; ok {
		config = base
	}
	config.Provider = p.ID
	config.Model = model.ID
	config.Credential = p.Credential
	config.Effort = parseEffort(m.Effort)
	config.Mode = []protocol.WorkMode{"code", "plan", "flagship"}[min(max(int(m.Mode), 0), 2)]
	config.Permission = []protocol.Permission{"ask", "project", "full"}[min(max(int(m.Permission), 0), 2)]
	if m.Effort == "" || m.Effort == "composer_effort_unavailable" {
		config.Effort = model.DefaultEffort
	}
	config.Features = map[string]string{}
	if model.Capabilities != nil {
		for _, f := range model.Capabilities.Features {
			if value, ok := m.Features[f.ID]; ok && slices.ContainsFunc(f.Options, func(o protocol.ConversationFeaturesFeatureOption) bool { return o.ID == value }) {
				config.Features[f.ID] = value
			}
		}
	}
	return config, p.Revision, nil
}

func (r *Runtime) rememberConfig(config protocol.SessionConfig, team string) {
	source := r.controller.Target()
	r.localRuntime().Preferences.Data.Composer = &preferences.Composer{Mode: config.Mode, Permission: config.Permission, Team: team, Model: &preferences.Model{Source: source, Provider: config.Provider, Model: config.Model, Effort: config.Effort, Features: config.Features}}
}

func (r *Runtime) references(cv *viewmodel.Conversation, comp *composer.State) ([]composer.Reference, []composer.Reference) {
	snapshot := r.snapshot()
	locations := []composer.Reference{{ID: "host", Kind: composer.Location, Label: "local_host_name", Key: true, Icon: icons.Monitor}}
	for _, project := range snapshot.Projects {
		if project.ID.String() == comp.Project {
			path := comp.Worktree
			if path == "" {
				for _, worktree := range snapshot.Worktrees {
					if worktree.Project != nil && *worktree.Project == project.ID && (path == "" || worktree.Main) {
						path = worktree.Path
					}
				}
			}
			locations = append(locations, composer.Reference{ID: "project", Kind: composer.Location, Label: project.Name, Icon: icons.Folder}, composer.Reference{ID: "worktree", Kind: composer.Location, Label: path, Icon: icons.Folder})
			break
		}
	}
	refs := []composer.Reference{{ID: "context", Kind: composer.Page, Label: "reference_context", Key: true, Group: "reference_group_context", Icon: icons.Folder, Children: locations}}
	var sessions []composer.Reference
	for _, s := range snapshot.Sessions {
		if s.ID.String() == cv.ID || s.Archived {
			continue
		}
		same := s.Project == nil && comp.Project == "" || s.Project != nil && s.Project.String() == comp.Project
		if same {
			sessions = append(sessions, composer.Reference{ID: s.ID.String(), Kind: composer.Session, Label: s.Activity.Title, Icon: icons.Chat})
		}
	}
	if len(sessions) > 0 {
		refs = append(refs, composer.Reference{ID: "sessions", Kind: composer.Page, Label: "reference_sessions", Key: true, Group: "reference_group_context", Icon: icons.Chat, Children: sessions})
	}
	var models, efforts []composer.Reference
	offered := r.offeredModels(cv.ID, r.models())
	for _, m := range offered {
		models = append(models, composer.Reference{ID: m.ID, Kind: composer.ModelCommand, Label: m.Name, Insert: m.ID, Description: m.Provider, Icon: icons.Bot})
	}
	chosen := comp.Chosen(offered)
	for _, e := range chosen.Efforts {
		efforts = append(efforts, composer.Reference{ID: e, Kind: composer.EffortCommand, Label: e, Insert: e, Icon: icons.Bot})
	}
	var commands []composer.Reference
	if len(models) > 0 {
		commands = append(commands, composer.Reference{ID: "model", Kind: composer.Page, Label: "/model", Icon: icons.Bot, Children: models, Group: "composer_group_session"})
	}
	if len(efforts) > 0 {
		commands = append(commands, composer.Reference{ID: "reasoning", Kind: composer.Page, Label: "/reasoning", Icon: icons.Bot, Children: efforts, Group: "composer_group_session"})
	}
	if _, ok := r.session(cv.ID); ok {
		commands = append(commands, composer.Reference{ID: "compact", Kind: composer.Compact, Label: "/compact", Icon: icons.Minimize, Group: "composer_group_session"})
	}
	for _, mode := range []string{"code", "plan", "flagship"} {
		commands = append(commands, composer.Reference{ID: "mode/" + mode, Kind: composer.ModeCommand, Label: "/" + mode, Insert: "composer_mode_" + mode, Icon: icons.Bot, Group: "composer_group_modes"})
	}
	for _, prompt := range []string{"review", "test", "explain"} {
		commands = append(commands, composer.Reference{ID: prompt, Kind: composer.Prompt, Label: "/" + prompt, Insert: "composer_prompt_" + prompt, Icon: icons.Writing, Group: "composer_group_prompts"})
	}
	refs = append(refs, r.teamReferences(cv, comp)...)
	return refs, commands
}

func conversationInput(m composer.Message) protocol.ConversationInput {
	input := protocol.DefaultConversationInput()
	input.Text = m.Text
	for _, attachment := range m.Attachments {
		if id, err := protocol.ParseAttachmentID(attachment.ID); err == nil {
			input.Attachments = append(input.Attachments, id)
		}
	}
	for _, ref := range m.References {
		target := protocol.ConversationReferenceTarget{}
		switch ref.Kind {
		case composer.File:
			target = protocol.ConversationReferenceTarget{Kind: "file", Data: ref.ID}
		case composer.Folder:
			target = protocol.ConversationReferenceTarget{Kind: "directory", Data: ref.ID}
		case composer.Session:
			if id, err := protocol.ParseSessionID(ref.ID); err == nil {
				target = protocol.ConversationReferenceTarget{Kind: "session", Data: id}
			}
		case composer.Agent:
			var role protocol.RoleReference
			if json.Unmarshal([]byte(ref.Insert), &role) == nil && role.ID.String() == ref.ID {
				target = protocol.ConversationReferenceTarget{Kind: "agent", Data: role}
			}
		case composer.Skill:
			if packageName, name, ok := strings.Cut(ref.ID, ":"); ok && packageName != "" && name != "" {
				target = protocol.ConversationReferenceTarget{Kind: "skill", Data: protocol.ConversationReferenceTargetSkillPayload{Package: packageName, Name: name}}
			}
		case composer.Plugin:
			target = protocol.ConversationReferenceTarget{Kind: "plugin", Data: ref.ID}
		case composer.Location:
			if ref.ID == "host" || ref.ID == "project" || ref.ID == "worktree" {
				target.Kind = ref.ID
			} else if ssh, ok := sshReferenceTarget(ref.ID); ok {
				target = ssh
			}
		case composer.Database:
			if database, ok := databaseReferenceTarget(ref.ID); ok {
				target = database
			}
		case composer.Element:
			// One reference per file behind the chip, named as the chip, so
			// they read as one selection.
			for _, attachment := range m.Attachments {
				if id, err := protocol.ParseAttachmentID(attachment.ID); err == nil && attachment.Chip == ref.ID {
					input.References = append(input.References, protocol.ConversationReference{Target: protocol.ConversationReferenceTarget{Kind: "attachment", Data: id}, Label: ref.Label})
				}
			}
		}
		if target.Kind != "" {
			input.References = append(input.References, protocol.ConversationReference{Target: target, Label: ref.Label})
		}
	}
	return input
}

func composerReference(ref protocol.ConversationReference) (composer.Reference, bool) {
	value := composer.Reference{Label: ref.Label}
	switch ref.Target.Kind {
	case "file", "directory":
		path, ok := detail[string](ref.Target.Data)
		if !ok {
			return value, false
		}
		value.ID, value.Label, value.Kind, value.Icon = path, ref.Label, composer.File, icons.File
		if ref.Target.Kind == "directory" {
			value.Kind, value.Icon = composer.Folder, icons.Folder
		}
	case "session":
		id, ok := detail[protocol.SessionID](ref.Target.Data)
		if !ok {
			return value, false
		}
		value.ID, value.Kind, value.Icon = id.String(), composer.Session, icons.Chat
	case "agent":
		role, ok := detail[protocol.RoleReference](ref.Target.Data)
		if !ok {
			return value, false
		}
		encoded, err := json.Marshal(role)
		if err != nil {
			return value, false
		}
		value.ID, value.Kind, value.Icon, value.Insert = role.ID.String(), composer.Agent, icons.Bot, string(encoded)
	case "skill":
		skill, ok := detail[protocol.ConversationReferenceTargetSkillPayload](ref.Target.Data)
		if !ok {
			return value, false
		}
		value.ID, value.Kind, value.Icon = skill.Package+":"+skill.Name, composer.Skill, icons.Bot
	case "plugin":
		name, ok := detail[string](ref.Target.Data)
		if !ok {
			return value, false
		}
		value.ID, value.Kind, value.Icon = name, composer.Plugin, icons.Puzzle
	case "ssh":
		id, ok := detail[protocol.SshID](ref.Target.Data)
		if !ok {
			return value, false
		}
		value.ID, value.Kind, value.Icon = sshReferencePrefix+id.String(), composer.Location, icons.Network
	case "database":
		payload, ok := detail[protocol.ConversationReferenceTargetDatabasePayload](ref.Target.Data)
		if !ok {
			return value, false
		}
		value.ID, value.Kind, value.Icon = databaseReferenceID(payload), composer.Database, icons.HardDrive
	case "host", "project", "worktree":
		value.ID, value.Kind, value.Icon = ref.Target.Kind, composer.Location, icons.Folder
		if ref.Target.Kind == "host" {
			value.Icon = icons.Monitor
		}
	default:
		return value, false
	}
	return value, true
}

type conversationActions struct {
	r       *Runtime
	c       *ui.Context
	cv      *viewmodel.Conversation
	comp    *composer.State
	created func(protocol.Session)
	// restore is the submission a failed send offers to put back.
	restore      *composer.Message
	restoreDraft string // the composer's fingerprint when it was captured
	// settled hears whether a send from another surface was admitted.
	settled func(bool)
}

func (x *conversationActions) owned() bool {
	return x.r.runtimeFor(x.cv.Node) == x.r
}

func (x *conversationActions) unavailable()             { kit.Toast(x.c, kit.Failed, i18n.T(x.c, "unavailable")) }
func (x *conversationActions) Send(m composer.Message)  { x.deliver(m, false, false) }
func (x *conversationActions) Guide(m composer.Message) { x.deliver(m, true, false) }
func (x *conversationActions) Queue(m composer.Message) { x.deliver(m, false, true) }

func (x *conversationActions) deliver(m composer.Message, guide, queue bool) {
	x.countSubmit(m, guide, queue)
	// The view clears before dispatch; the live owner retains the captured draft.
	x.comp.Draft = m.Text
	x.comp.References = slices.Clone(m.References)
	x.comp.Attachments = slices.Clone(m.Attachments)
	x.submit(m, guide, queue, composerFingerprint(x.comp))
}

// submit sends the captured message, keeping edits made while it is pending.
func (x *conversationActions) submit(m composer.Message, guide, queue bool, draft string) {
	failed := func() {
		if x.settled != nil {
			x.settled(false)
		}
	}
	if x.readonly() {
		x.unavailable()
		failed()
		return
	}
	draftSession := unsentDraft(x.cv)
	session, exists := x.r.session(x.cv.ID)
	if x.settled != nil && (draftSession || !exists) {
		x.unavailable()
		failed()
		return
	}
	if draftSession {
		if pending := x.r.conversationCreations[x.cv.ID]; pending != nil {
			pending.resume()
			return
		}
		if x.r.conversationCreating[x.cv.ID] {
			return
		}
	}
	for _, attachment := range m.Attachments {
		if attachment.Status != composer.Ready || attachment.ID == "" {
			x.unavailable()
			failed()
			return
		}
	}
	sender := *x
	sender.restore, sender.restoreDraft = &m, draft
	x = &sender
	var pending *conversationOutgoing
	if !guide && !queue {
		pending = x.stageOutgoing(m)
	}
	if exists && session.Profile != nil && session.Profile.Source != x.r.controller.Target() {
		x.r.discardCapturedOutgoing(x.cv.ID, pending)
		x.r.reportCreation(x.c, &link.FaultError{Fault: protocol.Fault{Code: protocol.ErrorCodeNotConfigured, Message: "legacy remote model sessions cannot start new turns on this Node"}})
		failed()
		return
	}
	config, _, err := x.r.composerConfig(x.comp, m)
	if err != nil {
		x.r.discardCapturedOutgoing(x.cv.ID, pending)
		if exists {
			x.r.report(x.c, err, "session_changed")
		} else {
			x.r.reportCreation(x.c, err)
		}
		failed()
		return
	}
	if draftSession {
		if config.Assistant == nil {
			x.r.rememberConfig(config, x.comp.Team)
		}
		x.start(m, config, !guide && !queue)
		return
	}
	finish := func(output protocol.Output, err error) {
		// An unknown turn may already have landed. Only a definite result
		// settles the originating form, including later request reconciliation.
		if x.settled != nil && faultCode(err) != protocol.ErrorCodeOutcomeUnknown {
			x.settled(err == nil)
		}
		x.finishSubmission(output, false, err == nil)
	}
	if exists {
		submit := func(session protocol.Session) {
			input := conversationInput(m)
			if guide {
				if run, ok := x.activeRun(); ok && acceptsGuidance(run) {
					x.submitTurn(protocol.Command{Kind: "guide_turn", Data: protocol.CommandGuideTurnPayload{Recipient: run.Turn, ExpectedRevision: session.Revision, Message: input}}, pending, func(output protocol.Output, err error) {
						finish(output, err)
						if queued, admitted := detail[protocol.QueuedTurn](output.Data); err == nil && admitted && queued.Session == session.ID && queued.ID != (protocol.TurnID{}) {
							x.guided(queued.ID, run.Turn)
						}
					})
				} else {
					x.unavailable()
					failed()
				}
				return
			}
			kind := "submit_turn"
			var data any = protocol.CommandSubmitTurnPayload{Session: session.ID, ExpectedRevision: session.Revision, Message: input}
			if queue {
				kind = "queue_turn"
				data = protocol.CommandQueueTurnPayload{Session: session.ID, ExpectedRevision: session.Revision, Message: input}
			}
			x.submitTurn(protocol.Command{Kind: kind, Data: data}, pending, finish)
		}
		x.r.withComposerConfig(session, config, func(session protocol.Session) {
			x.r.withComposerTeam(session, x.comp, submit, failed)
		}, failed)
		return
	}
	x.r.discardCapturedOutgoing(x.cv.ID, pending)
	x.unavailable()
	failed()
}

func (x *conversationActions) finishSubmission(output protocol.Output, draftSession, ok bool) {
	if !ok {
		return
	}
	files := x.restore.Attachments
	x.r.attachmentsCommitted(files)
	if composerFingerprint(x.comp) == x.restoreDraft {
		x.comp.Draft = ""
		x.comp.References = nil
		x.comp.Attachments = nil
		if draftSession {
			x.r.App.Messages.List(x.cv.ID).ScrollToEnd()
		}
	} else if draftSession {
		x.comp.Attachments = slices.DeleteFunc(x.comp.Attachments, func(a *composer.Attachment) bool {
			return slices.Contains(files, a)
		})
	}
	x.r.pluginCommandIntent(composerPluginScope(x.r, x.cv, x.comp), x.cv.ID, output)
}

func (x *conversationActions) activeRun() (protocol.ConversationRun, bool) {
	id, err := protocol.ParseSessionID(x.cv.ID)
	if err != nil {
		return protocol.ConversationRun{}, false
	}
	if state, ok := x.r.controller.Conversation(id); ok {
		return activeConversationRun(state.Snapshot.Page)
	}
	return protocol.ConversationRun{}, false
}

func activeConversationRun(page protocol.ConversationPage) (protocol.ConversationRun, bool) {
	for i := len(page.Runs) - 1; i >= 0; i-- {
		run := page.Runs[i]
		if run.Status == protocol.ConversationStatusRunning || run.Status == protocol.ConversationStatusStopping {
			return run, true
		}
	}
	return protocol.ConversationRun{}, false
}

func acceptsGuidance(run protocol.ConversationRun) bool {
	return run.Kind == protocol.ConversationRunKindTask && run.Status == protocol.ConversationStatusRunning
}

func (x *conversationActions) Stop() {
	if x.readonly() {
		x.unavailable()
		return
	}
	// Stop belongs to the reply that exposed the control. The Node can
	// cancel it while it is still queued or acknowledge that it already ended.
	if x.cv.Running() {
		if turn, err := protocol.ParseTurnID(x.cv.Turns[len(x.cv.Turns)-1].ID); err == nil {
			x.r.command(protocol.Command{Kind: "stop_turn", Data: protocol.CommandStopTurnPayload{Turn: turn}}, "session_changed", nil)
		}
	}
}
func (x *conversationActions) LoadOlder() {
	if !x.owned() {
		return
	}
	if id, err := protocol.ParseSessionID(x.cv.ID); err == nil {
		x.r.async(func() { _ = x.r.controller.LoadOlderConversation(x.r.ctx, id) })
	}
}
func (x *conversationActions) Quote(text string) {
	if x.comp.Draft != "" {
		x.comp.Draft += "\n\n"
	}
	x.comp.Draft += text
}
func (x *conversationActions) Read(turn, text string) {
	if x.owned() && x.r.readAloud != nil {
		x.r.readAloud(x.cv.ID, turn, text)
	} else {
		x.unavailable()
	}
}
func (x *conversationActions) Export(turn string, format conversation.ExportFormat) {
	if x.owned() && x.r.exportConversation != nil {
		x.r.exportConversation(x.cv.ID, turn, format)
	} else {
		x.unavailable()
	}
}
func (x *conversationActions) OpenChild(id string) {
	if x.owned() {
		x.r.openLiveChild(x.cv, id)
	}
}
func (x *conversationActions) OpenFile(file conversation.File) {
	if x.owned() {
		x.r.openAttachmentFile(x, file)
	}
}
func (x *conversationActions) FileAction(file conversation.File, action conversation.FileAction) {
	if x.owned() {
		x.r.attachmentFileAction(x, file, action)
	}
}
func (x *conversationActions) ImageAction(action kit.ImageAction, image conversation.Image) {
	if x.owned() {
		x.r.attachmentImageAction(x, action, image)
	}
}
func (x *conversationActions) Undo(turn, path string) {
	if !x.readonly() {
		x.r.undoWorkspaceTurn(x, turn, path)
	}
}
func (x *conversationActions) Review(turn, path string) {
	if x.owned() {
		x.r.reviewWorkspaceTurn(x, turn, path)
	}
}
func (x *conversationActions) Worktree(turn string) {
	if !x.readonly() {
		x.r.forkWorkspaceTurn(x, turn)
	}
}
func (x *conversationActions) CreateWorktree() {
	if !x.readonly() {
		x.r.createComposerWorktree(x)
	}
}
func (x *conversationActions) AddProject() {
	if !x.readonly() {
		x.r.App.Sidebar.OpenEditor(nil)
	}
}
func (x *conversationActions) ManageTeams() {
	if !x.readonly() {
		x.r.openNodeSettings(settings.Teams)
	}
}
func (x *conversationActions) OpenChange(path string) {
	if x.owned() {
		x.r.openWorkspaceChange(x, path)
	}
}
func (x *conversationActions) Badge(id, button string) {
	if button == "" && x.r.invokePluginStatus(composerPluginScope(x.r, x.cv, x.comp), id) {
		return
	}
	x.unavailable()
}
func (x *conversationActions) StopCommand(id string) {
	if x.owned() {
		x.stopWorkspaceCommand(id)
	}
}
func (x *conversationActions) OpenService(port int) {
	if x.owned() {
		x.forwardCommandService(port)
	}
}
func (x *conversationActions) Upload(a *composer.Attachment) {
	if !x.readonly() && !x.attachmentsLocked() {
		x.r.uploadAttachment(x, a)
	}
}
func (x *conversationActions) Discard(a *composer.Attachment) {
	if !x.readonly() && !x.attachmentsLocked() {
		x.r.discardAttachment(x, a)
	}
}

func (x *conversationActions) attachmentsLocked() bool {
	return unsentDraft(x.cv) && x.r.draftSubmitted(x.cv) && x.r.guardedCreation(x.cv.ID) == nil
}

var _ shell.ConversationActions = (*conversationActions)(nil)
