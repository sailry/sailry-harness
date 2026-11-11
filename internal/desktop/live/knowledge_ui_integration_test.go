//go:build integration

package live

import (
	"context"
	"encoding/json"
	"fmt"
	"net/http"
	"net/http/httptest"
	"os"
	"path/filepath"
	"slices"
	"strings"
	"sync/atomic"
	"testing"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/desktop/composer"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/preferences"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/node"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// knowledgeUITransport counts the citation reads the Desktop sends.
type knowledgeUITransport struct {
	link.Transport
	reads *atomic.Int32
}

func (transport knowledgeUITransport) Dispatch(ctx context.Context, request protocol.Request) (link.Admission, error) {
	if request.Command.Kind == "read_knowledge_citation" {
		transport.reads.Add(1)
	}
	return transport.Transport.Dispatch(ctx, request)
}

// TestKnowledgeUIIntegration opens the knowledge links of a real reply in a
// headless App over a real Node: the viewer shows the citation and pages
// through it, an invalid link and a stale citation toast, and a heading link
// still scrolls the reply. Every wait is bounded by 15 s.
func TestKnowledgeUIIntegration(t *testing.T) {
	ctx, cancel := context.WithTimeout(t.Context(), 90*time.Second)
	defer cancel()
	var reply atomic.Value
	reply.Store("")
	server := httptest.NewServer(http.HandlerFunc(func(writer http.ResponseWriter, request *http.Request) {
		writer.Header().Set("Content-Type", "text/event-stream")
		chunk, _ := json.Marshal(map[string]any{"id": "fixture", "object": "chat.completion.chunk", "model": "fixture", "created": 1,
			"choices": []any{map[string]any{"index": 0, "delta": map[string]any{"role": "assistant", "content": reply.Load().(string)}, "finish_reason": nil}}})
		fmt.Fprintf(writer, "data: %s\n\n", chunk)
		fmt.Fprint(writer, "data: {\"id\":\"fixture\",\"object\":\"chat.completion.chunk\",\"model\":\"fixture\",\"created\":1,\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"stop\"}],\"usage\":{\"prompt_tokens\":12,\"completion_tokens\":3,\"total_tokens\":15}}\n\ndata: [DONE]\n\n")
	}))
	defer server.Close()
	profile := filepath.Join(t.TempDir(), "profile")
	owner, err := node.Start(ctx, profile)
	if err != nil {
		t.Fatal(err)
	}
	defer owner.Stop(context.Background())
	var reads atomic.Int32
	controller := client.New(knowledgeUITransport{Transport: link.NewLocal(owner.ID(), owner.ID(), owner), reads: &reads})
	if err := controller.Start(ctx); err != nil {
		t.Fatal(err)
	}
	defer controller.Close()
	do := func(command protocol.Command) protocol.Output {
		t.Helper()
		output, err := controller.Do(ctx, command)
		if err != nil {
			t.Fatalf("%s: %v", command.Kind, err)
		}
		return output
	}

	// A folder with a short note and a note whose cited line spans three
	// viewer pages. No embedding model is installed, so it is indexed for
	// keywords.
	folder := filepath.Join(t.TempDir(), "notes")
	if err := os.MkdirAll(folder, 0o700); err != nil {
		t.Fatal(err)
	}
	guide := "# Espresso machine\n\nRun a descaling cycle every three months.\n"
	var long strings.Builder
	long.WriteString("zebracrossing")
	for index := 0; long.Len() < 40_000; index++ {
		fmt.Fprintf(&long, " w%05d", index)
	}
	line := long.String()
	for name, text := range map[string]string{"guide.md": guide, "long.md": "# Long list\n\n" + line + "\n"} {
		if err := os.WriteFile(filepath.Join(folder, name), []byte(text), 0o600); err != nil {
			t.Fatal(err)
		}
	}
	created, valid := detail[protocol.KnowledgeCollection](do(protocol.Command{Kind: "create_knowledge_collection", Data: protocol.CommandCreateKnowledgeCollectionPayload{
		Definition: protocol.KnowledgeDefinition{Name: "Notes", Globs: []string{}, Tags: []string{}, SharedWith: []string{},
			Source: protocol.KnowledgeSource{Kind: "directory", Data: protocol.KnowledgeSourceDirectoryPayload{Path: folder}}},
	}}).Data)
	if !valid {
		t.Fatal("created collection was not returned")
	}
	for deadline := time.Now().Add(15 * time.Second); ; time.Sleep(20 * time.Millisecond) {
		listed, _ := detail[[]protocol.KnowledgeCollection](do(protocol.Command{Kind: "list_knowledge_collections"}).Data)
		if len(listed) == 1 && listed[0].ID == created.ID && listed[0].Progress.Status == protocol.KnowledgeStatusReady && listed[0].Progress.Sources == 2 {
			break
		}
		if time.Now().After(deadline) {
			t.Fatalf("the collection was not indexed within 15s: %+v", listed)
		}
	}
	cite := func(query, name string) protocol.KnowledgeHit {
		t.Helper()
		page, valid := detail[protocol.KnowledgePage](do(protocol.Command{Kind: "query_knowledge", Data: protocol.KnowledgeQuery{
			Query: query, Collections: []protocol.KnowledgeID{}, Mode: protocol.KnowledgeModeKeyword, Tags: []string{}, Limit: 5,
		}}).Data)
		if !valid || len(page.Hits) == 0 || page.Hits[0].Citation.Path != filepath.Join(folder, name) {
			t.Fatalf("%q did not find %s: %+v", query, name, page)
		}
		return page.Hits[0]
	}
	short, paged := cite("descaling", "guide.md"), cite("zebracrossing", "long.md")
	if !strings.HasPrefix(short.Citation.Link, "sailry-knowledge://") {
		t.Fatalf("citation link %q", short.Citation.Link)
	}

	// The reply opens with its heading, so following the end of the
	// conversation leaves the heading above the window.
	var text strings.Builder
	text.WriteString("# Target\n\nThe answer starts here.\n\n")
	for index := range 80 {
		fmt.Fprintf(&text, "Filler paragraph %d keeps the heading far above the links.\n\n", index)
	}
	text.WriteString("[Guide source](" + short.Citation.Link + ")\n\n")
	text.WriteString("[Long source](" + paged.Citation.Link + ")\n\n")
	text.WriteString("[Broken source](sailry-knowledge://not-a-collection/" + short.Citation.Chunk + "?revision=" + short.Citation.Revision + ")\n\n")
	text.WriteString("[Jump to target](#target)\n")
	reply.Store(text.String())

	provider := protocol.DefaultConversationProvider()
	provider.ID = protocol.NewProviderID()
	provider.Name, provider.Api = "Knowledge fixture", protocol.ConversationModelApiChatCompletions
	provider.Authentication, provider.Endpoint = protocol.AuthenticationApiKey, server.URL+"/v1"
	provider.Enabled, provider.DefaultModel = true, "fixture"
	model := protocol.DefaultConversationModel()
	model.ID, model.Context, model.Output = "fixture", 200000, 8192
	model.Reasoning, model.DefaultEffort = true, protocol.Effort{Kind: "none"}
	model.Efforts = []protocol.Effort{model.DefaultEffort}
	provider.Models = []protocol.ConversationModel{model}
	secret := protocol.NewSecret("fixture-key")
	do(protocol.Command{Kind: "save_provider", Data: protocol.CommandSaveProviderPayload{Provider: provider, Secret: &secret}})
	project, valid := detail[protocol.Project](do(protocol.Command{Kind: "create_project", Data: protocol.ProjectsDraft{
		Name: "Knowledge workspace", Path: t.TempDir(), Appearance: protocol.DefaultProjectsAppearance(), Source: protocol.ProjectsSource{Kind: "local"},
	}}).Data)
	if !valid {
		t.Fatal("registered project was not returned")
	}
	config := protocol.DefaultSessionConfig()
	config.Provider, config.Model, config.Effort = provider.ID, model.ID, model.DefaultEffort
	config.Mode, config.Permission = protocol.WorkModeCode, protocol.PermissionFull
	session, valid := detail[protocol.Session](do(protocol.Command{Kind: "create_session", Data: protocol.CommandCreateSessionPayload{Project: &project.ID, Config: &config}}).Data)
	if !valid {
		t.Fatal("created session was not returned")
	}

	runtime, err := NewWithPreferences(ctx, controller, preferences.Open(filepath.Join(profile, "desktop", "preferences.json")))
	if err != nil {
		t.Fatal(err)
	}
	defer runtime.Close()
	runtime.OpenSession(session.ID.String())
	tester := ui.NewTester(runtime.App.View, 1440, 1000)
	tester.SetLocale("en")
	wait := func(label string, ready func() bool) {
		t.Helper()
		deadline := time.NewTimer(15 * time.Second)
		defer deadline.Stop()
		ticker := time.NewTicker(10 * time.Millisecond)
		defer ticker.Stop()
		for {
			tester.Advance(16 * time.Millisecond)
			tester.Frame()
			if ready() {
				return
			}
			select {
			case <-ctx.Done():
				t.Fatalf("%s: %v", label, ctx.Err())
			case <-deadline.C:
				texts := tester.Texts()
				t.Fatalf("%s did not settle; last visible texts: %q", label, texts[max(0, len(texts)-40):])
			case <-ticker.C:
			}
		}
	}
	click := func(label string) {
		t.Helper()
		if err := tester.Click(label); err != nil {
			t.Fatalf("click %q: %v", label, err)
		}
	}
	// follow clicks a link of the reply. Its paragraph shows the same text
	// and is wider, so the link itself is located.
	follow := func(label string) {
		t.Helper()
		info, found := tester.AccessByRole(ui.RoleLink, label)
		if !found || info.Bounds.W <= 0 || info.Bounds.H <= 0 {
			t.Fatalf("no visible %q link: %+v", label, info.Bounds)
		}
		tester.ClickAt(info.Bounds.X+info.Bounds.W/2, info.Bounds.Y+info.Bounds.H/2)
	}
	disabled := func(label string) bool {
		t.Helper()
		info, found := tester.Access(label)
		if !found {
			t.Fatalf("no %q control", label)
		}
		return info.Disabled
	}
	wait("conversation", func() bool { return runtime.App.Data.Find(session.ID.String()) != nil })
	runtime.enqueue(func(view *ui.Context) {
		conversation := runtime.App.Data.Find(session.ID.String())
		input := runtime.App.Composer(conversation.ID)
		_, actions := runtime.conversation(view, conversation, input)
		actions.Send(composer.Message{
			Text: "Where is descaling described?", Model: input.Model, Effort: input.EffortName(view, input.Chosen(runtime.models())),
			Mode: input.Mode, Permission: input.Permission, Features: input.Features,
		})
	})
	wait("the reply with its links", func() bool {
		return tester.HasText("Guide source") && tester.HasText("Jump to target") && !runtime.App.Data.Find(session.ID.String()).Running()
	})
	// A box scrolled out of its list is clipped to nothing.
	if box, found := tester.Find("Target"); found && box.H > 0 {
		t.Fatalf("the heading is in view before its link is followed: %+v", box)
	}

	// A knowledge link opens the viewer on its citation.
	host := i18n.In("en", "local_host_name")
	source := i18n.In("en", "knowledge_source")
	follow("Guide source")
	wait("the viewer on the short citation", func() bool { return runtime.App.Reading() != nil })
	shown := runtime.App.Reading()
	lines := i18n.In("en", "knowledge_lines", "start", 1, "end", 3)
	if shown.Title != "Espresso machine" || shown.Host != host || shown.Path != filepath.Join(folder, "guide.md") || shown.Position != lines ||
		shown.Text != strings.TrimSuffix(guide, "\n") || !shown.First || !shown.Last {
		t.Fatalf("short citation shown as %+v", shown)
	}
	wait("the viewer's content", func() bool {
		return tester.HasText(source) && tester.HasText("Espresso machine") && tester.HasText(host+" · "+shown.Path+" · "+lines) && tester.HasText("Run a descaling cycle every three months.")
	})
	if !disabled("Previous") || !disabled("Next") {
		t.Fatal("a single page offers Previous or Next")
	}
	if reads.Load() != 1 {
		t.Fatalf("opening the citation sent %d reads", reads.Load())
	}
	click("Close")
	wait("the viewer closes", func() bool { return runtime.App.Reading() == nil })

	// Previous and Next walk the pages of a long citation.
	// The cited chunk starts at the heading and ends inside the long line,
	// so its excerpt is the three lines, read 16 KiB at a time.
	cited := "# Long list\n\n" + line
	pages := []string{cited[:16*1024], cited[16*1024 : 32*1024], cited[32*1024:]}
	follow("Long source")
	wait("the viewer on the long citation", func() bool { return runtime.App.Reading() != nil })
	shown = runtime.App.Reading()
	if shown.Title != "Long list" || shown.Path != filepath.Join(folder, "long.md") || shown.Position != i18n.In("en", "knowledge_lines", "start", 1, "end", 3) {
		t.Fatalf("long citation shown as %q, %q, %q", shown.Title, shown.Path, shown.Position)
	}
	on := func(page int) {
		t.Helper()
		wait(fmt.Sprintf("page %d", page+1), func() bool { return !shown.Pending && shown.Text == pages[page] })
		tester.Frame()
		if shown.First != (page == 0) || shown.Last != (page == len(pages)-1) || disabled("Previous") != (page == 0) || disabled("Next") != (page == len(pages)-1) {
			t.Fatalf("page %d: first %v, last %v, Previous disabled %v, Next disabled %v", page+1, shown.First, shown.Last, disabled("Previous"), disabled("Next"))
		}
	}
	on(0)
	click("Next")
	on(1)
	click("Next")
	on(2)
	// Next is disabled at the end: pressing it reads nothing.
	before := reads.Load()
	_ = tester.Click("Next")
	tester.Advance(200 * time.Millisecond)
	tester.Frame()
	if reads.Load() != before || shown.Text != pages[2] {
		t.Fatalf("Next at the last page read again: %d reads, was %d", reads.Load(), before)
	}
	click("Previous")
	on(1)
	click("Previous")
	on(0)
	before = reads.Load()
	_ = tester.Click("Previous")
	tester.Advance(200 * time.Millisecond)
	tester.Frame()
	if reads.Load() != before || shown.Text != pages[0] {
		t.Fatalf("Previous at the first page read again: %d reads, was %d", reads.Load(), before)
	}
	click("Close")
	wait("the viewer closes again", func() bool { return runtime.App.Reading() == nil })

	// ACT-008: a notification whose text details are the citation link shows
	// in the activity centre; its click marks it read on the Node and opens
	// the viewer on the cited chunk.
	if !slices.ContainsFunc(controller.Snapshot().Plugins, func(entry protocol.PluginSummary) bool { return entry.Name == "files" }) {
		do(protocol.Command{Kind: "install_bundled_plugin", Data: protocol.CommandInstallBundledPluginPayload{Name: "files"}})
	}
	publisher, valid := detail[protocol.PluginInfo](do(protocol.Command{Kind: "read_plugin", Data: protocol.CommandReadPluginPayload{Name: "files"}}).Data)
	if !valid {
		t.Fatal("the bundled files package was not returned")
	}
	if !publisher.Summary.Enabled {
		do(protocol.Command{Kind: "set_plugin_enabled", Data: protocol.CommandSetPluginEnabledPayload{Name: "files", ExpectedRevision: publisher.Summary.Revision, Enabled: true}})
		publisher, _ = detail[protocol.PluginInfo](do(protocol.Command{Kind: "read_plugin", Data: protocol.CommandReadPluginPayload{Name: "files"}}).Data)
	}
	do(protocol.Command{Kind: "publish_notification", Data: protocol.CommandPublishNotificationPayload{
		Package: protocol.PluginReference{Name: publisher.Summary.Name, Digest: publisher.Summary.Digest, SettingsRevision: publisher.Summary.SettingsRevision},
		Content: protocol.NotificationDraft{Title: "Descaling note indexed", Message: "guide.md", Kind: protocol.NotificationKindInfo,
			Details: &protocol.NotificationDetails{Kind: "text", Data: protocol.NotificationDetailsTextPayload{Body: short.Citation.Link}}},
	}})
	unread := func() (found, read bool) {
		for _, notice := range controller.Notices() {
			if notice.Title == "Descaling note indexed" {
				return true, notice.Read
			}
		}
		return false, false
	}
	wait("the published notice", func() bool { found, _ := unread(); return found })
	if _, read := unread(); read {
		t.Fatal("the published notice starts read")
	}
	before = reads.Load()
	click(i18n.In("en", "notifications"))
	var row ui.AccessInfo
	wait("the notice row", func() bool {
		for _, entry := range tester.AccessAll() {
			if entry.Role == ui.RoleButton && strings.HasPrefix(entry.Name, "Descaling note indexed · ") && entry.Bounds.H > 0 {
				row = entry
				return true
			}
		}
		return false
	})
	tester.ClickAt(row.Bounds.X+row.Bounds.W/2, row.Bounds.Y+row.Bounds.H/2)
	wait("the viewer on the notice's citation", func() bool { return runtime.App.Reading() != nil })
	shown = runtime.App.Reading()
	if shown.Title != "Espresso machine" || shown.Path != filepath.Join(folder, "guide.md") || shown.Position != lines || shown.Text != strings.TrimSuffix(guide, "\n") {
		t.Fatalf("the notice opened %+v", shown)
	}
	if reads.Load() != before+1 {
		t.Fatalf("the notice sent %d reads", reads.Load()-before)
	}
	wait("the notice read on the Node", func() bool { _, read := unread(); return read })
	click("Close")
	wait("the notice's viewer closes", func() bool { return runtime.App.Reading() == nil })

	// An invalid link toasts without asking the Node.
	before = reads.Load()
	follow("Broken source")
	wait("the invalid link toast", func() bool { return tester.HasText(i18n.In("en", "knowledge_citation_invalid")) })
	if runtime.App.Reading() != nil || reads.Load() != before {
		t.Fatalf("an invalid link opened the viewer or sent %d reads", reads.Load()-before)
	}

	// A citation whose source changed toasts the changed message.
	if err := os.WriteFile(filepath.Join(folder, "guide.md"), []byte("# Espresso machine\n\nReplace the water filter every two months.\n"), 0o600); err != nil {
		t.Fatal(err)
	}
	follow("Guide source")
	wait("the changed source toast", func() bool { return tester.HasText(i18n.In("en", "knowledge_citation_changed")) })
	if runtime.App.Reading() != nil || reads.Load() != before+1 {
		t.Fatalf("a stale citation opened the viewer, or sent %d reads", reads.Load()-before)
	}

	// A heading link still scrolls the reply to its heading.
	follow("Jump to target")
	wait("the heading scrolls into view", func() bool {
		box, found := tester.Find("Target")
		return found && box.H > 0 && box.Y >= 0 && box.Y+box.H <= 1000
	})
	box, _ := tester.Find("Target")
	t.Logf("the heading is shown at %+v after its link is followed", box)
	if runtime.App.Reading() != nil || len(tester.OpenedURLs()) != 0 {
		t.Fatalf("the heading link opened something: %v", tester.OpenedURLs())
	}
}
