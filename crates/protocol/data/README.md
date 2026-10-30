# Retail protocol data

The active files use the game version and wire protocol in
[`assets/bedrock-target.json`](../../../assets/bedrock-target.json).

- `retail_items_1_26_60.tsv` contains the ItemRegistry network ID and identifier
  of every item in the default CreativeContent packet.
- `retail_biomes_1_26_60.txt` contains the default BiomeDefinitionList identifiers.
- `item_capacity_1_26_60.tsv` contains `ItemStack.maxAmount` for every admitted
  identifier. Measurement records are maintained separately.
- `item_tags_native.tsv` contains native item-tag memberships generated from the
  pinned shared catalog by `tools/registrygen/cmd/itemtags`.

Entries absent from those positive retail surfaces remain omitted. Item capacities
are a metadata-zero baseline; server item properties and component overrides still
control negotiated inventory behavior.
