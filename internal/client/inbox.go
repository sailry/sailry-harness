package client

import (
	"bytes"
	"context"
	"errors"
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
	pending []protocol.Request
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
		if output.Kind != "notifications_updated" {
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

func notificationRequestIDs(request protocol.Request) []protocol.NotificationID {
	switch input := request.Command.Data.(type) {
	case protocol.CommandMarkNotificationsReadPayload:
		return input.IDs
	case *protocol.CommandMarkNotificationsReadPayload:
		return input.IDs
	}
	return nil
}

func (inbox *Inbox) MarkAllRead(ctx context.Context, clients map[protocol.NodeID]*Client) error {
	inbox.actions.Lock()
	defer inbox.actions.Unlock()
	inbox.mu.Lock()
	nodes := map[protocol.NodeID][]protocol.NotificationID{}
	local := 0
	for index := range inbox.notices {
		notice := &inbox.notices[index]
		if notice.Read {
			continue
		}
		if notice.ID.Target.Kind != "notification" {
			notice.Read = true
			local++
			continue
		}
		pending := slices.ContainsFunc(inbox.pending, func(request protocol.Request) bool {
			return request.Target == notice.ID.Node && slices.Contains(notificationRequestIDs(request), notice.ID.Target.Notification)
		})
		if !pending {
			nodes[notice.ID.Node] = append(nodes[notice.ID.Node], notice.ID.Target.Notification)
		}
	}
	ordered := make([]protocol.NodeID, 0, len(nodes))
	for node := range nodes {
		ordered = append(ordered, node)
	}
	slices.SortFunc(ordered, func(first, second protocol.NodeID) int { return bytes.Compare(first[:], second[:]) })
	for _, node := range ordered {
		inbox.pending = append(inbox.pending, protocol.NewRequest(node, protocol.Command{Kind: "mark_notifications_read", Data: protocol.CommandMarkNotificationsReadPayload{IDs: nodes[node]}}))
	}
	requests := copyValue(inbox.pending)
	inbox.mu.Unlock()
	var failures []error
	completed := local
	for _, request := range requests {
		controller := clients[request.Target]
		if controller == nil {
			failures = append(failures, fault(protocol.ErrorCodeUnavailable, "notification Node is unavailable"))
			continue
		}
		output, err := controller.Execute(ctx, request)
		if err == nil && output.Kind != "notifications_updated" {
			err = fault(protocol.ErrorCodeInternal, "notification update response expected")
		}
		inbox.mu.Lock()
		if faultCode(err) != protocol.ErrorCodeOutcomeUnknown {
			inbox.pending = slices.DeleteFunc(inbox.pending, func(pending protocol.Request) bool { return pending.ID == request.ID })
		}
		if err == nil {
			completed++
			for index := range inbox.notices {
				notice := &inbox.notices[index]
				if notice.ID.Node == request.Target && notice.ID.Target.Kind == "notification" && slices.Contains(notificationRequestIDs(request), notice.ID.Target.Notification) {
					notice.Read = true
				}
			}
		}
		inbox.mu.Unlock()
		if err != nil {
			failures = append(failures, err)
		}
	}
	if len(failures) == 0 {
		return nil
	}
	selected := failures[0]
	for _, err := range failures {
		if faultCode(err) == protocol.ErrorCodeOutcomeUnknown {
			selected = err
			break
		}
	}
	if completed == 0 {
		return selected
	}
	var detail protocol.Fault
	var pointer *protocol.Fault
	switch {
	case errors.As(selected, &pointer):
		detail = *pointer
	default:
		detail = protocol.Fault{Code: faultCode(selected), Message: selected.Error()}
	}
	detail.Message = "some notifications could not be updated: " + detail.Message
	return &detail
}
