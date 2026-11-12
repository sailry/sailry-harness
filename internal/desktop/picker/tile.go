package picker

import (
	"strings"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/icons"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

// Class is the family of an entry's type, which picks the glyph and the tint
// of its tile: folder, text, pdf, sheet, code, image, archive, package,
// audio, video or file.
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
	case "csv", "xls", "xlsx", "ods", "numbers":
		return "sheet"
	case "go", "rs", "py", "js", "jsx", "ts", "tsx", "css", "html", "sh", "c", "h", "cpp", "java", "kt", "swift", "dart":
		return "code"
	case "png", "jpg", "jpeg", "gif", "svg", "webp":
		return "image"
	case "zip", "tar", "gz", "tgz", "7z", "rar", "xz", "bz2", "zst", "iso":
		return "archive"
	case "deb", "rpm", "dmg", "pkg", "msi", "appimage", "apk":
		return "package"
	case "mp3", "wav":
		return "audio"
	case "mp4", "mov":
		return "video"
	}
	return "file"
}

// tile is an entry's glyph, side across, in the status color of the entry's
// class. A chosen row or cell is filled with the neutral selected color,
// over which the glyph keeps its own.
func tile(c *ui.Context, e Entry, side float32) ui.Element {
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
	case "package":
		icon, ink = icons.Package, p.Magenta
	case "audio":
		icon, ink = icons.Volume, p.Danger
	case "video":
		icon, ink = icons.Video, p.Danger
	}
	// The glyph alone, in its class's color: no wash behind it.
	return ui.Box(c).Size(side, side).Shrink(0).Center().Children(func() {
		ui.Icon(c, icon).FontSize(side).TextColor(ink)
	})
}
