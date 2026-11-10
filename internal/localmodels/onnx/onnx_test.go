package onnx

import (
	"crypto/sha256"
	"encoding/hex"
	"os"
	"path/filepath"
	"runtime"
	"strings"
	"testing"

	"google.golang.org/protobuf/encoding/protowire"
)

// TestRuntimeFiles covers the loader rules that need no native library: only
// ordinary files matching the lock are accepted, and a missing runtime is an
// error instead of a crash.
func TestRuntimeFiles(t *testing.T) {
	content := []byte("pinned runtime fixture")
	sum := sha256.Sum256(content)
	pinned := []file{{"runtime.bin", int64(len(content)), hex.EncodeToString(sum[:])}}
	cases := []struct {
		name    string
		prepare func(t *testing.T, path string)
		failure string
	}{
		{"matching", func(t *testing.T, path string) { write(t, path, content) }, ""},
		{"missing", func(*testing.T, string) {}, "file is unavailable"},
		{"directory", func(t *testing.T, path string) {
			if err := os.Mkdir(path, 0o700); err != nil {
				t.Fatal(err)
			}
		}, "not an ordinary file"},
		{"symlink", func(t *testing.T, path string) {
			if runtime.GOOS == "windows" {
				t.Skip("symbolic links need a privilege on Windows")
			}
			other := filepath.Join(filepath.Dir(path), "other")
			write(t, other, content)
			if err := os.Symlink(other, path); err != nil {
				t.Fatal(err)
			}
		}, "not an ordinary file"},
		{"size", func(t *testing.T, path string) { write(t, path, content[1:]) }, "pinned size"},
		{"digest", func(t *testing.T, path string) {
			write(t, path, []byte(strings.ToUpper(string(content))))
		}, "pinned SHA-256"},
	}
	for _, test := range cases {
		t.Run(test.name, func(t *testing.T) {
			directory := t.TempDir()
			test.prepare(t, filepath.Join(directory, pinned[0].name))
			path, err := verify(directory, pinned)
			if test.failure == "" {
				if err != nil || path != filepath.Join(directory, pinned[0].name) {
					t.Fatalf("verify = %q, %v", path, err)
				}
				return
			}
			if err == nil || !strings.Contains(err.Error(), test.failure) {
				t.Fatalf("verify error = %v, want %q", err, test.failure)
			}
		})
	}
	t.Run("lock", func(t *testing.T) {
		for target, files := range lock {
			for _, pinned := range files {
				if digest, err := hex.DecodeString(pinned.sha256); err != nil || len(digest) != sha256.Size || pinned.size <= 0 || pinned.name == "" {
					t.Errorf("%s: invalid lock entry %+v", target, pinned)
				}
			}
		}
	})
	t.Run("absent runtime", func(t *testing.T) {
		if _, packaged := lock[runtime.GOOS+"/"+runtime.GOARCH]; !packaged {
			t.Skip("no runtime is packaged for this target")
		}
		mutex.Lock()
		active := loaded != nil
		mutex.Unlock()
		if active {
			t.Skip("an integration test already loaded the runtime")
		}
		// Neither an empty directory nor the test binary's directory holds a runtime.
		if err := Load(t.TempDir()); err == nil {
			t.Fatal("Load succeeded in an empty directory")
		}
		if err := Initialize(); err == nil {
			t.Fatal("Initialize succeeded without a packaged runtime")
		}
		if err := Available(); err == nil {
			t.Fatal("Available succeeded without a packaged runtime")
		}
		if _, err := NewSession(filepath.Join("testdata", "matmul.onnx")); err == nil {
			t.Fatal("NewSession succeeded without a runtime")
		}
	})
}

func write(t *testing.T, path string, content []byte) {
	t.Helper()
	if err := os.WriteFile(path, content, 0o600); err != nil {
		t.Fatal(err)
	}
}

// TestValidate checks the model metadata projection on encoded messages.
func TestValidate(t *testing.T) {
	field := func(number protowire.Number, body []byte) []byte {
		return protowire.AppendBytes(protowire.AppendTag(nil, number, protowire.BytesType), body)
	}
	varint := func(number protowire.Number, value uint64) []byte {
		return protowire.AppendVarint(protowire.AppendTag(nil, number, protowire.VarintType), value)
	}
	join := func(parts ...[]byte) []byte {
		var out []byte
		for _, part := range parts {
			out = append(out, part...)
		}
		return out
	}
	weights := field(9, make([]byte, 4096)) // raw_data, skipped without being parsed
	nested := func(tensor []byte) []byte {
		// model.graph.node.attribute.graph.initializer
		return field(7, field(1, field(5, field(6, field(5, tensor)))))
	}
	deep := varint(14, 1)
	for range depth {
		deep = field(6, field(1, field(5, deep)))
	}
	matmul, err := os.ReadFile(filepath.Join("testdata", "matmul.onnx"))
	if err != nil {
		t.Fatal(err)
	}
	cases := []struct {
		name    string
		model   []byte
		failure string
	}{
		{"committed model", matmul, ""},
		{"inline weights", join(varint(1, 9), field(7, field(5, join(weights, varint(14, 0))))), ""},
		{"no graph", varint(1, 9), "graph is missing"},
		{"empty file", nil, "graph is missing"},
		{"external data entry", field(7, field(5, field(13, []byte("location")))), "external tensor data"},
		{"external location", field(7, field(5, join(varint(14, 0), varint(14, 1)))), "external tensor data"},
		{"packed external location", field(7, field(5, field(14, []byte{0, 1}))), "external tensor data"},
		{"nested external tensor", nested(varint(14, 1)), "external tensor data"},
		{"sparse external tensor", field(7, field(15, field(2, varint(14, 1)))), "external tensor data"},
		{"function external tensor", join(field(7, nil), field(25, field(11, field(10, varint(14, 1))))), "external tensor data"},
		{"training external tensor", join(field(7, nil), field(20, field(2, field(5, varint(14, 1))))), "external tensor data"},
		{"truncated", field(7, field(5, weights))[:100], "malformed"},
		{"graph with a scalar type", varint(7, 1), "malformed"},
		{"nesting limit", field(7, field(1, field(5, deep))), "malformed"},
	}
	for _, test := range cases {
		t.Run(test.name, func(t *testing.T) {
			path := filepath.Join(t.TempDir(), "model.onnx")
			write(t, path, test.model)
			err := Validate(path)
			if test.failure == "" && err != nil || test.failure != "" && (err == nil || !strings.Contains(err.Error(), test.failure)) {
				t.Fatalf("Validate error = %v, want %q", err, test.failure)
			}
		})
	}
}
