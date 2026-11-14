package host

import (
	"math"
	"slices"
	"strings"
	"testing"
	"time"

	"github.com/ELY-Open-Source/CrychicUI/feedback"
	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

func TestFormats(t *testing.T) {
	if Protocol(3, 3) != "v3" || Protocol(1, 3) != "v1-v3" || Protocol(3, 1) != "" || Protocol(0, 3) != "" {
		t.Error("protocol ranges")
	}
	if got := UptimeParts(3*24*time.Hour + 5*time.Hour + 12*time.Minute); len(got) != 2 || got[0][0] != "host_uptime_days" || got[1][0] != "host_uptime_hours" {
		t.Errorf("uptime keeps two units: %v", got)
	}
	if got := UptimeParts(30 * time.Second); len(got) != 1 || got[0][0] != "host_uptime_minutes" || got[0][1] != int64(0) {
		t.Errorf("uptime under a minute: %v", got)
	}
	if UptimeParts(-1) != nil {
		t.Error("unknown uptime")
	}
	if Bytes(1024) != "1.0 KiB" || Bytes(512) != "512 B" || Bytes(-1) != "" {
		t.Error("bytes")
	}
	if Rate(2048, 2*time.Second) != "1.0 KiB" || Rate(1, 0) != "" {
		t.Error("rates")
	}
	if Percent(250) != "250.0%" || Percent(-1) != "" {
		t.Error("percent")
	}
	if _, ok := Usage(10, 0); ok {
		t.Error("usage of an unknown total")
	}
	if v, _ := Usage(20, 10); v != 100 {
		t.Error("usage clamps")
	}
	if number, unit := Scaled(8 << 30); number != "8.0" || unit != "GiB" {
		t.Errorf("a figure's number and unit: %q %q", number, unit)
	}
	if Load(1.254) != "1.25" || Clock(time.Date(2026, 11, 12, 9, 5, 7, 0, time.UTC)) != "09:05:07" {
		t.Error("load averages and the charts' times")
	}
	if used, percent := (Disk{Total: 200, Available: 50}).Used(); used != 150 || percent != 75 {
		t.Errorf("a volume's use: %d %v", used, percent)
	}
	// A live host's names are its own; the preview's are i18n keys.
	var live, local, preview, process string
	tt := ui.NewTester(func(c *ui.Context) {
		live = (&Host{Live: true, Name: "git"}).label(c, "git")
		local = (&Host{Live: true}).label(c, "local_host_name")
		preview = (&Host{}).label(c, "remote_host_name")
		process = (&Host{}).label(c, "metrics_worker")
	}, 100, 100)
	tt.SetLocale("en")
	tt.Frame()
	if live != "git" || local != i18n.In("en", "local_host_name") || preview != i18n.In("en", "remote_host_name") || process != i18n.In("en", "metrics_worker") {
		t.Errorf("labels: live %q, local %q, preview %q, process %q", live, local, preview, process)
	}
}

// Each phase offers its action; a stale click does nothing.
func TestUpdatePhases(t *testing.T) {
	for _, ph := range []Phase{Checking, Downloading, Verifying, Applying, RestartWait} {
		if _, _, ok := (&Update{Phase: ph}).Primary(); ok {
			t.Errorf("%s has an action", ph)
		}
	}
	if _, _, ok := (&Update{Phase: Idle, Note: "n"}).Primary(); ok {
		t.Error("an unavailable update has an action")
	}
	u := &Update{Phase: Idle, Current: "0.1.0", Progress: -1}
	stale := *u
	u.Advance(*u)
	if u.Phase != Available || u.Target == "" {
		t.Fatalf("check: %+v", u)
	}
	u.Advance(stale)
	if u.Phase != Available {
		t.Error("a stale click advanced")
	}
	u.Advance(*u)
	if u.Phase != Staged || u.Progress != 100 {
		t.Errorf("download: %+v", u)
	}
	u.Advance(*u)
	if u.Phase != RestartWait {
		t.Errorf("apply: %+v", u)
	}
	f := &Update{Phase: Downloading, Error: "e", Retry: Status}
	if a, label, ok := f.Primary(); !ok || a != Status || label != "update_retry" {
		t.Error("a failed request retries its own action")
	}
}

func TestAddPort(t *testing.T) {
	ports := []Port{{Remote: 3000, Local: 13000, Listening: true}}
	for _, bad := range [][2]string{{"", ""}, {"0", ""}, {"70000", ""}, {"80", "-1"}, {"80", "x"}} {
		if _, p := AddPort(ports, bad[0], bad[1]); p != "port_invalid" {
			t.Errorf("%v: %q", bad, p)
		}
	}
	if _, p := AddPort(ports, "3000", ""); p != "port_exists" {
		t.Error("a repeated port")
	}
	if x, p := AddPort(ports, "8080", "0"); p != "" || x.Local == 0 || !x.Listening {
		t.Errorf("auto port %+v %q", x, p)
	}
	if x, _ := AddPort(ports, "8080", "9000"); x.Local != 9000 {
		t.Error("the local port")
	}
}

// The monitor's charts read the sample under the pointer: a tip names its
// time and each series' value, follows the pointer to another sample, shows
// a new value of the same sample in place, and goes when the pointer
// leaves. A value the sample did not have reads as a dash.
func TestChartTip(t *testing.T) {
	start := time.Date(2026, 11, 12, 9, 0, 0, 0, time.UTC)
	history := make([]Point, HistoryLimit)
	down, up := make([]float64, HistoryLimit), make([]float64, HistoryLimit)
	for i := range history {
		history[i] = Point{At: start.Add(time.Duration(i) * time.Second)}
		down[i], up[i] = float64(i)*1024, float64(i)*512
	}
	up[30] = math.NaN()
	// The plot has no axes, so its area is the chart's box: 590 wide from
	// 100, a sample every 10.
	tt := ui.NewTesterWith(func(c *ui.Context) {
		theme.Apply(c, "", theme.Light)
		p := theme.Of(c)
		ui.Column(c).Fill().Padding(100).AlignItems(ui.Start).Children(func() {
			ui.Box(c).Width(590).Children(func() {
				areaChart(c, "chart", 100, 64*1024, nil, nil, "Network", rates(c, history),
					series{down, p.Chart[1], "Download"}, series{up, p.Chart[3], "Upload"})
			})
		})
	}, 1000, 400, ui.TesterOptions{Preferences: ui.Preferences{ReduceTransparency: true}})
	tt.SetLocale("en")
	tt.Frame()
	tt.Frame()
	rate := func(kib string) string { return i18n.In("en", "metrics_rate", "value", kib+" KiB") }
	shows := func(want ...string) bool {
		for _, text := range want {
			if !tt.HasText(text) {
				return false
			}
		}
		return true
	}
	if tt.HasText("Download") || tt.HasText("09:00:30") {
		t.Fatalf("a chart shows a tip without the pointer: %q", tt.Texts())
	}
	hover := func(x, y float32) {
		tt.Move(x, y)
		tt.Frame()
		tt.Frame()
	}
	hover(100+10*30+2, 150)
	if !shows("09:00:30", "Download", rate("30.0"), "Upload", dash) {
		t.Fatalf("the tip at the thirty-first sample: %q", tt.Texts())
	}
	hover(100+10*45-2, 120)
	if !shows("09:00:45", rate("45.0"), rate("22.5")) || tt.HasText("09:00:30") || tt.HasText(dash) {
		t.Fatalf("the tip at the forty-sixth sample: %q", tt.Texts())
	}
	// The sample's value changes under a resting pointer: the tip reads it
	// where it stands.
	before, _ := tt.Find("09:00:45")
	down[45] = 7 * 1024
	tt.Frame()
	after, _ := tt.Find("09:00:45")
	if !shows(rate("7.0")) || tt.HasText(rate("45.0")) || before != after {
		t.Fatalf("the tip after a new value: at %v then %v: %q", before, after, tt.Texts())
	}
	hover(50, 350)
	tt.Advance(time.Second)
	tt.Frame()
	if tt.HasText("09:00:45") || tt.HasText("Download") {
		t.Fatalf("the tip stays after the pointer left: %q", tt.Texts())
	}
}

// The header's status follows the Node's connection and what is known of
// it.
func TestConnection(t *testing.T) {
	for _, x := range []struct {
		name string
		host Host
		want feedback.Connection
	}{
		{"connected", Host{Runtime: Runtime{Known: true, Version: "0.1.0"}}, feedback.Online},
		{"before its facts", Host{}, feedback.Reconnecting},
		{"offline", Host{Offline: true, Runtime: Runtime{Known: true}}, feedback.Offline},
		{"a failed read", Host{Runtime: Runtime{Known: true, Error: "closed"}}, feedback.Offline},
	} {
		if got := x.host.Connection(); got != x.want {
			t.Errorf("%s: %v, want %v", x.name, got, x.want)
		}
	}
}

// The history follows the samples: a minute of them at most, started again
// after a pause, kept as it is by a sample that is not newer, and each
// process's by its PID and start.
func TestFollow(t *testing.T) {
	start := time.Unix(1_800_000_000, 0)
	sample := func(second int, cpu float64) *Metrics {
		return &Metrics{
			At: start.Add(time.Duration(second) * time.Second), CPU: cpu, MemoryUsed: 1, Memory: 4,
			Received: 2048, Transmitted: 1024, Interval: 2 * time.Second,
			Disks:     []Disk{{Mount: "/", Total: 10, ReadBytes: 7, Written: 9}},
			Processes: []Process{{Name: "node", PID: 41, Started: 5, CPU: cpu}, {Name: "node", PID: 41, Started: 6, CPU: 1}},
		}
	}
	first := sample(0, 10)
	first.Follow(nil)
	if len(first.History) != 1 {
		t.Fatalf("the first sample's history: %v", first.History)
	}
	if p := first.History[0]; p.CPU != 10 || p.Memory != 25 || p.Received != 1024 || p.Transmitted != 512 || p.Disks["/"] != [2]float64{7, 9} {
		t.Errorf("the first point: %+v", p)
	}
	second := sample(1, 20)
	second.Follow(first)
	if len(second.History) != 2 || second.History[0].CPU != 10 || second.History[1].CPU != 20 {
		t.Errorf("the next sample's history: %+v", second.History)
	}
	if got := second.Processes[0].History; !slices.Equal(got, []float64{10, 20}) {
		t.Errorf("a process's history: %v", got)
	}
	if got := second.Processes[1].History; !slices.Equal(got, []float64{1, 1}) {
		t.Errorf("a reused PID's history: %v", got)
	}
	if len(first.History) != 1 {
		t.Error("following a sample changed its history")
	}
	// A sample that is not newer keeps the history.
	stale := sample(1, 99)
	stale.Follow(second)
	if len(stale.History) != 2 || stale.History[1].CPU != 20 || !slices.Equal(stale.Processes[0].History, []float64{10, 20}) {
		t.Errorf("a repeated sample: %+v %v", stale.History, stale.Processes[0].History)
	}
	// A pause starts it again.
	late := sample(30, 30)
	late.Follow(second)
	if len(late.History) != 1 || late.History[0].CPU != 30 || !slices.Equal(late.Processes[0].History, []float64{30}) {
		t.Errorf("after a pause: %+v %v", late.History, late.Processes[0].History)
	}
	// A minute at most.
	last := first
	for i := 1; i < HistoryLimit+10; i++ {
		next := sample(i, float64(i))
		next.Follow(last)
		last = next
	}
	if len(last.History) != HistoryLimit || last.History[HistoryLimit-1].CPU != float64(HistoryLimit+9) || len(last.Processes[0].History) != HistoryLimit {
		t.Errorf("the history holds %d points and %d loads", len(last.History), len(last.Processes[0].History))
	}
	// A value the sample does not have breaks the line.
	bare := &Metrics{At: start, CPU: math.NaN()}
	bare.Follow(nil)
	if p := bare.History[0]; !math.IsNaN(p.CPU) || !math.IsNaN(p.Memory) || !math.IsNaN(p.Received) || !math.IsNaN(p.Transmitted) {
		t.Errorf("a sample without values: %+v", p)
	}
}

// The disk card follows the chosen volume, else the root, else the first,
// and the GPU card the chosen accelerator, else the first. The Node lists
// only real volumes, so every disk of a sample is one.
func TestChoices(t *testing.T) {
	m := &Metrics{Disks: []Disk{
		{Mount: "/Volumes/Web", FileSystem: "apfs", Total: 1 << 40},
		{Mount: "/", FileSystem: "APFS", Total: 1 << 40},
		{Mount: "/boot", FileSystem: "ext4", Total: 1 << 30},
	}}
	for chosen, want := range map[string]string{"": "/", "/boot": "/boot", "/gone": "/"} {
		if d, ok := m.Volume(chosen); !ok || d.Mount != want {
			t.Errorf("chosen %q: %q %v, want %q", chosen, d.Mount, ok, want)
		}
	}
	if d, ok := (&Metrics{Disks: []Disk{{Mount: "/data", Total: 1}, {Mount: "/more", Total: 1}}}).Volume(""); !ok || d.Mount != "/data" {
		t.Errorf("without a root: %q %v", d.Mount, ok)
	}
	if _, ok := (&Metrics{}).Volume(""); ok {
		t.Error("a host without a volume has one")
	}

	first := &Metrics{At: time.Unix(100, 0), GPUs: []GPU{{ID: "a", Name: "Integrated", Usage: 12.5}, {ID: "b", Name: "Discrete", Usage: math.NaN()}}}
	first.Follow(nil)
	for chosen, want := range map[string]string{"": "a", "b": "b", "gone": "a"} {
		if g, ok := first.GPU(chosen); !ok || g.ID != want {
			t.Errorf("chosen accelerator %q: %q %v, want %q", chosen, g.ID, ok, want)
		}
	}
	if _, ok := (&Metrics{}).GPU(""); ok {
		t.Error("a host without an accelerator has one")
	}
	// The history keeps each accelerator's load, and none for one unknown.
	next := &Metrics{At: time.Unix(101, 0), GPUs: []GPU{{ID: "a", Usage: 50}, {ID: "b", Usage: 75}}}
	next.Follow(first)
	if got := gpuHistory(next, "a"); len(got) != 2 || got[0] != 12.5 || got[1] != 50 {
		t.Errorf("the first accelerator's history: %v", got)
	}
	if got := gpuHistory(next, "b"); len(got) != 2 || !math.IsNaN(got[0]) || got[1] != 75 {
		t.Errorf("the second accelerator's history: %v", got)
	}
}

// The processes sort by the chosen column and keep the search's matches by
// name or PID; an unknown load sorts below every known one.
func TestSortProcesses(t *testing.T) {
	rows := []Process{
		{Name: "node", PID: 41, CPU: 3, Memory: 30},
		{Name: "Zed", PID: 7, CPU: math.NaN(), Memory: 90},
		{Name: "gopls", PID: 410, CPU: 12, Memory: 60},
	}
	names := func(query, column string, descending bool) []string {
		var out []string
		for _, x := range SortProcesses(rows, query, column, descending) {
			out = append(out, x.Name)
		}
		return out
	}
	for _, x := range []struct {
		query, column string
		descending    bool
		want          []string
	}{
		{"", "cpu", true, []string{"gopls", "node", "Zed"}},
		{"", "cpu", false, []string{"Zed", "node", "gopls"}},
		{"", "memory", true, []string{"Zed", "gopls", "node"}},
		{"", "name", false, []string{"gopls", "node", "Zed"}},
		{"", "pid", false, []string{"Zed", "node", "gopls"}},
		{" ZE ", "cpu", true, []string{"Zed"}},
		{"41", "pid", false, []string{"node", "gopls"}},
		{"none", "cpu", true, nil},
	} {
		if got := names(x.query, x.column, x.descending); !slices.Equal(got, x.want) {
			t.Errorf("%q by %s descending %v: %v, want %v", x.query, x.column, x.descending, got, x.want)
		}
	}
	if rows[0].Name != "node" {
		t.Error("sorting changed the sample's order")
	}
}

// follows reports whether want shows in texts in its order, each after the
// one before.
func follows(texts, want []string) (string, bool) {
	at := 0
	for _, w := range want {
		i := slices.Index(texts[at:], w)
		if i < 0 {
			return w, false
		}
		at += i + 1
	}
	return "", true
}

// A live host's page is the reference's: the header once, with the
// computer's and Sailry's uptimes apart and the platform on one line; then
// the processor, the memory, the utilization, the network, the storage and
// the processes; then the running agents and the projects, with no runtime
// section: the header's line names the host's and the protocol's versions.
// Every volume the Node lists shows, and no preview note. The accelerator
// has its card where the sample has one. A process's right-click menu is the
// system's and asks before it ends one. An offline host keeps what is known
// under a warning.
func TestPage(t *testing.T) {
	const platform = "darwin · 27.2 · arm64"
	m := &Metrics{
		At: time.Unix(1_800_000_000, 0), CPU: 12.5, Cores: []float64{10, 20}, Load: &[3]float64{1, 0.5, 0.25},
		MemoryUsed: 8 << 30, MemoryCached: 4 << 30, Memory: 32 << 30,
		Received: 1024, Transmitted: 512, Interval: time.Second, Totals: true,
		Disks: []Disk{
			{Mount: "/", FileSystem: "apfs", Total: 1 << 40, Available: 1 << 39},
			{Mount: "/Volumes/Web", FileSystem: "apfs", Total: 1 << 40, Available: 1 << 38},
		},
		GPUs: []GPU{{ID: "7", Name: "Apple M4 Pro", Usage: 37.5}},
		Processes: []Process{
			{Name: "sailry-node", PID: 41, Started: 9, CPU: 3, Memory: 1 << 20},
			{Name: "launchd", PID: 1, Started: 2, CPU: 1, Memory: 1 << 20},
		}, ProcessCount: 2,
	}
	m.Follow(nil)
	h := &Host{
		Live: true, ID: "local", Name: "studio", Local: true, OS: "macos", Detail: platform,
		Activity: []Activity{{Session: "s", Title: "Rebuild the host page", Location: "sailry-harness / main", Status: "turn_thinking"}},
		Runtime: Runtime{
			Version: "0.1.0",
			Uptime:  6*24*time.Hour + 9*time.Hour, NodeUptime: 5 * time.Minute, ProtocolMin: 1, ProtocolMax: 1, Known: true,
		},
		Metrics: m,
	}
	st := &State{}
	type stopped struct {
		process Process
		force   bool
	}
	var stops []stopped
	act := Actions{StopProcess: func(x Process, force bool) { stops = append(stops, stopped{x, force}) }}
	// Reduce Transparency keeps the shader interpreter out of the headless
	// frames; the window is tall enough for the whole page.
	opaque := ui.TesterOptions{Preferences: ui.Preferences{ReduceTransparency: true}}
	tt := ui.NewTesterWith(func(c *ui.Context) {
		theme.Apply(c, "", theme.Light)
		ui.Column(c).Fill().Children(func() { Page(c, st, h, act) })
	}, 1200, 2400, opaque)
	tt.SetLocale("en")
	tt.Frame()
	tt.Frame()
	en := func(key string, args ...any) string { return i18n.In("en", key, args...) }
	// The header's line: the platform, then the two versions after their labels.
	line := platform + " · " + en("host_version") + " 0.1.0 · " + en("host_protocol") + " v1"
	count := func() map[string]int {
		out := map[string]int{}
		for _, text := range tt.Texts() {
			out[text]++
		}
		return out
	}

	t.Run("sections in order", func(t *testing.T) {
		order := []string{
			"studio", en("live_connected"), line, en("host_uptime"), en("host_node_uptime"),
			en("metrics_cpu"), en("metrics_gpu"), "Apple M4 Pro", en("metrics_memory"), en("host_utilization"), en("metrics_network"), en("metrics_disks"),
			en("host_storage"), en("host_process_search"), en("host_pid"),
			en("workspace_running"), "Rebuild the host page", "sailry-harness / main",
		}
		if missing, ok := follows(tt.Texts(), order); !ok {
			t.Errorf("no %q in its place: %q", missing, tt.Texts())
		}
	})
	t.Run("header once", func(t *testing.T) {
		shown := count()
		// The platform and the versions are the header's one line.
		if shown[line] != 1 || shown[platform] != 0 {
			t.Errorf("the header's line shows %d times, the platform alone %d: %q", shown[line], shown[platform], tt.Texts())
		}
		// The computer's uptime and Sailry's are two facts.
		days := en("host_uptime_days", "count", 6) + " " + en("host_uptime_hours", "count", 9)
		if shown[days] != 1 || shown[en("host_uptime_minutes", "count", 5)] != 1 {
			t.Errorf("uptimes: %q", tt.Texts())
		}
		for _, key := range []string{"metrics_preview", "workspace_host_status", "host_activity_hint"} {
			if tt.HasText(en(key)) {
				t.Errorf("a live host shows the preview's %s", key)
			}
		}
		if _, ok := tt.AccessByRole(ui.RoleButton, en("live_revoke")); ok {
			t.Error("the page has a Remove pairing button")
		}
	})
	t.Run("no runtime section", func(t *testing.T) {
		// The section's title and its grid's labels (AGENTS.md, Removed UI): their
		// copy is out of the catalog, so they are named as they read. The
		// versions' labels show in the header's line alone.
		shown := count()
		for _, text := range []string{
			"Runtime", "Host type", "Worktrees", "Sessions", "Terminal", "System version", "Time zone", "UTC",
			en("host_version"), en("host_protocol"), "0.1.0", "v1", en("host_unknown"),
		} {
			if shown[text] != 0 {
				t.Errorf("the page shows %q of the removed runtime section: %q", text, tt.Texts())
			}
		}
		// A version not known is left out of the line.
		version := h.Runtime.Version
		h.Runtime.Version, h.Runtime.ProtocolMax = "", 0
		tt.Frame()
		if shown := count(); shown[platform] != 1 {
			t.Errorf("the line without versions: %q", tt.Texts())
		}
		h.Runtime.Version, h.Runtime.ProtocolMax = version, 1
		tt.Frame()
	})
	t.Run("no projects section", func(t *testing.T) {
		// The section's title, its rows, its empty state and Add project
		// (AGENTS.md, Removed UI): the page ends with the running sessions. The
		// empty state's copy is out of the catalog, so it is named as it reads.
		absent := func(when string) {
			t.Helper()
			shown := count()
			for _, text := range []string{en("projects"), en("project_add"), "sailry-harness", "No projects on this host"} {
				if shown[text] != 0 {
					t.Errorf("%s: the page shows %q of the removed projects section: %q", when, text, tt.Texts())
				}
			}
			for _, text := range tt.Texts() {
				if strings.Contains(text, "Add a project") {
					t.Errorf("%s: the page shows the removed empty state %q", when, text)
				}
			}
			if _, ok := tt.AccessByRole(ui.RoleButton, en("project_add")); ok {
				t.Errorf("%s: the page has an Add project button", when)
			}
		}
		absent("with running sessions")
		if texts := tt.Texts(); len(texts) == 0 || texts[len(texts)-1] != en("turn_thinking") {
			t.Errorf("the page does not end with the running session's row: %q", texts)
		}
		// A host with no running session has no section after the monitor.
		activity := h.Activity
		h.Activity = nil
		tt.Frame()
		absent("without running sessions")
		if tt.HasText(en("workspace_running")) {
			t.Errorf("a host with no running session shows their section: %q", tt.Texts())
		}
		h.Activity = activity
		tt.Frame()
	})
	t.Run("chart tip", func(t *testing.T) {
		// The utilization's one sample is at the plot's right edge, over its
		// time on the axis: the tip names the time again and each series'
		// value beside the legend's.
		clock := Clock(m.At)
		tick, ok := tt.Find(clock)
		if shown := count(); !ok || shown[clock] != 1 || shown["12.5%"] != 1 || shown["25.0%"] != 1 || shown["37.5%"] != 1 {
			t.Fatalf("the utilization before the pointer: %v %q", ok, tt.Texts())
		}
		tt.Move(tick.X+tick.W/2-4, tick.Y-60)
		tt.Frame()
		tt.Frame()
		if shown := count(); shown[clock] != 2 || shown["12.5%"] != 2 || shown["25.0%"] != 2 || shown["37.5%"] != 2 {
			t.Errorf("the utilization's tip: %q", tt.Texts())
		}
		tt.Move(4, 4)
		tt.Frame()
		tt.Advance(time.Second)
		tt.Frame()
		if shown := count(); shown[clock] != 1 || shown["12.5%"] != 1 {
			t.Errorf("the tip stays after the pointer left: %q", tt.Texts())
		}
	})
	t.Run("volumes", func(t *testing.T) {
		shown := count()
		if shown["/"] == 0 || shown["/Volumes/Web"] == 0 {
			t.Errorf("volumes: / %d, /Volumes/Web %d", shown["/"], shown["/Volumes/Web"])
		}
		if !tt.HasText(en("host_partition_count", "count", 2)) {
			t.Errorf("the storage does not count its volumes: %q", tt.Texts())
		}
	})
	t.Run("accelerator", func(t *testing.T) {
		// The card's figure and the utilization's legend; a host with one
		// accelerator has no choice of it.
		shown := count()
		if shown["37.5"] != 1 || shown["37.5%"] != 1 || shown["Apple M4 Pro"] != 1 {
			t.Errorf("the accelerator's load: %q", tt.Texts())
		}
		if _, ok := tt.AccessByRole(ui.RoleMenuButton, en("metrics_gpu")); ok {
			t.Error("one accelerator has a choice")
		}
		// Several: the card follows the chosen one.
		m.GPUs = append(m.GPUs, GPU{ID: "8", Name: "Radeon Pro", Usage: math.NaN()})
		tt.Frame()
		if _, ok := tt.AccessByRole(ui.RoleMenuButton, en("metrics_gpu")); !ok || !tt.HasText("Apple M4 Pro") {
			t.Errorf("two accelerators have no choice: %q", tt.Texts())
		}
		st.gpu = "8"
		tt.Frame()
		if shown := count(); shown["37.5"] != 0 || !tt.HasText("Radeon Pro") {
			t.Errorf("the chosen accelerator: %q", tt.Texts())
		}
		// A sample without one has no card and no series: the Node reads
		// only macOS's.
		gpus := m.GPUs
		m.GPUs = nil
		tt.Frame()
		if tt.HasText(en("metrics_gpu")) {
			t.Errorf("a host without an accelerator shows one: %q", tt.Texts())
		}
		m.GPUs, st.gpu = gpus[:1], ""
		tt.Frame()
	})
	t.Run("process table look", func(t *testing.T) {
		// The application's table look (kit.TableFrame, kit.TableCell), as a
		// package's tables: rows exactly 36 apart under a filled header, a
		// line between them, and plain cells.
		header, headed := tt.AccessByRole(ui.RoleColumnHeader, en("host_process_name"))
		title := header.Bounds
		first, shown := tt.Find("sailry-node")
		second, also := tt.Find("launchd")
		if !headed || !shown || !also {
			t.Fatalf("the table lists no processes: %q", tt.Texts())
		}
		if pitch := second.Y - first.Y; pitch < 35.5 || pitch > 36.5 {
			t.Errorf("rows are %v apart, want 36", pitch)
		}
		// Past the name only the table's own fills are drawn.
		at := func(y float32) [3]uint8 {
			pixel := tt.Image().RGBAAt(int(first.X+first.W+40), int(y))
			return [3]uint8{pixel.R, pixel.G, pixel.B}
		}
		middle := first.Y + first.H/2
		ground := at(middle)
		if at(title.Y+title.H/2) == ground {
			t.Errorf("the header is not filled: %v; title %v, first %v, second %v, image %v", ground, title, first, second, tt.Image().Bounds())
		}
		separated := false
		for y := middle + 12; y <= middle+24; y++ {
			separated = separated || at(y) != ground
		}
		if !separated {
			t.Error("no separator between the first two rows")
		}
		// No letter tile before a name and no bar of the memory's share
		// (AGENTS.md, Removed UI).
		texts := count()
		if texts["s"] != 0 || texts["l"] != 0 {
			t.Errorf("a process has a letter tile: %q", tt.Texts())
		}
		for _, info := range tt.AccessAll() {
			if info.Role == ui.RoleProgress && (info.Name == "sailry-node" || info.Name == "launchd") {
				t.Errorf("the process %s has a memory bar", info.Name)
			}
		}
		// The loads end their column: each row's ends where the other's does,
		// before the PID's column.
		edge := func(text string, row ui.Rect) float32 {
			r, ok := tt.Find(text)
			if !ok || r.Y+r.H/2 < row.Y || r.Y+r.H/2 > row.Y+row.H {
				t.Fatalf("no %q in its process's row at %v: %v %v", text, row, ok, r)
			}
			return r.X + r.W
		}
		pid, headed := tt.AccessByRole(ui.RoleColumnHeader, en("host_pid"))
		if !headed {
			t.Fatalf("the process table has no PID column: %q", tt.Texts())
		}
		if a, b := edge(Percent(3), first), edge(Percent(1), second); a != b || a <= first.X+first.W || a >= pid.Bounds.X {
			t.Errorf("the loads end at %v and %v, want one edge before the PID's column at %v", a, b, pid.Bounds.X)
		}
	})
	t.Run("process menu", func(t *testing.T) {
		stop, force := en("host_process_stop"), en("host_process_force_stop")
		menu := func(name string) {
			t.Helper()
			r, ok := tt.Find(name)
			if !ok {
				t.Fatalf("no process %q: %q", name, tt.Texts())
			}
			tt.RightClickAt(r.X+2, r.Y+r.H/2)
		}
		menu("sailry-node")
		want := []string{en("host_process_details"), en("host_process_copy_pid"), en("host_process_copy_name"), "-", stop, force}
		if got := tt.Menu(); !slices.Equal(got, want) {
			t.Fatalf("the process menu is %q, want %q", got, want)
		}
		if err := tt.ChooseMenuItem(en("host_process_copy_pid")); err != nil || tt.Clipboard() != "41" {
			t.Errorf("Copy PID: %v, clipboard %q", err, tt.Clipboard())
		}
		// End process asks first, naming the process and what follows once;
		// Cancel sends nothing.
		shows := func(part string) int {
			n := 0
			for _, text := range tt.Texts() {
				if strings.Contains(text, part) {
					n++
				}
			}
			return n
		}
		menu("sailry-node")
		if err := tt.ChooseMenuItem(stop); err != nil {
			t.Fatal(err)
		}
		tt.Frame()
		if shows("sailry-node · PID 41") != 1 || shows(en("host_process_stop_confirm")) != 1 {
			t.Fatalf("the confirmation: %q", tt.Texts())
		}
		if err := tt.Click(en("settings_cancel")); err != nil {
			t.Fatal(err)
		}
		tt.Frame()
		tt.Frame()
		if len(stops) != 0 || shows(en("host_process_stop_confirm")) != 0 {
			t.Fatalf("cancel: %+v %q", stops, tt.Texts())
		}
		// The rows sort again under an open menu: the item acts on the
		// process the menu was opened on.
		menu("sailry-node")
		st.sort = ui.SortOrder{Column: "cpu"}
		tt.Frame()
		if err := tt.ChooseMenuItem(force); err != nil {
			t.Fatal(err)
		}
		tt.Frame()
		if shows("sailry-node · PID 41") != 1 || shows(en("host_process_force_confirm")) != 1 {
			t.Fatalf("the forced stop's confirmation: %q", tt.Texts())
		}
		var confirm ui.Rect
		for _, info := range tt.AccessAll() {
			if info.Role == ui.RoleButton && info.Name == force {
				confirm = info.Bounds
			}
		}
		tt.ClickAt(confirm.X+confirm.W/2, confirm.Y+confirm.H/2)
		tt.Frame()
		tt.Frame()
		if len(stops) != 1 || stops[0].process.PID != 41 || stops[0].process.Started != 9 || !stops[0].force {
			t.Fatalf("the confirmed stop: %+v", stops)
		}
		// The system's first process is not one to end, and no second
		// request is sent while one runs.
		menu("launchd")
		if tt.ChooseMenuItem(stop) == nil {
			t.Error("PID 1 could be ended")
		}
		tt.CloseMenu()
		h.Stopping = true
		tt.Frame()
		menu("sailry-node")
		if tt.ChooseMenuItem(force) == nil {
			t.Error("a second stop could be asked while one runs")
		}
		tt.CloseMenu()
		h.Stopping = false
		// A Windows host has only the forced stop.
		h.OS = "windows"
		tt.Frame()
		menu("sailry-node")
		if got := tt.Menu(); slices.Contains(got, stop) || !slices.Contains(got, force) {
			t.Errorf("a Windows host's process menu is %q", got)
		}
		// The details name the process as it was.
		if err := tt.ChooseMenuItem(en("host_process_details")); err != nil {
			t.Fatal(err)
		}
		tt.Frame()
		if !tt.HasText(en("host_process_details")) || !tt.HasText("41") {
			t.Errorf("the details: %q", tt.Texts())
		}
		tt.Key(0, ui.KeyEscape)
		tt.Frame()
		h.OS = "macos"
		tt.Frame()
		if len(stops) != 1 {
			t.Errorf("stops: %+v", stops)
		}
	})
	t.Run("summary column", func(t *testing.T) {
		panel := ui.NewTesterWith(func(c *ui.Context) {
			theme.Apply(c, "", theme.Light)
			ui.Column(c).Fill().Children(func() { MetricsPanel(c, st, h) })
		}, 560, 1200, opaque)
		panel.SetLocale("en")
		panel.Frame()
		shown := map[string]int{}
		for _, text := range panel.Texts() {
			shown[text]++
		}
		if shown["/"] == 0 || shown["/Volumes/Web"] == 0 || shown["sailry-node"] == 0 {
			t.Errorf("the summary's volumes and processes: %q", panel.Texts())
		}
	})
	t.Run("offline", func(t *testing.T) {
		h.Offline, h.Metrics, h.MetricsError = true, nil, "Host is disconnected"
		tt.Frame()
		tt.Frame()
		if !tt.HasText(en("connections_offline")) || !tt.HasText(en("host_sampling_failed")) {
			t.Errorf("no offline status or warning: %q", tt.Texts())
		}
		if tt.HasText(en("host_utilization")) || tt.HasText("Host is disconnected") {
			t.Errorf("an offline host shows a sample or the error's text in the page: %q", tt.Texts())
		}
		if missing, ok := follows(tt.Texts(), []string{"studio", line, en("workspace_running"), "Rebuild the host page"}); !ok {
			t.Errorf("no %q offline: %q", missing, tt.Texts())
		}
		if tt.HasText(en("projects")) || tt.HasText(en("project_add")) {
			t.Errorf("an offline host shows the removed projects section: %q", tt.Texts())
		}
	})
}
