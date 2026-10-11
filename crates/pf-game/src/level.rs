//! Loading and clearing the playable map.
//!
//! The shell stores a [`ChosenMap`]; the `Loading` screen reads it, builds the
//! collision and spawn point, then switches to `InGame`, where the visuals spawn.
//! Leaving `InGame` despawns everything the level made.

use bevy::prelude::*;
use pf_movement::StaticWorld;

use crate::Screen;

pub struct LevelPlugin;

impl Plugin for LevelPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(OnEnter(Screen::Loading), load_level)
            .add_systems(
                OnEnter(Screen::InGame),
                (
                    crate::map::spawn_test_map.run_if(resource_equals(ChosenMap::TestMap)),
                    (pf_world::spawn_map, pf_world::props::spawn_props)
                        .run_if(resource_equals(ChosenMap::Colony)),
                ),
            )
            .add_systems(OnExit(Screen::InGame), clear_level);
    }
}

#[derive(Resource, Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChosenMap {
    /// Built-in greybox: ramps, stairs, ledges, crates and walls. No game files.
    TestMap,
    Colony,
}

/// What the movement tick collides with.
#[derive(Resource)]
pub struct MapCollision(pub StaticWorld);

/// Where the player starts and resets to: feet position and yaw (radians).
#[derive(Resource, Clone, Copy)]
pub struct SpawnPoint {
    pub feet: Vec3,
    pub yaw: f32,
}

/// Marks entities the level spawned (besides `pf_world::MapEntity`).
#[derive(Component)]
pub struct LevelEntity;

fn load_level(mut commands: Commands, chosen: Res<ChosenMap>, mut next: ResMut<NextState<Screen>>) {
    match *chosen {
        ChosenMap::TestMap => {
            commands.insert_resource(MapCollision(crate::map::test_map_collision()));
            commands.insert_resource(SpawnPoint {
                feet: crate::map::SPAWN,
                yaw: 0.0,
            });
        }
        ChosenMap::Colony => {
            let loaded = pf_world::load("mp_colony").map_err(|e| e.to_string());
            let map = match loaded {
                Ok(map) => map,
                Err(e) => {
                    error!("can't load Colony: {e}");
                    // Fall back to the test map.
                    commands.insert_resource(ChosenMap::TestMap);
                    next.set(Screen::Loading);
                    return;
                }
            };
            // World and props go into one triangle mesh, so every movement
            // query searches a single tree.
            let (mut points, mut triangles) = pf_world::collision_triangles(&map.geometry);
            let (prop_points, prop_triangles) = pf_world::props::prop_collision(&map.props);
            let base = points.len() as u32;
            points.extend(prop_points);
            triangles.extend(prop_triangles.iter().map(|t| t.map(|i| base + i)));
            let mut world = StaticWorld::new();
            world.add_trimesh(&points, &triangles);
            info!(
                "Colony collision: {} triangles ({} from props)",
                triangles.len(),
                prop_triangles.len()
            );
            let (feet, yaw) = pf_world::spawn_point(&map.geometry);
            commands.insert_resource(MapCollision(world));
            // Spawn a little above the marker so we drop onto the floor.
            commands.insert_resource(SpawnPoint {
                feet: feet + Vec3::Y * 8.0,
                yaw,
            });
            map.insert(&mut commands);
        }
    }
    next.set(Screen::InGame);
}

/// Anything the level or the map renderer spawned.
type LevelOrMap = Or<(With<LevelEntity>, With<pf_world::MapEntity>)>;

fn clear_level(mut commands: Commands, level: Query<Entity, LevelOrMap>) {
    for e in &level {
        commands.entity(e).despawn();
    }
    commands.remove_resource::<MapCollision>();
    commands.remove_resource::<SpawnPoint>();
    commands.remove_resource::<pf_world::MapGeometry>();
    commands.remove_resource::<pf_world::props::StaticProps>();
}
