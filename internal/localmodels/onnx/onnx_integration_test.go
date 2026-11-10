//go:build integration

package onnx

import (
	"os"
	"path/filepath"
	"slices"
	"testing"
)

// TestInference runs a real CPU inference through the pinned runtime.
// SAILRY_ONNXRUNTIME_DIR names the directory holding the locked library.
func TestInference(t *testing.T) {
	directory := os.Getenv("SAILRY_ONNXRUNTIME_DIR")
	if directory == "" {
		t.Fatal("SAILRY_ONNXRUNTIME_DIR is not set")
	}
	if err := Load(directory); err != nil {
		t.Fatal(err)
	}
	if err := Load(directory); err != nil {
		t.Fatalf("repeated Load: %v", err)
	}
	if err := Load(t.TempDir()); err == nil {
		t.Fatal("a second runtime directory was accepted")
	}
	// A MatMul graph: [x, y] x [[2, 0], [0, 3]].
	session, err := NewSession(filepath.Join("testdata", "matmul.onnx"))
	if err != nil {
		t.Fatal(err)
	}
	defer session.Close()
	if len(session.Inputs) != 1 || len(session.Outputs) != 1 || session.Inputs[0].Element != ElementFloat32 || !slices.Equal(session.Inputs[0].Shape, []int64{1, 2}) {
		t.Fatalf("ports = %+v %+v", session.Inputs, session.Outputs)
	}
	for range 3 {
		outputs, err := session.Run([]Tensor{{Name: session.Inputs[0].Name, Shape: []int64{1, 2}, Float32: []float32{5, 7}}})
		if err != nil {
			t.Fatal(err)
		}
		if !slices.Equal(outputs[0].Shape, []int64{1, 2}) || !slices.Equal(outputs[0].Float32, []float32{10, 21}) {
			t.Fatalf("output = %v %v", outputs[0].Shape, outputs[0].Float32)
		}
	}
	if _, err := session.Run([]Tensor{{Name: "missing", Shape: []int64{1, 2}, Float32: []float32{5, 7}}}); err == nil {
		t.Fatal("an unknown input was accepted")
	}
	if _, err := NewSession(filepath.Join(t.TempDir(), "absent.onnx")); err == nil {
		t.Fatal("a missing model was accepted")
	}
}
