package live

import (
	"fmt"
	"strings"
	"testing"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/desktop/composer"
	"github.com/sailry/sailry-harness/internal/desktop/conversation"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/shell"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestMountedQueueHandoffKeepsDockedTranscriptAnchor(t *testing.T) {
	const local, admitted, replacement = "On its way", "Admitted preview", "Next message"
	const tail = "Last visible reply"
	id, request := protocol.SessionID{1}, protocol.RequestID{2}
	page := cacheTestPage(id, 1, "Earlier input")
	var lines []string
	for i := range 60 {
		lines = append(lines, fmt.Sprintf("History line %02d", i))
	}
	page.Entries[1].Parts[0].Data = strings.Join(lines, "\n\n")
	last := cacheTestPage(id, 2, "Last input")
	last.Entries[1].Parts[0].Data = tail
	page.Runs = append(page.Runs, last.Runs...)
	page.Entries = append(page.Entries, last.Entries...)

	cv := &viewmodel.Conversation{ID: id.String()}
	snapshot := protocol.DefaultSnapshot()
	session := protocol.DefaultSession()
	session.ID = id
	snapshot.Sessions = []protocol.Session{session}
	controller := client.New(&conversationTestTransport{snapshot: snapshot})
	defer controller.Close()
	feed := &conversationFeed{changed: make(chan struct{}, 1), taken: make(chan struct{}, 1)}
	r := &Runtime{controller: controller, snapshotCopy: snapshot, snapshotRead: true,
		conversationFeeds: map[protocol.SessionID]*conversationFeed{id: feed},
		App:               shell.NewLive(&viewmodel.Data{Workspace: &viewmodel.Workspace{}, Unassigned: []*viewmodel.Conversation{cv}})}
	comp := r.App.Composer(cv.ID)
	publish := func(items ...protocol.ConversationPending) {
		page.Revision++
		page.Queue.Revision++
		page.Queue.Items = items
		state := client.ConversationState{Connected: true, Snapshot: protocol.ConversationSnapshot{Page: page}}
		feed.latest.Store(r.prepareConversation(id, state, "en"))
	}
	publish()
	tt := ui.NewTesterWith(func(c *ui.Context) {
		theme.Apply(c, "", theme.Light)
		r.beginConversationFrame()
		defer r.finishConversationFrame()
		r.conversationSeen[id] = true
		state, _ := r.conversationState(id)
		r.projectWatchedConversation(c, cv, id, state)
		x := &conversationActions{r: r, c: c, cv: cv, comp: comp}
		ctx := &composer.Context{Queue: cv.Queue, Activity: composer.Activity{Connected: true}}
		r.projectComposerQueue(x, ctx)
		ui.Column(c).Fill().Children(func() {
			conversation.Docked(c, r.App.Messages, cv.ID, cv.Turns, &cv.Session, x, func() { composer.View(c, comp, ctx, x) })
		})
	}, 1200, 640, ui.TesterOptions{Preferences: ui.Preferences{ReduceTransparency: true}})
	tt.SetLocale("en")
	tt.Frame()
	tt.Frame()
	find := func(text string) ui.Rect {
		t.Helper()
		b, ok := tt.Find(text)
		if !ok || b.H <= 0 {
			t.Fatalf("missing %q: %q", text, tt.Texts())
		}
		return b
	}
	near := func(a, b float32) bool { return a-b <= 0.5 && b-a <= 0.5 }
	list := r.App.Messages.List(cv.ID)
	empty := find(tail).Y
	field := func() ui.Rect {
		t.Helper()
		e, ok := tt.AccessByRole(ui.RoleTextField, i18n.In("en", "composer"))
		if !ok {
			t.Fatal("missing composer draft")
		}
		return e.Bounds
	}
	draft := field()
	// Sample animation time, not elapsed execution time. Include the first
	// frame of each change, not just settled geometry on either side.
	frames := func(check func()) {
		t.Helper()
		tt.Frame()
		check()
		for range 8 {
			tt.Advance(32 * time.Millisecond)
			tt.Frame()
			check()
		}
	}
	atEnd := func() float32 {
		t.Helper()
		b := find(tail)
		if now := field(); !list.AtEnd() || !near(now.Y, draft.Y) || b.Y < 0 || b.Y+b.H >= now.Y {
			t.Fatalf("end=%v, last reply %+v, draft %+v (was %+v)", list.AtEnd(), b, now, draft)
		}
		return b.Y
	}
	x := &conversationActions{r: r, cv: cv, comp: comp}
	x.outgoing(composer.Message{Text: local}, false).request = request
	previous := empty
	frames(func() {
		y := atEnd()
		if y > previous+0.5 {
			t.Fatalf("reply reversed while the sending row appeared: %.1f -> %.1f", previous, y)
		}
		previous = y
	})
	full := previous
	if full >= empty-0.5 || !tt.HasText(local) {
		t.Fatalf("sending row did not lift the reply: %.1f -> %.1f", empty, full)
	}

	edit := func(message string, disabled bool) ui.Rect {
		t.Helper()
		// The transcript also has Edit buttons. The queue row and its text
		// have the same accessible name; the wider box contains its actions.
		nodes := tt.AccessAll()
		var row ui.Rect
		for _, e := range nodes {
			if e.Name == message && e.Bounds.W > row.W {
				row = e.Bounds
			}
		}
		var button ui.AccessInfo
		count := 0
		for _, e := range nodes {
			b := e.Bounds
			if e.Role == ui.RoleButton && e.Name == i18n.In("en", "queue_edit") && b.W > 0 && b.H > 0 &&
				b.X >= row.X && b.X+b.W <= row.X+row.W && b.Y >= row.Y && b.Y+b.H <= row.Y+row.H {
				button = e
				count++
			}
		}
		if count != 1 || button.Disabled != disabled {
			t.Fatalf("%q row %+v: %d Edit buttons, disabled=%v; want one, disabled=%v", message, row, count, button.Disabled, disabled)
		}
		return button.Bounds
	}
	sendingRow := edit(local, true)
	// The page identifies the request before any command response. A gap or
	// overlap between its two row owners must not move the mounted transcript.
	publish(protocol.ConversationPending{Turn: protocol.TurnID{3}, Request: &request, Kind: "task", Ready: true, Preview: admitted})
	frames(func() {
		row := edit(admitted, false)
		if y := atEnd(); !near(y, full) || !near(row.Y, sendingRow.Y) || !tt.HasText(admitted) || tt.HasText(local) {
			t.Fatalf("queue handoff: reply %.1f, want %.1f; edit control %+v, was %+v; texts %q", y, full, row, sendingRow, tt.Texts())
		}
	})

	publish()
	intermediate := false
	frames(func() {
		y := atEnd()
		if y < previous-0.5 || y > empty+0.5 {
			t.Fatalf("reply reversed or overshot while the tray withdrew: %.1f -> %.1f (empty %.1f)", previous, y, empty)
		}
		intermediate = intermediate || y > full+0.5 && y < empty-0.5
		previous = y
	})
	if !intermediate || !near(previous, empty) || tt.HasText(admitted) {
		t.Fatalf("tray withdrawal: full %.1f, last %.1f, empty %.1f, intermediate=%v", full, previous, empty, intermediate)
	}

	tt.Scroll(100, 280, 0, -900)
	tt.Frame()
	var anchor string
	var before ui.Rect
	for _, line := range lines {
		if b, ok := tt.Find(line); ok && b.Y > 40 && b.Y+b.H < 280 {
			anchor, before = line, b
			break
		}
	}
	if anchor == "" || list.AtEnd() {
		t.Fatal("wheel did not expose an earlier line to anchor")
	}
	publish(protocol.ConversationPending{Turn: protocol.TurnID{4}, Kind: "task", Ready: true, Preview: replacement})
	frames(func() {
		if b := find(anchor); list.AtEnd() || !near(b.Y, before.Y) {
			t.Fatalf("returning tray moved %q: %.1f -> %.1f, end=%v", anchor, before.Y, b.Y, list.AtEnd())
		}
	})
	if !tt.HasText(replacement) {
		t.Fatal("the replacement message did not restore the tray")
	}
	list.ScrollToEnd()
	tt.Frame()
	if y := atEnd(); !near(y, full) {
		t.Fatalf("return to latest: reply %.1f, want %.1f above the restored tray", y, full)
	}
}
