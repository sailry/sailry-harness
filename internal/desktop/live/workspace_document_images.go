package live

import (
	"slices"

	"github.com/egoist/mygo/ui/markdown"
)

func (p *workspaceProject) documentMarkdown(path string) *markdown.Options {
	file, loaded := p.files[path]
	if !loaded {
		return nil
	}
	p.checkDocumentImagesRevision(path, file.Revision)
	if p.documentImages == nil {
		p.documentImages = map[string]*documentImages{}
	}
	images := p.documentImages[path]
	if images == nil {
		version := ""
		if file.Revision != nil {
			version = *file.Revision
		}
		descriptor := attachmentDescriptor{worktree: p.owner.ID, path: path, revision: file.Revision}
		images = newDocumentImages(p.runtime.ctx, p.runtime, descriptor, version)
		p.documentImages[path] = images
	}
	return images.options()
}

func (p *workspaceProject) checkDocumentImagesRevision(path string, revision *string) {
	images := p.documentImages[path]
	if images == nil {
		return
	}
	version := ""
	if revision != nil {
		version = *revision
	}
	if images.version != version {
		images.cancel()
		delete(p.documentImages, path)
	}
}

func (p *workspaceProject) pruneDocumentImages() {
	if len(p.documentImages) == 0 {
		return
	}
	open := p.panel.Files.Open()
	for path, images := range p.documentImages {
		if !slices.Contains(open, path) {
			images.cancel()
			delete(p.documentImages, path)
		}
	}
}

func (p *workspaceProject) closeDocumentImages() {
	for _, images := range p.documentImages {
		images.cancel()
	}
	p.documentImages = nil
}
