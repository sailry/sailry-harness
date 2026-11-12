package composer

import (
	"github.com/egoist/mygo/ui"
)

// The composer's view model: what the backend offers it (Context) and what
// the user puts together (State, Message). Field names are the contract
// with the logic half.

// Context is what the conversation offers the composer, built by the page
// from the backend's data. The composer reads it and changes none of it;
// the user's choices go to State and the backend through Actions.
type Context struct {
	Models []Model
	// MemberModels are the models a team member may take, nil for Models:
	// on another Node only that Node's own, since a member's choice is not
	// forwarded with the local Node's provider as the lead's is.
	MemberModels []Model
	Families     []Family // the model picker's rail, in order
	Teams        []Team
	// Interactions are the approvals and question the run waits on.
	Interactions Interactions
	// Activity is what runs around the conversation: the activity strip.
	Activity Activity
	// Queue is the messages waiting for the reply to end, oldest first.
	Queue []Queued
	// Hosts, Projects and Repository locate the conversation: where it
	// runs, its project and worktree, and the worktree's checkout.
	Hosts      []Host
	Projects   []Project
	Repository Repository
	// References is the @ picker's root page; Commands the / picker's.
	References []Reference
	Commands   []Reference
	// Last is the user's last message, which Up puts back in an empty
	// draft; "" when it has none or it was more than text.
	Last string
	// Readonly keeps the draft from being sent: a child session's.
	Readonly bool
	// LocationLocked keeps an admitted session on its captured project.
	LocationLocked bool
	// PackageBranch says a package's context control chooses the branch and
	// worktree of the conversation's Git project: the bar shows that one
	// and not the native worktree menu, so there is one such control.
	PackageBranch bool
	// Indicators are the toolbar's progress rings in their order: a
	// session's context usage. A draft without a session has none.
	Indicators []Indicator
	// SendBlocked keeps Send and Enter from starting a turn although the
	// draft has input: the Node is away, its project or worktree is not
	// available, or a request is under way. Stop is not held by it. An empty
	// draft, files in transfer and an empty Models block Send by themselves.
	SendBlocked bool
	// SelectModel opens the Node's model settings from the chip that reads
	// Select model while Models is empty; nil leaves that chip disabled.
	SelectModel func()
	// PluginModes are the enabled packages' conversation modes, after the
	// built-in ones in the mode menu, the compact settings and the Shift+Tab
	// cycle.
	PluginModes []PluginMode
	// SelectMode chooses a mode by its key: "code", "plan", "flagship" or a
	// PluginMode's Key. nil leaves the built-in modes to State.Mode.
	SelectMode func(key string)
	// Child is a delegated session: the composer is not shown, and its queue
	// tray is read-only (Read, Copy, Close, Remove, Withdraw).
	Child bool
	// Compact compacts the conversation with the focus /compact was given,
	// "" for none; nil leaves it to Actions.Compact.
	Compact func(focus string)
	// Stale reports that a command or skill chosen from the picker has gone
	// since the picker listed it; nil for never.
	Stale func(Reference) bool
	// Outgoing is the message being sent to the queue, shown as a disabled
	// row until the Node lists it; nil for none.
	Outgoing *Queued
}

// PluginMode is a package's conversation mode (live/mode.rs PluginMode).
type PluginMode struct {
	Key, Label, Description string
	Icon                    *ui.SVG
	// Disabled is a mode that cannot be chosen now: its package is not
	// available, or the draft has no configured model.
	Disabled bool
	// Selected is the mode the conversation runs in.
	Selected bool
}

// Model is a model the picker offers, as the backend lists it.
type Model struct {
	ID, Name, Provider string
	// Reasoning makes it take an effort: Efforts, or the package's
	// default levels when nil.
	Reasoning     bool
	Efforts       []string
	DefaultEffort string
	// Family is the rail's group the provider belongs to (Family.ID).
	Family string
	// Score is the model's measured fit, nil when unmeasured.
	Score  *Score
	Scores map[string]*Score
	// Features are its options beyond the effort, as Fast mode.
	Features []Feature
	// Capabilities are what it understands and generates beyond text, as
	// the picker's rows mark them.
	Capabilities []Capability
}

// Capability is a model's capability tag the picker shows: understanding
// images, or generating images, video or speech.
type Capability struct {
	ID    string
	Label string // i18n key
	Icon  *ui.SVG
}

// Family is a group of providers on the model picker's rail: a cloud,
// a local runtime, a gateway (settings::ModelCategory).
type Family struct {
	ID    string
	Label string // i18n key
	Icon  *ui.SVG
}

// Score is a model's measured fit (model_picker/scores): the overall
// score and its five axes, measured at the effort Tested.
type Score struct {
	Overall float64
	Axes    [5]float64 // backend, frontend, reasoning, visual, maintainability
	Tested  string
}

// Feature is a model's option. A feature with ID "speed" is the Fast toggle;
// one with one option toggles it; others choose among their options from a
// menu.
type Feature struct {
	ID, Name string
	Icon     *ui.SVG
	Options  []FeatureOption
}

// FeatureOption is a choice of a Feature.
type FeatureOption struct {
	ID, Name string
	Icon     *ui.SVG
}

// Team is a team the conversation can run as: its lead model and members.
type Team struct {
	ID, Name string
	Members  []Member
}

// Member is a team's role, whose model the controls set apart.
type Member struct {
	ID, Name string
	// Overridden marks a member whose model differs from the team's.
	Overridden bool
}

// Queued is a message waiting for the reply to end (live/queue).
type Queued struct {
	ID       string
	Text     string
	Revision uint64
	Load     func(func(string, bool))
	Save     func(string, func(bool))
	// Attachments are the files it carries.
	Attachments []*Attachment
	// Notice is a background notice, Compaction a compaction request:
	// neither is edited, sent ahead or dragged.
	Notice, Compaction bool
	// Marker names a turn a package continues by itself: an automatic row
	// as a notice is, with the package's label for its text.
	Marker *Text
	// Guided is guidance pending (Pending.guidance): the running turn
	// takes it at its next model boundary unless it is withdrawn.
	Guided bool
	// Joining is guidance aimed at the turn that takes guidance now: Send
	// now stays enabled on it, as on a row not guiding. Guidance aimed at
	// another turn has it disabled.
	Joining bool
}

// Host is a computer the conversation can run on: execution-owner metadata.
// The composer draws no host choice (AGENTS.md, Removed UI).
type Host struct {
	ID    string
	Name  string
	Local bool
}

// Project is a location choice: a project and its worktrees.
type Project struct {
	ID, Name  string
	Host      string
	Worktrees []Worktree
}

// Worktree is a checkout of a project.
type Worktree struct {
	Branch string
	Main   bool
}

// RepositoryKind is what the worktree's folder is.
type RepositoryKind int

const (
	Inspecting  RepositoryKind = iota // the inspection runs
	Git                               // a Git repository
	Directory                         // a plain folder: no worktrees
	Unavailable                       // the folder is gone
)

// Repository is the inspection of the conversation's worktree: its kind
// and the branch checked out, or the short HEAD when detached.
type Repository struct {
	Kind   RepositoryKind
	Branch string // "" when neither: composer_branch_none
}

// ReferenceKind is what a Reference names, or does once chosen.
type ReferenceKind int

const (
	Page          ReferenceKind = iota // opens its Children
	File                               // a file of the worktree
	Folder                             // a folder: a page whose first row selects it
	Agent                              // a subagent: one per message
	Session                            // another conversation
	Location                           // the host, project or worktree
	Database                           // a database or table
	Skill                              // a plugin's skill
	Plugin                             // a plugin of the catalog
	Prompt                             // a / command putting Insert in the draft
	ModeCommand                        // a / command choosing the mode Insert names
	ModelCommand                       // a / command choosing the model Insert names
	EffortCommand                      // a / command choosing the effort Insert names
	Compact                            // /compact: compacts the conversation
	AttachFiles                        // opens the window's file picker
)

// Reference is a row of the @ and / pickers (live/references): a page of
// more rows, or something chosen into the draft as an inline chip.
type Reference struct {
	// Source identifies the owning dynamic picker page.
	Source      string
	ID          string
	Kind        ReferenceKind
	Label       string // shown as is; an i18n key for pages (Key)
	Key         bool   // Label and Description are i18n keys
	Description string
	Icon        *ui.SVG
	// Alias is a word the filter matches besides the label, as "files".
	Alias string
	// Insert is what a command puts in the draft or chooses.
	Insert   string
	Children []Reference
	// Group is the i18n key of the heading the picker shows over the
	// first row of a run of rows sharing it; "" for none.
	Group string
}

// AttachmentStatus is where an attachment is on its way to the host.
type AttachmentStatus int

const (
	Preparing AttachmentStatus = iota
	Sending                    // Progress tells how far
	Publishing
	Ready
	Failed // Error names why
	Uncertain
	Removing
)

// Attachment is a file of the draft (live/attachments). The composer adds
// one for each file chosen, dropped or pasted and hands it to
// Actions.Upload; the backend moves its Status and Progress on.
type Attachment struct {
	ID, Name, Mime string
	Size           int64
	// Image is a picture's thumbnail, nil for other files or until it
	// decodes.
	Image ui.ImageSource
	// Path is where a dropped or chosen file is; Data a pasted picture's
	// PNG bytes.
	Path     string
	Data     []byte
	Status   AttachmentStatus
	Progress float32 // 0 to 1 while Sending
	Error    string  // i18n key while Failed
}

// MaxAttachments is how many files a message carries (chat_attachment_limit).
const MaxAttachments = 8

// DictationPhase is where voice input is.
type DictationPhase int

const (
	Idle DictationPhase = iota
	PreparingSpeech
	Recording
	Transcribing
)

// Dictation is voice input's state, which the backend moves on: the phase
// and, while recording, the last levels of the microphone, 0 to 1.
type Dictation struct {
	Phase  DictationPhase
	Levels []float32
}

// Message is what Send and Queue take: the draft's text, its files, the
// references chipped into it, and the choices it runs with.
type Message struct {
	Text        string
	Attachments []*Attachment
	References  []Reference
	Mode        Mode
	Permission  Permission
	Model       string
	Effort      string
	Features    map[string]string
	Team        string
}
