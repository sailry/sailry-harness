//go:build integration && linux

package git

import (
	"context"
	"os"
	"os/exec"
	"path/filepath"
	"slices"
	"strings"
	"testing"
)

func TestLinuxNativeGitReadScope(t *testing.T) {
	git, err := exec.LookPath("git")
	if err != nil {
		t.Skip("Git executable is required")
	}
	root, _, _ := fixture(t)
	outside := filepath.Join(t.TempDir(), "private.txt")
	if err := os.WriteFile(outside, []byte("fixture\n"), 0o600); err != nil {
		t.Fatal(err)
	}
	settings := "[user]\nname = Fixture\n[commit]\ngpgsign = true\n[filter \"lfs\"]\nprocess = git-lfs filter-process\n"
	if err := os.WriteFile(filepath.Join(filepath.Dir(outside), ".gitconfig"), []byte(settings), 0o600); err != nil {
		t.Fatal(err)
	}
	command := exec.Command(git)
	command.Dir = root
	ctx := context.WithValue(t.Context(), gitReadKey{}, true)
	launch, err := prepareGitCommand(ctx, command, []string{"status"}, map[string]string{"HOME": filepath.Dir(outside)}, "")
	if err != nil {
		t.Fatal(err)
	}
	separator := slices.Index(launch.Args, "--")
	if separator < 0 || !slices.Contains(launch.Args[:separator], "--unshare-net") {
		launch.close()
		t.Fatal("local Git did not request a private network namespace")
	}
	// Probe the execution boundary directly without configuring a Git helper.
	mountPath := strings.NewReplacer("\\", `\134`, " ", `\040`, "\t", `\011`, "\n", `\012`).Replace(root)
	launch.Args = append(launch.Args[:separator+1], "/bin/sh", "-c",
		`test -r file.txt || exit 11
test ! -e "$1" || exit 12
test "$HOME" = /.sailry-git/home || exit 13
grep -qF " $2 ro," /proc/self/mountinfo || exit 14
test "$(git config --global --get user.name)" = Fixture || exit 15
test "$(git config --global --get commit.gpgsign)" = true || exit 16
test "$(git config --global --get filter.lfs.process)" = "git-lfs filter-process" || exit 17`, "probe", outside, mountPath)
	result, err := runGitCommand(ctx, launch, nil)
	if err != nil || result.ExitCode != 0 {
		t.Fatalf("sandbox permissions = %+v, %v", result, err)
	}
}
