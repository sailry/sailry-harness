package ocr

import (
	"errors"
	"math"
	"sort"
)

func distance(a, b Point) float32 {
	return float32(math.Hypot(float64(a.X-b.X), float64(a.Y-b.Y)))
}

func orientVertical(image *RGB) *RGB {
	if float32(image.H) >= float32(image.W)*1.5 {
		return rotate270(image)
	}
	return image
}

// rotateCrop cuts a quadrilateral region out of an image and rectifies it
// with a perspective transform. Tall results are turned to read
// horizontally.
func rotateCrop(source *RGB, box []Point) (*RGB, error) {
	if len(box) != 4 {
		return nil, errors.New("box must contain exactly 4 points")
	}
	minX, minY := float32(math.Inf(1)), float32(math.Inf(1))
	maxX, maxY := float32(math.Inf(-1)), float32(math.Inf(-1))
	for _, p := range box {
		minX, maxX, minY, maxY = min(minX, p.X), max(maxX, p.X), min(minY, p.Y), max(maxY, p.Y)
	}
	left, top := int(max(minX, 0)), int(max(minY, 0))
	right, bottom := int(min(maxX, float32(source.W))), int(min(maxY, float32(source.H)))
	if right <= left || bottom <= top {
		return nil, errors.New("invalid crop region")
	}
	width, height := right-left, bottom-top
	crop := NewRGB(width, height)
	for y := range height {
		copy(crop.Pix[y*width*3:(y+1)*width*3], source.Pix[((top+y)*source.W+left)*3:])
	}
	sorted := make([]Point, 4)
	for index, p := range box {
		sorted[index] = Point{p.X - float32(left), p.Y - float32(top)}
	}
	sort.SliceStable(sorted, func(i, j int) bool { return sorted[i].X < sorted[j].X })
	a, d := 0, 1
	if sorted[1].Y < sorted[0].Y {
		a, d = 1, 0
	}
	b, c := 2, 3
	if sorted[3].Y < sorted[2].Y {
		b, c = 3, 2
	}
	ordered := [4]Point{sorted[a], sorted[b], sorted[c], sorted[d]}
	w, h := float32(width), float32(height)
	if ordered == [4]Point{{0, 0}, {w, 0}, {w, h}, {0, h}} {
		return orientVertical(crop), nil
	}
	targetW := int(round32(max(distance(ordered[0], ordered[1]), distance(ordered[2], ordered[3]))))
	targetH := int(round32(max(distance(ordered[0], ordered[3]), distance(ordered[1], ordered[2]))))
	if targetW == 0 || targetH == 0 {
		return nil, errors.New("invalid crop dimensions")
	}
	target := [4]Point{{0, 0}, {float32(targetW), 0}, {float32(targetW), float32(targetH)}, {0, float32(targetH)}}
	matrix, err := perspective(ordered, target)
	if err != nil {
		return nil, err
	}
	inverse, err := invert3(matrix)
	if err != nil {
		return nil, err
	}
	out := NewRGB(targetW, targetH)
	for y := range targetH {
		for x := range targetW {
			fx, fy := float32(x), float32(y)
			sx := float32(inverse[0]*fx) + float32(inverse[1]*fy) + inverse[2]
			sy := float32(inverse[3]*fx) + float32(inverse[4]*fy) + inverse[5]
			sz := float32(inverse[6]*fx) + float32(inverse[7]*fy) + inverse[8]
			at := (y*targetW + x) * 3
			if float32(math.Abs(float64(sz))) > 1.1920929e-07 {
				bicubic(crop, sx/sz, sy/sz, out.Pix[at:at+3])
			} else {
				copy(out.Pix[at:at+3], crop.Pix[:3])
			}
		}
	}
	return orientVertical(out), nil
}

// perspective solves the 8x8 homography system with partially pivoted LU in float32.
func perspective(source, target [4]Point) ([9]float32, error) {
	var a [8][9]float32
	for index := range 4 {
		s, t := source[index], target[index]
		a[index*2] = [9]float32{s.X, s.Y, 1, 0, 0, 0, -s.X * t.X, -s.Y * t.X, t.X}
		a[index*2+1] = [9]float32{0, 0, 0, s.X, s.Y, 1, -s.X * t.Y, -s.Y * t.Y, t.Y}
	}
	for column := range 8 {
		pivot := column
		for row := column + 1; row < 8; row++ {
			if math.Abs(float64(a[row][column])) > math.Abs(float64(a[pivot][column])) {
				pivot = row
			}
		}
		if a[pivot][column] == 0 {
			return [9]float32{}, errors.New("cannot solve perspective transformation")
		}
		a[column], a[pivot] = a[pivot], a[column]
		for row := column + 1; row < 8; row++ {
			factor := a[row][column] / a[column][column]
			for k := column; k < 9; k++ {
				a[row][k] -= float32(factor * a[column][k])
			}
		}
	}
	var solution [9]float32
	for row := 7; row >= 0; row-- {
		value := a[row][8]
		for k := row + 1; k < 8; k++ {
			value -= float32(a[row][k] * solution[k])
		}
		solution[row] = value / a[row][row]
	}
	solution[8] = 1
	return solution, nil
}

func invert3(m [9]float32) ([9]float32, error) {
	c := func(a, b, c, d int) float32 { return float32(m[a]*m[b]) - float32(m[c]*m[d]) }
	cofactors := [9]float32{
		c(4, 8, 5, 7), c(2, 7, 1, 8), c(1, 5, 2, 4),
		c(5, 6, 3, 8), c(0, 8, 2, 6), c(2, 3, 0, 5),
		c(3, 7, 4, 6), c(1, 6, 0, 7), c(0, 4, 1, 3),
	}
	determinant := float32(m[0]*cofactors[0]) + float32(m[1]*cofactors[3]) + float32(m[2]*cofactors[6])
	if determinant == 0 {
		return [9]float32{}, errors.New("cannot invert transformation matrix")
	}
	for index := range cofactors {
		cofactors[index] /= determinant
	}
	return cofactors, nil
}

func cubic(t float32) float32 {
	const a = -0.5
	if t < 0 {
		t = -t
	}
	switch {
	case t <= 1:
		return float32(float32(float32((a+2)*t)*t)*t) - float32(float32((a+3)*t)*t) + 1
	case t < 2:
		return float32(float32(float32(a*t)*t)*t) - float32(float32(5*a*t)*t) + float32(8*a*t) - 4*a
	}
	return 0
}

// bicubic samples with a Catmull-Rom kernel and border replication.
func bicubic(image *RGB, x, y float32, out []uint8) {
	xi, yi := int(math.Floor(float64(x))), int(math.Floor(float64(y)))
	dx, dy := x-float32(xi), y-float32(yi)
	wx := [4]float32{cubic(dx + 1), cubic(dx), cubic(dx - 1), cubic(dx - 2)}
	wy := [4]float32{cubic(dy + 1), cubic(dy), cubic(dy - 1), cubic(dy - 2)}
	var result [3]float32
	for j, weightY := range wy {
		row := min(max(yi-1+j, 0), image.H-1) * image.W
		for i, weightX := range wx {
			weight := float32(weightX * weightY)
			at := (row + min(max(xi-1+i, 0), image.W-1)) * 3
			for channel := range 3 {
				result[channel] += float32(weight * float32(image.Pix[at+channel]))
			}
		}
	}
	for channel := range 3 {
		out[channel] = uint8(min(max(round32(result[channel]), 0), 255))
	}
}
