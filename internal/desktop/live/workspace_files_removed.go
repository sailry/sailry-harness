package live

import (
	"path"
	"strings"
)

func workspaceEntryContains(parent, relative string) bool {
	return parent == "" || relative == parent || strings.HasPrefix(relative, parent+"/")
}

func workspaceReadEntry(key string) (relative string, directory, found bool) {
	if relative, found = strings.CutPrefix(key, "directory/"); found {
		return relative, true, true
	}
	if relative, found = strings.CutPrefix(key, "file/"); found {
		return relative, false, true
	}
	if checked, checking := strings.CutPrefix(key, "file-check/"); checking {
		_, relative, found = strings.Cut(checked, "/")
	}
	return relative, false, found
}

func (p *workspaceProject) entryMissing(relative string) {
	if p.missingEntries == nil {
		p.missingEntries = map[string]bool{}
	}
	p.missingEntries[relative] = true
	for key := range p.loading {
		filename, _, entry := workspaceReadEntry(key)
		if entry && filename != relative && workspaceEntryContains(relative, filename) {
			if p.staleReads == nil {
				p.staleReads = map[string]bool{}
			}
			p.staleReads[key] = true
		}
	}
	for _, filename := range p.panel.Unsaved() {
		if workspaceEntryContains(relative, filename) {
			p.draftSource(filename, changedFile{missing: true})
		}
	}
	for filename := range p.files {
		if workspaceEntryContains(relative, filename) {
			if _, dirty := p.panel.FileDraft(filename); !dirty {
				p.clearChanged(filename)
				p.fileUnavailable(filename)
			}
		}
	}
}

func (p *workspaceProject) entryIsMissing(relative string) bool {
	for {
		if p.missingEntries[relative] {
			return true
		}
		if relative == "" {
			return false
		}
		parent := path.Dir(relative)
		if parent == relative || parent == "." {
			relative = ""
		} else {
			relative = parent
		}
	}
}

// A missing path is retried after a listing, read or confirmed write sees it
// again, not when an unrelated file wakes the workspace watch.
func (p *workspaceProject) entryFound(relative string) {
	for {
		if p.missingEntries[relative] {
			delete(p.missingEntries, relative)
			delete(p.failedReads, "file/"+relative)
			p.pendingFiles = true
		}
		if relative == "" {
			return
		}
		parent := path.Dir(relative)
		if parent == relative || parent == "." {
			relative = ""
		} else {
			relative = parent
		}
	}
}

func (p *workspaceProject) trashBlocksRead(key string) bool {
	if len(p.trashing) == 0 {
		return false
	}
	relative, directory, entry := workspaceReadEntry(key)
	if directory {
		return true
	}
	if entry {
		for target := range p.trashing {
			if workspaceEntryContains(target, relative) {
				return true
			}
		}
	}
	return false
}

func (p *workspaceProject) beginTrash(relative string) {
	p.fileDirectoryFence++
	if p.trashing == nil {
		p.trashing = map[string]bool{}
	}
	p.trashing[relative] = true
	for key := range p.loading {
		if p.trashBlocksRead(key) {
			if p.staleReads == nil {
				p.staleReads = map[string]bool{}
			}
			p.staleReads[key] = true
		}
	}
}

// Only a confirmed trash result retires these references. A failed or unknown
// mutation keeps its editors and its original request's recovery path.
func (p *workspaceProject) entryTrashed(relative string) {
	p.entryMissing(relative)
	paths := map[string]bool{}
	for _, filename := range p.panel.Unsaved() {
		paths[filename] = true
	}
	for filename := range p.files {
		paths[filename] = true
	}
	for _, filename := range p.panel.Files.Open() {
		paths[filename] = true
	}
	for filename := range paths {
		if !workspaceEntryContains(relative, filename) {
			continue
		}
		if _, dirty := p.panel.FileDraft(filename); dirty {
			continue
		}
		delete(p.files, filename)
		p.clearChanged(filename)
		p.panel.Files.Close(filename)
	}
	p.pruneDocumentImages()
}
