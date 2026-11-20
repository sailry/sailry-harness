//go:build !windows

package live

import (
	"errors"
	"log/slog"
	"os"
	"path/filepath"
	"syscall"
	"testing"

	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestLocalAttachmentSpecialSource(t *testing.T) {
	t.Parallel()
	path := attachmentSpecialSourceFixture(t)
	source, err := localSource(t.Context(), path, nil, "")
	var sourceError *attachmentSourceError
	if !errors.As(err, &sourceError) || sourceError.key != "files_upload_special_source" || faultCode(err) != protocol.ErrorCodeInvalidRequest || source.reader != nil {
		t.Fatalf("special source rejection lost its outcome: source %+v, error %v", source, err)
	}
}

func TestDraftAttachmentSpecialSourceRejection(t *testing.T) {
	testDraftAttachmentSourceRejection(t, attachmentSpecialSourceFixture(t), "files_upload_special_source", "special_source", "invalid_request", slog.LevelWarn)
}

func TestAttachmentUnreadableSourceRemainsError(t *testing.T) {
	if os.Geteuid() == 0 {
		t.Skip("root can read the permission-denied fixture")
	}
	path := filepath.Join(t.TempDir(), "unreadable.txt")
	if err := os.WriteFile(path, []byte("private fixture"), 0o000); err != nil {
		t.Fatal(err)
	}
	source, err := localSource(t.Context(), path, nil, "")
	var sourceError *attachmentSourceError
	if !errors.As(err, &sourceError) || sourceError.key != "files_upload_source" || !errors.Is(err, os.ErrPermission) || source.reader != nil {
		t.Fatalf("unreadable source lost its I/O fault: source %+v, error %v", source, err)
	}
	testDraftAttachmentSourceRejection(t, path, "files_upload_source", "read_failed", "internal", slog.LevelError)
}

func attachmentSpecialSourceFixture(t *testing.T) string {
	t.Helper()
	path := filepath.Join(t.TempDir(), "source.fifo")
	if err := syscall.Mkfifo(path, 0o600); err != nil {
		t.Fatal(err)
	}
	return path
}
