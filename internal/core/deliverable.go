package core

import (
	"path"
	"strings"
)

// deliverables are the extensions of files a conversation produces for the
// user to open: documents, spreadsheets, presentations, PDFs, standalone web
// pages, data exports, archives and media. Source code and configuration are
// references, and images show as pictures.
var deliverables = map[string]bool{
	".doc": true, ".docx": true, ".odt": true, ".rtf": true, ".pages": true,
	".xls": true, ".xlsx": true, ".ods": true, ".numbers": true, ".csv": true, ".tsv": true,
	".ppt": true, ".pptx": true, ".odp": true, ".key": true,
	".pdf": true, ".epub": true, ".md": true, ".markdown": true, ".txt": true,
	".html": true, ".htm": true,
	".zip": true, ".tar": true, ".gz": true, ".tgz": true, ".7z": true, ".rar": true,
	".mp3": true, ".wav": true, ".m4a": true, ".flac": true, ".ogg": true,
	".mp4": true, ".mov": true, ".webm": true, ".mkv": true,
}

// DeliverableFile reports whether a file name is a produced deliverable that
// a reply shows as a file card when it links it alone in a paragraph.
func DeliverableFile(name string) bool {
	return deliverables[strings.ToLower(path.Ext(name))]
}
