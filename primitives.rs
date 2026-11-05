fn litematica_on_world_primitives(
    game: &crate::game::Game<'_>,
    camera: [f64; 3],
    primitives: &mut crate::world_primitives::WorldPrimitives,
) {
    game.litematica_draw(camera, primitives);
}
