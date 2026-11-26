package scheduledtasksnode

import (
	"bytes"
	"encoding/json"
	"maps"
	"regexp"
	"slices"
	"strconv"
	"strings"

	"github.com/google/uuid"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

const (
	storageNamespace    = store.NativeScheduledTasksNamespace
	keyPrefix           = "task/"
	maxNameBytes        = 256
	maxPromptBytes      = 32768
	maxPageItems        = 32
	maxPageBytes        = 128 * 1024
	defaultHistoryLimit = 32
	maxSafeInt64        = (1 << 53) - 1
)

var (
	uuidPattern        = regexp.MustCompile(`^[0-9a-f]{8}(-[0-9a-f]{4}){3}-[0-9a-f]{12}$`)
	queuePattern       = regexp.MustCompile(`^[a-zA-Z0-9._-]{1,128}$`)
	positiveRevPattern = regexp.MustCompile(`^[1-9][0-9]*$`)
)

func fault(code protocol.ErrorCode, msg string) error {
	return &protocol.Fault{Code: code, Message: msg}
}

func newID() string {
	return uuid.Must(uuid.NewV7()).String()
}

func taskKey(id string) (string, error) {
	if !uuidPattern.MatchString(id) {
		return "", fault(protocol.ErrorCodeInvalidRequest, "Invalid task ID")
	}
	return keyPrefix + id, nil
}

func validateNodeID(id *protocol.NodeID) error {
	if id == nil {
		return nil
	}
	if *id == (protocol.NodeID{}) {
		return fault(protocol.ErrorCodeInvalidRequest, "Invalid execution Node")
	}
	return nil
}

func sameNode(a, b protocol.NodeID) bool { return a == b }

func sameProject(a, b *protocol.ProjectID) bool {
	if a == nil && b == nil {
		return true
	}
	if a == nil || b == nil {
		return false
	}
	return *a == *b
}

func sameTeamID(a, b *protocol.TeamSelection) bool {
	if a == nil && b == nil {
		return true
	}
	if a == nil || b == nil {
		return false
	}
	return a.ID == b.ID
}

func isJSONNull(raw json.RawMessage) bool {
	return bytes.Equal(bytes.TrimSpace(raw), []byte("null"))
}

func requireConfig(cfg protocol.SessionConfig) error {
	if cfg.Provider == (protocol.ProviderID{}) || strings.TrimSpace(cfg.Model) == "" {
		return fault(protocol.ErrorCodeNotConfigured, "Task requires an explicit model")
	}
	return nil
}

func normalizeExecution(mode protocol.ConversationExecution, team *protocol.TeamSelection, models map[protocol.RoleID]protocol.RoleModel) (protocol.ConversationExecution, *protocol.TeamSelection, map[protocol.RoleID]protocol.RoleModel, error) {
	if mode == "" {
		mode = protocol.ConversationExecutionTeam
	}
	if mode != protocol.ConversationExecutionModel && mode != protocol.ConversationExecutionTeam {
		return "", nil, nil, fault(protocol.ErrorCodeInvalidRequest, "Invalid task execution")
	}
	var normalizedTeam *protocol.TeamSelection
	if team != nil {
		if team.ID == (protocol.TeamID{}) || !uuidPattern.MatchString(team.ID.String()) {
			return "", nil, nil, fault(protocol.ErrorCodeInvalidRequest, "Invalid task team")
		}
		if team.Revision != nil && *team.Revision == 0 {
			return "", nil, nil, fault(protocol.ErrorCodeInvalidRequest, "Invalid task team")
		}
		// A recurring task chooses an identity; each admitted run freezes its current revision.
		normalizedTeam = &protocol.TeamSelection{ID: team.ID, Revision: nil}
	}
	normalizedModels := map[protocol.RoleID]protocol.RoleModel{}
	if len(models) > 0 {
		normalizedModels = maps.Clone(models)
	}
	if mode == protocol.ConversationExecutionModel && (normalizedTeam != nil || len(normalizedModels) > 0) {
		return "", nil, nil, fault(protocol.ErrorCodeInvalidRequest, "Model execution cannot select a team or member models")
	}
	return mode, normalizedTeam, normalizedModels, nil
}

func normalizeTiming(input protocol.DispatchTiming) (protocol.DispatchTiming, error) {
	switch input.Kind {
	case "once":
		var p protocol.DispatchTimingOncePayload
		switch v := input.Data.(type) {
		case protocol.DispatchTimingOncePayload:
			p = v
		case *protocol.DispatchTimingOncePayload:
			if v == nil {
				return protocol.DispatchTiming{}, fault(protocol.ErrorCodeInvalidRequest, "Invalid task timing")
			}
			p = *v
		default:
			return protocol.DispatchTiming{}, fault(protocol.ErrorCodeInvalidRequest, "Invalid task timing")
		}
		if p.AtMs < 0 || p.AtMs > maxSafeInt64 {
			return protocol.DispatchTiming{}, fault(protocol.ErrorCodeInvalidRequest, "Invalid task timing")
		}
		return protocol.DispatchTiming{Kind: "once", Data: p}, nil

	case "every":
		var p protocol.DispatchTimingEveryPayload
		switch v := input.Data.(type) {
		case protocol.DispatchTimingEveryPayload:
			p = v
		case *protocol.DispatchTimingEveryPayload:
			if v == nil {
				return protocol.DispatchTiming{}, fault(protocol.ErrorCodeInvalidRequest, "Invalid task timing")
			}
			p = *v
		default:
			return protocol.DispatchTiming{}, fault(protocol.ErrorCodeInvalidRequest, "Invalid task timing")
		}
		if p.AnchorMs < 0 || p.AnchorMs > maxSafeInt64 || p.IntervalMs < 1 || p.IntervalMs > maxSafeInt64 {
			return protocol.DispatchTiming{}, fault(protocol.ErrorCodeInvalidRequest, "Invalid task timing")
		}
		return protocol.DispatchTiming{Kind: "every", Data: p}, nil

	case "weekly":
		var p protocol.DispatchTimingWeeklyPayload
		switch v := input.Data.(type) {
		case protocol.DispatchTimingWeeklyPayload:
			p = v
		case *protocol.DispatchTimingWeeklyPayload:
			if v == nil {
				return protocol.DispatchTiming{}, fault(protocol.ErrorCodeInvalidRequest, "Invalid task timing")
			}
			p = *v
		default:
			return protocol.DispatchTiming{}, fault(protocol.ErrorCodeInvalidRequest, "Invalid task timing")
		}
		if p.Timezone == "" || p.Timezone != strings.TrimSpace(p.Timezone) {
			return protocol.DispatchTiming{}, fault(protocol.ErrorCodeInvalidRequest, "Invalid task timing")
		}
		if len(p.Weekdays) == 0 || len(p.Weekdays) > 7 {
			return protocol.DispatchTiming{}, fault(protocol.ErrorCodeInvalidRequest, "Invalid task timing")
		}
		seen := map[byte]bool{}
		days := make(protocol.Bytes, len(p.Weekdays))
		for i, day := range p.Weekdays {
			if day < 1 || day > 7 || seen[day] {
				return protocol.DispatchTiming{}, fault(protocol.ErrorCodeInvalidRequest, "Invalid task timing")
			}
			seen[day] = true
			days[i] = day
		}
		if p.Hour > 23 || p.Minute > 59 {
			return protocol.DispatchTiming{}, fault(protocol.ErrorCodeInvalidRequest, "Invalid task timing")
		}
		slices.Sort(days)
		p.Weekdays = days
		return protocol.DispatchTiming{Kind: "weekly", Data: p}, nil

	default:
		return protocol.DispatchTiming{}, fault(protocol.ErrorCodeInvalidRequest, "Invalid task timing")
	}
}

func normalizeDraft(input protocol.ScheduledTask, localNode protocol.NodeID, capture bool) (protocol.ScheduledTask, error) {
	if input.ID == "" {
		input.ID = newID()
	}
	if _, err := taskKey(input.ID); err != nil {
		return protocol.ScheduledTask{}, err
	}
	trimmedName := strings.TrimSpace(input.Name)
	if trimmedName == "" || len(input.Name) > maxNameBytes ||
		strings.TrimSpace(input.Prompt) == "" || len(input.Prompt) > maxPromptBytes ||
		!queuePattern.MatchString(input.Queue) {
		return protocol.ScheduledTask{}, fault(protocol.ErrorCodeInvalidRequest, "Invalid task content or queue")
	}
	if err := requireConfig(input.Config); err != nil {
		return protocol.ScheduledTask{}, err
	}
	if input.Project == nil && input.Worktree != nil {
		return protocol.ScheduledTask{}, fault(protocol.ErrorCodeInvalidRequest, "Task worktree requires a project")
	}
	if input.Config.Assistant != nil || input.Config.Resource != nil {
		return protocol.ScheduledTask{}, fault(protocol.ErrorCodeInvalidRequest, "Task requires a project or unassigned scope")
	}
	timing, err := normalizeTiming(input.Timing)
	if err != nil {
		return protocol.ScheduledTask{}, err
	}
	execMode, team, models, err := normalizeExecution(input.Execution, input.Team, input.Models)
	if err != nil {
		return protocol.ScheduledTask{}, err
	}
	if err := validateNodeID(input.ExecutionNode); err != nil {
		return protocol.ScheduledTask{}, err
	}
	var execNode *protocol.NodeID
	if input.ExecutionNode != nil {
		cp := *input.ExecutionNode
		execNode = &cp
	} else if capture {
		cp := localNode
		execNode = &cp
	}
	cfg := input.Config
	if cfg.Features == nil {
		cfg.Features = map[string]string{}
	}
	if cfg.Options == nil {
		cfg.Options = map[string]map[string]json.RawMessage{}
	}
	return protocol.ScheduledTask{
		ID:            input.ID,
		ExecutionNode: execNode,
		Name:          trimmedName,
		Prompt:        input.Prompt,
		Project:       input.Project,
		Worktree:      input.Worktree,
		Config:        cfg,
		Execution:     execMode,
		Team:          team,
		Models:        models,
		Timing:        timing,
		Enabled:       input.Enabled,
		Queue:         input.Queue,
	}, nil
}

func parseStoredTask(raw json.RawMessage, id string) (protocol.ScheduledTask, error) {
	var stored protocol.ScheduledTask
	if err := json.Unmarshal(raw, &stored); err != nil {
		return protocol.ScheduledTask{}, fault(protocol.ErrorCodeInvalidRequest, "Invalid stored task")
	}
	if stored.ID != id {
		return protocol.ScheduledTask{}, fault(protocol.ErrorCodeInvalidRequest, "Invalid stored task")
	}
	return normalizeDraft(stored, protocol.NodeID{}, false)
}

// canonicalSnapshot produces deterministic, sorted-key JSON for CAS value
// comparisons across independent source and execution Node revisions.
func canonicalSnapshot(item protocol.ScheduledTask) ([]byte, error) {
	if item.Execution == "" {
		item.Execution = protocol.ConversationExecutionTeam
	}
	if item.Team != nil {
		item.Team = &protocol.TeamSelection{ID: item.Team.ID, Revision: nil}
	}
	if item.Models == nil {
		item.Models = map[protocol.RoleID]protocol.RoleModel{}
	}
	if item.Timing.Kind == "weekly" {
		if normalized, err := normalizeTiming(item.Timing); err == nil {
			item.Timing = normalized
		}
	}
	raw, err := json.Marshal(item)
	if err != nil {
		return nil, err
	}
	dec := json.NewDecoder(bytes.NewReader(raw))
	dec.UseNumber()
	var generic any
	if err := dec.Decode(&generic); err != nil {
		return nil, err
	}
	return json.Marshal(generic)
}

func checkBase(expectedRev uint64, expectedVal *protocol.ScheduledTask, present bool, currentRev uint64, current protocol.ScheduledTask) (uint64, error) {
	if expectedVal != nil {
		if !present {
			return 0, fault(protocol.ErrorCodeRevisionConflict, "Task changed")
		}
		actualBytes, err := canonicalSnapshot(current)
		if err != nil {
			return 0, err
		}
		wantBytes, err := canonicalSnapshot(*expectedVal)
		if err != nil {
			return 0, err
		}
		if !bytes.Equal(actualBytes, wantBytes) {
			return 0, fault(protocol.ErrorCodeRevisionConflict, "Task changed")
		}
		return currentRev, nil
	}
	if currentRev != expectedRev {
		return 0, fault(protocol.ErrorCodeRevisionConflict, "Task changed")
	}
	return currentRev, nil
}

func parseRevisionString(raw string) (uint64, error) {
	if raw == "" {
		return 0, fault(protocol.ErrorCodeInvalidRequest, "Invalid task revision")
	}
	if len(raw) > 1 && raw[0] == '0' {
		return 0, fault(protocol.ErrorCodeInvalidRequest, "Invalid task revision")
	}
	v, err := strconv.ParseUint(raw, 10, 63)
	if err != nil {
		return 0, fault(protocol.ErrorCodeInvalidRequest, "Invalid task revision")
	}
	return v, nil
}

func parseStrictJSONInt64(raw json.RawMessage) (int64, error) {
	trimmed := bytes.TrimSpace(raw)
	if len(trimmed) == 0 || trimmed[0] == '"' || bytes.Equal(trimmed, []byte("null")) {
		return 0, fault(protocol.ErrorCodeInvalidRequest, "Invalid task timing")
	}
	// Reject floats and scientific notation to match JS Number.isSafeInteger.
	if bytes.ContainsAny(trimmed, ".eE") {
		return 0, fault(protocol.ErrorCodeInvalidRequest, "Invalid task timing")
	}
	var v int64
	if err := json.Unmarshal(trimmed, &v); err != nil {
		return 0, fault(protocol.ErrorCodeInvalidRequest, "Invalid task timing")
	}
	return v, nil
}

func parseStrictTiming(raw json.RawMessage) (protocol.DispatchTiming, error) {
	if isJSONNull(raw) {
		return protocol.DispatchTiming{}, fault(protocol.ErrorCodeInvalidRequest, "Invalid task timing")
	}
	var outer map[string]json.RawMessage
	if err := json.Unmarshal(raw, &outer); err != nil || outer == nil {
		return protocol.DispatchTiming{}, fault(protocol.ErrorCodeInvalidRequest, "Invalid task timing")
	}
	for k := range outer {
		if k != "kind" && k != "data" {
			return protocol.DispatchTiming{}, fault(protocol.ErrorCodeInvalidRequest, "Invalid task timing")
		}
	}
	kindRaw, hasKind := outer["kind"]
	dataRaw, hasData := outer["data"]
	if !hasKind || !hasData || isJSONNull(kindRaw) || isJSONNull(dataRaw) {
		return protocol.DispatchTiming{}, fault(protocol.ErrorCodeInvalidRequest, "Invalid task timing")
	}
	var kind string
	if err := json.Unmarshal(kindRaw, &kind); err != nil {
		return protocol.DispatchTiming{}, fault(protocol.ErrorCodeInvalidRequest, "Invalid task timing")
	}
	var data map[string]json.RawMessage
	if err := json.Unmarshal(dataRaw, &data); err != nil || data == nil {
		return protocol.DispatchTiming{}, fault(protocol.ErrorCodeInvalidRequest, "Invalid task timing")
	}
	switch kind {
	case "once":
		if len(data) != 1 {
			return protocol.DispatchTiming{}, fault(protocol.ErrorCodeInvalidRequest, "Invalid task timing")
		}
		atRaw, ok := data["at_ms"]
		if !ok {
			return protocol.DispatchTiming{}, fault(protocol.ErrorCodeInvalidRequest, "Invalid task timing")
		}
		atMs, err := parseStrictJSONInt64(atRaw)
		if err != nil {
			return protocol.DispatchTiming{}, err
		}
		return normalizeTiming(protocol.DispatchTiming{
			Kind: "once",
			Data: protocol.DispatchTimingOncePayload{AtMs: atMs},
		})

	case "every":
		if len(data) != 2 {
			return protocol.DispatchTiming{}, fault(protocol.ErrorCodeInvalidRequest, "Invalid task timing")
		}
		anchorRaw, ok1 := data["anchor_ms"]
		intervalRaw, ok2 := data["interval_ms"]
		if !ok1 || !ok2 {
			return protocol.DispatchTiming{}, fault(protocol.ErrorCodeInvalidRequest, "Invalid task timing")
		}
		anchorMs, err := parseStrictJSONInt64(anchorRaw)
		if err != nil {
			return protocol.DispatchTiming{}, err
		}
		intervalMs, err := parseStrictJSONInt64(intervalRaw)
		if err != nil || intervalMs < 1 {
			return protocol.DispatchTiming{}, fault(protocol.ErrorCodeInvalidRequest, "Invalid task timing")
		}
		return normalizeTiming(protocol.DispatchTiming{
			Kind: "every",
			Data: protocol.DispatchTimingEveryPayload{AnchorMs: anchorMs, IntervalMs: uint64(intervalMs)},
		})

	case "weekly":
		if len(data) != 4 {
			return protocol.DispatchTiming{}, fault(protocol.ErrorCodeInvalidRequest, "Invalid task timing")
		}
		tzRaw, ok1 := data["timezone"]
		daysRaw, ok2 := data["weekdays"]
		hourRaw, ok3 := data["hour"]
		minRaw, ok4 := data["minute"]
		if !ok1 || !ok2 || !ok3 || !ok4 || isJSONNull(tzRaw) || isJSONNull(daysRaw) {
			return protocol.DispatchTiming{}, fault(protocol.ErrorCodeInvalidRequest, "Invalid task timing")
		}
		var tz string
		if err := json.Unmarshal(tzRaw, &tz); err != nil {
			return protocol.DispatchTiming{}, fault(protocol.ErrorCodeInvalidRequest, "Invalid task timing")
		}
		var dayElems []json.RawMessage
		if err := json.Unmarshal(daysRaw, &dayElems); err != nil || dayElems == nil {
			return protocol.DispatchTiming{}, fault(protocol.ErrorCodeInvalidRequest, "Invalid task timing")
		}
		days := make(protocol.Bytes, len(dayElems))
		for i, elem := range dayElems {
			v, err := parseStrictJSONInt64(elem)
			if err != nil || v < 1 || v > 7 {
				return protocol.DispatchTiming{}, fault(protocol.ErrorCodeInvalidRequest, "Invalid task timing")
			}
			days[i] = byte(v)
		}
		hour, err := parseStrictJSONInt64(hourRaw)
		if err != nil || hour < 0 || hour > 23 {
			return protocol.DispatchTiming{}, fault(protocol.ErrorCodeInvalidRequest, "Invalid task timing")
		}
		minute, err := parseStrictJSONInt64(minRaw)
		if err != nil || minute < 0 || minute > 59 {
			return protocol.DispatchTiming{}, fault(protocol.ErrorCodeInvalidRequest, "Invalid task timing")
		}
		return normalizeTiming(protocol.DispatchTiming{
			Kind: "weekly",
			Data: protocol.DispatchTimingWeeklyPayload{
				Timezone: tz,
				Weekdays: days,
				Hour:     uint8(hour),
				Minute:   uint8(minute),
			},
		})

	default:
		return protocol.DispatchTiming{}, fault(protocol.ErrorCodeInvalidRequest, "Invalid task timing")
	}
}

func parseStrictTeam(raw json.RawMessage) (*protocol.TeamSelection, error) {
	if isJSONNull(raw) {
		return nil, nil
	}
	var obj map[string]json.RawMessage
	if err := json.Unmarshal(raw, &obj); err != nil || obj == nil {
		return nil, fault(protocol.ErrorCodeInvalidRequest, "Invalid task team")
	}
	for k := range obj {
		if k != "id" && k != "revision" {
			return nil, fault(protocol.ErrorCodeInvalidRequest, "Invalid task team")
		}
	}
	idRaw, hasID := obj["id"]
	if !hasID || isJSONNull(idRaw) {
		return nil, fault(protocol.ErrorCodeInvalidRequest, "Invalid task team")
	}
	var idStr string
	if err := json.Unmarshal(idRaw, &idStr); err != nil || !uuidPattern.MatchString(idStr) {
		return nil, fault(protocol.ErrorCodeInvalidRequest, "Invalid task team")
	}
	teamID, err := protocol.ParseTeamID(idStr)
	if err != nil {
		return nil, fault(protocol.ErrorCodeInvalidRequest, "Invalid task team")
	}
	if revRaw, hasRev := obj["revision"]; hasRev && !isJSONNull(revRaw) {
		var revStr string
		if err := json.Unmarshal(revRaw, &revStr); err != nil || !positiveRevPattern.MatchString(revStr) {
			return nil, fault(protocol.ErrorCodeInvalidRequest, "Invalid task team")
		}
	}
	return &protocol.TeamSelection{ID: teamID, Revision: nil}, nil
}

func marshalRaw(v any) (json.RawMessage, error) {
	b, err := json.Marshal(v)
	if err != nil {
		return nil, err
	}
	return json.RawMessage(b), nil
}
