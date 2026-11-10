package capture

import (
	"errors"
	"math"
	"os"
	"path/filepath"
	"testing"
)

func TestCapture(t *testing.T) {
	devices := parseDevices("1\t4275696c74\tMacBook Pro Microphone\n0\t757362\tUSB\tAudio\n0\t\tunnamed\nbroken\n")
	if len(devices) != 2 ||
		devices[0] != (Device{ID: "4275696c74", Name: "MacBook Pro Microphone", Default: true}) ||
		devices[1] != (Device{ID: "757362", Name: "USB\tAudio"}) {
		t.Fatalf("devices = %+v", devices)
	}
	if parseDevices("") != nil {
		t.Fatal("no devices are listed as some")
	}

	// The meter is silent below the noise floor, full at full scale and
	// moves 1/3 per 20 dB between.
	tone := func(amplitude float32) []float32 {
		samples := make([]float32, 480)
		for index := range samples {
			samples[index] = amplitude
			if index%2 == 1 {
				samples[index] = -amplitude
			}
		}
		return samples
	}
	for amplitude, expected := range map[float32]float64{0: 0, 0.001: 0, 0.01: 1.0 / 3, 0.1: 2.0 / 3, 1: 1, 4: 1} {
		if level := float64(Level(tone(amplitude))); math.Abs(level-expected) > 1e-4 {
			t.Fatalf("level of %v = %v, expected %v", amplitude, level, expected)
		}
	}
	if Level(nil) != 0 || Level([]float32{float32(math.NaN()), float32(math.Inf(1))}) != 0 {
		t.Fatal("no finite audio moves the meter")
	}

	// Only an ordinary file in the given directory is opened.
	directory := t.TempDir()
	if err := Load(directory); !errors.Is(err, ErrUnavailable) {
		t.Fatalf("empty directory: %v", err)
	}
	if err := os.Mkdir(filepath.Join(directory, LibraryName()), 0o700); err != nil {
		t.Fatal(err)
	}
	if err := Load(directory); !errors.Is(err, ErrUnavailable) {
		t.Fatalf("a directory in the library's place: %v", err)
	}
	if PermissionDenied.Allowed() || PermissionRestricted.Allowed() || PermissionNotDetermined.Allowed() || !PermissionGranted.Allowed() {
		t.Fatal("permission states")
	}
}
