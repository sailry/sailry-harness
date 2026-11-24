package sidebar

import "testing"

func TestTerminalTitleDropsShellPrompt(t *testing.T) {
	for title, want := range map[string]string{
		"lixinhua@appledeMBP-2:/Volumes/Web/sailry-go": "/Volumes/Web/sailry-go",
		"dev@host: ~/app": "~/app",
		"zsh":             "zsh",
		"vim notes@2.md":  "vim notes@2.md",
	} {
		if got := TerminalTitle(title); got != want {
			t.Errorf("TerminalTitle(%q) = %q, want %q", title, got, want)
		}
	}
}
