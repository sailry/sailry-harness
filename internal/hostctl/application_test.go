package hostctl

import (
	"bufio"
	"bytes"
	"errors"
	"os"
	"path/filepath"
	"strings"
	"testing"

	"github.com/sailry/sailry-harness/internal/core"
	"github.com/sailry/sailry-harness/internal/identity"
)

func TestHostProfileDefaultsRemainProductScoped(t *testing.T) {
	home := filepath.Join(t.TempDir(), "user")
	t.Setenv("HOME", home)
	t.Setenv("USERPROFILE", home)
	previous := core.Default
	t.Cleanup(func() { core.Default = previous })
	for _, shared := range []string{identity.SourceProfile, identity.InstalledProfile} {
		core.Default = shared
		for _, product := range []identity.Desktop{{}, identity.OfficialDesktop(), {ID: "code", DefaultProfile: "~/.sailry"}} {
			profile := product.DefaultProfile
			if profile == "" {
				profile = shared
			}
			want := filepath.Join(home, strings.TrimPrefix(profile, "~/"))
			options, help, err := parseService(nil, product.DefaultProfile)
			if err != nil || help || options.Profile != want {
				t.Fatalf("product %q service with shared %q = %+v, help %v, error %v; want %q", product.ID, shared, options, help, err, want)
			}
			task, help, err := parseTask([]string{"submit", "--session", "00000000-0000-0000-0000-000000000001", "--prompt", "Task"}, strings.NewReader(""), product.DefaultProfile)
			if err != nil || help || task.Profile != want {
				t.Fatalf("product %q task with shared %q = %+v, help %v, error %v; want %q", product.ID, shared, task, help, err, want)
			}
		}
	}
	if _, err := os.Stat(home); !errors.Is(err, os.ErrNotExist) {
		t.Fatalf("profile resolution wrote to the home directory: %v", err)
	}
}

func TestHostProductDefaultProfile(t *testing.T) {
	t.Parallel()
	profile, override := filepath.Join(t.TempDir(), "product"), filepath.Join(t.TempDir(), "override")
	for _, fixture := range []struct {
		args []string
		want string
	}{
		{nil, profile}, {[]string{"--data-dir", override}, override},
	} {
		options, help, err := parseService(fixture.args, profile)
		if err != nil || help || options.Profile != fixture.want {
			t.Fatalf("product service %q = %+v, help %v, error %v", fixture.args, options, help, err)
		}
		taskArgs := append([]string{"submit", "--session", "00000000-0000-0000-0000-000000000001", "--prompt", "--data-dir"}, fixture.args...)
		task, help, err := parseTask(taskArgs, strings.NewReader(""), profile)
		if err != nil || help || task.Profile != fixture.want || task.Task.Message.Text != "--data-dir" {
			t.Fatalf("product task %q = %+v, help %v, error %v", taskArgs, task, help, err)
		}
	}
}

func TestHostMenuUsesProductVersion(t *testing.T) {
	t.Parallel()
	var output bytes.Buffer
	command, err := runMenu(t.Context(), bufio.NewReader(strings.NewReader("0\n")), &output, identity.Desktop{Version: "9.8.7"})
	if command != "" || err != nil || !strings.HasPrefix(output.String(), "\nSailry Host 9.8.7\n") {
		t.Fatalf("product menu = %q, %v, %q", command, err, output.String())
	}
}
