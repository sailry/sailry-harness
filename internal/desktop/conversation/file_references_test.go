package conversation_test

import (
	"slices"
	"testing"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/conversation"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

func TestFileReferencesRetainNavigation(t *testing.T) {
	for _, row := range []struct {
		name, markdown, target string
		standalone             bool
	}{
		{"inline", "Review [Source](export.py#L7) for the issue", "export.py#L7", false},
		{"inline without line", "Review [Source](export.py) for the issue", "export.py", false},
		{"list", "- [Source](stock.py)\n\n  Existing source under review", "stock.py", false},
		{"blockquote", "> [Source](test_export.py)", "test_export.py", false},
		// The resolver keeps a standalone source link a link.
		{"top level", "[Source](export.py)", "export.py", true},
	} {
		t.Run(row.name, func(t *testing.T) {
			state := conversation.New()
			offered := 0
			state.Embed = func(_, dest, _ string, image bool) (conversation.Embed, bool) {
				offered++
				if row.standalone && !image && dest == row.target {
					return conversation.Embed{}, false
				}
				return conversation.Embed{File: &conversation.File{ID: "incorrect/reference", Path: row.target}}, true
			}
			var links []opened
			state.OpenLink = func(c *ui.Context, session, target string) bool {
				links = append(links, opened{session, target, conversation.External(c)})
				return true
			}
			var files []string
			turns := []conversation.Turn{{ID: "review", Reply: conversation.Reply{Status: conversation.Completed, Blocks: []conversation.Block{
				{Kind: conversation.TextBlock, Text: row.markdown},
				{Kind: conversation.FilesBlock, Files: []conversation.File{{ID: "publication/report", Path: "output/report.pdf", Mime: "application/pdf"}}},
			}}}}
			tester := ui.NewTester(func(c *ui.Context) {
				theme.Apply(c, "", theme.Light)
				ui.Column(c).Fill().Children(func() { conversation.View(c, state, "owned", turns, nil, fileActions{got: &files}) })
			}, 900, 700)
			tester.SetLocale("en")
			tester.Frame()
			if offered != 0 && !row.standalone {
				t.Fatalf("a reference within text was offered as a publication %d times", offered)
			}
			for _, modifier := range []ui.Modifiers{0, ui.Cmd} {
				link, ok := tester.AccessByRole(ui.RoleLink, "Source")
				if !ok || link.Bounds.W <= 0 || link.Bounds.H <= 0 || link.Offscreen {
					t.Fatalf("the source reference lost its visible inline link: %+v", link)
				}
				// A selectable paragraph can have its only link's name. Target
				// the link's words rather than the paragraph's full-width box.
				tester.ClickAtWith(modifier, link.Bounds.X+link.Bounds.W/2, link.Bounds.Y+link.Bounds.H/2)
				tester.Frame()
			}
			if !slices.Equal(links, []opened{{"owned", row.target, false}, {"owned", row.target, true}}) {
				t.Fatalf("reference clicks lost their target, line or modifier: %+v", links)
			}
			name := i18n.In("en", "artifact_open")
			cards := 0
			for _, info := range tester.AccessAll() {
				if info.Role == ui.RoleButton && info.Name == name {
					cards++
				}
			}
			if cards != 1 {
				t.Fatalf("got %d file-card Open buttons, want only the explicit publication", cards)
			}
			if err := tester.Click(name); err != nil {
				t.Fatal(err)
			}
			tester.Frame()
			if !slices.Equal(files, []string{"open output/report.pdf"}) {
				t.Fatalf("the explicit publication lost its file action: %v", files)
			}
		})
	}
}

// A link standing alone to a deliverable shows as its file card.
func TestStandaloneDeliverableLinkIsFileCard(t *testing.T) {
	state := conversation.New()
	state.Embed = func(_, dest, _ string, image bool) (conversation.Embed, bool) {
		if image || dest != "output/budget.xlsx" {
			return conversation.Embed{}, false
		}
		return conversation.Embed{File: &conversation.File{ID: "link/output/budget.xlsx", Path: dest}}, true
	}
	var files []string
	turns := []conversation.Turn{{ID: "deliver", Reply: conversation.Reply{Status: conversation.Completed, Blocks: []conversation.Block{
		{Kind: conversation.TextBlock, Text: "The budget is ready.\n\n[budget.xlsx](output/budget.xlsx)\n\nSee [budget.xlsx](output/budget.xlsx) for totals."},
	}}}}
	tester := ui.NewTester(func(c *ui.Context) {
		theme.Apply(c, "", theme.Light)
		ui.Column(c).Fill().Children(func() { conversation.View(c, state, "owned", turns, nil, fileActions{got: &files}) })
	}, 900, 700)
	tester.SetLocale("en")
	tester.Frame()
	name := i18n.In("en", "artifact_open")
	cards := 0
	for _, info := range tester.AccessAll() {
		if info.Role == ui.RoleButton && info.Name == name {
			cards++
		}
	}
	if cards != 1 {
		t.Fatalf("got %d file cards, want the standalone deliverable only", cards)
	}
	if _, ok := tester.AccessByRole(ui.RoleLink, "budget.xlsx"); !ok {
		t.Fatal("the deliverable mentioned within text lost its link")
	}
	if err := tester.Click(name); err != nil {
		t.Fatal(err)
	}
	tester.Frame()
	if !slices.Equal(files, []string{"open output/budget.xlsx"}) {
		t.Fatalf("the deliverable card lost its file action: %v", files)
	}
}
