package process

import (
	"strings"
	"testing"
)

func TestSandboxProfileMacOS(t *testing.T) {
	t.Parallel()
	policy := sandboxPolicy{
		paths:       []allowedPath{{"/workspace/a/.git", false}, {"/workspace", true}, {"/workspace/a/.git/tmp", true}, {"/read\"\\\n", false}, {"/profile/tmp/scratch", true}},
		deniedReads: []string{"/profile", "/Users/alice/.ssh"},
		protected:   []string{"/workspace/.sailry/worktrees"},
	}
	profile, err := sandboxProfileMacOS(policy, "")
	if err != nil {
		t.Fatal(err)
	}
	for _, required := range []string{
		"(deny default)",
		"(allow file-read*)",
		`(deny file-read* (subpath "/profile"))`,
		`(deny file-read* (subpath "/Users/alice/.ssh"))`,
		`(deny file-write* (subpath "/workspace/a/.git"))`,
		`(deny file-write-unlink (literal "/workspace/a"))`,
		`(deny file-write-unlink (literal "/workspace"))`,
		`(deny file-write-unlink (literal "/workspace/a/.git/tmp"))`,
		`(deny file-write-unlink (literal "/workspace/.sailry"))`,
		`(subpath "/read\"\\\x0a")`,
	} {
		if !strings.Contains(profile, required) {
			t.Fatal("missing sandbox rule", required, profile)
		}
	}
	if strings.Contains(profile, "(allow network") || strings.Contains(profile, "(subpath \"/dev\")") {
		t.Fatal("sandbox broadened access")
	}
	parent := strings.Index(profile, "(allow file-read* file-write* (subpath \"/workspace\"))")
	child := strings.Index(profile, "(deny file-write* (subpath \"/workspace/a/.git\"))")
	if parent < 0 || child < parent {
		t.Fatal("nested sandbox path order changed")
	}
	denyProfile := strings.Index(profile, `(deny file-read* (subpath "/profile"))`)
	allowScratch := strings.Index(profile, `(allow file-read* file-write* (subpath "/profile/tmp/scratch"))`)
	if denyProfile < 0 || allowScratch < denyProfile {
		t.Fatal("scratch grant must follow profile read deny so last-match-wins allows scratch")
	}
}

func TestSandboxProfileMacOSReadOnly(t *testing.T) {
	t.Parallel()
	policy := sandboxPolicy{
		paths:       []allowedPath{{"/workspace", false}, {"/profile/tmp/scratch", true}},
		deniedReads: []string{"/profile"},
		protected:   []string{"/workspace/.sailry/worktrees"},
	}
	profile, err := sandboxProfileMacOS(policy, "")
	if err != nil {
		t.Fatal(err)
	}
	if strings.Contains(profile, `(allow file-read* file-write* (subpath "/workspace"))`) {
		t.Fatalf("read-only sandbox granted write access to workspace:\n%s", profile)
	}
	if !strings.Contains(profile, `(allow file-read* (subpath "/workspace"))`) {
		t.Fatalf("read-only sandbox omitted read access to workspace:\n%s", profile)
	}
	if !strings.Contains(profile, `(allow file-read* file-write* (subpath "/profile/tmp/scratch"))`) {
		t.Fatalf("read-only sandbox omitted scratch write access:\n%s", profile)
	}
}
