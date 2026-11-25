package store

import (
	"database/sql"
	"fmt"
	"slices"
	"testing"

	"github.com/sailry/sailry-harness/internal/node/plugins"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestPluginRuntimePreservesEnableChoiceAndRejectsDependencies(t *testing.T) {
	t.Parallel()
	_, _, storage := schemaFixture(t)
	allowed := plugins.WithRuntime(t.Context(), plugins.Runtime{ProductID: "harness", Modules: []string{"files"}})
	blocked := plugins.WithRuntime(t.Context(), plugins.Runtime{ProductID: "code"})
	if err := storage.Node.Write(allowed, func(db *sql.Conn) error {
		info := pluginTestInfo("runtime-fixture", 'a')
		info.Requirements = &protocol.PluginRequirements{APIVersion: "v1", Products: []string{"harness"}, RequiresModules: []string{"files"}}
		if _, err := InstallPlugin(blocked, db, info, 0, true); err == nil {
			return fmt.Errorf("incompatible installation was accepted")
		}
		installed, err := InstallPlugin(allowed, db, info, 0, true)
		if err != nil {
			return err
		}
		read, err := ReadPlugin(blocked, db, info.Summary.Name)
		if err != nil || read == nil || !read.Summary.Enabled || read.Summary.Compatibility.Kind != "unsupported" {
			return fmt.Errorf("incompatible inventory lost its enable choice: %+v %v", read, err)
		}
		if _, err := SetPluginEnabled(blocked, db, info.Summary.Name, installed.Summary.Revision, true); err == nil {
			return fmt.Errorf("incompatible enabling was accepted")
		}
		if _, err := ResolvePluginInfo(blocked, db, plugins.PackageReference(installed)); err == nil {
			return fmt.Errorf("incompatible captured version was resolved")
		}
		disabled, err := SetPluginEnabled(blocked, db, info.Summary.Name, installed.Summary.Revision, false)
		if err != nil || disabled.Summary.Enabled {
			return fmt.Errorf("incompatible package could not be disabled: %v", err)
		}
		if _, err := SetPluginEnabled(allowed, db, info.Summary.Name, disabled.Summary.Revision, true); err != nil {
			return err
		}
		return nil
	}); err != nil {
		t.Fatal(err)
	}
}

func TestPreInventoryCaptureUsesAssembledModules(t *testing.T) {
	t.Parallel()
	_, _, storage := schemaFixture(t)
	ctx := plugins.WithRuntime(t.Context(), plugins.Runtime{ProductID: "harness"})
	if err := storage.Node.Read(ctx, func(db *sql.Conn) error {
		references, err := captureTurnPlugins(ctx, db, nil)
		if err != nil {
			return err
		}
		if slices.ContainsFunc(references, func(reference protocol.PluginReference) bool { return reference.Name == "files" }) {
			return fmt.Errorf("pre-inventory capture admitted a missing native module")
		}
		return nil
	}); err != nil {
		t.Fatal(err)
	}
}
