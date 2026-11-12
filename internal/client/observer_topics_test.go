package client

import (
	"context"
	"encoding/json"
	"strings"
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

// loginTestView reads what both authorization views share.
type loginTestView struct {
	Update *struct {
		Revision uint64 `json:"revision"`
		State    struct {
			Kind string `json:"kind"`
		} `json:"state"`
	} `json:"update"`
	Connected bool            `json:"connected"`
	Error     *protocol.Fault `json:"error"`
}

func TestLoginProjection(t *testing.T) {
	t.Parallel()
	attempt := protocol.ConversationLoginAttempt{ID: protocol.NewRequestID(), Provider: protocol.NewProviderID()}
	update := func(revision uint64, state string) protocol.ConversationLoginUpdate {
		return protocol.ConversationLoginUpdate{Attempt: attempt, Revision: revision, State: protocol.ConversationLoginState{Kind: state}}
	}
	other := update(1, "starting")
	other.Attempt.ID = protocol.NewRequestID()
	type step struct {
		update protocol.ConversationLoginUpdate
		want   Apply
		code   protocol.ErrorCode
	}
	for _, test := range []struct {
		name     string
		steps    []step
		revision uint64 // of the update held at the end, 0 without one
	}{
		{"revisions advance", []step{{update(1, "starting"), Applied, ""}, {update(3, "exchanging"), Applied, ""}, {update(4, "connected"), Applied, ""}}, 4},
		{"another attempt is refused", []step{{other, Ignored, protocol.ErrorCodeInvalidRequest}}, 0},
		{"revision 0 is refused", []step{{update(0, "starting"), Ignored, protocol.ErrorCodeInvalidRequest}}, 0},
		{"an older revision is ignored", []step{{update(2, "exchanging"), Applied, ""}, {update(1, "starting"), Ignored, ""}}, 2},
		{"an equal revision is ignored", []step{{update(2, "exchanging"), Applied, ""}, {update(2, "exchanging"), Ignored, ""}}, 2},
		{"a changed equal revision is refused", []step{{update(2, "exchanging"), Applied, ""}, {update(2, "starting"), Ignored, protocol.ErrorCodeInvalidRequest}}, 2},
		{"a completed attempt cannot change", []step{{update(1, "cancelled"), Applied, ""}, {update(2, "starting"), Ignored, protocol.ErrorCodeInvalidRequest}}, 1},
	} {
		t.Run(test.name, func(t *testing.T) {
			projection := &loginProjection[protocol.ConversationLoginAttempt, protocol.ConversationLoginUpdate]{
				attempt: attempt,
				facts: func(update protocol.ConversationLoginUpdate) (protocol.ConversationLoginAttempt, uint64, string) {
					return update.Attempt, update.Revision, update.State.Kind
				},
			}
			for index, step := range test.steps {
				applied, err := projection.apply(step.update)
				if applied != step.want || observerTestCode(err) != step.code {
					t.Fatalf("step %d = %d, %v; want %d, %q", index, applied, err, step.want, step.code)
				}
			}
			if held := projection.update; test.revision == 0 && held != nil || test.revision != 0 && (held == nil || held.Revision != test.revision) {
				t.Fatalf("held %+v, want revision %d", held, test.revision)
			}
		})
	}
}

func TestLoginObserver(t *testing.T) {
	provider := protocol.ConversationLoginAttempt{ID: protocol.NewRequestID(), Provider: protocol.NewProviderID()}
	connector := protocol.PluginConnectorAttempt{ID: protocol.NewRequestID(), Connection: protocol.NewRequestID(), Package: protocol.PluginReference{Name: "mail", Digest: strings.Repeat("a", 64)}, Revision: 1}
	for _, test := range []struct {
		method, kind string
		attempt      any
		id           protocol.RequestID
		update       func(revision uint64, state string) any
	}{
		{"watch_login", "provider_login", provider, provider.ID, func(revision uint64, state string) any {
			return protocol.ConversationLoginUpdate{Attempt: provider, Revision: revision, State: protocol.ConversationLoginState{Kind: state}}
		}},
		{"watch_connector_login", "connector_login", connector, connector.ID, func(revision uint64, state string) any {
			return protocol.PluginConnectorLoginUpdate{Attempt: connector, Revision: revision, State: protocol.PluginConnectorLoginState{Kind: state}}
		}},
	} {
		t.Run(test.method, func(t *testing.T) {
			synctest.Test(t, func(t *testing.T) {
				streams := []*observerTestStream{newObserverTestStream(), newObserverTestStream()}
				transport := observerTestTransportWith(streams...)
				controller := New(transport)
				argument, _ := json.Marshal(test.attempt)
				observer, err := controller.Watch(t.Context(), test.method, argument)
				if err != nil {
					t.Fatal(err)
				}
				defer func() { _ = observer.Close(); _ = controller.Close() }()
				next := func() loginTestView {
					t.Helper()
					synctest.Wait()
					return nextView[loginTestView](t, observer)
				}
				send := func(stream *observerTestStream, revision uint64, state string) {
					stream.events <- protocol.Update{Kind: test.kind, Data: test.update(revision, state)}
				}

				send(streams[0], 1, "starting")
				if view := next(); !view.Connected || view.Error != nil || view.Update == nil || view.Update.Revision != 1 || view.Update.State.Kind != "starting" {
					t.Fatalf("first update = %+v", view)
				}
				if topic := transport.topics[0]; topic.Kind != test.kind || topic.Data != any(test.id) {
					t.Fatalf("subscribed to %+v", topic)
				}

				// A transient loss keeps the update and subscribes again.
				streams[0].events <- fault(protocol.ErrorCodeUnavailable, "route lost")
				if view := next(); view.Connected || view.Error == nil || view.Error.Code != protocol.ErrorCodeUnavailable || view.Update == nil {
					t.Fatalf("loss = %+v", view)
				}
				time.Sleep(250 * time.Millisecond)
				synctest.Wait()
				if transport.subscribed() != 2 {
					t.Fatal("a loss did not subscribe again after 250 ms")
				}
				// The Node repeats the update it holds: observed again, unchanged.
				send(streams[1], 1, "starting")
				if view := next(); !view.Connected || view.Error != nil || view.Update.Revision != 1 {
					t.Fatalf("resumed = %+v", view)
				}

				// The attempt's last state is delivered once and ends the observer.
				send(streams[1], 2, "cancelled")
				if view := next(); view.Connected || view.Error != nil || view.Update == nil || view.Update.Revision != 2 || view.Update.State.Kind != "cancelled" {
					t.Fatalf("completion = %+v", view)
				}
				if _, err := observer.Next(t.Context()); faultCode(err) != protocol.ErrorCodeUnavailable {
					t.Fatalf("ended observer = %v", err)
				}
				time.Sleep(10 * time.Second)
				synctest.Wait()
				if transport.subscribed() != 2 {
					t.Fatal("a completed attempt was subscribed to again")
				}
			})
		})
	}

	t.Run("refused updates end the observer", func(t *testing.T) {
		stream := newObserverTestStream()
		controller := New(observerTestTransportWith(stream))
		argument, _ := json.Marshal(provider)
		observer, err := controller.Watch(t.Context(), "watch_login", argument)
		if err != nil {
			t.Fatal(err)
		}
		defer func() { _ = observer.Close(); _ = controller.Close() }()
		stream.events <- protocol.Update{Kind: "connector_login", Data: protocol.PluginConnectorLoginUpdate{Attempt: connector, Revision: 1, State: protocol.PluginConnectorLoginState{Kind: "starting"}}}
		// The failure is in every view from here on; the read after the last fails.
		for {
			body, err := observer.Next(t.Context())
			if err != nil {
				if faultCode(err) != protocol.ErrorCodeInvalidRequest {
					t.Fatalf("ended observer = %v", err)
				}
				break
			}
			var view loginTestView
			if err := json.Unmarshal(body, &view); err != nil || view.Connected || view.Update != nil || view.Error == nil || view.Error.Code != protocol.ErrorCodeInvalidRequest {
				t.Fatalf("refusal = %s, %v", body, err)
			}
		}
	})

	t.Run("an attempt is authored input", func(t *testing.T) {
		controller := New(observerTestTransportWith())
		defer func() { _ = controller.Close() }()
		for _, method := range []string{"watch_login", "watch_connector_login"} {
			for _, argument := range []string{`{}`, `"attempt"`, `{"id":"` + provider.ID.String() + `"}`} {
				if _, err := controller.Watch(t.Context(), method, json.RawMessage(argument)); faultCode(err) != protocol.ErrorCodeInvalidRequest {
					t.Fatalf("%s %s = %v", method, argument, err)
				}
			}
		}
	})
}

func TestConnectionsObserver(t *testing.T) {
	t.Parallel()
	changes := func(cursor uint64) protocol.Update {
		value := protocol.DefaultPluginConnectorChanges()
		value.Cursor = cursor
		return protocol.Update{Kind: "connections_changed", Data: value}
	}
	for _, test := range []struct {
		name string
		last any
		code protocol.ErrorCode
	}{
		{"another update ends it", protocol.Update{Kind: "reset_required"}, protocol.ErrorCodeInvalidRequest},
		{"a failure ends it", fault(protocol.ErrorCodePermissionDenied, "pairing revoked"), protocol.ErrorCodePermissionDenied},
	} {
		t.Run(test.name, func(t *testing.T) {
			stream := newObserverTestStream()
			transport := observerTestTransportWith(stream)
			controller := New(transport)
			observer, err := controller.Watch(t.Context(), "watch_connections", json.RawMessage("null"))
			if err != nil {
				t.Fatal(err)
			}
			defer func() { _ = observer.Close(); _ = controller.Close() }()
			if topic := transport.topics[0]; topic.Kind != "connections" {
				t.Fatalf("subscribed to %+v", topic)
			}
			// Every payload is delivered, in order, however late it is read.
			stream.events <- changes(1)
			stream.events <- changes(2)
			stream.events <- test.last
			for cursor := uint64(1); cursor <= 2; cursor++ {
				view := nextView[struct {
					Cursor      uint64            `json:"cursor"`
					Types       []json.RawMessage `json:"types"`
					Connections []json.RawMessage `json:"connections"`
				}](t, observer)
				if view.Cursor != cursor || view.Types == nil || view.Connections == nil {
					t.Fatalf("payload %d = %+v", cursor, view)
				}
			}
			for range 2 {
				if _, err := observer.Next(t.Context()); faultCode(err) != test.code {
					t.Fatalf("ended observer = %v, want %q", err, test.code)
				}
			}
		})
	}

	t.Run("a refused subscription fails the watch", func(t *testing.T) {
		controller := New(observerTestTransportWith())
		defer func() { _ = controller.Close() }()
		if _, err := controller.Watch(t.Context(), "watch_connections", json.RawMessage("null")); faultCode(err) != protocol.ErrorCodeUnavailable {
			t.Fatalf("watch = %v", err)
		}
	})
}
