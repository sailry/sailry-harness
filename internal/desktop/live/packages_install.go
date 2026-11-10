package live

import (
	"context"
	"errors"
	"path/filepath"
	"strings"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/settings"
	"github.com/sailry/sailry-harness/internal/node/plugins"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func (r *Runtime) installGitPlugin(repository, revision, selectedPath string, done func(bool)) {
	source := protocol.PluginSkillsSource{Repository: strings.TrimSpace(repository), GitRef: optionalPackageText(revision), Path: optionalPackageText(selectedPath)}
	ctx, cancel := context.WithCancel(r.ctx)
	r.packages.downloadCancel = cancel
	r.packages.downloadGeneration++
	generation := r.packages.downloadGeneration
	r.async(func() {
		out, err := r.controller.Do(ctx, protocol.Command{Kind: "inspect_plugin_source", Data: protocol.CommandInspectPluginSourcePayload{Source: source}})
		selection, valid := detail[protocol.PluginCatalogSelection](out.Data)
		if err == nil && (!valid || out.Kind != "plugin_source") {
			err = errors.New("unexpected plugin source response")
		}
		r.enqueue(func(c *ui.Context) {
			if err == nil {
				err = ctx.Err()
			}
			cancel()
			if r.packages.downloadGeneration != generation {
				done(false)
				return
			}
			r.packages.downloadCancel = nil
			if errors.Is(err, context.Canceled) {
				done(false)
				return
			}
			if err != nil {
				r.packageReport(c, err)
				done(false)
				return
			}
			name := selection.Info.Summary.Name
			expected := r.packages.infos[name].Summary.Revision
			r.packageCommand(protocol.Command{Kind: "install_plugin_source", Data: protocol.CommandInstallPluginSourcePayload{Source: selection.Source, Path: selection.Path, Name: name, ExpectedRevision: expected}}, func(_ protocol.Output, ok bool) { done(ok) })
		})
	})
}

func (r *Runtime) uploadPackage(ctx context.Context, data []byte) (protocol.PluginUpload, error) {
	source, err := localSource(ctx, "", data, "application/octet-stream", plugins.MaxPackageBytes)
	if err != nil {
		return protocol.PluginUpload{}, err
	}
	defer source.reader.Close()
	out, err := r.controller.Do(ctx, protocol.Command{Kind: "upload_plugin", Data: protocol.PluginUploadSpec{Size: source.size, Revision: source.revision}})
	if err != nil {
		return protocol.PluginUpload{}, err
	}
	upload, valid := detail[protocol.PluginUpload](out.Data)
	if !valid || out.Kind != "plugin_upload" {
		return upload, errors.New("unexpected plugin upload response")
	}
	if err := r.controller.Upload(ctx, upload.Stream, source.size, source.revision, source.reader, nil); err != nil {
		r.cancelAttachmentStream(upload.Stream)
		return protocol.PluginUpload{}, err
	}
	return upload, nil
}

func (r *Runtime) installLocalPlugin(selectedPath, workingDirectory string, done func(bool)) {
	if !filepath.IsAbs(selectedPath) && workingDirectory == "" {
		if entry, exists := r.packages.catalog[selectedPath]; exists && entry.Bundled {
			r.packageCommand(protocol.Command{Kind: "install_bundled_plugin", Data: protocol.CommandInstallBundledPluginPayload{Name: selectedPath, ExpectedRevision: r.packages.infos[selectedPath].Summary.Revision}}, func(_ protocol.Output, ok bool) { done(ok) })
			return
		}
	}
	r.async(func() {
		data, format, kind, err := plugins.NewManager(filepath.Dir(filepath.Dir(r.Preferences.Path))).SnapshotSkills(r.ctx, selectedPath, workingDirectory)
		if err == nil && format != protocol.PluginSkillsArchiveFormatZip {
			err = errors.New("plugin packages require a directory or ZIP archive")
		}
		var upload protocol.PluginUpload
		var info protocol.PluginInfo
		if err == nil {
			upload, err = r.uploadPackage(r.ctx, data)
		}
		if err == nil {
			var out protocol.Output
			out, err = r.controller.Do(r.ctx, protocol.Command{Kind: "inspect_plugin_upload", Data: protocol.CommandInspectPluginUploadPayload{Stream: upload.Stream}})
			var valid bool
			info, valid = detail[protocol.PluginInfo](out.Data)
			if err == nil && (!valid || out.Kind != "plugin") {
				err = errors.New("unexpected uploaded plugin response")
			}
		}
		r.enqueue(func(c *ui.Context) {
			if err != nil {
				r.packageReport(c, err)
				done(false)
				return
			}
			name := info.Summary.Name
			r.packageCommand(protocol.Command{Kind: "install_plugin_upload", Data: protocol.CommandInstallPluginUploadPayload{Stream: upload.Stream, Source: kind, Name: name, ExpectedRevision: r.packages.infos[name].Summary.Revision}}, func(_ protocol.Output, ok bool) { done(ok) })
		})
	})
}

func (r *Runtime) chooseSkillSource(source int, done func(string)) {
	root := r.localRuntime()
	if source == 3 {
		if root.pickDirectory != nil {
			root.pickDirectory(func(path string) { root.enqueue(func(*ui.Context) { done(path) }) })
		}
		return
	}
	if root.attachments.native.PickFiles != nil {
		root.attachments.native.PickFiles(func(paths []string) {
			if len(paths) > 0 {
				root.enqueue(func(*ui.Context) { done(paths[0]) })
			}
		})
	}
}

type skillInstallation struct {
	source     *protocol.PluginSkillsResolved
	registry   *protocol.PluginSkillsRegistryResolved
	imported   *protocol.PluginSkillsImportDiscovery
	candidates []protocol.PluginSkillsCandidate
	operation  *skillSourceOperation
}

func (r *Runtime) installSkillSource(draft settings.SkillSourceDraft, done func(bool)) {
	r.cancelSkillSource()
	ctx, cancel := context.WithCancel(r.ctx)
	operation := &skillSourceOperation{ctx: ctx, cancel: cancel}
	r.packages.skillOperation = operation
	r.async(func() {
		discovery := skillInstallation{operation: operation}
		var err error
		if draft.Source <= 1 {
			repository := draft.Repository
			if draft.Source == 1 {
				repository = draft.Link
			}
			source := protocol.PluginSkillsSource{Repository: repository, GitRef: optionalPackageText(draft.Revision), Path: optionalPackageText(draft.Path)}
			var out protocol.Output
			out, err = r.controller.Do(ctx, protocol.Command{Kind: "discover_skills", Data: protocol.CommandDiscoverSkillsPayload{Source: source}})
			value, valid := detail[protocol.PluginSkillsDiscovery](out.Data)
			if err == nil && (!valid || out.Kind != "skill_discovery") {
				err = errors.New("unexpected skill discovery response")
			}
			discovery.source, discovery.candidates = &value.Source, value.Skills
		} else if draft.Source == 2 {
			var source *protocol.PluginSkillsRegistrySource
			source, err = plugins.RegistrySourceURL(draft.Link)
			if err == nil && source == nil {
				err = errors.New("a Skillhub URL is required")
			}
			if err == nil {
				var out protocol.Output
				out, err = r.controller.Do(ctx, protocol.Command{Kind: "discover_registry_skill", Data: protocol.CommandDiscoverRegistrySkillPayload{Source: *source}})
				value, valid := detail[protocol.PluginSkillsRegistryDiscovery](out.Data)
				if err == nil && (!valid || out.Kind != "registry_skill_discovery") {
					err = errors.New("unexpected registry skill response")
				}
				discovery.registry, discovery.candidates = &value.Source, value.Skills
			}
		} else {
			var data []byte
			var format protocol.PluginSkillsArchiveFormat
			var kind protocol.PluginUploadSource
			data, format, kind, err = plugins.NewManager(filepath.Dir(filepath.Dir(r.Preferences.Path))).SnapshotSkills(ctx, draft.Path, "")
			var upload protocol.PluginUpload
			if err == nil {
				upload, err = r.uploadPackage(ctx, data)
				if err == nil {
					operation.mu.Lock()
					operation.stream = upload.Stream
					operation.mu.Unlock()
				}
			}
			if err == nil {
				var out protocol.Output
				out, err = r.controller.Do(ctx, protocol.Command{Kind: "discover_skill_upload", Data: protocol.CommandDiscoverSkillUploadPayload{Stream: upload.Stream, Format: format, Source: kind}})
				value, valid := detail[protocol.PluginSkillsImportDiscovery](out.Data)
				if err == nil && (!valid || out.Kind != "skill_import_discovery") {
					err = errors.New("unexpected skill import response")
				}
				discovery.imported, discovery.candidates = &value, value.Skills
			}
		}
		r.enqueue(func(c *ui.Context) {
			if ctx.Err() != nil || r.packages.skillOperation != operation {
				r.finishSkillSource(operation, false)
				done(false)
				return
			}
			if err != nil {
				r.finishSkillSource(operation, false)
				r.packageReport(c, err)
				done(false)
				return
			}
			r.chooseDiscoveredSkills(discovery, done)
		})
	})
}

func (r *Runtime) chooseDiscoveredSkills(discovery skillInstallation, done func(bool)) {
	if len(discovery.candidates) == 0 {
		r.finishSkillSource(discovery.operation, false)
		r.enqueue(func(c *ui.Context) { r.packageReport(c, errors.New("the source contains no skills")); done(false) })
		return
	}
	if len(discovery.candidates) == 1 {
		r.finishSkillSource(discovery.operation, true)
		r.installDiscoveredSkill(discovery, 0, "", done)
		return
	}
	selection := &settings.SkillSelection{Open: true, Cancel: func() { r.finishSkillSource(discovery.operation, false) }}
	for _, candidate := range discovery.candidates {
		selection.Candidates = append(selection.Candidates, settings.SkillCandidate{Name: candidate.Name, Description: candidate.Skill.Description, Selected: true})
	}
	selection.Install = func(indices []int, finished func(bool)) {
		r.finishSkillSource(discovery.operation, true)
		r.installSkillBatch(discovery, indices, 0, finished)
	}
	r.App.Settings.PackageActions.Selection = selection
	done(true)
}

func (r *Runtime) installSkillBatch(discovery skillInstallation, indices []int, next int, done func(bool)) {
	if next == len(indices) {
		done(true)
		return
	}
	r.installDiscoveredSkill(discovery, indices[next], "", func(ok bool) {
		if !ok {
			done(false)
			return
		}
		r.installSkillBatch(discovery, indices, next+1, done)
	})
}

func (r *Runtime) installDiscoveredSkill(discovery skillInstallation, index int, existing string, done func(bool)) {
	if index < 0 || index >= len(discovery.candidates) {
		done(false)
		return
	}
	candidate := discovery.candidates[index]
	name := existing
	if name == "" {
		name = candidate.Name
	}
	revision := r.packages.infos[name].Summary.Revision
	command := protocol.Command{}
	switch {
	case discovery.source != nil:
		command = protocol.Command{Kind: "install_skill", Data: protocol.CommandInstallSkillPayload{Source: *discovery.source, Path: candidate.Path, Name: name, ExpectedRevision: revision}}
	case discovery.registry != nil:
		command = protocol.Command{Kind: "install_registry_skill", Data: protocol.CommandInstallRegistrySkillPayload{Source: *discovery.registry, Name: name, ExpectedRevision: revision}}
	case discovery.imported != nil:
		input := discovery.imported
		command = protocol.Command{Kind: "install_skill_upload", Data: protocol.CommandInstallSkillUploadPayload{Stream: input.Stream, Source: protocol.PluginSkillsImported{Digest: input.Digest, Path: candidate.Path, Kind: input.Kind}, Name: name, ExpectedRevision: revision}}
	}
	if command.Kind == "" {
		done(false)
		return
	}
	r.packageCommand(command, func(_ protocol.Output, ok bool) { done(ok) })
}
