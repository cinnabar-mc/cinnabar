use super::{AssetError, IconBlockSheet, IconSprite, MAX_ICON_BLOCK_SHEETS, invalid};

/// Reads the complete independent GUI binding table within the verified payload.
pub(super) fn decode(
    bytes: &[u8],
    cursor: &mut usize,
    end: usize,
    sprites: &[IconSprite],
) -> Result<Vec<IconBlockSheet>, AssetError> {
    let start = cursor
        .checked_add(4)
        .filter(|start| *start <= end)
        .ok_or_else(|| invalid("missing GUI cube sheet count"))?;
    let count = super::read_u32(bytes, *cursor)? as usize;
    if count == 0 || count > MAX_ICON_BLOCK_SHEETS {
        return Err(invalid("invalid GUI cube sheet count"));
    }
    let table_end = start
        .checked_add(count * 8)
        .filter(|table_end| *table_end <= end)
        .ok_or_else(|| invalid("truncated GUI cube sheet table"))?;
    let sheets = (0..count)
        .map(|row| {
            let at = start + row * 8;
            Ok(IconBlockSheet {
                visual: crate::BlockVisualId(super::read_u32(bytes, at)?),
                sprite: super::read_u32(bytes, at + 4)?,
            })
        })
        .collect::<Result<Vec<_>, AssetError>>()?;
    super::block::validate(&sheets, sprites)?;
    *cursor = table_end;
    Ok(sheets)
}

/// Appends the validated GUI bindings without changing the held binding table.
pub(super) fn encode(payload: &mut Vec<u8>, sheets: &[IconBlockSheet]) -> Result<(), AssetError> {
    let mut table = Vec::with_capacity(4 + sheets.len() * 8);
    table.extend_from_slice(&(sheets.len() as u32).to_le_bytes());
    for sheet in sheets {
        table.extend_from_slice(&sheet.visual.0.to_le_bytes());
        table.extend_from_slice(&sheet.sprite.to_le_bytes());
    }
    crate::encoding::append_bounded(
        payload,
        &table,
        super::MAX_ICON_CARRIER_BYTES,
        super::HEADER_BYTES + super::HASH_BYTES,
    )
    .ok_or_else(|| invalid("icon carrier exceeds bound"))
}
