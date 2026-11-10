// Package capture records a microphone through a purego binding of the
// miniaudio shim built from native/capture. No cgo is used and the audio
// thread never calls into Go. Audio stays in memory on the controller.
package capture

import (
	"errors"
	"fmt"
	"math"
	"os"
	"path/filepath"
	"runtime"
	"strings"
	"sync"

	"github.com/ebitengine/purego"
)

// Version is the pinned miniaudio release compiled into the library.
const Version = "0.11.26"

// ErrUnavailable reports that the capture library is absent or not the
// pinned build.
var ErrUnavailable = errors.New("microphone capture library is unavailable")

var (
	mutex  sync.Mutex
	loaded string

	versionString func() string
	listDevices   func(output *byte, capacity uintptr) int32
	openDevice    func(device *byte, result *int32) uintptr
	sampleRate    func(capture uintptr) uint32
	channels      func(capture uintptr) uint32
	dropped       func(capture uintptr) uint32
	failed        func(capture uintptr) uint32
	startDevice   func(capture uintptr) int32
	stopDevice    func(capture uintptr) int32
	readFrames    func(capture uintptr, output *float32, frames uint32) uint32
	closeDevice   func(capture uintptr)
)

// LibraryName is the capture library's file name on this system.
func LibraryName() string {
	switch runtime.GOOS {
	case "windows":
		return "sailry-capture.dll"
	case "darwin":
		return "libsailry-capture.dylib"
	}
	return "libsailry-capture.so"
}

// Load opens the capture library in directory by its absolute path. The
// process keeps one library; a second directory is an error.
func Load(directory string) error {
	mutex.Lock()
	defer mutex.Unlock()
	path, err := filepath.Abs(filepath.Join(directory, LibraryName()))
	if err != nil {
		return fmt.Errorf("%w: %v", ErrUnavailable, err)
	}
	if loaded != "" {
		if loaded != path {
			return errors.New("microphone capture is already loaded from another library")
		}
		return nil
	}
	if info, err := os.Lstat(path); err != nil || !info.Mode().IsRegular() {
		return fmt.Errorf("%w: %s is not an ordinary file", ErrUnavailable, LibraryName())
	}
	handle, err := openLibrary(path)
	if err != nil {
		return fmt.Errorf("%w: %v", ErrUnavailable, err)
	}
	for _, binding := range []struct {
		name   string
		target any
	}{
		{"sailry_capture_version", &versionString}, {"sailry_capture_devices", &listDevices},
		{"sailry_capture_open", &openDevice}, {"sailry_capture_sample_rate", &sampleRate},
		{"sailry_capture_channels", &channels}, {"sailry_capture_dropped", &dropped},
		{"sailry_capture_failed", &failed}, {"sailry_capture_start", &startDevice},
		{"sailry_capture_stop", &stopDevice}, {"sailry_capture_read", &readFrames},
		{"sailry_capture_close", &closeDevice},
	} {
		symbol, err := lookup(handle, binding.name)
		if err != nil {
			return fmt.Errorf("%w: %s: %v", ErrUnavailable, binding.name, err)
		}
		purego.RegisterFunc(binding.target, symbol)
	}
	if got := versionString(); got != Version {
		return fmt.Errorf("%w: miniaudio %s does not match pinned %s", ErrUnavailable, got, Version)
	}
	loaded = path
	return nil
}

// Device is one input device. ID is stable for the audio backend in use and
// is what the dictation.microphone preference stores.
type Device struct {
	ID      string
	Name    string
	Default bool
}

// Devices lists the input devices of the default audio backend.
func Devices() ([]Device, error) {
	buffer := make([]byte, 256*1024)
	if count := listDevices(&buffer[0], uintptr(len(buffer))); count < 0 {
		return nil, fmt.Errorf("input devices could not be listed (miniaudio result %d)", count)
	}
	text, _, _ := strings.Cut(string(buffer), "\x00")
	return parseDevices(text), nil
}

func parseDevices(text string) []Device {
	var list []Device
	for line := range strings.Lines(text) {
		fields := strings.SplitN(strings.TrimSuffix(line, "\n"), "\t", 3)
		if len(fields) == 3 && fields[1] != "" {
			list = append(list, Device{ID: fields[1], Name: fields[2], Default: fields[0] == "1"})
		}
	}
	return list
}

// Recorder is one open input device delivering interleaved float32 frames at
// the device's own rate and channel count.
type Recorder struct {
	handle     uintptr
	SampleRate uint32
	Channels   int
}

// Open opens the device named by id, or the system default for "".
func Open(id string) (*Recorder, error) {
	var device *byte
	if id != "" {
		text := append([]byte(id), 0)
		device = &text[0]
	}
	var result int32
	handle := openDevice(device, &result)
	runtime.KeepAlive(device)
	if handle == 0 {
		return nil, fmt.Errorf("microphone could not be opened (miniaudio result %d)", result)
	}
	recorder := &Recorder{handle: handle, SampleRate: sampleRate(handle), Channels: int(channels(handle))}
	if recorder.SampleRate == 0 || recorder.Channels == 0 {
		recorder.Close()
		return nil, errors.New("microphone reports no audio format")
	}
	return recorder, nil
}

func (r *Recorder) Start() error {
	if result := startDevice(r.handle); result != 0 {
		return fmt.Errorf("microphone could not be started (miniaudio result %d)", result)
	}
	return nil
}

func (r *Recorder) Stop() error {
	if result := stopDevice(r.handle); result != 0 {
		return fmt.Errorf("microphone could not be stopped (miniaudio result %d)", result)
	}
	return nil
}

// Read copies the frames captured so far into buffer and returns the frame
// count; buffer holds Channels samples per frame.
func (r *Recorder) Read(buffer []float32) int {
	if len(buffer) < r.Channels {
		return 0
	}
	return int(readFrames(r.handle, &buffer[0], uint32(len(buffer)/r.Channels)))
}

// Failed reports that the device stopped by itself, as when it is unplugged.
func (r *Recorder) Failed() bool { return failed(r.handle) != 0 }

// Dropped is the number of frames discarded because the reader fell behind.
func (r *Recorder) Dropped() int { return int(dropped(r.handle)) }

func (r *Recorder) Close() {
	if r.handle != 0 {
		closeDevice(r.handle)
		r.handle = 0
	}
}

// Level is the loudness of a block of samples for a level meter, 0 to 1:
// the root mean square on a 60 dB scale, so that speech moves the meter and
// room noise barely does.
func Level(samples []float32) float32 {
	if len(samples) == 0 {
		return 0
	}
	var sum float64
	for _, sample := range samples {
		value := float64(sample)
		if math.IsNaN(value) || math.IsInf(value, 0) {
			continue
		}
		sum += value * value
	}
	mean := math.Sqrt(sum / float64(len(samples)))
	if mean <= 0.001 {
		return 0
	}
	return float32(min(1+20*math.Log10(mean)/60, 1))
}
