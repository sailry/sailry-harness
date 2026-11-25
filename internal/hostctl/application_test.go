package hostctl

import (
	"bufio"
	"bytes"
	"path/filepath"
	"strings"
	"testing"

	"github.com/sailry/sailry-harness/internal/identity"
)

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
