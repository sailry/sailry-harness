package live

import (
	"encoding/hex"
	"encoding/json"
	"runtime"
	"slices"
	"strings"
	"time"

	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/desktop/host"
	"github.com/sailry/sailry-harness/internal/desktop/preferences"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// Sidebar projects the Client snapshot without fixture data or mutable authority.
func Sidebar(snapshot protocol.Snapshot, prefs preferences.Data) *viewmodel.Data {
	owner := hex.EncodeToString(snapshot.Node[:])
	d := &viewmodel.Data{Workspace: &viewmodel.Workspace{Hosts: []viewmodel.Host{{ID: owner, Name: "local_host_name", Local: true}}}}
	projects := map[protocol.ProjectID]*viewmodel.Project{}
	var pinOrders []uint64
	if prefs.Pins != nil {
		for key, pin := range *prefs.Pins {
			if strings.HasPrefix(key, preferences.PinKey(snapshot.Node, "")) && pin.Pinned && pin.Order != nil {
				pinOrders = append(pinOrders, *pin.Order)
			}
		}
	}
	slices.Sort(pinOrders)
	pinOrders = slices.Compact(pinOrders)
	for _, p := range snapshot.Projects {
		pr := &viewmodel.Project{ID: p.ID.String(), Name: p.Name, Path: p.Path, Host: owner, Icon: p.Appearance.Icon, Color: p.Appearance.Color}
		for _, w := range snapshot.Worktrees {
			if w.Project != nil && *w.Project == p.ID {
				pr.Worktrees = append(pr.Worktrees, viewmodel.Worktree{Path: w.Path, Main: w.Main})
			}
		}
		projects[p.ID] = pr
		d.Projects = append(d.Projects, pr)
	}
	// Projects and sessions keep the Node's order; the sidebar sorts them by
	// the session order preference (sidebar.Sorted).
	for _, s := range snapshot.Sessions {
		if s.Delegation != nil {
			continue
		}
		if s.Project != nil && projects[*s.Project] == nil {
			continue
		}
		cv := &viewmodel.Conversation{Node: owner, ID: s.ID.String(), Title: s.Activity.Title, Archived: s.Archived, Created: int(s.CreationOrder), Revision: s.Revision, AttentionRevision: s.Activity.Attention.Revision}
		if last := lastRun(s); last != nil {
			cv.Updated = time.UnixMilli(*last)
		}
		switch client.AggregateLane(s, snapshot.Sessions) {
		case client.LaneRunning:
			cv.Status = viewmodel.Running
		case client.LaneWaiting:
			cv.Status = viewmodel.Waiting
		case client.LaneCompleted, client.LaneFailed:
			cv.Status = viewmodel.Unread
		default:
			if s.Activity.Attention.Unread {
				cv.Status = viewmodel.Unread
			}
		}
		pin := prefs.Pin(snapshot.Node, cv.ID)
		cv.Pinned = pin.Pinned && !cv.Archived
		if pin.Order != nil {
			order, _ := slices.BinarySearch(pinOrders, *pin.Order)
			cv.PinOrder = &order
		}
		for _, w := range snapshot.Worktrees {
			if w.ID == s.Worktree {
				cv.WorktreePath = w.Path
			}
		}
		if s.Project == nil {
			d.Unassigned = append(d.Unassigned, cv)
		} else {
			p := projects[*s.Project]
			p.Conversations = append(p.Conversations, cv)
		}
	}
	return d
}

func lastRun(s protocol.Session) *int64 {
	if s.Activity.Run == nil {
		return nil
	}
	if s.Activity.Run.FinishedMs != nil {
		return s.Activity.Run.FinishedMs
	}
	return s.Activity.Run.StartedMs
}

// Notices preserves typed details and generates the bounded technical report.
func Notices(list []client.Notice) []*viewmodel.Notice {
	out := make([]*viewmodel.Notice, 0, len(list))
	for _, n := range list {
		id := noticeID(n)
		kind := viewmodel.NoticeInfo
		switch n.Kind {
		case "completed":
			kind = viewmodel.NoticeCompleted
		case "failed":
			kind = viewmodel.NoticeFailed
		case "approval":
			kind = viewmodel.NoticeApproval
		case "input":
			kind = viewmodel.NoticeInput
		}
		if n.Kind == "notification" {
			switch n.NotificationKind {
			case protocol.NotificationKindSuccess:
				kind = viewmodel.NoticeCompleted
			case protocol.NotificationKindError:
				kind = viewmodel.NoticeFailed
			}
		}
		v := &viewmodel.Notice{Node: n.ID.Node.String(), ID: id, Kind: kind, Title: n.Title, Message: n.Message, Host: "local_host_name", Read: n.Read}
		v.TitleKey, v.Presentation = headingKey(n), n.Presentation
		if n.TimestampMs != nil {
			v.Time = time.UnixMilli(*n.TimestampMs)
		}
		if n.Session != nil {
			v.Session = n.Session.String()
		}
		if n.Details != nil {
			v.Details = n.Details
			if n.Details.Kind == "text" {
				if p, ok := detail[protocol.NotificationDetailsTextPayload](n.Details.Data); ok {
					v.DetailText = p.Body
					// A notice whose text is one knowledge citation opens it.
					if link := strings.TrimSpace(p.Body); knowledgeLink(link) {
						v.Link = link
					}
				}
			} else {
				report := struct {
					Version      int                           `json:"version"`
					Notification reportID                      `json:"notification"`
					TimestampMs  *int64                        `json:"timestamp_ms"`
					Session      *protocol.SessionID           `json:"session"`
					Controller   map[string]string             `json:"controller"`
					Details      *protocol.NotificationDetails `json:"details"`
				}{1, technicalID(n.ID), n.TimestampMs, n.Session, map[string]string{"version": "0.1.0", "platform": runtime.GOOS + "-" + runtime.GOARCH}, n.Details}
				if b, err := json.MarshalIndent(report, "", "  "); err == nil {
					v.Report = string(b)
				}
			}
		}
		out = append(out, v)
	}
	slices.SortStableFunc(out, func(a, b *viewmodel.Notice) int {
		if a.Time.IsZero() && !b.Time.IsZero() {
			return -1
		}
		if !a.Time.IsZero() && b.Time.IsZero() {
			return 1
		}
		return b.Time.Compare(a.Time)
	})
	return out
}

func noticeID(n client.Notice) string {
	if n.ID.Target.Kind == "notification" {
		return n.ID.Target.Notification.String()
	}
	b, _ := json.Marshal(n.ID)
	return string(b)
}

type reportID struct {
	Node   protocol.NodeID `json:"node"`
	Cursor uint64          `json:"cursor"`
	Target map[string]any  `json:"target"`
}

func technicalID(id client.NoticeID) reportID {
	target := map[string]any{}
	switch id.Target.Kind {
	case "notification":
		target["Notification"] = id.Target.Notification
	case "session":
		target["Session"] = id.Target.Session
	case "terminal":
		target["Terminal"] = id.Target.Terminal
	}
	return reportID{id.Node, id.Cursor, target}
}

func headingKey(n client.Notice) string {
	if n.Details != nil {
		switch n.Details.Kind {
		case "power":
			return "power_request_failed"
		case "diagnostic":
			if diagnostic, ok := detail[protocol.NotificationDetailsDiagnostic](n.Details.Data); ok && diagnostic.Cause.Kind == "compaction" {
				if failure, ok := detail[protocol.ConversationCompactionFailure](diagnostic.Cause.Data); ok && failure.Source == "automatic" {
					return "compact_auto_failed"
				}
				return "chat_compact_failed"
			}
			return "activity_failed"
		case "system":
			if report, ok := detail[protocol.NotificationSystemSystemDiagnostic](n.Details.Data); ok {
				if report.Code == protocol.ErrorCodeOutcomeUnknown {
					return "chat_execution_unknown"
				}
				if report.Reason == "cancelled" || report.Code == protocol.ErrorCodeCancelled {
					return "turn_cancelled"
				}
				if report.Reason == "timeout" {
					return "plugin_response_timeout"
				}
				switch report.Source {
				case "plugin_metadata":
					return "plugin_metadata_failed"
				case "plugin_initialization":
					return "plugin_initialization_failed"
				case "git_inspection":
					return "git_inspection_failed"
				case "plugin_callback":
					return "plugin_callback_failed"
				}
			}
		}
	}
	if strings.TrimSpace(n.Title) != "" {
		return ""
	}
	switch n.Target().Kind {
	case "session":
		return "conversation"
	case "terminal":
		return "terminal"
	default:
		return "notifications"
	}
}

func detail[T any](data any) (T, bool) {
	if v, ok := data.(T); ok {
		return v, true
	}
	if v, ok := data.(*T); ok && v != nil {
		return *v, true
	}
	var zero T
	return zero, false
}

func localHost(snapshot protocol.Snapshot, d *viewmodel.Data) *host.Host {
	id := hex.EncodeToString(snapshot.Node[:])
	osName := runtime.GOOS
	if osName == "darwin" {
		osName = "macos"
	}
	h := &host.Host{Live: true, ID: id, Name: "local_host_name", Local: true, OS: osName, Runtime: host.Runtime{Uptime: -1, ProtocolMin: 1, ProtocolMax: 1, Known: true}}
	for _, p := range d.Projects {
		for _, cv := range p.Conversations {
			if cv.Status == viewmodel.Running || cv.Status == viewmodel.Waiting {
				h.Activity = append(h.Activity, host.Activity{Session: cv.ID, Title: cv.Title, Location: p.Name, Waiting: cv.Status == viewmodel.Waiting})
			}
		}
	}
	return h
}
