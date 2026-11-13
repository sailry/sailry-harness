package live

import (
	"bytes"
	"context"
	"encoding/hex"
	"image"
	"image/png"
	"testing"
	"testing/synctest"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/desktop/shell"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
	"lukechampine.com/blake3"
)

type readRecoveryTransport struct {
	conversationTestTransport
	reject  bool
	counts  map[string]int
	streams map[string]*readRecoverySubscription
	image   []byte
}

type readRecoverySubscription struct {
	conversationTestSubscription
	failed chan struct{}
}

func (stream *readRecoverySubscription) Next(ctx context.Context) (protocol.Update, error) {
	select {
	case update := <-stream.updates:
		return update, nil
	case <-stream.failed:
		return protocol.Update{}, &protocol.Fault{Code: protocol.ErrorCodeInternal, Message: "observation interrupted"}
	case <-stream.closed:
		return protocol.Update{}, context.Canceled
	case <-ctx.Done():
		return protocol.Update{}, ctx.Err()
	}
}

func (source *readRecoveryTransport) Subscribe(ctx context.Context, topic protocol.Topic) (link.Subscription, error) {
	if topic.Kind != "installations" && topic.Kind != "python" && topic.Kind != "connections" {
		return source.conversationTestTransport.Subscribe(ctx, topic)
	}
	source.mu.Lock()
	defer source.mu.Unlock()
	source.counts[topic.Kind]++
	if source.reject {
		return nil, &protocol.Fault{Code: protocol.ErrorCodeInternal, Message: "initial observation refused"}
	}
	stream := &readRecoverySubscription{conversationTestSubscription: conversationTestSubscription{updates: make(chan protocol.Update, 1), closed: make(chan struct{})}, failed: make(chan struct{})}
	source.streams[topic.Kind] = stream
	update := protocol.Update{Kind: topic.Kind}
	switch topic.Kind {
	case "installations":
		update.Data = protocol.UpdateInstallationsPayload{Node: source.Target()}
	case "python":
		update.Data = protocol.UpdatePythonPayload{Node: source.Target(), Preparations: []protocol.PythonPreparation{{Package: "fixture", Stage: protocol.PythonStageInstalling}}}
	case "connections":
		connection := protocol.DefaultPluginConnectorConnection()
		connection.Connector = "fixture"
		update.Data = protocol.PluginConnectorChanges{Connections: []protocol.PluginConnectorConnection{connection}}
	}
	stream.updates <- update
	return stream, nil
}

type readRecoveryImageStream struct{ *bytes.Reader }

func (stream *readRecoveryImageStream) Write(data []byte) (int, error) { return len(data), nil }
func (*readRecoveryImageStream) Close() error                          { return nil }
func (*readRecoveryImageStream) CloseWrite() error                     { return nil }

func (source *readRecoveryTransport) Open(context.Context, protocol.StreamID) (link.Stream, error) {
	return &readRecoveryImageStream{Reader: bytes.NewReader(source.image)}, nil
}

func readRecoveryFixture(t *testing.T) (*Runtime, *readRecoveryTransport, func()) {
	t.Helper()
	snapshot := protocol.DefaultSnapshot()
	snapshot.Node = protocol.NodeID{1}
	source := &readRecoveryTransport{conversationTestTransport: conversationTestTransport{snapshot: snapshot}, reject: true, counts: map[string]int{}, streams: map[string]*readRecoverySubscription{}}
	source.handle = func(request protocol.Request) (link.Response, bool) {
		if request.Command.Kind != "read_python_settings" {
			return link.Response{}, false
		}
		source.counts["python_settings"]++
		if source.reject {
			return link.Response{Fault: &protocol.Fault{Code: protocol.ErrorCodeInternal, Message: "Python settings unavailable"}}, true
		}
		return link.Response{Output: protocol.Output{Kind: "python_settings", Data: protocol.PythonInfo{Version: "3.12.13", Settings: protocol.PythonSettings{Revision: 7}}}}, true
	}
	ctx, cancel := context.WithCancel(t.Context())
	controller := client.New(source)
	if err := controller.Start(ctx); err != nil {
		cancel()
		t.Fatal(err)
	}
	r := &Runtime{ctx: ctx, cancel: cancel, controller: controller, readsConnected: controller.Connected(), packages: newPackageState(), authorizations: &authorizationState{}, busy: map[string]bool{}, uncertain: map[string]protocol.Request{}, App: shell.NewLive(&viewmodel.Data{Workspace: &viewmodel.Workspace{}})}
	r.packages.selectedNode = snapshot.Node.String()
	r.initAttachments()
	// Only queued UI callbacks are under test. Keep the frame below the
	// raster worker threshold: its shared workers cannot join a synctest bubble.
	tester := ui.NewTesterWith(func(c *ui.Context) {
		theme.Apply(c, "", theme.Light)
		r.mu.Lock()
		queue := r.queue
		r.queue = nil
		r.mu.Unlock()
		for _, apply := range queue {
			apply(c)
		}
		r.resumeReadsOnReconnect()
	}, 1, 1, ui.TesterOptions{Preferences: ui.Preferences{ReduceTransparency: true}})
	settle := func() {
		for range 3 {
			synctest.Wait()
			tester.Frame()
		}
		synctest.Wait()
	}
	t.Cleanup(func() {
		cancel()
		r.closeAttachments()
		r.workers.Wait()
		r.observers.Wait()
		_ = controller.Close()
	})
	return r, source, settle
}

func TestReadRecovery(t *testing.T) {
	t.Run("failed subscriptions and Python settings wait for refresh or reconnect", func(t *testing.T) {
		synctest.Test(t, func(t *testing.T) {
			r, source, settle := readRecoveryFixture(t)
			r.observeInstallations()
			r.observePython()
			r.initConnectorUI()
			r.readPython()
			settle()
			assertCounts := func(want int) {
				t.Helper()
				source.mu.Lock()
				defer source.mu.Unlock()
				for _, kind := range []string{"installations", "python", "connections", "python_settings"} {
					if got := source.counts[kind]; got != want {
						t.Fatalf("%s reads = %d, want %d", kind, got, want)
					}
				}
			}
			if !r.packages.installFailed || !r.packages.pythonFailed || !r.packages.pythonReadFailed || !r.authorizations.connectionsFailed {
				t.Fatal("failed observations did not retain a recovery opportunity")
			}
			settle()
			assertCounts(1)
			source.mu.Lock()
			source.reject = false
			source.mu.Unlock()
			r.Refresh()
			settle()
			assertCounts(2)
			if !r.packages.connected || len(r.packages.preparations) != 1 || r.packages.pythonVersion != "3.12.13" || len(r.authorizations.connections) != 1 {
				t.Fatal("refresh did not restore the observations and Python settings")
			}
			for _, kind := range []string{"installations", "python", "connections"} {
				close(source.streams[kind].failed)
			}
			settle()
			assertCounts(2)
			// The frame that saw the disconnected Node records false. The
			// next connected frame reopens only the streams that ended.
			r.readsConnected = false
			settle()
			for _, kind := range []string{"installations", "python", "connections"} {
				if source.counts[kind] != 3 {
					t.Fatalf("%s did not resume after reconnect", kind)
				}
			}
			if source.counts["python_settings"] != 2 {
				t.Fatal("a successful read was repeated on reconnect")
			}
			settle()
			for _, kind := range []string{"installations", "python", "connections"} {
				if source.counts[kind] != 3 {
					t.Fatalf("%s reopened a healthy stream on another frame", kind)
				}
			}
		})
	})
	t.Run("picture refresh retries a transient read and keeps a permanent failure", func(t *testing.T) {
		synctest.Test(t, func(t *testing.T) {
			r, source, settle := readRecoveryFixture(t)
			var encoded bytes.Buffer
			if err := png.Encode(&encoded, image.NewNRGBA(image.Rect(0, 0, 2, 3))); err != nil {
				t.Fatal(err)
			}
			source.image = encoded.Bytes()
			digest := blake3.Sum256(source.image)
			attachment := protocol.Attachment{ID: protocol.AttachmentID{1}, Spec: protocol.AttachmentSpec{Worktree: protocol.WorktreeID{1}, Name: "picture.png", MediaType: "image/png", Size: uint64(len(source.image)), Revision: hex.EncodeToString(digest[:])}}
			failure := protocol.ErrorCodeBusy
			reads := 0
			respond := source.handle
			source.handle = func(request protocol.Request) (link.Response, bool) {
				if request.Command.Kind != "download_attachment" {
					return respond(request)
				}
				reads++
				if failure != "" {
					return link.Response{Fault: &protocol.Fault{Code: failure, Message: "picture unavailable"}}, true
				}
				return link.Response{Output: protocol.Output{Kind: "attachment_download", Data: protocol.AttachmentDownload{Stream: protocol.StreamID{1}, Attachment: attachment}}}, true
			}
			session := protocol.SessionID{1}.String()
			r.registerConversationAttachment(session, attachment)
			key := attachmentKey(session, attachment.ID.String())
			cache := r.attachmentImages(session)
			r.projectAttachmentImage(cache, key)
			settle()
			if reads != 1 || !cache.failed[key] || cache.loading[key] || cache.missing[key] {
				t.Fatal("a transient picture failure was left loading or marked missing")
			}
			for range 10 {
				r.projectAttachmentImage(cache, key)
			}
			settle()
			if reads != 1 {
				t.Fatal("drawing retried a failed picture")
			}
			failure = ""
			r.Refresh()
			settle()
			r.projectAttachmentImage(cache, key)
			settle()
			if reads != 2 || cache.images[key] == nil {
				t.Fatal("refresh did not recover the picture")
			}
			failure = protocol.ErrorCodeNotFound
			missing := attachmentKey(session, "missing")
			r.attachments.files[missing] = r.attachments.files[key]
			r.projectAttachmentImage(cache, missing)
			settle()
			r.Refresh()
			settle()
			r.projectAttachmentImage(cache, missing)
			r.projectAttachmentImage(cache, key)
			settle()
			if reads != 3 || !cache.missing[missing] || cache.images[key] == nil {
				t.Fatal("refresh repeated a permanently missing or already loaded picture")
			}
		})
	})
}

var _ link.Transport = (*readRecoveryTransport)(nil)
var _ link.Subscription = (*readRecoverySubscription)(nil)
