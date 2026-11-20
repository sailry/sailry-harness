package live

import (
	"bytes"
	"context"
	"fmt"
	"image"
	"image/png"
	"testing"
	"testing/synctest"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/conversation"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
	"lukechampine.com/blake3"
)

func cardPNG(t testing.TB, width, height int) []byte {
	t.Helper()
	var encoded bytes.Buffer
	if err := png.Encode(&encoded, image.NewNRGBA(image.Rect(0, 0, width, height))); err != nil {
		t.Fatal(err)
	}
	return encoded.Bytes()
}

func cardThumbnailDownload(worktree protocol.WorktreeID, path string, stream protocol.StreamID, data []byte) protocol.FileDownload {
	size, revision := uint64(len(data)), fmt.Sprintf("%x", blake3.Sum256(data))
	return protocol.FileDownload{
		Stream: stream, Worktree: worktree, Path: path, Size: size, Revision: revision,
		Thumbnail: &protocol.ThumbnailTransfer{Size: size, Revision: revision},
	}
}

func TestAttachmentCardDownloadsOnlyDerivativeUntilViewer(t *testing.T) {
	synctest.Test(t, func(t *testing.T) {
		r, source, settle := readRecoveryFixture(t)
		session, worktree := protocol.SessionID{1}, protocol.WorktreeID{1}
		original, derivative := cardPNG(t, 1536, 768), cardPNG(t, 480, 240)
		originalStream, derivativeStream := protocol.StreamID{1}, protocol.StreamID{2}
		source.images = map[protocol.StreamID][]byte{originalStream: original, derivativeStream: derivative}
		source.opens = map[protocol.StreamID]int{}
		originalRevision := fmt.Sprintf("%x", blake3.Sum256(original))
		const path = "picture.png"
		stamp := "captured stamp"
		calls := 0
		respond := source.handle
		source.handle = func(request protocol.Request) (link.Response, bool) {
			if request.Command.Kind != "download_file" {
				return respond(request)
			}
			calls++
			input, ok := detail[protocol.CommandDownloadFilePayload](request.Command.Data)
			if !ok || input.Worktree != worktree || input.Path != path {
				t.Errorf("download source = %+v", request.Command.Data)
			}
			download := protocol.FileDownload{Stream: originalStream, Worktree: worktree, Path: path, Size: uint64(len(original)), Revision: originalRevision, Stamp: stamp}
			if input.ThumbnailEdge != 0 {
				if input.ThumbnailEdge != attachmentThumbnailEdge {
					t.Errorf("requested edge = %d", input.ThumbnailEdge)
				}
				download.Stream = derivativeStream
				download.Thumbnail = &protocol.ThumbnailTransfer{Size: uint64(len(derivative)), Revision: fmt.Sprintf("%x", blake3.Sum256(derivative))}
			}
			return link.Response{Output: protocol.Output{Kind: "file_download", Data: download}}, true
		}
		r.conversationSeen = map[protocol.SessionID]bool{session: true}
		r.registerWorkspaceFile(session.String(), "first", worktree, path, &stamp)
		picture := conversation.Image{ID: "first"}
		r.resolveConversationImage(session.String(), picture)
		settle()
		seen := r.resolveConversationImage(session.String(), picture)
		if seen.Width != 480 || seen.Height != 240 || seen.OriginalRevision != originalRevision || source.opens[derivativeStream] != 1 || source.opens[originalStream] != 0 || calls != 1 {
			t.Fatalf("card = %+v, opened = %v, requests = %d", seen, source.opens, calls)
		}
		if got := r.resolveConversationImage(session.String(), picture); got.Source != seen.Source || calls != 1 {
			t.Fatal("another frame requested an already visible thumbnail")
		}
		if got := r.resolveConversationViewerImage(session.String(), seen); got != seen.Source {
			t.Fatal("viewer dropped the card while loading the original")
		}
		settle()
		viewer := r.resolveConversationViewerImage(session.String(), seen)
		if width, height := ui.ImageSize(viewer); width != 1536 || height != 768 || source.opens[originalStream] != 1 || calls != 2 {
			t.Fatalf("viewer = %gx%g, opened = %v, requests = %d", width, height, source.opens, calls)
		}
		r.pruneAttachmentImages()
		r.pruneAttachmentImages()
		for _, retained := range []ui.ImageSource{seen.Source, viewer} {
			if w, h := ui.ImageSize(retained); w != 0 || h != 0 {
				t.Fatalf("an old frame/gallery retained %gx%g pixels", w, h)
			}
		}
		if r.retainedThumbnailPixels() != 0 {
			t.Fatal("offscreen image did not release its budget")
		}
		facts := r.attachmentImages(session.String()).facts[attachmentKey(session.String(), seen.ID)]
		if facts.width != seen.Width || facts.height != seen.Height || facts.revision != seen.OriginalRevision {
			t.Fatalf("eviction changed the tile facts: %+v", facts)
		}
		seen.Source = nil // What an evicted member of the gallery keeps.
		r.resolveConversationViewerImage(session.String(), seen)
		settle()
		if w, h := ui.ImageSize(r.resolveConversationViewerImage(session.String(), seen)); w != 1536 || h != 768 || source.opens[originalStream] != 2 || source.opens[derivativeStream] != 1 {
			t.Fatalf("viewer after thumbnail eviction = %gx%g, streams = %v", w, h, source.opens)
		}
	})
}

func TestUnversionedWorkspaceThumbnailReloadsAfterEviction(t *testing.T) {
	for _, code := range []protocol.ErrorCode{protocol.ErrorCodeNotFound, protocol.ErrorCodeInvalidRequest} {
		t.Run(string(code), func(t *testing.T) {
			synctest.Test(t, func(t *testing.T) {
				r, source, settle := readRecoveryFixture(t)
				session, worktree, stream := protocol.SessionID{1}, protocol.WorktreeID{1}, protocol.StreamID{1}
				id, picture := session.String(), conversation.Image{ID: "picture"}
				const path = "picture.png"
				r.conversationSeen = map[protocol.SessionID]bool{session: true}
				r.registerWorkspaceFile(id, picture.ID, worktree, path, nil)
				cache, key := r.attachmentImages(id), attachmentKey(id, picture.ID)

				var failure protocol.ErrorCode
				var download protocol.FileDownload
				reads := 0
				respond := source.handle
				source.handle = func(request protocol.Request) (link.Response, bool) {
					if request.Command.Kind != "download_file" {
						return respond(request)
					}
					reads++
					input, ok := detail[protocol.CommandDownloadFilePayload](request.Command.Data)
					if !ok || input.Worktree != worktree || input.Path != path || input.ThumbnailEdge != attachmentThumbnailEdge {
						t.Errorf("thumbnail request = %+v", request.Command.Data)
					}
					if failure != "" {
						return link.Response{Fault: &protocol.Fault{Code: failure, Message: "picture unavailable"}}, true
					}
					return link.Response{Output: protocol.Output{Kind: "file_download", Data: download}}, true
				}

				for i, size := range []image.Point{{X: 16, Y: 9}, {X: 24, Y: 32}} {
					source.image = cardPNG(t, size.X, size.Y)
					download = cardThumbnailDownload(worktree, path, stream, source.image)
					want := attachmentImageFacts{width: size.X, height: size.Y, revision: download.Revision}
					r.resolveConversationImage(id, picture)
					settle()
					got := r.resolveConversationImage(id, picture)
					w, h := ui.ImageSize(got.Source)
					if int(w) != want.width || int(h) != want.height || got.Missing || got.Width != want.width || got.Height != want.height || got.OriginalRevision != want.revision || reads != i+1 {
						t.Fatalf("card at revision %d = %+v, pixels=%gx%g, reads=%d; want %+v, reads=%d", i+1, got, w, h, reads, want, i+1)
					}
					r.pruneAttachmentImages()
					r.pruneAttachmentImages()
					if w, h := ui.ImageSize(got.Source); w != 0 || h != 0 || cache.images[key] != nil || cache.facts[key] != want {
						t.Fatalf("eviction at revision %d: retained %gx%g pixels, facts=%+v; want %+v", i+1, w, h, cache.facts[key], want)
					}
				}

				failure = code
				r.resolveConversationImage(id, picture)
				settle()
				got := r.resolveConversationImage(id, picture)
				if _, hasFacts := cache.facts[key]; !got.Missing || got.Source != nil || got.Width != 0 || got.Height != 0 || got.OriginalRevision != "" || hasFacts {
					t.Fatalf("card after %s = %+v, retained facts=%t", code, got, hasFacts)
				}
				settle()
				if reads != 3 {
					t.Fatalf("reads after terminal failure = %d, want 3", reads)
				}
			})
		})
	}
}

func TestWorkspaceThumbnailDiscardsObsoleteCompletion(t *testing.T) {
	synctest.Test(t, func(t *testing.T) {
		r, source, settle := readRecoveryFixture(t)
		session, worktree := protocol.SessionID{1}, protocol.WorktreeID{1}
		id, picture := session.String(), conversation.Image{ID: "picture"}
		firstStream, nextStream := protocol.StreamID{1}, protocol.StreamID{2}
		source.images = map[protocol.StreamID][]byte{firstStream: cardPNG(t, 16, 9), nextStream: cardPNG(t, 24, 32)}
		first := cardThumbnailDownload(worktree, "first.png", firstStream, source.images[firstStream])
		next := cardThumbnailDownload(worktree, "next.png", nextStream, source.images[nextStream])
		downloads := map[string]protocol.FileDownload{first.Path: first, next.Path: next}
		reads := 0
		respond := source.handle
		source.handle = func(request protocol.Request) (link.Response, bool) {
			if request.Command.Kind != "download_file" {
				return respond(request)
			}
			reads++
			input, _ := detail[protocol.CommandDownloadFilePayload](request.Command.Data)
			return link.Response{Output: protocol.Output{Kind: "file_download", Data: downloads[input.Path]}}, true
		}
		r.conversationSeen = map[protocol.SessionID]bool{session: true}
		r.registerWorkspaceFile(id, picture.ID, worktree, first.Path, nil)
		cache, key := r.attachmentImages(id), attachmentKey(id, picture.ID)
		r.resolveConversationImage(id, picture)
		settle()
		r.pruneAttachmentImages()
		r.pruneAttachmentImages()
		if facts := cache.facts[key]; facts.revision != first.Revision || cache.images[key] != nil {
			t.Fatalf("starting without evicted thumbnail facts: %+v", facts)
		}

		opened, release := make(chan struct{}), make(chan struct{})
		source.openImage = func(ctx context.Context, stream protocol.StreamID) (link.Stream, error) {
			if stream == firstStream {
				close(opened)
				select {
				case <-release:
				case <-ctx.Done():
					return nil, ctx.Err()
				}
			}
			return &readRecoveryImageStream{Reader: bytes.NewReader(source.images[stream])}, nil
		}
		r.resolveConversationImage(id, picture)
		synctest.Wait()
		select {
		case <-opened:
		default:
			t.Fatal("old descriptor did not start reloading")
		}
		r.registerWorkspaceFile(id, picture.ID, worktree, next.Path, nil)
		close(release)
		settle()
		if cache.loading[key] != nil || cache.images[key] != nil || r.retainedThumbnailPixels() != 0 {
			t.Fatal("obsolete completion retained its load or pixels")
		}
		if got := r.resolveConversationImage(id, picture); got.Source != nil || got.Missing || got.Width != 0 || got.Height != 0 || got.OriginalRevision != "" {
			t.Fatalf("new descriptor projected old thumbnail facts: %+v", got)
		}
		settle()
		if got := r.resolveConversationImage(id, picture); got.Source == nil || got.Missing || got.Width != 24 || got.Height != 32 || got.OriginalRevision != next.Revision || reads != 3 {
			t.Fatalf("card after descriptor replacement = %+v, reads=%d; want 24x32, revision=%s, reads=3", got, reads, next.Revision)
		}
	})
}

func TestLongTurnThumbnailLoadsAfterScrollingPastFullBudget(t *testing.T) {
	r, source, _ := readRecoveryFixture(t)
	session, worktree, stream := protocol.SessionID{1}, protocol.WorktreeID{1}, protocol.StreamID{1}
	id := session.String()
	data := cardPNG(t, attachmentThumbnailEdge, attachmentThumbnailEdge)
	digest := fmt.Sprintf("%x", blake3.Sum256(data))
	source.image = data
	respond := source.handle
	source.handle = func(request protocol.Request) (link.Response, bool) {
		if request.Command.Kind != "download_file" {
			return respond(request)
		}
		input, _ := detail[protocol.CommandDownloadFilePayload](request.Command.Data)
		return link.Response{Output: protocol.Output{Kind: "file_download", Data: protocol.FileDownload{
			Stream: stream, Worktree: worktree, Path: input.Path, Size: uint64(len(data)), Revision: digest,
			Thumbnail: &protocol.ThumbnailTransfer{Size: uint64(len(data)), Revision: digest},
		}}}, true
	}
	r.conversationSeen = map[protocol.SessionID]bool{session: true}
	weight := thumbnailPixels(attachmentThumbnailEdge, attachmentThumbnailEdge)
	capacity := attachmentThumbnailBudget / weight
	pictures := make([]conversation.Image, capacity+5)
	for i := range pictures {
		name := fmt.Sprintf("picture-%02d.png", i+1)
		pictures[i] = conversation.Image{ID: name, Name: name}
		r.registerWorkspaceFile(id, name, worktree, name, nil)
	}
	turns := []conversation.Turn{{ID: "long", User: "Pictures", InputOnly: true, Controls: conversation.Controls{ReadOnly: true}, Images: pictures}}
	var requestPixels bool
	sess := &conversation.Session{ResolveImage: func(session string, image conversation.Image) conversation.Image {
		if requestPixels || image.ThumbnailFactsOnly() {
			return r.resolveConversationImage(session, image)
		}
		return image
	}}
	view := conversation.New()
	layout := view.List(id)
	layout.ScrollTo(0, ui.Start)
	const width, height = 220, 180
	tt := ui.NewTesterWith(func(c *ui.Context) {
		theme.Apply(c, "", theme.Light)
		r.mu.Lock()
		queue := r.queue
		r.queue = nil
		r.mu.Unlock()
		for _, apply := range queue {
			apply(c)
		}
		defer r.pruneAttachmentImages()
		defer view.FinishFrame()
		ui.Column(c).Fill().Children(func() { conversation.View(c, view, id, turns, sess, referenceViewActions{}) })
	}, width, height, ui.TesterOptions{Preferences: ui.Preferences{ReduceMotion: true, ReduceTransparency: true}})
	tt.Frame()
	cache := r.attachmentImages(id)
	last := pictures[len(pictures)-1]
	first, target := attachmentKey(id, pictures[0].ID), attachmentKey(id, last.ID)
	// Reproduce the old cache at saturation as if all earlier tiles in this
	// one mounted turn had kept their pixels. They were marked this pass.
	cache.visible = map[string]bool{}
	for _, picture := range pictures[:capacity] {
		key := attachmentKey(id, picture.ID)
		bitmap := ui.NewBitmap(image.NewRGBA(image.Rect(0, 0, attachmentThumbnailEdge, attachmentThumbnailEdge)))
		cache.images[key] = &attachmentThumbnail{bitmap: bitmap, originalRevision: digest, pixels: weight}
		r.attachments.thumbnailPixels += weight
		cache.sources[key] = r.attachments.files[key]
		cache.facts[key] = attachmentImageFacts{attachmentThumbnailEdge, attachmentThumbnailEdge, digest}
		cache.visible[key] = true
	}
	if pixels := r.retainedThumbnailPixels(); pixels > attachmentThumbnailBudget || pixels+weight <= attachmentThumbnailBudget {
		t.Fatalf("did not start at the thumbnail budget: %d", pixels)
	}
	old, facts := cache.images[first].bitmap, cache.facts[first]
	requestPixels = true
	layout.ScrollToEnd()
	tt.Frame()
	for range 5 {
		r.workers.Wait()
		tt.Frame()
		if r.retainedThumbnailPixels() > attachmentThumbnailBudget {
			t.Fatal("scrolling exceeded the window pixel budget")
		}
	}
	if box, ok := tt.Find(last.Name); !ok || box.W <= 0 || box.H <= 0 {
		t.Fatal("the last tile was not in the scrolled viewport")
	}
	if cache.images[target] == nil || r.retainedThumbnailPixels() == 0 {
		t.Fatal("the new visible tile stayed a skeleton behind the full cache")
	}
	if w, h := old.Size(); w != 0 || h != 0 || cache.images[first] != nil || cache.facts[first] != facts {
		t.Fatalf("old tile kept pixels or lost facts: %dx%d, %+v", w, h, cache.facts[first])
	}
}

func TestAttachmentThumbnailBudgetAcrossSessionsAndDocuments(t *testing.T) {
	synctest.Test(t, func(t *testing.T) {
		r, _, settle := readRecoveryFixture(t)
		var retained []*ui.Bitmap
		var expected int
		for index := range 40 {
			edge := attachmentThumbnailEdge
			if index%2 == 0 {
				edge = documentThumbnailEdge
			}
			key := fmt.Sprint(index)
			accepted := r.loadAttachmentThumbnail(t.Context(), edge, func(done attachmentThumbnailResult) {
				bitmap := ui.NewBitmap(image.NewRGBA(image.Rect(0, 0, edge, edge)))
				done(&attachmentThumbnail{bitmap: bitmap}, nil)
			}, func(thumb *attachmentThumbnail, err error) bool {
				if err != nil {
					t.Fatal(err)
				}
				retained = append(retained, thumb.bitmap)
				if index%2 == 0 {
					r.attachments.documents[key] = &attachmentDocumentImage{thumbnail: thumb}
				} else {
					r.attachmentImages(protocol.SessionID{byte(index)}.String()).images[key] = thumb
				}
				return true
			})
			weight := thumbnailPixels(edge, edge)
			if accepted != (expected+weight <= attachmentThumbnailBudget) {
				t.Fatalf("admission at %d retained + %d pixels = %t", expected, weight, accepted)
			}
			if accepted {
				expected += weight
			}
			settle()
		}
		if len(retained) == 0 || len(retained) == 40 || r.retainedThumbnailPixels() != expected {
			t.Fatalf("retained %d bitmaps and %d accounted pixels, want %d pixels", len(retained), r.retainedThumbnailPixels(), expected)
		}
		r.pruneAttachmentImages()
		if r.retainedThumbnailPixels() != 0 {
			t.Fatalf("retained budget after unmount: %d", r.retainedThumbnailPixels())
		}
		for _, bitmap := range retained {
			if w, h := bitmap.Size(); w != 0 || h != 0 {
				t.Fatalf("evicted bitmap still has %dx%d pixels", w, h)
			}
		}
	})
}

func TestAttachmentThumbnailRejectsOversizeDerivative(t *testing.T) {
	data := cardPNG(t, attachmentThumbnailEdge+1, 1)
	if _, err := decodeAttachmentThumbnail(data, "original", true); faultCode(err) != protocol.ErrorCodeInvalidRequest {
		t.Fatalf("larger-than-requested derivative = %v", err)
	}
	thumb, err := decodeAttachmentThumbnail(data, "original", false)
	if err != nil {
		t.Fatal(err)
	}
	if w, h := thumb.bitmap.Size(); w != attachmentThumbnailEdge || h != 1 || thumb.originalRevision != "original" {
		t.Fatalf("old Node's original fallback = %dx%d, %s", w, h, thumb.originalRevision)
	}
}

func TestAttachmentThumbnailRequiresSourceAndStreamRevisions(t *testing.T) {
	synctest.Test(t, func(t *testing.T) {
		r, source, settle := readRecoveryFixture(t)
		data := cardPNG(t, 100, 50)
		stream := protocol.StreamID{1}
		source.images = map[protocol.StreamID][]byte{stream: data}
		source.opens = map[protocol.StreamID]int{}
		worktree := protocol.WorktreeID{1}
		expected := fmt.Sprintf("%x", blake3.Sum256(cardPNG(t, 1000, 500)))
		current := "changed source"
		cancelled := 0
		respond := source.handle
		source.handle = func(request protocol.Request) (link.Response, bool) {
			switch request.Command.Kind {
			case "download_file":
				return link.Response{Output: protocol.Output{Kind: "file_download", Data: protocol.FileDownload{
					Stream: stream, Worktree: worktree, Path: "picture.png", Size: 2000, Revision: current,
					Thumbnail: &protocol.ThumbnailTransfer{Size: uint64(len(data)), Revision: expected},
				}}}, true
			case "cancel_file_transfer":
				cancelled++
				return link.Response{Output: protocol.Output{Kind: "file_transfer_cancelled", Data: protocol.OutputFileTransferCancelledPayload{Stream: stream}}}, true
			default:
				return respond(request)
			}
		}
		descriptor := attachmentDescriptor{worktree: worktree, path: "picture.png", revision: &expected}
		if _, err := r.readAttachmentThumbnail(t.Context(), descriptor); faultCode(err) != protocol.ErrorCodeRevisionConflict || source.opens[stream] != 0 || cancelled != 1 {
			t.Fatalf("stale source = %v, opened %v, cancelled %d", err, source.opens, cancelled)
		}
		current = expected
		if _, err := r.readAttachmentThumbnail(t.Context(), descriptor); err == nil || source.opens[stream] != 1 {
			t.Fatalf("bad derivative digest = %v, opened %v", err, source.opens)
		}
		settle()
		if _, _, _, err := r.prepareAttachmentDownload(t.Context(), descriptor); faultCode(err) != protocol.ErrorCodeInvalidRequest || source.opens[stream] != 1 {
			t.Fatalf("an original action accepted a derivative = %v, opened %v", err, source.opens)
		}
	})
}

func TestOffscreenAttachmentThumbnailCancelsPendingTransfer(t *testing.T) {
	synctest.Test(t, func(t *testing.T) {
		r, source, settle := readRecoveryFixture(t)
		session, worktree := protocol.SessionID{1}, protocol.WorktreeID{1}
		stream := protocol.StreamID{1}
		opened := make(chan struct{})
		source.openImage = func(ctx context.Context, id protocol.StreamID) (link.Stream, error) {
			if id != stream {
				t.Errorf("opened unexpected stream %s", id)
			}
			close(opened)
			<-ctx.Done()
			return nil, ctx.Err()
		}
		cancelled := 0
		respond := source.handle
		source.handle = func(request protocol.Request) (link.Response, bool) {
			switch request.Command.Kind {
			case "download_file":
				digest := fmt.Sprintf("%x", blake3.Sum256([]byte("fixture")))
				return link.Response{Output: protocol.Output{Kind: "file_download", Data: protocol.FileDownload{
					Stream: stream, Worktree: worktree, Path: "picture.png", Revision: digest, Size: 7,
					Thumbnail: &protocol.ThumbnailTransfer{Size: 7, Revision: digest},
				}}}, true
			case "cancel_file_transfer":
				cancelled++
				return link.Response{Output: protocol.Output{Kind: "file_transfer_cancelled", Data: protocol.OutputFileTransferCancelledPayload{Stream: stream}}}, true
			default:
				return respond(request)
			}
		}
		r.conversationSeen = map[protocol.SessionID]bool{session: true}
		r.registerWorkspaceFile(session.String(), "first", worktree, "picture.png", nil)
		r.resolveConversationImage(session.String(), conversation.Image{ID: "first"})
		synctest.Wait()
		select {
		case <-opened:
		default:
			t.Fatal("visible card did not start its thumbnail stream")
		}
		r.pruneAttachmentImages()
		r.pruneAttachmentImages()
		settle()
		if cancelled != 1 || r.retainedThumbnailPixels() != 0 || len(r.attachments.imageSlots) != 0 {
			t.Fatalf("offscreen request: cancelled=%d, reserved pixels=%d, slots=%d", cancelled, r.retainedThumbnailPixels(), len(r.attachments.imageSlots))
		}
	})
}

func BenchmarkAttachmentThumbnailDecode(b *testing.B) {
	for _, mode := range []struct {
		name string
		data []byte
		edge int
	}{
		{"original-stream", cardPNG(b, 3072, 2048), 1024},
		{"derived-stream", cardPNG(b, 480, 320), attachmentThumbnailEdge},
	} {
		b.Run(mode.name, func(b *testing.B) {
			b.ReportAllocs()
			var bitmap *ui.Bitmap
			for b.Loop() {
				var err error
				bitmap, err = decodeAttachmentBitmap(mode.data, mode.edge)
				if err != nil {
					b.Fatal(err)
				}
			}
			b.ReportMetric(float64(len(mode.data)), "stream-B/op")
			b.ReportMetric(float64(thumbnailPixels(bitmap.Size())), "retained-pixels/op")
		})
	}
}
