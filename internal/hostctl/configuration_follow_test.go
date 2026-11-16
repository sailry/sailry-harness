package hostctl

import "testing"

func TestParseConfigurationFollow(t *testing.T) {
	t.Parallel()
	for _, test := range []struct {
		args   []string
		valid  bool
		action string
	}{
		{[]string{"status"}, true, "status"}, {[]string{"sync", "--data-dir", "/fixture"}, true, "sync"},
		{[]string{"source", "local"}, true, "source"}, {[]string{"source", "none"}, true, "source"},
		{[]string{"target", "host", "--configuration", "off", "--secrets", "on"}, true, "target"},
		{[]string{"mcp-local", "plugin", "server", "on"}, true, "mcp-local"}, {[]string{"detach", "providers"}, true, "detach"},
		{[]string{"status", "--secrets", "on"}, false, ""}, {[]string{"source"}, false, ""}, {[]string{"mcp-local", "p", "s", "yes"}, false, ""},
		{[]string{"status", "--data-dir", "relative"}, false, ""}, {[]string{"target", "host", "--secrets", "yes"}, false, ""},
	} {
		options, help, err := ParseFollow(test.args)
		if (err == nil) != test.valid || help || test.valid && options.Action != test.action {
			t.Fatalf("ParseFollow(%q)=(%+v,%v,%v)", test.args, options, help, err)
		}
	}
}
