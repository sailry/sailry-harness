package plugins

import (
	"context"
	"encoding/hex"
	"errors"
	"io"
	"net/url"
	"os"
	"path"
	"path/filepath"
	"slices"
	"strings"
	"time"
	"unicode"

	"github.com/go-git/go-billy/v5/osfs"
	git "github.com/go-git/go-git/v5"
	"github.com/go-git/go-git/v5/config"
	"github.com/go-git/go-git/v5/plumbing"
	"github.com/go-git/go-git/v5/plumbing/cache"
	"github.com/go-git/go-git/v5/plumbing/filemode"
	"github.com/go-git/go-git/v5/plumbing/object"
	"github.com/go-git/go-git/v5/storage/filesystem"
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
		localPath := filepath.ToSlash(input)
		if !strings.HasPrefix(localPath, "/") {
			localPath = "/" + localPath
		}
		selected.Repository = (&url.URL{Scheme: "file", Path: localPath}).String()
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
			address.Scheme == "file" && (address.Path == "" || address.Opaque != "" || address.Host != "" && address.Host != "localhost") {
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

// One acquisition transfers a bounded revision of a plugin or skill repository.
const (
	maxRepositoryTransferBytes = 512 * 1024 * 1024
	repositoryTransferTimeout  = 120 * time.Second
	repositoryRevision         = "refs/sailry/source"
)

// transferStorage keeps a fetched repository on disk and stops a transfer
// whose pack grows beyond the remaining byte allowance.
type transferStorage struct {
	*filesystem.Storage
	remaining int64
	exceeded  bool
}

func (storage *transferStorage) PackfileWriter() (io.WriteCloser, error) {
	writer, err := storage.Storage.PackfileWriter()
	if err != nil {
		return nil, err
	}
	return &transferWriter{WriteCloser: writer, storage: storage}, nil
}

type transferWriter struct {
	io.WriteCloser
	storage *transferStorage
	ctx     context.Context
}

func (writer *transferWriter) Write(data []byte) (int, error) {
	if writer.ctx != nil {
		if err := writer.ctx.Err(); err != nil {
			return 0, err
		}
	}
	if int64(len(data)) > writer.storage.remaining {
		writer.storage.exceeded = true
		return 0, packageFault("Git transfer exceeds its size limit")
	}
	writer.storage.remaining -= int64(len(data))
	return writer.WriteCloser.Write(data)
}

type repositoryCaptureOptions struct {
	maximum  int
	licenses *[]packageEntry
	transfer int64 // pack bytes allowed; zero selects maxRepositoryTransferBytes
}

func acquireRepository(ctx context.Context, selected RepositorySelection, expectedCommit string, options ...repositoryCaptureOptions) (protocol.PluginSkillsResolved, []packageEntry, []protocol.PluginIssue, error) {
	rules := repositoryCaptureOptions{maximum: MaxPackageEntries}
	if len(options) > 0 {
		rules = options[0]
	}
	if rules.transfer == 0 {
		rules.transfer = maxRepositoryTransferBytes
	}
	resolved := protocol.PluginSkillsResolved{Repository: selected.Repository, GitRef: selected.GitRef}
	// A captured commit is fetched by its identifier; a moving reference is never resolved again.
	revision := selected.GitRef
	if expectedCommit != "" {
		if !commitHash(expectedCommit) {
			return resolved, nil, nil, packageFault("a resolved Git commit is required")
		}
		revision = expectedCommit
	}
	directory, err := os.MkdirTemp("", "sailry-plugin-repository-")
	if err != nil {
		return resolved, nil, nil, err
	}
	defer os.RemoveAll(directory)
	storage := &transferStorage{Storage: filesystem.NewStorage(osfs.New(directory), cache.NewObjectLRUDefault()), remaining: rules.transfer}
	defer storage.Close()
	repository, err := git.Init(storage.Storage, nil)
	if err != nil {
		return resolved, nil, nil, err
	}
	limited, cancel := context.WithTimeout(ctx, repositoryTransferTimeout)
	defer cancel()
	target := plumbing.Revision(repositoryRevision)
	if location, parseErr := url.Parse(selected.Repository); parseErr == nil && location.Scheme == "file" {
		var hash plumbing.Hash
		hash, err = acquireLocalRepository(limited, location, revision, directory, storage)
		target = plumbing.Revision(hash.String())
	} else {
		remote := git.NewRemote(storage, &config.RemoteConfig{Name: "origin", URLs: []string{selected.Repository}})
		fetch := func(depth int, specs ...config.RefSpec) error {
			err := remote.FetchContext(limited, &git.FetchOptions{RefSpecs: specs, Depth: depth, Tags: git.NoTags})
			if errors.Is(err, git.NoErrAlreadyUpToDate) {
				return nil
			}
			return err
		}
		err = fetch(1, config.RefSpec("+"+revision+":"+repositoryRevision))
		if errors.Is(err, git.ErrExactSHA1NotSupported) {
			// This server does not serve a commit by its identifier. Look for it in
			// the branches and tags instead, under the same size and time limits.
			target = plumbing.Revision(revision)
			err = fetch(0, "+refs/heads/*:refs/sailry/heads/*", "+refs/tags/*:refs/sailry/tags/*")
		}
	}
	switch {
	case err == nil:
	case ctx.Err() != nil:
		return resolved, nil, nil, ctx.Err()
	case storage.exceeded:
		return resolved, nil, nil, packageFault("Git transfer exceeds its size limit")
	case limited.Err() != nil:
		return resolved, nil, nil, &protocol.Fault{Code: protocol.ErrorCodeUnavailable, Message: "Git transfer timed out"}
	case errors.Is(err, git.NoMatchingRefSpecError{}), errors.Is(err, plumbing.ErrReferenceNotFound):
		return resolved, nil, nil, packageFault("plugin repository revision is unavailable")
	default:
		return resolved, nil, nil, &protocol.Fault{Code: protocol.ErrorCodeUnavailable, Message: "plugin repository is unavailable"}
	}
	hash, err := repository.ResolveRevision(target)
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
	// Only the newest inspection is kept for its installation; an older selection is acquired again.
	manager.repositoryMu.Lock()
	clear(manager.repositories)
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
	delete(manager.repositories, key)
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
