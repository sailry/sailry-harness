package live

import (
	"slices"
	"strings"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/desktop/composer"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// conversationModes are the conversation modes the Host's packages declare,
// with each one's package description for the mode menu.
func (r *Runtime) conversationModes() ([]client.ConversationMode, map[string]string) {
	modes, descriptions := []client.ConversationMode{}, map[string]string{}
	for _, info := range r.localRuntime().pluginViewCatalog(r) {
		if info.Extension == nil || info.Extension.Scope != protocol.PluginScopeHost {
			continue
		}
		for _, declaration := range info.Extension.ConversationModes {
			mode := client.ConversationMode{Package: packageReference(info.Summary), Declaration: declaration, Enabled: pluginNativePackageAvailable(info)}
			modes = append(modes, mode)
			description := stringValue(info.Summary.Description)
			if info.Extension.Description != nil {
				description = pluginNavigation(*info.Extension.Description, r.packageLocale())
			}
			descriptions[mode.Key()] = description
		}
	}
	return modes, descriptions
}

// projectComposerModes offers the packages' conversation modes beside the
// built-in ones. A mode is an option of its package in the session's
// configuration over a built-in base mode: choosing one, or a built-in mode
// again, rewrites the composer's base configuration, which the composer
// saves as any other change of it.
func (r *Runtime) projectComposerModes(c *ui.Context, cv *viewmodel.Conversation, comp *composer.State, ctx *composer.Context) {
	modes, descriptions := r.conversationModes()
	if len(modes) == 0 {
		return
	}
	config, configured := r.composerBases[comp]
	if !configured {
		config = protocol.DefaultSessionConfig()
	}
	config.Mode = []protocol.WorkMode{"code", "plan", "flagship"}[min(max(int(comp.Mode), 0), 2)]
	view := client.ProjectModes(config, modes)
	for _, mode := range modes {
		key := mode.Key()
		if !slices.Contains(view.Choices, key) {
			continue
		}
		row := composer.PluginMode{Key: key, Label: pluginLabel(mode.Declaration.Display, c.Locale()), Description: descriptions[key],
			Disabled: !mode.Enabled, Selected: view.Selected != nil && *view.Selected == key}
		if mode.Declaration.Display.Icon != nil {
			row.Icon = r.pluginNavigationIcon(c, mode.Declaration.Display.Icon)
		}
		ctx.PluginModes = append(ctx.PluginModes, row)
	}
	ctx.SelectMode = func(key string) {
		if ctx.Readonly {
			return
		}
		selected := client.SelectMode(config, modes, key)
		if selected == nil {
			// Shift+Tab chooses between frames: the toast waits for one.
			r.enqueue(func(c *ui.Context) { kit.Toast(c, kit.Failed, i18n.T(c, "unavailable")) })
			return
		}
		r.composerBases[comp] = *selected
		comp.Mode = modeChoice(selected.Mode)
	}
}

func (r *Runtime) composerProjects() []composer.Project {
	var projects []composer.Project
	for _, p := range r.localRuntime().App.Data.Projects {
		v := composer.Project{ID: p.ID, Name: p.Name, Host: p.Host}
		for _, w := range p.Worktrees {
			v.Worktrees = append(v.Worktrees, composer.Worktree{Branch: w.Path, Main: w.Main})
		}
		projects = append(projects, v)
	}
	return projects
}

// staleReference reports a chosen command or skill the picker listed on a
// page entered earlier and the conversation offers no more: its package was
// disabled, removed or reconfigured meanwhile.
func staleReference(ctx *composer.Context, reference composer.Reference) bool {
	if reference.Kind != composer.Skill && reference.Source != "plugin-contribution" {
		return false
	}
	var offered func([]composer.Reference) bool
	offered = func(rows []composer.Reference) bool {
		for _, row := range rows {
			if row.Kind == reference.Kind && row.Source == reference.Source && row.ID == reference.ID || offered(row.Children) {
				return true
			}
		}
		return false
	}
	return !offered(ctx.Commands) && !offered(ctx.References)
}

// projectComposerQueue marks the queue's rows the running turn is taking as
// guidance, and adds the message still on its way to the Host. The rows say
// nothing about why they wait and held background results are not rows
// (docs/removed-ui.md): what the user must know is a toast, once.
func (r *Runtime) projectComposerQueue(x *conversationActions, ctx *composer.Context) {
	if run, active := x.activeRun(); active {
		for i := range ctx.Queue {
			if item, ok := x.queued(ctx.Queue[i].ID); ok && item.Guidance != nil && *item.Guidance == run.Turn {
				ctx.Queue[i].Joining = true
			}
		}
	}
	x.settleGuidance()
	x.reportHalt()
	x.reportUsage()
	if outgoing, sending := r.composerOutgoing[x.cv.ID]; sending {
		if outgoing.Text == "" {
			outgoing.Text = i18n.T(x.c, "chat_attachment_count", "count", len(outgoing.Attachments))
		}
		ctx.Outgoing = &outgoing
	}
}

// outgoing shows m in the queue tray until the Host answers its submission.
func (x *conversationActions) outgoing(m composer.Message, guide bool) {
	if x.r.composerOutgoing == nil {
		x.r.composerOutgoing = map[string]composer.Queued{}
	}
	// A message of files alone is named by their count, in the frame.
	x.r.composerOutgoing[x.cv.ID] = composer.Queued{Text: strings.TrimSpace(m.Text), Guided: guide, Attachments: m.Attachments}
}

// queueCommand runs a command of the queue tray. A conflict says the queue
// changed; any other failure offers the same command again, as the row it
// acted on is still there. Guidance the running turn could not take, because
// the message's settings differ from the turn's, stays queued: the user is
// told it will run separately.
func (x *conversationActions) queueCommand(command protocol.Command) {
	x.r.execute(command, func(c *ui.Context, output protocol.Output, err error) {
		if err != nil {
			switch faultCode(err) {
			case protocol.ErrorCodeConflict, protocol.ErrorCodeRevisionConflict:
				x.r.report(c, err, "session_changed")
			case protocol.ErrorCodeOutcomeUnknown:
				kit.Retry(c, i18n.T(c, "chat_action_unknown"), func() { x.queueCommand(command) })
			default:
				kit.Retry(c, i18n.T(c, "chat_action_failed"), func() { x.queueCommand(command) })
			}
			return
		}
		guide, guiding := command.Data.(protocol.CommandGuideQueuedTurnPayload)
		update, valid := detail[protocol.ConversationQueueUpdate](output.Data)
		if guiding && valid && guidanceSeparate(update.Queue, guide.Turn) {
			kit.Notify(c, "", i18n.T(c, "queue_guidance_separate"))
		}
	})
}

// sentGuidance is guidance sent from the composer: the turn it was sent to,
// and whether the queue has listed it as waiting for that turn.
type sentGuidance struct {
	recipient protocol.TurnID
	guiding   bool
}

// guidanceFate is what became of guidance sent from the composer.
type guidanceFate int

const (
	guidanceWaiting   guidanceFate = iota // not listed yet
	guidanceGuiding                       // waiting for its recipient
	guidanceSettled                       // joined, started, held or changed by the user
	guidanceSeparated                     // other settings: queued as its own turn
	guidanceOutlived                      // its recipient ended first: queued as its own turn
)

// guidanceOutcome reads the fate of the guidance turn from the page. Guidance
// that is an ordinary item again while its recipient still runs was edited or
// paused by the user, which needs no report.
func guidanceOutcome(page protocol.ConversationPage, turn protocol.TurnID, sent sentGuidance) guidanceFate {
	run := func(id protocol.TurnID) (protocol.ConversationRun, bool) {
		index := slices.IndexFunc(page.Runs, func(run protocol.ConversationRun) bool { return run.Turn == id })
		if index < 0 {
			return protocol.ConversationRun{}, false
		}
		return page.Runs[index], true
	}
	index := slices.IndexFunc(page.Queue.Items, func(item protocol.ConversationPending) bool { return item.Turn == turn })
	if index < 0 {
		if own, listed := run(turn); sent.guiding || listed && (own.Joined != nil || own.Status != "queued") {
			return guidanceSettled
		}
		return guidanceWaiting
	}
	item := page.Queue.Items[index]
	if item.Guidance != nil {
		return guidanceGuiding
	}
	if recipient, listed := run(sent.recipient); !listed || recipient.Status != "running" && recipient.Status != "stopping" {
		return guidanceOutlived
	}
	if !item.Ready || sent.guiding {
		return guidanceSettled
	}
	return guidanceSeparated
}

// guided remembers guidance the Node admitted until its fate is known.
func (x *conversationActions) guided(turn, recipient protocol.TurnID) {
	if x.r.guidanceSent == nil {
		x.r.guidanceSent = map[string]map[protocol.TurnID]sentGuidance{}
	}
	if x.r.guidanceSent[x.cv.ID] == nil {
		x.r.guidanceSent[x.cv.ID] = map[protocol.TurnID]sentGuidance{}
	}
	x.r.guidanceSent[x.cv.ID][turn] = sentGuidance{recipient: recipient}
}

// settleGuidance says once that guidance sent from the composer runs as its
// own turn: its settings differ from the recipient's, or the recipient ended
// before it could join.
func (x *conversationActions) settleGuidance() {
	sent := x.r.guidanceSent[x.cv.ID]
	if len(sent) == 0 {
		return
	}
	_, state, ok := x.state()
	if !ok {
		return
	}
	for turn, guidance := range sent {
		switch guidanceOutcome(state.Page, turn, guidance) {
		case guidanceGuiding:
			guidance.guiding = true
			sent[turn] = guidance
		case guidanceSettled:
			delete(sent, turn)
		case guidanceSeparated:
			delete(sent, turn)
			kit.Notify(x.c, "", i18n.T(x.c, "queue_guidance_separate"))
		case guidanceOutlived:
			delete(sent, turn)
			kit.Notify(x.c, "", i18n.T(x.c, "queue_guidance_outlived"))
		}
	}
}

// queueHalt is why the Node stopped sending a queue that still holds the
// user's input, when it was not the user who stopped it: "interrupted" after
// a turn lost its owner, "unrecorded" after a turn ended before its input
// reached history; "" otherwise. The user's own Stop or pause is no halt.
func queueHalt(page protocol.ConversationPage) protocol.ConversationQueuePause {
	if !page.Queue.Paused || page.Queue.PauseReason == nil ||
		!slices.ContainsFunc(page.Queue.Items, func(item protocol.ConversationPending) bool { return !item.Notice }) {
		return ""
	}
	switch reason := *page.Queue.PauseReason; reason {
	case protocol.ConversationQueuePauseInterrupted, protocol.ConversationQueuePauseUnrecorded:
		return reason
	}
	return ""
}

// haltBegan records the halt read for a conversation's queue and reports
// whether it is new to this window: it began since the last read, or the
// conversation is read for the first time and is halted. Only a shown
// conversation is read, so a halt that happened off screen, or before this
// window started, is said once when its conversation is opened and nothing
// is said at startup for the conversations that stay closed.
func (r *Runtime) haltBegan(id string, halt protocol.ConversationQueuePause) bool {
	seen := r.queueHalts[id]
	if r.queueHalts == nil {
		r.queueHalts = map[string]protocol.ConversationQueuePause{}
	}
	r.queueHalts[id] = halt
	return halt != seen && halt != ""
}

// usageFailureBegan records whether a conversation's statistics are failed
// and reports whether that began since the last read.
func (r *Runtime) usageFailureBegan(id string, failed bool) bool {
	seen := r.usageFailed[id]
	if r.usageFailed == nil {
		r.usageFailed = map[string]bool{}
	}
	r.usageFailed[id] = failed
	return failed && !seen
}

// reportUsage says once that the Node could not refresh the conversation's
// statistics (the `statistics_failed` change). The usage keeps its place and
// shows again with the next statistics.
func (x *conversationActions) reportUsage() {
	session, ok := x.r.session(x.cv.ID)
	if !ok || !x.owned() {
		return
	}
	state, ok := x.r.conversationState(session.ID)
	if ok && x.r.usageFailureBegan(x.cv.ID, state.StatisticsErr != nil) {
		kit.Toast(x.c, kit.Failed, i18n.T(x.c, "usage_read_failed"))
	}
}

// reportHalt says once that the queue stopped sending: a warning after an
// interruption, whose outcome is uncertain, and an error for a message that
// was not sent. The rows stay ordinary queued rows.
func (x *conversationActions) reportHalt() {
	session, state, ok := x.state()
	if !ok || state.Page.Session != session.ID {
		return // not read yet
	}
	halt := queueHalt(state.Page)
	if !x.r.haltBegan(x.cv.ID, halt) {
		return
	}
	if halt == protocol.ConversationQueuePauseInterrupted {
		kit.Toast(x.c, kit.Uncertain, i18n.T(x.c, "queue_halt_interrupted"))
	} else {
		kit.Toast(x.c, kit.Failed, i18n.T(x.c, "queue_halt_unsent"))
	}
}

// guidanceSeparate reports whether guidance the Node admitted as turn stays
// an ordinary queued item: its settings differ from the recipient's, so it
// runs as its own turn. The answer to the command says so once.
func guidanceSeparate(queue protocol.ConversationQueue, turn protocol.TurnID) bool {
	return slices.ContainsFunc(queue.Items, func(item protocol.ConversationPending) bool {
		return item.Turn == turn && item.Ready && item.Guidance == nil
	})
}

// nameModelSources says whose configuration each offered provider is where
// two sources meet. A forwarded session has its own private provider beside
// the Host's; a draft on another Host has this desktop's providers, which
// the session would be forwarded with, beside that Host's own.
func (r *Runtime) nameModelSources(c *ui.Context, id string, models []composer.Model) {
	session, exists := r.session(id)
	if exists && session.Profile == nil || !exists && r.parent == nil {
		return
	}
	host := ""
	for _, h := range r.localRuntime().App.Data.Hosts {
		if h.ID == r.controller.Target().String() {
			host = h.Name
		}
	}
	for i := range models {
		provider, _, _ := strings.Cut(models[i].ID, "/")
		switch {
		case exists && provider == session.Profile.Provider.ID.String():
			models[i].Provider = i18n.T(c, "chat_provider_session", "name", models[i].Provider)
		case exists:
			models[i].Provider = i18n.T(c, "chat_provider_node", "name", models[i].Provider)
		case slices.ContainsFunc(r.snapshot().Providers, func(p protocol.ConversationProvider) bool { return p.ID.String() == provider }):
			models[i].Provider = i18n.T(c, "chat_provider_source", "name", models[i].Provider, "source", host)
		default:
			models[i].Provider = i18n.T(c, "chat_provider_source", "name", models[i].Provider, "source", i18n.T(c, "chat_config_desktop"))
		}
	}
}
