# Item capacity generator

This command projects a reviewed item-capacity measurement onto the active retail
allowlist. It validates the supplied measurement metadata, complete log, bundled
probe, completion counts and allowlist coverage before writing a deterministic TSV.
Keep raw logs and measurement metadata in a separate evidence store.

```text
go run ./cmd/itemcapacity -measurement <measurement.json> -probe-log <server-log> -retail-items <retail-items.tsv> -out <capacity.tsv> -provenance-out <evidence-output.json>
```

The metadata supplies the target server version, protocol, row counts and SHA-256
hashes of the server, log, probe script, probe manifest and retail allowlist. The
version and protocol must match the pinned catalog. The generator does not download
or run a server. The bundled probe reads the documented
[ItemStack.maxAmount property](https://learn.microsoft.com/en-us/minecraft/creator/scriptapi/minecraft/server/itemstack?view=minecraft-bedrock-stable#maxamount).
