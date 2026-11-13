package translate

import (
	"encoding/json"
	"fmt"
	"strings"
)

// chatColours maps Java chat colour names to their formatting code characters.
var chatColours = map[string]byte{
	"black": '0', "dark_blue": '1', "dark_green": '2', "dark_aqua": '3', "dark_red": '4',
	"dark_purple": '5', "gold": '6', "gray": '7', "dark_gray": '8', "blue": '9', "green": 'a',
	"aqua": 'b', "red": 'c', "light_purple": 'd', "yellow": 'e', "white": 'f',
}

// chatFormats holds the translation keys whose English text is only a layout of their
// arguments, so they can be shown without the game's language file.
var chatFormats = map[string]string{
	"chat.type.text":         "<%s> %s",
	"chat.type.emote":        "* %s %s",
	"chat.type.announcement": "[%s] %s",
	"chat.type.admin":        "[%s: %s]",
}

// chatStyle is the formatting a chat component passes to its children. Unset fields inherit.
type chatStyle struct {
	colour                   string
	bold, italic, obfuscated *bool
	hasColour                bool
}

// chatComponent is a Java chat component: a string, an object, or a list whose first entry
// parents the rest.
type chatComponent struct {
	Text       string            `json:"text"`
	Translate  string            `json:"translate"`
	With       []json.RawMessage `json:"with"`
	Extra      []json.RawMessage `json:"extra"`
	Colour     *string           `json:"color"`
	Bold       *bool             `json:"bold"`
	Italic     *bool             `json:"italic"`
	Obfuscated *bool             `json:"obfuscated"`
}

// chatText flattens a Java chat component to Bedrock formatted text. Text that is not valid
// component JSON is shown as it is.
func chatText(raw string) string {
	var out strings.Builder
	if !writeChat(&out, json.RawMessage(raw), chatStyle{}) {
		return raw
	}
	return out.String()
}

// writeChat appends one component and its children; it reports false when raw is not a
// chat component.
func writeChat(out *strings.Builder, raw json.RawMessage, parent chatStyle) bool {
	var text string
	if json.Unmarshal(raw, &text) == nil {
		writeStyled(out, text, parent)
		return true
	}
	var list []json.RawMessage
	if json.Unmarshal(raw, &list) == nil {
		for _, entry := range list {
			if !writeChat(out, entry, parent) {
				return false
			}
		}
		return true
	}
	var component chatComponent
	if json.Unmarshal(raw, &component) != nil {
		return false
	}
	style := parent
	if component.Colour != nil {
		style.colour, style.hasColour = *component.Colour, true
	}
	if component.Bold != nil {
		style.bold = component.Bold
	}
	if component.Italic != nil {
		style.italic = component.Italic
	}
	if component.Obfuscated != nil {
		style.obfuscated = component.Obfuscated
	}
	switch {
	case component.Translate != "":
		arguments := make([]any, len(component.With))
		for i, argument := range component.With {
			var nested strings.Builder
			if !writeChat(&nested, argument, style) {
				nested.WriteString(string(argument))
			}
			arguments[i] = nested.String()
		}
		writeStyled(out, translateChat(component.Translate, arguments), style)
	default:
		writeStyled(out, component.Text, style)
	}
	for _, child := range component.Extra {
		if !writeChat(out, child, style) {
			return false
		}
	}
	return true
}

// translateChat lays out a translated component. Keys outside chatFormats have no text
// here, so the key is shown with its arguments.
func translateChat(key string, arguments []any) string {
	if format, ok := chatFormats[key]; ok && strings.Count(format, "%s") == len(arguments) {
		return fmt.Sprintf(format, arguments...)
	}
	parts := make([]string, 0, len(arguments)+1)
	parts = append(parts, key)
	for _, argument := range arguments {
		parts = append(parts, argument.(string))
	}
	return strings.Join(parts, " ")
}

// writeStyled appends text behind the formatting codes of its style. Every piece resets
// first, so styles never leak between siblings.
func writeStyled(out *strings.Builder, text string, style chatStyle) {
	if text == "" {
		return
	}
	styled := style.hasColour || isSet(style.bold) || isSet(style.italic) || isSet(style.obfuscated)
	if styled || out.Len() > 0 {
		out.WriteString("§r")
	}
	if code, ok := chatColours[style.colour]; ok {
		out.WriteString("§")
		out.WriteByte(code)
	}
	if isSet(style.obfuscated) {
		out.WriteString("§k")
	}
	if isSet(style.bold) {
		out.WriteString("§l")
	}
	if isSet(style.italic) {
		out.WriteString("§o")
	}
	out.WriteString(text)
}

// isSet reports whether an optional flag is present and true.
func isSet(flag *bool) bool {
	return flag != nil && *flag
}
