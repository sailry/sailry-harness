package client

import (
	"context"
	"slices"
	"sync"

	"github.com/sailry/sailry-harness/internal/protocol"
)

type inboxCursor struct {
	node    protocol.NodeID
	durable bool
}

type Inbox struct {
	mu      sync.Mutex
	actions sync.Mutex
	notices []Notice
	cursors map[inboxCursor]uint64
}

func (inbox *Inbox) Merge(node protocol.NodeID, notices []Notice) []Notice {
	inbox.mu.Lock()
	defer inbox.mu.Unlock()
	if inbox.cursors == nil {
		inbox.cursors = map[inboxCursor]uint64{}
	}
	inbox.notices = slices.DeleteFunc(inbox.notices, func(current Notice) bool {
		return current.ID.Node == node && current.ID.Target.Kind == "notification" && !slices.ContainsFunc(notices, func(notice Notice) bool { return notice.ID == current.ID })
	})
	cursors := make(map[inboxCursor]uint64, len(inbox.cursors))
	for key, cursor := range inbox.cursors {
		cursors[key] = cursor
	}
	added := []Notice{}
	for index := len(notices) - 1; index >= 0; index-- {
		notice := copyValue(notices[index])
		if notice.ID.Node != node {
			continue
		}
		durable := notice.ID.Target.Kind == "notification"
		key := inboxCursor{node: node, durable: durable}
		cursor, exists := cursors[key]
		newer := !exists || notice.ID.Cursor > cursor
		current := slices.IndexFunc(inbox.notices, func(current Notice) bool { return current.ID == notice.ID })
		if current >= 0 {
			if durable {
				inbox.notices[current] = notice
			}
		} else if durable || newer {
			inbox.notices = append([]Notice{notice}, inbox.notices...)
		}
		if newer && !notice.Read && (!durable || exists) {
			added = append(added, notice)
		}
		inbox.cursors[key] = max(inbox.cursors[key], notice.ID.Cursor)
	}
	durable := inboxCursor{node: node, durable: true}
	if _, exists := inbox.cursors[durable]; !exists {
		inbox.cursors[durable] = 0
	}
	attention := 0
	inbox.notices = slices.DeleteFunc(inbox.notices, func(notice Notice) bool {
		if notice.ID.Target.Kind == "notification" {
			return false
		}
		attention++
		return attention > protocol.NotificationLimit
	})
	return added
}

func (inbox *Inbox) Notices() []Notice {
	inbox.mu.Lock()
	defer inbox.mu.Unlock()
	return copyValue(inbox.notices)
}

func (inbox *Inbox) Unread() int {
	inbox.mu.Lock()
	defer inbox.mu.Unlock()
	count := 0
	for _, notice := range inbox.notices {
		if !notice.Read {
			count++
		}
	}
	return count
}

func (inbox *Inbox) MarkRead(ctx context.Context, id NoticeID, clients map[protocol.NodeID]*Client) error {
	inbox.actions.Lock()
	defer inbox.actions.Unlock()
	if id.Target.Kind == "notification" {
		controller := clients[id.Node]
		if controller == nil {
			return fault(protocol.ErrorCodeUnavailable, "notification Node is unavailable")
		}
		output, err := controller.Do(ctx, protocol.Command{Kind: "mark_notification_read", Data: protocol.CommandMarkNotificationReadPayload{ID: id.Target.Notification}})
		if err != nil {
			return err
		}
		// The Node answers a single read with the notice itself.
		if output.Kind != "notification" {
			return fault(protocol.ErrorCodeInternal, "notification update response expected")
		}
	} else if controller := clients[id.Node]; controller != nil {
		controller.MarkAttentionRead(id)
	}
	inbox.mu.Lock()
	for index := range inbox.notices {
		if inbox.notices[index].ID == id {
			inbox.notices[index].Read = true
		}
	}
	inbox.mu.Unlock()
	return nil
}
