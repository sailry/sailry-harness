package preview

import (
	"testing"
	"time"
)

// The demo's queues are queued input only, the composer is offered each as
// it is, and the in-memory actions on their rows keep the queue in order.
func TestDemoQueue(t *testing.T) {
	for id, rows := range map[string]int{DemoStart: 5, "harbor-api/deploy": 2, "harbor-api/flaky-test": 0,
		"harbor-api/invoices": 0, "lumen-web/review": 0, "chat/translate": 0} {
		t.Run(id, func(t *testing.T) {
			for _, locale := range DemoLocales {
				d := Demo(locale, "local")
				c := d.Find(id)
				if c == nil {
					t.Fatal("not a demo conversation")
				}
				ctx := DemoComposer(d, c, c.Worktree)
				if len(c.Queue) != rows || len(ctx.Queue) != rows {
					t.Fatalf("%s: %d rows, the composer is offered %d, want %d", locale, len(c.Queue), len(ctx.Queue), rows)
				}
				for _, q := range c.Queue {
					if q.Text == "" && !q.Compaction {
						t.Errorf("%s: row %s has no text", locale, q.ID)
					}
				}
			}
		})
	}

	now := time.Unix(1_800_000_000, 0)
	t.Run("a queued message guides the run and returns when withdrawn", func(t *testing.T) {
		c := Demo("en", "local").Find(DemoStart)
		id := c.Queue[2].ID
		SendQueued(c, id, now)
		if !c.Queue[2].Guided {
			t.Fatalf("Send now: %+v", c.Queue[2])
		}
		WithdrawGuidance(c, id)
		if c.Queue[2].Guided || len(c.Queue) != 5 {
			t.Fatalf("Withdraw: %+v", c.Queue)
		}
	})
	t.Run("rows behind a run that waits", func(t *testing.T) {
		c := Demo("en", "local").Find("harbor-api/deploy")
		first, second := c.Queue[0].ID, c.Queue[1].ID
		SaveQueued(c, first, "Edited")
		MoveQueued(c, first, "")
		if c.Queue[1].ID != first || c.Queue[1].Text != "Edited" {
			t.Fatalf("edit and move: %+v", c.Queue)
		}
		RemoveQueued(c, second)
		Stop(c)
		if len(c.Queue) != 1 || c.Queue[0].ID != first || c.Running() {
			t.Fatalf("remove and Stop: %+v, running %v", c.Queue, c.Running())
		}
		// The row Stop left is sent as an ordinary message.
		turns := len(c.Turns)
		SendQueued(c, first, now)
		if len(c.Turns) != turns+1 || c.Turns[turns].User != "Edited" || len(c.Queue) != 0 || !c.Running() {
			t.Fatalf("send after Stop: %d turns, queue %+v, running %v", len(c.Turns), c.Queue, c.Running())
		}
	})
	// Every action on every row of every demo conversation leaves no row
	// guiding a reply that ended.
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
					for _, q := range DemoComposer(d, c, c.Worktree).Queue {
						if q.Guided && !c.Running() {
							t.Errorf("%s: row %s guides a reply that ended", id, q.ID)
						}
					}
				}
			}
		}
	})
}
