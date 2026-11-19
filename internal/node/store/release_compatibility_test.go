package store

import (
	"database/sql"
	"os"
	"path/filepath"
	"testing"

	"github.com/sailry/sailry-harness/internal/core"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestV020ActiveStorageUpgradesInPlaceBesideArchive(t *testing.T) {
	t.Parallel()
	ctx := t.Context()
	directory := databaseDirectory(t)
	profile := filepath.Dir(directory)
	node := protocol.NodeID{19}
	session, worktree, owner := protocol.NewSessionID(), protocol.NewWorktreeID(), protocol.NewUserID()

	// Both SQL fixtures are unmodified from v0.2.0 (2b16cad8575f). Do not
	// derive them by removing columns from today's schema.
	originals := map[string]os.FileInfo{}
	for _, name := range []string{"node", "knowledge"} {
		schema, err := os.ReadFile(filepath.Join("testdata", "v0.2.0-"+name+"-schema.sql"))
		if err != nil {
			t.Fatal(err)
		}
		path := filepath.Join(directory, name+".turso")
		handle, err := OpenHandle(ctx, path)
		if err != nil {
			t.Fatal(err)
		}
		t.Cleanup(func() {
			if err := handle.Close(); err != nil {
				t.Error(err)
			}
		})
		schemaExecute(t, handle, string(schema))
		if name == "node" {
			schemaExecute(t, handle, `PRAGMA application_id=0x5341494c; PRAGMA user_version=1;
INSERT INTO counters VALUES('sessions',1);
INSERT INTO local_model_settings VALUES(1,CAST('{"revision":4,"mirror":null,"choices":{}}' AS BLOB));
INSERT INTO telemetry_settings VALUES(1,CAST('{"revision":1,"errors":false,"metrics":false,"installation_id":"0197c0de-0000-7000-8000-000000000005"}' AS BLOB))`)
			schemaExecute(t, handle, "INSERT INTO node(singleton,identity,plugin_defaults,tool_defaults,tool_metadata_names) VALUES(1,?1,1,1,1)", node[:])
			schemaExecute(t, handle, "INSERT INTO users(id,name,administrator,local) VALUES(?1,'Release user',1,1)", owner.String())
			schemaExecute(t, handle, "INSERT INTO worktrees(id,owner,path,main) VALUES(?1,?2,?3,1)", worktree.String(), owner.String(), filepath.Join(profile, "worktrees", worktree.String()))
			schemaExecute(t, handle, "INSERT INTO sessions(rowid,id,state,owner,worktree,revision) VALUES(1,?1,'archived',?2,?3,1)", session.String(), owner.String(), worktree.String())
			schemaExecute(t, handle, "INSERT INTO session_revisions(session,revision,worktree,config,roles) VALUES(?1,1,?2,?3,?4)", session.String(), worktree.String(),
				[]byte(`{"provider":"0197c0de-0000-7000-8000-000000000006","model":"release-model","mode":"code","permission":"ask"}`), []byte(`{"execution":"team","profiles":[],"providers":[]}`))
			schemaExecute(t, handle, "INSERT INTO agent_state(owner,key,body) VALUES(?1,'sailry.session_title',?2)", session.String(), []byte(`"Release conversation"`))
		} else {
			schemaExecute(t, handle, "INSERT INTO collections(id,generation,scope,rebuild) VALUES('release',3,X'7B7D',0)")
		}
		if err := handle.Close(); err != nil {
			t.Fatal(err)
		}
		originals[path], err = os.Stat(path)
		if err != nil {
			t.Fatal(err)
		}
	}
	archive := filepath.Join(directory, "rust-archive")
	if err := core.PrivateDirectory(archive); err != nil {
		t.Fatal(err)
	}
	archived := filepath.Join(archive, "node.turso")
	if err := os.WriteFile(archived, []byte("inactive"), 0o600); err != nil {
		t.Fatal(err)
	}
	for range 2 {
		active := reopenSchemaFixture(t, profile, node)
		var saved protocol.Session
		if err := active.Node.Read(ctx, func(db *sql.Conn) error {
			var err error
			saved, err = ReadSession(ctx, db, session)
			return err
		}); err != nil || !saved.Archived || saved.Activity.Title != "Release conversation" || saved.Config.Model != "release-model" || saved.Worktree != worktree {
			t.Fatalf("release session was not retained: %+v, %v", saved, err)
		}
		if got := schemaInteger(t, active.Node, "SELECT storage_generation FROM node WHERE request_pruned_through='' AND singleton=1"); got != 1 {
			t.Fatalf("upgraded Node storage generation = %d", got)
		}
		if got := schemaInteger(t, active.Node, "SELECT count(*) FROM pragma_table_info('agent_delegations') WHERE name='stop_requested'"); got != 1 {
			t.Fatal("release delegation table did not receive the additive upgrade")
		}
		if got := schemaInteger(t, active.Knowledge, "SELECT generation FROM collections WHERE id='release'"); got != 3 {
			t.Fatalf("release knowledge generation = %d", got)
		}
		if err := active.Close(); err != nil {
			t.Fatal(err)
		}
		for path, before := range originals {
			if after, err := os.Stat(path); err != nil || !os.SameFile(before, after) {
				t.Fatalf("release database %s was replaced: %v", path, err)
			}
		}
		if body, err := os.ReadFile(archived); err != nil || string(body) != "inactive" {
			t.Fatalf("archive was changed: %v", err)
		}
	}
}
