mod app;
mod assets;
mod camera;
mod renderer;
mod scene;

use anyhow::{Result, bail};
use clap::Parser;
use glam::Vec3;
use std::path::PathBuf;
use zenkit::GameVersion;

#[derive(Parser)]
#[command(about = "Explore Gothic I/II world geometry and static VOBs with a free camera")]
pub struct Options {
    /// Loose .ZEN file or a virtual path/basename inside mounted archives.
    pub world: String,
    #[arg(long, value_parser = ["g1", "g2"], default_value = "g2")]
    pub game: String,
    /// Gothic install: mount Data/{Worlds,Meshes,Textures,Anims}*.vdf.
    #[arg(long)]
    pub game_dir: Option<PathBuf>,
    /// Additional .vdf/.mod archive or loose asset directory. Repeatable; later wins.
    #[arg(long = "mount")]
    pub mounts: Vec<PathBuf>,
    /// Camera position in original Gothic coordinates (centimetres).
    #[arg(long, num_args = 3, allow_hyphen_values = true)]
    pub position: Option<Vec<f32>>,
    #[arg(long, default_value_t = 0.0, allow_hyphen_values = true)]
    pub yaw: f32,
    #[arg(long, default_value_t = 0.0, allow_hyphen_values = true)]
    pub pitch: f32,
    /// Initial fly speed in metres/second. Mouse wheel adjusts it in the viewer.
    #[arg(long, default_value_t = 15.0)]
    pub speed: f32,
    #[arg(long)]
    pub no_vobs: bool,
    /// Render one offscreen frame to a PNG and exit (no window).
    #[arg(long)]
    pub screenshot: Option<PathBuf>,
    #[arg(long, default_value_t = 1280)]
    pub width: u32,
    #[arg(long, default_value_t = 720)]
    pub height: u32,
}

impl Options {
    pub fn game_version(&self) -> GameVersion {
        if self.game == "g1" {
            GameVersion::Gothic1
        } else {
            GameVersion::Gothic2
        }
    }
}

fn main() -> Result<()> {
    let options = Options::parse();
    if !(1..=8192).contains(&options.width) || !(1..=8192).contains(&options.height) {
        bail!("width and height must be between 1 and 8192");
    }
    if !options.speed.is_finite()
        || options.speed <= 0.0
        || !options.yaw.is_finite()
        || !options.pitch.is_finite()
        || options
            .position
            .as_ref()
            .is_some_and(|v| v.iter().any(|x| !x.is_finite()))
    {
        bail!("camera values must be finite and speed must be positive");
    }
    let assets = assets::Assets::load(&options)?;
    let scene = scene::Scene::load(&options, &assets)?;
    let mut camera = camera::Camera::new(scene.spawn, scene.target, options.speed);
    if let Some(p) = &options.position {
        camera.position = Vec3::new(p[0], p[1], -p[2]) * 0.01;
        camera.yaw = options.yaw.to_radians();
        camera.pitch = options.pitch.to_radians().clamp(-1.55, 1.55);
    }
    camera.save_home();
    eprintln!(
        "Initial camera: --position {:.1} {:.1} {:.1} --yaw {:.2} --pitch {:.2}",
        camera.position.x * 100.0,
        camera.position.y * 100.0,
        -camera.position.z * 100.0,
        camera.yaw.to_degrees(),
        camera.pitch.to_degrees()
    );
    if let Some(path) = &options.screenshot {
        pollster::block_on(renderer::screenshot(
            &scene, &assets, &camera, &options, path,
        ))
    } else {
        app::run(scene, assets, camera, options)
    }
}
