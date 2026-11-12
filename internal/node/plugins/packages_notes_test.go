package plugins

import (
	"reflect"
	"testing"
)

// The default changelog is the notes' text, not a locale named "md".
func TestReleaseNotesLocales(t *testing.T) {
	version := "0.1.0"
	notes := releaseNotes(map[string][]byte{
		"CHANGELOG.md":       []byte("## 0.1.0\nFirst release\n"),
		"CHANGELOG.zh-CN.md": []byte("## 0.1.0\n首个版本\n"),
	}, &version)
	if notes == nil || notes.Text != "First release" || !reflect.DeepEqual(notes.Locales, map[string]string{"zh-CN": "首个版本"}) {
		t.Fatalf("release notes = %+v", notes)
	}
}
