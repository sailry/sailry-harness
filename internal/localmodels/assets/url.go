package assets

import (
	"net/url"
	"strings"

	"github.com/sailry/sailry-harness/internal/protocol"
)

// URL is the immutable address of one pinned asset. A mirror replaces only the
// Hugging Face origin.
func URL(asset protocol.ModelsAsset, mirror *string) (*url.URL, error) {
	if !ValidSource(asset.Source) || !hexadecimal(asset.Source.Revision, 40) || !ValidPath(asset.Path) {
		return nil, ErrInvalidAsset
	}
	base := "https://modelscope.cn/api/v1/models"
	if asset.Source.Hub == protocol.ModelsHubHuggingFace {
		base = "https://huggingface.co"
		if mirror != nil {
			base = *mirror
		}
	}
	address, err := url.Parse(base)
	if err != nil || !Origin(address) {
		return nil, ErrInvalidAsset
	}
	segments := strings.Split(asset.Source.Repository, "/")
	if asset.Source.Hub == protocol.ModelsHubHuggingFace {
		segments = append(segments, "resolve", asset.Source.Revision)
		segments = append(segments, strings.Split(asset.Path, "/")...)
	} else {
		segments = append(segments, "repo")
	}
	address = address.JoinPath(segments...)
	if asset.Source.Hub == protocol.ModelsHubModelScope {
		address.RawQuery = "Revision=" + url.QueryEscape(asset.Source.Revision) + "&FilePath=" + url.QueryEscape(asset.Path)
	}
	return address, nil
}

// Origin reports an HTTPS base address without credentials, query or fragment.
func Origin(address *url.URL) bool {
	return address.Scheme == "https" && address.Hostname() != "" && address.User == nil && address.RawQuery == "" && !address.ForceQuery && address.Fragment == ""
}

// ValidSource reports a repository of at most two segments and a revision,
// both limited to path-safe ASCII.
func ValidSource(source protocol.ModelsSource) bool {
	return reference(source.Repository) && strings.Count(source.Repository, "/") <= 1 && reference(source.Revision)
}

func reference(value string) bool {
	return value != "" && len(value) <= 256 && ValidPath(value)
}

// ValidPath reports slash-separated path-safe ASCII segments without dot
// segments.
func ValidPath(path string) bool {
	for part := range strings.SplitSeq(path, "/") {
		if part == "" || part == "." || part == ".." || strings.ContainsFunc(part, func(char rune) bool {
			return !(char >= 'a' && char <= 'z' || char >= 'A' && char <= 'Z' || char >= '0' && char <= '9' || char == '-' || char == '_' || char == '.')
		}) {
			return false
		}
	}
	return true
}
