// Copyright (c) 2026 skejt23 (ZenKit Rust port)
// Copyright (c) 2021-2024 GothicKit Contributors.
// SPDX-License-Identifier: MIT
use super::*;
use crate::values::Mat4;

/// Complete camera save suffix, present only in Gothic 2 save archives, after
/// both frame lists. Time is stored verbatim, without playback validation.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct CameraState {
    pub paused: bool,
    pub started: bool,
    pub goto_time_mode: bool,
    pub cs_time: f32,
}
/// Cutscene camera with ordered, named position and target occurrences.
/// Each nonnull link must target a [`CameraFrame`] VOB. Nulls and repeated/shared
/// identities are valid, including aliases across the two lists. Counts derive
/// from the vectors; each must fit i32. There are no mutable duplicate counts.
///
/// Occurrences share the graph's `links` budget, including orphans. Their names
/// and `auto_focus_vob` share `copied_bytes`; no `sequence_items` charge applies.
/// Draft edits are checked by graph validation and complete-buffer serialization.
#[derive(Clone, Default, PartialEq)]
pub struct Camera {
    pub trajectory_for: CameraCoordinate,
    pub target_trajectory_for: CameraCoordinate,
    pub loop_mode: CameraLoop,
    pub lerp_mode: CameraLerp,
    pub ignore_for_vob_rotation: bool,
    pub ignore_for_vob_rotation_target: bool,
    pub adapt: bool,
    pub ease_first: bool,
    pub ease_last: bool,
    pub total_duration: f32,
    pub auto_focus_vob: Vec<u8>,
    pub auto_player_movable: bool,
    pub auto_untrigger_last: bool,
    pub auto_untrigger_last_delay: f32,
    pub positions: Vec<Link>,
    pub targets: Vec<Link>,
    pub state: Option<CameraState>,
}
impl fmt::Debug for Camera {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Camera")
            .field("trajectory_for", &self.trajectory_for)
            .field("target_trajectory_for", &self.target_trajectory_for)
            .field("loop_mode", &self.loop_mode)
            .field("lerp_mode", &self.lerp_mode)
            .field("ignore_for_vob_rotation", &self.ignore_for_vob_rotation)
            .field(
                "ignore_for_vob_rotation_target",
                &self.ignore_for_vob_rotation_target,
            )
            .field("adapt", &self.adapt)
            .field("ease_first", &self.ease_first)
            .field("ease_last", &self.ease_last)
            .field("total_duration", &self.total_duration)
            .field("auto_focus_vob_bytes", &self.auto_focus_vob.len())
            .field("auto_player_movable", &self.auto_player_movable)
            .field("auto_untrigger_last", &self.auto_untrigger_last)
            .field("auto_untrigger_last_delay", &self.auto_untrigger_last_delay)
            .field("position_count", &self.positions.len())
            .field("target_count", &self.targets.len())
            .field("state", &self.state)
            .finish()
    }
}
/// Camera trajectory keyframe. The matrix API stores columns while the 64-byte
/// wire prefix stores rows, matching the shared Mat4 codec. No normalization or
/// finite-only filtering is performed. Binary/raw matrix bits survive exactly;
/// ordinary ASCII float fields follow the archive's textual float contract.
///
/// `pose_tail` preserves additional bytes in tagged originalPose records and
/// shares `copied_bytes`. BINARY output refuses a nonempty tail because that
/// encoding has no raw-field delimiter. Other extra object fields still fail.
#[derive(Clone, PartialEq)]
pub struct CameraFrame {
    /// Retained class-version metadata. G1 supports 12289; G2 supports the
    /// standalone corpus's 31489, saved-world 9218 and default output's 52224.
    pub version: u16,
    pub time: f32,
    pub roll_angle: f32,
    pub fov_scale: f32,
    pub motion_type: CameraMotion,
    pub motion_type_fov: CameraMotion,
    pub motion_type_roll: CameraMotion,
    pub motion_type_time_scale: CameraMotion,
    pub tension: f32,
    pub cam_bias: f32,
    pub continuity: f32,
    pub time_scale: f32,
    pub time_fixed: bool,
    pub original_pose: Mat4,
    pub pose_tail: Vec<u8>,
}
impl CameraFrame {
    /// Uses version 12289 for G1 or 52224 for G2, with zero scalar
    /// values and the shared Mat4 default. No runtime playback defaults inferred.
    pub fn new(game: GameVersion) -> Self {
        Self {
            version: if game == GameVersion::Gothic1 {
                12289
            } else {
                52224
            },
            time: 0.,
            roll_angle: 0.,
            fov_scale: 0.,
            motion_type: CameraMotion::default(),
            motion_type_fov: CameraMotion::default(),
            motion_type_roll: CameraMotion::default(),
            motion_type_time_scale: CameraMotion::default(),
            tension: 0.,
            cam_bias: 0.,
            continuity: 0.,
            time_scale: 0.,
            time_fixed: false,
            original_pose: Mat4::default(),
            pose_tail: Vec::new(),
        }
    }
    pub(crate) fn version_supported(version: u16, game: GameVersion) -> bool {
        match game {
            GameVersion::Gothic1 => version == 12289,
            GameVersion::Gothic2 => matches!(version, 9218 | 31489 | 52224),
        }
    }
}
impl fmt::Debug for CameraFrame {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CameraFrame")
            .field("version", &self.version)
            .field("time", &self.time)
            .field("roll_angle", &self.roll_angle)
            .field("fov_scale", &self.fov_scale)
            .field("motion_type", &self.motion_type)
            .field("motion_type_fov", &self.motion_type_fov)
            .field("motion_type_roll", &self.motion_type_roll)
            .field("motion_type_time_scale", &self.motion_type_time_scale)
            .field("tension", &self.tension)
            .field("cam_bias", &self.cam_bias)
            .field("continuity", &self.continuity)
            .field("time_scale", &self.time_scale)
            .field("time_fixed", &self.time_fixed)
            .field("original_pose", &self.original_pose)
            .field("pose_tail_bytes", &self.pose_tail.len())
            .finish()
    }
}
impl VobKind {}

impl<'a> VobKindRef<'a> {
    pub(crate) fn camera_buffers(self) -> impl Iterator<Item = &'a [u8]> {
        match self {
            Self::Camera(c) => Some(c.auto_focus_vob.as_slice()),
            Self::CameraFrame(f) => Some(f.pose_tail.as_slice()),
            _ => None,
        }
        .into_iter()
    }
}

impl<'a> VobKindRef<'a> {
    pub(crate) fn camera_links(self) -> impl Iterator<Item = &'a Link> {
        let (positions, targets) = match self {
            Self::Camera(c) => (c.positions.as_slice(), c.targets.as_slice()),
            _ => ([].as_slice(), [].as_slice()),
        };
        positions.iter().chain(targets)
    }
}

impl<'a> VobKindRef<'a> {
    pub(crate) fn check_camera(self, game: GameVersion, save: bool) -> Result<()> {
        if let Self::Camera(c) = self {
            if c.state.is_some() != (save && game == GameVersion::Gothic2) {
                return Err(bad("camera state must match game/save mode"));
            }
            if c.positions.len() > i32::MAX as usize || c.targets.len() > i32::MAX as usize {
                return Err(bad("camera frame count exceeds i32"));
            }
        }
        if let Self::CameraFrame(f) = self
            && !CameraFrame::version_supported(f.version, game)
        {
            return Err(Error::operation(
                ErrorKind::Unsupported,
                "camera frame version",
            ));
        }
        Ok(())
    }
}
