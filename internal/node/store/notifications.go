package store

import (
	"context"
	"database/sql"
	"strings"
	"time"

	"github.com/sailry/sailry-harness/internal/protocol"
)

func ReadNotifications(ctx context.Context, db *sql.Conn) ([]protocol.NotificationNotice, error) {
	return readBodies[protocol.NotificationNotice](ctx, db, "SELECT body FROM notifications ORDER BY sequence DESC LIMIT 200")
}

func putNotice(ctx context.Context, db *sql.Conn, value protocol.NotificationNotice) error {
	body, err := encode(value)
	if err != nil {
		return err
	}
	_, err = db.ExecContext(ctx, "UPDATE notifications SET body=?2 WHERE id=?1", value.ID.String(), body)
	return err
}

func notificationCommand(ctx context.Context, db *sql.Conn, command protocol.Command) (protocol.Output, []protocol.Event, error) {
	switch input := command.Data.(type) {
	case *protocol.CommandPublishNotificationPayload:
		info, err := readBody[protocol.PluginInfo](ctx, db, "SELECT body FROM plugins WHERE name=?1 AND installed=1", input.Package.Name)
		if err != nil {
			return protocol.Output{}, nil, err
		}
		if info == nil {
			return protocol.Output{}, nil, fault(protocol.ErrorCodeNotFound, "notification plugin is unavailable")
		}
		summary := info.Summary
		if summary.Name != input.Package.Name || summary.Digest != input.Package.Digest || summary.SettingsRevision != input.Package.SettingsRevision {
			return protocol.Output{}, nil, fault(protocol.ErrorCodeRevisionConflict, "notification plugin changed")
		}
		if !summary.Enabled || summary.Compatibility.Kind != "compatible" {
			return protocol.Output{}, nil, fault(protocol.ErrorCodeNotConfigured, "notification plugin is disabled")
		}
		content := input.Content
		if strings.TrimSpace(content.Title) == "" || len(content.Title) > 512 || len(content.Message) > 4096 {
			return protocol.Output{}, nil, invalid("invalid notification content")
		}
		if p := content.Presentation; p != nil {
			if strings.TrimSpace(p.Heading) == "" || len(p.Heading) > 128 || len(p.Locales) > 32 {
				return protocol.Output{}, nil, invalid("invalid notification presentation")
			}
			for locale, heading := range p.Locales {
				valid := locale != "" && len(locale) <= 32
				for _, b := range []byte(locale) {
					if !((b >= 'a' && b <= 'z') || (b >= 'A' && b <= 'Z') || (b >= '0' && b <= '9') || b == '-') {
						valid = false
					}
				}
				if !valid || strings.TrimSpace(heading) == "" || len(heading) > 128 {
					return protocol.Output{}, nil, invalid("invalid notification presentation")
				}
			}
		}
		if content.Session != nil {
			if _, err := ReadSession(ctx, db, *content.Session); err != nil {
				return protocol.Output{}, nil, err
			}
		}
		if details := content.Details; details != nil {
			value, ok := details.Data.(*protocol.NotificationDetailsTextPayload)
			if details.Kind != "text" || !ok || strings.TrimSpace(value.Body) == "" || len(value.Body) > 8192 {
				return protocol.Output{}, nil, invalid("notification details must be bounded text")
			}
		}
		sequence, err := NextCounter(ctx, db, CounterNotifications)
		if err != nil {
			return protocol.Output{}, nil, err
		}
		value := protocol.NotificationNotice{ID: protocol.NewNotificationID(), Sequence: sequence, Package: input.Package.Name, Content: content, TimestampMs: time.Now().UnixMilli()}
		body, err := encode(value)
		if err != nil {
			return protocol.Output{}, nil, err
		}
		if _, err := db.ExecContext(ctx, "INSERT INTO notifications(sequence,id,body) VALUES(?1,?2,?3)", sequence, value.ID.String(), body); err != nil {
			return protocol.Output{}, nil, err
		}
		_, err = db.ExecContext(ctx, "DELETE FROM notifications WHERE sequence NOT IN (SELECT sequence FROM notifications ORDER BY sequence DESC LIMIT 200)")
		return protocol.Output{Kind: "notification", Data: value}, []protocol.Event{{Kind: "notification_changed", Data: value}}, err
	case *protocol.CommandMarkNotificationReadPayload:
		value, err := readBody[protocol.NotificationNotice](ctx, db, "SELECT body FROM notifications WHERE id=?1", input.ID.String())
		if err != nil {
			return protocol.Output{}, nil, err
		}
		if value == nil {
			return protocol.Output{}, nil, fault(protocol.ErrorCodeNotFound, "notification is unavailable")
		}
		if value.Read {
			return protocol.Output{Kind: "notification", Data: *value}, nil, nil
		}
		value.Read = true
		if err := putNotice(ctx, db, *value); err != nil {
			return protocol.Output{}, nil, err
		}
		return protocol.Output{Kind: "notification", Data: *value}, []protocol.Event{{Kind: "notification_changed", Data: *value}}, nil
	}
	var ids []protocol.NotificationID
	switch input := command.Data.(type) {
	case *protocol.CommandMarkNotificationsReadPayload:
		ids = input.IDs
	case *protocol.CommandDismissReadNotificationsPayload:
		ids = input.IDs
	case *protocol.CommandDismissNotificationsPayload:
		ids = input.IDs
	}
	if len(ids) > protocol.NotificationLimit {
		return protocol.Output{}, nil, invalid("too many notifications")
	}
	changed := []protocol.NotificationID{}
	for _, id := range ids {
		if command.Kind == "dismiss_notifications" {
			if _, err := db.ExecContext(ctx, "DELETE FROM notifications WHERE id=?1", id.String()); err != nil {
				return protocol.Output{}, nil, err
			}
			continue
		}
		value, err := readBody[protocol.NotificationNotice](ctx, db, "SELECT body FROM notifications WHERE id=?1", id.String())
		if err != nil {
			return protocol.Output{}, nil, err
		}
		if value == nil {
			continue
		}
		if command.Kind == "mark_notifications_read" && !value.Read {
			value.Read = true
			if err := putNotice(ctx, db, *value); err != nil {
				return protocol.Output{}, nil, err
			}
			changed = append(changed, id)
		}
		if command.Kind == "dismiss_read_notifications" && value.Read {
			if _, err := db.ExecContext(ctx, "DELETE FROM notifications WHERE id=?1", id.String()); err != nil {
				return protocol.Output{}, nil, err
			}
			changed = append(changed, id)
		}
	}
	output := protocol.Output{Kind: "notifications_updated"}
	if command.Kind == "dismiss_notifications" {
		output.Kind = "notifications_dismissed"
		changed = ids
	}
	if len(changed) == 0 {
		return output, nil, nil
	}
	if command.Kind == "mark_notifications_read" {
		return output, []protocol.Event{{Kind: "notifications_read", Data: protocol.EventNotificationsReadPayload{IDs: changed}}}, nil
	}
	return output, []protocol.Event{{Kind: "notifications_dismissed", Data: protocol.EventNotificationsDismissedPayload{IDs: changed}}}, nil
}
