package live

import (
	"github.com/sailry/sailry-harness/internal/desktop/settings"
	"github.com/sailry/sailry-harness/internal/desktop/shell"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// settingsOwner is the local Runtime that owns controller configuration.
func (r *Runtime) settingsOwner() *Runtime { return r.localRuntime() }

func (r *Runtime) managedSettingsNode() *Runtime {
	root := r.localRuntime()
	owner := root.runtimeFor(root.App.Settings.ManagedNode)
	if owner != nil && (owner == root || owner.controller.Connected()) {
		return owner
	}
	return nil
}

func (r *Runtime) openNodeSettings(section settings.Section) {
	root := r.localRuntime()
	if !root.App.Settings.HasSection(section) {
		return
	}
	target := ""
	if r != root {
		target = r.controller.Target().String()
	}
	root.App.Settings.ManagedNode = target
	root.App.Settings.Open(section)
	root.App.Navigate(shell.SettingsPage)
}

// machineSettingsStatus follows the Node's administrator boundary for
// settings shared by every user of that machine.
func (r *Runtime) machineSettingsStatus() string {
	if r == nil {
		return "connections_offline"
	}
	if r.parent != nil {
		if !r.controller.Connected() {
			return "connections_offline"
		}
		if user := r.snapshot().User; user == nil || !user.Administrator {
			return "permission_denied"
		}
	}
	return ""
}

func (r *Runtime) nodeSettingsOwner() *Runtime {
	if r.settingsSectionVisible(settings.General) || r.settingsSectionVisible(settings.Conversation) {
		return r.managedSettingsNode()
	}
	return r
}

// shownSettings is the Runtime currently bound to the core Node settings.
func (r *Runtime) shownSettings() *Runtime {
	root := r.localRuntime()
	if root.settingsGeneration == 0 {
		return root
	}
	return root.settingsShown
}

func (r *Runtime) showsSettings() bool { return r.shownSettings() == r }

// settingsCurrent checks whether a response still belongs to the same
// binding, including when the user switches away and back before it arrives.
func (r *Runtime) settingsCurrent() func() bool {
	root := r.localRuntime()
	generation := root.settingsGeneration
	return func() bool { return root.settingsGeneration == generation && r.showsSettings() }
}

func (r *Runtime) initNodeSettings() {
	m := r.App.Settings
	// The target selector is shared even when the provider editor is omitted.
	m.ManagedHosts = r.providerHosts
	onMachine := func(act func(owner *Runtime)) {
		owner := r.shownSettings()
		if owner == nil || !owner.controller.Connected() {
			r.Report(fileActionFault(protocol.ErrorCodeUnavailable, "the settings host is unavailable"))
			return
		}
		if owner.machineSettingsStatus() == "" {
			act(owner)
		}
	}
	m.ReadPersonalInstructions = func() { onMachine(func(owner *Runtime) { owner.readPersonalInstructions() }) }
	m.EditPersonalInstructions = func() { onMachine(func(owner *Runtime) { owner.queuePersonalInstructionsSave() }) }
	m.RetryPersonalInstructions = func() { onMachine(func(owner *Runtime) { owner.savePersonalInstructions() }) }
	m.SavePower = func() { onMachine(func(owner *Runtime) { owner.savePower() }) }
	m.ExecutionSettings = r.executionSettings
	r.enterNodeSettings()
}

func (r *Runtime) nodeSettingsFrame() {
	root := r.localRuntime()
	root.syncSettingsPages()
	owner := root.nodeSettingsOwner()
	if owner != root.shownSettings() {
		root.showNodeSettings(owner)
	}
	m := root.App.Settings
	status := owner.machineSettingsStatus()
	allowedNow := status == "" && m.ExecutionSettingsStatus != ""
	m.ExecutionSettingsStatus = status
	var editor *settings.PersonalInstructions
	if status == "" && root.settingsSectionVisible(settings.Conversation) {
		editor = &owner.personalInstructions.editor
	}
	if previous := m.PersonalInstructions; previous != editor {
		// Save through the old owner before another section or host takes its place.
		if previous != nil && previous.Detach() && root.instructionsOwner != nil {
			root.instructionsOwner.queuePersonalInstructionsSave()
		}
		m.PersonalInstructions, root.instructionsOwner = editor, nil
		if editor != nil {
			root.instructionsOwner = owner
			owner.readPersonalInstructions()
		}
	}
	if status == "" && owner.controller.Connected() {
		if allowedNow || owner.snapshot().PowerSettingsRevision > owner.powerObservedRevision && !m.Power.Pending {
			owner.readPower()
		}
		if allowedNow {
			owner.readExecutionSettings()
		}
	}
}

func (r *Runtime) showNodeSettings(owner *Runtime) {
	root := r.localRuntime()
	root.settingsShown = owner
	root.settingsGeneration++
	m := root.App.Settings
	m.Power = settings.Stored[bool]{Pending: owner == nil}
	m.ExecutionSettings = nil
	if owner != nil {
		m.ExecutionSettings = owner.executionSettings
		owner.enterNodeSettings()
	}
}

func (r *Runtime) enterNodeSettings() {
	if r.machineSettingsStatus() == "" {
		r.readPower()
		r.readExecutionSettings()
	}
}
