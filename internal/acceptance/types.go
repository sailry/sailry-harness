// Package acceptance runs explicitly requested acceptance cases through an
// already started Client. It never owns a Node or changes controller defaults.
package acceptance

import (
	"time"

	"github.com/sailry/sailry-harness/internal/protocol"
)

type Options struct {
	Suite       string                  `json:"suite"`
	Config      *protocol.SessionConfig `json:"config,omitempty"`
	Cases       []string                `json:"cases,omitempty"`
	CaseTimeout time.Duration           `json:"case_timeout,omitempty"`
}

type Evidence struct {
	Kind  string `json:"kind"`
	Value string `json:"value"`
}

type CaseResult struct {
	ID        string        `json:"id"`
	Group     string        `json:"group"`
	Title     string        `json:"title"`
	Status    string        `json:"status"`
	Detail    string        `json:"detail"`
	Duration  time.Duration `json:"duration"`
	Session   string        `json:"session,omitempty"`
	Turns     []string      `json:"turns"`
	Evidence  []Evidence    `json:"evidence"`
	Uncertain bool          `json:"uncertain,omitempty"`
}

type Report struct {
	Version  int              `json:"version"`
	Suite    string           `json:"suite"`
	Started  time.Time        `json:"started"`
	Finished time.Time        `json:"finished"`
	Provider string           `json:"provider"`
	Model    string           `json:"model"`
	Effort   *protocol.Effort `json:"effort,omitempty"`
	Cases    []CaseResult     `json:"cases"`
}

// OK requires an actual passing case and rejects incomplete execution.
func (report Report) OK() bool {
	passed := false
	for _, result := range report.Cases {
		switch result.Status {
		case "passed":
			passed = true
		case "skipped":
		default:
			return false
		}
	}
	return passed
}
