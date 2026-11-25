package preferences

import (
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"reflect"
	"strings"
	"testing"
)

func TestFinishStartupWriteFailureIsNotReplayed(t *testing.T) {
	profile := t.TempDir()
	s := ForStartup(profile)
	drafts := map[string]ArtifactDraft{"draft": {Form: "form", Fields: []ArtifactField{{Name: "text", Values: []string{"unsaved input"}}}}}
	s.Data.ArtifactDrafts = &drafts
	if err := os.WriteFile(filepath.Join(profile, "desktop"), []byte("block directory creation"), 0o600); err != nil {
		t.Fatal(err)
	}
	err := s.FinishStartup()
	if stage, reason := WriteFailure(err); err == nil || stage != "create_directory" || reason == "" || !s.Readable {
		t.Fatalf("startup write failure = %v, %q/%q, readable %v", err, stage, reason, s.Readable)
	}
	version := s.Revision()
	if err := s.FinishStartup(); err != nil || s.Revision() != version {
		t.Fatalf("a second window replayed or retried the startup save: %v", err)
	}
	if s.Err != err || s.Data.ArtifactDrafts == nil || !reflect.DeepEqual(*s.Data.ArtifactDrafts, drafts) {
		t.Fatal("a startup write failure lost its error or editable draft")
	}
}

func TestFinishStartupPreservesReadFailure(t *testing.T) {
	path := filepath.Join(t.TempDir(), "preferences.json")
	original := []byte("unreadable preferences")
	if err := os.WriteFile(path, original, 0o600); err != nil {
		t.Fatal(err)
	}
	s := Open(path)
	readErr := s.Err
	if s.Readable || readErr == nil {
		t.Fatal("fixture is readable")
	}
	s.Data.Terminal.FontSize = 20
	for range 2 {
		if err := s.FinishStartup(); err != readErr {
			t.Fatalf("a window lost the original read error: %v", err)
		}
	}
	if err := s.Save(); err != readErr || s.Data.Terminal.FontSize != 20 {
		t.Fatal("unreadable settings lost the read error or editable draft")
	}
	got, err := os.ReadFile(path)
	if err != nil || string(got) != string(original) {
		t.Fatalf("unreadable settings were overwritten: %q, %v", got, err)
	}
}

func TestWriteFailureDiagnostic(t *testing.T) {
	for _, stage := range []string{"create_directory", "create_temp", "write", "sync", "close", "replace"} {
		for _, cause := range []struct {
			err    error
			reason string
		}{{os.ErrPermission, "permission_denied"}, {os.ErrNotExist, "not_found"}, {os.ErrExist, "already_exists"}, {errors.New("private failure text"), "other"}} {
			original := &os.PathError{Op: "private operation", Path: "/private/preferences.json", Err: cause.err}
			err := fmt.Errorf("save: %w", &writeFailure{stage: stage, err: original})
			gotStage, gotReason := WriteFailure(err)
			if gotStage != stage || gotReason != cause.reason || !errors.Is(err, cause.err) {
				t.Fatalf("write diagnostic = %q/%q, %v", gotStage, gotReason, err)
			}
			var pathErr *os.PathError
			if !errors.As(err, &pathErr) || pathErr != original || strings.Contains(err.Error(), "private") {
				t.Fatalf("write error lost its cause or disclosed its text: %v", err)
			}
		}
	}
	for _, err := range []error{nil, errors.New("unclassified error")} {
		if stage, reason := WriteFailure(err); stage != "" || reason != "" {
			t.Fatalf("ordinary error acquired a write diagnostic: %q/%q", stage, reason)
		}
	}
}
