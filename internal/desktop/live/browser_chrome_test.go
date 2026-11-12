package live

import (
	"bytes"
	"context"
	"encoding/json"
	"image"
	"image/color"
	"image/jpeg"
	"image/png"
	"os"
	"path/filepath"
	"slices"
	"strings"
	"testing"
	"time"

	"github.com/egoist/mygo"
	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/desktop/composer"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/panel"
	"github.com/sailry/sailry-harness/internal/desktop/preferences"
	"github.com/sailry/sailry-harness/internal/desktop/shell"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/node"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// browserChrome drives the browser tab's chrome of the whole desktop against
// a real Node, in Chinese and dark: the address policy, the loading, loaded
// and failed states, the menu, the page's frame against the tab's body
// through every change of the layout, and a picked element reaching the
// composer. The page's native view is a typed nil placed by the production
// layout: the headless window records where it goes, and nothing loads. It
// runs as a step of TestPanelProjection. SAILRY_SNAPSHOTS names a folder that
// takes the frames; a native page never shows in them.
func browserChrome(t *testing.T) {
	ctx, cancel := context.WithTimeout(t.Context(), 3*time.Minute)
	defer cancel()
	profile := filepath.Join(t.TempDir(), "profile")
	owner, err := node.StartWithOptions(ctx, profile, node.Options{})
	if err != nil {
		t.Fatal(err)
	}
	defer owner.Stop(context.Background())
	controller := client.New(link.NewLocal(owner.ID(), owner.ID(), owner))
	if err := controller.Start(ctx); err != nil {
		t.Fatal(err)
	}
	defer controller.Close()
	r, err := NewWithPreferences(ctx, controller, preferences.ForStartup(profile))
	if err != nil {
		t.Fatal(err)
	}
	defer r.Close()
	r.browser = &browserState{available: true, tabs: map[string]*browserTab{}, serving: map[*Runtime]context.CancelFunc{r: func() {}},
		documents: map[string]*documentView{}, sources: map[string]protocol.ArtifactContent{}}
	// The adapter's own actions, but for the ones that need a native page:
	// those change the tab's state as the page's events would.
	var navigated []string
	reloads, inspected, configured := 0, 0, 0
	r.App.Panel.BrowserActions = &panel.BrowserActions{
		View: func(c *ui.Context, id string) bool {
			if tab := r.browser.tabs[id]; tab != nil && tab.url != "" {
				browserSurface(c, (*mygo.WebView)(nil))
				return true
			}
			return r.browserView(c, id)
		},
		Page: r.browserPage,
		Navigate: func(id, address string) {
			navigated = append(navigated, address)
			tab := r.browserTabFor(id)
			tab.url, tab.title = address, ""
			tab.begin()
		},
		Back: func(string) {}, Forward: func(string) {},
		Reload:   func(id string) { reloads++; r.browser.tabs[id].begin() },
		Stop:     r.browserStop,
		Pick:     r.browserPickToggle,
		Inspect:  func(string) { inspected++ },
		Settings: func() { configured++ },
	}
	r.App.Mode = theme.Dark
	width, height := 1280, 820
	options := ui.TesterOptions{Dark: true, Preferences: ui.Preferences{ReduceTransparency: true}}
	tester := ui.NewTesterWith(r.App.View, width, height, options)
	tester.SetLocale("zh-CN")
	text := func(key string) string { return i18n.In("zh-CN", key) }
	app := r.App
	settle := func() {
		t.Helper()
		for range 6 {
			r.workers.Wait()
			tester.Advance(200 * time.Millisecond)
			tester.Frame()
		}
	}
	shot := func(name string) {
		t.Helper()
		dir := os.Getenv("SAILRY_SNAPSHOTS")
		if dir == "" {
			return
		}
		file, err := os.Create(filepath.Join(dir, name+".png"))
		if err != nil {
			t.Fatal(err)
		}
		defer file.Close()
		if err := png.Encode(file, tester.Image()); err != nil {
			t.Fatal(err)
		}
	}
	// control is a control of the tab's chrome or body, under the header.
	control := func(role ui.Role, name string) (ui.AccessInfo, bool) {
		for _, info := range tester.AccessAll() {
			if info.Role == role && info.Name == name && info.Bounds.Y >= shell.HeaderHeight && info.Bounds.X >= float32(width)-app.PanelWidth-1 {
				return info, true
			}
		}
		return ui.AccessInfo{}, false
	}
	need := func(role ui.Role, name string) ui.AccessInfo {
		t.Helper()
		info, found := control(role, name)
		if !found {
			t.Fatalf("no %q in the browser tab; visible text: %q", name, tester.Texts())
		}
		return info
	}
	click := func(info ui.AccessInfo) {
		tester.ClickAt(info.Bounds.X+info.Bounds.W/2, info.Bounds.Y+info.Bounds.H/2)
		settle()
	}
	// dismiss closes the toast shown, which lies over the toolbar: its close
	// button is the one under the header.
	dismiss := func() {
		t.Helper()
		for _, info := range tester.AccessAll() {
			if info.Role == ui.RoleButton && info.Name == text("close") && info.Bounds.Y >= shell.HeaderHeight {
				tester.ClickAt(info.Bounds.X+info.Bounds.W/2, info.Bounds.Y+info.Bounds.H/2)
				settle()
				return
			}
		}
		t.Fatalf("no toast to close; visible text: %q", tester.Texts())
	}
	progress := func() bool {
		for _, info := range tester.AccessAll() {
			if info.Role == ui.RoleProgress && info.Bounds.Y >= shell.HeaderHeight && info.Bounds.H <= 2 {
				return true
			}
		}
		return false
	}
	// body is the tab's body: the panel's column under the 40 DIP toolbar
	// and, while the page loads, its 2 DIP bar, down to the window's edge.
	// The toolbar spans the column, 8 DIPs of padding around its first and
	// last buttons.
	body := func() ui.Rect {
		t.Helper()
		first, last := need(ui.RoleButton, text("browser_back")), need(ui.RoleMenuButton, text("more"))
		top := first.Bounds.Y + first.Bounds.H/2 + 20
		if progress() {
			top += 2
		}
		left, right := first.Bounds.X-8, last.Bounds.X+last.Bounds.W+8
		return ui.Rect{X: left, Y: top, W: right - left, H: float32(height) - top}
	}
	// frame is the box the window gave the page's native view, found from
	// inside the body outwards to a quarter of a DIP.
	frame := func() (ui.Rect, bool) {
		want := body()
		x, y := want.X+want.W/2, want.Y+want.H/2
		if tester.WebViewAt(x, y) == nil {
			return ui.Rect{}, false
		}
		edge := func(inside, outside float32, at func(float32) bool) float32 {
			for range 16 {
				middle := (inside + outside) / 2
				if at(middle) {
					inside = middle
				} else {
					outside = middle
				}
			}
			return (inside + outside) / 2
		}
		horizontal := func(value float32) bool { return tester.WebViewAt(value, y) != nil }
		vertical := func(value float32) bool { return tester.WebViewAt(x, value) != nil }
		left, right := edge(x, -8, horizontal), edge(x, float32(width)+8, horizontal)
		top, bottom := edge(y, -8, vertical), edge(y, float32(height)+8, vertical)
		return ui.Rect{X: left, Y: top, W: right - left, H: bottom - top}, true
	}
	// fills checks that the page's element is exactly the body and that the
	// window placed the native view there. The panel's resize handle lies
	// over the first DIPs of the left edge and keeps the pointer there, so
	// the view takes it from just inside that edge.
	fills := func(when string) {
		t.Helper()
		got, placed := frame()
		want := body()
		// While the panel slides, its column keeps its width and leaves by
		// the window's edge, which clips the page with it.
		sliding := want.X+want.W > float32(width)+1
		want.W = min(want.W, float32(width)-want.X)
		// Within a device pixel: the layout lands on whole pixels, which at
		// a fractional scale are not whole DIPs.
		near := func(a, b float32) bool { return a-b < 1 && b-a < 1 }
		if !placed || got.X < want.X-1 || got.X > want.X+4 || !near(got.Y, want.Y) || !near(got.X+got.W, want.X+want.W) || !near(got.Y+got.H, want.Y+want.H) {
			t.Fatalf("%s: the page's frame is %+v (placed %v), the tab's body %+v", when, got, placed, want)
		}
		element := false
		for _, info := range tester.AccessAll() {
			element = element || near(info.Bounds.X, want.X) && near(info.Bounds.Y, want.Y) && near(info.Bounds.W, want.W) && near(info.Bounds.H, want.H)
		}
		if !element && !sliding {
			t.Fatalf("%s: no element of the frame is the tab's body %+v", when, want)
		}
		if !near(want.X+want.W, float32(width)) || want.W < 200 {
			t.Fatalf("%s: the tab's body %+v does not reach the window's edge at %d", when, want, width)
		}
	}

	// The address policy, as the Rust browser's.
	for typed, want := range map[string]string{
		"https://www.baidu.com/": "https://www.baidu.com/", "example.com": "https://example.com", "localhost:3000/a": "http://localhost:3000/a",
		"127.0.0.1:8080": "http://127.0.0.1:8080", "[::1]:9": "http://[::1]:9", " go generics ": "https://www.google.com/search?q=go+generics",
		"C++ & more": "https://www.google.com/search?q=C%2B%2B+%26+more", "javascript:alert(1)": "", "file:///etc/passwd": "", "DATA:text/html,x": "", "  ": "",
	} {
		if got, ok := panel.BrowserDestination(typed); got != want || ok != (want != "") {
			t.Fatalf("destination of %q = %q, %v; want %q", typed, got, ok, want)
		}
	}

	settle()
	if tester.HasText(text("close")) {
		if err := tester.Click(text("close")); err != nil {
			t.Fatal(err)
		}
		settle()
	}
	tester.Key(ui.Cmd|ui.Alt, ui.KeyB)
	tester.Advance(time.Second)
	settle()
	if !app.PanelOpen || !app.Panel.Live {
		t.Fatalf("the panel: open %v, live %v", app.PanelOpen, app.Panel.Live)
	}

	// A new tab: the empty state, nothing to reload or select, one control
	// that opens a tab (the header's "+"), and the keyboard in the address.
	app.Panel.New(panel.Browser)
	settle()
	id := app.Panel.Active.ID
	if app.Panel.Active.Kind != panel.Browser || !tester.HasText(text("browser_empty")) {
		t.Fatalf("a new browser tab: %v; visible text: %q", app.Panel.Active, tester.Texts())
	}
	// The toolbar's buttons are named in full, so a New tab button beside
	// the header's + fails here whatever it is called.
	first := need(ui.RoleButton, text("browser_back"))
	var buttons []string
	for _, info := range tester.AccessAll() {
		if (info.Role == ui.RoleButton || info.Role == ui.RoleMenuButton) && info.Bounds.Y < first.Bounds.Y+first.Bounds.H && info.Bounds.Y+info.Bounds.H > first.Bounds.Y &&
			info.Bounds.X >= float32(width)-app.PanelWidth-1 {
			buttons = append(buttons, info.Name)
		}
	}
	if want := []string{text("browser_back"), text("browser_forward"), text("browser_reload"), text("browser_annotate"), text("more")}; !slices.Equal(buttons, want) {
		t.Fatalf("the toolbar's buttons are %q, want %q", buttons, want)
	}
	for _, name := range []string{"browser_back", "browser_forward", "browser_reload", "browser_annotate"} {
		if info := need(ui.RoleButton, text(name)); !info.Disabled {
			t.Fatalf("%s is enabled on an empty tab", name)
		}
	}
	if _, placed := frame(); placed {
		t.Fatal("an empty tab placed a page")
	}
	shot("browser-empty")
	// An address the tab never opens: a toast, and nothing navigates.
	tester.Type("javascript:alert(1)")
	tester.Key(0, ui.KeyEnter)
	settle()
	if len(navigated) != 0 || !tester.HasText(text("browser_invalid_address")) {
		t.Fatalf("an invalid address: navigated %v; visible text: %q", navigated, tester.Texts())
	}
	dismiss()
	click(need(ui.RoleTextField, text("browser_address")))
	tester.Key(ui.Cmd, ui.KeyA)
	tester.Type("www.baidu.com")
	tester.Key(0, ui.KeyEnter)
	tester.Frame()
	if len(navigated) != 1 || navigated[0] != "https://www.baidu.com" {
		t.Fatalf("navigated %v after a host typed into the address", navigated)
	}

	// Loading: the bar under the toolbar, Stop in Reload's place, and the
	// page already over the whole body.
	tab := r.browser.tabs[id]
	settle()
	if !tab.loading || !progress() {
		t.Fatalf("loading %v, progress bar %v; visible text: %q", tab.loading, progress(), tester.Texts())
	}
	if _, found := control(ui.RoleButton, text("browser_reload")); found {
		t.Fatal("Reload shows while the page loads")
	}
	if info := need(ui.RoleButton, text("browser_annotate")); !info.Disabled {
		t.Fatal("Select element is enabled while the page loads")
	}
	fills("loading")
	shot("browser-loading")
	click(need(ui.RoleButton, text("browser_stop")))
	if tab.loading || progress() {
		t.Fatal("Stop left the page loading")
	}
	click(need(ui.RoleButton, text("browser_reload")))
	if reloads != 1 || !tab.loading {
		t.Fatalf("Reload: %d reloads, loading %v", reloads, tab.loading)
	}

	// Loaded: Reload is back, the bar is gone, the tab takes the page's
	// title, and Select element is offered.
	tab.loading, tab.title = false, "百度一下，你就知道"
	settle()
	if progress() {
		t.Fatal("the progress bar stays after the load")
	}
	need(ui.RoleButton, text("browser_reload"))
	if info := need(ui.RoleButton, text("browser_annotate")); info.Disabled {
		t.Fatal("Select element is disabled on a loaded page")
	}
	if app.Panel.Title(nil, app.Panel.Active) != tab.title {
		t.Fatalf("the tab is titled %q, the page %q", app.Panel.Title(nil, app.Panel.Active), tab.title)
	}
	fills("loaded")
	shot("browser-loaded")

	// The page fills the body through every change of the layout.
	for _, panelWidth := range []float32{360, 520, 700, 440} {
		app.PanelWidth = panelWidth
		tester.Frame()
		fills("panel resized")
	}
	for _, size := range [][2]int{{1600, 1000}, {900, 600}, {1280, 820}} {
		width, height = size[0], size[1]
		tester.SetSize(width, height)
		tester.Frame()
		fills("window resized")
	}
	for _, scale := range []float32{2, 1.5, 1} {
		tester.SetScale(scale)
		tester.Frame()
		fills("display scale changed")
	}
	// Closing and opening the panel: on every frame of the slide the page
	// that takes the pointer is the body's part inside the window, and it is
	// nowhere once its tab is gone. A leaving pane is inert, so its page is
	// placed without taking the pointer and the headless window cannot be
	// asked where: those frames are counted, not measured.
	measured, inert := 0, 0
	for _, open := range []bool{false, true} {
		tester.Key(ui.Cmd|ui.Alt, ui.KeyB)
		for range 40 {
			tester.Advance(16 * time.Millisecond)
			tester.Frame()
			if _, found := control(ui.RoleButton, text("browser_back")); !found {
				if tester.WebViewAt(float32(width)-40, float32(height)-40) != nil {
					t.Fatal("the page stays on screen without its tab")
				}
			} else if _, placed := frame(); placed {
				measured++
				fills("panel sliding")
			} else {
				inert++
			}
		}
		settle()
		if app.PanelOpen != open {
			t.Fatalf("the panel is open %v, want %v", app.PanelOpen, open)
		}
	}
	if measured == 0 {
		t.Fatalf("no frame of the panel's slide placed a page that takes the pointer (%d inert)", inert)
	}
	t.Logf("panel slide: the page's frame was measured on %d frames and inert on %d", measured, inert)
	fills("panel reopened")

	// The menu: the web inspector and the browser's settings.
	click(need(ui.RoleMenuButton, text("more")))
	var items []string
	for _, info := range tester.AccessAll() {
		if info.Role == ui.RoleMenuItem {
			items = append(items, info.Name)
		}
	}
	if len(items) != 2 || !strings.HasPrefix(items[0], text("browser_inspect")) || !strings.HasPrefix(items[1], text("settings")) {
		t.Fatalf("the menu lists %q", items)
	}
	shot("browser-menu")
	choose := func(key string) {
		t.Helper()
		for _, info := range tester.AccessAll() {
			if info.Role == ui.RoleMenuItem && strings.HasPrefix(info.Name, text(key)) {
				click(info)
				return
			}
		}
		t.Fatalf("the menu has no %q", key)
	}
	choose("browser_inspect")
	click(need(ui.RoleMenuButton, text("more")))
	choose("settings")
	if inspected != 1 || configured != 1 {
		t.Fatalf("the menu's items ran: inspector %d, settings %d", inspected, configured)
	}

	// A picked element reaches the composer of the tab's conversation as its
	// picture and its page context; forged content does not.
	const token = "0123456789abcdef"
	message := `{"kind":"element","token":"` + token + `","review":false,"element":{"url":"https://www.baidu.com/","title":"百度",` +
		`"selector":"#su","path":"html > body > form > input#su","tag":"input","id":"su","classes":["btn","self-btn"],` +
		`"attributes":{"type":"submit"},"name":"百度一下","text":"","html":"<input id=\"su\" type=\"submit\">",` +
		`"styles":{"display":"inline-block"},"component":null,"bounds":[100,50,120,40],"viewport":[400,300],"truncated":false}}`
	element, ok := browserPickMessage(message, token)
	if !ok || element == nil || element.label() != "百度一下" {
		t.Fatalf("the picked element was not read: %v %+v", ok, element)
	}
	for name, forged := range map[string]string{
		"another selection's token": strings.Replace(message, token, "ffff", 1),
		"an unknown field":          strings.Replace(message, `"truncated":false`, `"truncated":false,"extra":1`, 1),
		"a file address":            strings.Replace(message, "https://www.baidu.com/", "file:///etc/passwd", 1),
		"bounds outside the page":   strings.Replace(message, "[100,50,120,40]", "[900,50,120,40]", 1),
		"an attribute not listed":   strings.Replace(message, `{"type":"submit"}`, `{"onclick":"x()"}`, 1),
	} {
		if _, ok := browserPickMessage(forged, token); ok {
			t.Fatalf("a message with %s was accepted", name)
		}
	}
	if cancelled, ok := browserPickMessage(`{"kind":"cancelled","token":"`+token+`"}`, token); !ok || cancelled != nil {
		t.Fatal("a cancelled selection was not read")
	}
	// A capture of the viewport at two pixels per CSS pixel, the element red.
	capture := image.NewRGBA(image.Rect(0, 0, 800, 600))
	for y := range 600 {
		for x := range 800 {
			shade := color.RGBA{255, 255, 255, 255}
			if x >= 200 && x < 440 && y >= 100 && y < 180 {
				shade = color.RGBA{220, 0, 0, 255}
			}
			capture.SetRGBA(x, y, shade)
		}
	}
	var encoded bytes.Buffer
	if err := png.Encode(&encoded, capture); err != nil {
		t.Fatal(err)
	}
	picture, ok := browserElementImage(encoded.Bytes(), element)
	if !ok {
		t.Fatal("the element was not cropped from the capture")
	}
	if crop, err := jpeg.Decode(bytes.NewReader(picture)); err != nil || crop.Bounds().Dx() != (120+16)*2 || crop.Bounds().Dy() != (40+16)*2 {
		t.Fatalf("the element's picture: %v, %v", err, crop.Bounds())
	}
	comp := app.Composer(app.Sidebar.Selected)
	// The selection under way shows as Cancel selection, which ends it.
	tab.pick = &browserPick{token: token, cancel: func() {}}
	pick := tab.pick
	settle()
	need(ui.RoleButton, text("browser_cancel_selection"))
	r.enqueue(func(c *ui.Context) {
		if tab.pick == pick {
			r.browserAttachElement(c, tab, element, picture)
		}
	})
	settle()
	if len(comp.Attachments) != 2 || comp.Attachments[0].Name != "browser-element-1.jpg" || comp.Attachments[0].Mime != "image/jpeg" ||
		comp.Attachments[1].Name != "browser-element-1.json" || comp.Attachments[1].Mime != "application/json" {
		t.Fatalf("the composer's attachments after a pick: %+v", comp.Attachments)
	}
	var document struct {
		Kind, Notice string
		Page         struct{ URL string }
		Element      struct {
			Selector string
			Name     string `json:"accessible_name"`
			Bounds   struct{ Width float64 }
		}
	}
	if err := json.Unmarshal(comp.Attachments[1].Data, &document); err != nil || document.Kind != "browser_element" || !strings.HasPrefix(document.Notice, "Untrusted page content") ||
		document.Page.URL != "https://www.baidu.com/" || document.Element.Selector != "#su" || document.Element.Name != "百度一下" || document.Element.Bounds.Width != 120 {
		t.Fatalf("the element's page context: %v\n%s", err, comp.Attachments[1].Data)
	}
	if !bytes.Equal(comp.Attachments[0].Data, picture) {
		t.Fatal("the composer's picture is not the element's")
	}
	// One chip in the draft stands for both files: they have no cards, and
	// leave with the chip.
	if len(comp.References) != 1 || comp.References[0].Kind != composer.Element || comp.References[0].Label != "百度一下" ||
		comp.Attachments[0].Chip != comp.References[0].ID || comp.Attachments[1].Chip != comp.References[0].ID || !strings.Contains(comp.Draft, "@百度一下") {
		t.Fatalf("the element's chip: draft %q, references %+v", comp.Draft, comp.References)
	}
	if tester.HasText("browser-element-1.json") {
		t.Errorf("the element's page context has a card: %q", tester.Texts())
	}
	click(need(ui.RoleButton, text("browser_cancel_selection")))
	if tab.pick != nil {
		t.Fatal("Cancel selection left the selection under way")
	}
	need(ui.RoleButton, text("browser_annotate"))
	// A page context stays within its bound whatever the page wrote.
	large := *element
	large.HTML, large.Path, large.Title = strings.Repeat("<i>", 600), strings.Repeat("div > ", 300), strings.Repeat("t", 1000)
	large.Text = strings.Repeat("文", 500)
	large.Attributes = map[string]string{}
	for _, name := range browserElementAttributes[:16] {
		large.Attributes[name] = strings.Repeat("a", 512)
	}
	large.Styles = map[string]string{}
	for _, name := range browserElementStyles {
		large.Styles[name] = strings.Repeat("s", 256)
	}
	if bounded := large.document(); !large.valid() || len(bounded) > browserContextBytes || !bytes.Contains(bounded, []byte(`"truncated":true`)) {
		t.Fatalf("a large element's context is %d bytes (valid %v)", len(bounded), large.valid())
	}
	// The page is its conversation's. Another conversation takes the screen,
	// an unsent chat in a project, as a pick arrives: the pick is refused
	// with a toast and ends the selection, no composer takes it, and the
	// page stays with its tab in its conversation's panel, off screen.
	home, open := app.Sidebar.Selected, panel.Tab{Kind: panel.Browser, ID: id}
	output, err := controller.Do(ctx, protocol.Command{Kind: "create_project", Data: protocol.ProjectsDraft{
		Name: "Another conversation", Path: t.TempDir(), Appearance: protocol.DefaultProjectsAppearance(), Source: protocol.ProjectsSource{Kind: "local"},
	}})
	if err != nil {
		t.Fatal(err)
	}
	folder, valid := detail[protocol.Project](output.Data)
	if !valid {
		t.Fatal("the project was not returned")
	}
	project := func() *viewmodel.Project {
		for _, shown := range app.Data.Projects {
			if shown.ID == folder.ID.String() {
				return shown
			}
		}
		return nil
	}
	for end := time.Now().Add(20 * time.Second); project() == nil; time.Sleep(10 * time.Millisecond) {
		if time.Now().After(end) {
			t.Fatalf("the project did not reach the window; visible text: %q", tester.Texts())
		}
		settle()
	}
	stale := func() int {
		count := 0
		for _, shown := range tester.Texts() {
			if shown == text("browser_annotation_stale") {
				count++
			}
		}
		return count
	}
	ended := 0
	tab.pick = &browserPick{token: token, cancel: func() { ended++ }}
	settle()
	need(ui.RoleButton, text("browser_cancel_selection"))
	r.newDraft(project())
	r.enqueue(func(c *ui.Context) { r.browserAttachElement(c, tab, element, picture) })
	settle()
	other := app.Sidebar.Selected
	if other == home || !strings.HasPrefix(other, "draft/") || tab.conversation != home {
		t.Fatalf("another conversation: %q shown, the page's is %q, was %q", other, tab.conversation, home)
	}
	if len(comp.Attachments) != 2 || len(app.Composer(other).Attachments) != 0 || stale() != 1 {
		t.Fatalf("a stale pick: %d and %d attachments; visible text: %q", len(comp.Attachments), len(app.Composer(other).Attachments), tester.Texts())
	}
	if tab.pick != nil || ended != 1 {
		t.Fatalf("a stale pick left the selection under way: %v, ended %d times", tab.pick, ended)
	}
	if slices.Contains(app.Panel.Tabs, open) || !slices.Contains(app.ConversationTabs(home), open) || r.browser.tabs[id] != tab {
		t.Fatalf("the page did not stay with its conversation: shown %v, its own %v, page %v", app.Panel.Tabs, app.ConversationTabs(home), r.browser.tabs[id])
	}
	if tester.WebViewAt(float32(width)-40, float32(height)-40) != nil {
		t.Fatal("the page stays on screen with another conversation")
	}
	dismiss()
	// Its conversation shows again: the tab and its page with it, nothing
	// selecting.
	back := func(when string) {
		t.Helper()
		app.Sidebar.Selected = home
		settle()
		if !slices.Equal(app.Panel.Tabs, []panel.Tab{open}) || app.Panel.Active != open || !app.PanelOpen || r.browser.tabs[id] != tab || tab.conversation != home {
			t.Fatalf("%s: tabs %v, active %v, open %v, page %+v", when, app.Panel.Tabs, app.Panel.Active, app.PanelOpen, r.browser.tabs[id])
		}
		if info := need(ui.RoleButton, text("browser_annotate")); info.Disabled {
			t.Fatalf("%s: Select element is disabled", when)
		}
		fills(when)
	}
	back("the conversation shown again")
	// A selection under way when its conversation leaves the screen ends
	// there, with no pick and no toast.
	toasts := stale()
	tab.pick = &browserPick{token: token, cancel: func() { ended++ }}
	settle()
	need(ui.RoleButton, text("browser_cancel_selection"))
	r.newDraft(project())
	settle()
	if app.Sidebar.Selected == home || tab.pick != nil || ended != 2 || stale() > toasts || len(comp.Attachments) != 2 {
		t.Fatalf("a selection outlived its conversation's leaving the screen: shown %q, selecting %v, ended %d times, %d attachments; visible text: %q",
			app.Sidebar.Selected, tab.pick != nil, ended, len(comp.Attachments), tester.Texts())
	}
	back("the conversation shown after a selection")

	// A failed load: the body says so over the address and offers the
	// reload; the page is not placed, and nothing is selectable.
	tab.loading, tab.failure = false, "browser_load_failed"
	settle()
	if !tester.HasText(text("browser_load_failed")) || !tester.HasText("https://www.baidu.com") || progress() {
		t.Fatalf("a failed load; visible text: %q", tester.Texts())
	}
	if _, placed := frame(); placed {
		t.Fatal("a failed page is still placed")
	}
	if info := need(ui.RoleButton, text("browser_annotate")); !info.Disabled {
		t.Fatal("Select element is enabled on a failed page")
	}
	shot("browser-failed")
	// The body's own Reload, below the toolbar's.
	var retry ui.AccessInfo
	for _, info := range tester.AccessAll() {
		if info.Role == ui.RoleButton && info.Name == text("browser_reload") && info.Bounds.Y > retry.Bounds.Y {
			retry = info
		}
	}
	click(retry)
	if reloads != 2 || !tab.loading || tab.failed() {
		t.Fatalf("Reload on a failed page: %d reloads, loading %v, failure %q", reloads, tab.loading, tab.failure)
	}
}
