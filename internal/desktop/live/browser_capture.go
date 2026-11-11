package live

import (
	"bytes"
	"context"
	"encoding/json"
	"image"
	"image/jpeg"
	"image/png"
	"math"
	"time"

	"github.com/MoeForge/moego/base64"
	"github.com/sailry/sailry-harness/internal/protocol"
	"golang.org/x/image/draw"
)

// Capture bounds of the v1 browser contract.
const (
	browserMaxImageBytes = 1024 * 1024
	browserMaxImageSide  = 1568
)

type browserRect struct{ x, y, width, height float64 }

// browserCapturePlan maps viewport CSS geometry to captured and encoded pixels.
type browserCapturePlan struct {
	region        browserRect // the visible part of the requested region, in viewport CSS pixels
	viewport      [2]float64
	ratio         float64
	width, height int // the encoded size before any size-bounded reduction
}

func planBrowserCapture(bounds browserRect, viewport [2]float64, ratio float64, scale string) (browserCapturePlan, bool) {
	valid := func(value float64) bool { return !math.IsNaN(value) && !math.IsInf(value, 0) && value > 0 }
	if !valid(viewport[0]) || !valid(viewport[1]) || !valid(ratio) {
		return browserCapturePlan{}, false
	}
	ratio = min(max(ratio, 0.25), 8)
	for _, value := range []float64{bounds.x, bounds.y, bounds.width, bounds.height} {
		if math.IsNaN(value) || math.IsInf(value, 0) {
			return browserCapturePlan{}, false
		}
	}
	left, top := max(bounds.x, 0), max(bounds.y, 0)
	right, bottom := min(bounds.x+bounds.width, viewport[0]), min(bounds.y+bounds.height, viewport[1])
	if right-left < 1 || bottom-top < 1 {
		return browserCapturePlan{}, false
	}
	region := browserRect{left, top, right - left, bottom - top}
	factor := 1.0
	if scale == string(protocol.BrowserScaleDevice) {
		factor = ratio
	}
	width, height := region.width*factor, region.height*factor
	fit := min(browserMaxImageSide/max(width, height), 1)
	side := func(value float64) int { return min(max(int(math.Round(value*fit)), 1), browserMaxImageSide) }
	return browserCapturePlan{region: region, viewport: viewport, ratio: ratio, width: side(width), height: side(height)}, true
}

// cropBrowserCapture is the pixel crop of a region from a capture of the
// whole CSS viewport.
func cropBrowserCapture(region browserRect, viewport [2]float64, size image.Point) image.Rectangle {
	axis := func(start, length, css float64, pixels int) (int, int) {
		factor := float64(pixels) / css
		first := min(max(int(math.Floor(start*factor)), 0), max(pixels-1, 0))
		last := min(max(int(math.Ceil((start+length)*factor)), first+1), max(pixels, 1))
		return first, last
	}
	left, right := axis(region.x, region.width, viewport[0], size.X)
	top, bottom := axis(region.y, region.height, viewport[1], size.Y)
	return image.Rect(left, top, right, bottom)
}

// encodeBrowserCapture crops the measured region and encodes it within the
// transport limit, shrinking by a quarter while it does not fit.
func encodeBrowserCapture(capture []byte, plan browserCapturePlan, format string) ([]byte, int, int, bool) {
	source, err := png.Decode(bytes.NewReader(capture))
	if err != nil {
		return nil, 0, 0, false
	}
	bounds := source.Bounds()
	crop := cropBrowserCapture(plan.region, plan.viewport, bounds.Size()).Add(bounds.Min)
	width, height := plan.width, plan.height
	for {
		scaled := image.NewRGBA(image.Rect(0, 0, width, height))
		draw.BiLinear.Scale(scaled, scaled.Bounds(), source, crop, draw.Src, nil)
		var output bytes.Buffer
		if format == string(protocol.BrowserFormatPng) {
			if png.Encode(&output, scaled) == nil && output.Len() <= browserMaxImageBytes {
				return output.Bytes(), width, height, true
			}
		} else {
			for _, quality := range []int{85, 70, 55} {
				output.Reset()
				if jpeg.Encode(&output, scaled, &jpeg.Options{Quality: quality}) == nil && output.Len() <= browserMaxImageBytes {
					return output.Bytes(), width, height, true
				}
			}
		}
		if max(width, height) <= 256 {
			return nil, 0, 0, false
		}
		width, height = max(width*3/4, 1), max(height*3/4, 1)
	}
}

// captureBrowser measures the page first so the region matches the captured
// pixels. The text facts let models without vision use the capture too.
func (r *Runtime) captureBrowser(ctx context.Context, plan browserPlan, request browserRequest) protocol.BrowserResult {
	failed := browserFailure(protocol.ErrorCodeUnavailable, "browser capture failed")
	if failure := browserLoaded(ctx, plan, time.Now().Add(20*time.Second)); failure != nil {
		return *failure
	}
	target := request.Target
	if len(target) == 0 {
		target = json.RawMessage("null")
	}
	measure := func(scroll bool) []byte {
		encoded, _ := json.Marshal(map[string]any{"action": "target", "target": target, "scroll": scroll})
		return encoded
	}
	if string(target) != "null" {
		if _, failure := browserEvaluate(ctx, plan.view, measure(true)); failure != nil {
			return *failure
		}
		// Measure after layout and painting settle at the scrolled position.
		if !browserSleep(ctx, 100*time.Millisecond) {
			return browserEnded(ctx)
		}
	}
	facts, failure := browserEvaluate(ctx, plan.view, measure(false))
	if failure != nil {
		return *failure
	}
	number := func(object any, key string) float64 {
		values, _ := object.(map[string]any)
		value, ok := values[key].(float64)
		if !ok {
			return math.NaN()
		}
		return value
	}
	bounds, viewport := facts["bounds"], facts["viewport"]
	geometry, visible := planBrowserCapture(browserRect{number(bounds, "x"), number(bounds, "y"), number(bounds, "width"), number(bounds, "height")},
		[2]float64{number(viewport, "width"), number(viewport, "height")}, number(facts, "ratio"), request.Scale)
	if !visible {
		return browserFailure(protocol.ErrorCodeConflict, "capture region is outside the visible page; read the page again")
	}
	type pixels struct {
		data []byte
		err  error
	}
	captured := make(chan pixels, 1)
	go func() {
		data, err := plan.view.CapturePage()
		captured <- pixels{data, err}
	}()
	timer := time.NewTimer(5 * time.Second)
	defer timer.Stop()
	var capture pixels
	select {
	case capture = <-captured:
	case <-timer.C:
		return failed
	case <-ctx.Done():
		return browserEnded(ctx)
	}
	if capture.err != nil {
		return failed
	}
	encoded, width, height, ok := encodeBrowserCapture(capture.data, geometry, request.Format)
	if !ok {
		return failed
	}
	round := func(value float64) float64 { return math.Round(value*10) / 10 }
	value := map[string]any{
		"tab": plan.tab.id, "url": facts["url"], "title": facts["title"], "viewport": facts["viewport"], "device_pixel_ratio": geometry.ratio,
		"bounds": map[string]float64{"x": round(geometry.region.x), "y": round(geometry.region.y), "width": round(geometry.region.width), "height": round(geometry.region.height)},
		"scale":  request.Scale, "mime_type": "image/" + request.Format, "width": width, "height": height,
		"base64_image": base64.StdEncoding.EncodeToString(encoded),
	}
	if string(target) != "null" {
		value["target"] = target
	}
	for _, key := range []string{"tag", "text"} {
		if text, ok := facts[key].(string); ok {
			value[key] = text
		}
	}
	return browserValue(value)
}
