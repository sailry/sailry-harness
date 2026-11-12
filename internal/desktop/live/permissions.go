package live

import (
	"context"
	"encoding/json"
	"errors"
	"os"
	"os/exec"
	"path/filepath"
	"runtime"
	"slices"
	"strings"

	"github.com/egoist/mygo/plugins/permissionflow"
	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/settings"
	"github.com/sailry/sailry-harness/internal/desktop/shell"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// The Settings permissions page shows one row per system resource. Sailry's
// own grants (Full Disk Access, Accessibility and the microphone) are read
// from this process; a package's grants, such as the computer helper's
// Accessibility and Screen Recording, are read and requested through the
// package's host handlers on the device that executes them.

const permissionApp = "sailry"

var permissionKinds = map[protocol.PermissionsKind]struct {
	key  string
	flow permissionflow.Permission
}{
	protocol.PermissionsKindFullDisk:        {"permission_full_disk", permissionflow.FullDiskAccess},
	protocol.PermissionsKindScreenRecording: {"permission_screen", permissionflow.ScreenRecording},
	protocol.PermissionsKindAccessibility:   {"permission_accessibility", permissionflow.Accessibility},
	protocol.PermissionsKindMicrophone:      {"permission_microphone", permissionflow.Microphone},
}

// permissionProbe is this process's view of the system's permissions.
type permissionProbe struct {
	goos     string
	check    func(permissionflow.Permission) (permissionflow.Status, error)
	request  func(context.Context, permissionflow.Permission) (permissionflow.Status, error)
	settings func(permissionflow.Permission) error
}

func systemPermissions() permissionProbe {
	return permissionProbe{goos: runtime.GOOS, check: permissionflow.Check, request: permissionflow.Request, settings: permissionflow.OpenSettings}
}

// permissionStatus names the status of one of this process's grants. macOS
// reports Accessibility, Screen Recording and Full Disk Access as denied
// until granted, which is a grant still to make; only the microphone tells a
// refusal apart.
func permissionStatus(goos string, kind protocol.PermissionsKind, status permissionflow.Status, err error) string {
	switch {
	case goos != "darwin" || errors.Is(err, errors.ErrUnsupported):
		return "permission_unavailable"
	case err != nil || status == permissionflow.Unknown:
		return "permission_unknown"
	case status == permissionflow.Granted:
		return "permission_granted"
	case status == permissionflow.Denied && kind == protocol.PermissionsKindMicrophone:
		return "permission_denied"
	}
	return "permission_required"
}

// permissionEntry is one grant a package declares with native handlers.
type permissionEntry struct {
	owner       *Runtime
	pack        protocol.PluginReference
	declaration protocol.PermissionsContribution
}

type permissionCard struct {
	device   protocol.NodeID
	subject  string
	kind     protocol.PermissionsKind
	status   string
	settings bool
	check    bool
	request  bool
	entry    *permissionEntry // nil for a grant of this process
}

func (card permissionCard) satisfied() bool {
	return card.status == "permission_granted" || card.status == "permission_not_needed"
}

func (card permissionCard) blocked() bool {
	return card.status == "permission_denied" || card.status == "permission_restricted"
}

func (card permissionCard) absent() bool {
	return card.status == "permission_remote" || card.status == "permission_unavailable"
}

// opensSettings reports a grant only the system's settings can change.
func (card permissionCard) opensSettings() bool {
	return !card.absent() && card.settings && (card.kind == protocol.PermissionsKindFullDisk || card.blocked())
}

func (card permissionCard) actionable() bool {
	return !card.satisfied() && !card.absent() && (card.opensSettings() || card.request)
}

func (card permissionCard) owned(other permissionCard) bool {
	return card.device == other.device && card.subject == other.subject
}

// permissionDirectory lists the first declaration of each grant among a
// device's enabled packages.
func permissionDirectory(owner *Runtime, packages []protocol.PluginInfo) []permissionEntry {
	var entries []permissionEntry
	for _, info := range packages {
		if !info.Summary.Enabled || info.Extension == nil {
			continue
		}
		for _, declaration := range info.Extension.Permissions {
			if _, known := permissionKinds[declaration.Kind]; !known || declaration.Subject == "" || slices.ContainsFunc(entries, func(entry permissionEntry) bool {
				return entry.owner == owner && entry.declaration.Subject == declaration.Subject && entry.declaration.Kind == declaration.Kind
			}) {
				continue
			}
			entries = append(entries, permissionEntry{
				owner:       owner,
				pack:        protocol.PluginReference{Name: info.Summary.Name, Digest: info.Summary.Digest, SettingsRevision: info.Summary.SettingsRevision},
				declaration: declaration,
			})
		}
	}
	return entries
}

// permissionCards builds the application's grants followed by the packages'.
// A package grant without a check handler is this process's own; on another
// device it can only be granted there.
func permissionCards(goos string, local protocol.NodeID, entries []permissionEntry, device func(permissionEntry) (protocol.NodeID, bool), previous []permissionCard) []permissionCard {
	native := func(kind protocol.PermissionsKind) permissionCard {
		card := permissionCard{device: local, subject: permissionApp, kind: kind, status: "permission_unknown", settings: true, check: true, request: kind != protocol.PermissionsKindFullDisk}
		if goos != "darwin" {
			card.status, card.check, card.request = "permission_unavailable", false, false
		}
		return card
	}
	cards := []permissionCard{native(protocol.PermissionsKindFullDisk), native(protocol.PermissionsKindAccessibility), native(protocol.PermissionsKindMicrophone)}
	for index := range entries {
		entry := &entries[index]
		target, connected := device(*entry)
		card := permissionCard{device: target, subject: entry.declaration.Subject, kind: entry.declaration.Kind}
		if slices.ContainsFunc(cards, func(existing permissionCard) bool { return existing.kind == card.kind && existing.owned(card) }) {
			continue
		}
		switch {
		case entry.declaration.Check != nil:
			card.status, card.entry = "permission_unknown", entry
			card.settings = goos == "darwin" && target == local
			card.check = connected
			card.request = connected && target == local && entry.declaration.Request != nil
		case target != local:
			card.status = "permission_remote"
		default:
			owned := native(card.kind)
			owned.subject = card.subject
			card = owned
		}
		cards = append(cards, card)
	}
	for index := range cards {
		card := &cards[index]
		if card.absent() {
			continue
		}
		for _, earlier := range previous {
			if earlier.kind == card.kind && earlier.owned(*card) && !earlier.absent() {
				card.status = earlier.status
			}
		}
	}
	return cards
}

// permissionRows folds the cards of one resource into the page's row: all
// granted, some granted, or none.
func permissionRows(cards []permissionCard) []viewmodel.Permission {
	var rows []viewmodel.Permission
	for _, card := range cards {
		key := permissionKinds[card.kind].key
		if slices.ContainsFunc(rows, func(row viewmodel.Permission) bool { return row.Resource == key }) {
			continue
		}
		status, granted, missing, settings, opens := card.status, 0, 0, false, false
		members := 0
		for _, member := range cards {
			if member.kind != card.kind {
				continue
			}
			members++
			if member.satisfied() {
				granted++
			} else {
				missing++
			}
			settings = settings || member.settings && !member.absent()
			opens = opens || member.actionable() && member.opensSettings()
		}
		if members > 1 {
			switch {
			case missing == 0:
				status = "permission_granted"
			case granted > 0:
				status = "permission_partial"
			default:
				status = "permission_required"
			}
		}
		row := viewmodel.Permission{Resource: key, Status: status}
		// The row's flag selects both its settings shortcut and the label of
		// its action: the shortcut of a granted resource, or an action that
		// opens the system's settings.
		if status == "permission_granted" || status == "permission_not_needed" {
			row.Settings = settings
		} else {
			row.Settings = opens
		}
		rows = append(rows, row)
	}
	return rows
}

// permissionRequests selects the cards one resource's action works on: every
// missing grant of this process, and a package's when it declares a request.
func permissionRequests(cards []permissionCard, kind protocol.PermissionsKind) []permissionCard {
	var selected []permissionCard
	for _, card := range cards {
		if card.kind != kind || card.satisfied() || card.absent() || card.entry != nil && card.entry.declaration.Request == nil {
			continue
		}
		selected = append(selected, card)
	}
	return selected
}

type permissionChange struct {
	kind   protocol.PermissionsKind
	status string
}

type permissionState struct {
	probe      permissionProbe
	visible    bool
	cards      []permissionCard
	entries    []permissionEntry
	binding    string
	checking   bool
	pending    bool
	failed     bool // a step of the running work failed; reported once when it ends
	generation int
	actions    settings.PermissionActions
}

// permissionsFrame projects the page while it shows, and checks every grant
// when the page opens or its packages change.
func (r *Runtime) permissionsFrame() {
	root := r.localRuntime()
	state := &root.permissions
	m := root.App.Settings
	if root.App.Page != shell.SettingsPage || m.Section != settings.Permissions {
		if state.visible {
			state.visible, state.checking, state.pending = false, false, false
			m.SetBusy(settings.BusyPermissions, false)
			state.generation++
			state.failed = false
		}
		return
	}
	if state.probe.check == nil {
		state.probe = systemPermissions()
		state.actions = settings.PermissionActions{Check: root.checkPermissions, Request: root.requestPermission, Settings: root.openPermissionSettings, ShowApp: root.showPermissionApp}
	}
	refresh := !state.visible
	state.visible = true
	local := root.controller.Target()
	var entries []permissionEntry
	binding := []string{}
	for _, owner := range []*Runtime{root.packageOwner(), root} {
		if owner == nil || slices.ContainsFunc(entries, func(entry permissionEntry) bool { return entry.owner == owner }) {
			continue
		}
		device := owner.controller.Target()
		listed := permissionDirectory(owner, root.pluginViewCatalog(owner))
		for _, entry := range listed {
			binding = append(binding, device.String(), entry.pack.Name, entry.pack.Digest, entry.declaration.Subject, string(entry.declaration.Kind))
		}
		if len(listed) != 0 {
			binding = append(binding, device.String(), map[bool]string{true: "connected", false: "offline"}[owner.controller.Connected()])
		}
		entries = append(entries, listed...)
	}
	if key := strings.Join(binding, "\x00"); refresh && state.cards == nil || key != state.binding {
		state.binding, state.entries = key, entries
		state.generation++
		state.checking, state.pending, state.failed = false, false, false
		state.cards = permissionCards(state.probe.goos, local, entries, func(entry permissionEntry) (protocol.NodeID, bool) {
			return entry.owner.controller.Target(), entry.owner.controller.Connected()
		}, state.cards)
		refresh = true
	}
	if refresh {
		root.checkPermissions()
	}
	m.Permissions, m.PermissionActions = permissionRows(state.cards), &state.actions
	m.SetBusy(settings.BusyPermissions, state.checking)
}

func (state *permissionState) apply(owner permissionCard, changes []permissionChange) {
	for _, change := range changes {
		for index := range state.cards {
			if card := &state.cards[index]; card.kind == change.kind && card.owned(owner) {
				card.status = change.status
			}
		}
	}
}

// permissionWork runs steps off the UI thread in order and applies each
// result on it. Leaving the page or rebinding its packages drops the rest.
func (r *Runtime) permissionWork(steps []func() func(*ui.Context), done func(*ui.Context)) {
	state := &r.permissions
	generation := state.generation
	r.async(func() {
		for _, step := range steps {
			apply := step()
			r.enqueue(func(c *ui.Context) {
				if state.generation == generation && apply != nil {
					apply(c)
				}
			})
		}
		r.enqueue(func(c *ui.Context) {
			if state.generation == generation {
				done(c)
				state.report(c)
			}
		})
	})
}

// report toasts the failures of one action once, when its work and the
// check that follows a request have ended.
func (state *permissionState) report(c *ui.Context) {
	if state.failed && !state.checking && !state.pending {
		state.failed = false
		kit.Toast(c, kit.Failed, i18n.T(c, "permission_failed"))
	}
}

func (r *Runtime) checkPermissions() {
	state := &r.permissions
	if state.checking || state.pending {
		return
	}
	var steps []func() func(*ui.Context)
	for _, card := range state.cards {
		if card.check {
			steps = append(steps, r.permissionStep(card, false))
		}
	}
	if len(steps) == 0 {
		return
	}
	state.checking = true
	r.permissionWork(steps, func(*ui.Context) { state.checking = false })
}

// permissionStep reads one card's grants, asking for them first when
// request is set, and marks a failure for the work's one toast. A grant the
// system does not keep is the row's unavailable status, not a failure.
func (r *Runtime) permissionStep(card permissionCard, request bool) func() func(*ui.Context) {
	state := &r.permissions
	probe := state.probe
	return func() func(*ui.Context) {
		var changes []permissionChange
		var err error
		if card.entry != nil {
			changes, err = r.packagePermission(*card.entry, request)
		} else {
			flow := permissionKinds[card.kind].flow
			var status permissionflow.Status
			if request {
				_, err = probe.request(r.ctx, flow)
			}
			status, checkErr := probe.check(flow)
			if err == nil && !errors.Is(checkErr, errors.ErrUnsupported) {
				err = checkErr
			}
			changes = []permissionChange{{card.kind, permissionStatus(probe.goos, card.kind, status, checkErr)}}
		}
		return func(c *ui.Context) {
			if err != nil {
				if card.entry != nil {
					changes = []permissionChange{{card.kind, "permission_unknown"}}
				}
				state.failed = true
			}
			state.apply(card, changes)
		}
	}
}

// packagePermission calls a package's check or request handler. A request
// first reads the grants, so a granted or unrequestable permission never
// prompts.
func (r *Runtime) packagePermission(entry permissionEntry, request bool) ([]permissionChange, error) {
	call := func(request bool) (protocol.PermissionsStatus, error) {
		var status protocol.PermissionsStatus
		handler, kind := entry.declaration.Check, "read_plugin_callback"
		if request {
			handler, kind = entry.declaration.Request, "call_plugin"
		}
		if handler == nil {
			return status, &protocol.Fault{Code: protocol.ErrorCodeInvalidRequest, Message: "permission requires a native handler"}
		}
		input, err := json.Marshal(map[string]string{"kind": string(entry.declaration.Kind), "subject": entry.declaration.Subject})
		if err != nil {
			return status, err
		}
		command := protocol.Command{Kind: kind, Data: protocol.CommandReadPluginCallbackPayload{Handler: *handler, Input: input}}
		if request {
			command.Data = protocol.CommandCallPluginPayload{Handler: *handler, Input: input}
		}
		prepared := entry.owner.controller.Prepare(command)
		prepared.Plugin = &protocol.PluginContext{Surface: protocol.PluginDesktopSurface{Kind: "permissions"}, Package: entry.pack}
		output, err := entry.owner.controller.Execute(r.ctx, prepared)
		if err != nil {
			return status, err
		}
		// A plugin result is the callback's own JSON value.
		body, err := json.Marshal(output.Data)
		if err == nil && output.Kind == "plugin_result" {
			err = json.Unmarshal(body, &status)
		}
		if err != nil || output.Kind != "plugin_result" || !slices.ContainsFunc(status.Grants, func(grant protocol.PermissionsGrant) bool { return grant.Kind == entry.declaration.Kind }) {
			return status, &protocol.Fault{Code: protocol.ErrorCodeInvalidRequest, Message: "invalid permission status"}
		}
		return status, nil
	}
	status, err := call(false)
	if err == nil && request && status.CanRequest && !slices.ContainsFunc(status.Grants, func(grant protocol.PermissionsGrant) bool {
		return grant.Kind == entry.declaration.Kind && grant.Authorized != nil && *grant.Authorized
	}) {
		status, err = call(true)
	}
	if err != nil {
		return nil, err
	}
	var changes []permissionChange
	for _, grant := range status.Grants {
		change := permissionChange{grant.Kind, "permission_unavailable"}
		if grant.Authorized != nil {
			change.status = map[bool]string{true: "permission_granted", false: "permission_required"}[*grant.Authorized]
		}
		changes = append(changes, change)
	}
	return changes, nil
}

func (r *Runtime) permissionKind(resource string) (protocol.PermissionsKind, bool) {
	for kind, names := range permissionKinds {
		if names.key == resource {
			return kind, true
		}
	}
	return "", false
}

// requestPermission is a row's action: it asks for the resource's missing
// grants in turn, or opens the system's settings for a grant only they can
// change, then checks again.
func (r *Runtime) requestPermission(resource string) {
	state := &r.permissions
	kind, known := r.permissionKind(resource)
	if !known || state.checking || state.pending {
		return
	}
	requests := permissionRequests(state.cards, kind)
	if len(requests) == 0 {
		return
	}
	if len(requests) == 1 && requests[0].subject == permissionApp && requests[0].opensSettings() {
		r.openPermissionSettings(resource)
		return
	}
	probe := state.probe
	var steps []func() func(*ui.Context)
	for _, card := range requests {
		if !card.opensSettings() {
			if card.request {
				steps = append(steps, r.permissionStep(card, true))
			}
			continue
		}
		if card.kind == protocol.PermissionsKindFullDisk && card.check {
			steps = append(steps, r.permissionStep(card, false))
		}
		steps = append(steps, func() func(*ui.Context) {
			err := probe.settings(permissionKinds[card.kind].flow)
			return func(*ui.Context) {
				if err != nil {
					state.failed = true
				}
			}
		})
	}
	if len(steps) == 0 {
		return
	}
	state.pending = true
	r.permissionWork(steps, func(*ui.Context) {
		state.pending = false
		r.checkPermissions()
	})
}

// openPermissionSettings opens the resource's page of the system's settings.
// Full Disk Access is checked first: a refused protected open is what lists
// the application on that page.
func (r *Runtime) openPermissionSettings(resource string) {
	state := &r.permissions
	kind, known := r.permissionKind(resource)
	if !known || state.checking || state.pending {
		return
	}
	index := slices.IndexFunc(state.cards, func(card permissionCard) bool { return card.kind == kind && card.settings && !card.absent() })
	if index < 0 {
		return
	}
	probe := state.probe
	var steps []func() func(*ui.Context)
	if card := state.cards[index]; kind == protocol.PermissionsKindFullDisk && card.check {
		steps = append(steps, r.permissionStep(card, false))
	}
	steps = append(steps, func() func(*ui.Context) {
		err := probe.settings(permissionKinds[kind].flow)
		return func(*ui.Context) {
			if err != nil {
				state.failed = true
			}
		}
	})
	state.pending = true
	r.permissionWork(steps, func(*ui.Context) { state.pending = false })
}

// permissionBundle is the application bundle that holds an executable; the
// system's lists take the bundle, never a bare executable.
func permissionBundle(executable string) string {
	for path := executable; path != filepath.Dir(path); path = filepath.Dir(path) {
		if filepath.Ext(path) == ".app" {
			return path
		}
	}
	return ""
}

// showPermissionApp reveals the application to drag into the system's list.
func (r *Runtime) showPermissionApp() {
	executable, _ := os.Executable()
	bundle := permissionBundle(executable)
	if bundle == "" || r.permissions.probe.goos != "darwin" {
		r.enqueue(func(c *ui.Context) {
			c.AddToast(ui.Toast{Title: i18n.T(c, "permission_app_unavailable"), Timeout: kit.ToastDuration(c)})
		})
		return
	}
	r.async(func() {
		if err := exec.CommandContext(r.ctx, "/usr/bin/open", "-R", bundle).Run(); err != nil {
			r.enqueue(func(c *ui.Context) { kit.Toast(c, kit.Failed, i18n.T(c, "permission_failed")) })
		}
	})
}
