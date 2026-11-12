package theme

import (
	"math"
	"testing"

	"github.com/egoist/mygo/ui"
)

// linear is an sRGB color's linear components.
func linear(c ui.Color) [3]float64 {
	var out [3]float64
	for i, v := range [3]uint8{c.R, c.G, c.B} {
		f := float64(v) / 255
		if f <= 0.04045 {
			out[i] = f / 12.92
		} else {
			out[i] = math.Pow((f+0.055)/1.055, 2.4)
		}
	}
	return out
}

// oklab is linear RGB in OKLab, where a distance is a perceived difference.
func oklab(c [3]float64) [3]float64 {
	l := math.Cbrt(0.4122214708*c[0] + 0.5363325363*c[1] + 0.0514459929*c[2])
	m := math.Cbrt(0.2119034982*c[0] + 0.6806995451*c[1] + 0.1073969566*c[2])
	s := math.Cbrt(0.0883024619*c[0] + 0.2817188376*c[1] + 0.6299787005*c[2])
	return [3]float64{
		0.2104542553*l + 0.7936177850*m - 0.0040720468*s,
		1.9779984951*l - 2.4285922050*m + 0.4505937099*s,
		0.0259040371*l + 0.7827717662*m - 0.8086757660*s,
	}
}

func distance(a, b [3]float64) float64 {
	return math.Sqrt((a[0]-b[0])*(a[0]-b[0]) + (a[1]-b[1])*(a[1]-b[1]) + (a[2]-b[2])*(a[2]-b[2]))
}

// contrast is the WCAG contrast ratio of two opaque colors.
func contrast(a, b ui.Color) float64 {
	luminance := func(c ui.Color) float64 {
		l := linear(c)
		return 0.2126*l[0] + 0.7152*l[1] + 0.0722*l[2]
	}
	hi, lo := luminance(a), luminance(b)
	if hi < lo {
		hi, lo = lo, hi
	}
	return (hi + 0.05) / (lo + 0.05)
}

// Without red or green vision, as Machado, Oliveira and Fernandes (2009)
// simulate protanopia and deuteranopia on linear RGB.
var dichromacies = map[string][3][3]float64{
	"protanopia":   {{0.152286, 1.052583, -0.204868}, {0.114503, 0.786281, 0.099216}, {-0.003882, -0.048116, 1.051998}},
	"deuteranopia": {{0.367322, 0.860646, -0.227968}, {0.280085, 0.672501, 0.047413}, {-0.011820, 0.042940, 0.968881}},
}

// testChartColors asserts the built-in chart colors are a categorical set
// in both modes, a package's own replace them, and the application's series
// start at the blue: a step of TestPaletteOverlaysNeutral.
func testChartColors(t *testing.T) {
	names := [ChartColors]string{"amber", "blue", "emerald", "violet", "rose", "cyan", "pink", "gray"}
	for _, dark := range []bool{false, true} {
		p := palette(Builtin, dark)
		popover, ground := p.Popover, p.Background
		popover.A, ground.A = 255, 255
		lightest, darkest := 0.0, 1.0
		for i, c := range p.Chart {
			lab := oklab(linear(c))
			lightest, darkest = max(lightest, lab[0]), min(darkest, lab[0])
			// Legible as a mark on the surfaces charts are drawn on.
			for surface, color := range map[string]ui.Color{"popover": popover, "background": ground} {
				if ratio := contrast(c, color); ratio < 3 {
					t.Errorf("dark=%v: %s is %.2f:1 against the %s, want 3:1", dark, names[i], ratio, surface)
				}
			}
			chroma := math.Hypot(lab[1], lab[2])
			if gray := i == ChartColors-1; gray && chroma > 0.03 || !gray && chroma < 0.1 {
				t.Errorf("dark=%v: %s has chroma %.3f", dark, names[i], chroma)
			}
			// No entry reads as the danger status.
			if d := distance(lab, oklab(linear(p.Danger))); d < 0.05 {
				t.Errorf("dark=%v: %s is %.3f from the danger color", dark, names[i], d)
			}
			for j := i + 1; j < ChartColors; j++ {
				other := oklab(linear(p.Chart[j]))
				if d := distance(lab, other); d < 0.1 {
					t.Errorf("dark=%v: %s and %s are %.3f apart, want 0.1", dark, names[i], names[j], d)
				}
				// Two hues, not two tones of one: 30 degrees apart.
				if j < ChartColors-1 {
					turn := math.Abs(math.Atan2(lab[2], lab[1])-math.Atan2(other[2], other[1])) * 180 / math.Pi
					if turn = min(turn, 360-turn); turn < 30 {
						t.Errorf("dark=%v: %s and %s are %.0f degrees of hue apart, want 30", dark, names[i], names[j], turn)
					}
				}
				for vision, matrix := range dichromacies {
					seen := func(c ui.Color) [3]float64 {
						l := linear(c)
						var out [3]float64
						for row := range matrix {
							out[row] = min(max(matrix[row][0]*l[0]+matrix[row][1]*l[1]+matrix[row][2]*l[2], 0), 1)
						}
						return oklab(out)
					}
					if d := distance(seen(c), seen(p.Chart[j])); d < 0.05 {
						t.Errorf("dark=%v: %s and %s are %.3f apart with %s, want 0.05", dark, names[i], names[j], d, vision)
					}
				}
			}
		}
		// One band of lightness, so no entry shouts.
		if lightest-darkest > 0.12 {
			t.Errorf("dark=%v: lightness spans %.3f to %.3f", dark, darkest, lightest)
		}
		// The application's own charts start at the blue and use every
		// color once before a lap toward the background.
		used := map[ui.Color]bool{}
		for i := range ChartColors {
			used[p.Series(i)] = true
		}
		if p.Series(0) != p.Chart[1] || len(used) != ChartColors || used[p.Series(ChartColors)] || p.Series(ChartColors) == p.Series(2*ChartColors) {
			t.Errorf("dark=%v: series colors %v", dark, used)
		}
	}
	// A package's chart colors replace the built-in ones it names.
	pkg := &Package{ID: "charted", Light: &Variant{}, Dark: &Variant{}}
	pkg.Light.Theme.Colors = map[string]Color{"chart.2": {V: ui.Hex("#102030"), OK: true}}
	p := palette(pkg, false)
	if p.Chart[1] != ui.Hex("#102030") || p.Chart[0] != charts[false][0] || p.Chart[7] != charts[false][7] {
		t.Errorf("a package's chart colors: %v", p.Chart)
	}
	for _, k := range chartKeys {
		if _, known := themeColorKeys[k]; !known {
			t.Errorf("%s is not a key a theme package may set", k)
		}
	}
}
