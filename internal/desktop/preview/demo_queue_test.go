package preview

import (
	"testing"
	"time"

	"github.com/sailry/sailry-harness/internal/desktop/composer"
)

// The demo's queues say why their rows wait and which background results
// are held, the composer is offered both, and the in-memory actions on
// those rows behave as the Node's do.
func TestDemoQueue(t *testing.T) {
	waits := func(c *Conversation) (rows, held []composer.Wait) {
		for _, q := range c.Queue {
			rows = append(rows, q.Wait)
		}
		for _, h := range c.Held {
			held = append(held, h.Wait)
		}
		return rows, held
	}
	equal := func(a, b []composer.Wait) bool {
		if len(a) != len(b) {
			return false
		}
		for i := range a {
			if a[i] != b[i] {
				return false
			}
		}
		return true
	}
	for _, row := range []struct {
		id         string
		rows, held []composer.Wait
	}{
		{DemoStart, []composer.Wait{composer.WaitNone, composer.WaitNone, composer.WaitHeld, composer.WaitNone, composer.WaitNone}, nil},
		{"harbor-api/deploy", []composer.Wait{composer.WaitPaused, composer.WaitPaused}, nil},
		{"harbor-api/flaky-test", []composer.Wait{composer.WaitUnsent, composer.WaitPaused}, []composer.Wait{composer.WaitFailed, composer.WaitHeld}},
		{"harbor-api/invoices", []composer.Wait{composer.WaitStopped, composer.WaitStopped}, []composer.Wait{composer.WaitStopped}},
		{"lumen-web/review", nil, []composer.Wait{composer.WaitLimit, composer.WaitLimit}},
		{"chat/translate", []composer.Wait{composer.WaitInterrupted}, []composer.Wait{composer.WaitInterrupted}},
	} {
		t.Run(row.id, func(t *testing.T) {
			for _, locale := range DemoLocales {
				d := Demo(locale, "local")
				c := d.Find(row.id)
				if c == nil {
					t.Fatal("not a demo conversation")
				}
				rows, held := waits(c)
				if !equal(rows, row.rows) || !equal(held, row.held) {
					t.Fatalf("%s: rows wait on %v, results on %v", locale, rows, held)
				}
				ctx := DemoComposer(d, c, c.Worktree)
				if len(ctx.Queue) != len(c.Queue) || len(ctx.Held) != len(c.Held) {
					t.Fatalf("%s: the composer is offered %d rows and %d held results", locale, len(ctx.Queue), len(ctx.Held))
				}
				for _, q := range c.Queue {
					if q.Text == "" && !q.Compaction {
						t.Errorf("%s: row %s has no text", locale, q.ID)
					}
				}
				for _, h := range c.Held {
					if h.ID == "" || h.Title == "" {
						t.Errorf("%s: held result %+v is not named", locale, h)
					}
				}
			}
		})
	}

	now := time.Unix(1_800_000_000, 0)
	t.Run("send now from a paused queue resumes it", func(t *testing.T) {
		c := Demo("en", "local").Find("harbor-api/invoices")
		first, turns := c.Queue[0], len(c.Turns)
		SendQueued(c, first.ID, now)
		if len(c.Turns) != turns+1 || c.Turns[turns].User != first.Text || !c.Running() {
			t.Fatalf("the message was not sent: %d turns, running %v", len(c.Turns), c.Running())
		}
		if len(c.Queue) != 1 || c.Queue[0].Wait != composer.WaitNone || c.Held != nil {
			t.Fatalf("the queue stayed paused or kept its results: %+v, %+v", c.Queue, c.Held)
		}
		// Stopping the reply pauses what waits again.
		Stop(c)
		if c.Running() || c.Queue[0].Wait != composer.WaitStopped {
			t.Fatalf("Stop left %+v, running %v", c.Queue, c.Running())
		}
	})
	t.Run("a message that was not sent is sent again", func(t *testing.T) {
		c := Demo("en", "local").Find("harbor-api/flaky-test")
		unsent, turns := c.Queue[0], len(c.Turns)
		SendQueued(c, unsent.ID, now)
		if len(c.Turns) != turns+1 || c.Turns[turns].User != unsent.Text || len(c.Queue) != 1 || c.Queue[0].Wait != composer.WaitNone || len(c.Held) != 0 {
			t.Fatalf("sent %d turns, queue %+v, held %+v", len(c.Turns)-turns, c.Queue, c.Held)
		}
	})
	t.Run("a new message takes the held results", func(t *testing.T) {
		c := Demo("en", "local").Find("chat/translate")
		Send(c, "Go on", now)
		if c.Held != nil || c.Queue[0].Wait != composer.WaitNone {
			t.Fatalf("after a send: %+v, %+v", c.Queue, c.Held)
		}
	})
	t.Run("a held message guides the run and is held again when withdrawn", func(t *testing.T) {
		c := Demo("en", "local").Find(DemoStart)
		id := c.Queue[2].ID
		SendQueued(c, id, now)
		if !c.Queue[2].Guided || c.Queue[2].Wait != composer.WaitNone {
			t.Fatalf("Send now: %+v", c.Queue[2])
		}
		WithdrawGuidance(c, id)
		if c.Queue[2].Guided || c.Queue[2].Wait != composer.WaitHeld {
			t.Fatalf("Withdraw: %+v", c.Queue[2])
		}
		// A row that was not guiding is left as it is.
		WithdrawGuidance(c, c.Queue[0].ID)
		if c.Queue[0].Wait != composer.WaitNone {
			t.Fatalf("Withdraw changed a queued row: %+v", c.Queue[0])
		}
	})
	t.Run("a paused queue behind a run that waits", func(t *testing.T) {
		c := Demo("en", "local").Find("harbor-api/deploy")
		first, second := c.Queue[0].ID, c.Queue[1].ID
		SaveQueued(c, first, "Edited")
		MoveQueued(c, first, "")
		if c.Queue[1].ID != first || c.Queue[1].Text != "Edited" || c.Queue[1].Wait != composer.WaitPaused {
			t.Fatalf("edit and move: %+v", c.Queue)
		}
		RemoveQueued(c, second)
		Stop(c)
		if len(c.Queue) != 1 || c.Queue[0].Wait != composer.WaitPaused {
			t.Fatalf("remove and Stop: %+v", c.Queue)
		}
	})
	// Every action on every row of every demo conversation leaves the queue
	// in order, whatever the row waits on.
	t.Run("every action on every row", func(t *testing.T) {
		d := Demo("en", "local")
		var ids []string
		for _, p := range d.Projects {
			for _, c := range p.Conversations {
				ids = append(ids, c.ID)
			}
		}
		for _, c := range d.Unassigned {
			ids = append(ids, c.ID)
		}
		acts := []func(*Conversation, string){
			func(c *Conversation, id string) { SendQueued(c, id, now) },
			WithdrawGuidance,
			func(c *Conversation, id string) { SaveQueued(c, id, "Edited") },
			func(c *Conversation, id string) { MoveQueued(c, id, "") },
			RemoveQueued,
			func(c *Conversation, _ string) { Stop(c) },
			func(c *Conversation, _ string) { Send(c, "Next", now) },
		}
		for _, id := range ids {
			rows := len(d.Find(id).Queue)
			for i := range rows {
				for _, act := range acts {
					c := Demo("en", "local").Find(id)
					act(c, c.Queue[i].ID)
					Advance(c, now.Add(time.Minute))
					ctx := DemoComposer(d, c, c.Worktree)
					for _, q := range ctx.Queue {
						if q.Guided && q.Wait != composer.WaitNone {
							t.Errorf("%s: guiding row %s also waits on %d", id, q.ID, q.Wait)
						}
						if q.Guided && !c.Running() {
							t.Errorf("%s: row %s guides a reply that ended", id, q.ID)
						}
					}
				}
			}
		}
	})
}
