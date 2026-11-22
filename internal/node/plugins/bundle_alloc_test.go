package plugins

import (
	"fmt"
	"os"
	"path/filepath"
	"strings"
	"testing"
)

func BenchmarkReadBundle(b *testing.B) {
	source := b.TempDir()
	write := func(name, text string) {
		b.Helper()
		if err := os.WriteFile(filepath.Join(source, name), []byte(text), 0o600); err != nil {
			b.Fatal(err)
		}
	}
	write("plugin.json", `{"$schema":"`+ManifestSchema+`","name":"fixture","version":"0.1.0"}`)
	write("entry.js", "export default 1")
	asset := strings.Repeat("x", 256<<10)
	for i := range 8 {
		write(fmt.Sprintf("asset-%d.bin", i), asset)
	}
	manager := NewManager(b.TempDir())
	info, err := manager.Install(b.Context(), source, "", "fixture")
	if err != nil {
		b.Fatal(err)
	}
	reference := PackageReference(info)
	b.ReportAllocs()
	for b.Loop() {
		files, err := manager.ReadBundle(b.Context(), reference, "entry.js", nil)
		if err != nil || files["entry.js"] != "export default 1" {
			b.Fatalf("read bundle: %v, %v", files, err)
		}
	}
}
