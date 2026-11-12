package client

import (
	"context"
	"encoding/json"
	"sync"
	"testing"
	"testing/synctest"
	"time"

	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// observerTestStream yields what a test sends: an update, or an error that
// fails the read.
type observerTestStream struct {
	events chan any
	closed chan struct{}
	once   sync.Once
}

func newObserverTestStream() *observerTestStream {
	return &observerTestStream{events: make(chan any, 8), closed: make(chan struct{})}
}

func (stream *observerTestStream) Next(ctx context.Context) (protocol.Update, error) {
	select {
	case event := <-stream.events:
		if err, failed := event.(error); failed {
			return protocol.Update{}, err
		}
		return event.(protocol.Update), nil
	case <-ctx.Done():
		return protocol.Update{}, ctx.Err()
	case <-stream.closed:
		return protocol.Update{}, context.Canceled
	}
}

func (stream *observerTestStream) Close() error {
	stream.once.Do(func() { close(stream.closed) })
	return nil
}

// observerTestTransport hands out its streams in order and records the
// topics asked for; with none left a subscription is unavailable.
type observerTestTransport struct {
	*clientTransport
	streams []*observerTestStream
	topics  []protocol.Topic
}

func (transport *observerTestTransport) Subscribe(_ context.Context, topic protocol.Topic) (link.Subscription, error) {
	transport.mu.Lock()
	defer transport.mu.Unlock()
	transport.topics = append(transport.topics, topic)
	if len(transport.streams) == 0 {
		return nil, fault(protocol.ErrorCodeUnavailable, "fixture topic unavailable")
	}
	stream := transport.streams[0]
	transport.streams = transport.streams[1:]
	return stream, nil
}

func (transport *observerTestTransport) subscribed() int {
	transport.mu.Lock()
	defer transport.mu.Unlock()
	return len(transport.topics)
}

func observerTestTransportWith(streams ...*observerTestStream) *observerTestTransport {
	return &observerTestTransport{clientTransport: &clientTransport{}, streams: streams}
}

// observerTestCode is the fault code of err, empty without one.
func observerTestCode(err error) protocol.ErrorCode {
	if err == nil {
		return ""
	}
	return faultCode(err)
}

// nextView reads and decodes the observer's next view.
func nextView[T any](t *testing.T, observer *Observer) T {
	t.Helper()
	body, err := observer.Next(t.Context())
	if err != nil {
		t.Fatal(err)
	}
	var view T
	if err := json.Unmarshal(body, &view); err != nil {
		t.Fatalf("%s: %v", body, err)
	}
	return view
}

func terminalTestScreen(rows, scrollback int) protocol.TerminalScreen {
	screen := protocol.DefaultTerminalScreen()
	screen.Columns = 80
	screen.Rows = make([]protocol.TerminalScreenLine, rows)
	screen.Scrollback = make([]protocol.TerminalScreenLine, scrollback)
	return screen
}

func terminalTestInfo(id protocol.TerminalID, revision uint64) protocol.TerminalInfo {
	info := protocol.DefaultTerminalInfo()
	info.ID, info.Revision, info.Status = id, revision, protocol.TerminalStatus{Kind: "running"}
	return info
}

func terminalTestSnapshot(node protocol.NodeID, id protocol.TerminalID, revision, sequence uint64, screen protocol.TerminalScreen) protocol.Update {
	return protocol.Update{Kind: "terminal_snapshot", Data: protocol.TerminalSnapshot{Node: node, Info: terminalTestInfo(id, revision), Sequence: sequence, Screen: screen}}
}

func terminalTestFrame(node protocol.NodeID, id protocol.TerminalID, sequence uint64, kind string, data any) protocol.Update {
	return protocol.Update{Kind: "terminal_frame", Data: protocol.TerminalFrame{Node: node, Info: terminalTestInfo(id, 1), Sequence: sequence, Screen: protocol.TerminalScreenScreenUpdate{Kind: kind, Data: data}}}
}

func TestTerminalProjection(t *testing.T) {
	t.Parallel()
	node, id := protocol.NodeID{1}, protocol.NewTerminalID()
	snapshot := func(revision, sequence uint64) protocol.Update {
		return terminalTestSnapshot(node, id, revision, sequence, terminalTestScreen(24, 2))
	}
	replace := func(sequence uint64) protocol.Update {
		return terminalTestFrame(node, id, sequence, "replace", protocol.TerminalScreenScreenUpdateReplacePayload{Screen: terminalTestScreen(24, 0)})
	}
	patch := func(sequence uint64, dropped uint32, appended int) protocol.Update {
		change := protocol.DefaultTerminalScreenPatch()
		change.Columns, change.Rows = 80, make([]protocol.TerminalScreenLine, 24)
		change.DroppedScrollbackRows, change.AppendedScrollback = dropped, make([]protocol.TerminalScreenLine, appended)
		return terminalTestFrame(node, id, sequence, "patch", protocol.TerminalScreenScreenUpdatePatchPayload{Patch: change})
	}
	stream := terminalTestScreen(0, 0)
	stream.VT = &protocol.TerminalVT{Viewport: protocol.TerminalViewport{Columns: 80, Rows: 24}, Bytes: protocol.Bytes("$ ")}
	type step struct {
		update    protocol.Update
		reconnect bool
		want      Apply
		code      protocol.ErrorCode
	}
	const none = ^uint64(0)
	for _, test := range []struct {
		name       string
		steps      []step
		sequence   uint64 // of the snapshot held at the end, none without one
		scrollback int
	}{
		{"frames follow their snapshot in order", []step{
			{update: snapshot(1, 0), want: Applied}, {update: replace(1), want: Applied},
			{update: replace(1), want: Ignored}, {update: replace(3), want: Recover},
			// Nothing is applied again before a new subscription's snapshot.
			{update: replace(2), want: Recover},
		}, 1, 0},
		{"a frame needs a snapshot", []step{{update: replace(1), want: Recover}}, none, 0},
		{"an older snapshot is ignored", []step{
			{update: snapshot(2, 5), want: Applied}, {update: snapshot(1, 9), want: Ignored},
			{update: snapshot(2, 4), want: Ignored}, {update: snapshot(2, 5), want: Applied},
		}, 5, 2},
		{"a new subscription numbers from its own snapshot", []step{
			{update: snapshot(2, 5), want: Applied},
			{update: replace(6), reconnect: true, want: Recover},
			{update: snapshot(2, 0), reconnect: true, want: Applied}, {update: replace(1), want: Applied},
		}, 1, 0},
		{"a patch keeps the scrollback it does not drop", []step{
			{update: snapshot(1, 0), want: Applied}, {update: patch(1, 1, 2), want: Applied},
			{update: patch(2, 9, 0), want: Recover},
		}, 1, 3},
		{"a reset asks for recovery", []step{
			{update: snapshot(1, 0), want: Applied}, {update: protocol.Update{Kind: "reset_required"}, want: Recover},
			{update: replace(1), want: Recover},
		}, 0, 2},
		{"a VT screen has no parsed rows", []step{{update: terminalTestSnapshot(node, id, 1, 0, stream), want: Applied}}, 0, 0},
		{"an invalid screen asks for recovery", []step{{update: terminalTestSnapshot(node, id, 1, 0, terminalTestScreen(0, 0)), want: Recover}}, none, 0},
		{"an undeclared screen change asks for recovery", []step{
			{update: snapshot(1, 0), want: Applied}, {update: terminalTestFrame(node, id, 1, "other", nil), want: Recover},
		}, 0, 2},
		{"another Node is refused", []step{{update: terminalTestSnapshot(protocol.NodeID{2}, id, 1, 0, terminalTestScreen(24, 0)), want: Ignored, code: protocol.ErrorCodeWrongTarget}}, none, 0},
		{"another terminal is refused", []step{
			{update: snapshot(1, 0), want: Applied},
			{update: terminalTestFrame(node, protocol.NewTerminalID(), 1, "replace", protocol.TerminalScreenScreenUpdateReplacePayload{Screen: terminalTestScreen(24, 0)}), want: Ignored, code: protocol.ErrorCodeWrongTarget},
		}, 0, 2},
		{"another update is refused", []step{{update: protocol.Update{Kind: "connections_changed"}, want: Ignored, code: protocol.ErrorCodeInvalidRequest}}, none, 0},
	} {
		t.Run(test.name, func(t *testing.T) {
			projection := newTerminalProjection(node, id)
			for index, step := range test.steps {
				if step.reconnect {
					projection.reconnect()
				}
				applied, err := projection.apply(step.update)
				if applied != step.want || observerTestCode(err) != step.code {
					t.Fatalf("step %d = %d, %v; want %d, %q", index, applied, err, step.want, step.code)
				}
			}
			switch held := projection.snapshot; {
			case test.sequence == none && held != nil:
				t.Fatalf("kept snapshot %d", held.Sequence)
			case test.sequence != none && (held == nil || held.Sequence != test.sequence || len(held.Screen.Scrollback) != test.scrollback):
				t.Fatalf("held %+v, want sequence %d with %d scrollback rows", held, test.sequence, test.scrollback)
			}
		})
	}
}

// terminalTestStream is a screen sent as a VT stream of the given size.
func terminalTestStream(rows uint16, bytes string) protocol.TerminalScreen {
	screen := terminalTestScreen(0, 0)
	screen.VT = &protocol.TerminalVT{Viewport: protocol.TerminalViewport{Columns: 80, Rows: rows}, Bytes: protocol.Bytes(bytes)}
	return screen
}

func terminalTestStreamFrame(node protocol.NodeID, id protocol.TerminalID, sequence uint64, rows uint16, bytes string) protocol.Update {
	return terminalTestFrame(node, id, sequence, "replace", protocol.TerminalScreenScreenUpdateReplacePayload{Screen: terminalTestStream(rows, bytes)})
}

// Every VT byte reaches the reader once and in order, however many updates
// arrive between two deliveries.
func TestTerminalProjectionKeepsStreamBytes(t *testing.T) {
	t.Parallel()
	node, id := protocol.NodeID{1}, protocol.NewTerminalID()
	projection := newTerminalProjection(node, id)
	apply := func(update protocol.Update, want Apply) {
		t.Helper()
		if applied, err := projection.apply(update); applied != want || err != nil {
			t.Fatalf("applied = %d, %v; want %d", applied, err, want)
		}
	}
	take := func(bytes string, restart bool, sequence uint64) {
		t.Helper()
		snapshot, restarted := projection.take()
		if snapshot == nil || string(snapshot.Screen.VT.Bytes) != bytes || restarted != restart || snapshot.Sequence != sequence {
			t.Fatalf("took %q restart %v of %+v; want %q restart %v at %d", snapshot.Screen.VT.Bytes, restarted, snapshot, bytes, restart, sequence)
		}
	}
	if snapshot, restart := projection.take(); snapshot != nil || restart {
		t.Fatalf("took %+v before a snapshot", snapshot)
	}
	apply(terminalTestSnapshot(node, id, 1, 0, terminalTestStream(24, "a")), Applied)
	apply(terminalTestStreamFrame(node, id, 1, 24, "b"), Applied)
	apply(terminalTestStreamFrame(node, id, 2, 24, "c"), Applied)
	take("abc", true, 2)
	take("", false, 2)
	apply(terminalTestStreamFrame(node, id, 3, 24, "d"), Applied)
	apply(terminalTestStreamFrame(node, id, 3, 24, "d"), Ignored)
	take("d", false, 3)

	// Another size starts over: what was drawn for the old one is not read.
	apply(terminalTestStreamFrame(node, id, 4, 24, "e"), Applied)
	apply(terminalTestStreamFrame(node, id, 5, 30, "f"), Applied)
	apply(terminalTestStreamFrame(node, id, 6, 30, "g"), Applied)
	take("fg", true, 6)

	// A gap keeps what came in order; the next subscription starts over.
	apply(terminalTestStreamFrame(node, id, 7, 30, "h"), Applied)
	apply(terminalTestStreamFrame(node, id, 9, 30, "lost"), Recover)
	take("h", false, 7)
	projection.reconnect()
	apply(terminalTestSnapshot(node, id, 1, 0, terminalTestStream(30, "i")), Applied)
	apply(terminalTestStreamFrame(node, id, 1, 30, "j"), Applied)
	take("ij", true, 1)

	// Past the bound the bytes go and the subscription recovers.
	full := string(make([]byte, terminalMaxPending))
	apply(terminalTestStreamFrame(node, id, 2, 30, full), Applied)
	apply(terminalTestStreamFrame(node, id, 3, 30, "k"), Recover)
	if !projection.stalled {
		t.Fatal("dropped bytes were not reported")
	}
	apply(terminalTestStreamFrame(node, id, 4, 30, "l"), Recover)
	take("", false, 2)
	projection.reconnect()
	// A starting frame is kept whatever its size.
	apply(terminalTestSnapshot(node, id, 1, 0, terminalTestStream(30, full+"m")), Applied)
	take(full+"m", true, 0)
}

func TestTerminalScreenValidation(t *testing.T) {
	t.Parallel()
	line := func(spans ...protocol.TerminalScreenSpan) []protocol.TerminalScreenLine {
		return []protocol.TerminalScreenLine{{Spans: spans}}
	}
	image := protocol.TerminalGraphicsImage{ID: 1, Width: 4, Height: 4}
	placement := protocol.TerminalGraphicsPlacement{Image: 1, Source: [4]uint32{0, 0, 4, 4}, Cell: [2]uint32{8, 16}}
	for _, test := range []struct {
		name   string
		change func(*protocol.TerminalScreen)
		valid  bool
	}{
		{"a grid", func(*protocol.TerminalScreen) {}, true},
		{"no columns", func(screen *protocol.TerminalScreen) { screen.Columns = 0 }, false},
		{"too many columns", func(screen *protocol.TerminalScreen) { screen.Columns = 501 }, false},
		{"no rows", func(screen *protocol.TerminalScreen) { screen.Rows = nil }, false},
		{"too many rows", func(screen *protocol.TerminalScreen) { screen.Rows = make([]protocol.TerminalScreenLine, 201) }, false},
		{"too much scrollback", func(screen *protocol.TerminalScreen) { screen.Scrollback = make([]protocol.TerminalScreenLine, 1001) }, false},
		{"a VT viewport outside the range", func(screen *protocol.TerminalScreen) {
			screen.Rows, screen.VT = nil, &protocol.TerminalVT{Viewport: protocol.TerminalViewport{Columns: 80, Rows: 201}}
		}, false},
		{"a cursor in the viewport", func(screen *protocol.TerminalScreen) {
			screen.Cursor = &protocol.TerminalScreenCursor{Column: 79, Row: 23}
		}, true},
		{"a cursor past the last column", func(screen *protocol.TerminalScreen) { screen.Cursor = &protocol.TerminalScreenCursor{Column: 80} }, false},
		{"a cursor past the last row", func(screen *protocol.TerminalScreen) { screen.Cursor = &protocol.TerminalScreenCursor{Row: 24} }, false},
		{"spans in order", func(screen *protocol.TerminalScreen) {
			screen.Scrollback = line(protocol.TerminalScreenSpan{Column: 0, Columns: 2}, protocol.TerminalScreenSpan{Column: 2, Columns: 78})
		}, true},
		{"overlapping spans", func(screen *protocol.TerminalScreen) {
			screen.Rows = line(protocol.TerminalScreenSpan{Column: 0, Columns: 2}, protocol.TerminalScreenSpan{Column: 1, Columns: 1})
		}, false},
		{"an empty span", func(screen *protocol.TerminalScreen) { screen.Rows = line(protocol.TerminalScreenSpan{Column: 0}) }, false},
		{"a span past the last column", func(screen *protocol.TerminalScreen) {
			screen.Rows = line(protocol.TerminalScreenSpan{Column: 79, Columns: 2})
		}, false},
		{"a placed image", func(screen *protocol.TerminalScreen) {
			screen.Graphics.Images, screen.Graphics.Placements = []protocol.TerminalGraphicsImage{image}, []protocol.TerminalGraphicsPlacement{placement}
		}, true},
		{"a repeated image", func(screen *protocol.TerminalScreen) {
			screen.Graphics.Images = []protocol.TerminalGraphicsImage{image, image}
		}, false},
		{"an image without pixels", func(screen *protocol.TerminalScreen) {
			screen.Graphics.Images = []protocol.TerminalGraphicsImage{{ID: 1, Height: 4}}
		}, false},
		{"images over the pixel budget", func(screen *protocol.TerminalScreen) {
			screen.Graphics.Images = []protocol.TerminalGraphicsImage{{ID: 1, Width: 2048, Height: 1025}}
		}, false},
		{"a placement without its image", func(screen *protocol.TerminalScreen) {
			screen.Graphics.Placements = []protocol.TerminalGraphicsPlacement{placement}
		}, false},
		{"a placement outside its image", func(screen *protocol.TerminalScreen) {
			outside := placement
			outside.Source = [4]uint32{1, 0, 4, 4}
			screen.Graphics.Images, screen.Graphics.Placements = []protocol.TerminalGraphicsImage{image}, []protocol.TerminalGraphicsPlacement{outside}
		}, false},
		{"a placement outside its tiles", func(screen *protocol.TerminalScreen) {
			tiled := placement
			tiled.Tile = &[4]uint32{2, 0, 2, 1}
			screen.Graphics.Images, screen.Graphics.Placements = []protocol.TerminalGraphicsImage{image}, []protocol.TerminalGraphicsPlacement{tiled}
		}, false},
	} {
		t.Run(test.name, func(t *testing.T) {
			screen := terminalTestScreen(24, 0)
			test.change(&screen)
			if validTerminalScreen(screen) != test.valid {
				t.Fatalf("valid = %v, want %v", !test.valid, test.valid)
			}
		})
	}
}

func TestTerminalObserver(t *testing.T) {
	synctest.Test(t, func(t *testing.T) {
		streams := []*observerTestStream{newObserverTestStream(), newObserverTestStream(), newObserverTestStream(), newObserverTestStream()}
		transport := observerTestTransportWith(streams...)
		controller := New(transport)
		id := protocol.NewTerminalID()
		argument, _ := json.Marshal(id)
		observer, err := controller.Watch(t.Context(), "watch_terminal", argument)
		if err != nil {
			t.Fatal(err)
		}
		defer func() { _ = observer.Close(); _ = controller.Close() }()
		node := controller.Target()
		screen := terminalTestScreen(24, 0)
		next := func() terminalView {
			t.Helper()
			synctest.Wait()
			return nextView[terminalView](t, observer)
		}
		// after reports the subscriptions made once the wait has passed.
		after := func(wait time.Duration) int {
			time.Sleep(wait)
			synctest.Wait()
			return transport.subscribed()
		}

		streams[0].events <- terminalTestSnapshot(node, id, 1, 0, screen)
		if view := next(); !view.Connected || view.Recovering || view.Error != nil || view.Snapshot == nil || view.Snapshot.Sequence != 0 {
			t.Fatalf("first snapshot = %+v", view)
		}
		if topic := transport.topics[0]; topic.Kind != "terminal" || topic.Data != any(id) {
			t.Fatalf("subscribed to %+v", topic)
		}
		streams[0].events <- terminalTestFrame(node, id, 1, "replace", protocol.TerminalScreenScreenUpdateReplacePayload{Screen: screen})
		if view := next(); !view.Connected || view.Snapshot.Sequence != 1 {
			t.Fatalf("frame = %+v", view)
		}

		// A gap keeps the screen, reports recovery and subscribes again.
		streams[0].events <- terminalTestFrame(node, id, 3, "replace", protocol.TerminalScreenScreenUpdateReplacePayload{Screen: screen})
		if view := next(); view.Connected || !view.Recovering || view.Error != nil || view.Snapshot == nil || view.Snapshot.Sequence != 1 {
			t.Fatalf("gap = %+v", view)
		}
		if after(249*time.Millisecond) != 1 || after(time.Millisecond) != 2 {
			t.Fatal("recovery did not subscribe again after 250 ms")
		}
		streams[1].events <- terminalTestSnapshot(node, id, 1, 0, screen)
		if view := next(); !view.Connected || view.Recovering || view.Snapshot.Sequence != 0 {
			t.Fatalf("recovered = %+v", view)
		}

		// A transient loss is retried: 250 ms after progress, then doubled.
		streams[1].events <- fault(protocol.ErrorCodeUnavailable, "route lost")
		if view := next(); view.Connected || view.Recovering || view.Error == nil || view.Error.Code != protocol.ErrorCodeUnavailable || view.Snapshot == nil {
			t.Fatalf("loss = %+v", view)
		}
		if after(249*time.Millisecond) != 2 || after(time.Millisecond) != 3 {
			t.Fatal("a loss after progress did not retry after 250 ms")
		}
		streams[2].events <- fault(protocol.ErrorCodeBusy, "Node busy")
		if view := next(); view.Connected || view.Error == nil || view.Error.Code != protocol.ErrorCodeBusy {
			t.Fatalf("busy = %+v", view)
		}
		if after(499*time.Millisecond) != 3 || after(time.Millisecond) != 4 {
			t.Fatal("a loss without progress did not double its delay")
		}

		// Any other failure is delivered once and ends the observer.
		streams[3].events <- fault(protocol.ErrorCodeNotFound, "terminal closed")
		if view := next(); view.Connected || view.Recovering || view.Error == nil || view.Error.Code != protocol.ErrorCodeNotFound || view.Snapshot == nil {
			t.Fatalf("failure = %+v", view)
		}
		for range 2 {
			if _, err := observer.Next(t.Context()); faultCode(err) != protocol.ErrorCodeNotFound {
				t.Fatalf("ended observer = %v", err)
			}
		}
		if after(10*time.Second) != 4 {
			t.Fatal("an ended observer subscribed again")
		}
	})
}

// The observer hands a VT screen's bytes to its reader in order, marks the
// deliveries that start the terminal over, and stops holding bytes for a
// reader that takes none.
func TestTerminalObserverDeliversStreamBytes(t *testing.T) {
	synctest.Test(t, func(t *testing.T) {
		streams := []*observerTestStream{newObserverTestStream(), newObserverTestStream(), newObserverTestStream()}
		transport := observerTestTransportWith(streams...)
		controller := New(transport)
		id := protocol.NewTerminalID()
		argument, _ := json.Marshal(id)
		observer, err := controller.Watch(t.Context(), "watch_terminal", argument)
		if err != nil {
			t.Fatal(err)
		}
		defer func() { _ = observer.Close(); _ = controller.Close() }()
		node := controller.Target()
		next := func() (terminalView, string) {
			t.Helper()
			synctest.Wait()
			view := nextView[terminalView](t, observer)
			if view.Snapshot == nil || view.Snapshot.Screen.VT == nil {
				t.Fatalf("view = %+v", view)
			}
			return view, string(view.Snapshot.Screen.VT.Bytes)
		}
		after := func(wait time.Duration) int {
			time.Sleep(wait)
			synctest.Wait()
			return transport.subscribed()
		}

		// Two frames between two reads deliver both runs in order.
		streams[0].events <- terminalTestSnapshot(node, id, 1, 0, terminalTestStream(24, "one "))
		if view, bytes := next(); !view.Reset || !view.Connected || bytes != "one " {
			t.Fatalf("snapshot = %q, %+v", bytes, view)
		}
		streams[0].events <- terminalTestStreamFrame(node, id, 1, 24, "two ")
		streams[0].events <- terminalTestStreamFrame(node, id, 2, 24, "three")
		if view, bytes := next(); view.Reset || bytes != "two three" || view.Snapshot.Sequence != 2 {
			t.Fatalf("frames = %q, %+v", bytes, view)
		}

		// A reconnect delivers no byte twice and then starts over.
		streams[0].events <- fault(protocol.ErrorCodeUnavailable, "route lost")
		if view, bytes := next(); view.Reset || view.Connected || bytes != "" {
			t.Fatalf("loss = %q, %+v", bytes, view)
		}
		if after(250*time.Millisecond) != 2 {
			t.Fatal("a loss did not subscribe again")
		}
		streams[1].events <- terminalTestSnapshot(node, id, 1, 0, terminalTestStream(24, "again "))
		streams[1].events <- terminalTestStreamFrame(node, id, 1, 24, "more")
		if view, bytes := next(); !view.Reset || !view.Connected || bytes != "again more" {
			t.Fatalf("reconnected = %q, %+v", bytes, view)
		}

		// A reader that takes nothing holds at most the bound: the bytes go,
		// and the next subscription waits for the reader.
		streams[1].events <- terminalTestStreamFrame(node, id, 2, 24, string(make([]byte, terminalMaxPending)))
		streams[1].events <- terminalTestStreamFrame(node, id, 3, 24, "over")
		if after(time.Minute) != 2 {
			t.Fatal("subscribed again for a reader that takes nothing")
		}
		if view, bytes := next(); view.Reset || view.Connected || !view.Recovering || bytes != "" {
			t.Fatalf("dropped = %d bytes, %+v", len(bytes), view)
		}
		synctest.Wait()
		if transport.subscribed() != 3 {
			t.Fatal("the reader's return did not subscribe again")
		}
		streams[2].events <- terminalTestSnapshot(node, id, 1, 0, terminalTestStream(24, "fresh"))
		if view, bytes := next(); !view.Reset || !view.Connected || view.Recovering || bytes != "fresh" {
			t.Fatalf("recovered = %q, %+v", bytes, view)
		}
	})
}
