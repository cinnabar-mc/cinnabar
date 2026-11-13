package translate

import (
	"github.com/hashimthearab/rust-mcbe/core/javaedition/legacy"
	java "github.com/hashimthearab/rust-mcbe/core/javaedition/protocol"
	"github.com/sandertv/gophertunnel/minecraft/protocol"
	"github.com/sandertv/gophertunnel/minecraft/protocol/packet"
)

// Java player inventory window slots.
const (
	javaArmorStart  = 5
	javaMainStart   = 9
	javaHotbarStart = 36
	javaWindowSize  = 45
	// bedrockInventorySize is the Bedrock inventory: nine hotbar slots, then the main rows.
	bedrockInventorySize = 36
)

// bedrockItem converts a Java item stack. Block items take the Bedrock block state of their
// id and damage; other items take the item table's entry, and damage that does not select a
// variant carries over as wear. Items with no Bedrock counterpart become empty.
func bedrockItem(stack java.ItemStack) protocol.ItemInstance {
	if stack.Empty() || stack.Count <= 0 {
		return protocol.ItemInstance{}
	}
	instance := protocol.ItemInstance{Stack: protocol.ItemStack{Count: uint16(stack.Count)}}
	if int(stack.ID) <= legacy.MaxBlockID {
		state := legacy.Block(uint16(stack.ID), uint8(stack.Damage))
		id, ok := legacy.ItemID(state.Name)
		if !ok || state.Name == "minecraft:air" {
			return protocol.ItemInstance{}
		}
		instance.Stack.NetworkID = int32(id)
		instance.Stack.BlockRuntimeID = int32(state.NetworkHash())
		return instance
	}
	item, variant, ok := legacy.JavaItem(stack.ID, stack.Damage)
	if !ok {
		return protocol.ItemInstance{}
	}
	instance.Stack.NetworkID = int32(item.ID)
	instance.Stack.MetadataValue = uint32(item.Metadata)
	if !variant && stack.Damage > 0 {
		instance.Stack.NBTData = map[string]any{"Damage": int32(stack.Damage)}
	}
	return instance
}

// bedrockSlot maps a Java player-inventory window slot to a Bedrock inventory slot: the Java
// hotbar is the window's last row and the Bedrock hotbar its first.
func bedrockSlot(slot int) (int, bool) {
	switch {
	case slot >= javaHotbarStart && slot < javaWindowSize:
		return slot - javaHotbarStart, true
	case slot >= javaMainStart && slot < javaHotbarStart:
		return slot, true
	}
	return 0, false
}

// windowItems replaces the player inventory and armour with the server's contents. Other
// windows are not translated yet.
func (s *Session) windowItems(pk *java.WindowItems) {
	if pk.WindowID != 0 {
		return
	}
	s.state.inventory = append(s.state.inventory[:0], pk.Items...)
	content := make([]protocol.ItemInstance, bedrockInventorySize)
	for slot, item := range pk.Items {
		if target, ok := bedrockSlot(slot); ok {
			content[target] = bedrockItem(item)
		}
	}
	s.queue(&packet.InventoryContent{WindowID: protocol.WindowIDInventory, Content: content})
	s.queueArmor()
}

// setSlot updates one slot of the player inventory.
func (s *Session) setSlot(pk *java.SetSlot) {
	if pk.WindowID != 0 || pk.Slot < 0 || int(pk.Slot) >= javaWindowSize {
		return
	}
	for len(s.state.inventory) < javaWindowSize {
		s.state.inventory = append(s.state.inventory, java.EmptyItemStack())
	}
	s.state.inventory[pk.Slot] = pk.Item
	if target, ok := bedrockSlot(int(pk.Slot)); ok {
		s.queue(&packet.InventorySlot{WindowID: protocol.WindowIDInventory, Slot: uint32(target), NewItem: bedrockItem(pk.Item)})
		return
	}
	if pk.Slot >= javaArmorStart && pk.Slot < javaMainStart {
		s.queueArmor()
	}
}

// queueArmor sends the four armour slots, which Java orders from the helmet down.
func (s *Session) queueArmor() {
	armor := make([]protocol.ItemInstance, 4)
	for index := range armor {
		if slot := javaArmorStart + index; slot < len(s.state.inventory) {
			armor[index] = bedrockItem(s.state.inventory[slot])
		}
	}
	s.queue(&packet.InventoryContent{WindowID: protocol.WindowIDArmour, Content: armor})
}

// heldItem is the Java stack in the selected hotbar slot, which block placement reports.
func (s *Session) heldItem() java.ItemStack {
	slot := javaHotbarStart + int(s.state.heldSlot)
	if slot < 0 || slot >= len(s.state.inventory) {
		return java.EmptyItemStack()
	}
	return s.state.inventory[slot]
}
