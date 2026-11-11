package live

import (
	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/settings"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// The code index switch (Settings, Conversation) is the Node's
// code_index_settings: off, the Node builds no index and its code queries
// answer that the index is not configured. It is the Node Settings edits
// (settings_target.go). The Node reports no revision for it in its
// snapshot, so it is read when the pages turn to the Node and again after
// a save that did not take, which also puts the switch back.

func (r *Runtime) readCodeIndex() {
	m := &r.App.Settings.CodeIndex
	if m.Pending {
		return
	}
	m.Pending = true
	r.execute(protocol.Command{Kind: "read_code_index_settings"}, func(c *ui.Context, out protocol.Output, err error) {
		if !r.showsSettings() {
			return
		}
		m.Pending = false
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
		m.Saved, m.Revision = value.Enabled, value.Revision
	})
}

func (r *Runtime) saveCodeIndex() {
	m := &r.App.Settings.CodeIndex
	if m.Pending {
		return
	}
	m.Pending = true
	command := protocol.Command{Kind: "save_code_index_settings", Data: protocol.CommandSaveCodeIndexSettingsPayload{
		Settings: protocol.CodeIndexSettings{Revision: m.Revision, Enabled: m.Draft}, ExpectedRevision: m.Revision,
	}}
	r.saveSetting(command, settings.CodeIndexMessages, func(out protocol.Output, ok bool) {
		if !r.showsSettings() {
			return
		}
		m.Pending = false
		if !ok {
			// The toast says what happened; the switch goes back to the
			// Node's value, read again with the revision to save at.
			m.Draft = m.Saved
			r.readCodeIndex()
			return
		}
		if value, valid := detail[protocol.CodeIndexSettings](out.Data); valid {
			m.Saved, m.Revision = value.Enabled, value.Revision
		}
	})
}
