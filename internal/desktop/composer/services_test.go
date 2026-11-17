package composer

import (
	"strconv"
	"testing"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

func TestSplitComposerServiceMenusStayIndependent(t *testing.T) {
	left, right := New(""), New("")
	tester := ui.NewTester(func(c *ui.Context) {
		theme.Apply(c, "", theme.Light)
		ui.Row(c).Fill().Padding(20).Gap(24).Children(func() {
			left.services(c.Key("left"), []Command{{Command: "left service", Ports: []int{5101}}}, true, nil)
			right.services(c.Key("right"), []Command{{Command: "right service", Ports: []int{5102}}}, true, nil)
		})
	}, 600, 300)
	tester.SetLocale("en")
	tester.Frame()
	for index, port := range []int{5101, 5102} {
		tester.Key(0, ui.KeyEscape)
		tester.Advance(time.Second)
		name := i18n.In("en", "port_mappings") + ": " + strconv.Itoa(port)
		trigger, ok := tester.AccessByRole(ui.RoleMenuButton, name)
		if !ok {
			t.Fatalf("service menu %q is missing", name)
		}
		tester.ClickAt(trigger.Bounds.X+trigger.Bounds.W/2, trigger.Bounds.Y+trigger.Bounds.H/2)
		tester.Advance(time.Second)
		if tester.HasText("left service") != (index == 0) || tester.HasText("right service") != (index == 1) {
			t.Fatalf("one composer's menu changed the other's open state: %v", tester.Texts())
		}
	}
}
