package acceptance

import (
	"context"
	"encoding/json"
	"errors"
	"os"
	"path/filepath"
	"reflect"
	"slices"
	"strconv"
	"sync"
	"time"

	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/core"
	"github.com/sailry/sailry-harness/internal/protocol"
)

const (
	MaxRecordingFrames     = 256
	MaxRecordingBytes      = 16 * 1024 * 1024
	recordingFrameBudget   = 7 * 1024 * 1024
	recordingHistoryBudget = 6 * 1024 * 1024
	recordingToolBudget    = 2 * 1024 * 1024
)

type RecordingFrame struct {
	At       time.Duration                 `json:"at"`
	Snapshot protocol.ConversationSnapshot `json:"snapshot"`
}

type RecordingTool struct {
	Reference protocol.ToolDetailReference `json:"reference"`
	Detail    *protocol.ToolDetail         `json:"detail,omitempty"`
	Error     string                       `json:"error,omitempty"`
}

// RecordingSession contains only a test-owned conversation's public history,
// snapshots and original tool details. It never serializes a Node snapshot,
// SessionConfig, turn capture, provider settings or credential configuration.
// Truncated includes omitted history, tool details and frames. A replay must
// expose that flag rather than claim to reproduce the entire live run.
type RecordingSession struct {
	Version       int                          `json:"version"`
	Node          protocol.NodeID              `json:"node"`
	Session       protocol.SessionID           `json:"session"`
	Captured      time.Time                    `json:"captured"`
	History       protocol.ConversationHistory `json:"history"`
	Tools         []RecordingTool              `json:"tools"`
	Frames        []RecordingFrame             `json:"frames"`
	Truncated     bool                         `json:"truncated"`
	DroppedFrames int                          `json:"dropped_frames"`
}

// Recorder aggregates copied frames in memory. Flush merges them with any
// canonical capture already on disk. Append does no filesystem work.
type Recorder struct {
	mu        sync.Mutex
	directory string
	recording RecordingSession
	bytes     int
}

func RecordingPath(directory string, session protocol.SessionID) string {
	return filepath.Join(directory, "recordings", session.String()+".json")
}

func NewRecorder(directory string, node protocol.NodeID, session protocol.SessionID) (*Recorder, error) {
	if err := requireOwned(directory, node, []protocol.SessionID{session}); err != nil {
		return nil, err
	}
	recording := newRecording(node, session)
	return &Recorder{directory: directory, recording: recording}, nil
}

func newRecording(node protocol.NodeID, session protocol.SessionID) RecordingSession {
	history := protocol.DefaultConversationHistory()
	history.Page.Session = session
	return RecordingSession{Version: 1, Node: node, Session: session, History: history, Tools: []RecordingTool{}, Frames: []RecordingFrame{}}
}

func (recorder *Recorder) Append(frame RecordingFrame) error {
	recorder.mu.Lock()
	defer recorder.mu.Unlock()
	if frame.At < 0 || frame.Snapshot.Node != recorder.recording.Node || frame.Snapshot.Page.Session != recorder.recording.Session {
		return errors.New("acceptance recording frame belongs to another conversation or time")
	}
	frames := recorder.recording.Frames
	if len(frames) > 0 {
		if frame.At < frames[len(frames)-1].At {
			return errors.New("acceptance recording frame time moved backwards")
		}
		if reflect.DeepEqual(frame.Snapshot, frames[len(frames)-1].Snapshot) {
			return nil
		}
	}
	encoded, err := json.Marshal(frame)
	if err != nil {
		return err
	}
	if len(encoded) > recordingFrameBudget {
		recorder.recording.Truncated = true
		recorder.recording.DroppedFrames++
		return nil
	}
	var copied RecordingFrame
	if err := json.Unmarshal(encoded, &copied); err != nil {
		return err
	}
	recorder.recording.Frames = append(frames, copied)
	recorder.bytes += len(encoded) + 1
	for len(recorder.recording.Frames) > MaxRecordingFrames || recorder.bytes > recordingFrameBudget {
		// Keep the start and the latest state. Every omitted intermediate frame
		// is counted, including frames lost to the byte budget before the cap.
		index := 1
		if len(recorder.recording.Frames) <= 2 {
			index = 0
		}
		removed, _ := json.Marshal(recorder.recording.Frames[index])
		recorder.bytes -= len(removed) + 1
		recorder.recording.Frames = slices.Delete(recorder.recording.Frames, index, index+1)
		recorder.recording.Truncated = true
		recorder.recording.DroppedFrames++
	}
	return nil
}

func (recorder *Recorder) Flush() error {
	recorder.mu.Lock()
	defer recorder.mu.Unlock()
	return updateRecording(recorder.directory, recorder.recording.Node, recorder.recording.Session, func(recording *RecordingSession) {
		recording.Frames = slices.Clone(recorder.recording.Frames)
		recording.Truncated = recording.Truncated || recorder.recording.Truncated
		recording.DroppedFrames = recorder.recording.DroppedFrames
	})
}

func AppendRecordingFrame(directory string, node protocol.NodeID, session protocol.SessionID, frame RecordingFrame) error {
	if err := requireOwned(directory, node, []protocol.SessionID{session}); err != nil {
		return err
	}
	recorder, err := NewRecorder(directory, node, session)
	if err != nil {
		return err
	}
	previous, err := ReadRecording(directory, session)
	if err == nil {
		recorder.recording = previous
		for _, frame := range previous.Frames {
			encoded, _ := json.Marshal(frame)
			recorder.bytes += len(encoded) + 1
		}
	} else if !errors.Is(err, os.ErrNotExist) {
		return err
	}
	if err := recorder.Append(frame); err != nil {
		return err
	}
	return recorder.Flush()
}

func ReadRecording(directory string, session protocol.SessionID) (RecordingSession, error) {
	var recording RecordingSession
	err := withAcceptanceFiles(directory, func(absolute string) error {
		var err error
		recording, err = readRecording(absolute, session)
		return err
	})
	return recording, err
}

func readRecording(directory string, session protocol.SessionID) (RecordingSession, error) {
	var recording RecordingSession
	data, err := readAcceptanceFile(RecordingPath(directory, session), MaxRecordingBytes)
	if err != nil {
		return recording, err
	}
	if err := json.Unmarshal(data, &recording); err != nil {
		return recording, errors.New("acceptance recording is invalid")
	}
	if recording.Version != 1 || recording.Session != session || recording.History.Page.Session != session || len(recording.Frames) > MaxRecordingFrames {
		return recording, errors.New("acceptance recording has an invalid identity or frame count")
	}
	return recording, nil
}

func updateRecording(directory string, node protocol.NodeID, session protocol.SessionID, update func(*RecordingSession)) error {
	return withAcceptanceFiles(directory, func(absolute string) error {
		ledger, err := readOwnership(absolute, node)
		if err != nil {
			return err
		}
		if !ledger.owns(session) {
			return errors.New("acceptance recording requires an explicitly owned session")
		}
		recording, err := readRecording(absolute, session)
		if errors.Is(err, os.ErrNotExist) {
			recording, err = newRecording(node, session), nil
		}
		if err != nil {
			return err
		}
		if recording.Node != node {
			return errors.New("acceptance recording belongs to another Node")
		}
		update(&recording)
		recording.Captured = time.Now().UTC()
		data, err := json.Marshal(recording)
		if err != nil {
			return err
		}
		if len(data)+1 > MaxRecordingBytes {
			return errors.New("acceptance recording exceeds its byte limit")
		}
		if err := core.PrivateDirectory(filepath.Join(absolute, "recordings")); err != nil {
			return err
		}
		return writeReportFile(RecordingPath(absolute, session), append(data, '\n'))
	})
}

// CaptureRecording reads only an explicitly owned conversation. Missing entry
// pages are completed against the captured history revision before removal.
// Tool details are read through Client's revision-checked public API.
func CaptureRecording(ctx context.Context, controller *client.Client, directory string, session protocol.SessionID) error {
	if controller == nil {
		return errors.New("acceptance recording requires an existing Client")
	}
	if err := requireOwned(directory, controller.Target(), []protocol.SessionID{session}); err != nil {
		return err
	}
	history, truncated, err := captureHistory(ctx, controller, session)
	if err != nil {
		return err
	}
	return captureRecordingHistory(ctx, controller, directory, session, history, truncated)
}

func captureRecordingHistory(ctx context.Context, controller *client.Client, directory string, session protocol.SessionID, history protocol.ConversationHistory, truncated bool) error {
	tools := []RecordingTool{}
	toolBytes := 0
	var problems []error
	for _, reference := range recordingToolReferences(history.Page) {
		recorded := RecordingTool{Reference: reference}
		if len(tools) >= 128 || toolBytes >= recordingToolBudget {
			truncated = true
			continue
		}
		detail, readErr := controller.ReadToolDetail(ctx, reference)
		if readErr != nil {
			recorded.Error, truncated = safeError(readErr), true
			problems = append(problems, readErr)
		} else if data, encodeErr := json.Marshal(detail); encodeErr != nil || toolBytes+len(data) > recordingToolBudget {
			recorded.Error, truncated = "Tool detail omitted by the recording byte limit", true
		} else {
			recorded.Detail = &detail
			toolBytes += len(data)
		}
		tools = append(tools, recorded)
	}
	writeErr := updateRecording(directory, controller.Target(), session, func(recording *RecordingSession) {
		recording.History, recording.Tools = history, tools
		recording.Truncated = recording.Truncated || truncated
	})
	return errors.Join(append(problems, writeErr)...)
}

// Presentation also exposes standalone tool results. Keep the final result's
// reference for a unique matched call, and retain each unmatched result without
// changing the stricter pairing used by acceptance assertions in turnView.tools.
func recordingToolReferences(page protocol.ConversationPage) []protocol.ToolDetailReference {
	type source struct {
		name string
		ref  protocol.ToolDetailReference
	}
	sources := map[string]source{}
	pending := map[string][]string{}
	order := []string{}
	for _, entry := range page.Entries {
		for index, part := range entry.Parts {
			key := entry.ID + "/" + strconv.Itoa(index)
			var call *string
			var name string
			var summary *protocol.ToolDetailSummary
			switch part.Kind {
			case "tool_call":
				value, err := decode[protocol.ConversationPartToolCallPayload](part.Data)
				if err != nil {
					continue
				}
				call, name, summary = value.ID, value.Name, value.Summary
				if call != nil {
					identity := entry.Turn.String() + "\x00" + entry.Branch + "\x00" + *call
					pending[identity] = append(pending[identity], key)
				}
			case "tool_result":
				value, err := decode[protocol.ConversationPartToolResultPayload](part.Data)
				if err != nil {
					continue
				}
				call, name, summary = value.ID, value.Name, value.Summary
				if call != nil {
					identity := entry.Turn.String() + "\x00" + entry.Branch + "\x00" + *call
					if keys := pending[identity]; len(keys) == 1 && sources[keys[0]].name == name {
						key = keys[0]
					}
					delete(pending, identity)
				}
			default:
				continue
			}
			if _, exists := sources[key]; !exists {
				order = append(order, key)
			}
			ref := protocol.ToolDetailReference{Session: page.Session, Turn: entry.Turn, Entry: entry.ID, Index: uint64(index), Call: call}
			if summary != nil {
				ref.Revision = summary.Revision
			}
			sources[key] = source{name, ref}
		}
	}
	refs := []protocol.ToolDetailReference{}
	for _, key := range order {
		if ref := sources[key].ref; ref.Revision != "" {
			refs = append(refs, ref)
		}
	}
	return refs
}

func captureHistory(ctx context.Context, controller *client.Client, session protocol.SessionID) (protocol.ConversationHistory, bool, error) {
	history, err := readOwnedHistory(ctx, controller, session)
	if err != nil {
		return history, false, err
	}
	// The acceptance cases have few turns, but older reports must remain bounded.
	for history.Page.NextBefore != nil && len(history.Page.Runs) < 1000 {
		before := *history.Page.NextBefore
		output, err := controller.Do(ctx, protocol.Command{Kind: "read_conversation", Data: protocol.CommandReadConversationPayload{Session: session, Before: &before, Limit: 100}})
		if err != nil {
			return history, true, err
		}
		older, err := outputAs[protocol.ConversationHistory](output, "conversation")
		if err != nil || older.Page.Session != session || older.Page.Revision != history.Page.Revision || older.Page.NextBefore != nil && *older.Page.NextBefore == before {
			return history, true, errors.New("acceptance history changed or its pagination did not advance")
		}
		mergeHistoryPage(&history.Page, older.Page)
		history.Missing = append(history.Missing, older.Missing...)
		history.Page.NextBefore = older.Page.NextBefore
		data, err := json.Marshal(history)
		if err != nil || len(data) > recordingHistoryBudget {
			return boundedHistory(history), true, nil
		}
	}
	missing := slices.Clone(history.Missing)
	for _, turn := range missing {
		var before *uint64
		for {
			output, err := controller.Do(ctx, protocol.Command{Kind: "read_turn", Data: protocol.CommandReadTurnPayload{Session: session, Turn: turn, ExpectedRevision: history.Page.Revision, Before: before, Limit: 500}})
			if err != nil {
				return history, true, err
			}
			part, err := outputAs[protocol.ConversationTurnHistory](output, "turn_history")
			if err != nil || part.Run.Session != session || part.Run.Turn != turn || part.Revision != history.Page.Revision || before != nil && part.NextBefore != nil && *part.NextBefore >= *before {
				return history, true, errors.New("acceptance turn history changed or its pagination did not advance")
			}
			mergeHistoryPage(&history.Page, protocol.ConversationPage{Entries: part.Entries, Approvals: part.Approvals, Questions: part.Questions, Children: part.Children})
			data, err := json.Marshal(history)
			if err != nil || len(data) > recordingHistoryBudget {
				return boundedHistory(history), true, nil
			}
			if part.NextBefore == nil {
				history.Missing = slices.DeleteFunc(history.Missing, func(id protocol.TurnID) bool { return id == turn })
				break
			}
			before = part.NextBefore
		}
	}
	data, err := json.Marshal(history)
	if err != nil {
		return history, false, err
	}
	if len(data) > recordingHistoryBudget {
		return boundedHistory(history), true, nil
	}
	return history, len(history.Missing) > 0 || history.Page.NextBefore != nil, nil
}

func boundedHistory(history protocol.ConversationHistory) protocol.ConversationHistory {
	// Preserve the paging markers and newest entries. The enclosing recording's
	// Truncated flag also covers oversized runs or metadata that must be omitted.
	for {
		data, err := json.Marshal(history)
		if err == nil && len(data) <= recordingHistoryBudget {
			return history
		}
		if len(history.Page.Entries) > 0 {
			history.Page.Entries = history.Page.Entries[1:]
			continue
		}
		empty := protocol.DefaultConversationHistory()
		empty.Page.Session, empty.Page.Revision, empty.Sequence = history.Page.Session, history.Page.Revision, history.Sequence
		return empty
	}
}

func mergeHistoryPage(into *protocol.ConversationPage, page protocol.ConversationPage) {
	into.Entries = mergeRecordingValues(into.Entries, page.Entries, func(value protocol.ConversationEntry) string { return value.Turn.String() + "/" + value.ID })
	into.Runs = mergeRecordingValues(into.Runs, page.Runs, func(value protocol.ConversationRun) string { return value.Turn.String() })
	into.Approvals = mergeRecordingValues(into.Approvals, page.Approvals, func(value protocol.ConversationApproval) string { return value.ID.String() })
	into.Questions = mergeRecordingValues(into.Questions, page.Questions, func(value protocol.ConversationQuestion) string { return value.ID.String() })
	into.Children = mergeRecordingValues(into.Children, page.Children, func(value protocol.ConversationChild) string { return value.Run.Turn.String() })
	slices.SortFunc(into.Entries, func(a, b protocol.ConversationEntry) int { return compareSequence(a.Sequence, b.Sequence) })
	slices.SortFunc(into.Runs, func(a, b protocol.ConversationRun) int { return compareSequence(a.Sequence, b.Sequence) })
}

func compareSequence(a, b uint64) int {
	if a < b {
		return -1
	}
	if a > b {
		return 1
	}
	return 0
}

func mergeRecordingValues[T any](into, values []T, key func(T) string) []T {
	for _, value := range values {
		if !slices.ContainsFunc(into, func(existing T) bool { return key(existing) == key(value) }) {
			into = append(into, value)
		}
	}
	return into
}
