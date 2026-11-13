package translate

import "testing"

func TestChatText(t *testing.T) {
	for _, test := range []struct{ name, raw, want string }{
		{"plain string", `"hello"`, "hello"},
		{"text", `{"text":"hello"}`, "hello"},
		{"colour and bold", `{"text":"hi","color":"gold","bold":true}`, "§r§6§lhi"},
		{"children inherit and override", `{"text":"a","color":"red","extra":[{"text":"b"},{"text":"c","color":"blue","italic":true}]}`, "§r§ca§r§cb§r§9§oc"},
		{"plain siblings reset", `{"text":"","extra":[{"text":"a","color":"green"},"b"]}`, "§r§aa§rb"},
		{"list", `["a",{"text":"b"}]`, "a§rb"},
		{"a list's first entry styles the rest", `[{"text":"a","color":"red"},"b"]`, "§r§ca§r§cb"},
		{"layout text returns to its style after an argument", `{"translate":"chat.type.announcement","color":"gold","with":[{"text":"Server","color":"red"},"stop"]}`, "§r§6[§r§cServer§r§6] §r§6stop§r§6"},
		{"layout translation", `{"translate":"chat.type.announcement","with":["Server","stop"]}`, "[Server] stop"},
		{"unknown translation keeps its key", `{"translate":"death.attack.mob","with":["Steve","Zombie"]}`, "death.attack.mob Steve Zombie"},
		{"not a component", `Timed out`, "Timed out"},
		{"legacy codes pass through", `{"text":"§aGreen"}`, "§aGreen"},
	} {
		if got := chatText(test.raw); got != test.want {
			t.Errorf("%s: chatText(%s) = %q, want %q", test.name, test.raw, got, test.want)
		}
	}
}
