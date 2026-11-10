package mobile

import (
	"context"
	"encoding/json"
	"sync"
	"testing"
)

func TestMobileInvokeOps(t *testing.T) {
	t.Parallel()
	for _, test := range []struct {
		method string
		input  string
		ok     bool
	}{
		{"new", `{"kind":"digest"}`, true},
		{"default", `{"kind":"digest"}`, true},
		{"new", `{"kind":"speech_input","directory":"unused"}`, true},
		{"missing", `{}`, false},
		{"new", `{`, false},
	} {
		t.Run(test.method+test.input, func(t *testing.T) {
			results := make(chan envelope, 2)
			runtime := New(func(_ uint64, data []byte) {
				var value envelope
				if err := json.Unmarshal(data, &value); err != nil {
					t.Errorf("decode completion: %v", err)
				}
				results <- value
			})
			if !runtime.Invoke(0, test.method, []byte(test.input), 1) {
				t.Fatal("operation was not accepted")
			}
			got := <-results
			if got.OK != test.ok {
				t.Fatalf("completion = %#v, want ok=%v", got, test.ok)
			}
			runtime.Close(2)
			if final := <-results; !final.Closed || final.Count != "2" {
				t.Fatalf("close acknowledgement = %#v", final)
			}
		})
	}
}

type waitingObject struct {
	started chan struct{}
	stopped chan struct{}
	once    sync.Once
}

func (owner *waitingObject) invoke(ctx context.Context, _ *Runtime, _ uint64, _ string, _ json.RawMessage) (any, error) {
	owner.once.Do(func() { close(owner.started) })
	<-ctx.Done()
	return nil, ctx.Err()
}
func (owner *waitingObject) close() error {
	close(owner.stopped)
	return nil
}

func TestMobileSubscribeCancel(t *testing.T) {
	t.Parallel()
	var mu sync.Mutex
	completed := map[uint64]int{}
	callbacks := make(chan uint64, 4)
	runtime := New(func(request uint64, _ []byte) {
		mu.Lock()
		completed[request]++
		mu.Unlock()
		callbacks <- request
	})
	owner := &waitingObject{started: make(chan struct{}), stopped: make(chan struct{})}
	lifetime, cancel := context.WithCancel(runtime.ctx)
	handle, err := runtime.add(0, "updates", owner, lifetime, cancel)
	if err != nil || handle.Handle != "1" {
		t.Fatalf("register handle = %#v, %v", handle, err)
	}
	if !runtime.Invoke(1, "next", []byte("{}"), 1) {
		t.Fatal("subscription was not accepted")
	}
	<-owner.started
	runtime.Cancel(1)
	if request := <-callbacks; request != 1 {
		t.Fatalf("cancel completion = %d", request)
	}
	if !runtime.Invoke(1, "dispose", []byte("{}"), 2) {
		t.Fatal("dispose was not accepted")
	}
	if request := <-callbacks; request != 2 {
		t.Fatalf("dispose completion = %d", request)
	}
	<-owner.stopped
	runtime.Close(3)
	if request := <-callbacks; request != 3 {
		t.Fatalf("close acknowledgement = %d", request)
	}
	mu.Lock()
	defer mu.Unlock()
	for _, request := range []uint64{1, 2, 3} {
		if completed[request] != 1 {
			t.Fatalf("request %d completed %d times", request, completed[request])
		}
	}
}
