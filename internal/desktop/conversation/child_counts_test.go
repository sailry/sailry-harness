package conversation

import "testing"

func TestChildCountsTreatEveryEndAsDone(t *testing.T) {
	t.Parallel()
	kids := []Child{
		{Status: ChildCompleted}, {Status: ChildFailed}, {Status: ChildCancelled}, {Status: ChildInterrupted},
		{Status: ChildRunning}, {Status: ChildQueued}, {Status: ChildStopping}, {Status: ChildContinued},
	}
	if done, total := childCounts(kids); done != 4 || total != 7 {
		t.Fatalf("counts = %d/%d, want 4/7", done, total)
	}
}
