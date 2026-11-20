package live

import (
	"bytes"
	"context"
	"encoding/hex"
	"fmt"
	"image"
	"image/png"
	"testing"
	"testing/synctest"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/desktop/conversation"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/shell"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
	"lukechampine.com/blake3"
)

type readRecoveryTransport struct {
	conversationTestTransport
	reject    bool
	counts    map[string]int
	streams   map[string]*readRecoverySubscription
	image     []byte
	images    map[protocol.StreamID][]byte
	opens     map[protocol.StreamID]int
	openImage func(context.Context, protocol.StreamID) (link.Stream, error)
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

func (source *readRecoveryTransport) Open(ctx context.Context, id protocol.StreamID) (link.Stream, error) {
	if source.openImage != nil {
		return source.openImage(ctx, id)
	}
	source.mu.Lock()
	defer source.mu.Unlock()
	if source.opens != nil {
		source.opens[id]++
	}
	data := source.image
	if source.images != nil {
		data = source.images[id]
	}
	return &readRecoveryImageStream{Reader: bytes.NewReader(data)}, nil
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
			r.resolveAttachmentImage(cache, key, true)
			settle()
			if reads != 1 || !cache.failed[key] || cache.loading[key] != nil || cache.missing[key] {
				t.Fatal("a transient picture failure was left loading or marked missing")
			}
			for range 10 {
				r.resolveAttachmentImage(cache, key, true)
			}
			settle()
			if reads != 1 {
				t.Fatal("drawing retried a failed picture")
			}
			failure = ""
			r.Refresh()
			settle()
			r.resolveAttachmentImage(cache, key, true)
			settle()
			if reads != 2 || cache.images[key] == nil {
				t.Fatal("refresh did not recover the picture")
			}
			failure = protocol.ErrorCodeNotFound
			missing := attachmentKey(session, "missing")
			r.attachments.files[missing] = r.attachments.files[key]
			r.resolveAttachmentImage(cache, missing, true)
			settle()
			r.Refresh()
			settle()
			r.resolveAttachmentImage(cache, missing, true)
			r.resolveAttachmentImage(cache, key, true)
			settle()
			if reads != 3 || !cache.missing[missing] || cache.images[key] == nil {
				t.Fatal("refresh repeated a permanently missing or already loaded picture")
			}
			r.conversationSeen = map[protocol.SessionID]bool{{1}: true}
			r.pruneAttachmentImages()
			if cache.images[key] == nil {
				t.Fatal("visible bitmap released")
			}
			r.pruneAttachmentImages()
			if cache.images[key] != nil || !cache.missing[missing] {
				t.Fatal("hidden bitmap or missing-image state retained incorrectly")
			}

		})
	})
}

func TestVisibleDocumentThumbnailLifetime(t *testing.T) {
	synctest.Test(t, func(t *testing.T) {
		r, _, _ := readRecoveryFixture(t)
		session := protocol.SessionID{1}
		r.conversationSeen = map[protocol.SessionID]bool{session: true}
		r.attachments.native.RenderDocument = func(context.Context, protocol.ArtifactSummary, string, func([]byte, error)) {
			t.Fatal("cached preview rendered again")
		}
		artifact := protocol.ArtifactSummary{Kind: protocol.ArtifactKindPreview, Revision: 1}
		id := "published-preview"
		r.attachments.files[attachmentKey(session.String(), id)] = attachmentDescriptor{artifact: &artifact}
		key := fmt.Sprintf("%s/%s/%d", session, artifact.ID, artifact.Revision)
		ctx, cancel := context.WithCancel(t.Context())
		defer cancel()
		r.attachments.documents[key] = &attachmentDocumentImage{cancel: cancel, image: &conversation.Image{Width: 42}}
		if got := r.resolveConversationImage(session.String(), conversation.Image{ID: id}); got.Width != 42 {
			t.Fatal("visible file card did not resolve its published preview")
		}
		r.pruneAttachmentImages()
		if ctx.Err() != nil {
			t.Fatal("visible preview was released")
		}
		r.pruneAttachmentImages()
		if ctx.Err() == nil || len(r.attachments.documents) != 0 {
			t.Fatal("offscreen preview retained its renderer or bitmap")
		}
	})
}

func TestAttachmentViewerLoadsSelectedOriginal(t *testing.T) {
	synctest.Test(t, func(t *testing.T) {
		r, source, settle := readRecoveryFixture(t)
		var encoded bytes.Buffer
		if err := png.Encode(&encoded, image.NewNRGBA(image.Rect(0, 0, 1536, 240))); err != nil {
			t.Fatal(err)
		}
		source.image = encoded.Bytes()
		digest := blake3.Sum256(source.image)
		revision := hex.EncodeToString(digest[:])
		session, worktree, stream := protocol.SessionID{1}, protocol.WorktreeID{1}, protocol.StreamID{1}
		source.opens = map[protocol.StreamID]int{}
		const path = "picture.png"
		reads := 0
		respond := source.handle
		source.handle = func(request protocol.Request) (link.Response, bool) {
			if request.Command.Kind != "download_file" {
				return respond(request)
			}
			reads++
			return link.Response{Output: protocol.Output{Kind: "file_download", Data: protocol.FileDownload{
				Stream: stream, Worktree: worktree, Path: path, Size: uint64(len(source.image)), Revision: revision,
			}}}, true
		}
		r.conversationSeen = map[protocol.SessionID]bool{session: true}
		id := session.String()
		first, second := conversation.Image{ID: "first"}, conversation.Image{ID: "second"}
		for _, picture := range []conversation.Image{first, second} {
			r.registerWorkspaceFile(id, picture.ID, worktree, path, nil)
			r.resolveConversationImage(id, picture)
		}
		settle()
		first, second = r.resolveConversationImage(id, first), r.resolveConversationImage(id, second)
		if first.Width != 480 || first.Height != 75 || first.OriginalRevision != revision || reads != 2 {
			t.Fatalf("bounded thumbnail: %+v, reads=%d", first, reads)
		}
		if got := r.resolveConversationViewerImage(id, first); got != first.Source {
			t.Fatal("opening the viewer did not keep the thumbnail while loading")
		}
		settle()
		original := r.resolveConversationViewerImage(id, first)
		if width, height := ui.ImageSize(original); width != 1536 || height != 240 || reads != 3 {
			t.Fatalf("selected image: %gx%g, reads=%d", width, height, reads)
		}
		cache := r.attachmentImages(id)
		selected := cache.viewer
		// The workspace path now serves a different revision. Navigating to
		// its other card still shows that card's captured thumbnail.
		source.image = cardPNG(t, 32, 24)
		revision = fmt.Sprintf("%x", blake3.Sum256(source.image))
		if got := r.resolveConversationViewerImage(id, second); got != second.Source || selected.ctx.Err() == nil {
			t.Fatal("navigating retained the previous gallery selection")
		}
		settle()
		if got := r.resolveConversationViewerImage(id, second); got != second.Source || reads != 4 || source.opens[stream] != 3 {
			t.Fatalf("the viewer did not keep the captured revision: reads=%d, opens=%d", reads, source.opens[stream])
		}
		copied := false
		r.attachments.native.CopyImage = func([]byte, string) { copied = true }
		r.enqueue(func(c *ui.Context) {
			x := &conversationActions{r: r, c: c, cv: &viewmodel.Conversation{ID: id}}
			r.attachmentImageAction(x, kit.ImageCopy, second)
		})
		settle()
		if copied || reads != 5 || source.opens[stream] != 3 {
			t.Fatalf("copy did not keep the captured revision: copied=%t, reads=%d, opens=%d", copied, reads, source.opens[stream])
		}
		selected = cache.viewer
		r.pruneAttachmentImages()
		if cache.viewer != selected {
			t.Fatal("the visible viewer was released")
		}
		r.pruneAttachmentImages()
		if cache.viewer != nil || selected.ctx.Err() == nil {
			t.Fatal("closing the viewer retained the current selection")
		}
	})
}

var _ link.Transport = (*readRecoveryTransport)(nil)
var _ link.Subscription = (*readRecoverySubscription)(nil)
