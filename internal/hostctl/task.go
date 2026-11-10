package hostctl

import (
	"encoding/json"
	"errors"
	"io"
	"os"
	"strings"
	"unicode"
	"unicode/utf8"

	"github.com/sailry/sailry-harness/internal/protocol"
)

const MaxTaskRequest = 1024 * 1024
const MaxTaskResponse = 8 * 1024 * 1024

type Target struct {
	Kind             string
	Session          protocol.SessionID
	ExpectedRevision *uint64
	Worktree         protocol.WorktreeID
	Config           *protocol.SessionConfig
	Execution        protocol.ConversationExecution
	Team             *protocol.TeamSelection
	Association      *protocol.ConversationAssociation
}

type sessionTarget struct {
	Kind             string             `json:"kind"`
	Session          protocol.SessionID `json:"session"`
	ExpectedRevision *uint64            `json:"expected_revision"`
}

type worktreeTarget struct {
	Kind        string                            `json:"kind"`
	Worktree    protocol.WorktreeID               `json:"worktree"`
	Config      *protocol.SessionConfig           `json:"config"`
	Execution   protocol.ConversationExecution    `json:"execution"`
	Team        *protocol.TeamSelection           `json:"team"`
	Association *protocol.ConversationAssociation `json:"association"`
}

func (target Target) MarshalJSON() ([]byte, error) {
	switch target.Kind {
	case "session":
		return json.Marshal(sessionTarget{target.Kind, target.Session, target.ExpectedRevision})
	case "worktree":
		return json.Marshal(worktreeTarget{target.Kind, target.Worktree, target.Config, target.Execution, target.Team, target.Association})
	}
	return nil, errors.New("invalid Host task target")
}

func (target *Target) UnmarshalJSON(body []byte) error {
	var tag struct {
		Kind string `json:"kind"`
	}
	if err := json.Unmarshal(body, &tag); err != nil {
		return err
	}
	switch tag.Kind {
	case "session":
		var input sessionTarget
		if err := protocol.DecodeObject(body, &input, []string{"kind", "session"}, true); err != nil {
			return err
		}
		*target = Target{Kind: "session", Session: input.Session, ExpectedRevision: input.ExpectedRevision}
		return nil
	case "worktree":
		input := worktreeTarget{Execution: protocol.ConversationExecutionTeam}
		if err := protocol.DecodeObject(body, &input, []string{"kind", "worktree"}, true); err != nil {
			return err
		}
		*target = Target{Kind: "worktree", Worktree: input.Worktree, Config: input.Config, Execution: input.Execution, Team: input.Team, Association: input.Association}
		return nil
	}
	return errors.New("invalid Host task target")
}

type Task struct {
	Command string
	Version uint8
	Request protocol.RequestID
	Target  Target
	Message protocol.ConversationInput
	Title   string
	Wait    bool
	Timeout uint64
	Session *protocol.SessionID
	Turn    *protocol.TurnID
}

type submitTask struct {
	Command string                     `json:"command"`
	Version uint8                      `json:"version"`
	Request protocol.RequestID         `json:"request"`
	Target  Target                     `json:"target"`
	Message protocol.ConversationInput `json:"message"`
	Title   string                     `json:"title"`
	Wait    bool                       `json:"wait"`
	Timeout uint64                     `json:"timeout"`
}

type waitTask struct {
	Command string             `json:"command"`
	Version uint8              `json:"version"`
	Session protocol.SessionID `json:"session"`
	Turn    protocol.TurnID    `json:"turn"`
	Timeout uint64             `json:"timeout"`
}

func (task Task) MarshalJSON() ([]byte, error) {
	if task.Command == "submit" {
		return json.Marshal(submitTask{task.Command, task.Version, task.Request, task.Target, task.Message, task.Title, task.Wait, task.Timeout})
	}
	if task.Command == "wait" && task.Session != nil && task.Turn != nil {
		return json.Marshal(waitTask{task.Command, task.Version, *task.Session, *task.Turn, task.Timeout})
	}
	return nil, errors.New("invalid Host task request")
}

func (task *Task) UnmarshalJSON(body []byte) error {
	var tag struct {
		Command string `json:"command"`
	}
	if err := json.Unmarshal(body, &tag); err != nil {
		return err
	}
	switch tag.Command {
	case "submit":
		var input submitTask
		if err := protocol.DecodeObject(body, &input, []string{"command", "version", "request", "target", "message", "title", "wait", "timeout"}, true); err != nil {
			return err
		}
		*task = Task{Command: input.Command, Version: input.Version, Request: input.Request, Target: input.Target, Message: input.Message, Title: input.Title, Wait: input.Wait, Timeout: input.Timeout}
	case "wait":
		var input waitTask
		if err := protocol.DecodeObject(body, &input, []string{"command", "version", "session", "turn", "timeout"}, true); err != nil {
			return err
		}
		*task = Task{Command: input.Command, Version: input.Version, Session: &input.Session, Turn: &input.Turn, Timeout: input.Timeout, Wait: true}
	default:
		return errors.New("invalid Host task request")
	}
	if task.Version != 1 || task.Timeout < 1 || task.Timeout > 86400 {
		return errors.New("unsupported Host task request")
	}
	if task.Command == "submit" && (strings.TrimSpace(task.Message.Text) == "" || !utf8.ValidString(task.Message.Text)) {
		return errors.New("task text is empty or invalid")
	}
	return nil
}

type TaskResult struct {
	Version   uint8                           `json:"version"`
	Node      *protocol.NodeID                `json:"node"`
	Request   *protocol.RequestID             `json:"request"`
	Session   *protocol.SessionID             `json:"session"`
	Turn      *protocol.TurnID                `json:"turn"`
	Revision  *uint64                         `json:"revision"`
	State     string                          `json:"state"`
	Text      string                          `json:"text"`
	Error     *protocol.Fault                 `json:"error"`
	Approvals []protocol.ConversationApproval `json:"approvals"`
	Questions []protocol.ConversationQuestion `json:"questions"`
}

func (result *TaskResult) UnmarshalJSON(body []byte) error {
	type plain TaskResult
	var decoded plain
	if err := protocol.DecodeObject(body, &decoded, []string{"version", "state", "text", "approvals", "questions"}, true); err != nil {
		return err
	}
	var fields map[string]json.RawMessage
	if err := json.Unmarshal(body, &fields); err != nil {
		return err
	}
	for _, name := range []string{"node", "request", "session", "turn", "revision", "error"} {
		if _, present := fields[name]; !present {
			return errors.New("Host task result is missing " + name)
		}
	}
	switch decoded.State {
	case "admitted", "observing", "completed", "failed", "cancelled", "interrupted", "continued", "awaiting_approval", "awaiting_input", "paused", "timed_out", "uncertain", "observation_cancelled":
	default:
		return errors.New("invalid Host task state")
	}
	*result = TaskResult(decoded)
	return nil
}

func (task Task) Result() TaskResult {
	result := TaskResult{Version: 1, State: "admitted", Approvals: []protocol.ConversationApproval{}, Questions: []protocol.ConversationQuestion{}}
	if task.Command == "submit" {
		request := task.Request
		result.Request = &request
		if task.Target.Kind == "session" {
			session := task.Target.Session
			result.Session, result.Revision = &session, task.Target.ExpectedRevision
		}
	} else {
		result.Session, result.Turn, result.State = task.Session, task.Turn, "observing"
	}
	return result
}

func (result TaskResult) ExitCode() int {
	switch result.State {
	case "admitted", "observing", "completed":
		return 0
	case "awaiting_approval", "awaiting_input", "paused":
		return 3
	case "uncertain":
		return 4
	case "timed_out":
		return 124
	case "observation_cancelled":
		return 130
	}
	return 1
}

func (result TaskResult) failed(err error) TaskResult {
	result.Error = asFault(err)
	result.State = "failed"
	if result.Error.Code == protocol.ErrorCodeOutcomeUnknown {
		result.State = "uncertain"
	}
	return result
}

func (result TaskResult) uncertain(message string) TaskResult {
	return result.failed(&protocol.Fault{Code: protocol.ErrorCodeOutcomeUnknown, Message: message})
}

func matchesResponse(known, incoming TaskResult) bool {
	equal := func(left, right any) bool {
		leftJSON, _ := json.Marshal(left)
		rightJSON, _ := json.Marshal(right)
		return string(leftJSON) == string(rightJSON)
	}
	return incoming.Version == 1 && incoming.Node != nil &&
		(known.Node == nil || equal(known.Node, incoming.Node)) &&
		equal(known.Request, incoming.Request) &&
		(known.Session == nil || equal(known.Session, incoming.Session)) &&
		(known.Turn == nil || equal(known.Turn, incoming.Turn)) &&
		(known.Revision == nil || equal(known.Revision, incoming.Revision)) &&
		(incoming.State != "admitted" && incoming.State != "observing" || incoming.Session != nil && incoming.Turn != nil)
}

func readJSONFile(path string, target any) error {
	file, err := os.Open(path)
	if err != nil {
		return err
	}
	defer file.Close()
	body, err := io.ReadAll(io.LimitReader(file, MaxTaskRequest+1))
	if err != nil {
		return err
	}
	if len(body) > MaxTaskRequest {
		return errors.New("task input exceeds its limit")
	}
	return json.Unmarshal(body, target)
}

func validateAssociation(value protocol.ConversationAssociation) error {
	label := func(value string) bool {
		if value == "" || len(value) > 64 {
			return false
		}
		for _, char := range value {
			if char >= 'a' && char <= 'z' || char >= 'A' && char <= 'Z' || char >= '0' && char <= '9' || char == '-' || char == '_' || char == '.' {
				continue
			}
			return false
		}
		return true
	}
	if !label(value.Namespace) || !label(value.Kind) || strings.TrimSpace(value.ID) == "" || strings.TrimSpace(value.ID) != value.ID || len(value.ID) > 256 || strings.ContainsFunc(value.ID, unicode.IsControl) {
		return errors.New("session association is invalid")
	}
	return nil
}
