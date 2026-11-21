package composer

import (
	"strconv"
	"strings"
	"testing"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

// serviceOwner records the services a composer opens.
type serviceOwner struct {
	Actions
	opened []int
}

func (o *serviceOwner) OpenService(port int) { o.opened = append(o.opened, port) }

// longCommand is a command far wider than any capsule it could show in.
var longCommand = "npx vite --host 0.0.0.0 --strictPort --config " + strings.Repeat("configs/very/deep/", 12) + "vite.config.ts"

func serviceChipName(port int) string {
	return i18n.In("en", "port_mappings") + ": " + strconv.Itoa(port)
}

// serviceChips are the port capsules the window shows, in its order.
func serviceChips(tester *ui.Tester) []ui.AccessInfo {
	var chips []ui.AccessInfo
	for _, e := range tester.AccessAll() {
		if e.Role == ui.RoleButton && strings.HasPrefix(e.Name, i18n.In("en", "port_mappings")+": ") {
			chips = append(chips, e)
		}
	}
	return chips
}

func pressAt(tester *ui.Tester, e ui.AccessInfo) {
	tester.ClickAt(e.Bounds.X+e.Bounds.W/2, e.Bounds.Y+e.Bounds.H/2)
	tester.Advance(time.Second)
	tester.Frame()
}

func pressService(t *testing.T, tester *ui.Tester, port int) ui.AccessInfo {
	t.Helper()
	e, ok := tester.AccessByRole(ui.RoleButton, serviceChipName(port))
	if !ok {
		t.Fatalf("no capsule for %d: %q", port, tester.Texts())
	}
	pressAt(tester, e)
	return e
}

// A port is one capsule however many commands serve it; one press opens
// its service at once, showing no command, which only assistive technology
// reads; a busy or offline port opens nothing.
func TestComposerServicesOpenOnOnePress(t *testing.T) {
	s := New("")
	owner := &serviceOwner{}
	connected, busy := true, map[int]bool{}
	s.ActivityCallbacks = &ActivityCallbacks{ServiceBusy: func(port int) bool { return busy[port] }}
	running := []Command{
		{ID: "a", Command: longCommand, Ports: []int{5173, 5174}},
		{ID: "b", Command: "go run ./cmd/api", Ports: []int{5173}},
	}
	tester := ui.NewTester(func(c *ui.Context) {
		theme.Apply(c, "", theme.Light)
		ui.Row(c).Fill().Padding(20).Gap(8).AlignItems(ui.Start).Children(func() {
			s.services(c, running, connected, owner)
		})
	}, 600, 200)
	tester.SetLocale("en")
	tester.Frame()

	if chips := serviceChips(tester); len(chips) != 2 {
		t.Fatalf("two ports show %d capsules: %q", len(chips), tester.Texts())
	}
	if tester.HasText(longCommand) || tester.HasText("go run ./cmd/api") {
		t.Fatalf("a port capsule shows its command: %q", tester.Texts())
	}
	chip := pressService(t, tester, 5174)
	if chip.Bounds.H != 28 {
		t.Errorf("a port capsule is %v high, not one strip line", chip.Bounds.H)
	}
	if chip.Description != longCommand {
		t.Errorf("the capsule describes %q, not its command", chip.Description)
	}
	if len(owner.opened) != 1 || owner.opened[0] != 5174 {
		t.Fatalf("one press opened %v, not 5174", owner.opened)
	}
	if tester.HasText(longCommand) {
		t.Errorf("opening a service expands its command: %q", tester.Texts())
	}

	busy[5173] = true
	tester.Frame()
	pressService(t, tester, 5173)
	connected, busy = false, map[int]bool{}
	tester.Frame()
	pressService(t, tester, 5174)
	if len(owner.opened) != 1 {
		t.Errorf("a busy or offline port opened a service: %v", owner.opened)
	}
}

// within reports whether b lies wholly inside a, to the half DIP.
func within(a, b ui.Rect) bool {
	return b.W > 0 && b.H > 0 && b.X >= a.X-0.5 && b.Y >= a.Y-0.5 && b.X+b.W <= a.X+a.W+0.5 && b.Y+b.H <= a.Y+a.H+0.5
}

// shownPorts are the capsules wholly inside the ports' column as it shows,
// top to bottom; a capsule cut by its edge is an error.
func shownPorts(t *testing.T, tester *ui.Tester, area ui.Rect) []ui.AccessInfo {
	t.Helper()
	var shown []ui.AccessInfo
	for _, chip := range serviceChips(tester) {
		b := chip.Bounds
		if b.W == 0 || b.H == 0 || b.Y+b.H <= area.Y || b.Y >= area.Y+area.H {
			continue
		}
		if !within(area, b) || b.H != 28 {
			t.Errorf("%q shows cut: %v in the column %v", chip.Name, b, area)
			continue
		}
		shown = append(shown, chip)
	}
	return shown
}

// Many ports are a one-line capsule each, one under another, in a column
// serviceRows capsules high that scrolls; no group stands between a port
// and its press, and a port scrolled to opens with one press.
func TestComposerServicesManyPortsOneRowEach(t *testing.T) {
	s := New("")
	owner := &serviceOwner{}
	var ports []int
	for port := 5101; port <= 5108; port++ {
		ports = append(ports, port)
	}
	running := []Command{{ID: "a", Command: longCommand, Ports: ports}, {ID: "b", Command: "duplicate", Ports: []int{5101, 5102}}}
	tester := ui.NewTester(func(c *ui.Context) {
		theme.Apply(c, "", theme.Light)
		ui.Row(c).Fill().Padding(20).Gap(8).AlignItems(ui.Start).Children(func() {
			s.services(c, running, true, owner)
		})
	}, 600, 400)
	tester.SetLocale("en")
	tester.Frame()

	column, ok := tester.AccessByRole(ui.RoleScroll, i18n.In("en", "port_mappings"))
	if !ok {
		t.Fatalf("the ports have no column: %q", tester.Texts())
	}
	area := column.Bounds
	if area.H != servicesHeight {
		t.Fatalf("the ports' column is %v high, not %d", area.H, servicesHeight)
	}
	if area.W > 200 {
		t.Errorf("the ports' column is %v wide for a port each", area.W)
	}
	named := map[string]bool{}
	for _, chip := range serviceChips(tester) {
		if named[chip.Name] {
			t.Errorf("%q is listed twice", chip.Name)
		}
		named[chip.Name] = true
		if chip.Name == serviceChipName(5101) && chip.Description != longCommand {
			t.Errorf("5101 is described by %q, not the first command serving it", chip.Description)
		}
	}
	shown := shownPorts(t, tester, area)
	if len(shown) != serviceRows {
		t.Fatalf("the column shows %d ports, not %d", len(shown), serviceRows)
	}
	for i, chip := range shown {
		if chip.Name != serviceChipName(ports[i]) {
			t.Errorf("row %d is %q, not %d", i, chip.Name, ports[i])
		}
		if chip.Bounds.X != shown[0].Bounds.X || chip.Bounds.Y != shown[0].Bounds.Y+float32(i*(28+serviceGap)) {
			t.Errorf("%q is at %v, not under the one before: the first at %v", chip.Name, chip.Bounds, shown[0].Bounds)
		}
	}
	if tester.HasText(longCommand) || tester.HasText(i18n.In("en", "port_count", "count", len(ports))) {
		t.Errorf("the ports show a command or a group: %q", tester.Texts())
	}

	pressAt(tester, shown[1])
	if len(owner.opened) != 1 || owner.opened[0] != ports[1] {
		t.Fatalf("one press opened %v, not %d", owner.opened, ports[1])
	}

	tester.Scroll(area.X+area.W/2, area.Y+area.H/2, 0, 10_000)
	tester.Advance(time.Second)
	tester.Frame()
	shown = shownPorts(t, tester, area)
	if len(shown) != serviceRows || shown[len(shown)-1].Name != serviceChipName(ports[len(ports)-1]) {
		t.Fatalf("scrolled to the end, the column shows %v", shown)
	}
	last := shown[len(shown)-1]
	pressAt(tester, last)
	if len(owner.opened) != 2 || owner.opened[1] != ports[len(ports)-1] {
		t.Fatalf("a press on the last port opened %v", owner.opened)
	}
}

// Split composers listing the same ports keep them apart: the strips
// build side by side, and a press on one's port opens only its service.
func TestSplitComposerSamePortsStayApart(t *testing.T) {
	left, right := New(""), New("")
	leftOwner, rightOwner := &serviceOwner{}, &serviceOwner{}
	running := []Command{{Command: "npm run dev", Ports: []int{5173, 5174}}}
	tester := ui.NewTester(func(c *ui.Context) {
		theme.Apply(c, "", theme.Light)
		ui.Row(c).Fill().Padding(20).Gap(24).AlignItems(ui.Start).Children(func() {
			left.services(c.Key("left"), running, true, leftOwner)
			right.services(c.Key("right"), running, true, rightOwner)
		})
	}, 900, 200)
	tester.SetLocale("en")
	tester.Frame()

	var same []ui.AccessInfo
	for _, chip := range serviceChips(tester) {
		if chip.Name == serviceChipName(5173) {
			same = append(same, chip)
		}
	}
	if len(same) != 2 {
		t.Fatalf("the split shows %d capsules for 5173", len(same))
	}
	pressAt(tester, same[1])
	if len(leftOwner.opened) != 0 || len(rightOwner.opened) != 1 || rightOwner.opened[0] != 5173 {
		t.Fatalf("a press on the right's port opened left %v, right %v", leftOwner.opened, rightOwner.opened)
	}
	pressAt(tester, same[0])
	if len(leftOwner.opened) != 1 || len(rightOwner.opened) != 1 {
		t.Errorf("a press on the left's port opened left %v, right %v", leftOwner.opened, rightOwner.opened)
	}
}
