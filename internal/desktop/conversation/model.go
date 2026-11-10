package conversation

import (
	"strings"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
)

// This file is the transcript's contract: the view models a backend fills
// and the actions the transcript emits. The view groups, colors and lays
// them out; it never fetches or changes them (docs/conversation.md).

// RunStatus is the status of a reply's run.
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
	ID   string
	User string
	// Sent is when the user message was sent; the footer shows it on hover.
	Sent time.Time
	// Images and Files are the message's attachments: thumbnails and file cards
	// under the bubble.
	Images []Image
	Files  []File
	// Notices are background results delivered with the turn, shown as a bubble
	// on the user's side.
	Notices []Notice
	// Compaction makes the turn a context compaction: a marker row, its summary
	// or failure, and no reply frame.
	Compaction *Compaction
	Reply      Reply
}

// Reply is the assistant's part of a turn: blocks in order.
type Reply struct {
	Status  RunStatus
	Elapsed time.Duration
	Phase   Phase
	Waiting int // children waited for, with WaitingChildren
	Blocks  []Block
	// Finished is when the run ended; the footer shows it on hover.
	Finished time.Time
	// Citations are the answer's sources, numbered links placed at their End (a
	// rune offset into the answer's text) or after it.
	Citations []Citation
	// Changes is the files the turn edited, the card after the answer; nil for
	// none.
	Changes *Changes
	// Notice marks a run that only delivered background notices: no footer
	// menus.
	Notice bool
}

// BlockKind says what a block holds.
type BlockKind int

const (
	TextBlock       BlockKind = iota // Markdown in Text
	ReasoningBlock                   // the model's thinking in Text
	ToolsBlock                       // Calls, grouped by the view
	QuestionBlock                    // Question, its history row
	ChildrenBlock                    // Children: subagent sessions
	ProgressBlock                    // Steps: a task's progress
	ErrorBlock                       // the turn's failure: Label and Text
	ImagesBlock                      // Images: generated or attached pictures
	FilesBlock                       // Files: output files as cards
	SearchBlock                      // grounding: Text is the answer, Sources its pages
	RetryBlock                       // Retry: the model request retried
	VisualBlock                      // Visual: a chart or diagram the model drew
	CompactionBlock                  // Text: an automatic handoff's disclosed summary
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
	Images   []Image
	Files    []File
	Sources  []Citation
	Retry    *Retry
	Visual   *Visual
}

// Image is a picture of a message: its name, size in pixels and the decoded
// source, nil while it loads.
type Image struct {
	ID, Name      string
	Width, Height int
	Source        ui.ImageSource
}

// FileKind is how the file preview shows a file (content/files).
type FileKind int

const (
	TextFile     FileKind = iota // plain text
	CodeFile                     // highlighted by its extension
	MarkdownFile                 // rendered Markdown
	ImageFile                    // Image
	OtherFile                    // no native preview (PDF, office...): the system's app opens it
)

// PreviewState is what the preview of a file has come to.
type PreviewState int

const (
	PreviewReady PreviewState = iota
	PreviewLoading
	PreviewFailed
	PreviewLarge   // past the 2 MiB preview limit
	PreviewChanged // the file changed since the turn wrote it
)

// File is a file a message attached or a turn produced, as a card that opens
// its preview in the right panel.
type File struct {
	ID   string
	Path string // worktree-relative; its last element is the label
	Mime string
	Size int64
	Kind FileKind
	// Text is a text, code or Markdown file's content; Image a picture's.
	Text  string
	Image *Image
	State PreviewState
	// Editable lets Request edit act on the file.
	Editable bool
}

// Name is the file's label: the last element of its path.
func (f File) Name() string {
	if i := strings.LastIndexByte(f.Path, '/'); i >= 0 {
		return f.Path[i+1:]
	}
	return f.Path
}

// Citation is a source of the answer or of a search: its address, title
// and the rune offset in the answer where its mark goes (0: after it).
type Citation struct {
	URI, Title string
	End        int
}

// Retry is a model request retried: the attempt of Limit, its reason (an i18n
// key, chat_model_*) and whether it gave up.
type Retry struct {
	Attempt, Limit int
	Reason         string
	Detail         string // the provider's message, shown when opened
	Exhausted      bool
}

// Visual is a diagram or chart the model drew, as Mermaid source, shown
// in place (content/visuals), laid out by limu.
type Visual struct {
	Title   string
	Mermaid string
}

// NoticeOutcome is how a background task ended.
type NoticeOutcome int

const (
	OutcomeCompleted NoticeOutcome = iota
	OutcomeFailed
	OutcomeCancelled
	OutcomeStopped
	OutcomeTimedOut
	OutcomeInterrupted
	OutcomeUnknown
	OutcomeStalled
)

// Notice is a background task's result: an agent's or a command's.
type Notice struct {
	Title   string
	Agent   bool
	Outcome NoticeOutcome
}

// CompactionState is where a context compaction is.
type CompactionState int

const (
	Compacted CompactionState = iota
	CompactionRunning
	CompactionQueued
	CompactionFailed
	CompactionCancelled
	CompactionUnneeded
)

// Compaction is a compaction turn: its state, the summary it wrote and,
// failed, the cause.
type Compaction struct {
	State   CompactionState
	Summary string
	Cause   string
}

// Changes is the files a turn edited, and which the user undid.
type Changes struct {
	Files  []ChangedFile
	Undone map[string]bool // by path; "" undid them all
}

// ChangedFile is an edited file: its path and line counts.
type ChangedFile struct {
	Path           string
	Added, Removed int
}

// RowState is the state text a tool row shows.
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
	// Failed is a failed call, the only red row: the tool could not run
	// (unknown, invalid arguments, execution or transport error). Warning
	// is an error the tool returned from a call that ran, shown plain with
	// its error line, or, with a State, a denied, interrupted or unexecuted
	// call (warning color).
	Failed, Warning, Pending, Running bool
	// Standalone calls never group; Terminal ones (ask_user, plan review)
	// are interactions rather than work.
	Standalone, Terminal bool
	// Output is what expanding the row shows; nil rows have no caret.
	Output *Output
	// Load requests revision-scoped details when this disclosure first opens.
	Load func()

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

// Texts returns the lines before and after the change, for a diff
// viewer.
func (d *Diff) Texts() (old, new string) {
	var o, n strings.Builder
	for _, l := range d.Lines {
		if l.Op != '+' {
			o.WriteString(l.Text + "\n")
		}
		if l.Op != '-' {
			n.WriteString(l.Text + "\n")
		}
	}
	return o.String(), n.String()
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
	ChildStopping
	ChildContinued
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

// Actions is what the transcript asks of the backend. Copying, search,
// selection and the image viewer are the view's own.
type Actions interface {
	// OpenChild opens a subagent's session.
	OpenChild(id string)
	// Retry runs a failed turn again.
	Retry(turn string)
	// Fork starts a session from the conversation up to turn.
	Fork(turn string)
	// Rewind keeps turn and removes the later ones, once confirmed.
	Rewind(turn string)
	// Worktree creates a worktree to go on in.
	Worktree(turn string)
	// Export saves turn, or the conversation for "", as Markdown, HTML or an
	// image.
	Export(turn string, format ExportFormat)
	// Read reads the answer aloud, or stops it (reader).
	Read(turn, text string)
	// Edit edits a sent message.
	Edit(turn string)
	// OpenFile shows a file in the right panel's preview.
	OpenFile(f File)
	// FileAction is a file card's menu: open it in another app, save it,
	// or ask the model to edit it.
	FileAction(f File, action FileAction)
	// ImageAction is the image viewer's Copy, Save or Open of a picture.
	ImageAction(a kit.ImageAction, im Image)
	// Undo reverts a turn's changes to path, or all of them for "", and Review
	// opens their review.
	Undo(turn, path string)
	Review(turn, path string)
	// LoadOlder asks for the page before the first turn.
	LoadOlder()
	// Reconnect retries the connection while recovering.
	Reconnect()
	// Quote adds a selection to the draft, quoted.
	Quote(text string)
}

// ExportFormat is what a turn or conversation exports to.
type ExportFormat int

const (
	ExportMarkdown ExportFormat = iota
	ExportHTML
	ExportImage
)

// FileAction is an action of a file card's menu.
type FileAction int

const (
	OpenExternal FileAction = iota
	Download
	RequestEdit
)

// Session is the conversation around its turns: its history paging, its
// connection, a diagnostic of what failed, and its usage. nil is a loaded,
// connected session at its start.
type Session struct {
	// Older is how many turns come before the first one shown, 0 at the
	// start; LoadingOlder while the page before loads, OlderFailed when it
	// did not.
	Older        int
	LoadingOlder bool
	OlderFailed  bool
	// Loading is the first page loading: no turns to show yet.
	Loading    bool
	Connection Connection
	Diagnostic *Diagnostic
	Usage      *Usage
}

// Connection is the session's link to its host.
type Connection int

const (
	Connected Connection = iota
	Reconnecting
	Offline
)

// Diagnostic is a failure the session reports in detail: its heading, the
// fields of the error in order, and the raw message.
type Diagnostic struct {
	Title  string
	Fields [][2]string
	Detail string
}

// Usage is the session's metrics in the composer's context bar
// (conversation/usage): tokens, generation speed, estimated cost, cached
// tokens and turns, and how full the context window is (0 to 1).
type Usage struct {
	Tokens  int
	Speed   float64 // tokens a second
	Cost    float64 // USD
	Cached  int
	Turns   int
	Context float64
}

// Active reports a child that is still executing.
func (s ChildStatus) Active() bool { return s == ChildRunning || s == ChildStopping }
