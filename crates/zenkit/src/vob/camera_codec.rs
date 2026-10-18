// Copyright (c) 2026 skejt23 (ZenKit Rust port)
// Copyright (c) 2021-2024 GothicKit Contributors.
// SPDX-License-Identifier: MIT
use super::{codec::Fields, *};
use crate::{
    archive::{self, Value as V, ValueType as T},
    io, objects,
    values::Mat4,
};

pub(super) fn read(
    f: &mut Fields<'_, '_, '_>,
    kind: &mut VobKindMut<'_>,
    phase: &mut Phase,
    game: GameVersion,
    links_left: usize,
) -> Result<usize> {
    let save = f.r.header().save_game;
    match kind {
        VobKindMut::Camera(c) => {
            if matches!(*phase, Phase::Base) {
                c.trajectory_for = CameraCoordinate::from_raw(f.enumeration(b"camTrjFOR")?);
                c.target_trajectory_for =
                    CameraCoordinate::from_raw(f.enumeration(b"targetTrjFOR")?);
                c.loop_mode = CameraLoop::from_raw(f.enumeration(b"loopMode")?);
                c.lerp_mode = CameraLerp::from_raw(f.enumeration(b"splLerpMode")?);
                c.ignore_for_vob_rotation = f.boolean(b"ignoreFORVobRotCam")?;
                c.ignore_for_vob_rotation_target = f.boolean(b"ignoreFORVobRotTarget")?;
                c.adapt = f.boolean(b"adaptToSurroundings")?;
                c.ease_first = f.boolean(b"easeToFirstKey")?;
                c.ease_last = f.boolean(b"easeFromLastKey")?;
                c.total_duration = f.float(b"totalTime")?;
                c.auto_focus_vob = f.string(b"autoCamFocusVobName")?;
                c.auto_player_movable = f.boolean(b"autoCamPlayerMovable")?;
                c.auto_untrigger_last = f.boolean(b"autoCamUntriggerOnLastKey")?;
                c.auto_untrigger_last_delay = f.float(b"autoCamUntriggerOnLastKeyDelay")?;
                let p = f.r.position();
                let positions = usize::try_from(f.int(b"numPos")?).map_err(|_| {
                    Error::new(ErrorKind::InvalidData, p, "negative camera position count")
                })?;
                let p = f.r.position();
                let targets = usize::try_from(f.int(b"numTargets")?).map_err(|_| {
                    Error::new(ErrorKind::InvalidData, p, "negative camera target count")
                })?;
                let count = positions
                    .checked_add(targets)
                    .filter(|n| *n <= links_left)
                    .ok_or_else(|| {
                        Error::new(
                            ErrorKind::LimitExceeded,
                            p,
                            "camera aggregate occurrence count",
                        )
                    })?;
                if count != 0 {
                    *phase = Phase::Camera { positions };
                    return Ok(count);
                }
            }
            if save && game == GameVersion::Gothic2 {
                c.state = Some(CameraState {
                    paused: f.boolean(b"paused")?,
                    started: f.boolean(b"started")?,
                    goto_time_mode: f.boolean(b"gotoTimeMode")?,
                    cs_time: f.float(b"csTime")?,
                });
            }
        }
        VobKindMut::CameraFrame(v) => {
            v.time = f.float(b"time")?;
            v.roll_angle = f.float(b"angleRollDeg")?;
            v.fov_scale = f.float(b"camFOVScale")?;
            v.motion_type = CameraMotion::from_raw(f.enumeration(b"motionType")?);
            v.motion_type_fov = CameraMotion::from_raw(f.enumeration(b"motionTypeFOV")?);
            v.motion_type_roll = CameraMotion::from_raw(f.enumeration(b"motionTypeRoll")?);
            v.motion_type_time_scale =
                CameraMotion::from_raw(f.enumeration(b"motionTypeTimeScale")?);
            v.tension = f.float(b"tension")?;
            v.cam_bias = f.float(b"bias")?;
            v.continuity = f.float(b"continuity")?;
            v.time_scale = f.float(b"timeScale")?;
            v.time_fixed = f.boolean(b"timeIsFixed")?;
            let p = f.r.position();
            let V::Raw(raw) = f.field(b"originalPose", T::Raw(64))? else {
                unreachable!()
            };
            v.original_pose = Mat4::read(&mut io::Reader::new(&raw[..64]), "camera original pose")?;
            v.pose_tail = objects::copy(&raw[64..], f.copied, f.limit, p)?;
        }
        _ => unreachable!(),
    }
    *phase = Phase::Done;
    Ok(0)
}
pub(super) fn write(
    w: &mut archive::Writer,
    kind: VobKindRef<'_>,
    phase: &mut Phase,
    limits: archive::Limits,
) -> Result<usize> {
    match kind {
        VobKindRef::Camera(c) => {
            if matches!(*phase, Phase::Base) {
                w.write(b"camTrjFOR", V::Enum(c.trajectory_for.raw()))?;
                w.write(b"targetTrjFOR", V::Enum(c.target_trajectory_for.raw()))?;
                w.write(b"loopMode", V::Enum(c.loop_mode.raw()))?;
                w.write(b"splLerpMode", V::Enum(c.lerp_mode.raw()))?;
                w.write(b"ignoreFORVobRotCam", V::Bool(c.ignore_for_vob_rotation))?;
                w.write(
                    b"ignoreFORVobRotTarget",
                    V::Bool(c.ignore_for_vob_rotation_target),
                )?;
                w.write(b"adaptToSurroundings", V::Bool(c.adapt))?;
                w.write(b"easeToFirstKey", V::Bool(c.ease_first))?;
                w.write(b"easeFromLastKey", V::Bool(c.ease_last))?;
                w.write(b"totalTime", V::Float(c.total_duration))?;
                w.write(
                    b"autoCamFocusVobName",
                    V::String(c.auto_focus_vob.as_slice().into()),
                )?;
                w.write(b"autoCamPlayerMovable", V::Bool(c.auto_player_movable))?;
                w.write(b"autoCamUntriggerOnLastKey", V::Bool(c.auto_untrigger_last))?;
                w.write(
                    b"autoCamUntriggerOnLastKeyDelay",
                    V::Float(c.auto_untrigger_last_delay),
                )?;
                let positions = i32::try_from(c.positions.len())
                    .map_err(|_| bad("camera position count exceeds i32"))?;
                let targets = i32::try_from(c.targets.len())
                    .map_err(|_| bad("camera target count exceeds i32"))?;
                w.write(b"numPos", V::Int(positions))?;
                w.write(b"numTargets", V::Int(targets))?;
                let count = c
                    .positions
                    .len()
                    .checked_add(c.targets.len())
                    .ok_or_else(|| bad("camera count overflow"))?;
                if count != 0 {
                    *phase = Phase::Camera {
                        positions: c.positions.len(),
                    };
                    return Ok(count);
                }
            }
            if let Some(s) = c.state {
                w.write(b"paused", V::Bool(s.paused))?;
                w.write(b"started", V::Bool(s.started))?;
                w.write(b"gotoTimeMode", V::Bool(s.goto_time_mode))?;
                w.write(b"csTime", V::Float(s.cs_time))?;
            }
        }
        VobKindRef::CameraFrame(v) => {
            w.write(b"time", V::Float(v.time))?;
            w.write(b"angleRollDeg", V::Float(v.roll_angle))?;
            w.write(b"camFOVScale", V::Float(v.fov_scale))?;
            w.write(b"motionType", V::Enum(v.motion_type.raw()))?;
            w.write(b"motionTypeFOV", V::Enum(v.motion_type_fov.raw()))?;
            w.write(b"motionTypeRoll", V::Enum(v.motion_type_roll.raw()))?;
            w.write(
                b"motionTypeTimeScale",
                V::Enum(v.motion_type_time_scale.raw()),
            )?;
            w.write(b"tension", V::Float(v.tension))?;
            w.write(b"bias", V::Float(v.cam_bias))?;
            w.write(b"continuity", V::Float(v.continuity))?;
            w.write(b"timeScale", V::Float(v.time_scale))?;
            w.write(b"timeIsFixed", V::Bool(v.time_fixed))?;
            if w.format() == archive::Format::Binary && !v.pose_tail.is_empty() {
                return Err(Error::operation(
                    ErrorKind::Unsupported,
                    "camera pose tail has no BINARY delimiter",
                ));
            }
            let size = 64usize
                .checked_add(v.pose_tail.len())
                .ok_or_else(|| Error::operation(ErrorKind::LimitExceeded, "camera pose bytes"))?;
            super::trigger_codec::raw_size(w, limits, b"originalPose", size)?;
            let mut raw = io::Writer::new(size);
            v.original_pose.write(&mut raw, "camera original pose")?;
            raw.bytes(&v.pose_tail, "camera pose tail")?;
            w.write(b"originalPose", V::Raw(raw.into_inner().into()))?;
        }
        _ => unreachable!(),
    }
    *phase = Phase::Done;
    Ok(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn mutable_links_visit_base_then_both_lists() {
        let mut v = Vob::new(GameVersion::Gothic2);
        v.base.visual = Some(Link::default());
        v.kind = VobKind::Camera(Camera {
            positions: vec![Link::default()],
            targets: vec![Link::default(); 2],
            ..Default::default()
        });
        for (i, link) in v.as_mut().links_mut().enumerate() {
            link.name = i.to_string().into_bytes();
        }
        assert_eq!(
            v.as_ref()
                .links()
                .map(|l| l.name.as_slice())
                .collect::<Vec<_>>(),
            [b"0".as_slice(), b"1", b"2", b"3"]
        );
        let VobKind::Camera(c) = &v.kind else {
            unreachable!()
        };
        assert_eq!(c.positions[0].name, b"1");
        assert_eq!(c.targets[0].name, b"2");
        assert_eq!(c.targets[1].name, b"3");
    }
    #[test]
    fn both_counts_are_checked_before_yield_without_placeholder_allocation() {
        for (positions, targets) in [(0, 7), (7, 0), (3, 4)] {
            let limits = archive::Limits::default();
            let mut header = archive::Header::new(archive::Format::Binary);
            header.save_game = true;
            let mut writer = archive::Writer::new(header, limits).unwrap();
            let kind = VobKind::Camera(Camera {
                positions: vec![Link::default(); positions],
                targets: vec![Link::default(); targets],
                state: Some(CameraState::default()),
                ..Default::default()
            });
            assert_eq!(
                write(&mut writer, kind.as_ref(), &mut Phase::Base, limits).unwrap(),
                7
            );
            let raw = writer.finish().unwrap();
            for cap in [6, 7] {
                let mut reader = archive::Reader::new(&raw, limits).unwrap();
                let mut copied = 0;
                let mut f = Fields {
                    r: &mut reader,
                    copied: &mut copied,
                    limit: 0,
                };
                let mut kind = VobKind::Camera(Camera::default());
                let mut phase = Phase::Base;
                let result = read(
                    &mut f,
                    &mut kind.as_mut(),
                    &mut phase,
                    GameVersion::Gothic2,
                    cap,
                );
                if cap == 6 {
                    assert_eq!(result.unwrap_err().kind, ErrorKind::LimitExceeded);
                } else {
                    assert_eq!(result.unwrap(), 7);
                    assert!(matches!(phase,Phase::Camera{positions:n} if n==positions));
                }
                let VobKind::Camera(c) = kind else {
                    unreachable!()
                };
                assert_eq!(
                    (
                        c.positions.len(),
                        c.positions.capacity(),
                        c.targets.len(),
                        c.targets.capacity()
                    ),
                    (0, 0, 0, 0)
                );
                assert!(c.state.is_none());
            }
        }
    }
}
