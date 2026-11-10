// Package ocr recognizes text in images on the CPU with PaddleOCR detection,
// optional text-line orientation and recognition models. Model files are
// acquired and verified by the model registry before an engine loads them.
package ocr

import (
	"errors"
	"fmt"
	"math"
	"path/filepath"

	"github.com/sailry/sailry-harness/internal/localmodels/onnx"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// Image limits accepted by Recognize.
const (
	MaxImagePixels = 40_000_000
	MaxImageSide   = 16_384
)

// ErrCancelled reports a recognition stopped by its caller.
var ErrCancelled = errors.New("OCR cancelled")

// Line is one recognized text line with the corners of its region in image
// coordinates, clockwise from the top left.
type Line struct {
	Text       string
	Confidence float32
	Points     [4][2]float32
}

// Engine is one loaded OCR model. It is not safe for concurrent use.
type Engine struct {
	detector, recognizer, orientation *model
	dictionary                        dictionary
}

func invalid(message string) error { return errors.New("OCR model is invalid: " + message) }

type role uint8

const (
	detector role = iota
	recognizer
	orientation
)

type model struct {
	session    *onnx.Session
	role       role
	classes    int    // recognizer vocabulary size
	dimensions [2]int // fixed height and width; zero when dynamic
}

// dimensions validates an NCHW image input for a role and returns its fixed
// height and width, zero for a dynamic side.
func dimensions(role role, shape []int64) ([2]int, bool) {
	if len(shape) != 4 || shape[0] != -1 && shape[0] != 1 || shape[1] != 3 {
		return [2]int{}, false
	}
	within := func(value, low, high int64) bool { return value == -1 || value >= low && value <= high }
	fixed := func(value int64) int { return int(max(value, 0)) }
	height, width := shape[2], shape[3]
	switch {
	case role == detector && within(height, 32, 2048) && within(width, 32, 2048):
		return [2]int{fixed(height), fixed(width)}, true
	case role == recognizer && height == 48 && within(width, 32, 3200):
		return [2]int{48, fixed(width)}, true
	case role == orientation && height == 80 && width == 160:
		return [2]int{80, 160}, true
	}
	return [2]int{}, false
}

// load opens one model and validates its interface, including the shape of a
// real output for a blank image.
func load(path string, role role, classes int) (*model, error) {
	if err := onnx.Validate(path); err != nil {
		return nil, fmt.Errorf("OCR storage failed: %w", err)
	}
	session, err := onnx.NewSession(path)
	if err != nil {
		return nil, fmt.Errorf("OCR inference failed: %w", err)
	}
	loaded, err := inspect(session, role, classes)
	if err != nil {
		session.Close()
		return nil, err
	}
	return loaded, nil
}

func inspect(session *onnx.Session, role role, classes int) (*model, error) {
	if len(session.Inputs) != 1 || len(session.Outputs) != 1 {
		return nil, invalid("OCR models require one input and one output")
	}
	if session.Inputs[0].Element != onnx.ElementFloat32 || session.Outputs[0].Element != onnx.ElementFloat32 {
		return nil, invalid("OCR tensors must be float32")
	}
	if shape := session.Inputs[0].Shape; len(shape) != 4 || shape[0] != -1 && shape[0] != 1 || shape[1] != 3 {
		return nil, invalid("OCR image input must be float32 NCHW")
	}
	sides, ok := dimensions(role, session.Inputs[0].Shape)
	if !ok {
		return nil, invalid("unsupported OCR image dimensions")
	}
	loaded := &model{session: session, role: role, classes: classes, dimensions: sides}
	height, width := sides[0], sides[1]
	if height == 0 {
		height = 64
	}
	if width == 0 {
		width = 64
	}
	if _, err := loaded.run(make([]float32, 3*height*width), height, width); err != nil {
		return nil, err
	}
	return loaded, nil
}

func positive(value, limit int64) bool { return value >= 1 && value <= limit }

// run evaluates one NCHW image and validates the output shape of the role.
// Every model emits probabilities.
func (m *model) run(values []float32, height, width int) (onnx.Tensor, error) {
	outputs, err := m.session.Run([]onnx.Tensor{{
		Name: m.session.Inputs[0].Name, Shape: []int64{1, 3, int64(height), int64(width)}, Float32: values,
	}})
	if err != nil {
		return onnx.Tensor{}, fmt.Errorf("OCR inference failed: %w", err)
	}
	shape, valid := outputs[0].Shape, false
	switch m.role {
	case detector:
		valid = len(shape) == 4 && shape[0] == 1 && shape[1] == 1 && positive(shape[2], 2048) && positive(shape[3], 2048)
	case recognizer:
		valid = len(shape) == 3 && shape[0] == 1 && positive(shape[1], 3200) && shape[2] == int64(m.classes)
	case orientation:
		valid = len(shape) == 2 && shape[0] == 1 && shape[1] == 2
	}
	if !valid {
		return onnx.Tensor{}, invalid("unsupported OCR output dimensions")
	}
	for _, value := range outputs[0].Float32 {
		if math.IsNaN(float64(value)) || value < 0 || value > 1.001 {
			return onnx.Tensor{}, invalid("OCR output probabilities are invalid")
		}
	}
	return outputs[0], nil
}

// Load opens the model of a registered definition from its verified files:
// detector.onnx, recognizer.onnx, dictionary.txt and, when the definition
// prescribes text-line orientation, orientation.onnx.
func Load(definition *protocol.ModelsDefinition, directory string) (*Engine, error) {
	var format protocol.ModelsFormatPaddleOcrPayload
	switch data := definition.Format.Data.(type) {
	case protocol.ModelsFormatPaddleOcrPayload:
		format = data
	case *protocol.ModelsFormatPaddleOcrPayload:
		format = *data
	default:
		return nil, invalid("unsupported OCR family")
	}
	entries, err := loadDictionary(filepath.Join(directory, "dictionary.txt"))
	if err != nil {
		return nil, err
	}
	if err := onnx.Initialize(); err != nil {
		return nil, err
	}
	engine := &Engine{dictionary: entries}
	if engine.detector, err = load(filepath.Join(directory, "detector.onnx"), detector, 0); err != nil {
		return nil, err
	}
	if engine.recognizer, err = load(filepath.Join(directory, "recognizer.onnx"), recognizer, len(entries)); err != nil {
		engine.Close()
		return nil, err
	}
	if format.Orientation {
		if engine.orientation, err = load(filepath.Join(directory, "orientation.onnx"), orientation, 0); err != nil {
			engine.Close()
			return nil, err
		}
	}
	return engine, nil
}

// Close releases the models.
func (e *Engine) Close() {
	for _, loaded := range []*model{e.detector, e.recognizer, e.orientation} {
		if loaded != nil {
			loaded.session.Close()
		}
	}
}

// Recognize returns the text lines of an image in reading order. cancelled is
// polled between model runs.
func (e *Engine) Recognize(image *RGB, cancelled func() bool) ([]Line, error) {
	if image.W <= 0 || image.H <= 0 || image.W*image.H > MaxImagePixels || max(image.W, image.H) > MaxImageSide ||
		len(image.Pix) != image.W*image.H*3 {
		return nil, invalid("OCR image dimensions are invalid")
	}
	if cancelled() {
		return nil, ErrCancelled
	}
	height, width := detectionSize(image.W, image.H, e.detector.dimensions)
	prediction, err := e.detector.run(normalized(Resize(image, width, height), width, true, false), height, width)
	if err != nil {
		return nil, err
	}
	if cancelled() {
		return nil, ErrCancelled
	}
	boxes := boxesFromBitmap(prediction.Float32, int(prediction.Shape[3]), int(prediction.Shape[2]), image.W, image.H)
	var lines []Line
	for _, box := range sortQuadBoxes(boxes) {
		if cancelled() {
			return nil, ErrCancelled
		}
		var points [4][2]float32
		for index, point := range box {
			if point.X < 0 || point.Y < 0 || point.X > float32(image.W) || point.Y > float32(image.H) {
				return nil, invalid("detector returned invalid coordinates")
			}
			points[index] = [2]float32{point.X, point.Y}
		}
		crop, err := rotateCrop(image, box)
		if err != nil {
			return nil, invalid(err.Error())
		}
		if e.orientation != nil {
			output, err := e.orientation.run(normalized(Resize(crop, 160, 80), 160, false, false), 80, 160)
			if err != nil {
				return nil, err
			}
			if output.Float32[1] > 0.9 && output.Float32[1] > output.Float32[0] {
				crop = rotate180(crop)
			}
			if cancelled() {
				return nil, ErrCancelled
			}
		}
		content, inputWidth := recognitionSize(crop.W, crop.H, e.recognizer.dimensions[1])
		output, err := e.recognizer.run(normalized(Resize(crop, content, 48), inputWidth, true, true), 48, inputWidth)
		if err != nil {
			return nil, err
		}
		if cancelled() {
			return nil, ErrCancelled
		}
		text, confidence, err := e.dictionary.decode(output.Float32, int(output.Shape[1]))
		if err != nil {
			return nil, err
		}
		if text != "" {
			lines = append(lines, Line{Text: text, Confidence: confidence, Points: points})
		}
	}
	return lines, nil
}

// detectionSize is the detector input for an image: the longest side at most
// 960 and both sides multiples of 32, unless the model fixes them.
func detectionSize(width, height int, fixed [2]int) (int, int) {
	ratio := min(960/float32(max(width, height)), 1)
	side := func(value, fixed int) int {
		if fixed != 0 {
			return fixed
		}
		return max(int(round32(float32(value)*ratio/32))*32, 32)
	}
	return side(height, fixed[0]), side(width, fixed[1])
}

// recognitionSize returns the width a text line is resized to at height 48
// and the width of the zero-padded recognizer input.
func recognitionSize(width, height, fixed int) (int, int) {
	needed := int(math.Ceil(float64(float32(48) * float32(width) / float32(max(height, 1)))))
	input := fixed
	if input == 0 {
		input = min(max(needed, 320), 3200)
	}
	return min(max(needed, 1), input), input
}
