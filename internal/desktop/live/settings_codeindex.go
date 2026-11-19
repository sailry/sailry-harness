package live

import (
	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/settings"
	"github.com/sailry/sailry-harness/internal/protocol"
)

type codeIndexSettingsState struct {
	settings.Stored[bool]
	persistent *bool
}

func (r *Runtime) readCodeIndex() {
	m := &r.codeIndexSettings
	r.publishCodeIndex()
	if m.Pending {
		return
	}
	m.Pending = true
	r.publishCodeIndex()
	r.execute(protocol.Command{Kind: "read_code_index_settings"}, func(c *ui.Context, out protocol.Output, err error) {
		m.Pending = false
		defer r.publishCodeIndex()
		if err != nil {
			r.reportKeys(c, err, settings.CodeIndexMessages.Failed, settings.CodeIndexMessages.Conflict, settings.CodeIndexMessages.Uncertain)
			return
		}
		value, ok := detail[protocol.CodeIndexSettings](out.Data)
		if !ok {
			return
		}
		if m.Draft == m.Saved {
			m.Draft = value.Enabled
		}
		m.load(value)
	})
}

func (r *Runtime) saveCodeIndex() {
	m := &r.codeIndexSettings
	if m.Pending {
		return
	}
	m.Draft = r.App.Settings.CodeIndex.Draft
	r.saveCodeIndexSettings(nil)
}

func (r *Runtime) savePersistentCodeIndex(enabled bool) {
	m := &r.codeIndexSettings
	if m.Pending || !m.Saved || m.persistent == nil || *m.persistent == enabled {
		return
	}
	r.saveCodeIndexSettings(new(enabled))
}

func (r *Runtime) saveCodeIndexSettings(persistent *bool) {
	m := &r.codeIndexSettings
	m.Pending = true
	r.publishCodeIndex()
	command := protocol.Command{Kind: "save_code_index_settings", Data: protocol.CommandSaveCodeIndexSettingsPayload{
		Settings: protocol.CodeIndexSettings{
			Revision: m.Revision, Enabled: m.Draft, Persistent: persistent,
		},
		ExpectedRevision: m.Revision,
	}}
	r.saveSetting(command, settings.CodeIndexMessages, func(out protocol.Output, ok bool) {
		m.Pending = false
		defer r.publishCodeIndex()
		if !ok {
			// The toast says what happened; the switch goes back to the
			// Node's value, read again with the revision to save at.
			m.Draft = m.Saved
			r.readCodeIndex()
			return
		}
		if value, valid := detail[protocol.CodeIndexSettings](out.Data); valid {
			m.load(value)
		}
	})
}

func (m *codeIndexSettingsState) load(value protocol.CodeIndexSettings) {
	m.Saved, m.Revision = value.Enabled, value.Revision
	m.persistent = value.Persistent
}

func (r *Runtime) publishCodeIndex() {
	if r.showsSettings() {
		r.App.Settings.CodeIndex = r.codeIndexSettings.Stored
		r.App.Settings.PersistentCodeIndex = r.codeIndexSettings.persistent
	}
}
