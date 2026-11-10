package speech

import (
	"crypto/sha256"
	"encoding/hex"
	"fmt"
	"io"
	"os"
	"path/filepath"
	"runtime"
)

// Version is the only sherpa-onnx release this binding accepts.
const Version = "1.13.8"

// library is the sherpa-onnx C API library of one target, pinned by size
// and SHA-256.
type library struct {
	name   string
	size   int64
	sha256 string
}

// lock pins the "speech" member of the sherpa-onnx v1.13.8 shared archives,
// the same archives the onnxruntime files come from.
var lock = map[string]library{
	"darwin/arm64":  {"libsherpa-onnx-c-api.dylib", 4172832, "ee098d8b419d49b92101cde3c970a333b361066eb2d79a11ab480a116552b908"},
	"darwin/amd64":  {"libsherpa-onnx-c-api.dylib", 4542656, "d32eb199b94c076afef9cfc42e445c581c0326a72fb2cfdfdc177ebacf9bf38f"},
	"linux/amd64":   {"libsherpa-onnx-c-api.so", 5124192, "b8351ca1632571ac108adbb317bcc4bf7cfe84b72690e3017316b0da3e1e344f"},
	"linux/arm64":   {"libsherpa-onnx-c-api.so", 4613032, "11378e1aea169aecb59a4d41ca8da6f5ee4f3614c34a4b908dd78de7646a518d"},
	"windows/amd64": {"sherpa-onnx-c-api.dll", 4605952, "300e0c88400903fc4cfc88be88a8ae587a24cc658653e0e1fd1c2b8dbbc68557"},
	"windows/arm64": {"sherpa-onnx-c-api.dll", 4584960, "48fc834012df0874d60282f0575705f133a1130b5a57cdcdcfb848fd42b47439"},
}

// pinned returns the absolute path of this platform's library in directory
// and what it is pinned to.
func pinned(directory string) (string, library, error) {
	expected, packaged := lock[runtime.GOOS+"/"+runtime.GOARCH]
	if !packaged {
		return "", library{}, fmt.Errorf("%w: no sherpa-onnx build for this platform", ErrUnavailable)
	}
	path, err := filepath.Abs(filepath.Join(directory, expected.name))
	if err != nil {
		return "", library{}, fmt.Errorf("%w: %v", ErrUnavailable, err)
	}
	return path, expected, nil
}

// verify checks that path is an ordinary file of the pinned size and digest.
func verify(path string, expected library) error {
	info, err := os.Lstat(path)
	if err != nil || !info.Mode().IsRegular() {
		return fmt.Errorf("%w: %s is not an ordinary file", ErrUnavailable, expected.name)
	}
	if info.Size() != expected.size {
		return fmt.Errorf("%w: %s does not match its pinned size", ErrUnavailable, expected.name)
	}
	source, err := os.Open(path)
	if err != nil {
		return fmt.Errorf("%w: %v", ErrUnavailable, err)
	}
	defer source.Close()
	hash := sha256.New()
	if _, err := io.Copy(hash, source); err != nil {
		return fmt.Errorf("%w: %v", ErrUnavailable, err)
	}
	if hex.EncodeToString(hash.Sum(nil)) != expected.sha256 {
		return fmt.Errorf("%w: %s does not match its pinned SHA-256", ErrUnavailable, expected.name)
	}
	return nil
}
