package speech

import (
	"encoding/json"
	"runtime"
	"strings"
	"unsafe"
)

// Recognizer is one loaded SenseVoice model, reused for the successive
// decodes of a recording. It is not safe for concurrent use.
type Recognizer struct {
	handle uintptr
}

func cString(value string) *byte {
	data := make([]byte, len(value)+1)
	copy(data, value)
	return &data[0]
}

// NewRecognizer loads a SenseVoice graph and its vocabulary after Load. A
// path that cannot be a C string, or files sherpa-onnx rejects, are
// ErrInvalidModel.
func NewRecognizer(model, tokens string, language Language) (*Recognizer, error) {
	mutex.Lock()
	ready := loaded != ""
	mutex.Unlock()
	if !ready {
		return nil, ErrUnavailable
	}
	if strings.ContainsRune(model, 0) || strings.ContainsRune(tokens, 0) {
		return nil, ErrInvalidModel
	}
	config := recognizerConfig{sampleRate: SampleRate, featureDim: 80, maxActivePaths: 4}
	config.model.senseVoice = senseVoiceConfig{model: cString(model), language: cString(language.Code()), useITN: 1}
	config.model.tokens = cString(tokens)
	config.model.threads = int32(min(max(runtime.NumCPU(), 1), 4))
	config.model.provider = cString("cpu")
	var pinner runtime.Pinner
	defer pinner.Unpin()
	for _, pointer := range []*byte{config.model.senseVoice.model, config.model.senseVoice.language, config.model.tokens, config.model.provider} {
		pinner.Pin(pointer)
	}
	handle := createRecognizer(&config)
	runtime.KeepAlive(&config)
	if handle == 0 {
		return nil, ErrInvalidModel
	}
	return &Recognizer{handle: handle}, nil
}

// Close releases the model. The recognizer must not be used afterwards.
func (r *Recognizer) Close() {
	if r.handle != 0 {
		destroyRecognizer(r.handle)
		r.handle = 0
	}
}

// Recognize decodes cumulative mono audio at SampleRate; each result
// replaces the previous hypothesis. Silence is the empty text.
func (r *Recognizer) Recognize(samples []float32) (string, error) {
	if len(samples) > MaxSamples || !finite(samples) {
		return "", ErrInvalidAudio
	}
	if Silent(samples) {
		return "", nil
	}
	stream := createStream(r.handle)
	if stream == 0 {
		return "", ErrRecognition
	}
	defer destroyStream(stream)
	acceptWaveform(stream, SampleRate, &samples[0], int32(len(samples)))
	decodeStream(r.handle, stream)
	text := resultJSON(stream)
	if text == nil {
		return "", ErrRecognition
	}
	defer destroyResultJSON(text)
	length := 0
	for *(*byte)(unsafe.Add(unsafe.Pointer(text), length)) != 0 {
		length++
	}
	var result struct {
		Text string `json:"text"`
	}
	if err := json.Unmarshal(unsafe.Slice(text, length), &result); err != nil {
		return "", ErrRecognition
	}
	return strings.TrimSpace(result.Text), nil
}
