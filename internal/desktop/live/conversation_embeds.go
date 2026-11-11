package live

import (
	"net/url"
	"path"
	"strconv"
	"strings"
	"unicode"
	"unicode/utf8"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/conversation"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/protocol"
	"golang.org/x/net/html"
)

// imageReference reads sailry-image:<result>/<index>, how a reply names a
// picture a tool returned, as the key Image.Reference carries.
func imageReference(dest string) (string, bool) {
	const scheme = "sailry-image:"
	if len(dest) <= len(scheme) || !strings.EqualFold(dest[:len(scheme)], scheme) {
		return "", false
	}
	raw := dest[len(scheme):]
	slash := strings.LastIndexByte(raw, '/')
	if slash < 1 {
		return "", false
	}
	call, err := url.PathUnescape(raw[:slash])
	number := raw[slash+1:]
	if err != nil || call == "" || len(call) > 512 || !utf8.ValidString(call) || !decimal(number) || len(number) > 1 && number[0] == '0' {
		return "", false
	}
	if _, err = strconv.ParseUint(number, 10, 64); err != nil {
		return "", false
	}
	return call + "/" + number, true
}

// chatEmbed resolves a link or an image standing alone in a reply: a
// picture a tool returned by its reference, a workspace picture, or a
// workspace file's card. Links with a fragment or a query, and paths with a
// line, keep their ordinary navigation.
func (r *Runtime) chatEmbed(session, dest, _ string, image bool) (conversation.Embed, bool) {
	cv := r.App.Data.Find(session)
	if cv == nil {
		return conversation.Embed{}, false
	}
	owner := r.conversationRuntime(cv, nil)
	if owner == nil || owner.attachments == nil {
		return conversation.Embed{}, false
	}
	if reference, ok := imageReference(dest); ok && image {
		for ti := range cv.Turns {
			for _, block := range cv.Turns[ti].Reply.Blocks {
				for _, call := range block.Calls {
					if call.Output == nil {
						continue
					}
					for _, candidate := range call.Output.Images {
						if candidate.Reference == reference {
							if candidate.Missing && candidate.Source == nil {
								return conversation.Embed{Missing: true}, true
							}
							return conversation.Embed{Image: &candidate}, true
						}
					}
				}
			}
		}
		return conversation.Embed{Missing: true}, true
	}
	if strings.ContainsAny(dest, "#?") {
		return conversation.Embed{}, false
	}
	current, ok := owner.session(session)
	if !ok {
		return conversation.Embed{}, false
	}
	root := ""
	for _, candidate := range owner.snapshot().Worktrees {
		if candidate.ID == current.Worktree {
			root = candidate.Path
		}
	}
	target, ok := resolveFileLink(normalizeLink(dest), root)
	if !ok || root == "" || !target.workspace || target.path == "" || target.line != 0 || strings.HasSuffix(dest, "/") {
		return conversation.Embed{}, false
	}
	kind := attachmentFileKind(target.path, "")
	if !image && kind == conversation.ImageFile {
		return conversation.Embed{}, false
	}
	id := "link/" + target.path
	owner.registerWorkspaceFile(session, id, current.Worktree, target.path, nil)
	if image && kind == conversation.ImageFile && owner.attachmentConversationVisible(session) {
		cache := owner.attachmentImages(session)
		key := attachmentKey(session, id)
		picture := conversation.Image{ID: id, Name: path.Base(target.path)}
		if source := owner.projectAttachmentImage(cache, key); source != nil {
			picture.Source = source
			picture.Width, picture.Height = source.Size()
		}
		// A picture that cannot show falls back to its file's card.
		if !cache.missing[key] {
			return conversation.Embed{Image: &picture}, true
		}
	}
	file := conversation.File{ID: id, Path: target.path, Kind: kind,
		Editable: kind == conversation.TextFile || kind == conversation.CodeFile || kind == conversation.MarkdownFile}
	return conversation.Embed{File: &file}, true
}

// searchSuggestions are the links of a search provider's suggestion markup,
// which the reference shows in a scriptless web view: their text and their
// http or https destinations, at most 24.
func searchSuggestions(markup string) []conversation.Citation {
	root, err := html.Parse(strings.NewReader(markup))
	if err != nil {
		return nil
	}
	var out []conversation.Citation
	var text func(*html.Node, *strings.Builder)
	text = func(node *html.Node, b *strings.Builder) {
		if node.Type == html.TextNode {
			b.WriteString(node.Data)
		}
		for child := node.FirstChild; child != nil; child = child.NextSibling {
			text(child, b)
		}
	}
	var walk func(*html.Node)
	walk = func(node *html.Node) {
		if node.Type == html.ElementNode && (node.Data == "script" || node.Data == "style") {
			return
		}
		if node.Type == html.ElementNode && node.Data == "a" && len(out) < 24 {
			for _, attribute := range node.Attr {
				if attribute.Key != "href" {
					continue
				}
				parsed, err := url.Parse(strings.TrimSpace(attribute.Val))
				if err != nil || parsed.Scheme != "https" && parsed.Scheme != "http" || parsed.Host == "" || parsed.User != nil {
					continue
				}
				var label strings.Builder
				text(node, &label)
				out = append(out, conversation.Citation{URI: parsed.String(), Title: strings.Join(strings.Fields(label.String()), " ")})
			}
			return
		}
		for child := node.FirstChild; child != nil; child = child.NextSibling {
			walk(child)
		}
	}
	walk(root)
	return out
}

// exportDocument saves a published document's captured revision where the
// user chooses: its HTML, or the picture the document renders as.
func (r *Runtime) exportDocument(x *conversationActions, file conversation.File, image bool) {
	descriptor, ok := r.attachmentDescriptor(x.cv.ID, file.ID)
	native := r.attachments.native
	artifact := descriptor.artifact
	if !ok || artifact == nil || !r.controller.Connected() || native.SaveFile == nil ||
		image && (native.RenderDocument == nil || artifact.Kind != protocol.ArtifactKindDocument) {
		kit.Toast(x.c, kit.Failed, i18n.T(x.c, "artifact_export_failed"))
		return
	}
	name := []rune{}
	for _, character := range artifact.Title {
		if len(name) == 80 {
			break
		}
		if !unicode.IsLetter(character) && !unicode.IsNumber(character) && character != '-' && character != '_' {
			character = '_'
		}
		name = append(name, character)
	}
	if len(name) == 0 {
		name = []rune("document")
	}
	extension := "html"
	if image {
		extension = "png"
	}
	ctx := r.attachments.ctx
	native.SaveFile(string(name)+"-v"+strconv.FormatUint(uint64(artifact.Revision), 10)+"."+extension, func(target string) {
		if target == "" {
			return
		}
		finish := func(data []byte, err error) {
			if err == nil {
				var destination *downloadDestination
				if destination, err = prepareDownloadDestination(target); err == nil {
					if _, err = destination.file.Write(data); err == nil {
						err = destination.publish(ctx)
					}
					destination.close()
				}
			}
			r.enqueue(func(c *ui.Context) {
				if err != nil {
					r.reportKeys(c, err, "artifact_export_failed", "files_download_conflict", "workspace_action_unknown")
					return
				}
				kit.Toast(c, kit.Done, i18n.T(c, "files_download_done"))
			})
		}
		r.async(func() {
			content, err := r.readPublishedArtifact(ctx, descriptor)
			if err != nil || !image {
				finish([]byte(content.Html), err)
				return
			}
			r.enqueue(func(*ui.Context) {
				native.RenderDocument(ctx, *artifact, content.Html, func(data []byte, err error) {
					r.async(func() { finish(data, err) })
				})
			})
		})
	})
}
