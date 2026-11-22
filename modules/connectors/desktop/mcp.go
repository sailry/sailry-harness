package desktop

import (
	"fmt"
	"slices"
	"strings"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/icons"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/preview"
	"github.com/sailry/sailry-harness/internal/desktop/settings"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

// mcpEditor is a server's name, transport, command and arguments or URL, and
// its environment or headers.
type mcpEditor struct {
	open      bool
	original  string // the service edited, "" for a new one
	name      string
	transport int
	command   string
	args      []string
	url       string
	slots     []preview.EnvVar
	marks     []slotMark // beside slots: what is saved for each and how it shows
	revision  uint64
	// source is the transport the saved server has: a credential saved for
	// a command does not carry over to a URL, nor one for a URL to a command.
	source int
	// declared is a plugin's own server: its name and transport are fixed.
	declared bool
	// loading is the saved configuration being read and unread that the
	// read failed, which leaves the editor locked with Reload. deferred is
	// an editor that opens only once its configuration is read.
	loading, unread, deferred bool
	// uncertain is a save whose outcome is not known: the draft is locked
	// and Save retries that same save. detail is the host's reason for the
	// last failure, shown while details is on.
	uncertain bool
	detail    string
	details   bool
	// request is the save under way: Save waits for it, and the editor that
	// replaced this one has another or none, so a late completion does not
	// close it.
	request *int
	// removed counts the rows taken out of args and slots. It is part of a
	// row's identity: a row that moves into a removed one's place does not
	// take its text state (undo history, selection) with the place.
	removed int
}

// slotMark is a slot's saved credential: the name it is saved under, "" for
// none, whether saving clears it, and whether its typed value shows.
type slotMark struct {
	saved string
	shown bool
}

// mark is the slot's mark, made on first use for a slot added since.
func (e *mcpEditor) mark(i int) *slotMark {
	for len(e.marks) < len(e.slots) {
		e.marks = append(e.marks, slotMark{})
	}
	return &e.marks[i]
}

// kept reports whether saving keeps the slot's saved credential: nothing
// typed over it, under the name and the kind of transport it was saved for.
func (e *mcpEditor) kept(i int) bool {
	return e.pinned(i) && (e.source == 0) == (e.transport == 0)
}

// pinned reports whether the slot still names its saved credential with no
// value typed over it.
func (e *mcpEditor) pinned(i int) bool {
	mark := e.mark(i)
	return mark.saved != "" && mark.saved == strings.TrimSpace(e.slots[i].Key) && e.slots[i].Value == ""
}

// locked reports whether the draft cannot change now: its configuration is
// not read, or a save is under way or uncertain.
func (e *mcpEditor) locked() bool {
	return e.loading || e.unread || e.request != nil || e.uncertain
}

// configureServer opens the editor of a server added by hand. The preview's
// row is the whole server; a live row is not, so its editor opens locked on
// what the row has while the Node reads the saved configuration and which
// slots hold a credential.
func (m *page) configureServer(s *preview.Service) {
	if len(s.Servers) == 0 {
		return
	}
	if !m.Live || m.ConnectorActions == nil || m.ConnectorActions.ConfigureMCP == nil {
		var saved []string
		for _, slot := range s.Servers[0].Env {
			saved = append(saved, slot.Key)
		}
		m.editServer(s, saved)
		return
	}
	m.editServer(s, nil)
	m.mcp.loading = true
	m.ConnectorActions.ConfigureMCP(s.ID)
}

// configurePackage asks for the configuration of a plugin's own servers: its
// settings form when it declares one, else the server's editor once read.
func (m *page) configurePackage(s *preview.Service) {
	m.mcp = mcpEditor{original: s.ID, loading: true, deferred: true}
	m.ConnectorActions.ConfigureMCP(s.ID)
}

// editServer opens the editor on the service's server. The slots named in
// saved hold a credential; no slot carries a value.
func (m *page) editServer(s *preview.Service, saved []string) {
	x := s.Servers[0]
	m.mcp = mcpEditor{open: true, original: s.ID, revision: s.Revision, name: s.Name, transport: x.Transport,
		source: x.Transport, command: x.Command, args: slices.Clone(x.Args), url: x.URL}
	for _, slot := range x.Env {
		mark := slotMark{}
		if slices.Contains(saved, slot.Key) {
			mark.saved = slot.Key
		}
		m.mcp.slots = append(m.mcp.slots, preview.EnvVar{Key: slot.Key})
		m.mcp.marks = append(m.mcp.marks, mark)
	}
}

// OpenMCPConfiguration shows the saved configuration in the server's editor.
// Only the editor waiting for this service takes it: one closed while it
// waited, or opened on another service since, is left as it is.
func (m *page) OpenMCPConfiguration(configuration settings.MCPConfiguration) {
	e := &m.mcp
	if !e.loading || e.original != configuration.Service.ID || !e.open && !e.deferred {
		return
	}
	if len(configuration.Service.Servers) == 0 {
		e.loading = false
		return
	}
	m.editServer(&configuration.Service, configuration.Saved)
	m.mcp.declared = configuration.Declared
}

// FailMCPConfiguration leaves the editor waiting for the service unloaded,
// with the reason for its diagnostics: Reload reads again.
func (m *page) FailMCPConfiguration(id, detail string) {
	if m.mcp.loading && m.mcp.original == id {
		m.mcp.loading, m.mcp.unread, m.mcp.detail = false, true, detail
	}
}

// mcpDialog is name, transport, the command and its arguments or the URL,
// then the environment or headers, whose saved credentials stay unless typed
// over or removed; then the last failure's reason when asked for, and
// Reload, Details, Cancel and Save. A connector that authorizes the server is
// the row's, not the editor's. A saved server's editor
// is locked until its configuration is read, and while a save is uncertain.
func mcpDialog(c *ui.Context, m *page) {
	e := &m.mcp
	if !e.open {
		return
	}
	p := theme.Of(c)
	t := func(k string, a ...any) string { return i18n.T(c, k, a...) }
	title := t("mcp_add")
	if e.original != "" {
		title = t("mcp_edit")
	}
	locked := e.locked()
	live := m.Live && m.ConnectorActions != nil
	kit.Dialog(c.Key("mcp-editor"), &e.open, func() {
		ui.Row(c.Key("title")).Gap(8).Children(func() {
			ui.Text(c, title).FontSize(16).FontWeight(600)
			if e.loading {
				kit.Busy(c)
			}
		})
		ui.Scroll(c.Key("body")).Grow(1).Shrink(1).Gap(16).Busy(e.loading).Children(func() {
			kit.Field(c, "name", t("settings_name"), "", func() {
				// A saved server keeps the name it was added under.
				ui.TextInput(c.Key("input"), &e.name).Label(t("settings_name")).FillWidth().Disabled(locked || e.original != "")
			})
			kit.Field(c, "transport", t("mcp_transport"), "", func() {
				kit.Segmented(c, "tabs", &e.transport, []string{t("plugins_stdio"), t("plugins_http"), t("plugins_sse")}, kit.SegmentedProps{Block: true}).
					Label(t("mcp_transport")).Disabled(locked || e.declared)
			})
			if e.transport == 0 {
				kit.Field(c, "command", t("mcp_command"), "", func() {
					ui.TextInput(c.Key("input"), &e.command).Label(t("mcp_command")).Placeholder(t("mcp_command_hint")).FillWidth().Font("monospace").Disabled(locked)
				})
				kit.Field(c, "args", t("mcp_arguments"), "", func() {
					ui.Column(c).Gap(6).Children(func() {
						for i := range e.args {
							ui.Row(c.Key([2]int{e.removed, i})).Gap(4).Children(func() {
								ui.TextInput(c.Key("input"), &e.args[i]).Label(t("mcp_argument")).Grow(1).Font("monospace").Disabled(locked)
								kit.Action(c.Key("remove"), icons.Close, t("mcp_remove_argument"), locked).OnClick(func() {
									e.args = slices.Delete(e.args, i, i+1)
									e.removed++
								})
							})
						}
						kit.Secondary(c.Key("add"), t("mcp_add_argument")).Height(28).AlignSelf(ui.Start).Disabled(locked).OnClick(func() { e.args = append(e.args, "") })
					})
				})
			} else {
				kit.Field(c, "url", t("mcp_url"), "", func() {
					ui.TextInput(c.Key("input"), &e.url).Label(t("mcp_url")).Placeholder(t("mcp_url_hint")).FillWidth().Disabled(locked)
				})
			}
			slots, add := "mcp_environment", "mcp_add_environment"
			if e.transport != 0 {
				slots, add = "mcp_headers", "mcp_add_header"
			}
			kit.Field(c, "slots", t(slots), "", func() {
				ui.Column(c).Gap(6).Children(func() {
					for i := range e.slots {
						ui.Column(c.Key([2]int{e.removed, i})).Gap(4).Children(func() {
							shown := e.mark(i).shown
							ui.Row(c).Gap(4).Children(func() {
								ui.TextInput(c.Key("name"), &e.slots[i].Key).Label(t("mcp_slot_name")).Placeholder(t("mcp_slot_name")).Width(160).Disabled(locked)
								// A saved value stays unless typed over; the row's
								// removal drops it.
								hint := "form_secret_hint"
								if e.kept(i) {
									hint = "mcp_secret_saved"
								}
								value := ui.TextInput(c.Key("value"), &e.slots[i].Value).Label(t("mcp_slot_value")).Placeholder(t(hint)).Grow(1).Disabled(locked)
								mask := "mcp_hide_secret"
								if !shown {
									value.Password()
									mask = "mcp_show_secret"
								}
								kit.IconButton(c.Key("mask"), icons.Eye, t(mask), 28, shown).OnClick(func() { e.mark(i).shown = !shown })
								kit.Action(c.Key("remove"), icons.Close, t("mcp_remove_slot"), locked).OnClick(func() {
									e.mark(i)
									e.slots, e.marks = slices.Delete(e.slots, i, i+1), slices.Delete(e.marks, i, i+1)
									e.removed++
								})
							})
						})
					}
					kit.Secondary(c.Key("add"), t(add)).Height(28).AlignSelf(ui.Start).Disabled(locked).OnClick(func() { e.slots = append(e.slots, preview.EnvVar{}) })
				})
			})
		})
		if e.details && e.detail != "" {
			ui.Text(c.Key("detail"), e.detail).FontSize(12).TextColor(p.MutedForeground).Selectable()
		}
		reload := func() {
			if live && m.ConnectorActions.ConfigureMCP != nil && !e.loading {
				e.loading = true
				m.ConnectorActions.ConfigureMCP(e.original)
			}
		}
		save := func() {
			if e.request != nil || e.loading || e.unread {
				return
			}
			if problem := e.Check(); problem != "" {
				kit.Toast(c, kit.Failed, t(problem))
				return
			}
			if live && m.ConnectorActions.SaveMCP != nil {
				request := new(int)
				e.request = request
				draft := e.draft()
				draft.Failed = func(uncertain bool, detail string) {
					if e.request == request {
						e.uncertain, e.detail = uncertain, detail
					}
				}
				m.ConnectorActions.SaveMCP(draft, func(ok bool) {
					if e.request != request {
						return
					}
					e.request = nil
					if ok {
						e.open = false
					}
				})
				return
			}
			e.save(m)
			kit.Toast(c, kit.Done, t("mcp_configuration_saved"))
			e.open = false
		}
		ui.Row(c.Key("footer")).Gap(8).Justify(ui.End).Children(func() {
			if e.unread {
				kit.Secondary(c.Key("reload"), t("plugins_settings_reload")).Disabled(e.loading).OnClick(reload)
			}
			if e.detail != "" {
				kit.TextAction(c.Key("diagnostics"), t("plugins_settings_diagnostics"), false).OnClick(func() { e.details = !e.details })
			}
			kit.Secondary(c.Key("cancel"), t("settings_cancel")).OnClick(func() { e.open = false })
			// A save whose outcome is unknown is asked for again as it was.
			action := t("settings_save")
			if e.uncertain {
				action = t("plugins_retry")
			}
			pendingButton(c, "ok", action, e.loading || e.unread, e.request != nil).OnClick(save)
		})
	}).Width(settings.DialogWidth(c, 520)).Label(title)
}

// draft is the server as it is saved: its slots under their trimmed names,
// and the ones whose saved credential stays.
func (e *mcpEditor) draft() settings.MCPDraft {
	name := strings.TrimSpace(e.name)
	draft := settings.MCPDraft{Service: e.original, Name: name, Revision: e.revision,
		Server: preview.Server{Name: name, Transport: e.transport, Command: e.command, Args: slices.Clone(e.args), URL: e.url}}
	for i, slot := range e.slots {
		key := strings.TrimSpace(slot.Key)
		draft.Server.Env = append(draft.Server.Env, preview.EnvVar{Key: key, Value: slot.Value})
		if e.kept(i) {
			draft.Keep = append(draft.Keep, key)
		}
	}
	return draft
}

// Check validates the server: the i18n key of the problem, or "". A saved
// server's name is not the draft's to change.
func (e *mcpEditor) Check() string {
	n := strings.TrimSpace(e.name)
	ok := n != ""
	for _, r := range n {
		if !(r >= 'a' && r <= 'z' || r >= '0' && r <= '9' || r == '.' || r == '-') {
			ok = false
		}
	}
	if !ok && e.original == "" {
		return "mcp_invalid"
	}
	if e.transport == 0 && strings.TrimSpace(e.command) == "" {
		return "mcp_invalid"
	}
	if e.transport != 0 && !strings.HasPrefix(e.url, "http://") && !strings.HasPrefix(e.url, "https://") {
		return "mcp_invalid"
	}
	for _, s := range e.slots {
		if strings.TrimSpace(s.Key) == "" {
			return "mcp_invalid"
		}
	}
	return ""
}

func (e *mcpEditor) save(m *page) {
	name := strings.TrimSpace(e.name)
	x := preview.Server{Name: "mcp-" + name, Transport: e.transport, Command: e.command, Args: e.args, URL: e.url, Allowed: true, State: preview.ServerStarting}
	for _, s := range e.slots {
		x.Env = append(x.Env, preview.EnvVar{Key: s.Key}) // values are never kept
	}
	desc := e.url
	if e.transport == 0 {
		desc = strings.TrimSpace(e.command + " " + strings.Join(e.args, " "))
	}
	if s := m.service(e.original); s != nil {
		s.Name, s.Description, s.Servers = name, desc, []preview.Server{x}
		return
	}
	m.Services = append(m.Services, preview.Service{ID: fmt.Sprintf("mcp-%s-%d", name, len(m.Services)), Name: name, Description: desc, Manual: true, Servers: []preview.Server{x}})
}
