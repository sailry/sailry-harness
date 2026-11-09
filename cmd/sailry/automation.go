//go:build e2e

package main

// The automation plugin drives the window from outside in test builds:
// go build -tags e2e, then MYGO_AUTOMATION=<socket>.
import _ "github.com/egoist/mygo/plugins/automation"
