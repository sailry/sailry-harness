package live

import (
	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/settings"
	"github.com/sailry/sailry-harness/internal/protocol"
)

type workspaceSettingsState struct {
	sandboxRevision uint64
	sandboxModel    settings.Stored[bool]
	sandboxKnown    bool
}

func (r *Runtime) workspaceSettingsFrame() {
	r.sandboxFrame()
	r.hooksSettingsFrame()
}

func (r *Runtime) sandboxFrame() {
	owner := r.shownSettings()
	m := r.App.Settings
	if owner.machineSettingsStatus() != "" {
		m.SaveSandbox = nil
		m.Sandbox = settings.Stored[bool]{Pending: true}
		return
	}
	state := &owner.workspaceSettings
	m.SaveSandbox = func() {
		if owner.showsSettings() && owner.machineSettingsStatus() == "" {
			state.sandboxModel.Draft = m.Sandbox.Draft
			owner.saveSandbox()
		}
	}
	if (!state.sandboxKnown || owner.snapshot().SandboxSettingsRevision > state.sandboxRevision) && !state.sandboxModel.Pending {
		owner.readSandbox()
	}
	owner.publishSandbox()
}

func (r *Runtime) readSandbox() {
	m := &r.workspaceSettings.sandboxModel
	if m.Pending {
		return
	}
	m.Pending = true
	r.workspaceSettings.sandboxRevision = r.snapshot().SandboxSettingsRevision
	r.workspaceSettings.sandboxKnown = true
	r.publishSandbox()
	r.execute(protocol.Command{Kind: "read_sandbox_settings"}, func(c *ui.Context, out protocol.Output, err error) {
		m.Pending = false
		defer r.publishSandbox()
		if err != nil {
			r.reportKeys(c, err, "sandbox_settings_failed", "sandbox_settings_conflict", "sandbox_settings_unknown")
			return
		}
		value, ok := detail[protocol.SandboxSettings](out.Data)
		if !ok {
			return
		}
		if m.Draft == m.Saved {
			m.Draft = value.Enabled
		}
		m.Saved, m.Revision = value.Enabled, value.Revision
		r.workspaceSettings.sandboxRevision = value.Revision
	})
}
func (r *Runtime) saveSandbox() {
	if r.ctx != nil && r.ctx.Err() != nil {
		r.Report(fileActionFault(protocol.ErrorCodeUnavailable, "the sandbox execution host is unavailable"))
		return
	}
	m := &r.workspaceSettings.sandboxModel
	if m.Pending {
		return
	}
	m.Pending = true
	r.publishSandbox()
	r.saveSetting(protocol.Command{Kind: "save_sandbox_settings", Data: protocol.SandboxSettings{Revision: m.Revision, Enabled: m.Draft}}, settings.Messages{Done: "sandbox_settings_saved", Failed: "sandbox_settings_failed", Conflict: "sandbox_settings_conflict", Uncertain: "sandbox_settings_unknown"}, func(out protocol.Output, ok bool) {
		m.Pending = false
		defer r.publishSandbox()
		if !ok {
			return
		}
		value, valid := detail[protocol.SandboxSettings](out.Data)
		if !valid {
			return
		}
		m.Saved, m.Revision = value.Enabled, value.Revision
		r.workspaceSettings.sandboxRevision = value.Revision
	})
}

func (r *Runtime) publishSandbox() {
	if r.showsSettings() {
		r.App.Settings.Sandbox = r.workspaceSettings.sandboxModel
	}
}
