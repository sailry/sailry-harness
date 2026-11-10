package main

import (
	"bytes"
	"context"
	"image"
	_ "image/gif"
	_ "image/jpeg"
	"image/png"

	"github.com/egoist/mygo"
	"github.com/sailry/sailry-harness/internal/desktop/exporter"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/live"
	"github.com/sailry/sailry-harness/internal/protocol"
	_ "golang.org/x/image/webp"
)

func bindFiles(entry *desktopWindow, local protocol.NodeID) {
	runtime := entry.live
	runtime.SetNativeFileActions(live.NativeFileActions{LocalNode: local, RenderDocument: func(ctx context.Context, artifact protocol.ArtifactSummary, html string, done func([]byte, error)) {
		go func() { data, err := exporter.PreviewImage(ctx, entry.window, html); done(data, err) }()
	}, PickFiles: func(selected func([]string)) {
		paths, err := mygo.Dialog.Open(mygo.OpenDialogOptions{Parent: entry.window, Title: i18n.In(i18n.Match(mygo.App.Locale()), "composer_attachment"), Multiple: true})
		if err != nil {
			runtime.Report(err)
			return
		}
		selected(paths)
	}, SaveFile: func(name string, selected func(string)) {
		path, err := mygo.Dialog.Save(mygo.SaveDialogOptions{Parent: entry.window, DefaultPath: name, CreateDirectories: true})
		if err != nil {
			runtime.Report(err)
			return
		}
		selected(path)
	}, OpenLocal: func(path string) {
		if err := mygo.Shell.OpenPath(path); err != nil {
			runtime.Report(err)
		}
	}, RevealLocal: mygo.Shell.ShowItemInFolder, CopyImage: func(data []byte, mime string) {
		if mime != "image/png" {
			decoded, _, err := image.Decode(bytes.NewReader(data))
			if err != nil {
				runtime.Report(err)
				return
			}
			var output bytes.Buffer
			if err = png.Encode(&output, decoded); err != nil {
				runtime.Report(err)
				return
			}
			data = output.Bytes()
		}
		if err := mygo.Clipboard.WriteImage(data); err != nil {
			runtime.Report(err)
		}
	}})
	runtime.SetWorkspaceNative(live.WorkspaceNative{Open: runtime.OpenWorkspaceExternal, Reveal: runtime.RevealWorkspaceFile, Download: runtime.DownloadWorkspaceFile, Insert: runtime.InsertWorkspaceReferences})
}
