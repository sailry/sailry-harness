package preview

import (
	"testing"
	"time"

	cv "github.com/sailry/sailry-harness/internal/desktop/conversation"
)

// The demo's conversation actions change the conversation in memory: a turn
// runs again, the conversation rewinds, compacts and undoes a turn's
// changes, each as the control says.
func TestDemoReplay(t *testing.T) {
	now := time.Unix(1_800_000_000, 0)
	const id = "harbor-api/flaky-test"
	played := func(t *testing.T, c *Conversation, at int) {
		t.Helper()
		Advance(c, now.Add(time.Second))
		if r := c.Turns[at].Reply; r.Status != cv.Running || c.Status != Running || !c.Running() {
			t.Fatalf("after a second the reply is %d, the conversation %d", r.Status, c.Status)
		}
		Advance(c, now.Add(time.Minute))
		r := c.Turns[at].Reply
		if r.Status != cv.Completed || c.Status != Idle || len(r.Blocks) != 2 || r.Blocks[1].Text == "" {
			t.Fatalf("the replayed reply ended %d with %d blocks", r.Status, len(r.Blocks))
		}
	}
	t.Run("Retry", func(t *testing.T) {
		c := Demo("en", "local").Find(id)
		first, before := c.Turns[0].ID, c.Turns[0].Reply
		Retry(c, first, now)
		if len(c.Turns) != 1 || c.Turns[0].Reply.Status != cv.Queued || c.Status != Running {
			t.Fatalf("Retry left %d turns, the reply %d", len(c.Turns), c.Turns[0].Reply.Status)
		}
		played(t, c, 0)
		if len(before.Blocks) == 0 || c.Turns[0].Reply.Blocks[1].Text == before.Blocks[len(before.Blocks)-1].Text {
			t.Fatal("the reply did not play again")
		}
		Retry(c, "no such turn", now)
		if len(c.Turns) != 1 || c.Running() {
			t.Fatal("an unknown turn ran")
		}
	})
	t.Run("Resend", func(t *testing.T) {
		c := Demo("en", "local").Find(id)
		c.Turns[0].Editing = &cv.Editing{Text: "draft"}
		Resend(c, c.Turns[0].ID, "Fix the other test", now)
		if c.Turns[0].User != "Fix the other test" || c.Turns[0].Editing != nil || len(c.Turns) != 1 {
			t.Fatalf("Resend: %q, editing %v, %d turns", c.Turns[0].User, c.Turns[0].Editing, len(c.Turns))
		}
		played(t, c, 0)
	})
	t.Run("Rewind", func(t *testing.T) {
		c := Demo("en", "local").Find(DemoStart)
		n := len(c.Turns)
		if n < 2 || !c.Running() {
			t.Fatalf("the demo's first conversation has %d turns, running %v", n, c.Running())
		}
		Rewind(c, c.Turns[n-1].ID)
		if len(c.Turns) != n {
			t.Fatal("rewinding to the last turn dropped turns")
		}
		Rewind(c, c.Turns[0].ID)
		if len(c.Turns) != 1 || c.Status != Idle || len(c.Waiting.Approvals) != 0 || c.Waiting.Question != nil {
			t.Fatalf("Rewind left %d turns, status %d, waiting %+v", len(c.Turns), c.Status, c.Waiting)
		}
	})
	t.Run("Compact", func(t *testing.T) {
		for _, locale := range DemoLocales {
			c := Demo(locale, "local").Find(id)
			n := len(c.Turns)
			Compact(c, now)
			last := &c.Turns[n]
			if len(c.Turns) != n+1 || last.Compaction == nil || last.Compaction.State != cv.CompactionRunning || !c.Running() {
				t.Fatalf("Compact added %d turns: %+v", len(c.Turns)-n, last.Compaction)
			}
			Compact(c, now)
			if len(c.Turns) != n+1 {
				t.Fatal("a second compaction started while one runs")
			}
			Advance(c, now.Add(time.Second))
			if c.Turns[n].Compaction.State != cv.CompactionRunning {
				t.Fatal("the compaction ended at once")
			}
			Advance(c, now.Add(compactFor))
			k := c.Turns[n].Compaction
			if k.State != cv.Compacted || k.Summary == "" || k.Summary == previewScript.summary || c.Running() || c.Status != Idle {
				t.Fatalf("%s: the compaction ended as %+v, status %d", locale, k, c.Status)
			}
		}
	})
	t.Run("Undo", func(t *testing.T) {
		c := Demo("en", "local").Find(id)
		at := -1
		for i := range c.Turns {
			if c.Turns[i].Reply.Changes != nil {
				at = i
			}
		}
		if at < 0 {
			t.Fatal("the conversation has no change card")
		}
		ch := c.Turns[at].Reply.Changes
		Undo(c, c.Turns[at].ID, ch.Files[0].Path)
		if !ch.Undone[ch.Files[0].Path] || ch.Undone[""] {
			t.Fatalf("one file's undo: %v", ch.Undone)
		}
		Undo(c, c.Turns[at].ID, "")
		if !ch.Undone[""] {
			t.Fatalf("Undo all: %v", ch.Undone)
		}
	})
	t.Run("Goal", func(t *testing.T) {
		c := Demo("en", "local").Find("lumen-web/pursuit")
		badge := c.Badges[0]
		if GoalAction(c, badge.ID, "") || GoalAction(c, "other", "goal-pause") {
			t.Fatal("an unknown goal button acted")
		}
		if !GoalAction(c, badge.ID, "goal-pause") || c.Badges[0].Panel.Buttons[0].ID != "goal-resume" || c.Badges[0].Segment[0].Text.Label == badge.Segment[0].Text.Label {
			t.Fatalf("Pause: %+v", c.Badges[0])
		}
		if len(badge.Panel.Buttons) != 2 || badge.Panel.Buttons[0].ID != "goal-pause" {
			t.Fatal("Pause changed the panel the badge had")
		}
		if !GoalAction(c, badge.ID, "goal-resume") || c.Badges[0].Panel.Buttons[0].ID != "goal-pause" {
			t.Fatalf("Resume: %+v", c.Badges[0])
		}
		if !GoalAction(c, badge.ID, "goal-end") || len(c.Badges) != 0 {
			t.Fatalf("End left %d badges", len(c.Badges))
		}
	})
}
