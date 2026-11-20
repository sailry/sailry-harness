package files

import (
	"bytes"
	"context"
	"errors"
	"image"
	"image/color"
	"image/png"
	"io"
	"os"
	"path/filepath"
	"testing"
	"testing/synctest"
	"time"

	"github.com/sailry/sailry-harness/internal/core"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func syntheticThumbnailSource(t testing.TB, w, h int) []byte {
	t.Helper()
	img := image.NewNRGBA(image.Rect(0, 0, w, h))
	state := uint32(0x5a17)
	for i := 0; i < len(img.Pix); i += 4 {
		state ^= state << 13
		state ^= state >> 17
		state ^= state << 5
		img.Pix[i], img.Pix[i+1], img.Pix[i+2], img.Pix[i+3] = byte(state), byte(state>>8), byte(state>>16), 128
	}
	var result bytes.Buffer
	encoder := png.Encoder{CompressionLevel: png.BestSpeed}
	if err := encoder.Encode(&result, img); err != nil {
		t.Fatal(err)
	}
	return result.Bytes()
}

func readThumbnailTransfer(t testing.TB, transfers *Transfers, caller protocol.NodeID, user protocol.UserID, streamID protocol.StreamID) []byte {
	t.Helper()
	stream, err := transfers.Open(t.Context(), caller, user, streamID)
	if err != nil {
		t.Fatal(err)
	}
	defer stream.Close()
	if err := stream.CloseWrite(); err != nil {
		t.Fatal(err)
	}
	data, err := io.ReadAll(stream)
	if err != nil {
		t.Fatal(err)
	}
	return data
}

func TestThumbnailTransfersKeepOriginalFactsAndScope(t *testing.T) {
	transfers, root, caller := transferFixture(t)
	const edge = 128
	source := syntheticThumbnailSource(t, 800, 400)
	worktree, id, user := protocol.NewWorktreeID(), protocol.NewAttachmentID(), protocol.NewUserID()
	attachment := protocol.Attachment{ID: id, Spec: protocol.AttachmentSpec{Name: "picture.png", MediaType: "image/png", Size: uint64(len(source)), Revision: revision(source)}}
	if err := os.Mkdir(filepath.Join(transfers.profile, "attachments"), 0o700); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(transfers.profile, "attachments", id.String()), source, 0o600); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(root, attachment.Spec.Name), source, 0o600); err != nil {
		t.Fatal(err)
	}
	check := func(stream protocol.StreamID, facts *protocol.ThumbnailTransfer, owner protocol.UserID, scope StreamScope) {
		t.Helper()
		if got, found, err := transfers.Scope(caller, owner, stream); !found || err != nil || got != scope {
			t.Fatalf("scope = %+v, found %t, err %v, want %+v", got, found, err, scope)
		}
		if _, _, err := transfers.Scope(protocol.NodeID{2}, owner, stream); err == nil {
			t.Fatal("a second controller could use the cached thumbnail stream")
		}
		if scope.Draft {
			if _, _, err := transfers.Scope(caller, protocol.NewUserID(), stream); err == nil {
				t.Fatal("a second user of a controller could use the draft stream")
			}
		}
		data := readThumbnailTransfer(t, transfers, caller, owner, stream)
		if facts == nil || facts.Size != uint64(len(data)) || facts.Revision != revision(data) || len(data) >= len(source) {
			t.Fatalf("thumbnail facts = %+v, stream bytes = %d, original bytes = %d", facts, len(data), len(source))
		}
		decoded, err := png.Decode(bytes.NewReader(data))
		if err != nil {
			t.Fatal(err)
		}
		_, _, _, alpha := decoded.At(1, 1).RGBA()
		if got := decoded.Bounds().Size(); got != image.Pt(128, 64) || alpha != 128*257 {
			t.Fatalf("thumbnail dimensions = %v, alpha = %d", got, alpha)
		}
	}
	file, err := transfers.DownloadFileThumbnail(t.Context(), caller, root, worktree, attachment.Spec.Name, core.ManualFiles, edge)
	if err != nil || file.Size != attachment.Spec.Size || file.Revision != attachment.Spec.Revision || file.Stamp == "" {
		t.Fatalf("file source facts = %+v: %v", file, err)
	}
	check(file.Stream, file.Thumbnail, protocol.UserID{}, StreamScope{Worktree: worktree})
	draft, err := transfers.DownloadAttachmentThumbnail(t.Context(), caller, user, "", attachment, edge)
	if err != nil || draft.Attachment != attachment {
		t.Fatalf("draft source facts = %+v: %v", draft, err)
	}
	check(draft.Stream, draft.Thumbnail, user, StreamScope{Draft: true})
	attachment.Spec.Worktree = worktree
	canonical, err := transfers.DownloadBytes(t.Context(), caller, root, attachment, source, edge)
	if err != nil || canonical.Attachment != attachment {
		t.Fatalf("captured source facts = %+v: %v", canonical, err)
	}
	check(canonical.Stream, canonical.Thumbnail, protocol.UserID{}, StreamScope{Worktree: worktree})
	original, err := transfers.DownloadBytes(t.Context(), caller, root, attachment, source, 0)
	if err != nil || original.Thumbnail != nil || !bytes.Equal(readThumbnailTransfer(t, transfers, caller, protocol.UserID{}, original.Stream), source) {
		t.Fatalf("original was not preserved: %+v, %v", original, err)
	}
	corrupted := bytes.Clone(source)
	corrupted[0] ^= 1
	if _, err := transfers.DownloadBytes(t.Context(), caller, root, attachment, corrupted, edge); faultCodeForThumbnail(err) != protocol.ErrorCodeRevisionConflict {
		t.Fatalf("a cache hit trusted a stale original digest: %v", err)
	}
}

func faultCodeForThumbnail(err error) protocol.ErrorCode {
	var fault *protocol.Fault
	if errors.As(err, &fault) {
		return fault.Code
	}
	return ""
}

func TestThumbnailCacheBoundsExpiryAndCancellation(t *testing.T) {
	synctest.Test(t, func(t *testing.T) {
		transfers := NewTransfers(t.TempDir())
		t.Cleanup(func() { _ = transfers.Close() })
		cacheSize := func() (int, int) {
			transfers.mu.Lock()
			defer transfers.mu.Unlock()
			return len(transfers.thumbnails.cache), transfers.thumbnails.bytes
		}
		source := syntheticThumbnailSource(t, 256, 256)
		digest := revision(source)
		entry, err := transfers.thumbnailFor(t.Context(), source, digest, 256)
		if err != nil {
			t.Fatal(err)
		}
		if again, err := transfers.thumbnailFor(t.Context(), source, digest, 256); err != nil || again != entry {
			t.Fatalf("unchanged source was not reused: %v", err)
		}
		for edge := uint32(257); edge <= protocol.MaxThumbnailEdge; edge++ {
			if _, err := transfers.thumbnailFor(t.Context(), source, digest, edge); err != nil {
				t.Fatal(err)
			}
		}
		if entries, size := cacheSize(); entries == 0 || entries > thumbnailCacheEntries || size > thumbnailCacheBytes {
			t.Fatalf("cache retained %d entries and %d bytes", entries, size)
		}
		time.Sleep(thumbnailCacheTTL)
		synctest.Wait()
		if entries, size := cacheSize(); entries != 0 || size != 0 {
			t.Fatal("cache did not release bytes after its TTL without another read")
		}
		ctx, cancel := context.WithCancel(t.Context())
		cancel()
		if _, _, err := transfers.BeginThumbnail(ctx, 256); !errors.Is(err, context.Canceled) {
			t.Fatalf("cancelled preparation = %v", err)
		}
		_, err = transfers.thumbnailFor(ctx, source, digest, 256)
		if entries, size := cacheSize(); !errors.Is(err, context.Canceled) || entries != 0 || size != 0 {
			t.Fatalf("cancelled decode = %v", err)
		}
	})
	var wider bytes.Buffer
	imageTooWide := image.NewPaletted(image.Rect(0, 0, thumbnailSourceEdge+1, 1), color.Palette{color.Black})
	if err := png.Encode(&wider, imageTooWide); err != nil {
		t.Fatal(err)
	}
	if _, err := makeThumbnail(t.Context(), wider.Bytes(), 128); faultCodeForThumbnail(err) != protocol.ErrorCodeInvalidRequest {
		t.Fatalf("unbounded source dimensions: %v", err)
	}
}

func BenchmarkAttachmentThumbnailTransfer(b *testing.B) {
	root, err := filepath.EvalSymlinks(b.TempDir())
	if err != nil {
		b.Fatal(err)
	}
	source := syntheticThumbnailSource(b, 2560, 1600)
	digest := revision(source)
	const path = "benchmark.png"
	const edge = 480
	if err := os.WriteFile(filepath.Join(root, path), source, 0o600); err != nil {
		b.Fatal(err)
	}
	caller, worktree := protocol.NodeID{1}, protocol.NewWorktreeID()
	for _, mode := range []string{"original", "thumbnail-cold", "thumbnail-cached"} {
		b.Run(mode, func(b *testing.B) {
			transfers := NewTransfers(b.TempDir())
			b.Cleanup(func() { _ = transfers.Close() })
			b.ReportAllocs()
			if mode == "thumbnail-cached" {
				if _, err := transfers.thumbnailFor(b.Context(), source, digest, edge); err != nil {
					b.Fatal(err)
				}
			}
			var bytesRead int64
			for b.Loop() {
				var prepared protocol.FileDownload
				var err error
				if mode == "original" {
					prepared, err = transfers.DownloadFile(b.Context(), caller, root, worktree, path, core.ManualFiles)
				} else {
					if mode == "thumbnail-cold" {
						transfers.mu.Lock()
						transfers.dropThumbnailLocked(thumbnailCacheKey{digest, edge})
						transfers.mu.Unlock()
					}
					prepared, err = transfers.DownloadFileThumbnail(b.Context(), caller, root, worktree, path, core.ManualFiles, edge)
				}
				if err != nil {
					b.Fatal(err)
				}
				stream, err := transfers.Open(b.Context(), caller, protocol.UserID{}, prepared.Stream)
				if err != nil {
					b.Fatal(err)
				}
				if err := stream.CloseWrite(); err != nil {
					_ = stream.Close()
					b.Fatal(err)
				}
				bytesRead, err = io.Copy(io.Discard, stream)
				_ = stream.Close()
				if err != nil {
					b.Fatal(err)
				}
			}
			b.ReportMetric(float64(bytesRead), "stream-B/op")
		})
	}
}
