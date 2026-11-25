package preferences

import (
	"encoding/json"
	"strings"
	"testing"
)

func TestWelcomeMarkPreference(t *testing.T) {
	t.Parallel()
	d := Default()
	encoded, err := json.Marshal(d)
	if err != nil {
		t.Fatal(err)
	}
	if strings.Contains(string(encoded), "welcome_mark") {
		t.Fatal("an unset welcome lettering was written")
	}
	mark := "Hello, World! 2026 (v1.0)"
	d.WelcomeMark = &mark
	if err := d.Validate(); err != nil {
		t.Fatal(err)
	}
	b, err := json.Marshal(d)
	if err != nil {
		t.Fatal(err)
	}
	var back Data
	if err := json.Unmarshal(b, &back); err != nil || back.WelcomeMark == nil || *back.WelcomeMark != mark {
		t.Fatalf("welcome lettering came back as %v: %v", back.WelcomeMark, err)
	}
	for _, bad := range []string{"Café", "tab\there", strings.Repeat("x", WelcomeMarkMax+1)} {
		d.WelcomeMark = &bad
		if d.Validate() == nil {
			t.Errorf("welcome lettering %q validated", bad)
		}
	}
	if got := CleanWelcomeMark("Caf\u00e9\tBar\n" + strings.Repeat("y", 40)); got != "CafBar"+strings.Repeat("y", WelcomeMarkMax-6) {
		t.Fatalf("cleaned to %q", got)
	}
}
