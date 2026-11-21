package host

import (
	"io"
	"strings"
	"testing"
)

func TestSSHOutputLimitDuringCopy(t *testing.T) {
	prefix := strings.Repeat("x", sshOutputLimit)
	for _, test := range []struct {
		name, suffix string
		truncated    bool
	}{
		{"exact limit", "", false},
		{"one byte over", "y", true},
	} {
		t.Run(test.name, func(t *testing.T) {
			body := prefix + test.suffix
			reader := struct{ io.Reader }{strings.NewReader(body)}
			var output limitedOutput
			if copied, err := io.Copy(&output, reader); err != nil || copied != int64(len(body)) {
				t.Fatalf("drained %d of %d bytes: %v", copied, len(body), err)
			}
			if got := output.buffer.String(); got != prefix || output.truncated != test.truncated {
				t.Fatalf("retained %d bytes (want the %d-byte prefix), truncated = %v (want %v)", len(got), len(prefix), output.truncated, test.truncated)
			}
		})
	}
}
