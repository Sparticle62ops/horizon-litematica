// Runs at the head of the level-tick post hook (after each level tick pass).
fn litematica_on_level_tick(game: &mut crate::game::Game<'_>, dimension: &str) {
    game.litematica_level_tick(dimension);
}
