package application

import (
	"os/exec"
	"strings"
	"testing"
)

func TestForkCommandDependencies(t *testing.T) {
	output, err := exec.CommandContext(t.Context(), "go", "list", "-deps", "./testdata/fork").CombinedOutput()
	if err != nil {
		t.Fatalf("go list fork dependencies: %v\n%s", err, output)
	}
	const module = "github.com/sailry/sailry-harness/"
	for _, path := range strings.Fields(string(output)) {
		if path == module+"internal/assembly/desktop" || strings.HasPrefix(path, module+"modules/") {
			t.Errorf("minimal fork imports %s", path)
		}
	}
}
