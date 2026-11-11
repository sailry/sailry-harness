package speech

import (
	"errors"
	"fmt"
	"sync"

	"github.com/ebitengine/purego"
	"github.com/sailry/sailry-harness/internal/localmodels/onnx"
	"github.com/sailry/sailry-harness/internal/shipped"
)

// The structures below are the sherpa-onnx v1.13.8 c-api.h layouts of the
// offline recognizer configuration. Every field is a pointer, an int32 or a
// float32, and the configuration is passed by pointer, so no structure
// crosses the ABI by value.
type (
	transducerConfig struct{ encoder, decoder, joiner *byte }
	whisperConfig    struct {
		encoder, decoder, language, task                 *byte
		tailPaddings, tokenTimestamps, segmentTimestamps int32
	}
	senseVoiceConfig struct {
		model, language *byte
		useITN          int32
	}
	moonshineConfig struct{ preprocessor, encoder, uncachedDecoder, cachedDecoder, mergedDecoder *byte }
	canaryConfig    struct {
		encoder, decoder, sourceLanguage, targetLanguage *byte
		usePNC                                           int32
	}
	funASRNanoConfig struct {
		encoderAdaptor, llm, embedding, tokenizer, systemPrompt, userPrompt *byte
		maxNewTokens                                                        int32
		temperature, topP                                                   float32
		seed                                                                int32
		language                                                            *byte
		itn                                                                 int32
		hotwords                                                            *byte
	}
	qwen3Config struct {
		convFrontend, encoder, decoder, tokenizer *byte
		maxTotalLength, maxNewTokens              int32
		temperature, topP                         float32
		seed                                      int32
		hotwords                                  *byte
	}
	cohereConfig struct {
		encoder, decoder, language *byte
		usePunct, useITN           int32
	}
	modelConfig struct {
		transducer                                           transducerConfig
		paraformer, nemoCTC                                  *byte
		whisper                                              whisperConfig
		tdnn                                                 *byte
		tokens                                               *byte
		threads, debug                                       int32
		provider, modelType, modelingUnit, bpeVocab, teleCTC *byte
		senseVoice                                           senseVoiceConfig
		moonshine                                            moonshineConfig
		fireRedEncoder, fireRedDecoder                       *byte
		dolphin, zipformerCTC                                *byte
		canary                                               canaryConfig
		wenetCTC, omnilingual, medASR                        *byte
		funASRNano                                           funASRNanoConfig
		fireRedCTC                                           *byte
		qwen3                                                qwen3Config
		cohere                                               cohereConfig
	}
	recognizerConfig struct {
		sampleRate, featureDim int32
		model                  modelConfig
		lmModel                *byte
		lmScale                float32
		decodingMethod         *byte
		maxActivePaths         int32
		hotwordsFile           *byte
		hotwordsScore          float32
		ruleFSTs, ruleFARs     *byte
		blankPenalty           float32
		hrDictDir, hrLexicon   *byte
		hrRuleFSTs             *byte
	}
)

var (
	mutex  sync.Mutex
	loaded string

	versionString     func() string
	createRecognizer  func(config *recognizerConfig) uintptr
	destroyRecognizer func(recognizer uintptr)
	createStream      func(recognizer uintptr) uintptr
	destroyStream     func(stream uintptr)
	acceptWaveform    func(stream uintptr, sampleRate int32, samples *float32, count int32)
	decodeStream      func(recognizer, stream uintptr)
	resultJSON        func(stream uintptr) *byte
	destroyResultJSON func(text *byte)
)

// Initialize loads the runtime packaged beside the executable, the only
// place it is loaded from.
func Initialize() error {
	directory, err := onnx.Directory()
	if err != nil {
		return fmt.Errorf("%w: %v", ErrUnavailable, err)
	}
	return Load(directory)
}

// Load opens the pinned sherpa-onnx C API library in directory. The pinned
// onnxruntime of the same directory is opened first, so that sherpa-onnx
// binds to the image every other consumer uses. The process keeps one
// runtime: a second directory is refused.
func Load(directory string) error {
	mutex.Lock()
	defer mutex.Unlock()
	path, expected, err := pinned(directory)
	if err != nil {
		return err
	}
	if loaded != "" {
		if loaded != path {
			return errors.New("sherpa-onnx is already loaded from another library")
		}
		return nil
	}
	if err := onnx.Load(directory); err != nil {
		return fmt.Errorf("%w: %v", ErrUnavailable, err)
	}
	// Signing a macOS application rewrites the library; the package seal
	// then records it instead of the upstream lock.
	seal, err := shipped.Sealed(directory)
	if err != nil {
		return fmt.Errorf("%w: %v", ErrUnavailable, err)
	}
	if seal != nil {
		record, listed := seal.File(expected.name)
		if !listed {
			return fmt.Errorf("%w: %s is not in the package seal", ErrUnavailable, expected.name)
		}
		expected.size, expected.sha256 = record.Size, record.SHA256
	}
	if err := verify(path, expected); err != nil {
		return err
	}
	handle, err := openLibrary(path)
	if err != nil {
		return fmt.Errorf("%w: %v", ErrUnavailable, err)
	}
	for _, binding := range []struct {
		name   string
		target any
	}{
		{"SherpaOnnxGetVersionStr", &versionString},
		{"SherpaOnnxCreateOfflineRecognizer", &createRecognizer},
		{"SherpaOnnxDestroyOfflineRecognizer", &destroyRecognizer},
		{"SherpaOnnxCreateOfflineStream", &createStream},
		{"SherpaOnnxDestroyOfflineStream", &destroyStream},
		{"SherpaOnnxAcceptWaveformOffline", &acceptWaveform},
		{"SherpaOnnxDecodeOfflineStream", &decodeStream},
		{"SherpaOnnxGetOfflineStreamResultAsJson", &resultJSON},
		{"SherpaOnnxDestroyOfflineStreamResultJson", &destroyResultJSON},
	} {
		symbol, err := lookup(handle, binding.name)
		if err != nil {
			return fmt.Errorf("%w: %s: %v", ErrUnavailable, binding.name, err)
		}
		purego.RegisterFunc(binding.target, symbol)
	}
	if version := versionString(); version != Version {
		return fmt.Errorf("%w: sherpa-onnx %s does not match pinned %s", ErrUnavailable, version, Version)
	}
	loaded = path
	return nil
}
