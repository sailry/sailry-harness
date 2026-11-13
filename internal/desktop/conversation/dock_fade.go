package conversation

import (
	"math"

	"github.com/egoist/mygo/ui"
)

// dockFadeEffect keeps the native list and its scroll bar in one layer.
// MyGo's layered masks rasterize a full-size CPU image whenever the list's
// width changes. Its Effect API reuses a layer, but Painter has no gradient
// clip. Copying the fade's device pixel rows with native image tint keeps
// GPU batching and software rendering inexpensive without a shader interpreter.
type dockFadeEffect struct{ reserve float32 }

func (dockFadeEffect) EffectMargin() ui.Point { return ui.Point{} }

func (f dockFadeEffect) PaintEffect(p *ui.Painter, box ui.Rect, content *ui.Bitmap, inset ui.Point) {
	scale := p.Scale()
	width := max(box.W-scrollStrip, 0)
	end := min(max(box.H-f.reserve, 0), box.H)
	start := max(end-dockFade, 0)
	// The layer starts on a device pixel. Snap all three regions to the
	// same boundaries so fractional animation positions never resample text.
	pixel := func(dip float32) float32 { return float32(math.Round(float64(dip * scale))) }
	layerX, layerY := box.X*scale-inset.X, box.Y*scale-inset.Y
	left, right := pixel(box.X), pixel(box.X+box.W)
	top, bottom := pixel(box.Y), pixel(box.Y+box.H)
	strip, fadeStart, fadeEnd := pixel(box.X+width), pixel(box.Y+start), pixel(box.Y+end)
	copy := func(x0, y0, x1, y1 float32, options ui.ImageOptions) {
		p.ImageRegion(content,
			ui.Rect{X: x0 - layerX, Y: y0 - layerY, W: x1 - x0, H: y1 - y0},
			ui.Rect{X: x0 / scale, Y: y0 / scale, W: (x1 - x0) / scale, H: (y1 - y0) / scale}, options)
	}
	copy(left, top, strip, fadeStart, ui.ImageOptions{})
	copy(strip, top, right, bottom, ui.ImageOptions{})
	for y := fadeStart; y < fadeEnd; y++ {
		alpha := 1 - (y+0.5-fadeStart)/(fadeEnd-fadeStart)
		copy(left, y, strip, y+1, ui.ImageOptions{Tint: ui.RGBA(255, 255, 255, alpha)})
	}
}
