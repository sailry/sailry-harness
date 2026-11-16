package main

import (
	"encoding/json"
	"fmt"
	"os"
	"path/filepath"
	"slices"
	"strings"
	"testing"

	"github.com/sailry/sailry-harness/internal/shipped"
)

func crtArtifact(t *testing.T, directory string) []string {
	t.Helper()
	files := []string{"msvcp140.dll", "msvcp140_1.dll", "vcruntime140.dll", "vcruntime140_1.dll", "windows-crt-Redist.txt", "windows-crt-ThirdPartyNotices.txt"}
	manifest, err := json.Marshal(shipped.WindowsCRT{Version: 1, Target: "windows/amd64", Toolset: "fixture", Files: files})
	if err != nil {
		t.Fatal(err)
	}
	all := append([]string{shipped.WindowsCRTManifest}, files...)
	var sums strings.Builder
	for _, name := range all {
		body := []byte("fixture " + name)
		if name == shipped.WindowsCRTManifest {
			body = manifest
		}
		if err := os.WriteFile(filepath.Join(directory, name), body, 0o600); err != nil {
			t.Fatal(err)
		}
		fmt.Fprintf(&sums, "%s  %s\n", digest(body), name)
	}
	if err := os.WriteFile(filepath.Join(directory, "SHA256SUMS"), []byte(sums.String()), 0o600); err != nil {
		t.Fatal(err)
	}
	return all
}

func TestWindowsCRTArtifact(t *testing.T) {
	artifacts := t.TempDir()
	files := crtArtifact(t, artifacts)
	run := &stage{goos: "windows", triple: triples["windows/amd64"], output: t.TempDir()}
	if err := run.windowsCRT(artifacts); err != nil {
		t.Fatal(err)
	}
	for _, name := range files {
		if !slices.Contains(run.shipped, name) {
			t.Errorf("%s is not shipped", name)
		}
		actual, err := os.ReadFile(filepath.Join(run.output, name))
		expected, _ := os.ReadFile(filepath.Join(artifacts, name))
		if err != nil || string(actual) != string(expected) {
			t.Errorf("%s was not staged unchanged: %v", name, err)
		}
	}
	wrongTarget := &stage{goos: "windows", triple: triples["windows/arm64"], output: t.TempDir()}
	if err := wrongTarget.windowsCRT(artifacts); err == nil || !strings.Contains(err.Error(), "another platform") {
		t.Fatalf("wrong target accepted: %v", err)
	}
	if err := os.WriteFile(filepath.Join(artifacts, "msvcp140.dll"), []byte("changed"), 0o600); err != nil {
		t.Fatal(err)
	}
	if err := run.windowsCRT(artifacts); err == nil || !strings.Contains(err.Error(), "SHA256SUMS") {
		t.Fatalf("changed artifact accepted: %v", err)
	}
	other := &stage{goos: "linux", output: t.TempDir()}
	if err := other.windowsCRT(artifacts); err != nil || len(other.shipped) != 0 {
		t.Fatalf("CRT staged for Linux: %v", err)
	}
}

func TestWindowsCRTSeal(t *testing.T) {
	t.Chdir("../..")
	directory := t.TempDir()
	crtFiles := crtArtifact(t, directory)
	run := &stage{goos: "windows", triple: triples["windows/amd64"]}
	_, onnx, err := run.load("native/onnxruntime.lock.json")
	if err != nil {
		t.Fatal(err)
	}
	names := append(built("windows"), tursoLibrary("windows"))
	for _, file := range onnx.Files {
		names = append(names, file.Name)
	}
	for _, name := range names {
		if err := os.WriteFile(filepath.Join(directory, name), []byte("fixture "+name), 0o600); err != nil {
			t.Fatal(err)
		}
	}
	for _, host := range []bool{false, true} {
		if err := run.seal(directory, "windows/amd64", host, "", ""); err != nil {
			t.Fatal(err)
		}
		seal, err := shipped.Read(directory, "windows/amd64")
		if err != nil {
			t.Fatal(err)
		}
		for _, name := range crtFiles {
			if err := seal.Check(directory, name); err != nil {
				t.Errorf("host=%v, %s: %v", host, name, err)
			}
		}
		if err := seal.CheckWindowsCRT(directory); err != nil {
			t.Fatal(err)
		}
	}
	if err := os.Remove(filepath.Join(directory, "vcruntime140_1.dll")); err != nil {
		t.Fatal(err)
	}
	if err := run.seal(directory, "windows/amd64", false, "", ""); err == nil {
		t.Fatal("sealed an incomplete CRT")
	}
}
