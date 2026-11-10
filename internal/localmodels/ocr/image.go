package ocr

import (
	"image"
	"image/draw"
	"math"
)

// RGB is a packed 8-bit RGB image, three bytes per pixel in row order.
type RGB struct {
	W, H int
	Pix  []uint8
}

// NewRGB returns a black image.
func NewRGB(w, h int) *RGB { return &RGB{W: w, H: h, Pix: make([]uint8, w*h*3)} }

// FromImage converts a decoded image to 8-bit RGB, dropping alpha without
// blending.
func FromImage(source image.Image) *RGB {
	bounds := source.Bounds()
	nrgba := image.NewNRGBA(image.Rect(0, 0, bounds.Dx(), bounds.Dy()))
	draw.Draw(nrgba, nrgba.Bounds(), source, bounds.Min, draw.Src)
	out := NewRGB(bounds.Dx(), bounds.Dy())
	for index := 0; index < out.W*out.H; index++ {
		copy(out.Pix[index*3:index*3+3], nrgba.Pix[index*4:index*4+3])
	}
	return out
}

func round32(value float32) float32 { return float32(math.Round(float64(value))) }

// weights returns the first source index and the normalized triangle-filter
// weights of one output coordinate. Arithmetic stays in float32 so results
// match the reference image library bit for bit.
func weights(out, size, newSize int) (int, []float32) {
	ratio := float32(size) / float32(newSize)
	sratio := ratio
	if sratio < 1 {
		sratio = 1
	}
	support := sratio // the triangle filter has unit support
	input := float32(float32(out)+0.5) * ratio
	left := int(math.Floor(float64(input - support)))
	left = min(max(left, 0), size-1)
	right := int(math.Ceil(float64(input + support)))
	right = min(max(right, left+1), size)
	input -= 0.5
	ws := make([]float32, 0, right-left)
	var sum float32
	for index := left; index < right; index++ {
		x := float32(float32(index)-input) / sratio
		if x < 0 {
			x = -x
		}
		var w float32
		if x < 1 {
			w = 1 - x
		}
		ws = append(ws, w)
		sum += w
	}
	for index := range ws {
		ws[index] /= sum
	}
	return left, ws
}

// Resize scales an image with a triangle filter: a vertical pass into
// float32, then a horizontal pass rounded to 8 bits.
func Resize(source *RGB, width, height int) *RGB {
	if source.W == width && source.H == height {
		return &RGB{W: width, H: height, Pix: append([]uint8(nil), source.Pix...)}
	}
	vertical := make([]float32, source.W*height*3)
	for y := range height {
		top, ws := weights(y, source.H, height)
		for x := range source.W {
			var t [3]float32
			for index, w := range ws {
				at := ((top+index)*source.W + x) * 3
				for channel := range 3 {
					t[channel] += float32(float32(source.Pix[at+channel]) * w)
				}
			}
			copy(vertical[(y*source.W+x)*3:], t[:])
		}
	}
	out := NewRGB(width, height)
	for x := range width {
		left, ws := weights(x, source.W, width)
		for y := range height {
			var t [3]float32
			for index, w := range ws {
				at := (y*source.W + left + index) * 3
				for channel := range 3 {
					t[channel] += float32(vertical[at+channel] * w)
				}
			}
			for channel := range 3 {
				out.Pix[(y*width+x)*3+channel] = uint8(round32(min(max(t[channel], 0), 255)))
			}
		}
	}
	return out
}

// normalized converts an image to NCHW float32 with the model's mean and
// scale, zero padded on the right to width.
func normalized(source *RGB, width int, bgr, recognition bool) []float32 {
	mean, scale := [3]float32{0.485, 0.456, 0.406}, [3]float32{0.229, 0.224, 0.225}
	if recognition {
		mean, scale = [3]float32{0.5, 0.5, 0.5}, [3]float32{0.5, 0.5, 0.5}
	}
	out := make([]float32, 3*source.H*width)
	for y := range source.H {
		for x := range source.W {
			for channel := range 3 {
				from := channel
				if bgr {
					from = 2 - channel
				}
				value := float32(source.Pix[(y*source.W+x)*3+from]) / 255
				out[(channel*source.H+y)*width+x] = float32(value-mean[channel]) / scale[channel]
			}
		}
	}
	return out
}

func rotate180(source *RGB) *RGB {
	out := NewRGB(source.W, source.H)
	count := source.W * source.H
	for index := range count {
		copy(out.Pix[(count-1-index)*3:(count-index)*3], source.Pix[index*3:index*3+3])
	}
	return out
}

// rotate270 turns an image a quarter turn counter-clockwise.
func rotate270(source *RGB) *RGB {
	out := NewRGB(source.H, source.W)
	for y := range source.H {
		for x := range source.W {
			copy(out.Pix[((source.W-1-x)*out.W+y)*3:], source.Pix[(y*source.W+x)*3:(y*source.W+x)*3+3])
		}
	}
	return out
}
