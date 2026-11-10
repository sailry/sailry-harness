package client

import (
	"cmp"
	"slices"

	"github.com/sailry/sailry-harness/internal/protocol"
)

type ActivityLane string

const (
	ActivityIdle      ActivityLane = "idle"
	ActivityRunning   ActivityLane = "running"
	ActivityWaiting   ActivityLane = "waiting"
	ActivityCompleted ActivityLane = "completed"
	ActivityFailed    ActivityLane = "failed"
)

type TerminalActivityLane struct {
	Terminal protocol.TerminalID
	Lane     ActivityLane
}

type ActivityProjection struct {
	SessionLanes  map[string]ActivityLane `json:"session_lanes"`
	TerminalLanes map[string]ActivityLane `json:"terminal_lanes"`
	ActivityOrder []string                `json:"activity_order"`
}

func SessionLane(session protocol.Session) ActivityLane {
	return activityLane(LaneOf(session))
}

func activityLane(lane Lane) ActivityLane {
	switch lane {
	case LaneRunning:
		return ActivityRunning
	case LaneWaiting:
		return ActivityWaiting
	case LaneCompleted:
		return ActivityCompleted
	case LaneFailed:
		return ActivityFailed
	default:
		return ActivityIdle
	}
}

func ActivityPrecedence(lane ActivityLane) uint8 {
	switch lane {
	case ActivityWaiting:
		return 0
	case ActivityCompleted, ActivityFailed:
		return 1
	case ActivityRunning:
		return 2
	default:
		return 3
	}
}

func ActivityOrder(sessions []protocol.Session, terminals []TerminalActivityLane) []NoticeTarget {
	type item struct {
		precedence uint8
		recent     *int64
		target     NoticeTarget
	}
	items := make([]item, 0, len(sessions)+len(terminals))
	for _, session := range sessions {
		var recent *int64
		if run := session.Activity.Run; run != nil {
			recent = run.FinishedMs
			if recent == nil {
				recent = run.StartedMs
			}
		}
		items = append(items, item{
			precedence: ActivityPrecedence(activityLane(AggregateLane(session, sessions))), recent: recent,
			target: NoticeTarget{Kind: "session", Session: session.ID},
		})
	}
	for _, terminal := range terminals {
		items = append(items, item{
			precedence: ActivityPrecedence(terminal.Lane),
			target:     NoticeTarget{Kind: "terminal", Terminal: terminal.Terminal},
		})
	}
	slices.SortStableFunc(items, func(first, second item) int {
		if priority := cmp.Compare(first.precedence, second.precedence); priority != 0 {
			return priority
		}
		if first.precedence > ActivityPrecedence(ActivityCompleted) {
			return 0
		}
		switch {
		case first.recent != nil && second.recent != nil:
			return cmp.Compare(*second.recent, *first.recent)
		case first.recent != nil:
			return -1
		case second.recent != nil:
			return 1
		default:
			return 0
		}
	})
	targets := make([]NoticeTarget, 0, len(items))
	for _, item := range items {
		targets = append(targets, item.target)
	}
	return targets
}

func TerminalLane(info protocol.TerminalInfo) (ActivityLane, bool) {
	switch info.Status.Kind {
	case "running":
		return ActivityRunning, true
	case "exited":
		result, err := payload[protocol.TerminalStatusExitedPayload](info.Status.Data)
		if err == nil && result.Code == 0 {
			return ActivityCompleted, true
		}
		return ActivityFailed, true
	case "failed":
		return ActivityFailed, true
	case "stopped":
		return ActivityIdle, true
	default:
		return ActivityIdle, false
	}
}

func TerminalActivity(info protocol.TerminalInfo) (ActivityLane, bool) {
	if info.Status.Kind != "running" {
		return TerminalLane(info)
	}
	if info.Activity != nil {
		switch info.Activity.State {
		case protocol.TerminalActivityWorking:
			return ActivityRunning, true
		case protocol.TerminalActivityWaitingForInput:
			return ActivityWaiting, true
		case protocol.TerminalActivityCompleted:
			return ActivityCompleted, true
		case protocol.TerminalActivityFailed:
			return ActivityFailed, true
		}
	}
	return ActivityIdle, true
}

func ProjectActivity(catalog protocol.ActivityCatalog, unread []protocol.TerminalID) ActivityProjection {
	projection := ActivityProjection{
		SessionLanes: map[string]ActivityLane{}, TerminalLanes: map[string]ActivityLane{},
		ActivityOrder: []string{},
	}
	for _, session := range catalog.Sessions {
		projection.SessionLanes[session.ID.String()] = activityLane(AggregateLane(session, catalog.Sessions))
	}
	terminals := make([]TerminalActivityLane, 0, len(catalog.Terminals))
	for _, terminal := range catalog.Terminals {
		lane, present := TerminalActivity(terminal)
		if !present {
			continue
		}
		projection.TerminalLanes[terminal.ID.String()] = lane
		if (lane == ActivityCompleted || lane == ActivityFailed) && !slices.Contains(unread, terminal.ID) {
			lane = ActivityIdle
		}
		terminals = append(terminals, TerminalActivityLane{Terminal: terminal.ID, Lane: lane})
	}
	for _, target := range ActivityOrder(catalog.Sessions, terminals) {
		if target.Kind == "session" {
			projection.ActivityOrder = append(projection.ActivityOrder, "session:"+target.Session.String())
		} else if target.Kind == "terminal" {
			projection.ActivityOrder = append(projection.ActivityOrder, "terminal:"+target.Terminal.String())
		}
	}
	return projection
}

func (client *Client) UnreadTerminals() []protocol.TerminalID {
	unread := []protocol.TerminalID{}
	for _, notice := range client.Notices() {
		target := notice.Target()
		if !notice.Read && notice.ID.Node == client.Target() && target.Kind == "terminal" && !slices.Contains(unread, target.Terminal) {
			unread = append(unread, target.Terminal)
		}
	}
	slices.SortFunc(unread, func(first, second protocol.TerminalID) int {
		return slices.Compare(first[:], second[:])
	})
	return unread
}
