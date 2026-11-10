package plugins

import (
	"context"
	"encoding/hex"
	"io"
	"net/url"
	"path"
	"path/filepath"
	"slices"
	"strings"
	"unicode"

	git "github.com/go-git/go-git/v5"
	"github.com/go-git/go-git/v5/plumbing"
	"github.com/go-git/go-git/v5/plumbing/filemode"
	"github.com/go-git/go-git/v5/plumbing/object"
	"github.com/go-git/go-git/v5/storage/memory"
	"github.com/sailry/sailry-harness/internal/protocol"
)

type RepositorySelection struct {
	Repository string
	GitRef     string
	Path       string
}

func commitHash(value string) bool {
	decoded, err := hex.DecodeString(value)
	return err == nil && len(decoded) == 20 && value == strings.ToLower(value)
}

func ParseRepositorySource(source protocol.PluginSkillsSource) (RepositorySelection, error) {
	selected := RepositorySelection{Repository: strings.TrimSpace(source.Repository), GitRef: "HEAD"}
	if source.GitRef != nil && strings.TrimSpace(*source.GitRef) != "" {
		selected.GitRef = strings.TrimSpace(*source.GitRef)
	}
	if source.Path != nil {
		selected.Path = strings.TrimSpace(*source.Path)
	}
	if selected.Repository == "" || len(selected.Repository) > 2048 || strings.ContainsFunc(selected.Repository, unicode.IsControl) ||
		selected.Path != "" && !resourcePath(selected.Path) || len(selected.Path) > 512 ||
		selected.GitRef == "" || len(selected.GitRef) > 255 || strings.ContainsFunc(selected.GitRef, unicode.IsSpace) ||
		strings.ContainsAny(selected.GitRef, "~^:?*[\\") || strings.Contains(selected.GitRef, "..") || strings.Contains(selected.GitRef, "@{") ||
		strings.HasPrefix(selected.GitRef, "-") || strings.HasSuffix(selected.GitRef, "/") || strings.HasSuffix(selected.GitRef, ".") {
		return selected, packageFault("invalid repository selection")
	}
	input := selected.Repository
	if filepath.IsAbs(input) {
		selected.Repository = (&url.URL{Scheme: "file", Path: filepath.ToSlash(input)}).String()
		return selected, nil
	}
	if !strings.Contains(input, ":") {
		parts := strings.Split(input, "/")
		if len(parts) != 2 || parts[0] == "" || parts[1] == "" {
			return selected, packageFault("a Git repository URL is required")
		}
		input = "https://github.com/" + input
	}
	address, err := url.Parse(input)
	if err == nil && address.Scheme != "" {
		if !slices.Contains([]string{"https", "http", "git", "ssh", "ssh+git", "git+ssh", "file"}, address.Scheme) ||
			address.RawQuery != "" || address.Fragment != "" || address.Scheme != "file" && address.Hostname() == "" ||
			address.Scheme == "file" && address.Host != "" && address.Host != "localhost" {
			return selected, packageFault("a Git repository URL without embedded credentials is required")
		}
		if address.User != nil {
			_, password := address.User.Password()
			if password || address.Scheme == "http" || address.Scheme == "https" || address.Scheme == "git" || address.Scheme == "file" {
				return selected, packageFault("repository credentials must not be embedded in its URL")
			}
		}
		if address.Hostname() == "github.com" && address.Scheme == "https" {
			parts := strings.Split(strings.Trim(address.Path, "/"), "/")
			if len(parts) < 2 || parts[0] == "" || parts[1] == "" {
				return selected, packageFault("invalid GitHub repository")
			}
			selected.Repository = "https://github.com/" + strings.ToLower(parts[0]) + "/" + strings.TrimSuffix(strings.ToLower(parts[1]), ".git") + ".git"
			if len(parts) > 2 {
				if parts[2] != "tree" || len(parts) < 4 {
					return selected, packageFault("unsupported GitHub repository URL")
				}
				tail := strings.Join(parts[3:], "/")
				if source.GitRef == nil {
					selected.GitRef = parts[3]
					tail = strings.Join(parts[4:], "/")
				} else if tail == selected.GitRef {
					tail = ""
				} else if strings.HasPrefix(tail, selected.GitRef+"/") {
					tail = strings.TrimPrefix(tail, selected.GitRef+"/")
				} else {
					return selected, packageFault("GitHub tree reference differs from its selection")
				}
				if source.Path == nil {
					selected.Path = tail
				}
			}
		} else {
			selected.Repository = address.String()
		}
		return selected, nil
	}
	if !strings.Contains(input, "://") && strings.Contains(input, "@") && strings.Contains(input, ":") && !strings.ContainsFunc(input, unicode.IsSpace) {
		host, directory, _ := strings.Cut(input, ":")
		if host != "" && directory != "" && !strings.Contains(host, "/") && !strings.Contains(directory, "\\") {
			selected.Repository = input
			return selected, nil
		}
	}
	return selected, packageFault("a Git repository URL is required")
}

type repositoryCaptureOptions struct {
	maximum  int
	licenses *[]packageEntry
}

func acquireRepository(ctx context.Context, selected RepositorySelection, expectedCommit string, options ...repositoryCaptureOptions) (protocol.PluginSkillsResolved, []packageEntry, []protocol.PluginIssue, error) {
	rules := repositoryCaptureOptions{maximum: MaxPackageEntries}
	if len(options) > 0 {
		rules = options[0]
	}
	resolved := protocol.PluginSkillsResolved{Repository: selected.Repository, GitRef: selected.GitRef}
	repository, err := git.CloneContext(ctx, memory.NewStorage(), nil, &git.CloneOptions{URL: selected.Repository, NoCheckout: true})
	if err != nil {
		return resolved, nil, nil, &protocol.Fault{Code: protocol.ErrorCodeUnavailable, Message: "plugin repository is unavailable"}
	}
	revision := selected.GitRef
	if expectedCommit != "" {
		if !commitHash(expectedCommit) {
			return resolved, nil, nil, packageFault("a resolved Git commit is required")
		}
		revision = expectedCommit
	}
	hash, err := repository.ResolveRevision(plumbing.Revision(revision))
	if err != nil && !strings.HasPrefix(revision, "refs/") && revision != "HEAD" && !commitHash(revision) {
		hash, err = repository.ResolveRevision(plumbing.Revision("refs/remotes/origin/" + revision))
	}
	if err != nil {
		return resolved, nil, nil, packageFault("plugin repository revision is unavailable")
	}
	commit, err := repository.CommitObject(*hash)
	if err != nil {
		return resolved, nil, nil, err
	}
	resolved.Commit = commit.Hash.String()
	tree, err := commit.Tree()
	if err != nil {
		return resolved, nil, nil, err
	}
	repositoryTree := tree
	if selected.Path != "" {
		tree, err = tree.Tree(selected.Path)
		if err != nil {
			return resolved, nil, nil, packageFault("plugin repository directory is unavailable")
		}
	}
	entries := map[string]packageEntry{}
	issues := []protocol.PluginIssue{}
	total := int64(0)
	iterator := tree.Files()
	defer iterator.Close()
	err = iterator.ForEach(func(file *object.File) error {
		if err := ctx.Err(); err != nil {
			return err
		}
		if !resourcePath(file.Name) || len(strings.Split(file.Name, "/")) > MaxPackageDepth {
			return packageFault("plugin repository path exceeds its limit")
		}
		if file.Mode != filemode.Regular && file.Mode != filemode.Executable && file.Mode != filemode.Deprecated {
			issues = append(issues, protocol.PluginIssue{Path: file.Name, Kind: protocol.PluginIssueKindUnavailablePath})
			return nil
		}
		if file.Size > MaxPackageBytes-total {
			return packageFault("plugin repository exceeds its expanded size limit")
		}
		reader, err := file.Reader()
		if err != nil {
			return err
		}
		data, err := io.ReadAll(io.LimitReader(reader, MaxPackageBytes-total+1))
		closeErr := reader.Close()
		if err != nil {
			return err
		}
		if closeErr != nil {
			return closeErr
		}
		total += int64(len(data))
		if total > MaxPackageBytes {
			return packageFault("plugin repository exceeds its expanded size limit")
		}
		entries[file.Name] = packageEntry{name: file.Name, data: data, executable: file.Mode == filemode.Executable}
		for directory := path.Dir(file.Name); directory != "."; directory = path.Dir(directory) {
			entries[directory] = packageEntry{name: directory, directory: true}
		}
		if len(entries) > rules.maximum {
			return packageFault("plugin repository exceeds its entry limit")
		}
		return nil
	})
	if err == nil && rules.licenses != nil {
		licenses := repositoryTree.Files()
		defer licenses.Close()
		err = licenses.ForEach(func(file *object.File) error {
			if !slices.Contains(sourceLicenseNames, file.Name) {
				return nil
			}
			if err := ctx.Err(); err != nil {
				return err
			}
			if file.Mode != filemode.Regular && file.Mode != filemode.Executable && file.Mode != filemode.Deprecated {
				return packageFault("repository license contains an unsupported file")
			}
			if file.Size > 1024*1024 || file.Size > MaxPackageBytes-total {
				return packageFault("repository license exceeds its size limit")
			}
			reader, err := file.Reader()
			if err != nil {
				return err
			}
			body, err := io.ReadAll(io.LimitReader(reader, 1024*1024+1))
			closeErr := reader.Close()
			if err != nil {
				return err
			}
			if closeErr != nil {
				return closeErr
			}
			if len(body) > 1024*1024 || len(body) != int(file.Size) {
				return packageFault("repository license exceeds its size limit")
			}
			total += int64(len(body))
			*rules.licenses = append(*rules.licenses, packageEntry{name: "SOURCE-" + file.Name, data: body})
			return nil
		})
	}
	values := make([]packageEntry, 0, len(entries))
	for _, entry := range entries {
		values = append(values, entry)
	}
	slices.SortFunc(values, func(left, right packageEntry) int {
		return slices.Compare(strings.Split(left.name, "/"), strings.Split(right.name, "/"))
	})
	return resolved, values, issues, err
}

func (manager *Manager) InspectRepository(ctx context.Context, source protocol.PluginSkillsSource) (protocol.PluginCatalogSelection, error) {
	selected, err := ParseRepositorySource(source)
	if err != nil {
		return protocol.PluginCatalogSelection{}, err
	}
	resolved, entries, issues, err := acquireRepository(ctx, selected, "")
	if err != nil {
		return protocol.PluginCatalogSelection{}, err
	}
	info, err := describePackage(entries, issues, "")
	if err != nil {
		return protocol.PluginCatalogSelection{}, err
	}
	info.Origin = &protocol.PluginOrigin{Kind: "online", Data: protocol.PluginOriginOnlinePayload{Source: resolved, Path: selected.Path}}
	manager.repositoryMu.Lock()
	manager.repositories[resolved.Repository+"\x00"+resolved.Commit+"\x00"+selected.Path] = entries
	manager.repositoryMu.Unlock()
	return protocol.PluginCatalogSelection{Source: resolved, Path: selected.Path, Info: info}, nil
}

func (manager *Manager) InstallRepository(ctx context.Context, selection protocol.PluginCatalogSelection) (protocol.PluginInfo, error) {
	source := protocol.PluginSkillsSource{Repository: selection.Source.Repository, GitRef: &selection.Source.GitRef, Path: &selection.Path}
	selected, err := ParseRepositorySource(source)
	if err != nil || selected.Repository != selection.Source.Repository || !commitHash(selection.Source.Commit) {
		return protocol.PluginInfo{}, packageFault("invalid immutable repository selection")
	}
	key := selection.Source.Repository + "\x00" + selection.Source.Commit + "\x00" + selection.Path
	manager.repositoryMu.Lock()
	entries, exists := manager.repositories[key]
	manager.repositoryMu.Unlock()
	issues := []protocol.PluginIssue{}
	if !exists {
		_, entries, issues, err = acquireRepository(ctx, selected, selection.Source.Commit)
		if err != nil {
			return protocol.PluginInfo{}, err
		}
	}
	if digestPackage(entries) != selection.Info.Summary.Digest {
		return protocol.PluginInfo{}, &protocol.Fault{Code: protocol.ErrorCodeRevisionConflict, Message: "plugin repository selection changed"}
	}
	origin := protocol.PluginOrigin{Kind: "online", Data: protocol.PluginOriginOnlinePayload{Source: selection.Source, Path: selection.Path}}
	info, err := manager.publish(ctx, entries, issues, selection.Info.Summary.Name, origin)
	if err == nil && !sameOptionalString(info.Summary.Version, selection.Info.Summary.Version) {
		return protocol.PluginInfo{}, packageFault("plugin repository version differs from its selection")
	}
	return info, err
}

func sameOptionalString(left, right *string) bool {
	return left == nil && right == nil || left != nil && right != nil && *left == *right
}
