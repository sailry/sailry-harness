//go:build integration && darwin

package computer

import (
	"os"
	"os/exec"
	"strconv"
	"testing"
	"time"

	"github.com/ebitengine/purego"
)

func TestMain(m *testing.M) {
	Trampoline()
	os.Exit(m.Run())
}

// TestHelperDisclaim starts a harmless program through the trampoline and
// reads which process macOS holds responsible for it. A disclaimed process
// answers for itself; an ordinary child answers to this process's owner.
func TestHelperDisclaim(t *testing.T) {
	address, err := purego.Dlsym(purego.RTLD_DEFAULT, "responsibility_get_pid_responsible_for_pid")
	if err != nil {
		t.Skip("the responsibility interface is unavailable")
	}
	var responsible func(pid int32) int32
	purego.RegisterFunc(&responsible, address)
	start := func(program string, args ...string) *exec.Cmd {
		command := exec.Command(program, args...)
		if err := command.Start(); err != nil {
			t.Fatal(err)
		}
		t.Cleanup(func() { _ = command.Process.Kill(); _ = command.Wait() })
		// The trampoline replaces its image shortly after it starts.
		time.Sleep(500 * time.Millisecond)
		return command
	}
	ordinary := start("/bin/sleep", "30")
	program, args := Disclaim("/bin/sleep", []string{"30"})
	if program == "/bin/sleep" {
		t.Fatal("the launch does not pass through the trampoline")
	}
	disclaimed := start(program, args...)
	own := responsible(int32(os.Getpid()))
	plain, helper := responsible(int32(ordinary.Process.Pid)), responsible(int32(disclaimed.Process.Pid))
	t.Logf("test %d answers to %d; child %d to %d; disclaimed %d to %d", os.Getpid(), own, ordinary.Process.Pid, plain, disclaimed.Process.Pid, helper)
	if plain != own {
		t.Errorf("an ordinary child answers to %d, not %d", plain, own)
	}
	if helper != int32(disclaimed.Process.Pid) {
		t.Errorf("the disclaimed helper answers to %d, not itself", helper)
	}
	output, err := exec.Command("/bin/ps", "-o", "comm=", "-p", strconv.Itoa(disclaimed.Process.Pid)).Output()
	if err != nil || string(output) != "/bin/sleep\n" {
		t.Errorf("the trampoline became %q, %v", output, err)
	}
}
