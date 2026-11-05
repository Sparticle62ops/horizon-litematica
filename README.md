# Litematica for Horizon (Rust port) - in progress

Port of [maruohon/litematica](https://github.com/maruohon/litematica) (Java, Fabric) to a Horizon native mod.

## License
Upstream: LGPL-3.0. Per horizon-mods MOD-POLICY, source under LGPL cannot be reviewed by registry
reviewers without a written grant from the authors, so this port cannot be LISTED on the registry
unless masa grants it. Until then it ships from this repo for personal use; entry drafting deferred.

## Upstream scope (mapped 2026-11-05, shallow clone masa-faki master)
238 Java files, ~43k lines. Main packages:
- schematic/ (container, verifiers, placements, projects, data) - the core data model
- materials/ - material lists
- render/schematic - ghost-block overlay rendering
- task/ - printer + paste + fill tasks
- util/world, world/ - world access glue
- input/, event/, data/, gui/ (elsewhere), mixin/ - Java-side glue to drop in ports

## Port plan (phases)
1. Pure logic: schematic data model + .litematic (NBT) reader/writer, multi-region container; unit tests.
2. Pure logic: verifier (wrong block / wrong state / extra), material lists, placements (move/rotate/mirror).
3. Horizon client render: ghost-block overlay (translucent models, layers, see-through option).
4. Printer: placement legality via Horizon's own placement logic, bottom-up placement order.
5. Selection tool, saving own .litematic, menus (M), hotkeys, pick-block, highlights.

## Status
- [x] Phase 0: upstream mapping (this README)
- [x] Phase 1: schematic data model + .litematic (NBT) reader/writer, multi-region container; 9 unit tests (crate `horizon-mod-litematica`, `src/litematic.rs` + `src/nbt.rs`). Format details verified against litemapy (SmylerMC/litemapy): palette bit width = max(ceil(log2(palette_len)), 2), continuous LSB-first packing across i64 words, y-major block indices, signed Size encoding.
- [ ] Phase 2: verifier, material lists, placements

## Degradations tracked on horizon-rust/horizon
Phase 1 is pure logic with no Horizon surface needed yet. Phases 3-5 (ghost-block
overlay rendering, printer placement legality, selection tool/menus/hotkeys) ride the
tracked API issues: world snapshots #10152, HUD/local-player state #10151, entity types
#10150. Registry listing stays deferred per the LGPL review policy above until masa
grants source review.
