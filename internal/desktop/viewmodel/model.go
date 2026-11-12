// Package viewmodel owns presentation data shared by preview and live controllers.
package viewmodel

import (
	"strings"
	"time"

	"github.com/sailry/sailry-harness/internal/desktop/composer"
	"github.com/sailry/sailry-harness/internal/desktop/conversation"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// Status is a conversation's state in the sidebar.
type Status int

const (
	Idle Status = iota
	Running
	Unread
	Waiting
)

type Conversation struct {
	Node              string
	Revision          uint64
	AttentionRevision uint64
	ID                string
	Title             string
	Status            Status
	Pinned            bool
	// PinOrder is where a pin goes among the pinned: nil for a pin made before
	// pins had an order.
	PinOrder *int
	Archived bool
	Updated  time.Time
	// Created is the order the conversation was made in, newest highest.
	Created      int
	Worktree     string // the branch of its worktree; "" is the project's main one
	WorktreePath string // captured local path, independent of a Git branch label
	Turns        []conversation.Turn
	// Waiting is what the running reply waits on the user for.
	Waiting composer.Interactions
	// Queue is the messages sent while the reply runs.
	Queue []composer.Queued
	// Commands are its background commands; Badges the plugins' Status
	// badges over its composer (the goals plugin's goal).
	Commands []composer.Command
	Badges   []composer.StatusBadge
	// Session is the conversation around its turns: older pages, the
	// connection, a diagnostic and usage.
	Session conversation.Session

	Animating bool // presentation requests another frame while content is changing
}

type Project struct {
	Icon, Color   string
	ID            string
	Name          string
	Path          string
	Branch        string
	Host          string // the Host's ID
	Worktrees     []Worktree
	Trusted       bool
	Conversations []*Conversation
}

// Data is the whole dataset.
type Data struct {
	Projects   []*Project
	Unassigned []*Conversation // chats without a project
	Models     []Model
	Providers  []Provider
	Plugins    []Plugin
	*Workspace
}

type Model = composer.Model

type Provider struct {
	ID, Name, Description string
	Badge                 string // i18n key or ""
	Enabled               bool
}

type Plugin struct {
	ID, Name, Description, Version string
	Official, Enabled              bool
}

// Host is a computer running a Node: this one and a paired one
// (local_host_name, remote_host_name in the reference's preview).
type Host struct {
	ID     string
	Name   string // an i18n key for the preview hosts
	Local  bool
	Update bool // an update is available
}

// Worktree is a checkout of a project: the main one, or one made for a
// session.
type Worktree struct {
	Branch string
	Path   string
	Main   bool
}

// NoticeKind is what a notice tells.
type NoticeKind int

const (
	NoticeCompleted NoticeKind = iota
	NoticeFailed
	NoticeApproval
	NoticeInput
	NoticeInfo
)

// Notice is an entry of the activity centre.
type Notice struct {
	Node       string
	TitleKey   string // fallback or system heading, localized by the surface
	Details    *protocol.NotificationDetails
	DetailText string
	Report     string
	ID         string
	Kind       NoticeKind
	Title      string
	Message    string
	Session    string // the conversation it opens, "" for none
	Link       string // a knowledge source it opens, "" for none
	Host       string
	Time       time.Time
	Read       bool

	// Presentation is the publishing package's own heading and persistence.
	Presentation *protocol.NotificationPresentation
}

// FileNode is a file or a folder of the project tree in the right panel.
type FileNode struct {
	Name     string
	Path     string
	Children []*FileNode // nil for a file
	Text     string
}

// LineKind is a diff line's side.
type LineKind int

const (
	Context LineKind = iota
	Added
	Removed
)

// DiffLine is a line of a hunk, with its numbers on either side (0 where
// it has none).
type DiffLine struct {
	Kind     LineKind
	Old, New int
	Text     string
}

// Hunk is a block of a file's changes.
type Hunk struct {
	Header string
	Lines  []DiffLine
}

// FileChange is a changed file of the working tree: what changed, in the
// index (Staged) and in the work tree only (Unstaged), as M, A or D, ""
// where it has not; an untracked file has neither.
type FileChange struct {
	Path             string
	Status           string // M, A, D
	Staged, Unstaged string
	Untracked        bool
	Added, Removed   int
	Hunks            []Hunk
}

// Branch is a branch of the repository: a local one, or a remote's as
// origin/main; Tracking is how it stands to its upstream ("↑1 ↓2"), ""
// when even or without one.
type Branch struct {
	Name     string
	Remote   bool
	Current  bool
	Commit   string
	Tracking string
}

// Remote is a remote of the repository.
type Remote struct {
	Name, URL string
}

// Commit is a commit of the history, newest first; Refs are the
// references at it, as heads/main, remotes/origin/main or tags/v0.1.0, and
// HEAD at the commit checked out. Parents are its parents' IDs, the first
// the line it continues: empty for a root commit and nil when the source
// does not say, which leaves the history without a graph.
type Commit struct {
	ID      string
	Message string
	Author  string
	Time    time.Time // when it was authored; zero when the source does not say
	Refs    []string
	Parents []string
}

// Repository is the project's Git repository as the Git panel shows it.
type Repository struct {
	Branch   string // the branch checked out
	Upstream string // its upstream, "" for none
	Branches []Branch
	Remotes  []Remote
	Stashes  []string // their messages, newest first
	Tags     []string
	Commits  []Commit
}

// Checkpoint is the files a turn changed, which undo restores.
type Checkpoint struct {
	ID             string
	Turn           string // the user's message of the turn
	Time           time.Time
	Files          []string
	Added, Removed int
}

// Document is a document of the session, as a plan or a report.
type Document struct {
	ID    string
	Title string
	Path  string
	Text  string
}

// Citation is a knowledge source a reply cited, in pages.
type Citation struct {
	Link       string
	Title      string
	Host       string
	Path       string
	Start, End int // lines
	Pages      []string
}

// ScreenTone is the color of a run of a sample terminal screen.
type ScreenTone int

const (
	TonePlain ScreenTone = iota
	ToneMuted
	ToneAccent
	ToneGood
	ToneBad
	ToneNote
)

// ScreenSpan is a run of one tone in a terminal line.
type ScreenSpan struct {
	Text string
	Tone ScreenTone
}

// Screen is what a terminal shows where no shell runs: its lines, oldest
// first, each a row of spans.
type Screen [][]ScreenSpan

// WebPage is the page a browser tab shows where nothing is loaded: its
// address, title and content as markdown.
type WebPage struct {
	Address string
	Title   string
	Text    string
}

// Workspace is the window's hosts, notices and panel data.
type Workspace struct {
	Hosts       []Host
	Notices     []*Notice
	Files       *FileNode
	Changes     []FileChange
	Repo        Repository
	Checkpoints []Checkpoint
	Documents   []Document
	Citations   []Citation
	// Screen and Page are a terminal's screen and a browser's page of a
	// dataset that reaches no Node: the demo's.
	Screen Screen
	Page   WebPage
}

// Find returns the conversation with id.
func (d *Data) Find(id string) *Conversation {
	for _, p := range d.Projects {
		for _, c := range p.Conversations {
			if c.ID == id {
				return c
			}
		}
	}
	for _, c := range d.Unassigned {
		if c.ID == id {
			return c
		}
	}
	return nil
}

// ProjectOf returns the project holding the conversation, nil when none.
func (d *Data) ProjectOf(id string) *Project {
	for _, p := range d.Projects {
		for _, c := range p.Conversations {
			if c.ID == id {
				return p
			}
		}
	}
	return nil
}

// Texts returns the file before and after the change, for a diff
// viewer: the hunks' lines at their numbers, the lines between them,
// which the preview has not, blank.
func (f FileChange) Texts() (old, new string) {
	var o, n []string
	for _, h := range f.Hunks {
		for _, l := range h.Lines {
			for l.Kind != Added && len(o) < l.Old-1 {
				o = append(o, "")
			}
			for l.Kind != Removed && len(n) < l.New-1 {
				n = append(n, "")
			}
			if l.Kind != Added {
				o = append(o, l.Text)
			}
			if l.Kind != Removed {
				n = append(n, l.Text)
			}
		}
	}
	return strings.Join(o, "\n") + "\n", strings.Join(n, "\n") + "\n"
}

// Playing reports changing content that needs another presentation frame.
func (c *Conversation) Playing() bool { return c.Animating || c.Session.LoadingOlder }

// Running reports an active last reply.
func (c *Conversation) Running() bool {
	n := len(c.Turns)
	return n > 0 && c.Turns[n-1].Reply.Status.Active()
}
