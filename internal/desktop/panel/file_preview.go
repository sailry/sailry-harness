package panel

import (
	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/fileview"
)

type fileDocumentPreview struct {
	view    fileview.View
	draw    func(*ui.Context, *fileview.View)
	suspend func()
}

func (v *FileView) releaseItem(path string) {
	if item := v.items[path]; item != nil {
		item.closePreview()
		delete(v.items, path)
	}
}

func (item *fileItem) closePreview() {
	if item.preview != nil {
		item.preview.suspend()
		item.preview = nil
	}
}

func (pr *Project) ShowFilePreview(path string, draw func(*ui.Context, *fileview.View), suspend func()) {
	pr.Files.Show(path)
	item := pr.Files.item(path)
	item.closePreview()
	item.preview = &fileDocumentPreview{draw: draw, suspend: suspend}
}

func (pr *Project) SuspendFilePreviews() {
	for _, item := range pr.Files.items {
		if item.preview != nil {
			item.preview.suspend()
			item.preview.view = fileview.View{}
		}
	}
}

func (pr *Project) ClearFilePreview(path string) {
	if item := pr.Files.items[path]; item != nil {
		item.closePreview()
	}
}

func (pr *Project) HasFilePreview(path string) bool {
	item := pr.Files.items[path]
	return item != nil && item.preview != nil
}
