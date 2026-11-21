package main

import (
	"bytes"
	"encoding/json"
	"log"
	"sync"

	"github.com/sailry/sailry-harness/internal/desktop/shell"
)

// Every window writes the same ui-state.json. Keep the newest snapshot while
// a write is in flight, and let a newly opened window read it immediately.
type windowPreferences struct {
	directory string
	write     func(string, shell.Prefs) error
	mu        sync.Mutex
	latest    []byte
	queued    bool
	done      chan struct{}
}

func (p *windowPreferences) load() shell.Prefs {
	p.mu.Lock()
	snapshot := p.latest
	p.mu.Unlock()
	if snapshot != nil {
		var prefs shell.Prefs
		if json.Unmarshal(snapshot, &prefs) == nil {
			return prefs
		}
	}
	return shell.LoadPrefs(p.directory)
}

func (p *windowPreferences) save(prefs shell.Prefs) {
	// Panes and preview preferences can still point into the live UI.
	snapshot, err := json.Marshal(prefs)
	if err != nil {
		log.Printf("window preferences: %v", err)
		return
	}
	p.mu.Lock()
	defer p.mu.Unlock()
	p.latest = snapshot
	p.queued = true
	if p.done == nil {
		p.done = make(chan struct{})
		go p.drain()
	}
}

func (p *windowPreferences) drain() {
	p.mu.Lock()
	for {
		snapshot := p.latest
		p.queued = false
		p.mu.Unlock()
		var prefs shell.Prefs
		err := json.Unmarshal(snapshot, &prefs)
		if err == nil {
			err = p.write(p.directory, prefs)
		}
		if err != nil {
			log.Printf("window preferences: %v", err)
		}
		p.mu.Lock()
		// If a second identical save arrived during a failed write, try it.
		if !p.queued || err == nil && bytes.Equal(snapshot, p.latest) {
			p.latest = nil
			close(p.done)
			p.done = nil
			p.mu.Unlock()
			return
		}
	}
}

func (p *windowPreferences) flush() {
	p.mu.Lock()
	done := p.done
	p.mu.Unlock()
	if done != nil {
		<-done
	}
}
