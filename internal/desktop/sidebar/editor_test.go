package sidebar

import (
	"slices"
	"testing"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/preview"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// TestEditorChecks groups the project editor's pure checks; each case keeps its own fixture.
func TestEditorChecks(t *testing.T) {
	t.Run("SuggestName", testSuggestName)
	t.Run("CheckProject", testCheckProject)
	t.Run("CheckClone", testCheckClone)
	t.Run("SampleTree", testSampleTree)
}

func testSuggestName(t *testing.T) {
	for in, want := range map[string]string{
		"~/Web/sailry-go":                       "sailry-go",
		"~/Web/sailry-go/":                      "sailry-go",
		`C:\Work\billing`:                       "billing",
		"https://github.com/org/repository.git": "repository",
		"git@github.com:org/tool.git":           "tool",
	} {
		if got := SuggestName(in, true); got != want {
			t.Errorf("SuggestName(%q) = %q, want %q", in, got, want)
		}
	}
	// Only a repository loses ".git": a folder keeps its name.
	if got := SuggestName("~/Work/mirror.git/ ", false); got != "mirror.git" {
		t.Errorf("SuggestName of a folder = %q", got)
	}
}

func testCheckProject(t *testing.T) {
	a := &preview.Project{Path: "~/Web/a", Host: "local"}
	others := []*preview.Project{a}
	if k := CheckProject("", "~/x", "local", others, nil); k != "project_required" {
		t.Errorf("no name: %q", k)
	}
	if k := CheckProject("b", "~/Web/a/", "local", others, nil); k != "project_duplicate_path" {
		t.Errorf("same path: %q", k)
	}
	if k := CheckProject("b", "~/Web/a", "remote", others, nil); k != "" {
		t.Errorf("another host: %q", k)
	}
	if k := CheckProject("a", "~/Web/a", "local", others, a); k != "" {
		t.Errorf("editing itself: %q", k)
	}
}

func testCheckClone(t *testing.T) {
	if k := CheckClone("tool", "https://x/tool.git", "~/src"); k != "" {
		t.Errorf("valid clone: %q", k)
	}
	for _, name := range []string{"", ".", "..", "a/b", "a:b"} {
		if CheckClone(name, "r", "p") == "" {
			t.Errorf("name %q passed", name)
		}
	}
	if got := Destination(`C:\src\`, "tool"); got != `C:\src\tool` {
		t.Errorf("destination %q", got)
	}
}

// The preview's sample tree holds the host's sample projects under the
// sample account's home, and the files the right panel shows under their
// project, so a picker opens on what the window names.
func testSampleTree(t *testing.T) {
	s := New(preview.Load(), preview.Big)
	catalog := s.Catalog("local")
	if catalog == nil || catalog != s.Catalog("local") {
		t.Fatal("the preview keeps no sample tree for its host")
	}
	listing, problem := catalog.List("/Users/alex/Web/sailry-go")
	if problem != "" {
		t.Fatalf("the sample project is not in the tree: %s", problem)
	}
	var names []string
	for _, entry := range listing.Entries {
		names = append(names, entry.Name+":"+entry.Kind)
	}
	if want := []string{"README.md:file", "docs:directory", "go.mod:file", "internal:directory"}; !slices.Equal(names, want) {
		t.Fatalf("the sample project lists %q, want %q", names, want)
	}
	if catalog.Size("/Users/alex/Web/sailry-go/go.mod") == 0 || catalog.Size("/Users/alex/Web/sailry-go/docs") != 0 {
		t.Fatal("a sample file has no size, or a directory has one")
	}
	// A project of another host is not in this host's tree.
	if _, problem := catalog.List("/Users/alex/Work/billing-api"); problem == "" {
		t.Fatal("the remote host's project is in the local tree")
	}
	var places []string
	for _, place := range s.projectPlaces("local") {
		places = append(places, place.Path)
	}
	if !slices.Contains(places, "/Users/alex/Web/sailry-go") || slices.Contains(places, "~/Web/sailry-go") {
		t.Fatalf("the picker's projects are %q", places)
	}
	s.Actions = &Actions{}
	if s.Catalog("local") != nil {
		t.Fatal("a live window has a sample tree")
	}
}

func TestProjectEditorVisibilityDefaults(t *testing.T) {
	s := New(preview.Load(), preview.Big)
	for _, open := range []struct {
		name string
		run  func()
	}{
		{"new", func() { s.OpenEditor(nil) }},
		{"host", func() { s.OpenEditorAtHost("remote") }},
		{"clone", s.OpenCloneEditor},
	} {
		t.Run(open.name, func(t *testing.T) {
			open.run()
			if got := s.Draft(); got.Project != nil || got.Visibility != string(protocol.ProjectVisibilityShared) {
				t.Fatalf("new project draft = %+v", got)
			}
		})
	}
	for _, visibility := range []protocol.ProjectVisibility{"", protocol.ProjectVisibilityPrivate, protocol.ProjectVisibilityShared} {
		project := &preview.Project{Name: "Existing", Path: "/existing", Visibility: visibility}
		s.OpenEditor(project)
		want := visibility
		if want == "" {
			want = protocol.ProjectVisibilityPrivate
		}
		if got := s.Draft(); got.Project != project || got.Visibility != string(want) {
			t.Fatalf("editing %q changed visibility to %q", visibility, got.Visibility)
		}
	}
	// Open folder also creates a new project when its directory is not registered.
	s.OpenFolder(nil, "new-host")
	if got := s.Draft(); got.Visibility != string(protocol.ProjectVisibilityShared) || !got.folder {
		t.Fatalf("open-folder draft = %+v", got)
	}
}

func renderEditorDialog(t *testing.T, s *State) *ui.Tester {
	t.Helper()
	tester := ui.NewTester(func(c *ui.Context) {
		theme.Apply(c, "", theme.Light)
		ui.Column(c).Fill().Children(func() {
			s.editorDialog(c)
		})
	}, 720, 640)
	tester.SetLocale("en")
	tester.Frame()
	return tester
}

func TestProjectEditorCloneGating(t *testing.T) {
	cloneLabel := i18n.In("en", "project_clone")
	repoLabel := i18n.In("en", "project_repository")
	hostFieldLabel := i18n.In("en", "project_host")

	for _, tc := range []struct {
		name      string
		host      viewmodel.Host
		wantClone bool
	}{
		{
			name: "harness host hides clone even with git",
			host: viewmodel.Host{
				ID:             "harness",
				Name:           "local_host_name",
				Local:          true,
				ProjectSources: []string{"local"},
				Tools:          []protocol.HostTool{{Name: "git", Available: true}},
				CanClone:       false,
			},
			wantClone: false,
		},
		{
			name: "code host with git shows clone",
			host: viewmodel.Host{
				ID:             "code-git",
				Name:           "local_host_name",
				Local:          true,
				ProjectSources: []string{"local", "clone"},
				Tools:          []protocol.HostTool{{Name: "git", Available: true}},
				CanClone:       true,
			},
			wantClone: true,
		},
		{
			name: "code host without git hides clone",
			host: viewmodel.Host{
				ID:             "code-no-git",
				Name:           "local_host_name",
				Local:          true,
				ProjectSources: []string{"local", "clone"},
				Tools:          []protocol.HostTool{{Name: "git", Available: false}},
				CanClone:       false,
			},
			wantClone: false,
		},
	} {
		t.Run(tc.name, func(t *testing.T) {
			data := &viewmodel.Data{Workspace: &viewmodel.Workspace{Hosts: []viewmodel.Host{tc.host}}}
			s := New(data, "")
			s.Actions = &Actions{}
			s.OpenEditor(nil)
			tester := renderEditorDialog(t, s)
			if got := tester.HasText(cloneLabel); got != tc.wantClone {
				t.Fatalf("HasText(%q) = %v, want %v; texts=%q", cloneLabel, got, tc.wantClone, tester.Texts())
			}
			if got := s.Draft(); got.Clone != 0 {
				t.Fatalf("initial Clone = %d, want 0", got.Clone)
			}

			// OpenCloneEditor selects Clone only when the host supports it.
			s.OpenCloneEditor()
			tester.Frame()
			if tc.wantClone {
				if got := s.Draft(); got.Clone != 1 || !tester.HasText(repoLabel) {
					t.Fatalf("OpenCloneEditor on clone-capable host: draft=%+v texts=%q", got, tester.Texts())
				}
			} else {
				if got := s.Draft(); got.Clone != 0 || tester.HasText(cloneLabel) || tester.HasText(repoLabel) {
					t.Fatalf("OpenCloneEditor on non-clone host did not stay local: draft=%+v texts=%q", got, tester.Texts())
				}
			}
		})
	}

	t.Run("OpenCloneEditor picks first clone-capable host", func(t *testing.T) {
		data := &viewmodel.Data{Workspace: &viewmodel.Workspace{Hosts: []viewmodel.Host{
			{ID: "local", Name: "local_host_name", Local: true, ProjectSources: []string{"local"}, CanClone: false},
			{ID: "remote-code", Name: "Build Box", Local: false, ProjectSources: []string{"local", "clone"}, Tools: []protocol.HostTool{{Name: "git", Available: true}}, CanClone: true},
		}}}
		s := New(data, "")
		s.Actions = &Actions{}
		s.SelectedHost = "local"
		s.OpenCloneEditor()
		tester := renderEditorDialog(t, s)
		if got := s.Draft(); got.Clone != 1 || got.host != "remote-code" || !tester.HasText(cloneLabel) || !tester.HasText(repoLabel) {
			t.Fatalf("OpenCloneEditor draft = %+v, texts = %q", got, tester.Texts())
		}
	})

	t.Run("switching host resets clone draft to local", func(t *testing.T) {
		data := &viewmodel.Data{Workspace: &viewmodel.Workspace{Hosts: []viewmodel.Host{
			{ID: "code-host", Name: "Code Box", Local: false, ProjectSources: []string{"local", "clone"}, Tools: []protocol.HostTool{{Name: "git", Available: true}}, CanClone: true},
			{ID: "harness-host", Name: "Harness Box", Local: false, ProjectSources: []string{"local"}, Tools: []protocol.HostTool{{Name: "git", Available: true}}, CanClone: false},
		}}}
		s := New(data, "")
		s.Actions = &Actions{}
		s.OpenCloneEditor()
		s.editor.Repo = "https://github.com/org/tool.git"
		s.editor.Branch = "release"
		s.editor.Path = "/srv/src"
		tester := renderEditorDialog(t, s)
		if got := s.Draft(); got.Clone != 1 || got.Name != "tool" || !tester.HasText(cloneLabel) || !tester.HasText(repoLabel) {
			t.Fatalf("before switch: draft=%+v texts=%q", got, tester.Texts())
		}

		// Switch Host select from Code Box to Harness Box.
		trigger, ok := tester.AccessByRole(ui.RoleMenuButton, hostFieldLabel)
		if !ok {
			t.Fatalf("no host select trigger %q: %+v", hostFieldLabel, tester.AccessAll())
		}
		tester.ClickAt(trigger.Bounds.X+trigger.Bounds.W/2, trigger.Bounds.Y+trigger.Bounds.H/2)
		tester.Frame()
		if err := tester.Click("Harness Box"); err != nil {
			t.Fatalf("click Harness Box: %v; texts=%q", err, tester.Texts())
		}
		tester.Frame()
		tester.Frame()

		if got := s.Draft(); got.host != "harness-host" || got.Clone != 0 || got.mode != 0 || got.Repo != "" || got.Branch != "" || got.Path != "" || got.Name != "" {
			t.Fatalf("after switch to non-clone host: draft=%+v", got)
		}
		if tester.HasText(cloneLabel) || tester.HasText(repoLabel) {
			t.Fatalf("clone UI still visible after switching to non-clone host: %q", tester.Texts())
		}
	})
}
