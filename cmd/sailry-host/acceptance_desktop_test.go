//go:build integration

package main

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"os"
	"os/exec"
	"os/signal"
	"path/filepath"
	"runtime"
	"slices"
	"testing"
	"time"

	core "github.com/sailry/sailry-harness/internal/acceptance"
)

func acceptanceHelperArgs(mode string, paths ...string) []string {
	return append([]string{"-test.run=^TestAcceptanceProcessHelper$", "--", "--acceptance-test-helper", mode}, paths...)
}

// The helper is this test binary, with no Node, profile or model. Its descendants
// deliberately inherit stdout so the parent must handle an incomplete pipe.
func TestAcceptanceProcessHelper(t *testing.T) {
	index := slices.Index(os.Args, "--acceptance-test-helper")
	if index < 0 {
		return
	}
	args := os.Args[index+1:]
	if len(args) < 1 {
		os.Exit(2)
	}
	mode := args[0]
	emit := func(kind string, result *core.CaseResult, report *core.Report) {
		if err := json.NewEncoder(os.Stdout).Encode(struct {
			Kind   string           `json:"kind"`
			Case   *core.CaseResult `json:"case,omitempty"`
			Report *core.Report     `json:"report,omitempty"`
		}{kind, result, report}); err != nil {
			os.Exit(3)
		}
	}
	result := core.CaseResult{ID: "helper", Group: "ui", Status: "passed"}
	report := core.Report{Version: 1, Suite: "ui", Cases: []core.CaseResult{result}}
	waitFile := func(path string) {
		deadline := time.Now().Add(4 * time.Second)
		for time.Now().Before(deadline) {
			if _, err := os.Stat(path); err == nil {
				return
			}
			time.Sleep(5 * time.Millisecond)
		}
		os.Exit(4)
	}
	switch mode {
	case "stream":
		emit("case", &result, nil)
		waitFile(args[1])
		emit("report", nil, &report)
		os.Exit(0)
	case "held_pipe":
		command := exec.Command(os.Args[0], acceptanceHelperArgs("descendant", args[1])...)
		command.Stdout, command.Stderr = os.Stdout, os.Stderr
		if err := command.Start(); err != nil {
			os.Exit(5)
		}
		waitFile(args[1])
		emit("case", &result, nil)
		emit("report", nil, &report)
		os.Exit(0)
	case "descendant":
		if err := os.WriteFile(args[1], []byte(fmt.Sprint(os.Getpid())), 0o600); err != nil {
			os.Exit(6)
		}
		time.Sleep(time.Hour)
		os.Exit(0)
	case "interrupt":
		interrupted := make(chan os.Signal, 1)
		signal.Notify(interrupted, os.Interrupt)
		emit("case", &result, nil)
		<-interrupted
		result.ID, result.Status = "helper.stopped", "canceled"
		emit("case", &result, nil)
		report.Cases = append(report.Cases, result)
		emit("report", nil, &report)
		os.Exit(1)
	case "ignore_interrupt":
		signal.Ignore(os.Interrupt)
		emit("case", &result, nil)
		time.Sleep(time.Hour)
		os.Exit(0)
	case "after_report":
		signal.Ignore(os.Interrupt)
		emit("case", &result, nil)
		emit("report", nil, &report)
		time.Sleep(time.Hour)
		os.Exit(0)
	case "malformed":
		signal.Ignore(os.Interrupt)
		fmt.Fprintln(os.Stdout, "invalid acceptance JSON")
		time.Sleep(time.Hour)
		os.Exit(0)
	case "missing_report":
		emit("case", &result, nil)
		os.Exit(0)
	case "failed_report_only", "failed_cases", "changed_final_case", "updated_failed_case":
		if mode == "changed_final_case" {
			emit("case", &result, nil)
		} else if mode == "updated_failed_case" {
			result.Status = "running"
			emit("case", &result, nil)
		}
		result.Status, result.Detail = "failed", "The helper assertion failed"
		report.Cases = []core.CaseResult{result}
		if mode == "failed_cases" || mode == "updated_failed_case" {
			emit("case", &result, nil)
		}
		emit("report", nil, &report)
		os.Exit(1)
	}
	os.Exit(7)
}

func TestAcceptanceProcessStreamsBeforeChildExit(t *testing.T) {
	executable, err := os.Executable()
	if err != nil {
		t.Fatal(err)
	}
	gate := filepath.Join(t.TempDir(), "release")
	ctx, cancel := context.WithTimeout(t.Context(), 5*time.Second)
	defer cancel()
	emitted := 0
	err = runAcceptanceProcess(ctx, executable, acceptanceHelperArgs("stream", gate), nil, func(result core.CaseResult) {
		emitted++
		if result.ID != "helper" {
			t.Errorf("unexpected streamed case %q", result.ID)
		}
		if err := os.WriteFile(gate, []byte("consumed"), 0o600); err != nil {
			t.Error(err)
		}
	}, 100*time.Millisecond)
	if err != nil || emitted != 1 {
		t.Fatalf("streamed subprocess = cases %d, %v", emitted, err)
	}
}

func TestAcceptanceProcessHeldPipeHasBoundedExit(t *testing.T) {
	if runtime.GOOS == "windows" {
		t.Skip("the inherited pipe fixture uses a Unix process group")
	}
	executable, err := os.Executable()
	if err != nil {
		t.Fatal(err)
	}
	pidFile := filepath.Join(t.TempDir(), "descendant")
	t.Cleanup(func() {
		var pid int
		if data, err := os.ReadFile(pidFile); err == nil {
			if _, err := fmt.Sscan(string(data), &pid); err == nil {
				if process, err := os.FindProcess(pid); err == nil {
					_ = process.Kill()
				}
			}
		}
	})
	ctx, cancel := context.WithTimeout(t.Context(), 5*time.Second)
	defer cancel()
	started := time.Now()
	emitted := 0
	err = runAcceptanceProcess(ctx, executable, acceptanceHelperArgs("held_pipe", pidFile), nil, func(core.CaseResult) { emitted++ }, 100*time.Millisecond)
	if err == nil || ctx.Err() != nil || emitted != 1 || time.Since(started) > 3*time.Second {
		t.Fatalf("held pipe did not end at its own exit bound: emitted=%d elapsed=%s error=%v", emitted, time.Since(started), err)
	}
	if _, err := os.Stat(pidFile); err != nil {
		t.Fatal("helper never held stdout in a descendant")
	}
}

func TestAcceptanceProcessCancellationWaitsForCleanupAndKillsUnresponsiveChild(t *testing.T) {
	executable, err := os.Executable()
	if err != nil {
		t.Fatal(err)
	}
	for _, mode := range []string{"interrupt", "ignore_interrupt"} {
		t.Run(mode, func(t *testing.T) {
			if runtime.GOOS == "windows" && mode == "interrupt" {
				t.Skip("Windows cancels the child by terminating it")
			}
			ctx, cancel := context.WithTimeout(t.Context(), 5*time.Second)
			defer cancel()
			seen := []string{}
			started := time.Now()
			err := runAcceptanceProcess(ctx, executable, acceptanceHelperArgs(mode), nil, func(result core.CaseResult) {
				seen = append(seen, result.ID)
				if result.ID == "helper" {
					cancel()
				}
			}, 150*time.Millisecond)
			if !errors.Is(err, context.Canceled) || time.Since(started) > 3*time.Second {
				t.Fatalf("child cancellation did not finish: elapsed=%s error=%v", time.Since(started), err)
			}
			if mode == "interrupt" && !slices.Equal(seen, []string{"helper", "helper.stopped"}) {
				t.Fatalf("graceful cleanup events were lost: %v", seen)
			}
			if mode == "ignore_interrupt" && !slices.Equal(seen, []string{"helper"}) {
				t.Fatalf("unexpected unresponsive child output: %v", seen)
			}
		})
	}
}

func TestAcceptanceProcessRejectsBrokenOutput(t *testing.T) {
	executable, err := os.Executable()
	if err != nil {
		t.Fatal(err)
	}
	for _, mode := range []string{"malformed", "missing_report", "after_report"} {
		t.Run(mode, func(t *testing.T) {
			ctx, cancel := context.WithTimeout(t.Context(), 5*time.Second)
			defer cancel()
			err := runAcceptanceProcess(ctx, executable, acceptanceHelperArgs(mode), nil, nil, 100*time.Millisecond)
			if err == nil || ctx.Err() != nil {
				t.Fatalf("broken child was not rejected and reaped: %v", err)
			}
		})
	}
}

func TestAcceptanceProcessChecksFinalCaseReport(t *testing.T) {
	executable, err := os.Executable()
	if err != nil {
		t.Fatal(err)
	}
	for _, test := range []struct {
		mode     string
		statuses []string
		wantErr  bool
	}{
		{"failed_report_only", nil, true},
		{"failed_cases", []string{"failed"}, false},
		{"changed_final_case", []string{"passed"}, true},
		{"updated_failed_case", []string{"running", "failed"}, false},
	} {
		t.Run(test.mode, func(t *testing.T) {
			ctx, cancel := context.WithTimeout(t.Context(), 5*time.Second)
			defer cancel()
			var statuses []string
			err := runAcceptanceProcess(ctx, executable, acceptanceHelperArgs(test.mode), nil, func(result core.CaseResult) {
				statuses = append(statuses, result.Status)
			}, 100*time.Millisecond)
			if ctx.Err() != nil || (err != nil) != test.wantErr {
				t.Fatalf("final report validation = %v, want error %t (context %v)", err, test.wantErr, ctx.Err())
			}
			if test.wantErr && err.Error() != "Acceptance subprocess report differs from its case events" {
				t.Fatalf("final report mismatch was not identified: %v", err)
			}
			if !slices.Equal(statuses, test.statuses) {
				t.Fatalf("streamed case statuses = %v, want %v", statuses, test.statuses)
			}
		})
	}
}
