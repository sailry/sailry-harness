//go:build integration

package speech_test

import (
	"encoding/binary"
	"os"
	"path/filepath"
	"testing"

	"github.com/sailry/sailry-harness/internal/speech"
)

// TestTranscription decodes the SenseVoice fixture recordings with the
// pinned runtime. This file sorts after speech_test.go, whose absent-runtime
// case must run before a runtime is loaded into the process.
//
//	SAILRY_ONNXRUNTIME_DIR  directory holding the pinned ONNX Runtime and
//	                        sherpa-onnx C API libraries
//	SAILRY_SPEECH_MODEL_DIR directory holding model.int8.onnx, tokens.txt
//	                        and test_wavs/{en,zh}.wav
func TestTranscription(t *testing.T) {
	runtime, model := os.Getenv("SAILRY_ONNXRUNTIME_DIR"), os.Getenv("SAILRY_SPEECH_MODEL_DIR")
	if runtime == "" || model == "" {
		t.Skip("SAILRY_ONNXRUNTIME_DIR and SAILRY_SPEECH_MODEL_DIR are not set")
	}
	if err := speech.Load(runtime); err != nil {
		t.Fatal(err)
	}
	if err := speech.Load(runtime); err != nil {
		t.Fatalf("loading the same runtime again: %v", err)
	}
	if err := speech.Load(t.TempDir()); err == nil {
		t.Fatal("a second runtime directory was accepted")
	}
	graph, tokens := filepath.Join(model, "model.int8.onnx"), filepath.Join(model, "tokens.txt")
	english, chinese := wave(t, filepath.Join(model, "test_wavs", "en.wav")), wave(t, filepath.Join(model, "test_wavs", "zh.wav"))
	const (
		englishText = "The tribal chieftain called for the boy and presented him with 50 pieces of code."
		chineseText = "开饭时间早上9点至下午5点。"
	)
	for _, row := range []struct {
		name     string
		language speech.Language
		audio    []float32
		gain     float32
		expected string
	}{
		{"english", speech.English, english, 1, englishText},
		{"english detected", speech.Auto, english, 1, englishText},
		{"chinese", speech.Chinese, chinese, 1, chineseText},
		{"chinese detected", speech.Auto, chinese, 1, chineseText},
		{"quiet english", speech.English, english, 0.001, englishText},
		{"silence", speech.Auto, make([]float32, speech.SampleRate), 1, ""},
	} {
		recognizer, err := speech.NewRecognizer(graph, tokens, row.language)
		if err != nil {
			t.Fatalf("%s: %v", row.name, err)
		}
		audio := make([]float32, len(row.audio))
		for index, sample := range row.audio {
			audio[index] = sample * row.gain
		}
		// A recognizer decodes cumulative audio repeatedly, as a
		// dictation's partial hypotheses do.
		if _, err := recognizer.Recognize(audio[:len(audio)/2]); err != nil {
			t.Fatalf("%s: partial: %v", row.name, err)
		}
		text, err := recognizer.Recognize(audio)
		recognizer.Close()
		recognizer.Close()
		t.Logf("%s: %q", row.name, text)
		if err != nil || text != row.expected {
			t.Fatalf("%s: %q, %v", row.name, text, err)
		}
	}
}

// wave reads a mono 16-bit PCM recording and brings it to the recognizer's
// rate.
func wave(t *testing.T, path string) []float32 {
	t.Helper()
	content, err := os.ReadFile(path)
	if err != nil {
		t.Fatal(err)
	}
	if len(content) < 12 || string(content[:4]) != "RIFF" || string(content[8:12]) != "WAVE" {
		t.Fatalf("%s is not a WAVE file", path)
	}
	var (
		rate    uint32
		samples []float32
	)
	for offset := 12; offset+8 <= len(content); {
		size := int(binary.LittleEndian.Uint32(content[offset+4:]))
		body := content[offset+8 : min(offset+8+size, len(content))]
		switch string(content[offset : offset+4]) {
		case "fmt ":
			if len(body) < 16 || binary.LittleEndian.Uint16(body) != 1 || binary.LittleEndian.Uint16(body[2:]) != 1 || binary.LittleEndian.Uint16(body[14:]) != 16 {
				t.Fatalf("%s is not mono 16-bit PCM", path)
			}
			rate = binary.LittleEndian.Uint32(body[4:])
		case "data":
			for index := 0; index+2 <= len(body); index += 2 {
				samples = append(samples, float32(int16(binary.LittleEndian.Uint16(body[index:])))/32768)
			}
		}
		offset += 8 + size + size%2
	}
	audio, err := speech.Normalize(samples, rate)
	if err != nil || len(audio) == 0 {
		t.Fatalf("%s: %d samples at %d Hz: %v", path, len(samples), rate, err)
	}
	return audio
}
