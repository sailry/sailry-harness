package crychic

import (
	"context"
	"fmt"
	"image"
	"image/png"
	"log/slog"
	"math"
	"os"
	"path/filepath"
	"runtime"
	"slices"
	"strings"
	"sync"
	"testing"
	"time"

	"github.com/ELY-Open-Source/CrychicUI/agent"
	"github.com/ELY-Open-Source/CrychicUI/chat"
	"github.com/ELY-Open-Source/CrychicUI/code"
	"github.com/ELY-Open-Source/CrychicUI/color"
	"github.com/ELY-Open-Source/CrychicUI/desktop"
	"github.com/ELY-Open-Source/CrychicUI/devtools"
	"github.com/ELY-Open-Source/CrychicUI/diagram"
	"github.com/ELY-Open-Source/CrychicUI/feedback"
	"github.com/ELY-Open-Source/CrychicUI/files"
	"github.com/ELY-Open-Source/CrychicUI/form"
	"github.com/ELY-Open-Source/CrychicUI/input"
	"github.com/ELY-Open-Source/CrychicUI/media"
	"github.com/ELY-Open-Source/CrychicUI/navigation"
	"github.com/ELY-Open-Source/CrychicUI/overlay"
	"github.com/ELY-Open-Source/CrychicUI/sound"
	"github.com/egoist/mygo/ui"
	"github.com/egoist/mygo/ui/editor"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

// probe builds one approved component as a feature would: every copy option
// filled, as Sailry fills them from its catalogs.
type probe struct {
	name string
	// idle returns the view of the component at rest, over fresh state.
	idle func() func(c *ui.Context)
	// active returns the view of the state that moves by design, if any.
	active func() func(c *ui.Context)
	// hover names the element the pointer rests on before the frame is judged.
	hover string
	// english are the library's defaults the filled options replace, and
	// fixed the English it gives no option for.
	english, fixed []string
}

var (
	day    = time.Date(2026, 11, 2, 9, 30, 0, 0, time.UTC)
	photo  = ui.NewBitmap(image.NewRGBA(image.Rect(0, 0, 28, 20)))
	cited  = agent.Source{Title: "Go release notes", Host: "go.dev", URL: "https://go.dev/doc/devel/release"}
	swatch = []ui.Color{ui.RGB(0xe8, 0x68, 0x7f), ui.RGB(0x3b, 0x82, 0xf6), ui.RGB(0x22, 0xc5, 0x5e)}
	steps  = []agent.RunStep{{Name: "plan", Duration: 2 * time.Second, State: agent.StepDone},
		{Name: "edit", Duration: 5 * time.Second, State: agent.StepFailed}, {Name: "test", State: agent.StepPending}}
	history = []code.GraphCommit{{Hash: "c3d4e5f6a7", Message: "merge parser", Parents: []string{"b2c3d4e5f6", "a1b2c3d4e5"}, Refs: []string{"main"}},
		{Hash: "b2c3d4e5f6", Message: "parse flags", Lane: 1, Parents: []string{"a1b2c3d4e5"}}, {Hash: "a1b2c3d4e5", Message: "first commit"}}
	logLines = []devtools.LogLine{{ID: "1", Time: "09:30:01", Message: "node ready", Level: devtools.LevelInfo},
		{ID: "2", Time: "09:30:02", Message: "socket closed", Source: "link", Level: devtools.LevelError}}
)

var probes = []probe{
	{name: "color.Picker", idle: func() func(*ui.Context) {
		v := swatch[0]
		return func(c *ui.Context) { color.Picker(c, "t:accent", &v) }
	}},
	{name: "color.SwatchGrid", idle: func() func(*ui.Context) {
		v := swatch[1]
		return func(c *ui.Context) {
			color.SwatchGrid(c, "t:accent", swatch, &v, color.SwatchGridOptions{Names: []string{"t:coral", "t:blue", "t:green"}})
		}
	}},
	{name: "color.Palette", idle: func() func(*ui.Context) {
		return func(c *ui.Context) { color.Palette(c, swatch[1], color.PaletteOptions{}) }
	}},
	{name: "files.Dropzone", english: []string{"Drop files here", "Files keep", "files added"}, idle: func() func(*ui.Context) {
		paths := []string{"/a/one.txt", "/a/two.txt"}
		return func(c *ui.Context) {
			files.Dropzone(c, "t:attachments", &paths, files.DropzoneOptions{Prompt: "t:prompt", Detail: "t:detail", One: "t:%d one", Many: "t:%d many"})
		}
	}},
	{name: "form.FileField", english: []string{"Drop a file", "Browse", "Remove file"}, idle: func() func(*ui.Context) {
		path := "/a/key.pem"
		return func(c *ui.Context) {
			form.FileField(c, "t:key file", &path, form.FileFieldOptions{Empty: "t:empty", Browse: "t:browse", Remove: "t:remove"})
		}
	}},
	{name: "files.FileCard", english: []string{"File actions", "Nov 2, 2026"}, fixed: []string{"KB"}, idle: func() func(*ui.Context) {
		var action files.FileAction
		return func(c *ui.Context) {
			files.FileCard(c, files.FileItem{Name: "report.pdf", Size: 48213, Modified: day}, &action, files.FileCardOptions{
				Menu: "t:menu", Open: "t:open", Rename: "t:rename", Delete: "t:remove", FormatDate: func(time.Time) string { return "t:date" }})
		}
	}},
	{name: "input.KeyValueField", english: []string{"Add row", "Key", "Value"}, fixed: []string{"Remove row"}, idle: func() func(*ui.Context) {
		rows := []input.KeyValueRow{{Key: "PATH", Value: "/usr/bin"}, {}}
		return func(c *ui.Context) {
			input.KeyValueField(c, &rows, input.KeyValueFieldOptions{Add: "t:add", Key: "t:name", Value: "t:content"})
		}
	}},
	{name: "input.PasswordField", fixed: []string{"Reveal t:secret"}, idle: func() func(*ui.Context) {
		v := "hunter2A!"
		return func(c *ui.Context) {
			input.PasswordField(c, "t:secret", &v, input.PasswordFieldOptions{Placeholder: "t:hint"})
		}
	}},
	{name: "input.TagInput", english: []string{"Add tag"}, idle: func() func(*ui.Context) {
		tags, draft := []string{"go", "ui"}, ""
		return func(c *ui.Context) { input.TagInput(c, &tags, &draft, input.TagInputOptions{Placeholder: "t:hint"}) }
	}},
	{name: "feedback.ErrorState", english: []string{"Something went wrong", "Retry"}, idle: func() func(*ui.Context) {
		return func(c *ui.Context) {
			feedback.ErrorState(c, "t:message", feedback.ErrorStateOptions{Title: "t:title", Retry: "t:again"})
		}
	}},
	{name: "feedback.RetryIn", english: []string{"Retry"}, idle: func() func(*ui.Context) {
		return func(c *ui.Context) {
			feedback.RetryIn(c, c.Now().Add(-time.Minute), feedback.RetryInOptions{Wait: "t:wait %d", Retry: "t:again"})
		}
	}, active: func() func(*ui.Context) {
		var at time.Time
		return func(c *ui.Context) {
			if at.IsZero() {
				at = c.Now().Add(time.Hour)
			}
			feedback.RetryIn(c, at, feedback.RetryInOptions{Wait: "t:wait %d", Retry: "t:again"})
		}
	}},
	{name: "feedback.SaveStatus", english: []string{"Saved", "Saving"}, idle: func() func(*ui.Context) {
		return func(c *ui.Context) {
			feedback.SaveStatus(c, false, feedback.SaveStatusOptions{Saving: "t:busy", Saved: "t:kept"})
		}
	}, active: func() func(*ui.Context) {
		return func(c *ui.Context) {
			feedback.SaveStatus(c, true, feedback.SaveStatusOptions{Saving: "t:busy", Saved: "t:kept"})
		}
	}},
	{name: "feedback.ConnectionStatus", english: []string{"Online", "Reconnecting", "Offline"}, idle: func() func(*ui.Context) {
		o := feedback.ConnectionStatusOptions{Online: "t:up", Reconnecting: "t:again", Offline: "t:down"}
		return func(c *ui.Context) {
			feedback.ConnectionStatus(c, feedback.Online, o)
			feedback.ConnectionStatus(c, feedback.Offline, o)
		}
	}, active: func() func(*ui.Context) {
		return func(c *ui.Context) {
			feedback.ConnectionStatus(c, feedback.Reconnecting, feedback.ConnectionStatusOptions{Online: "t:up", Reconnecting: "t:again", Offline: "t:down"})
		}
	}},
	{name: "form.TypeToConfirm", english: []string{"to confirm", "Delete"}, idle: func() func(*ui.Context) {
		typed := "sail"
		return func(c *ui.Context) {
			form.TypeToConfirm(c, "sailry-go", &typed, form.TypeToConfirmOptions{Prompt: "t:type %s", Action: "t:remove"})
		}
	}},
	{name: "overlay.DangerDialog", english: []string{"Cancel", "Delete"}, idle: func() func(*ui.Context) {
		open := true
		return func(c *ui.Context) {
			overlay.DangerDialog(c, "t:remove project", "t:consequence", &open, overlay.DangerDialogOptions{Cancel: "t:keep", Confirm: "t:remove"})
		}
	}},
	{name: "agent.ImageGenerating", idle: func() func(*ui.Context) {
		return func(c *ui.Context) { agent.ImageGenerating(c, "a lighthouse at dusk", 1) }
	}, active: func() func(*ui.Context) {
		return func(c *ui.Context) { agent.ImageGenerating(c, "a lighthouse at dusk", 0.4) }
	}},
	{name: "agent.SourceCard", idle: func() func(*ui.Context) {
		return func(c *ui.Context) { agent.SourceCard(c, 1, cited) }
	}},
	{name: "agent.Citation", idle: func() func(*ui.Context) {
		return func(c *ui.Context) { agent.Citation(c, 2, cited) }
	}},
	{name: "agent.RunTimeline", english: []string{"3 steps"}, idle: func() func(*ui.Context) {
		return func(c *ui.Context) {
			agent.RunTimeline(c, "t:run", steps, agent.RunTimelineOptions{Format: "t:%d of %s"})
		}
	}, active: func() func(*ui.Context) {
		running := append(slices.Clone(steps), agent.RunStep{Name: "lint", Duration: time.Second, State: agent.StepRunning})
		return func(c *ui.Context) {
			agent.RunTimeline(c, "t:run", running, agent.RunTimelineOptions{Format: "t:%d of %s"})
		}
	}},
	{name: "agent.TaskProgress", english: []string{"Step 3 of 3", "Finished"}, idle: func() func(*ui.Context) {
		return func(c *ui.Context) {
			agent.TaskProgress(c, "t:task", []string{"read", "edit", "check"}, 3, agent.TaskProgressOptions{Format: "t:%d/%d", Finished: "t:done"})
		}
	}, active: func() func(*ui.Context) {
		return func(c *ui.Context) {
			agent.TaskProgress(c, "t:task", []string{"read", "edit", "check"}, 1, agent.TaskProgressOptions{Format: "t:%d/%d", Finished: "t:done"})
		}
	}},
	{name: "chat.LinkPreview", idle: func() func(*ui.Context) {
		return func(c *ui.Context) { chat.LinkPreview(c, "Go release notes", "go.dev", photo) }
	}},
	{name: "chat.JumpToLatest", english: []string{"new message"}, idle: func() func(*ui.Context) {
		return func(c *ui.Context) {
			chat.JumpToLatest(c, 3, chat.JumpToLatestOptions{One: "t:%d one", Many: "t:%d many"})
		}
	}},
	{name: "overlay.Lightbox", english: []string{"Previous", "Next", "Close"}, idle: func() func(*ui.Context) {
		at, open := 0, true
		return func(c *ui.Context) {
			overlay.Lightbox(c, "t:images", []*ui.Bitmap{photo, photo}, []string{"t:first", "t:second"}, &at, &open,
				overlay.LightboxOptions{Back: "t:back", Forth: "t:forth", Close: "t:shut"})
		}
	}},
	{name: "overlay.ShortcutSheet", english: []string{"Close"}, idle: func() func(*ui.Context) {
		open := true
		groups := []overlay.ShortcutGroup{{Title: "t:panel", Shortcuts: []overlay.Shortcut{{Label: "t:files", Keys: []string{"⌘", "P"}}}}}
		return func(c *ui.Context) {
			overlay.ShortcutSheet(c, "t:shortcuts", groups, &open, overlay.ShortcutSheetOptions{Close: "t:shut"})
		}
	}},
	{name: "overlay.WhatsNew", english: []string{"Back", "Next", "Done"}, idle: func() func(*ui.Context) {
		page, open := 0, true
		features := []overlay.Feature{{Icon: "sparkles", Title: "t:first", Text: "t:text"}, {Icon: "rocket", Title: "t:second"}}
		return func(c *ui.Context) {
			overlay.WhatsNew(c, "t:news", features, &page, &open, overlay.WhatsNewOptions{Back: "t:back", Next: "t:forth", Done: "t:shut"})
		}
	}},
	{name: "desktop.UpdatePrompt", english: []string{"A new version", "Version ", "Install update", "Later"}, idle: func() func(*ui.Context) {
		return func(c *ui.Context) {
			desktop.UpdatePrompt(c, "Sailry", "1.2.0", []string{"t:note"},
				desktop.UpdatePromptOptions{Title: "t:%s ready", Version: "t:v%s", Install: "t:install", Later: "t:wait"})
		}
	}},
	{name: "overlay.HoverCard", hover: "t:anchor", idle: func() func(*ui.Context) {
		return func(c *ui.Context) {
			anchor := ui.Button(c, "t:anchor")
			overlay.HoverCard(c, "t:card", anchor, func() { ui.Text(c, "t:inside") })
		}
	}},
	{name: "navigation.ZoomControl", english: []string{"Zoom out", "Zoom in", "Fit"}, idle: func() func(*ui.Context) {
		zoom := 1.5
		return func(c *ui.Context) {
			navigation.ZoomControl(c, "t:zoom", &zoom, navigation.ZoomControlOptions{Out: "t:less", In: "t:more", Fit: "t:all"})
		}
	}},
	{name: "media.Waveform", idle: func() func(*ui.Context) {
		return func(c *ui.Context) {
			media.Waveform(c, "t:clip", []float64{0.2, 0.8, 0.5, 1, 0.1, 0.6}, 0.5, media.WaveformOptions{})
		}
	}},
	{name: "media.Recorder", english: []string{"Record", "Pause", "Resume", "Stop"}, idle: func() func(*ui.Context) {
		take := media.Idle
		return func(c *ui.Context) { media.Recorder(c, "t:voice", &take, 0.3, recorderCopy) }
	}, active: func() func(*ui.Context) {
		take := media.Recording
		return func(c *ui.Context) { media.Recorder(c, "t:voice", &take, 0.3, recorderCopy) }
	}},
	{name: "media.AudioPlayer", english: []string{"Play", "Position"}, idle: func() func(*ui.Context) {
		pos, playing := time.Duration(0), false
		return func(c *ui.Context) {
			media.AudioPlayer(c, "t:clip", clip, &pos, &playing, media.AudioPlayerOptions{Play: "t:go", Seek: "t:at"})
		}
	}, active: func() func(*ui.Context) {
		pos, playing := time.Duration(0), true
		return func(c *ui.Context) {
			// A clip that ends would stop itself: this one is kept playing.
			pos, playing = 0, true
			media.AudioPlayer(c, "t:clip", clip, &pos, &playing, media.AudioPlayerOptions{Play: "t:go", Seek: "t:at"})
		}
	}},
	{name: "devtools.LogView", english: []string{"Logs", "Search logs", "Follow", "No matching", "INFO", "ERROR"}, idle: func() func(*ui.Context) {
		query, follow := "", true
		return func(c *ui.Context) { devtools.LogView(c, logLines, &query, &follow, logCopy) }
	}},
	{name: "devtools.LevelFilter", english: []string{"Debug", "Info", "Warn", "Error"}, idle: func() func(*ui.Context) {
		on := [4]bool{true, true, false, true}
		return func(c *ui.Context) {
			devtools.LevelFilter(c, &on, [4]int{4, 12, 0, 1}, devtools.LevelFilterOptions{Levels: logCopy.Levels})
		}
	}},
	{name: "code.CommitGraph", idle: func() func(*ui.Context) {
		return func(c *ui.Context) { code.CommitGraph(c, "t:history", history) }
	}},
	{name: "code.BranchPicker", english: []string{"Search branches", "No branches", "current"}, fixed: []string{"Clear search"}, idle: func() func(*ui.Context) {
		current, query := 0, "ma"
		branches := []code.Branch{{Name: "main"}, {Name: "feature/map", Ahead: 2, Behind: 1}}
		return func(c *ui.Context) {
			code.BranchPicker(c, "t:branches", branches, &current, &query, code.BranchPickerOptions{Placeholder: "t:find", Empty: "t:none", Current: "t:here"})
		}
	}},
	{name: "code.Blame", english: []string{"now", "1d"}, idle: func() func(*ui.Context) {
		lines := []code.BlameLine{{Author: "Ada", Text: "package main", At: day}, {Author: "Lin", Text: "func main() {}", At: day.Add(48 * time.Hour)}}
		return func(c *ui.Context) {
			code.Blame(c, "t:blame", lines, code.BlameOptions{Lang: "go", FormatAge: func(time.Duration) string { return "t:age" }})
		}
	}},
	{name: "code.DiffView", english: []string{"Changes", "Split", "Unified"}, idle: func() func(*ui.Context) {
		st, split := changes(), false
		return func(c *ui.Context) { code.DiffView(c, st, &split, diffCopy) }
	}},
	{name: "files.FileIcon", idle: func() func(*ui.Context) {
		return func(c *ui.Context) {
			files.FileIcon(c, "report.pdf", files.FileIconOptions{})
			files.FileIcon(c, "notes", files.FileIconOptions{Size: 28})
		}
	}},
	{name: "files.FileList", english: []string{"Name", "Size", "Modified", "Nov 2, 2026"}, fixed: []string{"KB"}, idle: func() func(*ui.Context) {
		selected, sort := 1, files.SortName
		return func(c *ui.Context) { files.FileList(c, "t:files", folder, &selected, &sort, listCopy) }
	}},
	{name: "diagram.Mermaid", english: []string{"Diagram"}, idle: func() func(*ui.Context) {
		return func(c *ui.Context) {
			diagram.Mermaid(c, "flowchart LR\n  Cart --> Pay\n  Pay -->|ok| Shipped", diagram.MermaidOptions{Label: "t:flow", Failed: "t:failed"})
		}
	}},
}

var (
	recorderCopy = media.RecorderOptions{Record: "t:start", Pause: "t:hold", Resume: "t:on", Stop: "t:end"}
	logCopy      = devtools.LogViewOptions{Title: "t:title", Search: "t:find", Follow: "t:tail", Empty: "t:none", Levels: [4]string{"t:d", "t:i", "t:w", "t:e"}}
	clip         = media.Clip{Notes: []float64{440, 0, 660, 880}, Beat: 10 * time.Minute}
	diffCopy     = code.DiffViewOptions{Name: "main.go", Split: "t:two", Unified: "t:one"}
	folder       = []files.FileItem{{Name: "docs", Dir: true, Modified: day}, {Name: "report.pdf", Size: 48213, Modified: day}}
	listCopy     = files.FileListOptions{Name: "t:name", Size: "t:size", Date: "t:date", FormatDate: func(time.Time) string { return "t:day" }}
	updateCopy   = desktop.UpdatePromptOptions{Title: "t:%s ready", Version: "t:v%s", Install: "t:install", Later: "t:wait"}
)

// changes is a two-line edit, as a panel's diff tab holds one.
func changes() *editor.DiffState {
	return &editor.DiffState{Old: editor.NewBuffer("package main\n\nfunc main() {}\n"), New: editor.NewBuffer("package main\n\nfunc main() { run() }\n")}
}

// stage is one headless window showing a view in the application's theme.
type stage struct {
	tt             *ui.Tester
	builds, paints int
}

func newStage(view func(*ui.Context), mode theme.Mode) *stage {
	s := &stage{}
	// Opaque surfaces: frost costs most of a headless frame and none of
	// the assertions.
	s.tt = ui.NewTesterWith(func(c *ui.Context) {
		theme.Apply(c, "", mode)
		s.builds++
		ui.Column(c).Fill().Padding(24).Gap(12).Background(c.Theme().Background).
			DrawOver(func(*ui.Painter, ui.Rect) { s.paints++ }).Children(func() { view(c) })
	}, 640, 520, ui.TesterOptions{Dark: mode == theme.Dark, Preferences: ui.Preferences{ReduceTransparency: true}})
	s.tt.SetLocale("en")
	s.tt.Frame()
	return s
}

// settle lets arrivals and transitions finish.
func (s *stage) settle() {
	s.tt.Advance(3 * time.Second)
	s.tt.Frame()
}

// moves reports whether the window builds, paints or asks for a frame while
// d passes with no input.
func (s *stage) moves(d time.Duration) bool {
	builds, paints := s.builds, s.paints
	asked := s.tt.FramePending()
	s.tt.Advance(d)
	return asked || s.tt.FramePending() || s.builds != builds || s.paints != paints
}

// said is everything the frame shows or tells a screen reader.
func (s *stage) said() []string {
	out := s.tt.Texts()
	for _, a := range s.tt.AccessAll() {
		out = append(out, a.Name, a.Description, a.Placeholder)
	}
	return out
}

// shot writes the last frame to $SAILRY_SNAPSHOTS/name.png, when set.
func (s *stage) shot(t *testing.T, name string) {
	dir := os.Getenv("SAILRY_SNAPSHOTS")
	if dir == "" {
		return
	}
	f, err := os.Create(filepath.Join(dir, strings.ReplaceAll(name, ".", "-")+".png"))
	if err != nil {
		t.Fatal(err)
	}
	defer f.Close()
	if err := png.Encode(f, s.tt.Image()); err != nil {
		t.Fatal(err)
	}
}

// Every approved component builds in the application's light and dark themes,
// comes to rest without asking for another frame, shows no English of its own
// once its options are filled, and moves only in the states documented to,
// which stop in a hidden window.
func TestApprovedComponents(t *testing.T) {
	said := func(all []string, s string) bool {
		return slices.ContainsFunc(all, func(v string) bool { return strings.Contains(v, s) })
	}
	for _, p := range probes {
		t.Run(p.name, func(t *testing.T) {
			for _, mode := range []theme.Mode{theme.Light, theme.Dark} {
				name := map[theme.Mode]string{theme.Light: "light", theme.Dark: "dark"}[mode]
				s := newStage(p.idle(), mode)
				if p.hover != "" {
					at, ok := s.tt.Find(p.hover)
					if !ok {
						t.Fatalf("%s: no %q to hover: %q", name, p.hover, s.tt.Texts())
					}
					s.tt.Move(at.X+at.W/2, at.Y+at.H/2)
					s.tt.Frame()
				}
				s.settle()
				s.shot(t, p.name+"-"+name)
				if s.moves(10 * time.Second) {
					t.Errorf("%s: redraws while idle", name)
				}
				all := s.said()
				for _, english := range p.english {
					if said(all, english) {
						t.Errorf("%s: shows its own %q with every option filled: %q", name, english, all)
					}
				}
				for _, english := range p.fixed {
					if !said(all, english) {
						t.Errorf("%s: %q is documented as built in and is gone: update docs/crychic.md: %q", name, english, all)
					}
				}
				if p.active == nil {
					continue
				}
				s = newStage(p.active(), mode)
				s.settle()
				s.shot(t, p.name+"-active-"+name)
				if !s.moves(2 * time.Second) {
					t.Errorf("%s: documented as moving while active and does not", name)
				}
				s.tt.SetWindowHidden(true)
				s.tt.Frame()
				s.tt.Frame()
				if s.moves(10 * time.Second) {
					t.Errorf("%s: redraws in a hidden window", name)
				}
			}
		})
	}
}

// records keeps what the library logs.
type records struct {
	mu   sync.Mutex
	seen []slog.Record
}

func (r *records) Enabled(context.Context, slog.Level) bool { return true }
func (r *records) WithAttrs([]slog.Attr) slog.Handler       { return r }
func (r *records) WithGroup(string) slog.Handler            { return r }
func (r *records) Handle(_ context.Context, record slog.Record) error {
	r.mu.Lock()
	defer r.mu.Unlock()
	r.seen = append(r.seen, record)
	return nil
}

// Setup mutes the library before anything plays, its logs stay at debug, and
// the adapters build, or decline, data no component would take. The library
// panics exactly where docs/crychic.md says, DiffView and CommitGraph size
// and settle as it says, and the floating surfaces it says hide the page do,
// while content hosted in Kit's leaves the frost showing.
func TestPolicy(t *testing.T) {
	t.Run("setup mutes every cue and starts no audio output", func(t *testing.T) {
		logs := &records{}
		previous := slog.Default()
		slog.SetDefault(slog.New(logs))
		t.Cleanup(func() { slog.SetDefault(previous) })
		// A shipped binary starts with cues on; a test binary does not.
		sound.SetEnabled(true)
		Setup()
		if sound.Enabled() {
			t.Fatal("Setup left cues on")
		}
		var on [4]bool
		s := newStage(func(c *ui.Context) {
			devtools.LevelFilter(c, &on, [4]int{}, devtools.LevelFilterOptions{Levels: logCopy.Levels})
		}, theme.Light)
		s.settle()
		if err := s.tt.Click("t:w"); err != nil {
			t.Fatal(err)
		}
		s.settle()
		if !on[devtools.LevelWarn] {
			t.Fatal("the click did not reach the component")
		}
		// The cue was asked for and muted, so the path that would play ran.
		if recent := sound.Recent(); len(recent) == 0 || recent[len(recent)-1] != sound.Toggle {
			t.Fatalf("the interaction asked for no cue: %v", recent)
		}
		stacks := make([]byte, 1<<20)
		stacks = stacks[:runtime.Stack(stacks, true)]
		for _, running := range []string{"github.com/egoist/mygo/audio.", "CrychicUI/sound.watch"} {
			if strings.Contains(string(stacks), running) {
				t.Errorf("a muted cue started %s", running)
			}
		}
		logs.mu.Lock()
		defer logs.mu.Unlock()
		if len(logs.seen) == 0 {
			t.Fatal("the library logged nothing to judge")
		}
		for _, record := range logs.seen {
			if record.Level > slog.LevelDebug || !strings.HasPrefix(record.Message, "crychic: ") {
				t.Errorf("the library logs %q at %v", record.Message, record.Level)
			}
		}
	})
	t.Run("adapters take what the components refuse", func(t *testing.T) {
		if Unit(math.NaN()) != 0 || Unit(-1) != 0 || Unit(7) != 1 || Unit(math.Inf(1)) != 1 || Unit(0.25) != 0.25 {
			t.Error("Unit leaves a share outside 0 to 1")
		}
		if got, ok := source(1, agent.Source{URL: "https://go.dev/doc"}); !ok || got.Host != "go.dev" || got.Title != "go.dev" {
			t.Errorf("a source with only an address became %+v %v", got, ok)
		}
		zoom, at, current, open, shut := math.NaN(), 9, -3, true, true
		query, follow := "", false
		ok := map[string]bool{}
		broken := []code.GraphCommit{{Hash: "a"}, {Hash: "a"}}
		s := newStage(func(c *ui.Context) {
			_, ok["image"] = ImageGenerating(c, "a lighthouse", math.NaN())
			_, ok["image, no prompt"] = ImageGenerating(c, "", 2)
			_, ok["source"] = SourceCard(c, 1, agent.Source{URL: "https://go.dev/doc"})
			_, ok["source, no address"] = SourceCard(c, 1, agent.Source{Title: "Go", Host: "go.dev"})
			_, ok["citation"] = Citation(c, 1, agent.Source{Host: "go.dev", URL: "https://go.dev"})
			_, ok["citation, no number"] = Citation(c, 0, cited)
			_, ok["timeline"] = RunTimeline(c, "t:run", []agent.RunStep{{Duration: time.Second}, {Name: "edit", Duration: -time.Second, State: 9}, {Name: "test", Duration: time.Second}}, agent.RunTimelineOptions{})
			_, ok["timeline, no time yet"] = RunTimeline(c, "t:run", []agent.RunStep{{Name: "plan", State: agent.StepRunning}}, agent.RunTimelineOptions{})
			_, ok["task"] = TaskProgress(c, "t:task", []string{"read", ""}, 7, agent.TaskProgressOptions{})
			_, ok["task, no steps"] = TaskProgress(c, "t:task", nil, 0, agent.TaskProgressOptions{})
			_, ok["link"] = LinkPreview(c, "", "go.dev", nil)
			_, ok["link, no host"] = LinkPreview(c, "Go", "", nil)
			_, ok["lightbox"] = Lightbox(c, "t:images", []*ui.Bitmap{photo, photo}, []string{"one"}, &at, &open, overlay.LightboxOptions{})
			_, ok["lightbox, undecoded"] = Lightbox(c, "t:images", []*ui.Bitmap{photo, nil}, nil, &at, &shut, overlay.LightboxOptions{})
			ZoomControl(c, "t:zoom", &zoom, navigation.ZoomControlOptions{})
			_, ok["waveform"] = Waveform(c, "t:clip", []float64{-1, 3, math.NaN()}, 4, media.WaveformOptions{})
			_, ok["waveform, no levels"] = Waveform(c, "t:clip", nil, 0, media.WaveformOptions{})
			LogView(c, []devtools.LogLine{{ID: "1", Message: "kept"}, {ID: "1", Time: "09:30", Message: "kept again", Level: 9},
				{ID: " ", Time: "09:31", Message: "kept too", Level: -4}, {ID: "#1", Time: "09:32", Message: "kept as well"}, {ID: "5", Time: "09:33"}}, &query, &follow, devtools.LogViewOptions{})
			_, ok["graph"] = CommitGraph(c, "t:history", []code.GraphCommit{{Hash: "b", Lane: 11, Parents: []string{"a", "gone"}}, {Hash: "a", Lane: -2}})
			_, ok["graph, repeated commit"] = CommitGraph(c, "t:history", broken)
			_, ok["graph, child after parent"] = CommitGraph(c, "t:history", []code.GraphCommit{{Hash: "a"}, {Hash: "b", Parents: []string{"a"}}})
			_, _, ok["branches"] = BranchPicker(c, "t:branches", []code.Branch{{Name: "main", Ahead: -1, Behind: -1}}, &current, &query, code.BranchPickerOptions{})
			_, _, ok["branches, repeated"] = BranchPicker(c, "t:branches", []code.Branch{{Name: "main"}, {Name: "main"}}, &current, &query, code.BranchPickerOptions{})
			_, ok["blame"] = Blame(c, "t:blame", []code.BlameLine{{Text: "package main", At: day}}, code.BlameOptions{})
			_, ok["blame, no time"] = Blame(c, "t:blame", []code.BlameLine{{Author: "Ada", Text: "package main"}}, code.BlameOptions{})
			_, ok["mermaid"] = Mermaid(c, "flowchart LR\n  Cart --> Pay", diagram.MermaidOptions{Failed: "t:failed"})
			_, ok["mermaid, a syntax error"] = Mermaid(c, "flowchart LR\n  Cart -->", diagram.MermaidOptions{Failed: "t:failed"})
			_, ok["mermaid, no kind"] = Mermaid(c, "no such diagram", diagram.MermaidOptions{Failed: "t:failed"})
			_, ok["mermaid, a reserved property"] = Mermaid(c, "flowchart LR\n  A --> B\n  style A fill:var(--pele-bg)", diagram.MermaidOptions{Failed: "t:failed"})
		}, theme.Light)
		s.settle()
		for name, built := range ok {
			if want := !strings.Contains(name, ","); built != want {
				t.Errorf("%s: built %v, want %v", name, built, want)
			}
		}
		if zoom != 1 || at != 1 || current != 0 || !open || shut {
			t.Errorf("bound values after repair: zoom %v at %d current %d open %v shut %v", zoom, at, current, open, shut)
		}
		for _, text := range []string{"kept", "kept again", "kept too", "kept as well"} {
			if !s.tt.HasText(text) {
				t.Errorf("the log lost %q: %q", text, s.tt.Texts())
			}
		}
	})
	t.Run("the library panics exactly where the documents say", func(t *testing.T) {
		at, open, sheet, split := 0, true, true, false
		query, follow, current := "", false, 0
		none, past, sort := -1, 5, files.SortName
		named := func(commits ...code.GraphCommit) []code.GraphCommit {
			for i := range commits {
				commits[i].Message = "edit"
			}
			return commits
		}
		cases := []struct {
			name   string
			panics bool
			build  func(c *ui.Context)
		}{
			{"files.FileList: an empty directory", true, func(c *ui.Context) { files.FileList(c, "t:files", nil, &none, &sort, listCopy) }},
			{"files.FileList: a repeated name", true, func(c *ui.Context) {
				files.FileList(c, "t:files", []files.FileItem{{Name: "a.txt"}, {Name: "a.txt"}}, &none, &sort, listCopy)
			}},
			{"files.FileList: an unnamed entry", true, func(c *ui.Context) { files.FileList(c, "t:files", []files.FileItem{{}}, &none, &sort, listCopy) }},
			{"files.FileList: a selection past the end", true, func(c *ui.Context) { files.FileList(c, "t:files", folder, &past, &sort, listCopy) }},
			{"files.FileList: one name in two folders", false, func(c *ui.Context) {
				files.FileList(c, "t:files", []files.FileItem{{Name: "a.txt", Path: "/one"}, {Name: "a.txt", Path: "/two"}}, &none, &sort, listCopy)
			}},
			{"files.FileIcon: no name", true, func(c *ui.Context) { files.FileIcon(c, "", files.FileIconOptions{}) }},
			{"files.FileIcon: a dotfile", false, func(c *ui.Context) { files.FileIcon(c, ".gitignore", files.FileIconOptions{}) }},
			{"code.CommitGraph: no commits", true, func(c *ui.Context) { code.CommitGraph(c, "t:history", nil) }},
			{"code.CommitGraph: no message", true, func(c *ui.Context) { code.CommitGraph(c, "t:history", []code.GraphCommit{{Hash: "a"}}) }},
			{"code.CommitGraph: a ninth lane", true, func(c *ui.Context) { code.CommitGraph(c, "t:history", named(code.GraphCommit{Hash: "a", Lane: 8})) }},
			{"code.CommitGraph: a repeated hash", true, func(c *ui.Context) {
				code.CommitGraph(c, "t:history", named(code.GraphCommit{Hash: "a"}, code.GraphCommit{Hash: "a"}))
			}},
			{"code.CommitGraph: a parent before its child", true, func(c *ui.Context) {
				code.CommitGraph(c, "t:history", named(code.GraphCommit{Hash: "a"}, code.GraphCommit{Hash: "b", Parents: []string{"a"}}))
			}},
			{"code.CommitGraph: a parent outside the page", false, func(c *ui.Context) {
				code.CommitGraph(c, "t:history", named(code.GraphCommit{Hash: "c", Lane: 7, Parents: []string{"gone"}}))
			}},
			{"code.BranchPicker: no branches", true, func(c *ui.Context) {
				code.BranchPicker(c, "t:branches", nil, &current, &query, code.BranchPickerOptions{})
			}},
			{"code.DiffView: no state", true, func(c *ui.Context) { code.DiffView(c, nil, &split, diffCopy) }},
			{"overlay.Lightbox: an image not decoded", true, func(c *ui.Context) {
				overlay.Lightbox(c, "t:images", []*ui.Bitmap{nil}, nil, &at, &open, overlay.LightboxOptions{})
			}},
			{"overlay.ShortcutSheet: no groups", true, func(c *ui.Context) {
				overlay.ShortcutSheet(c, "t:keys", nil, &sheet, overlay.ShortcutSheetOptions{})
			}},
			{"devtools.LogView: a line without a time", true, func(c *ui.Context) {
				devtools.LogView(c, []devtools.LogLine{{ID: "1", Message: "ready"}}, &query, &follow, logCopy)
			}},
			{"devtools.LogView: a line without a message", true, func(c *ui.Context) {
				devtools.LogView(c, []devtools.LogLine{{ID: "1", Time: "09:30"}}, &query, &follow, logCopy)
			}},
			{"chat.JumpToLatest: nothing new", false, func(c *ui.Context) { chat.JumpToLatest(c, 0, chat.JumpToLatestOptions{}) }},
			{"feedback.ErrorState: no message", false, func(c *ui.Context) {
				feedback.ErrorState(c, "", feedback.ErrorStateOptions{Title: "t:title", Retry: "t:again"})
			}},
		}
		// Each refusal comes before the component builds anything, so the
		// frame around a recovered one stays whole.
		got := map[string]bool{}
		s := newStage(func(c *ui.Context) {
			for _, k := range cases {
				func() {
					defer func() { got[k.name] = recover() != nil }()
					k.build(c)
				}()
			}
		}, theme.Light)
		s.settle()
		for _, k := range cases {
			if got[k.name] != k.panics {
				t.Errorf("%s: panicked %v, documented %v", k.name, got[k.name], k.panics)
			}
		}
		// A dotfile's whole name is its extension pill, and the Retry of an
		// error state cannot be left out.
		for _, text := range []string{"GITIGNORE", "t:again"} {
			if !s.tt.HasText(text) {
				t.Errorf("%q is documented as shown and is not: %q", text, s.tt.Texts())
			}
		}
	})
	t.Run("DiffView keeps its height unless told to fill, and its flag owns the layout", func(t *testing.T) {
		// body is the diff's height in a tab body h tall, as built or
		// with Grow(1) on the root it returns. As built it keeps its
		// default in a taller body and shrinks into a shorter one.
		body := func(h int, fill bool) (got, standard float32) {
			st, split := changes(), false
			tt := ui.NewTesterWith(func(c *ui.Context) {
				theme.Apply(c, "", theme.Light)
				standard = c.Theme().Space(80)
				ui.Column(c).Fill().Children(func() {
					if root := code.DiffView(c, st, &split, diffCopy); fill {
						root.Grow(1)
					}
				})
			}, 640, h, ui.TesterOptions{Preferences: ui.Preferences{ReduceTransparency: true}})
			tt.Frame()
			tt.Advance(time.Second)
			tt.Frame()
			box, ok := tt.AccessByRole(ui.RoleGroup, diffCopy.Name)
			if !ok {
				t.Fatalf("no diff group in a body of %d", h)
			}
			return box.Bounds.H, standard
		}
		for _, k := range []struct {
			h    int
			fill bool
			want float32
		}{{600, false, 0}, {200, false, 200}, {600, true, 600}, {200, true, 200}} {
			// Zero is the component's own default, 80 spaces of the theme.
			got, standard := body(k.h, k.fill)
			if k.want == 0 {
				k.want = standard
			}
			if got != k.want {
				t.Errorf("body %d, fill %v: the diff is %v tall, documented %v", k.h, k.fill, got, k.want)
			}
		}
		st, split := changes(), false
		st.Layout = editor.DiffSplit
		s := newStage(func(c *ui.Context) { code.DiffView(c, st, &split, diffCopy) }, theme.Light)
		s.settle()
		if st.Layout != editor.DiffUnified {
			t.Errorf("the state kept layout %v against the flag", st.Layout)
		}
		if !s.tt.HasText("+1") || !s.tt.HasText("−1") {
			t.Errorf("the header lost the counts: %q", s.tt.Texts())
		}
		if err := s.tt.Click("t:two"); err != nil {
			t.Fatal(err)
		}
		s.settle()
		if !split || st.Layout != editor.DiffSplit {
			t.Errorf("after the split side: flag %v, layout %v", split, st.Layout)
		}
	})
	// The Git history puts a row of its own over each of the graph's, by
	// these figures: a library that lays its rows elsewhere fails here.
	t.Run("CommitGraph lays its rows where CommitGraphRows says", func(t *testing.T) {
		var top, height, lanes, pad float32
		s := newStage(func(c *ui.Context) {
			top, height = CommitGraphRows(c)
			lanes, pad = CommitGraphLanes(c, 2), CommitGraphPad(c)
			CommitGraph(c, "t:history", history)
		}, theme.Light)
		s.settle()
		list, ok := s.tt.AccessByRole(ui.RoleList, "t:history")
		if !ok {
			t.Fatal("no graph")
		}
		for i, k := range history {
			row, ok := s.tt.AccessByRole(ui.RoleListItem, k.Message)
			if !ok {
				t.Fatalf("no row %q", k.Message)
			}
			if want := list.Bounds.Y + top + float32(i)*height; row.Bounds.Y != want || row.Bounds.H != height {
				t.Errorf("row %d is at %v, %v tall; CommitGraphRows says %v, %v", i, row.Bounds.Y, row.Bounds.H, want, height)
			}
		}
		// The text starts after the lanes: two here.
		hash, ok := s.tt.Find(history[0].Hash[:7])
		if want := list.Bounds.X + pad + lanes; !ok || hash.X != want {
			t.Errorf("the text starts at %v, want %v after two lanes", hash.X, want)
		}
	})
	t.Run("CommitGraph builds every row of a long history and comes to rest", func(t *testing.T) {
		const n = 400
		hash := func(i int) string { return fmt.Sprintf("%07x", n-i) }
		long := make([]code.GraphCommit, n)
		for i := range long {
			k := code.GraphCommit{Hash: hash(i), Message: fmt.Sprintf("commit %d", n-i), Author: "Ada"}
			if i+1 < n {
				k.Parents = []string{hash(i + 1)}
			}
			// A merge every twenty commits, and a side lane after it.
			if i%20 == 0 && i+3 < n {
				k.Parents = append(k.Parents, hash(i+3))
			}
			if i%20 == 1 || i%20 == 2 {
				k.Lane = 1
			}
			long[i] = k
		}
		built := false
		start := time.Now()
		s := newStage(func(c *ui.Context) {
			ui.Scroll(c).FillWidth().Grow(1).Children(func() { _, built = CommitGraph(c, "t:history", long) })
		}, theme.Light)
		first := time.Since(start)
		s.settle()
		if !built {
			t.Fatal("the adapter declined a well-formed history")
		}
		rows := 0
		for _, a := range s.tt.AccessAll() {
			if a.Role == ui.RoleListItem {
				rows++
			}
		}
		list, _ := s.tt.AccessByRole(ui.RoleList, "t:history")
		t.Logf("%d commits: %d rows built, %v tall, first frames in %v", n, rows, list.Bounds.H, first)
		if rows != n || list.Bounds.H < n*30 {
			t.Errorf("%d rows built, %v tall, for %d commits: documented as one each at full height, not virtual", rows, list.Bounds.H, n)
		}
		if s.moves(10 * time.Second) {
			t.Error("a long history redraws while idle")
		}
	})
	t.Run("floating surfaces stay Kit's frost", func(t *testing.T) {
		above := func(text string) func(*ui.Tester) (x, y float32, ok bool) {
			return func(tt *ui.Tester) (float32, float32, bool) {
				r, ok := tt.Find(text)
				return r.X - 4, r.Y - 4, ok
			}
		}
		// before is the gap at the start of a text, where the title is
		// also the name of the whole component.
		before := func(text string) func(*ui.Tester) (x, y float32, ok bool) {
			return func(tt *ui.Tester) (float32, float32, bool) {
				r, ok := tt.Find(text)
				return r.X - 6, r.Y + r.H/2, ok
			}
		}
		// hosted is content in Kit's dialog, the frosted surface.
		hosted := func(content func(c *ui.Context)) func() func(*ui.Context) {
			return func() func(*ui.Context) {
				open := true
				return func(c *ui.Context) { kit.Dialog(c, &open, func() { content(c) }) }
			}
		}
		cases := []struct {
			name string
			// covers is what the documents say: the surface at the spot is
			// the component's own and hides what is under it.
			covers bool
			view   func() func(*ui.Context)
			show   func(tt *ui.Tester) error
			spot   func(tt *ui.Tester) (x, y float32, ok bool)
		}{
			{name: "kit.Dialog", view: hosted(func(c *ui.Context) { ui.Text(c, "t:ours") }), spot: above("t:ours")},
			{name: "overlay.DangerDialog", covers: true, view: func() func(*ui.Context) {
				open := true
				return func(c *ui.Context) {
					overlay.DangerDialog(c, "t:remove project", "t:consequence", &open, overlay.DangerDialogOptions{Cancel: "t:keep", Confirm: "t:remove"})
				}
			}, spot: above("t:remove project")},
			{name: "overlay.ShortcutSheet", covers: true, view: func() func(*ui.Context) {
				open := true
				groups := []overlay.ShortcutGroup{{Title: "t:panel", Shortcuts: []overlay.Shortcut{{Label: "t:files", Keys: []string{"P"}}}}}
				return func(c *ui.Context) {
					overlay.ShortcutSheet(c, "t:shortcuts", groups, &open, overlay.ShortcutSheetOptions{Close: "t:shut"})
				}
			}, spot: above("t:shortcuts")},
			{name: "overlay.WhatsNew", covers: true, view: func() func(*ui.Context) {
				page, open := 0, true
				features := []overlay.Feature{{Icon: "sparkles", Title: "t:first", Text: "t:text"}}
				return func(c *ui.Context) {
					overlay.WhatsNew(c, "t:news", features, &page, &open, overlay.WhatsNewOptions{Back: "t:back", Next: "t:forth", Done: "t:shut"})
				}
			}, spot: above("t:news")},
			{name: "overlay.HoverCard", covers: true, view: func() func(*ui.Context) {
				return func(c *ui.Context) {
					anchor := ui.Button(c, "t:anchor")
					overlay.HoverCard(c, "t:card", anchor, func() { ui.Text(c, "t:inside") })
				}
			}, show: func(tt *ui.Tester) error {
				at, ok := tt.Find("t:anchor")
				if !ok {
					return fmt.Errorf("no anchor")
				}
				tt.Move(at.X+at.W/2, at.Y+at.H/2)
				return nil
			}, spot: above("t:inside")},
			// A scrim, not a panel: an eighth of the page shows through it.
			{name: "overlay.Lightbox", view: func() func(*ui.Context) {
				at, open := 0, true
				return func(c *ui.Context) {
					overlay.Lightbox(c, "t:images", []*ui.Bitmap{photo}, []string{"t:first"}, &at, &open, overlay.LightboxOptions{Back: "t:back", Forth: "t:forth", Close: "t:shut"})
				}
			}, spot: above("t:first")},
			{name: "color.Picker's popover", covers: true, view: func() func(*ui.Context) {
				v := swatch[0]
				return func(c *ui.Context) { color.Picker(c, "t:accent", &v) }
			}, show: func(tt *ui.Tester) error { return tt.Click("t:accent") }, spot: func(tt *ui.Tester) (float32, float32, bool) {
				// MyGo's popover hangs 4 DIP under its anchor, from its start.
				a, ok := tt.Access("t:accent")
				return a.Bounds.X + 3, a.Bounds.Y + a.Bounds.H + 7, ok
			}},
			{name: "desktop.UpdatePrompt in kit.Dialog", covers: true, view: hosted(func(c *ui.Context) {
				desktop.UpdatePrompt(c, "Sailry", "1.2.0", []string{"t:note"}, updateCopy)
			}), spot: before("t:v1.2.0")},
			{name: "crychic.UpdatePrompt in kit.Dialog", view: hosted(func(c *ui.Context) {
				UpdatePrompt(c, "Sailry", "1.2.0", []string{"t:note"}, updateCopy)
			}), spot: before("t:v1.2.0")},
			{name: "form.TypeToConfirm in kit.Dialog", view: func() func(*ui.Context) {
				open, typed := true, ""
				return func(c *ui.Context) {
					kit.Dialog(c, &open, func() {
						form.TypeToConfirm(c, "sailry-go", &typed, form.TypeToConfirmOptions{Prompt: "t:type %s", Action: "t:remove"})
					})
				}
			}, spot: func(tt *ui.Tester) (float32, float32, bool) {
				// Between the prompt and the field, inside the component.
				r, ok := tt.Find("t:type sailry-go")
				return r.X + 2, r.Y + r.H + 6, ok
			}},
		}
		for _, k := range cases {
			// The same frame over a red page and a blue one: a surface that
			// hides the page gives the same pixel twice, frost does not.
			var seen [2][3]int
			for i, page := range []ui.Color{ui.RGB(0xff, 0, 0), ui.RGB(0, 0, 0xff)} {
				view := k.view()
				tt := ui.NewTesterWith(func(c *ui.Context) {
					theme.Apply(c, "", theme.Light)
					ui.Column(c).Fill().Padding(24).Gap(12).Background(page).Children(func() { view(c) })
				}, 640, 520, ui.TesterOptions{})
				tt.SetLocale("en")
				tt.Frame()
				if k.show != nil {
					if err := k.show(tt); err != nil {
						t.Fatalf("%s: %v", k.name, err)
					}
				}
				tt.Advance(time.Second)
				tt.Frame()
				x, y, ok := k.spot(tt)
				if !ok {
					t.Fatalf("%s: nothing to sample by: %q", k.name, tt.Texts())
				}
				px := tt.Image().RGBAAt(int(x), int(y))
				seen[i] = [3]int{int(px.R), int(px.G), int(px.B)}
				if i == 1 {
					(&stage{tt: tt}).shot(t, "surface-"+strings.NewReplacer(" ", "-", "'", "").Replace(k.name))
				}
			}
			apart := 0
			for ch := range 3 {
				apart += max(seen[0][ch]-seen[1][ch], seen[1][ch]-seen[0][ch])
			}
			if covers := apart <= 6; covers != k.covers {
				t.Errorf("%s: covers the page %v, documented %v (over red %v, over blue %v)", k.name, covers, k.covers, seen[0], seen[1])
			}
		}
		// FileCard's own menu is MyGo's menu button, which Sailry's windows
		// show as the system's menu.
		var action files.FileAction
		s := newStage(func(c *ui.Context) {
			files.FileCard(c, files.FileItem{Name: "report.pdf", Size: 48213, Modified: day}, &action, files.FileCardOptions{
				Menu: "t:menu", Open: "t:open", Rename: "t:rename", Delete: "t:remove", FormatDate: func(time.Time) string { return "t:date" }})
		}, theme.Light)
		s.settle()
		if err := s.tt.Click("t:menu"); err != nil {
			t.Fatal(err)
		}
		if got := s.tt.Menu(); !slices.Equal(got, []string{"t:open", "t:rename", "t:remove"}) {
			t.Errorf("FileCard's menu shows %q", got)
		}
	})
}
