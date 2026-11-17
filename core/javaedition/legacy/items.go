package legacy

import (
	"bufio"
	"bytes"
	"compress/gzip"
	_ "embed"
	"encoding/json"
	"fmt"
	"strconv"
	"strings"
	"sync"
)

//go:embed bedrock_items.jsonl.gz
var registryData []byte

//go:embed items.tsv
var itemTable string

// RegistryItem is one entry of the Bedrock item registry the translator announces. NBT is the
// entry's component data in minecraft-data's typed JSON form.
type RegistryItem struct {
	Name           string          `json:"name"`
	ID             int16           `json:"id"`
	ComponentBased bool            `json:"component_based"`
	Version        string          `json:"version"`
	NBT            json.RawMessage `json:"nbt"`
}

// Data converts the entry's typed NBT to the Go values a Bedrock NBT encoder expects.
func (item RegistryItem) Data() (map[string]any, error) {
	if len(item.NBT) == 0 {
		return map[string]any{}, nil
	}
	var tag typedTag
	if err := json.Unmarshal(item.NBT, &tag); err != nil {
		return nil, fmt.Errorf("%s: %w", item.Name, err)
	}
	value, err := tag.value()
	if err != nil {
		return nil, fmt.Errorf("%s: %w", item.Name, err)
	}
	compound, ok := value.(map[string]any)
	if !ok {
		return nil, fmt.Errorf("%s: component data is not a compound", item.Name)
	}
	return compound, nil
}

// typedTag is an NBT tag in minecraft-data's JSON form: a type name and a value.
type typedTag struct {
	Type  string          `json:"type"`
	Value json.RawMessage `json:"value"`
}

// value converts the tag to the Go type Bedrock NBT encoders map to its tag type.
func (tag typedTag) value() (any, error) {
	switch tag.Type {
	case "compound":
		var fields map[string]typedTag
		if err := json.Unmarshal(tag.Value, &fields); err != nil {
			return nil, err
		}
		compound := make(map[string]any, len(fields))
		for key, field := range fields {
			value, err := field.value()
			if err != nil {
				return nil, fmt.Errorf("%s: %w", key, err)
			}
			compound[key] = value
		}
		return compound, nil
	case "list":
		var list struct {
			Type  string            `json:"type"`
			Value []json.RawMessage `json:"value"`
		}
		if err := json.Unmarshal(tag.Value, &list); err != nil {
			return nil, err
		}
		values := make([]any, len(list.Value))
		for i, raw := range list.Value {
			value, err := typedTag{Type: list.Type, Value: raw}.value()
			if err != nil {
				return nil, err
			}
			values[i] = value
		}
		return values, nil
	case "string":
		var text string
		err := json.Unmarshal(tag.Value, &text)
		return text, err
	case "byte", "short", "int", "long", "float", "double":
		var number float64
		if err := json.Unmarshal(tag.Value, &number); err != nil {
			return nil, err
		}
		switch tag.Type {
		case "byte":
			return uint8(int8(number)), nil
		case "short":
			return int16(number), nil
		case "int":
			return int32(number), nil
		case "long":
			return int64(number), nil
		case "float":
			return float32(number), nil
		}
		return number, nil
	}
	return nil, fmt.Errorf("unsupported NBT tag type %q", tag.Type)
}

// MaxItemID is the highest item id Java Edition 1.7.10 registers.
const MaxItemID = 2267

// RegisteredItem reports whether Java Edition 1.7.10 registers a non-block item under the id:
// 256 to 408, 417 to 422 and the music discs 2256 to 2267.
func RegisteredItem(id int) bool {
	return (id >= 256 && id <= 408) || (id >= 417 && id <= 422) || (id >= 2256 && id <= MaxItemID)
}

// Item is a Bedrock item: its registry id and metadata.
type Item struct {
	Name     string
	ID       int16
	Metadata int16
}

// items holds the Bedrock registry and the Java item table, loaded once.
type items struct {
	registry []RegistryItem
	byName   map[string]int16
	java     map[[2]int16]Item
}

var loadItems = sync.OnceValues(func() (*items, error) {
	table := &items{byName: map[string]int16{}, java: map[[2]int16]Item{}}
	reader, err := gzip.NewReader(bytes.NewReader(registryData))
	if err != nil {
		return nil, fmt.Errorf("bedrock_items.jsonl.gz: %w", err)
	}
	scanner := bufio.NewScanner(reader)
	scanner.Buffer(nil, 1<<24)
	for scanner.Scan() {
		var item RegistryItem
		if err := json.Unmarshal(scanner.Bytes(), &item); err != nil {
			return nil, fmt.Errorf("bedrock_items.jsonl.gz: %w", err)
		}
		table.registry = append(table.registry, item)
		table.byName[item.Name] = item.ID
	}
	if err := scanner.Err(); err != nil {
		return nil, fmt.Errorf("bedrock_items.jsonl.gz: %w", err)
	}
	for number, line := range strings.Split(itemTable, "\n") {
		if line == "" || strings.HasPrefix(line, "#") {
			continue
		}
		fields := strings.Split(line, "\t")
		if len(fields) != 5 {
			return nil, fmt.Errorf("items.tsv line %d: %d fields", number+1, len(fields))
		}
		var values [4]int64
		for i, index := range []int{0, 1, 3, 4} {
			value, err := strconv.ParseInt(fields[index], 10, 16)
			if err != nil {
				return nil, fmt.Errorf("items.tsv line %d: %w", number+1, err)
			}
			values[i] = value
		}
		if id, ok := table.byName[fields[2]]; !ok || int64(id) != values[2] {
			return nil, fmt.Errorf("items.tsv line %d: %s is not registry item %d", number+1, fields[2], values[2])
		}
		table.java[[2]int16{int16(values[0]), int16(values[1])}] = Item{Name: fields[2], ID: int16(values[2]), Metadata: int16(values[3])}
	}
	return table, nil
})

// mustItems returns the embedded tables; malformed tables are a build defect the package's
// tests catch.
func mustItems() *items {
	table, err := loadItems()
	if err != nil {
		panic(err)
	}
	return table
}

// Registry returns the Bedrock item registry in id order.
func Registry() []RegistryItem {
	return mustItems().registry
}

// ItemID returns the registry id of a Bedrock item name.
func ItemID(name string) (int16, bool) {
	id, ok := mustItems().byName[name]
	return id, ok
}

// JavaItem returns the Bedrock item for a 1.7.10 item id and damage, and whether the damage
// selected a variant. When it did not, the damage is the item's wear and the caller carries
// it over; ok is false for ids with no Bedrock item.
func JavaItem(id, damage int16) (item Item, variant, ok bool) {
	table := mustItems()
	if item, ok := table.java[[2]int16{id, damage}]; ok {
		return item, true, true
	}
	item, ok = table.java[[2]int16{id, 0}]
	return item, false, ok
}
