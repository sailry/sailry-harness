package conversation

import "time"

// This file is the transcript's contract: the view models a backend fills
// and the actions the transcript emits. The view groups, colors and lays
// them out; it never fetches or changes them (docs/conversation.md).

// RunStatus is the status of a reply's run (messages/frame.rs).
type RunStatus int

const (
	Completed RunStatus = iota
	Queued
	Running
	Stopping
	Continued
	Cancelled
	Interrupted
	Failed
	Unsynced // active, but the host's status is not synced
)

// Active reports a run still going: the work region stays open and the
// phase row shows.
func (s RunStatus) Active() bool {
	return s == Queued || s == Running || s == Stopping || s == Unsynced
}

// Phase is what an active run is doing, shown at the end of the reply.
type Phase int

const (
	PhaseNone Phase = iota
	Thinking
	Generating
	AwaitingResponse
	Compacting
	WaitingChildren // Reply.Waiting is the count
)

// Turn is a user message and the reply to it.
type Turn struct {
	ID    string
	User  string
	Reply Reply
}

// Reply is the assistant's part of a turn: blocks in order.
type Reply struct {
	Status  RunStatus
	Elapsed time.Duration
	Phase   Phase
	Waiting int // children waited for, with WaitingChildren
	Blocks  []Block
}

// BlockKind says what a block holds.
type BlockKind int

const (
	TextBlock      BlockKind = iota // Markdown in Text
	ReasoningBlock                  // the model's thinking in Text
	ToolsBlock                      // Calls, grouped by the view
	QuestionBlock                   // Question, its history row
	ChildrenBlock                   // Children: subagent sessions
	ProgressBlock                   // Steps: a task's progress
	ErrorBlock                      // the turn's failure: Label and Text
)

// Block is one part of a reply.
type Block struct {
	Kind     BlockKind
	Text     string
	Label    string // ErrorBlock: its localized title
	Calls    []Call
	Question *Question
	Children []Child
	Steps    []Step
}

// RowState is the state text a tool row shows (tools/row.rs).
type RowState int

const (
	StateNone RowState = iota
	Denied
	StateInterrupted
	StateFailed // shown by color and the error line, without state text
	Pending     // awaiting approval
	NotExecuted
)

// Call is a tool call as the backend projected it: the label, icon and
// detail are already resolved from the tool's declaration.
type Call struct {
	ID   string
	Name string // calls of the same name group together
	// Label is the tool's label from its declaration; LabelKey, used when
	// Label is empty, is the i18n key of a built-in tool's (tool_load_skill).
	Label, LabelKey string
	Icon            string // icons.ByName; "" falls back to settings-2
	// Detail is the summary line; Code draws it in mono. Error replaces it
	// while the call failed or warns, unless it was denied.
	Detail string
	Code   bool
	Error  string
	State  RowState
	// Failed is a runtime failure (danger), Warning a tool's own failure
	// or a denied, interrupted or unexecuted call (warning).
	Failed, Warning, Pending, Running bool
	// Standalone calls never group; Terminal ones (ask_user, plan review)
	// are interactions rather than work.
	Standalone, Terminal bool
	// Output is what expanding the row shows; nil rows have no caret.
	Output *Output

	note string // view-only: state text in place of State's
	wrap bool   // view-only: the label wraps rather than truncates
}

// Output is a call's result, one or more of its parts.
type Output struct {
	// Text is plain output; Command keeps only its last 100 lines.
	Text    string
	Command bool
	// Paths lists the files a search or read touched.
	Paths []string
	Diff  *Diff
	Table *Table
	// Failure is a failed call's text, tinted danger (runtime) or, with
	// ToolFailure, warning.
	Failure     string
	ToolFailure bool
	Notices     []string
	Partial     bool // a table or diff was cut short
}

// Diff is a file change preview.
type Diff struct {
	Path           string
	Added, Removed int
	Lines          []DiffLine
}

// DiffLine is a line of a diff: Op is ' ', '+' or '-'.
type DiffLine struct {
	Op   byte
	Text string
}

// Table is tabular output.
type Table struct {
	Columns []string
	Rows    [][]string
}

// QuestionState is the state of a question the model asked.
type QuestionState int

const (
	QuestionPending    QuestionState = iota
	QuestionAnswered                 // answered, and the call returned
	QuestionAccepted                 // answered while the call still runs
	QuestionUnconsumed               // answered, but the run ended first
	QuestionCancelled
	QuestionDeclined
	QuestionInterrupted
	QuestionTimedOut
)

// Question is an ask_user call: its prompt, choices and answers.
type Question struct {
	CallID  string
	Prompt  string
	Options []string
	Multi   bool
	State   QuestionState
	Answers []string
}

// ChildStatus is a subagent session's status.
type ChildStatus int

const (
	ChildRunning ChildStatus = iota
	ChildCompleted
	ChildFailed
	ChildInterrupted
	ChildCancelled
	ChildQueued
)

// Child is a subagent session a call delegated to.
type Child struct {
	ID, Title     string
	Model, Effort string
	Status        ChildStatus
}

// StepState is the state of a task step.
type StepState int

const (
	StepPending StepState = iota
	StepInProgress
	StepCompleted
	StepSkipped
	StepPaused
	StepBlocked
	StepFailed
)

// Step is a step of a task's progress.
type Step struct {
	Text  string
	State StepState
}

// Actions is what the transcript asks of the backend.
type Actions interface {
	// OpenChild opens a subagent's session.
	OpenChild(id string)
	// Retry runs a failed turn again.
	Retry(turn string)
}
