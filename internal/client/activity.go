package client

import (
	"encoding/json"
	"fmt"
	"slices"

	"github.com/sailry/sailry-harness/internal/protocol"
)

type NoticeTarget struct {
	Kind         string
	Notification protocol.NotificationID
	Session      protocol.SessionID
	Terminal     protocol.TerminalID
}

type NoticeID struct {
	Node   protocol.NodeID `json:"node"`
	Cursor uint64          `json:"cursor"`
	Target NoticeTarget    `json:"target"`
}

func (target NoticeTarget) MarshalJSON() ([]byte, error) {
	switch target.Kind {
	case "notification":
		return json.Marshal(map[string]any{"Notification": target.Notification})
	case "session":
		return json.Marshal(map[string]any{"Session": target.Session})
	case "terminal":
		return json.Marshal(map[string]any{"Terminal": target.Terminal})
	default:
		return nil, fmt.Errorf("unknown notification target %q", target.Kind)
	}
}

func (target *NoticeTarget) UnmarshalJSON(body []byte) error {
	var fields map[string]json.RawMessage
	if err := json.Unmarshal(body, &fields); err != nil {
		return err
	}
	if len(fields) != 1 {
		return fmt.Errorf("invalid notification target")
	}
	for kind, value := range fields {
		switch kind {
		case "Notification":
			target.Kind = "notification"
			return json.Unmarshal(value, &target.Notification)
		case "Session":
			target.Kind = "session"
			return json.Unmarshal(value, &target.Session)
		case "Terminal":
			target.Kind = "terminal"
			return json.Unmarshal(value, &target.Terminal)
		}
	}
	return fmt.Errorf("invalid notification target")
}

type Notice struct {
	ID               NoticeID                           `json:"id"`
	Project          *protocol.ProjectID                `json:"project"`
	Worktree         *protocol.WorktreeID               `json:"worktree"`
	Title            string                             `json:"title"`
	Message          string                             `json:"message"`
	Error            *protocol.Fault                    `json:"error"`
	Presentation     *protocol.NotificationPresentation `json:"presentation"`
	Kind             string                             `json:"-"`
	NotificationKind protocol.NotificationKind          `json:"-"`
	Read             bool                               `json:"read"`
	Package          string                             `json:"package"`
	Session          *protocol.SessionID                `json:"session"`
	TimestampMs      *int64                             `json:"timestamp_ms"`
	Details          *protocol.NotificationDetails      `json:"details"`
}

func (notice Notice) MarshalJSON() ([]byte, error) {
	type plain Notice
	var kind any
	switch notice.Kind {
	case "notification":
		kind = map[string]any{"Notification": notice.NotificationKind}
	case "completed":
		kind = "Completed"
	case "failed":
		kind = "Failed"
	case "approval":
		kind = "Approval"
	case "input":
		kind = "Input"
	default:
		return nil, fmt.Errorf("unknown notification kind %q", notice.Kind)
	}
	return json.Marshal(struct {
		plain
		Kind any `json:"kind"`
	}{plain(notice), kind})
}

func (notice Notice) Target() NoticeTarget {
	if notice.Session != nil {
		return NoticeTarget{Kind: "session", Session: *notice.Session}
	}
	if notice.Details != nil && notice.Details.Kind == "terminal" {
		if terminal, err := payload[protocol.NotificationDetailsTerminal](notice.Details.Data); err == nil {
			return NoticeTarget{Kind: "terminal", Terminal: terminal.ID}
		}
	}
	return notice.ID.Target
}

// Notices includes the durable inbox and transient attention transitions reduced
// before UI invalidations can coalesce. It returns an independent read copy.
func (client *Client) Notices() []Notice {
	client.mu.RLock()
	defer client.mu.RUnlock()
	return copyValue(client.feed.notices)
}

// MarkAttentionRead changes only controller-local attention. Durable notices use
// the ordinary Node notification commands and await their committed events.
func (client *Client) MarkAttentionRead(id NoticeID) {
	client.mu.Lock()
	changed := false
	for index := range client.feed.notices {
		notice := &client.feed.notices[index]
		if notice.ID == id && notice.ID.Target.Kind != "notification" && !notice.Read {
			notice.Read, changed = true, true
		}
	}
	client.mu.Unlock()
	if changed {
		client.changed()
	}
}

func (client *Client) DismissAttention(ids []NoticeID) {
	client.mu.Lock()
	before := len(client.feed.notices)
	client.feed.notices = slices.DeleteFunc(client.feed.notices, func(notice Notice) bool {
		return notice.ID.Target.Kind != "notification" && slices.Contains(ids, notice.ID)
	})
	changed := len(client.feed.notices) != before
	client.mu.Unlock()
	if changed {
		client.changed()
	}
}

type activityFeed struct {
	primed   bool
	sessions map[protocol.SessionID]protocol.ActivitySummary
	notices  []Notice
}

func (feed *activityFeed) observe(snapshot protocol.Snapshot) {
	feed.notices = slices.DeleteFunc(feed.notices, func(notice Notice) bool {
		return notice.ID.Target.Kind == "notification" && !slices.ContainsFunc(snapshot.Notifications, func(entry protocol.NotificationNotice) bool { return entry.ID == notice.ID.Target.Notification })
	})
	for index := len(snapshot.Notifications) - 1; index >= 0; index-- {
		entry := snapshot.Notifications[index]
		id := NoticeID{Node: snapshot.Node, Cursor: entry.Sequence, Target: NoticeTarget{Kind: "notification", Notification: entry.ID}}
		existing := slices.IndexFunc(feed.notices, func(notice Notice) bool { return notice.ID == id })
		if existing >= 0 {
			feed.notices[existing].Read = entry.Read
			continue
		}
		notice := Notice{ID: id, Title: entry.Content.Title, Message: entry.Content.Message,
			Presentation: entry.Content.Presentation, Kind: "notification", NotificationKind: entry.Content.Kind,
			Read: entry.Read, Package: entry.Package, Session: entry.Content.Session, TimestampMs: &entry.TimestampMs, Details: entry.Content.Details}
		if entry.Content.Session != nil {
			for _, session := range snapshot.Sessions {
				if session.ID == *entry.Content.Session {
					notice.Project, notice.Worktree = session.Project, &session.Worktree
					break
				}
			}
		}
		if entry.Content.Details != nil {
			switch entry.Content.Details.Kind {
			case "task":
				if task, err := payload[protocol.NotificationDetailsTask](entry.Content.Details.Data); err == nil {
					notice.Kind = "failed"
					if task.Status == protocol.ConversationStatusCompleted {
						notice.Kind = "completed"
					}
					if task.Error != nil {
						notice.Error = &protocol.Fault{Code: *task.Error}
					}
				}
			case "terminal":
				if terminal, err := payload[protocol.NotificationDetailsTerminal](entry.Content.Details.Data); err == nil {
					notice.Kind = "completed"
					if terminal.Failed {
						notice.Kind = "failed"
					}
					if notice.Worktree == nil {
						for _, info := range snapshot.Terminals {
							if info.ID == terminal.ID {
								notice.Worktree = info.Worktree
								break
							}
						}
					}
				}
			}
		}
		if notice.Project == nil && notice.Worktree != nil {
			for _, worktree := range snapshot.Worktrees {
				if worktree.ID == *notice.Worktree {
					notice.Project = worktree.Project
					break
				}
			}
		}
		feed.notices = append([]Notice{notice}, feed.notices...)
	}
	if feed.sessions == nil {
		feed.sessions = make(map[protocol.SessionID]protocol.ActivitySummary)
	}
	present := make(map[protocol.SessionID]struct{}, len(snapshot.Sessions))
	for _, session := range snapshot.Sessions {
		present[session.ID] = struct{}{}
		if session.Delegation != nil {
			continue
		}
		activity := session.Activity
		previous, exists := feed.sessions[session.ID]
		feed.sessions[session.ID] = activity
		if !feed.primed || !exists || activity.Run == nil {
			continue
		}
		if sameAttention(previous, activity) || previous.Run != nil && previous.Run.Sequence > activity.Run.Sequence {
			continue
		}
		if activity.Run.Status != protocol.ConversationStatusRunning || activity.Waiting == nil {
			continue
		}
		kind := "input"
		if *activity.Waiting == protocol.ActivityWaitingApproval {
			kind = "approval"
		}
		notice := Notice{ID: NoticeID{Node: snapshot.Node, Cursor: snapshot.Cursor, Target: NoticeTarget{Kind: "session", Session: session.ID}},
			Project: session.Project, Worktree: &session.Worktree, Title: activity.Title, Kind: kind, Session: &session.ID}
		feed.notices = append([]Notice{notice}, feed.notices...)
	}
	for id := range feed.sessions {
		if _, exists := present[id]; !exists {
			delete(feed.sessions, id)
		}
	}
	attention := 0
	feed.notices = slices.DeleteFunc(feed.notices, func(notice Notice) bool {
		if notice.ID.Target.Kind == "notification" {
			return false
		}
		attention++
		return attention > protocol.NotificationLimit
	})
	feed.primed = true
}

func sameAttention(a, b protocol.ActivitySummary) bool {
	if a.Run == nil || b.Run == nil {
		return a.Run == nil && b.Run == nil
	}
	if a.Run.Turn != b.Run.Turn || a.Run.Status != b.Run.Status {
		return false
	}
	if a.Waiting == nil || b.Waiting == nil {
		return a.Waiting == nil && b.Waiting == nil
	}
	return *a.Waiting == *b.Waiting
}
