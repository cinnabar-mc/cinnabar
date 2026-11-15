package translate

import (
	"testing"

	java "github.com/hashimthearab/rust-mcbe/core/javaedition/protocol"
	"github.com/sandertv/gophertunnel/minecraft/protocol"
	"github.com/sandertv/gophertunnel/minecraft/protocol/packet"
)

// scoreEntries returns the entries of the SetScore packets among packets.
func scoreEntries(packets []packet.Packet) []protocol.ScoreboardEntry {
	var entries []protocol.ScoreboardEntry
	for _, pk := range packets {
		if score, ok := pk.(*packet.SetScore); ok {
			entries = append(entries, score.Entries...)
		}
	}
	return entries
}

// A sidebar built the way PvP networks build theirs: each line is a placeholder entry whose
// team's prefix and suffix carry the text, ordered by score. The Bedrock client is sent the
// line as the 1.7.10 client draws it, and every later change.
func TestSidebarLinesCarryTheirTeamsText(t *testing.T) {
	ts := startTestSession(t)
	ts.serve(&java.JoinGame{EntityID: 7, LevelType: "default"})
	// Nothing is shown until a slot shows the objective, so these send the client nothing.
	ts.send(&java.ScoreboardObjective{Name: "board", DisplayName: "§d§lPractice", Mode: objectiveCreate})
	ts.send(&java.Teams{Name: "line1", Mode: java.TeamCreate, Prefix: "§7Online: ", Suffix: "§f2972", Players: []string{"§a"}})
	ts.send(&java.UpdateScore{ItemName: "§a", Action: scoreUpdate, Objective: "board", Value: 15})

	shown := ts.serve(&java.DisplayScoreboard{Position: 1, Objective: "board"})
	display, ok := shown[0].(*packet.SetDisplayObjective)
	if !ok || display.DisplaySlot != packet.ScoreboardSlotSidebar || display.DisplayName != "§d§lPractice" || display.SortOrder != packet.ScoreboardSortOrderDescending {
		t.Fatalf("showing the sidebar translated to %+v", shown)
	}
	entries := scoreEntries(shown)
	if len(entries) != 1 || entries[0].DisplayName != "§7Online: §a§f2972" || entries[0].Score != 15 || entries[0].IdentityType != protocol.ScoreboardIdentityFakePlayer {
		t.Fatalf("sidebar entries = %+v", entries)
	}
	first := entries[0].EntryID

	// A new suffix redraws the line under a new id; a new score keeps it.
	entries = scoreEntries(ts.serve(&java.Teams{Name: "line1", Mode: java.TeamUpdate, Prefix: "§7Online: ", Suffix: "§f3001"}))
	if len(entries) != 2 || entries[0].IdentityType != protocol.ScoreboardIdentityRemove || entries[0].EntryID != first || entries[1].DisplayName != "§7Online: §a§f3001" {
		t.Fatalf("a team update translated to %+v", entries)
	}
	second := entries[1].EntryID
	entries = scoreEntries(ts.serve(&java.UpdateScore{ItemName: "§a", Action: scoreUpdate, Objective: "board", Value: 14}))
	if len(entries) != 1 || entries[0].EntryID != second || entries[0].Score != 14 {
		t.Fatalf("a score change translated to %+v", entries)
	}

	// A new title shows the objective afresh with its lines.
	retitled := ts.serve(&java.ScoreboardObjective{Name: "board", DisplayName: "§bPractice", Mode: objectiveUpdate})
	if _, ok := retitled[0].(*packet.RemoveObjective); !ok {
		t.Fatalf("a title change translated to %+v", retitled)
	}
	if display, ok := retitled[1].(*packet.SetDisplayObjective); !ok || display.DisplayName != "§bPractice" {
		t.Fatalf("a title change translated to %+v", retitled)
	}
	if entries := scoreEntries(retitled); len(entries) != 1 || entries[0].Score != 14 {
		t.Fatalf("a title change kept lines %+v", entries)
	}

	// Removing the entry removes its line; clearing the slot removes the objective.
	entries = scoreEntries(ts.serve(&java.UpdateScore{ItemName: "§a", Action: scoreRemove}))
	if len(entries) != 1 || entries[0].IdentityType != protocol.ScoreboardIdentityRemove {
		t.Fatalf("removing the entry translated to %+v", entries)
	}
	cleared := ts.serve(&java.DisplayScoreboard{Position: 1, Objective: ""})
	if len(cleared) != 1 {
		t.Fatalf("clearing the sidebar translated to %+v", cleared)
	}
	if removed, ok := cleared[0].(*packet.RemoveObjective); !ok || removed.ObjectiveName != "board" {
		t.Fatalf("clearing the sidebar translated to %+v", cleared)
	}
}

// Scores of an objective no slot shows reach the client only once it is shown, and a
// dimension change keeps the scoreboard, as the 1.7.10 client does.
func TestHiddenObjectivesWaitAndSurviveADimensionChange(t *testing.T) {
	ts := startTestSession(t)
	ts.serve(&java.JoinGame{EntityID: 7, LevelType: "default"})
	ts.send(&java.ScoreboardObjective{Name: "kills", DisplayName: "Kills", Mode: objectiveCreate})
	ts.send(&java.UpdateScore{ItemName: "Steve", Action: scoreUpdate, Objective: "kills", Value: 3})
	// The next batch is the marker's, so the hidden score sent the client nothing.
	if next := ts.serve(&java.ChatMessage{Message: `"marker"`}); len(next) != 1 || next[0].ID() != packet.IDText {
		t.Fatalf("a hidden score reached the client: %+v", next)
	}
	ts.serve(&java.Respawn{Dimension: -1, LevelType: "default"})
	shown := ts.serve(&java.DisplayScoreboard{Position: 2, Objective: "kills"})
	display, ok := shown[0].(*packet.SetDisplayObjective)
	if !ok || display.DisplaySlot != packet.ScoreboardSlotBelowName {
		t.Fatalf("showing below names translated to %+v", shown)
	}
	if entries := scoreEntries(shown); len(entries) != 1 || entries[0].DisplayName != "Steve" || entries[0].Score != 3 {
		t.Fatalf("below-name entries = %+v", entries)
	}
	if removed := ts.serve(&java.ScoreboardObjective{Name: "kills", Mode: objectiveRemove}); len(removed) != 1 {
		t.Fatalf("removing the objective translated to %+v", removed)
	}
}

// send writes a server packet that translates to nothing the client is sent.
func (ts *testSession) send(pk java.Packet) {
	ts.t.Helper()
	if err := ts.server.Write(pk); err != nil {
		ts.t.Fatal(err)
	}
}

// An objective shown in two slots stays in the one still showing it when the other moves on,
// an entry's removal reaches every objective, and a removed team's lines lose its text.
func TestScoreboardSlotsTeamsAndRemovals(t *testing.T) {
	ts := startTestSession(t)
	ts.serve(&java.JoinGame{EntityID: 7, LevelType: "default"})
	for _, name := range []string{"kills", "deaths"} {
		ts.send(&java.ScoreboardObjective{Name: name, DisplayName: name, Mode: objectiveCreate})
		ts.send(&java.UpdateScore{ItemName: "Steve", Action: scoreUpdate, Objective: name, Value: 1})
	}
	ts.send(&java.Teams{Name: "red", Mode: java.TeamCreate, Prefix: "§c", Players: []string{"Steve"}})
	ts.serve(&java.DisplayScoreboard{Position: 0, Objective: "kills"})
	ts.serve(&java.DisplayScoreboard{Position: 1, Objective: "kills"})

	moved := ts.serve(&java.DisplayScoreboard{Position: 1, Objective: "deaths"})
	var slots []string
	for _, pk := range moved {
		if display, ok := pk.(*packet.SetDisplayObjective); ok {
			slots = append(slots, display.ObjectiveName+"@"+display.DisplaySlot)
		}
	}
	want := []string{"kills@" + packet.ScoreboardSlotList, "deaths@" + packet.ScoreboardSlotSidebar}
	if len(slots) != 2 || slots[0] != want[0] || slots[1] != want[1] {
		t.Fatalf("moving the sidebar showed %v, want %v", slots, want)
	}

	entries := scoreEntries(ts.serve(&java.Teams{Name: "red", Mode: java.TeamRemove}))
	var redrawn []string
	for _, entry := range entries {
		if entry.IdentityType == protocol.ScoreboardIdentityFakePlayer {
			redrawn = append(redrawn, entry.ObjectiveName+":"+entry.DisplayName)
		}
	}
	if len(redrawn) != 2 {
		t.Fatalf("removing the team redrew %v", redrawn)
	}
	for _, line := range redrawn {
		if line != "kills:Steve" && line != "deaths:Steve" {
			t.Fatalf("removing the team redrew %v", redrawn)
		}
	}

	entries = scoreEntries(ts.serve(&java.UpdateScore{ItemName: "Steve", Action: scoreRemove}))
	if len(entries) != 2 || entries[0].IdentityType != protocol.ScoreboardIdentityRemove || entries[1].IdentityType != protocol.ScoreboardIdentityRemove {
		t.Fatalf("removing the entry translated to %+v", entries)
	}
}
