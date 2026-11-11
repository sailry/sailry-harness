package picker

import (
	"strings"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/icons"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

// Class is the family of an entry's type, which picks the glyph and the tint
// of its tile: folder, text, pdf, sheet, code, image, archive, audio, video
// or file.
func Class(e Entry) string {
	if e.Dir() {
		return "folder"
	}
	extension := ""
	if i := strings.LastIndex(e.Name, "."); i >= 0 {
		extension = strings.ToLower(e.Name[i+1:])
	}
	switch extension {
	case "pdf":
		return "pdf"
	case "txt", "md", "markdown", "json", "toml", "yaml", "yml", "log", "doc", "docx":
		return "text"
	case "csv", "xlsx", "numbers":
		return "sheet"
	case "go", "rs", "py", "js", "jsx", "ts", "tsx", "css", "html", "sh", "c", "h", "cpp", "java", "kt", "swift", "dart":
		return "code"
	case "png", "jpg", "jpeg", "gif", "svg", "webp":
		return "image"
	case "zip", "tar", "gz":
		return "archive"
	case "mp3", "wav":
		return "audio"
	case "mp4", "mov":
		return "video"
	}
	return "file"
}

// tile is the tinted square holding an entry's glyph, as the rows of
// CrychicUI's file list show it: side wide, the glyph half of that, in the
// status color of the entry's class over a wash of the same color. on marks a
// chosen row or cell, which the accent fills: its tile takes the text color
// on the accent.
func tile(c *ui.Context, e Entry, side float32, on bool) ui.Element {
	p := theme.Of(c)
	icon, ink := icons.File, p.MutedForeground
	switch Class(e) {
	case "folder":
		icon, ink = icons.Folder, p.Warning
	case "text":
		icon = icons.FileText
	case "pdf":
		icon, ink = icons.FileText, p.Danger
	case "sheet":
		icon, ink = icons.FileText, p.Success
	case "code":
		icon, ink = icons.FileCode, p.Primary
	case "image":
		icon, ink = icons.FileImage, p.Success
	case "archive":
		icon, ink = icons.Archive, p.Warning
	case "audio":
		icon, ink = icons.Volume, p.Danger
	case "video":
		icon, ink = icons.Video, p.Danger
	}
	if on {
		ink = p.PrimaryForeground
	}
	return ui.Box(c).Size(side, side).Shrink(0).Center().Radius(p.Radius).Background(ink.Alpha(0.14)).Children(func() {
		ui.Icon(c, icon).FontSize(side / 2).TextColor(ink)
	})
}
