package live

import (
	"context"
	"encoding/json"
	"fmt"
	"runtime"
	"strings"
	"sync/atomic"
	"testing"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/desktop/conversation"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/preferences"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
	"lukechampine.com/blake3"
)

func TestDesktopLargeToolDetailLifecycle(t *testing.T) {
	const bodyMarker = "Full tool result sentinel"
	const turnCount = 32
	// The marker heads the result, so it stays in the preview an open
	// row shows.
	body := bodyMarker + "\n" + strings.Repeat("A result line from the tool.\n", 6000)
	first, other := protocol.SessionID{1}, protocol.SessionID{2}
	node, worktree := protocol.NodeID{3}, protocol.WorktreeID{4}
	card := protocol.PluginCardSummary{
		Node: node, Worktree: worktree,
		Receipt: protocol.PluginCardReceipt{Session: first, Turn: protocol.TurnID{1}, Call: protocol.RequestID{1}, Block: 2},
		Package: protocol.PluginReference{Name: "detail-fixture", Digest: strings.Repeat("a", 64)},
		Key:     "fixture", Props: json.RawMessage(`{"title":"Tool card"}`),
		Fallback: protocol.PluginDesktopNavigation{Label: "Closed details should not bind this card"},
	}
	const imagePath = "output/chart.png"
	imageRevision := strings.Repeat("c", 64)
	file := protocol.ToolContentBlock{Kind: "file", Data: protocol.ToolContentFile{Path: imagePath, Mime: "image/png", Revision: &imageRevision}}
	content := &protocol.ToolContent{Version: 1, Blocks: []protocol.ToolContentBlock{
		{Kind: "text", Data: protocol.ToolContentText{Path: "/text"}}, file, {Kind: "card", Data: card},
	}}
	raw, err := json.Marshal(map[string]any{"text": body})
	if err != nil {
		t.Fatal(err)
	}
	encoded, err := json.Marshal(protocol.ToolDetail{Arguments: json.RawMessage("null"), Result: new(json.RawMessage(raw)), Content: content})
	if err != nil {
		t.Fatal(err)
	}
	ref := protocol.ToolDetailReference{Session: first, Turn: protocol.TurnID{1}, Entry: "large-result", Call: new("large-call"), Revision: strings.Repeat("b", 64)}
	summary := &protocol.ToolDetailSummary{Revision: ref.Revision, HasDetails: true, Published: true,
		Publications: []protocol.ToolDetailPublication{{Index: 1, Block: file}}}
	snapshot := protocol.DefaultSnapshot()
	snapshot.Node = node
	snapshot.Worktrees = []protocol.Worktree{{ID: worktree, Path: t.TempDir()}}
	for _, id := range []protocol.SessionID{first, other} {
		s := protocol.DefaultSession()
		s.ID, s.Revision, s.Worktree = id, 1, worktree
		snapshot.Sessions = append(snapshot.Sessions, s)
	}
	page := protocol.DefaultConversationPage()
	page.Session, page.Revision = first, 1
	for i := range turnCount {
		row := cacheTestPage(first, byte(i+1), fmt.Sprintf("Question %02d", i+1))
		row.Runs[0].Worktree = worktree
		if i == 0 {
			row.Entries[1].Parts = append([]protocol.ConversationPart{{Kind: "tool_call", Data: protocol.ConversationPartToolCallPayload{
				ID: ref.Call, Name: "detail_fixture", Grouping: protocol.ToolGroupingStandalone,
			}}}, row.Entries[1].Parts...)
			row.Entries = append(row.Entries, protocol.ConversationEntry{ID: ref.Entry, Turn: ref.Turn, Sequence: row.Entries[1].Sequence + 1, Author: "tool",
				Parts: []protocol.ConversationPart{{Kind: "tool_result", Data: protocol.ConversationPartToolResultPayload{ID: ref.Call, Name: "detail_fixture", Summary: summary}}}})
		}
		for _, entry := range row.Entries {
			entry.Sequence = uint64(len(page.Entries) + 1)
			page.Entries = append(page.Entries, entry)
		}
		page.Runs = append(page.Runs, row.Runs...)
	}
	conversationSnapshot := func(page protocol.ConversationPage) protocol.ConversationSnapshot {
		s := protocol.DefaultConversationSnapshot()
		s.Node, s.Sequence, s.Page = node, 100, page
		return s
	}
	transport := &largeDetailTransport{
		desktopHistoryTransport: desktopHistoryTransport{png: cardPNG(t, 32, 32), liveWorkspaceBenchmarkTransport: liveWorkspaceBenchmarkTransport{
			conversationTestTransport: conversationTestTransport{snapshot: snapshot},
			conversations: map[protocol.SessionID]protocol.ConversationSnapshot{
				first: conversationSnapshot(page), other: conversationSnapshot(cacheTestPage(other, 1, "Other page")),
			}},
		},
		ref: ref, body: string(encoded), gates: make(chan detailCompletionGate, 1), worktree: worktree, imagePath: imagePath, imageRevision: imageRevision,
	}
	controller := client.New(transport)
	if err := controller.Start(t.Context()); err != nil {
		t.Fatal(err)
	}
	defer controller.Close()
	r, err := NewWithPreferences(t.Context(), controller, preferences.Open(t.TempDir()+"/preferences.json"))
	if err != nil {
		t.Fatal(err)
	}
	defer r.Close()
	r.App.Effects = false
	prefs := r.App.Prefs()
	prefs.NavOpen, prefs.PanelOpen, prefs.OverviewHidden = false, false, true
	r.App.SetPrefs(prefs)
	tester := ui.NewTesterWith(r.App.View, 1120, 700, ui.TesterOptions{Preferences: ui.Preferences{ReduceMotion: true, ReduceTransparency: true}})
	tester.SetLocale("en")
	frame := func() { tester.Advance(time.Second / 60); tester.Frame() }
	settle := func(stage string, done func() bool) {
		t.Helper()
		for deadline := time.Now().Add(5 * time.Second); time.Now().Before(deadline); {
			frame()
			if done() {
				return
			}
			time.Sleep(time.Millisecond)
		}
		layout := r.App.Messages.List(first.String())
		start, end := layout.Visible()
		t.Fatalf("%s did not settle: reads=%d pages=%d visible=%d..%d at end=%t", stage, transport.reads.Load(), transport.pages.Load(), start, end, layout.AtEnd())
	}
	click := func(label string) {
		t.Helper()
		if err := tester.Click(label); err != nil {
			t.Fatal(err)
		}
		frame()
	}
	findTool := func() bool { return tester.HasText("detail_fixture") }
	vm := func() *conversation.Turn {
		cv := r.App.Data.Find(first.String())
		if cv == nil || len(cv.Turns) == 0 {
			return nil
		}
		return &cv.Turns[0]
	}
	imageKey := attachmentKey(first.String(), fmt.Sprintf("published/%s/%s/0/1", ref.Turn, ref.Entry))
	hasThumbnail := func() bool {
		cache := r.attachments.imageSessions[first.String()]
		return cache != nil && cache.images[imageKey] != nil
	}
	check := func(stage string, reads int32) {
		t.Helper()
		if transport.reads.Load() != reads {
			t.Fatalf("%s: detail reads=%d, want %d", stage, transport.reads.Load(), reads)
		}
		if _, cached := controller.ConversationDetail(ref); cached {
			t.Fatalf("%s: full JSON entered Client's conversation cache", stage)
		}
		if transport.originalImages.Load() != 0 {
			t.Fatalf("%s: a thumbnail opened the original image", stage)
		}
		if turn := vm(); turn != nil {
			for _, block := range turn.Reply.Blocks {
				for _, call := range block.Calls {
					if call.Output != nil && strings.Contains(call.Output.Text, bodyMarker) {
						t.Fatalf("%s: full output entered the conversation view model", stage)
					}
				}
			}
		}
	}
	var initialStats runtime.MemStats
	runtime.ReadMemStats(&initialStats)
	previousAlloc := initialStats.TotalAlloc
	measure := func(stage string) {
		runtime.GC()
		var stats runtime.MemStats
		runtime.ReadMemStats(&stats)
		_, descriptor := r.attachments.files[imageKey]
		t.Logf("%s: reads=%d pages=%d image descriptor=%t thumbnail=%t live heap=%d KiB allocated since previous stage=%d KiB",
			stage, transport.reads.Load(), transport.pages.Load(), descriptor, hasThumbnail(), stats.HeapAlloc>>10, (stats.TotalAlloc-previousAlloc)>>10)
		previousAlloc = stats.TotalAlloc
	}
	r.App.OpenConversation(first.String())
	settle("render page", func() bool { return vm() != nil })
	list := r.App.Messages.List(first.String())
	list.ScrollTo(0, ui.Start)
	settle("show first turn", func() bool { return tester.HasText("Question 01") })
	if !findTool() {
		click(i18n.In("en", "turn_completed"))
	}
	settle("show tool header", findTool)
	settle("load the published image thumbnail", hasThumbnail)
	check("closed", 0)
	if descriptor := r.attachments.files[imageKey]; descriptor.path != imagePath || descriptor.revision == nil || *descriptor.revision != imageRevision {
		t.Fatalf("closed tool lost the VM image descriptor: %+v", descriptor)
	}
	measure("closed")
	click("detail_fixture")
	settle("show full result", func() bool { return tester.HasText(bodyMarker) })
	check("open", 1)
	if r.pluginCards == nil || len(r.pluginCards.cards) != 1 {
		t.Fatal("open disclosure did not attach its captured plugin card")
	}
	measure("open")
	const steadyFrames = 8
	for range steadyFrames {
		frame()
	}
	check("steady open", 1)
	var steadyStats runtime.MemStats
	runtime.ReadMemStats(&steadyStats)
	t.Logf("steady open: %d B allocated per frame over %d frames", (steadyStats.TotalAlloc-previousAlloc)/steadyFrames, steadyFrames)
	previousAlloc = steadyStats.TotalAlloc
	click("detail_fixture")
	if tester.HasText(bodyMarker) {
		t.Fatal("folded disclosure kept its formatted result")
	}
	check("folded", 1)
	measure("folded")
	click("detail_fixture")
	settle("reopen full result", func() bool { return tester.HasText(bodyMarker) })
	list.ScrollTo(turnCount-2, ui.Start)
	settle("scroll out of the virtual window", func() bool {
		start, _ := list.Visible()
		return start > 0 && !tester.HasText(bodyMarker) && !hasThumbnail()
	})
	check("offscreen", 2)
	if r.attachments.files[imageKey].path == "" || tester.HasText(bodyMarker) {
		t.Fatal("offscreen turn lost its image reference or kept rendering its full output")
	}
	measure("offscreen")
	transport.gated.Store(true)
	list.ScrollTo(0, ui.Start)
	settle("reenter virtual window", func() bool { return len(transport.gates) != 0 })
	gate := <-transport.gates
	r.App.OpenConversation(other.String())
	settle("replace page", func() bool { return vm() == nil && gate.ctx.Err() != nil })
	gate.reply <- gate.result
	close(gate.reply)
	r.workers.Wait()
	frame()
	if tester.HasText(bodyMarker) || r.pluginCards != nil && len(r.pluginCards.cards) != 0 || r.attachments.files[imageKey].path != "" {
		t.Fatal("late response restored the closed page's tool output, card, or attachment")
	}
	check("replaced", 3)
	measure("replaced")

	r.App.OpenConversation(first.String())
	settle("reopen page", func() bool { return vm() != nil })
	r.App.Messages.List(first.String()).ScrollTo(0, ui.Start)
	settle("start the next disclosure read", func() bool { return len(transport.gates) != 0 })
	gate = <-transport.gates
	gate.reply <- gate.result
	close(gate.reply)
	r.workers.Wait()
	if len(r.queue) == 0 || tester.HasText(bodyMarker) {
		t.Fatal("fixture did not queue a formatted detail before closing")
	}
	if err := r.Close(); err != nil {
		t.Fatal(err)
	}
	if len(r.queue) != 0 || len(r.attachments.imageSessions) != 0 || r.attachments.files[imageKey].path != "" {
		t.Fatal("closed window retained queued details or its attachment data")
	}
	check("closed window", 4)
	measure("closed window")
}

type detailCompletionGate struct {
	ctx    context.Context
	reply  chan link.Response
	result link.Response
}

type largeDetailTransport struct {
	desktopHistoryTransport
	ref            protocol.ToolDetailReference
	body           string
	reads          atomic.Int32
	pages          atomic.Int32
	gated          atomic.Bool
	gates          chan detailCompletionGate
	worktree       protocol.WorktreeID
	imagePath      string
	imageRevision  string
	originalImages atomic.Int32
}

func (t *largeDetailTransport) Dispatch(ctx context.Context, request protocol.Request) (link.Admission, error) {
	if request.Command.Kind == "download_file" {
		in, ok := detail[protocol.CommandDownloadFilePayload](request.Command.Data)
		if !ok || in.Worktree != t.worktree || in.Path != t.imagePath || in.ThumbnailEdge != attachmentThumbnailEdge {
			t.originalImages.Add(1)
			return link.Admission{}, &protocol.Fault{Code: protocol.ErrorCodeInvalidRequest, Message: "unexpected image transfer"}
		}
		return detailTestCompletion(link.Response{Output: protocol.Output{Kind: "file_download", Data: protocol.FileDownload{
			Stream: protocol.StreamID{1}, Worktree: in.Worktree, Path: in.Path, Revision: t.imageRevision, Size: 4096,
			Thumbnail: &protocol.ThumbnailTransfer{Size: uint64(len(t.png)), Revision: fmt.Sprintf("%x", blake3.Sum256(t.png))},
		}}}), nil
	}
	if request.Command.Kind != "read_tool_detail" {
		return t.desktopHistoryTransport.Dispatch(ctx, request)
	}
	in, ok := detail[protocol.CommandReadToolDetailPayload](request.Command.Data)
	if !ok || in.Session != t.ref.Session || in.Entry != t.ref.Entry || in.ExpectedRevision != t.ref.Revision {
		return link.Admission{}, &protocol.Fault{Code: protocol.ErrorCodeInvalidRequest, Message: "unexpected tool detail reference"}
	}
	page := protocol.ToolDetailPage{Reference: t.ref, TotalBytes: uint64(len(t.body))}
	if in.Cursor == nil {
		t.reads.Add(1)
	} else {
		page.Offset = *in.Cursor
	}
	end := min(page.TotalBytes, page.Offset+in.Limit)
	if end < page.TotalBytes {
		page.NextOffset = &end
	}
	page.Text = t.body[page.Offset:end]
	t.pages.Add(1)
	result := link.Response{Output: protocol.Output{Kind: "tool_detail", Data: page}}
	if t.gated.Load() && in.Cursor == nil {
		reply := make(chan link.Response, 1)
		t.gates <- detailCompletionGate{ctx: ctx, reply: reply, result: result}
		return link.Admission{Completion: reply}, nil
	}
	return detailTestCompletion(result), nil
}

func detailTestCompletion(result link.Response) link.Admission {
	reply := make(chan link.Response, 1)
	reply <- result
	close(reply)
	return link.Admission{Completion: reply}
}
