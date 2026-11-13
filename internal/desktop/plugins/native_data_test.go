package plugins

import (
	"encoding/json"
	"math"
	"testing"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

func TestNativeTable(t *testing.T) {
	for _, mode := range []theme.Mode{theme.Light, theme.Dark} {
		name := map[theme.Mode]string{theme.Light: "light", theme.Dark: "dark"}[mode]
		t.Run(name, func(t *testing.T) {
			makeTable := func(props string) *ui.Tester {
				t.Helper()
				native := NewNativeBinding(t.Context())
				var failure error
				tester := ui.NewTesterWith(func(view *ui.Context) {
					theme.Apply(view, "", mode)
					ui.Column(view).Fill().Padding(20).Children(func() {
						_, failure = native.renderTable(view, NativeElement{ID: 1, Module: "sailry/ui", Export: "DataTable", Props: json.RawMessage(props)}, nil, nil)
					})
				}, 600, 300, ui.TesterOptions{Dark: mode == theme.Dark})
				tester.Frame()
				tester.Frame()
				if failure != nil {
					t.Fatal(failure)
				}
				return tester
			}
			t.Run("stripe preserves hover and selection", func(t *testing.T) {
				tester := makeTable(`["table",{"columns":["Name","Size"],"rows":[["alpha","1"],["beta","2"],["gamma","3"]],"stripe":true}]`)
				first, shown := tester.Find("alpha")
				second, also := tester.Find("beta")
				third, last := tester.Find("gamma")
				if !shown || !also || !last {
					t.Fatalf("missing rows: %q", tester.Texts())
				}
				pixel := func(row ui.Rect, edge float32) [3]uint8 {
					px := tester.Image().RGBAAt(int(edge), int(row.Y+row.H/2))
					return [3]uint8{px.R, px.G, px.B}
				}
				for _, x := range []float32{25, 150, 190, 205, 330, 375} {
					if pixel(first, x) == pixel(second, x) || pixel(first, x) != pixel(third, x) {
						t.Fatalf("stripe does not span alternating rows at %v: %v %v %v", x, pixel(first, x), pixel(second, x), pixel(third, x))
					}
				}
				tester.Move(first.X+first.W/2, first.Y+first.H/2)
				tester.Frame()
				tester.Frame()
				hover := pixel(first, 150)
				tester.Move(second.X+second.W/2, second.Y+second.H/2)
				tester.Frame()
				tester.Frame()
				if hover != pixel(second, 150) {
					t.Fatalf("striped hover differs from an ordinary row: %v, %v", hover, pixel(second, 150))
				}
				tester.ClickAt(second.X+second.W/2, second.Y+second.H/2)
				tester.Move(2, 2)
				tester.Frame()
				tester.Frame()
				chosen := pixel(second, 150)
				tester.ClickAt(first.X+first.W/2, first.Y+first.H/2)
				tester.Move(2, 2)
				tester.Frame()
				tester.Frame()
				if chosen != pixel(first, 150) {
					t.Fatalf("striped selection differs from an ordinary row: %v, %v", chosen, pixel(first, 150))
				}
			})
			t.Run("bordered false draws no frame around the table", func(t *testing.T) {
				edge := func(table string) [3]uint8 {
					tester := makeTable(table)
					row, shown := tester.Find("alpha")
					if !shown {
						t.Fatalf("missing rows: %q", tester.Texts())
					}
					px := tester.Image().RGBAAt(20, int(row.Y+row.H/2))
					return [3]uint8{px.R, px.G, px.B}
				}
				framed := edge(`["table",{"columns":["Name"],"rows":[["alpha"]]}]`)
				flush := edge(`["table",{"columns":["Name"],"rows":[["alpha"]],"bordered":false}]`)
				inside := func() [3]uint8 {
					tester := makeTable(`["table",{"columns":["Name"],"rows":[["alpha"]],"bordered":false}]`)
					row, _ := tester.Find("alpha")
					px := tester.Image().RGBAAt(24, int(row.Y+row.H/2))
					return [3]uint8{px.R, px.G, px.B}
				}()
				if framed == flush || flush != inside {
					t.Fatalf("a flush table keeps its frame: framed %v, flush %v, inside %v", framed, flush, inside)
				}
			})
			t.Run("header false removes its strip and accessible cells", func(t *testing.T) {
				tester := makeTable(`["table",{"columns":["Name","Size"],"rows":[["alpha","1"],["beta","2"]],"header":false}]`)
				if tester.HasText("Name") || tester.HasText("Size") {
					t.Fatalf("hidden header is drawn: %q", tester.Texts())
				}
				if _, exists := tester.AccessByRole(ui.RoleColumnHeader, "Name"); exists {
					t.Fatal("hidden header remains accessible")
				}
				first, shown := tester.Find("alpha")
				if !shown || first.Y < 20 || first.Y > 32 {
					t.Fatalf("header false leaves space above the first row: %v", first)
				}
			})
			t.Run("columns without rows keep the header and centered empty state", func(t *testing.T) {
				tester := makeTable(`["table",{"columns":["Name","Size"],"rows":[],"empty":"No fixture results","empty_icon":"database"}]`)
				title, headed := tester.Find("Name")
				empty, shown := tester.Find("No fixture results")
				if !headed || !shown || empty.Y <= title.Y+48 || math.Abs(float64(empty.X+empty.W/2-300)) > 1 {
					t.Fatalf("header or centered empty state missing: header %v, empty %v, text %q", title, empty, tester.Texts())
				}
			})
		})
	}
}
