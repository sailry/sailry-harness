package mcp

import (
	"os"
	"path/filepath"
	"strings"
	"testing"
)

func TestStdioCommandStartsWindowsBatchShim(t *testing.T) {
	t.Setenv("Path", t.TempDir())
	bin := filepath.Join(t.TempDir(), "npm bin")
	if err := os.Mkdir(bin, 0700); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(bin, "fixture-mcp"), []byte("#!/bin/sh\nexit 1\n"), 0600); err != nil {
		t.Fatal(err)
	}
	batch := "@echo off\r\n" +
		"if not \"%~1\"==\"serve\" exit /b 1\r\n" +
		"if not \"%~2\"==\"--mcp\" exit /b 2\r\n" +
		"if not \"%~3\"==\"a & b\" exit /b 3\r\n" +
		"echo ready\r\n"
	if err := os.WriteFile(filepath.Join(bin, "fixture-mcp.cmd"), []byte(batch), 0600); err != nil {
		t.Fatal(err)
	}
	command, err := stdioCommand(Launch{Command: "fixture-mcp", Args: []string{"serve", "--mcp", "a & b"},
		Environment: []string{"Path=" + bin, "Pathext=CMD"}, Directory: t.TempDir()})
	if err != nil {
		t.Fatal(err)
	}
	output, err := command.Output()
	if err != nil || strings.TrimSpace(string(output)) != "ready" {
		t.Fatalf("batch shim output = %q, error = %v", output, err)
	}
}
