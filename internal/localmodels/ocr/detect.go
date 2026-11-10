package ocr

import (
	"math"
	"sort"
)

// Point is a float32 image coordinate.
type Point struct{ X, Y float32 }

// Differentiable-binarization post-processing parameters (the PaddleOCR
// defaults).
const (
	dbThresh      = 0.3
	dbBoxThresh   = 0.6
	dbCandidates  = 1000
	dbUnclipRatio = 1.5
	dbMinSize     = 3.0
)

// findContours follows every outer and hole border of a binary mask
// (Suzuki-Abe border following) and returns their points in order.
func findContours(mask []bool, width, height int) [][]Point {
	values := make([]int32, width*height)
	for index, set := range mask {
		if set {
			values[index] = 1
		}
	}
	at := func(x, y int) int { return x + width*y }
	nonZero := func(x, y int) bool {
		return x > -1 && x < width && y > -1 && y < height && values[at(x, y)] != 0
	}
	diffs := [][2]int{{-1, 0}, {-1, -1}, {0, -1}, {1, -1}, {1, 0}, {1, 1}, {0, 1}, {-1, 1}}
	rotateTo := func(dx, dy int) {
		for index, diff := range diffs {
			if diff == [2]int{dx, dy} {
				diffs = append(append([][2]int{}, diffs[index:]...), diffs[:index]...)
				return
			}
		}
	}
	var contours [][]Point
	border := int32(1)
	for y := range height {
		for x := range width {
			if values[at(x, y)] == 0 {
				continue
			}
			adjacent, found := 0, false
			if values[at(x, y)] == 1 && x > 0 && values[at(x-1, y)] == 0 {
				adjacent, found = x-1, true
			} else if values[at(x, y)] > 0 && x+1 < width && values[at(x+1, y)] == 0 {
				adjacent, found = x+1, true
			}
			if !found {
				continue
			}
			border++
			var points []Point
			rotateTo(adjacent-x, 0)
			first, ok := [2]int{}, false
			for _, diff := range diffs {
				if nonZero(x+diff[0], y+diff[1]) {
					first, ok = [2]int{x + diff[0], y + diff[1]}, true
					break
				}
			}
			if !ok {
				points = append(points, Point{float32(x), float32(y)})
				values[at(x, y)] = -border
				contours = append(contours, points)
				continue
			}
			start := [2]int{x, y}
			previous, current := first, start
			for {
				points = append(points, Point{float32(current[0]), float32(current[1])})
				rotateTo(previous[0]-current[0], previous[1]-current[1])
				var next [2]int
				for index := len(diffs) - 1; index >= 0; index-- { // counter-clockwise
					if nonZero(current[0]+diffs[index][0], current[1]+diffs[index][1]) {
						next = [2]int{current[0] + diffs[index][0], current[1] + diffs[index][1]}
						break
					}
				}
				rightEdge := false
				for index := len(diffs) - 1; index >= 0; index-- {
					if diffs[index] == [2]int{next[0] - current[0], next[1] - current[1]} {
						break
					}
					if diffs[index] == [2]int{1, 0} {
						rightEdge = true
						break
					}
				}
				if current[0]+1 == width || rightEdge {
					values[at(current[0], current[1])] = -border
				} else if values[at(current[0], current[1])] == 1 {
					values[at(current[0], current[1])] = border
				}
				if next == start && current == first {
					break
				}
				previous, current = current, next
			}
			contours = append(contours, points)
		}
	}
	return contours
}

func signStep(value float32) int {
	switch {
	case value > 0:
		return 1
	case value < 0:
		return -1
	}
	return 0
}

// simplifyChain drops points inside straight runs of a pixel chain.
func simplifyChain(points []Point) []Point {
	if len(points) <= 2 {
		return points
	}
	var simplified []Point
	n := len(points)
	for index := range n {
		previous, current, next := points[(index+n-1)%n], points[index], points[(index+1)%n]
		if signStep(current.X-previous.X) != signStep(next.X-current.X) || signStep(current.Y-previous.Y) != signStep(next.Y-current.Y) {
			simplified = append(simplified, current)
		}
	}
	if len(simplified) < 3 {
		return points
	}
	return simplified
}

func cross(a, b, c Point) float32 {
	return float32((b.X-a.X)*(c.Y-a.Y)) - float32((b.Y-a.Y)*(c.X-a.X))
}

func atan2(y, x float32) float32 { return float32(math.Atan2(float64(y), float64(x))) }

func convexHull(source []Point) []Point {
	if len(source) < 3 {
		return source
	}
	points := append([]Point(nil), source...)
	start := 0
	for index := 1; index < len(points); index++ {
		if points[index].Y < points[start].Y || points[index].Y == points[start].Y && points[index].X < points[start].X {
			start = index
		}
	}
	points[0], points[start] = points[start], points[0]
	origin := points[0]
	rest := points[1:]
	sort.SliceStable(rest, func(i, j int) bool {
		a, b := rest[i], rest[j]
		angleA, angleB := atan2(a.Y-origin.Y, a.X-origin.X), atan2(b.Y-origin.Y, b.X-origin.X)
		if angleA != angleB {
			return angleA < angleB
		}
		distanceA := float32((a.X-origin.X)*(a.X-origin.X)) + float32((a.Y-origin.Y)*(a.Y-origin.Y))
		distanceB := float32((b.X-origin.X)*(b.X-origin.X)) + float32((b.Y-origin.Y)*(b.Y-origin.Y))
		return distanceA < distanceB
	})
	hull := make([]Point, 0, len(points))
	for _, point := range points {
		for len(hull) > 1 && cross(hull[len(hull)-2], hull[len(hull)-1], point) <= 0 {
			hull = hull[:len(hull)-1]
		}
		hull = append(hull, point)
	}
	return hull
}

type minRect struct {
	center        Point
	width, height float32
	angle         float32 // degrees
}

// minAreaRect returns the minimum-area rectangle around points by rotating
// calipers over their convex hull.
func minAreaRect(source []Point) minRect {
	if len(source) < 3 {
		return minRect{}
	}
	hull := convexHull(source)
	if len(hull) < 3 {
		minX, minY := float32(math.Inf(1)), float32(math.Inf(1))
		maxX, maxY := float32(math.Inf(-1)), float32(math.Inf(-1))
		for _, p := range source {
			minX, maxX, minY, maxY = min(minX, p.X), max(maxX, p.X), min(minY, p.Y), max(maxY, p.Y)
		}
		return minRect{center: Point{(minX + maxX) * 0.5, (minY + maxY) * 0.5}, width: maxX - minX, height: maxY - minY}
	}
	minArea := float32(math.MaxFloat32)
	var best minRect
	for index := range hull {
		next := hull[(index+1)%len(hull)]
		edgeX, edgeY := next.X-hull[index].X, next.Y-hull[index].Y
		lengthSquared := float32(edgeX*edgeX) + float32(edgeY*edgeY)
		if lengthSquared < 1.1920929e-07 {
			continue
		}
		inverse := 1 / float32(math.Sqrt(float64(lengthSquared)))
		nx, ny := edgeX*inverse, edgeY*inverse
		px, py := -ny, nx
		minN, maxN := float32(math.MaxFloat32), float32(-math.MaxFloat32)
		minP, maxP := float32(math.MaxFloat32), float32(-math.MaxFloat32)
		for _, point := range hull {
			dx, dy := point.X-hull[index].X, point.Y-hull[index].Y
			projectedN := float32(nx*dx) + float32(ny*dy)
			projectedP := float32(px*dx) + float32(py*dy)
			minN, maxN, minP, maxP = min(minN, projectedN), max(maxN, projectedN), min(minP, projectedP), max(maxP, projectedP)
		}
		width, height := maxN-minN, maxP-minP
		if area := float32(width * height); area < minArea {
			minArea = area
			centerN, centerP := (minN+maxN)*0.5, (minP+maxP)*0.5
			best = minRect{
				center: Point{
					hull[index].X + float32(centerN*nx) + float32(centerP*px),
					hull[index].Y + float32(centerN*ny) + float32(centerP*py),
				},
				width: width, height: height,
				angle: atan2(ny, nx) * 180 / math.Pi,
			}
		}
	}
	return best
}

// miniBox returns the corners of the minimum-area rectangle, clockwise from
// the top left, and the length of its short side.
func miniBox(points []Point) ([]Point, float32, bool) {
	if len(points) < 3 {
		return nil, 0, false
	}
	rect := minAreaRect(points)
	side := min(rect.width, rect.height)
	if math.IsNaN(float64(side)) || math.IsInf(float64(side), 0) || side <= 0 {
		return nil, 0, false
	}
	radians := float64(rect.angle * math.Pi / 180)
	cos, sin := float32(math.Cos(radians)), float32(math.Sin(radians))
	halfW, halfH := rect.width/2, rect.height/2
	corners := make([]Point, 0, 4)
	for _, corner := range [][2]float32{{-halfW, -halfH}, {halfW, -halfH}, {halfW, halfH}, {-halfW, halfH}} {
		corners = append(corners, Point{
			float32(corner[0]*cos) - float32(corner[1]*sin) + rect.center.X,
			float32(corner[0]*sin) + float32(corner[1]*cos) + rect.center.Y,
		})
	}
	sort.SliceStable(corners, func(i, j int) bool { return corners[i].X < corners[j].X })
	first, fourth := 1, 0
	if corners[1].Y > corners[0].Y {
		first, fourth = 0, 1
	}
	second, third := 3, 2
	if corners[3].Y > corners[2].Y {
		second, third = 2, 3
	}
	return []Point{corners[first], corners[second], corners[third], corners[fourth]}, side, true
}

// boxScoreFast is the mean probability inside a polygon.
func boxScoreFast(pred []float32, width, height int, box []Point) float32 {
	minX, minY := float32(math.Inf(1)), float32(math.Inf(1))
	maxX, maxY := float32(math.Inf(-1)), float32(math.Inf(-1))
	for _, p := range box {
		minX, maxX, minY, maxY = min(minX, p.X), max(maxX, p.X), min(minY, p.Y), max(maxY, p.Y)
	}
	clampX := func(value float32) int { return int(min(max(value, 0), float32(width)-1)) }
	clampY := func(value float32) int { return int(min(max(value, 0), float32(height)-1)) }
	startX, endX := clampX(float32(math.Floor(float64(minX)))), clampX(float32(math.Ceil(float64(maxX))))+1
	startY, endY := clampY(float32(math.Floor(float64(minY)))), clampY(float32(math.Ceil(float64(maxY))))+1
	var total float32
	pixels := 0
	// Large regions are summed per row and small ones in one pass, the
	// summation orders of the reference implementation.
	sequential := (endY-startY)*(endX-startX) < 8000
	intersections := make([]float32, 0, len(box))
	for y := startY; y < endY; y++ {
		scan := float32(y) + 0.5
		intersections = intersections[:0]
		for index := range box {
			p1, p2 := box[index], box[(index+1)%len(box)]
			if (p1.Y <= scan && scan < p2.Y || p2.Y <= scan && scan < p1.Y) && float32(math.Abs(float64(p2.Y-p1.Y))) > 1.1920929e-07 {
				intersections = append(intersections, p1.X+float32(float32((scan-p1.Y)*(p2.X-p1.X))/(p2.Y-p1.Y)))
			}
		}
		sort.SliceStable(intersections, func(i, j int) bool { return intersections[i] < intersections[j] })
		var line float32
		for index := 0; index+1 < len(intersections); index += 2 {
			x1 := int(max(intersections[index], float32(startX)))
			x2 := int(min(intersections[index+1], float32(endX)))
			if x1 < x2 && x1 >= startX && x2 <= endX {
				for x := x1; x < min(x2, width); x++ {
					if sequential {
						total += pred[y*width+x]
					} else {
						line += pred[y*width+x]
					}
				}
				pixels += max(min(x2, width)-x1, 0)
			}
		}
		total += line
	}
	if pixels == 0 {
		return 0
	}
	return total / float32(pixels)
}

// unclip expands a quadrilateral by area*ratio/perimeter on every side.
//
// The reference offsets the polygon with a polygon-clipping library (round
// joins, two decimal digits) and takes the minimum-area rectangle of the
// result. For a convex quadrilateral that rectangle is spanned by the offset
// edges, so this returns the endpoints of the four offset edges rounded to
// two digits (docs/models.md, Deviations).
func unclip(box []Point, ratio float64) []Point {
	n := len(box)
	xs, ys := make([]float64, n), make([]float64, n)
	for index, p := range box {
		// Coordinates are kept as integers in hundredths.
		xs[index], ys[index] = math.Round(float64(p.X)*100), math.Round(float64(p.Y)*100)
	}
	area, perimeter := 0.0, 0.0
	for index := range n {
		next := (index + 1) % n
		area += float64(box[index].X)*float64(box[next].Y) - float64(box[next].X)*float64(box[index].Y)
		perimeter += math.Hypot(float64(box[next].X)-float64(box[index].X), float64(box[next].Y)-float64(box[index].Y))
	}
	area = math.Abs(area) / 2
	if area <= 2.220446049250313e-16 || perimeter <= 2.220446049250313e-16 {
		return nil
	}
	delta := area * ratio / perimeter * 100
	// Outward normals depend on the winding of the scaled path.
	signed := 0.0
	for index := range n {
		next := (index + 1) % n
		signed += xs[index]*ys[next] - xs[next]*ys[index]
	}
	orientation := 1.0
	if signed < 0 {
		orientation = -1
	}
	out := make([]Point, 0, 2*n)
	for index := range n {
		next := (index + 1) % n
		dx, dy := xs[next]-xs[index], ys[next]-ys[index]
		length := math.Hypot(dx, dy)
		if length == 0 {
			continue
		}
		nx, ny := orientation*dy/length*delta, -orientation*dx/length*delta
		out = append(out,
			Point{float32(math.Round(xs[index]+nx) / 100), float32(math.Round(ys[index]+ny) / 100)},
			Point{float32(math.Round(xs[next]+nx) / 100), float32(math.Round(ys[next]+ny) / 100)},
		)
	}
	if len(out) < 3 {
		return nil
	}
	return out
}

// boxesFromBitmap turns a probability map into text-region quadrilaterals in
// source image coordinates.
func boxesFromBitmap(pred []float32, width, height, sourceWidth, sourceHeight int) [][]Point {
	mask := make([]bool, len(pred))
	for index, value := range pred {
		mask[index] = value > dbThresh
	}
	widthScale, heightScale := float32(sourceWidth)/float32(width), float32(sourceHeight)/float32(height)
	var boxes [][]Point
	contours := findContours(mask, width, height)
	if len(contours) > dbCandidates {
		contours = contours[:dbCandidates]
	}
	for _, contour := range contours {
		candidate := contour
		if simplified := simplifyChain(contour); len(simplified) >= 3 {
			candidate = simplified
		}
		mini, side, ok := miniBox(candidate)
		if !ok || side < dbMinSize {
			continue
		}
		if boxScoreFast(pred, width, height, mini) < dbBoxThresh {
			continue
		}
		expanded := unclip(mini, dbUnclipRatio)
		if len(expanded) == 0 {
			continue
		}
		points, side, ok := miniBox(expanded)
		if !ok || side < dbMinSize+2 {
			continue
		}
		scaled := make([]Point, len(points))
		for index, point := range points {
			scaled[index] = Point{
				min(max(round32(point.X*widthScale), 0), float32(sourceWidth)),
				min(max(round32(point.Y*heightScale), 0), float32(sourceHeight)),
			}
		}
		boxes = append(boxes, scaled)
	}
	return boxes
}

func xMin(box []Point) float32 {
	value := float32(math.Inf(1))
	for _, p := range box {
		value = min(value, p.X)
	}
	return value
}

func yMin(box []Point) float32 {
	value := float32(math.Inf(1))
	for _, p := range box {
		value = min(value, p.Y)
	}
	return value
}

// sortQuadBoxes orders regions top to bottom, and left to right within a
// line of regions whose tops differ by less than ten pixels.
func sortQuadBoxes(boxes [][]Point) [][]Point {
	sorted := append([][]Point(nil), boxes...)
	sort.SliceStable(sorted, func(i, j int) bool {
		if yMin(sorted[i]) != yMin(sorted[j]) {
			return yMin(sorted[i]) < yMin(sorted[j])
		}
		return xMin(sorted[i]) < xMin(sorted[j])
	})
	for i := 0; i+1 < len(sorted); i++ {
		for j := i; j >= 0; j-- {
			if float32(math.Abs(float64(yMin(sorted[j+1])-yMin(sorted[j])))) < 10 && xMin(sorted[j+1]) < xMin(sorted[j]) {
				sorted[j], sorted[j+1] = sorted[j+1], sorted[j]
			} else {
				break
			}
		}
	}
	return sorted
}
