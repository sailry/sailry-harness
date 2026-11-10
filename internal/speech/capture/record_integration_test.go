//go:build integration

package capture

import (
	"context"
	"os"
	"testing"
	"time"
)

// TestRecording lists the input devices and records half a second from the
// default one. It prints levels only: no audio is stored. This file sorts
// after capture_test.go, whose absent-library case must run before a
// library is loaded into the process.
//
//	SAILRY_CAPTURE_DIR directory holding the built capture library
//
// The system's permission prompt is not raised: without a granted
// microphone the recording is skipped.
func TestRecording(t *testing.T) {
	directory := os.Getenv("SAILRY_CAPTURE_DIR")
	if directory == "" {
		t.Skip("SAILRY_CAPTURE_DIR is not set")
	}
	if err := Load(directory); err != nil {
		t.Fatal(err)
	}
	if err := Load(directory); err != nil {
		t.Fatalf("loading the same library again: %v", err)
	}
	if err := Load(t.TempDir()); err == nil {
		t.Fatal("a second library directory was accepted")
	}
	devices, err := Devices()
	if err != nil {
		t.Fatal(err)
	}
	defaults := 0
	for _, device := range devices {
		t.Logf("input device %q id %s default %v", device.Name, device.ID, device.Default)
		if device.ID == "" || device.Name == "" {
			t.Fatalf("incomplete device %+v", device)
		}
		if device.Default {
			defaults++
		}
	}
	if len(devices) > 0 && defaults != 1 {
		t.Fatalf("%d default devices among %d", defaults, len(devices))
	}
	if _, err := Open("00"); err == nil {
		t.Fatal("an unknown device was opened")
	}

	status := Status()
	t.Logf("microphone permission %d", status)
	if status != PermissionGranted && status != PermissionUnknown {
		t.Skip("the microphone is not granted to this process; recording and the request path were not exercised")
	}
	ctx, cancel := context.WithTimeout(t.Context(), 10*time.Second)
	defer cancel()
	answered(t, ctx)
	if granted, err := Request(ctx); err != nil || !granted {
		t.Fatalf("request = %v, %v", granted, err)
	}
	if len(devices) == 0 {
		t.Skip("no input device")
	}

	for _, id := range []string{"", devices[0].ID} {
		recorder, err := Open(id)
		if err != nil {
			t.Fatal(err)
		}
		t.Logf("device %q: %d Hz, %d channels", id, recorder.SampleRate, recorder.Channels)
		if recorder.SampleRate == 0 || recorder.Channels <= 0 {
			t.Fatal("no audio format")
		}
		if err := recorder.Start(); err != nil {
			t.Fatal(err)
		}
		buffer := make([]float32, int(recorder.SampleRate)/10*recorder.Channels)
		frames := 0
		for range 10 {
			time.Sleep(50 * time.Millisecond)
			read := recorder.Read(buffer)
			frames += read
			t.Logf("level %.3f over %d frames", Level(buffer[:read*recorder.Channels]), read)
		}
		if err := recorder.Stop(); err != nil {
			t.Fatal(err)
		}
		frames += recorder.Read(buffer)
		if recorder.Failed() {
			t.Fatal("the device reports a failure after an ordinary stop")
		}
		t.Logf("%d frames in 0.5 s, %d dropped", frames, recorder.Dropped())
		// Half a second at the device's rate, within scheduling slack.
		if expected := int(recorder.SampleRate) / 2; frames < expected/2 || frames > expected*2 {
			t.Fatalf("%d frames in 0.5 s at %d Hz", frames, recorder.SampleRate)
		}
		recorder.Close()
		recorder.Close()
	}
}
