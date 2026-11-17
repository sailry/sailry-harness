package hostctl

import "testing"

func TestParseConfigurationFollow(t *testing.T) {
	t.Parallel()
	profile := t.TempDir()
	for _, test := range []struct {
		args   []string
		valid  bool
		action string
	}{
		{[]string{"status"}, true, "status"}, {[]string{"sync", "--data-dir", profile}, true, "sync"},
		{[]string{"source", "local"}, false, ""}, {[]string{"source", "none"}, false, ""},
		{[]string{"target", "host", "--configuration", "off", "--secrets", "on"}, false, ""},
		{[]string{"mcp-local", "plugin", "server", "on"}, false, ""}, {[]string{"detach", "providers"}, false, ""},
		{[]string{"status", "--secrets", "on"}, false, ""}, {[]string{"source"}, false, ""}, {[]string{"mcp-local", "p", "s", "yes"}, false, ""},
		{[]string{"status", "--data-dir", "relative"}, false, ""}, {[]string{"target", "host", "--secrets", "yes"}, false, ""},
	} {
		options, help, err := ParseFollow(test.args)
		if (err == nil) != test.valid || help || test.valid && options.Action != test.action {
			t.Fatalf("ParseFollow(%q)=(%+v,%v,%v)", test.args, options, help, err)
		}
	}
}
