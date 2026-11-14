// Package legacy maps Java Edition 1.7.10 numeric block ids to Bedrock block states.
package legacy

import (
	"encoding/binary"
	"fmt"
	"sort"
	"strconv"
	"strings"
)

// State is a Bedrock block state: a name and typed properties. Property values are bool,
// int32 or string, the three types Bedrock block states use.
type State struct {
	Name       string
	Properties map[string]any
}

// ParseState reads "name[key=value,...]". Values "true" and "false" are Booleans, decimal
// integers are int32 and everything else is a string.
func ParseState(text string) (State, error) {
	name, raw, bracket := strings.Cut(text, "[")
	if name == "" {
		return State{}, fmt.Errorf("block state %q has no name", text)
	}
	state := State{Name: name, Properties: map[string]any{}}
	if !bracket {
		return state, nil
	}
	raw, ok := strings.CutSuffix(raw, "]")
	if !ok {
		return State{}, fmt.Errorf("block state %q has unterminated properties", text)
	}
	if raw == "" {
		return state, nil
	}
	for _, pair := range strings.Split(raw, ",") {
		key, value, ok := strings.Cut(pair, "=")
		if !ok || key == "" || value == "" {
			return State{}, fmt.Errorf("block state %q has invalid property %q", text, pair)
		}
		if _, exists := state.Properties[key]; exists {
			return State{}, fmt.Errorf("block state %q repeats property %q", text, key)
		}
		state.Properties[key] = propertyValue(value)
	}
	return state, nil
}

// propertyValue gives a property string its Bedrock type.
func propertyValue(value string) any {
	switch value {
	case "true":
		return true
	case "false":
		return false
	}
	if number, err := strconv.ParseInt(value, 10, 32); err == nil {
		return int32(number)
	}
	return value
}

// keys returns the property names in the byte order Bedrock sorts them in.
func (s State) keys() []string {
	keys := make([]string, 0, len(s.Properties))
	for key := range s.Properties {
		keys = append(keys, key)
	}
	sort.Strings(keys)
	return keys
}

// String renders the state as "name[key=value,...]" with properties in key order, the form
// ParseState reads back.
func (s State) String() string {
	keys := s.keys()
	pairs := make([]string, len(keys))
	for i, key := range keys {
		pairs[i] = fmt.Sprintf("%s=%v", key, s.Properties[key])
	}
	return s.Name + "[" + strings.Join(pairs, ",") + "]"
}

// NetworkHash is the id a hashed-palette Bedrock server sends for the state: FNV-1a 32 over
// the little-endian NBT compound {name, states} with the state keys sorted.
func (s State) NetworkHash() uint32 {
	data := []byte{10, 0, 0}
	text := func(value string) {
		data = binary.LittleEndian.AppendUint16(data, uint16(len(value)))
		data = append(data, value...)
	}
	data = append(data, 8)
	text("name")
	text(s.Name)
	data = append(data, 10)
	text("states")
	for _, key := range s.keys() {
		switch value := s.Properties[key].(type) {
		case string:
			data = append(data, 8)
			text(key)
			text(value)
		case bool:
			data = append(data, 1)
			text(key)
			if value {
				data = append(data, 1)
			} else {
				data = append(data, 0)
			}
		case int32:
			data = append(data, 3)
			text(key)
			data = binary.LittleEndian.AppendUint32(data, uint32(value))
		}
	}
	data = append(data, 0, 0)
	hash := uint32(0x811c9dc5)
	for _, b := range data {
		hash = (hash ^ uint32(b)) * 0x01000193
	}
	return hash
}
