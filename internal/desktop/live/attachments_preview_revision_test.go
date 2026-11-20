package live

import (
	"fmt"
	"slices"
	"testing"
	"testing/synctest"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/conversation"
	"github.com/sailry/sailry-harness/internal/desktop/panel"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
	"lukechampine.com/blake3"
)

func TestArtifactVersionLoadFailure(t *testing.T) {
	synctest.Test(t, func(t *testing.T) {
		r, source, settle := readRecoveryFixture(t)
		artifact := protocol.DefaultArtifactSummary()
		artifact.ID, artifact.Revision = protocol.ArtifactID{1}, 1
		reads := []uint32{}
		source.handle = func(request protocol.Request) (link.Response, bool) {
			if request.Command.Kind != "read_artifact" {
				return link.Response{}, false
			}
			payload, _ := detail[protocol.CommandReadArtifactPayload](request.Command.Data)
			summary := artifact
			summary.Revision = *payload.Revision
			reads = append(reads, summary.Revision)
			if summary.Revision == 3 {
				return link.Response{Fault: &protocol.Fault{Code: protocol.ErrorCodeNotFound, Message: "revision unavailable"}}, true
			}
			content := protocol.ArtifactContent{Artifact: summary, LatestRevision: 3, Html: fmt.Sprintf("<p>%d</p>", summary.Revision)}
			return link.Response{Output: protocol.Output{Kind: "artifact", Data: content}}, true
		}
		file := conversation.File{ID: "preview/document", Path: "report.html", Kind: conversation.MarkdownFile}
		tab := panel.Tab{Kind: panel.Artifact, ID: file.ID}
		r.showCapturedPreview(file, attachmentDescriptor{artifact: &artifact})
		settle()
		assertShown := func(revision uint32) {
			t.Helper()
			if r.App.Panel.Active != tab || !slices.Contains(r.App.Panel.Tabs, tab) || r.App.Panel.Title(nil, tab) != file.Name() {
				t.Fatalf("lost the active tab for revision %d: %v", revision, r.App.Panel.Tabs)
			}
			if shown, latest := r.artifactVersions(file); shown != revision || latest != 3 {
				t.Fatalf("shown/latest = %d/%d, want %d/3", shown, latest, revision)
			}
			if target := r.attachments.previews[file.ID].artifact; target == nil || target.Revision != revision || target.ID != artifact.ID {
				t.Fatalf("revision %d actions target %+v", revision, target)
			}
			if got := r.attachments.previewContent[file.ID].Html; got != fmt.Sprintf("<p>%d</p>", revision) {
				t.Fatalf("revision %d content = %q", revision, got)
			}
		}
		assertShown(1)
		old := r.attachments.previewDocuments[file.ID].shown
		r.showArtifactVersion(file, 2)
		r.enqueue(func(*ui.Context) {
			// Let the read finish, but keep its UI callback queued until after
			// the next selection. Its success must not replace the shown version.
			r.workers.Wait()
			assertShown(1)
		})
		r.showArtifactVersion(file, 3)
		settle()
		assertShown(1)
		if old.ctx.Err() != nil || r.attachments.previewDocuments[file.ID].shown != old {
			t.Fatal("failed selection released the displayed document's images")
		}
		r.showArtifactVersion(file, 2)
		settle()
		assertShown(2)
		if old.ctx.Err() == nil || !slices.Equal(reads, []uint32{1, 2, 3, 2}) {
			t.Fatalf("successful retry did not replace the old document: reads=%v, old context=%v", reads, old.ctx.Err())
		}

		missing := file
		missing.ID += "/missing"
		unavailable := artifact
		unavailable.Revision = 3
		r.showCapturedPreview(missing, attachmentDescriptor{artifact: &unavailable})
		settle()
		if slices.Contains(r.App.Panel.Tabs, panel.Tab{Kind: panel.Artifact, ID: missing.ID}) || r.artifactRuntime(missing.ID) != nil || r.attachments.previewDocuments[missing.ID] != nil {
			t.Fatal("a failed first read left an empty document preview")
		}
	})
}

func TestDocumentPreviewTooLargeOnReload(t *testing.T) {
	synctest.Test(t, func(t *testing.T) {
		r, source, settle := readRecoveryFixture(t)
		source.image = []byte("original text")
		digest := fmt.Sprintf("%x", blake3.Sum256(source.image))
		file := conversation.File{ID: "preview/text", Path: "notes.txt", Kind: conversation.TextFile}
		tab := panel.Tab{Kind: panel.Artifact, ID: file.ID}
		descriptor := attachmentDescriptor{worktree: protocol.WorktreeID{1}, path: file.Path, revision: &digest}
		download := protocol.FileDownload{Worktree: descriptor.worktree, Path: file.Path, Stream: protocol.StreamID{1}, Revision: digest, Size: uint64(len(source.image))}
		cancels := 0
		source.handle = func(request protocol.Request) (link.Response, bool) {
			switch request.Command.Kind {
			case "download_file":
				return link.Response{Output: protocol.Output{Kind: "file_download", Data: download}}, true
			case "cancel_file_transfer":
				cancels++
			}
			return link.Response{}, false
		}
		r.showCapturedPreview(file, descriptor)
		settle()
		old := r.attachments.previewDocuments[file.ID]
		if old == nil || old.shown == nil {
			t.Fatal("original text preview did not load")
		}
		original := old.shown
		download.Size, download.Revision = attachmentPreviewMaximum+1, "oversized"
		file.Path, descriptor.path, download.Path = "updated.txt", "updated.txt", "updated.txt"
		descriptor.revision = &download.Revision
		r.showCapturedPreview(file, descriptor)
		settle()
		target := r.attachments.previews[file.ID]
		if r.App.Panel.Active != tab || r.App.Panel.Title(nil, tab) != "notes.txt" || original.ctx.Err() != nil || target.revision == nil || *target.revision != digest {
			t.Fatal("oversized replacement displaced the original text preview")
		}
		file.ID += "/large"
		r.showCapturedPreview(file, descriptor)
		settle()
		if cancels != 2 || slices.Contains(r.App.Panel.Tabs, panel.Tab{Kind: panel.Artifact, ID: file.ID}) || r.artifactRuntime(file.ID) != nil {
			t.Fatalf("an oversized first read retained its transfer or tab: cancellations=%d, tabs=%v", cancels, r.App.Panel.Tabs)
		}
	})
}
