package live

import (
	"context"
	"errors"
	"testing"
	"testing/synctest"

	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/desktop/shell"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestExplicitWorkspaceRefreshRevalidatesDocumentImages(t *testing.T) {
	for _, connected := range []bool{false, true} {
		t.Run(map[bool]string{false: "offline retains images", true: "online revalidates images"}[connected], func(t *testing.T) {
			synctest.Test(t, func(t *testing.T) {
				source := &conversationTestTransport{snapshot: protocol.Snapshot{Node: protocol.NodeID{1}}}
				controller := client.New(source)
				defer controller.Close()
				if connected {
					if err := controller.Start(t.Context()); err != nil {
						t.Fatal(err)
					}
					synctest.Wait()
				}
				ctx, cancel := context.WithCancel(t.Context())
				defer cancel()
				root := &Runtime{ctx: ctx, controller: controller, busy: map[string]bool{}, uncertain: map[string]protocol.Request{}, App: shell.NewLive(&viewmodel.Data{Workspace: &viewmodel.Workspace{}})}
				root.initWorkspace()
				project := root.workspaceFor(protocol.Worktree{ID: protocol.WorktreeID{1}, Path: "/fixture"})
				const document = "docs/readme.md"
				revision := "unchanged document version"
				content := protocol.FileContent{Path: document, Text: "![Logo](../logo.png)", Revision: &revision}
				project.files[document] = content
				project.panel.SetFile(document, content.Text)
				options := project.documentMarkdown(document)
				images := project.documentImages[document]
				ready := make(chan struct{})
				close(ready)
				images.images[documentImageKey{origin: images.origin, path: "logo.png"}] = &documentImage{ready: ready, revision: "old image"}
				images.images[documentImageKey{origin: images.origin, path: "missing.png"}] = &documentImage{ready: ready, err: errors.New("not found")}
				project.fileLoaded(document, content)
				if project.documentMarkdown(document) != options {
					t.Fatal("an unchanged document invalidated its image cache without a refresh")
				}
				project.refresh()
				refreshed := project.documentMarkdown(document)
				if connected {
					if images.ctx.Err() == nil || refreshed == options || len(project.documentImages[document].images) != 0 {
						t.Fatal("explicit refresh reused stale image bytes or failed dependencies")
					}
				} else if refreshed != options || images.ctx.Err() != nil {
					t.Fatal("offline refresh discarded already loaded images")
				}
				cancel()
				root.workers.Wait()
				root.closeWorkspace()
			})
		})
	}
}
