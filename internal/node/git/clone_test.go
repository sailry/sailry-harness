package git

import (
	"context"
	"path/filepath"
	"testing"
)

// A value url.Parse refuses still has to be an scp-style address: a
// helper::address form would start git-remote-<helper>.
func TestCloneURLForms(t *testing.T) {
	for _, test := range []struct {
		url  string
		runs bool
	}{
		{"git@example.invalid:team/repository.git", true},
		{"foo::anything #% @", false},
		{"foo::user@host:%zz", false},
		{"ext://user@host:%zz", false},
		{"user@host:path with space", false},
	} {
		ran := false
		s := New(Options{Run: func(context.Context, string, []string, []byte) (CommandResult, error) {
			ran = true
			return CommandResult{}, nil
		}})
		_, err := s.Clone(context.Background(), test.url, filepath.Join(t.TempDir(), "repository"), nil)
		if ran != test.runs || !test.runs && (err == nil || err.Error() != invalid("repository URL is invalid").Error()) {
			t.Errorf("%q: ran=%v err=%v", test.url, ran, err)
		}
	}
}
