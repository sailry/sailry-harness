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
		m.Saved, m.Revision = value.Enabled, value.Revision
	})
}

func (r *Runtime) saveCodeIndex() {
	m := &r.codeIndexSettings
	if m.Pending {
		return
	}
	m.Draft = r.App.Settings.CodeIndex.Draft
	m.Pending = true
	r.publishCodeIndex()
	command := protocol.Command{Kind: "save_code_index_settings", Data: protocol.CommandSaveCodeIndexSettingsPayload{
		Settings: protocol.CodeIndexSettings{Revision: m.Revision, Enabled: m.Draft}, ExpectedRevision: m.Revision,
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
			m.Saved, m.Revision = value.Enabled, value.Revision
		}
	})
}

func (r *Runtime) publishCodeIndex() {
	if r.showsSettings() {
		r.App.Settings.CodeIndex = r.codeIndexSettings
	}
}
