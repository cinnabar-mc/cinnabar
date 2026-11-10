package main

import (
	"github.com/sandertv/gophertunnel/minecraft/protocol"
	"github.com/sandertv/gophertunnel/minecraft/protocol/packet"
)

// partialMovesFixture names the updated source through the request that changed it.
func partialMovesFixture() *packet.ItemStackRequest {
	const requestID = -3
	place := func(count, container, slot byte, sourceID, destinationID int32) protocol.StackRequestAction {
		action := &protocol.PlaceStackRequestAction{}
		action.Count = count
		action.Source = protocol.StackRequestSlotInfo{
			Container: protocol.FullContainerName{ContainerID: protocol.ContainerLevelEntity},
			Slot:      2, StackNetworkID: sourceID,
		}
		action.Destination = protocol.StackRequestSlotInfo{
			Container: protocol.FullContainerName{ContainerID: container},
			Slot:      slot, StackNetworkID: destinationID,
		}
		return action
	}
	return &packet.ItemStackRequest{Requests: []protocol.ItemStackRequest{{
		RequestID: requestID,
		Actions: []protocol.StackRequestAction{
			place(2, protocol.ContainerHotBar, 5, 100, 10),
			place(3, protocol.ContainerInventory, 11, requestID, 11),
			place(5, protocol.ContainerHotBar, 0, requestID, 0),
		},
		FilterCause: -1,
	}}}
}
