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
- [x] Phase 1: schematic data model + .litematic (NBT) reader/writer, multi-region container (9 tests)
- [x] Phase 2: material lists, verifier (wrong block/state/extra), placements (move/rotate/mirror)
- [x] Phase 3: ghost-block overlay (overlay.rs logic, runtime.rs render glue via native_world_primitives,
      /litematica command hook: load <path> [x y z] / clear / status; 24/24 unit tests; compiles in-tree
      at Horizon 489a1334)
- [x] Phase 4: printer (print_order bottom-up y,x,z + world-recheck pending filter; /litematica build
      runtime glue places up to 64 blocks/tick with UPDATE_NEIGHBOURS|UPDATE_CLIENTS flags, /litematica
      build stop / status; 42/42 unit tests)
- [x] Phase 5 (core): /litematica save <path> (valid gzip .litematic, dependency-free stored-deflate
      writer), /litematica savearea <x y z> <w h l> <path> (captures a live-world box), real gzipped
      .litematic loading (dependency-free inflate: stored/fixed/dynamic Huffman), /litematica verify
      [x y z] (per-region mismatch report vs the live world)
- [ ] Phase 5 (rest): selection tool, menus (M), hotkeys, pick-block, highlights; /litematica materials

## Horizon API gaps and degradation notes (2026-11-05)
1. Ghost blocks draw as translucent filled boxes with white outlines (native_world_primitives
   filled_box + box_outline, .through_walls(), 128-block cap). Upstream Litematica renders the actual
   translucent block model per ghost block; Horizon's native-mod API has no block-model world primitive,
   so box outlines are the compromise. Per standing policy this gets an issue/PR on horizon-rust/horizon
   proposing block-model world primitives (or accepting the box compromise); tracked until patched, then
   the port upgrades to 1:1.
2. Bundle pins Horizon revision 489a1334 (verified compiling). The public mod template still pins
   3fb4c31, which predates the native_world_primitives hook entirely, so a fresh template-context build
   of this bundle fails until the template bumps its pin. Same standing-policy issue tracks it.
