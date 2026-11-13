package acceptance

import (
	"strings"

	"github.com/sailry/sailry-harness/internal/protocol"
)

func caseList(run *caseRun) error {
	if err := run.standard(); err != nil {
		return err
	}
	if err := run.seed("directory-marker.txt", "DIRECTORY-MARKER\n"); err != nil {
		return err
	}
	view, err := run.ask("Use list_directory to list this workspace root, and report the filename directory-marker.txt. Do not use any other tool.")
	if err != nil {
		return err
	}
	call, detail, err := run.requireTool(view, "list_directory")
	if err != nil {
		return err
	}
	if call.summary.Failure != nil || detail.Result == nil || !strings.Contains(string(*detail.Result), "directory-marker.txt") {
		return fail("Directory tool result did not contain the fixture filename")
	}
	return nil
}

func caseRead(run *caseRun) error {
	if err := run.standard(); err != nil {
		return err
	}
	if err := run.seed("lines.txt", "FIRST\nSECOND-SAILRY\nTHIRD-SAILRY\nFOURTH\n"); err != nil {
		return err
	}
	view, err := run.ask("Call read_file on lines.txt with offset=1 and limit=2 exactly once. Report the returned lines. Do not use another tool.")
	if err != nil {
		return err
	}
	call, detail, err := run.requireTool(view, "read_file")
	if err != nil {
		return err
	}
	args := arguments(detail)
	if call.summary.Failure != nil || argumentString(detail, "path") != "lines.txt" || string(args["offset"]) != "1" || string(args["limit"]) != "2" || detail.Result == nil || !strings.Contains(string(*detail.Result), "SECOND-SAILRY") || !strings.Contains(string(*detail.Result), "THIRD-SAILRY") || strings.Contains(string(*detail.Result), "FOURTH") {
		return fail("Canonical file read did not honor the requested line range")
	}
	run.evidence("read", "offset=1 limit=2 preserved in canonical tool arguments and result")
	return nil
}

func caseSearch(run *caseRun) error {
	if err := run.standard(); err != nil {
		return err
	}
	if err := run.seed("needle.go", "package fixture\n// SAILRY-SEARCH-9173\n"); err != nil {
		return err
	}
	if err := run.seed("ignore.txt", "ordinary text\n"); err != nil {
		return err
	}
	view, err := run.ask("Call find_files with globs=[\"*.go\"], then search_files with query=\"SAILRY-SEARCH-9173\" and mode=\"content\". Report the matching filename. Do not use another tool.")
	if err != nil {
		return err
	}
	for _, name := range []string{"find_files", "search_files"} {
		call, detail, err := run.requireTool(view, name)
		if err != nil {
			return err
		}
		if call.summary.Failure != nil || detail.Result == nil || !strings.Contains(string(*detail.Result), "needle.go") || strings.Contains(string(*detail.Result), "ignore.txt") {
			return fail("Search result omitted the matching fixture or included an unrelated file")
		}
	}
	return nil
}

func caseWrite(run *caseRun) error {
	if err := run.standard(); err != nil {
		return err
	}
	view, err := run.ask("Use write_file to create created.txt containing exactly SAILRY-CREATED followed by one newline, with expected_revision=null. Do not use another tool.")
	if err != nil {
		return err
	}
	if err := run.assertToolPath(view, "write_file", "created.txt"); err != nil {
		return err
	}
	return run.fileEquals("created.txt", "SAILRY-CREATED\n")
}

func caseFileEdit(run *caseRun) error {
	if err := run.standard(); err != nil {
		return err
	}
	if err := run.seed("edit.txt", "before\nkeep\n"); err != nil {
		return err
	}
	view, err := run.ask("Read edit.txt with read_file, then use edit_file and the returned revision to replace only before with after. Preserve the second line and final newline. Use no other tool.")
	if err != nil {
		return err
	}
	if err := run.assertToolPath(view, "edit_file", "edit.txt"); err != nil {
		return err
	}
	return run.fileEquals("edit.txt", "after\nkeep\n")
}

func casePatch(run *caseRun) error {
	if err := run.standard(); err != nil {
		return err
	}
	if err := run.seed("patch.txt", "old\n"); err != nil {
		return err
	}
	// Controller seeding does not record a model's observed file revision.
	// The patch tool requires that observation before updating an existing file.
	const patch = "*** Begin Patch\n*** Update File: patch.txt\n@@\n-old\n+new\n*** Add File: added.txt\n+SAILRY-PATCHED\n*** End Patch"
	view, err := run.ask("First read patch.txt with read_file exactly once, then use apply_patch exactly once with this exact patch. Use only those two tools:\n" + patch)
	if err != nil {
		return err
	}
	if err := run.assertToolPath(view, "read_file", "patch.txt"); err != nil {
		return err
	}
	calls := view.tools()
	if len(calls) != 2 || calls[0].name != "read_file" || calls[1].name != "apply_patch" {
		return fail("Patch fixture did not read its source before one multi-file patch")
	}
	call, detail, err := run.requireTool(view, "apply_patch")
	if err != nil {
		return err
	}
	if call.summary.Failure != nil || detail.Result == nil || argumentString(detail, "input") != patch {
		return fail("Patch did not complete as one canonical multi-file operation")
	}
	if err := run.fileEquals("patch.txt", "new\n"); err != nil {
		return err
	}
	return run.fileEquals("added.txt", "SAILRY-PATCHED\n")
}

func caseFileConflict(run *caseRun) error {
	if err := run.standard(); err != nil {
		return err
	}
	if err := run.seed("conflict.txt", "ORIGINAL\n"); err != nil {
		return err
	}
	view, err := run.ask("Use read_file to read conflict.txt. Do not edit it and do not use any other tool.")
	if err != nil {
		return err
	}
	if err := run.assertToolPath(view, "read_file", "conflict.txt"); err != nil {
		return err
	}
	before, err := run.read("conflict.txt")
	if err != nil {
		return err
	}
	if before.Revision == nil {
		return fail("Fixture read has no revision")
	}
	if _, err := run.command(run.ctx, "write_file", protocol.CommandWriteFilePayload{Worktree: run.session.Worktree, Path: "conflict.txt", Text: "EXTERNAL-CHANGE\n", ExpectedRevision: before.Revision}); err != nil {
		return err
	}
	_, err = run.command(run.ctx, "write_file", protocol.CommandWriteFilePayload{Worktree: run.session.Worktree, Path: "conflict.txt", Text: "STALE-OVERWRITE\n", ExpectedRevision: before.Revision})
	fault, ok := nodeFault(err)
	if !ok || fault.Code != protocol.ErrorCodeRevisionConflict {
		return fail("Stale write did not return revision_conflict")
	}
	run.evidence("conflict", "Stale controller revision rejected after a model read")
	return run.fileEquals("conflict.txt", "EXTERNAL-CHANGE\n")
}
