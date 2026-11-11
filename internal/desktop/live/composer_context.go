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

// composerHosts are the Hosts a draft can run on: this computer and the
// paired ones whose connection is bound, with the draft's own even while it
// is away so the control still names it.
func (r *Runtime) composerHosts(comp *composer.State) []composer.Host {
	root := r.localRuntime()
	var hosts []composer.Host
	for _, h := range root.App.Data.Hosts {
		if h.Local || h.ID == comp.Host || root.runtimeFor(h.ID) != nil {
			hosts = append(hosts, composer.Host{ID: h.ID, Name: h.Name, Key: h.Name == "local_host_name", Local: h.Local})
		}
	}
	return hosts
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
// guidance, and adds the message still on its way to the Host.
func (r *Runtime) projectComposerQueue(x *conversationActions, ctx *composer.Context) {
	if run, active := x.activeRun(); active {
		for i := range ctx.Queue {
			if item, ok := x.queued(ctx.Queue[i].ID); ok && item.Guidance != nil && *item.Guidance == run.Turn {
				ctx.Queue[i].Joining = true
			}
		}
	}
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
		if !guiding || !valid {
			return
		}
		for _, item := range update.Queue.Items {
			if item.Turn == guide.Turn && item.Ready && item.Guidance == nil {
				kit.Notify(c, "", i18n.T(c, "queue_guidance_separate"))
			}
		}
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
