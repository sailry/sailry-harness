package conversation

import (
	"fmt"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-go/internal/i18n"
	"github.com/sailry/sailry-go/internal/icons"
	"github.com/sailry/sailry-go/internal/kit"
	"github.com/sailry/sailry-go/internal/theme"
)

// Zoom limits of the image viewer (content/images/preview/viewport.rs).
const (
	MinZoom = 0.25
	MaxZoom = 8
)

// Viewer is the image viewer's state (content/images/preview.rs): the
// gallery it opened on, the picture shown, and how it is zoomed, panned,
// turned and flipped. Its methods are the viewer's pure logic.
type Viewer struct {
	Open   bool
	Images []Image
	Index  int
	Zoom   float32
	Turns  int // quarter turns clockwise
	FlipH  bool
	FlipV  bool
	X, Y   float32 // pan in DIPs
}

// show opens the viewer on list at i, fitted.
func (v *Viewer) show(list []Image, i int) {
	*v = Viewer{Open: true, Images: list, Index: i, Zoom: 1}
}

// ZoomBy zooms by factor within MinZoom and MaxZoom.
func (v *Viewer) ZoomBy(factor float32) {
	v.Zoom = min(MaxZoom, max(MinZoom, v.Zoom*factor))
}

// Fit shows the picture whole again, unturned, unpanned.
func (v *Viewer) Fit() { v.Zoom, v.Turns, v.FlipH, v.FlipV, v.X, v.Y = 1, 0, false, false, 0, 0 }

// Step shows the next or previous picture, staying at the ends, fitted.
func (v *Viewer) Step(next bool) {
	i := v.Index - 1
	if next {
		i = v.Index + 1
	}
	if i < 0 || i >= len(v.Images) {
		return
	}
	v.Index = i
	v.Fit()
}

// viewer builds the image viewer over the window while open: the picture
// fitted to the window, zoomed, panned by dragging, turned and flipped,
// with Kit's toolbar (previous, next, zoom out, the zoom, zoom in, fit,
// rotate, flip, save) and its keys: ← →, + −, 0. Escape closes it.
func (s *State) viewer(c *ui.Context, actions Actions) {
	v := &s.view
	if !v.Open || len(v.Images) == 0 {
		v.Open = false
		return
	}
	p := theme.Of(c)
	w, h := c.Size()
	im := v.Images[min(v.Index, len(v.Images)-1)]
	ui.DialogBase(c, &v.Open, func(back, panel ui.Element) {
		back.Background(p.Backdrop().Alpha(0.85))
		panel.Fill().Label(i18n.T(c, "chat_image_preview"))
		switch {
		case panel.Shortcut(0, ui.KeyLeft):
			v.Step(false)
		case panel.Shortcut(0, ui.KeyRight):
			v.Step(true)
		case panel.Shortcut(0, ui.KeyEqual), panel.Shortcut(ui.Shift, ui.KeyEqual):
			v.ZoomBy(1.25)
		case panel.Shortcut(0, ui.KeyMinus):
			v.ZoomBy(0.8)
		case panel.Shortcut(0, ui.Key0):
			v.Fit()
		}
		stage := ui.Box(c.Key("stage")).Fill().Center().Clip().Cursor(ui.CursorGrab)
		if dx, dy, ok := stage.Dragged(); ok {
			v.X, v.Y = v.X+dx, v.Y+dy
		}
		stage.Children(func() {
			if im.Source == nil {
				kit.Loader(c, 24)
				return
			}
			iw, ih := ui.ImageSize(im.Source)
			if v.Turns%2 == 1 {
				iw, ih = ih, iw
			}
			k := min(1, min((w-96)/max(iw, 1), (h-140)/max(ih, 1)))
			sx, sy := v.Zoom, v.Zoom
			if v.FlipH {
				sx = -sx
			}
			if v.FlipV {
				sy = -sy
			}
			bw, bh := iw*k, ih*k
			if v.Turns%2 == 1 {
				bw, bh = bh, bw
			}
			ui.Image(c, im.Source).Size(bw, bh).Fit(ui.Contain).Translate(v.X, v.Y).Scale(sx, sy).Rotate(float32(90 * (v.Turns % 4)))
		})
		bar := ui.Row(c.Key("bar")).Absolute().Bottom(24).Left((w - 440) / 2).Width(440).Height(40).Padding(4).Gap(2).
			Radius(p.RadiusLg).Background(p.Floating()).TextColor(p.PopoverForeground).AlignItems(ui.Center).Justify(ui.Center)
		bar.Children(func() {
			btn := func(key string, icon *ui.SVG, label string, off bool, do func()) {
				kit.IconButton(c.Key(key), icon, i18n.T(c, label), 32, false).Disabled(off).OnClick(do)
			}
			if len(v.Images) > 1 {
				btn("prev", icons.ChevronLeft, "chat_image_previous", v.Index == 0, func() { v.Step(false) })
				btn("next", icons.ChevronRight, "chat_image_next", v.Index == len(v.Images)-1, func() { v.Step(true) })
			}
			btn("out", icons.Minus, "chat_image_zoom_out", v.Zoom <= MinZoom, func() { v.ZoomBy(0.8) })
			ui.Text(c, fmt.Sprintf("%d%%", int(v.Zoom*100+0.5))).FontSize(12).Width(44).TextAlign(ui.Center).TextColor(p.MutedForeground)
			btn("in", icons.Plus, "chat_image_zoom_in", v.Zoom >= MaxZoom, func() { v.ZoomBy(1.25) })
			btn("fit", icons.Minimize, "chat_image_fit", false, v.Fit)
			btn("rotate", icons.RotateCw, "chat_image_rotate", false, func() { v.Turns = (v.Turns + 1) % 4 })
			btn("flip-h", icons.Redo, "chat_image_flip_horizontal", false, func() { v.FlipH = !v.FlipH })
			btn("flip-v", icons.ArrowDown, "chat_image_flip_vertical", false, func() { v.FlipV = !v.FlipV })
			btn("save", icons.Save, "files_download", false, func() { actions.FileAction(File{ID: im.ID, Path: im.Name, Kind: ImageFile, Image: &im}, Download) })
			btn("close", icons.Close, "close", false, func() { v.Open = false })
		})
	})
}
