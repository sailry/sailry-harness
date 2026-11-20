package kit

import (
	"math"
	"slices"
	"strings"
	"testing"

	"github.com/egoist/mygo/ui"
	"github.com/egoist/mygo/ui/markdown"
	"github.com/egoist/mygo/ui/markdown/parser"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

func TestDocumentParagraphImages(t *testing.T) {
	for _, tc := range []struct {
		name, source string
		count        int
		link         string
	}{
		{name: "plain", source: "Text without images."},
		{name: "inline code", source: "Text `<img src=\"secret.png\">` after."},
		{name: "fenced code", source: "```html\n<img src=\"secret.png\">\n```"},
		{name: "inline HTML", source: `Before <img src="../images/logo.svg" alt="Logo"> after.`, count: 3},
		{name: "linked HTML", source: `Before <a href="https://example.test"><img src="../images/logo.svg" alt="Logo"></a> after.`, count: 3, link: "https://example.test"},
		{name: "reference destinations", source: "**Before** ![Logo][image] after [Guide][guide].\n\n[image]: ../images/logo.svg\n[guide]: https://example.test/guide", count: 3},
	} {
		t.Run(tc.name, func(t *testing.T) {
			d := parser.Parse(tc.source)
			runs := paragraphImageRuns(d, 1)
			if len(runs) != tc.count {
				t.Fatalf("runs %+v, want %d", runs, tc.count)
			}
			if d.Source() != tc.source {
				t.Fatal("the document source changed")
			}
			if len(runs) == 0 {
				return
			}
			image := parser.Parse(runs[1].markdown).Inlines(1)
			if len(image) == 0 || image[0].Kind != parser.Image || image[0].Dest != "../images/logo.svg" {
				t.Fatalf("image run %q did not preserve the resolved source", runs[1].markdown)
			}
			if runs[1].link != tc.link {
				t.Fatalf("image link %q, want %q", runs[1].link, tc.link)
			}
			if tc.name == "reference destinations" && (!strings.Contains(runs[0].markdown, "**Before**") || !strings.Contains(runs[2].markdown, "https://example.test/guide")) {
				t.Fatalf("surrounding formatting or resolved link lost: %+v", runs)
			}
		})
	}
}

func TestDocumentImageTextLinksReachOuterAnchor(t *testing.T) {
	for _, prefix := range []string{
		"[Install](#install) ![Logo](logo.svg) [Web](https://example.test)",
		`<p><a href="#install">Install</a> <img src="logo.svg" alt="Logo"> <a href="https://example.test">Web</a></p>`,
	} {
		t.Run(prefix, func(t *testing.T) {
			var scroll ui.ScrollState
			logo := ui.MustParseSVG([]byte(`<svg viewBox="0 0 80 2"><path d="M0 0h80v2H0z"/></svg>`))
			options := markdown.Options{LoadImage: func(string) (ui.ImageSource, error) { return logo, nil }}
			source := prefix + "\n\n" + strings.Repeat("Paragraph before the target.\n\n", 24) + "# Install\n\nTarget."
			tester := ui.NewTester(func(c *ui.Context) {
				c.DisableAnimations()
				theme.Apply(c, "", theme.Light)
				ui.Scroll(c).Fill().TrackScroll(&scroll).Children(func() {
					Markdown(c, source, MarkdownProps{Options: &options})
				})
			}, 400, 180)
			click := func(name string) {
				t.Helper()
				link, ok := tester.AccessByRole(ui.RoleLink, name)
				if !ok {
					t.Fatalf("document link %q is missing", name)
				}
				tester.ClickAt(link.Bounds.X+link.Bounds.W/2, link.Bounds.Y+link.Bounds.H/2)
				tester.Frame()
			}
			click("Web")
			if urls := tester.OpenedURLs(); len(urls) != 1 || urls[0] != "https://example.test" {
				t.Fatalf("ordinary link lost its default behavior: %v", urls)
			}
			click("Install")
			if options.Anchor != "#install" || scroll.Y <= 0 {
				t.Fatalf("nested anchor did not reach the outer document: anchor=%q scroll=%g", options.Anchor, scroll.Y)
			}
			scroll.Y = 0
			tester.Frame()
			version := options.AnchorVersion
			click("Install")
			if scroll.Y <= 0 || options.AnchorVersion <= version {
				t.Fatal("repeated nested anchor did not reveal the target")
			}
		})
	}
}

func TestDocumentLinkedImages(t *testing.T) {
	const target = "https://example.test/docs"
	badge := ui.MustParseSVG([]byte(`<svg viewBox="0 0 80 24"><path d="M0 0h80v24H0z"/></svg>`))
	for _, tc := range []struct{ name, source string }{
		{"Markdown", "[![Badge](badge.svg)](" + target + ")"},
		{"inline HTML", `Before <a href="` + target + `"><img src="badge.svg" alt="Badge"></a> after.`},
		{"HTML block", `<p align="center"><a href="` + target + `"><img src="https://badges.example.test/status?style=flat-square&amp;color=blue" alt="Badge"></a></p>`},
	} {
		t.Run(tc.name, func(t *testing.T) {
			var opened []string
			var menuTarget string
			options := markdown.Options{
				LoadImage: func(string) (ui.ImageSource, error) { return badge, nil },
				OnLink:    func(dest string) { opened = append(opened, dest) },
				LinkMenu: func(menu *ui.Menu, dest string) {
					if menu.Item("Copy target").Chosen() {
						menuTarget = dest
					}
				},
			}
			tester := ui.NewTester(func(c *ui.Context) {
				theme.Apply(c, "", theme.Light)
				Markdown(c, tc.source, MarkdownProps{Options: &options})
			}, 500, 300)
			tester.Frame()
			link, ok := tester.AccessByRole(ui.RoleLink, "Badge")
			if !ok || !link.Focusable || link.Bounds.W <= 0 || link.Bounds.H <= 0 {
				t.Fatalf("linked image is not a focusable, laid-out link: %+v, found=%v", link, ok)
			}
			x, y := link.Bounds.X+link.Bounds.W/2, link.Bounds.Y+link.Bounds.H/2
			tester.ClickAt(x, y)
			tester.Key(0, ui.KeyEnter)
			tester.Key(0, ui.KeySpace)
			if !slices.Equal(opened, []string{target, target}) {
				t.Fatalf("pointer and Enter must open the image target, but Space must not: %v", opened)
			}
			tester.RightClickAt(x, y)
			if err := tester.ChooseMenuItem("Copy target"); err != nil {
				t.Fatal(err)
			}
			if menuTarget != target {
				t.Fatalf("image context menu target %q, want %q", menuTarget, target)
			}
		})
	}
}

func TestDocumentBadgeLayout(t *testing.T) {
	const first = "[![Build badge](https://badges.example.test/build)](#build)"
	const second = "[![Tests badge](https://badges.example.test/tests)](#tests)"
	for _, tc := range []struct {
		name, source   string
		centred, mixed bool
	}{
		{name: "Markdown", source: first + "\n" + second},
		{name: "centred HTML", centred: true, source: `<p align="center">
  <a href="#build"><img src="https://badges.example.test/build" alt="Build badge"></a>
  <a href="#tests"><img src="https://badges.example.test/tests" alt="Tests badge"></a>
</p>`},
		{name: "mixed text", source: first + " between badges " + second, mixed: true},
	} {
		t.Run(tc.name, func(t *testing.T) {
			const width = 500
			options := markdown.Options{LoadImage: func(string) (ui.ImageSource, error) { return nil, nil }}
			tester := ui.NewTester(func(c *ui.Context) {
				theme.Apply(c, "", theme.Light)
				Markdown(c, tc.source, MarkdownProps{Options: &options})
			}, width, 300)
			bounds := func(name string) ui.Rect {
				t.Helper()
				link, ok := tester.AccessByRole(ui.RoleLink, name)
				if !ok || !link.Focusable || link.Bounds.W <= 0 || link.Bounds.H <= 0 {
					t.Fatalf("badge %q is not a laid-out link: %+v", name, link)
				}
				return link.Bounds
			}
			build, tests := bounds("Build badge"), bounds("Tests badge")
			if tc.mixed {
				if tests.Y <= build.Y || !tester.HasText("between badges") {
					t.Fatalf("mixed paragraph lost its vertical runs: build=%+v tests=%+v", build, tests)
				}
				return
			}
			if tests.X < build.X+build.W || tests.Y != build.Y {
				t.Fatalf("adjacent badges did not share a row: build=%+v tests=%+v", build, tests)
			}
			if tc.centred && math.Abs(float64(build.X+tests.X+tests.W-width)) > 2 {
				t.Fatalf("wide badge row is not centered: build=%+v tests=%+v", build, tests)
			}
			narrow := int(max(build.W, tests.W) + min(build.W, tests.W)/2)
			tester.SetSize(narrow, 300)
			build, tests = bounds("Build badge"), bounds("Tests badge")
			if tests.Y < build.Y+build.H {
				t.Fatalf("badges did not wrap at %d: build=%+v tests=%+v", narrow, build, tests)
			}
			if tc.centred && (math.Abs(float64(2*build.X+build.W-float32(narrow))) > 2 || math.Abs(float64(2*tests.X+tests.W-float32(narrow))) > 2) {
				t.Fatalf("wrapped badges are not centered: build=%+v tests=%+v", build, tests)
			}
			tester.ClickAt(tests.X+tests.W/2, tests.Y+tests.H/2)
			if options.Anchor != "#tests" {
				t.Fatalf("wrapped badge link activated %q, want #tests", options.Anchor)
			}
		})
	}
}

func TestDocumentImageHTMLAncestors(t *testing.T) {
	runs := splitDocumentImages(`<strong>Before <a href="https://example.test">linked <img src="logo.png" alt="Logo"> text</a> after</strong>`)
	if len(runs) != 3 || runs[1].link != "https://example.test" {
		t.Fatalf("image and link runs %+v", runs)
	}
	if !strings.Contains(runs[0].markdown, "**Before") || !strings.Contains(runs[0].markdown, "https://example.test") || !strings.Contains(runs[2].markdown, "https://example.test") || !strings.Contains(runs[2].markdown, "after**") {
		t.Fatalf("ancestor formatting or surrounding links lost: %+v", runs)
	}
}
