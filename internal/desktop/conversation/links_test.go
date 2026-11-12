package conversation_test

import (
	"slices"
	"testing"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/conversation"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

// opened is one call of OpenLink: the link and whether the click held the
// platform modifier.
type opened struct {
	session, link string
	external      bool
}

// fileActions records what the file card and the changes card ask for.
type fileActions struct {
	noActions
	got *[]string
}

func (a fileActions) OpenFile(f conversation.File) { *a.got = append(*a.got, "open "+f.Path) }
func (a fileActions) FileAction(f conversation.File, action conversation.FileAction) {
	if action == conversation.OpenExternal {
		*a.got = append(*a.got, "system "+f.Path)
	}
}
func (a fileActions) Review(_, path string) { *a.got = append(*a.got, "review "+path) }

// linksTurns is a reply with every place a link or a file shows.
func linksTurns() []conversation.Turn {
	return []conversation.Turn{{ID: "links", User: "Show the links", Reply: conversation.Reply{Status: conversation.Completed,
		Citations: []conversation.Citation{{URI: "https://cite.example/one"}},
		Changes:   &conversation.Changes{Files: []conversation.ChangedFile{{Path: "src/changed.go", Added: 1}}},
		Blocks: []conversation.Block{
			{Kind: conversation.ToolsBlock, Calls: []conversation.Call{
				{ID: "read", Name: "read", Label: "Read files", Output: &conversation.Output{Paths: []string{"a/one.go", "a/two#2.go"}}},
			}},
			{Kind: conversation.TextBlock, Text: "Then the change."},
			{Kind: conversation.ToolsBlock, Calls: []conversation.Call{
				{ID: "edit", Name: "edit", Label: "Edit file", Output: &conversation.Output{Diff: &conversation.Diff{Path: "src/edited.go", Added: 1,
					Lines: []conversation.DiffLine{{Op: '+', Text: "x"}}}}},
			}},
			{Kind: conversation.SearchBlock, Text: "Found [inner](https://inner.example/i).",
				Sources: []conversation.Citation{{URI: "https://source.example/s", Title: "Source one"}, {URI: "https://user:pw@source.example/", Title: "Source two"}}},
			{Kind: conversation.TextBlock, Text: "See [site](https://example.com/a), https://auto.example/x, [mail](mailto:a@example.com), " +
				"[code](src/main.go#L12) and [top](#intro).\n\n![plan](notes/plan.txt)\n\nThe end."},
			{Kind: conversation.FilesBlock, Files: []conversation.File{{ID: "report", Path: "docs/report.md", Kind: conversation.MarkdownFile}}},
		}}}}
}

// checkLinks is the transcript's side of the link rule: every link and file
// of the content goes to OpenLink or the file actions, and the platform
// modifier is read at the click.
func checkLinks(t *testing.T) {
	var links []opened
	var files []string
	s := conversation.New()
	s.OpenLink = func(c *ui.Context, session, link string) bool {
		if link == "#intro" {
			return false
		}
		links = append(links, opened{session, link, conversation.External(c)})
		return true
	}
	// A picture that is no file of the conversation: its address as a link.
	s.Embed = func(_, dest, _ string, image bool) (conversation.Embed, bool) {
		return conversation.Embed{}, image && dest == "notes/plan.txt"
	}
	turns := linksTurns()
	tt := ui.NewTester(func(c *ui.Context) {
		theme.Apply(c, "", theme.Light)
		ui.Column(c).Fill().Children(func() { conversation.View(c, s, "session", turns, nil, fileActions{got: &files}) })
	}, 900, 1400)
	tt.SetLocale("en")
	tt.Frame()
	tt.Frame()
	click := func(mods ui.Modifiers, label string) {
		t.Helper()
		if err := tt.ClickWith(mods, label); err != nil {
			t.Fatalf("%v: %q", err, tt.Texts())
		}
		tt.Frame()
		tt.Frame()
	}
	// link clicks label plainly and with the modifier: both reach OpenLink
	// with the link, the second as external.
	link := func(label, want string) {
		t.Helper()
		links = nil
		click(0, label)
		click(ui.Cmd, label)
		if !slices.Equal(links, []opened{{"session", want, false}, {"session", want, true}}) {
			t.Errorf("%s: OpenLink took %+v, want %q plainly and externally", label, links, want)
		}
	}
	link("site", "https://example.com/a")
	link("https://auto.example/x", "https://auto.example/x")
	link("mail", "mailto:a@example.com")
	link("code", "src/main.go#L12")
	link("1", "https://cite.example/one")
	link("notes/plan.txt", "notes/plan.txt")
	// A heading link OpenLink leaves scrolls in the transcript.
	links = nil
	click(0, "top")
	if len(links) != 0 || len(tt.OpenedURLs()) != 0 {
		t.Errorf("a heading link left the transcript: %+v %q", links, tt.OpenedURLs())
	}

	// The work region's rows: a path of a call's list, and a diff's file.
	click(0, i18n.In("en", "turn_completed"))
	click(0, "Read files")
	link("a/one.go", "a/one.go")
	link("a/two#2.go", "a/two%232.go")
	click(0, "Edit file")
	link("edited.go", "src/edited.go")

	// The changes card: a click reviews the change, the modifier opens the file.
	links, files = nil, nil
	click(0, "src/changed.go")
	click(ui.Cmd, "src/changed.go")
	if !slices.Equal(files, []string{"review src/changed.go"}) || !slices.Equal(links, []opened{{"session", "src/changed.go", true}}) {
		t.Errorf("the changes row: actions %q, links %+v", files, links)
	}

	// The file card: a click previews, the modifier opens the system's app.
	files = nil
	click(0, i18n.In("en", "artifact_open"))
	click(ui.Cmd, i18n.In("en", "artifact_open"))
	if !slices.Equal(files, []string{"open docs/report.md", "system docs/report.md"}) {
		t.Errorf("the file card: %q", files)
	}

	// The search result: its answer's links and its source rows go the same
	// way; an internal open closes the dialog over the panel, an external
	// one leaves it, and a source with credentials opens nowhere.
	result := i18n.In("en", "chat_search_result")
	for _, row := range []struct{ label, want string }{{"inner", "https://inner.example/i"}, {"Source one", "https://source.example/s"}} {
		links = nil
		click(0, result)
		if !tt.HasText(row.label) {
			t.Fatalf("the search result does not show %q: %q", row.label, tt.Texts())
		}
		click(ui.Cmd, row.label)
		if !tt.HasText(row.label) {
			t.Errorf("%s: an external open closed the search result", row.label)
		}
		click(0, row.label)
		if tt.HasText(row.label) {
			t.Errorf("%s: the search result stays over the panel", row.label)
		}
		if !slices.Equal(links, []opened{{"session", row.want, true}, {"session", row.want, false}}) {
			t.Errorf("%s: OpenLink took %+v", row.label, links)
		}
	}
	if tt.HasText("Source two") {
		t.Error("a source with credentials shows as a link")
	}
	if got := tt.OpenedURLs(); len(got) != 0 {
		t.Errorf("the transcript opened %q itself", got)
	}

	t.Run("no application", func(t *testing.T) {
		// Without OpenLink only what ui.SafeToOpen allows leaves the window,
		// and the paths of tool rows are not links.
		s := conversation.New()
		turns := linksTurns()
		tt := ui.NewTester(func(c *ui.Context) {
			theme.Apply(c, "", theme.Light)
			ui.Column(c).Fill().Children(func() { conversation.View(c, s, "session", turns, nil, noActions{}) })
		}, 900, 1400)
		tt.SetLocale("en")
		tt.Frame()
		tt.Frame()
		for _, label := range []string{"site", "code", "top", "mail"} {
			if err := tt.Click(label); err != nil {
				t.Fatal(err)
			}
			tt.Frame()
		}
		if got := tt.OpenedURLs(); !slices.Equal(got, []string{"https://example.com/a", "mailto:a@example.com"}) {
			t.Errorf("opened %q", got)
		}
		for _, label := range []string{i18n.In("en", "turn_completed"), "Read files"} {
			if err := tt.Click(label); err != nil {
				t.Fatal(err)
			}
			tt.Frame()
		}
		if a, ok := tt.AccessByRole(ui.RoleButton, "a/one.go"); ok {
			t.Errorf("a path row is a button without an application: %+v", a.Bounds)
		}
	})
}
