package live

import (
	"os"
	"path/filepath"
	"testing"
	"testing/synctest"

	"github.com/sailry/sailry-harness/internal/core"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/link"
	nodefiles "github.com/sailry/sailry-harness/internal/node/files"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestWorkspaceFileClickOpensOnlyPreviewableContent(t *testing.T) {
	synctest.Test(t, func(t *testing.T) {
		fixture := newPluginLocationTestFixture(t)
		root := fixture.root
		root.busy, root.uncertain = map[string]bool{}, map[string]protocol.Request{}
		root.initAttachments()
		t.Cleanup(root.closeAttachments)
		project := root.workspaceFor(fixture.main)
		project.initialized = true
		root.workspace.selected = fixture.main.ID
		directory, err := filepath.EvalSymlinks(t.TempDir())
		if err != nil {
			t.Fatal(err)
		}
		const licence, source, slow = "LICENSE", "program.fixturedata", "slow.txt"
		const licenceText = "Permission is granted to use this fixture.\n"
		const sourceText = "Text with an unusual suffix.\n"
		macho := []byte{0xcf, 0xfa, 0xed, 0xfe, 0, 0, 0, 0}
		for name, data := range map[string][]byte{
			licence: []byte(licenceText), source: []byte(sourceText), slow: []byte("Superseded content\n"),
			"sailry-desktop": macho, "binary.go": macho,
		} {
			if err := os.WriteFile(filepath.Join(directory, name), data, 0o600); err != nil {
				t.Fatal(err)
			}
		}
		late := make(chan link.Response, 1)
		fixture.source.dispatch = func(request protocol.Request) (link.Admission, error) {
			command := request.Command
			switch command.Kind {
			case "read_file":
				input, _ := detail[protocol.CommandReadFilePayload](command.Data)
				if input.Path == slow {
					return link.Admission{Receipt: protocol.Receipt{ID: request.ID}, Completion: late}, nil
				}
				command.Data = &input
			case "list_directory":
				input, _ := detail[protocol.CommandListDirectoryPayload](command.Data)
				command.Data = &input
			default:
				return workspaceFilesAdmission(request, protocol.Output{}, fileActionFault(protocol.ErrorCodeInvalidRequest, "unexpected file action"))
			}
			out, err := nodefiles.Inspect(root.ctx, directory, command, core.ManualFiles, nil)
			return workspaceFilesAdmission(request, out, err)
		}
		window := workspaceFilesWindow(root, project, nil)
		settle := func() {
			for range 4 {
				synctest.Wait()
				window.Frame()
			}
		}
		click := func(name string) {
			t.Helper()
			project.openWorkspaceFile(name, 0)
			settle()
		}
		before := project.panel.Files.File()
		click("sailry-desktop")
		if project.panel.Files.File() != before || len(root.attachments.previews) != 0 {
			t.Fatal("binary created a preview or changed the active editor")
		}
		click(licence)
		if project.panel.Files.File() != licence || project.files[licence].Text != licenceText || !project.panel.FileLoaded(licence) {
			t.Fatalf("the extensionless file did not load in the editor: %q", window.Texts())
		}
		click("binary.go")
		if project.panel.Files.File() != licence || len(root.attachments.previews) != 0 || window.HasText(i18n.In("en", "files_open_failed")) {
			t.Fatalf("binary content with a source extension did not stay selected only: %q", window.Texts())
		}
		click(slow)
		if project.panel.Files.File() != licence || project.openingFile == nil {
			t.Fatal("pending read showed an editor before the file was identified")
		}
		click(source)
		late <- link.Response{Output: protocol.Output{Kind: "file_content", Data: protocol.FileContent{Path: slow, Text: "Superseded content\n"}}}
		settle()
		if project.panel.Files.File() != source || project.files[source].Text != sourceText || project.openingFile != nil {
			t.Fatal("the superseded read replaced the selected text file")
		}
		if _, retained := project.files[slow]; retained {
			t.Fatal("the superseded read kept content without an editor")
		}
		reads := 0
		for _, kind := range fixture.source.commandKinds() {
			switch kind {
			case "read_file":
				reads++
			case "list_directory":
			default:
				t.Fatalf("a file click started an unrelated or external command: %s", kind)
			}
		}
		if reads != 5 {
			t.Fatalf("file identification must reuse the editor read: got %d reads for 5 clicks", reads)
		}
	})
}
