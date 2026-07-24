//! SuperGame: a 2D side-scroller adventure RPG.
//!
//! The crate is a library so that the game binary, the headless `sim` runner,
//! and integration tests all share one definition of the simulation. The
//! split that makes that possible: everything in [`sim`] runs without a
//! `ggez::Context`, and only [`scenes`] touches the GPU.

pub mod app;
pub mod assets;
pub mod config;
pub mod debug;
pub mod ecs;
pub mod level;
pub mod physics;
pub mod scenes;
pub mod sim;
pub mod systems;
