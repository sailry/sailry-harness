//go:build integration && (darwin || linux || windows)

package process

import (
	"crypto/sha256"
	"fmt"
	"io"
	"os"
	"runtime"
	"strings"
	"testing"
	"time"

	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestNativeForegroundStdin(t *testing.T) {
	const mode = "SAILRY_PROCESS_STDIN_FIXTURE"
	switch os.Getenv(mode) {
	case "read":
		data, err := io.ReadAll(os.Stdin)
		if err != nil {
			fmt.Fprint(os.Stderr, err)
			os.Exit(1)
		}
		fmt.Fprintf(os.Stdout, "%x", sha256.Sum256(data))
		os.Exit(0)
	case "ignore":
		fmt.Fprint(os.Stdout, "legacy")
		os.Exit(7)
	case "block":
		time.Sleep(8 * time.Second)
		os.Exit(1)
	}
	executable, err := os.Executable()
	if err != nil {
		t.Fatal(err)
	}
	command := `"$SAILRY_PROCESS_TEST_EXE" -test.run="^TestNativeForegroundStdin$"`
	if runtime.GOOS == "windows" {
		command = `"%SAILRY_PROCESS_TEST_EXE%" -test.run="^TestNativeForegroundStdin$"`
	}
	service, err := New(Options{Profile: t.TempDir()})
	if err != nil {
		t.Fatal(err)
	}
	defer service.Close()
	record := `{"cwd":"中文 $HOME %USERPROFILE% \" ' & |"}` + "\n"
	largeInput := strings.Repeat("x", 2<<20)
	for _, fixture := range []struct {
		mode, stdin, outcome, stdout string
		code                         int32
		timeout                      uint64
	}{
		{"read", record, "exited", fmt.Sprintf("%x", sha256.Sum256([]byte(record))), 0, 10000},
		{"ignore", largeInput, "exited", "legacy", 7, 10000},
		{"block", largeInput, "timed_out", "", 0, 500},
	} {
		t.Run(fixture.mode, func(t *testing.T) {
			launch := Launch{
				ID: protocol.NewRequestID(), Root: t.TempDir(), Command: command, Access: protocol.ProcessAccessUnrestricted,
				Stdin: fixture.stdin, TimeoutMs: fixture.timeout, Environment: map[string]string{mode: fixture.mode, "SAILRY_PROCESS_TEST_EXE": executable},
			}
			result, err := service.Execute(t.Context(), launch)
			if err != nil || result.Outcome.Kind != fixture.outcome || result.Stdout.Text != fixture.stdout || result.Stderr.Text != "" {
				t.Fatalf("foreground process returned outcome %+v, stdout %q, stderr %q, error %v", result.Outcome, result.Stdout.Text, result.Stderr.Text, err)
			}
			if fixture.outcome == "exited" && result.Outcome.Data != fixture.code {
				t.Fatalf("foreground exit status = %+v, want %d", result.Outcome, fixture.code)
			}
		})
	}
}

func TestNativeStdinReaderClosed(t *testing.T) {
	reader, writer, err := os.Pipe()
	if err != nil {
		t.Fatal(err)
	}
	defer writer.Close()
	if err := reader.Close(); err != nil {
		t.Fatal(err)
	}
	if _, err := io.WriteString(writer, "fixture"); !nativeInputClosed(err) {
		t.Fatalf("writing to a closed reader returned %v", err)
	}
}
