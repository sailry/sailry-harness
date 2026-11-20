package core

import "testing"

func TestDeliverableFile(t *testing.T) {
	for name, want := range map[string]bool{
		"output/预算.XLSX": true, "report.docx": true, "slides.pptx": true, "report.pdf": true,
		"site/index.html": true, "data.csv": true, "notes.md": true, "bundle.zip": true, "clip.mp4": true,
		"main.go": false, "app.py": false, "config.json": false, "chart.png": false, "Makefile": false,
	} {
		if DeliverableFile(name) != want {
			t.Errorf("%s: want %v", name, want)
		}
	}
}
