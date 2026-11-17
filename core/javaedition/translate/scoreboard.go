package translate

import (
	java "github.com/hashimthearab/rust-mcbe/core/javaedition/protocol"
	"github.com/sandertv/gophertunnel/minecraft/protocol"
	"github.com/sandertv/gophertunnel/minecraft/protocol/packet"
)

// Java scoreboard actions this translator reads.
const (
	objectiveCreate = 0
	objectiveRemove = 1
	objectiveUpdate = 2

	scoreUpdate = 0
	scoreRemove = 1
)

// scoreboardSlots are the Bedrock display slots of the 1.7.10 positions: the player list, the
// sidebar and below names.
var scoreboardSlots = [...]string{packet.ScoreboardSlotList, packet.ScoreboardSlotSidebar, packet.ScoreboardSlotBelowName}

// scoreboard is the 1.7.10 client's scoreboard, which the Bedrock client shows only for the
// objectives in a display slot. Like the 1.7.10 client, it lasts the whole connection, across
// respawns and dimension changes.
type scoreboard struct {
	// objectives maps each objective to its display name, and scores holds its scores by entry.
	objectives map[string]string
	scores     map[string]map[string]int32
	// slots holds the objective each 1.7.10 display position shows, or "".
	slots [len(scoreboardSlots)]string
	// teams maps a team to its prefix and suffix, and members each entry to its team.
	teams   map[string]team
	members map[string]string
	// onClient holds the objectives the Bedrock client was sent, and shown the entry each of
	// their scores is drawn as.
	onClient map[string]bool
	shown    map[scoreKey]shownScore
	nextID   int64
}

// team is the formatting a 1.7.10 team gives its members' names.
type team struct{ prefix, suffix string }

// scoreKey names one score: an entry in an objective.
type scoreKey struct{ objective, entry string }

// shownScore is a score's Bedrock entry: its id, its text and its value.
type shownScore struct {
	id    int64
	text  string
	score int32
}

func newScoreboard() scoreboard {
	return scoreboard{
		objectives: map[string]string{},
		scores:     map[string]map[string]int32{},
		teams:      map[string]team{},
		members:    map[string]string{},
		onClient:   map[string]bool{},
		shown:      map[scoreKey]shownScore{},
	}
}

// scoreboardObjective creates, removes or renames an objective.
func (s *Session) scoreboardObjective(pk *java.ScoreboardObjective) {
	board := &s.state.board
	switch pk.Mode {
	case objectiveCreate:
		board.objectives[pk.Name] = pk.DisplayName
		if board.scores[pk.Name] == nil {
			board.scores[pk.Name] = map[string]int32{}
		}
	case objectiveRemove:
		if _, ok := board.objectives[pk.Name]; !ok {
			return
		}
		for slot, objective := range board.slots {
			if objective == pk.Name {
				board.slots[slot] = ""
			}
		}
		s.hideObjective(pk.Name)
		delete(board.objectives, pk.Name)
		delete(board.scores, pk.Name)
	case objectiveUpdate:
		if _, ok := board.objectives[pk.Name]; !ok {
			return
		}
		board.objectives[pk.Name] = pk.DisplayName
		// Bedrock takes a title only with the objective, so a shown one is shown afresh.
		if board.displayed(pk.Name) {
			s.hideObjective(pk.Name)
			s.showObjective(pk.Name)
		}
	}
}

// displayScoreboard shows an objective in a display position, or clears the position.
func (s *Session) displayScoreboard(pk *java.DisplayScoreboard) {
	board := &s.state.board
	if pk.Position < 0 || int(pk.Position) >= len(board.slots) {
		return
	}
	objective := pk.Objective
	if _, ok := board.objectives[objective]; !ok {
		objective = ""
	}
	old := board.slots[pk.Position]
	if old == objective {
		return
	}
	board.slots[pk.Position] = objective
	if old != "" {
		// Bedrock removes an objective from every slot at once, so it returns to the slots
		// that still show it.
		s.hideObjective(old)
		if board.displayed(old) {
			s.showObjective(old)
		}
	}
	if objective != "" {
		s.hideObjective(objective)
		s.showObjective(objective)
	}
}

// updateScore sets an entry's score, or removes the entry from every objective as the 1.7.10
// client does.
func (s *Session) updateScore(pk *java.UpdateScore) {
	board := &s.state.board
	switch pk.Action {
	case scoreUpdate:
		scores, ok := board.scores[pk.Objective]
		if !ok {
			return
		}
		scores[pk.ItemName] = pk.Value
		s.syncScore(scoreKey{pk.Objective, pk.ItemName})
	case scoreRemove:
		for objective, scores := range board.scores {
			if _, ok := scores[pk.ItemName]; ok {
				delete(scores, pk.ItemName)
				s.syncScore(scoreKey{objective, pk.ItemName})
			}
		}
	}
}

// teams changes a team or its members, then redraws the members' scores with the team's
// prefix and suffix.
func (s *Session) teams(pk *java.Teams) {
	board := &s.state.board
	changed := map[string]bool{}
	switch pk.Mode {
	case java.TeamCreate, java.TeamUpdate:
		if _, ok := board.teams[pk.Name]; !ok && pk.Mode == java.TeamUpdate {
			return
		}
		board.teams[pk.Name] = team{prefix: pk.Prefix, suffix: pk.Suffix}
		for entry, member := range board.members {
			if member == pk.Name {
				changed[entry] = true
			}
		}
	case java.TeamRemove:
		delete(board.teams, pk.Name)
		for entry, member := range board.members {
			if member == pk.Name {
				delete(board.members, entry)
				changed[entry] = true
			}
		}
	}
	switch pk.Mode {
	case java.TeamCreate, java.TeamAddPlayers:
		if _, ok := board.teams[pk.Name]; !ok {
			return
		}
		for _, entry := range pk.Players {
			board.members[entry] = pk.Name
			changed[entry] = true
		}
	case java.TeamRemovePlayers:
		for _, entry := range pk.Players {
			if board.members[entry] == pk.Name {
				delete(board.members, entry)
				changed[entry] = true
			}
		}
	}
	for objective, scores := range board.scores {
		for entry := range changed {
			if _, ok := scores[entry]; ok {
				s.syncScore(scoreKey{objective, entry})
			}
		}
	}
}

// displayed reports whether any display position shows the objective.
func (board *scoreboard) displayed(objective string) bool {
	for _, shown := range board.slots {
		if shown == objective {
			return true
		}
	}
	return false
}

// text is an entry's line: its team's prefix, the entry and the team's suffix, as the 1.7.10
// client draws it.
func (board *scoreboard) text(entry string) string {
	member, ok := board.teams[board.members[entry]]
	if !ok {
		return entry
	}
	return member.prefix + entry + member.suffix
}

// showObjective sends an objective to every slot that shows it, then its scores.
func (s *Session) showObjective(objective string) {
	board := &s.state.board
	for slot, shown := range board.slots {
		if shown == objective {
			board.onClient[objective] = true
			s.queue(&packet.SetDisplayObjective{
				DisplaySlot:   scoreboardSlots[slot],
				ObjectiveName: objective,
				DisplayName:   board.objectives[objective],
				CriteriaName:  "dummy",
				SortOrder:     packet.ScoreboardSortOrderDescending,
			})
		}
	}
	for entry := range board.scores[objective] {
		s.syncScore(scoreKey{objective, entry})
	}
}

// hideObjective removes an objective from the Bedrock client, which forgets its scores too.
func (s *Session) hideObjective(objective string) {
	board := &s.state.board
	for key := range board.shown {
		if key.objective == objective {
			delete(board.shown, key)
		}
	}
	if board.onClient[objective] {
		delete(board.onClient, objective)
		s.queue(&packet.RemoveObjective{ObjectiveName: objective})
	}
}

// syncScore brings a score's Bedrock entry in line with the scoreboard: added, changed,
// redrawn under a new id when its text changed, or removed.
func (s *Session) syncScore(key scoreKey) {
	board := &s.state.board
	shown, wasShown := board.shown[key]
	score, exists := board.scores[key.objective][key.entry]
	if !exists || !board.displayed(key.objective) {
		if wasShown {
			delete(board.shown, key)
			s.queue(&packet.SetScore{Entries: []protocol.ScoreboardEntry{{
				EntryID: shown.id, ObjectiveName: key.objective, IdentityType: protocol.ScoreboardIdentityRemove,
			}}})
		}
		return
	}
	text := board.text(key.entry)
	if wasShown && shown.text == text && shown.score == score {
		return
	}
	var entries []protocol.ScoreboardEntry
	id := shown.id
	if !wasShown || shown.text != text {
		if wasShown {
			entries = append(entries, protocol.ScoreboardEntry{
				EntryID: shown.id, ObjectiveName: key.objective, IdentityType: protocol.ScoreboardIdentityRemove,
			})
		}
		board.nextID++
		id = board.nextID
	}
	entries = append(entries, protocol.ScoreboardEntry{
		EntryID: id, ObjectiveName: key.objective, Score: score,
		IdentityType: protocol.ScoreboardIdentityFakePlayer, DisplayName: text,
	})
	board.shown[key] = shownScore{id: id, text: text, score: score}
	s.queue(&packet.SetScore{Entries: entries})
}
