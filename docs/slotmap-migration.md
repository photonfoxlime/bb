# SlotMap migration notes

- Replaced block identity generation from UUID to `slotmap` keys via `new_key_type!` (`BlockId`).
- Primary store moved from `HashMap<BlockId, BlockNode>` to `SlotMap<BlockId, BlockNode>`.
- All block-keyed side stores in app state and undo snapshots moved to `SecondaryMap<BlockId, V>`.
- `EditorStore` now uses `SecondaryMap`; insertion paths were updated to avoid `entry` APIs.
- New block creation paths now come from `SlotMap::insert`, which is the single source of fresh `BlockId` values.
- Tests that used synthetic IDs now use `BlockId::default()` for unknown-key paths, and topology-building tests now allocate keys by inserting nodes into `SlotMap`.
- `uuid` was removed as a direct dependency in `Cargo.toml`; `slotmap` with `serde` was added.
