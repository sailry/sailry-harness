package live

import (
	"fmt"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/settings"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func (r *Runtime) readSettings() { r.readPower(); r.readTelemetry(); r.readSandbox() }

func (r *Runtime) readPower() {
	m := &r.App.Settings.Power
	if m.Pending {
		return
	}
	m.Pending = true
	r.powerObservedRevision = r.snapshot().PowerSettingsRevision
	r.async(func() {
		output, err := r.controller.Do(r.ctx, protocol.Command{Kind: "read_power_settings"})
		r.enqueue(func(c *ui.Context) {
			m.Pending = false
			value, ok := detail[protocol.PowerSettings](output.Data)
			if err == nil && (output.Kind != "power_settings" || !ok) {
				err = fmt.Errorf("unexpected power settings response")
			}
			if err != nil {
				settings.Report(c, kit.Failed, settings.PowerMessages)
				return
			}
			if m.Draft == m.Saved {
				m.Draft = value.PreventSleep
			}
			m.Saved, m.Revision = value.PreventSleep, value.Revision
		})
	})
}
func (r *Runtime) readTelemetry() {
	m := &r.App.Settings.Telemetry
	if m.Pending {
		return
	}
	m.Pending = true
	r.async(func() {
		output, err := r.controller.Do(r.ctx, protocol.Command{Kind: "read_telemetry_settings"})
		r.enqueue(func(c *ui.Context) {
			m.Pending = false
			value, ok := detail[protocol.TelemetrySettings](output.Data)
			if err == nil && (output.Kind != "telemetry_settings" || !ok) {
				err = fmt.Errorf("unexpected telemetry settings response")
			}
			if err != nil {
				settings.Report(c, kit.Failed, settings.TelemetryMessages)
				return
			}
			draft := settings.Telemetry{Errors: value.Errors, Metrics: value.Metrics}
			if m.Draft == m.Saved {
				m.Draft = draft
			}
			m.Saved, m.Revision = draft, value.Revision
		})
	})
}

func (r *Runtime) savePower() {
	m := &r.App.Settings.Power
	if m.Pending {
		return
	}
	m.Pending = true
	request := protocol.PowerSettings{Revision: m.Revision, PreventSleep: m.Draft}
	r.saveSetting(protocol.Command{Kind: "save_power_settings", Data: request}, settings.PowerMessages, func(output protocol.Output, ok bool) {
		m.Pending = false
		if !ok {
			return
		}
		if value, valid := detail[protocol.PowerSettings](output.Data); valid && output.Kind == "power_settings" {
			m.Saved, m.Revision = value.PreventSleep, value.Revision
			r.powerObservedRevision = value.Revision
		}
	})
}
func (r *Runtime) saveTelemetry() {
	m := &r.App.Settings.Telemetry
	if m.Pending {
		return
	}
	m.Pending = true
	request := protocol.TelemetryUpdate{Revision: m.Revision, Errors: m.Draft.Errors, Metrics: m.Draft.Metrics}
	r.saveSetting(protocol.Command{Kind: "save_telemetry_settings", Data: request}, settings.TelemetryMessages, func(output protocol.Output, ok bool) {
		m.Pending = false
		if !ok {
			return
		}
		if value, valid := detail[protocol.TelemetrySettings](output.Data); valid && output.Kind == "telemetry_settings" {
			m.Saved, m.Revision = settings.Telemetry{Errors: value.Errors, Metrics: value.Metrics}, value.Revision
		}
	})
}

func (r *Runtime) saveSetting(command protocol.Command, messages settings.Messages, done func(protocol.Output, bool)) {
	r.execute(command, func(c *ui.Context, output protocol.Output, err error) {
		outcome := kit.Done
		if err != nil {
			outcome = kit.Failed
			switch faultCode(err) {
			case protocol.ErrorCodeConflict, protocol.ErrorCodeRevisionConflict:
				outcome = kit.Conflict
			case protocol.ErrorCodeOutcomeUnknown:
				outcome = kit.Uncertain
			}
		}
		settings.Report(c, outcome, messages)
		done(output, err == nil)
	})
}

func (r *Runtime) readNotices(ids []string) {
	if r.parent == nil {
		for _, bound := range r.bindings {
			bound.readNotices(ids)
		}
	}
	selected := map[string]bool{}
	for _, id := range ids {
		selected[id] = true
	}
	var durable []protocol.NotificationID
	for _, n := range r.controller.Notices() {
		if !selected[noticeID(n)] {
			continue
		}
		if n.ID.Target.Kind == "notification" {
			durable = append(durable, n.ID.Target.Notification)
		} else {
			r.controller.MarkAttentionRead(n.ID)
		}
	}
	if len(durable) > 0 {
		r.command(protocol.Command{Kind: "mark_notifications_read", Data: protocol.CommandMarkNotificationsReadPayload{IDs: durable}}, "notification_update_failed", nil)
	}
}
func (r *Runtime) dismissRead(ids []string)    { r.dismiss(ids, true) }
func (r *Runtime) dismissNotices(ids []string) { r.dismiss(ids, false) }
func (r *Runtime) dismiss(ids []string, readOnly bool) {
	if r.parent == nil {
		for _, bound := range r.bindings {
			bound.dismiss(ids, readOnly)
		}
	}
	selected := map[string]bool{}
	for _, id := range ids {
		selected[id] = true
	}
	var durable []protocol.NotificationID
	var attention []client.NoticeID
	for _, n := range r.controller.Notices() {
		if !selected[noticeID(n)] || readOnly && !n.Read {
			continue
		}
		if n.ID.Target.Kind == "notification" {
			durable = append(durable, n.ID.Target.Notification)
		} else {
			attention = append(attention, n.ID)
		}
	}
	if len(attention) > 0 {
		r.controller.DismissAttention(attention)
	}
	if len(durable) == 0 {
		return
	}
	command := protocol.Command{Kind: "dismiss_notifications", Data: protocol.CommandDismissNotificationsPayload{IDs: durable}}
	if readOnly {
		command = protocol.Command{Kind: "dismiss_read_notifications", Data: protocol.CommandDismissReadNotificationsPayload{IDs: durable}}
	}
	r.command(command, "notification_update_failed", nil)
}
