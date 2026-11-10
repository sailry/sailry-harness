package process

import (
	"strings"
	"testing"
)

func TestSandboxProfileMacOS(t *testing.T) {
	t.Parallel()
	policy := sandboxPolicy{paths: []allowedPath{{"/workspace/a/.git", false}, {"/workspace", true}, {"/workspace/a/.git/tmp", true}, {"/read\"\\\n", false}}, protected: []string{"/workspace/.sailry/worktrees"}}
	profile, err := sandboxProfileMacOS(policy, "")
	if err != nil {
		t.Fatal(err)
	}
	for _, required := range []string{"(deny default)", "(deny file-write* (subpath \"/workspace/a/.git\"))", "(deny file-write-unlink (literal \"/workspace/a\"))", "(deny file-write-unlink (literal \"/workspace\"))", "(deny file-write-unlink (literal \"/workspace/a/.git/tmp\"))", "(deny file-write-unlink (literal \"/workspace/.sailry\"))", `(subpath "/read\"\\\x0a")`} {
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
}
