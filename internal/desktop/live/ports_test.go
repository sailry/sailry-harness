package live

import "testing"

func TestForwardURL(t *testing.T) {
	t.Parallel()
	for name, test := range map[string]struct {
		announced string
		port      uint16
		want      string
	}{
		"token query and path survive":    {"http://localhost:8888/lab?token=fixture", 49152, "http://127.0.0.1:49152/lab?token=fixture"},
		"https stays https":               {"https://localhost:3000", 49153, "https://127.0.0.1:49153"},
		"another loopback host is local":  {"http://[::1]:5173/app/#/route", 49154, "http://127.0.0.1:49154/app/#/route"},
		"the announced port is replaced":  {"http://127.0.0.1:8080/", 8080, "http://127.0.0.1:8080/"},
		"a manual forward opens its root": {"", 49155, "http://127.0.0.1:49155"},
		"an unparsed URL opens the root":  {"://broken", 49156, "http://127.0.0.1:49156"},
		"another scheme opens the root":   {"ws://localhost:9000/socket", 49157, "http://127.0.0.1:49157"},
	} {
		t.Run(name, func(t *testing.T) {
			t.Parallel()
			if got := forwardURL(test.announced, test.port); got != test.want {
				t.Fatalf("forwardURL(%q, %d) = %q, want %q", test.announced, test.port, got, test.want)
			}
		})
	}
}

func TestRestoreStep(t *testing.T) {
	t.Parallel()
	for name, test := range map[string]struct {
		kind      string
		connected bool
		restores  int
		attempt   bool
		next      int
	}{
		"a listening forward is left alone":            {"listening", true, 0, false, 0},
		"a forward the user or its service closed":     {"closed", true, 0, false, 0},
		"a lost forward waits for its Host":            {"failed", false, 0, false, 0},
		"the Host returned":                            {"failed", true, 0, true, 0},
		"the last attempt":                             {"failed", true, forwardRestores - 1, true, forwardRestores - 1},
		"the attempts are used up":                     {"failed", true, forwardRestores, false, forwardRestores},
		"losing the Host again starts the count over":  {"failed", false, forwardRestores, false, 0},
		"a forward restored earlier keeps no attempts": {"listening", false, 2, false, 2},
	} {
		t.Run(name, func(t *testing.T) {
			t.Parallel()
			if attempt, next := restoreStep(test.kind, test.connected, test.restores); attempt != test.attempt || next != test.next {
				t.Fatalf("restoreStep(%q, %v, %d) = %v, %d", test.kind, test.connected, test.restores, attempt, next)
			}
		})
	}
}
