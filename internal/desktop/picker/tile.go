package picker

import (
	"log"
	"path/filepath"
	"strconv"
	"strings"
	"time"

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

// SystemIcons are this computer's file manager icons, which the desktop
// application provides: File draws a local path's (a special folder, an
// application, a document), Type a file extension's and Folder a plain
// folder's, each as PNG data px pixels across. Without them, as in the
// preview and in tests, entries keep their glyphs.
var SystemIcons struct {
	File   func(path string, px int) ([]byte, error)
	Type   func(ext string, px int) ([]byte, error)
	Folder func(px int) ([]byte, error)
}

// systemIcon is a decoded system icon, nil where the system had none.
type systemIcon struct{ image ui.ImageSource }

var (
	// systemIcons keeps the icons read, by what was asked and its size,
	// failures too, so that each is read once. Only the UI thread reads it.
	systemIcons = map[string]systemIcon{}
	// iconFrame and iconReads limit the icons read in one frame: a large
	// directory fills in over a few frames rather than stalling one.
	iconFrame time.Time
	iconReads int
	// iconLogged keeps the log to the first failure: a system that draws no
	// icons would otherwise log every entry.
	iconLogged bool
)

const (
	iconBudget = 24   // the icons read in a frame
	iconLimit  = 4096 // the icons kept before the cache starts over
)

// systemImage is the system's icon of e in the directory shown, side DIPs
// across: a local entry's own, a remote one's by its type. It reports false
// where there is none, or none yet.
func (s *State) systemImage(c *ui.Context, e Entry, side float32) (ui.ImageSource, bool) {
	icons := SystemIcons
	if s.o.Preview || icons.File == nil || icons.Type == nil || icons.Folder == nil {
		return nil, false
	}
	// Twice the size keeps it sharp on a high-density display.
	px := int(side * 2)
	var key string
	var read func() ([]byte, error)
	switch {
	case !s.o.Remote:
		path := filepath.Join(s.listing.Path, e.Name)
		key, read = "path:"+path, func() ([]byte, error) { return icons.File(path, px) }
	case e.Dir():
		key, read = "folder", func() ([]byte, error) { return icons.Folder(px) }
	default:
		ext := ""
		if i := strings.LastIndex(e.Name, "."); i > 0 {
			ext = strings.ToLower(e.Name[i+1:])
		}
		key, read = "type:"+ext, func() ([]byte, error) { return icons.Type(ext, px) }
	}
	key += "@" + strconv.Itoa(px)
	if icon, ok := systemIcons[key]; ok {
		return icon.image, icon.image != nil
	}
	if now := c.Now(); now != iconFrame {
		iconFrame, iconReads = now, 0
	}
	if iconReads >= iconBudget {
		c.After(time.Millisecond)
		return nil, false
	}
	iconReads++
	if len(systemIcons) >= iconLimit {
		clear(systemIcons)
	}
	var icon systemIcon
	data, err := read()
	if err == nil {
		icon.image, err = ui.DecodeImage(data)
	}
	if err != nil && !iconLogged {
		iconLogged = true
		log.Printf("file picker: no system icon for %s: %v", key, err)
	}
	systemIcons[key] = icon
	return icon.image, icon.image != nil
}

// tile is an entry's icon: the system's, image across, where the desktop
// provides it, else a glyph side across in the status color of the entry's
// class. A system icon draws its own shape inside its square, so it takes
// more room than a glyph. A chosen row or cell is filled with the neutral
// selected color, over which the icon keeps its own.
func (s *State) tile(c *ui.Context, e Entry, side, image float32) ui.Element {
	if icon, ok := s.systemImage(c, e, image); ok {
		return ui.Image(c, icon).Size(image, image).Shrink(0).Fit(ui.Contain)
	}
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
