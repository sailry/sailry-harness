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
