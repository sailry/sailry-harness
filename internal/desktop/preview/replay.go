package preview

import (
	"fmt"
	"slices"
	"time"

	"github.com/sailry/sailry-harness/internal/desktop/composer"
	cv "github.com/sailry/sailry-harness/internal/desktop/conversation"
)

// What the demo does to a conversation in memory where the application asks
// its Node: run a turn again, rewind to one, compact, undo a turn's changes.
// Like a send, each plays on the clock the view passes in and asks no model.

// compactFor is how long the played compaction runs.
const compactFor = 2 * time.Second

func turnIndex(c *Conversation, turn string) int {
	return slices.IndexFunc(c.Turns, func(t cv.Turn) bool { return t.ID == turn })
}

// settle ends whatever the conversation was playing or waiting on, as a
// rewind does to the turns it drops.
func settle(c *Conversation) {
	simulation(c).stream = nil
	c.Status, c.Waiting, c.Animating = Idle, composer.Interactions{}, false
	release(c)
}

// Retry runs a turn again: the turns after it leave, and its reply plays
// from the start.
func Retry(c *Conversation, turn string, now time.Time) {
	i := turnIndex(c, turn)
	if i < 0 {
		return
	}
	c.Turns = c.Turns[:i+1]
	t := &c.Turns[i]
	t.Reply, t.Editing = cv.Reply{Status: cv.Queued}, nil
	c.Waiting, c.Animating = composer.Interactions{}, true
	simulation(c).stream = &stream{start: now, turn: i}
	c.Status, c.Updated = Running, Now
}

// Resend replaces a turn's message and runs it again.
func Resend(c *Conversation, turn, text string, now time.Time) {
	if i := turnIndex(c, turn); i >= 0 {
		c.Turns[i].User = text
		Retry(c, turn, now)
	}
}

// Rewind drops the turns after a turn, which ends what they were running.
func Rewind(c *Conversation, turn string) {
	i := turnIndex(c, turn)
	if i < 0 || i == len(c.Turns)-1 {
		return
	}
	c.Turns = c.Turns[:i+1]
	settle(c)
}

// Compact adds a compaction turn that runs and then completes with the
// script's summary.
func Compact(c *Conversation, now time.Time) {
	if c.Running() {
		return
	}
	id := fmt.Sprintf("%s/compact-%d", c.ID, len(c.Turns))
	c.Turns = append(c.Turns, cv.Turn{ID: id, Compaction: &cv.Compaction{State: cv.CompactionRunning}, Reply: cv.Reply{Status: cv.Running}})
	c.Animating = true
	simulation(c).stream = &stream{start: now, turn: len(c.Turns) - 1}
	c.Status, c.Updated = Running, Now
}

// compacting moves a played compaction to where it is after d.
func compacting(c *Conversation, t *cv.Turn, play *script, d time.Duration) {
	t.Reply.Elapsed = d.Truncate(time.Second)
	if d < compactFor {
		return
	}
	t.Compaction = &cv.Compaction{State: cv.Compacted, Summary: play.summary, Elapsed: compactFor}
	t.Reply = cv.Reply{Status: cv.Completed, Elapsed: compactFor}
	c.Session.Usage.Context = min(c.Session.Usage.Context, 0.08)
	simulation(c).stream, c.Status, c.Animating = nil, Idle, false
}

// Undo marks a turn's changes undone: one file's, or with "" all of them.
func Undo(c *Conversation, turn, path string) {
	i := turnIndex(c, turn)
	if i < 0 || c.Turns[i].Reply.Changes == nil {
		return
	}
	ch := c.Turns[i].Reply.Changes
	if ch.Undone == nil {
		ch.Undone = map[string]bool{}
	}
	ch.Undone[path] = true
}
