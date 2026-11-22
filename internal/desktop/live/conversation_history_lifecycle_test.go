package live

import (
	"bytes"
	"context"
	"encoding/json"
	"fmt"
	"runtime"
	"strings"
	"testing"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/preferences"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
	"lukechampine.com/blake3"
)

type desktopHistoryTransport struct {
	liveWorkspaceBenchmarkTransport
	png []byte
}

func (s *desktopHistoryTransport) Open(context.Context, protocol.StreamID) (link.Stream, error) {
	return &readRecoveryImageStream{Reader: bytes.NewReader(s.png)}, nil
}

func TestDesktopHistoryLifecycle(t *testing.T) {
	const total, size, older = 80, 20, 11
	const query = "History question 11"
	first, other := protocol.SessionID{1}, protocol.SessionID{2}
	node := protocol.DefaultSnapshot()
	node.Node = protocol.NodeID{3}
	worktree := protocol.WorktreeID{4}
	node.Worktrees = []protocol.Worktree{{ID: worktree, Path: t.TempDir()}}
	for _, id := range []protocol.SessionID{first, other} {
		s := protocol.DefaultSession()
		s.ID, s.Revision, s.Worktree = id, 1, worktree
		node.Sessions = append(node.Sessions, s)
	}
	png := cardPNG(t, 32, 32)
	digest := fmt.Sprintf("%x", blake3.Sum256(png))
	image := func(i byte) protocol.Attachment {
		return protocol.Attachment{ID: protocol.AttachmentID{i}, Spec: protocol.AttachmentSpec{
			Worktree: worktree, Name: fmt.Sprintf("history-%02d.png", i), MediaType: "image/png", Size: uint64(len(png)), Revision: digest,
		}}
	}
	toolSummary := &protocol.ToolDetailSummary{Revision: "history-detail", HasDetails: true}
	card := protocol.PluginCardSummary{
		Receipt: protocol.PluginCardReceipt{Session: first, Turn: protocol.TurnID{older}, Call: protocol.RequestID{1}},
		Node:    node.Node, Worktree: worktree, Package: protocol.PluginReference{Name: "history-fixture", Digest: strings.Repeat("a", 64)},
		Key: "history", Props: json.RawMessage(`{"title":"A historical tool card"}`), Fallback: protocol.PluginDesktopNavigation{Label: "Historical tool card"},
	}
	cardBody, err := json.Marshal(protocol.ToolDetail{Content: &protocol.ToolContent{Version: 1, Blocks: []protocol.ToolContentBlock{{Kind: "card", Data: card}}}, Images: []protocol.ConversationImage{}})
	if err != nil {
		t.Fatal(err)
	}
	page := func(end int) protocol.ConversationPage {
		p := protocol.DefaultConversationPage()
		p.Session, p.Revision = first, 7
		start := max(0, end-size)
		for i := start + 1; i <= end; i++ {
			row := cacheTestPage(first, byte(i), fmt.Sprintf("History question %02d", i))
			row.Runs[0].Worktree = worktree
			row.Entries[0].Parts = append(row.Entries[0].Parts, protocol.ConversationPart{Kind: "attachment", Data: image(byte(i))})
			row.Entries[1].Parts[0].Data = fmt.Sprintf("## History reply %02d\n\n```text\n%s```", i,
				strings.Repeat("The isolated history payload is rendered inside a fenced Markdown block.\n", 300))
			if i == older {
				row.Entries[1].Parts = append([]protocol.ConversationPart{{Kind: "tool_call", Data: protocol.ConversationPartToolCallPayload{
					Name: "history_fixture", Summary: toolSummary,
				}}}, row.Entries[1].Parts...)
			}
			p.Runs = append(p.Runs, row.Runs...)
			p.Entries = append(p.Entries, row.Entries...)
		}
		if start > 0 {
			p.NextBefore = &p.Runs[0].Turn
		}
		return p
	}
	makeSnapshot := func(p protocol.ConversationPage) protocol.ConversationSnapshot {
		s := protocol.DefaultConversationSnapshot()
		s.Node, s.Sequence, s.Page = node.Node, 200, p
		return s
	}
	transport := &desktopHistoryTransport{png: png, liveWorkspaceBenchmarkTransport: liveWorkspaceBenchmarkTransport{conversationTestTransport: conversationTestTransport{snapshot: node},
		conversations: map[protocol.SessionID]protocol.ConversationSnapshot{
			first: makeSnapshot(page(total)), other: makeSnapshot(cacheTestPage(other, 1, "Another conversation")),
		}}}
	pageCursors := make(chan byte, 32)
	transport.handle = func(request protocol.Request) (link.Response, bool) {
		switch request.Command.Kind {
		case "read_conversation":
			in, valid := detail[protocol.CommandReadConversationPayload](request.Command.Data)
			if !valid || in.Session != first || in.Limit != size || in.Before == nil {
				return link.Response{Fault: &protocol.Fault{Code: protocol.ErrorCodeInvalidRequest, Message: "unexpected history cursor"}}, true
			}
			select {
			case pageCursors <- in.Before[0]:
			default:
				return link.Response{Fault: &protocol.Fault{Code: protocol.ErrorCodeInvalidRequest, Message: "too many history reads"}}, true
			}
			return link.Response{Output: protocol.Output{Kind: "conversation", Data: protocol.ConversationHistory{
				Sequence: 200, Page: page(int(in.Before[0]) - 1),
			}}}, true
		case "search_conversation":
			in, valid := detail[protocol.CommandSearchConversationPayload](request.Command.Data)
			if !valid || in.Session != first || in.Query.Text != query || in.Query.Before != nil {
				return link.Response{Fault: &protocol.Fault{Code: protocol.ErrorCodeInvalidRequest, Message: "unexpected search query"}}, true
			}
			turn := protocol.TurnID{older}
			return link.Response{Output: protocol.Output{Kind: "conversation_matches", Data: protocol.ConversationSearchPage{
				Session: first, Revision: 7, Matches: []protocol.ConversationSearchMatch{{Turn: turn, TurnSequence: older, Entry: turn.String() + "/user", Sequence: older*2 - 1,
					Author: "user", Snippet: query, Highlight: protocol.Range[uint64]{Start: 0, End: uint64(len(query))}}},
			}}}, true
		case "download_attachment":
			in, _ := detail[protocol.CommandDownloadAttachmentPayload](request.Command.Data)
			return link.Response{Output: protocol.Output{Kind: "attachment_download", Data: protocol.AttachmentDownload{
				Stream: protocol.StreamID{in.Attachment[0]}, Attachment: image(in.Attachment[0]),
				Thumbnail: &protocol.ThumbnailTransfer{Size: uint64(len(png)), Revision: digest},
			}}}, true
		case "read_tool_detail":
			in, _ := detail[protocol.CommandReadToolDetailPayload](request.Command.Data)
			return link.Response{Output: protocol.Output{Kind: "tool_detail", Data: protocol.ToolDetailPage{
				Reference:  protocol.ToolDetailReference{Session: in.Session, Turn: in.Turn, Entry: in.Entry, Index: in.Index, Call: in.Call, Revision: in.ExpectedRevision},
				TotalBytes: uint64(len(cardBody)), Text: string(cardBody),
			}}}, true
		default:
			return link.Response{}, false
		}
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
	app := r.App
	app.Effects = false
	prefs := app.Prefs()
	prefs.NavOpen, prefs.PanelOpen, prefs.OverviewHidden = false, false, true
	app.SetPrefs(prefs)
	tester := ui.NewTesterWith(app.View, 1120, 700, ui.TesterOptions{
		Preferences: ui.Preferences{ReduceMotion: true, ReduceTransparency: true},
	})
	tester.SetLocale("en")
	frame := func() {
		time.Sleep(time.Millisecond)
		tester.Advance(time.Second / 60)
		tester.Frame()
	}
	settle := func(done func() bool) {
		t.Helper()
		for deadline := time.Now().Add(5 * time.Second); time.Now().Before(deadline); {
			frame()
			if done() {
				return
			}
		}
		t.Fatalf("history did not settle: turns=%d, watch=%+v", len(app.Data.Find(first.String()).Turns), r.conversationCache[first].state.OlderErr)
	}
	turns := func() int { return len(app.Data.Find(first.String()).Turns) }
	checkCursor := func(want byte) {
		t.Helper()
		select {
		case actual := <-pageCursors:
			if actual != want {
				t.Fatalf("history requested cursor %d, want %d", actual, want)
			}
		default:
			t.Fatalf("history never requested cursor %d", want)
		}
	}
	navigateTo := func(row int) {
		t.Helper()
		rail, ok := tester.AccessByRole(ui.RoleSlider, i18n.In("en", "chat_navigation"))
		if !ok {
			t.Fatal("message navigation rail is unavailable")
		}
		n := float32(turns())
		height := min(rail.Bounds.H, n*10)
		tester.ClickAt(rail.Bounds.X+16, rail.Bounds.Y+(rail.Bounds.H-height)/2+height*(float32(row)+0.5)/n)
		frame()
	}
	measure := func(stage string) {
		t.Helper()
		runtime.GC()
		var mem runtime.MemStats
		runtime.ReadMemStats(&mem)
		cached := r.conversationCache[first].state.Snapshot.Page
		var body int
		for _, entry := range cached.Entries {
			for _, part := range entry.Parts {
				if text, ok := part.Data.(string); ok {
					body += len(text)
				}
			}
		}
		cards := 0
		if r.pluginCards != nil {
			cards = len(r.pluginCards.cards)
		}
		t.Logf("%s: turns=%d entries=%d text=%d B descriptors=%d cards=%d thumbnail pixels=%d Go heap=%d KiB", stage, turns(), len(cached.Entries), body,
			len(r.attachments.files), cards, r.attachments.thumbnailPixels, mem.HeapAlloc>>10)
	}
	app.OpenConversation(first.String())
	settle(func() bool { return turns() == size })
	layout := app.Messages.List(first.String())
	frame()
	measure("recent")
	for want := 2 * size; want <= total; want += size {
		if turns() < want {
			navigateTo(0)
			settle(func() bool { return turns() >= want })
		}
		checkCursor(byte(total - want + size + 1))
	}
	measure("paged")
	navigateTo(older - 1)
	frame()
	anchor := app.Data.Find(first.String()).Turns[older-1].ID
	oldKey := attachmentKey(first.String(), image(older).ID.String())
	settle(func() bool {
		cache := r.attachments.imageSessions[first.String()]
		return cache != nil && cache.images[oldKey] != nil
	})
	app.OpenConversation(other.String())
	settle(func() bool {
		_, watched := controller.ConversationView(first, client.ConversationState{})
		return !watched && turns() == 0 && len(app.Data.Find(other.String()).Turns) == 1
	})
	measure("left while reading")
	if _, exists := r.attachments.files[oldKey]; exists || r.attachments.imageSessions[first.String()] != nil {
		t.Fatal("unmounted transcript kept its pictures")
	}
	app.OpenConversation(first.String())
	settle(func() bool { return turns() == total })
	layout = app.Messages.List(first.String())
	frame()
	row, _ := layout.Visible()
	if row < 0 || row >= total || app.Data.Find(first.String()).Turns[row].ID != anchor || layout.AtEnd() {
		t.Fatalf("reopen did not restore the older reading position: first row %d", row)
	}
	openCard := func() {
		t.Helper()
		if err := tester.Click(i18n.In("en", "turn_completed")); err != nil {
			t.Fatal(err)
		}
		if err := tester.Click("history_fixture"); err != nil {
			t.Fatal(err)
		}
		settle(func() bool { return tester.HasText("Historical tool card") })
	}
	jumpToLatest := func() {
		t.Helper()
		jump, ok := tester.AccessByRole(ui.RoleButton, i18n.In("en", "turn_latest"))
		if !ok || jump.Disabled {
			t.Fatal("return-to-latest button is unavailable")
		}
		tester.ClickAt(jump.Bounds.X+jump.Bounds.W/2, jump.Bounds.Y+jump.Bounds.H/2)
		settle(layout.AtEnd)
	}
	openCard()
	measure("opened older tool card")
	jumpToLatest()
	settle(func() bool { return turns() == size })
	measure("back to latest")
	state, watched := controller.ConversationView(first, r.conversationCache[first].state)
	if !watched || len(state.Snapshot.Page.Runs) != size || len(state.Snapshot.Page.Entries) != 2*size || state.Snapshot.Page.NextBefore == nil {
		t.Fatal("return to latest failed to replace the Client history range or keep its cursor")
	}
	cache := r.attachments.imageSessions[first.String()]
	if _, exists := r.attachments.files[oldKey]; exists || cache != nil && cache.images[oldKey] != nil || len(r.pluginCards.cards) != 0 || len(r.pluginCards.calls) != 0 {
		t.Fatal("return to latest kept an older image or tool card")
	}
	for len(pageCursors) > 0 {
		<-pageCursors
	}
	navigateTo(0)
	settle(func() bool { return turns() >= 2*size })
	checkCursor(total - size + 1)
	loaded := app.Data.Find(first.String()).Turns
	for i, row := range loaded {
		want := protocol.TurnID{byte(total - len(loaded) + i + 1)}
		if row.ID != want.String() {
			t.Fatalf("repaging produced a gap or duplicate at row %d: %s, want %s", i, row.ID, want)
		}
	}
	jumpToLatest()
	settle(func() bool { return turns() == size })
	tester.Key(ui.Cmd, ui.KeyF)
	frame()
	tester.Type(query)
	settle(func() bool { return !layout.AtEnd() && turns() == total && tester.HasText(query) })
	if !app.Messages.SearchOpenIn(first.String()) {
		t.Fatal("reveal did not keep the search bar open")
	}
	tester.Key(0, ui.KeyEscape)
	frame()
	navigateTo(older - 1)
	settle(func() bool { return tester.HasText("Historical tool card") })
	jumpToLatest()
	settle(func() bool { return turns() == size })
	app.OpenConversation(other.String())
	settle(func() bool { return turns() == 0 })
	measure("left from latest")
	app.OpenConversation(first.String())
	settle(func() bool { return turns() == size })
	for range 4 {
		frame()
	}
	if turns() != size || !app.Messages.List(first.String()).AtEnd() {
		t.Fatal("reopening at latest reloaded the earlier reading range")
	}
}
